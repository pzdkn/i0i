# RFC 0083: `??` is a reference, and a link should not eat the app

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0056 (HTML reader with annotation and chat), RFC 0065 (add web
page to vault by URL).

## Summary

Two faults in the HTML reader, found in the same paragraph of the same paper:

- **Every cross-reference renders as `??`.** In the cached copy of
  `transformer-circuits.pub/2026/workspace`, 321 anchors read `??`. They are not
  broken references in the source — they are placeholders the site's JavaScript
  fills in at runtime, and we (correctly) do not run the site's JavaScript.
- **Clicking a link can take the whole app with it.** Nothing intercepts clicks
  inside the rendered article, so an external `<a href="https://…">` navigates
  the Tauri webview away from the app. The app is now a web page; closing that
  page closes the app.

Both are one surface: what happens when the reader meets an `<a>`.

**Out of scope:** running the source page's scripts (never), rendering
JS-generated figure widgets (most of this paper's 102 `<figure>` elements have
no `<img>` — there is nothing static to show), opening links in an in-app tab.

---

## 1. Every cross-reference is `??`

### Diagnosis

Measured against the raw page and the cached ingest, not inferred.

The source markup is:

```html
<a class="fig-ref" data-ref="structure" href="#fig-structure">??</a>
```

with targets like:

```html
<figure data-fignum="1" id="fig-workspace-functional-properties">
  <figcaption><span class="fig-num">Figure 1: </span>…</figcaption>
</figure>
```

The site ships `??` in the HTML and rewrites it in the browser. `ingest_html`
(`src-tauri/src/html_ingestion.rs`) extracts with `dom_smoothie` and sanitizes
with `ammonia` — no scripts, by design — so the placeholder survives verbatim.

The caption numbering, though, is **static**: `Figure 2: ` is in the source
markup, not injected. Every in-document reference is therefore resolvable
without running anything.

Two further findings from the cached file:

- **`id` count is zero.** ammonia's default allowlist drops `id`, so all 407
  fragment links in the cache point at targets that no longer exist. Even a
  correctly labelled reference would jump nowhere.
- **44 of the 128 distinct references have no target in the source at all**
  (`#fig-structure`, `#app-ignition`, `#ws-modulation` — sections and appendix
  pages the site resolves across documents). These are *not* resolvable
  statically, and pretending otherwise would produce a link that lies.

### Change

R1.1 `html_ingestion` builds a label map from the **raw document** before
extraction: for every element with an `id`, its label is the `fig-num` text of
its `<figcaption>` when it has one (`Figure 2`), else the text of its heading,
else nothing. The raw document is used because Readability may drop the figure
the reference points at.

R1.2 Every `<a href="#ID">` whose text is `??` or empty is rewritten:

| Case | Result |
|---|---|
| `ID` in the label map | anchor text becomes the label (`Figure 2`), `href` kept |
| `ID` unknown | the anchor is replaced by `<span class="i0i-ref-unresolved">ref</span>` — visible as a reference, not clickable, not a lie |

R1.3 `sanitize` allows `id`, so fragment targets survive into the rendered
article and R2.1's in-document jump has something to find. `id` carries no
script and no network fetch; the reader resolves it scoped to the article root,
so a collision with an app element id cannot misdirect a jump.

R1.4 Anchor text that is neither `??` nor empty is left alone. A source that
already says "Figure 2" needs nothing from us.

---

## 2. A link click can navigate the app away

### Diagnosis

`HtmlReader.svelte:302` renders `{@html html}` inside `<article>` with
`onmouseup` and `oncontextmenu` handlers and no click handler. The sanitized
article contains 8 absolute `https://` links in this paper alone. Clicking one
navigates the webview: the SPA is unloaded, and the window's close button now
closes the app rather than the page. There is no back affordance, because the
app that would have drawn one is gone.

The correct destination already exists in this very component — `viewOriginal()`
calls `openUrl` from `@tauri-apps/plugin-opener` to hand a URL to the system
browser.

### Change

R2.1 A single delegated click handler on the article root, on the nearest
enclosing `<a>`:

| Href | Behaviour |
|---|---|
| `#fragment` | `preventDefault`; scroll the target into view inside the article and flash it (reusing the citation-flash idiom of RFC 0077); if no target, do nothing |
| `http(s)://…` | `preventDefault`; `openUrl(href)` — the system browser, never the app window |
| anything else | `preventDefault`; ignore |

