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

	// 2. Fuzzy fallback: match on the alphanumeric sequence, tolerant of spacing,
	// punctuation, hyphenation, ligature, and case differences between the two
	// text extractions.
	const { normalized: normalizedText, indexMap } = normalizeWithIndexMap(fullText);
	const normalizedQuote = normalizeForMatch(trimmedQuote);
	if (normalizedQuote.length === 0) {
		return null;
	}

	const normalizedIndex = normalizedText.indexOf(normalizedQuote);
	if (normalizedIndex === -1) {
		return null;
	}

	const lastMatchedNormalized = normalizedIndex + normalizedQuote.length - 1;

	// `indexMap[k]` is the original-text index of the k-th kept (alphanumeric)
	// character. The span starts at the first matched char and ends just after
	// the last matched original char (so trailing spaces/punctuation aren't
	// swept into the highlight).
	const start = indexMap[normalizedIndex];
	const lastOriginal = indexMap[lastMatchedNormalized];

	if (start === undefined || lastOriginal === undefined) {
		return null;
	}

	return { start, end: lastOriginal + 1 };
}

/**
 * Reduce text to a lowercase ALPHANUMERIC sequence: NFKD-normalize (which splits
 * ligatures like "ﬁ"→"fi" and accents), lowercase, and drop everything that
 * isn't [a-z0-9] — all whitespace, punctuation, and hyphens.
 *
 * This is what makes a model's quote (from the app's text extraction) match the
 * rendered text (from PDF.js's text layer, a *different* extraction): the two
 * disagree on spacing (PDF.js joins words at span/line boundaries), hyphenated
 * line breaks, punctuation, and ligatures — but agree on the letters. For a
 * sentence-length quote the alphanumeric run is still unique, so false matches
 * are not a practical risk.
 */
function normalizeForMatch(text: string): string {
	return text.normalize('NFKD').toLowerCase().replace(/[^a-z0-9]+/g, '');
}

/**
 * Same alphanumeric reduction as `normalizeForMatch`, but records, for each kept
 * character in the normalized string, the index of the ORIGINAL character it
 * came from — so a match in normalized space maps back to original offsets.
 */
function normalizeWithIndexMap(text: string): { normalized: string; indexMap: number[] } {
	let normalized = '';
	const indexMap: number[] = [];

	for (let i = 0; i < text.length; i++) {
		// A single source char may decompose to several (e.g. "ﬁ" → "f","i");
		// each kept output char maps back to this same original index.
		const decomposed = text[i].normalize('NFKD').toLowerCase();
		for (const ch of decomposed) {
			if (ch >= 'a' && ch <= 'z') {
				normalized += ch;
				indexMap.push(i);
			} else if (ch >= '0' && ch <= '9') {
				normalized += ch;
				indexMap.push(i);
			}
		}
	}

	return { normalized, indexMap };
}
