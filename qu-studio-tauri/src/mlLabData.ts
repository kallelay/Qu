// Pure TypeScript for the ML lab's data input: the typed table the lab works
// on, identifier safety, the built-in synthetic examples and the helpers that
// turn pasted text or a session matrix into CSV. The delimited-text reading,
// column scanning and statistics are the shared ones in `dataTable.ts`. No
// React here; `mlLabData.test.ts` covers it and `mlLabDesign.ts` consumes the result.

import { columnStats, isMissingToken, quNumberText, readGrid, scanColumn, toNumber } from './dataTable';

// These used to live here; other files and tests still import them from this module.
export { INLINE_ROW_LIMIT, detectDelimiter, isMissingToken, quNumberText, splitLine, toNumber } from './dataTable';

export type ColType = 'numeric' | 'category' | 'ignore';

export interface ColumnMeta {
  /** The name as it appears in the data. */
  name: string;
  /** A name safe to use after `ml_t.` (letters, digits, underscore). */
  ident: string;
  /** Position in the file (0-based). */
  index: number;
  type: ColType;
  /** Distinct values for a category column (capped), in first-seen order,
   *  numbers spelled the way Qu prints them. */
  levels: string[];
  missing: number;
  distinct: number;
  min: number | null;
  max: number | null;
  mean: number | null;
  std: number | null;
}

export interface TableMeta {
  columns: ColumnMeta[];
  rows: number;
  /** The first line holds column names. */
  hasHeader: boolean;
  /** Columns are read as `col1`, `col2`... and the header row is skipped:
   *  used when a file's header has names that are not valid identifiers. */
  positional: boolean;
  sep: string;
  decimal: string;
}

export interface ParsedTable {
  meta: TableMeta;
  /** The first rows as text, for the preview. */
  head: string[][];
  /** Every data row, text as read (capped at MAX_ROWS). */
  rows: string[][];
  problems: string[];
  truncated: boolean;
}

export const MAX_ROWS = 50_000;
export const MAX_LEVELS = 30;

/** Words Qu reserves: a column with one of these names cannot be read as
 *  `ml_t.name`. */
export const RESERVED = new Set([
  'and', 'or', 'not', 'mod', 'in', 'as', 'to', 'step', 'skip', 'by', 'if', 'then', 'else', 'elseif', 'end', 'for', 'while', 'do',
  'repeat', 'until', 'return', 'function', 'sub', 'class', 'new', 'true', 'false', 'none', 'backend', 'with', 'import', 'const',
  'constant', 'try', 'catch', 'unsafe', 'global', 'nrows', 'ncols',
]);

export const isIdent = (s: string): boolean => /^[A-Za-z_][A-Za-z0-9_]*$/.test(s) && !RESERVED.has(s);

/** A safe, unique identifier for `name` (a, b, a_2 ...). */
export function sanitizeIdent(name: string, used: Set<string>, fallback: string): string {
  let base = name.trim().replace(/[^A-Za-z0-9_]+/g, '_').replace(/^_+|_+$/g, '');
  if (!base) base = fallback;
  if (/^[0-9]/.test(base)) base = `c_${base}`;
  if (RESERVED.has(base)) base = `${base}_`;
  let out = base;
  let k = 2;
  while (used.has(out)) out = `${base}_${k++}`;
  used.add(out);
  return out;
}

// ---------------------------------------------------------------- parsing

