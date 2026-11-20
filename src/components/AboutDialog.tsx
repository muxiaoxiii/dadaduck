import { useState, type MouseEvent } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { X } from "lucide-react";
import { version } from "../../package.json";
import { Dialog } from "../Dialog";

const projectUrl = "https://github.com/muxiaoxiii/dadaduck-public";
const xiaohongshuUrl = "https://xhslink.com/m/A3Ibe0w27UC";

export function AboutDialog({ onClose }: { onClose: () => void }) {
  const [linkError, setLinkError] = useState(false);

  function openLink(event: MouseEvent<HTMLAnchorElement>) {
    if (!isTauri()) return;
    event.preventDefault();
    setLinkError(false);
    void openUrl(event.currentTarget.href).catch(() => setLinkError(true));
  }

  return (
    <Dialog title="关于小达鸭" className="about-dialog" onClose={onClose}>
      <div className="card-header">
        <div className="about-brand">
          <img src="/app-icon.svg" alt="" width="36" height="36" />
          <div>
            <h2 className="modal-title">小达鸭</h2>
            <span className="hint-text">DadaDuck v{version}</span>
          </div>
        </div>
        <button type="button" className="icon-btn" aria-label="关闭关于页" onClick={onClose}>
          <X size={16} />
        </button>
      </div>

      <div className="about-links">
        <a href={xiaohongshuUrl} target="_blank" rel="noreferrer" onClick={openLink}>
          <img className="about-qr" src="/xiaohongshu-qr.png" width="111" height="111" alt="木小樨的小红书主页二维码" />
        </a>
        <div>
          <p className="about-author">木小樨</p>
          <p className="hint-text">小红书 · 扫码查看</p>
          <a href={projectUrl} target="_blank" rel="noreferrer" onClick={openLink}>GitHub 源码</a>
        </div>
      </div>

      <div className="about-legal">
        <p>© 2026 木小樨（muxiaoxiii） · <a href={`${projectUrl}/blob/main/LICENSE`} target="_blank" rel="noreferrer" onClick={openLink}>非商业许可</a></p>
        <p>免责声明：本工具辅助归档，文书及送达状态以法院平台和正式通知为准。</p>
      </div>
      {linkError && <p className="error-inline" role="alert">无法打开链接，请重试或扫码访问。</p>}
    </Dialog>
  );
}
