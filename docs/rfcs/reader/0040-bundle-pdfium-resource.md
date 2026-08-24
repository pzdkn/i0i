# RFC 0040: Bundle Pdfium As Reader Extraction Resource

Status: Stale
Date: 2026-07-07
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0035 (background text extraction)

## Summary

Install Pdfium as an app resource at:

```text
src-tauri/resources/pdfium/libpdfium.dylib
```

and configure Tauri to bundle that file. The binary itself is not committed.
Developers install it locally with `scripts/setup_pdfium.sh`.

## Context

The PDF reader uses frontend pdf.js and can render cached PDFs without Pdfium.
The Rust background extractor uses `pdfium-render`, which dynamically loads the
native `libpdfium.dylib` at runtime to extract plain page text.

The current fallback paths point at extractor-spike Python virtual environments.
That is useful for a quick local experiment, but it is not an app dependency.

## Decision

- Keep `I0I_PDFIUM_LIBRARY_PATH` as the strongest explicit override.
- Add a product-shaped local resource path:
  `src-tauri/resources/pdfium/libpdfium.dylib`.
- Configure Tauri to bundle that resource for app builds.
- Add `scripts/setup_pdfium.sh` to copy a local Pdfium dylib into the resource
  path.
- The setup script must reject incompatible Pdfium builds. For the current
  `pdfium-render` binding set, the dylib must export
  `FPDF_StructElement_GetExpansion`.
- Ignore the dylib in git; keep only `.gitkeep` and the setup script.

## Compatibility Note

`pdfium-render` dynamically loads Pdfium symbols at runtime. Finding
`libpdfium.dylib` is not enough; the library must also contain the C API
functions expected by the selected Cargo feature.

The binding is process-global. After the first successful `Pdfium::new(...)`,
later extraction jobs must reuse the existing bindings instead of trying to
initialize the native library again.

The app currently uses:

```toml
pdfium-render = { features = ["pdfium_latest", "thread_safe"] }
```

In `pdfium-render` 0.9.2, `pdfium_latest` maps to Pdfium build `7763`. A dylib
older than that can open successfully but still fail during symbol binding, for
example with:

```text
dlsym(..., FPDF_StructElement_GetExpansion): symbol not found
```

For local development, the setup script prefers a compatible spike-installed
Pdfium build when available.

## Non-Goals

- No bundled Pdfium binary in git.
- No cross-platform Pdfium resolver yet.
- No MinerU/structured extraction integration.
- No PDF download fallback changes.

## Validation Plan

1. Run:

   ```bash
   bash scripts/setup_pdfium.sh
   ```

   or pass an explicit dylib path.

2. Confirm:

   ```text
   src-tauri/resources/pdfium/libpdfium.dylib
   ```

3. Start the app and open a cached PDF.

4. Check backend logs for:

   ```text
   bound pdfium path=...
   ```

5. Run:

   ```bash
   cargo check
   cargo test
   pnpm check
   ```
