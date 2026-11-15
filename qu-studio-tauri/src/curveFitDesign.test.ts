import { describe, expect, it } from 'vitest';
import {
  MODELS, SUMMARY_NAMES, buildCurveFit, buildCurveFitScript, defaultSpec, emptyRef, errorContext, needsDataFile, plainError, readRanking, readStats, resolveModel, resolveOde,
  scriptErrors, signalVar, validateSpec, STAT_FIELDS, type FitSpec,
} from './curveFitDesign';
import { EXAMPLES, columnStats, csvForScript, dataLines, parseTable, vectorLiteral, type Table } from './curveFitData';
import { parseExpr } from './mathExpr';
import { buildSolve, defaultSolveSpec, readSolve } from './solveDesign';

const table = (cols: Record<string, number[]>): Table => ({
  columns: Object.entries(cols).map(([name, values]) => ({ name, values })), rows: Object.values(cols)[0].length, header: true, delimiter: 'comma', problems: [],
});
const spec = (patch: Partial<FitSpec> = {}): FitSpec => ({
  ...defaultSpec(), source: 'paste', table: table({ x: [1, 2, 3, 4, 5, 6], y: [2, 4, 5, 8, 10, 12] }), cols: { x: 'x', y: 'y', z: '', err: '', t: 'x', signals: ['y'] }, ...patch,
});
const ref = (id: string, extra: Record<string, unknown> = {}) => ({ ...emptyRef(id), ...extra });

describe('model catalogue', () => {
  it('every model resolves, with one start value per parameter', () => {
    for (const m of MODELS) {
      for (const shape of m.shapes) {
        const deg = m.degree ? m.degree.min + 1 : 2;
        const r = resolveModel(ref(m.id, { degree: deg }), shape, shape === 'multi' ? 't' : 'x');
        expect(r.ok, `${m.id}/${shape}`).toBe(true);
        if (r.ok) {
          expect(r.value.guess.length, m.id).toBe(r.value.params.length);
          expect(new Set(r.value.params).size, m.id).toBe(r.value.params.length);
        }
      }
    }
  });

  it('every model expression parses with exactly its declared parameters', () => {
    for (const m of MODELS) {
      const deg = m.degree ? 3 : 0;
      const p = parseExpr(m.expr(deg), { vars: ['x', 'y'], allowParams: true });
      expect(p.ok, m.id).toBe(true);
      if (p.ok) expect([...p.params].sort(), m.id).toEqual([...m.params(deg)].sort());
    }
  });

  it('start values only use the data-summary names that exist', () => {
    for (const m of MODELS) {
      const surface = m.shapes.includes('surface');
      const known = new Set(surface ? SUMMARY_NAMES.surface : SUMMARY_NAMES.yx);
      const text = [...m.guess(m.degree ? 2 : 0), ...(m.pre?.(m.degree ? 2 : 0) ?? [])].join('\n');
      const defined = new Set([...text.matchAll(/^(d_[A-Za-z0-9]+)\s*=/gm)].map((x) => x[1]));
      for (const u of text.matchAll(/\bd_[A-Za-z0-9]+/g)) expect(known.has(u[0]) || defined.has(u[0]), `${m.id}: ${u[0]}`).toBe(true);
    }
  });

  it('polynomial naming follows polyfit order; surface naming is c<i><j>', () => {
    const r = resolveModel(ref('poly', { degree: 3 }), 'yx', 'x');
    expect(r.ok && r.value.params).toEqual(['c3', 'c2', 'c1', 'c0']);
    expect(r.ok && r.value.body).toBe('polyval([c3, c2, c1, c0], {x})');
    const q = resolveModel(ref('quad', { degree: 2 }), 'surface', 'x');
    expect(q.ok && q.value.params).toEqual(['c00', 'c10', 'c01', 'c20', 'c11', 'c02']);
  });

  it('rejects a bad polynomial degree and unknown models', () => {
    for (const degree of [0, 10, 1.5]) expect(resolveModel(ref('poly', { degree }), 'yx', 'x').ok).toBe(false);
    expect(resolveModel(ref('nope'), 'yx', 'x').ok).toBe(false);
    expect(resolveModel(ref('plane'), 'yx', 'x').ok).toBe(false);
  });
});

