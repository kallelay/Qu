/** The engine gate for the Linear Algebra panel: skipped unless QU_BIN names a
 *  qu executable. Each scenario builds the script exactly as the panel does
 *  (summary run for the advisor's flags, then the task script), runs the real
 *  binary, and checks the numbers against truth built here independently of the
 *  code under test: x is chosen first and b = A x computed in this file, spectra
 *  are constructed from known orthogonal matrices, and so on. */
import { describe, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  DEFAULT_SPEC,
  EXAMPLES,
  adviseSolve,
  buildLinalgScript,
  buildSummaryScript,
  checkPasses,
  conditionWarning,
  describeEngineError,
  flagsFromVars,
  gridOf,
  columnGrid,
  indexVars,
  matVar,
  parseMarkers,
  scalarOrInf,
  scalarVar,
  sourceValues,
  stagedBytes,
  validateSpec,
  vectorVar,
  type CardSpec,
  type LinalgSpec,
  type MatrixFlags,
  type MatrixSource,
  type RunVars,
  type ScriptPlan,
} from './linalgDesign';

const QU = process.env.QU_BIN;
const gate = QU ? describe : describe.skip;

interface Run {
  ok: boolean;
  stdout: string;
  stderr: string;
  vars: RunVars;
}

const dir = QU ? mkdtempSync(join(tmpdir(), 'linalg-gate-')) : '';
let seq = 0;

function runQu(script: string): Run {
  const id = ++seq;
  const file = join(dir, `s${id}.qu`);
  const out = join(dir, `s${id}.json`);
  writeFileSync(file, script);
  const r = spawnSync(QU!, ['run', file, '--emit-data', out], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 120_000, encoding: 'utf8' });
  let vars: RunVars = {};
  try {
    vars = indexVars(JSON.parse(readFileSync(out, 'utf8')));
  } catch {
    /* a failed run writes no data */
  }
  return { ok: r.status === 0 && !/runtime error|parse error/.test(r.stderr ?? ''), stdout: r.stdout ?? '', stderr: r.stderr ?? '', vars };
}

/** What the panel does: measure A first, then write the task script from those flags. */
function panelRun(spec: LinalgSpec): { plan: ScriptPlan; run: Run; flags: MatrixFlags | null } {
  const summary = runQu(buildSummaryScript('A', spec.matrices.A));
  expect(summary.ok, summary.stderr).toBe(true);
  const flags = flagsFromVars(summary.vars);
  const plan = buildLinalgScript(spec, flags);
  const run = runQu(plan.script);
  return { plan, run, flags };
}

const withSpec = (patch: Omit<Partial<LinalgSpec>, 'matrices'> & { matrices?: Partial<LinalgSpec['matrices']> }): LinalgSpec => ({
  ...DEFAULT_SPEC,
  ...patch,
  matrices: { ...DEFAULT_SPEC.matrices, ...(patch.matrices ?? {}) },
});

// ---------------------------------------------------- independent arithmetic
type M = number[][];
const mul = (a: M, b: M): M => a.map((r) => b[0].map((_, j) => r.reduce((s, v, k) => s + v * b[k][j], 0)));
const mv = (a: M, x: number[]): number[] => a.map((r) => r.reduce((s, v, k) => s + v * x[k], 0));
const col = (v: number[]): M => v.map((x) => [x]);
const fl = (m: M) => m.map((r) => r[0]);
const norm = (v: number[]) => Math.sqrt(v.reduce((s, x) => s + x * x, 0));
const sub = (a: number[], b: number[]) => a.map((x, i) => x - b[i]);
const T = (a: M): M => a[0].map((_, j) => a.map((r) => r[j]));
const solve2 = (a: M, b: number[]): number[] => {
  const n = b.length;
  const m = a.map((r, i) => [...r, b[i]]);
  for (let c = 0; c < n; c++) {
    let p = c;
    for (let r = c + 1; r < n; r++) if (Math.abs(m[r][c]) > Math.abs(m[p][c])) p = r;
    [m[c], m[p]] = [m[p], m[c]];
    for (let r = c + 1; r < n; r++) {
      const f = m[r][c] / m[c][c];
      for (let k = c; k <= n; k++) m[r][k] -= f * m[c][k];
    }
  }
  const x = new Array(n).fill(0);
  for (let i = n - 1; i >= 0; i--) x[i] = (m[i][n] - m[i].slice(i + 1, n).reduce((s, v, k) => s + v * x[i + 1 + k], 0)) / m[i][i];
  return x;
};
const num = (run: Run, name: string) => scalarVar(run.vars, name);
const vec = (run: Run, name: string) => matVar(run.vars, name)!.re.map((r) => r[0]);

const poisson = (n: number): MatrixSource => ({ kind: 'builder', builder: { kind: 'poisson1d', n } });
const hilbert = (n: number): MatrixSource => ({ kind: 'builder', builder: { kind: 'hilbert', n } });

function solveSpec(A: MatrixSource, xTrue: number[], patch: Partial<LinalgSpec['solve']> = {}): { spec: LinalgSpec; xTrue: number[] } {
  const a = sourceValues(A)!;
  const b = mv(a, xTrue);
  return { spec: withSpec({ task: 'solve', matrices: { A, b: columnGrid(b) }, solve: { ...DEFAULT_SPEC.solve, ...patch } }), xTrue };
}

