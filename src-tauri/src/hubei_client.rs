use crate::zxfw_client::{ZxfwClient, ZxfwDocEntry, ZxfwError};
use reqwest::header::{HeaderMap, HeaderValue, REFERER, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use url::Url;

pub const DEFAULT_HUBEI_BASE: &str = "https://dzsd.hbfy.gov.cn/delimobile";
const CONNECT_TIMEOUT_SECS: u64 = 15;
const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_ATTEMPTS: usize = 3;
const RETRY_DELAYS_SECS: [u64; 2] = [3, 8];

#[derive(Debug, Error)]
pub enum HubeiError {
    #[error("湖北平台当前要求图形验证码，需用浏览器打开处理")]
    CaptchaRequired,
    #[error("湖北平台无法建立 HTTPS 加密连接，已停止自动下载以保护送达凭据，请用浏览器打开")]
    InsecureTransport,
    #[error("请求失败: {0}")]
    Request(String),
    #[error("下载失败: {0}")]
    Download(#[from] ZxfwError),
}

#[derive(Debug, Deserialize)]
struct HubeiResponse {
    data: Option<HubeiData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubeiData {
    is_need_captcha: Option<String>,
    doc_list: Option<Vec<HubeiDoc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubeiDoc {
    doc_name: Option<String>,
    download_path: Option<String>,
    pdf_path: Option<String>,
}

pub struct HubeiClient {
    http: Client,
    base_url: String,
    require_https: bool,
    downloader: ZxfwClient,
}

impl HubeiClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Self::build(DEFAULT_HUBEI_BASE, true)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, reqwest::Error> {
        Self::build(base_url, false)
    }

    fn build(base_url: &str, require_https: bool) -> Result<Self, reqwest::Error> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://dzsd.hbfy.gov.cn/deli-mobile-ui/"),
        );
        let http = Client::builder()
            .cookie_store(true)
            .https_only(require_https)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .default_headers(headers)
            .build()?;
        let downloader = ZxfwClient::from_client(http.clone(), base_url);
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            require_https,
            downloader,
        })
    }

    pub async fn fetch_doc_list(&self, msg: &str) -> Result<Vec<ZxfwDocEntry>, HubeiError> {
        if self.require_https && !self.base_url.starts_with("https://") {
            return Err(HubeiError::InsecureTransport);
        }

        let mut last_err = String::new();
        let mut parsed_opt: Option<HubeiResponse> = None;

        for attempt in 0..MAX_ATTEMPTS {
            let ts = chrono::Utc::now().timestamp_millis();
            let url = format!("{}/tDeliSms/findSmsInfo?t={}", self.base_url, ts);
            match self
                .http
                .post(&url)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .json(&serde_json::json!({ "msg": msg }))
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if is_transient_status(status) {
                        last_err = format!("HTTP {}", status.as_u16());
                    } else if !status.is_success() {
                        return Err(HubeiError::Request(format!("HTTP {}", status.as_u16())));
                    } else {
                        let parsed: HubeiResponse = resp
                            .json()
                            .await
                            .map_err(|e| HubeiError::Request(e.to_string()))?;
                        parsed_opt = Some(parsed);
                        break;
                    }
                }
                Err(e) => {
                    if self.require_https && (e.is_connect() || e.is_builder()) {
                        return Err(HubeiError::InsecureTransport);
                    }
                    last_err = e.to_string();
                }
            }

            if attempt + 1 < MAX_ATTEMPTS {
                let delay = RETRY_DELAYS_SECS[attempt.min(RETRY_DELAYS_SECS.len() - 1)];
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }

        let parsed = parsed_opt.ok_or(HubeiError::Request(last_err))?;
        let data = parsed
            .data
            .ok_or_else(|| HubeiError::Request("响应缺少 data".to_string()))?;

        if data.is_need_captcha.as_deref() == Some("Y") {
            return Err(HubeiError::CaptchaRequired);
        }

        let raw_docs = data.doc_list.unwrap_or_default();
        let mut entries = Vec::with_capacity(raw_docs.len());

        for doc in raw_docs {
            let rel_opt = doc
                .download_path
                .filter(|s| !s.trim().is_empty())
                .or_else(|| doc.pdf_path.filter(|s| !s.trim().is_empty()));

            let wjlj = match rel_opt {
                Some(rel) => {
                    let full_url = resolve_hubei_doc_url(&self.base_url, &rel);
                    if self.require_https {
                        let Ok(parsed_dl) = Url::parse(&full_url) else {
                            return Err(HubeiError::InsecureTransport);
                        };
                        if parsed_dl.scheme() != "https" {
                            return Err(HubeiError::InsecureTransport);
                        }
                    }
                    Some(full_url)
                }
                None => None,
            };

            entries.push(ZxfwDocEntry {
                c_wsmc: doc.doc_name,
                wjlj,
                c_fymc: Some("湖北法院电子送达".to_string()),
                c_wsbh: None,
                c_wjgs: None,
            });
        }

        Ok(entries)
    }

    pub fn downloader(&self) -> &ZxfwClient {
        &self.downloader
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429) || status.is_server_error()
}