export function parseTable(text: string, opts: { sep?: string; header?: boolean } = {}): ParsedTable {
  const problems: string[] = [];
  const empty = (msg: string): ParsedTable => ({
    meta: { columns: [], rows: 0, hasHeader: false, positional: false, sep: ',', decimal: '.' },
    head: [],
    rows: [],
    problems: [msg],
    truncated: false,
  });
  const read = readGrid(text, opts);
  if (!read.ok) return empty('The text is empty: paste a table with one row per line.');
  const { names, width, sep, decimal, hasHeader, ragged } = read.grid;
  let data = read.grid.rows;
  if (width < 1 || (width === 1 && data.length + (hasHeader ? 1 : 0) > 1 && sep !== ',')) {
    return empty('Could not find columns: separate fields with commas, semicolons or tabs.');
  }
  if (ragged > 0) {
    problems.push(`${ragged} line${ragged > 1 ? 's have' : ' has'} a different number of fields than the first line (${width}); short lines are padded with missing values, long ones cut.`);
  }
  let truncated = false;
  if (data.length > MAX_ROWS) {
    data = data.slice(0, MAX_ROWS);
    truncated = true;
    problems.push(`Only the first ${MAX_ROWS} rows are profiled here (the generated script reads the whole file).`);
  }
  if (!data.length) return empty('There is a header but no data rows.');

  const used = new Set<string>();
  const columns: ColumnMeta[] = names.map((name, index) => {
    const ident = sanitizeIdent(name, used, `col${index + 1}`);
    const scan = scanColumn(
      data.map((r) => r[index] ?? ''),
      decimal,
      0.95,
    );
    const col: ColumnMeta = {
      name,
      ident,
      index,
      type: 'ignore', // free text or an identifier column, unless one of the cases below applies
      levels: [],
      missing: scan.missing,
      distinct: scan.distinct,
      min: null,
      max: null,
      mean: null,
      std: null,
    };
    if (scan.isNumeric) {
      const st = columnStats(scan.nums.filter((x): x is number => x !== null));
      col.type = 'numeric';
      col.min = st.min;
      col.max = st.max;
      col.mean = st.mean;
      col.std = st.std;
      if (scan.numericCount < scan.present.length) problems.push(`Column "${name}": ${scan.present.length - scan.numericCount} non-numeric cell(s) are read as missing.`);
    } else if (scan.distinct <= MAX_LEVELS && scan.present.length > 0) {
      col.type = 'category';
    }
    if (col.type !== 'ignore') col.levels = categoryLevels(scan.present, decimal);
    return col;
  });
  // A numeric column with very few distinct integer values is still numeric,
  // but its levels are kept so the user can flip it to category.
  for (const c of columns) {
    if (c.type === 'numeric' && c.distinct <= MAX_LEVELS) {
      const cells = data.map((r) => r[c.index] ?? '').filter((x) => !isMissingToken(x));
      c.levels = categoryLevels(cells, decimal);
    }
  }
  return {
    meta: { columns, rows: data.length, hasHeader, positional: false, sep, decimal },
    head: data.slice(0, 8),
    rows: data,
    problems,
    truncated,
  };
}

/** Levels in first-seen order; cells that are all numbers use Qu's own number spelling. */
export function categoryLevels(cells: string[], decimal = '.'): string[] {
  const allNum = cells.length > 0 && cells.every((c) => toNumber(c, decimal) !== null);
  const seen: string[] = [];
  const set = new Set<string>();
  for (const c of cells) {
    const k = allNum ? quNumberText(toNumber(c, decimal)!) : c;
    if (!set.has(k)) {
      set.add(k);
      seen.push(k);
    }
  }
  if (allNum) seen.sort((a, b) => Number(a) - Number(b));
  return seen.slice(0, MAX_LEVELS + 1);
}

/** For a FILE source: can its header be used as `ml_t.name`? When not, the
 *  script reads the file by position. */
export function needsPositional(meta: TableMeta): boolean {
  return meta.hasHeader && meta.columns.some((c) => c.name !== c.ident);
}

/** A file's meta with the positional flag decided. */
export function forFile(parsed: ParsedTable): TableMeta {
  return { ...parsed.meta, positional: needsPositional(parsed.meta) };
}

/** A column's type overridden by the user. Switching to category recomputes
 *  the levels from the rows. */
export function withType(parsed: ParsedTable, meta: TableMeta, name: string, type: ColType): TableMeta {
  return {
    ...meta,
    columns: meta.columns.map((c) => {
      if (c.name !== name) return c;
      const cells = parsed.rows.map((r) => r[c.index] ?? '').filter((x) => !isMissingToken(x));
      return { ...c, type, levels: type === 'ignore' ? [] : categoryLevels(cells, meta.decimal) };
    }),
  };
}

