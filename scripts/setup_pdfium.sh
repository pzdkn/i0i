#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/src-tauri/resources/pdfium"
TARGET="$TARGET_DIR/libpdfium.dylib"

mkdir -p "$TARGET_DIR"

copy_from() {
  local source="$1"
  if [[ -f "$source" ]]; then
    if ! nm -gU "$source" | grep -q "_FPDF_StructElement_GetExpansion"; then
      echo "Skipping incompatible Pdfium library: $source" >&2
      echo "Missing required symbol: FPDF_StructElement_GetExpansion" >&2
      return 1
    fi

    cp "$source" "$TARGET"
    chmod 0644 "$TARGET"
    echo "Installed Pdfium:"
    echo "  $TARGET"
    echo "Source:"
    echo "  $source"
    return 0
  fi
  return 1
}

if [[ "${1:-}" != "" ]]; then
  copy_from "$1" || {
    echo "Pdfium source not found: $1" >&2
    exit 1
  }
  exit 0
fi

if [[ "${I0I_PDFIUM_SOURCE:-}" != "" ]]; then
  copy_from "$I0I_PDFIUM_SOURCE" || {
    echo "I0I_PDFIUM_SOURCE does not point to a file: $I0I_PDFIUM_SOURCE" >&2
    exit 1
  }
  exit 0
fi

exec node "$ROOT_DIR/scripts/prepare_runtime.mjs" --component pdfium
