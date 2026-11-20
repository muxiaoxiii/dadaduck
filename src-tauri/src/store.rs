use crate::models::{
    AppSettings, ItemStatus, LawyerProfile, LegacyDocumentEntry, Platform, QueueItem, ReceiptStatus,
};
use crate::parser::{extract_case_number, extract_court_name};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

pub fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join("queue_history.json")
}

pub fn exported_legacy_path(config_dir: &Path) -> PathBuf {
    config_dir.join("exported_legacy.json")
}

pub fn staging_dir_path(config_dir: &Path) -> PathBuf {
    config_dir.join("pending_staging")
}

pub fn load_queue_history(config_dir: &Path) -> Vec<QueueItem> {
    read_queue_history(config_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| !item.documents.is_empty())
        .map(|mut item| {
            redact_history_item(&mut item);
            item
        })
        .collect()
}

fn read_queue_history(config_dir: &Path) -> Result<Vec<QueueItem>, String> {
    let raw = match std::fs::read_to_string(history_path(config_dir)) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.to_string()),
    };
    serde_json::from_str(&raw).map_err(|err| format!("无法读取旧版队列记录：{err}"))
}

fn receipt_attempt_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("receipt_attempts")
}

fn receipt_attempt_path(config_dir: &Path, key: &str) -> PathBuf {
    receipt_attempt_dir(config_dir).join(format!("{:x}.json", Sha256::digest(key.as_bytes())))
}

fn receipt_attempt_key(item: &QueueItem) -> String {
    match (&item.params.qdbh, &item.params.sdbh) {
        (Some(channel), Some(delivery)) => format!("{}:{channel}{delivery}", channel.len()),
        _ => item.fingerprint.clone(),
    }
}

pub fn read_receipt_attempt(
    config_dir: &Path,
    item: &QueueItem,
) -> Result<Option<ReceiptStatus>, String> {
    let key = receipt_attempt_key(item);
    let status = read_receipt_status_at(receipt_attempt_path(config_dir, &key))?;
    if status.is_some() || key == item.fingerprint {
        return Ok(status);
    }
    read_receipt_status_at(receipt_attempt_path(config_dir, &item.fingerprint))
}