gate('engine gate: solve with the advisor', () => {
  it('SPD: chooses Cholesky and recovers the exact x (b = A x built here)', () => {
    const x = [1, -2, 3, 0.5, 4, -1, 2, 7];
    const { spec } = solveSpec(poisson(8), x);
    const { plan, run, flags } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(flags!.spd).toBe(1);
    expect(plan.method).toBe('cholesky');
    expect(norm(sub(vec(run, 'x'), x)) / norm(x)).toBeLessThan(1e-12);
    expect(num(run, 'relres')).toBeLessThan(1e-14);
  });

  it('general square: chooses LU and recovers x', () => {
    const A = [[2, 1, -1, 3], [-3, -1, 2, 1], [-2, 1, 2, 5], [4, 0, 1, -2]];
    const x = [1, 2, -1, 3];
    const { spec } = solveSpec(gridOf(A), x);
    const { plan, run, flags } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(flags!.symmetric).toBe(false);
    expect(plan.method).toBe('lu');
    expect(norm(sub(vec(run, 'x'), x))).toBeLessThan(1e-12);
  });

  it('triangular: chooses substitution', () => {
    const A = [[2, 0, 0], [1, 3, 0], [4, -1, 5]];
    const x = [3, -1, 2];
    const { spec } = solveSpec(gridOf(A), x);
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(plan.method).toBe('triangular');
    expect(norm(sub(vec(run, 'x'), x))).toBeLessThan(1e-13);
  });

  it('over-determined: chooses QR and recovers a polynomial exactly', () => {
    const A: MatrixSource = { kind: 'builder', builder: { kind: 'vandermonde', points: 20, cols: 3 } };
    const x = [2, -3, 1];
    const { spec } = solveSpec(A, x);
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(plan.method).toBe('qr');
    expect(norm(sub(vec(run, 'x'), x))).toBeLessThan(1e-10);
    expect(num(run, 'relres')).toBeLessThan(1e-12);
  });

  it('under-determined: chooses lstsq and returns the MINIMUM-norm solution', () => {
    const A = [[1, 2, 3, 4], [0, 1, 0, 1]];
    const b = [10, 3];
    // truth: x* = A' (A A')^-1 b, computed here
    const y = solve2(mul(A, T(A)), b);
    const xmin = mv(T(A), y);
    const spec = withSpec({ task: 'solve', matrices: { A: gridOf(A), b: columnGrid(b) } });
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(plan.method).toBe('lstsq');
    expect(norm(sub(vec(run, 'x'), xmin))).toBeLessThan(1e-12);
    expect(num(run, 'relres')).toBeLessThan(1e-13);
  });

  it('rank-deficient: chooses lstsq, solves the consistent system, x orthogonal to the null space', () => {
    const A = [[1, 2, 3], [2, 4, 6], [1, 1, 1]]; // null space spanned by [1, -2, 1]
    const b = mv(A, [1, 1, 1]);
    const spec = withSpec({ task: 'solve', matrices: { A: gridOf(A), b: columnGrid(b) } });
    const { plan, run, flags } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(flags!.rank).toBe(2);
    expect(plan.method).toBe('lstsq');
    const x = vec(run, 'x');
    expect(num(run, 'relres')).toBeLessThan(1e-12);
    expect(Math.abs(x[0] * 1 + x[1] * -2 + x[2] * 1)).toBeLessThan(1e-12); // minimum norm = no null-space part
  });

  it('ill-conditioned Hilbert 8x8: residual is tiny, the error is not, and the advisor warns', () => {
    const x = new Array(8).fill(1);
    const { spec } = solveSpec(hilbert(8), x);
    const { plan, run, flags } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    // the published 2-norm condition number of the 8x8 Hilbert matrix is 1.5258e10
    expect(Math.abs(flags!.cond / 1.5258e10 - 1)).toBeLessThan(0.01);
    expect(plan.method).toBe('cholesky'); // it is SPD in exact arithmetic and for Cholesky here
    const w = conditionWarning(scalarOrInf(run.vars, 'condA'));
    expect(w.level).toBe('warn'); // 1.5e10 is above 1e8 and below 1e12
    expect(w.text).toMatch(/ill-conditioned/);
    expect(num(run, 'relres')).toBeLessThan(1e-14); // the residual looks perfect...
    expect(norm(sub(vec(run, 'x'), x))).toBeGreaterThan(1e-9); // ...while x is visibly off
  });

  it('large sparse SPD (n = 250): chooses conjugate gradient and it converges', () => {
    const n = 250;
    const x = Array.from({ length: n }, (_, i) => Math.sin(i / 7));
    const { spec } = solveSpec(poisson(n), x);
    const { plan, run, flags } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(flags!.density).toBeLessThan(0.1);
    expect(plan.method).toBe('cg');
    expect(num(run, 'converged')).toBe(1);
    expect(norm(sub(vec(run, 'x'), x)) / norm(x)).toBeLessThan(1e-6);
    expect(num(run, 'relres')).toBeLessThan(1e-9);
  });

  it('large banded non-symmetric (n = 250): chooses sparse LU', () => {
    const n = 250;
    const A: M = Array.from({ length: n }, (_, i) => Array.from({ length: n }, (_, j) => (i === j ? 4 : j === i + 1 ? -1 : j === i - 1 ? -2 : 0)));
    const x = Array.from({ length: n }, (_, i) => Math.cos(i / 5));
    const { spec } = solveSpec(gridOf(A), x);
    // 62500 entries: staged as a file, as the panel does for large data
    const src: MatrixSource = { kind: 'data', rows: n, cols: n, values: A.flat(), origin: 'test' };
    const f = join(dir, 'banded.f64');
    writeFileSync(f, Buffer.from(stagedBytes(src as never).buffer));
    (src as { file?: string }).file = f;
    const sp = withSpec({ ...spec, matrices: { A: src, b: columnGrid(mv(A, x)) } });
    const { plan, run } = panelRun(sp);
    expect(run.ok, run.stderr).toBe(true);
    expect(plan.method).toBe('sparselu');
    expect(norm(sub(vec(run, 'x'), x)) / norm(x)).toBeLessThan(1e-10);
  });

  it('large sparse unsymmetric, wide band (n = 250): chooses GMRES and it converges', () => {
    const n = 250;
    const A: M = Array.from({ length: n }, () => new Array(n).fill(0));
    for (let i = 0; i < n; i++) {
      A[i][i] = 4;
      A[i][(i * 7 + 3) % n] += -0.5;
      A[i][(i * 13 + 100) % n] += 0.3;
    }
    const x = Array.from({ length: n }, (_, i) => 1 + (i % 5));
    const src: MatrixSource = { kind: 'data', rows: n, cols: n, values: A.flat(), origin: 'test' };
    const f = join(dir, 'sparse.f64');
    writeFileSync(f, Buffer.from(stagedBytes(src as never).buffer));
    (src as { file?: string }).file = f;
    const spec = withSpec({ task: 'solve', matrices: { A: src, b: columnGrid(mv(A, x)) } });
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(plan.method).toBe('gmres');
    expect(num(run, 'converged')).toBe(1);
    expect(norm(sub(vec(run, 'x'), x)) / norm(x)).toBeLessThan(1e-6);
  });

  it('every override method runs and agrees on a well-conditioned system', () => {
    const A = [[4, 1, 0], [1, 3, 1], [0, 1, 2]];
    const x = [1, 2, 3];
    for (const m of ['cholesky', 'lu', 'qr', 'svd', 'lstsq', 'pinv', 'cg', 'gmres', 'sparselu'] as const) {
      const { spec } = solveSpec(gridOf(A), x, { method: m });
      expect(validateSpec(spec), m).toBeNull();
      const { run } = panelRun(spec);
      expect(run.ok, `${m}: ${run.stderr}`).toBe(true);
      expect(norm(sub(vec(run, 'x'), x)), m).toBeLessThan(1e-9);
    }
  });

  it('regularised methods: Tikhonov with tiny lambda and TSVD with all values reproduce the solution; TSVD k=1 does not', () => {
    const A = [[3, 0, 0], [0, 2, 0], [0, 0, 1]];
    const x = [1, 1, 1];
    const tik = panelRun(solveSpec(gridOf(A), x, { method: 'tikhonov', lambda: 1e-12 }).spec).run;
    expect(tik.ok, tik.stderr).toBe(true);
    expect(norm(sub(vec(tik, 'x'), x))).toBeLessThan(1e-9);
    const full = panelRun(solveSpec(gridOf(A), x, { method: 'tsvd', tsvdK: 3 }).spec).run;
    expect(norm(sub(vec(full, 'x'), x))).toBeLessThan(1e-12);
    const one = panelRun(solveSpec(gridOf(A), x, { method: 'tsvd', tsvdK: 1 }).spec).run;
    expect(one.ok, one.stderr).toBe(true);
    const x1 = vec(one, 'x');
    expect(Math.abs(x1[0] - 1) + Math.abs(x1[1]) + Math.abs(x1[2])).toBeLessThan(1e-12); // keeps only the sigma = 3 direction: [1, 0, 0]
  });
});

