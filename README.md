# markitdown-rs

把 `nfeuism/markitdown-webui2` 用 Rust 重写后的极简版本：**一个二进制 + 一个本地网页**。

- 不再有 Python venv / Flask / PyInstaller / Tauri `.app` / LaunchAgent / 十几个安装脚本
- 不下载 OCR 模型：中文 OCR 默认用**系统内置**引擎（macOS Apple Vision / Windows 内置 OCR）
- 可选挂载本地 **PaddleOCR-VL-1.6**（0.9B VLM）走 Apple Silicon Metal GPU，见下节
- 文档解析用纯 Rust（`zip` + `quick-xml` + `pdf-extract`），常见格式完全不依赖 Python
- 只绑定 `127.0.0.1`，不联网、不上传
- 原来的 Mac「应用」本来只是个指向 localhost 的 WebView，已删除，直接用浏览器打开网页

> 各平台系统 OCR 的能力边界见 [docs/OCR-ENGINES.md](docs/OCR-ENGINES.md)。
> 第三方 ONNX 轻量识别模型与文档 VLM 曾按需求移除，后又按需求把**本地已挂载的
> PaddleOCR-VL-1.6** 重新接回为可选引擎（默认关闭）。

---

## 一行命令安装（会问你选哪个 OCR 引擎）

**macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/nfeuism/markitdown-rs/main/install.sh | bash
```

**Windows**（PowerShell）

```powershell
irm https://raw.githubusercontent.com/nfeuism/markitdown-rs/main/install.ps1 | iex
```

安装器会交互询问 OCR 引擎，然后只装该选项需要的依赖、下载预编译二进制、启动服务并打开
`http://127.0.0.1:5001`。选择结果写进启动器，之后 WebUI 会预选它（页面里仍可临时改）。

| 平台 | 选项 | 说明 |
|---|---|---|
| macOS | 1) **Apple Vision**（默认） | 系统内置、零下载、中文精准、ANE 加速 |
| macOS | 2) Tesseract | `brew` 装简体/繁体中文包，跨平台兜底 |
| macOS | 3) 两者 | Vision 优先 + Tesseract 兜底 |
| macOS | 4) **PaddleOCR-VL**（可选） | 克隆 4-bit MLX 量化模型，走 M 系列 Metal GPU；需 `scripts/start-paddleocr-vl.sh` |
| Windows | 1) Windows 内置 OCR | 系统级、零模型，**需 zh-CN OCR 语言包**（管理员/联网） |
| Windows | 2) **Tesseract 精准**（默认） | winget 装引擎 + `tessdata_best` 简/繁/英 |
| Windows | 3) 两者 | 内置 OCR 优先 + Tesseract 兜底 |
| Windows | 4) **PaddleOCR-VL**（可选） | 克隆官方 BF16 模型（约 1.79 GiB）；推理需另起服务，见下 |

**选项 4 会自动挂载模型**（两个平台用不同仓库/格式，安装器自动判断）：

| 平台 | 安装器执行的命令 | 权重 |
|---|---|---|
| macOS | `git clone https://www.modelscope.cn/mlx-community/PaddleOCR-VL-1.6-4bit.git ~/PaddleOCR-VL-1.6-4bit` | `model.safetensors` 703,562,705 B ≈ 671 MiB（4-bit MLX） |
| Windows | `git clone https://www.modelscope.cn/PaddlePaddle/PaddleOCR-VL-1.6.git %USERPROFILE%\PaddleOCR-VL-1.6` | `model.safetensors` 1,917,255,968 B ≈ 1.79 GiB（官方 BF16） |

两个仓库都用 **Git LFS**：安装器会检测 `git-lfs`，自动 `git lfs install --local && git lfs pull`，
并校验 `model.safetensors` 不是 134 字节的指针。没装 git-lfs 时会打印修复命令，其余 OCR 不受影响。
已存在的目录会被复用，不会重复克隆。

> ⚠️ **平台差异**：Windows 没有 MLX，官方 BF16 权重**不能**直接用 macOS 那套 `mlx_vlm.server` 跑。
> 官方建议 WSL2 + Docker 或 PaddlePaddle CPU 部署推理服务；服务起来后设置
> `MARKITDOWN_VLM_URL` 指向你的 OpenAI 兼容 endpoint，WebUI 的 VLM 选项即可启用。
> 安装器在 Windows 上会明确打印这段提示，不会假装已经可用。

