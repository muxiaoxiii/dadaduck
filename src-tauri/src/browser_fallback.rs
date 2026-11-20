use crate::queue::QueueManager;
use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use std::collections::{HashMap, VecDeque};
use std::path::Path;
#[cfg(desktop)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg(desktop)]
use crate::queue::TauriProgressSink;
#[cfg(desktop)]
use tauri::{
    webview::NewWindowResponse, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
#[cfg(desktop)]
use url::Url;
#[cfg(desktop)]
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackSessionSnapshot {
    pub task_id: String,
    pub label: String,
    pub url: String,
    pub case_number: Option<String>,
    pub court_name: Option<String>,
    pub platform_label: String,
    pub sms_code: Option<String>,
    pub captured_count: usize,
    pub last_message: Option<String>,
    pub last_error: Option<String>,
}

#[cfg(desktop)]
#[derive(Clone)]
struct PendingWebDownload {
    dest_path: PathBuf,
    suggested_name: String,
}

#[derive(Default)]
struct FallbackState {
    session: Option<FallbackSessionSnapshot>,
    #[cfg(desktop)]
    pending: HashMap<String, VecDeque<PendingWebDownload>>,
}

#[derive(Clone, Default)]
pub struct FallbackManager {
    inner: Arc<Mutex<FallbackState>>,
    #[cfg(desktop)]
    staging_dir: PathBuf,
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl FallbackManager {
    pub fn new(config_dir: &Path) -> Self {
        #[cfg(desktop)]
        let staging_dir = {
            let dir = config_dir.join("fallback_staging");
            if dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            let _ = std::fs::remove_file(p);
                        }
                    }
                }
            }
            dir
        };
        #[cfg(not(desktop))]
        let _ = config_dir;
        Self {
            inner: Arc::new(Mutex::new(FallbackState::default())),
            #[cfg(desktop)]
            staging_dir,
        }
    }

    pub fn snapshot(&self) -> Option<FallbackSessionSnapshot> {
        lock_unpoisoned(&self.inner).session.clone()
    }

    #[cfg(desktop)]
    pub fn staging_dir(&self) -> &Path {
        &self.staging_dir
    }
}

pub fn supports_embedded_fallback() -> bool {
    cfg!(desktop)
}

#[cfg(desktop)]
fn emit_session(app: &AppHandle, manager: &FallbackManager) {
    let snap = manager.snapshot();
    let _ = app.emit_to("main", "fallback-updated", snap);
}

#[cfg(desktop)]
fn sanitize_hint_name(raw: &str) -> String {
    let base = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == ' ' || c == '.');
    if trimmed.is_empty() {
        "document".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(desktop)]
fn is_task_origin(url: &Url, task_url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.origin() == task_url.origin()
}

#[cfg(desktop)]
fn guarded_page_script(task_url: &Url, script: &str) -> String {
    let origin = serde_json::to_string(&task_url.origin().ascii_serialization())
        .expect("serializing an origin cannot fail");
    format!("if (window === window.top && location.origin === {origin}) {{ {script} }}")
}

