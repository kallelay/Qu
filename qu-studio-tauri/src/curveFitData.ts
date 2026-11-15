/** Data input of the Fit tab, pure TypeScript: parsing pasted/CSV text into
 *  numeric columns (with plain-words problem reports), the built-in seeded
 *  examples (written as Qu code), and the Qu lines that load the chosen
 *  columns into the script. The text parsing, typing and statistics are the
 *  shared ones in `dataTable.ts`. No DOM, no Tauri. */

import { badCellLines, columnStats, needsStaging, readGrid, scanColumn, toNumber } from './dataTable';

export { columnStats };
export type { ColumnStats } from './dataTable';

export interface Column {
  name: string;
  values: number[];
}

export interface Table {
  columns: Column[];
  rows: number;
  header: boolean;
  delimiter: string;
  /** Plain-words notes: dropped text columns, cells that were not numbers, ragged rows. */
  problems: string[];
}

const DELIMITER_NAME: Record<string, string> = { ',': 'comma', '\t': 'tab', ';': 'semicolon', '|': 'pipe', ' ': 'whitespace' };

/** Parses pasted text or a CSV file. Never throws: a table with `columns: []`
 *  and a `problems` entry says what is wrong. */
export function parseTable(text: string): Table {
  const problems: string[] = [];
  const read = readGrid(text);
  if (!read.ok) return { columns: [], rows: 0, header: false, delimiter: ',', problems: ['No data: paste at least two columns of numbers.'] };
  const { names, rows: body, width, hasHeader: header, sep, decimal, ragged } = read.grid;
  const columns: Column[] = [];
  const seen = new Set<string>();
  for (let j = 0; j < width; j++) {
    const raw = body.map((r) => r[j]);
    // a column is numeric when most of its filled cells are numbers; the rest are reported below
    if (!scanColumn(raw, decimal, 0.6).isNumeric) {
      problems.push(`Column '${names[j]}' is not numeric and was left out.`);
      continue;
    }
    let name = names[j];
    while (seen.has(name)) name += '_';
    seen.add(name);
    const values = raw.map((c) => toNumber(c, decimal) ?? NaN);
    const bad = badCellLines(values, header ? 2 : 1);
    if (bad.length) problems.push(`Column '${name}': ${bad.length} cell${bad.length > 1 ? 's are' : ' is'} not a number (line ${bad.slice(0, 3).join(', ')}${bad.length > 3 ? ', ...' : ''}); those rows are skipped when the column is used.`);
    columns.push({ name, values });
  }
  if (ragged) problems.push(`${ragged} line${ragged > 1 ? 's have' : ' has'} a different number of fields than the first line.`);
  if (columns.length < 2) problems.push('Need at least two numeric columns (for example x and y).');
  return { columns, rows: body.length, header, delimiter: DELIMITER_NAME[sep] ?? 'whitespace', problems };
}

/** Rows (by index) where every one of `cols` is a finite number. */
export function cleanRows(table: Table, cols: string[]): number[] {
  const cs = cols.map((c) => table.columns.find((k) => k.name === c));
  const out: number[] = [];
  for (let i = 0; i < table.rows; i++) if (cs.every((c) => c && Number.isFinite(c.values[i]))) out.push(i);
  return out;
}

// ------------------------------------------------------------------ examples

export type Shape = 'yx' | 'surface' | 'multi';

export interface Example {
  id: string;
  shape: Shape;
  label: string;
  /** Model id (see curveFitDesign) a first click should use. */
  model: string;
  /** Qu lines that define the data variables (seeded, so reproducible). */
  lines: string[];
  /** Names of the columns the lines define: x/y/(sg) for yx, x/y/z for surface, t and signals for multi. */
  columns: string[];
  note: string;
}

