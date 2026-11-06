//! OCR backends.
//!
//! Preferred engine on macOS is Apple's built-in Vision framework (Metal /
//! Apple Neural Engine, zero downloads, excellent Simplified/Traditional
//! Chinese). Tesseract is the portable fallback. Both are optional: the binary
//! still builds and runs on Linux/Windows with just Tesseract.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use uuid::Uuid;

#[cfg(target_os = "macos")]
extern "C" {
    fn md_vision_available() -> i32;
    fn md_vision_ocr(
        bytes: *const u8,
        length: usize,
        languages: *const c_char,
    ) -> *mut c_char;
    fn md_pdf_page_count(bytes: *const u8, length: usize) -> usize;
    fn md_pdf_render_page(
        bytes: *const u8,
        length: usize,
        page_index: usize,
        dpi: f64,
        out_length: *mut usize,
    ) -> *mut u8;
    fn md_free(ptr: *mut c_void);
}

// ---------------------------------------------------------------------------
// Capability probing
// ---------------------------------------------------------------------------

pub fn find_executable(name: &str) -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // GUI launches on macOS often lack Homebrew in PATH.
    for fallback in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let candidate = Path::new(fallback).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub fn vision_available() -> bool {
    unsafe { md_vision_available() == 1 }
}

#[cfg(not(target_os = "macos"))]
pub fn vision_available() -> bool {
    false
}

pub fn tesseract_available() -> bool {
    find_executable("tesseract").is_some()
}

pub fn markitdown_available() -> bool {
    find_executable("markitdown").is_some()
}

fn installed_tesseract_languages() -> &'static Vec<String> {
    static LANGS: OnceLock<Vec<String>> = OnceLock::new();
    LANGS.get_or_init(|| {
        let Some(exe) = find_executable("tesseract") else {
            return Vec::new();
        };
        let Ok(output) = Command::new(exe).arg("--list-langs").output() else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1) // "List of available languages in ..."
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    })
}

/// Map Apple language identifiers to Tesseract traineddata names.
fn to_tesseract_languages(vision_languages: &str) -> String {
    let installed = installed_tesseract_languages();
    let mut picked: Vec<String> = Vec::new();
    for id in vision_languages.split(',') {
        let code = match id.trim() {
            "zh-Hans" | "yue-Hans" => "chi_sim",
            "zh-Hant" | "yue-Hant" => "chi_tra",
            "en-US" | "en-GB" | "en" => "eng",
            "ja-JP" | "ja" => "jpn",
            "ko-KR" | "ko" => "kor",
            "fr-FR" | "fr" => "fra",
            "de-DE" | "de" => "deu",
            "es-ES" | "es" => "spa",
            "ru-RU" | "ru" => "rus",
            "pt-BR" | "pt" => "por",
            other => other, // allow passing tesseract codes directly
        };
        if installed.is_empty() || installed.iter().any(|x| x == code) {
            if !picked.iter().any(|x| x == code) {
                picked.push(code.to_string());
            }
        }
    }
    if picked.is_empty() {
        "eng".to_string()
    } else {
        picked.join("+")
    }
}

