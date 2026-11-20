use crate::android_storage::{guess_mime_type, AndroidStorageHandle};
use crate::hubei_client::{HubeiClient, HubeiError};
use crate::models::{
    AppSettings, DocFailure, DocSaveStatus, DownloadedDoc, FileProgress, ItemStatus,
    LegacyExportInput, LegacyExportResult, ParseBatchSummary, Platform, QueueItem, QueueSnapshot,
    TransferControl,
};
use crate::parser::{normalize_case_number, parse_batch_input};
use crate::renamer::{
    render_stem, resolve_subdir_segments, safe_extension, strip_trailing_ext,
    unique_destination, RenameContext,
};
use crate::zxfw_client::{ZxfwClient, ZxfwDocEntry, ZxfwError};
use chrono::Local;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

pub trait ProgressSink: Send + Sync + Clone + 'static {
    fn on_queue_updated(&self, snapshot: QueueSnapshot);
    fn on_file_progress(&self, progress: FileProgress);
}

#[derive(Clone)]
pub struct TauriProgressSink {
    app: AppHandle,
}

impl TauriProgressSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ProgressSink for TauriProgressSink {
    fn on_queue_updated(&self, snapshot: QueueSnapshot) {
        let _ = self.app.emit_to("main", "queue-updated", snapshot);
    }

    fn on_file_progress(&self, progress: FileProgress) {
        let _ = self.app.emit_to("main", "file-progress", progress);
    }
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct QueueState {
    items: Vec<QueueItem>,
    is_running: bool,
    is_paused: bool,
    active_workers: usize,
    claimed_ids: HashSet<String>,
    signal_tx: Option<watch::Sender<TransferControl>>,
}

struct WorkerSlotGuard<'a, S: ProgressSink> {
    manager: &'a QueueManager,
    claimed_task_id: Option<String>,
    sink: &'a S,
}

impl<S: ProgressSink> Drop for WorkerSlotGuard<'_, S> {
    fn drop(&mut self) {
        self.manager
            .release_worker_slot(self.claimed_task_id.as_deref(), self.sink);
    }
}

#[derive(Clone)]
pub struct QueueManager {
    inner: Arc<Mutex<QueueState>>,
    settings: Arc<Mutex<AppSettings>>,
    config_dir: PathBuf,
    storage: AndroidStorageHandle,
    download_lock: Arc<tokio::sync::Mutex<()>>,
}

impl QueueManager {
    pub fn new(settings: AppSettings, config_dir: PathBuf) -> Self {
        Self::with_storage(settings, config_dir, AndroidStorageHandle::default())
    }

    pub fn with_storage(
        settings: AppSettings,
        config_dir: PathBuf,
        storage: AndroidStorageHandle,
    ) -> Self {
        let items = crate::store::load_queue_history(&config_dir);
        let mgr = Self {
            inner: Arc::new(Mutex::new(QueueState {
                items,
                is_running: false,
                is_paused: false,
                active_workers: 0,
                claimed_ids: HashSet::new(),
                signal_tx: None,
            })),
            settings: Arc::new(Mutex::new(settings)),
            config_dir,
            storage,
            download_lock: Arc::new(tokio::sync::Mutex::new(())),
        };
        mgr.cleanup_stale_staging();
        mgr
    }

    fn persist_history(&self) {
        let state = lock_unpoisoned(&self.inner);
        let _ = crate::store::save_queue_history(&self.config_dir, &state.items);
    }

