import { describe, expect, it } from 'vitest';
import {
  DEFAULT_SPEC,
  EXAMPLES,
  INLINE_LIMIT,
  adviseSolve,
  buildLinalgScript,
  buildSummaryScript,
  checkPasses,
  columnGrid,
  columnStats,
  conditionWarning,
  correctDigits,
  dataSource,
  describeEngineError,
  describeFlags,
  flagsFromVars,
  formatComplex,
  formatNumber,
  gridOf,
  indexVars,
  matFromRows,
  matToCsv,
  matToQuLiteral,
  matVar,
  matricesNeeded,
  matrixDefinition,
  methodProblem,
  parseCell,
  parseList,
  parseMarkers,
  parseMatrixText,
  parsePattern,
  resolveSolveMethod,
  scalarOrInf,
  sourceDims,
  sourceFromTable,
  sourceProblem,
  sourceValues,
  stagedBytes,
  structureClass,
  validateSpec,
  type LinalgSpec,
  type MatrixFlags,
  type MatrixSource,
} from './linalgDesign';

const flags = (over: Partial<MatrixFlags> = {}): MatrixFlags => ({
  rows: 5, cols: 5, symmetric: false, spd: 0, lower: false, upper: false, lowerBand: 4, upperBand: 4,
  nnz: 25, density: 1, rank: 5, cond: 10, det: 1, heavy: false, ...over,
});
const spec = (patch: Partial<LinalgSpec> = {}): LinalgSpec => ({ ...DEFAULT_SPEC, ...patch });
const withA = (A: MatrixSource, patch: Partial<LinalgSpec> = {}) => spec({ ...patch, matrices: { ...DEFAULT_SPEC.matrices, ...(patch.matrices ?? {}), A } });

describe('numbers and cells', () => {
  it('parses numbers and fractions, rejects text', () => {
    expect(parseCell('3')).toBe(3);
    expect(parseCell(' -2.5e-3 ')).toBeCloseTo(-0.0025, 15);
    expect(parseCell('1/4')).toBe(0.25);
    expect(parseCell('1/0')).toBeNull();
    expect(parseCell('abc')).toBeNull();
    expect(parseCell('')).toBeNull();
    expect(parseList('1, 2 3;4')).toEqual([1, 2, 3, 4]);
    expect(parseList('1, x')).toBeNull();
  });

  it('formats with a precision, scientific for tiny and huge values', () => {
    expect(formatNumber(0)).toBe('0');
    expect(formatNumber(1 / 3, 4)).toBe('0.3333');
    expect(formatNumber(2, 6)).toBe('2');
    expect(formatNumber(1234.5678, 6)).toBe('1234.57');
    expect(formatNumber(1.5e-9, 3)).toBe('1.5e-9');
    expect(formatNumber(3.0e10, 3)).toBe('3e10');
    expect(formatNumber(Infinity)).toBe('inf');
    expect(formatNumber(NaN)).toBe('NaN');
    expect(formatNumber(-2.50001e7, 3)).toBe('-2.5e7');
  });

  it('prints complex numbers the way the engine does: a + bj', () => {
    expect(formatComplex(1, 2)).toBe('1 + 2j');
    expect(formatComplex(0.5, -2)).toBe('0.5 - 2j');
    expect(formatComplex(3, 0)).toBe('3');
    expect(formatComplex(1.22e-16, 2, 3)).toBe('1.22e-16 + 2j');
  });

  it('writes matrices as Qu literals (vectors stay genuine matrices) and CSV', () => {
    expect(matToQuLiteral('A', matFromRows([[1, 2], [3, 4]]))).toBe('A = [1, 2; 3, 4]');
    expect(matToQuLiteral('b', matFromRows([[1], [2], [3]]))).toBe('b = [1, 2, 3] as matrix(3, 1)');
    expect(matToQuLiteral('r', matFromRows([[1, 2, 3]]))).toBe('r = [1, 2, 3] as matrix(1, 3)');
    expect(matToQuLiteral('Z', matFromRows([[1, 2], [3, 4]], [[0, -1], [0, 0]]))).toBe('Z = [1, 2 - 1j; 3, 4]');
    expect(matToCsv(matFromRows([[1, 2.5], [3, 4]]))).toBe('1,2.5\n3,4');
    expect(matToCsv(matFromRows([[1, 2]], [[0, 3]]))).toBe('1,2+3j');
  });
});

