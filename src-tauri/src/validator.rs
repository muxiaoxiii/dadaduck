use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("文件为空")]
    EmptyFile,
    #[error("返回了 HTML 页面而非文书文件")]
    HtmlInsteadOfDocument,
    #[error("返回了 JSON 响应而非二进制文书")]
    UnverifiedJsonDocument,
    #[error("不支持的文件格式: {0}")]
    UnsupportedFormat(String),
    #[error("文件头魔数与扩展名不匹配")]
    InvalidHeader,
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
}

pub fn validate_file(path: &Path, extension: &str) -> Result<u64, ValidationError> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    if size == 0 {
        return Err(ValidationError::EmptyFile);
    }

    let mut file = File::open(path)?;
    let mut prefix = [0u8; 1024];
    let n = file.read(&mut prefix)?;
    let prefix = &prefix[..n];

    let without_bom = prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(prefix);
    let trimmed = trim_ascii_start(without_bom);
    let lower_head = String::from_utf8_lossy(&trimmed[..trimmed.len().min(512)]).to_ascii_lowercase();

    if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
        return Err(ValidationError::HtmlInsteadOfDocument);
    }

    let ext = extension.trim().trim_start_matches('.').to_ascii_lowercase();
    if ext == "json" {
        return Err(ValidationError::UnverifiedJsonDocument);
    }

    if !matches!(ext.as_str(), "rtf" | "txt")
        && trimmed.starts_with(b"{")
        && ["\"code\"", "\"msg\"", "\"message\"", "\"error\""]
            .iter()
            .any(|k| lower_head.contains(k))
    {
        return Err(ValidationError::UnverifiedJsonDocument);
    }

    let valid = match ext.as_str() {
        "pdf" => prefix.windows(5).any(|w| w == b"%PDF-"),
        "docx" | "xlsx" | "pptx" | "ofd" | "zip" => {
            prefix.starts_with(b"PK\x03\x04")
                || prefix.starts_with(b"PK\x05\x06")
                || prefix.starts_with(b"PK\x07\x08")
        }
        "doc" | "xls" | "ppt" | "wps" => {
            prefix.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1")
                || prefix.starts_with(b"PK\x03\x04")
                || (matches!(ext.as_str(), "doc" | "wps") && trimmed.starts_with(b"{\\rtf"))
        }
        "rtf" => trimmed.starts_with(b"{\\rtf"),
        "rar" => prefix.starts_with(b"Rar!\x1A\x07"),
        "7z" => prefix.starts_with(b"7z\xBC\xAF\x27\x1C"),
        "webp" => prefix.len() >= 12 && &prefix[0..4] == b"RIFF" && &prefix[8..12] == b"WEBP",
        "txt" => !prefix.contains(&0),
        "jpg" | "jpeg" => prefix.starts_with(b"\xFF\xD8\xFF"),
        "png" => prefix.starts_with(b"\x89PNG\r\n\x1A\n"),
        "gif" => prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a"),
        "tif" | "tiff" => prefix.starts_with(b"II*\x00") || prefix.starts_with(b"MM\x00*"),
        "bmp" => prefix.starts_with(b"BM"),
        other => return Err(ValidationError::UnsupportedFormat(other.to_string())),
    };

    if !valid {
        return Err(ValidationError::InvalidHeader);
    }

    Ok(size)
}

