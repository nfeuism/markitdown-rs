#!/usr/bin/env bash
# markitdown-rs — one-line installer for macOS
#
#   curl -fsSL https://raw.githubusercontent.com/nfeuism/markitdown-rs/main/install.sh | bash
#
# Non-interactive (CI / scripted):
#   MARKITDOWN_OCR=vision curl -fsSL .../install.sh | bash
#   MARKITDOWN_OCR=tesseract MARKITDOWN_PORT=5055 curl -fsSL .../install.sh | bash
#
# The installer asks which OCR engine to use, installs only what that choice
# needs, downloads a prebuilt binary (or builds from source as a fallback),
# then starts the local WebUI at http://127.0.0.1:5001.
#
# Everything stays on 127.0.0.1. No file is ever uploaded anywhere.

set -euo pipefail

REPO="nfeuism/markitdown-rs"
RAW="https://raw.githubusercontent.com/$REPO/main"
INSTALL_DIR="${MARKITDOWN_INSTALL_DIR:-$HOME/.local/bin}"
PORT="${MARKITDOWN_PORT:-5001}"
URL="http://127.0.0.1:$PORT"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m==> 错误:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "这个安装器只用于 macOS。Windows 请用 install.ps1。"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64)  ASSET="markitdown-rs-macos-arm64.tar.gz" ;;
  x86_64) ASSET="markitdown-rs-macos-x64.tar.gz" ;;
  *)      die "不支持的 CPU 架构：$ARCH" ;;
esac

# `curl | bash` consumes stdin, so read the answer from the terminal instead.
read_tty() {
  local prompt="$1" answer=""
  if [[ -r /dev/tty ]]; then
    printf '%s' "$prompt" > /dev/tty
    IFS= read -r answer < /dev/tty || true
  fi
  printf '%s' "$answer"
}

info "markitdown-rs 安装程序（macOS ${ARCH}）"
printf '\n' >&2
printf '请选择 OCR 引擎（中文识别）：\n' >&2
printf '  1) Apple Vision   系统内置、零下载、中文精准、走 ANE 加速  [推荐]\n' >&2
printf '  2) Tesseract      brew 安装简体/繁体中文包，跨平台兜底\n' >&2
printf '  3) 两者都要       Vision 优先 + Tesseract 兜底（最稳）\n' >&2
printf '  4) PaddleOCR-VL   0.9B VLM 高精度（可选，自动下载 4-bit MLX 模型约 0.7GB）\n' >&2
printf '\n' >&2

CHOICE="${MARKITDOWN_OCR:-}"
if [[ -z "$CHOICE" ]]; then
  CHOICE="$(read_tty '输入 1 / 2 / 3 / 4（默认 1）：')"
fi

# macOS ships bash 3.2, which has no ${VAR,,}; use tr for lowercasing.
CHOICE_LOWER="$(printf '%s' "$CHOICE" | tr '[:upper:]' '[:lower:]')"
case "$CHOICE_LOWER" in
  1|vision|apple|"") ENGINE="vision";  DESC="Apple Vision" ;;
  2|tesseract)        ENGINE="tesseract"; DESC="Tesseract" ;;
  3|both|auto)        ENGINE="auto";   DESC="Apple Vision + Tesseract" ;;
  4|paddleocr-vl|vlm|paddle) ENGINE="paddleocr-vl"; DESC="PaddleOCR-VL-1.6（本地 VLM）" ;;
  *) die "无法识别的选项：${CHOICE}（可选 1/2/3/4）" ;;
esac
info "已选择：$DESC → 默认引擎 $ENGINE"

# ---- optional local VLM model mount ----------------------------------------
# The model repos use Git LFS: a plain `git clone` yields 134-byte pointers,
# so we always verify the real weights and pull them when needed.
VLM_MAC_REPO="${MARKITDOWN_VLM_REPO:-https://www.modelscope.cn/mlx-community/PaddleOCR-VL-1.6-4bit.git}"

mount_vlm_model() {
  local dir="$1" repo="$2" sz
  if [[ ! -f "$dir/config.json" ]]; then
    info "未检测到本地模型，克隆：${repo}"
    command -v git >/dev/null 2>&1 || die "需要 git 才能克隆模型仓库"
    rm -rf "$dir"
    GIT_LFS_SKIP_SMUDGE=1 git clone --depth 1 "$repo" "$dir" || die "模型克隆失败：$repo"
  else
    info "复用已存在的模型目录：${dir}"
  fi

  sz="$(stat -f '%z' "$dir/model.safetensors" 2>/dev/null || echo 0)"
  if (( sz < 1048576 )); then
    if ! git lfs version >/dev/null 2>&1; then
      warn "git-lfs 未安装，权重目前只是 LFS 指针。安装后在这个目录执行："
      warn "  brew install git-lfs && cd '${dir}' && git lfs install --local && git lfs pull"
      return 1
    fi
    info "拉取 Git LFS 权重（约 0.7GB）…"
    (cd "$dir" && git lfs install --local >/dev/null && git lfs pull) \
      || warn "git lfs pull 失败，请稍后手动重试"
    sz="$(stat -f '%z' "$dir/model.safetensors" 2>/dev/null || echo 0)"
  fi

  if (( sz < 1048576 )); then
    warn "权重仍未就绪（${sz} 字节）。请执行：cd '${dir}' && git lfs pull"
    return 1
  fi
  info "模型就绪：${dir} ($(( sz / 1048576 )) MiB · 4-bit MLX / Metal GPU)"
  export MARKITDOWN_VLM_MODEL="$dir"
  return 0
}