fn read_receipt_status_at(path: PathBuf) -> Result<Option<ReceiptStatus>, String> {
    match std::fs::read(path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub fn reserve_receipt_attempt(
    config_dir: &Path,
    item: &QueueItem,
) -> Result<Option<ReceiptStatus>, String> {
    let entry = write_receipt_temp(config_dir, ReceiptStatus::Unknown)?;
    match entry.persist_noclobber(receipt_attempt_path(config_dir, &receipt_attempt_key(item))) {
        Ok(_) => Ok(None),
        Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(Some(
            read_receipt_attempt(config_dir, item)?.unwrap_or(ReceiptStatus::Unknown),
        )),
        Err(err) => Err(err.error.to_string()),
    }
}

pub fn finish_receipt_attempt(
    config_dir: &Path,
    item: &QueueItem,
    status: ReceiptStatus,
) -> Result<(), String> {
    write_receipt_temp(config_dir, status)?
        .persist(receipt_attempt_path(config_dir, &receipt_attempt_key(item)))
        .map(|_| ())
        .map_err(|err| err.error.to_string())
}

fn write_receipt_temp(
    config_dir: &Path,
    status: ReceiptStatus,
) -> Result<tempfile::NamedTempFile, String> {
    let dir = receipt_attempt_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let mut entry = tempfile::NamedTempFile::new_in(&dir).map_err(|err| err.to_string())?;
    serde_json::to_writer(entry.as_file_mut(), &status).map_err(|err| err.to_string())?;
    entry.as_file().sync_all().map_err(|err| err.to_string())?;
    Ok(entry)
}

pub fn clear_receipt_attempts(config_dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(receipt_attempt_dir(config_dir)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

pub(crate) fn download_url_key(url: &str) -> String {
    format!("url-sha256:{:x}", Sha256::digest(url.as_bytes()))
}

fn redact_history_item(item: &mut QueueItem) {
    item.raw_text.clear();
    item.url.clear();
    item.params = Default::default();
    item.sms_code = None;
    item.parties.clear();
    item.bytes_received = 0;
    item.total_bytes = None;
    item.speed_bytes_per_sec = 0;
    item.current_doc_name = None;
    item.current_target_file_name = None;
    if item.status == ItemStatus::Completed {
        item.stage_label = "已归档".into();
        item.status_message = "本机归档记录".into();
    } else {
        if !matches!(item.status, ItemStatus::Failed | ItemStatus::Partial) {
            item.status = ItemStatus::Failed;
        }
        item.stage_label = "已结束".into();
        item.status_message = "取件凭证未保留；如需继续下载，请重新导入原始短信。".into();
    }
    for doc in &mut item.documents {
        if doc.display_location.is_empty() {
            doc.display_location = doc.saved_path.clone();
        }
        if let Some(url) = doc
            .source_key
            .as_deref()
            .and_then(|key| key.strip_prefix("url:"))
        {
            doc.source_key = Some(download_url_key(url));
        }
        if doc.save_error.is_some() {
            doc.save_error = Some("此文件尚未保存，可尝试重新保存。".into());
        }
    }
    for failure in &mut item.failures {
        if let Some(url) = failure
            .source_key
            .as_deref()
            .and_then(|key| key.strip_prefix("url:"))
        {
            failure.source_key = Some(download_url_key(url));
        }
        failure.reason = "该文书未完成归档".into();
    }
}

pub fn save_queue_history(config_dir: &Path, items: &[QueueItem]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let archive_records: Vec<_> = items
        .iter()
        .filter(|item| !item.documents.is_empty())
        .cloned()
        .map(|mut item| {
            redact_history_item(&mut item);
            item
        })
        .collect();
    let path = history_path(config_dir);
    let mut tmp = tempfile::NamedTempFile::new_in(config_dir).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(tmp.as_file_mut(), &archive_records).map_err(|e| e.to_string())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

pub fn migrate_sensitive_storage(config_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    if settings_path(config_dir).exists() {
        save_settings(config_dir, settings)?;
    }
    migrate_queue_history(config_dir, config_dir)?;

    if let Some(legacy_dir) = config_dir
        .parent()
        .map(|parent| parent.join("com.dadaduck.app"))
        .filter(|legacy_dir| legacy_dir != config_dir)
    {
        let path = settings_path(&legacy_dir);
        if path.exists() {
            let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
            let legacy = serde_json::from_str::<AppSettings>(&raw)
                .map_err(|err| format!("无法迁移旧版设置：{err}"))?;
            save_settings(&legacy_dir, &legacy)?;
        }
        migrate_queue_history(&legacy_dir, config_dir)?;
    }
    Ok(())
}

fn migrate_queue_history(history_dir: &Path, config_dir: &Path) -> Result<(), String> {
    if !history_path(history_dir).exists() {
        return Ok(());
    }
    let items = read_queue_history(history_dir)?;
    for item in &items {
        if item.platform == Platform::Zxfw
            && item.receipt_status != ReceiptStatus::NotRequested
            && read_receipt_attempt(config_dir, item)?.is_none()
            && reserve_receipt_attempt(config_dir, item)?.is_none()
        {
            finish_receipt_attempt(config_dir, item, item.receipt_status)?;
        }
    }
    save_queue_history(history_dir, &items)
}

pub fn load_exported_legacy_paths(config_dir: &Path) -> HashSet<String> {
    let path = exported_legacy_path(config_dir);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    serde_json::from_str::<HashSet<String>>(&raw).unwrap_or_default()
}

pub fn save_exported_legacy_paths(config_dir: &Path, set: &HashSet<String>) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = exported_legacy_path(config_dir);
    let mut tmp = tempfile::NamedTempFile::new_in(config_dir).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(tmp.as_file_mut(), set).map_err(|e| e.to_string())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

struct LegacyScanCtx<'a> {
    base_root: &'a Path,
    config_canon: &'a Path,
    staging_canon: &'a Path,
    excluded_canon: &'a [PathBuf],
    exported_set: &'a HashSet<String>,
    history_items: &'a [QueueItem],
}

pub fn scan_legacy_documents(
    config_dir: &Path,
    candidate_roots: &[PathBuf],
    exclude_dirs: &[PathBuf],
    history_items: &[QueueItem],
) -> Vec<LegacyDocumentEntry> {
    let exported_set = load_exported_legacy_paths(config_dir);
    let config_canon = std::fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let staging_dir = staging_dir_path(config_dir);
    let staging_canon = std::fs::canonicalize(&staging_dir).unwrap_or(staging_dir);
    let excluded_canon: Vec<PathBuf> = exclude_dirs
        .iter()
        .map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.clone()))
        .collect();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut results = Vec::new();

    for root in candidate_roots {
        if !root.exists() {
            continue;
        }
        let Ok(root_canon) = std::fs::canonicalize(root) else {
            continue;
        };
        let ctx = LegacyScanCtx {
            base_root: &root_canon,
            config_canon: &config_canon,
            staging_canon: &staging_canon,
            excluded_canon: &excluded_canon,
            exported_set: &exported_set,
            history_items,
        };
        collect_legacy_recursive(&ctx, &root_canon, &mut seen_paths, &mut results, 0);
    }

    results.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    results
}

fn collect_legacy_recursive(
    ctx: &LegacyScanCtx<'_>,
    dir: &Path,
    seen_paths: &mut HashSet<PathBuf>,
    out: &mut Vec<LegacyDocumentEntry>,
    depth: usize,
) {
    if depth > 6 {
        return;
    }
    let dir_canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if dir_canon.starts_with(ctx.staging_canon)
        || ctx.excluded_canon.iter().any(|ex| dir_canon.starts_with(ex))
    {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_legacy_recursive(ctx, &path, seen_paths, out, depth + 1);
            continue;
        }
        if !meta.is_file() || meta.len() == 0 {
            continue;
        }
        let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if canon.starts_with(ctx.staging_canon)
            || ctx.excluded_canon.iter().any(|ex| canon.starts_with(ex))
        {
            continue;
        }
        if !seen_paths.insert(canon.clone()) {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if file_name.starts_with('.') || file_name.ends_with(".part") || file_name.ends_with(".tmp") {
            continue;
        }
        if canon.parent() == Some(ctx.config_canon)
            && matches!(
                file_name.as_str(),
                "settings.json" | "queue_history.json" | "exported_legacy.json"
            )
        {
            continue;
        }
        let lower_name = file_name.to_ascii_lowercase();
        let is_doc_ext = [
            ".pdf", ".ofd", ".doc", ".docx", ".zip", ".rar", ".wps", ".xls", ".xlsx", ".rtf", ".txt",
        ]
        .iter()
        .any(|ext| lower_name.ends_with(ext));
        let under_dadaduck_archive = path
            .components()
            .any(|c| c.as_os_str().to_string_lossy() == "DadaDuck送达文书");
        if !is_doc_ext && (!under_dadaduck_archive || lower_name.ends_with(".json")) {
            continue;
        }

        let path_str = canon.to_string_lossy().to_string();
        let rel_dir = canon
            .parent()
            .and_then(|p| p.strip_prefix(ctx.base_root).ok())
            .or_else(|| path.parent().and_then(|p| p.strip_prefix(ctx.base_root).ok()))
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let history_match = ctx.history_items.iter().find(|it| {
            it.documents
                .iter()
                .any(|d| !d.saved_path.is_empty() && (d.saved_path == path_str || d.saved_path == path.to_string_lossy()))
        });

        let inferred_case_number = history_match
            .and_then(|it| it.case_number.clone())
            .or_else(|| extract_case_number(&rel_dir))
            .or_else(|| extract_case_number(&file_name));
        let inferred_court_name = history_match
            .and_then(|it| it.court_name.clone())
            .or_else(|| extract_court_name(&rel_dir))
            .or_else(|| extract_court_name(&file_name));

        let exported_before =
            ctx.exported_set.contains(&path_str) || ctx.exported_set.contains(path.to_string_lossy().as_ref());

        out.push(LegacyDocumentEntry {
            needs_case_confirmation: inferred_case_number.is_none(),
            path: path_str,
            file_name,
            relative_dir: if rel_dir.is_empty() {
                "根目录".to_string()
            } else {
                rel_dir
            },
            size_bytes: meta.len(),
            inferred_case_number,
            inferred_court_name,
            exported_before,
        });
    }
}

pub fn load_settings(config_dir: &Path, default_output_dir: String) -> AppSettings {
    let path = settings_path(config_dir);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<AppSettings>(&raw) {
            if parsed.output_dir.trim().is_empty() {
                parsed.output_dir = default_output_dir;
            }
            return parsed;
        }
    }

    // One-time migration from legacy com.dadaduck.app config directory when new config doesn't exist yet.
    if !path.exists() {
        if let Some(parent) = config_dir.parent() {
            let legacy_path = parent.join("com.dadaduck.app").join("settings.json");
            if legacy_path != path {
                if let Ok(raw) = std::fs::read_to_string(&legacy_path) {
                    if let Ok(mut parsed) = serde_json::from_str::<AppSettings>(&raw) {
                        if parsed.output_dir.trim().is_empty() {
                            parsed.output_dir = default_output_dir;
                        }
                        let _ = save_settings(config_dir, &parsed);
                        return parsed;
                    }
                }
            }
        }
    }

    AppSettings::default_with_dir(default_output_dir)
}

pub fn save_settings(config_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = settings_path(config_dir);
    let mut tmp = tempfile::NamedTempFile::new_in(config_dir).map_err(|e| e.to_string())?;
    let mut persistent = serde_json::to_value(settings).map_err(|err| err.to_string())?;
    if let Some(object) = persistent.as_object_mut() {
        object.remove("profiles");
    }
    serde_json::to_writer_pretty(tmp.as_file_mut(), &persistent).map_err(|err| err.to_string())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

pub fn upsert_profile(
    settings: &mut AppSettings,
    name: String,
    mobile_last4: String,
    id_last4: Option<String>,
) -> Result<LawyerProfile, String> {
    let trimmed_name = name.trim().to_string();
    let tail = mobile_last4.trim().to_string();
    if trimmed_name.is_empty() {
        return Err("姓名不能为空".to_string());
    }
    if tail.len() != 4 || !tail.chars().all(|c| c.is_ascii_digit()) {
        return Err("手机尾号必须为 4 位数字".to_string());
    }
    let clean_id = id_last4
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty());
    if let Some(ref id4) = clean_id {
        if id4.len() != 4 || !id4.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("证件后四位必须为 4 位数字或字母".to_string());
        }
    }

    if let Some(existing) = settings.profiles.iter_mut().find(|p| p.name == trimmed_name) {
        existing.mobile_last4 = tail;
        existing.id_last4 = clean_id;
        return Ok(existing.clone());
    }

    let profile = LawyerProfile {
        id: Uuid::new_v4().to_string(),
        name: trimmed_name,
        mobile_last4: tail,
        id_last4: clean_id,
    };
    settings.profiles.push(profile.clone());
    Ok(profile)
}

