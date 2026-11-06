//! Windows built-in OCR (`Windows.Media.Ocr`) — the zero-download counterpart
//! to Apple Vision on macOS.
//!
//! It is invoked through an embedded Windows PowerShell 5.1 script (no Rust
//! WinRT dependency, so the crate stays trivially cross-compilable). Any
//! failure — missing zh-CN OCR language capability, package-identity issue,
//! PowerShell unavailable — is reported back and the caller falls back to
//! Tesseract. See docs/OCR-ENGINES.md for the platform caveats.

#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
use uuid::Uuid;

#[cfg(target_os = "windows")]
const SCRIPT: &str = include_str!("../scripts/winocr.ps1");

/// Whether the built-in engine is plausibly callable. A language pack may
/// still be missing; that is discovered at conversion time and handled by
/// falling back to Tesseract.
#[cfg(target_os = "windows")]
pub fn available() -> bool {
    crate::ocr::find_executable("powershell.exe").is_some()
}

#[cfg(not(target_os = "windows"))]
pub fn available() -> bool {
    false
}

/// Map Apple-style language identifiers (used by the WebUI) to Windows BCP-47.
#[allow(dead_code)] // used by the Windows-only ocr_image; kept testable on macOS
fn to_bcp47(languages: &str) -> String {
    let first = languages.split(',').next().unwrap_or("").trim();
    match first {
        "zh-Hans" | "yue-Hans" => "zh-CN",
        "zh-Hant" | "yue-Hant" => "zh-TW",
        "ja-JP" | "ja" => "ja-JP",
        "ko-KR" | "ko" => "ko-KR",
        "en-US" | "en-GB" | "en" => "en-US",
        other if !other.is_empty() => other,
        _ => "zh-CN",
    }
    .to_string()
}

#[cfg(target_os = "windows")]
pub fn ocr_image(bytes: &[u8], languages: &str) -> Result<(String, String), String> {
    let dir = std::env::temp_dir().join(format!("mdwinocr-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let image: PathBuf = dir.join("page.png");
    let script: PathBuf = dir.join("winocr.ps1");
    let _ = std::fs::write(&image, bytes);
    std::fs::write(&script, SCRIPT).map_err(|e| format!("写入 OCR 脚本失败: {e}"))?;

    let lang = to_bcp47(languages);
    let result = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-ImagePath")
        .arg(&image)
        .arg("-Language")
        .arg(&lang)
        .output();

    let _ = std::fs::remove_dir_all(&dir);

    let output = result.map_err(|e| format!("无法启动 Windows PowerShell: {e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows 内置 OCR 不可用（{}）。通常是没有安装 zh-CN OCR 语言包：{}",
            lang,
            detail.trim().chars().take(200).collect::<String>()
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err("Windows 内置 OCR 没有识别出文字".to_string());
    }
    Ok((format!("Windows 内置 OCR ({lang})"), text))
}

#[cfg(not(target_os = "windows"))]
pub fn ocr_image(_bytes: &[u8], _languages: &str) -> Result<(String, String), String> {
    Err("Windows 内置 OCR 仅在 Windows 上可用".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_mapping() {
        assert_eq!(to_bcp47("zh-Hans,en-US"), "zh-CN");
        assert_eq!(to_bcp47("zh-Hant,en-US"), "zh-TW");
        assert_eq!(to_bcp47("ja-JP,en-US"), "ja-JP");
        assert_eq!(to_bcp47(""), "zh-CN");
    }

    #[test]
    fn non_windows_reports_unavailable() {
        if !cfg!(target_os = "windows") {
            assert!(!available());
            assert!(ocr_image(b"x", "zh-Hans,en-US").is_err());
        }
    }
}
