import type { HighlightColor } from "$lib/domain/highlight";

// Shared rgba palette for highlight fills — must match the CSS Custom
// Highlight colors used by HtmlReader.svelte (RFC 0056).
const FILLS: Record<HighlightColor, string> = {
  yellow: "rgba(242, 201, 76, 0.32)",
  green: "rgba(111, 207, 151, 0.32)",
  blue: "rgba(86, 156, 214, 0.32)",
  red: "rgba(235, 87, 87, 0.32)",
  purple: "rgba(187, 107, 217, 0.32)",
  orange: "rgba(242, 153, 74, 0.32)",
};

export function highlightFill(color: HighlightColor): string {
  return FILLS[color];
}

// Subtle neutral fill for a note-only passage (no color chosen) — RFC 0061.
export const NEUTRAL_FILL = "rgba(148, 148, 148, 0.20)";

/// Fill for a mark whose color may be null (note-only passages render neutral).
export function markFill(color: HighlightColor | null): string {
  return color ? FILLS[color] : NEUTRAL_FILL;
}
