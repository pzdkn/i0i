// In-document search matcher (RFC 0063). Pure, DOM-free: given the article's
// plain text and a query, return the character-offset ranges of every
// case-insensitive match. `HtmlReader` maps these offsets back to DOM ranges via
// the same text-node walk its annotation highlights use.

export type TextMatch = { start: number; end: number };

/// Every case-insensitive, non-overlapping occurrence of `query` in `haystack`,
/// as `[start, end)` char-offset ranges. Whitespace-only or empty queries match
/// nothing.
export function findTextMatches(haystack: string, query: string): TextMatch[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [];
  }
  const hay = haystack.toLowerCase();
  const matches: TextMatch[] = [];
  let from = 0;
  // Guard against a query that trims to empty producing an infinite loop.
  if (needle.length === 0) {
    return matches;
  }
  for (;;) {
    const at = hay.indexOf(needle, from);
    if (at === -1) {
      break;
    }
    matches.push({ start: at, end: at + needle.length });
    from = at + needle.length;
  }
  return matches;
}
