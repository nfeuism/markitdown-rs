# markitdown-rs — one-line installer for Windows
#
#   irm https://raw.githubusercontent.com/nfeuism/markitdown-rs/main/install.ps1 | iex
#
# Non-interactive:
#   $env:MARKITDOWN_OCR='tesseract'; irm .../install.ps1 | iex
#   $env:MARKITDOWN_OCR='paddleocr-vl'; irm .../install.ps1 | iex
#
# Asks which OCR engine to use, installs only what that choice needs,
# downloads a prebuilt binary (or builds from source as a fallback), then
# starts the local WebUI at http://127.0.0.1:5001.
# Everything stays on 127.0.0.1. No file is ever uploaded anywhere.

$ErrorActionPreference = 'Stop'

$Repo        = 'nfeuism/markitdown-rs'
$InstallDir  = if ($env:MARKITDOWN_INSTALL_DIR) { $env:MARKITDOWN_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'markitdown-rs' }
$Port        = if ($env:MARKITDOWN_PORT) { $env:MARKITDOWN_PORT } else { '5001' }
$Url         = "http://127.0.0.1:$Port"
$Asset       = 'markitdown-rs-windows-x64.zip'
# Windows uses the official BF16 weights; macOS uses the 4-bit MLX quant.
$VlmWinRepo  = if ($env:MARKITDOWN_VLM_REPO) { $env:MARKITDOWN_VLM_REPO } else { 'https://www.modelscope.cn/PaddlePaddle/PaddleOCR-VL-1.6.git' }
$VlmWinDir   = if ($env:MARKITDOWN_VLM_MODEL) { $env:MARKITDOWN_VLM_MODEL } else { Join-Path $env:USERPROFILE 'PaddleOCR-VL-1.6' }

function Info($m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "==> $m" -ForegroundColor Yellow }
function Die($m)  { Write-Host "==> 错误: $m" -ForegroundColor Red; throw $m }

if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { Die '这个安装器只用于 Windows。macOS 请用 install.sh。' }

Write-Host ''
Write-Host '请选择 OCR 引擎（中文识别）：' -ForegroundColor White
Write-Host '  1) Windows 内置 OCR   系统级、零模型；需已装 zh-CN OCR 语言包（管理员/联网）'
Write-Host '  2) Tesseract 精准      winget 安装 + tessdata_best 简体/繁体/英文  [推荐]'
Write-Host '  3) 两者都要            内置 OCR 优先 + Tesseract 兜底（最稳）'
Write-Host '  4) PaddleOCR-VL-1.6    可选高精度 VLM；下载官方 BF16 模型（约 1.79 GiB）'
Write-Host ''

$choice = $env:MARKITDOWN_OCR
if (-not $choice) {
    $raw = Read-Host '输入 1 / 2 / 3 / 4（默认 2）'
    $choice = if ($raw) { $raw } else { '2' }
}

switch -Regex ($choice.ToString().ToLower()) {
    '^(1|winocr|windows)$' { $engine = 'winocr';    $desc = 'Windows 内置 OCR' }
    '^(2|tesseract)$'      { $engine = 'tesseract'; $desc = 'Tesseract 精准' }
    '^(3|both|auto)$'      { $engine = 'auto';      $desc = '内置 OCR + Tesseract' }
    '^(4|paddleocr-vl|vlm|paddle)$' { $engine = 'paddleocr-vl'; $desc = 'PaddleOCR-VL-1.6（官方 BF16）' }
    default { Die "无法识别的选项：${choice}（可选 1/2/3/4）" }
}
Info "已选择：$desc → 默认引擎 $engine"

# ---- Windows built-in OCR language capability ------------------------------
if ($engine -in @('winocr', 'auto')) {
    try {
        $cap = Get-WindowsCapability -Online |
            Where-Object { $_.Name -like 'Language.OCR*zh-CN*' } |
            Select-Object -First 1
        if ($cap -and $cap.State -ne 'Installed') {
            $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
            $principal = New-Object Security.Principal.WindowsPrincipal($identity)
            $admin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
            if ($admin) {
                Info '安装简体中文 OCR 语言包（需要联网 / Windows Update）…'
                $cap | Add-WindowsCapability -Online | Out-Null
            } else {
                Warn '需要管理员权限安装 zh-CN OCR 语言包，请以管理员 PowerShell 运行：'
                Warn "  $($cap.Name)  →  Add-WindowsCapability -Online -Name '$($cap.Name)'"
                Warn '未安装语言包时，内置 OCR 会自动回退到 Tesseract（若已安装）。'
            }
        } elseif ($cap) {
            Info 'zh-CN OCR 语言包已安装'
        } else {
            Warn '未查询到 zh-CN OCR 语言能力（可能是 Home Single Language 版）。'
        }
    } catch {
        Warn "语言包检测失败：$($_.Exception.Message)"
    }
}

