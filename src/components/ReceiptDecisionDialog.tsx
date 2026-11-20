import { useState } from "react";
import { Dialog } from "../Dialog";
import type { ReceiptPrompt } from "../types";

interface ReceiptDecisionDialogProps {
  prompt: ReceiptPrompt;
  onDecide: (submit: boolean, remember: boolean) => Promise<void>;
}

export function ReceiptDecisionDialog({ prompt, onDecide }: ReceiptDecisionDialogProps) {
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function decide(submit: boolean, rememberChoice: boolean) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await onDecide(submit, rememberChoice);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title="提交送达回执？" onClose={() => void decide(false, false)}>
      <div className="card-header">
        <h2 className="modal-title">提交送达回执？</h2>
      </div>
      <div className="modal-body">
        <p>{prompt.caseNumber || "案号待核对"} · {prompt.courtName || "全国统一送达"}</p>
        <p>已找到 {prompt.documentCount} 份文书。提交回执会向法院确认签收，送达状态以法院平台记录为准。</p>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={remember}
            disabled={busy}
            onChange={(event) => setRemember(event.currentTarget.checked)}
          />
          记住本次选择，下次不再询问
        </label>
        <p className="hint-text">此选择适用于全国统一送达，可在归档设置中改回每次询问。</p>
        {error && <p className="error-inline" role="alert">{error}</p>}
      </div>
      <div className="modal-footer">
        <button type="button" className="btn" autoFocus disabled={busy} onClick={() => void decide(false, remember)}>
          仅下载
        </button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void decide(true, remember)}>
          提交回执并下载
        </button>
      </div>
    </Dialog>
  );
}