#[cfg(desktop)]
fn build_assist_script(task_url: &Url) -> String {
    guarded_page_script(
        task_url,
        r#"(function() {
  if (window.__dadaduckAssist) return;
  const CODE_HINT_RE = /(验证码|校验码|提取码|查收码|送达码|查看码|查阅码|访问码|查看密码|图形码|动态码|captcha|verify|vcode|yzm|code)/i;
  const PHONE_HINT_RE = /(手机|电话|mobile|phone|tel)/i;

  function collectDocs() {
    const docs = [document];
    const frames = document.querySelectorAll('iframe');
    for (const f of frames) {
      try {
        if (f.contentDocument) docs.push(f.contentDocument);
      } catch (_) {}
    }
    return docs;
  }

  function isVisible(el) {
    if (!el || el.disabled || el.readOnly) return false;
    const style = window.getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 12 && rect.height > 10;
  }

  function inputContextText(doc, el) {
    const parts = [
      el.placeholder || '',
      el.name || '',
      el.id || '',
      el.getAttribute('aria-label') || '',
      el.title || ''
    ];
    if (el.id) {
      try {
        const lbl = doc.querySelector('label[for="' + CSS.escape(el.id) + '"]');
        if (lbl) parts.push(lbl.textContent || '');
      } catch (_) {}
    }
    const parent = el.closest('label, .form-item, .el-form-item, .ant-form-item, .field, .row, tr, div');
    if (parent && parent.textContent) {
      parts.push(parent.textContent.slice(0, 120));
    }
    return parts.join(' ');
  }

  function findCodeInput() {
    let best = null;
    let bestScore = 0;
    const visibleTextInputs = [];
    for (const doc of collectDocs()) {
      const inputs = doc.querySelectorAll('input');
      for (const el of inputs) {
        const type = (el.type || 'text').toLowerCase();
        if (['hidden', 'submit', 'button', 'checkbox', 'radio', 'file', 'reset', 'image'].includes(type)) {
          continue;
        }
        if (!isVisible(el)) continue;
        visibleTextInputs.push({ doc, el, type });
      }
    }
    for (const item of visibleTextInputs) {
      const ctx = inputContextText(item.doc, item.el);
      let score = 0;
      if (CODE_HINT_RE.test(ctx)) score += 10;
      const maxLen = item.el.maxLength;
      if (maxLen >= 4 && maxLen <= 8) score += 4;
      if (maxLen === 11 || PHONE_HINT_RE.test(ctx)) score -= 8;
      if (item.type === 'password' && !/(查看密码|提取码|查收码)/.test(ctx)) score -= 4;
      if (visibleTextInputs.length === 1 && score >= 0) score += 2;
      if (score > bestScore) {
        bestScore = score;
        best = item.el;
      }
    }
    return best;
  }

  function fillCode(rawCode) {
    const code = String(rawCode || '').trim();
    if (!code) return false;
    const el = findCodeInput();
    if (!el) return false;
    const ownerWin = el.ownerDocument && el.ownerDocument.defaultView ? el.ownerDocument.defaultView : window;
    const proto = ownerWin.HTMLInputElement ? ownerWin.HTMLInputElement.prototype : HTMLInputElement.prototype;
    const desc = Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && typeof desc.set === 'function') {
      desc.set.call(el, code);
    } else {
      el.value = code;
    }
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    el.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true }));
    try {
      el.focus();
      el.scrollIntoView({ block: 'center', behavior: 'smooth' });
      const prevOutline = el.style.outline;
      el.style.outline = '2px solid #205541';
      setTimeout(() => { el.style.outline = prevOutline; }, 1500);
    } catch (_) {}
    return true;
  }

  function triggerDownloadUrl(url, fileName) {
    try {
      const a = document.createElement('a');
      a.href = url;
      a.download = fileName || '';
      a.style.display = 'none';
      document.body.appendChild(a);
      a.click();
      setTimeout(() => a.remove(), 1200);
      return true;
    } catch (_) {
      return false;
    }
  }

  function scanDocumentLinks() {
    const DOC_EXT_RE = /\.(pdf|ofd|docx?|xlsx?|pptx?|wps|rtf|zip|rar|7z|jpe?g|png|gif|bmp|webp|tiff?)(\?|$|#)/i;
    const seen = new Set();
    let count = 0;
    for (const doc of collectDocs()) {
      const pageBase = (doc.location ? doc.location.href : location.href).split('#')[0];
      const anchors = doc.querySelectorAll('a[href]');
      for (const a of anchors) {
        const href = a.href;
        if (!href || href.startsWith('javascript:') || seen.has(href)) continue;
        if (!a.hasAttribute('download') && !DOC_EXT_RE.test(href) && href.split('#')[0] === pageBase) continue;
        const text = (a.textContent || '').trim();
        if (a.hasAttribute('download') || DOC_EXT_RE.test(href) || /下载|导出|保存|文书|回证/.test(text)) {
          seen.add(href);
          triggerDownloadUrl(href, a.getAttribute('download') || '');
          count += 1;
          if (count >= 10) return count;
        }
      }
      const embeds = doc.querySelectorAll('iframe[src], embed[src], object[data]');
      for (const node of embeds) {
        const src = node.getAttribute('src') || node.getAttribute('data') || '';
        if (!src || src === 'about:blank' || src.startsWith('javascript:')) continue;
        let abs = src;
        try { abs = new URL(src, doc.baseURI || location.href).href; } catch (_) {}
        if (seen.has(abs)) continue;
        if (abs.startsWith('blob:') || DOC_EXT_RE.test(abs) || /pdf|download|file|ws|doc/i.test(abs)) {
          seen.add(abs);
          triggerDownloadUrl(abs, '');
          count += 1;
          if (count >= 10) return count;
        }
      }
    }
    return count;
  }

  const origOpen = window.open;
  window.open = function(url, target, features) {
    if (typeof url === 'string' && url.trim()) {
      const trimmed = url.trim();
      if (trimmed.startsWith('blob:') || /\.(pdf|ofd|docx?|zip|rar)(\?|$)/i.test(trimmed)) {
        triggerDownloadUrl(trimmed, '');
        return null;
      }
      if (/^https?:/i.test(trimmed)) {
        window.location.href = trimmed;
        return window;
      }
    }
    return origOpen ? origOpen.apply(this, arguments) : null;
  };

  window.__dadaduckAssist = {
    findCodeInput,
    fillCode,
    scanDocumentLinks,
  };

})();"#,
    )
}