# ---- Tesseract + Chinese tessdata ------------------------------------------
if ($engine -in @('tesseract', 'auto')) {
    $tessExe = Get-Command tesseract -ErrorAction SilentlyContinue
    if (-not $tessExe) {
        $default = 'C:\Program Files\Tesseract-OCR\tesseract.exe'
        if (Test-Path $default) { $tessExe = $default }
    }
    if (-not $tessExe) {
        if (Get-Command winget -ErrorAction SilentlyContinue) {
            Info '安装 Tesseract OCR（winget）…'
            winget install -e --id UB-Mannheim.TesseractOCR --accept-source-agreements --accept-package-agreements
        } elseif (Get-Command choco -ErrorAction SilentlyContinue) {
            Info '安装 Tesseract OCR（chocolatey）…'
            choco install tesseract -y
        } else {
            Warn '未找到 winget/choco，请手动安装 https://github.com/UB-Mannheim/tesseract/wiki'
        }
    }

    $roots = @('C:\Program Files\Tesseract-OCR', 'C:\Program Files (x86)\Tesseract-OCR')
    $tessdata = $null
    foreach ($r in $roots) { if (Test-Path (Join-Path $r 'tesseract.exe')) { $tessdata = Join-Path $r 'tessdata'; break } }
    if ($tessdata) {
        Info "下载中文 tessdata_best 到 $tessdata …"
        New-Item -ItemType Directory -Force $tessdata | Out-Null
        foreach ($lang in @('chi_sim', 'chi_tra', 'eng')) {
            $target = Join-Path $tessdata "$lang.traineddata"
            if (-not (Test-Path $target)) {
                $src = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_best/main/$lang.traineddata"
                try { Invoke-WebRequest -Uri $src -OutFile $target -UseBasicParsing } catch {
                    Warn "$lang 下载失败：$($_.Exception.Message)（可稍后手动下载）"
                }
            }
        }
        [Environment]::SetEnvironmentVariable('TESSDATA_PREFIX', (Split-Path $tessdata -Parent), 'User')
        $env:TESSDATA_PREFIX = Split-Path $tessdata -Parent
    } else {
        Warn '未定位到 Tesseract 安装目录；中文语言包需手动放入 tessdata 目录。'
    }
}

# ---- optional PaddleOCR-VL-1.6 model mount (Windows, official BF16) --------
# ModelScope uses Git LFS: a plain clone leaves 134-byte pointers. We always
# verify model.safetensors is really downloaded.
if ($engine -eq 'paddleocr-vl') {
    if (-not (Test-Path (Join-Path $VlmWinDir 'config.json'))) {
        Info "克隆官方模型仓库：$VlmWinRepo"
        if (Test-Path $VlmWinDir) { Remove-Item -Recurse -Force $VlmWinDir }
        $env:GIT_LFS_SKIP_SMUDGE = '1'
        try { git clone --depth 1 $VlmWinRepo $VlmWinDir } finally {
            Remove-Item Env:\GIT_LFS_SKIP_SMUDGE -ErrorAction SilentlyContinue
        }
    } else {
        Info "复用已存在的模型目录：$VlmWinDir"
    }

    $weight = Join-Path $VlmWinDir 'model.safetensors'
    $size = if (Test-Path $weight) { (Get-Item $weight).Length } else { 0 }
    if ($size -lt 1MB) {
        $hasLfs = $false
        try { git lfs version | Out-Null; $hasLfs = $true } catch { }
        if ($hasLfs) {
            Info '拉取 Git LFS 权重（约 1.79 GiB，需要一些时间）…'
            Push-Location $VlmWinDir
            try { git lfs install --local | Out-Null; git lfs pull } finally { Pop-Location }
            $size = if (Test-Path $weight) { (Get-Item $weight).Length } else { 0 }
        } else {
            Warn 'git-lfs 未安装。请安装 Git for Windows（自带 git-lfs）或 https://git-lfs.com/'
            Warn "然后在模型目录执行：cd '$VlmWinDir'; git lfs install --local; git lfs pull"
        }
    }

    if ($size -ge 1MB) {
        Info "模型就绪：$VlmWinDir ($([math]::Round($size / 1MB)) MiB · 官方 BF16)"
        $env:MARKITDOWN_VLM_MODEL = $VlmWinDir
    } else {
        Warn "权重仍未就绪（$size 字节）；VLM 选项暂不可用，但其余 OCR 不受影响。"
    }

    Warn '注意：Windows 没有 MLX，BF16 权重不能直接用 macOS 的 mlx_vlm.server 跑。'
    Warn '官方建议 WSL2 + Docker 或 PaddlePaddle CPU 部署推理服务；起来后设置'
    Warn '  $env:MARKITDOWN_VLM_URL=http://127.0.0.1:8111'
    Warn '指向你自己的 OpenAI 兼容服务，WebUI 的 VLM 选项即可启用。'
}