if [[ "$ENGINE" == "paddleocr-vl" ]]; then
  VLM_DIR="${MARKITDOWN_VLM_MODEL:-$HOME/PaddleOCR-VL-1.6-4bit}"
  mount_vlm_model "$VLM_DIR" "$VLM_MAC_REPO" || true
  warn "启动高精度服务：./scripts/start-paddleocr-vl.sh（需 uv + Python 3.12）"
fi

# ---- dependencies for the chosen engine ------------------------------------
# Only Tesseract-based choices need Tesseract. The VLM choice uses its own
# MLX service and does not require Tesseract at all.
if [[ "$ENGINE" == "tesseract" || "$ENGINE" == "auto" ]]; then
  if command -v tesseract >/dev/null 2>&1; then
    info "Tesseract 已安装"
  else
    if command -v brew >/dev/null 2>&1; then
      info "安装 Tesseract 及中文语言包（brew）…"
      brew install tesseract tesseract-lang
    else
      warn "未检测到 Homebrew，无法自动安装 Tesseract。"
      warn "请先安装 https://brew.sh 后重跑，或改用选项 1（Apple Vision，零依赖）。"
      warn "本次将以 Apple Vision 继续，Tesseract 兜底不可用。"
      [[ "$ENGINE" == "tesseract" ]] && ENGINE="auto"
    fi
  fi
  if command -v tesseract >/dev/null 2>&1 && ! tesseract --list-langs 2>/dev/null | grep -q '^chi_sim$'; then
    warn "Tesseract 缺少 chi_sim 中文包；请执行：brew install tesseract-lang"
  fi
fi

# ---- get the binary --------------------------------------------------------
mkdir -p "$INSTALL_DIR"
BIN="$INSTALL_DIR/markitdown-rs"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

DOWNLOAD="https://github.com/$REPO/releases/latest/download/$ASSET"
if curl -fsSL --max-time 120 "$DOWNLOAD" -o "$TMP/$ASSET" 2>/dev/null; then
  info "下载预编译二进制：$ASSET"
  tar xzf "$TMP/$ASSET" -C "$TMP"
  install -m 755 "$TMP/markitdown-rs" "$BIN"
else
  info "没有找到预编译包，改为从源码构建（首次约 1-2 分钟）…"
  if ! command -v cargo >/dev/null 2>&1; then
    info "安装 Rust（rustup）…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi
  git clone --depth 1 "https://github.com/$REPO.git" "$TMP/src"
  (cd "$TMP/src" && cargo build --release)
  install -m 755 "$TMP/src/target/release/markitdown-rs" "$BIN"
fi
[[ -x "$BIN" ]] || die "二进制安装失败"

# ---- launcher with the chosen OCR engine ----------------------------------
LAUNCHER="$INSTALL_DIR/markitdown-rs-start"
VLM_LINE=""
if [[ -n "${MARKITDOWN_VLM_MODEL:-}" ]]; then
  VLM_LINE="export MARKITDOWN_VLM_MODEL=\"${MARKITDOWN_VLM_MODEL}\""
fi
cat > "$LAUNCHER" <<EOF
#!/usr/bin/env bash
# Generated by install.sh — chosen OCR engine: $DESC
export MARKITDOWN_OCR_ENGINE="$ENGINE"
$VLM_LINE
exec "$BIN" --port "$PORT"
EOF
chmod 755 "$LAUNCHER"

# ---- start -----------------------------------------------------------------
if curl -fsS --max-time 2 "$URL/health" >/dev/null 2>&1; then
  info "服务已在运行：${URL}（本次只更新了二进制与启动器）"
else
  info "启动服务并打开浏览器…"
  nohup "$LAUNCHER" > "$HOME/.markitdown-rs.log" 2>&1 &
  for _ in $(seq 1 50); do
    curl -fsS --max-time 1 "$URL/health" >/dev/null 2>&1 && break
    sleep 0.2
  done
  open "$URL" 2>/dev/null || true
fi

printf '\n' >&2
info "完成：$URL"
printf '  OCR 引擎   %s\n 二进制     %s\n 启动器     %s\n 日志       %s\n' \
  "$DESC" "$BIN" "$LAUNCHER" "$HOME/.markitdown-rs.log" >&2
printf '  停止       pkill -f markitdown-rs\n' >&2
printf '  换引擎     重跑安装器，或在网页里临时切换\n' >&2
