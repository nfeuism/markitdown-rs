#!/usr/bin/env bash
# markitdown-rs — one-line installer for macOS
#
#   curl -fsSL https://raw.githubusercontent.com/nfeuism/markitdown-rs/main/install.sh | bash
#
# Non-interactive (CI / scripted):
#   MARKITDOWN_OCR=vision curl -fsSL .../install.sh | bash
#   MARKITDOWN_OCR="1 2 4" curl -fsSL .../install.sh | bash   # 多选，空格或逗号分隔
#   MARKITDOWN_OCR=tesseract MARKITDOWN_PORT=5055 curl -fsSL .../install.sh | bash
#
# The installer asks which OCR engines to install. Any combination is allowed
# (e.g. "1 2 4"): it installs only what those choices need, downloads a
# prebuilt binary (or builds from source as a fallback), then starts the local
# WebUI at http://127.0.0.1:5001.
#
# Everything stays on 127.0.0.1. No file is ever uploaded anywhere.

set -euo pipefail

REPO="nfeuism/markitdown-rs"
RAW="https://raw.githubusercontent.com/$REPO/main"
INSTALL_DIR="${MARKITDOWN_INSTALL_DIR:-$HOME/.local/bin}"
PORT="${MARKITDOWN_PORT:-5001}"
URL="http://127.0.0.1:$PORT"
VLM_PORT="${MARKITDOWN_VLM_PORT:-8111}"
VLM_URL="http://127.0.0.1:$VLM_PORT"
VLM_HOME="$HOME/.local/share/markitdown-rs"
VLM_START="$VLM_HOME/scripts/start-paddleocr-vl.sh"
VLM_LOG="$HOME/.markitdown-vlm.log"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m==> 错误:\033[0m %s\n' "$*" >&2; exit 1; }

# ---- parse the OCR engine choice (one OR many) ----------------------------
# macOS ships bash 3.2, which has no ${VAR,,}; use tr for lowercasing.
parse_choices() {
  local input tok
  WANT_VISION=0
  WANT_TESSERACT=0
  WANT_VLM=0
  input="$(printf '%s' "$1" | tr ',;' '  ')"
  for tok in $input; do   # shellcheck disable=SC2086
    case "$(printf '%s' "$tok" | tr '[:upper:]' '[:lower:]')" in
      1|vision|apple)            WANT_VISION=1 ;;
      2|tesseract)               WANT_TESSERACT=1 ;;
      3|both|auto)               WANT_VISION=1; WANT_TESSERACT=1 ;;
      4|paddleocr-vl|vlm|paddle) WANT_VLM=1 ;;
      *) die "无法识别的选项：${tok}（可选 1/2/3/4，可多选）" ;;
    esac
  done
  # Empty answer = the recommended default (option 1).
  if (( ! WANT_VISION && ! WANT_TESSERACT && ! WANT_VLM )); then WANT_VISION=1; fi
}

# Default engine = the strongest selected backend. The dropdown still lists
# every engine that is actually available, so "installed" != "default".
pick_engine() {
  if   (( WANT_VLM ));                  then ENGINE="paddleocr-vl"
  elif (( WANT_VISION && WANT_TESSERACT )); then ENGINE="auto"
  elif (( WANT_TESSERACT ));            then ENGINE="tesseract"
  else                                       ENGINE="vision"
  fi
  DESC=""
  if (( WANT_VISION ));    then DESC="Apple Vision"; fi
  if (( WANT_TESSERACT )); then DESC="${DESC:+$DESC + }Tesseract"; fi
  if (( WANT_VLM ));       then DESC="${DESC:+$DESC + }PaddleOCR-VL-1.6（本地 VLM）"; fi
}

# Self-check for the parser: MARKITDOWN_INSTALL_SELFTEST=1 bash install.sh
if [[ "${MARKITDOWN_INSTALL_SELFTEST:-}" == "1" ]]; then
  while IFS='|' read -r input expect; do
    parse_choices "$input"
    pick_engine
    [[ "$ENGINE" == "$expect" ]] || {
      printf "FAIL '%s' -> %s (want %s)\n" "$input" "$ENGINE" "$expect"; exit 1; }
    printf "OK   %-20s -> %s\n" "'$input'" "$ENGINE"
  done <<'CASES'
1|vision
2|tesseract
3|auto
4|paddleocr-vl
1 2|auto
1,2,4|paddleocr-vl
vision paddleocr-vl|paddleocr-vl
|vision
CASES
  echo "installer choices: OK"
  exit 0
fi

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
printf '请选择要安装的 OCR 引擎（中文识别，可多选）：\n' >&2
printf '  1) Apple Vision   系统内置、零下载、中文精准、走 ANE 加速  [推荐]\n' >&2
printf '  2) Tesseract      brew 安装简体/繁体中文包，跨平台兜底\n' >&2
printf '  3) 1+2 组合       Vision 优先 + Tesseract 兜底（最稳）\n' >&2
printf '  4) PaddleOCR-VL   0.9B VLM 高精度（自动下载 4-bit MLX 模型约 0.7GB 并启动服务）\n' >&2
printf '\n' >&2
printf '  可组合，例如输入  1 2 4  或  1,2,4（默认 1）\n' >&2

