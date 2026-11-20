import type { AppSettings } from "./types";

export const previewSettings: AppSettings = {
  outputDir: "~/Downloads/DadaDuck送达文书",
  createCaseSubdir: true,
  subdirMode: "case_number",
  renameTemplate: "case_archive",
  customTemplate: "{case_no}_{title}_{date}",
  prefixIndex: false,
  receiptPreference: "ask",
  matrixEnabled: false,
  requestDelayMs: 400,
  profiles: [],
};
