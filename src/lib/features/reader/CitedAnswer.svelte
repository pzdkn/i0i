<script lang="ts">
  import type { ContextCitation } from "$lib/domain/context";
  import { citationLabel, splitCitedAnswer } from "$lib/features/reader/cited-answer";

  let {
    body,
    citations = [],
    onOpenCitation = () => {},
  }: {
    body: string;
    citations?: ContextCitation[];
    onOpenCitation?: (citation: ContextCitation) => void;
  } = $props();

  const pieces = $derived(splitCitedAnswer(body, citations));
</script>

<p>
  {#each pieces as piece, index (index)}
    {#if piece.citation}
      <button
        class="cite"
        type="button"
        title={citationLabel(piece.citation)}
        onclick={() => onOpenCitation(piece.citation!)}
      >[{piece.text}]</button>
    {:else}
      {piece.text}
    {/if}
  {/each}
</p>

<style>
  p {
    margin: 0;
    white-space: pre-wrap;
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
    background: var(--bg-2);
    color: var(--amber);
  }
</style>
