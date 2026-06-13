#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON_VERSION="${I0I_SPIKE_PYTHON_VERSION:-3.12}"

install_docling() {
  uv venv "$ROOT/.venvs/docling" --python "$PYTHON_VERSION"
  uv pip install --python "$ROOT/.venvs/docling/bin/python" docling
}

install_marker() {
  uv venv "$ROOT/.venvs/marker" --python "$PYTHON_VERSION"
  uv pip install --python "$ROOT/.venvs/marker/bin/python" marker-pdf
}

install_mineru() {
  uv venv "$ROOT/.venvs/mineru" --python "$PYTHON_VERSION"
  uv pip install --python "$ROOT/.venvs/mineru/bin/python" "mineru[all]"
}

usage() {
  cat <<EOF
Usage: $0 [docling] [marker] [mineru] [all]

Installs extractor spike tools into isolated uv venvs under:
  $ROOT/.venvs/

Environment:
  I0I_SPIKE_PYTHON_VERSION=$PYTHON_VERSION
EOF
}

if [[ $# -eq 0 ]]; then
  usage
  exit 0
fi

for tool in "$@"; do
  case "$tool" in
    docling)
      install_docling
      ;;
    marker)
      install_marker
      ;;
    mineru)
      install_mineru
      ;;
    all)
      install_docling
      install_marker
      install_mineru
      ;;
    -h|--help|help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown tool: $tool" >&2
      usage >&2
      exit 2
      ;;
  esac
done