    fn cleanup_stale_staging(&self) {
        let staging_dir = crate::store::staging_dir_path(&self.config_dir);
        if !staging_dir.exists() {
            return;
        }
        let referenced: HashSet<PathBuf> = {
            let guard = lock_unpoisoned(&self.inner);
            guard
                .items
                .iter()
                .flat_map(|it| it.documents.iter())
                .filter_map(|doc| doc.staging_path.as_ref().map(PathBuf::from))
                .collect()
        };
        if let Ok(entries) = std::fs::read_dir(&staging_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && !referenced.contains(&p) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn storage(&self) -> &AndroidStorageHandle {
        &self.storage
    }

    pub fn get_settings(&self) -> AppSettings {
        lock_unpoisoned(&self.settings).clone()
    }

    pub fn update_settings(&self, new_settings: AppSettings) -> Result<AppSettings, String> {
        let mut guard = lock_unpoisoned(&self.settings);
        crate::store::save_settings(&self.config_dir, &new_settings)?;
        *guard = new_settings.clone();
        Ok(new_settings)
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        let guard = lock_unpoisoned(&self.inner);
        build_snapshot(&guard)
    }

    pub fn add_batch<S: ProgressSink>(&self, raw_input: &str, sink: &S) -> ParseBatchSummary {
        let mut guard = lock_unpoisoned(&self.inner);
        let existing = guard
            .items
            .iter()
            .filter(|item| !item.url.is_empty() || item.status == ItemStatus::Completed)
            .map(|item| item.fingerprint.clone())
            .collect();
        let mut summary = parse_batch_input(raw_input, &existing);
        for parsed in &mut summary.added {
            if let Some(archive) = guard
                .items
                .iter_mut()
                .find(|item| item.fingerprint == parsed.fingerprint)
            {
                parsed.id.clone_from(&archive.id);
                parsed.case_number = archive
                    .case_number
                    .clone()
                    .or_else(|| parsed.case_number.clone());
                parsed.documents.clone_from(&archive.documents);
                parsed.failures.clone_from(&archive.failures);
                parsed.receipt_status = archive.receipt_status.clone();
                parsed.identity_attempts_used = archive.identity_attempts_used;
                *archive = parsed.clone();
            } else {
                guard.items.push(parsed.clone());
            }
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        self.persist_history();
        sink.on_queue_updated(snap);
        summary
    }

    pub fn update_item_case_number<S: ProgressSink>(
        &self,
        task_id: &str,
        case_number: Option<String>,
        sink: &S,
    ) -> Result<(), String> {
        let normalized = case_number
            .map(|s| normalize_case_number(&s))
            .filter(|s| !s.trim().is_empty());
        let mut guard = lock_unpoisoned(&self.inner);
        let item = guard
            .items
            .iter_mut()
            .find(|i| i.id == task_id)
            .ok_or_else(|| "任务不存在".to_string())?;
        if item.status == ItemStatus::Running {
            return Err("下载进行中，暂不可修改案号".to_string());
        }
        item.case_number = normalized;
        let snap = build_snapshot(&guard);
        drop(guard);
        self.persist_history();
        sink.on_queue_updated(snap);
        Ok(())
    }

    pub fn remove_item<S: ProgressSink>(&self, task_id: &str, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        guard
            .items
            .retain(|i| i.id != task_id || i.status == ItemStatus::Running);
        let snap = build_snapshot(&guard);
        drop(guard);
        self.persist_history();
        self.cleanup_stale_staging();
        sink.on_queue_updated(snap);
    }

    pub fn clear_finished<S: ProgressSink>(&self, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        guard.items.retain(|i| {
            !matches!(
                i.status,
                ItemStatus::Completed | ItemStatus::Failed | ItemStatus::BrowserRequired
            )
        });
        let snap = build_snapshot(&guard);
        drop(guard);
        self.persist_history();
        self.cleanup_stale_staging();
        sink.on_queue_updated(snap);
    }

    pub fn clear_local_data<S: ProgressSink>(&self, sink: &S) -> Result<AppSettings, String> {
        let mut state = lock_unpoisoned(&self.inner);
        if state.active_workers > 0
            || state
                .items
                .iter()
                .any(|item| item.status == ItemStatus::Running)
        {
            return Err("请先结束正在进行的下载、核验或归档。".into());
        }
        crate::store::save_queue_history(&self.config_dir, &[])?;
        if crate::store::exported_legacy_path(&self.config_dir).exists() {
            crate::store::save_exported_legacy_paths(&self.config_dir, &HashSet::new())?;
        }
        state.items.clear();
        let snapshot = build_snapshot(&state);
        drop(state);
        let settings = {
            let mut settings = lock_unpoisoned(&self.settings);
            settings.profiles.clear();
            settings.clone()
        };
        self.cleanup_stale_staging();
        sink.on_queue_updated(snapshot);
        Ok(settings)
    }

    pub fn retry_failed<S: ProgressSink>(&self, sink: S) -> bool {
        let retry_ids = {
            let mut guard = lock_unpoisoned(&self.inner);
            if guard.is_running {
                return false;
            }
            let mut ids = HashSet::new();
            for item in &mut guard.items {
                if !item.url.is_empty()
                    && matches!(item.status, ItemStatus::Failed | ItemStatus::Partial)
                    && item.identity_attempts_used < item.identity_attempts_limit
                {
                    item.status = ItemStatus::Pending;
                    item.stage_label = "等待重试".to_string();
                    item.status_message = "等待重试".to_string();
                    item.failures.clear();
                    item.bytes_received = 0;
                    item.total_bytes = None;
                    item.speed_bytes_per_sec = 0;
                    ids.insert(item.id.clone());
                }
            }
            if ids.is_empty() {
                return false;
            }
            ids
        };
        self.start_queue_scoped(Some(retry_ids), sink)
    }

    pub fn pause_queue<S: ProgressSink>(&self, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        if guard.is_running {
            guard.is_paused = true;
            if let Some(tx) = &guard.signal_tx {
                let _ = tx.send(TransferControl::Paused);
            }
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        sink.on_queue_updated(snap);
    }

    pub fn resume_queue<S: ProgressSink>(&self, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        if guard.is_running {
            guard.is_paused = false;
            if let Some(tx) = &guard.signal_tx {
                let _ = tx.send(TransferControl::Running);
            }
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        sink.on_queue_updated(snap);
    }

    pub fn cancel_queue<S: ProgressSink>(&self, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        if let Some(tx) = &guard.signal_tx {
            let _ = tx.send(TransferControl::Cancelled);
        }
        guard.is_paused = false;
        let changed = finalize_paused_items_as_stopped(&mut guard.items);
        let snap = build_snapshot(&guard);
        drop(guard);
        if changed {
            self.persist_history();
        }
        sink.on_queue_updated(snap);
    }

    fn acquire_worker_slot_locked(
        state: &mut QueueState,
        claimed_task_id: Option<&str>,
    ) -> watch::Receiver<TransferControl> {
        if let Some(id) = claimed_task_id {
            state.claimed_ids.insert(id.to_string());
        }
        if state.active_workers == 0 {
            let (tx, rx) = watch::channel(TransferControl::Running);
            state.is_running = true;
            state.is_paused = false;
            state.active_workers = 1;
            state.signal_tx = Some(tx);
            rx
        } else if let Some(tx) = state.signal_tx.as_ref() {
            state.active_workers += 1;
            if *tx.borrow() == TransferControl::Cancelled {
                let initial = if state.is_paused {
                    TransferControl::Paused
                } else {
                    TransferControl::Running
                };
                let _ = tx.send(initial);
            }
            tx.subscribe()
        } else {
            let initial = if state.is_paused {
                TransferControl::Paused
            } else {
                TransferControl::Running
            };
            let (tx, rx) = watch::channel(initial);
            state.active_workers += 1;
            state.signal_tx = Some(tx);
            rx
        }
    }

    fn release_worker_slot<S: ProgressSink>(&self, claimed_task_id: Option<&str>, sink: &S) {
        let mut guard = lock_unpoisoned(&self.inner);
        if let Some(id) = claimed_task_id {
            guard.claimed_ids.remove(id);
        }
        if guard.active_workers > 0 {
            guard.active_workers -= 1;
        }
        let mut changed = false;
        if guard.active_workers == 0 {
            let was_cancelled = guard
                .signal_tx
                .as_ref()
                .is_some_and(|tx| *tx.borrow() == TransferControl::Cancelled);
            guard.is_running = false;
            guard.is_paused = false;
            guard.signal_tx = None;
            if was_cancelled {
                changed = finalize_paused_items_as_stopped(&mut guard.items);
            }
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        if changed {
            self.persist_history();
        }
        sink.on_queue_updated(snap);
    }

    pub fn start_queue<S: ProgressSink>(&self, sink: S) -> bool {
        self.start_queue_scoped(None, sink)
    }

    fn start_queue_scoped<S: ProgressSink>(
        &self,
        target_ids: Option<HashSet<String>>,
        sink: S,
    ) -> bool {
        let rx = {
            let mut guard = lock_unpoisoned(&self.inner);
            if guard.is_running {
                if guard.is_paused {
                    guard.is_paused = false;
                    if let Some(tx) = &guard.signal_tx {
                        let _ = tx.send(TransferControl::Running);
                    }
                    let snap = build_snapshot(&guard);
                    drop(guard);
                    sink.on_queue_updated(snap);
                }
                return false;
            }
            let has_eligible = guard.items.iter().any(|i| {
                i.status == ItemStatus::Pending
                    && !guard.claimed_ids.contains(&i.id)
                    && target_ids
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&i.id))
            });
            if !has_eligible {
                return false;
            }
            let rx = Self::acquire_worker_slot_locked(&mut guard, None);
            let snap = build_snapshot(&guard);
            drop(guard);
            sink.on_queue_updated(snap);
            rx
        };

        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            manager.run_loop(rx, target_ids, sink).await;
        });
        true
    }

    async fn run_loop<S: ProgressSink>(
        &self,
        mut rx: watch::Receiver<TransferControl>,
        target_ids: Option<HashSet<String>>,
        sink: S,
    ) {
        let _slot_guard = WorkerSlotGuard {
            manager: self,
            claimed_task_id: None,
            sink: &sink,
        };
        loop {
            let signal = *rx.borrow_and_update();
            match signal {
                TransferControl::Cancelled => break,
                TransferControl::Paused => {
                    if rx.changed().await.is_err() {
                        break;
                    }
                    continue;
                }
                TransferControl::Running => {}
            }

            let permit = self.download_lock.lock().await;
            if *rx.borrow() != TransferControl::Running {
                continue;
            }
            let next_item = {
                let mut guard = lock_unpoisoned(&self.inner);
                let pos = guard.items.iter().position(|i| {
                    i.status == ItemStatus::Pending
                        && !guard.claimed_ids.contains(&i.id)
                        && target_ids
                            .as_ref()
                            .is_none_or(|ids| ids.contains(&i.id))
                });
                if let Some(idx) = pos {
                    guard.items[idx].status = ItemStatus::Running;
                    guard.items[idx].stage_label = "连接接口".to_string();
                    guard.items[idx].status_message = "正在连接法院接口并获取文书清单...".to_string();
                    guard.items[idx].current_doc_index = 0;
                    guard.items[idx].current_doc_total = 0;
                    guard.items[idx].current_doc_name = None;
                    guard.items[idx].current_target_file_name = None;
                    guard.items[idx].bytes_received = 0;
                    guard.items[idx].total_bytes = None;
                    guard.items[idx].speed_bytes_per_sec = 0;
                    let item = guard.items[idx].clone();
                    let snap = build_snapshot(&guard);
                    drop(guard);
                    sink.on_queue_updated(snap);
                    Some(item)
                } else {
                    None
                }
            };

            let Some(item) = next_item else {
                break;
            };

            let settings = self.get_settings();
            self.execute_single_item(item, &settings, rx.clone(), &sink)
                .await;
            drop(permit);

            match *rx.borrow() {
                TransferControl::Cancelled => break,
                TransferControl::Paused => continue,
                TransferControl::Running => {}
            }
            if settings.request_delay_ms > 0 {
                tokio::select! {
                    _ = rx.changed() => {}
                    _ = tokio::time::sleep(Duration::from_millis(settings.request_delay_ms)) => {}
                }
            }
        }
    }

    async fn execute_single_item<S: ProgressSink>(
        &self,
        item: QueueItem,
        settings: &AppSettings,
        rx: watch::Receiver<TransferControl>,
        sink: &S,
    ) {
        match item.platform {
            Platform::Zxfw => self.process_zxfw_item(item, settings, rx, sink).await,
            Platform::HubeiPublic => self.process_hubei_item(item, settings, rx, sink).await,
            _ => {
                self.update_item(&item.id, sink, |it| {
                    it.status = ItemStatus::BrowserRequired;
                    it.status_message = "该平台需浏览器打开完成下载".to_string();
                });
            }
        }
    }

    async fn process_zxfw_item<S: ProgressSink>(
        &self,
        item: QueueItem,
        settings: &AppSettings,
        mut rx: watch::Receiver<TransferControl>,
        sink: &S,
    ) {
        let Ok(client) = ZxfwClient::new() else {
            self.mark_failed(&item.id, "初始化 HTTP 客户端失败", sink);
            return;
        };

        let docs = tokio::select! {
            biased;
            ctrl = wait_non_running(&mut rx) => {
                self.handle_interrupted_item(&item.id, ctrl, &item.documents, &item.failures, sink);
                return;
            }
            res = client.fetch_ws_list(&item.params) => match res {
                Ok(list) => list,
                Err(ZxfwError::IdentityRequired) => {
                    self.update_item(&item.id, sink, |it| {
                        it.status = ItemStatus::NeedsIdentity;
                        it.status_message = "法院后端要求输入手机或证件后四位核验".to_string();
                    });
                    return;
                }
                Err(err) => {
                    self.mark_failed(&item.id, &err.to_string(), sink);
                    return;
                }
            }
        };

        self.download_doc_entries(&item, &client, docs, settings, Some(rx), sink)
            .await;
    }

    async fn process_hubei_item<S: ProgressSink>(
        &self,
        item: QueueItem,
        settings: &AppSettings,
        mut rx: watch::Receiver<TransferControl>,
        sink: &S,
    ) {
        let Some(msg) = item.params.msg.as_deref() else {
            self.mark_failed(&item.id, "缺少湖北送达 msg 参数", sink);
            return;
        };
        let Ok(client) = HubeiClient::new() else {
            self.mark_failed(&item.id, "初始化湖北下载客户端失败", sink);
            return;
        };

        let fetch_res = tokio::select! {
            biased;
            ctrl = wait_non_running(&mut rx) => {
                self.handle_interrupted_item(&item.id, ctrl, &item.documents, &item.failures, sink);
                return;
            }
            res = client.fetch_doc_list(msg) => res
        };

        match fetch_res {
            Ok(docs) => {
                self.download_doc_entries(&item, client.downloader(), docs, settings, Some(rx), sink)
                    .await;
            }
            Err(HubeiError::CaptchaRequired) => {
                self.update_item(&item.id, sink, |it| {
                    it.status = ItemStatus::BrowserRequired;
                    it.stage_label = "需浏览器".to_string();
                    it.status_message = "湖北平台要求验证码，请点击用浏览器打开".to_string();
                });
            }
            Err(HubeiError::InsecureTransport) => {
                self.update_item(&item.id, sink, |it| {
                    it.status = ItemStatus::BrowserRequired;
                    it.stage_label = "需浏览器".to_string();
                    it.status_message =
                        "湖北平台无法建立 HTTPS 加密连接，已停止自动下载以保护送达凭据，请用浏览器打开"
                            .to_string();
                });
            }
            Err(err) => {
                self.mark_failed(&item.id, &err.to_string(), sink);
            }
        }
    }

    pub async fn verify_and_download<S: ProgressSink>(
        &self,
        task_id: String,
        method: String,
        tail: String,
        sink: S,
    ) -> Result<(), String> {
        self.verify_and_download_with_base(task_id, method, tail, None, sink)
            .await
    }

    pub async fn verify_and_download_with_base<S: ProgressSink>(
        &self,
        task_id: String,
        method: String,
        tail: String,
        base_url_override: Option<&str>,
        sink: S,
    ) -> Result<(), String> {
        let client = match base_url_override {
            Some(base) => ZxfwClient::with_base_url(base),
            None => ZxfwClient::new(),
        }
        .map_err(|e| e.to_string())?;
        let item = {
            let mut guard = lock_unpoisoned(&self.inner);
            let pos = guard
                .items
                .iter()
                .position(|i| i.id == task_id)
                .ok_or_else(|| "任务不存在".to_string())?;
            let it = &mut guard.items[pos];
            if it.status != ItemStatus::NeedsIdentity {
                return Err("任务当前无需核验".into());
            }
            if it.identity_attempts_used >= it.identity_attempts_limit {
                return Err("该链接已达核验次数上限（2次），请核对法院登记号码".to_string());
            }
            it.status = ItemStatus::Running;
            it.stage_label = "核验身份".to_string();
            it.status_message = "正在提交后四位核验...".to_string();
            let cloned = it.clone();
            let snap = build_snapshot(&guard);
            drop(guard);
            sink.on_queue_updated(snap);
            cloned
        };

        match client.identity_gate(&item.params).await {
            Some(true) => {}
            Some(false) => {
                let message = "列表要求核验但平台核验开关显示关闭，状态冲突，请用浏览器打开原链接";
                self.update_item(&item.id, &sink, |it| {
                    it.status = ItemStatus::BrowserRequired;
                    it.stage_label = "需浏览器".to_string();
                    it.status_message = message.into();
                });
                return Err(message.into());
            }
            None => {
                let message = "无法查询身份核验开关，暂未提交尾号，请稍后重试或用浏览器打开";
                self.update_item(&item.id, &sink, |it| {
                    it.status = ItemStatus::NeedsIdentity;
                    it.stage_label = "需核验".to_string();
                    it.status_message = message.into();
                });
                return Err(message.into());
            }
        }

        match client.verify_identity(&item.params, &method, &tail).await {
            Ok(()) => {
                let rx = {
                    let mut guard = lock_unpoisoned(&self.inner);
                    let rx = Self::acquire_worker_slot_locked(&mut guard, Some(&item.id));
                    if let Some(it) = guard.items.iter_mut().find(|i| i.id == item.id) {
                        it.stage_label = "准备下载".to_string();
                        it.status_message = if *rx.borrow() == TransferControl::Paused {
                            "核验通过，队列已暂停，点击「继续」恢复下载".to_string()
                        } else {
                            "核验通过，正在获取文书列表...".to_string()
                        };
                    }
                    let snap = build_snapshot(&guard);
                    drop(guard);
                    sink.on_queue_updated(snap);
                    rx
                };

                let manager = self.clone();
                let item_id = item.id.clone();
                tauri::async_runtime::spawn(async move {
                    manager
                        .run_verified_item_download(item_id, client, rx, sink)
                        .await;
                });
                Ok(())
            }
            Err(ZxfwError::IdentityRejected) => {
                let mut exhausted = false;
                self.update_item(&item.id, &sink, |it| {
                    it.identity_attempts_used += 1;
                    if it.identity_attempts_used >= it.identity_attempts_limit {
                        exhausted = true;
                        it.status = ItemStatus::Failed;
                        it.stage_label = "失败".to_string();
                        it.status_message = "后四位连续2次不匹配，请核对法院登记号码".to_string();
                    } else {
                        it.status = ItemStatus::NeedsIdentity;
                        it.stage_label = "需核验".to_string();
                        it.status_message = "后四位不匹配，请确认后重试（还剩1次）".to_string();
                    }
                });
                Err(if exhausted {
                    "后四位连续2次不匹配，已停止尝试".to_string()
                } else {
                    "后四位不匹配，请核对实际接收号码".to_string()
                })
            }
            Err(err) => {
                self.update_item(&item.id, &sink, |it| {
                    it.status = ItemStatus::NeedsIdentity;
                    it.stage_label = "需核验".to_string();
                    it.status_message = format!("核验未完成：{}", err);
                });
                Err(err.to_string())
            }
        }
    }

    async fn run_verified_item_download<S: ProgressSink>(
        &self,
        task_id: String,
        client: ZxfwClient,
        mut rx: watch::Receiver<TransferControl>,
        sink: S,
    ) {
        let _slot_guard = WorkerSlotGuard {
            manager: self,
            claimed_task_id: Some(task_id.clone()),
            sink: &sink,
        };

        loop {
            let signal = *rx.borrow_and_update();
            match signal {
                TransferControl::Cancelled => {
                    let latest = {
                        let guard = lock_unpoisoned(&self.inner);
                        guard.items.iter().find(|i| i.id == task_id).cloned()
                    };
                    if let Some(it) = latest {
                        self.handle_interrupted_item(
                            &task_id,
                            TransferControl::Cancelled,
                            &it.documents,
                            &it.failures,
                            &sink,
                        );
                    }
                    return;
                }
                TransferControl::Paused => {
                    if rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                }
                TransferControl::Running => {}
            }

            let permit = self.download_lock.lock().await;
            match *rx.borrow() {
                TransferControl::Cancelled => {
                    drop(permit);
                    continue;
                }
                TransferControl::Paused => {
                    drop(permit);
                    continue;
                }
                TransferControl::Running => {}
            }

            let latest_item = {
                let mut guard = lock_unpoisoned(&self.inner);
                let Some(it) = guard.items.iter_mut().find(|i| i.id == task_id) else {
                    return;
                };
                if !matches!(it.status, ItemStatus::Running | ItemStatus::Pending) {
                    return;
                }
                it.status = ItemStatus::Running;
                it.stage_label = "连接接口".to_string();
                it.status_message = "核验通过，正在获取文书列表...".to_string();
                let cloned = it.clone();
                let snap = build_snapshot(&guard);
                drop(guard);
                sink.on_queue_updated(snap);
                cloned
            };

            let settings = self.get_settings();
            let docs = tokio::select! {
                biased;
                ctrl = wait_non_running(&mut rx) => {
                    self.handle_interrupted_item(
                        &task_id,
                        ctrl,
                        &latest_item.documents,
                        &latest_item.failures,
                        &sink,
                    );
                    drop(permit);
                    if ctrl == TransferControl::Paused {
                        continue;
                    }
                    return;
                }
                res = client.fetch_ws_list(&latest_item.params) => match res {
                    Ok(docs) => docs,
                    Err(err) => {
                        self.mark_failed(&task_id, &err.to_string(), &sink);
                        return;
                    }
                }
            };

            self.download_doc_entries(&latest_item, &client, docs, &settings, Some(rx.clone()), &sink)
                .await;
            drop(permit);

            match *rx.borrow() {
                TransferControl::Paused => continue,
                _ => return,
            }
        }
    }

    async fn download_doc_entries<S: ProgressSink>(
        &self,
        item: &QueueItem,
        client: &ZxfwClient,
        docs: Vec<ZxfwDocEntry>,
        settings: &AppSettings,
        control_rx: Option<watch::Receiver<TransferControl>>,
        sink: &S,
    ) {
        if docs.is_empty() {
            self.update_item(&item.id, sink, |it| {
                it.status = ItemStatus::Failed;
                it.status_message = "接口返回文书列表为空".to_string();
            });
            return;
        }

        let now = Local::now();
        let today = now.format("%Y%m%d").to_string();
        let today_dash = now.format("%Y-%m-%d").to_string();
        let now_time = now.format("%H%M%S").to_string();
        let api_court = docs
            .iter()
            .find_map(|d| d.c_fymc.as_deref())
            .filter(|s| !s.trim().is_empty())
            .or(item.court_name.as_deref())
            .map(str::to_owned);

        let subdir_mode = settings.effective_subdir_mode();
        let fallback_staging_dir = crate::store::staging_dir_path(&self.config_dir);

        let total_docs = docs.len();
        let mut used_names = HashSet::new();
        let mut processed_keys = HashSet::new();
        let mut saved_docs = Vec::new();
        let mut failures = Vec::new();

        self.update_item_transient(&item.id, sink, |it| {
            it.stage_label = "准备下载".to_string();
            it.current_doc_total = total_docs;
            if it.court_name.is_none() {
                it.court_name = api_court.clone();
            }
        });

        for (idx, doc) in docs.into_iter().enumerate() {
            let doc_num = idx + 1;
            let raw_title = doc
                .c_wsmc
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("文书_{}", doc_num));

            let Some(dl_url) = doc.wjlj.as_deref().filter(|s| !s.trim().is_empty()) else {
                let missing_sk = doc
                    .c_wsbh
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("id:{id}"));
                if let Some(ref sk) = missing_sk {
                    processed_keys.insert(sk.clone());
                }
                failures.push(DocFailure {
                    index: doc_num,
                    name: Some(raw_title),
                    reason: "缺少下载地址".to_string(),
                    source_key: missing_sk,
                });
                continue;
            };

            let source_key = doc
                .c_wsbh
                .clone()
                .filter(|id| !id.is_empty())
                .map(|id| format!("id:{id}"))
                .unwrap_or_else(|| crate::store::download_url_key(dl_url));
            processed_keys.insert(source_key.clone());

            let previous_opt = item
                .documents
                .iter()
                .find(|saved| saved.source_key.as_deref() == Some(source_key.as_str()));
            if let Some(previous) = previous_opt {
                if previous.save_status == DocSaveStatus::Saved && !previous.saved_path.is_empty() {
                    if let Ok(acc) = self.storage.check_document_access(&previous.saved_path).await {
                        if acc.exists {
                            used_names.insert(previous.file_name.to_ascii_lowercase());
                            saved_docs.push(previous.clone());
                            continue;
                        }
                    }
                }
            }

            let doc_court = doc
                .c_fymc
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or(api_court.as_deref());
            let ext = safe_extension(doc.c_wjgs.as_deref(), dl_url);
            let stem = render_stem(
                &settings.rename_template,
                &settings.custom_template,
                settings.prefix_index,
                &RenameContext {
                    raw_title: &raw_title,
                    case_number: item.case_number.as_deref(),
                    court_name: doc_court,
                    parties: &item.parties,
                    date_str: Some(&today),
                    date_dash_str: Some(&today_dash),
                    time_str: Some(&now_time),
                    index: doc_num,
                    total_docs,
                    extension: &ext,
                },
            );
            let clean_stem = strip_trailing_ext(&stem, ext.trim_start_matches('.'));
            let target_file_name = format!("{}{}", clean_stem, ext);
            let subdir_segments = resolve_subdir_segments(
                &subdir_mode,
                item.case_number.as_deref(),
                doc_court,
                &today,
            );
            let doc_staging_dir = if self.storage.is_available() {
                fallback_staging_dir.clone()
            } else {
                let mut target_dir = PathBuf::from(&settings.output_dir);
                for seg in &subdir_segments {
                    target_dir = target_dir.join(seg);
                }
                if std::fs::create_dir_all(&target_dir).is_ok() {
                    target_dir
                } else {
                    fallback_staging_dir.clone()
                }
            };

            // Reuse existing validated staging file if only the publish step previously failed.
            let staged = if let Some(prev) = previous_opt {
                if let Some(ref existing_staging) = prev.staging_path {
                    let sp = PathBuf::from(existing_staging);
                    if sp.is_file() {
                        Some((sp, prev.size_bytes))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let staging_res = if let Some((sp, sz)) = staged {
                Ok((sp, sz))
            } else {
                self.update_item_transient(&item.id, sink, |it| {
                    it.stage_label = "正在下载".to_string();
                    it.current_doc_index = doc_num;
                    it.current_doc_name = Some(raw_title.clone());
                    it.current_target_file_name = Some(target_file_name.clone());
                    it.status_message =
                        format!("正在下载第 {}/{} 份：{}", doc_num, total_docs, raw_title);
                    it.bytes_received = 0;
                    it.total_bytes = None;
                    it.speed_bytes_per_sec = 0;
                });

                let tracker = Arc::new(Mutex::new(ProgressTracker {
                    started_at: Instant::now(),
                    last_emit: Instant::now() - Duration::from_secs(1),
                    last_received: 0,
                    last_total: None,
                }));
                let tracker_cb = tracker.clone();
                let task_id = item.id.clone();
                let doc_title_for_cb = raw_title.clone();
                let target_name_for_cb = target_file_name.clone();
                let sink_cb = sink.clone();
                let inner_cb = self.inner.clone();

                let tracker_stage = tracker;
                let task_id_stage = item.id.clone();
                let doc_title_stage = raw_title.clone();
                let target_name_stage = target_file_name.clone();
                let sink_stage = sink.clone();
                let inner_stage = self.inner.clone();

                client
                    .download_to_staging_with_control(
                        dl_url,
                        &doc_staging_dir,
                        &ext,
                        move |received, total| {
                            let speed = {
                                let mut tr = lock_unpoisoned(&tracker_cb);
                                tr.last_received = received;
                                tr.last_total = total;
                                let finished = total.map(|t| received >= t).unwrap_or(false);
                                if !finished && tr.last_emit.elapsed() < Duration::from_millis(80) {
                                    return;
                                }
                                tr.last_emit = Instant::now();
                                let secs = tr.started_at.elapsed().as_secs_f64().max(0.05);
                                (received as f64 / secs) as u64
                            };
                            let mut guard = lock_unpoisoned(&inner_cb);
                            if let Some(it) = guard.items.iter_mut().find(|i| i.id == task_id) {
                                it.bytes_received = received;
                                it.total_bytes = total;
                                it.speed_bytes_per_sec = speed;
                            }
                            drop(guard);
                            sink_cb.on_file_progress(FileProgress {
                                task_id: task_id.clone(),
                                doc_index: doc_num,
                                doc_total: total_docs,
                                doc_name: doc_title_for_cb.clone(),
                                target_file_name: Some(target_name_for_cb.clone()),
                                stage: "正在下载".to_string(),
                                bytes_received: received,
                                total_bytes: total,
                                speed_bytes_per_sec: speed,
                            });
                        },
                        move |new_stage| {
                            let (bytes_received, total_bytes) = {
                                let mut tr = lock_unpoisoned(&tracker_stage);
                                if new_stage == "正在下载" {
                                    tr.started_at = Instant::now();
                                    tr.last_emit = Instant::now() - Duration::from_secs(1);
                                    tr.last_received = 0;
                                    tr.last_total = None;
                                }
                                (tr.last_received, tr.last_total)
                            };
                            let msg = if new_stage == "正在校验" {
                                format!("正在校验第 {}/{} 份：{}", doc_num, total_docs, doc_title_stage)
                            } else {
                                format!("正在下载第 {}/{} 份：{}", doc_num, total_docs, doc_title_stage)
                            };
                            let mut guard = lock_unpoisoned(&inner_stage);
                            if let Some(it) =
                                guard.items.iter_mut().find(|i| i.id == task_id_stage)
                            {
                                it.stage_label = new_stage.to_string();
                                it.status_message = msg;
                                it.bytes_received = bytes_received;
                                it.total_bytes = total_bytes;
                                it.speed_bytes_per_sec = 0;
                            }
                            drop(guard);
                            sink_stage.on_file_progress(FileProgress {
                                task_id: task_id_stage.clone(),
                                doc_index: doc_num,
                                doc_total: total_docs,
                                doc_name: doc_title_stage.clone(),
                                target_file_name: Some(target_name_stage.clone()),
                                stage: new_stage.to_string(),
                                bytes_received,
                                total_bytes,
                                speed_bytes_per_sec: 0,
                            });
                        },
                        control_rx.clone(),
                    )
                    .await
            };

            match staging_res {
                Ok((staged_path, size_bytes)) => {
                    let save_stage = if self.storage.is_available() {
                        "正在保存到下载目录"
                    } else {
                        "正在保存"
                    };
                    self.update_item_transient(&item.id, sink, |it| {
                        it.stage_label = save_stage.to_string();
                        it.status_message =
                            format!("{}第 {}/{} 份：{}", save_stage, doc_num, total_docs, raw_title);
                    });

                    match self
                        .publish_staged_file(
                            PublishRequest {
                                source_file: &staged_path,
                                subdir_segments: &subdir_segments,
                                stem: &clean_stem,
                                ext: &ext,
                                expected_bytes: size_bytes,
                                settings,
                                remove_source_on_success: true,
                            },
                            &mut used_names,
                        )
                        .await
                    {
                        Ok(published) => {
                            saved_docs.push(DownloadedDoc {
                                raw_title,
                                file_name: published.file_name,
                                saved_path: published.saved_path,
                                storage_uri: published.storage_uri,
                                display_location: published.display_location,
                                staging_path: None,
                                save_status: DocSaveStatus::Saved,
                                save_error: None,
                                size_bytes: published.size_bytes,
                                court_name: doc.c_fymc,
                                document_id: doc.c_wsbh,
                                source_key: Some(source_key),
                                format: doc.c_wjgs,
                            });
                        }
                        Err(save_err) => {
                            saved_docs.push(DownloadedDoc {
                                raw_title: raw_title.clone(),
                                file_name: target_file_name,
                                saved_path: String::new(),
                                storage_uri: None,
                                display_location: "待保存到目录（已保留临时文件）".to_string(),
                                staging_path: Some(staged_path.to_string_lossy().to_string()),
                                save_status: DocSaveStatus::SaveFailed,
                                save_error: Some(save_err.clone()),
                                size_bytes,
                                court_name: doc.c_fymc,
                                document_id: doc.c_wsbh,
                                source_key: Some(source_key.clone()),
                                format: doc.c_wjgs,
                            });
                            failures.push(DocFailure {
                                index: doc_num,
                                name: Some(raw_title),
                                reason: format!("已下载并校验，保存失败：{}", save_err),
                                source_key: Some(source_key),
                            });
                        }
                    }

                    let merged_docs =
                        merge_docs_preserving_unvisited(&saved_docs, &item.documents, &processed_keys);
                    self.update_item(&item.id, sink, |it| {
                        it.documents = merged_docs;
                        it.failures = failures.clone();
                    });
                }
                Err(ZxfwError::Paused) => {
                    let merged_docs =
                        merge_docs_preserving_unvisited(&saved_docs, &item.documents, &processed_keys);
                    self.handle_interrupted_item(
                        &item.id,
                        TransferControl::Paused,
                        &merged_docs,
                        &failures,
                        sink,
                    );
                    return;
                }
                Err(ZxfwError::Cancelled) => {
                    let merged_docs =
                        merge_docs_preserving_unvisited(&saved_docs, &item.documents, &processed_keys);
                    self.handle_interrupted_item(
                        &item.id,
                        TransferControl::Cancelled,
                        &merged_docs,
                        &failures,
                        sink,
                    );
                    return;
                }
                Err(err) => {
                    failures.push(DocFailure {
                        index: doc_num,
                        name: Some(raw_title),
                        reason: err.to_string(),
                        source_key: Some(source_key),
                    });
                }
            }
        }

        let final_docs = merge_docs_preserving_unvisited(&saved_docs, &item.documents, &processed_keys);
        self.update_item(&item.id, sink, |it| {
            it.documents = final_docs;
            it.failures = failures;
            it.current_target_file_name = None;
            it.speed_bytes_per_sec = 0;
            recompute_item_completion_state(it);
        });
    }

    async fn publish_staged_file(
        &self,
        req: PublishRequest<'_>,
        used_names: &mut HashSet<String>,
    ) -> Result<PublishedOutcome, String> {
        let ext_dot = if req.ext.starts_with('.') {
            req.ext.to_string()
        } else {
            format!(".{}", req.ext)
        };
        let clean_stem = strip_trailing_ext(req.stem, ext_dot.trim_start_matches('.'));
        let file_name = format!("{}{}", clean_stem, ext_dot);

        if self.storage.is_available() {
            let res = self
                .storage
                .publish_file(
                    &req.source_file.to_string_lossy(),
                    req.subdir_segments.to_vec(),
                    &file_name,
                    Some(guess_mime_type(&file_name)),
                    &req.settings.android_storage_mode,
                    req.settings.android_tree_uri.as_deref(),
                )
                .await?;
            if req.remove_source_on_success {
                let _ = tokio::fs::remove_file(req.source_file).await;
            }
            return Ok(PublishedOutcome {
                file_name: res.final_file_name,
                saved_path: res.content_uri.clone(),
                storage_uri: Some(res.content_uri),
                display_location: res.display_path,
                size_bytes: res.size_bytes,
            });
        }

        let mut target_dir = PathBuf::from(&req.settings.output_dir);
        for seg in req.subdir_segments {
            target_dir = target_dir.join(seg);
        }
        tokio::fs::create_dir_all(&target_dir)
            .await
            .map_err(|e| format!("无法创建目标目录：{}", e))?;
        let destination = commit_file_noclobber(
            req.source_file,
            &target_dir,
            &clean_stem,
            &ext_dot,
            req.expected_bytes,
            used_names,
            req.remove_source_on_success,
        )?;

        let dest_str = destination.to_string_lossy().to_string();
        let final_name = destination
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_name)
            .to_string();
        Ok(PublishedOutcome {
            file_name: final_name,
            saved_path: dest_str.clone(),
            storage_uri: None,
            display_location: dest_str,
            size_bytes: req.expected_bytes,
        })
    }

    pub async fn retry_save_document<S: ProgressSink>(
        &self,
        task_id: String,
        doc_index: usize,
        sink: S,
    ) -> Result<(), String> {
        let (doc_snapshot, case_number, item_court, parties, doc_count) = {
            let guard = lock_unpoisoned(&self.inner);
            let item = guard
                .items
                .iter()
                .find(|i| i.id == task_id)
                .ok_or_else(|| "任务不存在".to_string())?;
            if item.status == ItemStatus::Running {
                return Err("任务正在下载中，请稍后再试".to_string());
            }
            let doc = item
                .documents
                .get(doc_index)
                .cloned()
                .ok_or_else(|| "文书不存在".to_string())?;
            (
                doc,
                item.case_number.clone(),
                item.court_name.clone(),
                item.parties.clone(),
                item.documents.len(),
            )
        };

        let staging_str = doc_snapshot
            .staging_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "没有可重试保存的临时文件，请重试下载任务".to_string())?;
        let staging_path = PathBuf::from(staging_str);
        if !staging_path.is_file() {
            return Err("临时文件已失效，请重新下载该任务".to_string());
        }

        let settings = self.get_settings();
        let now = Local::now();
        let today = now.format("%Y%m%d").to_string();
        let today_dash = now.format("%Y-%m-%d").to_string();
        let now_time = now.format("%H%M%S").to_string();
        let doc_court = doc_snapshot
            .court_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(item_court.as_deref());
        let ext = safe_extension(doc_snapshot.format.as_deref(), &doc_snapshot.file_name);
        let stem = render_stem(
            &settings.rename_template,
            &settings.custom_template,
            settings.prefix_index,
            &RenameContext {
                raw_title: &doc_snapshot.raw_title,
                case_number: case_number.as_deref(),
                court_name: doc_court,
                parties: &parties,
                date_str: Some(&today),
                date_dash_str: Some(&today_dash),
                time_str: Some(&now_time),
                index: doc_index + 1,
                total_docs: doc_count,
                extension: &ext,
            },
        );
        let subdir_segments = resolve_subdir_segments(
            &settings.effective_subdir_mode(),
            case_number.as_deref(),
            doc_court,
            &today,
        );
        let mut used_names = HashSet::new();
        let outcome = self
            .publish_staged_file(
                PublishRequest {
                    source_file: &staging_path,
                    subdir_segments: &subdir_segments,
                    stem: &stem,
                    ext: &ext,
                    expected_bytes: doc_snapshot.size_bytes,
                    settings: &settings,
                    remove_source_on_success: true,
                },
                &mut used_names,
            )
            .await?;

        self.update_item(&task_id, &sink, |it| {
            if let Some(doc) = it.documents.get_mut(doc_index) {
                doc.file_name = outcome.file_name;
                doc.saved_path = outcome.saved_path;
                doc.storage_uri = outcome.storage_uri;
                doc.display_location = outcome.display_location;
                doc.staging_path = None;
                doc.save_status = DocSaveStatus::Saved;
                doc.save_error = None;
                doc.size_bytes = outcome.size_bytes;
            }
            if doc_snapshot.source_key.is_some() {
                it.failures
                    .retain(|f| f.source_key.as_ref() != doc_snapshot.source_key.as_ref());
            } else {
                it.failures
                    .retain(|f| f.name.as_deref() != Some(doc_snapshot.raw_title.as_str()));
            }
            recompute_item_completion_state(it);
        });
        Ok(())
    }

    pub async fn refresh_document_statuses<S: ProgressSink>(&self, sink: S) {
        let checks: Vec<(String, usize, String, DocSaveStatus)> = {
            let guard = lock_unpoisoned(&self.inner);
            guard
                .items
                .iter()
                .flat_map(|it| {
                    it.documents.iter().enumerate().filter_map(move |(idx, doc)| {
                        if doc.save_status == DocSaveStatus::SaveFailed || doc.saved_path.is_empty() {
                            None
                        } else {
                            Some((
                                it.id.clone(),
                                idx,
                                doc.saved_path.clone(),
                                doc.save_status.clone(),
                            ))
                        }
                    })
                })
                .collect()
        };

        if checks.is_empty() {
            return;
        }

        let mut updates = Vec::new();
        for (task_id, idx, saved_path, prev_status) in checks {
            if let Ok(acc) = self.storage.check_document_access(&saved_path).await {
                let next_status = if acc.exists {
                    DocSaveStatus::Saved
                } else {
                    DocSaveStatus::Missing
                };
                if next_status != prev_status {
                    updates.push((task_id, idx, saved_path, prev_status, next_status));
                }
            }
        }

        if updates.is_empty() {
            return;
        }

        let mut guard = lock_unpoisoned(&self.inner);
        let mut changed = false;
        for (task_id, idx, checked_path, prev_status, next_status) in updates {
            if let Some(it) = guard.items.iter_mut().find(|i| i.id == task_id) {
                if let Some(doc) = it.documents.get_mut(idx) {
                    if doc.saved_path == checked_path && doc.save_status == prev_status {
                        doc.save_status = next_status;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return;
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        self.persist_history();
        sink.on_queue_updated(snap);
    }

    pub async fn remove_downloaded_file<S: ProgressSink>(
        &self,
        task_id: String,
        doc_index: usize,
        sink: S,
    ) -> Result<(), String> {
        let (saved_path, staging_path) = {
            let guard = lock_unpoisoned(&self.inner);
            let item = guard
                .items
                .iter()
                .find(|i| i.id == task_id)
                .ok_or_else(|| "任务不存在".to_string())?;
            let doc = item
                .documents
                .get(doc_index)
                .ok_or_else(|| "文书不存在".to_string())?;
            (doc.saved_path.clone(), doc.staging_path.clone())
        };

        if !saved_path.is_empty() {
            let deleted = self.storage.delete_document(&saved_path).await?;
            if !deleted {
                return Err(
                    "未能删除本机文件，请检查存储权限或在系统文件管理中手动删除".to_string(),
                );
            }
        }
        if let Some(sp) = staging_path {
            let _ = tokio::fs::remove_file(sp).await;
        }

        self.update_item(&task_id, &sink, |it| {
            if let Some(doc) = it.documents.get_mut(doc_index) {
                doc.save_status = DocSaveStatus::Missing;
                doc.staging_path = None;
                doc.save_error = None;
                doc.display_location = "已从本机删除".to_string();
            }
        });
        Ok(())
    }

    pub async fn export_legacy_documents(
        &self,
        inputs: Vec<LegacyExportInput>,
    ) -> LegacyExportResult {
        let settings = self.get_settings();
        let now = Local::now();
        let today = now.format("%Y%m%d").to_string();
        let subdir_mode = settings.effective_subdir_mode();

        let mut exported_set = crate::store::load_exported_legacy_paths(&self.config_dir);
        let mut used_names = HashSet::new();
        let mut exported = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();
        let mut exported_docs = Vec::new();

        for input in inputs {
            let src = PathBuf::from(&input.path);
            let meta = match std::fs::metadata(&src) {
                Ok(m) if m.is_file() && m.len() > 0 => m,
                _ => {
                    skipped += 1;
                    errors.push(format!("源文件不存在或为空：{}", input.path));
                    continue;
                }
            };

            let raw_name = src
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("送达文书.pdf");
            let ext = safe_extension(None, raw_name);
            let case_clean = input
                .case_number
                .as_deref()
                .map(normalize_case_number)
                .filter(|s| !s.trim().is_empty());
            let raw_stem = strip_trailing_ext(raw_name, ext.trim_start_matches('.'));
            let subdir_segments =
                resolve_subdir_segments(&subdir_mode, case_clean.as_deref(), None, &today);

            match self
                .publish_staged_file(
                    PublishRequest {
                        source_file: &src,
                        subdir_segments: &subdir_segments,
                        stem: &raw_stem,
                        ext: &ext,
                        expected_bytes: meta.len(),
                        settings: &settings,
                        remove_source_on_success: false,
                    },
                    &mut used_names,
                )
                .await
            {
                Ok(outcome) => {
                    exported += 1;
                    exported_set.insert(input.path.clone());
                    exported_docs.push(DownloadedDoc {
                        raw_title: raw_name.to_string(),
                        file_name: outcome.file_name,
                        saved_path: outcome.saved_path,
                        storage_uri: outcome.storage_uri,
                        display_location: outcome.display_location,
                        staging_path: None,
                        save_status: DocSaveStatus::Saved,
                        save_error: None,
                        size_bytes: outcome.size_bytes,
                        court_name: None,
                        document_id: None,
                        source_key: None,
                        format: Some(ext.trim_start_matches('.').to_string()),
                    });
                }
                Err(err) => {
                    errors.push(format!("导出「{}」失败：{}", raw_name, err));
                }
            }
        }

        let _ = crate::store::save_exported_legacy_paths(&self.config_dir, &exported_set);
        LegacyExportResult {
            exported,
            skipped,
            errors,
            exported_docs,
        }
    }

    pub async fn archive_demo_document<S: ProgressSink>(
        &self,
        sink: &S,
    ) -> Result<DownloadedDoc, String> {
        std::fs::create_dir_all(&self.config_dir).map_err(|err| err.to_string())?;
        let staged =
            tempfile::NamedTempFile::new_in(&self.config_dir).map_err(|err| err.to_string())?;
        std::fs::write(
            staged.path(),
            "小达鸭演示文书\n虚构演示案 · 示例人民法院\n本文书仅用于演示本机归档流程。\n",
        )
        .map_err(|err| err.to_string())?;
        let task_id = Uuid::new_v4().to_string();
        {
            let mut state = lock_unpoisoned(&self.inner);
            state.items.push(QueueItem {
                id: task_id.clone(),
                raw_text: String::new(),
                platform: Platform::Unknown,
                platform_label: "本机演示".into(),
                url: String::new(),
                params: Default::default(),
                fingerprint: task_id.clone(),
                case_number: Some("虚构演示案".into()),
                court_name: Some("示例人民法院".into()),
                parties: Vec::new(),
                sms_code: None,
                status: ItemStatus::Running,
                stage_label: "演示归档".into(),
                status_message: "正在保存演示文书".into(),
                receipt_status: Default::default(),
                identity_attempts_used: 0,
                identity_attempts_limit: 0,
                documents: Vec::new(),
                failures: Vec::new(),
                current_doc_index: 0,
                current_doc_total: 1,
                current_doc_name: None,
                current_target_file_name: None,
                bytes_received: 0,
                total_bytes: None,
                speed_bytes_per_sec: 0,
            });
        }
        let result = self
            .ingest_captured_file(&task_id, staged.path(), "演示文书.txt", None, false, sink)
            .await;
        if result.is_err() {
            self.update_item(&task_id, sink, |item| {
                item.status = ItemStatus::Failed;
            });
            self.remove_item(&task_id, sink);
        }
        result
    }

    pub async fn ingest_captured_file<S: ProgressSink>(
        &self,
        task_id: &str,
        source_path: &Path,
        suggested_name: &str,
        source_url: Option<&str>,
        remove_source_on_success: bool,
        sink: &S,
    ) -> Result<DownloadedDoc, String> {
        let (case_number, court_name, parties, doc_num, total_hint, mut used_names) = {
            let guard = lock_unpoisoned(&self.inner);
            let item = guard
                .items
                .iter()
                .find(|i| i.id == task_id)
                .ok_or_else(|| "任务不存在".to_string())?;
            let saved_so_far = item
                .documents
                .iter()
                .filter(|d| d.save_status == DocSaveStatus::Saved)
                .count();
            let used: HashSet<String> = item
                .documents
                .iter()
                .map(|d| d.file_name.to_ascii_lowercase())
                .collect();
            (
                item.case_number.clone(),
                item.court_name.clone(),
                item.parties.clone(),
                saved_so_far + 1,
                item.current_doc_total.max(saved_so_far + 1),
                used,
            )
        };

        let ext = extract_document_extension(suggested_name)
            .or_else(|| source_url.and_then(extract_document_extension))
            .or_else(|| {
                crate::validator::sniff_extension_from_file(source_path).map(str::to_string)
            })
            .unwrap_or_else(|| ".pdf".to_string());

        let size_bytes = match crate::validator::validate_file(source_path, &ext) {
            Ok(sz) => sz,
            Err(err) => {
                if remove_source_on_success {
                    let _ = tokio::fs::remove_file(source_path).await;
                }
                return Err(format!("拦截到的文件未通过校验：{}", err));
            }
        };

        let cleaned_suggested = Path::new(suggested_name.trim())
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .trim();
        let suggested_stem = strip_trailing_ext(cleaned_suggested, ext.trim_start_matches('.'));
        let lower_stem = suggested_stem.trim().to_ascii_lowercase();
        let raw_title = if lower_stem.is_empty()
            || matches!(
                lower_stem.as_str(),
                "download" | "file" | "index" | "getfile" | "view" | "blob" | "document"
            ) {
            format!("文书_{}", doc_num)
        } else {
            cleaned_suggested.to_string()
        };

        let settings = self.get_settings();
        let now = Local::now();
        let today = now.format("%Y%m%d").to_string();
        let today_dash = now.format("%Y-%m-%d").to_string();
        let now_time = now.format("%H%M%S").to_string();
        let stem = render_stem(
            &settings.rename_template,
            &settings.custom_template,
            settings.prefix_index,
            &RenameContext {
                raw_title: &raw_title,
                case_number: case_number.as_deref(),
                court_name: court_name.as_deref(),
                parties: &parties,
                date_str: Some(&today),
                date_dash_str: Some(&today_dash),
                time_str: Some(&now_time),
                index: doc_num,
                total_docs: total_hint,
                extension: &ext,
            },
        );
        let subdir_segments = resolve_subdir_segments(
            &settings.effective_subdir_mode(),
            case_number.as_deref(),
            court_name.as_deref(),
            &today,
        );

        let outcome = self
            .publish_staged_file(
                PublishRequest {
                    source_file: source_path,
                    subdir_segments: &subdir_segments,
                    stem: &stem,
                    ext: &ext,
                    expected_bytes: size_bytes,
                    settings: &settings,
                    remove_source_on_success,
                },
                &mut used_names,
            )
            .await?;

        let doc = DownloadedDoc {
            raw_title,
            file_name: outcome.file_name,
            saved_path: outcome.saved_path,
            storage_uri: outcome.storage_uri,
            display_location: outcome.display_location,
            staging_path: None,
            save_status: DocSaveStatus::Saved,
            save_error: None,
            size_bytes: outcome.size_bytes,
            court_name,
            document_id: None,
            source_key: Some(format!("fallback:{}", Uuid::new_v4())),
            format: Some(ext.trim_start_matches('.').to_string()),
        };

        let doc_for_return = doc.clone();
        self.update_item(task_id, sink, |it| {
            it.documents.push(doc);
            it.failures.clear();
            it.bytes_received = 0;
            it.total_bytes = None;
            it.speed_bytes_per_sec = 0;
            it.current_target_file_name = None;
            let saved_count = it
                .documents
                .iter()
                .filter(|d| d.save_status == DocSaveStatus::Saved)
                .count();
            it.status = ItemStatus::Completed;
            it.stage_label = "已归档".to_string();
            it.status_message = format!("已通过保底方式归档 {} 份文书", saved_count);
        });

        Ok(doc_for_return)
    }

    fn handle_interrupted_item<S: ProgressSink>(
        &self,
        task_id: &str,
        ctrl: TransferControl,
        saved_docs: &[DownloadedDoc],
        failures: &[DocFailure],
        sink: &S,
    ) {
        self.update_item(task_id, sink, |it| {
            it.documents = saved_docs.to_vec();
            it.failures = failures.to_vec();
            it.bytes_received = 0;
            it.total_bytes = None;
            it.speed_bytes_per_sec = 0;
            it.current_target_file_name = None;
            let saved_count = it
                .documents
                .iter()
                .filter(|d| d.save_status == DocSaveStatus::Saved)
                .count();
            match ctrl {
                TransferControl::Paused => {
                    it.status = ItemStatus::Pending;
                    it.stage_label = "已暂停".to_string();
                    it.status_message = if it.documents.is_empty() {
                        "已暂停当前下载，点击「继续」恢复".to_string()
                    } else {
                        format!(
                            "已暂停（已保存 {} 份），点击「继续」恢复剩余文书",
                            saved_count
                        )
                    };
                }
                _ => {
                    it.status = if it.documents.is_empty() && it.failures.is_empty() {
                        ItemStatus::Pending
                    } else {
                        ItemStatus::Partial
                    };
                    it.stage_label = "已停止".to_string();
                    it.status_message = if it.documents.is_empty() {
                        "已停止下载，点击「开始下载」可重新开始".to_string()
                    } else {
                        format!("已停止下载（已保存 {} 份）", saved_count)
                    };
                }
            }
        });
    }

    fn mark_failed<S: ProgressSink>(&self, task_id: &str, message: &str, sink: &S) {
        self.update_item(task_id, sink, |it| {
            it.status = ItemStatus::Failed;
            it.stage_label = "失败".to_string();
            it.status_message = message.to_string();
        });
    }

    fn update_item<S: ProgressSink, F: FnOnce(&mut QueueItem)>(
        &self,
        task_id: &str,
        sink: &S,
        f: F,
    ) {
        self.update_item_inner(task_id, sink, true, f);
    }

    fn update_item_transient<S: ProgressSink, F: FnOnce(&mut QueueItem)>(
        &self,
        task_id: &str,
        sink: &S,
        f: F,
    ) {
        self.update_item_inner(task_id, sink, false, f);
    }

    fn update_item_inner<S: ProgressSink, F: FnOnce(&mut QueueItem)>(
        &self,
        task_id: &str,
        sink: &S,
        persist: bool,
        f: F,
    ) {
        let mut guard = lock_unpoisoned(&self.inner);
        if let Some(it) = guard.items.iter_mut().find(|i| i.id == task_id) {
            f(it);
        }
        let snap = build_snapshot(&guard);
        drop(guard);
        if persist {
            self.persist_history();
        }
        sink.on_queue_updated(snap);
    }
}

struct ProgressTracker {
    started_at: Instant,
    last_emit: Instant,
    last_received: u64,
    last_total: Option<u64>,
}

struct PublishRequest<'a> {
    source_file: &'a Path,
    subdir_segments: &'a [String],
    stem: &'a str,
    ext: &'a str,
    expected_bytes: u64,
    settings: &'a AppSettings,
    remove_source_on_success: bool,
}

struct PublishedOutcome {
    file_name: String,
    saved_path: String,
    storage_uri: Option<String>,
    display_location: String,
    size_bytes: u64,
}

fn extract_document_extension(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path_str = url::Url::parse(trimmed)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_else(|| trimmed.to_string());
    let ext = Path::new(&path_str)
        .extension()
        .and_then(|e| e.to_str())?
        .trim()
        .to_ascii_lowercase();
    if ext.is_empty()
        || ext.len() > 10
        || !ext.chars().all(|c| c.is_ascii_alphanumeric())
        || matches!(
            ext.as_str(),
            "bin"
                | "tmp"
                | "part"
                | "download"
                | "crdownload"
                | "html"
                | "htm"
                | "php"
                | "do"
                | "action"
                | "jsp"
                | "aspx"
                | "asp"
                | "cgi"
        )
    {
        return None;
    }
    Some(format!(".{}", ext))
}

fn finalize_paused_items_as_stopped(items: &mut [QueueItem]) -> bool {
    let mut changed = false;
    for it in items.iter_mut() {
        if it.status == ItemStatus::Pending && it.stage_label == "已暂停" {
            changed = true;
            it.stage_label = "已停止".to_string();
            let saved_count = it
                .documents
                .iter()
                .filter(|d| d.save_status == DocSaveStatus::Saved)
                .count();
            if it.documents.is_empty() && it.failures.is_empty() {
                it.status_message = "已停止下载，点击「开始下载」可重新开始".to_string();
            } else {
                it.status = ItemStatus::Partial;
                it.status_message = format!("已停止下载（已保存 {} 份）", saved_count);
            }
        }
    }
    changed
}

fn merge_docs_preserving_unvisited(
    current: &[DownloadedDoc],
    prior: &[DownloadedDoc],
    processed_keys: &HashSet<String>,
) -> Vec<DownloadedDoc> {
    let mut merged = current.to_vec();
    let mut present_keys: HashSet<String> = current
        .iter()
        .filter_map(|d| d.source_key.clone())
        .collect();
    for doc in prior {
        if !matches!(
            doc.save_status,
            DocSaveStatus::Saved | DocSaveStatus::SaveFailed
        ) {
            continue;
        }
        let dominated = match doc.source_key.as_deref() {
            Some(k) => processed_keys.contains(k) || present_keys.contains(k),
            None => merged
                .iter()
                .any(|m| m.raw_title == doc.raw_title && m.file_name == doc.file_name),
        };
        if !dominated {
            if let Some(ref k) = doc.source_key {
                present_keys.insert(k.clone());
            }
            merged.push(doc.clone());
        }
    }
    merged
}

fn commit_file_noclobber(
    src: &Path,
    target_dir: &Path,
    stem: &str,
    ext_dot: &str,
    expected_bytes: u64,
    used_names: &mut HashSet<String>,
    remove_source_on_success: bool,
) -> Result<PathBuf, String> {
    for _ in 0..1024 {
        let candidate = unique_destination(target_dir, stem, ext_dot, used_names);
        if remove_source_on_success {
            match std::fs::hard_link(src, &candidate) {
                Ok(()) => {
                    let _ = std::fs::remove_file(src);
                    return Ok(candidate);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(_) => {}
            }
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut dst) => {
                let mut src_file =
                    std::fs::File::open(src).map_err(|e| format!("读取临时文件失败：{}", e))?;
                let copied = std::io::copy(&mut src_file, &mut dst).map_err(|e| {
                    let _ = std::fs::remove_file(&candidate);
                    format!("写入目标文件失败：{}", e)
                })?;
                if let Err(e) = dst.sync_all() {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(format!("写入同步校验失败：{}", e));
                }
                if copied != expected_bytes {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(format!(
                        "文件写入校验未通过（写入 {} / 预期 {} 字节）",
                        copied, expected_bytes
                    ));
                }
                drop(dst);
                drop(src_file);
                if remove_source_on_success {
                    let _ = std::fs::remove_file(src);
                }
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("创建目标文件失败：{}", e)),
        }
    }
    Err("未能生成不重名的目标文件名".to_string())
}

fn recompute_item_completion_state(it: &mut QueueItem) {
    let saved_count = it
        .documents
        .iter()
        .filter(|d| d.save_status == DocSaveStatus::Saved)
        .count();
    let save_failed_count = it
        .documents
        .iter()
        .filter(|d| d.save_status == DocSaveStatus::SaveFailed)
        .count();

    if it.failures.is_empty() && save_failed_count == 0 {
        it.status = ItemStatus::Completed;
        it.stage_label = "已保存".to_string();
        it.status_message = format!("已完成 {} 份文书下载并保存", it.documents.len());
    } else if saved_count > 0 || save_failed_count > 0 {
        it.status = if saved_count == 0 {
            ItemStatus::Failed
        } else {
            ItemStatus::Partial
        };
        it.stage_label = if save_failed_count > 0 {
            "保存待重试".to_string()
        } else {
            "部分完成".to_string()
        };
        it.status_message = if save_failed_count > 0 {
            format!(
                "已保存 {} 份，{} 份保存到目录失败（已保留临时文件可重试保存）",
                saved_count, save_failed_count
            )
        } else {
            format!("成功 {} 份，失败 {} 份", saved_count, it.failures.len())
        };
    } else {
        it.status = ItemStatus::Failed;
        it.stage_label = "失败".to_string();
        it.status_message = it
            .failures
            .first()
            .map(|f| f.reason.clone())
            .unwrap_or_else(|| "下载失败".to_string());
    }
}

async fn wait_non_running(rx: &mut watch::Receiver<TransferControl>) -> TransferControl {
    loop {
        let cur = *rx.borrow_and_update();
        if cur != TransferControl::Running {
            return cur;
        }
        if rx.changed().await.is_err() {
            return TransferControl::Cancelled;
        }
    }
}

fn build_snapshot(state: &QueueState) -> QueueSnapshot {
    let total_count = state.items.len();
    let completed_count = state
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::Completed)
        .count();
    let failed_count = state
        .items
        .iter()
        .filter(|i| matches!(i.status, ItemStatus::Failed | ItemStatus::Partial))
        .count();
    let needs_identity_count = state
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::NeedsIdentity)
        .count();
    let pending_count = state
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::Pending)
        .count();
    let browser_required_count = state
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::BrowserRequired)
        .count();
    let total_downloaded_docs = state
        .items
        .iter()
        .flat_map(|i| i.documents.iter())
        .filter(|d| d.save_status == DocSaveStatus::Saved)
        .count();
    let total_downloaded_bytes = state
        .items
        .iter()
        .flat_map(|i| i.documents.iter())
        .filter(|d| d.save_status == DocSaveStatus::Saved)
        .map(|d| d.size_bytes)
        .sum();

