import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { ContextCitation } from '../../domain/context.ts';
import { citationLabel, splitCitedAnswer } from './cited-answer.ts';

function citation(handle: string, overrides: Partial<ContextCitation> = {}): ContextCitation {
  return {
    handle,
    itemId: `item-${handle}`,
    paperId: "vaswani2017",
    pageStart: 2,
    headingPath: null,
    chunkId: `chunk-${handle}`,
    rectsJson: "[]",
    ...overrides,
  };
}

test("an answer with no citations comes back as one literal piece", () => {
  const pieces = splitCitedAnswer("Scaling stabilizes gradients.", []);
  assert.deepEqual(pieces, [{ text: "Scaling stabilizes gradients.", citation: null }]);
});

test("known markers become citation pieces, prose around them survives", () => {
  const pieces = splitCitedAnswer("They scale by 1/sqrt(d) [C1] to keep softmax sharp.", [
    citation("C1"),
  ]);

  assert.deepEqual(
    pieces.map((piece) => piece.text),
    ["They scale by 1/sqrt(d) ", "C1", " to keep softmax sharp."],
  );
  assert.equal(pieces[1].citation?.handle, "C1");
  assert.equal(pieces[0].citation, null);
});

test("a marker the assembly never minted stays literal text", () => {
  // The model can cite a passage it was never given. A button that jumps
  // nowhere is worse than the characters it replaced.
  const pieces = splitCitedAnswer("As shown in [C9], it works.", [citation("C1")]);
  assert.deepEqual(pieces, [{ text: "As shown in [C9], it works.", citation: null }]);
});

test("adjacent markers each become their own piece", () => {
  const pieces = splitCitedAnswer("Both agree [C1][C2].", [citation("C1"), citation("C2")]);
  assert.deepEqual(
    pieces.map((piece) => piece.text),
    ["Both agree ", "C1", "C2", "."],
  );
  assert.equal(pieces[1].citation?.handle, "C1");
  assert.equal(pieces[2].citation?.handle, "C2");
});

test("markers at the very start and very end are not dropped", () => {
  const pieces = splitCitedAnswer("[C1] is the claim [C1]", [citation("C1")]);
  assert.deepEqual(
    pieces.map((piece) => piece.text),
    ["C1", " is the claim ", "C1"],
  );
});

test("pages are labelled 1-based, since storage counts from zero", () => {
  assert.equal(citationLabel(citation("C1", { pageStart: 0 })), "p1");
});

test("the section is included when the chunk has one", () => {
  assert.equal(
    citationLabel(citation("C1", { headingPath: "Introduction > Motivation" })),
    "p3 \u00b7 Introduction > Motivation",
  );
});
