#!/usr/bin/env bash
# Start a *local* PaddleOCR-VL-1.6 (0.9B VLM) inference server on Apple Silicon.
#
# The model runs on the M-series GPU via MLX / Metal. Nothing is uploaded;
# the Rust binary only talks to 127.0.0.1:8111 over an OpenAI-compatible API.
#
# Usage:
#   ./scripts/start-paddleocr-vl.sh            # foreground (Ctrl-C to stop)
#   ./scripts/start-paddleocr-vl.sh --check    # only probe, no install
#
# Model path resolution (highest priority first):
#   1. MARKITDOWN_VLM_MODEL env var (absolute path or HF repo id)
#   2. ~/PaddleOCR-VL-1.6-4bit  (the locally mounted directory on this machine)
#   3. mlx-community/PaddleOCR-VL-1.6-4bit  (downloaded on demand, ~0.68 GB)

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
URL="${MARKITDOWN_VLM_URL:-http://127.0.0.1:8111}"
PORT="${MARKITDOWN_VLM_PORT:-8111}"
VENV="$ROOT/.venv-vlm"

if [[ -n "${MARKITDOWN_VLM_MODEL:-}" ]]; then
  MODEL="$MARKITDOWN_VLM_MODEL"
elif [[ -f "$HOME/PaddleOCR-VL-1.6-4bit/config.json" ]]; then
  MODEL="$HOME/PaddleOCR-VL-1.6-4bit"
else
  MODEL="mlx-community/PaddleOCR-VL-1.6-4bit"
fi

if [[ "${1:-}" == "--check" ]]; then
  echo "模型     : $MODEL"
  if [[ "$MODEL" = /* ]]; then
    sz=$(stat -f '%z' "$MODEL/model.safetensors" 2>/dev/null || echo 0)
    echo "权重     : ${sz} bytes"
    if (( sz < 1048576 )); then
      echo "  ⚠ 仍是 Git LFS 指针，请执行: brew install git-lfs && cd '$MODEL' && git lfs install --local && git lfs pull"
    fi
  fi
  echo -n "服务     : "; curl -fsS --max-time 3 "$URL/health" >/dev/null 2>&1 && echo "可用" || echo "未启动"
  exit 0
fi

if curl -fsS --max-time 2 "$URL/health" >/dev/null 2>&1; then
  echo "PaddleOCR-VL 服务已在运行：$URL"
  exit 0
fi

# ---- verify locally mounted weights ---------------------------------------
if [[ "$MODEL" = /* ]]; then
  if [[ ! -f "$MODEL/config.json" ]]; then
    echo "错误：找不到本地模型目录 $MODEL" >&2
    exit 1
  fi
  sz=$(stat -f '%z' "$MODEL/model.safetensors" 2>/dev/null || echo 0)
  if (( sz < 1048576 )); then
    echo "错误：$MODEL/model.safetensors 只有 ${sz} 字节，是 Git LFS 指针，权重没下载。" >&2
    echo "修复：brew install git-lfs && cd '$MODEL' && git lfs install --local && git lfs pull" >&2
    exit 1
  fi
  echo "本地权重：$MODEL ($(( sz / 1048576 )) MiB)"
fi

# ---- isolated Python 3.12 environment -------------------------------------
if ! command -v uv >/dev/null 2>&1; then
  echo "需要 uv（https://docs.astral.sh/uv/）。安装：curl -LsSf https://astral.sh/uv/install.sh | sh" >&2
  exit 1
fi
if [[ ! -x "$VENV/bin/python" ]]; then
  echo "创建隔离 Python 3.12 环境：$VENV"
  uv venv --python 3.12 "$VENV"
fi
if ! "$VENV/bin/python" -c "import mlx_vlm" >/dev/null 2>&1; then
  echo "安装 mlx-vlm（MLX + Metal GPU，首次可能几分钟）…"
  uv pip install --python "$VENV/bin/python" "mlx-vlm>=0.6.10"
fi

echo
echo "模型 : $MODEL"
echo "服务 : $URL （仅本机）"
"$VENV/bin/python" -c "import mlx.core as mx; print('设备 :', mx.default_device(), '| Metal 可用:', mx.metal.is_available())" 2>/dev/null || echo "设备 : 无法检测 MLX 设备"
echo "停止 : Ctrl-C"
echo

export MARKITDOWN_VLM_MODEL="$MODEL"
exec "$VENV/bin/python" -m mlx_vlm.server \
  --model "$MODEL" \
  --host 127.0.0.1 \
  --port "$PORT"
