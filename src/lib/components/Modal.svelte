<script lang="ts">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";

  let {
    open = false,
    title = "",
    onClose = () => {},
    children,
  }: {
    open?: boolean;
    title?: string;
    onClose?: () => void;
    children: Snippet;
  } = $props();

  let dialogEl: HTMLElement | undefined = $state();
  let previouslyFocused: HTMLElement | null = null;

  // Minimal focus handling (RFC 0055): focus the first field on open, and
  // return focus to whatever opened the dialog on close.
  $effect(() => {
    if (open) {
      previouslyFocused = document.activeElement as HTMLElement | null;
      void tick().then(() => {
        const first = dialogEl?.querySelector<HTMLElement>(
          'input, button, select, textarea, [tabindex]:not([tabindex="-1"])',
        );
        (first ?? dialogEl)?.focus();
      });
    } else if (previouslyFocused) {
      previouslyFocused.focus();
      previouslyFocused = null;
    }
  });

  function onKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.stopPropagation();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(event) => event.target === event.currentTarget && onClose()}
  >
    <div
      class="modal hair"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      bind:this={dialogEl}
    >
      <header class="modal-head hair-b row">
        <span class="title">{title}</span>
        <button class="close" type="button" title="Close" aria-label="Close" onclick={onClose}>
          ✕
        </button>
      </header>
      <div class="modal-body">
        {@render children()}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
  }

  .modal {
    width: min(760px, 92vw);
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-1, #111);
    color: var(--fg-1, #ddd);
    box-shadow: 0 12px 48px rgba(0, 0, 0, 0.5);
  }

  .modal-head {
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    flex-shrink: 0;
  }

  .title {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--amber, #f2a93b);
  }

  .close {
    border: 0;
    background: transparent;
    color: var(--fg-3, #888);
    cursor: pointer;
    font-size: 13px;
    padding: 2px 6px;
  }

  .close:hover {
    color: var(--amber, #f2a93b);
  }

  .modal-body {
    min-height: 0;
    overflow: auto;
    padding: 0;
  }
</style>
