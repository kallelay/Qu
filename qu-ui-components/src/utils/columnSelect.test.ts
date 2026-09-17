import { describe, expect, it } from 'vitest';
import { columnSelectionRanges } from './columnSelect';

// A document with one deliberately short line, since that is where the
// interesting behaviour is:
//   1: abcdefghij   (10 chars -> max column 11)
//   2: klmnopqrst   (10 chars -> max column 11)
//   3: uv           ( 2 chars -> max column  3)
//   4: wxyzABCDEF   (10 chars -> max column 11)
const MAX = [11, 11, 3, 11];
const lineMaxColumn = (n: number) => MAX[n - 1];

describe('columnSelectionRanges', () => {
  it('produces one range per line spanned', () => {
    const r = columnSelectionRanges({ lineNumber: 1, column: 3 }, { lineNumber: 4, column: 7 }, lineMaxColumn);
    expect(r.map((x) => x.lineNumber)).toEqual([1, 2, 3, 4]);
  });

  it('gives every full-length line the same column span', () => {
    const r = columnSelectionRanges({ lineNumber: 1, column: 3 }, { lineNumber: 4, column: 7 }, lineMaxColumn);
    expect(r[0]).toEqual({ lineNumber: 1, startColumn: 3, endColumn: 7 });
    expect(r[1]).toEqual({ lineNumber: 2, startColumn: 3, endColumn: 7 });
    expect(r[3]).toEqual({ lineNumber: 4, startColumn: 3, endColumn: 7 });
  });

  it('clamps a line too short to reach the far edge', () => {
    // Line 3 ends at column 3, so a rectangle spanning columns 3..7
    // overhangs it. Without clamping this range would claim text past the
    // end of the line.
    const r = columnSelectionRanges({ lineNumber: 1, column: 3 }, { lineNumber: 4, column: 7 }, lineMaxColumn);
    expect(r[2]).toEqual({ lineNumber: 3, startColumn: 3, endColumn: 3 });
  });

  it('collapses a line shorter than BOTH edges to an empty range', () => {
    // Columns 5..9 are both past line 3's end; it contributes nothing
    // rather than a phantom selection.
    const r = columnSelectionRanges({ lineNumber: 2, column: 5 }, { lineNumber: 4, column: 9 }, lineMaxColumn);
    expect(r[1]).toEqual({ lineNumber: 3, startColumn: 3, endColumn: 3 });
  });

  it('handles an upward drag, spanning the same lines', () => {
    const up = columnSelectionRanges({ lineNumber: 4, column: 7 }, { lineNumber: 1, column: 3 }, lineMaxColumn);
    expect(up.map((x) => x.lineNumber)).toEqual([1, 2, 3, 4]);
  });

  it('keeps the caret on the edge the mouse is on when dragging leftwards', () => {
    // Dragging right-to-left, the caret (endColumn) must end up on the
    // LEFT edge -- the same way an ordinary selection's caret follows the
    // mouse rather than jumping to the other end.
    const r = columnSelectionRanges({ lineNumber: 1, column: 8 }, { lineNumber: 2, column: 2 }, lineMaxColumn);
    expect(r[0]).toEqual({ lineNumber: 1, startColumn: 8, endColumn: 2 });
    expect(r[1]).toEqual({ lineNumber: 2, startColumn: 8, endColumn: 2 });
  });

  it('decides direction ONCE, not per line', () => {
    // The regression this guards: a right-to-left drag whose columns both
    // clamp to the same value on a short line. Deciding direction per line
    // from the clamped numbers would make that line the only one facing
    // the other way, and Monaco would put its caret on the wrong edge.
    const r = columnSelectionRanges({ lineNumber: 1, column: 9 }, { lineNumber: 3, column: 5 }, lineMaxColumn);
    expect(r[2]).toEqual({ lineNumber: 3, startColumn: 3, endColumn: 3 });
    // ...and the unclamped lines still face left.
    expect(r[0].startColumn).toBeGreaterThan(r[0].endColumn);
  });

  it('returns a single collapsed range before the mouse has moved', () => {
    // The mouse-down instant: anchor === target. Must be one empty range,
    // not zero ranges (Monaco rejects an empty selection array).
    const r = columnSelectionRanges({ lineNumber: 2, column: 4 }, { lineNumber: 2, column: 4 }, lineMaxColumn);
    expect(r).toHaveLength(1);
    expect(r[0]).toEqual({ lineNumber: 2, startColumn: 4, endColumn: 4 });
  });

  it('never returns an empty array', () => {
    for (const [a, b] of [[1, 1], [1, 4], [4, 1], [3, 3]] as const) {
      const r = columnSelectionRanges({ lineNumber: a, column: 2 }, { lineNumber: b, column: 6 }, lineMaxColumn);
      expect(r.length).toBeGreaterThan(0);
    }
  });
});
