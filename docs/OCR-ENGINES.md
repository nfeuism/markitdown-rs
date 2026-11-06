# OCR 引擎选型说明（macOS / Windows）

> 本文档只说明**本项目实际采用**的 OCR 引擎及安装时的选择。第三方 ONNX / VLM
> 模型方案已按需求从代码与文档中全部移除，不在本项目范围内。
> 核实时间基准：2026-11-06；引用均为一手源，未做真机 benchmark。

## 1. 本项目支持的引擎

| 引擎 | 平台 | 下载体积 | 运行时离线 | 中文能力 | 备注 |
|---|---|---:|---|---|---|
| **Apple Vision** `VNRecognizeTextRequest` | macOS 11+ | 0 MB | ✅ | 简/繁中文、中英混排好 | 系统 ANE/GPU 加速、~0.1s/图；无版面/表格理解 |
| **Windows 内置 OCR** `Windows.Media.Ocr` | Windows 10/11 | 0 MB | ✅ | 需 zh-CN / zh-TW OCR 语言包 | 词框 + 行；无版面/表格/公式；失败自动回退 Tesseract |
| **Tesseract 5** | 全平台 | chi_sim best 12.47 MiB + 引擎 | ✅ | 清晰印刷体可用，复杂中文偏弱 | 跨平台兜底；`tessdata_best` 比 `fast` 准 |

三者都**不联网、不上传**，服务固定绑定 `127.0.0.1`。

## 2. 安装时如何选择

安装器会问一次，选择结果写进启动器脚本的 `MARKITDOWN_OCR_ENGINE` 环境变量；
WebUI 会预选该引擎，用户仍可在页面上临时改。

| 平台 | 选项 | 会安装什么 | 需要联网/管理员 |
|---|---|---|---|
| macOS | 1) Apple Vision（默认） | 无 | 否 |
| macOS | 2) Tesseract | `brew install tesseract tesseract-lang` | 装依赖时需联网 |
| macOS | 3) 两者 | 同 2，引擎设为 `auto` | 装依赖时需联网 |
| Windows | 1) Windows 内置 OCR | 检测并提示安装 `Language.OCR~~~zh-CN~0.0.1.0` | **是**（管理员 + Windows Update/FOD） |
| Windows | 2) Tesseract 精准（默认） | winget 装 Tesseract + 下载 `tessdata_best` 的 chi_sim/chi_tra/eng | 装依赖时需联网 |
| Windows | 3) 两者 | 同上，引擎设为 `auto` | 是 |

**「纯离线」的准确含义**：运行时全部离线；安装阶段下载二进制、Tesseract 或语言包需要联网。
真正零下载的组合是 macOS 的 Apple Vision，或 Windows 已预装 zh-CN OCR 语言包的情况。

## 3. Apple Vision 的两个必须知道的点

