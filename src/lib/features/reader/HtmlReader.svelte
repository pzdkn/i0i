<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ExternalLink, RefreshCw } from "@lucide/svelte";
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
    onReExtract,
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
    /** RFC 0083 R4.1: re-fetch and re-ingest this page. */
    onReExtract?: () => void | Promise<void>;
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

  type CaretPoint = { node: Node; offset: number };

  /// The text position under a viewport point, across both spellings of the
  /// API. Returns null when neither exists or the point is not over text.
  function resolveCaret(clientX: number, clientY: number): CaretPoint | null {
    const doc = document as Document & {
      caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
    };
    const position = doc.caretPositionFromPoint?.(clientX, clientY);
    if (position) {
      return { node: position.offsetNode, offset: position.offset };
    }
    const range = doc.caretRangeFromPoint?.(clientX, clientY);
    return range ? { node: range.startContainer, offset: range.startOffset } : null;
  }

  /// RFC 0079 R1.1: right-click a mark to reach its actions — Remove included.
  /// The HTML reader has no per-mark element to hang a hover affordance on
  /// (marks are ranges painted over the article), so the pointer position is
  /// resolved to an offset the same way a plain click is.
  function handleContextMenu(event: MouseEvent) {
    if (!root || !onHighlightClick) return;
    const target = event.target as Node | null;
    if (!target || !root.contains(target)) return;
    // `caretPositionFromPoint` is the standard; WebKit still ships only the
    // older `caretRangeFromPoint`, and this reader runs in a WKWebView.
    const point = resolveCaret(event.clientX, event.clientY);
    if (!point) return;
    const offset = offsetIn(root, point.node, point.offset);
    const hit = findHighlightForOffset(highlights, sourceId, offset);
    if (!hit) return;
    event.preventDefault();
    onHighlightClick(hit.id, event.clientX, event.clientY);
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

  // RFC 0083 R2.1: every click on a link inside the article comes here, and the
  // default is to prevent it. An unhandled `<a href="https://…">` navigates the
  // webview away from the app — the SPA unloads, and the window's close button
  // then closes i0i rather than the page. Preventing by default makes that class
  // of bug impossible instead of fixing it one href at a time.
  function handleArticleClick(event: MouseEvent) {
    const anchor = (event.target as HTMLElement | null)?.closest?.("a");
    if (!anchor || !root?.contains(anchor)) {
      return;
    }
    event.preventDefault();

    const href = anchor.getAttribute("href") ?? "";
    if (href.startsWith("#")) {
      jumpToTarget(href.slice(1));
    } else if (/^https?:\/\//i.test(href)) {
      // The system browser, never this window.
      void openUrl(href).catch(() => {});
    }
  }

  /**
   * Scroll an in-document reference target into view and flash it, remembering
   * where the jump started so R3.1's Return chip can undo it. Scoped to the
   * article root: ids come from someone else's document and may collide with
   * the app's own.
   */
  function jumpToTarget(id: string) {
    const target = root?.querySelector(`[id="${CSS.escape(id)}"]`);
    const scroller = root?.closest(".html-reader") as HTMLElement | null;
    if (!target || !scroller) {
      return;
    }
    returnScrollTop = scroller.scrollTop;
    jumpSettlesAt = performance.now() + 900;
    // Same one-scroller geometry as `focusMatch` and for the same reason
    // (RFC 0073 R2.2): `scrollIntoView` would also scroll the inspector panel.
    const rect = target.getBoundingClientRect();
    const scRect = scroller.getBoundingClientRect();
    scroller.scrollBy({
      top: rect.top - scRect.top - scroller.clientHeight / 2 + rect.height / 2,
      behavior: "smooth",
    });
    target.classList.add("i0i-ref-flash");
    setTimeout(() => target.classList.remove("i0i-ref-flash"), 1600);
  }

  // RFC 0083 R3.1: the scroll position a reference jump left behind. A second
  // jump replaces it rather than stacking — one step back is the affordance,
  // not a history.
  let returnScrollTop = $state<number | null>(null);
  // The jump itself scrolls, and it scrolls smoothly — without this the chip
  // would dismiss itself on the first frame of the animation, while the page is
  // still sitting at the origin R3.2 checks against.
  let jumpSettlesAt = 0;

  /** R3.2: scrolling back near where you were retires the chip on its own. */
  function handleScroll(event: Event) {
    if (returnScrollTop === null || performance.now() < jumpSettlesAt) {
      return;
    }
    const scroller = event.currentTarget as HTMLElement;
    if (Math.abs(scroller.scrollTop - returnScrollTop) < scroller.clientHeight) {
      returnScrollTop = null;
    }
  }

  function returnFromJump() {
    const scroller = root?.closest(".html-reader");
    if (scroller && returnScrollTop !== null) {
      scroller.scrollTo({ top: returnScrollTop, behavior: "smooth" });
    }
    returnScrollTop = null;
  }

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

<div class="html-reader col" onscroll={handleScroll}>
  <div class="toolbar row">
    <span class="kind">HTML</span>
    {#if sourceUrl}
      <div class="row toolbar-actions">
        {#if onReExtract}
          <!-- RFC 0083 R4.1: a page cached before reference resolution existed
               keeps its `??`s until it is fetched again. -->
          <button class="view-original" type="button" title="Fetch and extract this page again" onclick={() => void onReExtract?.()}>
            Re-extract <RefreshCw size={12} strokeWidth={1.75} aria-hidden="true" />
          </button>
        {/if}
        <button class="view-original" type="button" onclick={viewOriginal}>
          View original <ExternalLink size={12} strokeWidth={1.75} aria-hidden="true" />
        </button>
      </div>
    {/if}
  </div>

  {#if loadError}
    <div class="notice">Could not load article: {loadError}</div>
  {:else if !html}
    <div class="notice">Loading article…</div>
  {:else}
    <!-- Safe: the backend sanitized this (ammonia allowlist, no scripts/externals). -->
    <!-- The click handler intercepts links inside the article, which are
         keyboard-activatable in their own right (RFC 0083 R2.1). -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <article
      class="html-content"
      bind:this={root}
      onmouseup={handleMouseUp}
      oncontextmenu={handleContextMenu}
      onclick={handleArticleClick}
    >
      {@html html}
    </article>
  {/if}

  {#if returnScrollTop !== null}
    <button class="return-chip" type="button" onclick={returnFromJump}>
      ← Back to where you were
    </button>
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

  .toolbar-actions {
    gap: 10px;
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

  /* RFC 0083 R1.2: a reference we could not resolve — visible as a reference,
     but not dressed as something you can follow. */
  .html-content :global(.i0i-ref-unresolved) {
    color: var(--fg-3, #7a7a7a);
    font-size: 0.85em;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  /* R2.1: the landing flash, so a jump to a figure lands somewhere visible. */
  .html-content :global(.i0i-ref-flash) {
    animation: ref-flash 1.6s ease-out;
  }

  @keyframes ref-flash {
    from {
      background: rgba(242, 169, 59, 0.28);
    }
    to {
      background: transparent;
    }
  }

  /* R3.1: one step back from a reference jump. Sticky, so it rides the bottom
     of the reading surface without covering the inspector. */
  .return-chip {
    position: sticky;
    bottom: 14px;
    z-index: 2;
    align-self: center;
    margin-top: -30px;
    padding: 5px 11px;
    border: 1px solid var(--border-2, #3a3a3a);
    border-radius: 3px;
    background: var(--panel, #1a1a1a);
    box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
    color: var(--fg-1, #ddd);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .return-chip:hover {
    border-color: var(--amber, #f2a93b);
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
