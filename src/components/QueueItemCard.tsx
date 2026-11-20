import {
  AlertCircle,
  Copy,
  ExternalLink,
  FileText,
  FolderUp,
  Globe,
  KeyRound,
  RotateCcw,
  Share2,
  Trash2,
} from "lucide-react";
import type { DownloadedDoc, QueueItem } from "../types";
import {
  countSavedDocs,
  formatBytes,
  formatEta,
  formatSpeed,
  savedDocsBytes,
  statusLabel,
} from "../utils/format";

interface QueueItemCardProps {
  item: QueueItem;
  index: number;
  isMobile: boolean;
  supportsEmbeddedFallback: boolean;
  activeFallbackTaskId?: string | null;
  isEditingCase: boolean;
  editingCaseValue: string;
  onStartEditCase: (taskId: string, currentCase: string) => void;
  onChangeEditCase: (value: string) => void;
  onSaveEditCase: (taskId: string) => void;
  onCancelEditCase: () => void;
  expandedDocs: Record<string, boolean>;
  onToggleExpandDoc: (key: string) => void;
  docActionErrors: Record<string, { message: string; canShare: boolean }>;
  confirmDeleteKey: string | null;
  onSetConfirmDeleteKey: (key: string | null) => void;
  onCopySmsCode: (code: string) => void;
  onOpenVerify: (taskId: string) => void;
  onOpenFallback: (taskId: string) => void;
  onImportLocalDocs: (taskId: string) => void;
  onOpenBrowser: (url: string) => void;
  onRequestSigning: (taskId: string) => void;
  onRemoveItem: (taskId: string) => void;
  onOpenDoc: (taskId: string, docIdx: number, reveal: boolean) => void;
  onShareDoc: (taskId: string, docIdx: number) => void;
  onRetrySaveDoc: (taskId: string, docIdx: number) => void;
  onConfirmDeleteDocFile: (taskId: string, docIdx: number) => void;
}

