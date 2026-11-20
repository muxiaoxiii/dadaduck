use crate::models::{DeliveryParams, TransferControl};
use crate::validator::{validate_file, ValidationError};
use futures_util::StreamExt;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

pub const DEFAULT_ZXFW_BASE: &str = "https://zxfw.court.gov.cn/yzw/yzw-zxfw-sdfw/api/v1/sdfw";
const CONNECT_TIMEOUT_SECS: u64 = 15;
const LIST_TIMEOUT_SECS: u64 = 30;
const HEADER_TIMEOUT_SECS: u64 = 30;
const CHUNK_IDLE_TIMEOUT_SECS: u64 = 45;
const MAX_ATTEMPTS: usize = 3;
const RETRY_DELAYS_SECS: [u64; 2] = [3, 8];

#[derive(Debug, Error)]
pub enum ZxfwError {
    #[error("需要身份核验（手机或证件后四位）")]
    IdentityRequired,
    #[error("接口返回业务错误码 {0}")]
    ApiBusinessError(String),
    #[error("接口返回数据格式异常: {0}")]
    InvalidResponse(String),
    #[error("网络请求重试耗尽: {0}")]
    TransientExhausted(String),
    #[error("身份核验失败: 后四位不匹配")]
    IdentityRejected,
    #[error("身份核验响应不明: {0}")]
    IdentityUnknown(String),
    #[error("后四位格式必须为4位数字或字母")]
    InvalidTailFormat,
    #[error("文件校验失败: {0}")]
    Validation(#[from] ValidationError),
    #[error("不支持的下载地址协议")]
    InvalidDownloadScheme,
    #[error("HTTP 状态错误 {0}")]
    HttpStatus(u16),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("下载已暂停")]
    Paused,
    #[error("下载已停止")]
    Cancelled,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    code: Option<serde_json::Value>,
    msg: Option<String>,
    message: Option<String>,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZxfwDocEntry {
    pub c_wsmc: Option<String>,
    pub wjlj: Option<String>,
    pub c_fymc: Option<String>,
    pub c_wsbh: Option<String>,
    pub c_wjgs: Option<String>,
}

fn identity_required_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:请|需要|须|尚未|未完成|未进行|未通过).{0,12}身份(?:核验|验证|校验)|(?:请|需要|须).{0,12}(?:核验|验证|校验)身份|身份(?:核验|验证|校验).{0,8}(?:未通过|不通过|未完成)|(?:请输入|请填写|请核对).{0,24}(?:手机号|手机号码|证件号码|身份证号码).{0,4}后四位|(?:手机号|手机号码|证件号码|身份证号码).{0,4}后四位.{0,8}(?:不匹配|有误|错误|不通过)",
        )
        .unwrap()
    })
}

fn identity_mismatch_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:手机号(?:码)?|手机号码|证件(?:号码)?|身份证(?:号码)?|后四位)[^，。；\n]{0,24}(?:有误|错误|不符|不匹配|不一致|不正确|未通过|不通过|校验失败|验证失败|核验失败)",
        )
        .unwrap()
    })
}

fn has_service_error_word(msg: &str) -> bool {
    const WORDS: &[&str] = &[
        "服务", "系统", "接口", "超时", "繁忙", "异常", "不可用", "网络", "稍后",
    ];
    WORDS.iter().any(|w| msg.contains(w))
}

pub fn api_requires_identity(code: i64, msg: Option<&str>) -> bool {
    if !matches!(code, 400 | 401 | 403 | 500) {
        return false;
    }
    let Some(message) = msg else {
        return false;
    };
    if has_service_error_word(message) {
        return false;
    }
    identity_required_regex().is_match(message)
}