#[cfg(desktop)]
pub fn open_fallback_window(
    app: &AppHandle,
    queue: &QueueManager,
    fallback_mgr: &FallbackManager,
    task_id: &str,
) -> Result<FallbackSessionSnapshot, String> {
    let snap = queue.snapshot();
    let item = snap
        .items
        .into_iter()
        .find(|i| i.id == task_id)
        .ok_or_else(|| "任务不存在".to_string())?;
    let parsed_url =
        url::Url::parse(&item.url).map_err(|_| "该任务缺少有效的网页链接".to_string())?;
    if !is_task_origin(&parsed_url, &parsed_url) || !parsed_url.has_host() {
        return Err("内置取件仅支持 HTTPS 原始送达地址，请使用系统浏览器。".to_string());
    }

    let existing_session = {
        let guard = lock_unpoisoned(&fallback_mgr.inner);
        guard.session.clone().filter(|s| s.task_id == item.id)
    };
    if let Some(session) = existing_session {
        if let Some(existing) = app.get_webview_window(&session.label) {
            let _ = existing.unminimize();
            let _ = existing.set_focus();
            return Ok(session);
        }
    }

    std::fs::create_dir_all(fallback_mgr.staging_dir())
        .map_err(|e| format!("无法初始化保底暂存目录：{}", e))?;

    let prev_label = {
        let mut guard = lock_unpoisoned(&fallback_mgr.inner);
        for (_, mut q) in guard.pending.drain() {
            while let Some(entry) = q.pop_front() {
                let _ = std::fs::remove_file(entry.dest_path);
            }
        }
        guard.session.take().map(|s| s.label)
    };
    if let Some(old_label) = prev_label {
        if let Some(old_win) = app.get_webview_window(&old_label) {
            let _ = old_win.close();
        }
    }

    let label = format!("fallback-{}", Uuid::new_v4().simple());
    let title_case = item
        .case_number
        .clone()
        .or_else(|| item.court_name.clone())
        .unwrap_or_else(|| "法院送达".to_string());
    let window_title = format!("小达鸭保底窗口 · {} · {}", title_case, item.platform_label);

    let session = FallbackSessionSnapshot {
        task_id: item.id.clone(),
        label: label.clone(),
        url: item.url.clone(),
        case_number: item.case_number.clone(),
        court_name: item.court_name.clone(),
        platform_label: item.platform_label.clone(),
        sms_code: item.sms_code.clone(),
        captured_count: 0,
        last_message: Some("请核对法院页面。完成网页下载后将自动校验归档。".to_string()),
        last_error: None,
    };

    {
        let mut guard = lock_unpoisoned(&fallback_mgr.inner);
        guard.session = Some(session.clone());
    }

    let assist_script = build_assist_script(&parsed_url);
    let nav_origin = parsed_url.clone();
    let popup_origin = parsed_url.clone();
    let download_origin = parsed_url.clone();
    let app_for_nav = app.clone();
    let mgr_for_nav = fallback_mgr.clone();
    let label_for_nav = label.clone();
    let label_for_dl = label.clone();
    let app_for_new = app.clone();
    let label_for_new = label.clone();

    let app_for_dl = app.clone();
    let queue_for_dl = queue.clone();
    let mgr_for_dl = fallback_mgr.clone();
    let task_for_dl = item.id.clone();

    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed_url))
        .title(&window_title)
        .inner_size(1080.0, 780.0)
        .incognito(true)
        .initialization_script(&assist_script)
        .on_navigation(move |url| {
            if is_task_origin(url, &nav_origin) {
                return true;
            }
            report_restricted_page(&app_for_nav, &mgr_for_nav, &label_for_nav);
            false
        })
        .on_new_window(move |url, _features| {
            if is_task_origin(&url, &popup_origin) {
                if let Some(w) = app_for_new.get_webview_window(&label_for_new) {
                    let _ = w.navigate(url);
                }
            }
            NewWindowResponse::Deny
        })
        .on_download(move |_webview, event| match event {
            tauri::webview::DownloadEvent::Requested { url, destination } => {
                if !(is_task_origin(&url, &download_origin)
                    || (url.scheme() == "blob" && url.origin() == download_origin.origin()))
                {
                    report_restricted_page(&app_for_dl, &mgr_for_dl, &label_for_dl);
                    return false;
                }
                let mut guard = lock_unpoisoned(&mgr_for_dl.inner);
                if !guard
                    .session
                    .as_ref()
                    .is_some_and(|session| session.label == label_for_dl)
                {
                    return false;
                }
                let hint_from_dest = destination
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let hint_from_url = url
                    .path_segments()
                    .and_then(|mut s| s.next_back())
                    .unwrap_or("")
                    .to_string();
                let raw_hint = if !hint_from_dest.trim().is_empty() {
                    hint_from_dest
                } else if !hint_from_url.trim().is_empty() {
                    hint_from_url
                } else {
                    "document".to_string()
                };
                let clean_hint = sanitize_hint_name(&raw_hint);
                let dest_path = mgr_for_dl.staging_dir().join(format!(
                    "web_{}_{}",
                    Uuid::new_v4().simple(),
                    clean_hint
                ));
                *destination = dest_path.clone();

                guard
                    .pending
                    .entry(url.to_string())
                    .or_default()
                    .push_back(PendingWebDownload {
                        dest_path,
                        suggested_name: raw_hint,
                    });
                if let Some(ref mut s) = guard.session {
                    s.last_error = None;
                    s.last_message = Some(format!("正在从保底窗口捕获下载：{}", clean_hint));
                }
                drop(guard);
                emit_session(&app_for_dl, &mgr_for_dl);
                true
            }
            tauri::webview::DownloadEvent::Finished { url, path, success } => {
                let pending_entry = {
                    let mut guard = lock_unpoisoned(&mgr_for_dl.inner);
                    if !guard
                        .session
                        .as_ref()
                        .is_some_and(|session| session.label == label_for_dl)
                    {
                        return true;
                    }

                    let key = url.to_string();
                    let Some(downloads) = guard.pending.get_mut(&key) else {
                        return true;
                    };
                    let position = match path {
                        Some(ref actual) => downloads
                            .iter()
                            .position(|entry| entry.dest_path == *actual),
                        None if downloads.len() == 1 => Some(0),
                        None => None,
                    };
                    let Some(entry) = position.and_then(|pos| downloads.remove(pos)) else {
                        return true;
                    };
                    if downloads.is_empty() {
                        guard.pending.remove(&key);
                    }
                    entry
                };
                let staged_path = pending_entry.dest_path;
                let suggested_name = pending_entry.suggested_name;

                if !success {
                    let _ = std::fs::remove_file(&staged_path);
                    {
                        let mut guard = lock_unpoisoned(&mgr_for_dl.inner);
                        if let Some(s) = guard.session.as_mut().filter(|s| s.label == label_for_dl)
                        {
                            s.last_error =
                                Some("网页下载未能完成，请在保底窗口重试点击".to_string());
                        }
                    }
                    emit_session(&app_for_dl, &mgr_for_dl);
                    return true;
                }

                let app_bg = app_for_dl.clone();
                let queue_bg = queue_for_dl.clone();
                let mgr_bg = mgr_for_dl.clone();
                let task_bg = task_for_dl.clone();
                let label_bg = label_for_dl.clone();
                let url_str = url.to_string();
                tauri::async_runtime::spawn(async move {
                    let sink = TauriProgressSink::new(app_bg.clone());
                    let res = queue_bg
                        .ingest_captured_file(
                            &task_bg,
                            &staged_path,
                            &suggested_name,
                            Some(&url_str),
                            true,
                            &sink,
                        )
                        .await;
                    {
                        let mut guard = lock_unpoisoned(&mgr_bg.inner);
                        if let Some(s) = guard.session.as_mut().filter(|s| s.label == label_bg) {
                            match res {
                                Ok(doc) => {
                                    s.captured_count += 1;
                                    s.last_error = None;
                                    s.last_message = Some(format!("已归档：{}", doc.file_name));
                                }
                                Err(err) => {
                                    s.last_error = Some(err);
                                }
                            }
                        }
                    }
                    if staged_path.is_file() {
                        let _ = tokio::fs::remove_file(&staged_path).await;
                    }
                    emit_session(&app_bg, &mgr_bg);
                });
                true
            }
            _ => false,
        })
        .build()
        .map_err(|e| {
            let mut guard = lock_unpoisoned(&fallback_mgr.inner);
            guard.session = None;
            format!("创建保底窗口失败：{}", e)
        })?;

    let app_for_evt = app.clone();
    let mgr_for_evt = fallback_mgr.clone();
    let label_for_evt = label.clone();
    win.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let mut should_emit = false;
            {
                let mut guard = lock_unpoisoned(&mgr_for_evt.inner);
                if guard
                    .session
                    .as_ref()
                    .is_some_and(|s| s.label == label_for_evt)
                {
                    guard.session = None;
                    for (_, mut q) in guard.pending.drain() {
                        while let Some(entry) = q.pop_front() {
                            let _ = std::fs::remove_file(entry.dest_path);
                        }
                    }
                    should_emit = true;
                }
            }
            if should_emit {
                emit_session(&app_for_evt, &mgr_for_evt);
            }
        }
    });

    let _ = win.set_focus();
    emit_session(app, fallback_mgr);
    Ok(session)
}

