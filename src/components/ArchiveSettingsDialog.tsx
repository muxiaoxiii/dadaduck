import { FolderOpen, X } from "lucide-react";
import { Dialog } from "../Dialog";
import type { AppSettings, RenameTemplate, SubdirMode } from "../types";
import {
  buildFilenamePreview,
  PLACEHOLDER_TOKENS,
  RENAME_PRESETS,
  SUBDIR_OPTIONS,
} from "../utils/format";

interface ArchiveSettingsDialogProps {
  draft: AppSettings;
  desktop: boolean;
  isMobile: boolean;
  busy: boolean;
  archiveError: string;
  archivePreviewPath: string;
  onChangeDraft: (next: AppSettings) => void;
  onChooseFolder: () => void;
  onUseAndroidDownloads: () => void;
  onPickAndroidDir: () => void;
  onSave: () => void;
  onClose: () => void;
}

export function ArchiveSettingsDialog({
  draft,
  desktop,
  isMobile,
  busy,
  archiveError,
  archivePreviewPath,
  onChangeDraft,
  onChooseFolder,
  onUseAndroidDownloads,
  onPickAndroidDir,
  onSave,
  onClose,
}: ArchiveSettingsDialogProps) {
  return (
    <Dialog title="归档设置" onClose={() => { if (!busy) onClose(); }}>
      <div className="card-header">
        <h2 className="modal-title">归档与命名设置</h2>
        <button
          aria-label="关闭归档设置"
          disabled={busy}
          className="icon-btn"
          onClick={onClose}
        >
          <X size={16} />
        </button>
      </div>
      <div className="settings-grid">
        <div>
          <div className="input-label">文书保存至</div>
          <div className="folder-row">
            <span className="folder-path">
              {isMobile
                ? draft.androidStorageMode === "saf_tree"
                  ? draft.androidTreeLabel || draft.outputDir
                  : "手机系统目录：下载 / DadaDuck送达文书"
                : draft.outputDir}
            </span>
            {desktop && !isMobile && (
              <button className="btn" onClick={onChooseFolder} disabled={busy}>
                <FolderOpen size={14} />
                更改目录
              </button>
            )}
          </div>
          {desktop && isMobile && (
            <div className="storage-choice-group">
              <button
                type="button"
                className={`btn btn-sm ${
                  draft.androidStorageMode !== "saf_tree" ? "btn-primary" : ""
                }`}
                disabled={busy}
                onClick={onUseAndroidDownloads}
              >
                使用手机下载目录（推荐）
              </button>
              <button
                type="button"
                className={`btn btn-sm ${
                  draft.androidStorageMode === "saf_tree" ? "btn-primary" : ""
                }`}
                disabled={busy}
                onClick={onPickAndroidDir}
              >
                <FolderOpen size={13} />
                选择其他文件夹
              </button>
            </div>
          )}
          {isMobile && (
            <div className="hint-text" style={{ marginTop: 6 }}>
              默认写入系统可见的「下载 / DadaDuck送达文书」，重启、覆盖升级或从系统文件管理都能直接访问。
            </div>
          )}
        </div>

        <label className="field-inline">
          <span>目录归类</span>
          <select
            className="select-input"
            value={draft.subdirMode}
            onChange={(e) =>
              onChangeDraft({
                ...draft,
                subdirMode: e.target.value as SubdirMode,
                createCaseSubdir: e.target.value !== "none",
              })
            }
          >
            {SUBDIR_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>

        <label className="field-inline">
          <span>命名格式</span>
          <select
            className="select-input"
            value={draft.renameTemplate}
            onChange={(e) =>
              onChangeDraft({
                ...draft,
                renameTemplate: e.target.value as RenameTemplate,
              })
            }
          >
            {RENAME_PRESETS.map((preset) => (
              <option key={preset.value} value={preset.value}>
                {preset.label}
              </option>
            ))}
          </select>
        </label>

        {draft.renameTemplate === "custom" && (
          <div className="custom-template-box">
            <input
              className="text-input"
              style={{ width: "100%" }}
              aria-label="自定义命名模板"
              value={draft.customTemplate}
              placeholder="{case_no}_{title}_{date}"
              onChange={(e) =>
                onChangeDraft({ ...draft, customTemplate: e.target.value })
              }
            />
            <div className="token-chips">
              {PLACEHOLDER_TOKENS.map((t) => (
                <button
                  key={t.token}
                  type="button"
                  className="token-chip"
                  onClick={() =>
                    onChangeDraft({
                      ...draft,
                      customTemplate: `${draft.customTemplate}${
                        draft.customTemplate && !draft.customTemplate.endsWith("_")
                          ? "_"
                          : ""
                      }${t.token}`,
                    })
                  }
                >
                  {t.label}
                </button>
              ))}
            </div>
          </div>
        )}

        <div className="rename-preview-box">
          <span className="hint-text">实际归档路径预览：</span>
          <code>{archivePreviewPath || buildFilenamePreview(draft)}</code>
        </div>

        {draft.renameTemplate !== "index_title" && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={draft.prefixIndex}
              onChange={(e) =>
                onChangeDraft({ ...draft, prefixIndex: e.target.checked })
              }
            />
            多份文书时自动加两位序号前缀（01_、02_）
          </label>
        )}

        {archiveError && (
          <div className="error-inline" role="alert">
            {archiveError}
          </div>
        )}
      </div>
      <div className="modal-footer">
        <button className="btn" disabled={busy} onClick={onClose}>
          取消
        </button>
        <button className="btn btn-primary" disabled={busy} onClick={onSave}>
          保存设置
        </button>
      </div>
    </Dialog>
  );
}
