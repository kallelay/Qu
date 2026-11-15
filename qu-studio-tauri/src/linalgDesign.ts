/** Pure helpers behind the Linear Algebra panel: matrix sources, the Qu
 *  scripts it generates (one per task), the structure advisor, the plain-words
 *  warnings, and the reading of the numbers a run hands back. No DOM, no Tauri,
 *  so all of it is testable on its own (see linalgDesign.test.ts and the
 *  engine gate linalgEngine.test.ts).
 *
 *  Everything the panel shows is computed by the generated Qu script. The
 *  TypeScript here only (a) writes that script, (b) decides which method to
 *  write from structure flags the ENGINE computed, and (c) formats numbers. */

// ------------------------------------------------------------------ constants

/** Matrices up to this size get rank / cond / det / Cholesky tests in the
 *  summary; beyond it those O(n^3) tests are skipped (the engine would be
 *  busy for seconds on every keystroke). */
export const DENSE_LIMIT = 600;
/** A matrix with at most this many entries is written into the script as a
 *  literal; larger ones are read back from a staged file. */
export const INLINE_LIMIT = 400;
/** The grid editor offers this many rows and columns at most. */
export const GRID_LIMIT = 12;
/** A square, sparse system at least this large is solved iteratively/sparsely. */
export const LARGE_SPARSE_N = 200;
export const LARGE_SPARSE_DENSITY = 0.1;
/** Machine epsilon, written into scripts as a literal (Qu has no `eps`). */
export const EPS = 2.220446049250313e-16;

// ---------------------------------------------------------------------- types

export type MatrixName = 'A' | 'b' | 'B' | 'C';

export type Builder =
  | { kind: 'identity'; n: number }
  | { kind: 'zeros'; rows: number; cols: number }
  | { kind: 'ones'; rows: number; cols: number }
  | { kind: 'diagonal'; values: string }
  | { kind: 'random'; rows: number; cols: number; seed: number; dist: 'normal' | 'uniform' | 'spd' }
  | { kind: 'hilbert'; n: number }
  | { kind: 'vandermonde'; points: number; cols: number }
  | { kind: 'toeplitz'; col: string; row: string }
  | { kind: 'poisson1d'; n: number }
  | { kind: 'pattern'; rows: number; cols: number; entries: string };

export type BuilderKind = Builder['kind'];

export const BUILDERS: Array<{ id: BuilderKind; label: string; note: string }> = [
  { id: 'identity', label: 'Identity', note: 'ones on the diagonal, zeros elsewhere' },
  { id: 'zeros', label: 'Zeros', note: 'all entries zero' },
  { id: 'ones', label: 'Ones', note: 'all entries one' },
  { id: 'diagonal', label: 'Diagonal', note: 'a comma-separated list on the diagonal' },
  { id: 'random', label: 'Random (seeded)', note: 'reproducible: the same seed gives the same matrix' },
  { id: 'hilbert', label: 'Hilbert (ill-conditioned)', note: '1 / (i + j - 1): the classic ill-conditioned matrix' },
  { id: 'vandermonde', label: 'Vandermonde', note: 'powers of equally spaced points on [0, 1]' },
  { id: 'toeplitz', label: 'Toeplitz', note: 'constant diagonals from a first column (and row)' },
  { id: 'poisson1d', label: '1-D Poisson (tridiagonal)', note: '2 on the diagonal, -1 beside it: symmetric positive definite, sparse' },
  { id: 'pattern', label: 'From a sparse pattern', note: 'one "row col value" entry per line (0-based, like Qu)' },
];

export type MatrixSource =
  | { kind: 'grid'; cells: string[][] }
  | { kind: 'builder'; builder: Builder }
  /** Numbers parsed from pasted text, a CSV file, or a session variable.
   *  `values` is row-major. Above INLINE_LIMIT entries the script reads
   *  `file` (a raw little-endian f64 file, column-major, staged by the panel). */
  | { kind: 'data'; rows: number; cols: number; values: number[]; origin: string; file?: string; header?: string[] };

export type TaskId = 'solve' | 'decomp' | 'props' | 'lsq' | 'matfun' | 'eig';

export type SolveMethodId =
  | 'cholesky' | 'lu' | 'triangular' | 'qr' | 'svd' | 'lstsq' | 'pinv'
  | 'cg' | 'gmres' | 'sparselu' | 'tikhonov' | 'tsvd';

export type DecompKind = 'lu' | 'qr' | 'chol' | 'svd' | 'eig' | 'schur';
export type MatFunKind = 'expm' | 'sqrtm' | 'logm' | 'mpower' | 'lyap' | 'sylvester';

export interface LinalgSpec {
  matrices: Record<MatrixName, MatrixSource>;
  task: TaskId;
  solve: { method: SolveMethodId | 'auto'; lambda: number; tsvdK: number };
  decomp: { kind: DecompKind };
  lsq: { weighted: boolean; truth: 'ones' | 'given' };
  matfun: { kind: MatFunKind; power: number };
  precision: number;
}

/** Facts about a matrix, all computed by the engine (see buildSummaryScript). */
export interface MatrixFlags {
  rows: number;
  cols: number;
  symmetric: boolean;
  /** 0 = no, 1 = yes (a Cholesky factorisation succeeded), 2 = likely: too
   *  large to factorise, but symmetric with a positive, dominant diagonal. */
  spd: 0 | 1 | 2;
  lower: boolean;
  upper: boolean;
  lowerBand: number;
  upperBand: number;
  nnz: number;
  density: number;
  /** NaN when skipped for size. */
  rank: number;
  cond: number;
  det: number;
  heavy: boolean;
}

// ------------------------------------------------------------ number handling

/** A cell's number: a plain number, or a fraction `a/b`. null when it is neither. */
export function parseCell(s: string): number | null {
  const t = s.trim();
  if (t === '') return null;
  const num = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
  if (num.test(t)) return Number(t);
  const frac = /^([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)\s*\/\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)$/.exec(t);
  if (frac) {
    const d = Number(frac[2]);
    if (d === 0) return null;
    return Number(frac[1]) / d;
  }
  return null;
}

/** A number as Qu source. Always finite here (validated before use). */
export function numLit(x: number): string {
  if (Object.is(x, -0)) return '0';
  return String(x);
}

const nonNeg = (n: number) => Number.isInteger(n) && n >= 1;

/** Digits shown for a number: scientific for tiny or huge values. */
export function formatNumber(v: number, precision = 6): string {
  if (Number.isNaN(v)) return 'NaN';
  if (!Number.isFinite(v)) return v > 0 ? 'inf' : '-inf';
  if (v === 0) return '0';
  const p = Math.max(1, Math.min(15, Math.round(precision)));
  const a = Math.abs(v);
  if (a < 1e-4 || a >= 1e6) {
    const s = v.toExponential(p - 1);
    const [m, e] = s.split('e');
    const mm = m.includes('.') ? m.replace(/0+$/, '').replace(/\.$/, '') : m;
    return `${mm}e${e.replace('+', '')}`;
  }
  const s = v.toPrecision(p);
  if (s.includes('e')) return String(Number(s));
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s;
}

/** Complex as the engine prints it: `a + bj`. */
export function formatComplex(re: number, im: number, precision = 6): string {
  if (im === 0) return formatNumber(re, precision);
  const sign = im < 0 || Object.is(im, -0) ? '-' : '+';
  return `${formatNumber(re, precision)} ${sign} ${formatNumber(Math.abs(im), precision)}j`;
}

export interface Mat {
  rows: number;
  cols: number;
  /** row-major */
  re: number[][];
  im?: number[][];
}

export function matFromRows(re: number[][], im?: number[][]): Mat {
  return { rows: re.length, cols: re.length ? re[0].length : 0, re, im };
}

export function matToCsv(m: Mat): string {
  const cell = (i: number, j: number) => {
    const r = m.re[i][j];
    const c = m.im ? m.im[i][j] : 0;
    return c === 0 ? String(r) : `${r}${c < 0 ? '-' : '+'}${Math.abs(c)}j`;
  };
  return m.re.map((row, i) => row.map((_, j) => cell(i, j)).join(',')).join('\n');
}

/** `name = [...]` for a (possibly complex) matrix. Column vectors and row
 *  vectors use `as matrix(r, c)` so they stay genuine matrices, as a bare
 *  `[1; 2]` literal is a plain vector in Qu. */
export function matToQuLiteral(name: string, m: Mat): string {
  const ent = (i: number, j: number) => {
    const r = numLit(m.re[i][j]);
    const c = m.im ? m.im[i][j] : 0;
    return c === 0 ? r : `${r} ${c < 0 ? '-' : '+'} ${numLit(Math.abs(c))}j`;
  };
  if (m.rows === 0 || m.cols === 0) return `${name} = zeros(${m.rows}, ${m.cols})`;
  if (m.cols === 1 || m.rows === 1) {
    const flat: string[] = [];
    for (let j = 0; j < m.cols; j++) for (let i = 0; i < m.rows; i++) flat.push(ent(i, j));
    return `${name} = [${flat.join(', ')}] as matrix(${m.rows}, ${m.cols})`;
  }
  const rowsTxt = m.re.map((row, i) => row.map((_, j) => ent(i, j)).join(', '));
  return `${name} = [${rowsTxt.join('; ')}]`;
}

// ------------------------------------------------------------- pasted tables

export interface ParsedTable {
  rows: number;
  cols: number;
  values: number[][];
  /** The first line was a header and was dropped. */
  header: string[] | null;
  problems: string[];
}

/** Parses pasted spreadsheet / CSV text. Tabs, commas, semicolons or spaces
 *  separate cells. A first line that holds no numbers is taken as a header. */
export function parseMatrixText(text: string): ParsedTable {
  const problems: string[] = [];
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l !== '' && !l.startsWith('#'));
  if (lines.length === 0) return { rows: 0, cols: 0, values: [], header: null, problems: ['Nothing to read: the text is empty.'] };
  const sep = lines.some((l) => l.includes('\t')) ? /\t/ : lines.some((l) => l.includes(';')) ? /;/ : lines.some((l) => l.includes(',')) ? /,/ : /\s+/;
  const split = (l: string) => l.split(sep).map((c) => c.trim());
  const table = lines.map(split);
  let header: string[] | null = null;
  if (table.length > 1 && table[0].every((c) => c !== '' && parseCell(c) === null)) header = table.shift()!;
  const width = Math.max(...table.map((r) => r.length));
  const values: number[][] = [];
  table.forEach((r, i) => {
    if (r.length !== width) problems.push(`Line ${i + 1 + (header ? 1 : 0)} has ${r.length} cells, the widest line has ${width}.`);
    values.push(
      Array.from({ length: width }, (_, j) => {
        const c = r[j] ?? '';
        const v = parseCell(c);
        if (v === null) {
          problems.push(`Line ${i + 1 + (header ? 1 : 0)}, cell ${j + 1}: "${c}" is not a number.`);
          return NaN;
        }
        return v;
      }),
    );
  });
  return { rows: values.length, cols: width, values, header, problems };
}

