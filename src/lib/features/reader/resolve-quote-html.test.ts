import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveQuoteInText } from './resolve-quote-html.ts';

test('exact match returns correct offsets', () => {
	const fullText = 'The quick brown fox jumps over the lazy dog.';
	const quote = 'brown fox jumps';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result);
	assert.equal(fullText.slice(result!.start, result!.end), quote);
});

test('whitespace-normalized match: extra spaces and a newline in source vs single-spaced quote', () => {
	const fullText = 'The quick brown   fox\njumps over the lazy dog.';
	const quote = 'brown fox jumps';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result, 'expected a match');
	const matchedOriginal = fullText.slice(result!.start, result!.end);
	// The matched original-text slice, once normalized, should equal the
	// normalized quote.
	assert.equal(matchedOriginal.replace(/\s+/g, ' ').trim(), quote);
});

test('PDF.js span concatenation: words joined with no separators still match', () => {
	// PDF.js text layers often join runs with no space and use ligatures; the
	// model quotes normally-spaced text. The alphanumeric matcher bridges both.
	const fullText = 'DeepLearning(DL)modelsareﬁnebutlarge';
	const quote = 'Deep Learning (DL) models are fine but large';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result, 'expected a match despite missing spaces + a ligature');
	assert.equal(fullText.slice(result!.start, result!.end), 'DeepLearning(DL)modelsareﬁnebutlarge');
});

test('hyphenated line break in source matches an un-hyphenated quote (PDF case)', () => {
	// PDFs break words across lines with a hyphen; the model quotes the joined word.
	const fullText = 'exhibit funda-\nmental limitations to fit these models';
	const quote = 'fundamental limitations';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result, 'expected a match across the hyphenated break');
	// The matched original span covers the hyphenated word through "limitations".
	const matched = fullText.slice(result!.start, result!.end);
	assert.ok(matched.includes('funda-'), `matched span should include the hyphen: ${matched}`);
	assert.ok(matched.includes('limitations'));
});

test('case-insensitive fuzzy match', () => {
	const fullText = 'Zero Redundancy Optimizer eliminates memory redundancies.';
	const quote = 'zero redundancy optimizer';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result, 'expected a case-insensitive match');
	assert.equal(fullText.slice(result!.start, result!.end), 'Zero Redundancy Optimizer');
});

test('multi-line source with quote spanning a paragraph break', () => {
	const fullText = [
		'Paragraph one starts here.',
		'',
		'It continues onto a second line',
		'and wraps into a third line before ending.'
	].join('\n');
	const quote = 'It continues onto a second line and wraps into a third line';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result, 'expected a match across the newline-joined lines');
	const matchedOriginal = fullText.slice(result!.start, result!.end);
	assert.equal(matchedOriginal.replace(/\s+/g, ' ').trim(), quote);
});

test('not found returns null', () => {
	const fullText = 'The quick brown fox jumps over the lazy dog.';
	const quote = 'this text does not appear anywhere';
	const result = resolveQuoteInText(fullText, quote);
	assert.equal(result, null);
});

test('empty (or whitespace-only) quote returns null', () => {
	const fullText = 'The quick brown fox jumps over the lazy dog.';
	assert.equal(resolveQuoteInText(fullText, ''), null);
	assert.equal(resolveQuoteInText(fullText, '   \n\t  '), null);
});

test('quote with leading/trailing whitespace is trimmed before exact match', () => {
	const fullText = 'The quick brown fox jumps over the lazy dog.';
	const quote = '  brown fox jumps  ';
	const result = resolveQuoteInText(fullText, quote);
	assert.ok(result);
	assert.equal(fullText.slice(result!.start, result!.end), quote.trim());
});