// ---------------------------------------------------------------------------
// Image OCR
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn vision_ocr(bytes: &[u8], languages: &str) -> Option<String> {
    let langs = CString::new(languages).ok()?;
    unsafe {
        let ptr = md_vision_ocr(bytes.as_ptr(), bytes.len(), langs.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let text = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        md_free(ptr as *mut c_void);
        Some(text)
    }
}

#[cfg(not(target_os = "macos"))]
fn vision_ocr(_bytes: &[u8], _languages: &str) -> Option<String> {
    None
}

fn tesseract_ocr(bytes: &[u8], languages: &str) -> Result<String, String> {
    let exe = find_executable("tesseract")
        .ok_or_else(|| "Tesseract 未安装（brew install tesseract tesseract-lang）".to_string())?;
    let tmp = std::env::temp_dir().join(format!("mdocr-{}.img", Uuid::new_v4()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入临时图片失败: {e}"))?;

    let result = Command::new(exe)
        .arg(&tmp)
        .arg("stdout")
        .arg("-l")
        .arg(to_tesseract_languages(languages))
        .output();
    let _ = std::fs::remove_file(&tmp);

    let output = result.map_err(|e| format!("无法启动 Tesseract: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Tesseract 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Recognise one image. Returns `(engine label, text)`.
///
/// `paddleocr-vl` is an explicit opt-in high-accuracy backend. It is never
/// selected by `auto`, because it requires a separately started local MLX
/// server and a ~0.68 GB model download.
pub fn ocr_image(
    bytes: &[u8],
    engine: &str,
    languages: &str,
) -> Result<(String, String), String> {
    if engine == "paddleocr-vl" {
        return crate::vlm::ocr_image(bytes);
    }

    let want_vision = matches!(engine, "auto" | "vision");
    let want_tesseract = matches!(engine, "auto" | "tesseract");

    if want_vision && vision_available() {
        if let Some(text) = vision_ocr(bytes, languages) {
            if !text.trim().is_empty() {
                return Ok((format!("Apple Vision ({languages})"), clean_ocr(&text)));
            }
        }
    }
    if want_tesseract {
        let langs = to_tesseract_languages(languages);
        let text = tesseract_ocr(bytes, languages)?;
        return Ok((format!("Tesseract ({langs})"), clean_ocr(&text)));
    }
    Err("没有可用的 OCR 引擎：macOS 需要 Vision，或安装 Tesseract".to_string())
}

fn clean_ocr(text: &str) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\u{feff}').trim())
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Scanned PDF: render pages, then OCR each one
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
/// Guard against a pathological document monopolising the OCR worker.
const MAX_OCR_PAGES: usize = 300;

/// Number of pages reported by PDFKit (0 when unavailable / non-macOS).
#[cfg(target_os = "macos")]
pub fn pdf_page_count(pdf: &[u8]) -> usize {
    if pdf.is_empty() {
        return 0;
    }
    unsafe { md_pdf_page_count(pdf.as_ptr(), pdf.len()) }
}

#[cfg(not(target_os = "macos"))]
pub fn pdf_page_count(_pdf: &[u8]) -> usize {
    0
}

fn render_with_pdfkit(pdf: &[u8], dpi: u32) -> Option<Vec<Vec<u8>>> {
    let count = unsafe { md_pdf_page_count(pdf.as_ptr(), pdf.len()) };
    if count == 0 {
        return None;
    }
    let mut pages = Vec::with_capacity(count);
    for index in 0..count {
        let mut len: usize = 0;
        let ptr = unsafe {
            md_pdf_render_page(pdf.as_ptr(), pdf.len(), index, dpi as f64, &mut len)
        };
        if ptr.is_null() || len == 0 {
            if !ptr.is_null() {
                unsafe { md_free(ptr as *mut c_void) };
            }
            continue;
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        unsafe { md_free(ptr as *mut c_void) };
        pages.push(slice);
    }
    (!pages.is_empty()).then_some(pages)
}

#[cfg(not(target_os = "macos"))]
fn render_with_pdfkit(_pdf: &[u8], _dpi: u32) -> Option<Vec<Vec<u8>>> {
    None
}

fn render_with_poppler(pdf: &[u8], dpi: u32) -> Result<Vec<Vec<u8>>, String> {
    let exe = find_executable("pdftoppm").ok_or_else(|| {
        "扫描版 PDF 需要 macOS 的 PDFKit 或 poppler (brew install poppler)".to_string()
    })?;
    let dir = std::env::temp_dir().join(format!("mdpdf-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = dir.join("input.pdf");
    let prefix = dir.join("page");
    let _ = std::fs::write(&input, pdf);

    let status = Command::new(exe)
        .arg("-png")
        .arg("-r")
        .arg(dpi.to_string())
        .arg(&input)
        .arg(&prefix)
        .output();

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("page") && n.ends_with(".png"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    let pages: Vec<Vec<u8>> = files
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);

    if status.is_err() || pages.is_empty() {
        return Err("PDF 页面渲染失败".to_string());
    }
    Ok(pages)
}

fn render_pdf_pages(pdf: &[u8], dpi: u32) -> Result<Vec<Vec<u8>>, String> {
    if let Some(pages) = render_with_pdfkit(pdf, dpi) {
        return Ok(pages);
    }
    render_with_poppler(pdf, dpi)
}

/// OCR every page of a textless PDF. Returns `(engine label, markdown)`.
pub fn ocr_pdf(
    pdf: &[u8],
    engine: &str,
    languages: &str,
    dpi: u32,
) -> Result<(String, String), String> {
    let declared = pdf_page_count(pdf);
    if declared > MAX_OCR_PAGES {
        return Err(format!(
            "PDF 共 {declared} 页，超过单次 OCR 上限 {MAX_OCR_PAGES} 页；请拆分文件后重试"
        ));
    }
    let pages = render_pdf_pages(pdf, dpi)?;
    if pages.is_empty() {
        return Err("PDF 没有可渲染的页面".to_string());
    }
    if pages.len() > MAX_OCR_PAGES {
        return Err(format!(
            "PDF 共 {} 页，超过单次 OCR 上限 {MAX_OCR_PAGES} 页；请拆分文件后重试",
            pages.len()
        ));
    }
    let total = pages.len();
    let mut label = String::new();
    let mut markdown = String::new();

    for (index, page) in pages.iter().enumerate() {
        let (engine_label, text) = ocr_image(page, engine, languages)?;
        if label.is_empty() {
            label = engine_label;
        }
        if total > 1 {
            markdown.push_str(&format!("\n\n<!-- 第 {}/{} 页 -->\n\n", index + 1, total));
        }
        markdown.push_str(&text);
    }
    Ok((label, markdown.trim().to_string()))
}
