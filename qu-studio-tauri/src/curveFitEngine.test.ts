/** Engine gate for the Fit tab. Skipped unless QU_BIN points at a qu executable.
 *  Every scenario builds the script exactly as the panel does, runs it with the
 *  real binary, and compares with INDEPENDENT truth: data generated here from a
 *  known model with a seeded generator, closed-form regression formulas, known
 *  roots. Nothing is compared with output copied from the code under test. */
import { describe, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  buildCurveFit, defaultSpec, emptyRef, readBlock, readRanking, scriptErrors, validateSpec, needsDataFile,
  type FitSpec, type ModelRef, type Plan, type Series, type BlockResult,
} from './curveFitDesign';
import { parseTable, type Table } from './curveFitData';
import { buildSolve, defaultSolveSpec, readSolve, type SolveSpec } from './solveDesign';

const QU = process.env.QU_BIN;
const d = QU ? describe : describe.skip;

// ------------------------------------------------------------------ harness

function rng(seed: number) {
  let a = seed >>> 0;
  const u = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const n = () => Math.sqrt(-2 * Math.log(u() + 1e-300)) * Math.cos(2 * Math.PI * u());
  return { u, n };
}
const linspace = (a: number, b: number, n: number) => Array.from({ length: n }, (_, i) => a + ((b - a) * i) / (n - 1));

interface Ran {
  code: number | null;
  out: string;
  err: string;
  series: Series;
}

function runScript(script: string): Ran {
  const dir = mkdtempSync(join(tmpdir(), 'curvefit-'));
  const file = join(dir, 'script.qu');
  const data = join(dir, 'data.json');
  writeFileSync(file, script);
  const r = spawnSync(QU!, ['run', file, '--emit-data', data], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 180000 });
  const series: Series = {};
  try {
    for (const v of JSON.parse(readFileSync(data, 'utf8')) as Array<{ name: string; data: (number | null)[] }>) series[v.name] = v.data;
  } catch {
    /* a failed run leaves no data */
  }
  return { code: r.status, out: r.stdout ?? '', err: r.stderr ?? '', series };
}

function tableOf(cols: Record<string, number[]>): Table {
  const names = Object.keys(cols);
  const rows = cols[names[0]].length;
  return { columns: names.map((name) => ({ name, values: cols[name] })), rows, header: true, delimiter: 'comma', problems: [] };
}

function specFor(cols: Record<string, number[]>, patch: Partial<FitSpec>): FitSpec {
  const names = Object.keys(cols);
  return { ...defaultSpec(), source: 'paste', table: tableOf(cols), cols: { x: names[0], y: names[1], z: names[2] ?? '', err: '', t: names[0], signals: names.slice(1) }, ...patch };
}

function fit(spec: FitSpec): { plan: Plan; ran: Ran; blocks: BlockResult[] } {
  let dataFile = '';
  const need = needsDataFile(spec);
  if (need) {
    const dir = mkdtempSync(join(tmpdir(), 'curvefit-csv-'));
    dataFile = join(dir, 'fit_data.csv');
    writeFileSync(dataFile, need.csv);
  }
  const p = buildCurveFit(spec, 'run', dataFile);
  if (!p.ok) throw new Error(p.error);
  const ran = runScript(p.plan.script);
  expect(ran.err, ran.err + '\n' + p.plan.script).toBe('');
  expect(ran.code).toBe(0);
  const errors = scriptErrors(ran.out);
  const blocks = p.plan.blocks.map((b) => readBlock(b, ran.series, errors, p.plan.layout === 'shared' ? p.plan.jointLabels : undefined, p.plan.layout === 'shared' ? [p.plan.jointLo!, p.plan.jointHi!] : undefined));
  return { plan: p.plan, ran, blocks };
}

const value = (b: BlockResult, name: string) => b.params.find((p) => p.name === name)!.value;
const near = (got: number, want: number, tol: number, what: string) => expect(Math.abs(got - want), `${what}: got ${got}, want ${want} +/- ${tol}`).toBeLessThanOrEqual(tol);
const ref = (id: string, extra: Partial<ModelRef> = {}): ModelRef => ({ ...emptyRef(id), ...extra });

// -------------------------------------------------------------------- y(x)

