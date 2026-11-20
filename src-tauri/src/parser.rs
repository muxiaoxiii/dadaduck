use crate::models::{
    DeliveryParams, ItemStatus, ParseBatchSummary, Platform, QueueItem, ReceiptStatus,
    UnrecognizedBlockDiagnostic, MAX_IDENTITY_ATTEMPTS,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;
use url::Url;
use uuid::Uuid;

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"https?://[^\s<>"'\]）】。，；！\u{3000}-\u{303F}\u{4E00}-\u{9FFF}\u{FF00}-\u{FFEF}]+"#)
            .unwrap()
    })
}

fn case_number_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"[（(\[]\d{4}[）)\]][ \t\r\n\u{3000}]*(?:[\u{4e00}-\u{9fa5}A-Za-z0-9]{1,20}[ \t\r\n\u{3000}]*?){1,3}\d+[ \t\u{3000}]*号",
        )
        .unwrap()
    })
}

pub fn normalize_case_number(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(close_pos) = trimmed.find([')', '）', ']']) {
        let close_len = trimmed[close_pos..].chars().next().map_or(1, char::len_utf8);
        let head = &trimmed[..close_pos + close_len];
        let rest = &trimmed[close_pos + close_len..];
        let had_sep = rest.starts_with(|c: char| c.is_whitespace() || c == '\u{3000}');
        let compact_tail: String = rest
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '\u{3000}')
            .collect();
        if had_sep {
            return format!("{} {}", head, compact_tail);
        }
        return format!("{}{}", head, compact_tail);
    }
    trimmed
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n'))
        .collect()
}

pub fn extract_case_number(text: &str) -> Option<String> {
    case_number_regex()
        .find(text)
        .map(|m| normalize_case_number(m.as_str()))
}

pub fn extract_court_name(text: &str) -> Option<String> {
    bracket_court_regex()
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            plain_court_regex()
                .captures(text)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
}

fn bracket_court_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"【([^】]{1,30}?(?:法院|法庭))】").unwrap())
}

fn plain_court_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([\u{4e00}-\u{9fa5}]{2,24}(?:高级|中级|铁路运输|海事|互联网|金融)?人民法院)").unwrap())
}

fn sms_code_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:验证码|提取码|校验码|查收码|送达码|查看码|查阅码|访问码|查看密码|动态码)(?:为|是|[：:\s]){1,3}([A-Za-z0-9]{4,8})",
        )
        .unwrap()
    })
}

fn parties_versus_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"([\u{4e00}-\u{9fa5}A-Za-z0-9（）()·]{2,20}?)(?:与|诉)([\u{4e00}-\u{9fa5}A-Za-z0-9（）()·]{2,20}?)(?:合同|民间借贷|买卖|租赁|劳动|股权|侵权|离婚|物业|金融|借款|纠纷|一案|案件)").unwrap()
    })
}

fn trim_trailing_punct(raw: &str) -> &str {
    raw.trim_end_matches(|c: char| {
        matches!(
            c,
            '.' | ','
                | ';'
                | '!'
                | ')'
                | ']'
                | '。'
                | '，'
                | '；'
                | '！'
                | '）'
                | '】'
                | '>'
                | '"'
                | '\''
                | '：'
                | '、'
                | '？'
        )
    })
}

fn split_sms_blocks(raw_input: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in raw_input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.trim().is_empty() {
                blocks.push(current.trim().to_string());
                current.clear();
            }
            continue;
        }

        // Split inline `【...】` segments only when it clearly starts another SMS message.
        let mut rest = trimmed;
        let mut search_from = rest.chars().next().map_or(0, char::len_utf8);
        while let Some(rel_pos) = rest[search_from..].find('【') {
            let pos = search_from + rel_pos;
            let head = rest[..pos].trim();
            let should_split = !head.is_empty()
                && (url_regex().is_match(head)
                    || url_regex().is_match(&current)
                    || (head.ends_with(['。', '！', '!', ';', '；'])
                        && bracket_court_regex()
                            .find(&rest[pos..])
                            .is_some_and(|m| m.start() == 0)));

            if should_split {
                if !current.is_empty() && (url_regex().is_match(&current) || head.starts_with('【')) {
                    blocks.push(current.trim().to_string());
                    current.clear();
                }
                if !current.is_empty() {
                    current.push('\n');
                }
                current.push_str(head);
                blocks.push(current.trim().to_string());
                current.clear();
                rest = rest[pos..].trim();
                search_from = rest.chars().next().map_or(0, char::len_utf8);
            } else {
                search_from = pos + '【'.len_utf8();
            }
        }

        let starts_new_header = rest.starts_with('【');
        let current_has_url = url_regex().is_match(&current);
        if !current.trim().is_empty() && (starts_new_header || current_has_url) {
            blocks.push(current.trim().to_string());
            current.clear();
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(rest);
    }

    if !current.trim().is_empty() {
        blocks.push(current.trim().to_string());
    }
    blocks
}

