import { useState } from "react";
import { Dialog } from "../Dialog";

interface DemoDialogProps {
  native: boolean;
  onArchive: () => Promise<string>;
  onClose: () => void;
}

export function DemoDialog({ native, onArchive, onClose }: DemoDialogProps) {
  const [step, setStep] = useState<"import" | "verify" | "archive" | "done">("import");
  const [code, setCode] = useState("");
  const [result, setResult] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function archive() {
    setBusy(true);
    setError("");
    try {
      setResult(await onArchive());
      setStep("done");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog title="本机演示" onClose={() => { if (!busy) onClose(); }}>
      <div className="modal-header"><h2>本机演示</h2></div>
      <div className="modal-body">
        <p>体验从送达通知到保存文书的流程。演示使用虚构案件，全程离线。</p>
        {step === "import" && (
          <p>【示例人民法院】虚构演示案有一份文书待查收。演示码为 0421。</p>
        )}
        {step === "verify" && (
          <form onSubmit={(event) => {
            event.preventDefault();
            if (code.trim() === "0421") {
              setError("");
              setStep("archive");
            } else {
              setError("请输入通知中的演示码 0421。");
            }
          }}>
            <label className="input-label" htmlFor="demo-code">输入演示码 0421</label>
            <input id="demo-code" value={code} onChange={(event) => setCode(event.target.value)}
              inputMode="numeric" autoComplete="off" maxLength={4} />
            <button className="btn btn-primary" type="submit" disabled={code.length !== 4}>核验</button>
          </form>
        )}
        {step === "archive" && <p>核验已通过。演示文书为 TXT 格式{native ? "，将按当前设置归档。" : "，可保存到浏览器下载目录。"}</p>}
        {step === "done" && <p role="status">{native ? "已归档至 " : "已下载 "}{result}</p>}
        {error && <p className="error-inline" role="alert">{error}</p>}
      </div>
      <div className="modal-footer">
        <button className="btn" disabled={busy} onClick={onClose}>{step === "done" ? "完成" : "关闭"}</button>
        {step === "import" && <button className="btn btn-primary" onClick={() => setStep("verify")}>导入演示通知</button>}
        {step === "archive" && <button className="btn btn-primary" disabled={busy} onClick={() => void archive()}>
          {busy ? "正在归档…" : native ? "保存并归档" : "下载演示文书"}
        </button>}
      </div>
    </Dialog>
  );
}