非交互（CI / 脚本）用环境变量跳过提问：

```bash
MARKITDOWN_OCR=vision curl -fsSL .../install.sh | bash      # 或 tesseract / auto / paddleocr-vl
```
```powershell
$env:MARKITDOWN_OCR='tesseract'; irm .../install.ps1 | iex  # 或 winocr / auto / paddleocr-vl
```

其它可覆盖变量：`MARKITDOWN_PORT`（默认 5001）、`MARKITDOWN_INSTALL_DIR`。

> 「纯离线」指**运行时**；安装时下载二进制、Tesseract 或 Windows OCR 语言包需要联网。
> 完全零下载的组合：macOS 选 Apple Vision，或 Windows 已预装 zh-CN OCR 语言包。

安装器优先从 [GitHub Releases](https://github.com/nfeuism/markitdown-rs/releases) 下载预编译包；
若还没有 Release，会回退到源码构建（自动装 rustup）。发布包由
[.github/workflows/release.yml](.github/workflows/release.yml) 在打 `v*` tag 时构建
（macOS arm64 / x64 + Windows x64）。

---

## 手动运行

```bash
cargo build --release
./target/release/markitdown-rs --open          # http://127.0.0.1:5001
```

```bash
./target/release/markitdown-rs --port 5055 --max-file-size-mb 250
curl http://127.0.0.1:5055/health
curl http://127.0.0.1:5055/api/ocr-capabilities
```

可选依赖（都不是必需）：

```bash
brew install tesseract tesseract-lang   # macOS：Tesseract 兜底 + 中文语言包
pip install 'markitdown[all]'           # 想用微软原版转换结果 / 打开 .doc .xls .ppt
brew install poppler                    # 非 macOS 上渲染扫描 PDF
```

---

## 可选：挂载本地 PaddleOCR-VL-1.6（0.9B VLM）

**默认关闭、显式选择才启用**。它专门补 Apple Vision 的短板：复杂表格、公式、印章、生僻字、版面阅读顺序。

### 本机已挂载的模型

| 项 | 值 |
|---|---|
| 目录 | `/Users/zosie/PaddleOCR-VL-1.6-4bit`（本项目按 `$HOME/PaddleOCR-VL-1.6-4bit` 自动探测） |
| 来源 | ModelScope `mlx-community/PaddleOCR-VL-1.6-4bit`，4-bit MLX 量化 |
| 权重 | `model.safetensors` 703,562,705 B ≈ 671 MiB；`tokenizer.json` 11.2 MB |
| 配置 | `model_type: paddleocr_vl`，4-bit affine（group_size 64），924 个张量 |
| 许可 | Apache-2.0 |

> 权重是用 `git lfs pull` 补齐的。若只 `git clone` 而不拉 LFS，`model.safetensors`
> 会是 134 字节的指针，MLX 无法加载；`scripts/start-paddleocr-vl.sh --check` 会检测并提示。

### 架构

0.9B 权重塞不进 2.5 MB 的 Rust 二进制，所以是**独立本地服务 + HTTP**：

```
浏览器 → markitdown-rs (Rust :5001) → 127.0.0.1:8111/v1/chat/completions
                                              ↓
                              mlx_vlm.server（隔离 Python 3.12 venv）
                                              ↓
                              PaddleOCR-VL-1.6-4bit 本地目录
                                              ↓
                              Apple Silicon Metal GPU（MLX 默认设备 = gpu）
```

- **GPU**：MLX 默认设备就是 `Device(gpu, 0)`，本机实测 Metal 可用、芯片 Apple M4、16GB 统一内存。加载即走 Metal GPU，无需任何参数；不是 CUDA，也不是 ANE。
- **不进 `auto` 链路**，默认仍是零下载的 Apple Vision。服务没起时 UI 置灰并显示启动命令；显式选中但不可达会明确报错。
- **不联网、不上传**：服务只绑定 `127.0.0.1`，模型从本地目录读取。
- [官方 Apple Silicon 教程](https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/docs/version3.x/pipeline_usage/PaddleOCR-VL-Apple-Silicon.md) 明确 M4 是唯一完成精度验证的芯片；本机正是 M4 Mac mini。

### 启用

```bash
./scripts/start-paddleocr-vl.sh --check   # 探测权重与服务状态
./scripts/start-paddleocr-vl.sh           # 前台启动（Ctrl-C 停止）
```

脚本会：验证本地权重不是 LFS 指针 → 用 `uv` 建隔离 Python 3.12 venv（`.venv-vlm/`）
并装 `mlx-vlm` → 以 `--model <本地目录>` 启动 `mlx_vlm.server`。起来后在 WebUI 的
「OCR 引擎」下拉里选 **PaddleOCR-VL-1.6**。

覆盖路径与参数：

| 变量 | 默认值 |
|---|---|
| `MARKITDOWN_VLM_MODEL` | 自动探测 `$HOME/PaddleOCR-VL-1.6-4bit`，否则 `mlx-community/PaddleOCR-VL-1.6-4bit` |
| `MARKITDOWN_VLM_URL` | `http://127.0.0.1:8111` |
| `MARKITDOWN_VLM_PORT` | `8111` |
| `MARKITDOWN_VLM_PROMPT` | `OCR:` |
| `MARKITDOWN_VLM_TIMEOUT_SECS` | `300` |

命令行直接试（不经过 WebUI）：

```bash
.venv-vlm/bin/python -m mlx_vlm generate \
  --model /Users/zosie/PaddleOCR-VL-1.6-4bit \
  --image /path/to/invoice.png --prompt "OCR:" --max-tokens 512
```

> 用**绝对路径**做 `--model`，不要写 repo id —— 否则 `mlx_vlm` 会重新下载一份到
> `~/.cache/huggingface/hub/`，白费 0.7GB。

### 本机实机验证（2026-11-06，M4 Mac mini / 16GB / macOS 26.2）

| 检查 | 结果 |
|---|---|
| `git lfs pull` | `model.safetensors` 703,562,705 B、`tokenizer.json` 11,188,983 B |
| MLX 环境 | `mlx 0.32.4` / `mlx-vlm 0.7.12`，`default_device = Device(gpu, 0)`，Metal 可用 |
| `scripts/start-paddleocr-vl.sh` | 权重校验通过 → 复用 `.venv-vlm` → 模型 **约 1 秒加载完成**，`/health` 返回 `loaded_model: /Users/zosie/PaddleOCR-VL-1.6-4bit` |
| Rust capabilities | `paddleocr_vl: true`，模型路径自动解析为本地目录 |
| 中文图片 `chinese.png` | `简体中文 OCR 精准识别测试 / MarkItDown WebUI 2026 / 上海 · 北京 · 深圳 12345` —— 比 Apple Vision 的 `MarkltDown WebUl` 拼写更准 |
| 水费电子发票 PDF | 440 字符，购买方/销售方分节、表格行 `*水冰雪*水费 经营用水 吨 1 3.7378640776699 3.74 3% 0.11`、合计 `¥3.74 / ¥0.11` 都保留了结构；Vision 会把表格散成一行行并出现错字 |

仍有小瑕疵（如「电子发票（普发票）」漏字、大写金额前一个 `⊗` 符号、个别数字与 Vision 结果不一致），说明它也不是逐字无损；复杂票据建议人工复核关键数字。

### 已知限制（诚实说明）

- [官方教程 3.1 节](https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/docs/version3.x/pipeline_usage/PaddleOCR-VL-Apple-Silicon.md) 明确：`mlx_vlm.server` **只负责 VLM 推理环节**，不提供完整端到端文档解析 API，且「强烈不建议直接通过 HTTP 请求或 OpenAI 客户端调用该服务处理文档图像」。本项目的 Rust 端正是直接调 `/v1/chat/completions`，因此拿到的是**元素级 OCR / 文本 spotting**，不含 PaddlePaddle 那层的版面检测与裁剪。要完整 page-level 解析需额外部署官方 pipeline（Apple Silicon 上不支持官方 Docker 镜像，只能手动装 PaddlePaddle CPU + `paddleocr[doc-parser]`）。
- 权重作者自测（M2 Pro / 32GB，非本机）：4bit 约 455 tok/s decode、峰值 0.785GB；并警告不同 prompt 下量化档位排序会翻转、样本不含照片/手写。**不是中文精度保证**，建议用自己的发票样本 A/B。
- 首次启动要加载 0.7GB 权重，比 Apple Vision（~0.1s）慢得多；长文档别进 `auto`。

---

## 新旧对比

| | 原项目 (markitdown-webui2) | markitdown-rs |
|---|---|---|
| 运行形态 | Python Flask + uv venv + LaunchAgent + Tauri WebView | 单个 Rust 二进制（约 2.5 MB） |
| Mac「应用」 | Tauri `.app` / DMG + PyInstaller sidecar（本质是网页） | **无**，直接浏览器打开 |
| 中文 OCR | 多个外部 OCR 引擎 + 大体积模型缓存 | 系统内置 Apple Vision / Windows OCR（0 下载）+ Tesseract 兜底 |
| 文档解析 | Microsoft MarkItDown (Python) | 纯 Rust，可选调用 markitdown CLI |
| 安装脚本 | `install.sh/ps1`、`start/stop/restart/status/uninstall`、`bootstrap`、`install-mcp` … | 一行命令，两个脚本 |
| 后台任务 / 历史 | SQLite + 批量队列 + 重试 + diff + 30 天留存 | 无（同步请求，浏览器里合并多文件） |
| MCP 接入 | 注册到三个 agent CLI | 去掉，HTTP API 可直接被 agent 调用 |
| 云 OCR | 第三方云解析（需显式同意上传） | 去掉，纯本地 |

> 取舍：批量队列、历史库、diff、MCP 注册、云 OCR 都是让项目「臃肿」的部分，本重写刻意删掉。
> 它们不影响核心能力（文档 → Markdown + 中文 OCR），HTTP API 保持简单，想接回 MCP 只需一层薄转发。

---

## 架构

```
src/main.rs        axum HTTP 服务，页面用 include_str! 内嵌进二进制
src/convert.rs     格式分发；纯 Rust 优先，可选 markitdown CLI，图片/扫描 PDF 走 OCR
src/documents.rs   txt/csv/json/xml/html/docx/xlsx/pptx/pdf 文字层的纯 Rust 转换器
src/ocr.rs         Apple Vision (FFI) / Windows 内置 OCR / Tesseract / PDF 渲染
src/winocr.rs      Windows.Media.Ocr 桥接（内嵌 PowerShell 脚本，失败自动回退）
src/vlm.rs         可选 PaddleOCR-VL-1.6 客户端（本地 MLX 服务，自动探测 $HOME 模型目录）
src/vision_ocr.m   macOS 原生桥：Vision 文字识别 + PDFKit 逐页渲染（ARC，C ABI）
scripts/winocr.ps1 Windows 内置 OCR 脚本（编译期内嵌）
scripts/start-paddleocr-vl.sh  启动本地 PaddleOCR-VL MLX 服务（Metal GPU）
build.rs           用 cc 编译 .m 并链接系统框架（用 CARGO_CFG_TARGET_OS 判断目标平台）
static/index.html  单页 WebUI（无构建、无 CDN、无外部资源）
install.sh / install.ps1   一行命令安装器（交互式选 OCR 引擎）
```

非 macOS 平台会自动跳过 Objective-C 文件与系统框架，退回 Windows 内置 OCR / Tesseract + poppler，
项目仍可编译运行（已用 `cargo check --target x86_64-pc-windows-gnu` 验证）。

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
| `.pdf` | `pdf-extract` 抽文字层；无文字层时用 PDFKit / poppler 逐页渲染 + OCR |
| 图片 `jpg/png/webp/bmp/tiff` | 直接 OCR |
| `.doc` `.xls` `.ppt` | 旧二进制格式，需已安装 markitdown CLI，或先另存为新格式 |

## Apple Vision 中文识别的关键细节

`VNRecognizeTextRequest`（revision 3 / `.accurate`）本身支持 `zh-Hans`、`zh-Hant`、
`yue-Hans`、`yue-Hant`，走系统 ANE/GPU，零模型下载，清晰印刷体中文很快很准。

**但有一个必须遵守的坑**：新版 macOS 上 `recognitionLanguages` 数组里中文必须排在英文**前面**：

| 设置 | 中英混排通过率 |
|---|---|
| `["zh-Hans","en-US"]` | 13/14 ✅ |
| `["en-US","zh-Hans"]` | 0/14 ❌ |
| 自动检测 | 3/14 ❌ |

来源：[macos27-vision-ocr-bug](https://github.com/missuo/macos27-vision-ocr-bug)。
所以本项目默认值固定为 `zh-Hans,en-US`，并且 `vision_ocr.m` 会动态调用
`supportedRecognitionLanguages(for:revision:)` 过滤不支持的语言标识，避免 Vision 抛异常。

**适用范围**：清晰印刷体、截图、简体/繁体中英混排很好；复杂表格、竖排、手写、生僻字、
印章、公式不在能力范围内。

## Windows 内置 OCR 的注意事项

Windows 侧没有 Apple Vision，用 `Windows.Media.Ocr` 作为零下载通道。它有三个硬限制：
中文需要管理员安装 `Language.OCR~~~zh-CN~0.0.1.0` 语言包（走 Windows Update，需联网）、
官方要求 MSIX 包标识（实测裸 exe 可用但存在口径矛盾）、无版面/表格/公式理解且中文精度偏弱。
本项目把它放在独立 PowerShell 子进程里调用，**失败自动回退 Tesseract**，所以缺语言包不会让转换失败。
详见 [docs/OCR-ENGINES.md](docs/OCR-ENGINES.md)。

## 已知限制

- `pdf-extract` 对部分 PDF 字体映射会把个别汉字输出成 CJK 兼容表意文字（如 `⼀`/`⽂`）；需要逐字精确时勾选「优先使用 MarkItDown CLI」。
- `pdf-extract` 0.7 只认 `Identity-H` 编码，遇到中文电子发票常用的 `UniGB-UCS2-H` 会内部断言崩溃。本版本已做**事前检测 + 自动降级**（见下），不再受影响。
- 纯 Rust 的 docx/xlsx/pptx 转换器是「够用」级别，不还原图片、合并单元格、批注、样式；要微软原版输出请勾 MarkItDown CLI。
- 扫描 PDF 逐页 OCR 是同步请求，长文档会让浏览器转圈几分钟（显示已用秒数），没有取消按钮；单次上限 300 页。
- Apple Vision 只在 macOS 11+ 可用；Windows 走内置 OCR / Tesseract + poppler。
- 不提供历史库、批量队列、MCP 注册和云 OCR——这是刻意的瘦身，不是遗漏。

## 故障排查：上传 PDF 后网站打不开

**已修复。** 早期版本上传某些中文 PDF 后整个服务会退出：

1. 中文电子发票等 PDF 的 Type0 字体常用 `/Encoding /UniGB-UCS2-H`；
2. `pdf-extract` 0.7 对该分支写的是 `assert!(name == "Identity-H")`，遇到就 panic；
3. 早期 release 配置里写了 `panic = "abort"`，一个请求的 panic 直接杀死整个进程。

修复做了四层防护，任一文档都**不可能再拖垮服务**：

| 层 | 措施 |
|---|---|
| 1 | `lopdf` 事前扫描 PDF 字体编码，发现非 `Identity-H` 的 Type0 编码就**根本不调用**会 panic 的分支 |
| 2 | `documents::pdf_text` 用 `catch_unwind` 隔离 `pdf-extract`，panic 转成普通 `Err` |
| 3 | `main.rs` 对每次转换再做一次请求级 `catch_unwind` 兜底 |
| 4 | 去掉 `panic = "abort"`，恢复默认 unwind |

降级链：**文字层 → 逐页渲染 + OCR（PDFKit/poppler） → MarkItDown CLI**。日志里会打印一行
`[pdf] 文字层解析不可用，降级到 OCR：...`，属正常提示。实测：`~/Downloads` 里 50 个 PDF
有 27 个会触发该断言（水费电子发票），连续转换全部返回 200、服务存活、日志 0 条 panic。
回归测试见 `cargo test`（11 项，含编码检测夹具 `tests/fixtures/`）。

## 页面打不开：先确认端口/进程

默认端口 **5001**，地址 `http://127.0.0.1:5001/`。

```bash
ps aux | grep '[m]arkitdown-rs'
lsof -nP -iTCP:5001 | grep LISTEN
curl http://127.0.0.1:5001/health
```

没在跑就重跑安装器，或：

```bash
nohup ./target/release/markitdown-rs --port 5001 > /tmp/markitdown-rs.log 2>&1 &
```

这个二进制是普通前台进程，不会随登录自启、崩溃后也不会自动拉起——安装器生成的
`~/.local/bin/markitdown-rs-start`（Windows 为 `start.cmd`）可用来手动启动。

## License

MIT。调用的框架/工具各自遵循其许可：Apple Vision / Windows.Media.Ocr（系统框架）、
Tesseract（Apache-2.0）、Microsoft MarkItDown（MIT）。
