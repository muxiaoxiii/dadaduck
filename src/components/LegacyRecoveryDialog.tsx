import { X } from "lucide-react";
import { Dialog } from "../Dialog";
import type { LegacyDocumentEntry } from "../types";
import { formatBytes } from "../utils/format";

interface LegacyRecoveryDialogProps {
  legacyDocs: LegacyDocumentEntry[];
  legacyCaseOverrides: Record<string, string>;
  legacySelected: Record<string, boolean>;
  legacyScanning: boolean;
  legacyExporting: boolean;
  legacySummaryMsg: string | null;
  onSetSelected: (next: Record<string, boolean>) => void;
  onToggleOne: (path: string, checked: boolean) => void;
  onChangeCaseOverride: (path: string, value: string) => void;
  onOpenLegacyDoc: (path: string) => void;
  onExportBatch: (targetPaths?: string[]) => void;
  onClose: () => void;
}

export function LegacyRecoveryDialog({
  legacyDocs,
  legacyCaseOverrides,
  legacySelected,
  legacyScanning,
  legacyExporting,
  legacySummaryMsg,
  onSetSelected,
  onToggleOne,
  onChangeCaseOverride,
  onOpenLegacyDoc,
  onExportBatch,
  onClose,
}: LegacyRecoveryDialogProps) {
  return (
    <Dialog
      title="应用内已有文书"
      onClose={() => {
        if (!legacyExporting) onClose();
      }}
    >
      <div className="card-header">
        <h2 className="modal-title">应用内已有文书</h2>
        <button
          aria-label="关闭已有文书窗口"
          disabled={legacyExporting}
          className="icon-btn"
          onClick={onClose}
        >
          <X size={16} />
        </button>
      </div>
      <p className="modal-desc">
        扫描小达鸭旧版私有目录里的送达文书，支持预览并安全复制到当前下载目录（校验字节完整、同名自动编号、保留原文件）。
      </p>

      {legacyScanning ? (
        <p className="hint-text">正在扫描应用内目录…</p>
      ) : legacyDocs.length === 0 ? (
        <div className="empty-state" style={{ padding: "22px 12px" }}>
          <strong>未发现待导出的旧目录文书</strong>
          <p>旧版私密目录中没有残留文书文件。</p>
        </div>
      ) : (
        <>
          <div className="parse-feedback-summary">
            <span>共扫描到 {legacyDocs.length} 份文书</span>
            <button
              type="button"
              className="link-btn"
              onClick={() => {
                const allSelected = legacyDocs.every((d) => legacySelected[d.path]);
                const next: Record<string, boolean> = {};
                for (const d of legacyDocs) {
                  next[d.path] = !allSelected;
                }
                onSetSelected(next);
              }}
            >
              {legacyDocs.every((d) => legacySelected[d.path]) ? "取消全选" : "全选"}
            </button>
          </div>

          <div className="legacy-list">
            {legacyDocs.map((entry) => (
              <div key={entry.path} className="legacy-card">
                <div className="legacy-card-top">
                  <input
                    type="checkbox"
                    checked={!!legacySelected[entry.path]}
                    onChange={(e) => onToggleOne(entry.path, e.target.checked)}
                  />
                  <div className="legacy-card-info">
                    <div className="legacy-card-name">{entry.fileName}</div>
                    <div className="legacy-card-sub">
                      目录：{entry.relativeDir} · {formatBytes(entry.sizeBytes)}
                      {entry.exportedBefore ? " · 已导出过" : ""}
                    </div>
                  </div>
                  <button
                    type="button"
                    className="doc-action-btn"
                    onClick={() => onOpenLegacyDoc(entry.path)}
                  >
                    打开
                  </button>
                  <button
                    type="button"
                    className="doc-action-btn primary"
                    disabled={legacyExporting}
                    onClick={() => onExportBatch([entry.path])}
                  >
                    导出
                  </button>
                </div>

                <div className="case-edit-inline" style={{ marginTop: 2 }}>
                  {entry.needsCaseConfirmation && !legacyCaseOverrides[entry.path] && (
                    <span className="case-pending-pill">案号待确认</span>
                  )}
                  <input
                    className="text-input"
                    placeholder="确认或补填归属案号（选填，用于建文件夹）"
                    value={legacyCaseOverrides[entry.path] ?? ""}
                    onChange={(e) => onChangeCaseOverride(entry.path, e.target.value)}
                  />
                </div>
              </div>
            ))}
          </div>
        </>
      )}

      {legacySummaryMsg && (
        <div className="toast-banner" style={{ marginTop: 8 }}>
          <span>{legacySummaryMsg}</span>
        </div>
      )}

      <div className="modal-footer">
        <button className="btn" disabled={legacyExporting} onClick={onClose}>
          关闭
        </button>
        {legacyDocs.length > 0 && (
          <button
            className="btn btn-primary"
            disabled={
              legacyExporting || !legacyDocs.some((d) => legacySelected[d.path])
            }
            onClick={() => onExportBatch()}
          >
            {legacyExporting ? "正在复制与校验…" : "批量导出到下载目录"}
          </button>
        )}
      </div>
    </Dialog>
  );
}
