use crate::models::{RenameTemplate, SubdirMode};
use chrono::Local;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use url::Url;

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn sanitize_component(input: &str, max_bytes: usize, fallback: &str) -> String {
    let mut cleaned = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }

    let trimmed = cleaned.trim_matches(|c: char| c == ' ' || c == '.');
    let mut result = truncate_utf8_bytes(trimmed, max_bytes).to_string();
    result = result
        .trim_matches(|c: char| c == ' ' || c == '.')
        .to_string();

    if result.is_empty() {
        return fallback.to_string();
    }

    let upper = result.to_ascii_uppercase();
    let stem_upper = upper.split('.').next().unwrap_or("");
    if WINDOWS_RESERVED.contains(&stem_upper) {
        if let Some(dot_pos) = result.find('.') {
            result.insert(dot_pos, '_');
        } else {
            result.push('_');
        }
    }
    result
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

pub fn safe_extension(raw_format: Option<&str>, download_url: &str) -> String {
    let normalize_candidate = |raw: &str| -> Option<String> {
        let candidate = raw.trim().trim_start_matches('.').to_ascii_lowercase();
        if !candidate.is_empty()
            && candidate.len() <= 10
            && candidate.chars().all(|c| c.is_ascii_alphanumeric())
        {
            Some(format!(".{}", candidate))
        } else {
            None
        }
    };

    if let Some(fmt) = raw_format {
        if let Some(ext) = normalize_candidate(fmt) {
            return ext;
        }
    }
    let path_str = Url::parse(download_url)
        .ok()
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|| download_url.to_string());
    if let Some(ext) = Path::new(&path_str).extension().and_then(|e| e.to_str()) {
        if let Some(norm) = normalize_candidate(ext) {
            return norm;
        }
    }
    ".pdf".to_string()
}

pub fn resolve_subdir_segments(
    subdir_mode: &SubdirMode,
    case_number: Option<&str>,
    court_name: Option<&str>,
    date_str: &str,
) -> Vec<String> {
    let case_dir = case_number
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_component(s, 120, "未知案号"));
    let court_dir = court_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_component(s, 90, "法院送达"));

    match subdir_mode {
        SubdirMode::None => Vec::new(),
        SubdirMode::CaseNumber => {
            let folder = case_dir
                .or_else(|| court_dir.map(|c| format!("{}_{}", c, date_str)))
                .unwrap_or_else(|| format!("未识别案号_{}", date_str));
            vec![folder]
        }
        SubdirMode::CourtAndCase => {
            let first = court_dir.unwrap_or_else(|| "法院送达".to_string());
            let second = case_dir.unwrap_or_else(|| format!("未识别案号_{}", date_str));
            vec![first, second]
        }
        SubdirMode::Date => vec![date_str.to_string()],
    }
}

pub fn resolve_target_dir(
    base_dir: &Path,
    subdir_mode: &SubdirMode,
    case_number: Option<&str>,
    court_name: Option<&str>,
    date_str: &str,
) -> PathBuf {
    let mut out = base_dir.to_path_buf();
    for seg in resolve_subdir_segments(subdir_mode, case_number, court_name, date_str) {
        out = out.join(seg);
    }
    out
}

pub fn preview_sample_archive_path(
    base_label: &str,
    subdir_mode: &SubdirMode,
    template: &RenameTemplate,
    custom_template: &str,
    prefix_index: bool,
) -> String {
    let now = Local::now();
    let today = now.format("%Y%m%d").to_string();
    let today_dash = now.format("%Y-%m-%d").to_string();
    let now_time = now.format("%H%M%S").to_string();
    let sample_parties = vec!["张三".to_string(), "李四".to_string()];

    let subdirs = resolve_subdir_segments(
        subdir_mode,
        Some("(2026) 示73行初90001号"),
        Some("北京知识产权法院"),
        &today,
    );
    let stem = render_stem(
        template,
        custom_template,
        prefix_index,
        &RenameContext {
            raw_title: "诉讼参与人举证材料.pdf",
            case_number: Some("(2026) 示73行初90001号"),
            court_name: Some("北京知识产权法院"),
            parties: &sample_parties,
            date_str: Some(&today),
            date_dash_str: Some(&today_dash),
            time_str: Some(&now_time),
            index: 1,
            total_docs: 2,
            extension: ".pdf",
        },
    );
    let clean_stem = strip_trailing_ext(&stem, "pdf");
    let file_name = format!("{}.pdf", clean_stem);

    let base_trimmed = base_label.trim_end_matches(&['/', '\\'][..]);
    let mut parts = Vec::with_capacity(2 + subdirs.len());
    if !base_trimmed.is_empty() {
        parts.push(base_trimmed.to_string());
    }
    parts.extend(subdirs);
    parts.push(file_name);
    parts.join("/")
}