fn parse_api_code(code: Option<&serde_json::Value>) -> Option<i64> {
    match code? {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[derive(Clone)]
pub struct ZxfwClient {
    client: Client,
    base_url: String,
}

impl ZxfwClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Self::with_base_url(DEFAULT_ZXFW_BASE)
    }

    pub fn from_client(client: Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, reqwest::Error> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://zxfw.court.gov.cn"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://zxfw.court.gov.cn/zxfw/"),
        );

        let client = Client::builder()
            .cookie_store(true)
            .https_only(base_url.starts_with("https://"))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10
                    || attempt
                        .previous()
                        .first()
                        .is_some_and(|url| url.origin() != attempt.url().origin())
                {
                    return attempt.stop();
                }
                attempt.follow()
            }))
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .default_headers(headers)
            .build()?;

        Ok(Self::from_client(client, base_url))
    }

    pub async fn fetch_ws_list(
        &self,
        params: &DeliveryParams,
    ) -> Result<Vec<ZxfwDocEntry>, ZxfwError> {
        let url = format!("{}/getWsListBySdbhNew", self.base_url);
        let payload = params.to_zxfw_payload();
        let mut last_err = String::new();

        for attempt in 0..MAX_ATTEMPTS {
            match self
                .client
                .post(&url)
                .timeout(Duration::from_secs(LIST_TIMEOUT_SECS))
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if is_transient_status(status) {
                        last_err = format!("HTTP {}", status.as_u16());
                    } else if !status.is_success() {
                        return Err(ZxfwError::HttpStatus(status.as_u16()));
                    } else {
                        let text = resp.text().await.map_err(|e| ZxfwError::InvalidResponse(e.to_string()))?;
                        let env: ApiEnvelope = serde_json::from_str(&text)
                            .map_err(|_| ZxfwError::InvalidResponse("非 JSON 响应".to_string()))?;
                        let code = parse_api_code(env.code.as_ref());
                        let msg = env.msg.as_deref().or(env.message.as_deref());

                        if let Some(c) = code {
                            if api_requires_identity(c, msg) {
                                return Err(ZxfwError::IdentityRequired);
                            }
                            if c != 200 {
                                let detail = msg.unwrap_or("业务错误");
                                return Err(ZxfwError::ApiBusinessError(format!("{} ({})", c, detail)));
                            }
                        } else {
                            return Err(ZxfwError::InvalidResponse("缺少状态码".to_string()));
                        }

                        let data = env
                            .data
                            .ok_or_else(|| ZxfwError::InvalidResponse("缺少 data 字段".to_string()))?;
                        let list: Vec<ZxfwDocEntry> = serde_json::from_value(data)
                            .map_err(|_| ZxfwError::InvalidResponse("data 字段不是数组".to_string()))?;
                        return Ok(list);
                    }
                }
                Err(err) => {
                    last_err = err.to_string();
                }
            }

            if attempt + 1 < MAX_ATTEMPTS {
                let delay = RETRY_DELAYS_SECS[attempt.min(RETRY_DELAYS_SECS.len() - 1)];
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }

        Err(ZxfwError::TransientExhausted(last_err))
    }

    pub async fn identity_gate(&self, params: &DeliveryParams) -> Option<bool> {
        let url = format!("{}/getSfhyKg", self.base_url);
        let payload = params.to_zxfw_payload();
        let resp = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(LIST_TIMEOUT_SECS))
            .json(&payload)
            .send()
            .await
            .ok()?;
        let env: ApiEnvelope = resp.json().await.ok()?;
        if parse_api_code(env.code.as_ref()) != Some(200) {
            return None;
        }
        match env.data {
            Some(serde_json::Value::Bool(b)) => Some(b),
            Some(serde_json::Value::String(s)) if s == "true" => Some(true),
            Some(serde_json::Value::String(s)) if s == "false" => Some(false),
            _ => None,
        }
    }

    pub async fn verify_identity(
        &self,
        params: &DeliveryParams,
        method: &str,
        tail: &str,
    ) -> Result<(), ZxfwError> {
        let normalized = tail.trim();
        let valid_tail = if method == "sjhm" {
            normalized.len() == 4 && normalized.chars().all(|c| c.is_ascii_digit())
        } else {
            normalized.len() == 4 && normalized.chars().all(|c| c.is_ascii_alphanumeric())
        };
        if !matches!(method, "sjhm" | "zjhm") || !valid_tail {
            return Err(ZxfwError::InvalidTailFormat);
        }

        let jyz = if method == "zjhm" {
            normalized.to_ascii_uppercase()
        } else {
            normalized.to_string()
        };

        let url = format!("{}/checkSdryxx", self.base_url);
        let mut payload: BTreeMap<String, String> = params.to_zxfw_payload();
        payload.insert("jylx".to_string(), method.to_string());
        payload.insert("jyz".to_string(), jyz);

        let resp = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(LIST_TIMEOUT_SECS))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ZxfwError::IdentityUnknown(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ZxfwError::IdentityUnknown(format!(
                "HTTP {}",
                resp.status().as_u16()
            )));
        }

        let env: ApiEnvelope = resp
            .json()
            .await
            .map_err(|e| ZxfwError::IdentityUnknown(e.to_string()))?;
        let code = parse_api_code(env.code.as_ref());
        if code == Some(200) {
            return Ok(());
        }

        let msg = env.msg.as_deref().or(env.message.as_deref()).unwrap_or("");
        if matches!(code, Some(400 | 403 | 500))
            && !has_service_error_word(msg)
            && identity_mismatch_regex().is_match(msg)
        {
            return Err(ZxfwError::IdentityRejected);
        }

        Err(ZxfwError::IdentityUnknown(if msg.is_empty() {
            "核验返回状态不明".to_string()
        } else {
            msg.to_string()
        }))
    }

    pub async fn download_to_staging<F, S>(
        &self,
        download_url: &str,
        staging_dir: &Path,
        extension: &str,
        on_progress: F,
        on_stage: S,
    ) -> Result<(std::path::PathBuf, u64), ZxfwError>
    where
        F: FnMut(u64, Option<u64>),
        S: FnMut(&'static str),
    {
        self.download_to_staging_with_control(
            download_url,
            staging_dir,
            extension,
            on_progress,
            on_stage,
            None,
        )
        .await
    }

    pub async fn download_to_staging_with_control<F, S>(
        &self,
        download_url: &str,
        staging_dir: &Path,
        extension: &str,
        mut on_progress: F,
        mut on_stage: S,
        mut control_rx: Option<watch::Receiver<TransferControl>>,
    ) -> Result<(std::path::PathBuf, u64), ZxfwError>
    where
        F: FnMut(u64, Option<u64>),
        S: FnMut(&'static str),
    {
        let parsed = url::Url::parse(download_url).map_err(|_| ZxfwError::InvalidDownloadScheme)?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(ZxfwError::InvalidDownloadScheme);
        }

        tokio::fs::create_dir_all(staging_dir).await?;

        enum AttemptOutcome {
            Success,
            Paused,
            Cancelled,
            Transient(String),
            Fatal(String),
            IoError(std::io::Error),
        }

        let mut last_err = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            if let Some(ref rx) = control_rx {
                match *rx.borrow() {
                    TransferControl::Paused => return Err(ZxfwError::Paused),
                    TransferControl::Cancelled => return Err(ZxfwError::Cancelled),
                    TransferControl::Running => {}
                }
            }

            on_stage("正在下载");
            let temp = tempfile::NamedTempFile::new_in(staging_dir)?;
            let (file, temp_path) = temp.into_parts();

            let res = async {
                let req_fut = self.client.get(download_url).send();

                let resp = tokio::select! {
                    biased;
                    ctrl = wait_control_abort(&mut control_rx) => {
                        return match ctrl {
                            TransferControl::Paused => AttemptOutcome::Paused,
                            _ => AttemptOutcome::Cancelled,
                        };
                    }
                    r = tokio::time::timeout(Duration::from_secs(HEADER_TIMEOUT_SECS), req_fut) => match r {
                        Ok(Ok(r)) => r,
                        Ok(Err(e)) => return AttemptOutcome::Transient(e.to_string()),
                        Err(_) => return AttemptOutcome::Transient("等待响应头超时".to_string()),
                    }
                };

                let status = resp.status();
                if is_transient_status(status) {
                    return AttemptOutcome::Transient(format!("HTTP {}", status.as_u16()));
                }
                if !status.is_success() {
                    return AttemptOutcome::Fatal(format!("HTTP {}", status.as_u16()));
                }

                let total = resp.content_length();
                on_progress(0, total);

                let mut out = tokio::fs::File::from_std(file);
                let mut stream = resp.bytes_stream();
                let mut written: u64 = 0;

                loop {
                    let chunk_opt = tokio::select! {
                        biased;
                        ctrl = wait_control_abort(&mut control_rx) => {
                            return match ctrl {
                                TransferControl::Paused => AttemptOutcome::Paused,
                                _ => AttemptOutcome::Cancelled,
                            };
                        }
                        next_item = tokio::time::timeout(
                            Duration::from_secs(CHUNK_IDLE_TIMEOUT_SECS),
                            stream.next(),
                        ) => match next_item {
                            Ok(item) => item,
                            Err(_) => return AttemptOutcome::Transient("下载连接超时".to_string()),
                        },
                    };

                    let Some(chunk_res) = chunk_opt else {
                        break;
                    };
                    let chunk = match chunk_res {
                        Ok(c) => c,
                        Err(e) => return AttemptOutcome::Transient(e.to_string()),
                    };
                    if let Err(e) = out.write_all(&chunk).await {
                        return AttemptOutcome::IoError(e);
                    }
                    written += chunk.len() as u64;
                    on_progress(written, total);
                }

                if let Err(e) = out.flush().await {
                    return AttemptOutcome::IoError(e);
                }
                drop(out);

                if let Some(expected) = total {
                    if written != expected {
                        return AttemptOutcome::Transient("下载字节数不完整".to_string());
                    }
                }
                AttemptOutcome::Success
            }
            .await;

            match res {
                AttemptOutcome::Success => {
                    if let Some(ref rx) = control_rx {
                        match *rx.borrow() {
                            TransferControl::Paused => {
                                drop(temp_path);
                                return Err(ZxfwError::Paused);
                            }
                            TransferControl::Cancelled => {
                                drop(temp_path);
                                return Err(ZxfwError::Cancelled);
                            }
                            TransferControl::Running => {}
                        }
                    }
                    on_stage("正在校验");
                    let size = validate_file(&temp_path, extension)?;
                    let kept_path = temp_path.keep().map_err(|err| err.error)?;
                    return Ok((kept_path, size));
                }
                AttemptOutcome::Paused => {
                    drop(temp_path);
                    return Err(ZxfwError::Paused);
                }
                AttemptOutcome::Cancelled => {
                    drop(temp_path);
                    return Err(ZxfwError::Cancelled);
                }
                AttemptOutcome::IoError(err) => {
                    drop(temp_path);
                    return Err(ZxfwError::Io(err));
                }
                AttemptOutcome::Fatal(msg) => {
                    drop(temp_path);
                    if msg.starts_with("HTTP ") {
                        let code: u16 = msg.trim_start_matches("HTTP ").parse().unwrap_or(500);
                        return Err(ZxfwError::HttpStatus(code));
                    }
                    return Err(ZxfwError::InvalidResponse(msg));
                }
                AttemptOutcome::Transient(msg) => {
                    drop(temp_path);
                    last_err = msg;
                }
            }

            if attempt + 1 < MAX_ATTEMPTS {
                let delay = RETRY_DELAYS_SECS[attempt.min(RETRY_DELAYS_SECS.len() - 1)];
                tokio::select! {
                    biased;
                    ctrl = wait_control_abort(&mut control_rx) => {
                        return match ctrl {
                            TransferControl::Paused => Err(ZxfwError::Paused),
                            _ => Err(ZxfwError::Cancelled),
                        };
                    }
                    _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                }
            }
        }

        Err(ZxfwError::TransientExhausted(last_err))
    }

    pub async fn download_file<F>(
        &self,
        download_url: &str,
        destination: &Path,
        extension: &str,
        on_progress: F,
    ) -> Result<u64, ZxfwError>
    where
        F: FnMut(u64, Option<u64>),
    {
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let staging_dir = destination.parent().unwrap_or_else(|| Path::new("."));
        let (kept_path, size) = self
            .download_to_staging(download_url, staging_dir, extension, on_progress, |_| {})
            .await?;
        if destination.exists() {
            let _ = std::fs::remove_file(&kept_path);
            return Err(ZxfwError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination already exists",
            )));
        }
        match std::fs::rename(&kept_path, destination) {
            Ok(()) => Ok(size),
            Err(err) => {
                let _ = std::fs::remove_file(&kept_path);
                Err(ZxfwError::Io(err))
            }
        }
    }
}