export const EXAMPLES: Example[] = [
  {
    id: 'decay', shape: 'yx', label: 'Exponential decay with noise', model: 'exp', columns: ['x', 'y', 'sg'],
    lines: ['seed(7)', 'x = linspace(0, 10, 40)', 'y = 3 * exp(-0.5 * x) + 0.4 + 0.05 * randn(40)', 'sg = 0.05 * ones(40)'],
    note: 'y = 3 exp(-0.5 x) + 0.4 plus noise (sigma 0.05)',
  },
  {
    id: 'peak', shape: 'yx', label: 'Gaussian peak', model: 'gauss', columns: ['x', 'y'],
    lines: ['seed(11)', 'x = linspace(0, 10, 60)', 'y = 4 * exp(-(x - 5.5)^2 / (2 * 0.9^2)) + 0.08 * randn(60)'],
    note: 'peak height 4 at x = 5.5, sigma 0.9',
  },
  {
    id: 'wave', shape: 'yx', label: 'Noisy sine wave', model: 'sine', columns: ['x', 'y'],
    lines: ['seed(5)', 'x = linspace(0, 6, 120)', 'y = 2 * sin(3.1 * x + 0.7) + 0.5 + 0.15 * randn(120)'],
    note: 'amplitude 2, w = 3.1, phase 0.7, offset 0.5',
  },
  {
    id: 'saturation', shape: 'yx', label: 'Saturation (Michaelis-Menten)', model: 'mm', columns: ['x', 'y'],
    lines: ['seed(3)', 'x = linspace(0.2, 10, 30)', 'y = 5 * x / (1.5 + x) + 0.08 * randn(30)'],
    note: 'Vmax 5, Km 1.5',
  },
  {
    id: 'outliers', shape: 'yx', label: 'Line with gross outliers (try robust)', model: 'poly', columns: ['x', 'y'],
    lines: [
      'seed(2)', 'x = linspace(0, 10, 50)', 'y = 2 * x + 1 + 0.3 * randn(50)',
      'y[7] = y[7] + 25', 'y[19] = y[19] - 20', 'y[33] = y[33] + 30', 'y[44] = y[44] - 25',
    ],
    note: 'y = 2x + 1, four points knocked off by 20 to 30',
  },
  {
    id: 'bowl', shape: 'surface', label: 'Quadratic surface', model: 'quad', columns: ['x', 'y', 'z'],
    lines: [
      'seed(4)', 'x = 4 * rand(80) - 2', 'y = 4 * rand(80) - 2',
      'z = 1 + 0.5 * x - 0.3 * y + 0.2 * x^2 + 0.1 * x * y + 0.4 * y^2 + 0.05 * randn(80)',
    ],
    note: 'z = 1 + 0.5x - 0.3y + 0.2x^2 + 0.1xy + 0.4y^2',
  },
  {
    id: 'hill', shape: 'surface', label: '2-D Gaussian hill', model: 'gauss2d', columns: ['x', 'y', 'z'],
    lines: [
      'seed(8)', 'x = 6 * rand(150) - 3', 'y = 6 * rand(150) - 3',
      'z = 3 * exp(-((x - 0.5)^2 / (2 * 0.8^2) + (y + 0.4)^2 / (2 * 1.1^2))) + 0.2 + 0.04 * randn(150)',
    ],
    note: 'height 3 at (0.5, -0.4), widths 0.8 and 1.1, offset 0.2',
  },
  {
    id: 'decays', shape: 'multi', label: 'Three decaying signals (shared rate)', model: 'exp0', columns: ['t', 'a', 'b', 'c'],
    lines: [
      'seed(6)', 't = linspace(0, 6, 40)',
      'a = 5 * exp(-0.8 * t) + 0.04 * randn(40)', 'b = 3 * exp(-0.8 * t) + 0.04 * randn(40)', 'c = 1.5 * exp(-0.8 * t) + 0.04 * randn(40)',
    ],
    note: 'amplitudes 5, 3, 1.5 with one common rate 0.8',
  },
  {
    id: 'logistic_ode', shape: 'multi', label: 'Logistic growth (fit the ODE)', model: 'ode', columns: ['t', 'p'],
    lines: ['seed(9)', 't = linspace(0, 10, 40)', 'p = 10 / (1 + 9 * exp(-0.9 * t)) + 0.05 * randn(40)'],
    note: "p' = r p (1 - p/K) with r = 0.9, K = 10, p(0) = 1",
  },
];