describe('pasted tables', () => {
  it('reads tab, comma, semicolon and space separated blocks, and skips a header', () => {
    expect(parseMatrixText('1\t2\n3\t4').values).toEqual([[1, 2], [3, 4]]);
    expect(parseMatrixText('1,2,3\n4,5,6').cols).toBe(3);
    expect(parseMatrixText('1;2\n3;4').values).toEqual([[1, 2], [3, 4]]);
    expect(parseMatrixText('1 2\n3 4').values).toEqual([[1, 2], [3, 4]]);
    const h = parseMatrixText('x,y\n1,2\n3,4');
    expect(h.header).toEqual(['x', 'y']);
    expect(h.values).toEqual([[1, 2], [3, 4]]);
    expect(parseMatrixText('# note\n1 2').values).toEqual([[1, 2]]);
  });

  it('reports problems with line and cell, not silently', () => {
    const r = parseMatrixText('1,2,3\n4,x,6\n7,8');
    expect(r.problems.join('|')).toMatch(/Line 2, cell 2: "x" is not a number/);
    expect(r.problems.join('|')).toMatch(/Line 3 has 2 cells/);
    expect(parseMatrixText('   ').problems[0]).toMatch(/empty/);
  });

  it('small tables become an editable grid, big ones a data source with column stats', () => {
    expect(sourceFromTable(parseMatrixText('1 2\n3 4'), 'paste').kind).toBe('grid');
    const big = parseMatrixText(Array.from({ length: 20 }, (_, i) => `${i},${2 * i},1`).join('\n'));
    const src = sourceFromTable(big, 'paste');
    expect(src.kind).toBe('data');
    if (src.kind === 'data') {
      const st = columnStats(src);
      expect(st[0]).toMatchObject({ min: 0, max: 19, mean: 9.5 });
      expect(st[2]).toMatchObject({ min: 1, max: 1 });
    }
  });

  it('stages data column-major for read_array', () => {
    const src = dataSource({ rows: 2, cols: 3, values: [[1, 2, 3], [4, 5, 6]] }, 'x');
    if (src.kind !== 'data') throw new Error();
    expect(Array.from(stagedBytes(src))).toEqual([1, 4, 2, 5, 3, 6]);
  });
});

