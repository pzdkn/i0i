<script lang="ts">
  /**
   * RFC 0086 R4: this bar used to be a static mock — `p.2 / 15` and `mode: PDF`
   * for every document, including HTML articles, next to a legend for four keys
   * none of which were bound. It now describes the document that is open and
   * advertises only what §1 wired.
   */
  let {
    contentKind = "pdf",
    currentPage = 0,
    pageCount = 0,
    onStepPage,
  }: {
    /** Free-form on `ReaderDocument`; only "html" is distinguished. */
    contentKind?: string;
    currentPage?: number;
    pageCount?: number;
    onStepPage?: (delta: number) => void;
  } = $props();

  const isHtml = $derived(contentKind === "html");
  const hasPages = $derived(!isHtml && pageCount > 0);
</script>

<footer class="reader-footer row hair-t">
  {#if hasPages}
    <span class="pages row">
      <button class="key" type="button" aria-label="Previous page" onclick={() => onStepPage?.(-1)}>&lt;</button>
      p.{currentPage} / {pageCount}
      <button class="key" type="button" aria-label="Next page" onclick={() => onStepPage?.(1)}>&gt;</button>
    </span>
  {/if}
  <span class="mono-dim">mode: {isHtml ? "HTML" : "PDF"}</span>
  <div class="flex1"></div>
  <span><span class="key">h</span> highlight</span>
  <span><span class="key">n</span> note</span>
  <span><span class="key">q</span> question</span>
  <span><span class="key">cmd+enter</span> ask</span>
</footer>

<style>
  .pages {
    gap: 6px;
    align-items: center;
  }

  .key {
    color: var(--fg-4);
  }

  button.key {
    padding: 0 2px;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  button.key:hover {
    color: var(--amber);
  }

  .reader-footer {
    height: 26px;
    flex-shrink: 0;
    gap: 14px;
    padding: 0 18px;
    background: var(--bg-1);
    color: var(--fg-2);
    font-size: 10px;
    white-space: nowrap;
  }
</style>