d('Fit tab engine gate: y(x)', () => {
  it('polynomial: recovers a known quadratic', () => {
    const g = rng(1);
    const x = linspace(-3, 3, 50);
    const y = x.map((v) => 2 * v * v - 1.5 * v + 0.7 + 0.05 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('poly', { degree: 2 }) }));
    const b = blocks[0];
    expect(b.ok && b.converged).toBe(true);
    near(value(b, 'c2'), 2, 0.05, 'c2');
    near(value(b, 'c1'), -1.5, 0.05, 'c1');
    near(value(b, 'c0'), 0.7, 0.05, 'c0');
    expect(b.stats!.r2).toBeGreaterThan(0.999);
  });

  it('exponential decay: recovers a, b, c', () => {
    const g = rng(2);
    const x = linspace(0, 10, 60);
    const y = x.map((v) => 3 * Math.exp(-0.5 * v) + 0.4 + 0.03 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('exp') }));
    const b = blocks[0];
    expect(b.converged).toBe(true);
    near(value(b, 'a'), 3, 0.1, 'a');
    near(value(b, 'b'), 0.5, 0.03, 'b');
    near(value(b, 'c'), 0.4, 0.05, 'c');
    // the 95% interval of b brackets the truth
    const pb = b.params.find((p) => p.name === 'b')!;
    expect(pb.lo).toBeLessThan(0.5);
    expect(pb.hi).toBeGreaterThan(0.5);
  });

  it('exponential growth: the first click also works for b < 0', () => {
    const g = rng(12);
    const x = linspace(0, 5, 50);
    const y = x.map((v) => 0.5 * Math.exp(0.6 * v) + 0.02 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('exp0') }));
    near(value(blocks[0], 'b'), -0.6, 0.03, 'b (negative for growth)');
    near(value(blocks[0], 'a'), 0.5, 0.05, 'a');
  });

  it('power law: recovers a and b', () => {
    const g = rng(3);
    const x = linspace(1, 10, 40);
    const y = x.map((v) => 2.5 * Math.pow(v, 1.7) * (1 + 0.01 * g.n()));
    const { blocks } = fit(specFor({ x, y }, { model: ref('power') }));
    near(value(blocks[0], 'a'), 2.5, 0.1, 'a');
    near(value(blocks[0], 'b'), 1.7, 0.02, 'b');
  });

  it('Gaussian peak: recovers height, centre, width', () => {
    const g = rng(4);
    const x = linspace(0, 10, 80);
    const y = x.map((v) => 4 * Math.exp(-((v - 5.5) ** 2) / (2 * 0.9 ** 2)) + 0.05 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('gauss') }));
    near(value(blocks[0], 'a'), 4, 0.1, 'a');
    near(value(blocks[0], 'mu'), 5.5, 0.03, 'mu');
    near(Math.abs(value(blocks[0], 's')), 0.9, 0.03, 's');
  });

  it('sine: the FFT-peak start finds amplitude, frequency, phase, offset', () => {
    const g = rng(5);
    const x = linspace(0, 6, 150);
    const y = x.map((v) => 2 * Math.sin(3.1 * v + 0.7) + 0.5 + 0.1 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('sine') }));
    const b = blocks[0];
    near(Math.abs(value(b, 'a')), 2, 0.1, 'a');
    near(Math.abs(value(b, 'w')), 3.1, 0.05, 'w');
    near(value(b, 'c'), 0.5, 0.05, 'c');
    // sign of a and w can flip together with the phase: compare the curves instead of the phase
    const truth = x.map((v) => 2 * Math.sin(3.1 * v + 0.7) + 0.5);
    const rmse = Math.sqrt(b.pred.reduce((s, p, i) => s + (p - truth[i]) ** 2, 0) / truth.length);
    expect(rmse).toBeLessThan(0.05);
  });

  it('logistic: recovers L, k, x0', () => {
    const g = rng(6);
    const x = linspace(-6, 10, 70);
    const y = x.map((v) => 5 / (1 + Math.exp(-0.9 * (v - 2))) + 0.04 * g.n());
    const { blocks } = fit(specFor({ x, y }, { model: ref('logistic') }));
    near(value(blocks[0], 'L'), 5, 0.1, 'L');
    near(value(blocks[0], 'k'), 0.9, 0.06, 'k');
    near(value(blocks[0], 'x0'), 2, 0.1, 'x0');
  });

  it('custom expression a*exp(-b*x)+c: recovers the parameters, in the typed parameter names', () => {
    const g = rng(7);
    const x = linspace(0, 8, 50);
    const y = x.map((v) => 1.8 * Math.exp(-0.7 * v) + 0.25 + 0.02 * g.n());
    const { blocks, plan } = fit(specFor({ x, y }, { model: ref('custom', { expr: 'a*exp(-b*x)+c' }) }));
    expect(plan.script).toContain('a * exp(-b * x) + c');
    near(value(blocks[0], 'a'), 1.8, 0.08, 'a');
    near(value(blocks[0], 'b'), 0.7, 0.04, 'b');
    near(value(blocks[0], 'c'), 0.25, 0.03, 'c');
  });

  it('standard errors match the closed-form straight-line regression formulas', () => {
    const g = rng(8);
    const x = linspace(0, 10, 30);
    const y = x.map((v) => 1.3 * v + 2 + 0.8 * g.n());
    const n = x.length;
    const mx = x.reduce((a, b) => a + b) / n;
    const my = y.reduce((a, b) => a + b) / n;
    const sxx = x.reduce((a, v) => a + (v - mx) ** 2, 0);
    const slope = x.reduce((a, v, i) => a + (v - mx) * (y[i] - my), 0) / sxx;
    const icpt = my - slope * mx;
    const rss = x.reduce((a, v, i) => a + (y[i] - icpt - slope * v) ** 2, 0);
    const s2 = rss / (n - 2);
    const seSlope = Math.sqrt(s2 / sxx);
    const seIcpt = Math.sqrt(s2 * (1 / n + (mx * mx) / sxx));
    const { blocks } = fit(specFor({ x, y }, { model: ref('poly', { degree: 1 }) }));
    const b = blocks[0];
    near(value(b, 'c1'), slope, 1e-4, 'slope');
    near(value(b, 'c0'), icpt, 1e-4, 'intercept');
    near(b.params.find((p) => p.name === 'c1')!.se, seSlope, 0.01 * seSlope, 'se slope');
    near(b.params.find((p) => p.name === 'c0')!.se, seIcpt, 0.01 * seIcpt, 'se intercept');
    // 95% interval half-width = t(0.975, n-2) * se, t(0.975, 28) = 2.048407
    const p1 = b.params.find((p) => p.name === 'c1')!;
    near((p1.hi - p1.lo) / 2, 2.048407 * seSlope, 0.01 * seSlope, 'CI half-width');
    near(b.stats!.r2, 1 - rss / x.reduce((a, _, i) => a + (y[i] - my) ** 2, 0), 1e-6, 'R2');
    near(b.stats!.rmse, Math.sqrt(rss / n), 1e-6, 'RMSE');
  });

  it('weights: y errors change the fit and reduced chi-square is near 1 for honest errors', () => {
    const g = rng(9);
    const x = linspace(0, 10, 60);
    const sg = x.map((v) => 0.05 + 0.04 * v);
    const y = x.map((v, i) => 1 + 2 * v + sg[i] * g.n());
    const { blocks } = fit(specFor({ x, y, sg }, { model: ref('poly', { degree: 1 }), weight: 'sigma', cols: { x: 'x', y: 'y', z: '', err: 'sg', t: 'x', signals: [] } }));
    near(value(blocks[0], 'c1'), 2, 0.05, 'slope');
    near(blocks[0].stats!.chi2red, 1, 0.45, 'reduced chi-square');
  });

  it('robust fitting recovers the line when 10% of the points are gross outliers; plain least squares does not', () => {
    const g = rng(10);
    const x = linspace(0, 10, 60);
    const y = x.map((v) => 2 * v + 1 + 0.2 * g.n());
    for (const i of [3, 11, 20, 29, 41, 55]) y[i] += i % 2 ? 40 : -35;
    const plain = fit(specFor({ x, y }, { model: ref('poly', { degree: 1 }) })).blocks[0];
    const huber = fit(specFor({ x, y }, { model: ref('poly', { degree: 1 }), robust: 'huber' })).blocks[0];
    const bis = fit(specFor({ x, y }, { model: ref('poly', { degree: 1 }), robust: 'bisquare' })).blocks[0];
    expect(Math.abs(value(plain, 'c0') - 1) + Math.abs(value(plain, 'c1') - 2)).toBeGreaterThan(1);
    for (const b of [huber, bis]) {
      near(value(b, 'c1'), 2, 0.1, 'robust slope');
      near(value(b, 'c0'), 1, 0.4, 'robust intercept');
    }
    // the gross outliers are down-weighted to (almost) nothing
    expect(bis.rw[3]).toBeLessThan(0.1);
  });

  it('bounds and fixed parameters: a bound is enforced and flagged, a fixed value is held', () => {
    const g = rng(11);
    const x = linspace(0, 10, 50);
    const y = x.map((v) => 3 * Math.exp(-0.5 * v) + 0.4 + 0.03 * g.n());
    const bounded = fit(specFor({ x, y }, { model: ref('exp', { ps: { b: { hi: '0.3' } } }) })).blocks[0];
    expect(value(bounded, 'b')).toBeLessThanOrEqual(0.3 + 1e-12);
    expect(bounded.params.find((p) => p.name === 'b')!.atBound).toBe('upper');
    const fixed = fit(specFor({ x, y }, { model: ref('exp', { ps: { c: { fixed: true, guess: '0.4' } } }) }));
    expect(fixed.blocks[0].params.find((p) => p.name === 'c')!.fixed).toBe(true);
    expect(fixed.blocks[0].params.length).toBe(3);
    near(value(fixed.blocks[0], 'b'), 0.5, 0.03, 'b with c held at 0.4');
    expect(fixed.plan.script).toContain('q[1], 0.4)');
  });

  it('compare: ranks the true model first against wrong ones, and marks a failure instead of hiding it', () => {
    const g = rng(13);
    const x = linspace(0, 10, 60);
    const y = x.map((v) => 3 * Math.exp(-0.5 * v) + 0.4 + 0.03 * g.n());
    const compare = [ref('poly', { degree: 1 }), ref('exp'), ref('sine'), ref('custom', { expr: 'a*x/(b+c)' }), ref('log')];
    // 'log' has x = 0 in the data: ln(0) = -inf, which must not rank first nor vanish
    const { blocks, ran, plan } = fit(specFor({ x, y }, { mode: 'compare', compare, criterion: 'aic' }));
    const ranked = readRanking(ran.series, blocks);
    expect(ranked.length).toBe(compare.length);
    expect(ranked[0].index).toBe(1);
    expect(ranked[0].equalFit).toBe(true);
    expect(ranked.find((r) => r.index === 0)!.delta).toBeGreaterThan(20);
    expect(plan.script).toContain('cmp_order');
    // every model has an entry: ok, not converged, or failed, never silently absent
    for (const r of ranked) expect(['ok', 'not converged', 'failed']).toContain(r.status);
    // the unidentifiable custom model (b and c only enter as b + c) must not beat the true model
    expect(ranked[0].index).not.toBe(3);
  });

  it('compare: nested polynomials fit equally well when the higher degree adds nothing', () => {
    const g = rng(14);
    const x = linspace(-2, 2, 50);
    const y = x.map((v) => 1 + 2 * v + 0.1 * g.n());
    const { blocks, ran } = fit(specFor({ x, y }, { mode: 'compare', compare: [ref('poly', { degree: 1 }), ref('poly', { degree: 2 }), ref('exp0'), ref('sine')], criterion: 'bic' }));
    const ranked = readRanking(ran.series, blocks);
    expect(ranked[0].index).toBe(0); // BIC punishes the useless quadratic term
    expect([0, 1]).toContain(ranked[1].index);
  });

  it('a large pasted table goes through a CSV file and still fits', () => {
    const g = rng(15);
    const x = linspace(0, 10, 400);
    const y = x.map((v) => 0.5 * v + 3 + 0.1 * g.n());
    const spec = specFor({ x, y }, { model: ref('poly', { degree: 1 }) });
    const p = buildCurveFit(spec, 'show');
    expect(p.ok && p.plan.script).toContain('read_csv');
    const { blocks } = fit(spec);
    near(value(blocks[0], 'c1'), 0.5, 0.01, 'slope');
  });

  it('example data generated by Qu itself runs through every y(x) example', () => {
    const cases: Array<[string, string, Record<string, number>]> = [
      ['decay', 'exp', { b: 0.5 }],
      ['peak', 'gauss', { mu: 5.5 }],
      ['saturation', 'mm', { Vmax: 5, Km: 1.5 }],
    ];
    for (const [ex, model, truth] of cases) {
      const spec = { ...defaultSpec(), example: ex, model: ref(model) };
      const { blocks } = fit(spec);
      for (const [k, v] of Object.entries(truth)) near(value(blocks[0], k), v, 0.15 * Math.abs(v) + 0.05, `${ex}.${k}`);
    }
  });

  it('the confidence band matches the closed-form band of a straight-line regression', () => {
    const g = rng(16);
    const x = linspace(0, 10, 30);
    const y = x.map((v) => 1.3 * v + 2 + 0.8 * g.n());
    const n = x.length;
    const mx = x.reduce((a, b) => a + b) / n;
    const my = y.reduce((a, b) => a + b) / n;
    const sxx = x.reduce((a, v) => a + (v - mx) ** 2, 0);
    const slope = x.reduce((a, v, i) => a + (v - mx) * (y[i] - my), 0) / sxx;
    const rss = x.reduce((a, v, i) => a + (y[i] - my - slope * (v - mx)) ** 2, 0);
    const s2 = rss / (n - 2);
    const b = fit(specFor({ x, y }, { model: ref('poly', { degree: 1 }), band: true, extrap: 0.5 })).blocks[0];
    expect(b.gx.length).toBe(400);
    expect(b.gx[0]).toBeCloseTo(-5, 9);
    expect(b.gx[399]).toBeCloseTo(15, 9);
    for (const k of [0, 100, 200, 300, 399]) {
      const want = 2.048407 * Math.sqrt(s2 * (1 / n + (b.gx[k] - mx) ** 2 / sxx));
      near(b.gb[k], want, 0.005 * want, `band at x = ${b.gx[k]}`);
    }
  });

  it('log-x display uses a geometric grid', () => {
    const x = [1, 2, 4, 8, 16, 32, 64];
    const y = x.map((v) => 3 * Math.log(v) + 1);
    const b = fit(specFor({ x, y }, { model: ref('log'), logx: true, band: false })).blocks[0];
    near(b.gx[1] / b.gx[0], b.gx[399] / b.gx[398], 1e-9, 'constant ratio');
    near(value(b, 'a'), 3, 1e-6, 'a');
  });

  it('every catalogue model fits data drawn from itself on the first click (default starts)', () => {
    const g = rng(40);
    const cases: Array<[string, (v: number) => number, number, number, Record<string, number>?]> = [
      ['poly', (v) => 0.5 * v ** 3 - v ** 2 + 2 * v + 1, -2, 3, { degree: 3 } as never],
      ['origin', (v) => 2.5 * v, 0, 10],
      ['exp', (v) => 3 * Math.exp(-0.5 * v) + 0.4, 0, 10],
      ['exp0', (v) => 4 * Math.exp(-0.3 * v), 0, 10],
      ['power', (v) => 2 * v ** 1.5, 1, 10],
      ['log', (v) => 2 * Math.log(v) + 1, 1, 20],
      ['logistic', (v) => 5 / (1 + Math.exp(-1.2 * (v - 4))), 0, 10],
      ['sigmoid4', (v) => 1 + 4 / (1 + Math.exp(-1.2 * (v - 4))), 0, 10],
      ['gauss', (v) => 3 * Math.exp(-((v - 5) ** 2) / (2 * 1.2 ** 2)), 0, 10],
      ['gaussb', (v) => 0.5 + 3 * Math.exp(-((v - 5) ** 2) / (2 * 1.2 ** 2)), 0, 10],
      ['lorentz', (v) => 3 / (1 + ((v - 5) / 0.8) ** 2), 0, 10],
      ['exp2', (v) => 3 * Math.exp(-0.2 * v) + 2 * Math.exp(-2 * v), 0, 12],
      ['sine', (v) => 1.5 * Math.sin(2.2 * v + 0.4) + 0.3, 0, 8],
      ['rational', (v) => (2 * v + 1) / (v + 3), 0, 20],
      ['mm', (v) => (4 * v) / (1.5 + v), 0.2, 10],
      ['arrhenius', (v) => 5 * Math.exp(-300 / v), 250, 450],
      ['hill', (v) => (4 * v ** 2) / (3 ** 2 + v ** 2), 0.2, 10],
    ];
    const failed: string[] = [];
    for (const [id, f, lo, hi, extra] of cases) {
      const x = linspace(lo, hi, 80);
      const clean = x.map(f);
      const amp = Math.max(...clean) - Math.min(...clean);
      const y = clean.map((v) => v + 0.01 * amp * g.n());
      const b = fit(specFor({ x, y }, { model: ref(id, extra ?? {}) })).blocks[0];
      if (!b.ok || !b.converged || b.stats!.r2 < 0.99) failed.push(`${id}: ok=${b.ok} converged=${b.converged} R2=${b.stats?.r2} ${b.error ?? ''}`);
    }
    expect(failed).toEqual([]);
  });

  it('wrong specs are rejected before anything runs', () => {
    const x = [1, 2, 3, 4, 5];
    const y = [2, 4, 6, 8, 10];
    const bad = (model: ModelRef) => {
      const r = buildCurveFit(specFor({ x, y }, { model }), 'run');
      expect(r.ok).toBe(false);
      return r.ok ? '' : r.error;
    };
    expect(bad(ref('custom', { expr: 'a*exp(-b*x' }))).toMatch(/parenthesis/i);
    expect(bad(ref('custom', { expr: 'a*x + system(b)' }))).toMatch(/unknown function/i);
    expect(bad(ref('custom', { expr: 'a*x; b' }))).toMatch(/semicolon/i);
    expect(bad(ref('custom', { expr: 'x^2' }))).toMatch(/parameter/i);
  });

  it('a starting value that names something unknown is refused with the name', () => {
    const spec = specFor({ x: [1, 2, 3, 4, 5, 6], y: [1, 2, 3, 4, 5, 6] }, { model: ref('custom', { expr: 'a*x + b', ps: { a: { guess: 'd_nothere' } } }) });
    const r = buildCurveFit(spec, 'run');
    expect(r.ok).toBe(false);
    expect(r.ok ? '' : r.error).toMatch(/d_nothere/);
  });
});

