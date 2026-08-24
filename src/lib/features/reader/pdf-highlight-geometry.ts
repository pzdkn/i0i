/**
 * Projects precise PDF selection rectangles into calmer display geometry.
 *
 * Stored anchors remain untouched. This module only derives temporary visual
 * bands, using line-height-relative tolerances so the result is stable at any
 * reader zoom level.
 */

import type { PdfRect } from "../../domain/reader.ts";

export type HighlightProjectionOptions = {
  /** Require the input to fit the normalized PDF page coordinate space. */
  validateNormalized?: boolean;
};

const MIN_VERTICAL_OVERLAP = 0.6;
const MAX_CENTER_OFFSET_IN_LINE_HEIGHTS = 0.5;
const MAX_GAP_IN_LINE_HEIGHTS = 0.75;
const MIN_HORIZONTAL_ASPECT_RATIO = 0.75;

type Line = {
  rects: PdfRect[];
};

/** Returns conservative visual bands without modifying the stored rectangles. */
export function projectPdfHighlightRects(
  rects: readonly PdfRect[],
  options: HighlightProjectionOptions = {},
): PdfRect[] {
  const validateNormalized = options.validateNormalized ?? true;
  const validRects = rects
    .filter((rect) => isValidRect(rect, validateNormalized))
    .map((rect) => ({ ...rect }))
    .sort(compareRects);

  const horizontalRects = validRects.filter(isHorizontalBand);
  const passthroughRects = validRects.filter((rect) => !isHorizontalBand(rect));
  const lines: Line[] = [];

  for (const rect of horizontalRects) {
    const line = lines.find((candidate) => belongsToLine(rect, candidate));
    if (line) {
      line.rects.push(rect);
    } else {
      lines.push({ rects: [rect] });
    }
  }

  const projected = lines.flatMap(({ rects: lineRects }) => mergeLine(lineRects));
  return [...projected, ...passthroughRects].sort(compareRects);
}

function isValidRect(rect: PdfRect, validateNormalized: boolean): boolean {
  if (!rect || typeof rect !== "object") {
    return false;
  }
  const values = [rect.x, rect.y, rect.width, rect.height];
  if (!values.every(Number.isFinite) || rect.width <= 0 || rect.height <= 0) {
    return false;
  }
  if (!validateNormalized) {
    return true;
  }
  return rect.x >= 0 && rect.y >= 0 && rect.x + rect.width <= 1 && rect.y + rect.height <= 1;
}

function isHorizontalBand(rect: PdfRect): boolean {
  return rect.width / rect.height >= MIN_HORIZONTAL_ASPECT_RATIO;
}

function belongsToLine(rect: PdfRect, line: Line): boolean {
  return line.rects.some((candidate) => {
    const overlap = Math.min(bottom(rect), bottom(candidate)) - Math.max(rect.y, candidate.y);
    const overlapRatio = Math.max(0, overlap) / Math.min(rect.height, candidate.height);
    const centerOffset = Math.abs(centerY(rect) - centerY(candidate));
    const lineHeight = Math.max(rect.height, candidate.height);
    return (
      overlapRatio >= MIN_VERTICAL_OVERLAP &&
      centerOffset <= lineHeight * MAX_CENTER_OFFSET_IN_LINE_HEIGHTS
    );
  });
}

function mergeLine(rects: PdfRect[]): PdfRect[] {
  const sorted = [...rects].sort((left, right) => left.x - right.x);
  const merged: PdfRect[] = [];

  for (const rect of sorted) {
    const previous = merged.at(-1);
    if (!previous || rect.x - right(previous) > Math.max(previous.height, rect.height) * MAX_GAP_IN_LINE_HEIGHTS) {
      merged.push({ ...rect });
      continue;
    }

    const left = Math.min(previous.x, rect.x);
    const top = Math.min(previous.y, rect.y);
    const mergedRight = Math.max(right(previous), right(rect));
    const mergedBottom = Math.max(bottom(previous), bottom(rect));
    merged[merged.length - 1] = {
      x: left,
      y: top,
      width: mergedRight - left,
      height: mergedBottom - top,
    };
  }

  return merged;
}

function compareRects(left: PdfRect, right: PdfRect): number {
  return left.y - right.y || left.x - right.x;
}

function right(rect: PdfRect): number {
  return rect.x + rect.width;
}

function bottom(rect: PdfRect): number {
  return rect.y + rect.height;
}

function centerY(rect: PdfRect): number {
  return rect.y + rect.height / 2;
}
