# markitdown-rs

把 `nfeuism/markitdown-webui2` 用 Rust 重写后的极简版本：**一个二进制 + 一个本地网页**。

- 不再有 Python venv / Flask / PyInstaller / Tauri `.app` / LaunchAgent / 十几个安装脚本
- 不再下载 OCR 模型：中文 OCR 直接用 macOS 内置的 **Apple Vision**（ANE / Metal 加速）
- 文档解析用纯 Rust（`zip` + `quick-xml` + `pdf-extract`），常见格式完全不依赖 Python
- 只绑定 `127.0.0.1`，不联网、不上传
- 原来的 Mac「应用」本来就是套了个 WebView 指向 `http://127.0.0.1:51245`，现在直接删掉，用浏览器打开网页即可

---

## 快速开始

```bash
cd markitdown-rs
cargo build --release
./target/release/markitdown-rs --open
# 或手动打开 http://127.0.0.1:5001
```

常用参数：

```bash
./target/release/markitdown-rs --port 5055 --max-file-size-mb 250
curl http://127.0.0.1:5055/health
curl http://127.0.0.1:5055/api/ocr-capabilities
```

可选依赖（都不是必需的）：

```bash
brew install tesseract tesseract-lang   # Vision 不可用时的跨平台兜底
pip install 'markitdown[all]'           # 想要微软原版转换结果 / 打开 .doc .xls .ppt
brew install poppler                    # 非 macOS 上渲染扫描 PDF
```

---

## 新旧对比

| | 原项目 (markitdown-webui2) | markitdown-rs |
|---|---|---|
| 运行形态 | Python Flask + uv venv + LaunchAgent + Tauri WebView | 单个 Rust 二进制 |
| Mac「应用」 | Tauri `.app` / DMG + PyInstaller sidecar（本质是网页） | **无**，直接浏览器打开 |
| 中文 OCR | RapidOCR PP-OCRv6 (ONNX) + 160+ Tesseract 语言包 + MLX Unlimited-OCR 2.35GB | Apple Vision（0 下载）+ Tesseract 兜底 |
| 文档解析 | Microsoft MarkItDown (Python) | 纯 Rust，可选调用 markitdown CLI |
| 安装脚本 | `install.sh/ps1`、`start/stop/restart/status/uninstall`、`bootstrap`、`install-mcp` … | 无，`cargo build` 即可 |
| 后台任务 / 历史 | SQLite + 批量队列 + 重试 + diff 对比 + 30 天留存 | 无（同步请求，浏览器里合并多文件） |
| MCP 接入 | 注册到 Claude Code / Codex / Hermes 三个 CLI | 去掉，HTTP API 可直接被 agent 调用 |
| 云 OCR | MinerU Cloud（需显式同意上传） | 去掉，纯本地 |
| 源码规模 | 约 50 个文件 / 数千行 Python + 脚本 | 7 个 Rust/ObjC/HTML 文件 |

> 取舍说明：批量队列、历史库、diff、MCP 注册、云 OCR 都是让项目「臃肿」的部分，本重写刻意删掉。
> 它们不影响核心能力（文档 → Markdown + 中文 OCR），而且 HTTP API 保持简单，想接回 MCP 只需一层很薄的转发。

---

## 架构

```
src/main.rs        axum HTTP 服务，页面用 include_str! 内嵌进二进制
src/convert.rs     格式分发；纯 Rust 优先，可选 markitdown CLI，图片/扫描 PDF 走 OCR
src/documents.rs   txt/csv/json/xml/html/docx/xlsx/pptx/pdf 文字层的纯 Rust 转换器
src/ocr.rs         Apple Vision (FFI) / Tesseract 兜底 / PDF 渲染
src/vision_ocr.m   macOS 原生桥：Vision 文字识别 + PDFKit 逐页渲染（ARC，C ABI）
build.rs           用 cc 编译 .m 并链接 Vision/AppKit/PDFKit/Foundation 框架
static/index.html  单页 WebUI（无构建、无 CDN、无外部资源）
```

非 macOS 平台会自动跳过 Objective-C 文件，退回 Tesseract + poppler，项目仍可编译运行。

## 支持格式