describe('custom model', () => {
  it('discovers the parameters and asks for guesses (default 1)', () => {
    const r = resolveModel(ref('custom', { expr: 'a*exp(-b*x)+c' }), 'yx', 'x');
    expect(r.ok && r.value.params).toEqual(['a', 'b', 'c']);
    expect(r.ok && r.value.guess).toEqual(['1', '1', '1']);
  });
  it('explains each kind of mistake before anything runs', () => {
    const e = (expr: string, shape: 'yx' | 'surface' = 'yx') => {
      const r = resolveModel(ref('custom', { expr }), shape, 'x');
      expect(r.ok, expr).toBe(false);
      return r.ok ? '' : r.error;
    };
    expect(e('a*exp(-b*x')).toMatch(/parenthesis/);
    expect(e('x^2')).toMatch(/at least one named parameter/);
    expect(e('a + b')).toMatch(/does not use x/);
    expect(e('a*x*y')).toMatch(/cannot be a parameter/);
  });
  it('a surface custom model may use both x and y', () => {
    const r = resolveModel(ref('custom', { expr: 'a*x^2+b*x*y+c*y^2+d' }), 'surface', 'x');
    expect(r.ok && r.value.params).toEqual(['a', 'b', 'c', 'd']);
  });
});

describe('per-parameter settings', () => {
  it('validates start values, bounds, and refuses everything fixed', () => {
    expect(resolveModel(ref('exp', { ps: { a: { guess: 'd_y0 * 2' } } }), 'yx', 'x').ok).toBe(true);
    expect(resolveModel(ref('exp', { ps: { a: { guess: 'system(1)' } } }), 'yx', 'x').ok).toBe(false);
    expect(resolveModel(ref('exp', { ps: { a: { guess: '1; 2' } } }), 'yx', 'x').ok).toBe(false);
    expect(resolveModel(ref('exp', { ps: { a: { lo: 'abc' } } }), 'yx', 'x').ok).toBe(false);
    const wrong = resolveModel(ref('exp', { ps: { a: { lo: '5', hi: '1' } } }), 'yx', 'x');
    expect(wrong.ok === false && wrong.error).toMatch(/wrong way round/);
    const allFixed = resolveModel(ref('mm', { ps: { Vmax: { fixed: true }, Km: { fixed: true } } }), 'yx', 'x');
    expect(allFixed.ok === false && allFixed.error).toMatch(/Every parameter is fixed/);
  });
});