describe('matrix definitions', () => {
  it('writes every builder as plain Qu', () => {
    const def = (b: MatrixSource) => matrixDefinition('A', b);
    expect(def({ kind: 'builder', builder: { kind: 'identity', n: 4 } })).toBe('A = eye(4)');
    expect(def({ kind: 'builder', builder: { kind: 'zeros', rows: 2, cols: 3 } })).toBe('A = zeros(2, 3)');
    expect(def({ kind: 'builder', builder: { kind: 'ones', rows: 2, cols: 3 } })).toBe('A = ones(2, 3)');
    expect(def({ kind: 'builder', builder: { kind: 'diagonal', values: '1, 2, 3' } })).toBe('A = diag([1, 2, 3])');
    expect(def({ kind: 'builder', builder: { kind: 'random', rows: 3, cols: 4, seed: 7, dist: 'normal' } })).toBe('A = randn(3, 4, seed=7)');
    expect(def({ kind: 'builder', builder: { kind: 'random', rows: 3, cols: 4, seed: 7, dist: 'uniform' } })).toBe('A = rand(3, 4, seed=7)');
    expect(def({ kind: 'builder', builder: { kind: 'random', rows: 3, cols: 3, seed: 7, dist: 'spd' } })).toMatch(/randn\(3, 3, seed=7\)[\s\S]*eye\(3\)/);
    expect(def({ kind: 'builder', builder: { kind: 'hilbert', n: 6 } })).toMatch(/outer\(la_i, ones\(6\)\)/);
    expect(def({ kind: 'builder', builder: { kind: 'vandermonde', points: 9, cols: 3 } })).toBe('A = vander(linspace(0, 1, 9), 3)');
    expect(def({ kind: 'builder', builder: { kind: 'toeplitz', col: '1, 2, 3', row: '' } })).toBe('A = toeplitz([1, 2, 3])');
    expect(def({ kind: 'builder', builder: { kind: 'toeplitz', col: '1, 2', row: '1, 5, 6' } })).toBe('A = toeplitz([1, 2], [1, 5, 6])');
    expect(def({ kind: 'builder', builder: { kind: 'poisson1d', n: 5 } })).toMatch(/toeplitz\(la_c\)/);
    expect(def({ kind: 'builder', builder: { kind: 'pattern', rows: 3, cols: 3, entries: '0 1 5\n2,2,-1' } })).toBe('A = zeros(3, 3)\nA[0, 1] = 5\nA[2, 2] = -1');
  });

  it('inlines small data and reads large data from the staged file', () => {
    const small = dataSource({ rows: 2, cols: 2, values: [[1, 2], [3, 4]] }, 'x');
    expect(matrixDefinition('A', small)).toBe('A = [1, 2; 3, 4]');
    const n = Math.ceil(Math.sqrt(INLINE_LIMIT)) + 1;
    const big = { ...(dataSource({ rows: n, cols: n, values: Array.from({ length: n }, () => new Array(n).fill(1)) }, 'x') as Extract<MatrixSource, { kind: 'data' }>), file: 'C:\\tmp\\a.f64' };
    expect(matrixDefinition('A', big)).toBe(`A = reshape(read_array("C:/tmp/a.f64"), ${n}, ${n})`);
    expect(sourceProblem('A', { ...big, file: undefined })).toMatch(/staged/);
  });

  it('knows the size and, for deterministic builders, the numbers, without the engine', () => {
    expect(sourceDims({ kind: 'builder', builder: { kind: 'hilbert', n: 4 } })).toEqual({ rows: 4, cols: 4 });
    expect(sourceDims({ kind: 'builder', builder: { kind: 'random', rows: 3, cols: 5, seed: 1, dist: 'spd' } })).toEqual({ rows: 3, cols: 3 });
    expect(sourceDims({ kind: 'builder', builder: { kind: 'toeplitz', col: '1,2,3', row: '1,2' } })).toEqual({ rows: 3, cols: 2 });
    expect(sourceValues({ kind: 'builder', builder: { kind: 'hilbert', n: 3 } })![2][2]).toBeCloseTo(1 / 5, 15);
    expect(sourceValues({ kind: 'builder', builder: { kind: 'poisson1d', n: 3 } })).toEqual([[2, -1, 0], [-1, 2, -1], [0, -1, 2]]);
    expect(sourceValues({ kind: 'builder', builder: { kind: 'vandermonde', points: 3, cols: 3 } })).toEqual([[0, 0, 1], [0.25, 0.5, 1], [1, 1, 1]]);
    expect(sourceValues({ kind: 'builder', builder: { kind: 'random', rows: 2, cols: 2, seed: 1, dist: 'normal' } })).toBeNull();
  });

  it('refuses sources it cannot turn into Qu, naming the cell', () => {
    expect(sourceProblem('A', { kind: 'grid', cells: [['1', 'x'], ['3', '4']] })).toMatch(/A: row 1, column 2 \("x"\) is not a number/);
    expect(sourceProblem('A', { kind: 'builder', builder: { kind: 'identity', n: 0 } })).toMatch(/whole numbers/);
    expect(sourceProblem('A', { kind: 'builder', builder: { kind: 'poisson1d', n: 1 } })).toMatch(/n of 2 or more/);
    expect(sourceProblem('A', { kind: 'builder', builder: { kind: 'diagonal', values: 'a, b' } })).toMatch(/diagonal/);
    expect(sourceProblem('A', { kind: 'builder', builder: { kind: 'pattern', rows: 2, cols: 2, entries: '5 0 1' } })).toMatch(/outside the 2 x 2/);
    expect(sourceProblem('A', { kind: 'builder', builder: { kind: 'pattern', rows: 2, cols: 2, entries: 'zero' } })).toMatch(/row col value/);
    expect(parsePattern('# c\n0 0 2\n1 1').entries).toEqual([{ i: 0, j: 0, v: 2 }, { i: 1, j: 1, v: 1 }]);
  });
});

