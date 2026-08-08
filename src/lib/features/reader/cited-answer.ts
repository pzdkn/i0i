import type { ContextCitation } from "$lib/domain/context";

/**
 * An answer split into prose and `[C1]`-style citation markers (RFC 0077).
 *
 * A piece with a `citation` renders as a clickable marker; everything else is
 * literal text.
 */
export type AnswerPiece = { text: string; citation: ContextCitation | null };

const MARKER = /\[(C\d+)\]/g;

/**
 * Split `body` on the citation markers the assembly actually minted.
 *
 * Unknown markers are deliberately left as literal text. The model can write
 * `[C9]` for a passage it never saw — that happens — and a button that jumps
 * nowhere is worse than the characters it replaced.
 */
export function splitCitedAnswer(body: string, citations: ContextCitation[]): AnswerPiece[] {
  const byHandle = new Map(citations.map((citation) => [citation.handle, citation]));
  const pieces: AnswerPiece[] = [];
  let cursor = 0;

  for (const match of body.matchAll(MARKER)) {
    const citation = byHandle.get(match[1]);
    if (!citation) {
      continue;
    }
    const start = match.index ?? 0;
    if (start > cursor) {
      pieces.push({ text: body.slice(cursor, start), citation: null });
    }
    pieces.push({ text: match[1], citation });
    cursor = start + match[0].length;
  }

  if (cursor < body.length) {
    pieces.push({ text: body.slice(cursor), citation: null });
  }
  return pieces;
}

/** `p3 · Introduction > Motivation` — pages are 1-based to a reader. */
export function citationLabel(citation: ContextCitation): string {
  const page = `p${citation.pageStart + 1}`;
  return citation.headingPath ? `${page} · ${citation.headingPath}` : page;
}
