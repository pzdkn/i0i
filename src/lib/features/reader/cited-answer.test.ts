import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { ContextCitation } from '../../domain/context.ts';
import { citationLabel } from './cited-answer.ts';

function citation(handle: string, overrides: Partial<ContextCitation> = {}): ContextCitation {
  return {
    handle,
    itemId: `item-${handle}`,
    paperId: "vaswani2017",
    pageStart: 2,
    headingPath: null,
    chunkId: `chunk-${handle}`,
    preview: "We divide by sqrt(d_k).",
    rectsJson: "[]",
    ...overrides,
  };
}

test("pages are labelled 1-based, since storage counts from zero", () => {
  assert.equal(citationLabel(citation("1", { pageStart: 0 })), "p1");
});

test("the section is included when the chunk has one", () => {
  assert.equal(
    citationLabel(citation("1", { headingPath: "Introduction > Motivation" })),
    "p3 \u00b7 Introduction > Motivation",
  );
});
