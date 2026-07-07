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

for candidate in \
  "$ROOT_DIR/scripts/extractor_spike/.venvs/docling/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib" \
  "$ROOT_DIR/scripts/extractor_spike/.venvs/mineru/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib" \
  "$ROOT_DIR/scripts/extractor_spike/.venvs/marker/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib"
do
  if copy_from "$candidate"; then
    echo "Note: copied from extractor-spike tooling as a developer convenience."
    echo "For a clean install, pass an explicit Pdfium dylib path to this script."
    exit 0
  fi
done

cat >&2 <<EOF
Could not find libpdfium.dylib.

Usage:
  bash scripts/setup_pdfium.sh /absolute/path/to/libpdfium.dylib

or:
  I0I_PDFIUM_SOURCE=/absolute/path/to/libpdfium.dylib bash scripts/setup_pdfium.sh

Expected output:
  src-tauri/resources/pdfium/libpdfium.dylib
EOF
exit 1
