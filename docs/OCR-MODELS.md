# Mac / Apple Silicon 高精度中文 OCR 模型调研

> **2026-11-06 复核更新**：下面的模型存在性、参数量、体积、得分已用 HuggingFace API 和官方 README
> 一手核实，修正了初版调研中的几处转述误差；PaddleOCR-VL-1.6 现已作为**可选引擎接入**本项目
> （见 [README.md](../README.md) 的「可选引擎」一节）。核实数据列在文末。

## 1. 先说结论

你现在的项目其实已经选对了轻量模型：**RapidOCR PP-OCRv6 Small** 是 2026 年轻量级中文 OCR 的甜点。真正的问题不是模型，而是：

1. 用 Python + ONNXRuntime CPU 打包，没吃到 Mac 的 Core ML / ANE；
2. 为了兜底装了 160+ 个 Tesseract 语言包、又塞了 2.35GB 的 MLX Unlimited-OCR；
3. 外面再套 Tauri + PyInstaller sidecar，导致体积和复杂度爆炸。

所以「更好适配 Mac、中文更准」的答案分两类：

- **保持精度、更适配 Mac**：把同一套 PP-OCRv6 ONNX 模型搬到 Rust `ort` + CoreML EP，或用 Apple Vision 做零依赖快速通道。
- **中文/版面精度更高**：换 0.9B 级专用文档 VLM（PaddleOCR-VL-1.6 96.34% / MinerU2.5-Pro 95.75% / GLM-OCR 95.22%），而不是更大的通用 VLM（Qwen3-VL-235B 反而只有 89.78%）。

---

## 2. 2026 年精度排名（OmniDocBench v1.6）

| 模型 | 得分 | 参数量 | 许可 |
|---|---:|---:|---|
| **PaddleOCR-VL-1.6** | **96.33%**（官方 README） | 958,588,736 参数 | Apache-2.0 |
| MinerU2.5-Pro | 95.75% | 1.16B | Apache-2.0 + 附加条款 |
| GLM-OCR | 95.22% | 0.9B~1.33B（口径不一） | MIT |
| Qwen3-VL-235B | 89.78% | 235B | Qwen License |