describe('script generation', () => {
  const script = (patch: Partial<FitSpec> = {}) => {
    const r = buildCurveFit(spec(patch), 'show');
    if (!r.ok) throw new Error(r.error);
    return r.plan.script;
  };

  it('a single fit is plain least_squares on a visible residual function', () => {
    const s = script({ model: ref('exp') });
    expect(s).toContain('function cf_model(x, a, b, c)');
    expect(s).toContain('function cf_resid(q)');
    expect(s).toContain('cf_fit = least_squares("cf_resid", cf_q0, max_iter=500)');
    expect(s).toContain('x = [1, 2, 3, 4, 5, 6]');
  });

  it('bounds become lower=/upper=, a fixed parameter is inlined, weights multiply the residual', () => {
    const s = script({ model: ref('exp', { ps: { a: { lo: '0' }, b: { hi: '3' }, c: { fixed: true, guess: '0.5' } } }) });
    expect(s).toContain('lower=[0, -inf], upper=[inf, 3]');
    expect(s).toContain('cf_model(x, q[0], q[1], 0.5)');
    expect(s).toContain('# starting values of a, b');
    const w = script({ model: ref('exp'), weight: 'sigma', table: table({ x: [1, 2, 3, 4], y: [1, 2, 3, 4], e: [1, 1, 1, 1] }), cols: { x: 'x', y: 'y', z: '', err: 'e', t: 'x', signals: [] } });
    expect(w).toContain('wt = 1 ./ sg');
    expect(w).toContain('.* wt');
    expect(w).toContain('sg = [1, 1, 1, 1]');
  });

  it('robust and multi-start write their loops where the user can read them', () => {
    const s = script({ model: ref('poly', { degree: 1 }), robust: 'bisquare', starts: 3 });
    expect(s).toContain('iteratively reweighted least squares');
    expect(s).toContain('cf_rw = ones(length(y))');
    expect(s).toContain('.* cf_rw');
    expect(s).toContain('for cf_ms = 0 to 1');
    const h = script({ model: ref('poly', { degree: 1 }), robust: 'huber' });
    expect(h).toContain('Huber');
    expect(h).not.toContain('bisquare');
  });

  it('only the summary lines a start value needs are written', () => {
    const s = script({ model: ref('mm') });
    expect(s).toContain('cf_yhi = max(y)');
    expect(s).not.toContain('cf_ipk');
    const o = script({ model: ref('origin') });
    expect(o).toContain('cf_a0 = sum(x .* y) / sum(x .* x)');
  });

  it('compare writes one guarded block per model and a ranking', () => {
    const s = script({ mode: 'compare', compare: [ref('poly', { degree: 1 }), ref('exp'), ref('power')], criterion: 'bic' });
    expect(s.match(/^try$/gm)?.length).toBeGreaterThanOrEqual(3);
    expect(s).toContain('cf3_stat');
    expect(s).toContain('cmp_score = [cf1_stat[11], cf2_stat[11], cf3_stat[11]]');
    expect(s).toContain('cmp_order = argsort(cmp_key)');
    expect(buildCurveFit(spec({ mode: 'compare', compare: [ref('exp')] }), 'show').ok).toBe(false);
  });

  it('time series: shared parameter layout, ODE, separate', () => {
    const cols = table({ t: [0, 1, 2, 3, 4, 5], a: [5, 3, 2, 1, 0.8, 0.5], b: [3, 2, 1, 0.7, 0.5, 0.3] });
    const base = { shape: 'multi' as const, table: cols, cols: { x: '', y: '', z: '', err: '', t: 't', signals: ['a', 'b'] } };
    const sh = buildCurveFit(spec({ ...base, coupling: 'shared', model: ref('exp0'), shared: ['b'] }), 'show');
    expect(sh.ok && sh.plan.jointLabels).toEqual(['b (shared)', 'a (a)', 'a (b)']);
    expect(sh.ok && sh.plan.script).toContain('return [cf_eval1(t, q) - s_a, cf_eval2(t, q) - s_b]');
    const ode = buildCurveFit(spec({ ...base, coupling: 'ode', odeRhs: 'r*y*(1-y/K)' }), 'show');
    expect(ode.ok && ode.plan.script).toContain('ode45(f');
    expect(ode.ok && ode.plan.script).toContain('function cf1_rhs(t, y, r, K)');
    expect(ode.ok && ode.plan.script).toContain('argsort(t)');
    expect(ode.ok && ode.plan.blocks[0].free).toEqual(['r', 'K', 'y0']);
    expect(buildCurveFit(spec({ ...base, coupling: 'shared', model: ref('exp0'), shared: [] }), 'show').ok).toBe(false);
  });

  it('signal variable names are safe and unique', () => {
    const s = spec({ cols: { x: '', y: '', z: '', err: '', t: 't', signals: ['a b', 'a-b', 'c'] } });
    expect(signalVar(s, 'a b')).toBe('s_a_b');
    expect(signalVar(s, 'a-b')).toBe('s_a_b_');
    expect(signalVar(s, 'c')).toBe('s_c');
  });

  it('surfaces fit z on a 30 x 30 grid of flat vectors', () => {
    const t = table({ x: [0, 1, 2, 3, 0, 1, 2, 3], y: [0, 0, 1, 1, 2, 2, 3, 3], z: [1, 2, 3, 4, 5, 6, 7, 9] });
    const r = buildCurveFit(spec({ shape: 'surface', table: t, cols: { x: 'x', y: 'y', z: 'z', err: '', t: '', signals: [] }, model: ref('plane') }), 'show');
    expect(r.ok && r.plan.script).toContain('cf_gz = cf_eval(cf_gx, cf_gy, cf_q)');
    expect(r.ok && r.plan.script).toContain('function cf_eval(x, y, q)');
  });
});