export function dataSource(t: ParsedTable | { rows: number; cols: number; values: number[][]; header?: string[] | null }, origin: string): MatrixSource {
  return { kind: 'data', rows: t.rows, cols: t.cols, values: t.values.flat(), origin, header: t.header ?? undefined };
}

/** A parsed table as the right kind of source: a grid you can edit when it
 *  fits, else a data source (read from a staged file above INLINE_LIMIT). */
export function sourceFromTable(t: ParsedTable, origin: string): MatrixSource {
  if (t.rows <= GRID_LIMIT && t.cols <= GRID_LIMIT) return gridOf(t.values);
  return dataSource(t, origin);
}

/** Column statistics for the preview of a data source. */
export function columnStats(src: Extract<MatrixSource, { kind: 'data' }>): Array<{ name: string; min: number; max: number; mean: number }> {
  return Array.from({ length: src.cols }, (_, j) => {
    let mn = Infinity;
    let mx = -Infinity;
    let sum = 0;
    for (let i = 0; i < src.rows; i++) {
      const v = src.values[i * src.cols + j];
      mn = Math.min(mn, v);
      mx = Math.max(mx, v);
      sum += v;
    }
    return { name: src.header?.[j] ?? `col${j + 1}`, min: mn, max: mx, mean: sum / src.rows };
  });
}

/** Column-major raw little-endian f64 bytes of a data source (what `read_array` reads). */
export function stagedBytes(src: Extract<MatrixSource, { kind: 'data' }>): Float64Array {
  const out = new Float64Array(src.rows * src.cols);
  for (let i = 0; i < src.rows; i++) for (let j = 0; j < src.cols; j++) out[i + j * src.rows] = src.values[i * src.cols + j];
  return out;
}

// -------------------------------------------------------------------- sources

export function gridOf(rows: number[][]): MatrixSource {
  return { kind: 'grid', cells: rows.map((r) => r.map((v) => numLit(v))) };
}
export function emptyGrid(rows: number, cols: number): MatrixSource {
  return { kind: 'grid', cells: Array.from({ length: rows }, () => Array.from({ length: cols }, () => '0')) };
}
export function columnGrid(values: number[]): MatrixSource {
  return gridOf(values.map((v) => [v]));
}

/** `1, 2, 3` -> [1, 2, 3]; null when any piece is not a number. */
export function parseList(s: string): number[] | null {
  const parts = s.split(/[,\s;]+/).filter((p) => p !== '');
  if (parts.length === 0) return null;
  const out: number[] = [];
  for (const p of parts) {
    const v = parseCell(p);
    if (v === null) return null;
    out.push(v);
  }
  return out;
}

export interface PatternEntry {
  i: number;
  j: number;
  v: number;
}
/** Lines of `row col value` (comma, space or tab separated). Returns the entries and any problems. */
export function parsePattern(text: string): { entries: PatternEntry[]; problems: string[] } {
  const entries: PatternEntry[] = [];
  const problems: string[] = [];
  text.split(/\r?\n/).forEach((raw, ln) => {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) return;
    const p = line.split(/[,\s]+/);
    const i = Number(p[0]);
    const j = Number(p[1]);
    const v = p.length >= 3 ? parseCell(p[2]) : 1;
    if (p.length < 2 || p.length > 3 || !Number.isInteger(i) || !Number.isInteger(j) || i < 0 || j < 0 || v === null) {
      problems.push(`Line ${ln + 1}: "${line}" should be "row col value" with whole 0-based row and column.`);
      return;
    }
    entries.push({ i, j, v });
  });
  return { entries, problems };
}

export function sourceDims(src: MatrixSource): { rows: number; cols: number } | null {
  switch (src.kind) {
    case 'grid':
      return src.cells.length && src.cells[0].length ? { rows: src.cells.length, cols: src.cells[0].length } : null;
    case 'data':
      return src.rows > 0 && src.cols > 0 ? { rows: src.rows, cols: src.cols } : null;
    case 'builder': {
      const b = src.builder;
      switch (b.kind) {
        case 'identity': case 'hilbert': case 'poisson1d':
          return nonNeg(b.n) ? { rows: b.n, cols: b.n } : null;
        case 'zeros': case 'ones':
          return nonNeg(b.rows) && nonNeg(b.cols) ? { rows: b.rows, cols: b.cols } : null;
        case 'pattern':
          return nonNeg(b.rows) && nonNeg(b.cols) ? { rows: b.rows, cols: b.cols } : null;
        case 'random':
          if (!nonNeg(b.rows) || !nonNeg(b.cols)) return null;
          return b.dist === 'spd' ? { rows: b.rows, cols: b.rows } : { rows: b.rows, cols: b.cols };
        case 'diagonal': {
          const v = parseList(b.values);
          return v ? { rows: v.length, cols: v.length } : null;
        }
        case 'vandermonde':
          return nonNeg(b.points) && nonNeg(b.cols) ? { rows: b.points, cols: b.cols } : null;
        case 'toeplitz': {
          const c = parseList(b.col);
          const r = b.row.trim() === '' ? c : parseList(b.row);
          return c && r ? { rows: c.length, cols: r.length } : null;
        }
      }
    }
  }
  return null;
}

/** `null` if the source can be turned into Qu, else one sentence on what to fix. */
export function sourceProblem(name: string, src: MatrixSource): string | null {
  if (src.kind === 'grid') {
    if (!src.cells.length || !src.cells[0].length) return `${name} is empty.`;
    for (let i = 0; i < src.cells.length; i++) {
      for (let j = 0; j < src.cells[i].length; j++) {
        if (parseCell(src.cells[i][j]) === null) return `${name}: row ${i + 1}, column ${j + 1} ("${src.cells[i][j]}") is not a number.`;
      }
    }
    return null;
  }
  if (src.kind === 'data') {
    if (!(src.rows > 0 && src.cols > 0)) return `${name} has no data.`;
    if (src.values.length !== src.rows * src.cols) return `${name}: the data does not fill ${src.rows} x ${src.cols}.`;
    if (src.values.some((v) => !Number.isFinite(v))) return `${name}: the data holds a cell that is not a finite number.`;
    if (src.rows * src.cols > INLINE_LIMIT && !src.file) return `${name} is large (${src.rows} x ${src.cols}) and is still being staged to a file.`;
    return null;
  }
  const b = src.builder;
  const d = sourceDims(src);
  if (!d) {
    switch (b.kind) {
      case 'diagonal': return `${name}: type the diagonal as numbers, for example 1, 2, 3.`;
      case 'toeplitz': return `${name}: the first column (and row, if given) must be numbers like 1, 2, 3.`;
      default: return `${name}: sizes must be whole numbers, 1 or more.`;
    }
  }
  if (d.rows * d.cols > 4_000_000) return `${name}: ${d.rows} x ${d.cols} is too large for this panel.`;
  if (b.kind === 'poisson1d' && b.n < 2) return `${name}: the 1-D Poisson matrix needs n of 2 or more.`;
  if (b.kind === 'random' && !Number.isInteger(b.seed)) return `${name}: the seed must be a whole number.`;
  if (b.kind === 'toeplitz' && b.row.trim() !== '' && parseList(b.row) === null) return `${name}: the first row must be numbers.`;
  if (b.kind === 'pattern') {
    const { entries, problems } = parsePattern(b.entries);
    if (problems.length) return `${name}: ${problems[0]}`;
    for (const e of entries) {
      if (e.i >= b.rows || e.j >= b.cols) return `${name}: entry (${e.i}, ${e.j}) is outside the ${b.rows} x ${b.cols} matrix.`;
    }
  }
  return null;
}

/** The Qu lines that define matrix `name`. */
export function matrixDefinition(name: string, src: MatrixSource): string {
  if (src.kind === 'grid') {
    const re = src.cells.map((r) => r.map((c) => parseCell(c) ?? 0));
    return matToQuLiteral(name, matFromRows(re));
  }
  if (src.kind === 'data') {
    if (src.rows * src.cols <= INLINE_LIMIT) {
      const re: number[][] = [];
      for (let i = 0; i < src.rows; i++) re.push(src.values.slice(i * src.cols, (i + 1) * src.cols));
      return matToQuLiteral(name, matFromRows(re));
    }
    return `${name} = reshape(read_array("${(src.file ?? '').split('\\').join('/')}"), ${src.rows}, ${src.cols})`;
  }
  const b = src.builder;
  switch (b.kind) {
    case 'identity':
      return `${name} = eye(${b.n})`;
    case 'zeros':
      return `${name} = zeros(${b.rows}, ${b.cols})`;
    case 'ones':
      return `${name} = ones(${b.rows}, ${b.cols})`;
    case 'diagonal':
      return `${name} = diag([${(parseList(b.values) ?? []).map(numLit).join(', ')}])`;
    case 'random':
      if (b.dist === 'spd') {
        return `la_g = randn(${b.rows}, ${b.rows}, seed=${b.seed})\n${name} = la_g' * la_g + ${b.rows} * eye(${b.rows})   # symmetric positive definite`;
      }
      return `${name} = ${b.dist === 'normal' ? 'randn' : 'rand'}(${b.rows}, ${b.cols}, seed=${b.seed})`;
    case 'hilbert':
      return `la_i = linspace(1, ${b.n}, ${b.n})\n${name} = 1 ./ (outer(la_i, ones(${b.n})) + outer(ones(${b.n}), la_i) - 1)   # Hilbert: 1/(i+j-1)`;
    case 'vandermonde':
      return `${name} = vander(linspace(0, 1, ${b.points}), ${b.cols})`;
    case 'toeplitz': {
      const c = `[${(parseList(b.col) ?? []).map(numLit).join(', ')}]`;
      return b.row.trim() === '' ? `${name} = toeplitz(${c})` : `${name} = toeplitz(${c}, [${(parseList(b.row) ?? []).map(numLit).join(', ')}])`;
    }
    case 'poisson1d':
      return `la_c = zeros(${b.n})\nla_c[0] = 2\nla_c[1] = -1\n${name} = toeplitz(la_c)   # tridiagonal: 2 on the diagonal, -1 beside it`;
    case 'pattern': {
      const { entries } = parsePattern(b.entries);
      return [`${name} = zeros(${b.rows}, ${b.cols})`, ...entries.map((e) => `${name}[${e.i}, ${e.j}] = ${numLit(e.v)}`)].join('\n');
    }
  }
}