pub struct RenameContext<'a> {
    pub raw_title: &'a str,
    pub case_number: Option<&'a str>,
    pub court_name: Option<&'a str>,
    pub parties: &'a [String],
    pub date_str: Option<&'a str>,
    pub date_dash_str: Option<&'a str>,
    pub time_str: Option<&'a str>,
    pub index: usize,
    pub total_docs: usize,
    pub extension: &'a str,
}

pub fn render_stem(
    template: &RenameTemplate,
    custom_template: &str,
    prefix_index: bool,
    ctx: &RenameContext<'_>,
) -> String {
    let now = Local::now();
    let today = now.format("%Y%m%d").to_string();
    let today_dash = now.format("%Y-%m-%d").to_string();
    let now_time = now.format("%H%M%S").to_string();

    let date = ctx.date_str.unwrap_or(&today);
    let date_dash = ctx.date_dash_str.unwrap_or(&today_dash);
    let time = ctx.time_str.unwrap_or(&now_time);

    let ext_no_dot = ctx.extension.trim_start_matches('.');
    let title_no_ext = strip_trailing_ext(ctx.raw_title.trim(), ext_no_dot);
    let clean_title = sanitize_component(&title_no_ext, 120, "document");
    let case_no = ctx
        .case_number
        .map(|s| sanitize_component(s, 80, ""))
        .unwrap_or_default();
    let court = ctx
        .court_name
        .map(|s| sanitize_component(s, 60, ""))
        .unwrap_or_default();
    let parties_raw = ctx.parties.join("、");
    let parties = sanitize_component(&parties_raw, 60, "");
    let parties_clean = if parties_raw.trim().is_empty() {
        String::new()
    } else {
        parties
    };
    let idx2 = format!("{:02}", ctx.index);

    let mut rendered = match template {
        RenameTemplate::Simple => format!("{}_{}", clean_title, date),
        RenameTemplate::CaseArchive => collapse_separators(&format!("{}_{}_{}", case_no, clean_title, date)),
        RenameTemplate::CourtCaseTitle => {
            collapse_separators(&format!("{}_{}_{}_{}", court, case_no, clean_title, date))
        }
        RenameTemplate::DateCaseTitle => collapse_separators(&format!("{}_{}_{}", date, case_no, clean_title)),
        RenameTemplate::TitleCaseParen => {
            if case_no.is_empty() {
                format!("{}_{}", clean_title, date)
            } else {
                format!("{}（{}）_{}", clean_title, case_no, date)
            }
        }
        RenameTemplate::IndexTitle => format!("{}_{}", idx2, clean_title),
        RenameTemplate::Original => clean_title.clone(),
        RenameTemplate::Custom => {
            let pattern = if custom_template.trim().is_empty() {
                "{case_no}_{title}_{date}"
            } else {
                custom_template
            };
            let replaced = pattern
                .replace("{title}", &clean_title)
                .replace("{raw_title}", &clean_title)
                .replace("{case_no}", &case_no)
                .replace("{court}", &court)
                .replace("{parties}", &parties_clean)
                .replace("{date}", date)
                .replace("{date_dash}", date_dash)
                .replace("{time}", time)
                .replace("{index2}", &idx2)
                .replace("{index}", &ctx.index.to_string());
            collapse_separators(&replaced)
        }
    };

    if prefix_index && ctx.total_docs > 1 && *template != RenameTemplate::IndexTitle {
        rendered = format!("{}_{}", idx2, rendered);
    }

    sanitize_component(&rendered, 175, &clean_title)
}

pub fn strip_trailing_ext(name: &str, ext_no_dot: &str) -> String {
    if ext_no_dot.is_empty() {
        return name.to_string();
    }
    let suffix = format!(".{}", ext_no_dot);
    if name.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()) {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name.to_string()
    }
}

fn collapse_separators(input: &str) -> String {
    let mut out = input.to_string();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.replace("（）", "")
        .replace("()", "")
        .trim_matches(|c: char| c == '_' || c == '-' || c == ' ')
        .to_string()
}

