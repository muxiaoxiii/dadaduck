import React from "react";
import { CheckCircle2, ExternalLink, Globe, Search } from "lucide-react";
import { Dialog } from "../Dialog";
import type { AppSettings, LawyerProfile, QueueItem } from "../types";

interface VerifyIdentityDialogProps {
  verifyTarget: QueueItem;
  settings: AppSettings | null;
  verifyMethod: "sjhm" | "zjhm";
  verifyTail: string;
  verifyProfileSearch: string;
  verifyError: string | null;
  verifySubmitting: boolean;
  filteredVerifyProfiles: LawyerProfile[];
  onMethodChange: (method: "sjhm" | "zjhm") => void;
  onTailChange: (tail: string) => void;
  onProfileSearchChange: (search: string) => void;
  onSubmit: (e: React.FormEvent) => void;
  supportsEmbeddedFallback?: boolean;
  onOpenFallback?: (taskId: string) => void;
  onOpenBrowser: (url: string) => void;
  onClose: () => void;
}

export function VerifyIdentityDialog({
  verifyTarget,
  settings,
  verifyMethod,
  verifyTail,
  verifyProfileSearch,
  verifyError,
  verifySubmitting,
  filteredVerifyProfiles,
  onMethodChange,
  onTailChange,
  onProfileSearchChange,
  onSubmit,
  supportsEmbeddedFallback = false,
  onOpenFallback,
  onOpenBrowser,
  onClose,
}: VerifyIdentityDialogProps) {
  return (
    <Dialog
      title="身份核验"
      onClose={() => {
        if (!verifySubmitting) onClose();
      }}
    >
      <h3 className="modal-title">输入身份核验后四位</h3>
      <p className="modal-desc">
        {verifyTarget.caseNumber || verifyTarget.courtName || "该送达链接"}
        要求核验接收人信息（剩余尝试{" "}
        {verifyTarget.identityAttemptsLimit - verifyTarget.identityAttemptsUsed} 次）。
      </p>

      <form className="modal-form" onSubmit={onSubmit}>
        {settings && settings.profiles.length > 0 && (
          <div>
            <div className="verify-profile-head">
              <span className="hint-text">
                快速填入本机常用尾号（共 {settings.profiles.length} 个）：
              </span>
              {settings.profiles.length > 6 && (
                <div className="search-input-wrap compact-search">
                  <Search size={12} />
                  <input
                    className="search-input"
                    placeholder="筛选姓名/尾号…"
                    value={verifyProfileSearch}
                    onChange={(e) => onProfileSearchChange(e.target.value)}
                  />
                </div>
              )}
            </div>
            <div className="profile-chip-list scrollable-chips">
              {filteredVerifyProfiles.map((p) => {
                const tailText = verifyMethod === "sjhm" ? p.mobileLast4 : p.idLast4;
                return (
                  <button
                    key={p.id}
                    disabled={verifySubmitting || (verifyMethod === "zjhm" && !p.idLast4)}
                    type="button"
                    className="btn btn-sm"
                    onClick={() => onTailChange(tailText || "")}
                  >
                    <CheckCircle2 size={12} />
                    {p.name}
                    {tailText ? ` · ${tailText}` : ""}
                  </button>
                );
              })}
              {filteredVerifyProfiles.length === 0 && (
                <span className="hint-text">无匹配尾号</span>
              )}
            </div>
          </div>
        )}

        <label className="field-inline">
          <span>核验方式</span>
          <select
            className="select-input"
            value={verifyMethod}
            onChange={(e) => {
              onMethodChange(e.target.value as "sjhm" | "zjhm");
              onTailChange("");
            }}
          >
            <option value="sjhm">手机号后四位</option>
            <option value="zjhm">证件号后四位</option>
          </select>
        </label>

        <input
          className="text-input"
          maxLength={4}
          required
          aria-label="核验后四位"
          inputMode={verifyMethod === "sjhm" ? "numeric" : "text"}
          pattern={verifyMethod === "sjhm" ? "[0-9]{4}" : "[A-Za-z0-9]{4}"}
          placeholder={
            verifyMethod === "sjhm" ? "请输入手机号码后四位" : "请输入证件号码后四位"
          }
          value={verifyTail}
          onChange={(e) => onTailChange(e.target.value.replace(/\s+/g, ""))}
          autoFocus
        />

        {verifyError && <div className="error-inline">{verifyError}</div>}

        <div className="modal-footer">
          {supportsEmbeddedFallback && onOpenFallback ? (
            <button
              type="button"
              className="btn"
              style={{ marginRight: "auto" }}
              disabled={verifySubmitting}
              onClick={() => onOpenFallback(verifyTarget.id)}
              title="改用隔离内置浏览器打开页面并手动完成核验与下载"
            >
              <Globe size={13} />
              内置窗口取件
            </button>
          ) : (
            verifyTarget.url && (
              <button
                type="button"
                className="btn"
                style={{ marginRight: "auto" }}
                disabled={verifySubmitting}
                onClick={() => onOpenBrowser(verifyTarget.url)}
              >
                <ExternalLink size={13} />
                浏览器打开原链接
              </button>
            )
          )}
          <button
            type="button"
            className="btn"
            onClick={onClose}
            disabled={verifySubmitting}
          >
            取消
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={
              verifySubmitting ||
              verifyTarget.status !== "needs_identity" ||
              verifyTarget.identityAttemptsUsed >= verifyTarget.identityAttemptsLimit
            }
          >
            {verifySubmitting ? "核验中..." : "提交核验并下载"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