describe('the advisor picks the documented method for each structure class', () => {
  const cases: Array<[string, Partial<MatrixFlags>, string, string]> = [
    ['symmetric positive definite', { symmetric: true, spd: 1 }, 'cholesky', 'symmetric positive definite'],
    ['likely SPD (too large to factorise)', { symmetric: true, spd: 2 }, 'cholesky', 'symmetric positive definite'],
    ['general square', {}, 'lu', 'general square'],
    ['symmetric but indefinite', { symmetric: true, spd: 0 }, 'lu', 'general square'],
    ['lower triangular', { lower: true }, 'triangular', 'triangular'],
    ['upper triangular', { upper: true }, 'triangular', 'triangular'],
    ['over-determined', { rows: 20, cols: 3, rank: 3 }, 'qr', 'over-determined'],
    ['nearly rank-deficient over-determined', { rows: 20, cols: 3, rank: 3, cond: 1e14 }, 'svd', 'over-determined'],
    ['under-determined', { rows: 2, cols: 5, rank: 2 }, 'lstsq', 'under-determined'],
    ['rank-deficient square', { rank: 3 }, 'lstsq', 'rank-deficient'],
    ['large sparse SPD', { rows: 500, cols: 500, symmetric: true, spd: 1, density: 0.006, rank: 500, lowerBand: 1, upperBand: 1 }, 'cg', 'large sparse SPD'],
    ['large banded', { rows: 500, cols: 500, density: 0.006, rank: 500, lowerBand: 1, upperBand: 1 }, 'sparselu', 'large banded'],
    ['large sparse general', { rows: 500, cols: 500, density: 0.01, rank: 500, lowerBand: 400, upperBand: 400 }, 'gmres', 'large sparse'],
    ['skipped rank (very large) falls through to the structure', { rows: 900, cols: 900, density: 0.005, rank: NaN, symmetric: true, spd: 2, lowerBand: 1, upperBand: 1 }, 'cg', 'large sparse SPD'],
  ];
  for (const [name, over, method, cls] of cases) {
    it(name, () => {
      const f = flags(over);
      const a = adviseSolve(f);
      expect(a.method).toBe(method);
      expect(structureClass(f)).toBe(cls);
      expect(a.reason.length).toBeGreaterThan(20); // a one-line reason is always given
      expect(a.reason).not.toMatch(/\n/);
    });
  }

  it('a large sparse system with several right-hand sides falls back to the direct methods', () => {
    expect(adviseSolve(flags({ rows: 500, cols: 500, symmetric: true, spd: 1, density: 0.006, rank: 500, lowerBand: 1, upperBand: 1 }), 3).method).toBe('cholesky');
    expect(adviseSolve(flags({ rows: 500, cols: 500, density: 0.006, rank: 500, lowerBand: 1, upperBand: 1 }), 3).method).toBe('lu');
  });

  it('refuses methods that cannot work, and says why', () => {
    expect(methodProblem('cholesky', flags({ symmetric: false }), 1)).toMatch(/symmetric/);
    expect(methodProblem('cholesky', flags({ symmetric: true, spd: 0 }), 1)).toMatch(/not positive definite/);
    expect(methodProblem('cholesky', flags({ symmetric: true, spd: 1 }), 1)).toBeNull();
    expect(methodProblem('lu', flags({ rows: 4, cols: 3 }), 1)).toMatch(/square/);
    expect(methodProblem('qr', flags({ rows: 2, cols: 4 }), 1)).toMatch(/at least as many rows/);
    expect(methodProblem('qr', flags({ rank: 3 }), 1)).toMatch(/rank-deficient/);
    expect(methodProblem('cg', flags({ symmetric: true, spd: 0 }), 1)).toMatch(/positive definite/);
    expect(methodProblem('cg', flags({ symmetric: true, spd: 1 }), 2)).toMatch(/single column/);
    expect(methodProblem('svd', flags({ rows: 2, cols: 4 }), 1)).toBeNull();
    expect(methodProblem('lstsq', flags({ rows: 2, cols: 4 }), 1)).toBeNull();
  });

  it('the resolved method falls back sensibly before the structure is measured', () => {
    const r = resolveSolveMethod(withA(gridOf([[1, 2], [3, 4], [5, 6]]), { task: 'solve', matrices: { ...DEFAULT_SPEC.matrices, b: columnGrid([1, 2, 3]) } as never }));
    expect(r.auto).toBe(true);
    expect(r.reason).toMatch(/not measured yet/);
    const forced = resolveSolveMethod({ ...DEFAULT_SPEC, solve: { ...DEFAULT_SPEC.solve, method: 'svd' } });
    expect(forced).toMatchObject({ method: 'svd', auto: false });
  });
});