export const exampleById = (id: string): Example | undefined => EXAMPLES.find((e) => e.id === id);

// ------------------------------------------------------------- Qu data lines


/** A Qu-safe variable name made from a column name. */
export function sanitizeName(name: string): string {
  const s = name.replace(/[^A-Za-z0-9_]/g, '_').replace(/^[0-9]/, '_$&');
  return s === '' ? 'col' : s;
}

const lit = (v: number) => (Number.isInteger(v) && Math.abs(v) < 1e15 ? String(v) : String(Number(v.toPrecision(15))));

export function vectorLiteral(values: number[]): string {
  return `[${values.map(lit).join(', ')}]`;
}

export interface Need {
  /** Column of the table (or the session variable) to load. */
  col: string;
  /** Variable name in the script. */
  as: string;
}

export interface LoadInput {
  source: 'paste' | 'file' | 'variable' | 'example';
  table: Table | null;
  example?: string;
  /** For `file` and large data: the path read_csv should open (see `scriptPath`). */
  filePath?: string;
}

/** The Qu lines that define each `needs` variable, plus a leading comment.
 *  `mode: 'show'` is the text the user sees and copies; `'run'` is what the
 *  panel executes (they differ only for a session variable, whose samples a
 *  separate run cannot see, and a large table written to a temp file).
 *  Rows with a non-finite value in any needed column are dropped. */
export function dataLines(input: LoadInput, needs: Need[], mode: 'show' | 'run', runFile?: string): string[] {
  if (input.source === 'example') {
    const ex = exampleById(input.example ?? '');
    return ex ? [`# built-in example: ${ex.label} (${ex.note})`, ...ex.lines] : ['# unknown example'];
  }
  const table = input.table;
  if (!table) return ['# no data loaded'];
  const keep = cleanRows(table, needs.map((n) => n.col));
  const dropped = table.rows - keep.length;
  const note = dropped > 0 ? ` (${dropped} row${dropped > 1 ? 's' : ''} with missing values left out)` : '';
  if (input.source === 'variable' && mode === 'show') {
    return [`# data from session variables${note}`, ...needs.map((n) => `${n.as} = ${n.col}`)];
  }
  const get = (col: string) => {
    const c = table.columns.find((k) => k.name === col)!;
    return keep.map((i) => c.values[i]);
  };
  if (needsStaging(keep.length)) {
    const path = (mode === 'run' ? runFile : input.filePath) ?? 'fit_data.csv';
    const ident = needs.every((n) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(n.col));
    const lines = [`# data: ${keep.length} rows read from a CSV file${mode === 'show' && input.source !== 'file' ? ' (save your table there first)' : ''}`, `fit_table = read_csv("${path.split('\\').join('/')}")`];
    for (const n of needs) lines.push(ident ? `${n.as} = fit_table.${n.col}` : `${n.as} = fit_table.${sanitizeName(n.col)}`);
    return lines;
  }
  return [`# data: ${keep.length} rows${note}`, ...needs.map((n) => `${n.as} = ${vectorLiteral(get(n.col))}`)];
}

/** The CSV text a large table is written to (only the needed columns, clean rows,
 *  headers made Qu-safe so `fit_table.<name>` works). */
export function csvForScript(table: Table, needs: Need[]): string {
  const keep = cleanRows(table, needs.map((n) => n.col));
  const cols = needs.map((n) => table.columns.find((k) => k.name === n.col)!);
  const head = needs.map((n) => sanitizeName(n.col)).join(',');
  const rows = keep.map((i) => cols.map((c) => lit(c.values[i])).join(','));
  return [head, ...rows].join('\n') + '\n';
}