// ----------------------------------------------------------------- z(x, y)

d('Fit tab engine gate: surfaces z(x, y)', () => {
  const pts = (seed: number, n: number, lo: number, hi: number) => {
    const g = rng(seed);
    return { x: Array.from({ length: n }, () => lo + (hi - lo) * g.u()), y: Array.from({ length: n }, () => lo + (hi - lo) * g.u()), g };
  };

  it('plane: recovers a known plane', () => {
    const { x, y, g } = pts(21, 80, -3, 3);
    const z = x.map((v, i) => 1.5 * v - 0.8 * y[i] + 2 + 0.05 * g.n());
    const { blocks } = fit(specFor({ x, y, z }, { shape: 'surface', model: ref('plane') }));
    const b = blocks[0];
    near(value(b, 'a'), 1.5, 0.03, 'a');
    near(value(b, 'b'), -0.8, 0.03, 'b');
    near(value(b, 'c'), 2, 0.05, 'c');
    expect(b.gz.length).toBe(900);
  });

  it('quadratic surface: recovers all six coefficients', () => {
    const { x, y, g } = pts(22, 150, -2, 2);
    const z = x.map((v, i) => 1 + 0.5 * v - 0.3 * y[i] + 0.2 * v * v + 0.1 * v * y[i] + 0.4 * y[i] * y[i] + 0.03 * g.n());
    const { blocks } = fit(specFor({ x, y, z }, { shape: 'surface', model: ref('quad', { degree: 2 }) }));
    const b = blocks[0];
    const truth: Record<string, number> = { c00: 1, c10: 0.5, c01: -0.3, c20: 0.2, c11: 0.1, c02: 0.4 };
    for (const [k, v] of Object.entries(truth)) near(value(b, k), v, 0.04, k);
    expect(b.stats!.r2).toBeGreaterThan(0.99);
  });

  it('2-D Gaussian and a custom surface expression', () => {
    const { x, y, g } = pts(23, 220, -3, 3);
    const z = x.map((v, i) => 3 * Math.exp(-((v - 0.5) ** 2 / (2 * 0.8 ** 2) + (y[i] + 0.4) ** 2 / (2 * 1.1 ** 2))) + 0.2 + 0.03 * g.n());
    const gb = fit(specFor({ x, y, z }, { shape: 'surface', model: ref('gauss2d') })).blocks[0];
    near(value(gb, 'x0'), 0.5, 0.05, 'x0');
    near(value(gb, 'y0'), -0.4, 0.05, 'y0');
    near(Math.abs(value(gb, 'sx')), 0.8, 0.06, 'sx');
    const q = pts(24, 120, -2, 2);
    const zq = q.x.map((v, i) => 1.2 * v * v - 0.7 * v * q.y[i] + 0.9 * q.y[i] * q.y[i] + 3 + 0.04 * q.g.n());
    const cb = fit(specFor({ x: q.x, y: q.y, z: zq }, { shape: 'surface', model: ref('custom', { expr: 'a*x^2+b*x*y+c*y^2+d' }) })).blocks[0];
    near(value(cb, 'a'), 1.2, 0.05, 'a');
    near(value(cb, 'b'), -0.7, 0.05, 'b');
    near(value(cb, 'c'), 0.9, 0.05, 'c');
    near(value(cb, 'd'), 3, 0.05, 'd');
  });

  it('the Qu-generated surface examples run', () => {
    const { blocks } = fit({ ...defaultSpec(), shape: 'surface', example: 'bowl', model: ref('quad', { degree: 2 }) });
    near(value(blocks[0], 'c20'), 0.2, 0.05, 'c20');
    near(value(blocks[0], 'c02'), 0.4, 0.05, 'c02');
  });
});