来源：[Roboflow 2026 开源 OCR 横评](https://roboflow.com/blog/best-open-source-ocr-models)、[AITechModel 横评](https://aitechmodel.com/best-open-source-ocr-models/)。
**专用小模型全面胜过大 VLM**，这是选型的第一个结论。

---

## 3. 候选方案对比

| 方案 | 中文精度 | Mac/AS 优化 | 离线 | 体积 | 集成路径 | 许可 | 评价 |
|---|---|---|---|---:|---|---|---|
| **Apple Vision** `VNRecognizeTextRequest` | 中上（清晰印刷体好） | ★★★ 系统 ANE/GPU，~0.1s/图 | ✅ | 0 MB | Swift/ObjC；Rust `apple-vision` / `car-vision` / `macocr` / Swift FFI | 系统框架 | 零依赖最快；无版面理解；**语言顺序敏感** |
| **PP-OCRv6**（2026.06，你已在用） | 很高，比 v5_server 检测 +4.6% / 识别 +5.1%，50 语言 | ★★ 官方 6.1× on M4 (tiny)、5.2× CPU (OpenVINO) | ✅ | tiny 1.5M / small 7.7M / medium 34.5M 参数 | Python / ONNX / RapidOCR / Rust crates | Apache-2.0 | 轻量+高精度甜点，无版面理解 |
| **RapidOCR + ort CoreML EP** | 等同所载 PP-OCR 模型 | ★★★ `ort` 的 macOS 预编译二进制自带 CoreML EP，可走 ANE/GPU | ✅ | 模型十几 MB 级 | `rapidocr_core` / `rapidocr-rs` / `paddle-ocr-rs` / `paddleocr_rs_onnx` | 代码 Apache-2.0 | **Rust 重写的最佳落点**，去掉 Python |
| **PaddleOCR-VL-1.6 + MLX**（已接入为可选引擎） | 顶级 96.33%，文本/公式/表格/古籍/生僻字/印章/图表 | ★★★ `mlx_vlm.server` OpenAI 兼容服务走 Metal/ANE；官方另有 Apple Silicon 教程 | ✅ | MLX 4bit **0.68GB**（HF API `usedStorage` 714,751,688 B），905,601,648 参数 | **本项目：Rust `ureq` → 本地 HTTP**；也可 Python CLI/MLX-VLM | Apache-2.0 | 精度第一；需一次性下载 0.68GB，速度远慢于 Vision |
| **GLM-OCR** | 顶级 95.22%，强在复杂表格/代码/印章/混排 | ★★★ 官方 `examples/mlx-deploy`（MLX-VLM 走 Metal GPU） | ✅ | ~0.9B，有 4bit Mac 教程 | Python SDK / vLLM / Ollama / MLX | **MIT（最干净）** | 只 8 语言，生态小 |
| **MinerU2.5-Pro** | 顶级 95.75%，109 语言 | ★★★ AS 默认 MPS + llama.cpp/Metal，独占 `vlm-mlx-engine`；M4 mini 16GB 实测 ~32–38s/页 | ✅ | ~1.16B ≈2.3GB | Python CLI/API/MCP | Apache-2.0 + 附加条款 | 复杂 PDF 最强之一；重量级、许可有条件 |
| **PP-StructureV3** | 高（OCR + 版面/表格/公式/阅读顺序） | 同 PP-OCR | ✅ | 多模型数百 MB | Python → Markdown/JSON | Apache-2.0 | 坐标精细，仍多模型 |
| **PP-OCRv5 / v4** | 高 / 中上 | 一般 | ✅ | 十几 MB ~ 182MB | 同上 | Apache-2.0 | 已被 v6 超越，不必再选 |
| **Tesseract 5** | 中下（本组最弱），LSTM 2017 数据 | ✗ 纯 CPU | ✅ | `tessdata_best/chi_sim` 13.1MB，fast 2.47MB | CLI / `leptess` / `tesseract-rs` | Apache-2.0 | 只配兜底，中文主力不合理 |
| **Qwen3-VL / Qwen2.5-VL** | 中高（235B 仅 89.78%） | ★★ MLX 可用 | ✅ | 8B 5bit ≈5–6GB | MLX-VLM / Ollama | 逐个核对（3B 限非商用研究） | 通用推理强，OCR 性价比差 |
| **DeepSeek-OCR** | 中高，长文档上下文压缩 | ★★ `ollama run deepseek-ocr`（6.7GB） | ✅ | 3B / 6.7GB | Ollama / vLLM | MIT | 长文档特色，对 prompt 极敏感 |
| **GOT-OCR2.0** | 中高（2024，已被超越） | ✗ 官方要 CUDA；社区有 GGUF/MNN/CPU 移植 | ✅ | 580M ≈1.2GB | Python / 社区 GGUF | 权重 **CC BY-NC 4.0 风险** | 精度落后 + 商用风险，不推荐 |
| **olmOCR-2-7B** | 中（英文文档 SOTA，中文证据弱） | ★ 社区 MLX 8bit | ✅ | 8bit ≈8–9GB | vLLM / LM Studio | Apache-2.0 | 中文非目标 |
| **dots.ocr / dots.mocr** | 高（~3B，版面/图表最强） | △ 无官方 Mac/MLX 路径 | ✅ | ≈6GB | Python / vLLM | MIT + 附加条款 | Mac 需自移植 |
| **Unlimited-OCR**（项目已内置） | 高（3B） | ★★ 社区 MLX 4bit | ✅ | ~2.35GB | Python/MLX | MIT | 项目 README 已注明长文档会退化成重复文本，正确排除了 Auto |

---

## 4. Apple Vision 的必修坑

`VNRecognizeTextRequest` revision 3 + `.accurate` 在 macOS 27.2 (M1 Pro) 支持 33 种语言，含
`zh-Hans` / `zh-Hant` / `yue-Hans` / `yue-Hant`；**默认只有 `["en_US"]`**。

中英混排实测（[macos27-vision-ocr-bug](https://github.com/missuo/macos27-vision-ocr-bug)）：

| `recognitionLanguages` | 通过率 |
|---|---:|
| `["zh-Hans","en-US"]` | **13/14** ✅ |
| `["en-US","zh-Hans"]` | 0/14 ❌ |
| 自动检测 | 3/14 ❌ |
| `.fast` 路径中文 | 基本全废 |

CPU / GPU / ANE 限定都修不了，唯一解法是**中文标识符放数组第一位**，并用
`supportedRecognitionLanguages(for:revision:)` 动态过滤。本项目已按此实现。
官方资料：[Apple Vision 文档](https://developer.apple.com/documentation/vision/vnrecognizetextrequest)、[识别语言查询](https://developer.apple.com/documentation/vision/vnrecognizetextrequest/supportedrecognitionlanguages(for:revision:))。

---

## 5. Rust 路线可行性（已验证）

- `ort` crate 有 `features = ["coreml"]`；官方明确「**All macOS builds come with the CoreML EP enabled**」，pyke 预编译二进制即带 CoreML EP，无需自编译 ONNX Runtime。[ort 预编译说明](https://ort.pyke.io/misc/prebuilt-binaries)
- CoreML EP 支持 `MLComputeUnits`（`CPUOnly` / `CPUAndNeuralEngine` / `CPUAndGPU` / `ALL`）、`MLProgram`（macOS 12+）、`ModelCacheDirectory`（缓解首图编译数分钟）。[ONNX Runtime CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)
- 现成 Rust crate：`rapidocr_core`（约 200KB / 5K SLoC，det→cls→rec，TOML 配置）、`rapidocr-rs`、`paddle-ocr-rs`、`paddleocr_rs_onnx`；Vision 侧有 `apple-vision`、`car-vision`、`macocr`。
- 限制：CoreML EP 有算子白名单（如 Conv 仅 1D/2D）、动态 shape 掉性能，**需实测 PP-OCRv6 算子的实际覆盖率**，否则会大量回退 CPU。

---

## 6. 推荐排名

| 场景 | 首选 |
|---|---|
| **零依赖 / 最快 / 隐私最好**（本项目默认层） | **Apple Vision**，revision 3 + `.accurate`，`recognitionLanguages = ["zh-Hans","en-US"]`。0 模型、0 下载、系统 ANE。仅用于清晰印刷体/截图 |
| **Lean Rust 重写的最佳整体落点** | **PP-OCRv6 small ONNX + Rust `ort` + CoreML EP**。中文精度不变、7.7M 参数、Apache-2.0、完全离线、首次吃 ANE/GPU、无 Python sidecar |
| **精度优先（复杂版面/表格/公式/扫描 PDF）** | **PaddleOCR-VL-1.6 + MLX 4bit（0.68GB）**——厂商官方权重 + 官方组织 MLX 量化 + 官方 Apple Silicon 教程三者齐全，且**已接入本项目为可选引擎**。次选 GLM-OCR（MIT、官方 MLX 示例）、MinerU2.5-Pro（许可有附加条款） |
| **轻量/可嵌入** | PP-OCRv6 tiny（1.5M）或 RapidOCR 默认 small 套装；极致轻量就是 Vision（0 字节） |
| **只配兜底** | Tesseract `chi_sim+chi_tra+eng`（不必维护 160+ 全量语言包） |
| **不建议作中文主力** | Tesseract 5 中文、通用大 VLM（Qwen3-VL-235B 等）、GOT-OCR2.0（精度落后 + 权重 CC BY-NC 风险） |

---

## 7. 针对本项目的落地建议

1. **默认层**：Apple Vision（`zh-Hans,en-US`）——本 Rust 重写已实现，零下载、0.1s 级。
2. **快速层（尚未实现）**：PP-OCRv6 ONNX 经 `ort` + CoreML EP，作为「需要版面/坐标/更好混排」时的第二引擎。加在 `ocr::ocr_image` 一个入口即可。
3. **精度层（✅ 已实现，默认关闭）**：**PaddleOCR-VL-1.6 MLX 4bit（0.68GB）** 通过本地 OpenAI 兼容服务接入，WebUI 里显式选择才启用、不进 `auto`、服务不可达时明确报错。启用脚本 `scripts/start-paddleocr-vl.sh`。保留「长文档不进 Auto」的判断。
4. **兜底层瘦身**：Tesseract 只留 `chi_sim+chi_tra+eng`；扫描 PDF 默认引擎从 Tesseract 换成 Vision/PP-OCRv6，这是中文精度提升最明显的一处。
5. **隐私**：保持 `127.0.0.1` 绑定，不要为精度默认开启任何云 OCR。
6. **许可核对**：PP-OCRv6 / RapidOCR / PaddleOCR-VL / Vision 调用 / Tesseract 均无商用障碍；引入 MinerU、dots.mocr、GOT-OCR2.0、Qwen3-VL-3B 需逐条核对附加条款与非商用限制。

---

## 8. 一手核实数据（2026-11-06）

初版调研由子代理完成，其搜索后端曾返回 401、改用 DuckDuckGo HTML 抓取。以下项目已用
`huggingface.co/api/models/...` 与官方 `README.md` 原文重新核对，并修正了转述误差：

| 项 | 初版报告 | 一手核实结果 |
|---|---|---|
| OmniDocBench v1.6 得分 | 96.34% | **96.33%**（[官方模型卡](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6) 原文 "96.33%"） |
| 参数量 | 959M | **958,588,736**（HF API `safetensors.parameters.total`） |
| MLX 4bit 体积 | 703,562,705 B / 0.70GB | **714,751,688 B ≈ 0.68GB**（HF API `usedStorage`；权重 README 写 0.68 GB） |
| MLX 4bit 参数 | 906M | **905,601,648**（U32 770,607,232 + BF16 134,994,416） |
| arXiv | 2510.14528（初代 VL） | 1.6 是 **2606.03264**；2510.14528 属于 2025-10 的初代 PaddleOCR-VL |
| 模型存在性 | 未验证 | ✅ 官方仓库 2026-05-27 创建、95,141 下载、572 likes；MLX 量化 2026-08-12 创建 |

其他一手发现：

- 官方 1.6 README 明确写 **"For macOS users, please use Docker to set up the environment"**（指 PaddlePaddle 完整 pipeline）。本项目的接入绕开了它，直接用 [mlx-vlm](https://github.com/Blaizzy/mlx-vlm) 的本地服务跑 4bit 权重。
- `mlx_vlm.server` 端点（[官方 README](https://github.com/Blaizzy/mlx-vlm)）：`GET /health`、`GET /v1/models`、`POST /v1/chat/completions`（OpenAI 兼容，`image_url` 支持本地路径/data URL）。
- MLX 4bit 权重 README 自带量化保真度测量，并**明确警告**：6 份合成文档在不同 prompt 下 4bit/5bit/bf16 排序会翻转，"不是质量排名"；样本不含照片、手写、非拉丁文字。所以不要把它当作中文精度保证。
- 官方 1.6 的 CLI：`paddleocr doc_parser -i <image> --pipeline_version v1.6`；VLM 服务后端参数 `--vl_rec_backend vllm-server --vl_rec_server_url ...`。

**仍未验证**：真实 4bit 权重的本机精度/速度（未下载）、`ort` CoreML EP 对 PP-OCRv6 算子的覆盖率、
Apple Vision 在当前 macOS 版本的中英混排回归。版本基准 2026-11，实施前请复核官方最新发布说明。

## 9. 主要来源

- 项目：[nfeuism/markitdown-webui2](https://github.com/nfeuism/markitdown-webui2)
- Apple Vision：[官方文档](https://developer.apple.com/documentation/vision/vnrecognizetextrequest) · [Recognizing Text in Images](https://developer.apple.com/documentation/vision/recognizing-text-in-images) · [macOS 27 中英混排 bug](https://github.com/missuo/macos27-vision-ocr-bug)
- Rust / Vision：[apple-vision](https://docs.rs/apple-vision) · [car-vision](https://docs.rs/car-vision) · [macocr](https://lib.rs/crates/macocr) · [macOCR](https://github.com/schappim/macOCR) · [Swift-OCR Rust FFI](https://github.com/httpjamesm/macos-swift-ocr-rust-ffi)
- PaddleOCR：[GitHub](https://github.com/PaddlePaddle/PaddleOCR) · [Apple Silicon 教程](https://www.paddleocr.ai/main/en/version3.x/pipeline_usage/PaddleOCR-VL-Apple-Silicon.html) · [PP-OCRv5](https://www.paddleocr.ai/main/en/version3.x/algorithm/PP-OCRv5/PP-OCRv5.html) · [PaddleOCR-VL 论文](https://arxiv.org/abs/2510.14528) · [PaddleOCR-VL-1.6-4bit](https://huggingface.co/mlx-community/PaddleOCR-VL-1.6-4bit) · [paddleocr-vl.swift](https://github.com/mlx-community/paddleocr-vl.swift)
- RapidOCR：[GitHub](https://github.com/RapidAI/RapidOCR) · [模型列表](https://rapidai.github.io/RapidOCRDocs/main/en/model_list/) · [rapidocr_core](https://docs.rs/rapidocr-core) · [paddle-ocr-rs](https://github.com/mg-chao/paddle-ocr-rs)
- ONNX Runtime：[CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html) · [ort](https://github.com/pykeio/ort) · [预编译二进制](https://ort.pyke.io/misc/prebuilt-binaries)
- Tesseract：[tessdoc](https://tesseract-ocr.github.io/tessdoc/) · [Data Files](https://tesseract-ocr.github.io/tessdoc/Data-Files.html) · [leptess](https://houqp.github.io/leptess/leptess/index.html)
- 2026 基准：[Roboflow](https://roboflow.com/blog/best-open-source-ocr-models) · [AITechModel](https://aitechmodel.com/best-open-source-ocr-models/)
- VLM / MLX：[GLM-OCR MLX 部署](https://github.com/zai-org/GLM-OCR/blob/main/examples/mlx-deploy/README.md) · [MinerU](https://opendatalab.github.io/MinerU/quick_start/) · [olmOCR-2](https://allenai.org/blog/olmocr-2) · [dots.ocr](https://github.com/studio-dots-ai/dots.ocr) · [DeepSeek-OCR](https://github.com/deepseek-ai/DeepSeek-OCR) · [GOT-OCR2.0](https://github.com/Ucas-HaoranWei/GOT-OCR2.0) · [mlx-vlm](https://github.com/Blaizzy/mlx-vlm) · [mlx4ocr](https://pypi.org/project/mlx4ocr/)
