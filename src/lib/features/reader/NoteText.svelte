<script lang="ts">
  /**
   * A saved note, with `[@vault/key]` references rendered as links (RFC 0090 §3).
   *
   * Reuses `markdown.ts`'s typed block tree, which never accepts model HTML, so
   * references and math render consistently with chat answers.
   *
   * An unresolvable reference prints the text that was typed (R1.3). It is not
   * an error state and it is not styled as one.
   */
  import { parseMarkdown, type Inline } from "$lib/features/reader/markdown";
  import MathExpression from "$lib/features/reader/MathExpression.svelte";
  import { resolveRef, type RefIndex } from "$lib/features/reader/paper-refs";

  let {
    text,
    index,
    currentVaultId = "",
    onOpenPaper,
    onOpenVault,
  }: {
    text: string;
    index: RefIndex;
    currentVaultId?: string;
    onOpenPaper?: (paperId: string) => void;
    onOpenVault?: (vaultId: string) => void;
  } = $props();

  const blocks = $derived(parseMarkdown(text));
</script>

{#snippet inline(spans: Inline[])}
  {#each spans as span, position (position)}
    {#if span.kind === "paperRef"}
      {@const resolved = resolveRef(index, span, currentVaultId)}
      {#if resolved?.kind === "paper"}
        <button class="ref" type="button" title={resolved.title} onclick={() => onOpenPaper?.(resolved.paperId)}>
          {resolved.label}
        </button>
      {:else if resolved?.kind === "vault"}
        <button class="ref" type="button" title={`Vault: ${resolved.title}`} onclick={() => onOpenVault?.(resolved.vaultId)}>
          {resolved.label}
        </button>
      {:else}
        {span.raw}
      {/if}
    {:else if span.kind === "cite"}
      [{span.handle}]
    {:else if span.kind === "code"}
      <code>{span.text}</code>
    {:else if span.kind === "strong"}
      <strong>{span.text}</strong>
    {:else if span.kind === "em"}
      <em>{span.text}</em>
    {:else if span.kind === "math"}
      <MathExpression tex={span.tex} raw={span.raw} />
    {:else}
      {span.text}
    {/if}
  {/each}
{/snippet}

<div class="note-markdown">
  {#each blocks as block, position (position)}
    {#if block.kind === "p"}
      <p>{@render inline(block.spans)}</p>
    {:else if block.kind === "h"}
      <p class="head">{@render inline(block.spans)}</p>
    {:else if block.kind === "list"}
      {#if block.ordered}
        <ol>
          {#each block.items as item, itemPosition (itemPosition)}
            <li>{@render inline(item)}</li>
          {/each}
        </ol>
      {:else}
        <ul>
          {#each block.items as item, itemPosition (itemPosition)}
            <li>{@render inline(item)}</li>
          {/each}
        </ul>
      {/if}
    {:else if block.kind === "code"}
      <pre><code>{block.text}</code></pre>
    {:else if block.kind === "math"}
      <MathExpression tex={block.tex} raw={block.raw} display />
    {:else if block.kind === "quote"}
      <blockquote>{@render inline(block.spans)}</blockquote>
    {:else}
      <hr />
    {/if}
  {/each}
</div>

<style>
  .ref {
    padding: 0;
    border: 0;
    border-bottom: 1px solid rgba(242, 169, 59, 0.4);
    background: transparent;
    color: var(--amber);
    font: inherit;
    cursor: pointer;
  }

  .ref:hover {
    border-bottom-color: var(--amber);
  }

  code {
    font-size: 0.95em;
  }

  .note-markdown {
    min-width: 0;
  }

  p,
  ul,
  ol,
  pre,
  blockquote {
    margin: 0 0 6px;
  }

  p:last-child,
  ul:last-child,
  ol:last-child,
  pre:last-child,
  blockquote:last-child {
    margin-bottom: 0;
  }

  .head {
    font-weight: 600;
  }

  ul,
  ol {
    padding-left: 16px;
  }

  pre {
    overflow-x: auto;
    white-space: pre;
  }

  blockquote {
    padding-left: 7px;
    border-left: 2px solid var(--border-2);
    color: var(--fg-2);
  }

  hr {
    margin: 8px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
</style>
