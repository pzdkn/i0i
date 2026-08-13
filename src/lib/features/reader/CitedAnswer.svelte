<script lang="ts">
  import type { ContextCitation } from "$lib/domain/context";
  import { citationLabel } from "$lib/features/reader/cited-answer";
  import { parseMarkdown, type Inline } from "$lib/features/reader/markdown";

  let {
    body,
    citations = [],
    onOpenCitation = () => {},
  }: {
    body: string;
    citations?: ContextCitation[];
    onOpenCitation?: (citation: ContextCitation) => void;
  } = $props();

  const blocks = $derived(parseMarkdown(body));
  const byHandle = $derived(new Map(citations.map((citation) => [citation.handle, citation])));

  /**
   * A `[n]` the assembly never minted stays literal text.
   *
   * Papers are full of their own bracketed citations, so the model quoting
   * "as shown in [12]" must not produce a link to nothing (RFC 0078).
   */
  function citationFor(handle: string) {
    return byHandle.get(handle) ?? null;
  }
</script>

{#snippet inline(spans: Inline[])}
  {#each spans as span, index (index)}
    {#if span.kind === "code"}
      <code>{span.text}</code>
    {:else if span.kind === "strong"}
      <strong>{span.text}</strong>
    {:else if span.kind === "em"}
      <em>{span.text}</em>
    {:else if span.kind === "cite"}
      {@const citation = citationFor(span.handle)}
      {#if citation}
        <button
          class="cite"
          type="button"
          title={citationLabel(citation)}
          onclick={() => onOpenCitation(citation)}>[{span.handle}]</button
        >
      {:else}
        [{span.handle}]
      {/if}
    {:else if span.kind === "paperRef"}
      <!-- RFC 0090: a reference inside a model answer is not resolved — the
           notation is for the reader's own notes. It reads as what was typed. -->
      {span.raw}
    {:else}
      {span.text}
    {/if}
  {/each}
{/snippet}

<div class="md">
  {#each blocks as block, index (index)}
    {#if block.kind === "p"}
      <p>{@render inline(block.spans)}</p>
    {:else if block.kind === "h"}
      <!-- Levels collapse to one weight: an answer is a few sentences, not a
           document, and six heading sizes in a narrow panel is noise. -->
      <p class="head">{@render inline(block.spans)}</p>
    {:else if block.kind === "list"}
      {#if block.ordered}
        <ol>
          {#each block.items as item, itemIndex (itemIndex)}
            <li>{@render inline(item)}</li>
          {/each}
        </ol>
      {:else}
        <ul>
          {#each block.items as item, itemIndex (itemIndex)}
            <li>{@render inline(item)}</li>
          {/each}
        </ul>
      {/if}
    {:else if block.kind === "code"}
      <!-- Pseudocode is a first-class answer shape: five lines of it beat a
           paragraph the reader has to re-derive. Horizontal scroll rather than
           wrapping — a wrapped algorithm reads as a different algorithm. -->
      <div class="code-block">
        {#if block.lang}
          <div class="code-lang mono-dim">{block.lang}</div>
        {/if}
        <pre>{block.text}</pre>
      </div>
    {:else if block.kind === "quote"}
      <blockquote>{@render inline(block.spans)}</blockquote>
    {:else}
      <hr />
    {/if}
  {/each}
</div>

<style>
  /* Every node here is rendered as text by Svelte — nothing reaches innerHTML,
     so model output cannot become markup. */
  .md {
    min-width: 0;
  }

  p {
    margin: 0 0 6px;
  }

  p:last-child {
    margin-bottom: 0;
  }

  .head {
    color: var(--fg-1);
    font-weight: 600;
  }

  ul,
  ol {
    margin: 0 0 6px;
    padding-left: 16px;
  }

  li {
    margin-bottom: 2px;
  }

  code {
    padding: 0 2px;
    background: var(--bg-2);
    color: var(--amber-mid);
  }

  .code-block {
    margin: 0 0 6px;
    border: 1px solid var(--border);
    background: var(--bg-2);
  }

  .code-lang {
    padding: 2px 6px 0;
    font-size: 9px;
  }

  pre {
    overflow-x: auto;
    margin: 0;
    padding: 5px 6px;
    /* Indentation carries meaning in pseudocode, so it must not collapse. */
    white-space: pre;
    tab-size: 2;
  }

  blockquote {
    margin: 0 0 6px;
    padding-left: 7px;
    border-left: 2px solid var(--border-2);
    color: var(--fg-2);
  }

  hr {
    margin: 8px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  strong {
    color: var(--fg-1);
    font-weight: 600;
  }

  /* Reads as part of the sentence, not as a control that interrupts it. */
  .cite {
    display: inline;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    cursor: pointer;
  }

  .cite:hover {
    text-decoration: underline;
  }
</style>
