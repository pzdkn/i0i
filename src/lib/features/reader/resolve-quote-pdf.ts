/**
 * Resolves an agent-provided verbatim quote to a union of PDF page rects,
 * using PDF.js text-layer items (one entry per text run on the page, in
 * reading order).
 *
 * `coveringRectsForQuote` is the pure, unit-tested core exported here. Takes
 * a flat array of `{ str, rect }` items (extracted from a rendered page's
 * text layer) and a quote, and returns the union of rects for the items the
 * quote's matched span touches. `PdfPage.svelte` extracts the per-page
 * `PdfTextItem[]` and calls this directly to resolve agent highlight
 * intents (RFC 0059 Phase 2 / Task 8).
 *
 * String matching itself is delegated to `resolveQuoteInText` (char-span
 * exact + whitespace-normalized matching) — this module only builds the
 * page string / index map and maps the resulting span back to rects.
 */

import { resolveQuoteInText } from './resolve-quote-html.ts';

export type Rect = {
	x: number;
	y: number;
	width: number;
	height: number;
};

export type PdfTextItem = {
	str: string;
	rect: Rect;
};

/**
 * Given a page's text-layer items and a verbatim quote, finds the quote's
 * character span in the concatenated page string (items joined by a single
 * space) and returns the union of rects for every item the span touches.
 *
 * Returns `null` if the quote can't be located (see `resolveQuoteInText`),
 * or if the located span somehow touches no items (defensive; shouldn't
 * happen given how the index map is built).
 */
export function coveringRectsForQuote(items: PdfTextItem[], quote: string): Rect[] | null {
	if (items.length === 0) {
		return null;
	}

	// Concatenate item strings with a single-space joiner, building a map
	// from each character position in the page string to the item index it
	// came from. The joiner space (between items) is attributed to the
	// preceding item, matching how whitespace is treated at the end of an
	// item's own text.
	let pageString = '';
	const charItemIndex: number[] = [];

	for (let i = 0; i < items.length; i++) {
		if (i > 0) {
			pageString += ' ';
			charItemIndex.push(i - 1);
		}
		const { str } = items[i];
		for (let j = 0; j < str.length; j++) {
			pageString += str[j];
			charItemIndex.push(i);
		}
	}

	const span = resolveQuoteInText(pageString, quote);
	if (!span) {
		return null;
	}

	const { start, end } = span;
	const coveredIndices = new Set<number>();
	const clampedEnd = Math.min(end, charItemIndex.length);
	for (let pos = Math.max(start, 0); pos < clampedEnd; pos++) {
		coveredIndices.add(charItemIndex[pos]);
	}

	if (coveredIndices.size === 0) {
		return null;
	}

	return Array.from(coveredIndices)
		.sort((a, b) => a - b)
		.map((index) => items[index].rect);
}