enum SingleParseOutcome {
    Recognized(Box<QueueItem>),
    MissingParams,
    UnsupportedHost,
}

pub fn parse_batch_input(raw_input: &str, existing_fingerprints: &HashSet<String>) -> ParseBatchSummary {
    let mut seen = existing_fingerprints.clone();
    let mut added = Vec::new();
    let mut skipped_duplicates = 0usize;
    let mut unrecognized_blocks = Vec::new();
    let mut block_diagnostics = Vec::new();

    for block in split_sms_blocks(raw_input) {
        let urls: Vec<String> = url_regex()
            .find_iter(&block)
            .map(|m| trim_trailing_punct(m.as_str()).to_string())
            .collect();

        if urls.is_empty() {
            let case_number = extract_case_number(&block);
            let court_name = extract_court_name(&block);
            let reason = if case_number.is_some()
                || court_name.is_some()
                || block.contains("送达")
                || block.contains("文书")
                || block.contains("法院")
            {
                "缺少送达链接".to_string()
            } else {
                "未检测到可识别链接".to_string()
            };
            block_diagnostics.push(UnrecognizedBlockDiagnostic {
                raw_text: block.clone(),
                reason,
                case_number,
                court_name,
            });
            unrecognized_blocks.push(block);
            continue;
        }

        let mut block_recognized = false;
        let mut saw_missing_params = false;
        for raw_url in urls {
            match parse_single_delivery_outcome(&block, &raw_url) {
                SingleParseOutcome::Recognized(item) => {
                    block_recognized = true;
                    if seen.contains(&item.fingerprint) {
                        skipped_duplicates += 1;
                    } else {
                        seen.insert(item.fingerprint.clone());
                        added.push(*item);
                    }
                }
                SingleParseOutcome::MissingParams => {
                    saw_missing_params = true;
                }
                SingleParseOutcome::UnsupportedHost => {}
            }
        }
        if !block_recognized {
            let case_number = extract_case_number(&block);
            let court_name = extract_court_name(&block);
            let reason = if saw_missing_params {
                "链接缺少必要送达参数".to_string()
            } else {
                "暂不支持该链接域名".to_string()
            };
            block_diagnostics.push(UnrecognizedBlockDiagnostic {
                raw_text: block.clone(),
                reason,
                case_number,
                court_name,
            });
            unrecognized_blocks.push(block);
        }
    }

    ParseBatchSummary {
        added,
        skipped_duplicates,
        unrecognized_lines: unrecognized_blocks.len(),
        unrecognized_blocks,
        block_diagnostics,
    }
}

#[cfg(test)]
fn parse_single_delivery(context: &str, raw_url: &str) -> Option<QueueItem> {
    match parse_single_delivery_outcome(context, raw_url) {
        SingleParseOutcome::Recognized(item) => Some(*item),
        _ => None,
    }
}

