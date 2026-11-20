import type {
  AppSettings,
  DownloadedDoc,
  ItemStatus,
  RenameTemplate,
  SubdirMode,
} from "../types";

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "--";
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
}

export function formatEta(
  received: number,
  total: number | null | undefined,
  speed: number,
  compact = false
): string {
  if (!total || total <= received || speed <= 0) return "--";
  const secs = Math.ceil((total - received) / speed);
  if (secs < 60) return `${secs}${compact ? "s" : "秒"}`;
  const mins = Math.floor(secs / 60);
  return compact ? `${mins}m${secs % 60}s` : `${mins}分${secs % 60}秒`;
}

export function statusLabel(status: ItemStatus): string {
  switch (status) {
    case "pending":
      return "等待中";
    case "running":
      return "下载中";
    case "needs_identity":
      return "需核验";
    case "completed":
      return "已完成";
    case "partial":
      return "部分完成";
    case "failed":
      return "失败";
    case "browser_required":
      return "需浏览器";
  }
}

export function isItemFinished(status: ItemStatus): boolean {
  return (
    status === "completed" ||
    status === "partial" ||
    status === "failed" ||
    status === "browser_required"
  );
}

export function countSavedDocs(documents: DownloadedDoc[]): number {
  return documents.filter((d) => (d.saveStatus || "saved") === "saved").length;
}

export function savedDocsBytes(documents: DownloadedDoc[]): number {
  return documents
    .filter((d) => (d.saveStatus || "saved") === "saved")
    .reduce((acc, d) => acc + d.sizeBytes, 0);
}

export const RENAME_PRESETS: { value: RenameTemplate; label: string }[] = [
  { value: "case_archive", label: "案号_文书名_下载日期" },
  { value: "court_case_title", label: "法院_案号_文书名_下载日期" },
  { value: "date_case_title", label: "下载日期_案号_文书名" },
  { value: "title_case_paren", label: "文书名（案号）_下载日期" },
  { value: "simple", label: "文书名_下载日期" },
  { value: "index_title", label: "序号_文书名" },
  { value: "original", label: "保留原文件名" },
  { value: "custom", label: "自定义模板" },
];

export const SUBDIR_OPTIONS: { value: SubdirMode; label: string }[] = [
  { value: "case_number", label: "按案号建文件夹" },
  { value: "court_and_case", label: "按法院 / 案号两级目录" },
  { value: "date", label: "按下载日期建文件夹" },
  { value: "none", label: "直接保存在根目录" },
];

export const PLACEHOLDER_TOKENS: { token: string; label: string }[] = [
  { token: "{case_no}", label: "+ 案号" },
  { token: "{court}", label: "+ 法院" },
  { token: "{title}", label: "+ 文书名" },
  { token: "{parties}", label: "+ 当事人" },
  { token: "{date}", label: "+ 下载日期" },
  { token: "{date_dash}", label: "+ 下载年-月-日" },
  { token: "{time}", label: "+ 下载时间" },
  { token: "{index2}", label: "+ 两位序号" },
  { token: "{index}", label: "+ 序号" },
];

export function renameTemplateLabel(
  value: RenameTemplate,
  customTemplate: string
): string {
  if (value === "custom") return customTemplate || "自定义模板";
  return RENAME_PRESETS.find((p) => p.value === value)?.label ?? value;
}

export function subdirModeLabel(
  mode: SubdirMode,
  createCaseSubdir: boolean
): string {
  const effective = !createCaseSubdir && mode === "case_number" ? "none" : mode;
  return (
    SUBDIR_OPTIONS.find((o) => o.value === effective)?.label ?? "按案号建文件夹"
  );
}

function collapseSeparators(input: string): string {
  return input
    .replace(/__+/g, "_")
    .replace(/--+/g, "-")
    .replace(/（）/g, "")
    .replace(/\(\)/g, "")
    .replace(/^[_\- ]+|[_\- ]+$/g, "");
}

export function buildFilenamePreview(draft: AppSettings): string {
  const now = new Date();
  const pad2 = (n: number) => String(n).padStart(2, "0");
  const yyyy = String(now.getFullYear());
  const mm = pad2(now.getMonth() + 1);
  const dd = pad2(now.getDate());
  const date = `${yyyy}${mm}${dd}`;
  const dateDash = `${yyyy}-${mm}-${dd}`;
  const time = `${pad2(now.getHours())}${pad2(now.getMinutes())}${pad2(now.getSeconds())}`;

  const caseNo = "(2026) 示73行初90001号";
  const court = "北京知识产权法院";
  const title = "诉讼参与人举证材料";
  const parties = "张三、李四";
  const idx = "1";
  const idx2 = "01";

  let stem = "";
  switch (draft.renameTemplate) {
    case "simple":
      stem = `${title}_${date}`;
      break;
    case "case_archive":
      stem = collapseSeparators(`${caseNo}_${title}_${date}`);
      break;
    case "court_case_title":
      stem = collapseSeparators(`${court}_${caseNo}_${title}_${date}`);
      break;
    case "date_case_title":
      stem = collapseSeparators(`${date}_${caseNo}_${title}`);
      break;
    case "title_case_paren":
      stem = `${title}（${caseNo}）_${date}`;
      break;
    case "index_title":
      stem = `${idx2}_${title}`;
      break;
    case "original":
      stem = title;
      break;
    case "custom": {
      const tpl = draft.customTemplate.trim() || "{case_no}_{title}_{date}";
      const replaced = tpl
        .replace(/\{title\}/g, title)
        .replace(/\{raw_title\}/g, title)
        .replace(/\{case_no\}/g, caseNo)
        .replace(/\{court\}/g, court)
        .replace(/\{parties\}/g, parties)
        .replace(/\{date\}/g, date)
        .replace(/\{date_dash\}/g, dateDash)
        .replace(/\{time\}/g, time)
        .replace(/\{index2\}/g, idx2)
        .replace(/\{index\}/g, idx);
      stem = collapseSeparators(replaced);
      break;
    }
  }

  if (draft.prefixIndex && draft.renameTemplate !== "index_title") {
    stem = `${idx2}_${stem}`;
  }

  const file = `${stem || title}.pdf`;
  const effectiveSubdir: SubdirMode =
    !draft.createCaseSubdir && draft.subdirMode === "case_number"
      ? "none"
      : draft.subdirMode;
  switch (effectiveSubdir) {
    case "none":
      return file;
    case "case_number":
      return `${caseNo}/${file}`;
    case "court_and_case":
      return `${court}/${caseNo}/${file}`;
    case "date":
      return `${date}/${file}`;
  }
}
