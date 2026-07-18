<script lang="ts">
  import { onMount, type Snippet } from "svelte";

  export type ResizePane = {
    id: string;
    min: number;
    max?: number;
    default: number;
  };

  type Orientation = "horizontal" | "vertical";
  const HANDLE_SIZE = 6;

  let {
    panes,
    storageKey,
    orientation = "horizontal",
    pane,
  }: {
    panes: ResizePane[];
    storageKey: string;
    orientation?: Orientation;
    pane: Snippet<[string]>;
  } = $props();

  let sizes = $state<number[]>([]);
  let splitElement = $state<HTMLElement | undefined>();
  let dragState = $state<{ handleIndex: number; startPos: number; startSizes: number[] } | null>(null);

  const isVertical = $derived(orientation === "vertical");
  const gridTemplate = $derived.by(() => {
    const tracks = sizes.map((size) => `${size}px`).join(" 6px ");
    return isVertical ? `grid-template-rows: ${tracks};` : `grid-template-columns: ${tracks};`;
  });

  onMount(() => {
    sizes = loadSizes();
    const observer = new ResizeObserver(() => {
      sizes = fitToAvailable(sizes.length === panes.length ? sizes : panes.map((item) => item.default));
    });

    if (splitElement) {
      observer.observe(splitElement);
    }

    return () => {
      observer.disconnect();
      window.removeEventListener("pointermove", drag);
    };
  });

  $effect(() => {
    panes;
    if (sizes.length !== panes.length) {
      sizes = loadSizes();
    }
  });

  function loadSizes() {
    const defaults = panes.map((item) => item.default);

    if (typeof localStorage === "undefined") {
      return fitToAvailable(defaults);
    }

    try {
      const raw = localStorage.getItem(storageKey);
      if (!raw) {
        return fitToAvailable(defaults);
      }

      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed) || parsed.length !== panes.length) {
        return fitToAvailable(defaults);
      }

      return fitToAvailable(parsed.map(Number));
    } catch {
      return fitToAvailable(defaults);
    }
  }

  function persist(nextSizes: number[]) {
    if (typeof localStorage === "undefined") {
      return;
    }

    localStorage.setItem(storageKey, JSON.stringify(nextSizes));
  }

  function availableSize() {
    const size = (isVertical ? splitElement?.clientHeight : splitElement?.clientWidth) ?? 0;
    return Math.max(0, size - HANDLE_SIZE * Math.max(0, panes.length - 1));
  }

  function clampSizes(nextSizes: number[]) {
    return nextSizes.map((size, index) => {
      const paneConfig = panes[index];
      const min = paneConfig?.min ?? 0;
      const max = paneConfig?.max ?? Number.POSITIVE_INFINITY;
      const fallback = paneConfig?.default ?? min;
      const safeSize = Number.isFinite(size) ? size : fallback;
      return Math.max(min, Math.min(max, safeSize));
    });
  }

  function fitToAvailable(nextSizes: number[]) {
    const available = availableSize();
    const minimumTotal = panes.reduce((sum, item) => sum + item.min, 0);
    let fitted = clampSizes(nextSizes);

    if (available <= 0) {
      return fitted;
    }

    if (available <= minimumTotal) {
      return panes.map((item) => item.min);
    }

    const currentTotal = fitted.reduce((sum, size) => sum + size, 0);
    const delta = available - currentTotal;
    if (Math.abs(delta) < 1) {
      return fitted;
    }

    fitted = delta > 0 ? growSizes(fitted, delta) : shrinkSizes(fitted, -delta);
    return clampSizes(fitted);
  }

  function dominantPaneIndex() {
    if (panes.length <= 1) {
      return 0;
    }

    // The content pane (no max cap) should absorb container growth; capped
    // panes like inspectors keep their size when the window resizes.
    const flexIndex = panes.findIndex((item) => item.max === undefined);
    if (flexIndex >= 0) {
      return flexIndex;
    }

    return panes.length > 2 ? 1 : panes.length - 1;
  }

  function growSizes(nextSizes: number[], amount: number) {
    const order = [
      dominantPaneIndex(),
      ...panes.map((_, index) => index).filter((index) => index !== dominantPaneIndex()),
    ];
    let remaining = amount;

    for (const index of order) {
      const max = panes[index].max ?? Number.POSITIVE_INFINITY;
      const room = max - nextSizes[index];
      const growth = Math.min(room, remaining);
      if (growth > 0) {
        nextSizes[index] += growth;
        remaining -= growth;
      }
      if (remaining <= 0) {
        break;
      }
    }

    return nextSizes;
  }

  function shrinkSizes(nextSizes: number[], amount: number) {
    const order = panes
      .map((item, index) => ({ index, slack: nextSizes[index] - item.min }))
      .sort((left, right) => right.slack - left.slack)
      .map((item) => item.index);
    let remaining = amount;

    for (const index of order) {
      const slack = nextSizes[index] - panes[index].min;
      const shrink = Math.min(slack, remaining);
      if (shrink > 0) {
        nextSizes[index] -= shrink;
        remaining -= shrink;
      }
      if (remaining <= 0) {
        break;
      }
    }

    return nextSizes;
  }

  function pointerPosition(event: PointerEvent) {
    return isVertical ? event.clientY : event.clientX;
  }

  function startDrag(event: PointerEvent, handleIndex: number) {
    event.preventDefault();
    dragState = {
      handleIndex,
      startPos: pointerPosition(event),
      startSizes: [...sizes],
    };
    window.addEventListener("pointermove", drag);
    window.addEventListener("pointerup", stopDrag, { once: true });
  }

  function drag(event: PointerEvent) {
    if (!dragState) {
      return;
    }

    const leftIndex = dragState.handleIndex;
    const rightIndex = dragState.handleIndex + 1;
    const delta = pointerPosition(event) - dragState.startPos;
    const nextSizes = [...dragState.startSizes];

    nextSizes[leftIndex] = dragState.startSizes[leftIndex] + delta;
    nextSizes[rightIndex] = dragState.startSizes[rightIndex] - delta;

    sizes = clampPairedSizes(nextSizes, leftIndex, rightIndex);
  }

  function stopDrag() {
    window.removeEventListener("pointermove", drag);
    dragState = null;
    sizes = fitToAvailable(sizes);
    persist(sizes);
  }

  function clampPairedSizes(nextSizes: number[], leftIndex: number, rightIndex: number) {
    const total = dragState
      ? dragState.startSizes[leftIndex] + dragState.startSizes[rightIndex]
      : nextSizes[leftIndex] + nextSizes[rightIndex];
    const leftConfig = panes[leftIndex];
    const rightConfig = panes[rightIndex];
    const leftMin = leftConfig.min;
    const rightMin = rightConfig.min;
    const leftMax = leftConfig.max ?? total - rightMin;
    const rightMax = rightConfig.max ?? total - leftMin;
    const minLeft = Math.max(leftMin, total - rightMax);
    const maxLeft = Math.min(leftMax, total - rightMin);
    const left = Math.max(minLeft, Math.min(maxLeft, nextSizes[leftIndex]));

    nextSizes[leftIndex] = left;
    nextSizes[rightIndex] = total - left;
    return nextSizes;
  }

  function reset() {
    const nextSizes = fitToAvailable(panes.map((item) => item.default));
    sizes = nextSizes;
    persist(nextSizes);
  }

  function nudge(handleIndex: number, delta: number) {
    const leftIndex = handleIndex;
    const rightIndex = handleIndex + 1;
    const nextSizes = [...sizes];

    nextSizes[leftIndex] += delta;
    nextSizes[rightIndex] -= delta;
    sizes = clampPairedSizes(nextSizes, leftIndex, rightIndex);
    sizes = fitToAvailable(sizes);
    persist(sizes);
  }

  function onHandleKeydown(event: KeyboardEvent, handleIndex: number) {
    const shrinkKey = isVertical ? "ArrowUp" : "ArrowLeft";
    const growKey = isVertical ? "ArrowDown" : "ArrowRight";

    if (event.key === shrinkKey) {
      event.preventDefault();
      nudge(handleIndex, -16);
    }

    if (event.key === growKey) {
      event.preventDefault();
      nudge(handleIndex, 16);
    }

    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      reset();
    }
  }