# ---- get the binary --------------------------------------------------------
New-Item -ItemType Directory -Force $InstallDir | Out-Null
$bin = Join-Path $InstallDir 'markitdown-rs.exe'
$tmp = Join-Path $env:TEMP ("mdinstall-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

$download = "https://github.com/$Repo/releases/latest/download/$Asset"
try {
    Info "下载预编译二进制：$Asset"
    Invoke-WebRequest -Uri $download -OutFile (Join-Path $tmp $Asset) -UseBasicParsing
    Expand-Archive -Path (Join-Path $tmp $Asset) -DestinationPath $tmp -Force
    Copy-Item (Join-Path $tmp 'markitdown-rs.exe') $bin -Force
} catch {
    Info '没有找到预编译包，改为从源码构建（首次约 1-2 分钟）…'
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Info '安装 Rust（rustup）…'
        Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile (Join-Path $tmp 'rustup-init.exe') -UseBasicParsing
        & (Join-Path $tmp 'rustup-init.exe') -y --profile minimal
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    }
    git clone --depth 1 "https://github.com/$Repo.git" (Join-Path $tmp 'src')
    Push-Location (Join-Path $tmp 'src'); cargo build --release; Pop-Location
    Copy-Item (Join-Path $tmp 'src\target\release\markitdown-rs.exe') $bin -Force
}
if (-not (Test-Path $bin)) { Die '二进制安装失败' }

# ---- launcher --------------------------------------------------------------
$launcher = Join-Path $InstallDir 'start.cmd'
@"
@echo off
REM Generated by install.ps1 — chosen OCR engine: $desc
set MARKITDOWN_OCR_ENGINE=$engine
set TESSDATA_PREFIX=$env:TESSDATA_PREFIX
$(if ($env:MARKITDOWN_VLM_MODEL) { "set MARKITDOWN_VLM_MODEL=$($env:MARKITDOWN_VLM_MODEL)" })
$(if ($env:MARKITDOWN_VLM_URL)   { "set MARKITDOWN_VLM_URL=$($env:MARKITDOWN_VLM_URL)" })
start "" "$bin" --port $Port
start "" "$Url"
"@ | Set-Content -Path $launcher -Encoding ASCII

# ---- start -----------------------------------------------------------------
$running = $false
try { $r = Invoke-WebRequest -Uri "$Url/health" -UseBasicParsing -TimeoutSec 2; $running = $r.StatusCode -eq 200 } catch {}
if ($running) {
    Info "服务已在运行：${Url}（本次只更新了二进制与启动器）"
} else {
    Info '启动服务并打开浏览器…'
    $env:MARKITDOWN_OCR_ENGINE = $engine
    Start-Process -FilePath $bin -ArgumentList '--port', $Port -WindowStyle Hidden
    Start-Sleep -Seconds 1
    Start-Process $Url
}

Write-Host ''
Info "完成：$Url"
Write-Host "  OCR 引擎   $desc"
Write-Host "  二进制     $bin"
Write-Host "  启动器     $launcher"
Write-Host '  停止       Stop-Process -Name markitdown-rs'
Write-Host '  换引擎     重跑安装器，或在网页里临时切换'
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
