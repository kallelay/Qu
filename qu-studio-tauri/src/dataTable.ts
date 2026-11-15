// Shared, pure TypeScript for every data-input surface of Qu Studio (the Fit
// tab and the ML lab): reading delimited text into a grid of cells, deciding
// the separator, the decimal mark and whether there is a header, typing a
// column, per-column statistics, and the rule that decides when a table is
// too big to be written into a script as a literal. No React, no Tauri.
//
// `curveFitData.ts` and `mlLabData.ts` build their own table shapes ON this
// (the Fit tab wants numeric columns, the ML lab wants typed/categorical
// columns); `dataTable.test.ts` pins every edge case below.

// ------------------------------------------------------------ inline vs file

/** Above this many data rows a table goes through a temp CSV that the script
 *  reads with read_csv, instead of an inline literal in the script. */
export const INLINE_ROW_LIMIT = 200;

/** Must a table of `rows` data rows be staged through a file? */
export const needsStaging = (rows: number): boolean => rows > INLINE_ROW_LIMIT;

// ------------------------------------------------------------------- cells

const MISSING = new Set(['', 'na', 'n/a', 'nan', '?', 'null', 'none', '-', '--', '.', 'nil', 'missing']);
/** A cell that stands for "no value" (blank, NA, NaN, ?, ...). */
export const isMissingToken = (s: string): boolean => MISSING.has(s.trim().toLowerCase());

const PLAIN_NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
const GROUPED_NUMBER = /^[+-]?\d{1,3}(\.\d{3})+(,\d*)?([eE][+-]?\d+)?$/;

/** A cell as a finite number, or null. With `decimal = ','` the comma is the
 *  decimal mark and dots that group thousands (1.234,5) are dropped; a cell
 *  like 1.5 that cannot be a thousands group is still read as 1.5. Hex, "inf"
 *  and stray text are not numbers. */
export function toNumber(cell: string, decimal = '.'): number | null {
  let s = cell.trim();
  if (s === '') return null;
  if (decimal === ',') {
    if (GROUPED_NUMBER.test(s)) s = s.replace(/\./g, '').replace(',', '.');
    else if (!s.includes('.')) s = s.replace(',', '.');
  }
  if (!PLAIN_NUMBER.test(s)) return null;
  const v = Number(s);
  return Number.isFinite(v) ? v : null;
}

/** How Qu prints a number: 1.0 -> "1". Used for the levels of a category
 *  column whose cells are all numbers (so they match str(x) in the script). */
export function quNumberText(v: number): string {
  return String(Number(v.toPrecision(6)));
}

/** A number with `digits` significant digits and no trailing zeros; 'n/a' when it is not finite. */
export const formatSig = (v: number, digits = 6): string => (Number.isFinite(v) ? String(Number(v.toPrecision(digits))) : 'n/a');

// ----------------------------------------------------------------- splitting

/** Splits one line on `d`, honouring double quotes (a quoted field may hold
 *  the separator and `""` for a quote). A space `d` means "any whitespace,
 *  runs collapsed, ends ignored". Cells come back trimmed. */
export function splitLine(line: string, d: string): string[] {
  if (d === ' ') line = line.trim();
  const out: string[] = [];
  let cur = '';
  let q = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q) {
      if (c === '"') {
        if (line[i + 1] === '"') {
          cur += '"';
          i++;
        } else q = false;
      } else cur += c;
    } else if (c === '"' && cur.trim() === '') {
      q = true;
      cur = '';
    } else if (d === ' ' ? /\s/.test(c) : c === d) {
      if (d === ' ' && cur === '' && out.length > 0 && /\s/.test(line[i - 1] ?? '')) continue; // runs of spaces
      out.push(cur.trim());
      cur = '';
    } else cur += c;
  }
  out.push(cur.trim());
  return out;
}

/** Lines that carry data: not blank, not a `#` comment. A BOM and CRLF are handled here. */
export function contentLines(text: string): string[] {
  return text
    .replace(/^﻿/, '')
    .split(/\r?\n/)
    .filter((l) => l.trim().length > 0 && !l.trim().startsWith('#'));
}

/** The separator of `text`: tab, `;`, `,` or `|` (whichever splits the first
 *  lines most consistently, ties going in that order so a tab- or
 *  semicolon-separated file whose cells hold decimal commas is not read as
 *  comma-separated), else a space meaning "whitespace". */
export function detectDelimiter(text: string): string {
  const lines = contentLines(text).slice(0, 10);
  if (!lines.length) return ',';
  let best = '';
  let bestScore = -1;
  for (const d of ['\t', ';', ',', '|']) {
    const counts = lines.map((l) => splitLine(l, d).length);
    const first = counts[0];
    if (first < 2) continue;
    const consistent = counts.filter((c) => c === first).length / counts.length;
    if (consistent > bestScore) {
      bestScore = consistent;
      best = d;
    }
  }
  if (best) return best;
  return /\s/.test(lines[0].trim()) ? ' ' : ',';
}

/** The decimal mark of a grid. With a comma separator it is a point; otherwise
 *  a comma wins when more cells look like 1,5 than like 1.5. */