// ------------------------------------------------------------ decompositions
/** Every check card of a plan must pass; returns their values by id. */
function checks(plan: ScriptPlan, run: Run): Record<string, number> {
  const out: Record<string, number> = {};
  for (const c of plan.cards.filter((k: CardSpec) => k.kind === 'check')) {
    if ((c.id === 'err_av' || c.id === 'err_vo') && !run.vars[c.vars[0]]) continue; // eigenvectors exist only for symmetric A
    const v = num(run, c.vars[0]);
    out[c.id] = v;
    expect(checkPasses(v, c.tol!), `${c.id} = ${v} (tol ${c.tol})`).toBe(true);
  }
  return out;
}

// A = H diag(d) H with H the (symmetric, orthogonal) 4x4 Hadamard matrix / 2: known spectrum d
const H4: M = [[1, 1, 1, 1], [1, -1, 1, -1], [1, 1, -1, -1], [1, -1, -1, 1]].map((r) => r.map((v) => v / 2));
const diagOf = (d: number[]): M => d.map((v, i) => d.map((_, j) => (i === j ? v : 0)));
const D = [5, 3, 2, -1];
const SYM = mul(mul(H4, diagOf(D)), H4);
const GEN = [[2, 1, -1, 3], [-3, -1, 2, 1], [-2, 1, 2, 5], [4, 0, 1, -2]];
const maxDiff = (a: M, b: M) => Math.max(...a.flatMap((r, i) => r.map((v, j) => Math.abs(v - b[i][j]))));

