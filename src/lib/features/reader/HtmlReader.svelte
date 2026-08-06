<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ExternalLink } from "@lucide/svelte";
  import { getReaderHtml } from "$lib/bridge/library";
  import type { ReaderTextSelection } from "$lib/domain/reader";
  import type { Highlight, Locator } from "$lib/domain/highlight";
  import { HIGHLIGHT_COLORS } from "$lib/domain/highlight";
  import { findHighlightForOffset } from "$lib/features/reader/highlight-thread-match";
  import { resolveQuoteInText } from "$lib/features/reader/resolve-quote-html";
  import { findTextMatches, type TextMatch } from "$lib/features/reader/find-matches";

  let {
    sourceId,
    sourceUrl,
    highlights = [],
    conversationIds,
    chatEnabled = true,
    onSelectPassage,
    onHighlightClick,
  }: {
    sourceId: string;
    sourceUrl?: string;
    highlights?: Highlight[];
    // Highlight ids that have a conversation (RFC 0067): they draw the neutral
    // marker even without a color or note.
    conversationIds?: Set<string>;
    chatEnabled?: boolean;
    onSelectPassage: (selection: ReaderTextSelection) => void;
    onHighlightClick?: (highlightId: string, x: number, y: number) => void;
  } = $props();

  let root: HTMLElement | undefined = $state();
  let html = $state("");
  let loadError = $state("");

  // Load the sanitized article HTML for this source (RFC 0056).
  $effect(() => {
    const id = sourceId;
    html = "";
    loadError = "";
    getReaderHtml(id)
      .then((value) => (html = value))
      .catch((error) => (loadError = String(error)));
  });

  // Char offset of (node, nodeOffset) within `container`, by walking text nodes
  // in document order — the browser-side coordinate space the TextOffset anchor
  // uses (RFC 0056). Backend never slices by these offsets, so this is the sole
  // source of truth.
  function offsetIn(container: Node, node: Node, nodeOffset: number): number {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let count = 0;
    let current: Node | null;
    while ((current = walker.nextNode())) {
      if (current === node) return count + nodeOffset;
      count += current.textContent?.length ?? 0;
    }
    return count + nodeOffset;
  }

  // Inverse: locate the (text node, offset) for a char offset within `container`.
  function locate(container: Node, target: number): { node: Node; offset: number } | null {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let count = 0;
    let current: Node | null;
    while ((current = walker.nextNode())) {
      const length = current.textContent?.length ?? 0;
      if (count + length >= target) return { node: current, offset: target - count };
      count += length;
    }
    return null;
  }

  // Concatenates text nodes under `container` in document order — the same
  // walk `offsetIn`/`locate` use — so a char span found here lands at the
  // exact same offsets the highlight-rendering effect below uses.
  function extractFullText(container: Node): string {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let text = "";
    let current: Node | null;
    while ((current = walker.nextNode())) {
      text += current.textContent ?? "";
    }
    return text;
  }

  // Resolves an agent-provided verbatim quote to a textOffset Locator in this
  // article's plain text (RFC 0059 Phase 2 / Task 8). Exposed to ReaderView
  // via `bind:this`.
  export function resolveQuote(quote: string): Locator | null {
    if (!root) return null;
    const span = resolveQuoteInText(extractFullText(root), quote);
    if (!span) return null;
    return { kind: "textOffset", sourceId, startOffset: span.start, endOffset: span.end };
  }

  // In-document search (RFC 0063). Reuses the same text-node walk and Custom
  // Highlight API as annotations, under distinct highlight names so search marks
  // and annotation marks never collide. Exposed to ReaderView via `bind:this`.
  let searchMatches: TextMatch[] = [];

  function highlightApi() {
    const api = (CSS as unknown as { highlights?: Map<string, unknown> }).highlights;
    const Ctor = (globalThis as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
    if (!api || typeof Ctor !== "function") return null;
    return { api, Ctor };
  }

  function rangeForMatch(match: TextMatch): Range | null {
    if (!root) return null;
    const start = locate(root, match.start);
    const end = locate(root, match.end);
    if (!start || !end) return null;
    const range = document.createRange();
    range.setStart(start.node, start.offset);
    range.setEnd(end.node, end.offset);
    return range;
  }

  export function search(query: string): number {
    const hl = highlightApi();
    if (!root || !hl) return 0;
    searchMatches = findTextMatches(extractFullText(root), query);
    const ranges = searchMatches
      .map(rangeForMatch)
      .filter((range): range is Range => range !== null);
    hl.api.set("i0i-search", new hl.Ctor(...ranges));
    hl.api.delete("i0i-search-active");
    return searchMatches.length;
  }

  export function focusMatch(index: number): void {
    const hl = highlightApi();
    if (!hl || index < 0 || index >= searchMatches.length) return;
    const range = rangeForMatch(searchMatches[index]);
    if (!range) return;
    hl.api.set("i0i-search-active", new hl.Ctor(range));
    // Scroll the match *itself* (not its paragraph) into the middle of the
    // scrolling reader column, using the range's own geometry — works even
    // before the highlight has painted (so the first search scrolls too).
    const scroller = root?.closest(".html-reader") as HTMLElement | null;
    const rect = range.getBoundingClientRect();
    if (scroller && rect.height) {
      const scRect = scroller.getBoundingClientRect();
      const delta = rect.top - scRect.top - scroller.clientHeight / 2 + rect.height / 2;
      scroller.scrollBy({ top: delta, behavior: "smooth" });
    }
  }

  // RFC 0073 (R2.2): scroll a passage into view by its text offsets, so opening
  // a mark from the Marks list moves the reader. Same one-scroller `scrollBy`
  // geometry as `focusMatch`, and for the same reason: `scrollIntoView` would
  // also scroll the inspector panel the list sits in.
  export function focusOffsets(startOffset: number, endOffset: number): void {
    const range = rangeForMatch({ start: startOffset, end: endOffset });
    const scroller = root?.closest(".html-reader") as HTMLElement | null;
    if (!range || !scroller) return;
    const rect = range.getBoundingClientRect();
    if (!rect.height) return;
    const scRect = scroller.getBoundingClientRect();
    const delta = rect.top - scRect.top - scroller.clientHeight / 2 + rect.height / 2;
    scroller.scrollBy({ top: delta, behavior: "smooth" });
  }

  export function clearSearch(): void {
    const hl = highlightApi();
    searchMatches = [];
    if (hl) {
      hl.api.delete("i0i-search");
      hl.api.delete("i0i-search-active");
    }
  }

  function handleMouseUp(event: MouseEvent) {
    if (!root) return;
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0);
    if (!root.contains(range.commonAncestorContainer)) return;

    // A plain click (no drag) inside a highlight opens the click-a-highlight
    // popover for that mark (RFC 0058 Task 10; generalizes the old
    // click-opens-its-thread behavior to any mark, threaded or not).
    if (selection.isCollapsed) {
      if (onHighlightClick && selection.anchorNode) {
        const offset = offsetIn(root, selection.anchorNode, selection.anchorOffset);
        const hit = findHighlightForOffset(highlights, sourceId, offset);
        if (hit) onHighlightClick(hit.id, event.clientX, event.clientY);
      }
      return;
    }

    if (!chatEnabled) return;
    const startOffset = offsetIn(root, range.startContainer, range.startOffset);
    const endOffset = offsetIn(root, range.endContainer, range.endOffset);
    const selectedText = selection.toString().trim();
    if (!selectedText || endOffset <= startOffset) return;

    onSelectPassage({
      sourceId,
      startOffset,
      endOffset,
      selectedText,
      anchorKind: "text_offset",
    });
  }

  // Repaint one CSS Custom Highlight per color from the highlights list.
  $effect(() => {
    html;
    const current = highlights;
    const chats = conversationIds;
    if (!root || !html) return;
    const api = (CSS as unknown as { highlights?: Map<string, unknown> }).highlights;
    const Ctor = (globalThis as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
    if (!api || typeof Ctor !== "function") return;

    const byGroup = new Map<string, Range[]>();
    for (const hl of current) {
      if (hl.locator.kind !== "textOffset" || hl.locator.sourceId !== sourceId) continue;
      // A color mark → its color group; a note-only OR conversation-only passage
      // → the neutral "note" group (RFC 0061 + RFC 0067: ask leaves a marker).
      const group = hl.color ?? (hl.note || chats?.has(hl.id) ? "note" : null);
      if (!group) continue;
      const start = locate(root, hl.locator.startOffset);
      const end = locate(root, hl.locator.endOffset);
      if (!start || !end) continue;
      const range = document.createRange();
      range.setStart(start.node, start.offset);
      range.setEnd(end.node, end.offset);
      (byGroup.get(group) ?? byGroup.set(group, []).get(group)!).push(range);
    }
    const groups = [...HIGHLIGHT_COLORS, "note"];
    const names = groups.map((g) => `i0i-hl-${g}`);
    for (const g of groups) api.set(`i0i-hl-${g}`, new Ctor(...(byGroup.get(g) ?? [])));
    return () => names.forEach((n) => api.delete(n));
  });

  async function viewOriginal() {
    if (sourceUrl) {
      try {
        await openUrl(sourceUrl);
      } catch {
        // Best-effort; the URL is also shown below.
      }
    }
  }
</script>

<div class="html-reader col">
  <div class="toolbar row">
    <span class="kind">HTML</span>
    {#if sourceUrl}
      <button class="view-original" type="button" onclick={viewOriginal}>
        View original <ExternalLink size={12} strokeWidth={1.75} aria-hidden="true" />
      </button>
    {/if}
  </div>

  {#if loadError}
    <div class="notice">Could not load article: {loadError}</div>
  {:else if !html}
    <div class="notice">Loading article…</div>
  {:else}
    <!-- Safe: the backend sanitized this (ammonia allowlist, no scripts/externals). -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <article class="html-content" bind:this={root} onmouseup={handleMouseUp}>
      {@html html}
    </article>
  {/if}
</div>

<style>
  .html-reader {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    background: var(--bg-1, #111);
  }

  .toolbar {
    position: sticky;
    top: 0;
    z-index: 1;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 14px;
    background: var(--bg-1, #111);
    border-bottom: 1px solid var(--hair, #2a2a2a);
  }

  .kind {
    font-size: 10px;
    letter-spacing: 0.08em;
    color: var(--fg-3, #888);
  }

  .view-original {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--fg-3, #444);
    background: transparent;
    color: var(--fg-2, #bbb);
    font: inherit;
    font-size: 11px;
    padding: 4px 10px;
    cursor: pointer;
  }

  .view-original:hover {
    border-color: var(--amber, #f2a93b);
    color: var(--amber, #f2a93b);
  }

  .notice {
    padding: 24px;
    color: var(--fg-3, #888);
    font-size: 13px;
  }

  /* Reading column. Restyled article content; the app owns typography. */
  .html-content {
    max-width: 46rem;
    margin: 0 auto;
    padding: 28px 32px 96px;
    color: var(--fg-1, #ddd);
    font-size: 16px;
    line-height: 1.65;
  }

  .html-content :global(h1),
  .html-content :global(h2),
  .html-content :global(h3) {
    line-height: 1.25;
    margin: 1.6em 0 0.5em;
    color: var(--fg-0, #f0f0f0);
  }

  .html-content :global(p) {
    margin: 0 0 1em;
  }

  .html-content :global(a) {
    color: var(--amber, #f2a93b);
  }

  .html-content :global(img) {
    max-width: 100%;
    height: auto;
  }

  .html-content :global(figure) {
    margin: 1.5em 0;
  }

  .html-content :global(table) {
    border-collapse: collapse;
    display: block;
    overflow-x: auto;
    max-width: 100%;
  }

  .html-content :global(td),
  .html-content :global(th) {
    border: 1px solid var(--hair, #333);
    padding: 4px 8px;
  }

  .html-content :global(pre) {
    overflow-x: auto;
    background: var(--bg-0, #0b0b0b);
    padding: 12px;
  }

  .html-content :global(math) {
    font-size: 1.05em;
  }

  /* Annotation highlights (CSS Custom Highlight API — no DOM mutation), one
     rule per highlight color. */
  :global(::highlight(i0i-hl-yellow)) { background: rgba(242, 201, 76, 0.32); }
  :global(::highlight(i0i-hl-green))  { background: rgba(111, 207, 151, 0.32); }
  :global(::highlight(i0i-hl-blue))   { background: rgba(86, 156, 214, 0.32); }
  :global(::highlight(i0i-hl-red))    { background: rgba(235, 87, 87, 0.32); }
  :global(::highlight(i0i-hl-purple)) { background: rgba(187, 107, 217, 0.32); }
  :global(::highlight(i0i-hl-orange)) { background: rgba(242, 153, 74, 0.32); }
  /* Note-only passage: a subtle neutral marker, not a filled color (RFC 0061). */
  :global(::highlight(i0i-hl-note)) {
    background: rgba(148, 148, 148, 0.12);
    text-decoration: underline dotted rgba(148, 148, 148, 0.7);
  }
  /* In-document search matches (RFC 0063) — distinct from annotation marks. */
  :global(::highlight(i0i-search)) { background: rgba(255, 214, 10, 0.38); }
  :global(::highlight(i0i-search-active)) {
    background: rgba(242, 120, 34, 0.7);
    color: #111;
  }
</style>