// ------------------------------------------------------- signals against time

d('Fit tab engine gate: several signals against time', () => {
  const decays = (seed: number) => {
    const g = rng(seed);
    const t = linspace(0, 6, 50);
    const mk = (a: number) => t.map((v) => a * Math.exp(-0.8 * v) + 0.03 * g.n());
    return { t, a: mk(5), b: mk(3), c: mk(1.5) };
  };

  it('separate fits recover each signal\'s own parameters', () => {
    const cols = decays(31);
    const { blocks } = fit(specFor(cols, { shape: 'multi', coupling: 'separate', model: ref('exp0') }));
    expect(blocks.length).toBe(3);
    [5, 3, 1.5].forEach((a, i) => {
      near(value(blocks[i], 'a'), a, 0.12, `a of signal ${i}`);
      near(value(blocks[i], 'b'), 0.8, 0.05, `b of signal ${i}`);
    });
  });

  it('shared-parameter joint fit recovers the common rate and the separate amplitudes', () => {
    const cols = decays(32);
    const { blocks, plan } = fit(specFor(cols, { shape: 'multi', coupling: 'shared', model: ref('exp0'), shared: ['b'] }));
    const j = blocks[0];
    expect(plan.jointLabels).toEqual(['b (shared)', 'a (a)', 'a (b)', 'a (c)']);
    expect(j.ok).toBe(true);
    const get = (label: string) => j.params.find((p) => p.name === label)!;
    near(get('b (shared)').value, 0.8, 0.02, 'shared rate');
    near(get('a (a)').value, 5, 0.1, 'amplitude a');
    near(get('a (b)').value, 3, 0.1, 'amplitude b');
    near(get('a (c)').value, 1.5, 0.1, 'amplitude c');
    // the shared rate is better determined than any single-signal rate
    const sep = fit(specFor(cols, { shape: 'multi', coupling: 'separate', model: ref('exp0') })).blocks;
    expect(get('b (shared)').se).toBeLessThan(Math.min(...sep.map((b) => b.params.find((p) => p.name === 'b')!.se)));
  });

  it('ODE fit recovers k from an exponential-decay series', () => {
    const g = rng(33);
    const t = linspace(0, 8, 40);
    const y = t.map((v) => 2 * Math.exp(-0.45 * v) + 0.01 * g.n());
    const { blocks } = fit(specFor({ t, y }, { shape: 'multi', coupling: 'ode', odeRhs: '-k*y' }));
    const b = blocks[0];
    expect(b.converged).toBe(true);
    near(value(b, 'k'), 0.45, 0.02, 'k');
    near(value(b, 'y0'), 2, 0.05, 'y0');
  });

  it('ODE fit recovers r and K from a logistic series', () => {
    const g = rng(34);
    const t = linspace(0, 12, 50);
    const y = t.map((v) => 10 / (1 + 9 * Math.exp(-0.9 * v)) + 0.05 * g.n());
    const { blocks } = fit(specFor({ t, p: y }, { shape: 'multi', coupling: 'ode', odeRhs: 'r*y*(1-y/K)', odePs: { r: { guess: '0.5' }, K: { guess: '8' } } }));
    const b = blocks[0];
    near(value(b, 'r'), 0.9, 0.06, 'r');
    near(value(b, 'K'), 10, 0.2, 'K');
  });

  it('the Qu-generated time-series examples run', () => {
    const e = fit({ ...defaultSpec(), shape: 'multi', example: 'decays', coupling: 'shared', model: ref('exp0'), shared: ['b'] });
    near(e.blocks[0].params.find((p) => p.name === 'b (shared)')!.value, 0.8, 0.03, 'shared rate');
    const o = fit({ ...defaultSpec(), shape: 'multi', example: 'logistic_ode', coupling: 'ode', odeRhs: 'r*y*(1-y/K)', odePs: { r: { guess: '0.5' }, K: { guess: '8' } } });
    near(value(o.blocks[0], 'r'), 0.9, 0.08, 'r');
    near(value(o.blocks[0], 'K'), 10, 0.3, 'K');
  });

  it('a spec that cannot work is refused: shared with one signal, no shared parameter', () => {
    const cols = decays(35);
    expect(buildCurveFit(specFor({ t: cols.t, a: cols.a }, { shape: 'multi', coupling: 'shared', model: ref('exp0'), shared: ['b'] }), 'run').ok).toBe(false);
    expect(buildCurveFit(specFor(cols, { shape: 'multi', coupling: 'shared', model: ref('exp0'), shared: [] }), 'run').ok).toBe(false);
    expect(validateSpec(specFor(cols, { shape: 'multi', cols: { x: '', y: '', z: '', err: '', t: 't', signals: [] } }))).toMatch(/signal/i);
  });
});

