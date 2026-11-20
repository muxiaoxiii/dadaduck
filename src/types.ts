export type Platform =
  | "zxfw"
  | "hubei_public"
  | "hubei_account"
  | "gdems"
  | "jysd"
  | "sfpt"
  | "unknown";

export type RenameTemplate =
  | "simple"
  | "case_archive"
  | "court_case_title"
  | "date_case_title"
  | "title_case_paren"
  | "index_title"
  | "original"
  | "custom";

export type SubdirMode = "none" | "case_number" | "court_and_case" | "date";

export type ItemStatus =
  | "pending"
  | "running"
  | "needs_identity"
  | "completed"
  | "partial"
  | "failed"
  | "browser_required";

export type ReceiptStatus =
  | "not_requested"
  | "confirmed"
  | "unconfirmed"
  | "unknown";

export interface LawyerProfile {
  id: string;
  name: string;
  mobileLast4: string;
  idLast4?: string | null;
}

export type AndroidStorageMode = "media_store_downloads" | "saf_tree";

export type DocSaveStatus = "saved" | "save_failed" | "missing";

export interface AppSettings {
  outputDir: string;
  createCaseSubdir: boolean;
  subdirMode: SubdirMode;
  renameTemplate: RenameTemplate;
  customTemplate: string;
  prefixIndex: boolean;
  matrixEnabled: boolean;
  requestDelayMs: number;
  profiles: LawyerProfile[];
  androidStorageMode?: AndroidStorageMode;
  androidTreeUri?: string | null;
  androidTreeLabel?: string | null;
}

export interface DownloadedDoc {
  rawTitle: string;
  fileName: string;
  savedPath: string;
  storageUri?: string | null;
  displayLocation?: string;
  stagingPath?: string | null;
  saveStatus?: DocSaveStatus;
  saveError?: string | null;
  sizeBytes: number;
  courtName?: string | null;
  documentId?: string | null;
  sourceKey?: string | null;
  format?: string | null;
}

export interface UnrecognizedBlockDiagnostic {
  rawText: string;
  reason: string;
  caseNumber?: string | null;
  courtName?: string | null;
}

export interface LegacyDocumentEntry {
  path: string;
  fileName: string;
  relativeDir: string;
  sizeBytes: number;
  inferredCaseNumber?: string | null;
  inferredCourtName?: string | null;
  needsCaseConfirmation: boolean;
  exportedBefore: boolean;
}

export interface LegacyExportResult {
  exported: number;
  skipped: number;
  errors: string[];
  exportedDocs: DownloadedDoc[];
}

export interface DocFailure {
  index: number;
  name?: string | null;
  reason: string;
  sourceKey?: string | null;
}

export interface QueueItem {
  id: string;
  rawText: string;
  platform: Platform;
  platformLabel: string;
  url: string;
  fingerprint: string;
  caseNumber?: string | null;
  courtName?: string | null;
  parties: string[];
  smsCode?: string | null;
  status: ItemStatus;
  statusMessage: string;
  stageLabel: string;
  receiptStatus: ReceiptStatus;
  identityAttemptsUsed: number;
  identityAttemptsLimit: number;
  documents: DownloadedDoc[];
  failures: DocFailure[];
  currentDocIndex: number;
  currentDocTotal: number;
  currentDocName?: string | null;
  currentTargetFileName?: string | null;
  bytesReceived: number;
  totalBytes?: number | null;
  speedBytesPerSec: number;
}

export interface FileProgress {
  taskId: string;
  docIndex: number;
  docTotal: number;
  docName: string;
  targetFileName?: string | null;
  stage: string;
  bytesReceived: number;
  totalBytes?: number | null;
  speedBytesPerSec: number;
}

export interface ParseBatchSummary {
  added: QueueItem[];
  skippedDuplicates: number;
  unrecognizedLines: number;
  unrecognizedBlocks: string[];
  blockDiagnostics?: UnrecognizedBlockDiagnostic[];
}

export interface QueueSnapshot {
  items: QueueItem[];
  isRunning: boolean;
  isPaused: boolean;
  totalCount: number;
  completedCount: number;
  failedCount: number;
  needsIdentityCount: number;
  pendingCount: number;
  browserRequiredCount: number;
  totalDownloadedDocs: number;
  totalDownloadedBytes: number;
}

export interface FallbackSessionSnapshot {
  taskId: string;
  label: string;
  url: string;
  caseNumber?: string | null;
  courtName?: string | null;
  platformLabel: string;
  smsCode?: string | null;
  capturedCount: number;
  lastMessage?: string | null;
  lastError?: string | null;
}

export interface ImportTaskDocsResult {
  imported: number;
  errors: string[];
}
