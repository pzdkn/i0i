<script lang="ts">
  import type { ReaderDocument } from "$lib/domain/reader";

  let { document }: { document: ReaderDocument } = $props();
</script>

<aside class="margin-pane hair-l">
  <header class="row hair-b">
    <span class="label hot">Margin / §1</span>
    <div class="flex1"></div>
    <span class="mono-dim">{document.marks.length} marks</span>
  </header>

  <div class="marks">
    {#each document.marks as mark}
      <section class={mark.kind}>
        <div class="mark-meta row">
          <span>{mark.kind} / {mark.paragraphId}</span>
          <span>{mark.createdLabel}</span>
        </div>
        <p>{mark.body}</p>
      </section>
    {/each}
  </div>
</aside>

<style>
  .margin-pane {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    height: 26px;
    flex-shrink: 0;
    gap: 8px;
    padding: 0 10px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .marks {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px;
  }

  section {
    margin-bottom: 14px;
    padding: 7px 8px;
    border: 1px solid var(--border-2);
    border-left: 2px solid var(--amber-dim);
    background: rgba(242, 169, 59, 0.03);
  }

  section.question {
    border-left-color: var(--amber-bright);
  }

  section.note {
    border-left-color: var(--amber);
  }

  .mark-meta {
    justify-content: space-between;
    gap: 8px;
    color: var(--amber-mid);
    font-size: 9px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  p {
    margin: 6px 0 0;
    color: var(--fg-1);
    font-size: 10.5px;
    line-height: 1.5;
  }
</style>