/** The matrix as numbers, when it can be known without the engine (used to
 *  turn a deterministic builder into an editable grid and by the tests'
 *  independent truth). null for the random builders. */
export function sourceValues(src: MatrixSource): number[][] | null {
  if (src.kind === 'grid') return src.cells.map((r) => r.map((c) => parseCell(c) ?? 0));
  if (src.kind === 'data') {
    const out: number[][] = [];
    for (let i = 0; i < src.rows; i++) out.push(src.values.slice(i * src.cols, (i + 1) * src.cols));
    return out;
  }
  const b = src.builder;
  const mk = (r: number, c: number, f: (i: number, j: number) => number) => Array.from({ length: r }, (_, i) => Array.from({ length: c }, (_, j) => f(i, j)));
  switch (b.kind) {
    case 'identity': return mk(b.n, b.n, (i, j) => (i === j ? 1 : 0));
    case 'zeros': return mk(b.rows, b.cols, () => 0);
    case 'ones': return mk(b.rows, b.cols, () => 1);
    case 'diagonal': {
      const v = parseList(b.values);
      return v ? mk(v.length, v.length, (i, j) => (i === j ? v[i] : 0)) : null;
    }
    case 'hilbert': return mk(b.n, b.n, (i, j) => 1 / (i + j + 1));
    case 'vandermonde': {
      const x = (i: number) => (b.points === 1 ? 0 : i / (b.points - 1));
      return mk(b.points, b.cols, (i, j) => Math.pow(x(i), b.cols - 1 - j));
    }
    case 'toeplitz': {
      const c = parseList(b.col);
      const r = b.row.trim() === '' ? c : parseList(b.row);
      if (!c || !r) return null;
      return mk(c.length, r.length, (i, j) => (i >= j ? c[i - j] : r[j - i]));
    }
    case 'poisson1d': return mk(b.n, b.n, (i, j) => (i === j ? 2 : Math.abs(i - j) === 1 ? -1 : 0));
    case 'pattern': {
      const m = mk(b.rows, b.cols, () => 0);
      for (const e of parsePattern(b.entries).entries) if (e.i < b.rows && e.j < b.cols) m[e.i][e.j] = e.v;
      return m;
    }
    case 'random': return null;
  }
}

// ------------------------------------------------------------------ the spec

const DEFAULT_SPEC_PARTS = {
  solve: { method: 'auto' as SolveMethodId | 'auto', lambda: 0.001, tsvdK: 3 },
  decomp: { kind: 'lu' as DecompKind },
  lsq: { weighted: false, truth: 'ones' as 'ones' | 'given' },
  matfun: { kind: 'expm' as MatFunKind, power: 3 },
};

export const DEFAULT_SPEC: LinalgSpec = {
  matrices: {
    A: gridOf([[4, 1, 0], [1, 3, 1], [0, 1, 2]]),
    b: columnGrid([1, 2, 3]),
    B: gridOf([[1, 0], [0, 2]]),
    C: gridOf([[1, 0, 0], [0, 1, 0], [0, 0, 1]]),
  },
  task: 'solve',
  ...DEFAULT_SPEC_PARTS,
  precision: 6,
};

const mulV = (a: number[][], x: number[]) => a.map((r) => r.reduce((t, v, k) => t + v * x[k], 0));

/** Worked examples: each one fills in the matrices and options for a task. */
export interface Example {
  id: string;
  label: string;
  spec: Omit<LinalgSpec, 'precision'>;
}

function makeExamples(): Example[] {
  const base = { solve: DEFAULT_SPEC_PARTS.solve, decomp: DEFAULT_SPEC_PARTS.decomp, lsq: DEFAULT_SPEC_PARTS.lsq, matfun: DEFAULT_SPEC_PARTS.matfun };
  const mats = (A: MatrixSource, b: MatrixSource, extra: Partial<Record<MatrixName, MatrixSource>> = {}): Record<MatrixName, MatrixSource> => ({
    A, b, B: gridOf([[1, 0], [0, 2]]), C: gridOf([[1, 0, 0], [0, 1, 0], [0, 0, 1]]), ...extra,
  });
  const H8: MatrixSource = { kind: 'builder', builder: { kind: 'hilbert', n: 8 } };
  const hb = mulV(sourceValues(H8)!, new Array(8).fill(1));
  const line: MatrixSource = { kind: 'builder', builder: { kind: 'vandermonde', points: 20, cols: 2 } };
  const fitB = Array.from({ length: 20 }, (_, i) => 2 * (i / 19) + 1 + 0.05 * Math.sin(7 * i));
  const spdA = gridOf([[4, 1, 0], [1, 3, 1], [0, 1, 2]]);
  return [
    { id: 'spd', label: 'Solve: symmetric positive definite 3x3', spec: { ...base, task: 'solve', matrices: mats(spdA, columnGrid([1, 2, 3])) } },
    { id: 'general', label: 'Solve: general 4x4', spec: { ...base, task: 'solve', matrices: mats(gridOf([[2, 1, -1, 3], [-3, -1, 2, 1], [-2, 1, 2, 5], [4, 0, 1, -2]]), columnGrid([5, -2, 4, 3])) } },
    { id: 'overdet', label: 'Solve: fit a line to 20 points (over-determined)', spec: { ...base, task: 'solve', matrices: mats(line, dataSource({ rows: 20, cols: 1, values: fitB.map((v) => [v]) }, 'example')) } },
    { id: 'underdet', label: 'Solve: 2 equations, 4 unknowns (minimum norm)', spec: { ...base, task: 'solve', matrices: mats(gridOf([[1, 2, 3, 4], [0, 1, 0, 1]]), columnGrid([10, 3])) } },
    { id: 'rankdef', label: 'Solve: rank-deficient 3x3', spec: { ...base, task: 'solve', matrices: mats(gridOf([[1, 2, 3], [2, 4, 6], [1, 1, 1]]), columnGrid([6, 12, 3])) } },
    { id: 'hilbert', label: 'Solve: Hilbert 8x8 (ill-conditioned), true x = ones', spec: { ...base, task: 'solve', matrices: mats(H8, columnGrid(hb)) } },
    { id: 'poisson', label: 'Solve: 1-D Poisson, n = 500 (large sparse)', spec: { ...base, task: 'solve', matrices: mats({ kind: 'builder', builder: { kind: 'poisson1d', n: 500 } }, { kind: 'builder', builder: { kind: 'ones', rows: 500, cols: 1 } }) } },
    { id: 'lsq-hilbert', label: 'Least squares: Hilbert 8x8, see the accuracy gap', spec: { ...base, task: 'lsq', lsq: { weighted: false, truth: 'ones' }, matrices: mats(H8, columnGrid(hb)) } },
    { id: 'lsq-vander', label: 'Least squares: degree-9 Vandermonde fit, 30 points', spec: { ...base, task: 'lsq', lsq: { weighted: false, truth: 'ones' }, matrices: mats({ kind: 'builder', builder: { kind: 'vandermonde', points: 30, cols: 10 } }, columnGrid([1])) } },
    { id: 'decomp', label: 'Decompositions: the 3x3 SPD matrix, SVD', spec: { ...base, task: 'decomp', decomp: { kind: 'svd' }, matrices: mats(spdA, columnGrid([1, 2, 3])) } },
    { id: 'eig', label: 'Eigenproblem: a rotation block plus a real eigenvalue', spec: { ...base, task: 'eig', matrices: mats(gridOf([[0, -2, 0], [2, 0, 0], [0, 0, 5]]), columnGrid([1, 2, 3])) } },
    { id: 'sylvester', label: 'Equation: Sylvester A X + X B = C', spec: { ...base, task: 'matfun', matfun: { kind: 'sylvester', power: 3 }, matrices: mats(gridOf([[-2, 1, 0], [0, -3, 1], [1, 0, -4]]), columnGrid([1, 2, 3]), { B: gridOf([[1, 2], [0, 3]]), C: gridOf([[1, 2], [3, 4], [5, 6]]) }) } },
  ];
}

export const EXAMPLES: Example[] = makeExamples();

export const TASKS: Array<{ id: TaskId; label: string; blurb: string }> = [
  { id: 'solve', label: 'Solve A x = b', blurb: 'Picks a method from the structure of A, solves, and checks the residual and the conditioning.' },
  { id: 'decomp', label: 'Decompositions', blurb: 'LU, QR, Cholesky, SVD, eigen or Schur: the factors and the identity that proves them.' },
  { id: 'props', label: 'Properties', blurb: 'Determinant, rank, trace, norms, condition number, null space, range, pseudo-inverse, inverse.' },
  { id: 'lsq', label: 'Least squares', blurb: 'Normal equations, QR and SVD side by side: see the accuracy each one keeps.' },
  { id: 'matfun', label: 'Matrix functions', blurb: 'expm, sqrtm, logm, powers, and the Lyapunov and Sylvester equations, with the residual.' },
  { id: 'eig', label: 'Eigenproblems', blurb: 'All eigenvalues in the complex plane; for symmetric matrices the eigenvectors and their orthogonality.' },
];

export const DECOMPS: Array<{ id: DecompKind; label: string; call: string }> = [
  { id: 'lu', label: 'LU (partial pivoting)', call: 'lu(A)' },
  { id: 'qr', label: 'QR', call: 'qr(A)' },
  { id: 'chol', label: 'Cholesky', call: 'chol(A)' },
  { id: 'svd', label: 'SVD', call: 'svd(A)' },
  { id: 'eig', label: 'Eigen-decomposition', call: 'eig(A)' },
  { id: 'schur', label: 'Schur', call: 'schur(A)' },
];

export const MATFUNS: Array<{ id: MatFunKind; label: string; call: string; needs: MatrixName[] }> = [
  { id: 'expm', label: 'Matrix exponential', call: 'expm(A)', needs: ['A'] },
  { id: 'sqrtm', label: 'Matrix square root', call: 'sqrtm(A)', needs: ['A'] },
  { id: 'logm', label: 'Matrix logarithm', call: 'logm(A)', needs: ['A'] },
  { id: 'mpower', label: 'Integer power A^p', call: 'A ^ p', needs: ['A'] },
  { id: 'lyap', label: 'Lyapunov A X + X A\' + Q = 0', call: 'lyap(A, Q)', needs: ['A', 'C'] },
  { id: 'sylvester', label: 'Sylvester A X + X B = C', call: 'sylvester(A, B, C)', needs: ['A', 'B', 'C'] },
];