describe('conditioning warnings in plain words', () => {
  it('is silent for well-conditioned matrices and warns from 1e8', () => {
    expect(conditionWarning(1e3).level).toBe('ok');
    expect(conditionWarning(9e7).level).toBe('ok');
    const w = conditionWarning(1e9);
    expect(w.level).toBe('warn');
    expect(w.text).toMatch(/ill-conditioned.*loses about 9 of the 16 digits/);
    expect(conditionWarning(1e13).level).toBe('severe');
    expect(conditionWarning(1e13).text).toMatch(/only about 3 can be right/);
    expect(conditionWarning(1e17).level).toBe('singular');
    expect(conditionWarning(Infinity).level).toBe('singular');
    expect(conditionWarning(NaN).level).toBe('ok');
  });

  it('digits-correct is -log10 of the error, capped', () => {
    expect(correctDigits(1e-6)).toBeCloseTo(6, 12);
    expect(correctDigits(0)).toBe(16);
    expect(correctDigits(5)).toBe(0);
    expect(correctDigits(NaN)).toBe(0);
    expect(checkPasses(1e-14, 1e-12)).toBe(true);
    expect(checkPasses(1e-3, 1e-12)).toBe(false);
    expect(checkPasses(NaN, 1)).toBe(false);
  });
});

describe('validateSpec', () => {
  it('accepts every shipped example', () => {
    for (const ex of EXAMPLES) {
      const s = { ...ex.spec, precision: 6 };
      expect(validateSpec(s), ex.id).toBeNull();
      const plan = buildLinalgScript(s);
      expect(plan.script.length, ex.id).toBeGreaterThan(30);
      expect(plan.cards.length, ex.id).toBeGreaterThan(0);
    }
  });

  it('catches non-conformable sizes in words', () => {
    expect(validateSpec(withA(gridOf([[1, 2], [3, 4]]), { matrices: { ...DEFAULT_SPEC.matrices, b: columnGrid([1, 2, 3]) } as never }))).toMatch(/b needs 2 rows, but b has 3/);
    expect(validateSpec(spec({ task: 'eig', matrices: { ...DEFAULT_SPEC.matrices, A: gridOf([[1, 2, 3], [4, 5, 6]]) } }))).toMatch(/square/);
    expect(validateSpec(spec({ task: 'decomp', decomp: { kind: 'lu' }, matrices: { ...DEFAULT_SPEC.matrices, A: gridOf([[1, 2, 3], [4, 5, 6]]) } }))).toMatch(/LU .* square/);
  });

  it('only checks the matrices the task reads', () => {
    expect(matricesNeeded(spec({ task: 'solve' }))).toEqual(['A', 'b']);
    expect(matricesNeeded(spec({ task: 'eig' }))).toEqual(['A']);
    expect(matricesNeeded(spec({ task: 'matfun', matfun: { kind: 'sylvester', power: 2 } }))).toEqual(['A', 'B', 'C']);
    expect(matricesNeeded(spec({ task: 'lsq', lsq: { weighted: true, truth: 'ones' } }))).toEqual(['A', 'C']);
    // a broken B does not matter for a task that does not use it
    const s = spec({ task: 'props', matrices: { ...DEFAULT_SPEC.matrices, B: { kind: 'grid', cells: [['x']] } } });
    expect(validateSpec(s)).toBeNull();
  });

  it('validates regularisation parameters and measured flags', () => {
    expect(validateSpec(spec({ solve: { method: 'tikhonov', lambda: 0, tsvdK: 2 } }))).toMatch(/lambda/);
    expect(validateSpec(spec({ solve: { method: 'tsvd', lambda: 1, tsvdK: 0 } }))).toMatch(/whole number/);
    expect(validateSpec(spec({ solve: { method: 'cholesky', lambda: 1, tsvdK: 2 } }), { A: flags({ rows: 3, cols: 3, symmetric: false }) })).toMatch(/symmetric/);
    expect(validateSpec(spec({ task: 'decomp', decomp: { kind: 'chol' } }), { A: flags({ rows: 3, cols: 3, symmetric: true, spd: 0 }) })).toMatch(/not positive definite/);
  });
});

