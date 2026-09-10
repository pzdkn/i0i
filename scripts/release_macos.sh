#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT_DIR/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/i0i.app"
OUTPUT_DIR="$ROOT_DIR/package"
APP_VERSION="$(cd "$ROOT_DIR" && node -p "require('./package.json').version")"
DMG="$OUTPUT_DIR/i0i-$APP_VERSION-aarch64.dmg"

: "${I0I_SIGNING_IDENTITY:?Set I0I_SIGNING_IDENTITY to a Developer ID Application identity}"
: "${I0I_NOTARY_PROFILE:?Set I0I_NOTARY_PROFILE to a notarytool keychain profile}"

cd "$ROOT_DIR"
pnpm install --frozen-lockfile
pnpm runtime:prepare
node --test scripts/prepare_runtime.test.mjs
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
pnpm tauri build --target aarch64-apple-darwin --bundles app --ci --no-sign

node scripts/verify_macos_bundle.mjs "$APP"

resource_path() {
  local relative="$1"
  local direct="$APP/Contents/Resources/$relative"
  local prefixed="$APP/Contents/Resources/resources/$relative"
  if [[ -e "$direct" ]]; then
    printf '%s\n' "$direct"
  elif [[ -e "$prefixed" ]]; then
    printf '%s\n' "$prefixed"
  else
    printf 'Missing packaged resource: %s\n' "$relative" >&2
    return 1
  fi
}

OBSCURA_WORKER="$(resource_path obscura/obscura-worker)"
OBSCURA="$(resource_path obscura/obscura)"
PDFIUM="$(resource_path pdfium/libpdfium.dylib)"

# Sign inner code first, then seal the outer bundle with hardened runtime.
/usr/bin/codesign --force --options runtime --timestamp --sign "$I0I_SIGNING_IDENTITY" \
  "$OBSCURA_WORKER"
/usr/bin/codesign --force --options runtime --timestamp --sign "$I0I_SIGNING_IDENTITY" \
  "$OBSCURA"
/usr/bin/codesign --force --options runtime --timestamp --sign "$I0I_SIGNING_IDENTITY" \
  "$PDFIUM"
/usr/bin/codesign --force --options runtime --timestamp --sign "$I0I_SIGNING_IDENTITY" "$APP"

node scripts/verify_macos_bundle.mjs "$APP" --signatures
mkdir -p "$OUTPUT_DIR"
rm -f "$DMG"
/usr/bin/hdiutil create -volname i0i -srcfolder "$APP" -ov -format UDZO "$DMG"
/usr/bin/codesign --force --timestamp --sign "$I0I_SIGNING_IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile "$I0I_NOTARY_PROFILE" --wait
xcrun stapler staple "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
shasum -a 256 "$DMG"
