<script lang="ts">
  import type { ReaderDocument } from "$lib/domain/reader";

  let { document }: { document: ReaderDocument } = $props();

  function paragraphNumber(id: string) {
    return id.startsWith("p") ? id.replace("p", "") : "";
  }
</script>

<article class="paper-page">
  <div class="page-kicker">Extracted text / {document.identifier} / p.1-2</div>

  <div class="paper-title">
    <h2>{document.title}</h2>
    <p>{document.authors.join(" / ")}</p>
  </div>

  {#each document.paragraphs as paragraph}
    {#if paragraph.kind === "heading"}
      <h3>{paragraph.text}</h3>
    {:else}
      <p class:soft={paragraph.highlight === "soft"} class:strong={paragraph.highlight === "strong"}>
        <span class="paragraph-number">¶{paragraphNumber(paragraph.id)}</span>
        {paragraph.text}
      </p>
    {/if}
  {/each}

  <div class="page-footer">Page 2 of 15</div>
</article>

<style>
  .paper-page {
    width: 580px;
    max-width: 100%;
    min-height: 760px;
    flex-shrink: 0;
    padding: 36px 44px;
    border: 1px solid rgba(0, 0, 0, 0.22);
    background: #f0e6c8;
    color: #1a1208;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    font-family: "IBM Plex Mono", ui-monospace, Menlo, Consolas, monospace;
    font-size: 10.5px;
    line-height: 1.65;
  }

  .page-kicker {
    margin-bottom: 18px;
    color: #7a5a30;
    font-size: 8.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .paper-title {
    margin-bottom: 24px;
    text-align: center;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 18px;
    line-height: 1.2;
  }

  .paper-title p {
    margin: 0;
    color: #5a4630;
    font-size: 9.5px;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 11px;
  }

  p {
    position: relative;
    margin: 0 0 10px;
    padding-left: 24px;
  }

  p.soft {
    padding: 4px 10px 4px 28px;
    background: rgba(220, 160, 60, 0.35);
  }

  p.strong {
    padding: 4px 10px 4px 28px;
    background: rgba(220, 160, 60, 0.6);
    font-weight: 500;
  }

  .paragraph-number {
    position: absolute;
    top: 2px;
    left: 0;
    color: #7a5a30;
    font-size: 8px;
  }

  p.soft .paragraph-number,
  p.strong .paragraph-number {
    left: 6px;
  }

  .page-footer {
    margin-top: 22px;
    color: #7a5a30;
    font-size: 8px;
    letter-spacing: 0.2em;
    text-align: center;
    text-transform: uppercase;
  }
</style>
