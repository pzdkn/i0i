<script lang="ts">
  /**
   * The visible mark on a hidden zone's edge strip (RFC 0082 R1.2/R3.1).
   *
   * RFC 0081 hid the chrome behind 6px of unmarked window edge, which hides the
   * way back as well as the chrome. This is the smallest thing that reads as
   * "there is something here": a short bar, brighter on hover, amber when the
   * zone is pinned. Clicking it pins.
   */
  let {
    edge,
    pinned = false,
    label,
    onToggle,
  }: {
    edge: "top" | "bottom" | "left" | "right";
    pinned?: boolean;
    /** Named for the screen reader and the tooltip, e.g. "toolbar". */
    label: string;
    onToggle: () => void;
  } = $props();

  const vertical = $derived(edge === "left" || edge === "right");
</script>

<button
  class="reveal-handle {edge}"
  class:vertical
  class:pinned
  type="button"
  title={pinned ? `Unpin ${label}` : `Pin ${label} open`}
  aria-label={pinned ? `Unpin ${label}` : `Pin ${label} open`}
  aria-pressed={pinned}
  onclick={onToggle}
></button>

<style>
  .reveal-handle {
    position: absolute;
    z-index: 1;
    padding: 0;
    border: 0;
    border-radius: 2px;
    background: var(--border-2, #3a3a3a);
    cursor: pointer;
  }

  .reveal-handle:not(.vertical) {
    left: 50%;
    width: 28px;
    height: 2px;
    margin-left: -14px;
  }

  .reveal-handle.vertical {
    top: 50%;
    width: 2px;
    height: 28px;
    margin-top: -14px;
  }

  .reveal-handle.top {
    top: 2px;
  }

  .reveal-handle.bottom {
    bottom: 2px;
  }

  .reveal-handle.left {
    left: 2px;
  }

  .reveal-handle.right {
    right: 2px;
  }

  .reveal-handle:hover {
    background: var(--amber, #f2a93b);
  }

  .reveal-handle.pinned {
    background: var(--amber, #f2a93b);
  }
</style>