export function detectDecimal(grid: string[][], sep: string): string {
  if (sep === ',') return '.';
  let commaNums = 0;
  let dotNums = 0;
  for (const r of grid.slice(0, 50)) {
    for (const c of r) {
      const t = c.trim();
      if (/^[+-]?\d*,\d+([eE][+-]?\d+)?$/.test(t)) commaNums++;
      else if (/^[+-]?\d*\.\d+([eE][+-]?\d+)?$/.test(t)) dotNums++;
    }
  }
  return commaNums > dotNums ? ',' : '.';
}

/** Does the first row look like names rather than data? */
export function headerLooksPresent(rows: string[][], decimal: string): boolean {
  if (rows.length < 2) return false;
  const first = rows[0];
  const second = rows[1];
  const numeric = (c: string) => toNumber(c, decimal) !== null;
  // A name row has no number in it while the data below has at least one.
  const firstNumeric = first.filter(numeric).length;
  if (firstNumeric > 0 && firstNumeric >= first.length / 2) return false;
  if (rows.slice(1, 8).some((r) => r.some(numeric))) return true;
  // All text: a header when the first-row cells are all distinct and a later row differs.
  return new Set(first.map((c) => c.toLowerCase())).size === first.length && second.some((c, i) => c !== first[i]);
}

// ------------------------------------------------------------------- the grid

export interface Grid {
  /** Column names: the header row, or col1, col2... when there is none (blank header cells become colN too). */
  names: string[];
  /** Data rows as text, every one padded with '' or cut to `width`. */
  rows: string[][];
  width: number;
  hasHeader: boolean;
  /** ',', ';', tab, '|' or ' ' (whitespace). */
  sep: string;
  /** '.' or ','. */
  decimal: string;
  /** Lines (header included) that did not have `width` fields before they were padded or cut. */
  ragged: number;
}

export type GridRead = { ok: true; grid: Grid } | { ok: false; reason: 'empty' };

/** Reads delimited text into a Grid. Never throws. `opts.sep` / `opts.header`
 *  override the detection. */
export function readGrid(text: string, opts: { sep?: string; header?: boolean } = {}): GridRead {
  const lines = contentLines(text);
  if (!lines.length) return { ok: false, reason: 'empty' };
  const sep = opts.sep ?? detectDelimiter(text);
  const raw = lines.map((l) => splitLine(l, sep));
  const width = raw[0].length;
  const ragged = raw.filter((r) => r.length !== width).length;
  const cells = ragged ? raw.map((r) => (r.length >= width ? r.slice(0, width) : r.concat(new Array(width - r.length).fill('')))) : raw;
  const decimal = detectDecimal(cells, sep);
  const hasHeader = opts.header ?? headerLooksPresent(cells, decimal);
  const names = (hasHeader ? cells[0] : cells[0].map(() => '')).map((n, i) => (n === '' ? `col${i + 1}` : n));
  return { ok: true, grid: { names, rows: hasHeader ? cells.slice(1) : cells, width, hasHeader, sep, decimal, ragged } };
}

// ------------------------------------------------------------- column typing

export interface ColumnScan {
  /** Cells that are not a missing token. */
  present: string[];
  /** The numeric reading of each present cell (null when it is not a number). */
  nums: (number | null)[];
  numericCount: number;
  /** Blank / NA / NaN cells. */
  missing: number;
  distinct: number;
  /** At least `minNumeric` of the present cells are numbers (and there is one). */
  isNumeric: boolean;
}

/** Types a column from its cells: how many are missing, how many are numbers,
 *  how many distinct values. `minNumeric` is the fraction of present cells
 *  that must be numbers for the column to count as numeric (the Fit tab
 *  tolerates bad cells at 0.6 and reports them; the ML lab asks 0.95 because a
 *  column with real text in it is a category). */
export function scanColumn(cells: string[], decimal: string, minNumeric: number): ColumnScan {
  const present = cells.filter((c) => !isMissingToken(c));
  const nums = present.map((c) => toNumber(c, decimal));
  const numericCount = nums.filter((v) => v !== null).length;
  return {
    present,
    nums,
    numericCount,
    missing: cells.length - present.length,
    distinct: new Set(present).size,
    isNumeric: present.length > 0 && numericCount >= minNumeric * present.length,
  };
}

// --------------------------------------------------------------------- stats

export interface ColumnStats {
  n: number;
  min: number;
  max: number;
  mean: number;
  /** Sample standard deviation (n - 1); 0 for a single value. */
  std: number;
}

/** Count, range, mean and sample standard deviation of the finite values. */
export function columnStats(values: number[]): ColumnStats {
  let n = 0;
  let sum = 0;
  let min = Infinity;
  let max = -Infinity;
  for (const x of values) {
    if (!Number.isFinite(x)) continue;
    n++;
    sum += x;
    if (x < min) min = x;
    if (x > max) max = x;
  }
  if (n === 0) return { n: 0, min: NaN, max: NaN, mean: NaN, std: NaN };
  const mean = sum / n;
  let ss = 0;
  for (const x of values) if (Number.isFinite(x)) ss += (x - mean) ** 2;
  return { n, min, max, mean, std: n > 1 ? Math.sqrt(ss / (n - 1)) : 0 };
}

// ------------------------------------------------------------------ problems

/** Positions of the cells of `values` that are NaN, offset by `firstLine`. */
export function badCellLines(values: number[], firstLine: number): number[] {
  const out: number[] = [];
  values.forEach((v, i) => {
    if (Number.isNaN(v)) out.push(i + firstLine);
  });
  return out;
}