CHOICE="${MARKITDOWN_OCR:-}"
if [[ -z "$CHOICE" ]]; then
  CHOICE="$(read_tty '输入编号（可多选，默认 1）：')"
fi
parse_choices "$CHOICE"
pick_engine
info "已选择：$DESC → 默认引擎 $ENGINE"

# ---- optional local VLM model mount ----------------------------------------
# The model repos use Git LFS: a plain `git clone` yields 134-byte pointers,
# so we install git-lfs, pull *every* LFS file and verify the real weights.
VLM_MAC_REPO="${MARKITDOWN_VLM_REPO:-https://www.modelscope.cn/mlx-community/PaddleOCR-VL-1.6-4bit.git}"

mount_vlm_model() {
  local dir="$1" repo="$2" sz f missing=0
  if [[ ! -f "$dir/config.json" ]]; then
    info "未检测到本地模型，克隆：${repo}"
    command -v git >/dev/null 2>&1 || { warn "需要 git 才能克隆模型仓库"; return 1; }
    rm -rf "$dir"
    GIT_LFS_SKIP_SMUDGE=1 git clone --depth 1 "$repo" "$dir" || {
      warn "模型克隆失败：$repo"; return 1; }
  else
    info "复用已存在的模型目录：${dir}"
  fi

  # Without git-lfs the clone is just pointers — install it instead of
  # leaving a model directory that looks installed but cannot be loaded.
  if ! git lfs version >/dev/null 2>&1; then
    if command -v brew >/dev/null 2>&1; then
      info "安装 git-lfs（模型权重走 Git LFS）…"
      brew install git-lfs || { warn "git-lfs 安装失败"; return 1; }
    else
      warn "需要 git-lfs 才能下载模型权重：https://git-lfs.com/"
      return 1
    fi
  fi

  sz="$(stat -f '%z' "$dir/model.safetensors" 2>/dev/null || echo 0)"
  if (( sz < 1048576 )); then
    info "拉取 Git LFS 权重（约 0.7GB）…"
    (cd "$dir" && git lfs install --local >/dev/null && git lfs pull) || {
      warn "git lfs pull 失败；请在 ${dir} 手动重试"; return 1; }
  fi

  # tokenizer.json is LFS-tracked too: a leftover pointer breaks model loading
  # even when the main weight succeeded.
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    if head -c 42 "$dir/$f" 2>/dev/null | grep -q 'git-lfs.github.com/spec/v1'; then
      warn "仍是 LFS 指针：$f"
      missing=1
    fi
  done < <(cd "$dir" && git lfs ls-files -n 2>/dev/null)
  if (( missing )); then
    warn "模型权重不完整；请在 ${dir} 执行 git lfs pull"
    return 1
  fi

  sz="$(stat -f '%z' "$dir/model.safetensors" 2>/dev/null || echo 0)"
  if (( sz < 1048576 )); then
    warn "权重仍未就绪（${sz} 字节）；请在 ${dir} 执行 git lfs pull"
    return 1
  fi
  info "模型就绪：${dir} ($(( sz / 1048576 )) MiB · 4-bit MLX / Metal GPU)"
  export MARKITDOWN_VLM_MODEL="$dir"
  return 0
}

# The Rust binary is only a client: paddleocr_vl counts as enabled when the
# separate mlx_vlm.server answers /health. Recover the script the piped
# installer never downloaded, then start it in the background.
start_vlm_service() {
  local dir="$1" i
  mkdir -p "$(dirname "$VLM_START")"
  if ! curl -fsSL --max-time 30 "$RAW/scripts/start-paddleocr-vl.sh" -o "$VLM_START"; then
    warn "启动脚本下载失败：$RAW/scripts/start-paddleocr-vl.sh"
    return 1
  fi
  chmod 755 "$VLM_START"
  if ! command -v uv >/dev/null 2>&1; then
    info "安装 uv（Python 3.12 运行时）…"
    if ! curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null 2>&1; then
      warn "uv 安装失败；请装好 https://docs.astral.sh/uv/ 后运行 $VLM_START"
      return 1
    fi
    export PATH="$HOME/.local/bin:$PATH"
  fi
  if curl -fsS --max-time 2 "$VLM_URL/health" >/dev/null 2>&1; then
    info "PaddleOCR-VL 服务已在运行：$VLM_URL"
    return 0
  fi
  info "后台启动 PaddleOCR-VL 服务（首次需装 mlx-vlm，约几分钟）…"
  nohup env MARKITDOWN_VLM_MODEL="$dir" MARKITDOWN_VLM_PORT="$VLM_PORT" \
    "$VLM_START" < /dev/null > "$VLM_LOG" 2>&1 &
  for i in $(seq 1 30); do
    if curl -fsS --max-time 1 "$VLM_URL/health" >/dev/null 2>&1; then
      info "PaddleOCR-VL 服务已就绪：$VLM_URL"
      return 0
    fi
    sleep 2
  done
  # Detection is per-request, so the WebUI picks it up as soon as it is ready;
  # the launcher below also re-starts it after a reboot.
  warn "服务仍在后台初始化（日志 $VLM_LOG），就绪后 WebUI 会自动显示可用。"
  return 1
}