**语言顺序**：新版 macOS 中英混排时 `recognitionLanguages` 必须把中文放第一位。
`["zh-Hans","en-US"]` 通过率 13/14，`["en-US","zh-Hans"]` 0/14，自动检测 3/14，
`.fast` 路径中文基本失效；CPU/GPU/ANE 限定都修不了。
来源：[macos27-vision-ocr-bug](https://github.com/missuo/macos27-vision-ocr-bug)。
本项目默认值锁死 `zh-Hans,en-US`，并用
`supportedRecognitionLanguages(for:revision:)` 动态过滤不支持的标识符。

配置：revision 3、`.accurate`、`usesLanguageCorrection = YES`，按 boundingBox 恢复阅读顺序。
来源：[Apple Vision 文档](https://developer.apple.com/documentation/vision/vnrecognizetextrequest)。

## 4. Windows 内置 OCR 的限制（重要）

不能把它当成 Apple Vision 的对等物，实际是「零下载兜底层」：

1. **中文不是开箱即用**：默认通常只有显示语言（en-US）的 OCR 包。简体需管理员执行
   `Get-WindowsCapability -Online | ? Name -Like 'Language.OCR*'` +
   `Add-WindowsCapability -Online`（走 Windows Update/FOD，需联网）；离线环境可用 FOD ISO
   `-Source`。**Home Single Language 版无法添加语言**。
   来源：[Dynamsoft FAQ](https://www.dynamsoft.com/web-twain/docs/faq/add-ocr-language-for-windows-media-ocr.html)、
   [PowerToys Text Extractor](https://learn.microsoft.com/en-us/windows/powertoys/text-extractor)、
   [微软语言包文档](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/available-language-packs-for-windows?view=windows-11)。
2. **包标识（package identity）**：[Microsoft Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-28000)
   明确写「Windows.Media.Ocr 仅支持有包标识（MSIX）的桌面应用」；
   [WindowsAppSDK issue #867](https://github.com/microsoft/WindowsAppSDK/issues/867) 要求开放给非打包应用，2023 年被关闭为「1–2 年内不做」。
   **但** [winocr](https://github.com/riddleling/winocr) 的裸 exe 与 Windows PowerShell 5.1 实测可直接调用，
   官方口径与生态实测不一致。因此本项目把它放在隔离的 PowerShell 子进程里调用，
   **任何失败都自动回退 Tesseract**，不作为唯一路径。
3. **精度**：微软[新 TextRecognizer 文档](https://learn.microsoft.com/en-us/windows/ai/apis/text-recognition)
   称新版（需 NPU + WinAppSDK 1.7.2+，仍需包标识）比 legacy `OcrEngine` 更快更准，
   等于官方承认 legacy 更弱。未找到任何公开中文定量基准；社区定性反映中文断字/漏识明显
   （[Windrecorder #18](https://github.com/yuka-friends/Windrecorder/issues/18)）。
4. **能力**：只返回 Lines/Words + BoundingRect + TextAngle，无段落/阅读顺序/表格/公式；
   单实例只绑定一种语言，无自动语种检测、无多语混排。每个识别请求用 `-Language` 指定，
   WebUI 的 `zh-Hans` / `zh-Hant` 会映射成 `zh-CN` / `zh-TW`。
5. **系统级加速**：未见 legacy OcrEngine 的 GPU/NPU 加速官方文档，不能照搬 Apple Vision 的卖点。

## 5. Tesseract 中文精度定位

[OmniDocBench v1.0](https://raw.githubusercontent.com/opendatalab/OmniDocBench/main/README.md)
Text OCR 段落级归一化编辑距离（越低越好）：ZH 子集 **Tesseract 5.5 = 0.551**，
而同期专用中文 OCR 约 0.055、其它开源方案 0.068。即 Tesseract 中文**明显落后于专用中文 OCR**，
只适合清晰印刷体兜底。这也是本项目在 Windows 上把它放在内置 OCR 之后、并允许两者并用的原因。

数据档选择（[官方说明](https://tesseract-ocr.github.io/tessdoc/Data-Files.html)）：
`tessdata_best/chi_sim` 12.47 MiB 最准最慢，`tessdata_fast/chi_sim` 2.35 MiB 最快最不准。
安装器默认下载 `best` 的 chi_sim / chi_tra / eng。

## 6. 为什么没有更高精度的模型

更高精度的第三方方案（外部 ONNX 识别模型、文档 VLM 等）**按需求已全部移除**：
它们要么需要随包携带 ONNX Runtime / DirectML 运行时与模型权重，要么需要 Python + CUDA/WSL2/Docker
或额外的本地推理服务，与「单二进制、一行命令安装、纯离线」的目标冲突。
本项目的取舍是：**系统内置 OCR 负责零下载快速通道，Tesseract 负责跨平台兜底**，
复杂表格、公式、竖排、手写、印章不在能力范围内。

## 7. 主要来源

- Apple Vision：[官方文档](https://developer.apple.com/documentation/vision/vnrecognizetextrequest) · [语言支持查询](https://developer.apple.com/documentation/vision/vnrecognizetextrequest/supportedrecognitionlanguages(for:revision:))
- Windows OCR：[命名空间](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-28000) · [OcrEngine](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine?view=winrt-26100) · [TextRecognizer](https://learn.microsoft.com/en-us/windows/ai/apis/text-recognition) · [winocr](https://github.com/riddleling/winocr) · [WindowsAppSDK #867](https://github.com/microsoft/WindowsAppSDK/issues/867)
- 语言包：[Dynamsoft FAQ](https://www.dynamsoft.com/web-twain/docs/faq/add-ocr-language-for-windows-media-ocr.html) · [PowerToys](https://learn.microsoft.com/en-us/windows/powertoys/text-extractor) · [微软 FOD 文档](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/features-on-demand-language-fod)
- Tesseract：[Data Files](https://tesseract-ocr.github.io/tessdoc/Data-Files.html) · [Command-Line Usage](https://tesseract-ocr.github.io/tessdoc/Command-Line-Usage.html) · [winget 包](https://winget.run/pkg/UB-Mannheim/TesseractOCR)
- 基准：[OmniDocBench](https://github.com/opendatalab/OmniDocBench)