describe('validation before running', () => {
  it('says what is missing in plain words', () => {
    expect(validateSpec({ ...spec(), table: null })).toMatch(/Load some data/);
    expect(validateSpec(spec({ cols: { x: 'x', y: 'x', z: '', err: '', t: '', signals: [] } }))).toMatch(/same column/);
    expect(validateSpec(spec({ cols: { x: 'q', y: 'y', z: '', err: '', t: '', signals: [] } }))).toMatch(/Pick the x column/);
    expect(validateSpec(spec({ weight: 'sigma' }))).toMatch(/y-error/);
    expect(validateSpec(spec({ starts: 0 }))).toMatch(/Multi-start/);
    expect(validateSpec(spec({ extrap: -1 }))).toMatch(/extrapolation/);
    expect(validateSpec({ ...spec(), table: table({ x: [1, 2], y: [1, 2] }) })).toMatch(/more points than parameters/);
    expect(validateSpec(spec())).toBeNull();
  });
  it('an example of the wrong shape is refused', () => {
    expect(validateSpec({ ...defaultSpec(), shape: 'surface', example: 'decay' })).toMatch(/not z\(x, y\)/);
    expect(validateSpec({ ...defaultSpec(), example: 'decay' })).toBeNull();
  });
  it('rows with missing values do not count', () => {
    const t = table({ x: [1, 2, NaN, NaN, NaN], y: [1, 2, 3, 4, 5] });
    expect(validateSpec({ ...spec(), table: t })).toMatch(/Only 2 complete rows/);
  });
});

describe('data lines', () => {
  it('inlines up to 200 rows, reads a CSV beyond that, drops rows with missing values', () => {
    const small = dataLines({ source: 'paste', table: table({ x: [1, 2, NaN], y: [3, 4, 5] }) }, [{ col: 'x', as: 'x' }, { col: 'y', as: 'y' }], 'show');
    expect(small.join('\n')).toContain('x = [1, 2]');
    expect(small[0]).toMatch(/1 row with missing values left out/);
    const big = table({ x: Array.from({ length: 300 }, (_, i) => i), y: Array.from({ length: 300 }, (_, i) => i * 2) });
    const lines = dataLines({ source: 'paste', table: big }, [{ col: 'x', as: 'x' }, { col: 'y', as: 'y' }], 'run', 'C:\\tmp\\f.csv');
    expect(lines.join('\n')).toContain('read_csv("C:/tmp/f.csv")');
    expect(lines.join('\n')).toContain('x = fit_table.x');
    expect(csvForScript(big, [{ col: 'x', as: 'x' }, { col: 'y', as: 'y' }]).split('\n')[0]).toBe('x,y');
  });
  it('a session variable shows its own name but a run needs the numbers', () => {
    const t = table({ sigA: [1, 2, 3], sigB: [4, 5, 6] });
    const needs = [{ col: 'sigA', as: 'x' }, { col: 'sigB', as: 'y' }];
    expect(dataLines({ source: 'variable', table: t }, needs, 'show').join('\n')).toContain('x = sigA');
    expect(dataLines({ source: 'variable', table: t }, needs, 'run').join('\n')).toContain('x = [1, 2, 3]');
  });
  it('needsDataFile appears only for big tables', () => {
    expect(needsDataFile(spec())).toBeNull();
    const n = 250;
    const t = table({ x: Array.from({ length: n }, (_, i) => i), y: Array.from({ length: n }, (_, i) => i) });
    expect(needsDataFile(spec({ table: t }))?.csv.split('\n').length).toBe(n + 2);
  });
  it('number literals survive a round trip', () => {
    expect(vectorLiteral([1, 0.1, 1e-7, 123456789.123456789, -2.5])).toBe('[1, 0.1, 1e-7, 123456789.123457, -2.5]');
  });
});

