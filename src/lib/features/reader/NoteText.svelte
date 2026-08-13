<script lang="ts">
  /**
   * A saved note, with `[@vault/key]` references rendered as links (RFC 0090 §3).
   *
   * Reuses `markdown.ts`'s inline pass, which emits typed nodes and never HTML —
   * so a note can carry a reference without becoming an injection surface. Only
   * the reference node is interactive here; the rest of the inline vocabulary
   * (code, bold, italic) renders as it does in an answer.
   *
   * An unresolvable reference prints the text that was typed (R1.3). It is not
   * an error state and it is not styled as one.
   */
  import { parseInline } from "$lib/features/reader/markdown";
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

  const spans = $derived(parseInline(text));
</script>

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
  {:else}
    {span.text}
  {/if}
{/each}

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
</style>
