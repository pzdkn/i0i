import { test } from 'node:test';
import assert from 'node:assert/strict';
import { findTextMatches } from './find-matches.ts';

test('finds all case-insensitive occurrences', () => {
	const text = 'The Fox and the fox and the FOX.';
	const matches = findTextMatches(text, 'fox');
	assert.equal(matches.length, 3);
	for (const m of matches) {
		assert.equal(text.slice(m.start, m.end).toLowerCase(), 'fox');
	}
});

test('returns correct offsets', () => {
	const text = 'alpha beta gamma';
	const matches = findTextMatches(text, 'beta');
	assert.deepEqual(matches, [{ start: 6, end: 10 }]);
});

test('non-overlapping matches', () => {
	const matches = findTextMatches('aaaa', 'aa');
	assert.deepEqual(matches, [
		{ start: 0, end: 2 },
		{ start: 2, end: 4 },
	]);
});

test('empty / whitespace query matches nothing', () => {
	assert.deepEqual(findTextMatches('anything', ''), []);
	assert.deepEqual(findTextMatches('anything', '   '), []);
});

test('no match returns empty', () => {
	assert.deepEqual(findTextMatches('hello world', 'xyz'), []);
});