pub fn unique_destination(
    dir: &Path,
    stem: &str,
    extension: &str,
    used_names: &mut HashSet<String>,
) -> PathBuf {
    let ext = if extension.starts_with('.') {
        extension.to_string()
    } else {
        format!(".{}", extension)
    };
    let clean_stem = strip_trailing_ext(stem, ext.trim_start_matches('.'));
    let mut candidate_name = format!("{}{}", clean_stem, ext);
    let mut serial = 2usize;

    loop {
        let lower = candidate_name.to_ascii_lowercase();
        let candidate_path = dir.join(&candidate_name);
        if !used_names.contains(&lower) && !candidate_path.exists() {
            used_names.insert(lower);
            return candidate_path;
        }
        candidate_name = format!("{}_{}{}", clean_stem, serial, ext);
        serial += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitizes_cross_platform_illegal_chars() {
        assert_eq!(
            sanitize_component(r#"传票<第3>:测试/文件*?.pdf"#, 180, "doc"),
            "传票_第3__测试_文件__.pdf"
        );
        assert_eq!(sanitize_component("CON", 180, "doc"), "CON_");
    }

    #[test]
    fn renders_rich_templates_and_custom_placeholders() {
        let parties = vec!["张三".to_string(), "李四".to_string()];
        let ctx_with_case = RenameContext {
            raw_title: "诉讼参与人举证材料.pdf",
            case_number: Some("（2026）苏05民初123号"),
            court_name: Some("苏州工业园区人民法院"),
            parties: &parties,
            date_str: Some("20261118"),
            date_dash_str: Some("2026-11-18"),
            time_str: Some("120500"),
            index: 2,
            total_docs: 3,
            extension: ".pdf",
        };
        assert_eq!(
            render_stem(&RenameTemplate::CaseArchive, "", false, &ctx_with_case),
            "（2026）苏05民初123号_诉讼参与人举证材料_20261118"
        );
        assert_eq!(
            render_stem(&RenameTemplate::CourtCaseTitle, "", true, &ctx_with_case),
            "02_苏州工业园区人民法院_（2026）苏05民初123号_诉讼参与人举证材料_20261118"
        );
        assert_eq!(
            render_stem(
                &RenameTemplate::Custom,
                "{date_dash}_{parties}_{title}_{index2}",
                false,
                &ctx_with_case
            ),
            "2026-11-18_张三、李四_诉讼参与人举证材料_02"
        );

        let ctx_no_case = RenameContext {
            case_number: None,
            ..ctx_with_case
        };
        assert_eq!(
            render_stem(&RenameTemplate::CaseArchive, "", false, &ctx_no_case),
            "诉讼参与人举证材料_20261118"
        );
    }

    #[test]
    fn resolves_subdir_modes() {
        let base = Path::new("/tmp/docs");
        assert_eq!(
            resolve_target_dir(
                base,
                &SubdirMode::CourtAndCase,
                Some("（2026）沪01民初1号"),
                Some("浦东法院"),
                "20261118"
            ),
            PathBuf::from("/tmp/docs/浦东法院/（2026）沪01民初1号")
        );
        assert_eq!(
            resolve_target_dir(base, &SubdirMode::Date, None, None, "20261118"),
            PathBuf::from("/tmp/docs/20261118")
        );
    }

    #[test]
    fn deduplicates_case_insensitively() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("传票_20261118收.pdf"), b"x").unwrap();
        let mut used = HashSet::new();
        let dest = unique_destination(tmp.path(), "传票_20261118收", ".pdf", &mut used);
        assert_eq!(dest.file_name().unwrap().to_str().unwrap(), "传票_20261118收_2.pdf");
    }

    #[test]
    fn extracts_extension_from_local_filenames_and_strips_ext_before_truncating() {
        assert_eq!(safe_extension(None, "证据材料.docx"), ".docx");
        assert_eq!(safe_extension(None, "送达文书.ofd"), ".ofd");
        assert_eq!(sanitize_component("CON.foo", 180, "doc"), "CON_.foo");

        let long_title = format!("{}.pdf", "文".repeat(45));
        let stem = render_stem(
            &RenameTemplate::Original,
            "",
            false,
            &RenameContext {
                raw_title: &long_title,
                case_number: None,
                court_name: None,
                parties: &[],
                date_str: Some("20261119"),
                date_dash_str: Some("2026-11-19"),
                time_str: Some("100000"),
                index: 1,
                total_docs: 1,
                extension: ".pdf",
            },
        );
        assert!(!stem.ends_with(".p"));
        assert!(!stem.ends_with(".pd"));
        assert!(!stem.ends_with(".pdf"));
    }
}
