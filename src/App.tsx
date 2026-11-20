import React, { useEffect, useMemo, useRef, useState } from "react";
import { isTauri, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Archive,
  ArrowDownToLine,
  ClipboardList,
  ClipboardPaste,
  FolderInput,
  FolderOpen,
  Inbox,
  ListOrdered,
  Mail,
  Monitor,
  Pause,
  Play,
  Plus,
  RotateCcw,
  SlidersHorizontal,
  Square,
  Upload,
  UserCheck,
} from "lucide-react";
import { Airmail, DuckMark } from "./art/Postmark";
import { Dialog } from "./Dialog";
import { DemoDialog } from "./components/DemoDialog";
import { ArchiveSettingsDialog } from "./components/ArchiveSettingsDialog";
import { FallbackControlBar } from "./components/FallbackControlBar";
import { LegacyRecoveryDialog } from "./components/LegacyRecoveryDialog";
import { ProfilesDialog } from "./components/ProfilesDialog";
import { QueueItemCard } from "./components/QueueItemCard";
import { ReceiptDecisionDialog } from "./components/ReceiptDecisionDialog";
import { VerifyIdentityDialog } from "./components/VerifyIdentityDialog";
import { MatrixDownloadOverlay } from "./MatrixDownloadOverlay";
import { previewSettings } from "./preview";
import type {
  AppSettings,
  DownloadedDoc,
  FallbackSessionSnapshot,
  FileProgress,
  ImportTaskDocsResult,
  LegacyDocumentEntry,
  LegacyExportResult,
  ParseBatchSummary,
  QueueSnapshot,
  SubdirMode,
} from "./types";
import {
  formatBytes,
  isItemFinished,
  renameTemplateLabel,
  subdirModeLabel,
} from "./utils/format";