export const SOLVE_METHODS: Array<{ id: SolveMethodId; label: string; call: string }> = [
  { id: 'cholesky', label: 'Cholesky (symmetric positive definite)', call: 'chol(A), solve(L, b), solve(L\', y)' },
  { id: 'lu', label: 'LU with partial pivoting', call: 'solve(A, b)' },
  { id: 'triangular', label: 'Triangular substitution', call: 'solve(A, b)' },
  { id: 'qr', label: 'QR least squares', call: 'qr(A), solve(R, Q\' b)' },
  { id: 'svd', label: 'SVD least squares (minimum norm)', call: 'svd(A)' },
  { id: 'lstsq', label: 'lstsq: minimum-norm least squares', call: 'lstsq(A, b)' },
  { id: 'pinv', label: 'Pseudo-inverse', call: 'pinv(A) * b' },
  { id: 'cg', label: 'Conjugate gradient (sparse, SPD)', call: 'sparse.cg(S, b)' },
  { id: 'gmres', label: 'GMRES (sparse, general)', call: 'sparse.gmres(S, b)' },
  { id: 'sparselu', label: 'Sparse LU', call: 'sparse.solve(S, b)' },
  { id: 'tikhonov', label: 'Tikhonov (regularised)', call: 'svd(A), filter s/(s^2+lambda)' },
  { id: 'tsvd', label: 'Truncated SVD (regularised)', call: 'svd(A), keep k values' },
];

/** The matrices a task reads. */
export function matricesNeeded(spec: LinalgSpec): MatrixName[] {
  switch (spec.task) {
    case 'solve': return ['A', 'b'];
    case 'lsq': return spec.lsq.truth === 'ones' ? (spec.lsq.weighted ? ['A', 'C'] : ['A']) : spec.lsq.weighted ? ['A', 'b', 'C'] : ['A', 'b'];
    case 'matfun': return MATFUNS.find((m) => m.id === spec.matfun.kind)!.needs;
    default: return ['A'];
  }
}

const MATRIX_ROLES: Record<MatrixName, string> = {
  A: 'A',
  b: 'b (right-hand side)',
  B: 'B',
  C: 'C',
};
export function matrixRole(spec: LinalgSpec, n: MatrixName): string {
  if (n === 'C' && spec.task === 'lsq') return 'C (weights, one per row of A)';
  if (n === 'C' && spec.task === 'matfun' && spec.matfun.kind === 'lyap') return 'C (the Q of the equation)';
  if (n === 'B' && spec.task === 'matfun') return 'B (square)';
  return MATRIX_ROLES[n];
}

/** A plain-words problem with the spec, or null. Run before anything reaches the engine. */
export function validateSpec(spec: LinalgSpec, flags?: Partial<Record<MatrixName, MatrixFlags | null>>): string | null {
  const need = matricesNeeded(spec);
  const dims: Partial<Record<MatrixName, { rows: number; cols: number }>> = {};
  for (const n of need) {
    const p = sourceProblem(n, spec.matrices[n]);
    if (p) return p;
    dims[n] = sourceDims(spec.matrices[n])!;
  }
  const A = dims.A!;
  const square = (what: string) => (A.rows === A.cols ? null : `${what} needs a square matrix, but A is ${A.rows} x ${A.cols}.`);
  switch (spec.task) {
    case 'solve': {
      const b = dims.b!;
      if (b.rows !== A.rows) return `Sizes do not match: A is ${A.rows} x ${A.cols}, so b needs ${A.rows} rows, but b has ${b.rows}.`;
      const m = spec.solve.method === 'auto' ? null : spec.solve.method;
      if (m) {
        const f = flags?.A ?? null;
        const p = methodProblem(m, f ?? { rows: A.rows, cols: A.cols } as MatrixFlags, b.cols, !!f);
        if (p) return p;
      }
      if (spec.solve.method === 'tikhonov' && !(spec.solve.lambda > 0)) return 'The regularisation lambda must be above 0.';
      if (spec.solve.method === 'tsvd' && !(Number.isInteger(spec.solve.tsvdK) && spec.solve.tsvdK >= 1)) return 'Truncated SVD keeps a whole number of singular values, 1 or more.';
      return null;
    }
    case 'decomp': {
      const k = spec.decomp.kind;
      if (k === 'lu' || k === 'chol' || k === 'eig' || k === 'schur') {
        const p = square(DECOMPS.find((d) => d.id === k)!.label);
        if (p) return p;
      }
      const fa = flags?.A;
      if (k === 'chol' && fa) {
        if (!fa.symmetric) return 'Cholesky needs a symmetric matrix, and A is not symmetric.';
        if (fa.spd === 0) return 'A is symmetric but not positive definite, so it has no Cholesky factor.';
      }
      return null;
    }
    case 'lsq': {
      if (A.rows < A.cols) return `Least squares here needs at least as many rows as columns (A is ${A.rows} x ${A.cols}); for an under-determined system use Solve with the minimum-norm method.`;
      if (spec.lsq.truth === 'given' && dims.b!.rows !== A.rows) return `Sizes do not match: A has ${A.rows} rows but b has ${dims.b!.rows}.`;
      if (spec.lsq.truth === 'given' && dims.b!.cols !== 1) return 'Least squares compares methods for one right-hand side: b must have a single column.';
      if (spec.lsq.weighted) {
        const c = dims.C!;
        if (c.cols !== 1 || c.rows !== A.rows) return `The weights C must be one column with ${A.rows} entries (one per row of A); C is ${c.rows} x ${c.cols}.`;
        const w = sourceValues(spec.matrices.C);
        if (w && w.some((r) => !(r[0] > 0))) return 'Weights must all be above 0.';
      }
      return null;
    }
    case 'matfun': {
      const k = spec.matfun.kind;
      const sq = square(MATFUNS.find((m) => m.id === k)!.label);
      if (sq) return sq;
      if (k === 'mpower' && !(Number.isInteger(spec.matfun.power) && Math.abs(spec.matfun.power) <= 64)) return 'The power must be a whole number between -64 and 64.';
      if (k === 'lyap' && (dims.C!.rows !== A.rows || dims.C!.cols !== A.rows)) return `For the Lyapunov equation Q (matrix C) must be ${A.rows} x ${A.rows}, like A; C is ${dims.C!.rows} x ${dims.C!.cols}.`;
      if (k === 'sylvester') {
        const B = dims.B!;
        const C = dims.C!;
        if (B.rows !== B.cols) return `For the Sylvester equation B must be square; B is ${B.rows} x ${B.cols}.`;
        if (C.rows !== A.rows || C.cols !== B.rows) return `For A X + X B = C with A ${A.rows} x ${A.rows} and B ${B.rows} x ${B.rows}, C must be ${A.rows} x ${B.rows}; C is ${C.rows} x ${C.cols}.`;
      }
      return null;
    }
    case 'eig':
      return square('Eigenvalues need');
    case 'props':
      return null;
  }
}

// ------------------------------------------------------------ structure flags

/** One script per matrix: defines it as `M` and measures it. Every flag is a
 *  plain Qu number bound at top level, so the run hands it back. */
export function buildSummaryScript(label: string, src: MatrixSource): string {
  const d = sourceDims(src);
  const heavy = !d || d.rows > DENSE_LIMIT || d.cols > DENSE_LIMIT;
  const def = matrixDefinition('M', src);
  const lines = [
    `# Structure of ${label}: every flag below is measured by Qu`,
    def,
    'm_rows = rows(M)',
    'm_cols = cols(M)',
    'm_nnz = nnz(M)',
    'm_density = m_nnz / (m_rows * m_cols)',
    'm_lower = 0',
    'if nnz(triu(M, 1)) == 0',
    '  m_lower = 1',
    'end if',
    'm_upper = 0',
    'if nnz(tril(M, -1)) == 0',
    '  m_upper = 1',
    'end if',
    '# bandwidths by bisection: the largest k with a nonzero k diagonals away',
    'lo = 0',
    'hi = m_cols - 1',
    'while lo < hi',
    '  mid = floor((lo + hi + 1) / 2)',
    '  if nnz(triu(M, mid)) > 0',
    '    lo = mid',
    '  else',
    '    hi = mid - 1',
    '  end if',
    'end while',
    'm_uband = lo',
    'lo = 0',
    'hi = m_rows - 1',
    'while lo < hi',
    '  mid = floor((lo + hi + 1) / 2)',
    '  if nnz(tril(M, -mid)) > 0',
    '    lo = mid',
    '  else',
    '    hi = mid - 1',
    '  end if',
    'end while',
    'm_lband = lo',
    'm_sym = 0',
    'm_spd = 0',
    'if m_rows == m_cols',
    '  if norm(M - M\') <= 1e-12 * norm(M)',
    '    m_sym = 1',
    '  end if',
    'end if',
  ];
  if (heavy) {
    lines.push(
      '# too large to factorise on every edit: a symmetric matrix with a positive, dominant diagonal is called "likely" SPD',
      'if m_sym == 1',
      '  dd = diag(M)',
      '  slack = dd - (abs(M) * ones(m_rows, 1) - abs(dd))',
      '  if min(dd) > 0',
      '    if min(slack) >= -1e-12 * max(dd)',
      '      m_spd = 2',
      '    end if',
      '  end if',
      'end if',
      'm_rank = nan',
      'm_cond = nan',
      'm_det = nan',
      'm_heavy = 1',
    );
  } else {
    lines.push(
      'if m_sym == 1',
      '  try',
      '    la_l = chol(M)',
      '    m_spd = 1',
      '  catch e',
      '    m_spd = 0',
      '  end',
      'end if',
      'm_rank = rank(M)',
      'm_cond = cond(M)',
      'm_det = nan',
      'if m_rows == m_cols',
      '  m_det = det(M)',
      'end if',
      'm_heavy = 0',
    );
  }
  return lines.join('\n') + '\n';
}

export type RunVars = Record<string, { type: string; shape: number[]; data: (number | null)[] }>;

export function indexVars(list: Array<{ name: string; type?: string; shape?: number[]; data: (number | null)[] }> | undefined): RunVars {
  const out: RunVars = {};
  for (const v of list ?? []) out[v.name] = { type: v.type ?? '', shape: v.shape ?? [v.data.length, 1], data: v.data };
  return out;
}

export function scalarVar(vars: RunVars, name: string): number {
  const v = vars[name];
  if (!v || v.data.length === 0) return NaN;
  const x = v.data[0];
  return x == null ? NaN : x;
}

/** A scalar where JSON null means +inf (the engine's `cond` of a singular matrix). */
export function scalarOrInf(vars: RunVars, name: string): number {
  const v = vars[name];
  if (!v || v.data.length === 0) return NaN;
  const x = v.data[0];
  return x == null ? Infinity : x;
}