gate('engine gate: decompositions', () => {
  const dec = (kind: LinalgSpec['decomp']['kind'], A: M) => {
    const spec = withSpec({ task: 'decomp', decomp: { kind }, matrices: { A: gridOf(A) } });
    expect(validateSpec(spec)).toBeNull();
    return panelRun(spec);
  };

  it('LU: P A = L U, L unit lower, U upper', () => {
    const { plan, run } = dec('lu', GEN);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const L = matVar(run.vars, 'Lf')!.re;
    const U = matVar(run.vars, 'Uf')!.re;
    for (let i = 0; i < 4; i++) {
      expect(L[i][i]).toBeCloseTo(1, 12);
      for (let j = 0; j < 4; j++) {
        if (j > i) expect(L[i][j]).toBe(0);
        if (j < i) expect(U[i][j]).toBe(0);
      }
    }
    const P = matVar(run.vars, 'Pf')!.re;
    expect(maxDiff(mul(P, GEN), mul(L, U))).toBeLessThan(1e-12); // independent reconstruction
  });

  it('QR of a 6x3 matrix: Q orthonormal, R upper triangular, Q R = A', () => {
    const A: M = [[1, 2, 0], [3, -1, 4], [0, 5, 1], [2, 2, 2], [-1, 0, 3], [4, 1, -2]];
    const { plan, run } = dec('qr', A);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const Q = matVar(run.vars, 'Qf')!.re;
    const R = matVar(run.vars, 'Rf')!.re;
    expect(Q.length).toBe(6);
    expect(R.length).toBe(3);
    for (let i = 0; i < 3; i++) for (let j = 0; j < i; j++) expect(Math.abs(R[i][j])).toBeLessThan(1e-14);
    expect(maxDiff(mul(Q, R), A)).toBeLessThan(1e-13);
    expect(maxDiff(mul(T(Q), Q), diagOf([1, 1, 1]))).toBeLessThan(1e-13);
  });

  it('Cholesky of a constructed SPD matrix: L L-transpose = A', () => {
    const A = mul(mul(H4, diagOf([5, 3, 2, 1])), H4);
    const { plan, run, flags } = dec('chol', A);
    expect(flags!.spd).toBe(1);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const L = matVar(run.vars, 'Lc')!.re;
    expect(maxDiff(mul(L, T(L)), A)).toBeLessThan(1e-13);
  });

  it('Cholesky of a matrix that is not positive definite is refused up front, and by the engine in plain words', () => {
    const spec = withSpec({ task: 'decomp', decomp: { kind: 'chol' }, matrices: { A: gridOf(SYM) } }); // has eigenvalue -1
    const summary = runQu(buildSummaryScript('A', spec.matrices.A));
    const flags = flagsFromVars(summary.vars)!;
    expect(flags.symmetric).toBe(true);
    expect(flags.spd).toBe(0);
    expect(validateSpec(spec, { A: flags })).toMatch(/not positive definite/);
    // bypass the check on purpose: the engine must still refuse, clearly
    const plan = buildLinalgScript(spec);
    const run = runQu(plan.script);
    expect(run.ok).toBe(false);
    const err = describeEngineError(run.stderr, plan.script);
    expect(err.message).toMatch(/not symmetric positive definite/);
    expect(err.line).toBeGreaterThan(0);
    expect(err.source).toMatch(/chol/);
  });

  it('SVD: singular values equal |d| for A = H diag(d) H (known), factors reconstruct and are orthogonal', () => {
    const { plan, run } = dec('svd', SYM);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const s = vectorVar(run.vars, 'sv');
    const expected = D.map(Math.abs).sort((a, b) => b - a);
    s.forEach((v, i) => expect(v).toBeCloseTo(expected[i], 10));
  });

  it('eig of the same matrix returns exactly the constructed spectrum, with orthogonal eigenvectors', () => {
    const { plan, run } = dec('eig', SYM);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const ev = vectorVar(run.vars, 'ev_re').sort((a, b) => a - b);
    [...D].sort((a, b) => a - b).forEach((v, i) => expect(ev[i]).toBeCloseTo(v, 10));
    vectorVar(run.vars, 'ev_im').forEach((v) => expect(v).toBe(0));
    expect(num(run, 'err_av')).toBeLessThan(1e-12);
    expect(num(run, 'err_vo')).toBeLessThan(1e-12);
  });

  it('eig of a non-symmetric matrix with a complex pair: spectrum {2j, -2j, 5}', () => {
    const { run } = dec('eig', [[0, -2, 0], [2, 0, 0], [0, 0, 5]]);
    expect(run.ok, run.stderr).toBe(true);
    const re = vectorVar(run.vars, 'ev_re');
    const im = vectorVar(run.vars, 'ev_im');
    const got = re.map((r, i) => [Math.round(r * 1e8) / 1e8 + 0, Math.round(im[i] * 1e8) / 1e8 + 0]).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    expect(got).toEqual([[0, -2], [0, 2], [5, 0]]);
    expect(run.vars['V']).toBeUndefined(); // Qu gives eigenvectors only for symmetric matrices
  });

  it('Schur: A = U T U-transpose, U orthogonal, T quasi-triangular with trace(T) = trace(A)', () => {
    const { plan, run } = dec('schur', GEN);
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    const Tm = matVar(run.vars, 'Tf')!.re;
    for (let i = 2; i < 4; i++) for (let j = 0; j < i - 1; j++) expect(Math.abs(Tm[i][j])).toBeLessThan(1e-12);
    expect(Tm.reduce((s, r, i) => s + r[i], 0)).toBeCloseTo(GEN.reduce((s, r, i) => s + r[i], 0), 10);
  });
});

