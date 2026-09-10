#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/src-tauri/resources/obscura"
OBSCURA_TARGET="$TARGET_DIR/obscura"
WORKER_TARGET="$TARGET_DIR/obscura-worker"

mkdir -p "$TARGET_DIR"

sign_macos_targets() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    return
  fi

  codesign --force --sign - --timestamp=none "$OBSCURA_TARGET"
  if [[ -f "$WORKER_TARGET" ]]; then
    codesign --force --sign - --timestamp=none "$WORKER_TARGET"
  fi
}

verify_obscura() {
  local version
  version="$("$OBSCURA_TARGET" --version)"
  echo "Verified Obscura: $version"
}

install_from_dir() {
  local source_dir="$1"
  if [[ ! -x "$source_dir/obscura" ]]; then
    echo "Missing executable: $source_dir/obscura" >&2
    return 1
  fi
  cp "$source_dir/obscura" "$OBSCURA_TARGET"
  chmod 0755 "$OBSCURA_TARGET"
  if [[ -f "$source_dir/obscura-worker" ]]; then
    cp "$source_dir/obscura-worker" "$WORKER_TARGET"
    chmod 0755 "$WORKER_TARGET"
  fi
  sign_macos_targets
  verify_obscura
  echo "Installed Obscura:"
  echo "  $OBSCURA_TARGET"
  if [[ -f "$WORKER_TARGET" ]]; then
    echo "  $WORKER_TARGET"
  fi
}

if [[ "${1:-}" != "" ]]; then
  install_from_dir "$1"
  exit 0
fi

exec node "$ROOT_DIR/scripts/prepare_runtime.mjs" --component obscura