export function vectorVar(vars: RunVars, name: string): number[] {
  const v = vars[name];
  return v ? v.data.map((x) => (x == null ? NaN : x)) : [];
}

/** Column-major engine data -> a row-major matrix; an optional imaginary part. */
export function matVar(vars: RunVars, name: string, imName?: string): Mat | null {
  const v = vars[name];
  if (!v) return null;
  const [r, c] = v.shape.length === 2 ? v.shape : [v.data.length, 1];
  const grab = (data: (number | null)[]) =>
    Array.from({ length: r }, (_, i) => Array.from({ length: c }, (_, j) => {
      const x = data[i + j * r];
      return x == null ? NaN : x;
    }));
  const iv = imName ? vars[imName] : undefined;
  const im = iv && iv.data.length === v.data.length ? grab(iv.data) : undefined;
  const imNonzero = im && im.some((row) => row.some((x) => x !== 0 && !Number.isNaN(x)));
  return { rows: r, cols: c, re: grab(v.data), im: imNonzero ? im : undefined };
}

export function flagsFromVars(vars: RunVars): MatrixFlags | null {
  if (!vars['m_rows'] || !vars['m_cols']) return null;
  const heavy = scalarVar(vars, 'm_heavy') === 1;
  return {
    rows: scalarVar(vars, 'm_rows'),
    cols: scalarVar(vars, 'm_cols'),
    symmetric: scalarVar(vars, 'm_sym') === 1,
    spd: (scalarVar(vars, 'm_spd') || 0) as 0 | 1 | 2,
    lower: scalarVar(vars, 'm_lower') === 1,
    upper: scalarVar(vars, 'm_upper') === 1,
    lowerBand: scalarVar(vars, 'm_lband'),
    upperBand: scalarVar(vars, 'm_uband'),
    nnz: scalarVar(vars, 'm_nnz'),
    density: scalarVar(vars, 'm_density'),
    rank: scalarVar(vars, 'm_rank'),
    cond: heavy ? NaN : scalarOrInf(vars, 'm_cond'),
    det: scalarVar(vars, 'm_det'),
    heavy,
  };
}

/** The summary rows a matrix card shows. */
export function describeFlags(f: MatrixFlags, precision = 4): Array<[string, string]> {
  const rows: Array<[string, string]> = [['size', `${f.rows} x ${f.cols}`]];
  rows.push(['symmetric', f.rows === f.cols ? (f.symmetric ? 'yes' : 'no') : 'n/a (not square)']);
  if (f.rows === f.cols) {
    rows.push(['positive definite', f.spd === 1 ? 'yes (Cholesky succeeded)' : f.spd === 2 ? 'likely (diagonally dominant, too large to factorise)' : f.symmetric ? 'no (Cholesky failed)' : 'no (not symmetric)']);
  }
  const tri = f.lower && f.upper ? 'diagonal' : f.lower ? 'lower triangular' : f.upper ? 'upper triangular' : 'no';
  rows.push(['triangular', tri]);
  const banded = f.rows > 2 && Math.max(f.lowerBand, f.upperBand) < Math.min(f.rows, f.cols) - 1;
  rows.push(['banded', banded ? `yes (lower ${f.lowerBand}, upper ${f.upperBand})` : `no (lower ${f.lowerBand}, upper ${f.upperBand})`]);
  rows.push(['sparsity', `${f.nnz} nonzeros of ${f.rows * f.cols} (${formatNumber(f.density * 100, 3)}% filled)`]);
  rows.push(['rank', Number.isNaN(f.rank) ? 'skipped (large)' : `${f.rank} of ${Math.min(f.rows, f.cols)}`]);
  rows.push(['condition number', Number.isNaN(f.cond) ? 'skipped (large)' : formatNumber(f.cond, precision)]);
  rows.push(['determinant', f.rows !== f.cols ? 'n/a (not square)' : Number.isNaN(f.det) ? 'skipped (large)' : formatNumber(f.det, precision)]);
  return rows;
}

// ---------------------------------------------------------------- the advisor

export interface Advice {
  method: SolveMethodId;
  reason: string;
  structure: string;
}

export function structureClass(f: MatrixFlags): string {
  if (f.rank < Math.min(f.rows, f.cols)) return 'rank-deficient';
  if (f.rows > f.cols) return 'over-determined';
  if (f.rows < f.cols) return 'under-determined';
  if (f.lower || f.upper) return 'triangular';
  const largeSparse = f.rows >= LARGE_SPARSE_N && f.density <= LARGE_SPARSE_DENSITY;
  if (largeSparse) return f.spd ? 'large sparse SPD' : Math.max(f.lowerBand, f.upperBand) <= Math.max(2, f.rows / 10) ? 'large banded' : 'large sparse';
  if (f.spd) return 'symmetric positive definite';
  return 'general square';
}

/** The method for this structure, with the one-line reason shown to the user. */
export function adviseSolve(f: MatrixFlags, bCols = 1): Advice {
  const structure = structureClass(f);
  const m = f.rows;
  const n = f.cols;
  const has = (id: SolveMethodId, reason: string): Advice => ({ method: id, reason, structure });
  switch (structure) {
    case 'rank-deficient':
      return has('lstsq', `A is rank-deficient (rank ${f.rank} of ${Math.min(m, n)}), so there is no unique solution: lstsq returns the minimum-norm least-squares one, from the SVD.`);
    case 'over-determined':
      if (f.cond > 1e12) return has('svd', `A has more rows than columns (${m} x ${n}) and is nearly rank-deficient (cond ${formatNumber(f.cond, 3)}): the SVD least-squares solve is the safe choice.`);
      return has('qr', `A has more rows than columns (${m} x ${n}): least squares via QR, which avoids squaring the condition number as the normal equations would.`);
    case 'under-determined':
      return has('lstsq', `A has fewer rows than columns (${m} x ${n}), so there are infinitely many solutions: lstsq returns the one of minimum norm.`);
    case 'triangular':
      return has('triangular', `A is ${f.lower && f.upper ? 'diagonal' : f.lower ? 'lower' : 'upper'} triangular: forward/back substitution needs no factorisation.`);
    case 'large sparse SPD':
      return bCols === 1
        ? has('cg', `A is large (${n} x ${n}), sparse (${formatNumber(f.density * 100, 2)}% filled) and symmetric positive definite: conjugate gradient with a Jacobi preconditioner.`)
        : has('cholesky', 'A is symmetric positive definite: Cholesky (conjugate gradient solves one right-hand side at a time).');
    case 'large banded':
      return bCols === 1
        ? has('sparselu', `A is large and banded (bandwidth ${Math.max(f.lowerBand, f.upperBand)}): sparse LU keeps the fill-in inside the band, and it is not symmetric positive definite so conjugate gradient does not apply.`)
        : has('lu', 'A is general square: LU with partial pivoting.');
    case 'large sparse':
      return bCols === 1
        ? has('gmres', `A is large (${n} x ${n}), sparse (${formatNumber(f.density * 100, 2)}% filled) and not symmetric positive definite: restarted GMRES.`)
        : has('lu', 'A is general square: LU with partial pivoting.');
    case 'symmetric positive definite':
      return has('cholesky', f.spd === 2 ? 'A is symmetric with a dominant positive diagonal (likely positive definite): Cholesky, about twice as fast as LU.' : 'A is symmetric positive definite (a Cholesky factorisation succeeded): Cholesky is about twice as fast as LU and needs no pivoting.');
    default:
      return has('lu', `A is a general ${n} x ${n} matrix: LU with partial pivoting.`);
  }
}

/** Why `method` cannot be used on this system, or null when it can. */
export function methodProblem(method: SolveMethodId, f: MatrixFlags, bCols: number, known = true): string | null {
  const square = f.rows === f.cols;
  switch (method) {
    case 'cholesky':
      if (!square) return `Cholesky needs a square matrix, but A is ${f.rows} x ${f.cols}.`;
      if (known && !f.symmetric) return 'Cholesky needs a symmetric matrix, and A is not symmetric.';
      if (known && f.spd === 0) return 'A is symmetric but not positive definite, so Cholesky would fail.';
      return null;
    case 'lu': case 'triangular':
      return square ? null : `${method === 'lu' ? 'LU' : 'Triangular substitution'} needs a square matrix, but A is ${f.rows} x ${f.cols}. Use QR, SVD or lstsq.`;
    case 'qr':
      if (f.rows < f.cols) return `QR least squares needs at least as many rows as columns (A is ${f.rows} x ${f.cols}); use lstsq for the minimum-norm solution.`;
      if (known && f.rank < f.cols) return 'A is rank-deficient: QR would divide by a zero pivot. Use SVD or lstsq.';
      return null;
    case 'cg': case 'gmres': case 'sparselu':
      if (!square) return `${method === 'cg' ? 'Conjugate gradient' : method === 'gmres' ? 'GMRES' : 'Sparse LU'} needs a square matrix, but A is ${f.rows} x ${f.cols}.`;
      if (bCols !== 1) return 'The sparse solvers take one right-hand side: b must have a single column.';
      if (method === 'cg' && known && f.spd === 0) return 'Conjugate gradient needs a symmetric positive definite matrix, and A is not. Use GMRES or LU.';
      return null;
    default:
      return null;
  }
}

export const METHOD_LABEL = (id: SolveMethodId) => SOLVE_METHODS.find((m) => m.id === id)!.label;

export interface ConditionWarning {
  level: 'ok' | 'warn' | 'severe' | 'singular';
  digitsLost: number;
  text: string;
}

/** Plain words about what a condition number means for a solve in double precision. */
export function conditionWarning(cond: number): ConditionWarning {
  if (Number.isNaN(cond)) return { level: 'ok', digitsLost: 0, text: '' };
  const lost = Number.isFinite(cond) ? Math.max(0, Math.log10(cond)) : 16;
  if (!Number.isFinite(cond) || cond > 4.5e15) {
    return { level: 'singular', digitsLost: 16, text: 'A is numerically singular: its condition number is at or beyond 1 / machine-epsilon, so a solve keeps no reliable digits. The answer below is not trustworthy as a solution of A x = b; use a regularised method.' };
  }
  if (cond > 1e12) return { level: 'severe', digitsLost: lost, text: `A is very ill-conditioned (condition number ${formatNumber(cond, 3)}): a solve loses about ${Math.round(lost)} of the 16 digits double precision holds, so only about ${Math.max(0, 16 - Math.round(lost))} can be right. Do not trust the digits; consider a regularised solve.` };
  if (cond > 1e8) return { level: 'warn', digitsLost: lost, text: `A is ill-conditioned (condition number ${formatNumber(cond, 3)}): a solve loses about ${Math.round(lost)} of the 16 digits double precision holds, and small errors in b are amplified by that factor. A regularised solve is more reliable.` };
  return { level: 'ok', digitsLost: lost, text: '' };
}

