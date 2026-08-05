<script lang="ts">
  import { HIGHLIGHT_COLORS, type HighlightColor } from "$lib/domain/highlight";
  import { highlightFill } from "$lib/features/reader/highlight-colors";
  import type { AutoHighlightCategory } from "$lib/bridge/chat";

  // The discrete multi-select lens menu for ✨ Highlight with AI (RFC 0064).
  // Presentational: it collects the chosen categories and hands them up; the
  // network call + resolution live in ReaderView.
  let {
    busy = false,
    left,
    top,
    onRun,
    onClose,
  }: {
    busy?: boolean;
    // Viewport coordinates (RFC 0066 R5): the menu is fixed-positioned here so no
    // ancestor overflow clips it.
    left: number;
    top: number;
    onRun: (categories: AutoHighlightCategory[]) => void;
    onClose: () => void;
  } = $props();

  // Fixed v1 lens set, each with a default color (editing the set is deferred).
  const presets: { label: string; color: HighlightColor }[] = [
    { label: "Key contributions", color: "yellow" },
    { label: "Methods", color: "blue" },
    { label: "Results", color: "green" },
    { label: "Limitations", color: "red" },
    { label: "Definitions", color: "purple" },
  ];

  let selected = $state<Record<string, boolean>>({});
  let customText = $state("");
  let customColor = $state<HighlightColor>("orange");

  const chosen = $derived<AutoHighlightCategory[]>([
    ...presets
      .filter((preset) => selected[preset.label])
      .map((preset) => ({ label: preset.label, color: preset.color, prompt: null })),
    ...(customText.trim()
      ? [{ label: "Custom", color: customColor, prompt: customText.trim() }]
      : []),
  ]);

  let root: HTMLElement | undefined = $state();

  function handleWindowMousedown(event: MouseEvent) {
    if (root && !root.contains(event.target as Node)) {
      onClose();
    }
  }
  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onClose();
  }

  function run() {
    if (!chosen.length || busy) return;
    onRun(chosen);
  }
</script>

<svelte:window onmousedown={handleWindowMousedown} onkeydown={handleWindowKeydown} />

<div
  class="menu"
  bind:this={root}
  role="dialog"
  aria-label="Highlight with AI"
  tabindex="-1"
  style={`left: ${left}px; top: ${top}px;`}
  onmousedown={(e) => e.stopPropagation()}
>
  <div class="menu-title">Highlight with AI</div>
  <div class="grid">
    {#each presets as preset}
      <label class="lens" class:on={selected[preset.label]}>
        <input type="checkbox" bind:checked={selected[preset.label]} />
        <span class="dot" style={`background:${highlightFill(preset.color)}`}></span>
        <span class="lens-label">{preset.label}</span>
      </label>
    {/each}
  </div>

  <div class="custom">
    <input
      class="custom-text"
      type="text"
      bind:value={customText}
      placeholder="Custom… (e.g. open problems)"
      aria-label="Custom highlight instruction"
    />
    <div class="swatches" role="group" aria-label="Custom color">
      {#each HIGHLIGHT_COLORS as color}
        <button
          class="swatch"
          class:on={customColor === color}
          type="button"
          style={`background:${highlightFill(color)}`}
          aria-label={`Custom color ${color}`}
          title={color}
          onclick={() => (customColor = color)}
        ></button>
      {/each}
    </div>
  </div>

  <div class="actions">
    <button class="btn primary" type="button" disabled={!chosen.length || busy} onclick={run}>
      {busy ? "Marking…" : `Highlight${chosen.length ? ` (${chosen.length})` : ""}`}
    </button>
  </div>
</div>

<style>
  .menu {
    position: fixed;
    z-index: 60;
    width: 260px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--hair, var(--border-2));
    border-radius: 8px;
    background: var(--bg-1);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
  }

  .menu-title {
    color: var(--fg-2);
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 5px;
  }

  .lens {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 6px;
    border: 1px solid var(--border-2);
    border-radius: 4px;
    color: var(--fg-1);
    font-size: 11px;
    cursor: pointer;
  }

  .lens.on {
    border-color: var(--amber);
    background: rgba(242, 169, 59, 0.08);
  }

  .lens input {
    margin: 0;
  }

  .dot {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .lens-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .custom {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .custom-text {
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
  }

  .custom-text:focus {
    border-color: var(--cyan);
  }

  .swatches {
    display: flex;
    gap: 5px;
  }

  .swatch {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    border: 1px solid transparent;
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
  }

  .swatch.on {
    border-color: var(--fg-1);
    box-shadow: 0 0 0 1px var(--fg-1);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
