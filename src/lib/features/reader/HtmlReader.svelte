<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getReaderHtml } from "$lib/bridge/library";
  import type { ReaderTextSelection } from "$lib/domain/reader";
  import type { ChatThreadSummary } from "$lib/domain/chat";

  let {
    sourceId,
    sourceUrl,
    threads = [],
    chatEnabled = true,
    onSelectPassage,
    onOpenThread,
  }: {
    sourceId: string;
    sourceUrl?: string;
    threads?: ChatThreadSummary[];
    chatEnabled?: boolean;
    onSelectPassage: (selection: ReaderTextSelection) => void;
    onOpenThread?: (threadId: string) => void;
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

  /// The saved thread whose highlighted passage covers `offset`, if any.
  function threadAt(offset: number): ChatThreadSummary | undefined {
    return threads.find((thread) => {
      const anchor = thread.anchor;
      return (
        anchor.kind === "textOffset" &&
        anchor.sourceId === sourceId &&
        offset >= anchor.startOffset &&
        offset < anchor.endOffset
      );
    });
  }

  function handleMouseUp() {
    if (!root) return;
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0);
    if (!root.contains(range.commonAncestorContainer)) return;

    // A plain click (no drag) inside a highlight opens that thread — parity with
    // clicking a PDF highlight mark (RFC 0056).
    if (selection.isCollapsed) {
      if (onOpenThread && selection.anchorNode) {
        const offset = offsetIn(root, selection.anchorNode, selection.anchorOffset);
        const thread = threadAt(offset);
        if (thread) onOpenThread(thread.id);
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

  // Repaint highlights for this document's saved threads using the CSS Custom
  // Highlight API — no DOM mutation, so re-running is safe and cheap. Re-runs
  // whenever the rendered HTML or the thread set changes.
  $effect(() => {
    html;
    const currentThreads = threads;
    if (!root || !html) return;
    const highlightApi = (
      CSS as unknown as { highlights?: Map<string, unknown> }
    ).highlights;
    if (!highlightApi || typeof (globalThis as { Highlight?: unknown }).Highlight !== "function") {
      return;
    }

    const ranges: Range[] = [];
    for (const thread of currentThreads) {
      const anchor = thread.anchor;
      if (anchor.kind !== "textOffset" || anchor.sourceId !== sourceId) continue;
      const start = locate(root, anchor.startOffset);
      const end = locate(root, anchor.endOffset);
      if (!start || !end) continue;
      const range = document.createRange();
      range.setStart(start.node, start.offset);
      range.setEnd(end.node, end.offset);
      ranges.push(range);
    }

    const HighlightCtor = (globalThis as { Highlight: new (...ranges: Range[]) => unknown })
      .Highlight;
    highlightApi.set("i0i-annotation", new HighlightCtor(...ranges));
    return () => highlightApi.delete("i0i-annotation");
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
      <button class="view-original" type="button" onclick={viewOriginal}>View original ↗</button>
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

  /* Annotation highlights (CSS Custom Highlight API — no DOM mutation). */
  :global(::highlight(i0i-annotation)) {
    background: rgba(242, 169, 59, 0.28);
    color: inherit;
  }
</style>