fn resolve_hubei_doc_url(base_url: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        rel.to_string()
    } else if rel.starts_with('/') {
        if let Ok(parsed_base) = Url::parse(base_url) {
            let base_path = parsed_base.path().trim_end_matches('/');
            if !base_path.is_empty()
                && (rel == base_path || rel.starts_with(&format!("{}/", base_path)))
            {
                return format!("{}{}", parsed_base.origin().ascii_serialization(), rel);
            }
        }
        format!("{}{}", base_url, rel)
    } else {
        format!("{}/{}", base_url, rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn detects_captcha_and_preserves_missing_download_url_entries() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            for idx in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let body = if idx == 0 {
                    serde_json::json!({
                        "data": { "isNeedCaptcha": "Y", "docList": [] }
                    })
                    .to_string()
                } else {
                    serde_json::json!({
                        "data": {
                            "isNeedCaptcha": "N",
                            "docList": [
                                { "docName": "送达回证", "downloadPath": "/files/1.pdf" },
                                { "docName": "缺失地址文书", "downloadPath": null, "pdfPath": "" }
                            ]
                        }
                    })
                    .to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });

        let client = HubeiClient::with_base_url(&format!("http://{}", addr)).unwrap();
        let err = client.fetch_doc_list("MSG1").await.unwrap_err();
        assert!(matches!(err, HubeiError::CaptchaRequired));

        let docs = client.fetch_doc_list("MSG2").await.unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].c_wsmc.as_deref(), Some("送达回证"));
        assert!(docs[0].wjlj.as_deref().unwrap().ends_with("/files/1.pdf"));
        assert_eq!(docs[1].c_wsmc.as_deref(), Some("缺失地址文书"));
        assert!(docs[1].wjlj.is_none());
    }

    #[tokio::test]
    async fn rejects_plaintext_http_in_strict_https_mode() {
        let client = HubeiClient::build("http://dzsd.hbfy.gov.cn/delimobile", true).unwrap();
        let err = client.fetch_doc_list("MSG1").await.unwrap_err();
        assert!(matches!(err, HubeiError::InsecureTransport));
    }

    #[tokio::test]
    async fn preserves_non_pdf_extension_hint_and_dedups_base_path() {
        assert_eq!(
            resolve_hubei_doc_url("https://dzsd.hbfy.gov.cn/delimobile", "/delimobile/files/a.docx"),
            "https://dzsd.hbfy.gov.cn/delimobile/files/a.docx"
        );
        assert_eq!(
            resolve_hubei_doc_url("https://dzsd.hbfy.gov.cn/delimobile", "/files/a.docx"),
            "https://dzsd.hbfy.gov.cn/delimobile/files/a.docx"
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let body = serde_json::json!({
                    "data": {
                        "isNeedCaptcha": "N",
                        "docList": [
                            { "docName": "证据材料", "downloadPath": "/files/evidence.docx" }
                        ]
                    }
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });

        let client = HubeiClient::with_base_url(&format!("http://{}/delimobile", addr)).unwrap();
        let docs = client.fetch_doc_list("MSG3").await.unwrap();
        assert_eq!(docs.len(), 1);
        assert!(docs[0].c_wjgs.is_none());
        assert!(docs[0].wjlj.as_deref().unwrap().ends_with("/delimobile/files/evidence.docx"));
    }
}