| 输入 | 处理方式 |
|---|---|
| `.txt` `.md` | 原样输出 |
| `.csv` | 转成 Markdown 表格（自动猜分隔符 `,` / `\t` / `;`） |
| `.json` `.xml` | 包成对应语言的代码块 |
| `.html` `.htm` | 去标签，保留标题 / 列表 / 链接 / 加粗 / 表格 / 代码块 |
| `.docx` | 读 `word/document.xml`，识别 Heading 样式 → `#`，表格行 → `\|` |
| `.xlsx` | 读 `sharedStrings.xml` + `sheetN.xml` → Markdown 表格（多工作表） |
| `.pptx` | 读 `slideN.xml` 的 `<a:t>`，按页分节 |
| `.pdf` | `pdf-extract` 抽文字层；无文字层时用 PDFKit 逐页渲染 + OCR |
| 图片 `jpg/png/webp/bmp/tiff` | 直接 OCR |
| `.doc` `.xls` `.ppt` | 旧二进制格式，需已安装 markitdown CLI，或先另存为新格式 |

## Apple Vision 中文识别的关键细节

Apple Vision 的 `VNRecognizeTextRequest`（revision 3 / `.accurate`）本身支持 `zh-Hans`、`zh-Hant`、`yue-Hans`、`yue-Hant`，走系统 ANE/GPU，零模型下载，清晰印刷体中文很快很准。

**但有一个必须遵守的坑**：新版 macOS 上 `recognitionLanguages` 数组里中文必须排在英文**前面**：

| 设置 | 中英混排通过率 |
|---|---|
| `["zh-Hans","en-US"]` | 13/14 ✅ |
| `["en-US","zh-Hans"]` | 0/14 ❌ |
| 自动检测 | 3/14 ❌ |

