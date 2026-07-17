<script lang="ts">
  import type { Paper } from "$lib/domain/paper";

  let {
    papers,
    selectedPaper,
  }: {
    papers: Paper[];
    selectedPaper: Paper | undefined;
  } = $props();

  const readCount = $derived(papers.filter((paper) => paper.status === "READ").length);
  const readingCount = $derived(papers.filter((paper) => paper.status === "READING").length);
  const unreadCount = $derived(papers.filter((paper) => paper.status === "UNREAD").length);
  const annotationCount = $derived(
    papers.reduce((total, paper) => total + paper.annotationCount, 0),
  );
</script>

<aside class="inspector hair-l">
  <header class="hair-b">
    {#if selectedPaper}
      <div class="paper-name truncate">{selectedPaper.title}</div>
      <div class="mono-dim">{selectedPaper.year} / {selectedPaper.venue} / selected</div>
    {:else}
      <div class="paper-name truncate">No paper selected</div>
      <div class="mono-dim">Add papers to this vault</div>
    {/if}
  </header>

  {#if selectedPaper}
    <section>
      <div class="label hot">Selected paper</div>
      <div class="meta">
        <div><span>authors</span>{selectedPaper.authors.slice(0, 4).join(", ")}</div>
        <div><span>cites</span>{selectedPaper.citations.toLocaleString()}</div>
        <div><span>highlights</span>{selectedPaper.highlightCount}</div>
        <div><span>ann</span>{selectedPaper.annotationCount}</div>
      </div>
      <div class="tags row">
        {#each selectedPaper.tags as tag}
          <span class="chip">{tag}</span>
        {/each}
      </div>
    </section>
  {/if}

  <section>
    <div class="label hot">Reading stats</div>
    <div class="stat-row">
      <span>read</span>
      <div class="gauge"><i style={`width: ${papers.length ? (readCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{readCount} / {papers.length}</span>
    </div>
    <div class="stat-row">
      <span>reading</span>
      <div class="gauge"><i style={`width: ${papers.length ? (readingCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{readingCount} / {papers.length}</span>
    </div>
    <div class="stat-row">
      <span>unread</span>
      <div class="gauge"><i style={`width: ${papers.length ? (unreadCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{unreadCount} / {papers.length}</span>
    </div>
  </section>

  <section>
    <div class="label hot">Ask this folder</div>
    <div class="ask-box">
      <p>What are the main arguments against scaled dot-product attention's O(n^2) complexity?</p>
      <div class="row">
        <button class="btn primary" type="button">Run</button>
        <button class="btn ghost" type="button">cite 22</button>
      </div>
    </div>
  </section>

  <div class="flex1"></div>

  <section class="footer-section hair-t">
    <div class="label">Folder health</div>
    <div class="meta">
      <div><span>papers</span>{papers.length}</div>
      <div><span>annotations</span>{annotationCount}</div>
      <div><span>bibtex</span>ready</div>
    </div>
  </section>
</aside>

<style>
  .inspector {
    width: 100%;
    height: 100%;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    flex-shrink: 0;
    padding: 10px 14px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .paper-name {
    margin-bottom: 2px;
    color: var(--amber);
    font-size: 11px;
  }

  section {
    padding: 14px 14px 0;
  }

  .stat-row {
    display: grid;
    grid-template-columns: 64px 1fr 52px;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 11px;
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 10px;
  }

  .meta div {
    display: grid;
    grid-template-columns: 58px 1fr;
    gap: 8px;
  }

  .meta span {
    color: var(--fg-3);
  }

  .tags {
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 10px;
  }

  .ask-box {
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--amber-dim);
    background: rgba(242, 169, 59, 0.04);
  }

  .ask-box p {
    margin: 0 0 8px;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.5;
  }

  .ask-box .row {
    gap: 6px;
  }

  .footer-section {
    padding: 10px 14px;
    background: var(--bg-1);
  }
</style>
