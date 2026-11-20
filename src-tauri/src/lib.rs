pub mod android_storage;
pub mod browser_fallback;
pub mod hubei_client;
#[cfg(desktop)]
mod menu;
pub mod models;
pub mod parser;
pub mod queue;
pub mod renamer;
pub mod store;
pub mod validator;
pub mod zxfw_client;

use android_storage::{guess_mime_type, AndroidStorageHandle};
use browser_fallback::{FallbackManager, FallbackSessionSnapshot};
use models::{
    AndroidStorageMode, AppSettings, LawyerProfile, LegacyDocumentEntry, LegacyExportInput,
    LegacyExportResult, ParseBatchSummary, QueueSnapshot, RenameTemplate, SubdirMode,
};
use queue::{QueueManager, TauriProgressSink};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn get_settings(state: State<'_, QueueManager>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
fn update_settings(
    settings: AppSettings,
    state: State<'_, QueueManager>,
) -> Result<AppSettings, String> {
    state.update_settings(settings)
}

#[tauri::command]
fn save_lawyer_profile(
    name: String,
    mobile_last4: String,
    id_last4: Option<String>,
    state: State<'_, QueueManager>,
) -> Result<AppSettings, String> {
    let mut current = state.get_settings();
    store::upsert_profile(&mut current, name, mobile_last4, id_last4)?;
    state.update_settings(current)
}

#[tauri::command]
fn batch_save_lawyer_profiles(
    raw_text: String,
    state: State<'_, QueueManager>,
) -> Result<(AppSettings, usize), String> {
    let mut current = state.get_settings();
    let count = store::batch_upsert_profiles(&mut current, &raw_text)?;
    let saved = state.update_settings(current)?;
    Ok((saved, count))
}

#[tauri::command]
fn delete_lawyer_profile(
    profile_id: String,
    state: State<'_, QueueManager>,
) -> Result<AppSettings, String> {
    let mut current = state.get_settings();
    current.profiles.retain(|p: &LawyerProfile| p.id != profile_id);
    state.update_settings(current)
}

#[tauri::command]
fn get_queue_snapshot(state: State<'_, QueueManager>) -> QueueSnapshot {
    state.snapshot()
}

#[tauri::command]
fn parse_and_enqueue(
    raw_input: String,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> ParseBatchSummary {
    let sink = TauriProgressSink::new(app);
    state.add_batch(&raw_input, &sink)
}

#[tauri::command]
fn start_queue(app: AppHandle, state: State<'_, QueueManager>) -> bool {
    let sink = TauriProgressSink::new(app);
    state.start_queue(sink)
}

#[tauri::command]
fn respond_to_receipt(
    prompt_id: String,
    submit: bool,
    remember: bool,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<AppSettings, String> {
    state.respond_to_receipt(&prompt_id, submit, remember, &TauriProgressSink::new(app))
}

#[tauri::command]
fn pause_queue(app: AppHandle, state: State<'_, QueueManager>) {
    let sink = TauriProgressSink::new(app);
    state.pause_queue(&sink);
}

#[tauri::command]
fn resume_queue(app: AppHandle, state: State<'_, QueueManager>) {
    let sink = TauriProgressSink::new(app);
    state.resume_queue(&sink);
}

#[tauri::command]
fn cancel_queue(app: AppHandle, state: State<'_, QueueManager>) {
    let sink = TauriProgressSink::new(app);
    state.cancel_queue(&sink);
}

#[tauri::command]
fn retry_failed(app: AppHandle, state: State<'_, QueueManager>) -> bool {
    let sink = TauriProgressSink::new(app);
    state.retry_failed(sink)
}

#[tauri::command]
fn remove_queue_item(task_id: String, app: AppHandle, state: State<'_, QueueManager>) {
    let sink = TauriProgressSink::new(app);
    state.remove_item(&task_id, &sink);
}

#[tauri::command]
fn clear_finished(app: AppHandle, state: State<'_, QueueManager>) {
    let sink = TauriProgressSink::new(app);
    state.clear_finished(&sink);
}

#[tauri::command]
fn clear_local_data(
    app: AppHandle,
    state: State<'_, QueueManager>,
    fallback: State<'_, FallbackManager>,
) -> Result<AppSettings, String> {
    if fallback.snapshot().is_some() {
        return Err("请先结束当前内置取件，再清除本机记录。".into());
    }
    state.clear_local_data(&TauriProgressSink::new(app))
}

#[tauri::command]
async fn submit_identity(
    task_id: String,
    method: String,
    tail: String,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let sink = TauriProgressSink::new(app);
    state
        .verify_and_download(task_id, method, tail, sink)
        .await
}

#[tauri::command]
async fn archive_demo_document(
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<models::DownloadedDoc, String> {
    state
        .archive_demo_document(&TauriProgressSink::new(app))
        .await
}

#[tauri::command]
async fn copy_sensitive_text(text: String, state: State<'_, QueueManager>) -> Result<(), String> {
    state.storage().copy_sensitive_text(&text).await
}

#[tauri::command]
fn open_output_dir(app: AppHandle, state: State<'_, QueueManager>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.get_settings().output_dir;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_download(
    task_id: String,
    index: usize,
    reveal: bool,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let snap = state.snapshot();
    let doc = snap
        .items
        .iter()
        .find(|item| item.id == task_id)
        .and_then(|item| item.documents.get(index))
        .ok_or("未找到该文书")?
        .clone();

    if doc.saved_path.is_empty() {
        return Err("文书尚未保存到目录，请点击「重试保存」".to_string());
    }

    if state.storage().is_available() {
        let mime = guess_mime_type(&doc.file_name);
        let res = state
            .storage()
            .open_document(&doc.saved_path, Some(mime))
            .await;
        if let Err(ref msg) = res {
            if msg.starts_with("MISSING:") {
                let sink = TauriProgressSink::new(app);
                state.refresh_document_statuses(sink).await;
            }
        }
        return res;
    }

    #[cfg(desktop)]
    {
        use tauri_plugin_opener::OpenerExt;
        if !std::path::Path::new(&doc.saved_path).exists() {
            let sink = TauriProgressSink::new(app);
            state.refresh_document_statuses(sink).await;
            return Err("MISSING:文件已被移动或删除".to_string());
        }
        let res = if reveal {
            app.opener().reveal_item_in_dir(&doc.saved_path)
        } else {
            app.opener().open_path(&doc.saved_path, None::<&str>)
        };
        res.map_err(|e| e.to_string())
    }
    #[cfg(not(desktop))]
    {
        let _ = reveal;
        Err("当前环境不支持直接打开".to_string())
    }
}

#[tauri::command]
async fn share_download(
    task_id: String,
    index: usize,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let snap = state.snapshot();
    let doc = snap
        .items
        .iter()
        .find(|item| item.id == task_id)
        .and_then(|item| item.documents.get(index))
        .ok_or("未找到该文书")?
        .clone();

    if doc.saved_path.is_empty() {
        return Err("文书尚未保存成功，请先点击「重试保存」".to_string());
    }

    if state.storage().is_available() {
        let mime = guess_mime_type(&doc.file_name);
        let res = state
            .storage()
            .share_document(&doc.saved_path, Some(mime), Some(&doc.file_name))
            .await;
        if let Err(ref msg) = res {
            if msg.starts_with("MISSING:") {
                let sink = TauriProgressSink::new(app);
                state.refresh_document_statuses(sink).await;
            }
        }
        return res;
    }

    #[cfg(desktop)]
    {
        use tauri_plugin_opener::OpenerExt;
        if !std::path::Path::new(&doc.saved_path).exists() {
            let sink = TauriProgressSink::new(app);
            state.refresh_document_statuses(sink).await;
            return Err("MISSING:文件已被移动或删除".to_string());
        }
        app.opener()
            .reveal_item_in_dir(&doc.saved_path)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(desktop))]
    {
        Err("当前环境不支持系统分享".to_string())
    }
}

#[tauri::command]
async fn retry_save_document(
    task_id: String,
    index: usize,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let sink = TauriProgressSink::new(app);
    state.retry_save_document(task_id, index, sink).await
}

#[tauri::command]
async fn refresh_document_statuses(
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let sink = TauriProgressSink::new(app);
    state.refresh_document_statuses(sink).await;
    Ok(())
}

#[tauri::command]
async fn remove_downloaded_file(
    task_id: String,
    index: usize,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let sink = TauriProgressSink::new(app);
    state.remove_downloaded_file(task_id, index, sink).await
}

#[tauri::command]
fn update_item_case_number(
    task_id: String,
    case_number: Option<String>,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let sink = TauriProgressSink::new(app);
    state.update_item_case_number(&task_id, case_number, &sink)
}

#[tauri::command]
async fn pick_android_directory(state: State<'_, QueueManager>) -> Result<AppSettings, String> {
    let picked = state.storage().pick_directory().await?;
    if picked.cancelled {
        return Ok(state.get_settings());
    }
    let tree_uri = picked.tree_uri.ok_or("未返回文件夹授权地址")?;
    let label = picked
        .display_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "已选择文件夹".to_string());
    let mut settings = state.get_settings();
    settings.android_storage_mode = AndroidStorageMode::SafTree;
    settings.android_tree_uri = Some(tree_uri);
    settings.android_tree_label = Some(label.clone());
    settings.output_dir = label;
    state.update_settings(settings)
}

#[tauri::command]
fn use_android_downloads_dir(state: State<'_, QueueManager>) -> Result<AppSettings, String> {
    let mut settings = state.get_settings();
    settings.android_storage_mode = AndroidStorageMode::MediaStoreDownloads;
    settings.output_dir = "下载/DadaDuck送达文书".to_string();
    state.update_settings(settings)
}

#[tauri::command]
async fn read_clipboard_text(state: State<'_, QueueManager>) -> Result<String, String> {
    state.storage().read_clipboard_text().await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn preview_archive_path(
    subdir_mode: SubdirMode,
    rename_template: RenameTemplate,
    custom_template: String,
    prefix_index: bool,
    output_dir: Option<String>,
    android_storage_mode: Option<AndroidStorageMode>,
    android_tree_label: Option<String>,
    state: State<'_, QueueManager>,
) -> String {
    let settings = state.get_settings();
    let effective_mode = android_storage_mode.unwrap_or(settings.android_storage_mode);
    let effective_tree_label = android_tree_label.or(settings.android_tree_label);
    let effective_output_dir = output_dir
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(settings.output_dir);
    let base_label = if cfg!(target_os = "android") {
        match effective_mode {
            AndroidStorageMode::MediaStoreDownloads => "下载/DadaDuck送达文书",
            AndroidStorageMode::SafTree => effective_tree_label
                .as_deref()
                .unwrap_or("已选择文件夹"),
        }
    } else {
        effective_output_dir.as_str()
    };
    renamer::preview_sample_archive_path(
        base_label,
        &subdir_mode,
        &rename_template,
        &custom_template,
        prefix_index,
    )
}

fn collect_legacy_roots(
    app: &AppHandle,
    settings: &AppSettings,
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    let mut roots = Vec::new();
    #[allow(unused_mut)]
    let mut exclude_dirs = Vec::new();
    if let Ok(app_data) = app.path().app_data_dir() {
        roots.push(app_data.join("DadaDuck送达文书"));
        roots.push(app_data);
    }
    if let Ok(doc_dir) = app.path().document_dir() {
        roots.push(doc_dir.join("DadaDuck送达文书"));
    }
    if let Ok(dl_dir) = app.path().download_dir() {
        let p = dl_dir.join("DadaDuck送达文书");
        if cfg!(target_os = "android") || p.to_string_lossy() != settings.output_dir {
            roots.push(p);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        if !settings.output_dir.trim().is_empty() {
            exclude_dirs.push(std::path::PathBuf::from(&settings.output_dir));
        }
    }
    #[cfg(target_os = "android")]
    {
        roots.push(std::path::PathBuf::from(
            "/data/user/0/com.muxiaoxiii.dadaduck/DadaDuck送达文书",
        ));
        roots.push(std::path::PathBuf::from(
            "/data/data/com.muxiaoxiii.dadaduck/DadaDuck送达文书",
        ));
    }
    (roots, exclude_dirs)
}

fn validate_legacy_path(
    raw_path: &str,
    app: &AppHandle,
    state: &QueueManager,
) -> Result<std::path::PathBuf, String> {
    let candidate =
        std::fs::canonicalize(raw_path).map_err(|_| "文件不存在或无法访问".to_string())?;
    if !candidate.is_file() {
        return Err("目标不是有效文件".to_string());
    }
    let config_canon = std::fs::canonicalize(state.config_dir())
        .unwrap_or_else(|_| state.config_dir().to_path_buf());
    let staging_dir = store::staging_dir_path(state.config_dir());
    let staging_canon = std::fs::canonicalize(&staging_dir).unwrap_or(staging_dir);
    if candidate.starts_with(&staging_canon) {
        return Err("不允许访问临时暂存目录文件".to_string());
    }
    if candidate.parent() == Some(config_canon.as_path()) {
        if let Some(name) = candidate.file_name().and_then(|n| n.to_str()) {
            if matches!(
                name,
                "settings.json" | "queue_history.json" | "exported_legacy.json"
            ) {
                return Err("不允许访问应用配置文件".to_string());
            }
        }
    }

    let settings = state.get_settings();
    let (roots, exclude_dirs) = collect_legacy_roots(app, &settings);
    let in_allowed_root = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|rc| candidate.starts_with(&rc));
    let in_excluded = exclude_dirs
        .iter()
        .filter_map(|d| std::fs::canonicalize(d).ok())
        .any(|ec| candidate.starts_with(&ec));

    if !in_allowed_root || in_excluded {
        return Err("仅允许访问旧归档扫描范围内的文书文件".to_string());
    }
    Ok(candidate)
}

#[tauri::command]
fn scan_legacy_documents(
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Vec<LegacyDocumentEntry> {
    let settings = state.get_settings();
    let (roots, exclude_dirs) = collect_legacy_roots(&app, &settings);
    let snap = state.snapshot();
    store::scan_legacy_documents(state.config_dir(), &roots, &exclude_dirs, &snap.items)
}

#[tauri::command]
async fn open_legacy_document(
    path: String,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<(), String> {
    let valid_path = validate_legacy_path(&path, &app, &state)?;
    let valid_str = valid_path.to_string_lossy().to_string();
    if state.storage().is_available() {
        return state
            .storage()
            .open_document(&valid_str, Some(guess_mime_type(&valid_str)))
            .await;
    }
    #[cfg(desktop)]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_path(&valid_str, None::<&str>).map_err(|e| e.to_string())
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("当前环境不支持打开旧目录文件".to_string())
    }
}

#[tauri::command]
async fn export_legacy_documents(
    items: Vec<LegacyExportInput>,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<LegacyExportResult, String> {
    let mut allowed_inputs = Vec::new();
    let mut rejected_skipped = 0usize;
    let mut rejected_errors = Vec::new();

    for mut item in items {
        match validate_legacy_path(&item.path, &app, &state) {
            Ok(valid_path) => {
                item.path = valid_path.to_string_lossy().to_string();
                allowed_inputs.push(item);
            }
            Err(err) => {
                rejected_skipped += 1;
                rejected_errors.push(format!("跳过无效路径（{}）：{}", item.path, err));
            }
        }
    }

    let mut result = state.export_legacy_documents(allowed_inputs).await;
    result.skipped += rejected_skipped;
    result.errors.extend(rejected_errors);
    Ok(result)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportTaskDocsResult {
    imported: usize,
    errors: Vec<String>,
}

#[tauri::command]
fn supports_embedded_fallback() -> bool {
    browser_fallback::supports_embedded_fallback()
}

#[tauri::command]
fn get_fallback_session(state: State<'_, FallbackManager>) -> Option<FallbackSessionSnapshot> {
    state.snapshot()
}

#[tauri::command]
fn open_fallback_browser(
    task_id: String,
    app: AppHandle,
    queue: State<'_, QueueManager>,
    fallback: State<'_, FallbackManager>,
) -> Result<FallbackSessionSnapshot, String> {
    browser_fallback::open_fallback_window(&app, &queue, &fallback, &task_id)
}

#[tauri::command]
fn fill_fallback_code(
    code: String,
    app: AppHandle,
    fallback: State<'_, FallbackManager>,
) -> Result<(), String> {
    browser_fallback::fill_fallback_code(&app, &fallback, &code)
}

#[tauri::command]
fn scan_fallback_documents(
    app: AppHandle,
    fallback: State<'_, FallbackManager>,
) -> Result<(), String> {
    browser_fallback::scan_fallback_documents(&app, &fallback)
}

#[tauri::command]
fn focus_fallback_window(
    app: AppHandle,
    fallback: State<'_, FallbackManager>,
) -> Result<(), String> {
    browser_fallback::focus_fallback_window(&app, &fallback)
}

#[tauri::command]
fn close_fallback_window(
    app: AppHandle,
    fallback: State<'_, FallbackManager>,
) -> Result<(), String> {
    browser_fallback::close_fallback_window(&app, &fallback)
}

#[tauri::command]
async fn import_task_documents(
    task_id: String,
    paths: Vec<String>,
    app: AppHandle,
    state: State<'_, QueueManager>,
) -> Result<ImportTaskDocsResult, String> {
    let sink = TauriProgressSink::new(app);
    let mut imported = 0usize;
    let mut errors = Vec::new();
    for raw_path in paths {
        let path = std::path::PathBuf::from(&raw_path);
        let suggested = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        match state
            .ingest_captured_file(&task_id, &path, &suggested, None, false, &sink)
            .await
        {
            Ok(_) => imported += 1,
            Err(err) => errors.push(format!("{}：{}", suggested, err)),
        }
    }
    Ok(ImportTaskDocsResult { imported, errors })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(android_storage::init_plugin());
    #[cfg(desktop)]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle);

    builder
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("dadaduck"));
            let storage = app.state::<AndroidStorageHandle>().inner().clone();

            #[cfg(target_os = "android")]
            let default_output = "下载/DadaDuck送达文书".to_string();
            #[cfg(not(target_os = "android"))]
            let default_output = {
                let base_output_dir = app
                    .path()
                    .download_dir()
                    .or_else(|_| app.path().document_dir())
                    .unwrap_or_else(|_| std::env::temp_dir());
                base_output_dir
                    .join("DadaDuck送达文书")
                    .to_string_lossy()
                    .to_string()
            };

            #[allow(unused_mut)]
            let mut settings = store::load_settings(&config_dir, default_output.clone());
            #[cfg(target_os = "android")]
            if settings.android_storage_mode == AndroidStorageMode::MediaStoreDownloads
                && (settings.output_dir.starts_with("/data/")
                    || settings
                        .output_dir
                        .starts_with("/storage/emulated/0/Android/data/"))
            {
                settings.output_dir = default_output;
                let _ = store::save_settings(&config_dir, &settings);
            }

            store::migrate_sensitive_storage(&config_dir, &settings)
                .map_err(|err| std::io::Error::other(format!("无法更新本机数据存储：{err}")))?;
            let fallback_mgr = FallbackManager::new(&config_dir);
            let manager = QueueManager::with_storage(settings, config_dir, storage);
            let mgr_bg = manager.clone();
            let app_bg = app.handle().clone();
            app.manage(fallback_mgr);
            app.manage(manager);

            tauri::async_runtime::spawn(async move {
                let sink = TauriProgressSink::new(app_bg);
                mgr_bg.refresh_document_statuses(sink).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_output_dir,
            open_download,
            share_download,
            retry_save_document,
            refresh_document_statuses,
            remove_downloaded_file,
            update_item_case_number,
            pick_android_directory,
            use_android_downloads_dir,
            read_clipboard_text,
            preview_archive_path,
            scan_legacy_documents,
            open_legacy_document,
            export_legacy_documents,
            supports_embedded_fallback,
            get_fallback_session,
            open_fallback_browser,
            fill_fallback_code,
            scan_fallback_documents,
            focus_fallback_window,
            close_fallback_window,
            import_task_documents,
            get_settings,
            archive_demo_document,
            copy_sensitive_text,
            update_settings,
            save_lawyer_profile,
            batch_save_lawyer_profiles,
            delete_lawyer_profile,
            get_queue_snapshot,
            parse_and_enqueue,
            start_queue,
            respond_to_receipt,
            pause_queue,
            resume_queue,
            cancel_queue,
            retry_failed,
            remove_queue_item,
            clear_finished,
            clear_local_data,
            submit_identity
        ])
        .run(tauri::generate_context!())
        .expect("error while running DadaDuck");
}
