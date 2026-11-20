import React from "react";
import { Pencil, Search, X } from "lucide-react";
import { Dialog } from "../Dialog";
import type { AppSettings, LawyerProfile } from "../types";

interface ProfilesDialogProps {
  settings: AppSettings;
  desktop: boolean;
  profileSearch: string;
  filteredProfiles: LawyerProfile[];
  profileName: string;
  profileMobile4: string;
  profileId4: string;
  profileError: string | null;
  showBulkImport: boolean;
  bulkProfilesText: string;
  profileNameInputRef: React.RefObject<HTMLInputElement | null>;
  onSearchChange: (value: string) => void;
  onNameChange: (value: string) => void;
  onMobile4Change: (value: string) => void;
  onId4Change: (value: string) => void;
  onToggleBulkImport: () => void;
  onBulkTextChange: (value: string) => void;
  onSaveProfile: (e: React.FormEvent) => void;
  onBulkImport: () => void;
  onDeleteProfile: (profileId: string) => void;
  onClose: () => void;
}

export function ProfilesDialog({
  settings,
  desktop,
  profileSearch,
  filteredProfiles,
  profileName,
  profileMobile4,
  profileId4,
  profileError,
  showBulkImport,
  bulkProfilesText,
  profileNameInputRef,
  onSearchChange,
  onNameChange,
  onMobile4Change,
  onId4Change,
  onToggleBulkImport,
  onBulkTextChange,
  onSaveProfile,
  onBulkImport,
  onDeleteProfile,
  onClose,
}: ProfilesDialogProps) {
  return (
    <Dialog title="本次运行的尾号" onClose={onClose}>
      <p className="hint-text">仅在本次运行期间保留，退出应用后需要重新输入。</p>
      <div className="card-header" style={{ marginBottom: 8 }}>
        <h3 className="modal-title" style={{ margin: 0 }}>
          常用律师 / 接收人尾号
          <span className="count" style={{ marginLeft: 8 }}>
            {settings.profiles.length}
          </span>
        </h3>
        <button className="btn btn-sm" aria-label="关闭常用尾号" onClick={onClose}>
          <X size={13} />
        </button>
      </div>
      <p className="modal-desc">
        支持长期维护大量常用尾号，仅保存在本机配置目录，核验时支持搜索并一键填入。
      </p>

      {settings.profiles.length > 0 && (
        <>
          <div className="search-input-wrap" style={{ marginBottom: 8 }}>
            <Search size={13} />
            <input
              className="search-input"
              placeholder="搜索姓名、手机尾号或证件尾号…"
              value={profileSearch}
              onChange={(e) => onSearchChange(e.target.value)}
            />
            {profileSearch && (
              <button className="link-btn" onClick={() => onSearchChange("")}>
                清空
              </button>
            )}
          </div>

          <div className="profile-scroll-list">
            {filteredProfiles.map((p) => (
              <div key={p.id} className="doc-chip">
                <span className="doc-chip-name">
                  <strong>{p.name}</strong> · 手机{" "}
                  <span className="mono-tail">{p.mobileLast4}</span>
                  {p.idLast4 ? (
                    <>
                      {" "}
                      · 证件 <span className="mono-tail">{p.idLast4}</span>
                    </>
                  ) : null}
                </span>
                <div className="doc-chip-right">
                  <button
                    type="button"
                    className="link-btn"
                    title="填入下方编辑"
                    onClick={() => {
                      onNameChange(p.name);
                      onMobile4Change(p.mobileLast4);
                      onId4Change(p.idLast4 || "");
                      profileNameInputRef.current?.focus();
                    }}
                  >
                    <Pencil size={11} style={{ verticalAlign: "-1px", marginRight: 2 }} />
                    编辑
                  </button>
                  <button
                    type="button"
                    className="link-btn"
                    onClick={() => onDeleteProfile(p.id)}
                  >
                    删除
                  </button>
                </div>
              </div>
            ))}
            {filteredProfiles.length === 0 && (
              <div
                className="hint-text"
                style={{ padding: "12px 4px", textAlign: "center" }}
              >
                未找到匹配的尾号记录
              </div>
            )}
          </div>
        </>
      )}

      {desktop ? (
        <>
          <div className="profile-form-head">
            <span className="input-label" style={{ margin: 0 }}>
              新增或更新尾号（同名自动更新）
            </span>
            <button
              type="button"
              className="link-btn"
              onClick={onToggleBulkImport}
            >
              {showBulkImport ? "切回逐条录入" : "批量粘贴导入"}
            </button>
          </div>

          {showBulkImport ? (
            <div className="modal-form">
              <textarea
                className="sms-textarea"
                style={{
                  minHeight: 96,
                  border: "1px solid var(--border)",
                  borderRadius: 7,
                  padding: "8px 10px",
                  background: "#fcfcf7",
                }}
                placeholder={
                  "每行一条：姓名 手机后四位 [证件后四位]\n例如：\n张律师 1234\n李律师,5678,9012"
                }
                value={bulkProfilesText}
                onChange={(e) => onBulkTextChange(e.target.value)}
              />
              {profileError && <div className="error-inline">{profileError}</div>}
              <div className="modal-footer" style={{ marginTop: 4 }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={!bulkProfilesText.trim()}
                  onClick={onBulkImport}
                >
                  批量保存
                </button>
              </div>
            </div>
          ) : (
            <form className="modal-form" onSubmit={onSaveProfile}>
              <div className="profile-inline-inputs">
                <input
                  ref={profileNameInputRef}
                  className="text-input"
                  placeholder="姓名（如：张律师）"
                  value={profileName}
                  onChange={(e) => onNameChange(e.target.value)}
                />
                <input
                  className="text-input"
                  maxLength={4}
                  placeholder="手机后4位*"
                  value={profileMobile4}
                  onChange={(e) => onMobile4Change(e.target.value)}
                />
                <input
                  className="text-input"
                  maxLength={4}
                  placeholder="证件后4位"
                  value={profileId4}
                  onChange={(e) => onId4Change(e.target.value)}
                />
              </div>
              {profileError && <div className="error-inline">{profileError}</div>}
              <div className="modal-footer" style={{ marginTop: 4 }}>
                <button type="submit" className="btn btn-primary">
                  保存并继续添加
                </button>
              </div>
            </form>
          )}
        </>
      ) : (
        <p className="preview-note">请在桌面版管理接收人尾号。</p>
      )}
    </Dialog>
  );
}
