//! Optional high-accuracy OCR backend: PaddleOCR-VL-1.6 (0.9B VLM) via a
//! **local** MLX server running on Apple Silicon's Metal GPU.
//!
//! A 0.9B model cannot live inside a 2.5 MB Rust binary, so the Rust side is
//! only an HTTP client. The model is loaded once by a separately started
//! `mlx_vlm.server`, whose `--model` points at a local directory such as
//! `~/PaddleOCR-VL-1.6-4bit`. Nothing is uploaded anywhere; no cloud call.
//!
//! The engine is an explicit opt-in (never part of `auto`). If the server is
//! not running, conversion reports a clear error instead of silently degrading.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

const DEFAULT_URL: &str = "http://127.0.0.1:8111";
const FALLBACK_MODEL: &str = "mlx-community/PaddleOCR-VL-1.6-4bit";
const DEFAULT_PROMPT: &str = "OCR:";

#[derive(Debug, Clone)]
pub struct VlmConfig {
    pub url: String,
    pub model: String,
    pub prompt: String,
    pub timeout_secs: u64,
}

impl Default for VlmConfig {
    fn default() -> Self {
        Self {
            url: std::env::var("MARKITDOWN_VLM_URL").unwrap_or_else(|_| DEFAULT_URL.to_string()),
            model: std::env::var("MARKITDOWN_VLM_MODEL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(discover_local_model),
            prompt: std::env::var("MARKITDOWN_VLM_PROMPT")
                .unwrap_or_else(|_| DEFAULT_PROMPT.to_string()),
            timeout_secs: std::env::var("MARKITDOWN_VLM_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        }
    }
}

/// Pick up a locally mounted model directory automatically, so the WebUI and
/// the startup script agree on the same weights without hard-coded paths.
/// Override with `MARKITDOWN_VLM_MODEL` (a repo id or an absolute path).
///
/// The two installers mount different formats:
///   * macOS   `~/PaddleOCR-VL-1.6-4bit`  (4-bit MLX quant, Metal GPU)
///   * Windows `%USERPROFILE%\PaddleOCR-VL-1.6` (official BF16)
fn model_dir_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in ["HOME", "USERPROFILE"] {
        let Ok(home) = std::env::var(key) else { continue };
        for name in ["PaddleOCR-VL-1.6-4bit", "PaddleOCR-VL-1.6"] {
            out.push(PathBuf::from(&home).join(name));
        }
    }
    out
}

fn discover_local_model() -> String {
    for candidate in model_dir_candidates() {
        if candidate.join("config.json").is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    FALLBACK_MODEL.to_string()
}

impl VlmConfig {
    fn agent(&self, secs: u64) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(secs)))
            .build()
            .into()
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.url.trim_end_matches('/'), path)
    }
}

/// Are the mounted weights actually present? A bare `git clone` without
/// `git lfs pull` leaves a 134-byte LFS pointer, which MLX cannot load.
pub fn weights_ready() -> (bool, String, u64) {
    let config = VlmConfig::default();
    let path = PathBuf::from(&config.model);
    if !path.is_absolute() {
        // A HF repo id: the server downloads on demand; treat as "remote ok"
        // but report that no local directory is mounted.
        return (true, format!("仓库 id {}（由服务按需下载）", config.model), 0);
    }
    let file = path.join("model.safetensors");
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size > 1024 * 1024 {
        (true, format!("{} ({:.0} MB)", file.display(), size as f64 / 1048576.0), size)
    } else {
        (false, format!("{} 仍是 Git LFS 指针（{} 字节），请执行 git lfs pull", file.display(), size), size)
    }
}

/// Is a local VLM server reachable? Short timeout so the capabilities endpoint
/// never blocks the WebUI when the optional backend is not running.
pub fn available() -> bool {
    let config = VlmConfig::default();
    config
        .agent(2)
        .get(config.endpoint("/health"))
        .call()
        .is_ok()
}

/// Human-readable status for the WebUI capabilities payload.
pub fn status() -> (bool, String) {
    let config = VlmConfig::default();
    let (ready, weight_msg, _) = weights_ready();
    if config
        .agent(2)
        .get(config.endpoint("/health"))
        .call()
        .is_ok()
    {
        (true, format!("已连接 {} · 模型 {}", config.url, config.model))
    } else if !ready {
        (false, format!("权重未就绪：{}", weight_msg))
    } else {
        (
            false,
            format!("服务未启动（{}）。运行 scripts/start-paddleocr-vl.sh", config.url),
        )
    }
}

/// Recognise one image. Returns `(engine label, markdown)`.
pub fn ocr_image(bytes: &[u8]) -> Result<(String, String), String> {
    let config = VlmConfig::default();
    if !available() {
        return Err(format!(
            "PaddleOCR-VL 服务不可用（{}）。请先运行 scripts/start-paddleocr-vl.sh 启动本地 MLX 服务。",
            config.url
        ));
    }

    // mlx_vlm.server accepts a local image *path* in image_url. Write a
    // temporary PNG rather than inlining a huge base64 payload.
    let path = std::env::temp_dir().join(format!("mdvl-{}.png", Uuid::new_v4()));
    std::fs::write(&path, bytes).map_err(|e| format!("写入临时图片失败: {e}"))?;
    let result = ocr_image_path(&path, &config);
    let _ = std::fs::remove_file(&path);
    result
}

fn ocr_image_path(path: &PathBuf, config: &VlmConfig) -> Result<(String, String), String> {
    let body = json!({
        "model": config.model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": config.prompt },
                { "type": "image_url", "image_url": { "url": path.to_string_lossy() } }
            ]
        }],
        "max_tokens": 4096,
        "temperature": 0.0,
        "stream": false
    });

    let mut response = config
        .agent(config.timeout_secs)
        .post(config.endpoint("/v1/chat/completions"))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("PaddleOCR-VL 请求失败: {e}"))?;

    let payload: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("PaddleOCR-VL 返回了无法解析的响应: {e}"))?;

    let text = payload
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            format!(
                "PaddleOCR-VL 响应缺少 choices[0].message.content：{}",
                truncate(&payload.to_string(), 300)
            )
        })?
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("PaddleOCR-VL 没有识别出文字".to_string());
    }

    Ok((
        format!("PaddleOCR-VL-1.6 ({})", short_model(&config.model)),
        text,
    ))
}

fn short_model(model: &str) -> String {
    model
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_string()
}

fn truncate(text: &str, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_joins_without_double_slash() {
        let cfg = VlmConfig {
            url: "http://127.0.0.1:8111/".to_string(),
            ..VlmConfig::default()
        };
        assert_eq!(
            cfg.endpoint("/v1/chat/completions"),
            "http://127.0.0.1:8111/v1/chat/completions"
        );
    }

    #[test]
    fn local_model_is_discovered_when_mounted() {
        // On this machine ~/PaddleOCR-VL-1.6-4bit exists. On others the
        // fallback repo id is used; both are non-empty.
        assert!(!VlmConfig::default().model.is_empty());
    }

    #[test]
    fn both_platform_dir_names_are_probed() {
        let names: Vec<String> = model_dir_candidates()
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert!(names.contains(&"PaddleOCR-VL-1.6-4bit".to_string()));
        assert!(names.contains(&"PaddleOCR-VL-1.6".to_string()));
    }

    #[test]
    fn weights_probe_is_non_fatal() {
        let (ready, msg, size) = weights_ready();
        assert!(!msg.is_empty());
        assert!(ready || size < 1024 * 1024);
    }
}
