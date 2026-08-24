import assert from "node:assert/strict";
import { test } from "node:test";
import type { PdfRect } from "../../domain/reader.ts";
import { projectPdfHighlightRects } from "./pdf-highlight-geometry.ts";

function rect(x: number, y: number, width: number, height = 0.02): PdfRect {
  return { x, y, width, height };
}

test("merges nearby fragments on the same line", () => {
  const projected = projectPdfHighlightRects([
    rect(0.24, 0.1, 0.12),
    rect(0.1, 0.101, 0.13),
  ]);

  assert.equal(projected.length, 1);
  assert.deepEqual(
    { x: projected[0].x, y: projected[0].y, width: projected[0].width },
    { x: 0.1, y: 0.1, width: 0.26 },
  );
  assert.ok(Math.abs(projected[0].height - 0.021) < Number.EPSILON * 4);
});

test("keeps separate lines as separate bands", () => {
  const projected = projectPdfHighlightRects([
    rect(0.1, 0.1, 0.25),
    rect(0.1, 0.13, 0.3),
  ]);

  assert.deepEqual(projected, [rect(0.1, 0.1, 0.25), rect(0.1, 0.13, 0.3)]);
});

test("does not bridge columns or large gaps", () => {
  const projected = projectPdfHighlightRects([
    rect(0.08, 0.1, 0.18),
    rect(0.58, 0.1, 0.2),
    rect(0.4, 0.16, 0.1),
    rect(0.1, 0.16, 0.1),
  ]);

  assert.deepEqual(projected, [
    rect(0.08, 0.1, 0.18),
    rect(0.58, 0.1, 0.2),
    rect(0.1, 0.16, 0.1),
    rect(0.4, 0.16, 0.1),
  ]);
});

test("rejects malformed rectangles without changing the input", () => {
  const input: PdfRect[] = [
    rect(0.1, 0.1, 0.2),
    rect(-0.1, 0.2, 0.1),
    rect(0.2, 0.2, 0),
    rect(0.9, 0.2, 0.2),
    { x: Number.NaN, y: 0.2, width: 0.1, height: 0.02 },
    null as unknown as PdfRect,
  ];
  const snapshot = structuredClone(input);

  assert.deepEqual(projectPdfHighlightRects(input), [rect(0.1, 0.1, 0.2)]);
  assert.deepEqual(input, snapshot);
});

test("passes tall rotated-looking rectangles through without merging", () => {
  const projected = projectPdfHighlightRects([
    rect(0.1, 0.1, 0.01, 0.08),
    rect(0.115, 0.1, 0.01, 0.08),
  ]);

  assert.deepEqual(projected, [
    rect(0.1, 0.1, 0.01, 0.08),
    rect(0.115, 0.1, 0.01, 0.08),
  ]);
});

test("projection is independent of coordinate scale", () => {
  const input = [rect(0.1, 0.1, 0.1), rect(0.205, 0.101, 0.08)];
  const factor = 4;
  const scaled = input.map(({ x, y, width, height }) => ({
    x: x * factor,
    y: y * factor,
    width: width * factor,
    height: height * factor,
  }));
  const scaleDown = ({ x, y, width, height }: PdfRect): PdfRect => ({
    x: x / factor,
    y: y / factor,
    width: width / factor,
    height: height / factor,
  });

  assert.deepEqual(
    projectPdfHighlightRects(scaled, { validateNormalized: false }).map(scaleDown),
    projectPdfHighlightRects(input),
  );
});
