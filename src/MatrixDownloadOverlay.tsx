import { useEffect, useMemo, useRef } from "react";
import { Eye, Pause, Play, Square } from "lucide-react";
import type { QueueItem, QueueSnapshot } from "./types";
import {
  countSavedDocs,
  formatBytes,
  formatEta,
  formatSpeed,
  isItemFinished,
} from "./utils/format";

interface MatrixDownloadOverlayProps {
  snapshot: QueueSnapshot;
  activeItem?: QueueItem;
  overallPercent: number;
  onPause: () => void;
  onResume: () => void;
  onCancel: () => void;
  onMinimize: () => void;
}

const MATRIX_CHARS =
  "0123456789ABCDEFabcdef法达卷宗文书送达ZXFW<>[]{}=*+-#";

export function MatrixDownloadOverlay({
  snapshot,
  activeItem,
  overallPercent,
  onPause,
  onResume,
  onCancel,
  onMinimize,
}: MatrixDownloadOverlayProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const fontSize = 15;
    let width = window.innerWidth;
    let height = window.innerHeight;
    let columns = Math.ceil(width / fontSize);
    let drops: number[] = Array.from({ length: columns }, () =>
      Math.floor(Math.random() * -40)
    );

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      width = window.innerWidth;
      height = window.innerHeight;
      canvas.width = Math.floor(width * dpr);
      canvas.height = Math.floor(height * dpr);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const nextCols = Math.ceil(width / fontSize);
      if (nextCols !== columns) {
        columns = nextCols;
        drops = Array.from({ length: columns }, (_, idx) =>
          drops[idx] !== undefined ? drops[idx] : Math.floor(Math.random() * -40)
        );
      }
    };

    resize();
    window.addEventListener("resize", resize);

    let rafId = 0;
    let lastTick = 0;
    const frameInterval = 42;

    const draw = (now: number) => {
      rafId = window.requestAnimationFrame(draw);
      if (now - lastTick < frameInterval) return;
      lastTick = now;

      ctx.fillStyle = "rgba(3, 8, 5, 0.14)";
      ctx.fillRect(0, 0, width, height);

      ctx.font = `600 ${fontSize}px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`;

      for (let i = 0; i < columns; i++) {
        const ch = MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)];
        const x = i * fontSize;
        const y = drops[i] * fontSize;

        if (y > 0) {
          ctx.fillStyle = Math.random() > 0.72 ? "#d1fae5" : "#22c55e";
          ctx.fillText(ch, x, y);
        }

        if (y > height && Math.random() > 0.975) {
          drops[i] = Math.floor(Math.random() * -18);
        } else {
          drops[i] += 1;
        }
      }
    };

    rafId = window.requestAnimationFrame(draw);
    return () => {
      window.cancelAnimationFrame(rafId);
      window.removeEventListener("resize", resize);
    };
  }, []);

  const docPct =
    activeItem?.totalBytes && activeItem.totalBytes > 0
      ? Math.min(100, Math.round((activeItem.bytesReceived / activeItem.totalBytes) * 100))
      : null;

  const finishedCount = snapshot.items.filter((i) => isItemFinished(i.status)).length;

  const activeTaskPos = activeItem
    ? snapshot.items.findIndex((i) => i.id === activeItem.id) + 1
    : finishedCount;

  const recentLogs = useMemo(
    () =>
      snapshot.items
        .filter((i) => i.status !== "pending" && i.status !== "running")
        .slice(-3)
        .reverse(),
    [snapshot.items]
  );

  return (
    <div className="matrix-overlay" role="dialog" aria-label="正在下载中">
      <canvas ref={canvasRef} className="matrix-canvas" />

      <div className="matrix-top-bar">
        <span className="matrix-badge">
          DADADUCK // TASK {activeTaskPos}/{snapshot.totalCount} ·{" "}
          {activeItem?.stageLabel || (snapshot.isPaused ? "PAUSED" : "RUNNING")}
        </span>
        <button className="matrix-ghost-btn" onClick={onMinimize}>
          <Eye size={14} />
          切回列表视图
        </button>
      </div>

      <div className="matrix-center-card">
        <div className="matrix-duck-ring">
          <svg width="34" height="34" viewBox="0 0 32 32" fill="none" aria-hidden="true">
            <circle cx="16" cy="17" r="10.5" fill="#16a34a" fillOpacity="0.22" stroke="#4ade80" strokeWidth="1.4" />
            <circle cx="19.5" cy="12" r="6" fill="#22c55e" fillOpacity="0.25" stroke="#86efac" strokeWidth="1.4" />
            <path d="M24.5 12.5C27.5 12.5 29 13.7 29 14.8C29 16 27.2 16.7 24.3 16.5Z" fill="#4ade80" />
            <circle cx="21.2" cy="11.2" r="1.2" fill="#ecfdf5" />
          </svg>
        </div>

        <h2 className="matrix-title">
          {snapshot.isPaused ? "队列已暂停" : activeItem?.stageLabel || "正在下载中"}
          <span className="matrix-cursor">_</span>
        </h2>

        <p className="matrix-subtitle">
          {activeItem
            ? `${activeItem.caseNumber || activeItem.courtName || "送达文书任务"} · ${
                activeItem.courtName && activeItem.caseNumber ? `${activeItem.courtName} · ` : ""
              }${activeItem.platformLabel}`
            : "正在调度下一条送达任务..."}
        </p>

        {activeItem && (
          <div className="matrix-doc-panel">
            <div className="matrix-doc-row">
              <span className="matrix-doc-name">
                {activeItem.currentDocTotal > 0
                  ? `[${activeItem.currentDocIndex}/${activeItem.currentDocTotal}] ${
                      activeItem.currentDocName || "文书"
                    }`
                  : activeItem.statusMessage}
              </span>
              <span className="matrix-mono">
                {activeItem.currentDocTotal > 0
                  ? `${formatBytes(activeItem.bytesReceived)}${
                      activeItem.totalBytes
                        ? ` / ${formatBytes(activeItem.totalBytes)} (${docPct}%)`
                        : ""
                    }`
                  : activeItem.stageLabel || "连接中"}
              </span>
            </div>

            {activeItem.currentTargetFileName && (
              <div className="matrix-target-name">
                → 保存为：{activeItem.currentTargetFileName}
              </div>
            )}

            <div className="matrix-track">
              <div
                className={`matrix-fill ${docPct === null ? "indeterminate" : ""}`}
                style={{ width: docPct !== null ? `${docPct}%` : "42%" }}
              />
            </div>

            <div className="matrix-telemetry-row">
              <span>速率：{formatSpeed(activeItem.speedBytesPerSec)}</span>
              <span>
                剩余：
                {formatEta(
                  activeItem.bytesReceived,
                  activeItem.totalBytes,
                  activeItem.speedBytesPerSec,
                  true
                )}
              </span>
              <span>
                本案已保存：{countSavedDocs(activeItem.documents)}
                {activeItem.currentDocTotal > 0 ? `/${activeItem.currentDocTotal}` : ""} 份
              </span>
            </div>
          </div>
        )}

        <div className="matrix-overall">
          <div className="matrix-doc-row">
            <span>
              总队列进度 · 已保存 {snapshot.totalDownloadedDocs} 份（
              {formatBytes(snapshot.totalDownloadedBytes)}）
            </span>
            <span className="matrix-mono">
              已完成 {finishedCount}/{snapshot.totalCount} 项 ({overallPercent}%)
            </span>
          </div>
          <div className="matrix-track matrix-track-sm">
            <div className="matrix-fill" style={{ width: `${overallPercent}%` }} />
          </div>
          <div className="matrix-telemetry-row">
            <span>完成 {snapshot.completedCount}</span>
            <span>等待 {snapshot.pendingCount}</span>
            <span>待核验 {snapshot.needsIdentityCount}</span>
            <span>失败 {snapshot.failedCount}</span>
          </div>
        </div>

        {recentLogs.length > 0 && (
          <div className="matrix-recent-logs">
            {recentLogs.map((it) => (
              <div key={it.id} className="matrix-log-line">
                <span>
                  {it.status === "completed" ? "✓" : it.status === "needs_identity" ? "🔑" : "!"}
                  {" "}
                  {it.caseNumber || it.courtName || "送达任务"}
                </span>
                <span className="matrix-mono">{it.statusMessage}</span>
              </div>
            ))}
          </div>
        )}

        <div className="matrix-actions">
          {snapshot.isPaused ? (
            <button className="matrix-btn matrix-btn-primary" onClick={onResume}>
              <Play size={14} />
              继续下载
            </button>
          ) : (
            <button className="matrix-btn" onClick={onPause} title="立即暂停当前下载">
              <Pause size={14} />
              暂停
            </button>
          )}
          <button className="matrix-btn matrix-btn-danger" onClick={onCancel} title="立即停止下载">
            <Square size={14} />
            停止下载
          </button>
        </div>
      </div>
    </div>
  );
}
