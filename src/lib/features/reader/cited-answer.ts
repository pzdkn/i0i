import type { ContextCitation } from "$lib/domain/context";

/**
 * An answer split into prose and `[1]`-style citation markers (RFC 0078).
 *
 * A piece with a `citation` renders as a clickable marker; everything else is
 * literal text.
 */
export type AnswerPiece = { text: string; citation: ContextCitation | null };

const MARKER = /\[(\d+)\]/g;

/**
 * Split `body` on the citation markers the assembly actually minted.
 *
 * Unknown numbers are deliberately left as literal text, which matters more
 * with bare `[n]` than it did with `[Cn]`: papers are full of their own
 * bracketed citations, and the model quoting "as shown in [12]" must not
 * produce a link. Only numbers this turn actually handed out become buttons.
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
