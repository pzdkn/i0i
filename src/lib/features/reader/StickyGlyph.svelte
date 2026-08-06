<script lang="ts">
  import type { HighlightColor } from "$lib/domain/highlight";
  import { markFill } from "$lib/features/reader/highlight-colors";

  // RFC 0074: the sticky-note mark, in i0i's own hand — a square with a clipped
  // bottom-left corner and the fold drawn back in, so it reads as a folded page
  // corner rather than a rounded app icon. Shared by the standalone note and by
  // the comment marker on a highlight, so the two can never drift apart.
  let {
    color = null,
    size = 14,
  }: {
    color?: HighlightColor | null;
    size?: number;
  } = $props();

  const fill = $derived(markFill(color));
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  aria-hidden="true"
  class="sticky-glyph"
>
  <!-- Body with the corner cut away. -->
  <path d="M1.5 1.5 H14.5 V14.5 H6 L1.5 10 Z" fill={fill} stroke="currentColor" stroke-width="1.2" stroke-linejoin="miter" />
  <!-- The fold itself: the turned-up flap under the cut. -->
  <path d="M6 14.5 V10 H1.5 Z" fill="var(--bg-1, #151515)" stroke="currentColor" stroke-width="1.2" stroke-linejoin="miter" />
</svg>

<style>
  .sticky-glyph {
    flex-shrink: 0;
    color: var(--amber);
    /* The dark page chrome sits behind marks on a white PDF page, so the stroke
       carries the shape and the fill carries the color. */
    filter: drop-shadow(0 1px 1px rgba(0, 0, 0, 0.35));
  }
</style>