export default function App() {
  const desktop = isTauri();
  const isMobile = /Android|iPhone|iPad/i.test(navigator.userAgent);
  const [archiveDraft, setArchiveDraft] = useState<AppSettings | null>(null);
  const [archiveError, setArchiveError] = useState("");
  const [busy, setBusy] = useState(false);
  const [rawInput, setRawInput] = useState("");
  const [settings, setSettings] = useState<AppSettings | null>(
    desktop ? null : previewSettings
  );
  const [snapshot, setSnapshot] = useState<QueueSnapshot>({
    items: [],
    isRunning: false,
    isPaused: false,
    totalCount: 0,
    completedCount: 0,
    failedCount: 0,
    needsIdentityCount: 0,
    pendingCount: 0,
    browserRequiredCount: 0,
    totalDownloadedDocs: 0,
    totalDownloadedBytes: 0,
  });
  const [banner, setBanner] = useState<string | null>(null);
  const [showDemo, setShowDemo] = useState(false);
  const [showPrivacy, setShowPrivacy] = useState(false);
  const [confirmClearData, setConfirmClearData] = useState(false);
  const [signingTaskId, setSigningTaskId] = useState<string | null>(null);
  const signingTask = snapshot.items.find((item) => item.id === signingTaskId);
  const receiptPrompt = snapshot.receiptPrompt;
  const [verifyId, setVerifyId] = useState<string | null>(null);
  const verifyTarget = snapshot.items.find((item) => item.id === verifyId);
  const [verifyMethod, setVerifyMethod] = useState<"sjhm" | "zjhm">("sjhm");
  const [verifyTail, setVerifyTail] = useState("");
  const [verifyProfileSearch, setVerifyProfileSearch] = useState("");
  const [verifyError, setVerifyError] = useState<string | null>(null);
  const [verifySubmitting, setVerifySubmitting] = useState(false);

  const [lastParseSummary, setLastParseSummary] =
    useState<ParseBatchSummary | null>(null);
  const [editingCaseId, setEditingCaseId] = useState<string | null>(null);
  const [editingCaseValue, setEditingCaseValue] = useState("");
  const [expandedDocs, setExpandedDocs] = useState<Record<string, boolean>>({});
  const [docActionErrors, setDocActionErrors] = useState<
    Record<string, { message: string; canShare: boolean }>
  >({});
  const [confirmDeleteKey, setConfirmDeleteKey] = useState<string | null>(null);
  const [archivePreviewPath, setArchivePreviewPath] = useState("");

  const [showLegacyModal, setShowLegacyModal] = useState(false);
  const [legacyDocs, setLegacyDocs] = useState<LegacyDocumentEntry[]>([]);
  const [legacyCaseOverrides, setLegacyCaseOverrides] = useState<
    Record<string, string>
  >({});
  const [legacySelected, setLegacySelected] = useState<Record<string, boolean>>(
    {}
  );
  const [legacyScanning, setLegacyScanning] = useState(false);
  const [legacyExporting, setLegacyExporting] = useState(false);
  const [legacySummaryMsg, setLegacySummaryMsg] = useState<string | null>(null);

  const [showProfiles, setShowProfiles] = useState(false);
  const [profileSearch, setProfileSearch] = useState("");
  const [profileName, setProfileName] = useState("");
  const [profileMobile4, setProfileMobile4] = useState("");
  const [profileId4, setProfileId4] = useState("");
  const [profileError, setProfileError] = useState<string | null>(null);
  const [showBulkImport, setShowBulkImport] = useState(false);
  const [bulkProfilesText, setBulkProfilesText] = useState("");

  const [matrixMinimized, setMatrixMinimized] = useState(false);
  const [duckWobble, setDuckWobble] = useState(false);
  const [supportsEmbeddedFallback, setSupportsEmbeddedFallback] = useState(false);
  const [fallbackSession, setFallbackSession] =
    useState<FallbackSessionSnapshot | null>(null);

  const duckClicksRef = useRef<number[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const profileNameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;

    invoke<AppSettings>("get_settings")
      .then((res) => {
        if (!disposed) setSettings(res);
      })
      .catch((error) => {
        if (!disposed) setBanner(String(error));
      });
    invoke<QueueSnapshot>("get_queue_snapshot")
      .then((res) => {
        if (!disposed) setSnapshot(res);
      })
      .catch((error) => {
        if (!disposed) setBanner(String(error));
      });
    invoke<LegacyDocumentEntry[]>("scan_legacy_documents")
      .then((list) => {
        if (!disposed) setLegacyDocs(list);
      })
      .catch(() => {});
    invoke<boolean>("supports_embedded_fallback")
      .then((supported) => {
        if (!disposed) setSupportsEmbeddedFallback(supported);
      })
      .catch(() => {});
    invoke<FallbackSessionSnapshot | null>("get_fallback_session")
      .then((session) => {
        if (!disposed) setFallbackSession(session);
      })
      .catch(() => {});

    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        invoke("refresh_document_statuses").catch(() => {});
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    let unlistenQueueFn: (() => void) | undefined;
    let unlistenProgressFn: (() => void) | undefined;
    let unlistenFallbackFn: (() => void) | undefined;

    listen<QueueSnapshot>("queue-updated", (event) => {
      if (!disposed) setSnapshot(event.payload);
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenQueueFn = fn;
      }
    });

    listen<FallbackSessionSnapshot | null>("fallback-updated", (event) => {
      if (!disposed) setFallbackSession(event.payload);
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenFallbackFn = fn;
      }
    });

    listen<FileProgress>("file-progress", (event) => {
      if (disposed) return;
      const p = event.payload;
      setSnapshot((prev) => ({
        ...prev,
        items: prev.items.map((item) =>
          item.id === p.taskId
            ? {
                ...item,
                stageLabel: p.stage,
                currentDocIndex: p.docIndex,
                currentDocTotal: p.docTotal,
                currentDocName: p.docName,
                currentTargetFileName:
                  p.targetFileName ?? item.currentTargetFileName,
                bytesReceived: p.bytesReceived,
                totalBytes: p.totalBytes ?? null,
                speedBytesPerSec: p.speedBytesPerSec,
              }
            : item
        ),
      }));
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenProgressFn = fn;
      }
    });

    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVisibility);
      unlistenQueueFn?.();
      unlistenProgressFn?.();
      unlistenFallbackFn?.();
    };
  }, [desktop]);

  useEffect(() => {
    if (!archiveDraft) {
      setArchivePreviewPath("");
      return;
    }
    if (!desktop) {
      setArchivePreviewPath("");
      return;
    }
    let active = true;
    invoke<string>("preview_archive_path", {
      subdirMode: archiveDraft.subdirMode,
      renameTemplate: archiveDraft.renameTemplate,
      customTemplate: archiveDraft.customTemplate,
      prefixIndex: archiveDraft.prefixIndex,
      outputDir: archiveDraft.outputDir,
      androidStorageMode: archiveDraft.androidStorageMode ?? null,
      androidTreeLabel: archiveDraft.androidTreeLabel ?? null,
    })
      .then((path) => {
        if (active) setArchivePreviewPath(path);
      })
      .catch(() => {
        if (active) setArchivePreviewPath("");
      });
    return () => {
      active = false;
    };
  }, [
    desktop,
    archiveDraft?.subdirMode,
    archiveDraft?.renameTemplate,
    archiveDraft?.customTemplate,
    archiveDraft?.prefixIndex,
    archiveDraft?.outputDir,
    archiveDraft?.androidStorageMode,
    archiveDraft?.androidTreeLabel,
  ]);

  const overallPercent = useMemo(() => {
    if (snapshot.totalCount === 0) return 0;
    const finished = snapshot.items.filter((i) =>
      isItemFinished(i.status)
    ).length;
    return Math.round((finished / snapshot.totalCount) * 100);
  }, [snapshot]);

  const hasPending = useMemo(
    () => snapshot.items.some((i) => i.status === "pending"),
    [snapshot.items]
  );

  const activeItem = useMemo(
    () => snapshot.items.find((i) => i.status === "running"),
    [snapshot.items]
  );

  useEffect(() => {
    if (!snapshot.isRunning) {
      setMatrixMinimized(false);
    }
  }, [snapshot.isRunning]);

  const filteredProfiles = useMemo(() => {
    if (!settings) return [];
    const q = profileSearch.trim().toLowerCase();
    if (!q) return settings.profiles;
    return settings.profiles.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.mobileLast4.includes(q) ||
        (p.idLast4?.toLowerCase().includes(q) ?? false)
    );
  }, [settings, profileSearch]);

  const filteredVerifyProfiles = useMemo(() => {
    if (!settings) return [];
    const q = verifyProfileSearch.trim().toLowerCase();
    if (!q) return settings.profiles;
    return settings.profiles.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.mobileLast4.includes(q) ||
        (p.idLast4?.toLowerCase().includes(q) ?? false)
    );
  }, [settings, verifyProfileSearch]);

  async function handleDuckEasterEggClick() {
    if (!settings) return;
    const now = Date.now();
    const recent = duckClicksRef.current.filter((t) => now - t <= 2200);
    recent.push(now);
    duckClicksRef.current = recent;

    if (recent.length >= 2 && recent.length < 5) {
      setDuckWobble(true);
      window.setTimeout(() => setDuckWobble(false), 260);
    }

    if (recent.length >= 5) {
      duckClicksRef.current = [];
      const nextEnabled = !settings.matrixEnabled;
      const next = { ...settings, matrixEnabled: nextEnabled };
      setSettings(next);
      if (nextEnabled) setMatrixMinimized(false);
      if (desktop) {
        try {
          const saved = await invoke<AppSettings>("update_settings", {
            settings: next,
          });
          setSettings(saved);
        } catch (err) {
          setBanner(String(err));
          return;
        }
      }
      setBanner(
        nextEnabled
          ? "嘎！已解锁暗夜代码雨下载屏（再次连点小鸭子 5 下可关闭）"
          : "已关闭暗夜代码雨下载屏，恢复标准下载视图"
      );
    }
  }

  function editArchive() {
    if (!settings) return;
    const effectiveMode: SubdirMode =
      !settings.createCaseSubdir && settings.subdirMode === "case_number"
        ? "none"
        : settings.subdirMode || "case_number";
    setArchiveDraft({ ...settings, subdirMode: effectiveMode });
    setArchiveError("");
  }

  async function saveArchive() {
    if (!archiveDraft || !settings) return;
    if (
      archiveDraft.renameTemplate === "custom" &&
      !archiveDraft.customTemplate.trim()
    ) {
      setArchiveError("请输入自定义命名模板。");
      return;
    }
    const next: AppSettings = {
      ...archiveDraft,
      createCaseSubdir: archiveDraft.subdirMode !== "none",
      matrixEnabled: settings.matrixEnabled,
      profiles: settings.profiles,
    };
    try {
      setSettings(
        desktop
          ? await invoke<AppSettings>("update_settings", { settings: next })
          : next
      );
      setArchiveDraft(null);
    } catch (error) {
      setArchiveError(`保存失败：${String(error)}`);
    }
  }

  async function handleChooseFolder() {
    if (!desktop || !archiveDraft) return;
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: "选择文书保存目录",
        defaultPath: archiveDraft.outputDir,
      });
      if (typeof selected === "string" && selected.trim()) {
        setArchiveDraft({ ...archiveDraft, outputDir: selected });
      }
    } catch (error) {
      setArchiveError(String(error));
    }
  }

  async function handleUseAndroidDownloads() {
    if (!desktop || !archiveDraft) return;
    try {
      const updated = await invoke<AppSettings>("use_android_downloads_dir");
      setSettings(updated);
      setArchiveDraft({
        ...archiveDraft,
        outputDir: updated.outputDir,
        androidStorageMode: updated.androidStorageMode,
      });
      setArchiveError("");
    } catch (error) {
      setArchiveError(String(error));
    }
  }

  async function handlePickAndroidDir() {
    if (!desktop || !archiveDraft) return;
    try {
      const updated = await invoke<AppSettings>("pick_android_directory");
      setSettings(updated);
      setArchiveDraft({
        ...archiveDraft,
        outputDir: updated.outputDir,
        androidStorageMode: updated.androidStorageMode,
        androidTreeUri: updated.androidTreeUri,
        androidTreeLabel: updated.androidTreeLabel,
      });
      setArchiveError("");
    } catch (error) {
      setArchiveError(String(error));
    }
  }

  async function handlePasteClipboard() {
    try {
      let clipText = "";
      if (desktop && isMobile) {
        clipText = await invoke<string>("read_clipboard_text");
      } else if (navigator.clipboard?.readText) {
        clipText = await navigator.clipboard.readText();
      }
      if (!clipText.trim()) {
        setBanner("剪贴板暂无文本，可长按输入框直接粘贴。");
        return;
      }
      setRawInput((prev) =>
        prev.trim() ? `${prev.trimEnd()}\n${clipText}` : clipText
      );
    } catch {
      setBanner("未能读取系统剪贴板，请长按输入框手动粘贴。");
    }
  }

  async function handleSaveCaseNumber(taskId: string) {
    if (!desktop) return;
    await invoke("update_item_case_number", {
      taskId,
      caseNumber: editingCaseValue.trim() || null,
    });
    setEditingCaseId(null);
    setEditingCaseValue("");
  }

  function docKey(taskId: string, docIdx: number) {
    return `${taskId}:${docIdx}`;
  }

  function clearDocError(key: string) {
    setDocActionErrors((prev) => {
      if (!prev[key]) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }

  async function handleOpenDoc(taskId: string, docIdx: number, reveal: boolean) {
    const key = docKey(taskId, docIdx);
    clearDocError(key);
    try {
      await invoke("open_download", { taskId, index: docIdx, reveal });
    } catch (err) {
      const msg = String(err);
      if (msg.startsWith("NO_VIEWER:")) {
        setDocActionErrors((prev) => ({
          ...prev,
          [key]: {
            message: "已保存，手机暂未安装可打开此类文件的阅读器",
            canShare: true,
          },
        }));
      } else if (msg.startsWith("MISSING:")) {
        setDocActionErrors((prev) => ({
          ...prev,
          [key]: {
            message: isMobile
              ? "该文件已从手机存储移除"
              : "该文件已被移动或删除",
            canShare: false,
          },
        }));
      } else {
        setDocActionErrors((prev) => ({
          ...prev,
          [key]: {
            message: `暂时无法直接打开（${msg.replace(/^[A-Z_]+:/, "")}）`,
            canShare: isMobile,
          },
        }));
      }
    }
  }

  async function handleShareDoc(taskId: string, docIdx: number) {
    const key = docKey(taskId, docIdx);
    clearDocError(key);
    try {
      await invoke("share_download", { taskId, index: docIdx });
    } catch (err) {
      const msg = String(err).replace(/^[A-Z_]+:/, "");
      setDocActionErrors((prev) => ({
        ...prev,
        [key]: { message: `分享失败：${msg}`, canShare: false },
      }));
    }
  }

  async function handleRetrySaveDoc(taskId: string, docIdx: number) {
    const key = docKey(taskId, docIdx);
    clearDocError(key);
    try {
      await invoke("retry_save_document", { taskId, index: docIdx });
    } catch (err) {
      setDocActionErrors((prev) => ({
        ...prev,
        [key]: { message: `保存重试失败：${String(err)}`, canShare: false },
      }));
    }
  }

  async function handleConfirmDeleteDocFile(taskId: string, docIdx: number) {
    const key = docKey(taskId, docIdx);
    clearDocError(key);
    try {
      await invoke("remove_downloaded_file", { taskId, index: docIdx });
      setConfirmDeleteKey(null);
    } catch (err) {
      setDocActionErrors((prev) => ({
        ...prev,
        [key]: {
          message: `删除本机文件失败：${String(err)}`,
          canShare: false,
        },
      }));
    }
  }

  async function openLegacyRecoveryModal() {
    setShowLegacyModal(true);
    setLegacySummaryMsg(null);
    if (!desktop) return;
    setLegacyScanning(true);
    try {
      const list = await invoke<LegacyDocumentEntry[]>("scan_legacy_documents");
      setLegacyDocs(list);
      const nextOverrides: Record<string, string> = {};
      const nextSelected: Record<string, boolean> = {};
      for (const entry of list) {
        if (entry.inferredCaseNumber) {
          nextOverrides[entry.path] = entry.inferredCaseNumber;
        }
        if (!entry.exportedBefore) {
          nextSelected[entry.path] = true;
        }
      }
      setLegacyCaseOverrides(nextOverrides);
      setLegacySelected(nextSelected);
    } catch (err) {
      setLegacySummaryMsg(`扫描旧归档目录失败：${String(err)}`);
    } finally {
      setLegacyScanning(false);
    }
  }

  async function handleExportLegacyBatch(targetPaths?: string[]) {
    if (!desktop || legacyExporting) return;
    const chosenPaths =
      targetPaths ??
      legacyDocs.filter((d) => legacySelected[d.path]).map((d) => d.path);
    if (chosenPaths.length === 0) {
      setLegacySummaryMsg("请先勾选要导出的文书");
      return;
    }
    setLegacyExporting(true);
    setLegacySummaryMsg(null);
    try {
      const payload = chosenPaths.map((path) => ({
        path,
        caseNumber: legacyCaseOverrides[path]?.trim() || null,
      }));
      const res = await invoke<LegacyExportResult>("export_legacy_documents", {
        items: payload,
      });
      const updated = await invoke<LegacyDocumentEntry[]>(
        "scan_legacy_documents"
      );
      setLegacyDocs(updated);
      const msgParts = [`已导出 ${res.exported} 份（已校验并保留原件）`];
      if (res.skipped > 0) msgParts.push(`跳过 ${res.skipped} 份`);
      if (res.errors.length > 0) msgParts.push(res.errors[0]);
      setLegacySummaryMsg(msgParts.join(" · "));
    } catch (err) {
      setLegacySummaryMsg(`导出失败：${String(err)}`);
    } finally {
      setLegacyExporting(false);
    }
  }

  async function handleOpenOutputFolder() {
    if (!desktop) {
      setBanner("请在桌面版打开本机保存目录。");
      return;
    }
    if (!settings?.outputDir) return;
    try {
      await invoke("open_output_dir");
    } catch (err) {
      setBanner(`目录尚未创建或无法打开：${String(err)}`);
    }
  }

  async function handleEnqueue(autoStart: boolean) {
    if (!rawInput.trim()) {
      setBanner("请先粘贴法院送达短信或链接");
      return;
    }
    if (!desktop) {
      setBanner("实际送达链接请使用安装版。可点击「本机演示」体验流程。");
      return;
    }
    const submittedInput = rawInput;
    const res = await invoke<ParseBatchSummary>("parse_and_enqueue", {
      rawInput: submittedInput,
    });
    setLastParseSummary(res);
    const parts: string[] = [];
    if (res.added.length > 0) parts.push(`已加入 ${res.added.length} 条`);
    if (res.skippedDuplicates > 0)
      parts.push(`跳过重复 ${res.skippedDuplicates} 条`);
    if (res.unrecognizedLines > 0) {
      parts.push(`有 ${res.unrecognizedLines} 项未识别（已保留在输入框）`);
    }
    if (parts.length > 0) setBanner(parts.join("，"));

    setRawInput((curr) => {
      if (curr !== submittedInput) return curr;
      if (res.unrecognizedBlocks && res.unrecognizedBlocks.length > 0) {
        return res.unrecognizedBlocks.join("\n");
      }
      return res.added.length > 0 || res.skippedDuplicates > 0 ? "" : curr;
    });

    if (res.added.length > 0 && autoStart) {
      setMatrixMinimized(false);
      await invoke<boolean>("start_queue");
    }
  }

  async function handleFileImport(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    setRawInput((prev) => (prev.trim() ? `${prev}\n${text}` : text));
    e.target.value = "";
  }

  async function handleSubmitVerify(e: React.FormEvent) {
    e.preventDefault();
    if (
      !verifyTarget ||
      verifyTarget.status !== "needs_identity" ||
      verifySubmitting
    ) {
      return;
    }
    setVerifyError(null);
    setVerifySubmitting(true);
    try {
      await invoke("submit_identity", {
        taskId: verifyTarget.id,
        method: verifyMethod,
        tail: verifyTail.trim(),
      });
      setVerifyId(null);
      setVerifyTail("");
      setVerifyProfileSearch("");
    } catch (err) {
      setVerifyError(String(err));
    } finally {
      setVerifySubmitting(false);
    }
  }

  async function runAction(action: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    try {
      await action();
    } catch (error) {
      setBanner(String(error));
    } finally {
      setBusy(false);
    }
  }

  function queueAction(command: string, taskId?: string) {
    if (!desktop) {
      if (command === "remove_queue_item") {
        setSnapshot((prev) => ({
          ...prev,
          items: [],
          totalCount: 0,
          pendingCount: 0,
        }));
      } else {
        setBanner("请在桌面版下载实际送达文书。");
      }
      return;
    }
    if (command === "start_queue") {
      setMatrixMinimized(false);
    }
    void runAction(() => invoke(command, taskId ? { taskId } : {}));
  }

  async function handleCopySmsCode(code: string) {
    try {
      if (desktop && isMobile) {
        await invoke("copy_sensitive_text", { text: code });
      } else {
        await navigator.clipboard.writeText(code);
      }
      setBanner("已复制提取码。");
    } catch {
      setBanner("无法访问系统剪贴板，请手动输入提取码。");
    }
  }

  async function handleOpenExternalBrowser(url: string) {
    await openUrl(url);
  }

  async function handleArchiveDemo() {
    if (desktop) {
      const doc = await invoke<DownloadedDoc>("archive_demo_document");
      return doc.displayLocation || doc.fileName;
    }
    const documentUrl = URL.createObjectURL(new Blob(
      ["小达鸭演示文书\n虚构演示案 · 示例人民法院\n本文书仅用于演示本机归档流程。\n"],
      { type: "text/plain;charset=utf-8" },
    ));
    const download = document.createElement("a");
    download.href = documentUrl;
    download.download = "小达鸭_演示文书.txt";
    download.click();
    window.setTimeout(() => URL.revokeObjectURL(documentUrl), 1000);
    return download.download;
  }

  async function handleOpenFallback(taskId: string) {
    if (!desktop) {
      setBanner("请在桌面版使用内置保底取件窗口。");
      return;
    }
    if (verifyId === taskId) {
      setVerifyId(null);
    }
    await runAction(async () => {
      const session = await invoke<FallbackSessionSnapshot>(
        "open_fallback_browser",
        { taskId }
      );
      setFallbackSession(session);
    });
  }

  async function handleFillFallbackCode(code: string) {
    await runAction(() => invoke("fill_fallback_code", { code }));
  }

  async function handleScanFallbackDocs() {
    await runAction(() => invoke("scan_fallback_documents"));
  }

  async function handleFocusFallbackWindow() {
    await runAction(() => invoke("focus_fallback_window"));
  }

  async function handleCloseFallbackWindow() {
    await runAction(() => invoke("close_fallback_window"));
  }

  async function handleImportLocalDocs(taskId: string) {
    if (!desktop) return;
    const selected = await openDialog({
      multiple: true,
      directory: false,
      title: "选择已在外部下载的文书文件（保留原文件）",
    });
    const paths = Array.isArray(selected)
      ? selected
      : typeof selected === "string"
        ? [selected]
        : [];
    if (paths.length === 0) return;

    await runAction(async () => {
      const res = await invoke<ImportTaskDocsResult>("import_task_documents", {
        taskId,
        paths,
      });
      if (res.errors.length === 0) {
        setBanner(`已校验并归档 ${res.imported} 份本地文书（原文件已保留）。`);
      } else {
        setBanner(
          `本地导入完成：成功 ${res.imported} 份，跳过/失败 ${res.errors.length} 份（${res.errors[0]}）。`
        );
      }
    });
  }

  async function handleSaveProfile(e: React.FormEvent) {
    e.preventDefault();
    setProfileError(null);
    try {
      const updated = await invoke<AppSettings>("save_lawyer_profile", {
        name: profileName,
        mobileLast4: profileMobile4,
        idLast4: profileId4.trim() || null,
      });
      setSettings(updated);
      setProfileName("");
      setProfileMobile4("");
      setProfileId4("");
      profileNameInputRef.current?.focus();
    } catch (err) {
      setProfileError(String(err));
    }
  }

  async function handleBulkImportProfiles() {
    setProfileError(null);
    try {
      const [updated, count] = await invoke<[AppSettings, number]>(
        "batch_save_lawyer_profiles",
        { rawText: bulkProfilesText }
      );
      setSettings(updated);
      setBulkProfilesText("");
      setShowBulkImport(false);
      setBanner(`已批量导入 ${count} 条常用尾号`);
    } catch (err) {
      setProfileError(String(err));
    }
  }

  async function handleDeleteProfile(profileId: string) {
    const updated = await invoke<AppSettings>("delete_lawyer_profile", {
      profileId,
    });
    setSettings(updated);
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand">
          <span
            className={`brand-icon ${duckWobble ? "wobble" : ""} ${
              settings?.matrixEnabled ? "matrix-armed" : ""
            }`}
            onClick={handleDuckEasterEggClick}
            title="小达鸭 DadaDuck"
          >
            <DuckMark />
          </span>
          <strong>小达鸭</strong>
          <span className="wordmark">DadaDuck</span>
        </div>
        <div className="header-actions">
          {(isMobile || legacyDocs.length > 0) && (
            <button
              className="quiet"
              onClick={() => void openLegacyRecoveryModal()}
              disabled={!settings}
              title="找回并导出旧版本保存在应用私密目录的文书"
            >
              <FolderInput size={16} />
              已有文书{legacyDocs.length > 0 ? ` (${legacyDocs.length})` : ""}
            </button>
          )}
          <button
            className="quiet"
            onClick={() => setShowProfiles(true)}
            disabled={!settings}
          >
            <UserCheck size={16} />
            本次尾号
            {settings && settings.profiles.length > 0
              ? ` (${settings.profiles.length})`
              : ""}
          </button>
          {!isMobile && (
            <button
              className="quiet"
              onClick={handleOpenOutputFolder}
              disabled={!settings}
            >
              <FolderOpen size={16} />
              保存目录
            </button>
          )}
        </div>
      </header>
      <main>
        <section className="hero" aria-labelledby="desk-title">
          <div className="hero-copy">
            <span className="hero-label">
              <Mail size={13} /> 法院电子送达
            </span>
            <h1 id="desk-title">送达工作台</h1>
            <p>从法院短信提取送达链接，批量下载并按案归档。支持下载前确认是否提交送达回执。</p>
          </div>
          <Airmail />
        </section>

        {banner && (
          <div className="toast-banner" role="status">
            <span>{banner}</span>
            <button className="link-btn" onClick={() => setBanner(null)}>
              关闭
            </button>
          </div>
        )}

        <div className="workspace-grid">
          <section className="card intake">
            <div className="card-header">
              <h2 className="card-title">
                <ClipboardList />
                导入送达短信
              </h2>
              <div className="intake-header-actions">
                <button
                  className="quiet compact"
                  onClick={() => void handlePasteClipboard()}
                >
                  <ClipboardPaste size={14} />
                  粘贴
                </button>
                <button
                  className="quiet compact"
                  onClick={() => fileInputRef.current?.click()}
                >
                  <Upload size={14} />
                  导入文本
                </button>
              </div>
            </div>
            <label htmlFor="sms" className="input-label">
              短信原文或送达链接
            </label>
            <div className="writing-area">
              <textarea
                id="sms"
                className="sms-textarea"
                placeholder={"在这里粘贴法院送达短信或链接…"}
                value={rawInput}
                onChange={(e) => setRawInput(e.target.value)}
                onKeyDown={(e) => {
                  if (
                    (e.metaKey || e.ctrlKey) &&
                    e.key === "Enter" &&
                    !busy &&
                    rawInput.trim()
                  ) {
                    e.preventDefault();
                    void runAction(() => handleEnqueue(true));
                  }
                }}
              />
              <div className="input-meta">
                <span>支持多条混合粘贴 · 自动去重</span>
                {rawInput ? (
                  <button className="link-btn" onClick={() => setRawInput("")}>
                    清空
                  </button>
                ) : (
                  <button className="link-btn" onClick={() => setShowDemo(true)}>
                    本机演示
                  </button>
                )}
              </div>
            </div>
            <input
              ref={fileInputRef}
              type="file"
              accept=".txt,.csv,.log"
              hidden
              onChange={(e) => runAction(() => handleFileImport(e))}
            />
            <div className="input-footer">
              <div className="btn-group">
                <button
                  className="btn btn-primary"
                  disabled={busy || !rawInput.trim()}
                  onClick={() => runAction(() => handleEnqueue(true))}
                >
                  <ArrowDownToLine size={16} />
                  解析并下载
                </button>
                <button
                  className="btn"
                  disabled={busy || !rawInput.trim()}
                  onClick={() => runAction(() => handleEnqueue(false))}
                >
                  <Plus size={15} />
                  仅加入队列
                </button>
              </div>
            </div>

            {lastParseSummary && (
              <div
                className="parse-feedback-card"
                role="region"
                aria-label="导入解析结果"
              >
                <div className="parse-feedback-summary">
                  <span>
                    导入结果：识别 {lastParseSummary.added.length} 条链接
                    {lastParseSummary.skippedDuplicates > 0
                      ? ` · 跳过重复 ${lastParseSummary.skippedDuplicates} 条`
                      : ""}
                    {lastParseSummary.unrecognizedLines > 0
                      ? ` · 未识别 ${lastParseSummary.unrecognizedLines} 条`
                      : ""}
                  </span>
                  <button
                    className="link-btn"
                    onClick={() => setLastParseSummary(null)}
                  >
                    收起
                  </button>
                </div>
                {lastParseSummary.added.length > 0 && (
                  <div className="parse-feedback-list">
                    {lastParseSummary.added.map((it) => (
                      <div key={it.id} className="parse-feedback-item">
                        <span>
                          ✓ {it.courtName || "未知法院"} ·{" "}
                          {it.caseNumber || "案号待确认"}
                        </span>
                        <span className="hint-text">{it.platformLabel}</span>
                      </div>
                    ))}
                  </div>
                )}
                {lastParseSummary.blockDiagnostics &&
                  lastParseSummary.blockDiagnostics.length > 0 && (
                    <div className="parse-feedback-list">
                      {lastParseSummary.blockDiagnostics.map((d, i) => (
                        <div
                          key={i}
                          className="parse-feedback-item parse-feedback-warn"
                        >
                          <span>
                            ! {d.reason}
                            {d.caseNumber ? `（${d.caseNumber}）` : ""}：
                            {d.rawText.slice(0, 42)}
                            {d.rawText.length > 42 ? "…" : ""}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
              </div>
            )}
          </section>

          <section className="card archive">
            <div className="card-header archive-mobile-hide">
              <h2 className="card-title">
                <Archive />
                归档设置
              </h2>
              <button
                className="icon-btn"
                aria-label="编辑归档设置"
                title="编辑归档设置"
                disabled={!settings}
                onClick={editArchive}
              >
                <SlidersHorizontal size={16} />
              </button>
            </div>
            {settings ? (
              <>
                <div className="input-label archive-mobile-hide">
                  文书保存至
                </div>
                <div className="archive-compact-row">
                  <button
                    className="location"
                    title={settings.outputDir}
                    onClick={editArchive}
                  >
                    <FolderOpen size={18} />
                    <span>
                      {isMobile
                        ? settings.androidStorageMode === "saf_tree"
                          ? settings.androidTreeLabel || settings.outputDir
                          : "保存至：下载 / DadaDuck送达文书"
                        : settings.outputDir.split(/[/\\]/).filter(Boolean).pop() ||
                          settings.outputDir}
                    </span>
                  </button>
                  <button
                    className="btn btn-sm"
                    disabled={!settings}
                    onClick={editArchive}
                    aria-label="修改保存目录与归档规则"
                  >
                    <SlidersHorizontal size={13} />
                    归档规则
                  </button>
                </div>
                <dl className="archive-rules archive-mobile-hide">
                  <div>
                    <dt>文件命名</dt>
                    <dd title={settings.customTemplate}>
                      {settings.prefixIndex ? "[序号] " : ""}
                      {renameTemplateLabel(
                        settings.renameTemplate,
                        settings.customTemplate
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>目录归类</dt>
                    <dd>
                      {subdirModeLabel(
                        settings.subdirMode,
                        settings.createCaseSubdir
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>法院签收回执</dt>
                    <dd>
                      {{
                        ask: "下载前询问",
                        download_only: "仅下载",
                        submit: "自动提交回执",
                      }[settings.receiptPreference]}
                    </dd>
                  </div>
                </dl>
              </>
            ) : (
              <p className="hint-text">正在读取本机归档设置…</p>
            )}
          </section>
        </div>

        <section className="card queue-card">
          <div className="queue-toolbar">
            <div className="queue-stats">
              <h2 className="card-title">
                <ListOrdered />
                下载队列 <span className="count">{snapshot.totalCount}</span>
              </h2>
              {snapshot.totalCount > 0 && (
                <span className="hint-text">
                  完成 {snapshot.completedCount}/{snapshot.totalCount}
                  {snapshot.pendingCount > 0
                    ? ` · 等待 ${snapshot.pendingCount}`
                    : ""}
                  {snapshot.needsIdentityCount > 0
                    ? ` · 待核验 ${snapshot.needsIdentityCount}`
                    : ""}
                  {snapshot.failedCount > 0
                    ? ` · 失败 ${snapshot.failedCount}`
                    : ""}
                  {snapshot.totalDownloadedDocs > 0
                    ? ` · 已落盘 ${snapshot.totalDownloadedDocs} 份 (${formatBytes(
                        snapshot.totalDownloadedBytes
                      )})`
                    : ""}
                </span>
              )}
            </div>

            <div className="btn-group">
              {!snapshot.isRunning ? (
                <button
                  className="btn btn-primary btn-sm"
                  disabled={!hasPending}
                  onClick={() => queueAction("start_queue")}
                >
                  <Play size={13} />
                  开始下载
                </button>
              ) : (
                <>
                  {settings?.matrixEnabled && matrixMinimized && (
                    <button
                      className="btn btn-sm"
                      onClick={() => setMatrixMinimized(false)}
                    >
                      切回代码雨屏
                    </button>
                  )}
                  {snapshot.isPaused ? (
                    <button
                      className="btn btn-primary btn-sm"
                      onClick={() => queueAction("resume_queue")}
                    >
                      <Play size={13} />
                      继续
                    </button>
                  ) : (
                    <button
                      className="btn btn-warm btn-sm"
                      title="立即暂停当前下载"
                      onClick={() => queueAction("pause_queue")}
                    >
                      <Pause size={13} />
                      暂停
                    </button>
                  )}
                </>
              )}

              {snapshot.isRunning && (
                <button
                  className="btn btn-danger-ghost btn-sm"
                  title="立即停止下载队列"
                  onClick={() => queueAction("cancel_queue")}
                >
                  <Square size={13} />
                  停止
                </button>
              )}

              {snapshot.items.some(
                (i) =>
                  ["failed", "partial"].includes(i.status) &&
                  !!i.url && i.identityAttemptsUsed < i.identityAttemptsLimit
              ) &&
                !snapshot.isRunning && (
                  <button
                    className="btn btn-sm"
                    onClick={() => queueAction("retry_failed")}
                  >
                    <RotateCcw size={13} />
                    重试失败项
                  </button>
                )}

              <button
                className="btn btn-sm"
                disabled={
                  !snapshot.items.some((i) =>
                    ["completed", "browser_required", "failed"].includes(i.status)
                  )
                }
                onClick={() => queueAction("clear_finished")}
              >
                清理已结束
              </button>
            </div>
          </div>

          {snapshot.totalCount > 0 && (
            <div className="overall-progress-wrap">
              <div className="progress-track">
                <div
                  className="progress-fill"
                  style={{ width: `${overallPercent}%` }}
                />
              </div>
              <div className="progress-label">{overallPercent}%</div>
            </div>
          )}

          {fallbackSession && (
            <FallbackControlBar
              session={fallbackSession}
              busy={busy}
              onFillCode={handleFillFallbackCode}
              onScanDocs={handleScanFallbackDocs}
              onFocusWindow={handleFocusFallbackWindow}
              onCloseWindow={handleCloseFallbackWindow}
            />
          )}

          <div className="queue-list">
            {snapshot.items.length === 0 ? (
              <div className="empty-state">
                <span className="tray-icon">
                  <Inbox size={28} strokeWidth={1.45} />
                </span>
                <div>
                  <strong>队列为空</strong>
                  <p>加入短信后，可在这里查看文书下载进度。</p>
                </div>
              </div>
            ) : (
              snapshot.items.map((item, idx) => (
                <QueueItemCard
                  key={item.id}
                  item={item}
                  index={idx}
                  isMobile={isMobile}
                  supportsEmbeddedFallback={supportsEmbeddedFallback}
                  activeFallbackTaskId={fallbackSession?.taskId ?? null}
                  isEditingCase={editingCaseId === item.id}
                  editingCaseValue={editingCaseValue}
                  onStartEditCase={(taskId, currentCase) => {
                    setEditingCaseId(taskId);
                    setEditingCaseValue(currentCase);
                  }}
                  onChangeEditCase={setEditingCaseValue}
                  onSaveEditCase={(taskId) =>
                    void runAction(() => handleSaveCaseNumber(taskId))
                  }
                  onCancelEditCase={() => {
                    setEditingCaseId(null);
                    setEditingCaseValue("");
                  }}
                  expandedDocs={expandedDocs}
                  onToggleExpandDoc={(key) =>
                    setExpandedDocs((prev) => ({ ...prev, [key]: !prev[key] }))
                  }
                  docActionErrors={docActionErrors}
                  confirmDeleteKey={confirmDeleteKey}
                  onSetConfirmDeleteKey={setConfirmDeleteKey}
                  onCopySmsCode={(code) => void handleCopySmsCode(code)}
                  onOpenVerify={(taskId) => {
                    setVerifyId(taskId);
                    setVerifyMethod("sjhm");
                    setVerifyError(null);
                    setVerifyTail("");
                    setVerifyProfileSearch("");
                  }}
                  onOpenFallback={(taskId) => void handleOpenFallback(taskId)}
                  onImportLocalDocs={(taskId) =>
                    void handleImportLocalDocs(taskId)
                  }
                  onOpenBrowser={(url) => void runAction(() => handleOpenExternalBrowser(url))}
                  onRequestSigning={setSigningTaskId}
                  onRemoveItem={(taskId) =>
                    queueAction("remove_queue_item", taskId)
                  }
                  onOpenDoc={(taskId, docIdx, reveal) =>
                    void handleOpenDoc(taskId, docIdx, reveal)
                  }
                  onShareDoc={(taskId, docIdx) =>
                    void handleShareDoc(taskId, docIdx)
                  }
                  onRetrySaveDoc={(taskId, docIdx) =>
                    void runAction(() => handleRetrySaveDoc(taskId, docIdx))
                  }
                  onConfirmDeleteDocFile={(taskId, docIdx) =>
                    void handleConfirmDeleteDocFile(taskId, docIdx)
                  }
                />
              ))
            )}
          </div>
        </section>

        <footer className="desk-footer">
          <span>
            <Monitor size={13} />
            {desktop ? "本机工作区" : "浏览器预览"}
          </span>
          <button className="link-btn" onClick={() => { setShowPrivacy(true); setConfirmClearData(false); }}>
            隐私与数据
          </button>
        </footer>
      </main>

      {showDemo && <DemoDialog native={desktop} onArchive={handleArchiveDemo} onClose={() => setShowDemo(false)} />}
      {signingTask?.url && (
        <Dialog title="前往法院页面签收" onClose={() => setSigningTaskId(null)}>
          <div className="modal-header"><h2>前往法院页面签收</h2></div>
          <div className="modal-body">
            <p>{signingTask.caseNumber || "案号待核对"} · {signingTask.courtName || signingTask.platformLabel}</p>
            <p>请在法院页面核对受送达人和文书再签收。打开或查阅文书也可能影响送达时间，请以法院平台记录为准。</p>
            <p className="hint-text">取件站点 {new URL(signingTask.url).hostname}</p>
          </div>
          <div className="modal-footer">
            <button className="btn" onClick={() => setSigningTaskId(null)}>取消</button>
            <button className="btn btn-primary" disabled={busy} onClick={() => void runAction(async () => {
              await handleOpenExternalBrowser(signingTask.url);
              setSigningTaskId(null);
            })}>打开法院页面</button>
          </div>
        </Dialog>
      )}
      {showPrivacy && (
        <Dialog title="隐私与数据" onClose={() => { if (!busy) setShowPrivacy(false); }}>
          <div className="modal-header"><h2>隐私与数据</h2></div>
          <div className="modal-body">
            <p>小达鸭是独立的文书归档工具。短信在本机解析，取件链接、提取码和常用尾号在本次运行期间使用。</p>
            <p>取件时，必要的链接参数会发送至对应送达平台。仅在你提交核验时向该平台发送尾号。本地归档记录包含案号、文书名称和保存位置。提交签收时会保留回执结果，防止重试时再次提交。</p>
            {confirmClearData && <p>清除队列、历史、暂存文件和本次运行的尾号，并恢复下载前询问签收。已归档的文书会保留。</p>}
          </div>
          <div className="modal-footer">
            <button className="btn" disabled={busy} onClick={() => setShowPrivacy(false)}>关闭</button>
            {desktop && (
              <button className="btn" disabled={busy || snapshot.isRunning} onClick={() => {
                if (!confirmClearData) {
                  setConfirmClearData(true);
                  return;
                }
                void runAction(async () => {
                  const updated = await invoke<AppSettings>("clear_local_data");
                  setSettings(updated);
                  setLastParseSummary(null);
                  setVerifyId(null);
                  setVerifyTail("");
                  setVerifyProfileSearch("");
                  setProfileName("");
                  setProfileMobile4("");
                  setProfileId4("");
                  setBulkProfilesText("");
                  setRawInput("");
                  setBanner("本机记录已清除。");
                  setShowPrivacy(false);
                });
              }}>{confirmClearData ? "确认清除" : "清除本机记录"}</button>
            )}
          </div>
        </Dialog>
      )}

      {archiveDraft && (
        <ArchiveSettingsDialog
          draft={archiveDraft}
          desktop={desktop}
          isMobile={isMobile}
          busy={busy}
          archiveError={archiveError}
          archivePreviewPath={archivePreviewPath}
          onChangeDraft={setArchiveDraft}
          onChooseFolder={() => void runAction(handleChooseFolder)}
          onUseAndroidDownloads={() =>
            void runAction(handleUseAndroidDownloads)
          }
          onPickAndroidDir={() => void runAction(handlePickAndroidDir)}
          onSave={() => void runAction(saveArchive)}
          onClose={() => setArchiveDraft(null)}
        />
      )}

      {showLegacyModal && (
        <LegacyRecoveryDialog
          legacyDocs={legacyDocs}
          legacyCaseOverrides={legacyCaseOverrides}
          legacySelected={legacySelected}
          legacyScanning={legacyScanning}
          legacyExporting={legacyExporting}
          legacySummaryMsg={legacySummaryMsg}
          onSetSelected={setLegacySelected}
          onToggleOne={(path, checked) =>
            setLegacySelected((prev) => ({ ...prev, [path]: checked }))
          }
          onChangeCaseOverride={(path, value) =>
            setLegacyCaseOverrides((prev) => ({ ...prev, [path]: value }))
          }
          onOpenLegacyDoc={(path) =>
            void runAction(() => invoke("open_legacy_document", { path }))
          }
          onExportBatch={(targetPaths) =>
            void handleExportLegacyBatch(targetPaths)
          }
          onClose={() => setShowLegacyModal(false)}
        />
      )}

      {snapshot.isRunning &&
        settings?.matrixEnabled &&
        !matrixMinimized &&
        !verifyTarget && !snapshot.receiptPrompt && (
          <MatrixDownloadOverlay
            snapshot={snapshot}
            activeItem={activeItem}
            overallPercent={overallPercent}
            onPause={() => queueAction("pause_queue")}
            onResume={() => queueAction("resume_queue")}
            onCancel={() => queueAction("cancel_queue")}
            onMinimize={() => setMatrixMinimized(true)}
          />
        )}

      {verifyTarget && (
        <VerifyIdentityDialog
          verifyTarget={verifyTarget}
          settings={settings}
          verifyMethod={verifyMethod}
          verifyTail={verifyTail}
          verifyProfileSearch={verifyProfileSearch}
          verifyError={verifyError}
          verifySubmitting={verifySubmitting}
          filteredVerifyProfiles={filteredVerifyProfiles}
          onMethodChange={setVerifyMethod}
          onTailChange={setVerifyTail}
          onProfileSearchChange={setVerifyProfileSearch}
          onSubmit={handleSubmitVerify}
          supportsEmbeddedFallback={supportsEmbeddedFallback}
          onOpenFallback={(taskId) => void handleOpenFallback(taskId)}
          onOpenBrowser={(url) =>
            void runAction(() =>
              handleOpenExternalBrowser(url)
            )
          }
          onClose={() => setVerifyId(null)}
        />
      )}

      {showProfiles && settings && (
        <ProfilesDialog
          settings={settings}
          desktop={desktop}
          profileSearch={profileSearch}
          filteredProfiles={filteredProfiles}
          profileName={profileName}
          profileMobile4={profileMobile4}
          profileId4={profileId4}
          profileError={profileError}
          showBulkImport={showBulkImport}
          bulkProfilesText={bulkProfilesText}
          profileNameInputRef={profileNameInputRef}
          onSearchChange={setProfileSearch}
          onNameChange={setProfileName}
          onMobile4Change={setProfileMobile4}
          onId4Change={setProfileId4}
          onToggleBulkImport={() => {
            setShowBulkImport((v) => !v);
            setProfileError(null);
          }}
          onBulkTextChange={setBulkProfilesText}
          onSaveProfile={handleSaveProfile}
          onBulkImport={handleBulkImportProfiles}
          onDeleteProfile={(id) =>
            void runAction(() => handleDeleteProfile(id))
          }
          onClose={() => setShowProfiles(false)}
        />
      )}
      {receiptPrompt && (
        <ReceiptDecisionDialog
          key={receiptPrompt.id}
          prompt={receiptPrompt}
          onDecide={async (submit, remember) => {
            const updated = await invoke<AppSettings>("respond_to_receipt", {
              promptId: receiptPrompt.id, submit, remember,
            });
            setSettings(updated);
            if (remember) setArchiveDraft((draft) =>
              draft ? { ...draft, receiptPreference: updated.receiptPreference } : draft
            );
          }}
        />
      )}
    </div>
  );
}
