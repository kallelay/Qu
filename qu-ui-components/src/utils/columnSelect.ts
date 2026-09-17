/**
 * The geometry of a column (block) selection, separated from the mouse
 * plumbing that drives it.
 *
 * Monaco has column selection on Shift+Alt+drag but **no middle-button
 * binding of any kind** -- there is no option to switch on, so the gesture
 * has to be built. The event half of that is a few lines in
 * `CodeEditor.tsx`; this is the half with decisions in it, and it lives
 * here for the same reason `cells.ts` does: it is framework-free (no
 * Monaco, no React) and can be tested directly.
 *
 * That split is not academic. Monaco's `onMouseMove` cannot be driven by
 * synthetic events, so a browser-level test of the drag can press the
 * button but cannot move it -- the rectangle logic is exactly the part a
 * live test could not reach, and exactly the part with edge cases.
 */

/** One line's worth of a block selection. `endColumn` is the caret end. */
export interface ColumnRange {
  lineNumber: number;
  startColumn: number;
  endColumn: number;
}

export interface ColumnPoint {
  lineNumber: number;
  column: number;
}

/**
 * The per-line ranges a block selection covers, dragging from `anchor` to
 * `target`.
 *
 * `lineMaxColumn` reports a line's own maximum column (Monaco's
 * `model.getLineMaxColumn`, 1-based and one past the last character).
 * Columns are clamped to it per line, because a rectangle dragged across
 * ragged text overhangs the short lines: Monaco's native column select
 * puts the caret in virtual space out there, but a selection in virtual
 * space would report text that does not exist. Clamping means a short line
 * contributes a short -- possibly empty -- range instead, which is what
 * the same gesture does in other editors.
 *
 * Works in every drag direction, including upward and right-to-left; the
 * caret (`endColumn`) stays on the edge the mouse is on, matching how an
 * ordinary selection behaves.
 */
export function columnSelectionRanges(
  anchor: ColumnPoint,
  target: ColumnPoint,
  lineMaxColumn: (lineNumber: number) => number
): ColumnRange[] {
  const top = Math.min(anchor.lineNumber, target.lineNumber);
  const bottom = Math.max(anchor.lineNumber, target.lineNumber);
  // Direction is decided from the UNCLAMPED columns, once, for the whole
  // block. Deciding it per line from clamped values would flip the caret
  // to the other edge on any line short enough for both columns to clamp
  // to the same place.
  const rightToLeft = target.column < anchor.column;

  const ranges: ColumnRange[] = [];
  for (let lineNumber = top; lineNumber <= bottom; lineNumber++) {
    const maxColumn = lineMaxColumn(lineNumber);
    const a = Math.min(anchor.column, maxColumn);
    const b = Math.min(target.column, maxColumn);
    const low = Math.min(a, b);
    const high = Math.max(a, b);
    ranges.push(
      rightToLeft
        ? { lineNumber, startColumn: high, endColumn: low }
        : { lineNumber, startColumn: low, endColumn: high }
    );
  }
  return ranges;
}