// ---------------------------------------------------------------- properties
gate('engine gate: properties', () => {
  it('known values for [[1,2],[3,4]]: det -2, rank 2, trace 5, norms 6 / 7 / sqrt(30) / 5.4649857, cond 14.933034', () => {
    const spec = withSpec({ task: 'props', matrices: { A: gridOf([[1, 2], [3, 4]]) } });
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(num(run, 'det_A')).toBeCloseTo(-2, 12);
    expect(num(run, 'rank_A')).toBe(2);
    expect(num(run, 'trace_A')).toBe(5);
    expect(num(run, 'norm1')).toBe(6);
    expect(num(run, 'norm_inf')).toBe(7);
    expect(num(run, 'norm_fro')).toBeCloseTo(Math.sqrt(30), 12);
    expect(num(run, 'norm2')).toBeCloseTo(5.464985704219043, 10);
    expect(num(run, 'cond_A')).toBeCloseTo(14.933034373659268, 8);
    const inv = matVar(run.vars, 'Ainv')!.re;
    expect(inv[0][0]).toBeCloseTo(-2, 12);
    expect(inv[0][1]).toBeCloseTo(1, 12);
    expect(inv[1][0]).toBeCloseTo(1.5, 12);
    expect(inv[1][1]).toBeCloseTo(-0.5, 12);
    expect(matVar(run.vars, 'N')!.cols).toBe(0); // full rank: trivial null space
    checks(plan, run);
  });

  it('singular matrix: rank 1, a null vector, and inv reports a clear message instead of failing the run', () => {
    const spec = withSpec({ task: 'props', matrices: { A: gridOf([[1, 2], [2, 4]]) } });
    const { run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(num(run, 'rank_A')).toBe(1);
    expect(num(run, 'null_dim')).toBe(1);
    const N = matVar(run.vars, 'N')!.re;
    expect(Math.abs(N[0][0] * 1 + N[1][0] * 2)).toBeLessThan(1e-12);
    expect(Math.abs(N[0][0] * 2 + N[1][0] * 4)).toBeLessThan(1e-12);
    expect(num(run, 'inv_ok')).toBe(0);
    expect(parseMarkers(run.stdout)['inv_error']).toMatch(/singular/);
  });

  it('a rectangular matrix has no inverse but still gets rank, norms, null space and pinv', () => {
    const spec = withSpec({ task: 'props', matrices: { A: gridOf([[1, 2, 3], [4, 5, 6]]) } });
    const { plan, run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(num(run, 'rank_A')).toBe(2);
    expect(num(run, 'null_dim')).toBe(1);
    expect(parseMarkers(run.stdout)['inv_error']).toMatch(/not square/);
    expect(plan.cards.some((c) => c.id === 'Ainv')).toBe(false);
    checks(plan, run);
  });
});

// ------------------------------------------------------------- least squares
gate('engine gate: least squares, three ways', () => {
  it('Hilbert 8x8 with known x = ones: normal equations lose most digits, QR and SVD keep far more', () => {
    const spec = withSpec({ task: 'lsq', lsq: { weighted: false, truth: 'ones' }, matrices: { A: hilbert(8) } });
    expect(validateSpec(spec)).toBeNull();
    const { run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    const ne = num(run, 'err_ne');
    const qr = num(run, 'err_qr');
    const svd = num(run, 'err_svd');
    const cn = scalarOrInf(run.vars, 'cond_normal'); // may be +inf: A'A is numerically singular
    expect(cn / num(run, 'cond_A')).toBeGreaterThan(1e8); // roughly cond squared
    expect(qr).toBeLessThan(1e-3);
    expect(svd).toBeLessThan(1e-3);
    expect(ne).toBeGreaterThan(10 * Math.max(qr, svd)); // the accuracy gap is the whole point
  });

  it('well-conditioned 30x3 Vandermonde fit: all three agree with the true coefficients', () => {
    const spec = withSpec({ task: 'lsq', matrices: { A: { kind: 'builder', builder: { kind: 'vandermonde', points: 30, cols: 3 } } } });
    const { run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    for (const k of ['err_ne', 'err_qr', 'err_svd']) expect(num(run, k)).toBeLessThan(1e-10);
  });

  it('given b and weights: the three methods agree on a noisy fit, and weights change the answer as the weighted normal equations say', () => {
    const A = [[1, 0], [1, 1], [1, 2], [1, 3], [1, 4]];
    const b = [1.1, 2.9, 5.2, 6.8, 9.1];
    const run0 = panelRun(withSpec({ task: 'lsq', lsq: { weighted: false, truth: 'given' }, matrices: { A: gridOf(A), b: columnGrid(b) } })).run;
    expect(run0.ok, run0.stderr).toBe(true);
    const xref = solve2(mul(T(A), A), mv(T(A), b)); // independent normal equations
    expect(norm(sub(vec(run0, 'x_qr'), xref))).toBeLessThan(1e-10);
    expect(norm(sub(vec(run0, 'x_svd'), xref))).toBeLessThan(1e-10);
    expect(norm(sub(vec(run0, 'x_ne'), xref))).toBeLessThan(1e-10);
    const w = [1, 1, 1, 1, 100];
    const run1 = panelRun(withSpec({ task: 'lsq', lsq: { weighted: true, truth: 'given' }, matrices: { A: gridOf(A), b: columnGrid(b), C: columnGrid(w) } })).run;
    expect(run1.ok, run1.stderr).toBe(true);
    const WA = A.map((r, i) => r.map((v) => v * w[i]));
    const xw = solve2(mul(T(A), WA), mv(T(WA), b)); // (A' W A) x = A' W b
    expect(norm(sub(vec(run1, 'x_qr'), xw))).toBeLessThan(1e-9);
    expect(norm(sub(vec(run1, 'x_qr'), xref))).toBeGreaterThan(1e-3);
  });

  it('rejects an under-determined or badly weighted problem before the engine', () => {
    expect(validateSpec(withSpec({ task: 'lsq', matrices: { A: gridOf([[1, 2, 3], [4, 5, 6]]) } }))).toMatch(/at least as many rows/);
    expect(validateSpec(withSpec({ task: 'lsq', lsq: { weighted: true, truth: 'ones' }, matrices: { A: hilbert(4), C: columnGrid([1, 2]) } }))).toMatch(/weights/);
  });
});

// ----------------------------------------------------- functions and equations
gate('engine gate: matrix functions and equations', () => {
  const fun = (kind: LinalgSpec['matfun']['kind'], A: M, extra: Partial<LinalgSpec['matrices']> = {}, power = 3) => {
    const spec = withSpec({ task: 'matfun', matfun: { kind, power }, matrices: { A: gridOf(A), ...extra } });
    expect(validateSpec(spec), kind).toBeNull();
    return panelRun(spec);
  };
  const Y = (run: Run) => matVar(run.vars, 'Y_re')!.re;

  it('expm of a rotation generator is the rotation matrix', () => {
    const t = 0.7;
    const { plan, run } = fun('expm', [[0, -t], [t, 0]]);
    expect(run.ok, run.stderr).toBe(true);
    const y = Y(run);
    expect(y[0][0]).toBeCloseTo(Math.cos(t), 12);
    expect(y[0][1]).toBeCloseTo(-Math.sin(t), 12);
    expect(y[1][0]).toBeCloseTo(Math.sin(t), 12);
    checks(plan, run);
  });

  it('sqrtm of diag(4, 9) is diag(2, 3), and Y Y = A for a non-diagonal SPD matrix', () => {
    const d = fun('sqrtm', [[4, 0], [0, 9]]);
    expect(Y(d.run)[0][0]).toBeCloseTo(2, 12);
    expect(Y(d.run)[1][1]).toBeCloseTo(3, 12);
    const g = fun('sqrtm', [[4, 1], [1, 3]]);
    checks(g.plan, g.run);
    const y = Y(g.run);
    expect(maxDiff(mul(y, y), [[4, 1], [1, 3]])).toBeLessThan(1e-12);
  });

  it('sqrtm with a negative eigenvalue gives a complex root, delivered with its imaginary part', () => {
    const { run } = fun('sqrtm', [[-4, 0], [0, 1]]);
    expect(run.ok, run.stderr).toBe(true);
    const m = matVar(run.vars, 'Y_re', 'Y_im')!;
    expect(m.im).toBeDefined();
    expect(m.im![0][0]).toBeCloseTo(2, 12);
    expect(m.re[1][1]).toBeCloseTo(1, 12);
  });

  it('logm: expm(logm(A)) = A, and logm(expm(M)) = M for a small M', () => {
    const l = fun('logm', [[4, 1], [1, 3]]);
    checks(l.plan, l.run);
    const M0 = [[0.1, 0.2], [-0.2, 0.3]];
    const eA = Y(fun('expm', M0).run);
    const y = Y(fun('logm', eA).run);
    expect(maxDiff(y, M0)).toBeLessThan(1e-9);
  });

  it('integer powers: [[1,1],[0,1]]^5 = [[1,5],[0,1]]; negative power is the inverse power; zero is I', () => {
    const p = fun('mpower', [[1, 1], [0, 1]], {}, 5);
    expect(Y(p.run)).toEqual([[1, 5], [0, 1]]);
    checks(p.plan, p.run);
    const n = fun('mpower', [[2, 0], [0, 4]], {}, -2);
    expect(Y(n.run)[0][0]).toBeCloseTo(0.25, 14);
    expect(Y(n.run)[1][1]).toBeCloseTo(1 / 16, 14);
    checks(n.plan, n.run);
    expect(Y(fun('mpower', [[2, 1], [1, 3]], {}, 0).run)).toEqual([[1, 0], [0, 1]]);
  });

  it('lyap: A = diag(-1, -2), Q = I gives X = diag(1/2, 1/4)', () => {
    const { plan, run } = fun('lyap', [[-1, 0], [0, -2]], { C: gridOf([[1, 0], [0, 1]]) });
    expect(run.ok, run.stderr).toBe(true);
    const y = Y(run);
    expect(y[0][0]).toBeCloseTo(0.5, 12);
    expect(y[1][1]).toBeCloseTo(0.25, 12);
    expect(Math.abs(y[0][1])).toBeLessThan(1e-14);
    checks(plan, run);
  });

  it('sylvester: A = diag(1,2), B = diag(3,4), C = ones gives X_ij = 1 / (a_i + b_j)', () => {
    const { plan, run } = fun('sylvester', [[1, 0], [0, 2]], { B: gridOf([[3, 0], [0, 4]]), C: gridOf([[1, 1], [1, 1]]) });
    expect(run.ok, run.stderr).toBe(true);
    const y = Y(run);
    expect(y[0][0]).toBeCloseTo(1 / 4, 12);
    expect(y[0][1]).toBeCloseTo(1 / 5, 12);
    expect(y[1][0]).toBeCloseTo(1 / 5, 12);
    expect(y[1][1]).toBeCloseTo(1 / 6, 12);
    checks(plan, run);
  });

  it('a general lyap and sylvester satisfy their equations to residual tolerance', () => {
    const A = [[-2, 1, 0], [0, -3, 1], [1, 0, -4]];
    const l = fun('lyap', A, { C: gridOf([[2, 1, 0], [1, 2, 1], [0, 1, 2]]) });
    checks(l.plan, l.run);
    const s = fun('sylvester', A, { B: gridOf([[1, 2], [0, 3]]), C: gridOf([[1, 2], [3, 4], [5, 6]]) });
    checks(s.plan, s.run);
  });

  it('wrong specs are refused in plain words: non-square, mismatched Sylvester C, non-integer power', () => {
    expect(validateSpec(withSpec({ task: 'matfun', matfun: { kind: 'expm', power: 3 }, matrices: { A: gridOf([[1, 2, 3], [4, 5, 6]]) } }))).toMatch(/square/);
    expect(validateSpec(withSpec({ task: 'matfun', matfun: { kind: 'sylvester', power: 3 }, matrices: { A: gridOf([[1, 0], [0, 2]]), B: gridOf([[1]]), C: gridOf([[1, 2], [3, 4]]) } }))).toMatch(/C must be 2 x 1/);
    expect(validateSpec(withSpec({ task: 'matfun', matfun: { kind: 'mpower', power: 1.5 }, matrices: { A: gridOf([[1, 0], [0, 2]]) } }))).toMatch(/whole number/);
  });

  it('an equation with no unique solution fails with a clear message, not garbage', () => {
    // A and -B share the eigenvalue 1, so A X + X B = C has no unique solution
    const spec = withSpec({ task: 'matfun', matfun: { kind: 'sylvester', power: 3 }, matrices: { A: gridOf([[1, 0], [0, 2]]), B: gridOf([[-1, 0], [0, 5]]), C: gridOf([[1, 1], [1, 1]]) } });
    const plan = buildLinalgScript(spec);
    const run = runQu(plan.script);
    expect(run.ok).toBe(false);
    const err = describeEngineError(run.stderr, plan.script);
    expect(err.message).toMatch(/no unique solution/i);
    expect(err.source).toMatch(/sylvester/);
  });
});

// ------------------------------------------------------ eigenproblems and flags
gate('engine gate: eigenproblems', () => {
  it('Eigen task: spectrum of A = H diag(d) H, spectral radius, orthogonal eigenvectors', () => {
    const { plan, run } = panelRun(withSpec({ task: 'eig', matrices: { A: gridOf(SYM) } }));
    expect(run.ok, run.stderr).toBe(true);
    checks(plan, run);
    expect(num(run, 'spectral_radius')).toBeCloseTo(5, 10);
    const ev = vectorVar(run.vars, 'ev_re').sort((a, b) => a - b);
    [...D].sort((a, b) => a - b).forEach((v, i) => expect(ev[i]).toBeCloseTo(v, 10));
  });

  it('a non-square matrix is refused before the engine; the engine also says so plainly', () => {
    const spec = withSpec({ task: 'eig', matrices: { A: gridOf([[1, 2, 3], [4, 5, 6]]) } });
    expect(validateSpec(spec)).toMatch(/square/);
    const plan = buildLinalgScript(spec);
    const run = runQu(plan.script);
    expect(run.ok).toBe(false);
    expect(describeEngineError(run.stderr, plan.script).message).toMatch(/square/);
  });
});

gate('engine gate: structure detection through the engine, and the advisor on its output', () => {
  const flagsOf = (A: MatrixSource): MatrixFlags => {
    const r = runQu(buildSummaryScript('A', A));
    expect(r.ok, r.stderr).toBe(true);
    return flagsFromVars(r.vars)!;
  };

  it('SPD', () => {
    const f = flagsOf(poisson(6));
    expect(f).toMatchObject({ rows: 6, cols: 6, symmetric: true, spd: 1, lower: false, upper: false, lowerBand: 1, upperBand: 1, rank: 6 });
    expect(adviseSolve(f).method).toBe('cholesky');
  });
  it('symmetric but indefinite', () => {
    const f = flagsOf(gridOf(SYM));
    expect(f.symmetric).toBe(true);
    expect(f.spd).toBe(0);
    expect(adviseSolve(f).method).toBe('lu');
    expect(Math.abs(f.det - D.reduce((p, v) => p * v, 1))).toBeLessThan(1e-9); // det = product of the constructed spectrum
  });
  it('general square', () => {
    const f = flagsOf(gridOf(GEN));
    expect(f.symmetric).toBe(false);
    expect(adviseSolve(f).method).toBe('lu');
  });
  it('lower and upper triangular, diagonal', () => {
    const lo = flagsOf(gridOf([[2, 0, 0], [1, 3, 0], [4, -1, 5]]));
    expect(lo).toMatchObject({ lower: true, upper: false, lowerBand: 2, upperBand: 0 });
    expect(adviseSolve(lo).method).toBe('triangular');
    const up = flagsOf(gridOf([[2, 1, 4], [0, 3, -1], [0, 0, 5]]));
    expect(up).toMatchObject({ lower: false, upper: true });
    expect(adviseSolve(up).method).toBe('triangular');
    expect(flagsOf(gridOf([[2, 0], [0, 3]]))).toMatchObject({ lower: true, upper: true });
  });
  it('rank-deficient, over- and under-determined', () => {
    const rd = flagsOf(gridOf([[1, 2, 3], [2, 4, 6], [1, 1, 1]]));
    expect(rd.rank).toBe(2);
    expect(adviseSolve(rd).method).toBe('lstsq');
    const tall = flagsOf({ kind: 'builder', builder: { kind: 'vandermonde', points: 10, cols: 3 } });
    expect(tall).toMatchObject({ rows: 10, cols: 3, rank: 3 });
    expect(adviseSolve(tall).method).toBe('qr');
    expect(adviseSolve(flagsOf(gridOf([[1, 2, 3, 4], [0, 1, 0, 1]]))).method).toBe('lstsq');
  });
  it('bandwidths and sparsity are measured (8 nonzeros, band 2 up and 2 down)', () => {
    const pat: MatrixSource = { kind: 'builder', builder: { kind: 'pattern', rows: 6, cols: 6, entries: '0 0 1\n1 1 1\n2 2 1\n3 3 1\n4 4 1\n5 5 1\n0 2 5\n3 1 7' } };
    const f = flagsOf(pat);
    expect(f.upperBand).toBe(2);
    expect(f.lowerBand).toBe(2);
    expect(f.nnz).toBe(8);
    expect(f.density).toBeCloseTo(8 / 36, 12);
  });
  it('a 700 x 700 matrix (beyond the dense limit) skips the O(n^3) tests but still reports its structure', () => {
    const f = flagsOf(poisson(700));
    expect(f.heavy).toBe(true);
    expect(Number.isNaN(f.rank)).toBe(true);
    expect(f.symmetric).toBe(true);
    expect(f.spd).toBe(2); // likely: dominant positive diagonal
    expect(f.upperBand).toBe(1);
    expect(adviseSolve(f).method).toBe('cg');
  });
  it('builders: seeded random reproduces, SPD builder is SPD, the others match values built here', () => {
    const rnd = (seed: number): MatrixSource => ({ kind: 'builder', builder: { kind: 'random', rows: 4, cols: 4, seed, dist: 'normal' } });
    const s1 = runQu(buildSummaryScript('A', rnd(11)));
    const s2 = runQu(buildSummaryScript('A', rnd(11)));
    const s3 = runQu(buildSummaryScript('A', rnd(12)));
    expect(matVar(s1.vars, 'M')!.re).toEqual(matVar(s2.vars, 'M')!.re);
    expect(matVar(s3.vars, 'M')!.re).not.toEqual(matVar(s1.vars, 'M')!.re);
    expect(flagsOf({ kind: 'builder', builder: { kind: 'random', rows: 5, cols: 5, seed: 3, dist: 'spd' } })).toMatchObject({ symmetric: true, spd: 1 });
    const toe = runQu(buildSummaryScript('A', { kind: 'builder', builder: { kind: 'toeplitz', col: '1, 2, 3', row: '1, 5, 6' } }));
    expect(matVar(toe.vars, 'M')!.re).toEqual([[1, 5, 6], [2, 1, 5], [3, 2, 1]]);
    for (const b of [{ kind: 'hilbert', n: 5 }, { kind: 'vandermonde', points: 5, cols: 4 }, { kind: 'poisson1d', n: 5 }, { kind: 'diagonal', values: '1, 2, 3' }, { kind: 'identity', n: 3 }] as const) {
      const m = matVar(runQu(buildSummaryScript('A', { kind: 'builder', builder: b })).vars, 'M')!.re;
      const truth = sourceValues({ kind: 'builder', builder: b })!;
      expect(maxDiff(m, truth), b.kind).toBeLessThan(1e-12);
    }
  });
  it('pasted/file data above the inline limit is read from a staged f64 file', () => {
    const n = 25; // 625 entries > 400
    const A: M = Array.from({ length: n }, (_, i) => Array.from({ length: n }, (_, j) => (i === j ? 3 : Math.abs(i - j) === 1 ? -1 : 0)));
    const src = { kind: 'data' as const, rows: n, cols: n, values: A.flat(), origin: 'paste', file: join(dir, 'staged.f64') };
    writeFileSync(src.file, Buffer.from(stagedBytes(src).buffer));
    expect(flagsOf(src)).toMatchObject({ rows: n, cols: n, symmetric: true, spd: 1, upperBand: 1, lowerBand: 1 });
    expect(matVar(runQu(buildSummaryScript('A', src)).vars, 'M')!.re).toEqual(A);
  });
});

gate('engine gate: wrong specs are caught', () => {
  it('non-conformable b: refused by validation, and if sent anyway the engine message is plain and points at the line', () => {
    const spec = withSpec({ task: 'solve', matrices: { A: gridOf([[1, 2], [3, 4]]), b: columnGrid([1, 2, 3]) } });
    expect(validateSpec(spec)).toMatch(/b needs 2 rows, but b has 3/);
    const plan = buildLinalgScript(spec);
    const run = runQu(plan.script);
    expect(run.ok).toBe(false);
    const err = describeEngineError(run.stderr, plan.script);
    expect(err.message).toMatch(/sizes do not match/i);
    expect(err.line).toBeGreaterThan(0);
    expect(err.source).toBeTruthy();
  });

  it('the checks can fail: a wrong x gives a residual the check card rejects', () => {
    const { plan } = panelRun(withSpec({ task: 'solve', matrices: { A: gridOf([[4, 1], [1, 3]]), b: columnGrid([1, 2]) } }));
    const wrong = plan.script.replace(/^x = solve\(L', y\).*$/m, 'x = [9, 9] as matrix(2, 1)');
    expect(wrong).not.toEqual(plan.script);
    const run = runQu(wrong);
    expect(run.ok, run.stderr).toBe(true);
    expect(checkPasses(num(run, 'relres'), plan.cards.find((c) => c.id === 'relres')!.tol!)).toBe(false);
  });

  it('a singular system forced through LU is flagged by the conditioning warning, not silently trusted', () => {
    const spec = withSpec({ task: 'solve', solve: { ...DEFAULT_SPEC.solve, method: 'lu' }, matrices: { A: gridOf([[1, 2], [2, 4]]), b: columnGrid([1, 2]) } });
    const { run } = panelRun(spec);
    expect(run.ok, run.stderr).toBe(true);
    expect(['severe', 'singular']).toContain(conditionWarning(scalarOrInf(run.vars, 'condA')).level);
  });

  it('forcing Cholesky on a non-SPD matrix is refused by validation using the measured flags', () => {
    const spec = withSpec({ task: 'solve', solve: { ...DEFAULT_SPEC.solve, method: 'cholesky' }, matrices: { A: gridOf(GEN), b: columnGrid([1, 2, 3, 4]) } });
    const flags = flagsFromVars(runQu(buildSummaryScript('A', spec.matrices.A)).vars)!;
    expect(validateSpec(spec, { A: flags })).toMatch(/Cholesky needs a symmetric/);
  });
});

gate('engine gate: every shipped example runs exactly as the panel would run it', () => {
  for (const ex of EXAMPLES) {
    it(ex.id, () => {
      const s = { ...DEFAULT_SPEC, ...ex.spec };
      expect(validateSpec(s)).toBeNull();
      const { plan, run } = panelRun(s);
      expect(run.ok, `${ex.id}: ${run.stderr}`).toBe(true);
      checks(plan, run);
      for (const c of plan.cards) {
        if (['Ainv', 'V', 'err_av', 'err_vo'].includes(c.id)) continue; // legitimately absent for singular / non-symmetric input
        expect(run.vars[c.vars[0]], `${ex.id}: card ${c.id} reads ${c.vars[0]}`).toBeDefined();
      }
    });
  }
});
