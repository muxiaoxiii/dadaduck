import { useEffect, useState } from "react";
import { AlertCircle, ArrowUpRight, Globe, KeyRound, Search, X } from "lucide-react";
import type { FallbackSessionSnapshot } from "../types";

interface FallbackControlBarProps {
  session: FallbackSessionSnapshot;
  busy?: boolean;
  onFillCode: (code: string) => void | Promise<void>;
  onScanDocs: () => void | Promise<void>;
  onFocusWindow: () => void | Promise<void>;
  onCloseWindow: () => void | Promise<void>;
}

export function FallbackControlBar({
  session,
  busy = false,
  onFillCode,
  onScanDocs,
  onFocusWindow,
  onCloseWindow,
}: FallbackControlBarProps) {
  const [codeInput, setCodeInput] = useState(session.smsCode ?? "");

  useEffect(() => {
    setCodeInput(session.smsCode ?? "");
  }, [session.taskId, session.smsCode]);

  const title = session.caseNumber || session.label;

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = codeInput.trim();
    if (!trimmed || busy) return;
    void onFillCode(trimmed);
  }

  return (
    <div className="fallback-bar" role="region" aria-label="前端保底取件控制台">
      <div className="fallback-bar-head">
        <div className="fallback-bar-title">
          <Globe size={15} />
          <span className="fallback-bar-badge">保底取件窗口运行中</span>
          <strong className="fallback-bar-case">{title}</strong>
          {session.courtName && <span className="fallback-bar-sub">{session.courtName}</span>}
          <span className="fallback-bar-sub">{session.platformLabel}</span>
          <span className="fallback-bar-count">已拦截 {session.capturedCount} 份</span>
        </div>
        <div className="fallback-bar-actions">
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy}
            onClick={() => void onScanDocs()}
            title="扫描保底窗口页面中可下载的文书链接并逐个触发拦截"
          >
            <Search size={13} />
            扫描页面文书
          </button>
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy}
            onClick={() => void onFocusWindow()}
            title="将内置保底取件窗口置前显示"
          >
            <ArrowUpRight size={13} />
            前置窗口
          </button>
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy}
            onClick={() => void onCloseWindow()}
            title="结束本次保底取件并关闭内置窗口"
          >
            <X size={13} />
            完成并关闭
          </button>
        </div>
      </div>

      <form className="fallback-bar-form" onSubmit={handleSubmit}>
        <label className="fallback-bar-label" htmlFor="fallback-code-input">
          <KeyRound size={13} />
          验证码/提取码
        </label>
        <input
          id="fallback-code-input"
          type="password"
          className="fallback-bar-input"
          placeholder="输入页面验证码或提取码"
          value={codeInput}
          onChange={(e) => setCodeInput(e.target.value)}
          disabled={busy}
          autoComplete="off"
        />
        <button
          type="submit"
          className="btn btn-sm btn-primary"
          disabled={busy || !codeInput.trim()}
        >
          填入页面框
        </button>
        {session.smsCode && (
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy}
            onClick={() => {
              const code = session.smsCode || "";
              if (code) {
                void onFillCode(code);
              }
            }}
            title="将短信中提取的验证码重新填入页面输入框"
          >
            填入短信提取码
          </button>
        )}
        <span className="fallback-bar-hint">
          请核对页面和输入框后再提交。图形验证码可在法院页面直接输入。
        </span>
      </form>

      {session.lastError ? (
        <div className="fallback-bar-msg fallback-bar-msg--error">
          <AlertCircle size={13} />
          <span>{session.lastError}</span>
        </div>
      ) : session.lastMessage ? (
        <div className="fallback-bar-msg">{session.lastMessage}</div>
      ) : null}
    </div>
  );
}
