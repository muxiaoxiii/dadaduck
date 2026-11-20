use crate::models::AndroidStorageMode;
use serde::{Deserialize, Serialize};
use tauri::{plugin::TauriPlugin, Manager, Wry};

#[cfg(target_os = "android")]
use std::sync::Arc;
#[cfg(target_os = "android")]
use tauri::plugin::{mobile::PluginInvokeError, PluginHandle};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub content_uri: String,
    pub display_path: String,
    pub final_file_name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickDirResult {
    pub cancelled: bool,
    pub tree_uri: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocAccessResult {
    pub exists: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AndroidStorageHandle {
    #[cfg(target_os = "android")]
    handle: Option<Arc<PluginHandle<Wry>>>,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishFilePayload<'a> {
    temp_path: &'a str,
    subdir_segments: Vec<String>,
    file_name: &'a str,
    mime_type: Option<&'a str>,
    storage_mode: &'a str,
    tree_uri: Option<&'a str>,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocationPayload<'a> {
    location: &'a str,
    mime_type: Option<&'a str>,
    title: Option<&'a str>,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteResponse {
    deleted: bool,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardResponse {
    text: String,
}

pub fn guess_mime_type(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".ofd") {
        "application/ofd"
    } else if lower.ends_with(".doc") {
        "application/msword"
    } else if lower.ends_with(".docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else if lower.ends_with(".xls") {
        "application/vnd.ms-excel"
    } else if lower.ends_with(".xlsx") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else if lower.ends_with(".ppt") {
        "application/vnd.ms-powerpoint"
    } else if lower.ends_with(".pptx") {
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    } else if lower.ends_with(".wps") {
        "application/vnd.ms-works"
    } else if lower.ends_with(".rtf") {
        "application/rtf"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".zip") {
        "application/zip"
    } else if lower.ends_with(".rar") {
        "application/vnd.rar"
    } else if lower.ends_with(".7z") {
        "application/x-7z-compressed"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        "image/tiff"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

pub fn init_plugin() -> TauriPlugin<Wry> {
    tauri::plugin::Builder::new("android_storage")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let plugin_handle =
                    api.register_android_plugin("com.muxiaoxiii.dadaduck", "AndroidStoragePlugin")?;
                app.manage(AndroidStorageHandle {
                    handle: Some(Arc::new(plugin_handle)),
                });
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = api;
                app.manage(AndroidStorageHandle::default());
            }
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
fn format_plugin_err(err: PluginInvokeError) -> String {
    match err {
        PluginInvokeError::InvokeRejected(resp) => {
            let msg = resp.message.unwrap_or_else(|| "Android 存储调用失败".to_string());
            if let Some(code) = resp.code {
                format!("{}:{}", code, msg)
            } else {
                msg
            }
        }
        other => other.to_string(),
    }
}

impl AndroidStorageHandle {
    #[cfg(target_os = "android")]
    pub fn is_available(&self) -> bool {
        self.handle.is_some()
    }

    #[cfg(not(target_os = "android"))]
    pub fn is_available(&self) -> bool {
        false
    }

    #[cfg(target_os = "android")]
    pub async fn publish_file(
        &self,
        temp_path: &str,
        subdir_segments: Vec<String>,
        file_name: &str,
        mime_type: Option<&str>,
        storage_mode: &AndroidStorageMode,
        tree_uri: Option<&str>,
    ) -> Result<PublishResult, String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        let mode_str = match storage_mode {
            AndroidStorageMode::MediaStoreDownloads => "media_store_downloads",
            AndroidStorageMode::SafTree => "saf_tree",
        };
        handle
            .run_mobile_plugin_async(
                "publishFile",
                PublishFilePayload {
                    temp_path,
                    subdir_segments,
                    file_name,
                    mime_type,
                    storage_mode: mode_str,
                    tree_uri,
                },
            )
            .await
            .map_err(format_plugin_err)
    }

    #[cfg(not(target_os = "android"))]
    pub async fn publish_file(
        &self,
        _temp_path: &str,
        _subdir_segments: Vec<String>,
        _file_name: &str,
        _mime_type: Option<&str>,
        _storage_mode: &AndroidStorageMode,
        _tree_uri: Option<&str>,
    ) -> Result<PublishResult, String> {
        Err("仅 Android 支持 MediaStore / SAF 发布".to_string())
    }

    #[cfg(target_os = "android")]
    pub async fn pick_directory(&self) -> Result<PickDirResult, String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        handle
            .run_mobile_plugin_async("pickDirectory", serde_json::json!({}))
            .await
            .map_err(format_plugin_err)
    }

    #[cfg(not(target_os = "android"))]
    pub async fn pick_directory(&self) -> Result<PickDirResult, String> {
        Err("仅 Android 支持目录授权选择".to_string())
    }

    #[cfg(target_os = "android")]
    pub async fn check_document_access(&self, location: &str) -> Result<DocAccessResult, String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        handle
            .run_mobile_plugin_async(
                "checkDocumentAccess",
                LocationPayload {
                    location,
                    mime_type: None,
                    title: None,
                },
            )
            .await
            .map_err(format_plugin_err)
    }

    #[cfg(not(target_os = "android"))]
    pub async fn check_document_access(&self, location: &str) -> Result<DocAccessResult, String> {
        match std::fs::metadata(location) {
            Ok(meta) if meta.is_file() => Ok(DocAccessResult {
                exists: true,
                size_bytes: meta.len(),
            }),
            _ => Ok(DocAccessResult {
                exists: false,
                size_bytes: 0,
            }),
        }
    }

    #[cfg(target_os = "android")]
    pub async fn open_document(&self, location: &str, mime_type: Option<&str>) -> Result<(), String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        let _: serde_json::Value = handle
            .run_mobile_plugin_async(
                "openDocument",
                LocationPayload {
                    location,
                    mime_type,
                    title: None,
                },
            )
            .await
            .map_err(format_plugin_err)?;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    pub async fn open_document(&self, _location: &str, _mime_type: Option<&str>) -> Result<(), String> {
        Err("桌面端请使用 opener 打开".to_string())
    }

    #[cfg(target_os = "android")]
    pub async fn share_document(
        &self,
        location: &str,
        mime_type: Option<&str>,
        title: Option<&str>,
    ) -> Result<(), String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        let _: serde_json::Value = handle
            .run_mobile_plugin_async(
                "shareDocument",
                LocationPayload {
                    location,
                    mime_type,
                    title,
                },
            )
            .await
            .map_err(format_plugin_err)?;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    pub async fn share_document(
        &self,
        _location: &str,
        _mime_type: Option<&str>,
        _title: Option<&str>,
    ) -> Result<(), String> {
        Err("桌面端暂不支持系统分享面板".to_string())
    }

    #[cfg(target_os = "android")]
    pub async fn delete_document(&self, location: &str) -> Result<bool, String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        let resp: DeleteResponse = handle
            .run_mobile_plugin_async(
                "deleteDocument",
                LocationPayload {
                    location,
                    mime_type: None,
                    title: None,
                },
            )
            .await
            .map_err(format_plugin_err)?;
        Ok(resp.deleted)
    }

    #[cfg(not(target_os = "android"))]
    pub async fn delete_document(&self, location: &str) -> Result<bool, String> {
        let p = std::path::Path::new(location);
        if !p.exists() {
            return Ok(true);
        }
        std::fs::remove_file(p).map(|_| true).map_err(|e| e.to_string())
    }

    #[cfg(target_os = "android")]
    pub async fn copy_sensitive_text(&self, text: &str) -> Result<(), String> {
        let handle = self.handle.as_ref().ok_or("Android 存储桥未初始化")?;
        let _: serde_json::Value = handle
            .run_mobile_plugin_async("copySensitiveText", serde_json::json!({ "text": text }))
            .await
            .map_err(format_plugin_err)?;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    pub async fn copy_sensitive_text(&self, _text: &str) -> Result<(), String> {
        Err("桌面端使用浏览器剪贴板接口".into())
    }

    #[cfg(target_os = "android")]
    pub async fn read_clipboard_text(&self) -> Result<String, String> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "Android 存储桥未初始化".to_string())?;
        let resp: ClipboardResponse = handle
            .run_mobile_plugin_async("readClipboardText", serde_json::json!({}))
            .await
            .map_err(format_plugin_err)?;
        Ok(resp.text)
    }

    #[cfg(not(target_os = "android"))]
    pub async fn read_clipboard_text(&self) -> Result<String, String> {
        Err("桌面端使用浏览器剪贴板接口".to_string())
    }
}