async fn wait_control_abort(control_rx: &mut Option<watch::Receiver<TransferControl>>) -> TransferControl {
    let Some(rx) = control_rx.as_mut() else {
        return std::future::pending().await;
    };
    loop {
        let current = *rx.borrow_and_update();
        if current != TransferControl::Running {
            return current;
        }
        if rx.changed().await.is_err() {
            return TransferControl::Cancelled;
        }
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429) || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_mock_server<F, Fut>(handler: F) -> String
    where
        F: Fn(String, String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = (u16, &'static str, Vec<u8>)> + Send,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handler = Arc::new(handler);

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let handler = handler.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    let (status, content_type, resp_body) = handler(path, body).await;
                    let header = format!(
                        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status,
                        content_type,
                        resp_body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(&resp_body).await;
                });
            }
        });

        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn direct_api_and_download_succeeds() {
        let base = spawn_mock_server(|path, _body| async move {
            if path.ends_with("/getWsListBySdbhNew") {
                let json = serde_json::json!({
                    "code": 200,
                    "data": [{
                        "c_wsmc": "诉讼参与人举证材料",
                        "c_wjgs": "pdf",
                        "c_fymc": "测试法院",
                        "c_wsbh": "doc-1",
                        "wjlj": "PLACEHOLDER"
                    }]
                });
                (200, "application/json", json.to_string().into_bytes())
            } else {
                (200, "application/pdf", b"%PDF-1.7\nmock-pdf-content".to_vec())
            }
        })
        .await;

        let client = ZxfwClient::with_base_url(&base).unwrap();
        let params = DeliveryParams {
            qdbh: Some("Q1".into()),
            sdbh: Some("S1".into()),
            sdsin: Some("X1".into()),
            ..Default::default()
        };

        let docs = client.fetch_ws_list(&params).await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].c_wsmc.as_deref(), Some("诉讼参与人举证材料"));

        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("test.pdf");
        let mut progress_calls = 0usize;
        let size = client
            .download_file(&format!("{}/file/1.pdf", base), &dest, ".pdf", |_, _| {
                progress_calls += 1;
            })
            .await
            .unwrap();
        assert_eq!(size, 25);
        assert!(progress_calls >= 1);
        std::fs::write(&dest, b"existing document").unwrap();
        assert!(client.download_file(&format!("{}/file/1.pdf", base), &dest, ".pdf", |_, _| {}).await.is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"existing document");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn detects_identity_required_and_verifies() {
        let verified = Arc::new(AtomicUsize::new(0));
        let verified_clone = verified.clone();
        let base = spawn_mock_server(move |path, body| {
            let verified_flag = verified_clone.clone();
            async move {
                if path.ends_with("/getWsListBySdbhNew") {
                    if verified_flag.load(Ordering::SeqCst) == 0 {
                        let json = serde_json::json!({
                            "code": 403,
                            "msg": "请先完成身份核验"
                        });
                        (200, "application/json", json.to_string().into_bytes())
                    } else {
                        let json = serde_json::json!({"code": 200, "data": []});
                        (200, "application/json", json.to_string().into_bytes())
                    }
                } else if path.ends_with("/checkSdryxx") {
                    if body.contains("\"jyz\":\"1234\"") {
                        verified_flag.store(1, Ordering::SeqCst);
                        (200, "application/json", br#"{"code":200}"#.to_vec())
                    } else {
                        (
                            200,
                            "application/json",
                            serde_json::json!({"code": 400, "msg": "手机号码后四位不匹配"})
                                .to_string()
                                .into_bytes(),
                        )
                    }
                } else {
                    (404, "text/plain", b"not found".to_vec())
                }
            }
        })
        .await;

        let client = ZxfwClient::with_base_url(&base).unwrap();
        let params = DeliveryParams {
            qdbh: Some("Q1".into()),
            sdbh: Some("S1".into()),
            sdsin: Some("X1".into()),
            ..Default::default()
        };

        let err = client.fetch_ws_list(&params).await.unwrap_err();
        assert!(matches!(err, ZxfwError::IdentityRequired));

        let reject = client.verify_identity(&params, "sjhm", "9999").await.unwrap_err();
        assert!(matches!(reject, ZxfwError::IdentityRejected));

        client.verify_identity(&params, "sjhm", "1234").await.unwrap();
        let docs = client.fetch_ws_list(&params).await.unwrap();
        assert!(docs.is_empty());
    }

    #[tokio::test]
    async fn rejects_non_http_download_scheme() {
        let client = ZxfwClient::new().unwrap();
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("bad.pdf");
        let err = client
            .download_file("file:///etc/passwd", &dest, ".pdf", |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, ZxfwError::InvalidDownloadScheme));
    }

    #[tokio::test]
    async fn accepts_string_code_200_and_uppercases_zjhm_x() {
        let saw_upper_x = Arc::new(AtomicUsize::new(0));
        let saw_clone = saw_upper_x.clone();
        let base = spawn_mock_server(move |path, body| {
            let saw = saw_clone.clone();
            async move {
                if path.ends_with("/getWsListBySdbhNew") {
                    (200, "application/json", br#"{"code":"200","data":[]}"#.to_vec())
                } else if path.ends_with("/getSfhyKg") {
                    (200, "application/json", br#"{"code":"200","data":"true"}"#.to_vec())
                } else if path.ends_with("/checkSdryxx") {
                    if body.contains("\"jyz\":\"123X\"") {
                        saw.store(1, Ordering::SeqCst);
                    }
                    (200, "application/json", br#"{"code":"200"}"#.to_vec())
                } else {
                    (404, "text/plain", b"nope".to_vec())
                }
            }
        })
        .await;

        let client = ZxfwClient::with_base_url(&base).unwrap();
        let params = DeliveryParams {
            qdbh: Some("Q1".into()),
            sdbh: Some("S1".into()),
            sdsin: Some("X1".into()),
            ..Default::default()
        };
        assert!(client.fetch_ws_list(&params).await.unwrap().is_empty());
        assert_eq!(client.identity_gate(&params).await, Some(true));
        client.verify_identity(&params, "zjhm", "123x").await.unwrap();
        assert_eq!(saw_upper_x.load(Ordering::SeqCst), 1);
    }
}
