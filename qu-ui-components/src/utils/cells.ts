/**
 * Jupyter/VS-Code/MATLAB-style `#%%` cell parsing for the Qu editor.
 *
 * A line that matches `^#%%.*$` starts a new logical "cell"; everything up
 * to (but not including) the next such marker line -- or the end of the
 * file -- belongs to that cell. Text before the *first* marker (if any)
 * still forms an implicit, untitled leading cell, matching how VS Code's
 * own `# %%` cells behave for a file that starts without a marker.
 *
 * This module is deliberately framework-free (no Monaco, no React) so the
 * parsing and state-persistence logic can be unit tested in isolation from
 * the editor widget that consumes it.
 */

export interface QuCell {
  /** 0-based position of this cell among all cells parsed from the file. */
  index: number;
  /**
   * Text following the `#%%` marker on its own line, trimmed. Empty for a
   * bare `#%%` marker, and empty for the implicit leading cell that exists
   * when the file has content before its first marker (or no marker at
   * all).
   */
  title: string;
  /**
   * 1-based, inclusive. For a marked cell this is the marker line itself
   * (so UI affordances -- CodeLens, gutter decorations -- can anchor to
   * it). For the implicit leading cell, this is line 1.
   */
  startLine: number;
  /**
   * 1-based, inclusive: the last line belonging to this cell -- the line
   * immediately before the next `#%%` marker, or the file's last line.
   */
  endLine: number;
  /**
   * Raw source text of lines [startLine, endLine], joined with '\n'.
   * Includes the `#%%` marker line itself where present: it's just a `#`
   * comment to the Qu interpreter, so re-running it as part of the cell's
   * code is harmless.
   */
  code: string;
}

const CELL_MARKER_RE = /^#%%(.*)$/;

function splitLines(source: string): string[] {
  return source.split(/\r\n|\r|\n/);
}

/** Parse `source` into an ordered list of `#%%`-delimited cells. */
export function parseCells(source: string): QuCell[] {
  const lines = splitLines(source);

  const markerLineIndices: number[] = [];
  lines.forEach((line, i) => {
    if (CELL_MARKER_RE.test(line)) markerLineIndices.push(i);
  });

  // Always start scanning from line 0. If line 0 is itself a marker this
  // is a no-op (dedup below); otherwise it introduces the implicit leading
  // cell that covers whatever precedes the first real marker.
  const boundaries =
    markerLineIndices[0] === 0 ? markerLineIndices : [0, ...markerLineIndices];

  return boundaries.map((startIdx, i) => {
    const endIdx =
      i + 1 < boundaries.length ? boundaries[i + 1] - 1 : lines.length - 1;
    const marker = lines[startIdx].match(CELL_MARKER_RE);
    return {
      index: i,
      title: marker ? marker[1].trim() : '',
      startLine: startIdx + 1,
      endLine: endIdx + 1,
      code: lines.slice(startIdx, endIdx + 1).join('\n'),
    };
  });
}

/** Find the cell that contains the given 1-based line number, if any. */
export function findCellAtLine(
  cells: QuCell[],
  lineNumber: number
): QuCell | undefined {
  return cells.find((c) => lineNumber >= c.startLine && lineNumber <= c.endLine);
}

/**
 * Build the source to actually execute for "Run Cell" on `cells[cellIndex]`.
 *
 * State-persistence decision (documented here, and in BACKLOG.md, because
 * it's an easy semantic to get subtly wrong): `execute_code` spawns a fresh
 * `qu.exe run <tempfile>` process per call -- there is no persistent Qu
 * REPL process backing the editor. So "run cell 2" cannot mean "send just
 * cell 2's text to an already-warm process that remembers cell 1's
 * variables" the way a real Jupyter kernel would.
 *
 * Instead, running cell N concatenates and re-executes cells [0..N]
 * (inclusive) as a single fresh script. This is simple and *correct* for
 * variable visibility (cell 2 really does see cell 1's variables, because
 * cell 1's code just ran again immediately before it, in the same
 * process) but it also means any side effects in earlier cells -- prints,
 * plots -- re-fire every time a later cell runs. That trade-off is
 * accepted for this phase; true incremental (no re-execution) state would
 * require driving a long-lived `qu repl` process over stdin instead, which
 * is a materially bigger backend change than this pass makes.
 */
export function getPrefixSource(
  source: string,
  cells: QuCell[],
  cellIndex: number
): string {
  if (cellIndex < 0 || cellIndex >= cells.length) {
    throw new RangeError(`cellIndex ${cellIndex} out of range for ${cells.length} cells`);
  }
  const lines = splitLines(source);
  const endLine = cells[cellIndex].endLine; // 1-based inclusive
  return lines.slice(0, endLine).join('\n');
}