来源：[macos27-vision-ocr-bug](https://github.com/missuo/macos27-vision-ocr-bug)。
所以本项目默认值固定为 `zh-Hans,en-US`，并且 `vision_ocr.m` 会动态调用
`supportedRecognitionLanguages(for:revision:)` 过滤不支持的语言标识，避免 Vision 抛异常。
CPU/GPU/ANE 限定都修不了这个 bug，只能靠语言顺序。

**适用范围**：清晰印刷体、截图、简体/繁体中英混排很好；复杂表格、竖排、手写、生僻字、印章、公式不要指望它，需要高精度模型层（见下）。

## 关于「更好的 Mac 中文 OCR 模型」

完整调研见 [docs/OCR-MODELS.md](docs/OCR-MODELS.md)。结论速览：

| 需求 | 推荐 |
|---|---|
| 零依赖、最快、隐私最好（本项目默认） | **Apple Vision**，`zh-Hans,en-US`，0 MB |
| 同等精度但可换模型 / 走 CoreML ANE | **PP-OCRv6 small ONNX + Rust `ort` crate（`features=["coreml"]`）**，7.7M 参数、Apache-2.0，官方 macOS 预编译二进制自带 CoreML EP |
| 精度天花板（复杂版面/表格/公式/古籍） | **PaddleOCR-VL-1.6**（958M 参数，OmniDocBench v1.6 **96.33%**）+ MLX 4bit（**0.68GB**）——**已作为可选引擎接入本项目**（见下）；次选 GLM-OCR、MinerU2.5-Pro |
| 兜底 | Tesseract `chi_sim+chi_tra+eng`（中文精度最弱，只配兜底，不必装 160+ 语言包） |

一句话：你现在用的 PP-OCRv6 已经是对的轻量模型，问题不在模型而在「Python 打包 + 没用 CoreML」；
真正精度更高且更适配 Mac 的是 PaddleOCR-VL-1.6 / GLM-OCR 这类 0.9B 级专用 VLM，而不是更大的通用 VLM（Qwen3-VL-235B 只有 89.78%）。

代码里 [ocr.rs](<src/ocr.rs>) 的 `ocr_image()` 仍是唯一入口；再加 PP-OCRv6（`ort` + CoreML）作为第五层引擎不用动 WebUI 和转换层。

## 可选引擎：PaddleOCR-VL-1.6（0.9B VLM）

**默认关闭、显式选择才启用**。它专门解决 Apple Vision 的短板：复杂表格、公式、印章、生僻字、古籍、版面阅读顺序——也就是你那批水费电子发票散架的问题。

### 模型是真的吗？

是。2026-11-06 亲自用 HuggingFace API 核实（不是转述搜索结果）：

| 仓库 | 事实 |
|---|---|
| [`PaddlePaddle/PaddleOCR-VL-1.6`](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6) | 官方组织，2026-05-27 创建，**958,588,736 参数**，BF16 `model.safetensors` 1.93GB，Apache-2.0，arxiv:2606.03264，OmniDocBench v1.6 **96.33%**（官方 README，非第三方转述） |
| [`mlx-community/PaddleOCR-VL-1.6-4bit`](https://huggingface.co/mlx-community/PaddleOCR-VL-1.6-4bit) | 2026-08-12 创建，4-bit MLX 量化，**905,601,648 参数**，仓库实测 `usedStorage` **714,751,688 B ≈ 0.68GB**，base_model 指向官方 1.6，Apache-2.0。用 `mlx-vlm` 0.6.10 / `mlx` 0.32.0 转换 |

> 官方 README 有一句必须知道的话：**macOS 上完整 PaddleOCR pipeline 建议用 Docker**（PaddlePaddle CPU 版面分析那部分）。所以本项目不跑官方 pipeline，
> 而是直接调 [mlx-vlm](https://github.com/Blaizzy/mlx-vlm) 的本地 OpenAI 兼容服务跑 4bit 权重——纯 Metal / Apple Silicon，不需要 Docker、不需要 PaddlePaddle。

### 架构（为什么是「可选项」）

0.9B 模型塞不进 4MB 的 Rust 二进制，所以它是**独立进程 + 本地 HTTP**，Rust 端只多一个 `ureq` 依赖：

```
浏览器 → markitdown-rs (Rust, :5001) → 127.0.0.1:8111/v1/chat/completions → mlx_vlm.server (4bit MLX)
                                      ↑ 仅当用户选择 paddleocr-vl 引擎时才调用
```

- 不联网、不上传、模型缓存到 `~/.cache/huggingface`；
- 服务没起时，UI 里该项置灰并显示启动命令；显式选中但服务不可达会**明确报错**，不静默降级；
- **不进入 `auto` 链路**，默认仍是零下载的 Apple Vision；
- 非 macOS 平台也能编译（只是没有 MLX 服务可连）。

### 启用

```bash
./scripts/start-paddleocr-vl.sh          # 首次会装 mlx-vlm 并下载 ~0.68GB 权重
./scripts/start-paddleocr-vl.sh --check  # 只探测服务是否就绪
```

脚本用 `uv` 建隔离的 Python 3.12 venv（`.venv-vlm/`），不污染系统 Python。起来后在 WebUI 的
「OCR 引擎」下拉里选 **PaddleOCR-VL-1.6** 即可。

可选环境变量（也可用于接任何 OpenAI 兼容的 VLM 服务）：

| 变量 | 默认值 |
|---|---|
| `MARKITDOWN_VLM_URL` | `http://127.0.0.1:8111` |
| `MARKITDOWN_VLM_MODEL` | `mlx-community/PaddleOCR-VL-1.6-4bit` |
| `MARKITDOWN_VLM_PROMPT` | `Transcribe this document to markdown...` |
| `MARKITDOWN_VLM_TIMEOUT_SECS` | `300` |

### 验证状态（诚实说明）

- ✅ Rust 端协议链路已用本地 mock OpenAI 服务**端到端跑通**：`/health` 探测、`/v1/chat/completions` 请求（model / image_url / prompt / max_tokens / temperature）、`choices[0].message.content` 解析；中文发票 PDF 走降级链后由该引擎返回 Markdown，服务全程存活。
- ✅ 可用性切换、显式报错、`auto` 链路不受影响，均已实测。
- ⚠️ **真实 4bit 权重未在本机下载**，未做真机精度/速度 benchmark。量化权重的 README 自带 6 份合成文档的字段准确率测量（4bit 默认 prompt 下 1.0），并明确说明「不同 prompt 下排序不稳定、样本太小不足以排名」——别把它当成中文精度保证，建议用自己的发票样本做 A/B。

## 已知限制

- `pdf-extract` 对部分 PDF 字体映射会把个别汉字输出成 CJK 兼容表意文字（如 `⼀`/`⽂`）；需要逐字精确时勾选「优先使用 MarkItDown CLI」。
- `pdf-extract` 0.7 只认 `Identity-H` 编码，遇到中文电子发票常用的 `UniGB-UCS2-H` 会内部断言崩溃。本版本已做**事前检测 + 自动降级**（见下），不再受影响。
- 纯 Rust 的 docx/xlsx/pptx 转换器是「够用」级别，不还原图片、合并单元格、批注、样式；要微软原版输出请勾 MarkItDown CLI。
- 扫描 PDF 逐页 OCR 是同步请求，长文档会让浏览器转圈几分钟（显示已用秒数），没有取消按钮；单次上限 300 页。
- Apple Vision 只在 macOS 11+ 可用；Linux/Windows 走 Tesseract + poppler。
- 不提供历史库、批量队列、MCP 注册和云 OCR——这是刻意的瘦身，不是遗漏。

## 故障排查：上传 PDF 后网站打不开

**已修复（v0.1.0 后续提交）。** 早期版本上传某些中文 PDF 后整个服务会退出，页面打不开。原因链条：

1. 中文电子发票等 PDF 的 Type0 字体常用 `/Encoding /UniGB-UCS2-H`（Adobe-GB1）；
2. `pdf-extract` 0.7 对该分支写的是 `assert!(name == "Identity-H")`，遇到就 panic；
3. 早期 release 配置里写了 `panic = "abort"`，一个请求的 panic 直接杀死整个进程。

修复做了四层防护，任一文档都**不可能再拖垮服务**：

| 层 | 措施 |
|---|---|
| 1 | `lopdf` 事前扫描 PDF 字体编码，发现非 `Identity-H` 的 Type0 编码就**根本不调用**会 panic 的分支 |
| 2 | `documents::pdf_text` 用 `catch_unwind` 隔离 `pdf-extract`，panic 转成普通 `Err` |
| 3 | `main.rs` 对每次转换再做一次请求级 `catch_unwind` 兜底 |
| 4 | 去掉 `panic = "abort"`，恢复默认 unwind |

降级链：**文字层 → 逐页渲染 + OCR（PDFKit/poppler） → MarkItDown CLI**。所以即使是解析失败的 PDF，
也会走 Vision OCR 正常出 Markdown，而不是报错或崩溃。日志里会打印一行
`[pdf] 文字层解析不可用，降级到 OCR：...`，属正常提示。

实测：`~/Downloads` 里 50 个 PDF 有 27 个会触发该断言（水费电子发票），全部 24 个文件连续转换
均返回 HTTP 200、服务全程存活、日志 0 条 panic。回归测试见 `cargo test`（9 项，含编码检测夹具
`tests/fixtures/`）。

## 页面打不开：先确认是不是端口/进程问题

默认端口是 **5001**，地址固定为 `http://127.0.0.1:5001/`。如果打不开，先查进程和监听端口，
再决定是重启还是换端口：

```bash
# 有没有在跑、监听的是哪个端口
ps aux | grep '[m]arkitdown-rs'
lsof -nP -iTCP:5001 | grep LISTEN
curl http://127.0.0.1:5001/health

# 没在跑就重启（后台常驻，日志写到文件）
cd markitdown-rs
nohup ./target/release/markitdown-rs --port 5001 > /tmp/markitdown-rs.log 2>&1 &
```

> 注意：开发/测试时曾把实例起在 `--port 5099`，**文档里的 5001 才是默认值**。
> 端口不一致时页面当然打不开，这和 PDF 崩溃是两回事。

这个二进制是普通前台进程，不会自动随登录启动、崩溃后也不会自动拉起。如果需要长期常驻，
用一行 LaunchAgent 即可（可选，不是必需的——原项目那套 LaunchAgent/installer 脚本正是被删掉的臃肿部分）：

```bash
# 可选：把下面这行加进 ~/.zprofile，每次开终端自动拉起（幂等）
pgrep -x markitdown-rs >/dev/null || nohup /绝对路径/markitdown-rs --port 5001 >/tmp/markitdown-rs.log 2>&1 &
```

## License

MIT。调用的框架/工具各自遵循其许可：Apple Vision（系统框架）、Tesseract（Apache-2.0）、
Microsoft MarkItDown（MIT）、PaddleOCR / RapidOCR 模型（Apache-2.0）。