pub fn sniff_extension_from_file(path: &Path) -> Option<&'static str> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 1024];
    let n = file.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    let prefix = &buf[..n];
    let without_bom = prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(prefix);
    let trimmed = trim_ascii_start(without_bom);
    let lower_head = String::from_utf8_lossy(&trimmed[..trimmed.len().min(256)]).to_ascii_lowercase();
    if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
        return None;
    }

    if prefix.windows(5).any(|w| w == b"%PDF-") {
        Some(".pdf")
    } else if prefix.starts_with(b"\xFF\xD8\xFF") {
        Some(".jpg")
    } else if prefix.starts_with(b"\x89PNG\r\n\x1A\n") {
        Some(".png")
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        Some(".gif")
    } else if prefix.len() >= 12 && &prefix[0..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        Some(".webp")
    } else if prefix.starts_with(b"BM") {
        Some(".bmp")
    } else if prefix.starts_with(b"II*\x00") || prefix.starts_with(b"MM\x00*") {
        Some(".tif")
    } else if prefix.starts_with(b"Rar!\x1A\x07") {
        Some(".rar")
    } else if prefix.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(".7z")
    } else if trimmed.starts_with(b"{\\rtf") {
        Some(".rtf")
    } else if prefix.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        Some(".doc")
    } else if prefix.starts_with(b"PK\x03\x04")
        || prefix.starts_with(b"PK\x05\x06")
        || prefix.starts_with(b"PK\x07\x08")
    {
        Some(".zip")
    } else {
        None
    }
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let idx = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .unwrap_or(bytes.len());
    &bytes[idx..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn accepts_valid_pdf_header() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"%PDF-1.7\nhello").unwrap();
        assert_eq!(validate_file(tmp.path(), ".pdf").unwrap(), 14);
    }

    #[test]
    fn rejects_html_disguised_as_pdf() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"  <!DOCTYPE html><html>error</html>").unwrap();
        let err = validate_file(tmp.path(), ".pdf").unwrap_err();
        assert!(matches!(err, ValidationError::HtmlInsteadOfDocument));
    }

    #[test]
    fn rejects_empty_file() {
        let tmp = NamedTempFile::new().unwrap();
        let err = validate_file(tmp.path(), "pdf").unwrap_err();
        assert!(matches!(err, ValidationError::EmptyFile));
    }

    #[test]
    fn accepts_ofd_zip_magic() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"PK\x03\x04ofdcontent").unwrap();
        assert!(validate_file(tmp.path(), "ofd").is_ok());
    }

    #[test]
    fn rejects_json_error_disguised_as_pdf_and_accepts_extended_formats() {
        let mut json_pdf = NamedTempFile::new().unwrap();
        json_pdf
            .write_all(br#"{"code":500,"msg":"token expired"}"#)
            .unwrap();
        assert!(matches!(
            validate_file(json_pdf.path(), "pdf").unwrap_err(),
            ValidationError::UnverifiedJsonDocument
        ));

        let mut wps = NamedTempFile::new().unwrap();
        wps.write_all(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1wps").unwrap();
        assert!(validate_file(wps.path(), "wps").is_ok());

        let mut rtf = NamedTempFile::new().unwrap();
        rtf.write_all(b"{\\rtf1\\ansi doc}").unwrap();
        assert!(validate_file(rtf.path(), "rtf").is_ok());

        let mut rar = NamedTempFile::new().unwrap();
        rar.write_all(b"Rar!\x1A\x07\x01\x00data").unwrap();
        assert!(validate_file(rar.path(), "rar").is_ok());

        let mut webp = NamedTempFile::new().unwrap();
        webp.write_all(b"RIFF\x12\x00\x00\x00WEBPVP8 ").unwrap();
        assert!(validate_file(webp.path(), "webp").is_ok());

        let mut ooxml_doc = NamedTempFile::new().unwrap();
        ooxml_doc.write_all(b"PK\x03\x04word/").unwrap();
        assert!(validate_file(ooxml_doc.path(), "doc").is_ok());
    }

    #[test]
    fn sniffs_common_document_formats_and_rejects_html() {
        let mut pdf = NamedTempFile::new().unwrap();
        pdf.write_all(b"%PDF-1.6\nbody").unwrap();
        assert_eq!(sniff_extension_from_file(pdf.path()), Some(".pdf"));

        let mut png = NamedTempFile::new().unwrap();
        png.write_all(b"\x89PNG\r\n\x1A\nbody").unwrap();
        assert_eq!(sniff_extension_from_file(png.path()), Some(".png"));

        let mut zip = NamedTempFile::new().unwrap();
        zip.write_all(b"PK\x03\x04archive").unwrap();
        assert_eq!(sniff_extension_from_file(zip.path()), Some(".zip"));

        let mut html = NamedTempFile::new().unwrap();
        html.write_all(b"  <!DOCTYPE html><html>err</html>").unwrap();
        assert_eq!(sniff_extension_from_file(html.path()), None);
    }
}
