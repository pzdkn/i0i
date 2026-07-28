# RFC 0060: Iconography — adopt `@lucide/svelte`

Status: Proposed
Date: 2026-07-28
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: PDF Reader UX north star (`docs/design/pdf-reader-ux.md`) — RFC 0 of
the roadmap.

## Summary

Standardize on a single, professional icon set — **`@lucide/svelte`** — and
remove the two ad-hoc approaches in use today: **hand-inlined SVG paths** (the
settings cog in `ActivityRail`) and **emoji glyphs** used as UI icons (`✎` Note,
`💬` Ask, `✨` AI, `↗` external link). This is the cheapest, most isolated step
in the reader-UX roadmap and unblocks every UI RFC after it: 0061 (Note/Ask),
0062 (Annotations panel), 0063 (toolbar), 0064 (AI auto-highlight) all introduce
new controls that should ship with real icons from day one.

Plain version: replace emojis and copy-pasted `<path>`s with a maintained icon
library, so the UI looks intentional and consistent and new icons are one import
away.

## Problem

Two inconsistent patterns, neither good:

1. **Hand-inlined SVG.** `ActivityRail.svelte` embeds a raw `<svg viewBox="0 0
   24 24"><path …></svg>` for the settings cog. Every new icon means finding and
   pasting path data; there is no shared sizing/stroke/color convention.
2. **Emoji as icons.** Reader components use emoji for real controls:
   - `PdfRenderedPage.svelte`: `✎ Note`, `💬 Ask`.
   - `ReaderInspector.svelte`: `✎` (rename), `✨` (AI badge), `💬` (has-thread
     badge).
   - `HtmlReader.svelte`: `↗` (view original).

   Emoji render differently per-platform/font, don't inherit `currentColor` (so
   they ignore the theme and don't dim/hover with their text), can't have their
   stroke weight tuned, and read as unpolished. The north-star mockups lean on
   icons heavily (toolbar, badges, popover), so this only grows.

## Proposal

### Dependency

Add **`@lucide/svelte`** — the official Lucide package with Svelte 5 support.
Lucide is the maintained successor to Feather: ~1500 icons, consistent 24×24
grid, outline style, MIT. Tree-shakeable: only imported icons are bundled.

> Note on naming: the legacy package is `lucide-svelte`; `@lucide/svelte` is the
> Svelte-5-compatible successor. Pin an exact version in `package.json`.

### Usage convention

Import the named icon and render the component; icons inherit `currentColor` and
take `size` / `strokeWidth` props:

```svelte
<script lang="ts">
  import { Settings } from "@lucide/svelte";
</script>

<button class="rail-btn" aria-label="Settings">
  <Settings size={18} strokeWidth={1.75} aria-hidden="true" />
</button>
```

**Shared defaults (the house look):**
- **Size:** `16` for inline/badge icons, `18` for toolbar/rail buttons. Use these
  two sizes; don't scatter arbitrary values.
- **Stroke:** `strokeWidth={1.75}` everywhere (matches the current cog's weight).
- **Color:** never set a color prop — icons inherit `currentColor`, so they theme
  and hover with their surrounding text automatically.

To keep defaults DRY without a heavy abstraction, add a tiny wrapper
`src/lib/components/Icon.svelte` that forwards a Lucide component with the default
size/stroke, used where a component wants the house defaults:

```svelte
<!-- Icon.svelte -->
<script lang="ts">
  import type { Component } from "svelte";
  let { icon, size = 16, strokeWidth = 1.75, ...rest }:
    { icon: Component; size?: number; strokeWidth?: number } = $props();
  const Glyph = $derived(icon);
</script>
<Glyph {size} {strokeWidth} aria-hidden="true" {...rest} />
```

Direct import is also fine; the wrapper is a convenience, not a mandate.

### Accessibility

- **Decorative** icons (next to a text label): `aria-hidden="true"`.
- **Icon-only** buttons: keep the existing `aria-label` on the button (ActivityRail
  settings and the rename button already do this) and mark the glyph
  `aria-hidden="true"`.
- Badges that were emoji-with-`title` (AI, has-thread) become an icon with the
  same `title`/`aria-label` so hover text is preserved.

### Icon mapping (this RFC's replacements + the north-star vocabulary)

| Today | Lucide | Location |
|---|---|---|
| inline cog SVG | `Settings` | `ActivityRail` settings button |
| `✎` Note / rename | `Pencil` (rename), `StickyNote` (note) | `PdfRenderedPage`, `ReaderInspector` |
| `💬` Ask / has-thread | `MessageSquare` | `PdfRenderedPage`, `ReaderInspector` |
| `✨` AI badge | `Sparkles` | `ReaderInspector` badge |
| `↗` view original | `ExternalLink` | `HtmlReader` |

Reserved for later RFCs (no code yet, listed so the vocabulary is fixed):
`Search`, `Highlighter`, `ZoomIn`/`ZoomOut`, `ChevronLeft`/`ChevronRight`,
`Trash2`, `Star`. Color swatches stay CSS circles, not icons.

## Implementation

1. `pnpm add @lucide/svelte` (pinned).
2. Add `src/lib/components/Icon.svelte` (optional wrapper above).
3. Replace `ActivityRail`'s inline `<svg>` with `<Settings>`.
4. Replace the reader emojis per the mapping table (`PdfRenderedPage`,
   `ReaderInspector`, `HtmlReader`), preserving each button's existing
   `aria-label`/`title`.
5. Grep to confirm no UI emoji glyphs remain in `src/lib` (log/console strings
   are out of scope).

## Testing

- `pnpm check` clean (types; Lucide components are typed).
- `pnpm build` succeeds (tree-shaking; no external asset fetches — icons are
  inlined SVG components, CSP-safe for the Tauri webview).
- Visual smoke: the settings cog, reader Note/Ask buttons, AI badge, and "view
  original" render as Lucide icons, inherit theme color, and dim/hover with their
  text in both light and dark.

## Risks

- **Package name/version drift** (`@lucide/svelte` vs legacy `lucide-svelte`).
  Mitigation: pin the version; verify Svelte 5 compatibility on install.
- **Bundle size.** Negligible with tree-shaking (only imported icons ship); do
  not `import * as icons`.
- **Over-iconifying.** Keep text labels where they aid clarity (e.g. "Ask" reads
  better as `MessageSquare` + "Ask" than an icon alone in the popover).

## Rollout (slices)

1. Add the dependency + `Icon.svelte`; convert `ActivityRail` (proves the
   pattern, one file).
2. Convert the reader emojis (`PdfRenderedPage`, `ReaderInspector`, `HtmlReader`).
3. Grep-verify no UI emoji remain; update the north-star doc's note that icons
   are now real (not placeholders).

## Non-goals

- No new icons/controls — those come with 0061–0064; this RFC only swaps the
  *existing* glyphs and establishes the convention.
- No redesign of any component's layout or behavior.
- No custom/branded icons; Lucide's set is sufficient for v1.
