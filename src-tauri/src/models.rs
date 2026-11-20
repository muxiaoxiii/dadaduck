use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_IDENTITY_ATTEMPTS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferControl {
    Running,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Zxfw,
    HubeiPublic,
    HubeiAccount,
    Gdems,
    Jysd,
    Sfpt,
    Unknown,
}

impl Platform {
    pub fn display_name(&self) -> &'static str {
        match self {
            Platform::Zxfw => "全国统一送达",
            Platform::HubeiPublic => "湖北电子送达(免账号)",
            Platform::HubeiAccount => "湖北电子送达(账号)",
            Platform::Gdems => "广东电子送达",
            Platform::Jysd => "集约送达平台",
            Platform::Sfpt => "司法送达网",
            Platform::Unknown => "未知平台",
        }
    }

    pub fn supports_direct_api(&self) -> bool {
        matches!(self, Platform::Zxfw | Platform::HubeiPublic)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenameTemplate {
    Simple,         // {title}_{date}
    #[default]
    CaseArchive,    // {case_no}_{title}_{date}
    CourtCaseTitle, // {court}_{case_no}_{title}_{date}
    DateCaseTitle,  // {date}_{case_no}_{title}
    TitleCaseParen, // {title}（{case_no}）_{date}
    IndexTitle,     // {index2}_{title}
    Original,       // {raw_title}
    Custom,         // user-defined pattern
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubdirMode {
    None,
    #[default]
    CaseNumber,
    CourtAndCase,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LawyerProfile {
    pub id: String,
    pub name: String,
    pub mobile_last4: String,
    #[serde(default)]
    pub id_last4: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AndroidStorageMode {
    #[default]
    MediaStoreDownloads,
    SafTree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_dir: String,
    pub create_case_subdir: bool,
    #[serde(default)]
    pub subdir_mode: SubdirMode,
    pub rename_template: RenameTemplate,
    pub custom_template: String,
    #[serde(default)]
    pub prefix_index: bool,
    #[serde(default)]
    pub matrix_enabled: bool,
    pub request_delay_ms: u64,
    #[serde(default)]
    pub profiles: Vec<LawyerProfile>,
    #[serde(default)]
    pub android_storage_mode: AndroidStorageMode,
    #[serde(default)]
    pub android_tree_uri: Option<String>,
    #[serde(default)]
    pub android_tree_label: Option<String>,
}

impl AppSettings {
    pub fn default_with_dir(output_dir: String) -> Self {
        Self {
            output_dir,
            create_case_subdir: true,
            subdir_mode: SubdirMode::CaseNumber,
            rename_template: RenameTemplate::CaseArchive,
            custom_template: "{case_no}_{title}_{date}".to_string(),
            prefix_index: false,
            matrix_enabled: false,
            request_delay_ms: 400,
            profiles: Vec::new(),
            android_storage_mode: AndroidStorageMode::MediaStoreDownloads,
            android_tree_uri: None,
            android_tree_label: None,
        }
    }

    pub fn effective_subdir_mode(&self) -> SubdirMode {
        if !self.create_case_subdir && self.subdir_mode == SubdirMode::CaseNumber {
            SubdirMode::None
        } else {
            self.subdir_mode.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryParams {
    pub qdbh: Option<String>,
    pub sdbh: Option<String>,
    pub sdsin: Option<String>,
    pub mm: Option<String>,
    pub msg: Option<String>,
}

impl DeliveryParams {
    pub fn to_zxfw_payload(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        if let Some(v) = &self.qdbh {
            map.insert("qdbh".to_string(), v.clone());
        }
        if let Some(v) = &self.sdbh {
            map.insert("sdbh".to_string(), v.clone());
        }
        if let Some(v) = &self.sdsin {
            map.insert("sdsin".to_string(), v.clone());
        }
        if let Some(v) = &self.mm {
            map.insert("mm".to_string(), v.clone());
        }
        map
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Running,
    NeedsIdentity,
    Completed,
    Partial,
    Failed,
    BrowserRequired,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    #[default]
    NotRequested,
    Confirmed,
    Unconfirmed,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocSaveStatus {
    #[default]
    Saved,
    SaveFailed,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedDoc {
    pub raw_title: String,
    pub file_name: String,
    pub saved_path: String,
    #[serde(default)]
    pub storage_uri: Option<String>,
    #[serde(default)]
    pub display_location: String,
    #[serde(default)]
    pub staging_path: Option<String>,
    #[serde(default)]
    pub save_status: DocSaveStatus,
    #[serde(default)]
    pub save_error: Option<String>,
    pub size_bytes: u64,
    pub court_name: Option<String>,
    pub document_id: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocFailure {
    pub index: usize,
    pub name: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub source_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileProgress {
    pub task_id: String,
    pub doc_index: usize,
    pub doc_total: usize,
    pub doc_name: String,
    pub target_file_name: Option<String>,
    pub stage: String,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: String,
    pub raw_text: String,
    pub platform: Platform,
    pub platform_label: String,
    pub url: String,
    pub params: DeliveryParams,
    pub fingerprint: String,
    pub case_number: Option<String>,
    pub court_name: Option<String>,
    pub parties: Vec<String>,
    pub sms_code: Option<String>,
    pub status: ItemStatus,
    pub status_message: String,
    #[serde(default)]
    pub stage_label: String,
    pub receipt_status: ReceiptStatus,
    pub identity_attempts_used: u8,
    pub identity_attempts_limit: u8,
    pub documents: Vec<DownloadedDoc>,
    pub failures: Vec<DocFailure>,
    pub current_doc_index: usize,
    pub current_doc_total: usize,
    pub current_doc_name: Option<String>,
    #[serde(default)]
    pub current_target_file_name: Option<String>,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    #[serde(default)]
    pub speed_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnrecognizedBlockDiagnostic {
    pub raw_text: String,
    pub reason: String,
    #[serde(default)]
    pub case_number: Option<String>,
    #[serde(default)]
    pub court_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseBatchSummary {
    pub added: Vec<QueueItem>,
    pub skipped_duplicates: usize,
    pub unrecognized_lines: usize,
    #[serde(default)]
    pub unrecognized_blocks: Vec<String>,
    #[serde(default)]
    pub block_diagnostics: Vec<UnrecognizedBlockDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDocumentEntry {
    pub path: String,
    pub file_name: String,
    pub relative_dir: String,
    pub size_bytes: u64,
    pub inferred_case_number: Option<String>,
    pub inferred_court_name: Option<String>,
    pub needs_case_confirmation: bool,
    pub exported_before: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyExportInput {
    pub path: String,
    pub case_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyExportResult {
    pub exported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub exported_docs: Vec<DownloadedDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub items: Vec<QueueItem>,
    pub is_running: bool,
    pub is_paused: bool,
    pub total_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub needs_identity_count: usize,
    pub pending_count: usize,
    pub browser_required_count: usize,
    pub total_downloaded_docs: usize,
    pub total_downloaded_bytes: u64,
}
