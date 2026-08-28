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

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) archive="obscura-aarch64-macos-stealth.tar.gz" ;;
  Darwin-x86_64) archive="obscura-x86_64-macos-stealth.tar.gz" ;;
  Linux-aarch64) archive="obscura-aarch64-linux-stealth.tar.gz" ;;
  Linux-x86_64) archive="obscura-x86_64-linux-stealth.tar.gz" ;;
  *)
    echo "Unsupported platform: $(uname -s)-$(uname -m)" >&2
    echo "Pass an extracted Obscura directory explicitly:" >&2
    echo "  bash scripts/setup_obscura.sh /path/to/extracted/obscura" >&2
    exit 1
    ;;
esac

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

url="https://github.com/h4ckf0r0day/obscura/releases/latest/download/$archive"
echo "Downloading $url"
curl -L --silent "$url" -o "$tmp_dir/$archive"
tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"
install_from_dir "$tmp_dir"
