//! Conversion dispatcher.
//!
//! Three tiers, in order of preference:
//!   1. Pure Rust converters (no Python at all) for the common formats.
//!   2. Optional `markitdown` CLI, for users who want Microsoft's exact output.
//!   3. OCR for images and scanned PDFs (Apple Vision on macOS, Tesseract
//!      as portable fallback).
//!
//! Adding another OCR backend only requires extending `ocr::ocr_image`.

use std::path::Path;
use std::process::Command;

use uuid::Uuid;

use crate::{documents, ocr};

pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tif", "tiff"];
pub const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "doc", "pptx", "ppt", "xlsx", "xls", "html", "htm", "csv", "json", "xml",
    "txt", "md", "markdown",
];

#[derive(Debug, Clone)]
pub struct Options {
    /// auto | vision | tesseract | winocr
    pub engine: String,
    /// Apple language identifiers, Chinese first. See README.
    pub languages: String,
    pub dpi: u32,
    pub prefer_markitdown: bool,
    pub ocr_scanned_pdf: bool,
}

/// The OCR engine chosen at install time. The installer writes it into the
/// launcher script; the WebUI still lets the user override it per conversion.
/// macOS: `auto` | `vision` | `tesseract`. Windows: `auto` | `tesseract` | `winocr`.
pub fn configured_engine() -> String {
    let candidate = std::env::var("MARKITDOWN_OCR_ENGINE").unwrap_or_default();
    if ["auto", "vision", "tesseract", "winocr"].contains(&candidate.as_str()) {
        candidate
    } else {
        "auto".to_string()
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            engine: configured_engine(),
            // Ordering matters: a Chinese identifier MUST come first on recent
            // macOS, otherwise Vision silently degrades mixed CJK/Latin text.
            languages: "zh-Hans,en-US".to_string(),
            dpi: 200,
            prefer_markitdown: false,
            ocr_scanned_pdf: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Conversion {
    pub markdown: String,
    pub engine: String,
    pub filename: String,
}

pub fn supported(filename: &str) -> bool {
    extension(filename)
        .map(|ext| {
            IMAGE_EXTENSIONS.contains(&ext.as_str()) || DOCUMENT_EXTENSIONS.contains(&ext.as_str())
        })
        .unwrap_or(false)
}

fn extension(filename: &str) -> Option<String> {
    Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

pub fn convert(filename: &str, bytes: &[u8], options: &Options) -> Result<Conversion, String> {
    if bytes.is_empty() {
        return Err("文件是空的".to_string());
    }
    let Some(ext) = extension(filename) else {
        return Err("无法识别文件扩展名".to_string());
    };
    if !supported(filename) {
        return Err(format!("不支持的文件类型: .{ext}"));
    }

    // ---- Images -----------------------------------------------------------
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        let (engine, text) = ocr::ocr_image(bytes, &options.engine, &options.languages)?;
        return finish(text, engine, filename);
    }

    // ---- Optional Microsoft MarkItDown CLI --------------------------------
    if options.prefer_markitdown && ocr::markitdown_available() {
        let text = markitdown_convert(filename, bytes)?;
        if !text.trim().is_empty() {
            return finish(text, "Microsoft MarkItDown (CLI)".to_string(), filename);
        }
    }

    // ---- Pure Rust converters ---------------------------------------------
    let (markdown, engine): (String, String) = match ext.as_str() {
        "txt" | "md" | "markdown" => (documents::plain_text(bytes), "内置文本".into()),
        "csv" => (documents::csv_to_markdown(bytes), "内置 CSV→表格".into()),
        "json" => (
            documents::fenced(&documents::plain_text(bytes), "json"),
            "内置 JSON".into(),
        ),
        "xml" => (
            documents::fenced(&documents::plain_text(bytes), "xml"),
            "内置 XML".into(),
        ),
        "html" | "htm" => (documents::html_to_markdown(bytes), "内置 HTML".into()),
        "docx" => (documents::docx_to_markdown(bytes)?, "内置 DOCX (Rust)".into()),
        "xlsx" => (documents::xlsx_to_markdown(bytes)?, "内置 XLSX (Rust)".into()),
        "pptx" => (documents::pptx_to_markdown(bytes)?, "内置 PPTX (Rust)".into()),
        "pdf" => convert_pdf(filename, bytes, options)?,
        // Legacy binary Office formats need a real parser; only MarkItDown has one.
        "doc" | "xls" | "ppt" => {
            if ocr::markitdown_available() {
                (markitdown_convert(filename, bytes)?, "Microsoft MarkItDown (CLI)".into())
            } else {
                return Err(format!(
                    ".{ext} 是旧版二进制格式，请安装 markitdown CLI (pip install 'markitdown[all]') 或先另存为 .docx/.xlsx/.pptx"
                ));
            }
        }
        _ => return Err(format!("不支持的文件类型: .{ext}")),
    };

    finish(markdown, engine, filename)
}

fn finish(markdown: String, engine: String, filename: &str) -> Result<Conversion, String> {
    let text = markdown.trim().to_string();
    if text.is_empty() {
        return Err("转换结果为空：可能是纯图片扫描件，或 OCR 未识别到文字".to_string());
    }
    Ok(Conversion {
        markdown: text,
        engine,
        filename: Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("result")
            .to_string(),
    })
}

/// PDF handling with a resilient fallback chain.
///
/// 1. fast text-layer extraction (`pdf-extract`, panic-isolated);
/// 2. render pages with PDFKit/poppler and OCR them (works for scans AND for
///    PDFs whose font encoding the text extractor cannot handle);
/// 3. optional Microsoft MarkItDown CLI.
///
/// No single malformed document can abort the server.
fn convert_pdf(
    filename: &str,
    bytes: &[u8],
    options: &Options,
) -> Result<(String, String), String> {
    match documents::pdf_text(bytes) {
        Ok(text) if !text.trim().is_empty() => {
            return Ok((clean_pdf_text(&text), "内置 PDF 文字层 (pdf-extract)".into()));
        }
        Ok(_) => {}
        Err(reason) => eprintln!("[pdf] 文字层解析不可用，降级到 OCR：{reason}"),
    }

    if options.ocr_scanned_pdf {
        match ocr::ocr_pdf(bytes, &options.engine, &options.languages, options.dpi) {
            Ok((engine, text)) if !text.trim().is_empty() => return Ok((text, engine)),
            Ok(_) => eprintln!("[pdf] 逐页 OCR 未得到文字"),
            Err(reason) => eprintln!("[pdf] 逐页 OCR 失败：{reason}"),
        }
    }

    if ocr::markitdown_available() {
        if let Ok(text) = markitdown_convert(filename, bytes) {
            if !text.trim().is_empty() {
                return Ok((text, "Microsoft MarkItDown (CLI)".into()));
            }
        }
    }

    Err(if options.ocr_scanned_pdf {
        "无法解析该 PDF：文字层编码不受支持，逐页 OCR 也没有得到结果。可安装 markitdown CLI 或 poppler 后重试。".to_string()
    } else {
        "该 PDF 没有可用文字层（扫描件或特殊字体编码）。请勾选「扫描版 PDF 走 OCR」。".to_string()
    })
}

fn clean_pdf_text(text: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out.trim().to_string()
}

fn markitdown_convert(filename: &str, bytes: &[u8]) -> Result<String, String> {
    let exe = ocr::find_executable("markitdown")
        .ok_or_else(|| "未找到 markitdown 命令".to_string())?;
    let ext = extension(filename).unwrap_or_else(|| "bin".to_string());
    let path = std::env::temp_dir().join(format!("mdmd-{}.{ext}", Uuid::new_v4()));
    std::fs::write(&path, bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;

    let output = Command::new(exe).arg(&path).output();
    let _ = std::fs::remove_file(&path);
    let output = output.map_err(|e| format!("无法启动 markitdown: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "markitdown 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_formats() {
        assert!(supported("报告.pdf"));
        assert!(supported("截图.PNG"));
        assert!(supported("表格.xlsx"));
        assert!(!supported("恶意.exe"));
        assert!(!supported("没有扩展名"));
    }

    #[test]
    fn plain_text_roundtrip() {
        let result = convert("note.txt", "# 标题\n\n中文内容".as_bytes(), &Options::default()).unwrap();
        assert!(result.markdown.contains("中文内容"));
    }

    #[test]
    fn csv_becomes_a_markdown_table() {
        let result = convert("data.csv", "姓名,城市\n张三,上海\n李四,北京".as_bytes(), &Options::default())
            .unwrap();
        assert!(result.markdown.contains("| 姓名 | 城市 |"));
        assert!(result.markdown.contains("| 张三 | 上海 |"));
    }

    #[test]
    fn html_strips_tags_and_keeps_headings() {
        let html = "<html><head><style>x</style></head><body><h1>标题</h1><p>你好 &amp; 世界</p></body></html>".as_bytes();
        let result = convert("page.html", html, &Options::default()).unwrap();
        assert!(result.markdown.contains("# 标题"));
        assert!(result.markdown.contains("你好 & 世界"));
        assert!(!result.markdown.contains("<style>"));
    }

    #[test]
    fn empty_file_is_rejected() {
        assert!(convert("a.txt", b"", &Options::default()).is_err());
    }

    #[test]
    fn table_separator_is_well_formed() {
        let result =
            convert("data.csv", "a,b\nc,d".as_bytes(), &Options::default()).unwrap();
        assert!(result.markdown.contains("| --- | --- |"));
        assert!(!result.markdown.contains("||"));
    }

    #[test]
    fn pdf_with_unsupported_font_encoding_does_not_panic() {
        // Regression: pdf-extract 0.7 asserts `name == "Identity-H"` and used
        // to abort the whole server (release profile had panic = "abort").
        // The guard must turn that into an ordinary Result.
        let bytes = b"%PDF-1.7\n1 0 obj<</Encoding/Identity-V>>endobj\n%%EOF";
        let outcome = std::panic::catch_unwind(|| crate::documents::pdf_text(bytes));
        assert!(outcome.is_ok(), "pdf_text 不应把 panic 传播出去");
    }

    #[test]
    fn detects_unsupported_chinese_encoding() {
        // A Type0 font with UniGB-UCS2-H is exactly what Chinese e-invoices use.
        let bytes = include_bytes!("../tests/fixtures/unigb_encoding.pdf");
        assert!(crate::documents::pdf_has_unsupported_encoding(bytes));
        assert!(crate::documents::pdf_text(bytes).is_err());
    }

    #[test]
    fn identity_h_is_not_flagged() {
        let bytes = include_bytes!("../tests/fixtures/identity_h_encoding.pdf");
        assert!(!crate::documents::pdf_has_unsupported_encoding(bytes));
    }
}