export function QueueItemCard({
  item,
  index,
  isMobile,
  supportsEmbeddedFallback,
  activeFallbackTaskId,
  isEditingCase,
  editingCaseValue,
  onStartEditCase,
  onChangeEditCase,
  onSaveEditCase,
  onCancelEditCase,
  expandedDocs,
  onToggleExpandDoc,
  docActionErrors,
  confirmDeleteKey,
  onSetConfirmDeleteKey,
  onCopySmsCode,
  onOpenVerify,
  onOpenFallback,
  onImportLocalDocs,
  onOpenBrowser,
  onRequestSigning,
  onRemoveItem,
  onOpenDoc,
  onShareDoc,
  onRetrySaveDoc,
  onConfirmDeleteDocFile,
}: QueueItemCardProps) {
  const docPct =
    item.totalBytes && item.totalBytes > 0
      ? Math.min(100, Math.round((item.bytesReceived / item.totalBytes) * 100))
      : null;
  const itemTotalBytes = savedDocsBytes(item.documents);
  const itemSavedCount = countSavedDocs(item.documents);

  return (
    <div className={`queue-item ${item.status === "running" ? "running" : ""}`}>
      <div className="queue-item-top">
        <div className="item-meta">
          <div className="item-title-row">
            <span className="queue-seq">#{index + 1}</span>
            <span className="platform-tag">{item.platformLabel}</span>
            {item.caseNumber ? (
              <span className="case-title">{item.caseNumber}</span>
            ) : (
              <span className="case-pending-pill">
                <AlertCircle size={11} />
                案号待确认
              </span>
            )}
            {item.courtName && <span className="court-sub">{item.courtName}</span>}
            {item.smsCode && (
              <button
                type="button"
                className="sms-code-pill"
                onClick={() => onCopySmsCode(item.smsCode!)}
                title="点击复制短信提取码/验证码"
              >
                <Copy size={11} />
                复制提取码
              </button>
            )}
            {item.status !== "running" && !isEditingCase && (
              <button
                className="link-btn"
                onClick={() => onStartEditCase(item.id, item.caseNumber || "")}
              >
                {item.caseNumber ? "改案号" : "补填案号"}
              </button>
            )}
          </div>

          {isEditingCase && (
            <form
              className="case-edit-inline"
              onSubmit={(e) => {
                e.preventDefault();
                onSaveEditCase(item.id);
              }}
            >
              <input
                className="text-input"
                placeholder="如：(2026) 示73行初90001号"
                value={editingCaseValue}
                onChange={(e) => onChangeEditCase(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.preventDefault();
                    onCancelEditCase();
                  }
                }}
                autoFocus
              />
              <button type="submit" className="btn btn-primary btn-sm">
                确认案号
              </button>
              <button type="button" className="btn btn-sm" onClick={onCancelEditCase}>
                取消
              </button>
            </form>
          )}

          <div className="item-status-row">
            <span className={`status-pill pill-${item.status}`}>
              {item.status === "running" && item.stageLabel
                ? item.stageLabel
                : statusLabel(item.status)}
            </span>
            <span>{item.statusMessage}</span>
            {itemSavedCount > 0 && item.status !== "running" && (
              <span className="hint-text">· 共 {formatBytes(itemTotalBytes)}</span>
            )}
          </div>
        </div>

        <div className="item-actions">
          {item.status === "needs_identity" && (
            <button
              className="btn btn-warm btn-sm"
              onClick={() => onOpenVerify(item.id)}
            >
              <KeyRound size={13} />
              输入后四位
            </button>
          )}

          {(item.status === "browser_required" ||
            item.status === "failed" ||
            item.status === "partial" ||
            activeFallbackTaskId === item.id) &&
            item.url &&
            (supportsEmbeddedFallback && item.url.startsWith("https://") ? (
              <>
                <button
                  className={`btn btn-sm ${
                    activeFallbackTaskId === item.id ? "btn-primary" : "btn-warm"
                  }`}
                  onClick={() => onOpenFallback(item.id)}
                  title="在隔离的内置保底窗口中完成验证并自动拦截下载、校验与归档"
                >
                  <Globe size={13} />
                  {activeFallbackTaskId === item.id ? "保底窗口运行中" : "内置窗口取件"}
                </button>
                <button
                  className="btn btn-sm"
                  onClick={() => onImportLocalDocs(item.id)}
                  title="导入已在其他浏览器下载的文件，按本案模板检验与归档（保留原文件）"
                >
                  <FolderUp size={13} />
                  导入本地文件
                </button>
                <button
                  className="btn btn-sm"
                  onClick={() => onOpenBrowser(item.url)}
                  title="在系统默认浏览器中打开原链接"
                >
                  <ExternalLink size={13} />
                  系统浏览器
                </button>
              </>
            ) : (
              <button
                className="btn btn-sm"
                onClick={() => onOpenBrowser(item.url)}
                title="在系统默认浏览器中打开原链接"
              >
                <ExternalLink size={13} />
                浏览器打开
              </button>
            ))}

          {item.url && item.status !== "running" && (
            <button className="btn btn-sm" onClick={() => onRequestSigning(item.id)}>
              <ExternalLink size={13} />
              前往法院签收
            </button>
          )}

          {item.status !== "running" && (
            <button
              className="btn btn-sm"
              title="从队列移除"
              onClick={() => onRemoveItem(item.id)}
            >
              <Trash2 size={13} />
            </button>
          )}
        </div>
      </div>

      {item.status === "running" && (
        <div className="doc-progress-box">
          <div className="doc-progress-meta">
            <span>
              {item.currentDocTotal > 0
                ? `第 ${item.currentDocIndex}/${item.currentDocTotal} 份 · ${
                    item.currentDocName || "文书"
                  }`
                : item.statusMessage}
            </span>
            <span>
              {item.currentDocTotal > 0 ? (
                <>
                  {formatBytes(item.bytesReceived)}
                  {item.totalBytes
                    ? ` / ${formatBytes(item.totalBytes)} (${docPct}%)`
                    : ""}
                </>
              ) : (
                item.stageLabel || "处理中"
              )}
            </span>
          </div>

          {item.currentTargetFileName && (
            <div className="doc-target-hint">
              → 保存为：{item.currentTargetFileName}
            </div>
          )}

          <div className="progress-track">
            <div
              className={`progress-fill ${
                item.currentDocTotal === 0 || docPct === null ? "indeterminate" : ""
              }`}
              style={{
                width:
                  item.currentDocTotal > 0 && docPct !== null ? `${docPct}%` : "40%",
              }}
            />
          </div>

          {item.currentDocTotal > 0 && (
            <div className="doc-telemetry-row">
              <span>速率：{formatSpeed(item.speedBytesPerSec)}</span>
              <span>
                预计剩余：
                {formatEta(
                  item.bytesReceived,
                  item.totalBytes,
                  item.speedBytesPerSec
                )}
              </span>
              <span>
                本案已落盘：{itemSavedCount}/{item.currentDocTotal} 份
              </span>
            </div>
          )}
        </div>
      )}

      {item.receiptStatus !== "not_requested" && (
        <p className="hint-text" style={{ margin: "6px 0 0" }}>
          签收回执 ·{" "}
          {
            ({
              confirmed: "法院已确认",
              unconfirmed: "法院未确认，可在法院页面查看",
              unknown: "待到法院页面核实",
            })[item.receiptStatus]
          }
        </p>
      )}

      {item.failures.length > 0 && (
        <details className="doc-list">
          <summary>{item.failures.length} 份文书下载失败</summary>
          {item.failures.map((f) => (
            <p key={f.index}>
              {f.name || "文书"} · {f.reason}
            </p>
          ))}
        </details>
      )}

      {item.documents.length > 0 && (
        <div className="doc-list">
          {item.documents.map((doc: DownloadedDoc, docIdx) => {
            const key = `${item.id}:${docIdx}`;
            const isExpanded = !!expandedDocs[key];
            const localErr = docActionErrors[key];
            const status = doc.saveStatus || "saved";
            const isConfirmDelete = confirmDeleteKey === key;
            const locationText = doc.displayLocation || doc.savedPath;

            if (!isMobile && status !== "save_failed") {
              return (
                <div key={docIdx}>
                  <div className="doc-chip">
                    <span
                      className="doc-chip-name"
                      title={locationText || doc.fileName}
                    >
                      <FileText
                        size={12}
                        style={{ verticalAlign: "-2px", marginRight: 5 }}
                      />
                      {doc.fileName}
                    </span>
                    <div className="doc-chip-right">
                      <span>
                        {status === "missing"
                          ? "文件已移除"
                          : formatBytes(doc.sizeBytes)}
                      </span>
                      {status === "saved" && (
                        <>
                          <button
                            className="link-btn"
                            onClick={() => onOpenDoc(item.id, docIdx, false)}
                          >
                            打开
                          </button>
                          <button
                            className="link-btn"
                            onClick={() => onOpenDoc(item.id, docIdx, true)}
                          >
                            定位
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                  {localErr && (
                    <div
                      className="doc-local-error"
                      role="alert"
                      style={{ marginTop: 4 }}
                    >
                      <span>{localErr.message}</span>
                    </div>
                  )}
                </div>
              );
            }

            return (
              <div key={docIdx} className="doc-item-card">
                <button
                  type="button"
                  className="doc-item-row1"
                  title="点击展开完整文件名与保存位置"
                  onClick={() => onToggleExpandDoc(key)}
                >
                  <FileText size={14} style={{ flexShrink: 0, marginTop: 2 }} />
                  <span
                    className={`doc-item-filename ${isExpanded ? "expanded" : ""}`}
                  >
                    {doc.fileName}
                  </span>
                </button>

                {(isExpanded || status === "saved") && locationText && (
                  <div className="doc-item-path">保存位置：{locationText}</div>
                )}

                <div className="doc-item-row2">
                  <div className="doc-item-meta">
                    <span>{formatBytes(doc.sizeBytes)}</span>
                    <span className={`doc-save-tag ${status}`}>
                      {status === "saved"
                        ? "已保存"
                        : status === "save_failed"
                          ? "保存待重试"
                          : "文件已移除"}
                    </span>
                  </div>

                  <div className="doc-item-actions">
                    {status === "save_failed" ? (
                      <button
                        type="button"
                        className="doc-action-btn primary"
                        onClick={() => onRetrySaveDoc(item.id, docIdx)}
                      >
                        <RotateCcw size={12} />
                        重试保存
                      </button>
                    ) : status === "saved" ? (
                      <>
                        <button
                          type="button"
                          className="doc-action-btn primary"
                          onClick={() => onOpenDoc(item.id, docIdx, false)}
                        >
                          打开
                        </button>
                        {isMobile ? (
                          <button
                            type="button"
                            className="doc-action-btn"
                            onClick={() => onShareDoc(item.id, docIdx)}
                          >
                            <Share2 size={12} />
                            分享
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="doc-action-btn"
                            onClick={() => onOpenDoc(item.id, docIdx, true)}
                          >
                            定位
                          </button>
                        )}
                        {isConfirmDelete ? (
                          <>
                            <button
                              type="button"
                              className="doc-action-btn danger"
                              onClick={() => onConfirmDeleteDocFile(item.id, docIdx)}
                            >
                              确认删文件
                            </button>
                            <button
                              type="button"
                              className="doc-action-btn"
                              onClick={() => onSetConfirmDeleteKey(null)}
                            >
                              取消
                            </button>
                          </>
                        ) : (
                          <button
                            type="button"
                            className="doc-action-btn danger"
                            title="仅删除手机/磁盘上的此文书文件（保留队列记录）"
                            onClick={() => onSetConfirmDeleteKey(key)}
                          >
                            删文件
                          </button>
                        )}
                      </>
                    ) : null}
                  </div>
                </div>

                {localErr && (
                  <div className="doc-local-error" role="alert">
                    <span>{localErr.message}</span>
                    {localErr.canShare && (
                      <button
                        type="button"
                        className="doc-action-btn primary"
                        onClick={() => onShareDoc(item.id, docIdx)}
                      >
                        <Share2 size={12} />
                        分享 / 用其他应用打开
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