R2.2 The default is **prevent**. A link the reader does not understand does
nothing, rather than navigating the webview to it. This is the rule that makes
the class of bug impossible rather than fixed one href at a time.

R2.3 Middle-click and modifier-click take the same path as a plain click.
"Open in new tab" has no meaning in a window with no tabs.

---

## 3. Jumping to a figure loses your place

### Diagnosis

Even once R1 and R2 land, following a reference scrolls a long article to a
figure and leaves you there. The paper the report came from is ~3.4MB of HTML;
scrolling back by hand is not a way back.

### Change

R3.1 Following an in-document reference records the scroll position it left, and
the reader shows a **Return** chip pinned to the bottom of the reading surface:
`← Back to where you were`. Clicking it restores the position and dismisses the
chip; a subsequent jump replaces the recorded position rather than stacking.

R3.2 The chip is dismissible and disappears on its own once you scroll back into
the neighbourhood of the origin (within a viewport height).

---

---

## 4. Pages already in the cache keep the old rendering

### Diagnosis

Reference resolution happens at **ingest**. `cache_discovery_html`
(`reader_service.rs:255`) serves a cached page straight off disk whenever
`source.html` and its `meta.json` exist, so every page read before this RFC
keeps its `??`s and its stripped ids forever — including the paper the report
came from.

Re-ingesting on a version bump is the obvious fix and it is the wrong one.
HTML annotations are anchored by **character offset into the rendered article
text** (RFC 0056), and turning 321 `??`s into `Figure 31` moves every offset
after the first reference. A silent re-ingest would quietly relocate every
highlight in the document.

### Change

R4.1 `open_html_document` takes a `force` flag that skips the cache read, and
the HTML reader's toolbar gets a **Re-extract** button next to *View original*.
The reader decides when a document is re-rendered, because the reader is the one
who knows whether its highlights matter more than its cross-references.

R4.2 Nothing re-ingests on its own. A page opened for the first time after this
RFC gets resolved references because it was never cached; everything else stays
exactly as it was rendered.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.4 label map + reference rewriting, `id` preserved (Rust, fixture-tested) | yes | M |
| 2 | R2.1–R2.3 delegated link handling | yes | S |
| 3 | R3.1–R3.2 return chip | yes | S |
| 4 | R4.1–R4.2 forced re-extract | yes | S |

## Risks

- **Label heuristics are per-source-format.** The `figcaption` / heading rule
  covers LaTeXML, Distill-style and ordinary article HTML. A format that numbers
  figures only in script falls to R1.2's unresolved case, which is the honest
  outcome, not a regression.
- **Allowing `id` widens the sanitizer allowlist.** `id` is inert: no script, no
  fetch. The alternative — namespacing every id and rewriting every href — buys
  nothing, since jumps resolve scoped to the article root.
- **`ref` reads as a dead end.** It is one. The reference points at a section of
  a document we do not have; saying so is better than a link that scrolls to the
  top of the page.

## Verification

Rust side is fixture-tested in `html_ingestion.rs`, next to the existing
sanitizer tests:

1. `<a href="#f1">??</a>` with `<figure id="f1"><figcaption><span>Figure 2:
   </span>…` → anchor text becomes `Figure 2`, href intact.
2. The same anchor with no `#f1` in the document → no anchor in the output, a
   marked span instead.
3. `id` survives sanitization; `<script>` still does not.
4. An anchor already reading `Figure 2` is untouched.

Reader side is manual:

5. Open the workspace paper and press **Re-extract**: references read `Figure
   N`, not `??`. (Measured on the fetched page before shipping: 321 `??`
   anchors → 0, with 157 resolved to figure numbers and 164 marked unresolved —
   those 164 point at sections of other documents and have no target to resolve
   against, in the source or anywhere else.)
6. Click one → the article scrolls to that figure and flashes it; a Return chip
   appears; clicking it comes back.
7. Click an external link → it opens in the system browser and **the app is
   still there**.
8. Unresolved references are visibly inert and do not move the page.

## Success criteria

1. No `??` in a rendered article whose source can be resolved statically, and no
   clickable reference that resolves to nothing.
2. No click inside a rendered article can navigate the app window. Closing what
   a link opened never closes i0i.
