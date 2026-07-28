/**
 * Resolves an agent-provided verbatim quote to a character-offset span
 * within the reader's plain-text content.
 *
 * This is a pure function (no DOM/Svelte imports) so it can be unit-tested
 * in isolation and reused by both the HTML reader and future PDF/text
 * readers that expose a flat-text representation.
 *
 * Matching strategy:
 *  1. Exact substring match (`fullText.indexOf(quote.trim())`).
 *  2. Whitespace-tolerant fallback: collapse runs of whitespace to a single
 *     space in both strings, match in normalized space, then map the
 *     normalized match boundaries back to offsets in the original
 *     `fullText` via an index map built while normalizing.
 */

export function resolveQuoteInText(
	fullText: string,
	quote: string
): { start: number; end: number } | null {
	const trimmedQuote = quote.trim();
	if (trimmedQuote.length === 0) {
		return null;
	}

	// 1. Exact match.
	const exactIndex = fullText.indexOf(trimmedQuote);
	if (exactIndex !== -1) {
		return { start: exactIndex, end: exactIndex + trimmedQuote.length };
	}

	// 2. Whitespace-tolerant fallback.
	const { normalized: normalizedText, indexMap } = normalizeWithIndexMap(fullText);
	const normalizedQuote = normalizeWhitespace(trimmedQuote);
	if (normalizedQuote.length === 0) {
		return null;
	}

	const normalizedIndex = normalizedText.indexOf(normalizedQuote);
	if (normalizedIndex === -1) {
		return null;
	}

	const normalizedEnd = normalizedIndex + normalizedQuote.length;

	// indexMap[i] gives the original-text offset corresponding to
	// normalized-text position i. It has one entry per normalized char,
	// plus a trailing entry for the end-of-string position so that the
	// end boundary can be mapped even when the match reaches the end of
	// normalizedText.
	const start = indexMap[normalizedIndex];
	const end = indexMap[normalizedEnd];

	if (start === undefined || end === undefined) {
		return null;
	}

	return { start, end };
}

/** Collapses runs of whitespace (space/tab/newline/etc.) to a single space. */
function normalizeWhitespace(text: string): string {
	return text.replace(/\s+/g, ' ').trim();
}

/**
 * Normalizes whitespace while building a map from each position in the
 * normalized string to the corresponding position in the original string.
 */
function normalizeWithIndexMap(text: string): { normalized: string; indexMap: number[] } {
	let normalized = '';
	const indexMap: number[] = [];
	let inWhitespace = false;

	for (let i = 0; i < text.length; i++) {
		const ch = text[i];
		if (/\s/.test(ch)) {
			if (!inWhitespace && normalized.length > 0) {
				normalized += ' ';
				indexMap.push(i);
			}
			inWhitespace = true;
		} else {
			normalized += ch;
			indexMap.push(i);
			inWhitespace = false;
		}
	}

	// Trailing entry so the end offset of a match that reaches the end of
	// `normalized` maps to the end of `text`.
	indexMap.push(text.length);

	// If normalization produced a trailing space (source ended in
	// whitespace), trim it and its index-map entry so lookups stay aligned
	// with `normalized`'s actual length.
	if (normalized.endsWith(' ')) {
		normalized = normalized.slice(0, -1);
		indexMap.splice(normalized.length, 1);
	}

	return { normalized, indexMap };
}