pub fn batch_upsert_profiles(settings: &mut AppSettings, raw_text: &str) -> Result<usize, String> {
    let mut imported = 0usize;
    for (idx, line) in raw_text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed
            .split(|c: char| c == ',' || c == '，' || c == '\t' || c.is_whitespace())
            .filter(|s| !s.trim().is_empty())
            .collect();
        if parts.len() < 2 {
            return Err(format!("第 {} 行格式不正确，请使用「姓名 手机后四位」", idx + 1));
        }
        let name = parts[0].to_string();
        let mobile_last4 = parts[1].to_string();
        let id_last4 = parts.get(2).map(|s| s.to_string());
        upsert_profile(settings, name, mobile_last4, id_last4)
            .map_err(|e| format!("第 {} 行（{}）：{}", idx + 1, parts[0], e))?;
        imported += 1;
    }
    if imported == 0 {
        return Err("没有检测到有效尾号行".to_string());
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn migrates_legacy_settings_only_when_new_config_missing() {
        let root = TempDir::new().unwrap();
        let legacy_dir = root.path().join("com.dadaduck.app");
        let new_dir = root.path().join("com.muxiaoxiii.dadaduck");

        let mut legacy_settings = AppSettings::default_with_dir("/tmp/legacy".to_string());
        upsert_profile(&mut legacy_settings, "张律师".to_string(), "1234".to_string(), None).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let mut old_json = serde_json::to_value(&legacy_settings).unwrap();
        old_json["confirmReceipt"] = serde_json::json!(true);
        std::fs::write(
            settings_path(&legacy_dir),
            serde_json::to_vec(&old_json).unwrap(),
        )
        .unwrap();

        let loaded = load_settings(&new_dir, "/tmp/default".to_string());
        assert_eq!(loaded.output_dir, "/tmp/legacy");
        assert_eq!(loaded.profiles.len(), 1);
        migrate_sensitive_storage(&new_dir, &loaded).unwrap();
        for dir in [&legacy_dir, &new_dir] {
            let on_disk = std::fs::read_to_string(settings_path(dir)).unwrap();
            assert!(!on_disk.contains("profiles"));
            assert!(!on_disk.contains("confirmReceipt"));
            assert!(!on_disk.contains("张律师"));
        }

        // Existing new config must never be overwritten by legacy config.
        let mut custom_new = AppSettings::default_with_dir("/tmp/new-dir".to_string());
        custom_new.profiles.clear();
        save_settings(&new_dir, &custom_new).unwrap();
        let reloaded = load_settings(&new_dir, "/tmp/default".to_string());
        assert_eq!(reloaded.output_dir, "/tmp/new-dir");
        assert!(reloaded.profiles.is_empty());
    }

    #[test]
    fn scan_legacy_documents_excludes_staging_config_and_active_output_dirs() {
        let root = TempDir::new().unwrap();
        let config_dir = root.path().join("cfg");
        let staging_dir = staging_dir_path(&config_dir);
        let active_output = root.path().join("active_output");
        let legacy_dir = root.path().join("legacy_docs");

        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&staging_dir).unwrap();
        std::fs::create_dir_all(&active_output).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();

        save_settings(&config_dir, &AppSettings::default_with_dir(active_output.to_string_lossy().to_string())).unwrap();
        std::fs::write(staging_dir.join("temp.pdf"), b"%PDF-1.4\nstaging").unwrap();
        std::fs::write(active_output.join("current.pdf"), b"%PDF-1.4\ncurrent").unwrap();
        std::fs::write(legacy_dir.join("（2026）粤03民初1号_传票.pdf"), b"%PDF-1.4\nlegacy").unwrap();

        let entries = scan_legacy_documents(
            &config_dir,
            &[config_dir.clone(), active_output.clone(), legacy_dir.clone()],
            &[active_output],
            &[],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name, "（2026）粤03民初1号_传票.pdf");
        assert_eq!(
            entries[0].inferred_case_number.as_deref(),
            Some("（2026）粤03民初1号")
        );
    }

    #[test]
    fn upsert_profile_uppercases_id_last4() {
        let mut settings = AppSettings::default_with_dir("/tmp".to_string());
        let p = upsert_profile(
            &mut settings,
            "王律师".to_string(),
            "8888".to_string(),
            Some("123x".to_string()),
        )
        .unwrap();
        assert_eq!(p.id_last4.as_deref(), Some("123X"));
    }
}