fn parse_single_delivery_outcome(context: &str, raw_url: &str) -> SingleParseOutcome {
    let Ok(parsed_url) = Url::parse(raw_url) else {
        return SingleParseOutcome::UnsupportedHost;
    };
    let Some(host_raw) = parsed_url.host_str() else {
        return SingleParseOutcome::UnsupportedHost;
    };
    let host = host_raw.to_ascii_lowercase();
    let path_and_frag = format!(
        "{}{}",
        parsed_url.path(),
        parsed_url
            .fragment()
            .map(|f| format!("#{}", f))
            .unwrap_or_default()
    );

    let (platform, params, fingerprint, initial_status, status_message) =
        if host == "zxfw.court.gov.cn" {
            let Some(params) = extract_zxfw_params(&parsed_url) else {
                return SingleParseOutcome::MissingParams;
            };
            let fp = compute_zxfw_fingerprint(&params);
            (
                Platform::Zxfw,
                params,
                fp,
                ItemStatus::Pending,
                "等待下载".to_string(),
            )
        } else if host == "dzsd.hbfy.gov.cn" {
            if let Some(msg) = extract_hubei_msg(&parsed_url) {
                let params = DeliveryParams {
                    msg: Some(msg.clone()),
                    ..Default::default()
                };
                let fp = sha256_hex(&format!("hubei:{}", msg));
                (
                    Platform::HubeiPublic,
                    params,
                    fp,
                    ItemStatus::Pending,
                    "等待下载".to_string(),
                )
            } else if path_and_frag.contains("/sfsddz") {
                let fp = sha256_hex(raw_url);
                (
                    Platform::HubeiAccount,
                    DeliveryParams::default(),
                    fp,
                    ItemStatus::BrowserRequired,
                    "湖北账号模式需在浏览器登录处理".to_string(),
                )
            } else {
                return SingleParseOutcome::MissingParams;
            }
        } else if host == "sd.gdems.com" {
            let fp = sha256_hex(raw_url);
            (
                Platform::Gdems,
                DeliveryParams::default(),
                fp,
                ItemStatus::BrowserRequired,
                "广东平台无公开API，需点击用浏览器打开".to_string(),
            )
        } else if host == "jysd.10102368.com" {
            let fp = sha256_hex(raw_url);
            (
                Platform::Jysd,
                DeliveryParams::default(),
                fp,
                ItemStatus::BrowserRequired,
                "集约送达平台需点击用浏览器打开".to_string(),
            )
        } else if host == "sfpt.cdfy12368.gov.cn" || host == "171.106.48.55" {
            let fp = sha256_hex(raw_url);
            (
                Platform::Sfpt,
                DeliveryParams::default(),
                fp,
                ItemStatus::BrowserRequired,
                "司法送达网前端加密，需点击用浏览器打开".to_string(),
            )
        } else {
            return SingleParseOutcome::UnsupportedHost;
        };

    let case_number = extract_case_number(context);
    let court_name = extract_court_name(context);
    let context_without_urls = url_regex().replace_all(context, " ");
    let sms_code = sms_code_regex()
        .captures(&context_without_urls)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());
    let parties = parties_versus_regex()
        .captures(context)
        .map(|c| {
            vec![
                c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
                c.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect()
        })
        .unwrap_or_default();

    SingleParseOutcome::Recognized(Box::new(QueueItem {
        id: Uuid::new_v4().to_string(),
        raw_text: context.to_string(),
        platform_label: platform.display_name().to_string(),
        platform,
        url: raw_url.to_string(),
        params,
        fingerprint,
        case_number,
        court_name,
        parties,
        sms_code,
        status: initial_status,
        status_message,
        stage_label: String::new(),
        receipt_status: ReceiptStatus::NotRequested,
        identity_attempts_used: 0,
        identity_attempts_limit: MAX_IDENTITY_ATTEMPTS,
        documents: Vec::new(),
        failures: Vec::new(),
        current_doc_index: 0,
        current_doc_total: 0,
        current_doc_name: None,
        current_target_file_name: None,
        bytes_received: 0,
        total_bytes: None,
        speed_bytes_per_sec: 0,
    }))
}