#[cfg(desktop)]
fn report_restricted_page(app: &AppHandle, manager: &FallbackManager, label: &str) {
    let mut state = lock_unpoisoned(&manager.inner);
    if let Some(session) = state
        .session
        .as_mut()
        .filter(|session| session.label == label)
    {
        session.last_error = Some("该取件页需要访问其他站点。请使用系统浏览器。".into());
        drop(state);
        emit_session(app, manager);
    }
}

#[cfg(desktop)]
fn eval_task_page(app: &AppHandle, manager: &FallbackManager, script: &str) -> Result<(), String> {
    let session = manager.snapshot().ok_or("当前没有打开的保底窗口")?;
    let win = app
        .get_webview_window(&session.label)
        .ok_or("保底窗口已关闭，请重新打开")?;
    let expected = Url::parse(&session.url).map_err(|_| "原始取件地址无效")?;
    let current = win.url().map_err(|_| "无法确认当前取件页面地址")?;
    if !is_task_origin(&current, &expected) {
        return Err("当前页面与原始取件地址不同，请使用系统浏览器。".into());
    }
    win.eval(&guarded_page_script(&expected, script))
        .map_err(|_| "无法操作当前取件页".to_string())?;
    let _ = win.set_focus();
    Ok(())
}

#[cfg(desktop)]
pub fn fill_fallback_code(
    app: &AppHandle,
    fallback_mgr: &FallbackManager,
    code: &str,
) -> Result<(), String> {
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 32 {
        return Err("请输入 1–32 位验证码".to_string());
    }
    let code_json = serde_json::to_string(trimmed).map_err(|e| e.to_string())?;
    eval_task_page(
        app,
        fallback_mgr,
        &format!("window.__dadaduckAssist?.fillCode({code_json});"),
    )?;
    Ok(())
}

