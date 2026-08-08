import type { ContextCitation } from "$lib/domain/context";

/** `p3 · Introduction > Motivation` — pages are 1-based to a reader. */
export function citationLabel(citation: ContextCitation): string {
  const page = `p${citation.pageStart + 1}`;
  return citation.headingPath ? `${page} · ${citation.headingPath}` : page;
}