# China mirror for the first mlx-vlm install, e.g.
#   MARKITDOWN_PYPI_INDEX=https://pypi.tuna.tsinghua.edu.cn/simple
PYPI_LINE=""
if [[ -n "${MARKITDOWN_PYPI_INDEX:-}" ]]; then
  export UV_DEFAULT_INDEX="$MARKITDOWN_PYPI_INDEX"
  PYPI_LINE="export UV_DEFAULT_INDEX=\"${MARKITDOWN_PYPI_INDEX}\""
fi

VLM_DIR="${MARKITDOWN_VLM_MODEL:-$HOME/PaddleOCR-VL-1.6-4bit}"
if (( WANT_VLM )); then
  if ! mount_vlm_model "$VLM_DIR" "$VLM_MAC_REPO"; then
    warn "PaddleOCR-VL 未就绪，已从本次安装中移除；其余引擎不受影响。"
    WANT_VLM=0
    pick_engine
  else
    # Soft failure = still installing in the background; keep the launcher
    # hook so the service comes back after a reboot.
    start_vlm_service "$VLM_DIR" || true
  fi
fi

# ---- dependencies for the chosen engines -----------------------------------
# Only Tesseract-based choices need Tesseract. The VLM choice uses its own
# MLX service and does not require Tesseract at all.
if (( WANT_TESSERACT )); then
  if command -v tesseract >/dev/null 2>&1; then
    info "Tesseract 已安装"
  elif command -v brew >/dev/null 2>&1; then
    info "安装 Tesseract 及中文语言包（brew）…"
    brew install tesseract tesseract-lang
  else
    warn "未检测到 Homebrew，无法自动安装 Tesseract；已跳过该引擎。"
    WANT_TESSERACT=0
    pick_engine
  fi
  if (( WANT_TESSERACT )) && command -v tesseract >/dev/null 2>&1 \
      && ! tesseract --list-langs 2>/dev/null | grep -q '^chi_sim$'; then
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

# ---- launcher --------------------------------------------------------------
# The launcher also re-starts the VLM service, which is a separate process
# and would otherwise disappear after a reboot.
LAUNCHER="$INSTALL_DIR/markitdown-rs-start"
{
  cat <<EOF
#!/usr/bin/env bash
# Generated by install.sh — OCR engines: $DESC
export MARKITDOWN_OCR_ENGINE="$ENGINE"
EOF
  if (( WANT_VLM )); then
    cat <<EOF
export MARKITDOWN_VLM_MODEL="$VLM_DIR"
export MARKITDOWN_VLM_PORT="$VLM_PORT"
$PYPI_LINE

# The VLM is a separate process; bring it up if it isn't already healthy.
if [[ -x "$VLM_START" ]] && ! curl -fsS --max-time 2 "$VLM_URL/health" >/dev/null 2>&1; then
  nohup env MARKITDOWN_VLM_MODEL="$VLM_DIR" MARKITDOWN_VLM_PORT="$VLM_PORT" \\
    "$VLM_START" < /dev/null >> "$VLM_LOG" 2>&1 &
fi
EOF
  fi
  cat <<EOF

exec "$BIN" --port "$PORT"
EOF
} > "$LAUNCHER"
chmod 755 "$LAUNCHER"

# ---- start -----------------------------------------------------------------
if curl -fsS --max-time 2 "$URL/health" >/dev/null 2>&1; then
  info "服务已在运行：${URL}（本次只更新了二进制与启动器）"
else
  info "启动服务并打开浏览器…"
  nohup "$LAUNCHER" > "$HOME/.markitdown-rs.log" 2>&1 < /dev/null &
  for _ in $(seq 1 50); do
    curl -fsS --max-time 1 "$URL/health" >/dev/null 2>&1 && break
    sleep 0.2
  done
  open "$URL" 2>/dev/null || true
fi

printf '\n' >&2
info "完成：$URL"
printf '  已装引擎   %s\n  默认引擎   %s\n 二进制     %s\n 启动器     %s\n 日志       %s\n' \
  "$DESC" "$ENGINE" "$BIN" "$LAUNCHER" "$HOME/.markitdown-rs.log" >&2
if (( WANT_VLM )); then
  printf '  VLM 服务   %s（日志 %s）\n' "$VLM_URL" "$VLM_LOG" >&2
fi
printf '  停止       pkill -f markitdown-rs\n' >&2
printf '  换引擎     重跑安装器（可多选），或在网页里临时切换\n' >&2
