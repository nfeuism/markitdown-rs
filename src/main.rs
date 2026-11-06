//! markitdown-rs — one binary, one localhost page, zero Python required.
//!
//!   cargo run --release -- --open
//!   open http://127.0.0.1:5001

mod convert;
mod documents;
mod ocr;
mod winocr;

use axum::extract::{DefaultBodyLimit, Multipart};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use serde::Serialize;

const INDEX_HTML: &str = include_str!("../static/index.html");
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(
    name = "markitdown-rs",
    version = VERSION,
    about = "本地文档 → Markdown 的极简 WebUI（Rust 单文件 + Apple Vision 中文 OCR）"
)]
struct Args {
    /// 监听地址（只绑定本机，不对外暴露）
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// 端口
    #[arg(short, long, default_value_t = 5001)]
    port: u16,
    /// 启动后用默认浏览器打开页面
    #[arg(long)]
    open: bool,
    /// 单个上传文件的大小上限（MB）
    #[arg(long, default_value_t = 500)]
    max_file_size_mb: usize,
}

// ---------------------------------------------------------------------------
// JSON payloads
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
    vision: bool,
    tesseract: bool,
    markitdown: bool,
    default_languages: &'static str,
}

#[derive(Serialize)]
struct Capabilities {
    engines: Vec<EngineInfo>,
    language_presets: Vec<LanguagePreset>,
    vision: bool,
    tesseract: bool,
    markitdown: bool,
    /// Engine chosen at install time; the WebUI preselects it.
    default_engine: String,
}

#[derive(Serialize)]
struct EngineInfo {
    id: &'static str,
    label: &'static str,
    available: bool,
}

#[derive(Serialize)]
struct LanguagePreset {
    id: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
struct ConvertOk {
    success: bool,
    filename: String,
    markdown: String,
    engine: String,
    characters: usize,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(ErrorBody { error: self.0 })).into_response()
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: VERSION,
        vision: ocr::vision_available(),
        tesseract: ocr::tesseract_available(),
        markitdown: ocr::markitdown_available(),
        default_languages: "zh-Hans,en-US",
    })
}

async fn capabilities() -> Json<Capabilities> {
    let vision = ocr::vision_available();
    let tesseract = ocr::tesseract_available();
    let winocr = winocr::available();

    let auto_label = if cfg!(target_os = "windows") {
        "自动（Windows 内置 OCR → Tesseract）"
    } else {
        "自动（Apple Vision → Tesseract）"
    };

    let mut engines = vec![
        EngineInfo {
            id: "auto",
            label: auto_label,
            available: vision || tesseract || winocr,
        },
        EngineInfo {
            id: "vision",
            label: "Apple Vision（系统内置，中文精准、零下载 · 仅 macOS）",
            available: vision,
        },
        EngineInfo {
            id: "tesseract",
            label: "Tesseract（跨平台兜底）",
            available: tesseract,
        },
    ];
    if cfg!(target_os = "windows") {
        engines.push(EngineInfo {
            id: "winocr",
            label: "Windows 内置 OCR（系统级、零模型 · 需 zh-CN 语言包）",
            available: winocr,
        });
    }

    Json(Capabilities {
        engines,
        language_presets: vec![
            LanguagePreset { id: "zh-Hans,en-US", label: "简体中文 + English（推荐）" },
            LanguagePreset { id: "zh-Hant,en-US", label: "繁體中文 + English" },
            LanguagePreset { id: "zh-Hans,zh-Hant,en-US", label: "简繁中文 + English" },
            LanguagePreset { id: "zh-Hans,zh-Hant", label: "仅中文（简 + 繁）" },
            LanguagePreset { id: "ja-JP,en-US", label: "日本語 + English" },
            LanguagePreset { id: "ko-KR,en-US", label: "한국어 + English" },
        ],
        vision,
        tesseract,
        markitdown: ocr::markitdown_available(),
        default_engine: convert::configured_engine(),
    })
}

async fn api_convert(mut multipart: Multipart) -> Result<Json<ConvertOk>, ApiError> {
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut options = convert::Options::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(format!("读取上传数据失败: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("file").to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError(format!("读取文件失败: {e}")))?
                    .to_vec();
                file = Some((filename, data));
            }
            "engine" => options.engine = field.text().await.unwrap_or_default(),
            "languages" => options.languages = field.text().await.unwrap_or_default(),
            "dpi" => {
                options.dpi = field
                    .text()
                    .await
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(200)
                    .clamp(72, 600);
            }
            "prefer_markitdown" => {
                options.prefer_markitdown = truthy(&field.text().await.unwrap_or_default())
            }
            "ocr_scanned_pdf" => {
                options.ocr_scanned_pdf = truthy(&field.text().await.unwrap_or_default())
            }
            _ => {}
        }
    }

    let (filename, bytes) = file.ok_or_else(|| ApiError("没有收到文件".to_string()))?;
    if let Some(engine) = ["auto", "vision", "tesseract", "winocr"]
        .iter()
        .find(|e| **e == options.engine)
    {
        options.engine = (*engine).to_string();
    } else {
        options.engine = "auto".to_string();
    }
    if options.languages.trim().is_empty() {
        options.languages = "zh-Hans,en-US".to_string();
    }

    let converted = tokio::task::spawn_blocking(move || {
        // Last line of defence: a parser panic stays inside this request and
        // must never take the whole server down. Individual converters also
        // catch known panics so they can degrade to OCR / MarkItDown.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert::convert(&filename, &bytes, &options)
        }))
        .unwrap_or_else(|_| {
            Err("转换过程中解析器发生内部错误（已隔离，服务仍可用）".to_string())
        })
    })
    .await
    .map_err(|e| ApiError(format!("转换任务异常: {e}")))?
    .map_err(ApiError)?;

    Ok(Json(ConvertOk {
        success: true,
        characters: converted.markdown.chars().count(),
        filename: converted.filename,
        engine: converted.engine,
        markdown: converted.markdown,
    }))
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let addr = format!("{}:{}", args.host, args.port);
    let socket: std::net::SocketAddr = addr
        .parse()
        .unwrap_or_else(|_| panic!("无效的监听地址: {addr}"));

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/ocr-capabilities", get(capabilities))
        .route("/api/convert", post(api_convert))
        .layer(DefaultBodyLimit::max(args.max_file_size_mb * 1024 * 1024));

    let listener = tokio::net::TcpListener::bind(socket)
        .await
        .unwrap_or_else(|e| panic!("无法绑定 {socket}: {e}"));

    eprintln!("markitdown-rs {VERSION}");
    eprintln!("  页面      http://{socket}/");
    eprintln!("  Vision    {}", yes_no(ocr::vision_available()));
    eprintln!("  Tesseract {}", yes_no(ocr::tesseract_available()));
    eprintln!("  MarkItDown CLI {}", yes_no(ocr::markitdown_available()));
    eprintln!("  仅监听 {socket}，不会暴露到局域网");

    if args.open {
        open_in_browser(&format!("http://{socket}/"));
    }

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("服务器异常: {e}"));
}

fn yes_no(value: bool) -> &'static str {
    if value { "可用" } else { "不可用" }
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let result: std::io::Result<std::process::Child> = Ok(()).and_then(|_| {
        eprintln!("请手动打开 {url}");
        Err(std::io::Error::new(std::io::ErrorKind::Other, "unsupported"))
    });
    let _ = result;
}
