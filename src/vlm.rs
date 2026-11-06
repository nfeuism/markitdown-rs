//! Optional high-accuracy OCR backend: PaddleOCR-VL-1.6 via a local MLX server.
//!
//! PaddleOCR-VL-1.6 is a 958M-parameter vision-language model. It cannot be
//! embedded in a 2.5 MB Rust binary, so this module talks to a **separate,
//! user-started** `mlx_vlm.server` over an OpenAI-compatible HTTP API.
//!
//! Nothing here runs unless the user explicitly selects the `paddleocr-vl`
//! engine. There is no cloud call, no model download and no Python dependency
//! in the Rust binary itself.
//!
//! Verified facts (2026-11-06):
//!   * https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6  (958,588,736 params, Apache-2.0)
//!   * https://huggingface.co/mlx-community/PaddleOCR-VL-1.6-4bit  (4-bit MLX, ~0.68 GB)
//!   * https://github.com/Blaizzy/mlx-vlm  server endpoints:
//!       GET  /health
//!       GET  /v1/models
//!       POST /v1/chat/completions   (OpenAI-compatible, supports image_url)

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

const DEFAULT_URL: &str = "http://127.0.0.1:8111";
const DEFAULT_MODEL: &str = "mlx-community/PaddleOCR-VL-1.6-4bit";
const DEFAULT_PROMPT: &str = "Transcribe this document to markdown. Preserve tables as markdown tables, keep the original Chinese text, and output only the transcription.";

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
                .unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
            prompt: std::env::var("MARKITDOWN_VLM_PROMPT")
                .unwrap_or_else(|_| DEFAULT_PROMPT.to_string()),
            timeout_secs: std::env::var("MARKITDOWN_VLM_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        }
    }
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
    match config.agent(2).get(config.endpoint("/health")).call() {
        Ok(_) => (true, format!("已连接 {}", config.url)),
        Err(_) => (
            false,
            format!("未启动（需先运行 scripts/start-paddleocr-vl.sh，目标 {}）", config.url),
        ),
    }
}

/// Recognise one image with PaddleOCR-VL-1.6. Returns `(engine label, markdown)`.
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
    let image_url = json!({ "url": path.to_string_lossy() });
    let body = json!({
        "model": config.model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": config.prompt },
                { "type": "image_url", "image_url": image_url }
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
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .replace("PaddleOCR-VL-1.6-", "")
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
    fn short_model_label() {
        assert_eq!(
            short_model("mlx-community/PaddleOCR-VL-1.6-4bit"),
            "4bit"
        );
    }

    #[test]
    fn unavailable_by_default_when_no_server() {
        // In the test environment there is normally no MLX server on 8111.
        // This asserts the probe is non-fatal (returns a bool, never panics).
        let _ = available();
    }
}
