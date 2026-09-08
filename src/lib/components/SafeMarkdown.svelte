<script lang="ts">
  import MathExpression from "$lib/features/reader/MathExpression.svelte";
  import { parseMarkdown, type Inline } from "$lib/features/reader/markdown";

  let { text }: { text: string } = $props();
  const blocks = $derived(parseMarkdown(text));
</script>

{#snippet inline(spans: Inline[])}
  {#each spans as span, index (index)}
    {#if span.kind === "code"}<code>{span.text}</code>
    {:else if span.kind === "strong"}<strong>{span.text}</strong>
    {:else if span.kind === "em"}<em>{span.text}</em>
    {:else if span.kind === "math"}<MathExpression tex={span.tex} raw={span.raw} />
    {:else if span.kind === "cite"}[{span.handle}]
    {:else if span.kind === "paperRef"}{span.raw}
    {:else}{span.text}{/if}
  {/each}
{/snippet}

<div class="safe-markdown">
  {#each blocks as block, index (index)}
    {#if block.kind === "p"}<p>{@render inline(block.spans)}</p>
    {:else if block.kind === "h"}<p class="heading">{@render inline(block.spans)}</p>
    {:else if block.kind === "list"}
      {#if block.ordered}<ol>{#each block.items as item}<li>{@render inline(item)}</li>{/each}</ol>
      {:else}<ul>{#each block.items as item}<li>{@render inline(item)}</li>{/each}</ul>{/if}
    {:else if block.kind === "code"}<pre><code>{block.text}</code></pre>
    {:else if block.kind === "math"}<MathExpression tex={block.tex} raw={block.raw} display />
    {:else if block.kind === "quote"}<blockquote>{@render inline(block.spans)}</blockquote>
    {:else}<hr />{/if}
  {/each}
</div>

<style>
  :global(.safe-markdown p),
  :global(.safe-markdown ul),
  :global(.safe-markdown ol),
  :global(.safe-markdown pre),
  :global(.safe-markdown blockquote) { margin: 0 0 7px; }
  :global(.safe-markdown > :last-child) { margin-bottom: 0; }
  :global(.safe-markdown ul), :global(.safe-markdown ol) { padding-left: 18px; }
  :global(.safe-markdown .heading) { font-weight: 600; }
  :global(.safe-markdown pre) { overflow-x: auto; white-space: pre; }
  :global(.safe-markdown blockquote) {
    padding-left: 8px;
    border-left: 2px solid var(--border-2);
    color: var(--fg-2);
  }
</style>
