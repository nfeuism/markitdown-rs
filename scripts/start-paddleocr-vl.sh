#!/usr/bin/env bash
# Optional high-accuracy OCR backend for markitdown-rs.
#
# Starts a *local* mlx_vlm.server with PaddleOCR-VL-1.6 (4-bit MLX) on
# 127.0.0.1:8111. The Rust binary talks to it over an OpenAI-compatible API.
# Nothing is uploaded anywhere; the model is downloaded once from HuggingFace
# (~0.68 GB) and cached under ~/.cache/huggingface.
#
# Usage:
#   ./scripts/start-paddleocr-vl.sh          # foreground (Ctrl-C to stop)
#   ./scripts/start-paddleocr-vl.sh --check # just probe, no install
#
# Then pick "PaddleOCR-VL-1.6" in the WebUI's OCR engine dropdown.
# It is deliberately NOT part of the "auto" chain: you opt in explicitly.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"

URL="${MARKITDOWN_VLM_URL:-http://127.0.0.1:8111}"
PORT="${MARKITDOWN_VLM_PORT:-8111}"
MODEL="${MARKITDOWN_VLM_MODEL:-mlx-community/PaddleOCR-VL-1.6-4bit}"
VENV="$ROOT/.venv-vlm"

if [[ "${1:-}" == "--check" ]]; then
  echo "探测 $URL/health ..."
  curl -fsS --max-time 3 "$URL/health" && echo "  → 服务可用" || echo "  → 服务未启动"
  exit 0
fi

if curl -fsS --max-time 2 "$URL/health" >/dev/null 2>&1; then
  echo "PaddleOCR-VL 服务已经在运行：$URL"
  echo "如需重启，先停止现有进程再执行本脚本。"
  exit 0
fi

if ! command -v uv >/dev/null 2>&1; then
  echo "需要 uv（https://docs.astral.sh/uv/）。安装：curl -LsSf https://astral.sh/uv/install.sh | sh" >&2
  exit 1
fi

if [[ ! -x "$VENV/bin/python" ]]; then
  echo "创建隔离 Python 3.12 环境：$VENV"
  uv venv --python 3.12 "$VENV"
fi

if ! "$VENV/bin/python" -c "import mlx_vlm" >/dev/null 2>&1; then
  echo "安装 mlx-vlm（首次可能需要几分钟）..."
  uv pip install --python "$VENV/bin/python" "mlx-vlm>=0.6.10"
fi

echo
echo "模型：$MODEL"
echo "首次启动会从 HuggingFace 下载约 0.68 GB 权重并缓存，请耐心等待。"
echo "服务地址：$URL （仅本机）"
echo "停止：Ctrl-C   卸载：$VENV/bin/python -m mlx_vlm.server --help"
echo

exec "$VENV/bin/python" -m mlx_vlm.server \
  --model "$MODEL" \
  --host 127.0.0.1 \
  --port "$PORT"