    QueueSnapshot {
        items: state.items.clone(),
        is_running: state.is_running,
        is_paused: state.is_paused,
        total_count,
        completed_count,
        failed_count,
        needs_identity_count,
        pending_count,
        browser_required_count,
        total_downloaded_docs,
        total_downloaded_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[derive(Clone, Default)]
    struct MemorySink {
        snapshots: Arc<Mutex<Vec<QueueSnapshot>>>,
    }

    impl ProgressSink for MemorySink {
        fn on_queue_updated(&self, snapshot: QueueSnapshot) {
            self.snapshots.lock().unwrap().push(snapshot);
        }

        fn on_file_progress(&self, _progress: FileProgress) {}
    }

    #[tokio::test]
    async fn paused_queue_can_be_stopped_without_starting_another_task() {
        let tmp = TempDir::new().unwrap();
        let settings = AppSettings::default_with_dir(tmp.path().display().to_string());
        let manager = QueueManager::new(settings, tmp.path().into());
        let sink = MemorySink::default();
        manager.add_batch("https://zxfw.court.gov.cn/?qdbh=Q&sdbh=S&sdsin=X", &sink);
        let permit = manager.download_lock.lock().await;
        assert!(manager.start_queue(sink.clone()));
        manager.pause_queue(&sink);
        assert!(manager.snapshot().is_paused);
        drop(permit);
        tokio::task::yield_now().await;
        manager.cancel_queue(&sink);
        tokio::time::timeout(Duration::from_secs(2), async {
            while manager.snapshot().is_running {
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        assert_eq!(manager.snapshot().items[0].status, ItemStatus::Pending);
    }

    #[test]
    fn enqueue_deduplicates_and_removes_items() {
        let tmp = TempDir::new().unwrap();
        let settings = AppSettings::default_with_dir(tmp.path().to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, tmp.path().to_path_buf());
        let sink = MemorySink::default();

        let input = "https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1\n\
                     https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1\n\
                     https://sd.gdems.com/v3/dzsd/abc";
        let summary = mgr.add_batch(input, &sink);
        assert_eq!(summary.added.len(), 2);
        assert_eq!(summary.skipped_duplicates, 1);

        let snap = mgr.snapshot();
        assert_eq!(snap.total_count, 2);

        mgr.clear_finished(&sink);
        let snap_after_clear = mgr.snapshot();
        assert_eq!(snap_after_clear.total_count, 1);
        assert_eq!(snap_after_clear.items[0].platform, Platform::Zxfw);
    }

    #[tokio::test]
    async fn retry_failed_does_not_start_unrelated_pending_items() {
        let tmp = TempDir::new().unwrap();
        let settings = AppSettings::default_with_dir(tmp.path().to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, tmp.path().to_path_buf());
        let sink = MemorySink::default();

        let input = "https://zxfw.court.gov.cn/?qdbh=Q1&sdbh=S1&sdsin=X1\n\
                     https://zxfw.court.gov.cn/?qdbh=Q2&sdbh=S2&sdsin=X2";
        mgr.add_batch(input, &sink);

        let first_id = {
            let mut guard = mgr.inner.lock().unwrap();
            guard.items[0].status = ItemStatus::Failed;
            guard.items[0].status_message = "模拟失败".to_string();
            guard.items[0].id.clone()
        };

        let _permit = mgr.download_lock.lock().await;
        assert!(mgr.retry_failed(sink.clone()));
        let snap = mgr.snapshot();
        // Second item stays Pending and is never started by retry_failed.
        assert_eq!(snap.items[0].id, first_id);
        assert_eq!(snap.items[0].status, ItemStatus::Pending);
        assert_eq!(snap.items[1].status, ItemStatus::Pending);

        mgr.cancel_queue(&sink);
        drop(_permit);
        tokio::time::timeout(Duration::from_secs(2), async {
            while mgr.snapshot().is_running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(mgr.snapshot().items[1].status, ItemStatus::Pending);
    }

    #[tokio::test]
    async fn demo_archives_offline_and_clear_keeps_the_document() {
        let root = TempDir::new().unwrap();
        let output = root.path().join("output");
        let config = root.path().join("config");
        let manager = QueueManager::new(
            AppSettings::default_with_dir(output.to_string_lossy().into_owned()),
            config.clone(),
        );
        let sink = MemorySink::default();
        let doc = manager.archive_demo_document(&sink).await.unwrap();
        assert!(std::fs::read_to_string(&doc.saved_path)
            .unwrap()
            .contains("虚构演示案"));
        assert_eq!(crate::store::load_queue_history(&config).len(), 1);

        manager.clear_local_data(&sink).unwrap();
        assert!(manager.snapshot().items.is_empty());
        assert!(crate::store::load_queue_history(&config).is_empty());
        assert!(Path::new(&doc.saved_path).is_file());
    }

    #[tokio::test]
    async fn persists_history_updates_case_number_and_retries_staged_save() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, config_dir.clone());
        let sink = MemorySink::default();

        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q7&sdbh=S7&sdsin=X7", &sink);
        let item_id = mgr.snapshot().items[0].id.clone();

        mgr.update_item_case_number(&item_id, Some("（2026）\u{3000}粤03民初88号".to_string()), &sink)
            .unwrap();

        let staging_dir = crate::store::staging_dir_path(&config_dir);
        std::fs::create_dir_all(&staging_dir).unwrap();
        let staged_file = staging_dir.join("staged_sample.pdf");
        std::fs::write(&staged_file, b"%PDF-1.4\nsample payload").unwrap();

        {
            let mut guard = mgr.inner.lock().unwrap();
            let it = guard.items.iter_mut().find(|i| i.id == item_id).unwrap();
            it.status = ItemStatus::Failed;
            it.documents.push(DownloadedDoc {
                raw_title: "举证材料.pdf".to_string(),
                file_name: "举证材料.pdf".to_string(),
                saved_path: String::new(),
                storage_uri: None,
                display_location: "待保存".to_string(),
                staging_path: Some(staged_file.to_string_lossy().to_string()),
                save_status: DocSaveStatus::SaveFailed,
                save_error: Some("权限错误".to_string()),
                size_bytes: 23,
                court_name: Some("深圳中院".to_string()),
                document_id: None,
                source_key: Some("id:doc1".to_string()),
                format: Some("pdf".to_string()),
            });
            it.failures.push(DocFailure {
                index: 1,
                name: Some("举证材料.pdf".to_string()),
                reason: "保存失败".to_string(),
                source_key: Some("id:doc1".to_string()),
            });
        }
        // Write an old-format history to exercise migration rather than the new serializer.
        let mut old = mgr.snapshot().items;
        old[0].sms_code = Some("fixture-code".into());
        old[0].raw_text = "fixture-message".into();
        old[0].status_message = "https://example.invalid/?token=fixture-token".into();
        old[0].documents[0].source_key =
            Some("url:https://example.invalid/?token=fixture-token".into());
        old[0].failures[0].source_key = old[0].documents[0].source_key.clone();
        std::fs::write(
            crate::store::history_path(&config_dir),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        crate::store::migrate_sensitive_storage(&config_dir, &mgr.get_settings()).unwrap();
        let on_disk = std::fs::read_to_string(crate::store::history_path(&config_dir)).unwrap();
        for secret in [
            "fixture-code",
            "fixture-message",
            "fixture-token",
            "qdbh=Q7",
        ] {
            assert!(
                !on_disk.contains(secret),
                "history contains a private value"
            );
        }

        // Reload QueueManager from persisted history on restart.
        let reloaded = QueueManager::new(mgr.get_settings(), config_dir.clone());
        let snap = reloaded.snapshot();
        assert_eq!(snap.total_count, 1);
        assert_eq!(
            snap.items[0].case_number.as_deref(),
            Some("（2026） 粤03民初88号")
        );
        assert_eq!(snap.total_downloaded_docs, 0);
        assert!(snap.items[0].url.is_empty());
        assert!(!reloaded.retry_failed(sink.clone()));

        let reimported =
            reloaded.add_batch("https://zxfw.court.gov.cn/?qdbh=Q7&sdbh=S7&sdsin=X7", &sink);
        assert_eq!(reimported.added[0].id, item_id);
        assert_eq!(reimported.added[0].documents.len(), 1);

        reloaded
            .retry_save_document(item_id.clone(), 0, sink.clone())
            .await
            .unwrap();
        let after = reloaded.snapshot();
        assert_eq!(after.items[0].status, ItemStatus::Completed);
        assert_eq!(after.total_downloaded_docs, 1);
        assert_eq!(after.items[0].documents[0].save_status, DocSaveStatus::Saved);
        assert!(!staged_file.exists());
        assert!(Path::new(&after.items[0].documents[0].saved_path).exists());
    }

    #[test]
    fn commit_file_noclobber_never_overwrites_existing_files() {
        let tmp = TempDir::new().unwrap();
        let target_dir = tmp.path().join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        let existing_file = target_dir.join("传票_20261118.pdf");
        std::fs::write(&existing_file, b"ORIGINAL_DO_NOT_OVERWRITE").unwrap();

        let src_file = tmp.path().join("staged.part");
        let new_bytes = b"%PDF-1.4\nNEW_PAYLOAD";
        std::fs::write(&src_file, new_bytes).unwrap();

        let mut used_names = HashSet::new();
        let committed = commit_file_noclobber(
            &src_file,
            &target_dir,
            "传票_20261118",
            ".pdf",
            new_bytes.len() as u64,
            &mut used_names,
            true,
        )
        .unwrap();

        assert_eq!(
            std::fs::read(&existing_file).unwrap(),
            b"ORIGINAL_DO_NOT_OVERWRITE"
        );
        assert_eq!(
            committed.file_name().unwrap().to_str().unwrap(),
            "传票_20261118_2.pdf"
        );
        assert_eq!(std::fs::read(&committed).unwrap(), new_bytes);
        assert!(!src_file.exists());
    }

    #[tokio::test]
    async fn retry_save_preserves_other_docs_failures_when_indices_offset() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, config_dir.clone());
        let sink = MemorySink::default();

        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q8&sdbh=S8&sdsin=X8", &sink);
        let item_id = mgr.snapshot().items[0].id.clone();

        let staging_dir = crate::store::staging_dir_path(&config_dir);
        std::fs::create_dir_all(&staging_dir).unwrap();
        let staged_file = staging_dir.join("doc2.pdf");
        let payload = b"%PDF-1.4\ndoc2 payload";
        std::fs::write(&staged_file, payload).unwrap();

        {
            let mut guard = mgr.inner.lock().unwrap();
            let it = guard.items.iter_mut().find(|i| i.id == item_id).unwrap();
            it.status = ItemStatus::Failed;
            // Doc #1 failed download and was NOT placed in `documents`, so Doc #2 is at documents[0]!
            it.failures.push(DocFailure {
                index: 1,
                name: Some("第一份文书.pdf".to_string()),
                reason: "缺少下载地址".to_string(),
                source_key: Some("id:doc1".to_string()),
            });
            it.documents.push(DownloadedDoc {
                raw_title: "第二份文书.pdf".to_string(),
                file_name: "第二份文书.pdf".to_string(),
                saved_path: String::new(),
                storage_uri: None,
                display_location: "待保存".to_string(),
                staging_path: Some(staged_file.to_string_lossy().to_string()),
                save_status: DocSaveStatus::SaveFailed,
                save_error: Some("权限拒绝".to_string()),
                size_bytes: payload.len() as u64,
                court_name: None,
                document_id: Some("doc2".to_string()),
                source_key: Some("id:doc2".to_string()),
                format: Some("pdf".to_string()),
            });
            it.failures.push(DocFailure {
                index: 2,
                name: Some("第二份文书.pdf".to_string()),
                reason: "已下载并校验，保存失败：权限拒绝".to_string(),
                source_key: Some("id:doc2".to_string()),
            });
        }

        mgr.retry_save_document(item_id.clone(), 0, sink.clone())
            .await
            .unwrap();
        let after = mgr.snapshot();
        let it = &after.items[0];
        assert_eq!(it.status, ItemStatus::Partial);
        assert_eq!(it.failures.len(), 1);
        assert_eq!(it.failures[0].index, 1);
        assert_eq!(it.failures[0].source_key.as_deref(), Some("id:doc1"));
        assert_eq!(it.documents[0].save_status, DocSaveStatus::Saved);
    }

    #[tokio::test]
    async fn pause_and_cancel_interrupt_active_file_transfer_immediately() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut req_buf = [0u8; 512];
                    let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut req_buf).await;
                    let _ = stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\n\r\n%PDF-1.4\nstalled",
                        )
                        .await;
                    // Intentionally keep the socket open so download_doc_entries blocks mid-stream.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });

        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings.clone(), config_dir.clone());
        let sink = MemorySink::default();

        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q9&sdbh=S9&sdsin=X9", &sink);
        let item = mgr.snapshot().items[0].clone();
        let client = ZxfwClient::new().unwrap();
        let doc = ZxfwDocEntry {
            c_wsmc: Some("超大卷宗.pdf".to_string()),
            wjlj: Some(format!("http://{}/large.pdf", addr)),
            c_fymc: Some("深圳中院".to_string()),
            c_wsbh: Some("doc-stall".to_string()),
            c_wjgs: Some("pdf".to_string()),
        };