describe('parseTable', () => {
  it('reads comma, tab, semicolon (decimal comma) and whitespace data, with and without header', () => {
    expect(parseTable('x,y\n1,2\n3,4').columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(parseTable('1\t2\n3\t4').columns.map((c) => c.name)).toEqual(['col1', 'col2']);
    expect(parseTable('t;v\n0,5;1,5\n1,5;2,5').columns[1].values).toEqual([1.5, 2.5]);
    expect(parseTable('1 2\n3 4\n5 6').rows).toBe(3);
    expect(parseTable('# comment\nx y\n1 2\n3 4').header).toBe(true);
  });
  it('reports text columns, bad cells and too few columns without throwing', () => {
    const t = parseTable('name,x,y\na,1,2\nb,2,3\nc,3,oops');
    expect(t.columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(t.problems.join(' ')).toMatch(/'name' is not numeric/);
    expect(t.problems.join(' ')).toMatch(/not a number/);
    expect(parseTable('').problems[0]).toMatch(/No data/);
    expect(parseTable('1\n2\n3').problems.join(' ')).toMatch(/at least two numeric columns/);
  });
  it('column statistics ignore non-finite values', () => {
    expect(columnStats([1, 2, 3, NaN])).toMatchObject({ n: 3, min: 1, max: 3, mean: 2, std: 1 });
  });
});

describe('every built-in example is consistent with the generator', () => {
  it('its columns are defined by its lines and its model resolves', () => {
    for (const ex of EXAMPLES) {
      for (const c of ex.columns) expect(ex.lines.join('\n'), `${ex.id}.${c}`).toMatch(new RegExp(`^${c} = `, 'm'));
      const r = buildCurveFit({ ...defaultSpec(), shape: ex.shape, example: ex.id, model: ex.model === 'ode' ? ref('exp0') : ref(ex.model, { degree: ex.model === 'quad' ? 2 : 1 }), coupling: ex.model === 'ode' ? 'ode' : 'separate', odeRhs: 'r*y*(1-y/K)', mode: 'single' }, 'show');
      expect(r.ok, `${ex.id}: ${r.ok ? '' : r.error}`).toBe(true);
    }
  });
});

describe('reading results', () => {
  it('stats vector round trip and failure detection', () => {
    const v = STAT_FIELDS.map((_, i) => i + 1);
    expect(readStats(v)?.r2).toBe(5);
    expect(readStats(STAT_FIELDS.map(() => null))).toBeNull();
    expect(readStats(undefined)).toBeNull();
  });
  it('finds CFERR lines', () => {
    expect(scriptErrors('hello\nCFERR cf2 matrix is singular\nCFWARN cf1 x')).toEqual({ cf2: 'matrix is singular' });
  });
  it('ranking marks failed models and equal fits', () => {
    const plan = buildCurveFit(spec({ mode: 'compare', compare: [ref('poly', { degree: 1 }), ref('exp')] }), 'show');
    expect(plan.ok).toBe(true);
    if (!plan.ok) return;
    const blocks = plan.plan.blocks.map((info, i) => ({ info, ok: i === 0, converged: true, stats: null, params: [], gx: [], gy: [], gb: [], gz: [], pred: [], res: [], rw: [] }));
    const r = readRanking({ cmp_order: [0, 1], cmp_delta: [0, null], cmp_weight: [1, 0] }, blocks as never);
    expect(r[0]).toMatchObject({ index: 0, status: 'ok', equalFit: true });
    expect(r[1].status).toBe('failed');
  });
  it('errorContext finds the failing line of the script', () => {
    const script = 'a = 1\nb = nope(2)\nc = 3\n';
    expect(errorContext('qu: runtime error: unknown function `nope`\n  at script.qu:2', script)).toEqual({ line: 2, text: 'b = nope(2)' });
    expect(errorContext('something else', script)).toBeNull();
    expect(plainError('qu: runtime error: unknown function `nope`\n  at x.qu:2')).toBe('unknown function `nope`');
  });
});

describe('ODE resolution', () => {
  it('needs the state and a parameter, and forbids a parameter called y0', () => {
    const o = (rhs: string, extra: Partial<FitSpec> = {}) => resolveOde({ ...defaultSpec(), odeRhs: rhs, ...extra });
    expect(o('-k*y').ok).toBe(true);
    expect(o('-k*y; 1').ok).toBe(false);
    const noY = o('-k*t');
    expect(noY.ok === false && noY.error).toMatch(/does not use y/);
    const noParam = o('-y');
    expect(noParam.ok === false && noParam.error).toMatch(/named parameter/);
    expect(o('-y0*y').ok).toBe(false);
  });
});

describe('solve tool script generation', () => {
  it('polynomial input takes the roots() route, anything else scans', () => {
    const p = buildSolve({ ...defaultSolveSpec(), expr: '2*x^2+3*x' });
    expect(p.ok && p.plan.kind).toBe('polynomial');
    expect(p.ok && p.plan.script).toContain('sv_coef = [2, 3, 0]');
    expect(p.ok && p.plan.script).toContain('roots(sv_coef)');
    const s = buildSolve({ ...defaultSolveSpec(), expr: 'cos(x) - x' });
    expect(s.ok && s.plan.kind).toBe('scan');
    expect(s.ok && s.plan.script).toContain('fzero(f, s[i], s[i + 1])');
    const f = buildSolve({ ...defaultSolveSpec(), expr: 'x^2', forceNumeric: true });
    expect(f.ok && f.plan.kind).toBe('scan');
  });
  it('validates the interval, the unknown count, and constants', () => {
    expect(buildSolve({ ...defaultSolveSpec(), lo: '5', hi: '1' }).ok).toBe(false);
    expect(buildSolve({ ...defaultSolveSpec(), lo: 'a' }).ok).toBe(false);
    const two = buildSolve({ ...defaultSolveSpec(), expr: 'x + y' });
    expect(two.ok === false && two.error).toMatch(/More than one unknown/);
    const none = buildSolve({ ...defaultSolveSpec(), expr: '3 + 4' });
    expect(none.ok === false && none.error).toMatch(/no unknown/);
    const both = buildSolve({ ...defaultSolveSpec(), expr: 'x = 2', rhs: '5' });
    expect(both.ok === false && both.error).toMatch(/already has an '='/);
    const diffConst = buildSolve({ ...defaultSolveSpec(), expr: 'x + 1', rhs: 'x + 3' });
    expect(diffConst.ok === false && diffConst.error).toMatch(/differ by a constant/);
  });
  it('systems: linear detection, nonlinear guess, limits, multistart parameters', () => {
    const lin = buildSolve({ ...defaultSolveSpec(), task: 'system', eqs: ['2*x + 3*y = 5', 'x - y = 0', '', ''] });
    expect(lin.ok && lin.plan.kind).toBe('linear');
    expect(lin.ok && lin.plan.script).toContain('sv_A = [2, 3; 1, -1]');
    const non = buildSolve({ ...defaultSolveSpec(), task: 'system', guesses: { x: '2', y: '-2' } });
    expect(non.ok && non.plan.script).toContain('sv_x0 = [2, -2]');
    expect(buildSolve({ ...defaultSolveSpec(), task: 'system', eqs: ['x = 1', 'y = 2', 'z = 3', 'w = 4'].concat(['v = 5']) }).ok).toBe(false);
    expect(buildSolve({ ...defaultSolveSpec(), task: 'system', eqs: ['', '', '', ''] }).ok).toBe(false);
    expect(buildSolve({ ...defaultSolveSpec(), task: 'system', multistart: true, range: '-1' }).ok).toBe(false);
    expect(buildSolve({ ...defaultSolveSpec(), task: 'system', guesses: { x: 'system(1)' } }).ok).toBe(false);
    expect(buildSolve({ ...defaultSolveSpec(), task: 'system', eqs: ['sv_a + y = 1', 'y = 2', '', ''] }).ok).toBe(false);
  });
  it('reads the multistart solutions from the column-major matrix', () => {
    const plan = buildSolve({ ...defaultSolveSpec(), task: 'system', multistart: true });
    expect(plan.ok).toBe(true);
    if (!plan.ok) return;
    const S = Array(64).fill(0);
    S[0] = 1; S[32] = 2; S[1] = -1; S[33] = -2;
    const out = readSolve(plan.plan, { sv_S: S, sv_ns: [2], sv_x: [0, 0], sv_norm: [0], sv_conv: [1] });
    expect(out.solutions).toEqual([[1, 2], [-1, -2]]);
  });
});