// ---------------------------------------------------------------- the scripts

/** What one result card shows and which variables of the run feed it. */
export interface CardSpec {
  id: string;
  title: string;
  /** The Qu call that produced it, shown on the card. */
  call: string;
  kind: 'matrix' | 'scalar' | 'check' | 'values' | 'bars' | 'plane' | 'compare';
  /** variable names read from the run (matrix: [re, im?]; plane: [re, im]; values: [vec]) */
  vars: string[];
  /** check: passes when the value is at most this (a relative residual) */
  tol?: number;
  note?: string;
}

export interface ScriptPlan {
  script: string;
  cards: CardSpec[];
  /** One line naming the method, shown above the results. */
  headline: string;
  method?: SolveMethodId;
}

const TOL = 1e-8;

const head = (spec: LinalgSpec, task: string, extra: string[] = []) => [`# Qu Studio / Linear Algebra: ${task}`, ...extra];

function defs(spec: LinalgSpec, names: MatrixName[]): string[] {
  const out: string[] = [];
  for (const n of names) out.push(matrixDefinition(n, spec.matrices[n]));
  return out;
}

const NZ = (expr: string) => `max(${expr}, 1e-300)`;

export function resolveSolveMethod(spec: LinalgSpec, f?: MatrixFlags | null): { method: SolveMethodId; reason: string; auto: boolean } {
  const d = sourceDims(spec.matrices.A) ?? { rows: 1, cols: 1 };
  const bc = sourceDims(spec.matrices.b)?.cols ?? 1;
  if (spec.solve.method !== 'auto') return { method: spec.solve.method, reason: 'chosen by you', auto: false };
  const flags: MatrixFlags = f ?? { rows: d.rows, cols: d.cols, symmetric: false, spd: 0, lower: false, upper: false, lowerBand: d.rows, upperBand: d.cols, nnz: d.rows * d.cols, density: 1, rank: NaN, cond: NaN, det: NaN, heavy: false };
  const adv = adviseSolve(f ?? flags, bc);
  return { method: adv.method, reason: f ? adv.reason : `${adv.reason} (structure not measured yet)`, auto: true };
}

export function buildSolveScript(spec: LinalgSpec, f?: MatrixFlags | null): ScriptPlan {
  const { method, reason, auto } = resolveSolveMethod(spec, f);
  const dA = sourceDims(spec.matrices.A)!;
  const heavy = dA.rows > DENSE_LIMIT || dA.cols > DENSE_LIMIT;
  const sparseMethod = method === 'cg' || method === 'gmres' || method === 'sparselu';
  const L: string[] = head(spec, 'solve A x = b', [`# method: ${METHOD_LABEL(method)} -- ${auto ? 'chosen by the advisor' : 'chosen by you'}: ${reason}`]);
  if (sparseMethod) L.push('import sparse');
  L.push(...defs(spec, ['A', 'b']));
  L.push('n = cols(A)');
  switch (method) {
    case 'cholesky':
      L.push('L = chol(A)            # A = L L\'', 'y = solve(L, b)         # forward substitution', 'x = solve(L\', y)        # back substitution');
      break;
    case 'lu':
    case 'triangular':
      L.push(method === 'lu' ? 'x = solve(A, b)         # LU with partial pivoting' : 'x = solve(A, b)         # triangular: the LU factors are trivial');
      break;
    case 'qr':
      L.push('f = qr(A)              # A = Q R', 'x = solve(f.r, f.q\' * b)   # R x = Q\' b');
      break;
    case 'svd':
      L.push(
        'f = svd(A)',
        `tol = max(rows(A), cols(A)) * ${EPS} * f.s[0]   # singular values below this count as zero`,
        'sinv = where(f.s > tol, 1 ./ f.s, 0)',
        'x = f.vt\' * diag(sinv) * (f.u\' * b)     # x = V S^+ U\' b: the minimum-norm solution',
      );
      break;
    case 'lstsq':
      L.push('x = lstsq(A, b)        # minimum-norm least squares');
      break;
    case 'pinv':
      L.push('x = pinv(A) * b        # pseudo-inverse');
      break;
    case 'tikhonov':
      L.push(
        `lambda = ${numLit(spec.solve.lambda)}`,
        'f = svd(A)',
        'x = f.vt\' * diag(f.s ./ (f.s .^ 2 + lambda)) * (f.u\' * b)   # minimises |A x - b|^2 + lambda |x|^2',
      );
      break;
    case 'tsvd':
      L.push(
        `kk = ${spec.solve.tsvdK}                 # keep the ${spec.solve.tsvdK} largest singular values`,
        'f = svd(A)',
        'idx = linspace(0, length(f.s) - 1, length(f.s))',
        'sinv = where(idx < kk, 1 ./ f.s, 0)',
        'x = f.vt\' * diag(sinv) * (f.u\' * b)',
      );
      break;
    case 'cg':
    case 'gmres':
    case 'sparselu':
      L.push('S = sparse.from_dense(A)', 'bv = b[:, 0]');
      if (method === 'cg') L.push('sol = sparse.cg(S, bv, precond="jacobi")', 'x = reshape(sol.x, n, 1)', 'iterations = sol.iterations', 'converged = sol.converged');
      if (method === 'gmres') L.push('sol = sparse.gmres(S, bv, precond="jacobi")', 'x = reshape(sol.x, n, 1)', 'iterations = sol.iterations', 'converged = sol.converged');
      if (method === 'sparselu') L.push('x = reshape(sparse.solve(S, bv), n, 1)', 'iterations = 0', 'converged = true');
      break;
  }
  L.push('# checks: how well does x solve the system, and how trustworthy is the matrix?');
  L.push(`relres = norm(A * x - b) / ${NZ('norm(b)')}`);
  L.push(`gradres = norm(A' * (A * x - b)) / ${NZ('norm(A) * norm(b)')}   # zero exactly when x is a least-squares solution`);
  L.push('nx = norm(x)');
  if (!heavy) L.push('condA = cond(A)');
  const callLabel = SOLVE_METHODS.find((m) => m.id === method)!.call;
  const regularised = method === 'tikhonov' || method === 'tsvd';
  const leastSquares = method === 'qr' || method === 'svd' || method === 'lstsq' || method === 'pinv';
  const cards: CardSpec[] = [{ id: 'x', title: 'Solution x', call: callLabel, kind: 'matrix', vars: ['x'] }];
  if (leastSquares) {
    cards.push(
      { id: 'relres', title: 'Relative residual |A x - b| / |b|', call: 'norm(A * x - b) / norm(b)', kind: 'scalar', vars: ['relres'], note: 'zero when the system is consistent; for a least-squares problem a nonzero residual is expected' },
      { id: 'gradres', title: "Least-squares optimality |A' (A x - b)| / (|A| |b|)", call: "norm(A' * (A * x - b)) / (norm(A) * norm(b))", kind: 'check', vars: ['gradres'], tol: TOL, note: 'passes for an exact solution and for the best least-squares fit alike' },
    );
  } else if (regularised) {
    cards.push(
      { id: 'relres', title: 'Relative residual |A x - b| / |b|', call: 'norm(A * x - b) / norm(b)', kind: 'scalar', vars: ['relres'], note: 'regularisation trades a small residual for a stable, smaller x: the residual is not meant to be zero' },
      { id: 'nx', title: 'Size of the solution |x|', call: 'norm(x)', kind: 'scalar', vars: ['nx'] },
    );
  } else {
    cards.push({ id: 'relres', title: 'Relative residual |A x - b| / |b|', call: 'norm(A * x - b) / norm(b)', kind: 'check', vars: ['relres'], tol: TOL, note: 'small means x solves the system' });
  }
  if (!heavy) cards.push({ id: 'cond', title: 'Condition number of A', call: 'cond(A)', kind: 'scalar', vars: ['condA'] });
  if (sparseMethod && method !== 'sparselu') cards.push({ id: 'iters', title: 'Iterations', call: callLabel, kind: 'scalar', vars: ['iterations'] });
  return { script: L.join('\n') + '\n', cards, headline: `${METHOD_LABEL(method)}: ${reason}`, method };
}

