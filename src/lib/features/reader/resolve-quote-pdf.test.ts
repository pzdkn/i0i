import { test } from 'node:test';
import assert from 'node:assert/strict';
import { coveringRectsForQuote, type PdfTextItem } from './resolve-quote-pdf.ts';

function rect(x: number) {
	return { x, y: 0, width: 0.1, height: 0.02 };
}

test('quote spanning two adjacent items returns both items rects', () => {
	const items: PdfTextItem[] = [
		{ str: 'The quick brown', rect: rect(0) },
		{ str: 'fox jumps over', rect: rect(0.1) },
		{ str: 'the lazy dog.', rect: rect(0.2) }
	];
	const rects = coveringRectsForQuote(items, 'brown fox jumps');
	assert.ok(rects);
	assert.deepEqual(rects, [rect(0), rect(0.1)]);
});

test('single-item quote returns just that items rect', () => {
	const items: PdfTextItem[] = [
		{ str: 'The quick brown', rect: rect(0) },
		{ str: 'fox jumps over', rect: rect(0.1) },
		{ str: 'the lazy dog.', rect: rect(0.2) }
	];
	const rects = coveringRectsForQuote(items, 'fox jumps over');
	assert.ok(rects);
	assert.deepEqual(rects, [rect(0.1)]);
});

test('multi-item quote with whitespace variance across items', () => {
	const items: PdfTextItem[] = [
		{ str: 'The quick   brown', rect: rect(0) },
		{ str: '\nfox jumps', rect: rect(0.1) },
		{ str: 'over the lazy', rect: rect(0.2) },
		{ str: 'dog.', rect: rect(0.3) }
	];
	const rects = coveringRectsForQuote(items, 'brown fox jumps over');
	assert.ok(rects);
	assert.deepEqual(rects, [rect(0), rect(0.1), rect(0.2)]);
});

test('quote spanning three items unions all three rects', () => {
	const items: PdfTextItem[] = [
		{ str: 'Alpha', rect: rect(0) },
		{ str: 'beta', rect: rect(0.1) },
		{ str: 'gamma', rect: rect(0.2) },
		{ str: 'delta', rect: rect(0.3) }
	];
	const rects = coveringRectsForQuote(items, 'beta gamma delta');
	assert.ok(rects);
	assert.deepEqual(rects, [rect(0.1), rect(0.2), rect(0.3)]);
});

test('not-found quote returns null', () => {
	const items: PdfTextItem[] = [
		{ str: 'The quick brown', rect: rect(0) },
		{ str: 'fox jumps over', rect: rect(0.1) }
	];
	const rects = coveringRectsForQuote(items, 'this text does not appear anywhere');
	assert.equal(rects, null);
});

test('empty items array returns null', () => {
	const rects = coveringRectsForQuote([], 'anything');
	assert.equal(rects, null);
});
