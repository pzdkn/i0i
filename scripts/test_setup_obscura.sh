#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SETUP_SCRIPT="$ROOT_DIR/scripts/setup_obscura.sh"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

FIXTURE_ROOT="$TEST_DIR/repo"
SOURCE_DIR="$TEST_DIR/source"
FAKE_BIN="$TEST_DIR/bin"
TARGET_DIR="$FIXTURE_ROOT/src-tauri/resources/obscura"
CODESIGN_LOG="$TEST_DIR/codesign.log"

mkdir -p "$FIXTURE_ROOT/scripts" "$SOURCE_DIR" "$FAKE_BIN"
cp "$SETUP_SCRIPT" "$FIXTURE_ROOT/scripts/setup_obscura.sh"

cat > "$SOURCE_DIR/obscura" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
  if [[ "${FAKE_OBSCURA_FAIL:-0}" == "1" ]]; then
    exit 7
  fi
  echo "obscura 9.9.9"
fi
EOF
cat > "$SOURCE_DIR/obscura-worker" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$FAKE_BIN/uname" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  -s) echo "${FAKE_OS:?}" ;;
  -m) echo "${FAKE_ARCH:?}" ;;
  *) echo "${FAKE_OS:?}" ;;
esac
EOF
cat > "$FAKE_BIN/codesign" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${CODESIGN_LOG:?}"
EOF
cat > "$FAKE_BIN/curl" <<'EOF'
#!/usr/bin/env bash
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    -o)
      output="$2"
      shift 2
      ;;
    http*)
      url="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done
printf '%s\n' "${url:?}" >> "${CURL_LOG:?}"
cp "${FAKE_ARCHIVE:?}" "${output:?}"
EOF
chmod 0755 \
  "$SOURCE_DIR/obscura" \
  "$SOURCE_DIR/obscura-worker" \
  "$FAKE_BIN/uname" \
  "$FAKE_BIN/codesign" \
  "$FAKE_BIN/curl"
: > "$CODESIGN_LOG"

FAKE_OS=Darwin FAKE_ARCH=arm64 CODESIGN_LOG="$CODESIGN_LOG" PATH="$FAKE_BIN:$PATH" \
  bash "$FIXTURE_ROOT/scripts/setup_obscura.sh" "$SOURCE_DIR" \
  > "$TEST_DIR/darwin-output.log"

test "$(wc -l < "$CODESIGN_LOG" | tr -d ' ')" = "2"
grep -Fqx -- "--force --sign - --timestamp=none $TARGET_DIR/obscura" "$CODESIGN_LOG"
grep -Fqx -- "--force --sign - --timestamp=none $TARGET_DIR/obscura-worker" "$CODESIGN_LOG"
grep -Fq -- "Verified Obscura: obscura 9.9.9" "$TEST_DIR/darwin-output.log"

: > "$CODESIGN_LOG"
FAKE_OS=Linux FAKE_ARCH=x86_64 CODESIGN_LOG="$CODESIGN_LOG" PATH="$FAKE_BIN:$PATH" \
  bash "$FIXTURE_ROOT/scripts/setup_obscura.sh" "$SOURCE_DIR" \
  > "$TEST_DIR/linux-output.log"

test ! -s "$CODESIGN_LOG"
grep -Fq -- "Verified Obscura: obscura 9.9.9" "$TEST_DIR/linux-output.log"

if FAKE_OS=Linux FAKE_ARCH=x86_64 FAKE_OBSCURA_FAIL=1 CODESIGN_LOG="$CODESIGN_LOG" \
  PATH="$FAKE_BIN:$PATH" \
  bash "$FIXTURE_ROOT/scripts/setup_obscura.sh" "$SOURCE_DIR"; then
  echo "Expected setup to fail when obscura --version fails" >&2
  exit 1
fi

NODE_LOG="$TEST_DIR/node.log"
cat > "$FAKE_BIN/node" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" > "${NODE_LOG:?}"
EOF
chmod 0755 "$FAKE_BIN/node"
NODE_LOG="$NODE_LOG" PATH="$FAKE_BIN:$PATH" \
  bash "$FIXTURE_ROOT/scripts/setup_obscura.sh"
grep -Fqx -- "$FIXTURE_ROOT/scripts/prepare_runtime.mjs --component obscura" "$NODE_LOG"

echo "setup_obscura checks passed"