export function buildDecompScript(spec: LinalgSpec): ScriptPlan {
  const k = spec.decomp.kind;
  const meta = DECOMPS.find((d) => d.id === k)!;
  const L: string[] = head(spec, `${meta.label}`);
  L.push(...defs(spec, ['A']));
  L.push('na = max(norm(A), 1e-300)');
  const cards: CardSpec[] = [];
  const check = (id: string, title: string, call: string, tol = 1e-12, note?: string): CardSpec => ({ id, title, call, kind: 'check', vars: [id], tol, note });
  switch (k) {
    case 'lu':
      L.push('f = lu(A)              # P A = L U', 'Lf = f.l', 'Uf = f.u', 'Pf = f.p', 'err_lu = norm(Pf * A - Lf * Uf) / na');
      cards.push(
        { id: 'L', title: 'L (unit lower triangular)', call: 'lu(A).l', kind: 'matrix', vars: ['Lf'] },
        { id: 'U', title: 'U (upper triangular)', call: 'lu(A).u', kind: 'matrix', vars: ['Uf'] },
        { id: 'P', title: 'P (permutation)', call: 'lu(A).p', kind: 'matrix', vars: ['Pf'] },
        check('err_lu', '|P A - L U| / |A|', 'norm(P * A - L * U) / norm(A)'),
      );
      break;
    case 'qr':
      L.push('f = qr(A)              # A = Q R', 'Qf = f.q', 'Rf = f.r', 'err_qr = norm(A - Qf * Rf) / na', 'err_orth = norm(Qf\' * Qf - eye(cols(Qf)))');
      cards.push(
        { id: 'Q', title: 'Q (orthonormal columns)', call: 'qr(A).q', kind: 'matrix', vars: ['Qf'] },
        { id: 'R', title: 'R (upper triangular)', call: 'qr(A).r', kind: 'matrix', vars: ['Rf'] },
        check('err_qr', '|A - Q R| / |A|', 'norm(A - Q * R) / norm(A)'),
        check('err_orth', '|Q\' Q - I|', 'norm(Q\' * Q - eye(k))', 1e-12, 'orthogonality of Q'),
      );
      break;
    case 'chol':
      L.push('Lc = chol(A)           # A = L L\'', 'err_chol = norm(A - Lc * Lc\') / na');
      cards.push(
        { id: 'Lc', title: 'L (lower triangular)', call: 'chol(A)', kind: 'matrix', vars: ['Lc'] },
        check('err_chol', '|A - L L\'| / |A|', 'norm(A - L * L\') / norm(A)'),
      );
      break;
    case 'svd':
      L.push('f = svd(A)              # A = U S V\'', 'Uf = f.u', 'sv = f.s', 'Vt = f.vt', 'err_svd = norm(A - Uf * diag(sv) * Vt) / na', 'err_u = norm(Uf\' * Uf - eye(cols(Uf)))', 'err_v = norm(Vt * Vt\' - eye(rows(Vt)))');
      cards.push(
        { id: 'sv', title: 'Singular values', call: 'svd(A).s', kind: 'bars', vars: ['sv'] },
        { id: 'U', title: 'U (left singular vectors)', call: 'svd(A).u', kind: 'matrix', vars: ['Uf'] },
        { id: 'Vt', title: 'V\' (right singular vectors, transposed)', call: 'svd(A).vt', kind: 'matrix', vars: ['Vt'] },
        check('err_svd', '|A - U S V\'| / |A|', 'norm(A - U * diag(s) * Vt) / norm(A)'),
        check('err_u', '|U\' U - I|', 'norm(U\' * U - eye(k))', 1e-12, 'orthogonality of U'),
        check('err_v', '|V\' V - I|', 'norm(Vt * Vt\' - eye(k))', 1e-12, 'orthogonality of V'),
      );
      break;
    case 'eig':
      L.push(...eigLines(), 'err_trace = abs(sum(ev_re) - trace(A)) / na');
      cards.push(
        { id: 'plane', title: 'Eigenvalues in the complex plane', call: 'eig(A).values', kind: 'plane', vars: ['ev_re', 'ev_im'] },
        { id: 'values', title: 'Eigenvalues', call: 'eig(A).values', kind: 'values', vars: ['ev_re', 'ev_im'] },
        ...eigVectorCards(),
        check('err_trace', '|sum(eigenvalues) - trace(A)| / |A|', 'sum(eig(A).values) - trace(A)', 1e-10),
      );
      break;
    case 'schur':
      L.push('f = schur(A)            # A = U T U\'', 'Tf = f.T', 'Uf = f.U', 'err_schur = norm(A - Uf * Tf * Uf\') / na', 'err_orth = norm(Uf\' * Uf - eye(cols(Uf)))');
      cards.push(
        { id: 'T', title: 'T (quasi upper triangular)', call: 'schur(A).T', kind: 'matrix', vars: ['Tf'] },
        { id: 'U', title: 'U (orthogonal)', call: 'schur(A).U', kind: 'matrix', vars: ['Uf'] },
        check('err_schur', '|A - U T U\'| / |A|', 'norm(A - U * T * U\') / norm(A)'),
        check('err_orth', '|U\' U - I|', 'norm(U\' * U - eye(n))', 1e-12, 'orthogonality of U'),
      );
      break;
  }
  return { script: L.join('\n') + '\n', cards, headline: `${meta.label} via ${meta.call}` };
}

function eigLines(): string[] {
  return [
    'e = eig(A)',
    'ev = e.values',
    'ev_re = real(ev)',
    'ev_im = imag(ev)',
    'sym = 0',
    'if norm(A - A\') <= 1e-12 * max(norm(A), 1e-300)',
    '  sym = 1',
    '  V = e.vectors',
    '  err_av = norm(A * V - V * diag(ev_re)) / max(norm(A), 1e-300)',
    '  err_vo = norm(V\' * V - eye(rows(V)))',
    'end if',
  ];
}
function eigVectorCards(): CardSpec[] {
  return [
    { id: 'V', title: 'Eigenvectors (columns), symmetric A only', call: 'eig(A).vectors', kind: 'matrix', vars: ['V'], note: 'Qu returns eigenvectors for symmetric matrices; for a non-symmetric A only the eigenvalues are available.' },
    { id: 'err_av', title: '|A V - V D| / |A|', call: 'norm(A * V - V * diag(lambda))', kind: 'check', vars: ['err_av'], tol: 1e-12 },
    { id: 'err_vo', title: '|V\' V - I|', call: 'norm(V\' * V - eye(n))', kind: 'check', vars: ['err_vo'], tol: 1e-12, note: 'orthogonality of the eigenvectors' },
  ];
}

export function buildEigScript(spec: LinalgSpec): ScriptPlan {
  const L: string[] = head(spec, 'eigenvalues', ['# for a symmetric matrix the eigenvectors and their orthogonality are checked too']);
  L.push(...defs(spec, ['A']), ...eigLines());
  L.push('lam_min = min(ev_re)', 'lam_max = max(ev_re)', 'spectral_radius = max(sqrt(ev_re .^ 2 + ev_im .^ 2))', 'err_trace = abs(sum(ev_re) - trace(A)) / max(norm(A), 1e-300)');
  const cards: CardSpec[] = [
    { id: 'plane', title: 'Eigenvalues in the complex plane', call: 'eig(A).values', kind: 'plane', vars: ['ev_re', 'ev_im'] },
    { id: 'values', title: 'Eigenvalues', call: 'eig(A).values', kind: 'values', vars: ['ev_re', 'ev_im'] },
    { id: 'rho', title: 'Spectral radius', call: 'max(abs(eig(A).values))', kind: 'scalar', vars: ['spectral_radius'] },
    ...eigVectorCards(),
    { id: 'err_trace', title: '|sum(eigenvalues) - trace(A)| / |A|', call: 'sum(eig(A).values) - trace(A)', kind: 'check', vars: ['err_trace'], tol: 1e-10 },
  ];
  return { script: L.join('\n') + '\n', cards, headline: 'Eigenvalues via eig(A)' };
}

export function buildPropsScript(spec: LinalgSpec): ScriptPlan {
  const dA = sourceDims(spec.matrices.A)!;
  const sq = dA.rows === dA.cols;
  const L: string[] = head(spec, 'properties');
  L.push(...defs(spec, ['A']));
  L.push(
    'rank_A = rank(A)',
    'trace_A = trace(A)',
    '# induced norms are not built in: 1-norm = largest column sum, inf-norm = largest row sum, 2-norm = largest singular value',
    'norm1 = max(ones(1, rows(A)) * abs(A))',
    'norm_inf = max(abs(A) * ones(cols(A), 1))',
    'norm2 = svd(A).s[0]',
    'norm_fro = norm(A)',
    'cond_A = cond(A)',
    'N = null(A)                  # orthonormal basis of the null space',
    'O = orth(A)                  # orthonormal basis of the range',
    'P = pinv(A)                  # pseudo-inverse',
    'null_dim = cols(N)',
    'err_null = norm(A * N)',
    'err_pinv = norm(A * P * A - A) / max(norm(A), 1e-300)',
  );
  const cards: CardSpec[] = [];
  if (sq) {
    L.push('det_A = det(A)');
  }
  L.push('inv_ok = 0');
  if (sq) {
    L.push(
      'try',
      '  Ainv = inv(A)',
      '  inv_ok = 1',
      '  err_inv = norm(A * Ainv - eye(rows(A)))',
      'catch e',
      '  print("@@inv_error " + e.message)',
      'end',
    );
  } else {
    L.push('print("@@inv_error A is not square, so it has no inverse; the pseudo-inverse P is shown instead")');
  }
  cards.push(
    ...(sq ? [{ id: 'det', title: 'Determinant', call: 'det(A)', kind: 'scalar', vars: ['det_A'] } as CardSpec] : []),
    { id: 'rank', title: 'Rank', call: 'rank(A)', kind: 'scalar', vars: ['rank_A'] },
    { id: 'trace', title: 'Trace', call: 'trace(A)', kind: 'scalar', vars: ['trace_A'] },
    { id: 'norm1', title: '1-norm (max column sum)', call: 'max(ones(1, m) * abs(A))', kind: 'scalar', vars: ['norm1'] },
    { id: 'norm2', title: '2-norm (largest singular value)', call: 'svd(A).s[0]', kind: 'scalar', vars: ['norm2'] },
    { id: 'norminf', title: 'inf-norm (max row sum)', call: 'max(abs(A) * ones(n, 1))', kind: 'scalar', vars: ['norm_inf'] },
    { id: 'normfro', title: 'Frobenius norm', call: 'norm(A)', kind: 'scalar', vars: ['norm_fro'] },
    { id: 'cond', title: 'Condition number (2-norm)', call: 'cond(A)', kind: 'scalar', vars: ['cond_A'] },
    { id: 'N', title: 'Null space basis', call: 'null(A)', kind: 'matrix', vars: ['N'], note: 'a matrix with zero columns means A has full column rank: only x = 0 solves A x = 0' },
    { id: 'err_null', title: '|A N|', call: 'norm(A * null(A))', kind: 'check', vars: ['err_null'], tol: 1e-10 },
    { id: 'O', title: 'Range (column space) basis', call: 'orth(A)', kind: 'matrix', vars: ['O'] },
    { id: 'P', title: 'Pseudo-inverse', call: 'pinv(A)', kind: 'matrix', vars: ['P'] },
    { id: 'err_pinv', title: '|A P A - A| / |A|', call: 'norm(A * pinv(A) * A - A) / norm(A)', kind: 'check', vars: ['err_pinv'], tol: 1e-10 },
    ...(sq ? [{ id: 'Ainv', title: 'Inverse (solve is usually better than inv)', call: 'inv(A)', kind: 'matrix', vars: ['Ainv'], note: 'Forming an inverse is slower and less accurate than solve(A, b); use it only when you need the matrix itself.' } as CardSpec, { id: 'err_inv', title: '|A inv(A) - I|', call: 'norm(A * inv(A) - eye(n))', kind: 'check', vars: ['err_inv'], tol: 1e-8 } as CardSpec] : []),
  );
  return { script: L.join('\n') + '\n', cards, headline: 'Properties of A' };
}