pub fn extract_zxfw_params(url: &Url) -> Option<DeliveryParams> {
    let mut candidates = Vec::new();
    if let Some(q) = url.query() {
        candidates.push(q.to_string());
    }
    if let Some(frag) = url.fragment() {
        if let Some((_, frag_q)) = frag.split_once('?') {
            candidates.push(frag_q.to_string());
        }
    }

    let mut params = DeliveryParams::default();
    for query_str in candidates {
        for (k, v) in url::form_urlencoded::parse(query_str.as_bytes()) {
            match k.as_ref() {
                "qdbh" if !v.is_empty() => params.qdbh = Some(v.into_owned()),
                "sdbh" if !v.is_empty() => params.sdbh = Some(v.into_owned()),
                "sdsin" if !v.is_empty() => params.sdsin = Some(v.into_owned()),
                "mm" if !v.is_empty() => params.mm = Some(v.into_owned()),
                _ => {}
            }
        }
    }

    if params.qdbh.is_some() && params.sdbh.is_some() && params.sdsin.is_some() {
        Some(params)
    } else {
        None
    }
}

fn extract_hubei_msg(url: &Url) -> Option<String> {
    let full = url.as_str();
    let marker = "/hb/msg=";
    let pos = full.find(marker)?;
    let tail = &full[pos + marker.len()..];
    let msg: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '%'))
        .collect();
    if msg.is_empty() {
        None
    } else {
        Some(msg)
    }
}