/** Text that is safe inside a Qu string literal and a CSV cell. */
function safeCell(s: string): string {
  return s.replace(/[",{}\\\r\n]/g, ' ').replace(/\s+/g, ' ').trim();
}

/** The data as comma-separated text with identifier headers and "NaN" for
 *  missing numbers: the form that goes inline into a script or into a temp
 *  file. Numeric cells are rewritten with a decimal point. */
export function canonicalCsv(parsed: ParsedTable): { text: string; warnings: string[] } {
  const warnings: string[] = [];
  const cols = parsed.meta.columns;
  const header = cols.map((c) => c.ident).join(',');
  let altered = 0;
  const lines = parsed.rows.map((r) =>
    cols
      .map((c) => {
        const cell = r[c.index] ?? '';
        if (c.type === 'numeric') {
          const v = toNumber(cell, parsed.meta.decimal);
          return v === null ? 'NaN' : String(v);
        }
        const s = safeCell(cell);
        if (s !== cell.trim()) altered++;
        // A category cell that is blank stays blank; a numeric-looking one stays text-safe.
        return s;
      })
      .join(','),
  );
  if (altered > 0) warnings.push(`${altered} text cell(s) had quotes, commas, braces or backslashes replaced by spaces.`);
  return { text: `${header}\n${lines.join('\n')}`, warnings };
}

/** A session matrix (column-major as the kernel sends it) as CSV text with
 *  columns x1..xD, or a plain vector as one column. */
export function matrixToCsv(shape: number[], data: (number | null)[], colName = 'x'): string {
  const [r, c] = shape.length >= 2 ? [shape[0], shape[1]] : [shape[0], 1];
  const header = Array.from({ length: c }, (_, j) => (c === 1 ? colName : `${colName}${j + 1}`)).join(',');
  const lines: string[] = [];
  for (let i = 0; i < r; i++) {
    const row: string[] = [];
    for (let j = 0; j < c; j++) {
      const v = data[i + j * r];
      row.push(v === null || v === undefined || !Number.isFinite(v) ? 'NaN' : String(v));
    }
    lines.push(row.join(','));
  }
  return `${header}\n${lines.join('\n')}`;
}

// ---------------------------------------------------------------- examples

export interface ExampleData {
  id: string;
  label: string;
  /** What it is for. */
  blurb: string;
  /** The task it is best suited to. */
  task: 'classification' | 'regression' | 'clustering' | 'reduction' | 'anomaly';
  /** Qu code that builds the table `ml_t`. Seeded: the same table every run. */
  code: string;
  columns: Array<{ name: string; type: ColType; levels?: string[] }>;
  rows: number;
  target: string;
}

const exCols = (names: string[], target?: { name: string; levels: string[] }): ExampleData['columns'] => {
  const cols: ExampleData['columns'] = names.map((n) => ({ name: n, type: 'numeric' }));
  if (target) cols.push({ name: target.name, type: 'category', levels: target.levels });
  return cols;
};

export const EXAMPLES: ExampleData[] = [
  {
    id: 'blobs2',
    label: 'Two Gaussian blobs',
    blurb: 'Two well-separated classes in 2-D: every classifier should be near-perfect.',
    task: 'classification',
    rows: 200,
    target: 'label',
    columns: exCols(['x1', 'x2'], { name: 'label', levels: ['0', '1'] }),
    code: `# Two Gaussian blobs, 100 points each, centred at (+2, +2) and (-2, -2)
ml_a = randn(100, 2, seed = 11) * 0.8 + 2
ml_b = randn(100, 2, seed = 12) * 0.8 - 2
ml_xy = vstack(ml_a, ml_b)
ml_lab = zeros(200)
for ml_i = 100 to 199
    ml_lab[ml_i] = 1
end for
ml_t = table(x1 = ml_xy[:, 0], x2 = ml_xy[:, 1], label = ml_lab)`,
  },
  {
    id: 'xor',
    label: 'XOR (not linearly separable)',
    blurb: 'The class is the sign of x1 * x2: a straight line cannot separate it (about 50%); a tree, kNN or network can.',
    task: 'classification',
    rows: 400,
    target: 'label',
    columns: exCols(['x1', 'x2'], { name: 'label', levels: ['0', '1'] }),
    code: `# XOR: label 1 when x1 and x2 have the same sign, spread over four quadrants
ml_xy = randn(400, 2, seed = 21)
ml_lab = zeros(400)
for ml_i = 0 to 399
    if ml_xy[ml_i, 0] * ml_xy[ml_i, 1] > 0
        ml_lab[ml_i] = 1
    end if
end for
ml_t = table(x1 = ml_xy[:, 0], x2 = ml_xy[:, 1], label = ml_lab)`,
  },
  {
    id: 'moons',
    label: 'Two moons',
    blurb: 'Two interleaved half-circles with noise: needs a curved boundary.',
    task: 'classification',
    rows: 300,
    target: 'label',
    columns: exCols(['x1', 'x2'], { name: 'label', levels: ['0', '1'] }),
    code: `# Two interleaved half-circles plus Gaussian noise
ml_s = linspace(0, pi, 150)
ml_n1 = randn(150, 2, seed = 31) * 0.12
ml_n2 = randn(150, 2, seed = 32) * 0.12
ml_up = vstack(cos(ml_s), sin(ml_s))'
ml_dn = vstack(1 - cos(ml_s), 0.5 - sin(ml_s))'
ml_xy = vstack(ml_up + ml_n1, ml_dn + ml_n2)
ml_lab = zeros(300)
for ml_i = 150 to 299
    ml_lab[ml_i] = 1
end for
ml_t = table(x1 = ml_xy[:, 0], x2 = ml_xy[:, 1], label = ml_lab)`,
  },
  {
    id: 'blobs3',
    label: 'Three blobs (3 classes)',
    blurb: 'Three Gaussian clusters: multi-class classification, or k-means with k = 3.',
    task: 'classification',
    rows: 240,
    target: 'label',
    columns: exCols(['x1', 'x2'], { name: 'label', levels: ['0', '1', '2'] }),
    code: `# Three Gaussian blobs, 80 points each, centred at (0, 4), (4, -2) and (-4, -2)
ml_a = randn(80, 2, seed = 41) * 0.9
ml_a[:, 1] = ml_a[:, 1] + 4
ml_b = randn(80, 2, seed = 42) * 0.9
ml_b[:, 0] = ml_b[:, 0] + 4
ml_b[:, 1] = ml_b[:, 1] - 2
ml_c = randn(80, 2, seed = 43) * 0.9
ml_c[:, 0] = ml_c[:, 0] - 4
ml_c[:, 1] = ml_c[:, 1] - 2
ml_xy = vstack(ml_a, ml_b, ml_c)
ml_lab = zeros(240)
for ml_i = 80 to 239
    ml_lab[ml_i] = 1
end for
for ml_i = 160 to 239
    ml_lab[ml_i] = 2
end for
ml_t = table(x1 = ml_xy[:, 0], x2 = ml_xy[:, 1], label = ml_lab)`,
  },
  {
    id: 'linreg',
    label: 'Noisy linear (regression)',
    blurb: 'y = 3 x1 - 2 x2 + 1 + noise (sd 0.5); x3 is irrelevant. R-squared should be near 0.97.',
    task: 'regression',
    rows: 300,
    target: 'y',
    columns: exCols(['x1', 'x2', 'x3', 'y']),
    code: `# y = 3 x1 - 2 x2 + 1 + noise(sd 0.5); x3 does not matter
ml_x = randn(300, 3, seed = 51)
ml_y = 3 * ml_x[:, 0] - 2 * ml_x[:, 1] + 1 + 0.5 * randn(300, seed = 52)
ml_t = table(x1 = ml_x[:, 0], x2 = ml_x[:, 1], x3 = ml_x[:, 2], y = ml_y)`,
  },
  {
    id: 'poly',
    label: 'Curved relation (regression)',
    blurb: 'y = 1 + 2 x - 3 x^2 + noise: a straight line explains little; trees, forests and kNN follow the curve.',
    task: 'regression',
    rows: 300,
    target: 'y',
    columns: exCols(['x', 'y']),
    code: `# y = 1 + 2 x - 3 x^2 + noise(sd 0.1), x in [-1.5, 1.5]
ml_x = linspace(-1.5, 1.5, 300)
ml_y = 1 + 2 * ml_x - 3 * ml_x .^ 2 + 0.1 * randn(300, seed = 61)
ml_t = table(x = ml_x, y = ml_y)`,
  },
  {
    id: 'anomaly',
    label: 'Cluster with outliers',
    blurb: 'A 2-D Gaussian cloud plus 15 scattered outliers (column outlier = 1): anomaly detection.',
    task: 'anomaly',
    rows: 315,
    target: 'outlier',
    columns: exCols(['x1', 'x2'], { name: 'outlier', levels: ['0', '1'] }),
    code: `# 300 inliers N(0, 1) and 15 outliers spread over [-7, 7]^2 (outlier = 1)
ml_in = randn(300, 2, seed = 71)
ml_out = (rand(15, 2, seed = 72) - 0.5) * 14
ml_xy = vstack(ml_in, ml_out)
ml_lab = zeros(315)
for ml_i = 300 to 314
    ml_lab[ml_i] = 1
end for
ml_t = table(x1 = ml_xy[:, 0], x2 = ml_xy[:, 1], outlier = ml_lab)`,
  },
  {
    id: 'wide',
    label: 'Correlated features (PCA)',
    blurb: 'Five features built from two hidden factors plus noise: PCA finds that two components carry almost everything.',
    task: 'reduction',
    rows: 200,
    target: 'group',
    columns: exCols(['f1', 'f2', 'f3', 'f4', 'f5'], { name: 'group', levels: ['0', '1'] }),
    code: `# Five features from two hidden factors (one with twice the spread) plus small noise
ml_f = randn(200, 2, seed = 81)
ml_f[:, 0] = 2 * ml_f[:, 0]
ml_load = [1, 0.5, -0.5, 0.2, 0.8; 0.1, 1, 0.8, -1, 0.3]
ml_x = ml_f * ml_load + 0.15 * randn(200, 5, seed = 82)
ml_grp = zeros(200)
for ml_i = 0 to 199
    if ml_f[ml_i, 0] > 0
        ml_grp[ml_i] = 1
    end if
end for
ml_t = table(f1 = ml_x[:, 0], f2 = ml_x[:, 1], f3 = ml_x[:, 2], f4 = ml_x[:, 3], f5 = ml_x[:, 4], group = ml_grp)`,
  },
  {
    id: 'mixed',
    label: 'Mixed types with missing values',
    blurb: 'Numeric and category features, some missing cells, a two-class target: shows encoding and missing-value handling.',
    task: 'classification',
    rows: 240,
    target: 'segment',
    columns: [
      { name: 'age', type: 'numeric' },
      { name: 'income', type: 'numeric' },
      { name: 'city', type: 'category', levels: ['north', 'south', 'east'] },
      { name: 'segment', type: 'category', levels: ['basic', 'premium'] },
    ],
    code: `# Customers: age (some missing), income, city (category) and segment (the class)
ml_age = 20 + 45 * rand(240, seed = 91)
ml_inc = 20 + 60 * rand(240, seed = 92) + 0.4 * ml_age
ml_nz = 8 * randn(240, seed = 93)
ml_cn = ("north", "south", "east")
ml_sn = ("basic", "premium")
ml_city = lines("")
ml_seg = lines("")
for ml_i = 0 to 239
    ml_city = append(ml_city, ml_cn[ml_i mod 3])
    ml_score = ml_inc[ml_i] + ml_nz[ml_i]
    if ml_i mod 3 == 0
        ml_score = ml_score + 8
    end if
    if ml_score > 62
        ml_seg = append(ml_seg, ml_sn[1])
    else
        ml_seg = append(ml_seg, ml_sn[0])
    end if
end for
for ml_i = 5 to 239 step 17
    ml_age[ml_i] = nan
end for
ml_t = table(age = ml_age, income = ml_inc, city = ml_city, segment = ml_seg)`,
  },
];

export const exampleById = (id: string): ExampleData | undefined => EXAMPLES.find((e) => e.id === id);

/** The TableMeta of a built-in example (known statically; the engine gate
 *  runs every example through the real script path). */
export function exampleMeta(ex: ExampleData): TableMeta {
  const used = new Set<string>();
  return {
    columns: ex.columns.map((c, index) => ({
      name: c.name,
      ident: sanitizeIdent(c.name, used, `col${index + 1}`),
      index,
      type: c.type,
      levels: c.levels ?? [],
      missing: 0,
      distinct: c.levels?.length ?? ex.rows,
      min: null,
      max: null,
      mean: null,
      std: null,
    })),
    rows: ex.rows,
    hasHeader: true,
    positional: false,
    sep: ',',
    decimal: '.',
  };
}

// ---------------------------------------------------------------- profiles

/** Counts per level of a category column from the parsed rows. */
export function levelCounts(parsed: ParsedTable, name: string): { levels: string[]; counts: number[] } | null {
  const col = parsed.meta.columns.find((c) => c.name === name);
  if (!col || col.type === 'ignore') return null;
  const levels = col.levels.slice(0, MAX_LEVELS);
  const counts = new Array(levels.length).fill(0);
  const allNum = levels.length > 0 && levels.every((l) => /^[+-]?(\d|\.\d)/.test(l));
  for (const r of parsed.rows) {
    const cell = r[col.index] ?? '';
    if (isMissingToken(cell)) continue;
    const key = allNum && toNumber(cell, parsed.meta.decimal) !== null ? quNumberText(toNumber(cell, parsed.meta.decimal)!) : cell;
    const i = levels.indexOf(key);
    if (i >= 0) counts[i]++;
  }
  return { levels, counts };
}

/** A histogram of a numeric column from the parsed rows. */
export function numericHistogram(parsed: ParsedTable, name: string, bins = 12): { lo: number; hi: number; counts: number[] } | null {
  const col = parsed.meta.columns.find((c) => c.name === name);
  if (!col || col.type !== 'numeric') return null;
  const v = parsed.rows.map((r) => toNumber(r[col.index] ?? '', parsed.meta.decimal)).filter((x): x is number => x !== null);
  if (!v.length) return null;
  const lo = Math.min(...v);
  const hi = Math.max(...v);
  const counts = new Array(bins).fill(0);
  for (const x of v) counts[Math.min(bins - 1, Math.floor(((x - lo) / (hi - lo + 1e-12)) * bins))]++;
  return { lo, hi, counts };
}