export function buildLsqScript(spec: LinalgSpec): ScriptPlan {
  const truth = spec.lsq.truth === 'ones';
  const names: MatrixName[] = truth ? ['A'] : ['A', 'b'];
  if (spec.lsq.weighted) names.push('C');
  const L: string[] = head(spec, 'least squares, three ways', ['# the same problem solved by the normal equations, by QR and by the SVD']);
  L.push(...defs(spec, names));
  L.push('n = cols(A)');
  if (truth) L.push('x_true = ones(n, 1)', 'b = A * x_true        # exact data: the true answer is known');
  if (spec.lsq.weighted) {
    L.push('sw = sqrt(C)                          # minimise sum of w_i (A x - b)_i^2', 'Aw = A .* (sw * ones(1, n))', 'bw = b .* sw');
  } else {
    L.push('Aw = A', 'bw = b');
  }
  L.push(
    'cond_A = cond(Aw)',
    'cond_normal = cond(Aw\' * Aw)    # the normal equations square the condition number',
    '# 1. normal equations',
    'ne_failed = 0',
    'x_ne = zeros(n, 1)',
    'try',
    '  x_ne = solve(Aw\' * Aw, Aw\' * bw)',
    'catch e',
    '  ne_failed = 1',
    '  print("@@ne_error " + e.message)',
    'end',
    '# 2. QR',
    'f = qr(Aw)',
    'x_qr = solve(f.r, f.q\' * bw)',
    '# 3. SVD',
    'g = svd(Aw)',
    `tol = max(rows(Aw), cols(Aw)) * ${EPS} * g.s[0]`,
    'sinv = where(g.s > tol, 1 ./ g.s, 0)',
    'x_svd = g.vt\' * diag(sinv) * (g.u\' * bw)',
  );
  L.push(truth ? 'x_ref = x_true' : 'x_ref = x_svd              # no known answer: the SVD solution is the reference');
  L.push(
    'res_ne = norm(Aw * x_ne - bw)',
    'res_qr = norm(Aw * x_qr - bw)',
    'res_svd = norm(Aw * x_svd - bw)',
    'nref = max(norm(x_ref), 1e-300)',
    'err_ne = norm(x_ne - x_ref) / nref',
    'err_qr = norm(x_qr - x_ref) / nref',
    'err_svd = norm(x_svd - x_ref) / nref',
  );
  const cards: CardSpec[] = [
    { id: 'compare', title: truth ? 'Error against the known answer' : 'Difference from the SVD solution', call: 'solve(A\'A, A\'b) / qr(A) / svd(A)', kind: 'compare', vars: ['err_ne', 'err_qr', 'err_svd', 'res_ne', 'res_qr', 'res_svd'] },
    { id: 'cond', title: 'Condition number of A', call: 'cond(A)', kind: 'scalar', vars: ['cond_A'] },
    { id: 'condn', title: 'Condition number of A\'A (normal equations)', call: 'cond(A\' * A)', kind: 'scalar', vars: ['cond_normal'] },
    { id: 'x_ne', title: 'x from the normal equations', call: 'solve(A\' * A, A\' * b)', kind: 'matrix', vars: ['x_ne'] },
    { id: 'x_qr', title: 'x from QR', call: 'solve(qr(A).r, qr(A).q\' * b)', kind: 'matrix', vars: ['x_qr'] },
    { id: 'x_svd', title: 'x from the SVD', call: 'svd(A): V S^+ U\' b', kind: 'matrix', vars: ['x_svd'] },
  ];
  return { script: L.join('\n') + '\n', cards, headline: 'Normal equations vs QR vs SVD' };
}

export function buildMatFunScript(spec: LinalgSpec): ScriptPlan {
  const k = spec.matfun.kind;
  const meta = MATFUNS.find((m) => m.id === k)!;
  const L: string[] = head(spec, meta.label);
  L.push(...defs(spec, meta.needs));
  if (k === 'sylvester') L.push('# solve A X + X B = C for X');
  if (k === 'lyap') L.push('# solve A X + X A\' + Q = 0 for X, with Q = C');
  const cards: CardSpec[] = [];
  const res = (id: string, title: string, call: string, tol = 1e-8, note?: string): CardSpec => ({ id, title, call, kind: 'check', vars: [id], tol, note });
  const cplx = ['Y_re = real(Y)', 'Y_im = imag(Y)'];
  switch (k) {
    case 'expm':
      L.push('Y = expm(A)', 'err_fn = norm(Y * expm(-A) - eye(rows(A)))   # e^A e^-A = I', ...cplx);
      cards.push({ id: 'Y', title: 'e^A', call: 'expm(A)', kind: 'matrix', vars: ['Y_re', 'Y_im'] }, res('err_fn', '|e^A e^-A - I|', 'norm(expm(A) * expm(-A) - eye(n))', 1e-8, 'a relative check for large entries is looser'));
      break;
    case 'sqrtm':
      L.push('Y = sqrtm(A)', 'err_fn = norm(Y * Y - A) / max(norm(A), 1e-300)', ...cplx);
      cards.push({ id: 'Y', title: 'Principal square root', call: 'sqrtm(A)', kind: 'matrix', vars: ['Y_re', 'Y_im'], note: 'a negative real eigenvalue makes the principal root complex' }, res('err_fn', '|Y Y - A| / |A|', 'norm(sqrtm(A) * sqrtm(A) - A) / norm(A)', 1e-9));
      break;
    case 'logm':
      L.push('Y = logm(A)', 'err_fn = nan', 'try', '  err_fn = norm(expm(Y) - A) / max(norm(A), 1e-300)', 'catch e', '  print("@@check_skipped expm does not take a complex matrix, so the round trip cannot be checked here")', 'end', ...cplx);
      cards.push({ id: 'Y', title: 'Principal logarithm', call: 'logm(A)', kind: 'matrix', vars: ['Y_re', 'Y_im'] }, res('err_fn', '|expm(logm(A)) - A| / |A|', 'norm(expm(logm(A)) - A) / norm(A)', 1e-8));
      break;
    case 'mpower': {
      const p = spec.matfun.power;
      L.push(`p = ${p}`, 'Y = A ^ p');
      if (p === 0) L.push('err_fn = norm(Y - eye(rows(A)))        # A^0 = I');
      else if (p > 0) L.push('chk = eye(rows(A))', 'for k = 1 to p', '  chk = chk * A', 'end for', 'err_fn = norm(Y - chk) / max(norm(Y), 1e-300)   # repeated multiplication');
      else L.push('Z = A ^ (0 - p)', 'err_fn = norm(Y * Z - eye(rows(A)))        # A^p A^-p = I');
      L.push(...cplx);
      cards.push({ id: 'Y', title: `A^${p}`, call: 'A ^ p', kind: 'matrix', vars: ['Y_re', 'Y_im'] }, res('err_fn', p >= 0 ? '|A^p - A*A*...*A| / |A^p|' : '|A^p A^-p - I|', p >= 0 ? 'norm(A ^ p - repeated product) / norm(A ^ p)' : 'norm(A ^ p * A ^ -p - I)', 1e-8));
      break;
    }
    case 'lyap':
      L.push('Y = lyap(A, C)', 'err_fn = norm(A * Y + Y * A\' + C) / max(norm(C), 1e-300)', ...cplx);
      cards.push({ id: 'Y', title: 'X', call: 'lyap(A, Q)', kind: 'matrix', vars: ['Y_re', 'Y_im'] }, res('err_fn', '|A X + X A\' + Q| / |Q|', 'norm(A * X + X * A\' + Q) / norm(Q)', 1e-9));
      break;
    case 'sylvester':
      L.push('Y = sylvester(A, B, C)', 'err_fn = norm(A * Y + Y * B - C) / max(norm(C), 1e-300)', ...cplx);
      cards.push({ id: 'Y', title: 'X', call: 'sylvester(A, B, C)', kind: 'matrix', vars: ['Y_re', 'Y_im'] }, res('err_fn', '|A X + X B - C| / |C|', 'norm(A * X + X * B - C) / norm(C)', 1e-9));
      break;
  }
  return { script: L.join('\n') + '\n', cards, headline: `${meta.label}: ${meta.call}` };
}

/** The script, result cards and headline for the spec's current task. */
export function buildLinalgScript(spec: LinalgSpec, flagsA?: MatrixFlags | null): ScriptPlan {
  switch (spec.task) {
    case 'solve': return buildSolveScript(spec, flagsA);
    case 'decomp': return buildDecompScript(spec);
    case 'props': return buildPropsScript(spec);
    case 'lsq': return buildLsqScript(spec);
    case 'matfun': return buildMatFunScript(spec);
    case 'eig': return buildEigScript(spec);
  }
}

// ------------------------------------------------------------- reading a run

/** Lines `@@key text` a script printed (messages that numbers cannot carry). */
export function parseMarkers(output: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of output.split(/\r?\n/)) {
    const m = /^@@(\w+)\s*(.*)$/.exec(line.trim());
    if (m) out[m[1]] = m[2];
  }
  return out;
}

export interface EngineError {
  /** plain words */
  message: string;
  /** the engine's own text */
  raw: string;
  line?: number;
  source?: string;
}

const PLAIN: Array<[RegExp, string]> = [
  [/Cholesky decomposition failed/i, 'The matrix is not symmetric positive definite, so it has no Cholesky factor. Use LU, or check that A is symmetric and its eigenvalues are all positive.'],
  [/matrix is singular \(rank (\d+) of (\d+)\)/i, 'The matrix is singular, so it has no inverse or unique solution. Use the pseudo-inverse (pinv) or a least-squares method.'],
  [/inner dimensions to agree/i, 'The sizes do not match for this product or solve: check the rows and columns of A and b.'],
  [/expected a square matrix/i, 'This operation needs a square matrix.'],
  [/do not broadcast/i, 'The sizes of the matrices do not match.'],
  [/no unique solution|share an eigenvalue/i, 'The equation has no unique solution: the eigenvalues of the two sides cancel.'],
];

/** The engine's error text in plain words, with the generated line that failed. */
export function describeEngineError(raw: string, script: string): EngineError {
  const text = raw.trim();
  const first = text.split(/\r?\n/).find((l) => /error|failed|unknown|expected|needs/i.test(l)) ?? text.split(/\r?\n/)[0] ?? text;
  const lineMatch = /:(\d+)(?::\d+)?\s*$/m.exec(text);
  const line = lineMatch ? Number(lineMatch[1]) : undefined;
  const source = line ? script.split('\n')[line - 1] : undefined;
  const clean = first.replace(/^qu:\s*(runtime error|parse error)?:?\s*/i, '').trim();
  const hit = PLAIN.find(([re]) => re.test(text));
  return { message: hit ? `${hit[1]} (Qu said: ${clean})` : clean, raw: text, line, source };
}

/** Digits of agreement from a relative error, for the accuracy bars (capped at 16). */
export function correctDigits(err: number): number {
  if (!Number.isFinite(err)) return 0;
  if (err <= 1e-16) return 16;
  return Math.max(0, Math.min(16, -Math.log10(err)));
}

/** Pass/fail for a check card. */
export function checkPasses(value: number, tol: number): boolean {
  return Number.isFinite(value) && value <= tol;
}