#[cfg(desktop)]
pub fn scan_fallback_documents(
    app: &AppHandle,
    fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    eval_task_page(
        app,
        fallback_mgr,
        "window.__dadaduckAssist?.scanDocumentLinks();",
    )
}

#[cfg(desktop)]
pub fn focus_fallback_window(
    app: &AppHandle,
    fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    let label = {
        let guard = lock_unpoisoned(&fallback_mgr.inner);
        guard
            .session
            .as_ref()
            .map(|s| s.label.clone())
            .ok_or_else(|| "当前没有打开的保底窗口".to_string())?
    };
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| "保底窗口已关闭".to_string())?;
    win.set_focus().map_err(|e| e.to_string())
}

#[cfg(desktop)]
pub fn close_fallback_window(
    app: &AppHandle,
    fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    let label_opt = {
        let mut guard = lock_unpoisoned(&fallback_mgr.inner);
        for (_, mut q) in guard.pending.drain() {
            while let Some(entry) = q.pop_front() {
                let _ = std::fs::remove_file(entry.dest_path);
            }
        }
        guard.session.take().map(|s| s.label)
    };
    if let Some(label) = label_opt {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.close();
        }
    }
    emit_session(app, fallback_mgr);
    Ok(())
}

#[cfg(not(desktop))]
pub fn open_fallback_window(
    _app: &tauri::AppHandle,
    _queue: &QueueManager,
    _fallback_mgr: &FallbackManager,
    _task_id: &str,
) -> Result<FallbackSessionSnapshot, String> {
    Err("移动端暂不支持内置保底窗口，请复制验证码后用系统浏览器打开".to_string())
}