</script>

<div
  bind:this={splitElement}
  class="resizable-split"
  class:dragging={dragState !== null}
  class:vertical={isVertical}
  style={gridTemplate}
>
  {#each panes as paneConfig, index}
    <section class="split-pane" data-pane={paneConfig.id}>
      {@render pane(paneConfig.id)}
    </section>

    {#if index < panes.length - 1}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions: ARIA separators are focusable and keyboard-resizable. -->
      <div
        class="split-handle"
        role="separator"
        tabindex="0"
        aria-orientation={isVertical ? "horizontal" : "vertical"}
        aria-label="Resize panels"
        onpointerdown={(event) => startDrag(event, index)}
        ondblclick={reset}
        onkeydown={(event) => onHandleKeydown(event, index)}
      ></div>
    {/if}
  {/each}
</div>

<style>
  .resizable-split {
    display: grid;
    flex: 1;
    min-width: 0;
    min-height: 0;
    align-items: stretch;
    overflow: hidden;
  }

  .split-pane {
    display: flex;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .split-handle {
    position: relative;
    width: 6px;
    min-width: 6px;
    height: 100%;
    padding: 0;
    border: 0;
    border-left: 1px solid var(--border);
    border-right: 1px solid var(--border);
    background: var(--bg-1);
    cursor: col-resize;
  }

  .split-handle::after {
    position: absolute;
    inset: 0 2px;
    background: transparent;
    content: "";
  }

  .vertical .split-handle {
    width: 100%;
    min-width: 0;
    height: 6px;
    min-height: 6px;
    border: 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    cursor: row-resize;
  }

  .vertical .split-handle::after {
    inset: 2px 0;
  }

  .split-handle:hover,
  .split-handle:focus-visible,
  .dragging .split-handle {
    background: rgba(242, 169, 59, 0.12);
    outline: none;
  }

  .split-handle:hover::after,
  .split-handle:focus-visible::after,
  .dragging .split-handle::after {
    background: var(--amber-dim);
  }
</style>