fn compute_zxfw_fingerprint(params: &DeliveryParams) -> String {
    let map: BTreeMap<String, String> = params.to_zxfw_payload();
    let json = serde_json::to_string(&map).unwrap_or_default();
    sha256_hex(&format!("zxfw:{}", json))
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_platforms_by_their_configured_hosts() {
        assert!(parse_single_delivery("", "https://example.invalid/v3/dzsd/demo").is_none());
        assert!(parse_single_delivery("", "https://sd.gdems.com/v3/dzsd/demo").is_some());
    }

    #[test]
    fn parses_multiple_mixed_sms_and_deduplicates() {
        let input = r#"
【苏州工业园区人民法院】张三，您好！您有（2026）苏0591民初100号案件文书送达，请点击链接查收：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1。
https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1
【上海浦东新区人民法院】李四，您有(2026)沪0115民初200号文书：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q2&sdbh=S2&sdsin=X2&mm=M2
http://dzsd.hbfy.gov.cn/hb/msg=ABC12345
https://sd.gdems.com/v3/dzsd/xyz999
"#;
        let existing = HashSet::new();
        let res = parse_batch_input(input, &existing);
        assert_eq!(res.added.len(), 4);
        assert_eq!(res.skipped_duplicates, 1);
        assert_eq!(res.added[0].platform, Platform::Zxfw);
        assert_eq!(
            res.added[0].case_number.as_deref(),
            Some("（2026）苏0591民初100号")
        );
        assert_eq!(
            res.added[0].court_name.as_deref(),
            Some("苏州工业园区人民法院")
        );
        assert_eq!(res.added[1].params.mm.as_deref(), Some("M2"));
        assert_eq!(res.added[2].platform, Platform::HubeiPublic);
        assert_eq!(res.added[2].params.msg.as_deref(), Some("ABC12345"));
        assert_eq!(res.added[3].platform, Platform::Gdems);
        assert_eq!(res.added[3].status, ItemStatus::BrowserRequired);
    }

    #[test]
    fn does_not_bleed_case_info_from_prior_sms_without_link() {
        let input = r#"【甲法院】张三，您有（2026）甲民初1号开庭提醒，请准时到庭。
【乙法院】李四，您有（2026）乙民初2号文书送达：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q2&sdbh=S2&sdsin=X2"#;
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 1);
        assert_eq!(res.unrecognized_lines, 1);
        assert_eq!(res.unrecognized_blocks.len(), 1);
        assert_eq!(res.added[0].court_name.as_deref(), Some("乙法院"));
        assert_eq!(res.added[0].case_number.as_deref(), Some("（2026）乙民初2号"));
    }

    #[test]
    fn keeps_multiline_sms_without_header_attached_to_next_url_line() {
        let input = "示例收件人你好，请查收 (2026) 示73行初90001号案件中你的送达文书\nhttps://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q9&sdbh=S9&sdsin=X9";
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 1);
        assert_eq!(res.unrecognized_lines, 0);
        assert_eq!(res.unrecognized_blocks.len(), 0);
        assert_eq!(
            res.added[0].case_number.as_deref(),
            Some("(2026) 示73行初90001号")
        );
    }

    #[test]
    fn normalizes_fullwidth_space_and_newline_in_case_number_and_reports_missing_link() {
        let input = "【甲法院】通知：（2026）\u{3000}粤03民初12号开庭提醒，无链接。\n\
                     【乙法院】送达：(2026)\n示73行初90001号案件文书：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q5&sdbh=S5&sdsin=X5";
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 1);
        assert_eq!(
            res.added[0].case_number.as_deref(),
            Some("(2026) 示73行初90001号")
        );
        assert_eq!(res.block_diagnostics.len(), 1);
        assert_eq!(res.block_diagnostics[0].reason, "缺少送达链接");
        assert_eq!(
            res.block_diagnostics[0].case_number.as_deref(),
            Some("（2026） 粤03民初12号")
        );
    }

    #[test]
    fn stops_url_before_adjacent_cjk_text_and_keeps_inline_non_court_brackets() {
        let input = "【北京知识产权法院】通知：（2026）京73行初100号案件【传票】请点击https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1查看文书";
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 1);
        assert_eq!(res.unrecognized_lines, 0);
        assert_eq!(res.added[0].court_name.as_deref(), Some("北京知识产权法院"));
        assert_eq!(
            res.added[0].case_number.as_deref(),
            Some("（2026）京73行初100号")
        );
        assert_eq!(res.added[0].params.sdsin.as_deref(), Some("X1"));
        assert_eq!(
            res.added[0].url,
            "https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1&sdbh=S1&sdsin=X1"
        );
    }

    #[test]
    fn splits_two_inline_sms_when_separated_by_url_or_sentence_end_plus_court_header() {
        let input = "【甲法院】通知：（2026）甲民初1号开庭提醒。【乙法院】送达：（2026）乙民初2号文书：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q2&sdbh=S2&sdsin=X2【丙法院】送达：（2026）丙民初3号：https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q3&sdbh=S3&sdsin=X3";
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 2);
        assert_eq!(res.unrecognized_lines, 1);
        assert_eq!(res.added[0].court_name.as_deref(), Some("乙法院"));
        assert_eq!(res.added[1].court_name.as_deref(), Some("丙法院"));
        assert_eq!(res.block_diagnostics[0].reason, "缺少送达链接");
    }

    #[test]
    fn distinguishes_missing_params_and_supports_hubei_tokens() {
        let input = "https://zxfw.court.gov.cn/zxfw/#/pagesAjkj/app/wssd/index?qdbh=Q1\n\
                     https://dzsd.hbfy.gov.cn/hb/msg=AB_cd-12%3D";
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 1);
        assert_eq!(res.added[0].platform, Platform::HubeiPublic);
        assert_eq!(res.added[0].params.msg.as_deref(), Some("AB_cd-12%3D"));
        assert_eq!(res.block_diagnostics.len(), 1);
        assert_eq!(res.block_diagnostics[0].reason, "链接缺少必要送达参数");
    }

    #[test]
    fn extracts_diverse_sms_codes_without_capturing_url_tokens() {
        let input = r#"
【广东某法院】提取码为 A3B9，请查收：https://sd.gdems.com/v3/dzsd/abc?code=9999
【测试法院】请凭查阅码: 998877 打开 https://jysd.10102368.com/view?token=1
【湖北某法院】验证码：839201，登录地址：https://dzsd.hbfy.gov.cn/sfsddz/login
"#;
        let res = parse_batch_input(input, &HashSet::new());
        assert_eq!(res.added.len(), 3);
        assert_eq!(res.added[0].sms_code.as_deref(), Some("A3B9"));
        assert_eq!(res.added[1].sms_code.as_deref(), Some("998877"));
        assert_eq!(res.added[2].sms_code.as_deref(), Some("839201"));

        let url_only = "https://sd.gdems.com/v3/dzsd/abc?验证码=1234";
        let res2 = parse_batch_input(url_only, &HashSet::new());
        assert_eq!(res2.added.len(), 1);
        assert!(res2.added[0].sms_code.is_none());
    }
}