#[cfg(not(desktop))]
pub fn fill_fallback_code(
    _app: &tauri::AppHandle,
    _fallback_mgr: &FallbackManager,
    _code: &str,
) -> Result<(), String> {
    Err("移动端暂不支持向内置页面填码".to_string())
}

#[cfg(not(desktop))]
pub fn scan_fallback_documents(
    _app: &tauri::AppHandle,
    _fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    Err("移动端暂不支持页面扫描".to_string())
}

#[cfg(not(desktop))]
pub fn focus_fallback_window(
    _app: &tauri::AppHandle,
    _fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    Err("移动端暂不支持内置保底窗口".to_string())
}

#[cfg(not(desktop))]
pub fn close_fallback_window(
    _app: &tauri::AppHandle,
    _fallback_mgr: &FallbackManager,
) -> Result<(), String> {
    Ok(())
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_https_origin_is_allowed() {
        let task = Url::parse("https://court.example/entry?token=fixture").unwrap();
        assert!(is_task_origin(
            &Url::parse("https://court.example/next").unwrap(),
            &task
        ));
        for other in [
            "http://court.example/entry",
            "https://court.example:8443/entry",
            "https://court.example.invalid/entry",
            "https://attacker@court.example/entry",
            "https://another.example/entry",
        ] {
            assert!(!is_task_origin(&Url::parse(other).unwrap(), &task));
        }
        let script = build_assist_script(&task);
        assert!(script.contains("window === window.top"));
        assert!(!script.contains("fixture"));
    }
}