        // 1. Immediate pause during active streaming
        let (tx_pause, rx_pause) = watch::channel(TransferControl::Running);
        let mgr_pause = mgr.clone();
        let sink_pause = sink.clone();
        let item_pause = item.clone();
        let client_pause = client.clone();
        let doc_pause = doc.clone();
        let settings_pause = settings.clone();
        let task = tokio::spawn(async move {
            mgr_pause
                .download_doc_entries(
                    &item_pause,
                    &client_pause,
                    vec![doc_pause],
                    &settings_pause,
                    Some(rx_pause),
                    &sink_pause,
                )
                .await;
        });
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = tx_pause.send(TransferControl::Paused);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("pause must interrupt active file download immediately")
            .unwrap();
        let snap = mgr.snapshot();
        assert_eq!(snap.items[0].status, ItemStatus::Pending);
        assert_eq!(snap.items[0].stage_label, "已暂停");

        // 2. Immediate stop/cancel during active streaming
        let (tx_cancel, rx_cancel) = watch::channel(TransferControl::Running);
        let mgr_cancel = mgr.clone();
        let sink_cancel = sink.clone();
        let task2 = tokio::spawn(async move {
            mgr_cancel
                .download_doc_entries(
                    &item,
                    &client,
                    vec![doc],
                    &settings,
                    Some(rx_cancel),
                    &sink_cancel,
                )
                .await;
        });
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = tx_cancel.send(TransferControl::Cancelled);
        tokio::time::timeout(Duration::from_secs(1), task2)
            .await
            .expect("cancel must interrupt active file download immediately")
            .unwrap();
        let snap2 = mgr.snapshot();
        assert_eq!(snap2.items[0].status, ItemStatus::Pending);
        assert_eq!(snap2.items[0].stage_label, "已停止");
    }

    #[tokio::test]
    async fn pause_during_retry_preserves_previously_saved_unvisited_docs() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut req_buf = [0u8; 512];
                    let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut req_buf).await;
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\n\r\n%PDF-1.4\nstall")
                        .await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });

        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        std::fs::create_dir_all(&export_dir).unwrap();
        let existing_doc2 = export_dir.join("doc2.pdf");
        std::fs::write(&existing_doc2, b"%PDF-1.4\nalready saved doc2").unwrap();

        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings.clone(), config_dir);
        let sink = MemorySink::default();

        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q10&sdbh=S10&sdsin=X10", &sink);
        let item_id = mgr.snapshot().items[0].id.clone();
        {
            let mut guard = lock_unpoisoned(&mgr.inner);
            let it = guard.items.iter_mut().find(|i| i.id == item_id).unwrap();
            it.documents.push(DownloadedDoc {
                raw_title: "第二份文书.pdf".to_string(),
                file_name: "doc2.pdf".to_string(),
                saved_path: existing_doc2.to_string_lossy().to_string(),
                storage_uri: None,
                display_location: existing_doc2.to_string_lossy().to_string(),
                staging_path: None,
                save_status: DocSaveStatus::Saved,
                save_error: None,
                size_bytes: 27,
                court_name: None,
                document_id: Some("doc2".to_string()),
                source_key: Some("id:doc2".to_string()),
                format: Some("pdf".to_string()),
            });
        }

        let item = mgr.snapshot().items[0].clone();
        let client = ZxfwClient::new().unwrap();
        let docs = vec![
            ZxfwDocEntry {
                c_wsmc: Some("第一份文书.pdf".to_string()),
                wjlj: Some(format!("http://{}/doc1.pdf", addr)),
                c_fymc: None,
                c_wsbh: Some("doc1".to_string()),
                c_wjgs: Some("pdf".to_string()),
            },
            ZxfwDocEntry {
                c_wsmc: Some("第二份文书.pdf".to_string()),
                wjlj: Some(format!("http://{}/doc2.pdf", addr)),
                c_fymc: None,
                c_wsbh: Some("doc2".to_string()),
                c_wjgs: Some("pdf".to_string()),
            },
        ];

        let (tx_pause, rx_pause) = watch::channel(TransferControl::Running);
        let mgr_pause = mgr.clone();
        let sink_pause = sink.clone();
        let task = tokio::spawn(async move {
            mgr_pause
                .download_doc_entries(&item, &client, docs, &settings, Some(rx_pause), &sink_pause)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = tx_pause.send(TransferControl::Paused);
        tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap();

        let snap = mgr.snapshot();
        assert_eq!(snap.items[0].documents.len(), 1);
        assert_eq!(snap.items[0].documents[0].source_key.as_deref(), Some("id:doc2"));
    }

    #[test]
    fn recompute_marks_failed_when_zero_docs_saved() {
        let mut item = parse_batch_input(
            "https://zxfw.court.gov.cn/?qdbh=Q&sdbh=S&sdsin=X",
            &HashSet::new(),
        )
        .added
        .pop()
        .unwrap();
        item.documents.push(DownloadedDoc {
            raw_title: "doc2.pdf".to_string(),
            file_name: "doc2.pdf".to_string(),
            saved_path: String::new(),
            storage_uri: None,
            display_location: "待保存".to_string(),
            staging_path: Some("/tmp/doc2.pdf".to_string()),
            save_status: DocSaveStatus::SaveFailed,
            save_error: Some("权限错误".to_string()),
            size_bytes: 10,
            court_name: None,
            document_id: None,
            source_key: Some("id:2".to_string()),
            format: Some("pdf".to_string()),
        });
        item.failures.push(DocFailure {
            index: 1,
            name: Some("doc1.pdf".to_string()),
            reason: "404".to_string(),
            source_key: Some("id:1".to_string()),
        });
        item.failures.push(DocFailure {
            index: 2,
            name: Some("doc2.pdf".to_string()),
            reason: "保存失败".to_string(),
            source_key: Some("id:2".to_string()),
        });

        recompute_item_completion_state(&mut item);
        assert_eq!(item.status, ItemStatus::Failed);
    }

    #[tokio::test]
    async fn export_legacy_documents_preserves_docx_extension_and_does_not_hardlink() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let legacy_dir = tmp.path().join("legacy");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let orig_path = legacy_dir.join("证据清单.docx");
        std::fs::write(&orig_path, b"PK\x03\x04original docx bytes").unwrap();

        let mut settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        settings.create_case_subdir = false;
        settings.subdir_mode = crate::models::SubdirMode::None;
        let mgr = QueueManager::new(settings, config_dir);

        let res = mgr
            .export_legacy_documents(vec![LegacyExportInput {
                path: orig_path.to_string_lossy().to_string(),
                case_number: None,
            }])
            .await;
        assert_eq!(res.exported, 1);
        assert_eq!(res.exported_docs[0].file_name, "证据清单.docx");

        let exported_path = PathBuf::from(&res.exported_docs[0].saved_path);
        std::fs::write(&exported_path, b"MODIFIED_COPY").unwrap();
        assert_eq!(std::fs::read(&orig_path).unwrap(), b"PK\x03\x04original docx bytes");
    }

    #[tokio::test]
    async fn retry_save_document_preserves_parties_in_custom_template() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let mut settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        settings.rename_template = crate::models::RenameTemplate::Custom;
        settings.custom_template = "{parties}_{title}".to_string();
        settings.subdir_mode = crate::models::SubdirMode::None;
        settings.create_case_subdir = false;

        let mgr = QueueManager::new(settings, config_dir.clone());
        let sink = MemorySink::default();
        mgr.add_batch(
            "张三与李四买卖合同纠纷一案 https://zxfw.court.gov.cn/?qdbh=Q11&sdbh=S11&sdsin=X11",
            &sink,
        );
        let item_id = mgr.snapshot().items[0].id.clone();

        let staging_dir = crate::store::staging_dir_path(&config_dir);
        std::fs::create_dir_all(&staging_dir).unwrap();
        let staged_file = staging_dir.join("staged_party.pdf");
        let payload = b"%PDF-1.4\nparty payload";
        std::fs::write(&staged_file, payload).unwrap();

        {
            let mut guard = lock_unpoisoned(&mgr.inner);
            let it = guard.items.iter_mut().find(|i| i.id == item_id).unwrap();
            it.status = ItemStatus::Failed;
            it.documents.push(DownloadedDoc {
                raw_title: "判决书.pdf".to_string(),
                file_name: "判决书.pdf".to_string(),
                saved_path: String::new(),
                storage_uri: None,
                display_location: "待保存".to_string(),
                staging_path: Some(staged_file.to_string_lossy().to_string()),
                save_status: DocSaveStatus::SaveFailed,
                save_error: Some("权限错误".to_string()),
                size_bytes: payload.len() as u64,
                court_name: None,
                document_id: None,
                source_key: Some("id:p1".to_string()),
                format: Some("pdf".to_string()),
            });
        }

        mgr.retry_save_document(item_id, 0, sink).await.unwrap();
        let snap = mgr.snapshot();
        assert_eq!(snap.items[0].documents[0].file_name, "张三、李四_判决书.pdf");
    }

    #[tokio::test]
    async fn cancel_while_paused_clears_paused_stage_label() {
        let tmp = TempDir::new().unwrap();
        let settings = AppSettings::default_with_dir(tmp.path().to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, tmp.path().to_path_buf());
        let sink = MemorySink::default();
        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q12&sdbh=S12&sdsin=X12", &sink);
        {
            let mut guard = lock_unpoisoned(&mgr.inner);
            guard.is_running = true;
            guard.is_paused = true;
            guard.items[0].status = ItemStatus::Pending;
            guard.items[0].stage_label = "已暂停".to_string();
        }
        mgr.cancel_queue(&sink);
        let snap = mgr.snapshot();
        assert!(!snap.is_paused);
        assert_eq!(snap.items[0].stage_label, "已停止");
    }

    #[tokio::test]
    async fn verify_and_download_succeeds_without_consuming_attempt_and_runs_in_background() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let base_for_srv = base_url.clone();
        let receipt_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let receipt_counter = receipt_requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let srv_base = base_for_srv.clone();
                let receipt_counter = receipt_counter.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (ctype, body) = if path.ends_with("/getSfhyKg") {
                        ("application/json", br#"{"code":200,"data":true}"#.to_vec())
                    } else if path.ends_with("/updateSdxxBySdzt") {
                        receipt_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        ("application/json", br#"{"code":500}"#.to_vec())
                    } else if path.ends_with("/checkSdryxx") {
                        ("application/json", br#"{"code":200}"#.to_vec())
                    } else if path.ends_with("/getWsListBySdbhNew") {
                        let json = serde_json::json!({
                            "code": 200,
                            "data": [{
                                "c_wsmc": "应诉通知书",
                                "c_wjgs": "pdf",
                                "c_fymc": "测试法院",
                                "c_wsbh": "ws-1",
                                "wjlj": format!("{}/doc/ws-1.pdf", srv_base)
                            }]
                        });
                        ("application/json", json.to_string().into_bytes())
                    } else {
                        ("application/pdf", b"%PDF-1.7\nverified doc payload".to_vec())
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        ctype,
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });

        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, config_dir);
        let sink = MemorySink::default();

        mgr.add_batch("https://zxfw.court.gov.cn/?qdbh=Q13&sdbh=S13&sdsin=X13", &sink);
        let item_id = mgr.snapshot().items[0].id.clone();
        {
            let mut guard = lock_unpoisoned(&mgr.inner);
            guard.items[0].status = ItemStatus::NeedsIdentity;
        }

        mgr.verify_and_download_with_base(
            item_id.clone(),
            "sjhm".to_string(),
            "1234".to_string(),
            Some(&base_url),
            sink,
        )
        .await
        .unwrap();

        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let s = mgr.snapshot();
                if !s.is_running && s.items[0].status == ItemStatus::Completed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        let snap = mgr.snapshot();
        assert_eq!(snap.items[0].identity_attempts_used, 0);
        assert_eq!(
            snap.items[0].receipt_status,
            crate::models::ReceiptStatus::NotRequested
        );
        assert_eq!(
            receipt_requests.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert_eq!(snap.items[0].documents.len(), 1);
        assert_eq!(snap.items[0].documents[0].save_status, DocSaveStatus::Saved);
    }

    #[tokio::test]
    async fn ingest_captured_file_sniffs_renames_validates_and_archives() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("cfg");
        let export_dir = tmp.path().join("export");
        let fallback_dir = tmp.path().join("fallback_staging");
        std::fs::create_dir_all(&fallback_dir).unwrap();

        let settings = AppSettings::default_with_dir(export_dir.to_string_lossy().to_string());
        let mgr = QueueManager::new(settings, config_dir);
        let sink = MemorySink::default();

        mgr.add_batch(
            "【广东高院】您有（2026）粤民终100号案件文书：https://sd.gdems.com/v3/dzsd/demo1",
            &sink,
        );
        let item_id = mgr.snapshot().items[0].id.clone();

        // 1. Extensionless PDF file downloaded from fallback webview -> sniffed, renamed, source deleted.
        let temp_pdf = fallback_dir.join("raw_blob");
        std::fs::write(&temp_pdf, b"%PDF-1.7\nreal pdf bytes").unwrap();
        let doc = mgr
            .ingest_captured_file(
                &item_id,
                &temp_pdf,
                "download.do",
                Some("https://sd.gdems.com/download.do?id=1"),
                true,
                &sink,
            )
            .await
            .unwrap();
        assert!(!temp_pdf.exists());
        assert!(doc.file_name.ends_with(".pdf"));
        assert!(doc.file_name.contains("（2026）粤民终100号"));
        assert!(Path::new(&doc.saved_path).exists());

        // 2. Disguised HTML error page -> rejected and temp file removed.
        let bad_html = fallback_dir.join("err.pdf");
        std::fs::write(&bad_html, b"<!DOCTYPE html><html>not a doc</html>").unwrap();
        let err = mgr
            .ingest_captured_file(&item_id, &bad_html, "传票.pdf", None, true, &sink)
            .await
            .unwrap_err();
        assert!(err.contains("未通过校验"));
        assert!(!bad_html.exists());

        // 3. Manual local file import (remove_source_on_success = false) -> original preserved.
        let manual_dir = tmp.path().join("manual");
        std::fs::create_dir_all(&manual_dir).unwrap();
        let local_png = manual_dir.join("送达截图.png");
        std::fs::write(&local_png, b"\x89PNG\r\n\x1A\npng-payload").unwrap();
        let doc2 = mgr
            .ingest_captured_file(&item_id, &local_png, "送达截图.png", None, false, &sink)
            .await
            .unwrap();
        assert!(local_png.exists());
        assert!(doc2.file_name.ends_with(".png"));
        assert_eq!(mgr.snapshot().items[0].status, ItemStatus::Completed);
        assert_eq!(mgr.snapshot().items[0].documents.len(), 2);
    }
}