describe('generated scripts', () => {
  const A = gridOf([[4, 1, 0], [1, 3, 1], [0, 1, 2]]);

  it('says which method it chose and why, in a comment, and ends with the checks', () => {
    const plan = buildLinalgScript(withA(A), flags({ rows: 3, cols: 3, symmetric: true, spd: 1, rank: 3, density: 0.78 }));
    expect(plan.method).toBe('cholesky');
    expect(plan.script).toMatch(/# method: Cholesky .* chosen by the advisor: A is symmetric positive definite/);
    expect(plan.script).toContain('L = chol(A)');
    expect(plan.script).toMatch(/relres = norm\(A \* x - b\) \/ max\(norm\(b\), 1e-300\)/);
    expect(plan.script).toContain('condA = cond(A)');
    expect(plan.headline).toMatch(/^Cholesky/);
  });

  it('honours an override and names it as yours', () => {
    const plan = buildLinalgScript(withA(A, { solve: { method: 'qr', lambda: 1, tsvdK: 1 } }));
    expect(plan.script).toMatch(/chosen by you/);
    expect(plan.script).toContain('f = qr(A)');
  });

  it('sparse methods import sparse and use only qualified names', () => {
    const plan = buildLinalgScript(withA(A, { solve: { method: 'cg', lambda: 1, tsvdK: 1 } }));
    expect(plan.script).toContain('import sparse');
    expect(plan.script).toContain('sparse.cg(S, bv, precond="jacobi")');
    // a bare solve/size/diag/eye after `import sparse` would be an ambiguity error
    const body = plan.script.split('\n').filter((l) => !l.trim().startsWith('#')).join('\n');
    expect(body).not.toMatch(/(^|[^.\w])(solve|size|diag|eye|add|get|scale|transpose|random)\(/);
  });

  it('every task and decomposition builds a script with the right Qu calls', () => {
    const calls: Array<[LinalgSpec, RegExp]> = [
      [spec({ task: 'decomp', decomp: { kind: 'lu' } }), /lu\(A\)/],
      [spec({ task: 'decomp', decomp: { kind: 'qr' } }), /qr\(A\)/],
      [spec({ task: 'decomp', decomp: { kind: 'chol' } }), /chol\(A\)/],
      [spec({ task: 'decomp', decomp: { kind: 'svd' } }), /svd\(A\)/],
      [spec({ task: 'decomp', decomp: { kind: 'eig' } }), /eig\(A\)/],
      [spec({ task: 'decomp', decomp: { kind: 'schur' } }), /schur\(A\)/],
      [spec({ task: 'props' }), /null\(A\)[\s\S]*orth\(A\)[\s\S]*pinv\(A\)[\s\S]*inv\(A\)/],
      [spec({ task: 'lsq' }), /qr\(Aw\)[\s\S]*svd\(Aw\)/],
      [spec({ task: 'eig' }), /eig\(A\)/],
      [spec({ task: 'matfun', matfun: { kind: 'expm', power: 2 } }), /expm\(A\)/],
      [spec({ task: 'matfun', matfun: { kind: 'sqrtm', power: 2 } }), /sqrtm\(A\)/],
      [spec({ task: 'matfun', matfun: { kind: 'logm', power: 2 } }), /logm\(A\)/],
      [spec({ task: 'matfun', matfun: { kind: 'mpower', power: 4 } }), /A \^ p/],
      [spec({ task: 'matfun', matfun: { kind: 'lyap', power: 2 } }), /lyap\(A, C\)/],
      [spec({ task: 'matfun', matfun: { kind: 'sylvester', power: 2 } }), /sylvester\(A, B, C\)/],
    ];
    for (const [s, re] of calls) {
      const plan = buildLinalgScript(s);
      expect(plan.script, `${s.task}/${s.decomp.kind}/${s.matfun.kind}`).toMatch(re);
      // every result card names the Qu call that produced it
      for (const c of plan.cards) expect(c.call.length, c.id).toBeGreaterThan(0);
    }
  });

  it('never names a helper like a builtin it would shadow', () => {
    for (const t of ['solve', 'decomp', 'props', 'lsq', 'matfun', 'eig'] as const) {
      const plan = buildLinalgScript(spec({ task: t }), flags({ rows: 3, cols: 3, symmetric: true, spd: 1, rank: 3 }));
      expect(plan.script).not.toMatch(/function (solve|fit|step|filter|svd|qr|lu|chol|eig)\b/);
    }
  });

  it('the summary script measures structure with Qu code and skips the cubic tests when large', () => {
    const small = buildSummaryScript('A', A);
    expect(small).toMatch(/chol\(M\)/);
    expect(small).toMatch(/m_rank = rank\(M\)/);
    const big = buildSummaryScript('A', { kind: 'builder', builder: { kind: 'poisson1d', n: 1000 } });
    expect(big).not.toMatch(/chol\(M\)/);
    expect(big).toMatch(/m_heavy = 1/);
  });
});

describe('reading a run', () => {
  const vars = indexVars([
    { name: 'M', type: 'matrix', shape: [2, 3], data: [1, 4, 2, 5, 3, 6] },
    { name: 'e', type: 'vector', shape: [2, 1], data: [0.5, null] },
    { name: 'c', type: 'number', shape: [1, 1], data: [null] },
  ]);
  it('turns column-major data into rows, and null into NaN or inf as asked', () => {
    expect(matVar(vars, 'M')!.re).toEqual([[1, 2, 3], [4, 5, 6]]);
    expect(matVar(vars, 'e')!.re).toEqual([[0.5], [NaN]]);
    expect(scalarOrInf(vars, 'c')).toBe(Infinity);
    expect(matVar(vars, 'missing')).toBeNull();
  });
  it('drops an all-zero imaginary part and keeps a real one', () => {
    const v = indexVars([
      { name: 'r', type: 'matrix', shape: [1, 2], data: [1, 2] },
      { name: 'zi', type: 'matrix', shape: [1, 2], data: [0, 0] },
      { name: 'ni', type: 'matrix', shape: [1, 2], data: [0, 3] },
    ]);
    expect(matVar(v, 'r', 'zi')!.im).toBeUndefined();
    expect(matVar(v, 'r', 'ni')!.im).toEqual([[0, 3]]);
  });
  it('builds flags from the summary variables and describes them', () => {
    const f = flagsFromVars(indexVars([
      ...['m_rows', 'm_cols', 'm_nnz', 'm_density', 'm_lower', 'm_upper', 'm_uband', 'm_lband', 'm_sym', 'm_spd', 'm_rank', 'm_cond', 'm_det', 'm_heavy'].map((name, i) => ({
        name, type: 'number', shape: [1, 1], data: [[3, 3, 7, 7 / 9, 0, 0, 1, 1, 1, 1, 3, 12.5, 4.5, 0][i]],
      })),
    ]))!;
    expect(f).toMatchObject({ rows: 3, cols: 3, symmetric: true, spd: 1, rank: 3, cond: 12.5, upperBand: 1, heavy: false });
    const text = describeFlags(f).map(([k, v]) => `${k}: ${v}`).join('\n');
    expect(text).toMatch(/symmetric: yes/);
    expect(text).toMatch(/positive definite: yes/);
    expect(text).toContain('banded: yes (lower 1, upper 1)');
    expect(text).toMatch(/rank: 3 of 3/);
    expect(flagsFromVars({})).toBeNull();
  });
  it('reads @@ markers from stdout', () => {
    expect(parseMarkers('hello\n@@inv_error matrix is singular\n@@x 1')).toEqual({ inv_error: 'matrix is singular', x: '1' });
  });
});

describe('engine errors in plain words', () => {
  const script = 'a = 1\nb = 2\nL = chol(A)\n';
  it('translates the common engine messages and points at the line', () => {
    const e = describeEngineError('qu: runtime error: Cholesky decomposition failed\n  at C:/tmp/script.qu:3', script);
    expect(e.message).toMatch(/not symmetric positive definite/);
    expect(e.line).toBe(3);
    expect(e.source).toBe('L = chol(A)');
    expect(describeEngineError('qu: runtime error: matrix is singular (rank 1 of 2); use pinv(M)\n  at x.qu:1', script).message).toMatch(/singular, so it has no inverse/);
    expect(describeEngineError('qu: runtime error: matrix multiply (2x2) * (3x1) needs the inner dimensions to agree\n  at x.qu:2', script).message).toMatch(/sizes do not match/);
  });
  it('keeps an unknown message as the engine said it', () => {
    const e = describeEngineError('qu: runtime error: something new went wrong\n  at x.qu:2', script);
    expect(e.message).toBe('something new went wrong');
    expect(e.raw).toMatch(/something new/);
  });
});