// ------------------------------------------------------------ solve equations

function solve(patch: Partial<SolveSpec>) {
  const spec = { ...defaultSolveSpec(), ...patch };
  const r = buildSolve(spec);
  if (!r.ok) throw new Error(r.error);
  const ran = runScript(r.plan.script);
  expect(ran.err, ran.err + '\n' + r.plan.script).toBe('');
  expect(ran.code).toBe(0);
  return { plan: r.plan, out: readSolve(r.plan, ran.series), ran };
}
const sortedRe = (o: ReturnType<typeof readSolve>) => o.roots.filter((r) => Math.abs(r.im) < 1e-9).map((r) => r.re).sort((a, b) => a - b);

d('Solve equations engine gate', () => {
  it('roots of 2*x^2+3*x are exactly 0 and -1.5 (polynomial route)', () => {
    const { plan, out } = solve({ expr: '2*x^2+3*x' });
    expect(plan.kind).toBe('polynomial');
    const r = sortedRe(out);
    expect(r.length).toBe(2);
    near(r[0], -1.5, 1e-12, 'root');
    near(r[1], 0, 1e-12, 'root');
    // the minimum of 2x^2+3x is at x = -0.75, value -1.125
    expect(out.stationary.length).toBe(1);
    near(out.stationary[0].x, -0.75, 1e-12, 'minimum x');
    near(out.stationary[0].y, -1.125, 1e-12, 'minimum y');
    expect(out.stationary[0].kind).toBe('minimum');
  });

  it('roots of x^3-6x^2+11x-6 are 1, 2, 3', () => {
    const { out } = solve({ expr: 'x^3-6*x^2+11*x-6', lo: '-1', hi: '5' });
    const r = sortedRe(out);
    [1, 2, 3].forEach((v, i) => near(r[i], v, 1e-9, `root ${v}`));
  });

  it('complex roots are listed with their imaginary parts', () => {
    const { out } = solve({ expr: 'x^2+1' });
    expect(out.roots.length).toBe(2);
    expect(out.roots.map((r) => Math.abs(r.im)).sort()[0]).toBeCloseTo(1, 9);
  });

  it('a transcendental case: cos(x) - x has its root at 0.7390851332', () => {
    const { plan, out } = solve({ expr: 'cos(x) - x', lo: '-5', hi: '5' });
    expect(plan.kind).toBe('scan');
    const r = sortedRe(out);
    expect(r.length).toBe(1);
    near(r[0], 0.7390851332, 1e-9, 'root');
  });

  it('f(x) = g(x): x^2 and 2*x+3 meet at -1 and 3, by the polynomial route and by the scan', () => {
    for (const forceNumeric of [false, true]) {
      const { plan, out } = solve({ expr: 'x^2', rhs: '2*x+3', lo: '-5', hi: '5', forceNumeric });
      expect(plan.kind).toBe(forceNumeric ? 'scan' : 'polynomial');
      const r = sortedRe(out);
      expect(r.length).toBe(2);
      near(r[0], -1, 1e-9, 'x1');
      near(r[1], 3, 1e-9, 'x2');
      near(out.roots[0].y, out.roots[0].re ** 2, 1e-9, 'f at the intersection');
    }
  });

  it('f(x) = c with the equals typed in the expression', () => {
    const { out } = solve({ expr: 'x^2 = 9', rhs: '', lo: '-5', hi: '5' });
    const r = sortedRe(out);
    near(r[0], -3, 1e-9, '-3');
    near(r[1], 3, 1e-9, '3');
  });

  it('extrema and inflections of x^3 - 3x: maximum at -1, minimum at 1, inflection at 0 (polynomial and numeric)', () => {
    for (const forceNumeric of [false, true]) {
      const { out } = solve({ task: 'extrema', expr: 'x^3 - 3*x', lo: '-3', hi: '3', forceNumeric });
      const st = [...out.stationary].sort((a, b) => a.x - b.x);
      expect(st.length).toBe(2);
      near(st[0].x, -1, 1e-6, 'max x');
      expect(st[0].kind).toBe('maximum');
      near(st[0].y, 2, 1e-6, 'max y');
      near(st[1].x, 1, 1e-6, 'min x');
      expect(st[1].kind).toBe('minimum');
      expect(out.inflections.length).toBe(1);
      near(out.inflections[0].x, 0, 1e-4, 'inflection');
    }
  });

  it('the system x^2+y^2=4, x=y converges to a root of both equations', () => {
    const { out, plan } = solve({ task: 'system', eqs: ['x^2 + y^2 = 4', 'x = y', '', ''] });
    expect(plan.kind).toBe('nonlinear');
    expect(out.converged).toBe(true);
    near(Math.abs(out.solution![0]), Math.SQRT2, 1e-7, '|x|');
    near(out.solution![0], out.solution![1], 1e-7, 'x = y');
    expect(out.residual!).toBeLessThan(1e-8);
  });

  it('multistart finds both solutions (+-sqrt 2, +-sqrt 2)', () => {
    const { out, plan } = solve({ task: 'system', eqs: ['x^2 + y^2 = 4', 'x = y', '', ''], multistart: true });
    expect(plan.kind).toBe('multistart');
    expect(out.solutions!.length).toBe(2);
    const xs = out.solutions!.map((s) => s[0]).sort((a, b) => a - b);
    near(xs[0], -Math.SQRT2, 1e-6, '-sqrt2');
    near(xs[1], Math.SQRT2, 1e-6, 'sqrt2');
    for (const s of out.solutions!) near(s[0], s[1], 1e-6, 'x = y');
  });

  it('a linear system typed as equations is detected and solved exactly: 2x+3y=5, x-y=0 gives x = y = 1', () => {
    const { out, plan } = solve({ task: 'system', eqs: ['2*x + 3*y = 5', 'x - y = 0', '', ''] });
    expect(plan.kind).toBe('linear');
    expect(plan.matrix).toEqual([[2, 3], [1, -1]]);
    expect(plan.rhsVec).toEqual([5, 0]);
    near(out.solution![0], 1, 1e-12, 'x');
    near(out.solution![1], 1, 1e-12, 'y');
  });

  it('a 3x3 linear system with a known solution (x, y, z) = (1, -2, 3)', () => {
    const { out } = solve({ task: 'system', eqs: ['x + y + z = 2', '2*x - y + z = 7', 'x + 2*y - z = -6', ''] });
    [1, -2, 3].forEach((v, i) => near(out.solution![i], v, 1e-10, `u${i}`));
  });

  it('a singular linear system warns instead of pretending', () => {
    const { ran } = solve({ task: 'system', eqs: ['x + y = 2', '2*x + 2*y = 4', '', ''] });
    expect(ran.out).toContain('CFWARN');
  });

  it('inputs that are not plain math are refused before anything runs, and never reach a script', () => {
    const attacks = [
      'x; print(1)', 'x\nprint(1)', 'x end', 'x`ls`', 'foo(x)', 'system("rm -rf /")', 'x + system(1)', 'exp(x) function', '"abc"', 'x = 1; y = 2',
      'input(x)', 'x[0]', 'x{1}', 'if x then 1', 'x # comment', 'x ** 2', 'x && y', 'f(x) := 3',
    ];
    for (const a of attacks) {
      for (const task of ['equation', 'extrema', 'system'] as const) {
        const r = buildSolve({ ...defaultSolveSpec(), task, expr: a, eqs: [a, '', '', ''] });
        expect(r.ok, `${task}: ${JSON.stringify(a)}`).toBe(false);
        if (!r.ok) expect(r.error.length, a).toBeGreaterThan(5);
      }
    }
    // a rejected string is not echoed into any script: there is no script at all
    expect(buildSolve({ ...defaultSolveSpec(), expr: 'x; print(1)' })).toEqual({ ok: false, error: expect.stringMatching(/semicolon/) });
  });

  it('the same rejections hold for the Fit panel path (custom model, ODE, starting value)', () => {
    const x = [1, 2, 3, 4, 5, 6];
    const y = [2, 4, 6, 8, 10, 12];
    const evil = ['a*x; print(1)', 'a*x\nprint(1)', 'a*x end', 'a*x`ls`', 'system(a)*x', 'a*foo(x)'];
    for (const e of evil) {
      const r = buildCurveFit(specFor({ x, y }, { model: ref('custom', { expr: e }) }), 'run');
      expect(r.ok, e).toBe(false);
    }
    const r2 = buildCurveFit(specFor({ t: x, y }, { shape: 'multi', coupling: 'ode', odeRhs: '-k*y; print(1)' }), 'run');
    expect(r2.ok).toBe(false);
    const r3 = buildCurveFit(specFor({ x, y }, { model: ref('poly', { degree: 1, ps: { c1: { guess: 'system(1)' } } }) }), 'run');
    expect(r3.ok).toBe(false);
    // an accepted expression is re-emitted from its parse, not copied: odd spacing and a leading "y =" disappear
    const ok = buildCurveFit(specFor({ x, y }, { model: ref('custom', { expr: ' y =  a  * x  +b ' }) }), 'run');
    expect(ok.ok).toBe(true);
    if (ok.ok) expect(ok.plan.script).toContain('return a * x + b');
  });
});

describe('table parsing used by the Fit tab', () => {
  it('detects headers, delimiters and reports a bad cell', () => {
    const t = parseTable('x;y\n1,5;2,5\n2,5;3,5\nabc;4');
    expect(t.header).toBe(true);
    expect(t.delimiter).toBe('semicolon');
    expect(t.problems.join(' ')).toMatch(/not a number/);
  });
});
