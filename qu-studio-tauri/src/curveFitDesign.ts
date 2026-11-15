/** Pure helpers behind the Fit tab: the model catalogue, validation, the Qu
 *  script it generates and the reading of the vectors the script leaves
 *  behind. Every fit is `least_squares` (Levenberg-Marquardt, with its own
 *  standard errors / confidence intervals, box bounds and a `converged` flag)
 *  on a residual function the script writes out in full. No DOM, no Tauri. */

import { emitExpr, isName, parseExpr, stripLhs, type ParseResult } from './mathExpr';
import { needsStaging } from './dataTable';
import { csvForScript, dataLines, exampleById, sanitizeName, type Need, type Shape, type Table } from './curveFitData';

export type { Shape } from './curveFitData';

export interface ParamSetting {
  /** Starting value as a plain expression (may use the data-summary names d_x0 ... shown in the panel). */
  guess?: string;
  fixed?: boolean;
  lo?: string;
  hi?: string;
}

export interface ModelRef {
  id: string;
  degree: number;
  /** Custom model: an expression in the independent variable(s) and named parameters. */
  expr: string;
  ps: Record<string, ParamSetting>;
}

export type Criterion = 'aic' | 'aicc' | 'bic';

export interface FitSpec {
  shape: Shape;
  source: 'paste' | 'file' | 'variable' | 'example';
  table: Table | null;
  filePath?: string;
  example: string;
  cols: { x: string; y: string; z: string; err: string; t: string; signals: string[] };
  weight: 'none' | 'sigma' | 'weight';
  mode: 'single' | 'compare';
  model: ModelRef;
  compare: ModelRef[];
  criterion: Criterion;
  coupling: 'separate' | 'shared' | 'ode';
  shared: string[];
  odeRhs: string;
  odePs: Record<string, ParamSetting>;
  fitY0: boolean;
  robust: 'none' | 'huber' | 'bisquare';
  starts: number;
  band: boolean;
  extrap: number;
  logx: boolean;
  logy: boolean;
}

// ----------------------------------------------------------------- catalogue

interface ModelDef {
  id: string;
  label: string;
  /** What the picker shows under the name. */
  formula: string;
  shapes: Shape[];
  degree?: { min: number; max: number };
  params: (deg: number) => string[];
  /** Expression in `x` (and `y` for surfaces); a function of the degree for the polynomial families. */
  expr: (deg: number) => string;
  /** Raw Qu body override (the polynomials use polyval). `{x}` is replaced by the independent variable. */
  body?: (deg: number) => string;
  /** Trusted Qu starting guesses (`d_` names are the data summary). */
  guess: (deg: number) => string[];
  pre?: (deg: number) => string[];
  note?: string;
}

const polyNames = (d: number) => Array.from({ length: d + 1 }, (_, i) => `c${d - i}`);
const surfaceTerms = (d: number): Array<[number, number]> => {
  const t: Array<[number, number]> = [];
  for (let s = 0; s <= d; s++) for (let i = s; i >= 0; i--) t.push([i, s - i]);
  return t;
};
const surfaceNames = (d: number) => surfaceTerms(d).map(([i, j]) => `c${i}${j}`);

const DIR_EXP = [
  'd_eb = 3 / d_xr',
  'if abs(d_ym - d_y1) > abs(d_ym - d_y0) then',
  '  d_eb = -d_eb',
  'end if',
];
const DIR_SIG = ['d_lk = 4 / d_xr', 'if d_y1 < d_y0 then', '  d_lk = -d_lk', 'end if'];
const WIDTH = [
  'd_hm = find(abs({y} - d_ym) > abs(d_ypk - d_ym) / 2)',
  'd_gs = (max({x}[d_hm]) - min({x}[d_hm])) / 2.355',
  'd_gs = max(d_gs, d_xr / 100)',
];

export const MODELS: ModelDef[] = [
  {
    id: 'poly', label: 'Polynomial (degree n)', formula: 'c_n x^n + ... + c_1 x + c_0', shapes: ['yx', 'multi'], degree: { min: 1, max: 9 },
    params: polyNames,
    expr: (d) => polyNames(d).map((c, i) => (d - i === 0 ? c : d - i === 1 ? `${c}*x` : `${c}*x^${d - i}`)).join(' + '),
    body: (d) => `polyval([${polyNames(d).join(', ')}], {x})`,
    guess: (d) => polyNames(d).map((_, i) => `d_pc[${i}]`),
    pre: (d) => [`d_pc = polyfit({x}, {y}, ${d})`],
    note: 'Linear in its parameters: the starting guess is polyfit and the fit refines it with weights and errors.',
  },
  {
    id: 'origin', label: 'Line through the origin', formula: 'a x', shapes: ['yx', 'multi'],
    params: () => ['a'], expr: () => 'a*x', guess: () => ['d_a0'], pre: () => ['d_a0 = sum({x} .* {y}) / sum({x} .* {x})'],
  },
  {
    id: 'exp', label: 'Exponential decay / growth', formula: 'a exp(-b x) + c', shapes: ['yx', 'multi'],
    params: () => ['a', 'b', 'c'], expr: () => 'a*exp(-b*x) + c', guess: () => ['d_ea', 'd_eb', 'd_ec'],
    pre: () => [...DIR_EXP, 'd_ea = (d_y1 - d_y0) / (exp(-d_eb * d_x1) - exp(-d_eb * d_x0))', 'd_ec = d_y0 - d_ea * exp(-d_eb * d_x0)'],
    note: 'b > 0 decays toward c, b < 0 grows. The start passes through the first and last points.',
  },
  {
    id: 'exp0', label: 'Exponential without offset', formula: 'a exp(-b x)', shapes: ['yx', 'multi'],
    params: () => ['a', 'b'], expr: () => 'a*exp(-b*x)', guess: () => ['d_ea', 'd_eb'],
    pre: () => [...DIR_EXP, 'd_ea = d_y0 * exp(d_eb * d_x0)'],
  },
  {
    id: 'power', label: 'Power law', formula: 'a x^b', shapes: ['yx', 'multi'],
    params: () => ['a', 'b'], expr: () => 'a*x^b', guess: () => ['exp(d_pf[1])', 'd_pf[0]'], pre: () => ['d_pf = polyfit(log({x}), log({y}), 1)'],
    note: 'Needs x > 0 and y > 0 for the log-log starting guess.',
  },
  {
    id: 'log', label: 'Logarithmic', formula: 'a ln(x) + b', shapes: ['yx', 'multi'],
    params: () => ['a', 'b'], expr: () => 'a*log(x) + b', guess: () => ['d_lf[0]', 'd_lf[1]'], pre: () => ['d_lf = polyfit(log({x}), {y}, 1)'],
    note: 'Needs x > 0.',
  },
  {
    id: 'logistic', label: 'Logistic (sigmoid, from 0)', formula: 'L / (1 + exp(-k (x - x0)))', shapes: ['yx', 'multi'],
    params: () => ['L', 'k', 'x0'], expr: () => 'L/(1 + exp(-k*(x - x0)))', guess: () => ['d_yhi', 'd_lk', 'd_xh'], pre: () => DIR_SIG,
  },
  {
    id: 'sigmoid4', label: 'Sigmoid with baseline', formula: 'c + a / (1 + exp(-k (x - x0)))', shapes: ['yx', 'multi'],
    params: () => ['a', 'k', 'x0', 'c'], expr: () => 'c + a/(1 + exp(-k*(x - x0)))', guess: () => ['d_yhi - d_ylo', 'd_lk', 'd_xh', 'd_ylo'], pre: () => DIR_SIG,
  },
  {
    id: 'gauss', label: 'Gaussian peak', formula: 'a exp(-(x - mu)^2 / (2 s^2))', shapes: ['yx', 'multi'],
    params: () => ['a', 'mu', 's'], expr: () => 'a*exp(-(x - mu)^2/(2*s^2))', guess: () => ['d_ypk', 'd_xpk', 'd_gs'], pre: () => WIDTH,
  },
  {
    id: 'gaussb', label: 'Gaussian peak with baseline', formula: 'a exp(-(x - mu)^2 / (2 s^2)) + d', shapes: ['yx', 'multi'],
    params: () => ['a', 'mu', 's', 'd'], expr: () => 'a*exp(-(x - mu)^2/(2*s^2)) + d', guess: () => ['d_ypk - d_ym', 'd_xpk', 'd_gs', 'd_ym'], pre: () => WIDTH,
  },
  {
    id: 'lorentz', label: 'Lorentzian peak', formula: 'a / (1 + ((x - x0) / g)^2)', shapes: ['yx', 'multi'],
    params: () => ['a', 'x0', 'g'], expr: () => 'a/(1 + ((x - x0)/g)^2)', guess: () => ['d_ypk', 'd_xpk', 'd_gs'], pre: () => WIDTH,
  },
  {
    id: 'exp2', label: 'Sum of two exponentials', formula: 'a exp(-b x) + c exp(-d x)', shapes: ['yx', 'multi'],
    params: () => ['a', 'b', 'c', 'd'], expr: () => 'a*exp(-b*x) + c*exp(-d*x)', guess: () => ['d_y0 / 2', '1 / d_xr', 'd_y0 / 2', '10 / d_xr'],
    note: 'Hard to fit: the two rates can trade places. Use "multi-start" if the first try does not converge.',
  },
  {
    id: 'sine', label: 'Sine wave', formula: 'a sin(w x + p) + c', shapes: ['yx', 'multi'],
    params: () => ['a', 'w', 'p', 'c'], expr: () => 'a*sin(w*x + p) + c', guess: () => ['std({y}) * sqrt(2)', 'd_sw', 'd_sph', 'mean({y})'],
    pre: () => [
      'd_sp = rfft({y} - mean({y}))', 'd_sa = abs(d_sp)', 'd_sk = argmax(d_sa[1:length(d_sa) - 1]) + 1',
      'd_sw = 2 * pi * d_sk / (length({x}) * d_xr / (length({x}) - 1))', 'd_sph = angle(d_sp[d_sk]) + pi / 2 - d_sw * d_x0',
    ],
    note: 'The starting frequency is the FFT peak (assumes evenly spaced x).',
  },
  {
    id: 'rational', label: 'Rational', formula: '(a x + b) / (x + c)', shapes: ['yx', 'multi'],
    params: () => ['a', 'b', 'c'], expr: () => '(a*x + b)/(x + c)', guess: () => ['d_y1', 'd_y0 * (d_x0 + d_rc) - d_y1 * d_x0', 'd_rc'], pre: () => ['d_rc = 0.1 * d_xr - d_x0'],
  },
  {
    id: 'mm', label: 'Michaelis-Menten', formula: 'Vmax x / (Km + x)', shapes: ['yx', 'multi'],
    params: () => ['Vmax', 'Km'], expr: () => 'Vmax*x/(Km + x)', guess: () => ['d_yhi * 1.2', 'd_xh'],
  },
  {
    id: 'arrhenius', label: 'Arrhenius (x = temperature in K)', formula: 'A exp(-B / x)', shapes: ['yx', 'multi'],
    params: () => ['A', 'B'], expr: () => 'A*exp(-B/x)', guess: () => ['exp(d_af[1])', '-d_af[0]'], pre: () => ['d_af = polyfit(1 ./ {x}, log({y}), 1)'],
    note: 'B = Ea / R. Needs x > 0 and y > 0.',
  },
  {
    id: 'hill', label: 'Hill / dose-response', formula: 'a x^n / (k^n + x^n)', shapes: ['yx', 'multi'],
    params: () => ['a', 'n', 'k'], expr: () => 'a*x^n/(k^n + x^n)', guess: () => ['d_yhi * 1.1', '1', 'd_xh'],
  },
  {
    id: 'plane', label: 'Plane', formula: 'a x + b y + c', shapes: ['surface'],
    params: () => ['a', 'b', 'c'], expr: () => 'a*x + b*y + c', guess: () => ['0', '0', 'd_zm'],
  },
  {
    id: 'quad', label: 'Polynomial surface (degree n)', formula: 'sum c_ij x^i y^j, i + j <= n', shapes: ['surface'], degree: { min: 1, max: 6 },
    params: surfaceNames,
    expr: (d) => surfaceTerms(d).map(([i, j]) => [`c${i}${j}`, i ? (i === 1 ? 'x' : `x^${i}`) : '', j ? (j === 1 ? 'y' : `y^${j}`) : ''].filter(Boolean).join('*')).join(' + '),
    guess: (d) => surfaceNames(d).map((n) => (n === 'c00' ? 'd_zm' : '0')),
    note: 'Linear in its parameters.',
  },
  {
    id: 'gauss2d', label: '2-D Gaussian (with offset)', formula: 'a exp(-((x-x0)^2/(2 sx^2) + (y-y0)^2/(2 sy^2))) + d', shapes: ['surface'],
    params: () => ['a', 'x0', 'y0', 'sx', 'sy', 'd'], expr: () => 'a*exp(-((x - x0)^2/(2*sx^2) + (y - y0)^2/(2*sy^2))) + d',
    guess: () => ['d_zpk - d_zm', 'd_xpk', 'd_ypk', 'd_xr / 4', 'd_yr / 4', 'd_zm'],
  },
];

export const CUSTOM_ID = 'custom';
export const ODE_ID = 'ode';

export const modelsFor = (shape: Shape): ModelDef[] => MODELS.filter((m) => m.shapes.includes(shape));
export const modelDef = (id: string): ModelDef | undefined => MODELS.find((m) => m.id === id);

export const emptyRef = (id: string, degree = 2, expr = 'a*exp(-b*x) + c'): ModelRef => ({ id, degree, expr, ps: {} });

// -------------------------------------------------------------- data summary

/** Names the starting guesses may use; each line is emitted only when something refers to it. */
const SUMMARY_YX: Array<[string, string]> = [
  ['x0', 'd_x0 = min({x})'], ['x1', 'd_x1 = max({x})'], ['xr', 'd_xr = d_x1 - d_x0'],
  ['y0', 'd_y0 = {y}[argmin({x})]'], ['y1', 'd_y1 = {y}[argmax({x})]'],
  ['ylo', 'd_ylo = min({y})'], ['yhi', 'd_yhi = max({y})'], ['ym', 'd_ym = median({y})'],
  ['ipk', 'd_ipk = argmax(abs({y} - d_ym))'], ['xpk', 'd_xpk = {x}[d_ipk]'], ['ypk', 'd_ypk = {y}[d_ipk]'],
  ['xh', 'd_xh = {x}[argmin(abs({y} - (d_ylo + d_yhi) / 2))]'],
];
const SUMMARY_SURFACE: Array<[string, string]> = [
  ['xlo', 'd_xlo = min({x})'], ['xhi', 'd_xhi = max({x})'], ['xr', 'd_xr = d_xhi - d_xlo'],
  ['ylo', 'd_ylo = min({y})'], ['yhi', 'd_yhi = max({y})'], ['yr', 'd_yr = d_yhi - d_ylo'],
  ['zm', 'd_zm = median({z})'], ['ipk', 'd_ipk = argmax(abs({z} - d_zm))'],
  ['xpk', 'd_xpk = {x}[d_ipk]'], ['ypk', 'd_ypk = {y}[d_ipk]'], ['zpk', 'd_zpk = {z}[d_ipk]'],
];
export const SUMMARY_NAMES = {
  yx: SUMMARY_YX.map(([n]) => `d_${n}`),
  surface: SUMMARY_SURFACE.map(([n]) => `d_${n}`),
};

function summaryLines(kind: 'yx' | 'surface', used: string, force: string[]): string[] {
  const table = kind === 'yx' ? SUMMARY_YX : SUMMARY_SURFACE;
  const need = new Set<string>(force);
  for (const m of used.matchAll(/\bd_([A-Za-z0-9]+)/g)) need.add(m[1]);
  // pull in what the chosen lines themselves use, until nothing new appears
  for (let grew = true; grew; ) {
    grew = false;
    for (const [n, line] of table) {
      if (!need.has(n)) continue;
      for (const m of line.matchAll(/\bd_([A-Za-z0-9]+)/g)) if (!need.has(m[1]) && m[1] !== n) (need.add(m[1]), (grew = true));
    }
  }
  return table.filter(([n]) => need.has(n)).map(([, l]) => l);
}

// ------------------------------------------------------------ model resolving

export interface Resolved {
  title: string;
  formula: string;
  params: string[];
  /** Qu `return` expression of the model function. */
  body: string;
  pre: string[];
  /** Per-parameter start (raw Qu, `d_` prefix style) and settings. */
  guess: string[];
  settings: ParamSetting[];
  ivars: string[];
  /** ODE only: how many of `params` go to the right-hand side (the rest is y0). */
  odeCount?: number;
}

type Ok<T> = { ok: true; value: T };
type Err = { ok: false; error: string };

const fail = (error: string): Err => ({ ok: false, error });

/** Checks a user-typed starting value / bound and returns its Qu text. */
function userExpr(text: string, what: string, names: string[]): Ok<string> | Err {
  const r = parseExpr(stripLhs(text), { vars: [], allowParams: false, extraNames: names });
  if (!r.ok) return fail(`${what}: ${r.error}`);
  return { ok: true, value: emitExpr(r.ast) };
}

export function resolveModel(ref: ModelRef, shape: Shape, ivar: string): Ok<Resolved> | Err {
  const surface = shape === 'surface';
  const ivars = surface ? ['x', 'y'] : [ivar];
  const names = surface ? SUMMARY_NAMES.surface : SUMMARY_NAMES.yx;
  let r: Resolved;
  if (ref.id === CUSTOM_ID) {
    const parsed = parseExpr(stripLhs(ref.expr), { vars: ivars, allowParams: true, forbiddenParams: surface ? [] : ['x', 'y'] });
    if (!parsed.ok) return fail(`Custom model: ${parsed.error}`);
    if (parsed.params.length === 0) return fail('Custom model: it needs at least one named parameter (for example a*exp(-b*x)+c).');
    if (parsed.vars.length === 0) return fail(`Custom model: it does not use ${ivars.join(' or ')}.`);
    r = {
      title: 'custom model', formula: emitExpr(parsed.ast), params: parsed.params, body: emitExpr(parsed.ast), pre: [],
      guess: parsed.params.map(() => '1'), settings: [], ivars,
    };
  } else {
    const d = modelDef(ref.id);
    if (!d || !d.shapes.includes(shape)) return fail(`Unknown model '${ref.id}'.`);
    let deg = ref.degree;
    if (d.degree) {
      if (!Number.isInteger(deg) || deg < d.degree.min || deg > d.degree.max) return fail(`${d.label}: the degree must be a whole number from ${d.degree.min} to ${d.degree.max}.`);
    } else deg = 0;
    const parsed = parseExpr(d.expr(deg), { vars: ['x', 'y'], allowParams: true });
    const rename: Record<string, string> = ivars.length === 1 ? { x: ivars[0] } : {};
    r = {
      title: d.label.toLowerCase(), formula: parsed.ok ? emitExpr(parsed.ast, rename) : d.expr(deg),
      params: d.params(deg), body: d.body ? d.body(deg) : parsed.ok ? emitExpr(parsed.ast, rename) : d.expr(deg),
      pre: d.pre ? d.pre(deg) : [], guess: d.guess(deg), settings: [], ivars,
    };
    if (d.degree) r.title = `${d.label.replace('(degree n)', `(degree ${deg})`).toLowerCase()}`;
  }
  const settings: ParamSetting[] = [];
  const guess = [...r.guess];
  for (let i = 0; i < r.params.length; i++) {
    const s = ref.ps[r.params[i]] ?? {};
    settings.push(s);
    if (s.guess != null && s.guess.trim() !== '') {
      const g = userExpr(s.guess, `Start value of ${r.params[i]}`, names);
      if (!g.ok) return g;
      guess[i] = g.value;
    }
    for (const [k, label] of [['lo', 'lower bound'], ['hi', 'upper bound']] as const) {
      const t = s[k];
      if (t != null && t.trim() !== '' && !Number.isFinite(Number(t))) return fail(`The ${label} of ${r.params[i]} must be a number (leave it empty for none).`);
    }
    const lo = s.lo?.trim() ? Number(s.lo) : -Infinity;
    const hi = s.hi?.trim() ? Number(s.hi) : Infinity;
    if (lo >= hi) return fail(`The bounds of ${r.params[i]} are the wrong way round (lower ${lo} is not below upper ${hi}).`);
  }
  if (r.params.every((p, i) => settings[i].fixed)) return fail('Every parameter is fixed: free at least one to fit.');
  return { ok: true, value: { ...r, guess, settings } };
}

export function resolveOde(spec: FitSpec): Ok<Resolved> | Err {
  const parsed: ParseResult = parseExpr(stripLhs(spec.odeRhs), { vars: ['t', 'y'], allowParams: true, forbiddenParams: ['y0'] });
  if (!parsed.ok) return fail(`ODE right-hand side: ${parsed.error}`);
  if (parsed.params.length === 0) return fail('ODE right-hand side: it needs at least one named parameter (for example -k*y or r*y*(1 - y/K)).');
  if (!parsed.vars.includes('y')) return fail("ODE right-hand side: it does not use y, the state.");
  const params = [...parsed.params];
  const guess = parsed.params.map(() => '1');
  if (spec.fitY0) {
    params.push('y0');
    guess.push('d_y0');
  }
  const settings: ParamSetting[] = [];
  for (let i = 0; i < params.length; i++) {
    const s = spec.odePs[params[i]] ?? {};
    settings.push(s);
    if (s.guess != null && s.guess.trim() !== '') {
      const g = userExpr(s.guess, `Start value of ${params[i]}`, SUMMARY_NAMES.yx);
      if (!g.ok) return g;
      guess[i] = g.value;
    }
    for (const [k, label] of [['lo', 'lower bound'], ['hi', 'upper bound']] as const) {
      const t = s[k];
      if (t != null && t.trim() !== '' && !Number.isFinite(Number(t))) return fail(`The ${label} of ${params[i]} must be a number.`);
    }
    if ((s.lo?.trim() ? Number(s.lo) : -Infinity) >= (s.hi?.trim() ? Number(s.hi) : Infinity)) return fail(`The bounds of ${params[i]} are the wrong way round.`);
  }
  if (settings.every((s) => s.fixed)) return fail('Every parameter is fixed: free at least one to fit.');
  return {
    ok: true,
    value: {
      title: `ODE fit, y' = ${emitExpr(parsed.ast)}`, formula: `y' = ${emitExpr(parsed.ast)}`, params, body: emitExpr(parsed.ast), pre: [],
      guess, settings, ivars: ['t'], odeCount: parsed.params.length,
    },
  };
}

// ---------------------------------------------------------------- statistics

/** Order of the `<pfx>_stat` vector the script leaves behind. */
export const STAT_FIELDS = [
  'n', 'k', 'dof', 'ssr', 'r2', 'r2adj', 'rmse', 'sigma', 'chi2red', 'aic', 'aicc', 'bic',
  'resMean', 'resStd', 'skew', 'exkurt', 'jbP', 'lag1', 'converged', 'iterations',
] as const;
export type StatName = (typeof STAT_FIELDS)[number];
export type Stats = Record<StatName, number>;

export function readStats(v: (number | null)[] | undefined): Stats | null {
  if (!v || v.length < STAT_FIELDS.length) return null;
  const out = {} as Stats;
  STAT_FIELDS.forEach((f, i) => (out[f] = v[i] == null ? NaN : (v[i] as number)));
  return Number.isFinite(out.n) && Number.isFinite(out.ssr) ? out : null;
}

const NSTAT = STAT_FIELDS.length;

// ------------------------------------------------------------- script blocks

interface BlockOpts {
  pfx: string;
  kind: 'curve' | 'surface' | 'ode';
  xs: string[];
  ys: string;
  r: Resolved;
  wt: boolean;
  sigma: boolean;
  robust: FitSpec['robust'];
  starts: number;
  band: boolean;
  ext: number;
  logx: boolean;
  guard: boolean;
}

const isFree = (r: Resolved, i: number) => !r.settings[i]?.fixed;
const num = (v: number) => (v === Infinity ? 'inf' : v === -Infinity ? '-inf' : String(v));

/** Free-parameter bookkeeping shared by a block and the panel. */
export function freeInfo(r: Resolved): { names: string[]; lo: number[]; hi: number[] } {
  const names: string[] = [];
  const lo: number[] = [];
  const hi: number[] = [];
  r.params.forEach((p, i) => {
    if (!isFree(r, i)) return;
    names.push(p);
    lo.push(r.settings[i]?.lo?.trim() ? Number(r.settings[i].lo) : -Infinity);
    hi.push(r.settings[i]?.hi?.trim() ? Number(r.settings[i].hi) : Infinity);
  });
  return { names, lo, hi };
}

function subst(s: string, o: { pfx: string; xs: string[]; ys: string; kind: string }): string {
  const surface = o.kind === 'surface';
  return s
    .replace(/\bd_/g, `${o.pfx}_`)
    .replace(/\{x\}/g, o.xs[0])
    .replace(/\{y\}/g, surface ? o.xs[1] : o.ys)
    .replace(/\{z\}/g, o.ys);
}

function statLines(P: string, y: string, pred: string, k: string, o: { wt: boolean; sigma: boolean; order: string | null }): string[] {
  const w = o.wt ? ' .* wt' : '';
  const L: string[] = [];
  L.push(`${P}_n = length(${y})`, `${P}_k = ${k}`, `${P}_res = ${y} - ${pred}`, `${P}_dof = ${P}_n - ${P}_k`);
  L.push(o.wt ? `${P}_ssr = sum((${P}_res${w}) .^ 2)` : `${P}_ssr = sum(${P}_res .^ 2)`);
  if (o.wt) {
    L.push(`${P}_ybar = sum(wt .^ 2 .* ${y}) / sum(wt .^ 2)`, `${P}_sst = sum((wt .* (${y} - ${P}_ybar)) .^ 2)`);
  } else L.push(`${P}_sst = sum((${y} - mean(${y})) .^ 2)`);
  L.push(
    `${P}_r2 = 1 - ${P}_ssr / ${P}_sst`,
    `${P}_r2adj = 1 - (1 - ${P}_r2) * (${P}_n - 1) / ${P}_dof`,
    `${P}_rmse = sqrt(sum(${P}_res .^ 2) / ${P}_n)`,
    `${P}_sigma = sqrt(${P}_ssr / ${P}_dof)`,
    `${P}_chi2 = ${P}_ssr / ${P}_dof`,
  );
  L.push(o.sigma ? `${P}_aic = ${P}_ssr + 2 * ${P}_k` : `${P}_aic = ${P}_n * log(max(${P}_ssr, 1e-300) / ${P}_n) + 2 * ${P}_k`);
  L.push(`${P}_aicc = ${P}_aic + 2 * ${P}_k * (${P}_k + 1) / (${P}_n - ${P}_k - 1)`);
  L.push(o.sigma ? `${P}_bic = ${P}_ssr + ${P}_k * log(${P}_n)` : `${P}_bic = ${P}_n * log(max(${P}_ssr, 1e-300) / ${P}_n) + ${P}_k * log(${P}_n)`);
  L.push(
    `${P}_rc = ${P}_res - mean(${P}_res)`, `${P}_m2 = mean(${P}_rc .^ 2)`,
    `${P}_skew = mean(${P}_rc .^ 3) / ${P}_m2 ^ 1.5`, `${P}_kurt = mean(${P}_rc .^ 4) / ${P}_m2 ^ 2 - 3`,
    `${P}_jb = ${P}_n / 6 * (${P}_skew ^ 2 + ${P}_kurt ^ 2 / 4)`, `${P}_jbp = exp(-${P}_jb / 2)`,
  );
  if (o.order) {
    L.push(`${P}_rs = ${P}_res[argsort(${o.order})]`, `${P}_lag1 = sum(${P}_rs[0:${P}_n - 2] .* ${P}_rs[1:${P}_n - 1]) / sum(${P}_rs .^ 2)`);
  } else L.push(`${P}_lag1 = nan`);
  return L;
}

const statVector = (P: string, converged: string, iters: string) =>
  `${P}_stat = [${P}_n, ${P}_k, ${P}_dof, ${P}_ssr, ${P}_r2, ${P}_r2adj, ${P}_rmse, ${P}_sigma, ${P}_chi2, ${P}_aic, ${P}_aicc, ${P}_bic, mean(${P}_res), std(${P}_res), ${P}_skew, ${P}_kurt, ${P}_jbp, ${P}_lag1, ${converged}, ${iters}]`;

const STARTS = [0.5, 2, 0.25, 4, 0.1, 10, 0.05, 20];

function fitLines(P: string, o: BlockOpts, free: ReturnType<typeof freeInfo>): string[] {
  const L: string[] = [];
  const hasBounds = free.lo.some((v) => v > -Infinity) || free.hi.some((v) => v < Infinity);
  const bounds = hasBounds ? `, lower=[${free.lo.map(num).join(', ')}], upper=[${free.hi.map(num).join(', ')}]` : '';
  const call = (start: string) => `least_squares("${P}_resid", ${start}${bounds}, max_iter=500)`;
  L.push(`${P}_fit = ${call(`${P}_q0`)}`);
  if (o.starts > 1) {
    L.push(`# multi-start: also try the guess scaled by fixed factors and keep the lowest cost`, `${P}_scales = [${STARTS.join(', ')}]`);
    L.push(`for ${P}_ms = 0 to ${o.starts - 2}`, '  try', `    ${P}_try = ${call(`${P}_q0 * ${P}_scales[${P}_ms]`)}`, `    if ${P}_try.cost < ${P}_fit.cost then`, `      ${P}_fit = ${P}_try`, '    end if', '  catch e', '    # that start did not work; keep the best so far', '  end try', 'end for');
  }
  if (o.robust !== 'none') {
    const wv = o.wt ? ' .* wt' : '';
    const hub = o.robust === 'huber';
    const huber = [`${P}_u = abs(${P}_r) / (1.345 * ${P}_s)`, `${P}_rw = sqrt(1 ./ max(1, ${P}_u))`];
    const bisq = [`${P}_u = ${P}_r / (4.685 * ${P}_s)`, `${P}_rw = (abs(${P}_u) < 1) .* (1 - ${P}_u .^ 2)`];
    L.push(
      hub
        ? `# robust: iteratively reweighted least squares with Huber weights; outliers get weights below 1`
        : `# robust: iteratively reweighted least squares, 15 Huber steps to get close, then bisquare weights (outliers get weight 0)`,
      `for ${P}_it = 1 to 40`,
      `  ${P}_r = (${P}_eval(${o.xs.join(', ')}, ${P}_fit.params) - ${o.ys})${wv}`,
      `  ${P}_s = 1.4826 * median(abs(${P}_r - median(${P}_r)))`,
      `  if ${P}_s > 0 then`,
      ...(hub
        ? huber.map((t) => `    ${t}`)
        : [`    if ${P}_it <= 15 then`, ...huber.map((t) => `      ${t}`), '    else', ...bisq.map((t) => `      ${t}`), '    end if']),
      `    ${P}_prev = ${P}_fit.params`,
      `    ${P}_fit = ${call(`${P}_fit.params`)}`,
      `    if ${P}_it > 15 and max(abs(${P}_fit.params - ${P}_prev)) < 1e-9 * (1 + max(abs(${P}_prev))) then`,
      '      break',
      '    end if',
      '  end if',
      'end for',
    );
  }
  return L;
}

function genBlock(o: BlockOpts): string[] {
  const P = o.pfx;
  const r = o.r;
  const free = freeInfo(r);
  const L: string[] = [];
  const xa = o.xs.join(', ');
  const sub = (s: string) => subst(s, { pfx: P, xs: o.xs, ys: o.ys, kind: o.kind });
  const fixedArg = (i: number) => sub(r.guess[i]);
  const idx = r.params.map((_, i) => (isFree(r, i) ? free.names.indexOf(r.params[i]) : -1));
  const arg = (i: number) => (idx[i] >= 0 ? `q[${idx[i]}]` : fixedArg(i));
  const used = r.pre.join('\n') + '\n' + r.guess.join('\n');
  const force = o.kind === 'surface' ? ['xlo', 'xhi', 'xr', 'ylo', 'yhi', 'yr'] : o.kind === 'ode' ? ['x0', 'x1', 'xr', 'y0'] : ['x0', 'x1', 'xr'];
  const sum = summaryLines(o.kind === 'surface' ? 'surface' : 'yx', used, force);
  if (o.kind === 'ode') {
    // the ODE state is always the response; use the time/response names of this block
  }
  L.push(`# ---- ${r.title}: ${r.formula} ----`);
  L.push(...sum.map(sub));
  // model + evaluation + residual
  if (o.kind === 'ode') {
    const n = r.odeCount!;
    L.push(`function ${P}_rhs(t, y, ${r.params.slice(0, n).join(', ')})`, `  return ${r.body}`, 'end function');
    const rhsArgs = [...Array(n).keys()].map(arg).join(', ');
    const y0 = r.params.length > n ? arg(n) : `${o.ys}[0]`;
    L.push(
      `function ${P}_eval(${xa}, q)`, `  f = (s_t, s_y) := ${P}_rhs(s_t, s_y, ${rhsArgs})`,
      `  sol = ode45(f, [${o.xs[0]}[0], ${o.xs[0]}[length(${o.xs[0]}) - 1]], ${y0}, t_eval=${o.xs[0]})`, '  return sol.y[:, 0]', 'end function',
    );
  } else {
    L.push(`function ${P}_model(${xa}, ${r.params.join(', ')})`, `  return ${sub(r.body)}`, 'end function');
    L.push(`function ${P}_eval(${xa}, q)`, `  return ${P}_model(${xa}, ${r.params.map((_, i) => arg(i)).join(', ')})`, 'end function');
  }
  const w: string[] = [];
  if (o.wt) w.push('wt');
  if (o.robust !== 'none') w.push(`${P}_rw`);
  L.push(`function ${P}_resid(q)`, `  return (${P}_eval(${xa}, q) - ${o.ys})${w.map((v) => ` .* ${v}`).join('')}`, 'end function');
  const inner: string[] = [];
  inner.push(...r.pre.map(sub));
  inner.push(`${P}_q0 = [${free.names.map((nm) => sub(r.guess[r.params.indexOf(nm)])).join(', ')}]   # starting values of ${free.names.join(', ')}`);
  if (o.robust !== 'none') inner.push(`${P}_rw = ones(length(${o.ys}))`);
  inner.push(...fitLines(P, o, free));
  inner.push(`${P}_q = ${P}_fit.params`, `${P}_se = ${P}_fit.stderr`, `${P}_lo = ${P}_fit.ci[:, 0]`, `${P}_hi = ${P}_fit.ci[:, 1]`, `${P}_tv = ${P}_fit.t`, `${P}_pv = ${P}_fit.p_values`);
  inner.push(`${P}_pred = ${P}_eval(${xa}, ${P}_q)`);
  inner.push(...statLines(P, o.ys, `${P}_pred`, `length(${P}_q)`, { wt: o.wt, sigma: o.sigma, order: o.kind === 'surface' ? null : o.xs[0] }));
  inner.push(statVector(P, `${P}_fit.converged * 1`, `${P}_fit.iterations`));
  if (o.robust !== 'none') {
    const hub = o.robust === 'huber';
    const wv = o.wt ? ' .* wt' : '';
    inner.push(
      `${P}_r = ${P}_res${wv}`, `${P}_s = 1.4826 * median(abs(${P}_r - median(${P}_r)))`,
      hub ? `${P}_u = abs(${P}_r) / (1.345 * ${P}_s)` : `${P}_u = ${P}_r / (4.685 * ${P}_s)`,
      hub ? `${P}_ow = 1 ./ max(1, ${P}_u)` : `${P}_ow = ((abs(${P}_u) < 1) .* (1 - ${P}_u .^ 2)) .^ 2`,
    );
  }
  // curve / surface grid
  if (o.kind === 'surface') {
    inner.push(
      `${P}_ux = linspace(${P}_xlo - ${o.ext} * ${P}_xr, ${P}_xhi + ${o.ext} * ${P}_xr, 30)`, `${P}_uy = linspace(${P}_ylo - ${o.ext} * ${P}_yr, ${P}_yhi + ${o.ext} * ${P}_yr, 30)`,
      `${P}_gx = zeros(900)`, `${P}_gy = zeros(900)`, 'for i = 0 to 29', '  for j = 0 to 29', `    ${P}_gx[30 * i + j] = ${P}_ux[i]`, `    ${P}_gy[30 * i + j] = ${P}_uy[j]`, '  end for', 'end for',
      `${P}_gz = ${P}_eval(${P}_gx, ${P}_gy, ${P}_q)`,
    );
  } else {
    const lo = o.kind === 'ode' ? `${P}_x0` : o.logx ? `${P}_x0 * (${P}_x1 / ${P}_x0) ^ (-${o.ext})` : `${P}_x0 - ${o.ext} * ${P}_xr`;
    const hi = o.logx ? `${P}_x1 * (${P}_x1 / ${P}_x0) ^ ${o.ext}` : `${P}_x1 + ${o.ext} * ${P}_xr`;
    inner.push(o.logx ? `${P}_gx = exp(linspace(log(${lo}), log(${hi}), 400))` : `${P}_gx = linspace(${lo}, ${hi}, 400)`);
    inner.push(`${P}_gy = ${P}_eval(${P}_gx, ${P}_q)`);
    if (o.band && o.kind === 'curve') {
      inner.push(`${P}_gb = ${P}_band(${P}_gx, ${P}_q, ${P}_fit.cov, tinv(0.975, max(${P}_fit.dof, 1)))   # 95% confidence band of the curve`);
    }
  }
  if (o.band && o.kind === 'curve') {
    L.push(
      `# half-width of the confidence band: delta method, central differences of the model, covariance from the fit`,
      `function ${P}_band(gx, q, C, tc)`, '  m = length(gx)', '  k = length(q)', '  G = zeros(k, m)', '  for j = 0 to k - 1',
      '    h = 1e-6 * max(abs(q[j]), 1e-3)', '    qa = q', '    qa[j] = q[j] + h', '    qb = q', '    qb[j] = q[j] - h',
      `    G[j, :] = (${P}_eval(gx, qa) - ${P}_eval(gx, qb)) / (2 * h)`, '  end for', '  v = zeros(m)', '  for i = 0 to k - 1', '    for j = 0 to k - 1',
      '      v = v + C[i, j] * G[i, :] .* G[j, :]', '    end for', '  end for', '  return tc * sqrt(max(v, 0))', 'end function',
    );
  }
  // printed summary
  inner.push(`print("${r.title}: R2 =", ${P}_r2, " adjusted R2 =", ${P}_r2adj, " RMSE =", ${P}_rmse, " AIC =", ${P}_aic, " BIC =", ${P}_bic)`);
  free.names.forEach((nm, i) => inner.push(`print("  ${nm} =", ${P}_q[${i}], " +/- ", ${P}_se[${i}], "  95% CI [", ${P}_lo[${i}], ",", ${P}_hi[${i}], "]")`));
  inner.push(`if ${P}_fit.converged == false then`, `  print("CFWARN ${P} did not converge")`, 'end if');
  if (o.guard) {
    L.push('try', ...inner.map((s) => `  ${s}`), 'catch e', `  print("CFERR ${P} " + e.message)`, `  ${P}_stat = nan * ones(${NSTAT})`, 'end try');
  } else L.push(...inner);
  return L;
}

// ------------------------------------------------------------------- planning

export interface BlockInfo {
  pfx: string;
  title: string;
  formula: string;
  /** Free parameters, in the order of the `_q` vector. */
  free: string[];
  lo: number[];
  hi: number[];
  /** Fixed parameters and the text of their value. */
  fixed: Array<{ name: string; value: string }>;
  signal?: string;
  /** Script variables holding this block's data: independent variable(s) and response. */
  xv: string;
  xv2?: string;
  yv: string;
  kind: 'curve' | 'surface' | 'ode';
  hasBand: boolean;
  robust: boolean;
  modelId: string;
}

export interface Plan {
  script: string;
  layout: 'single' | 'compare' | 'surface' | 'separate' | 'shared' | 'ode';
  blocks: BlockInfo[];
  /** Joint fit: labels of the entries of cf_q. */
  jointLabels?: string[];
  jointLo?: number[];
  jointHi?: number[];
  signals?: string[];
  criterion: Criterion;
}

export type PlanResult = { ok: true; plan: Plan } | { ok: false; error: string };

const modelLabel = (ref: ModelRef) => (ref.id === CUSTOM_ID ? 'Custom: ' + ref.expr.trim() : modelDef(ref.id)?.label ?? ref.id);
export { modelLabel };

function needsFor(spec: FitSpec): Need[] {
  const c = spec.cols;
  if (spec.shape === 'yx') {
    const n: Need[] = [{ col: c.x, as: 'x' }, { col: c.y, as: 'y' }];
    if (spec.weight !== 'none' && c.err) n.push({ col: c.err, as: spec.weight === 'sigma' ? 'sg' : 'wgt' });
    return n;
  }
  if (spec.shape === 'surface') return [{ col: c.x, as: 'x' }, { col: c.y, as: 'y' }, { col: c.z, as: 'z' }];
  return [{ col: c.t, as: 't' }, ...c.signals.map((s) => ({ col: s, as: signalVar(spec, s) }))];
}

/** Script variable of a signal column: `s_<name>`, made unique. */
export function signalVar(spec: FitSpec, col: string): string {
  const used: string[] = [];
  for (const s of spec.cols.signals) {
    let v = `s_${sanitizeName(s)}`;
    while (used.includes(v)) v += '_';
    used.push(v);
    if (s === col) return v;
  }
  return `s_${sanitizeName(col)}`;
}

const exampleSignalVars = (ex: { columns: string[] }) => ex.columns.slice(1).map((c) => `s_${c}`);

/** Plain-words check of everything except the model details (those come from resolveModel). */
export function validateSpec(spec: FitSpec): string | null {
  if (spec.source === 'example') {
    const ex = exampleById(spec.example);
    if (!ex) return 'Pick an example.';
    if (ex.shape !== spec.shape) return `The example '${ex.label}' is not ${spec.shape === 'yx' ? 'y(x)' : spec.shape === 'surface' ? 'z(x, y)' : 'time-series'} data: pick another or change the data shape.`;
    if (spec.shape === 'yx' && spec.weight === 'sigma' && !ex.columns.includes('sg')) return 'This example has no y-error column: set the weights to none.';
    if (spec.shape === 'yx' && spec.weight === 'weight') return 'The examples have no weights column: set the weights to none or to y-error on the first example.';
  } else {
    if (!spec.table || spec.table.columns.length === 0) return 'Load some data first: paste two columns of numbers, open a CSV, or pick a variable.';
    const names = spec.table.columns.map((c) => c.name);
    const need = (label: string, v: string) => (names.includes(v) ? null : `Pick the ${label} column.`);
    if (spec.shape === 'yx') {
      const e = need('x', spec.cols.x) ?? need('y', spec.cols.y);
      if (e) return e;
      if (spec.cols.x === spec.cols.y) return 'x and y are the same column.';
      if (spec.weight !== 'none' && !names.includes(spec.cols.err)) return spec.weight === 'sigma' ? 'Pick the y-error column (or set the weights to none).' : 'Pick the weights column (or set the weights to none).';
    } else if (spec.shape === 'surface') {
      const e = need('x', spec.cols.x) ?? need('y', spec.cols.y) ?? need('z', spec.cols.z);
      if (e) return e;
      if (new Set([spec.cols.x, spec.cols.y, spec.cols.z]).size < 3) return 'x, y and z must be three different columns.';
    } else {
      const e = need('time', spec.cols.t);
      if (e) return e;
      if (spec.cols.signals.length === 0) return 'Tick at least one signal column.';
      const bad = spec.cols.signals.find((s) => !names.includes(s) || s === spec.cols.t);
      if (bad) return `Signal '${bad}' is not a usable column (it is missing or is the time column).`;
    }
    const used = needsFor(spec).map((n) => n.col);
    const rows = spec.table.rows - (spec.table.rows - clean(spec.table, used));
    if (rows < 3) return `Only ${rows} complete row${rows === 1 ? '' : 's'}: a fit needs more points than parameters.`;
  }
  if (!Number.isInteger(spec.starts) || spec.starts < 1 || spec.starts > 9) return 'Multi-start must be a whole number from 1 to 9.';
  if (!(spec.extrap >= 0 && spec.extrap <= 5)) return 'The extrapolation range must be from 0 to 5 (a fraction of the data range).';
  return null;
}

function clean(t: Table, cols: string[]): number {
  let n = 0;
  for (let i = 0; i < t.rows; i++) if (cols.every((c) => Number.isFinite(t.columns.find((k) => k.name === c)?.values[i] ?? NaN))) n++;
  return n;
}

function infoOf(pfx: string, r: Resolved, o: Pick<BlockInfo, 'kind' | 'hasBand' | 'robust' | 'signal' | 'xv' | 'xv2' | 'yv'> & { modelId: string }): BlockInfo {
  const f = freeInfo(r);
  const sig = (s: string) => subst(s, { pfx, xs: ['x'], ys: 'y', kind: 'curve' });
  return {
    pfx, title: r.title, formula: r.formula, free: f.names, lo: f.lo, hi: f.hi,
    fixed: r.params.map((p, i) => ({ p, i })).filter(({ i }) => !isFree(r, i)).map(({ p, i }) => ({ name: p, value: sig(r.guess[i]) })),
    ...o,
  };
}

const header = (title: string) => [`# ${title}`, '# Generated by the Fit tab: every number below comes from this script. Edit and re-run it freely.'];

/** The whole script, plus what the panel needs to read the results. */
export function buildCurveFit(spec: FitSpec, mode: 'show' | 'run' = 'show', runFile = ''): PlanResult {
  const problem = validateSpec(spec);
  if (problem) return { ok: false, error: problem };
  const needs = needsFor(spec);
  const input = { source: spec.source, table: spec.table, example: spec.example, filePath: spec.filePath };
  const data = dataLines(input, needs, mode, runFile);
  if (spec.shape === 'yx' && spec.weight !== 'none') data.push(spec.weight === 'sigma' ? 'wt = 1 ./ sg    # weights = 1 / y-error' : 'wt = sqrt(wgt)    # weights: squared residuals are multiplied by the weight column');
  if (spec.shape === 'multi' && spec.source === 'example') {
    const ex = exampleById(spec.example);
    ex?.columns.slice(1).forEach((c) => data.push(`s_${c} = ${c}`));
  }
  const sigmaMode = spec.shape === 'yx' && spec.weight === 'sigma';
  const useWt = spec.shape === 'yx' && spec.weight !== 'none';
  const L: string[] = [];
  const blocks: BlockInfo[] = [];
  const ext = spec.extrap;
  const common = { wt: useWt, sigma: sigmaMode, robust: spec.robust, starts: spec.starts, ext, logx: spec.logx };
  const exampleSig = spec.source === 'example' ? exampleSignalVars(EXAMPLE_LOOKUP(spec.example)) : null;

  if (spec.shape === 'surface') {
    const r = resolveModel(spec.model, 'surface', 'x');
    if (!r.ok) return r;
    if (r.value.params.length >= (spec.source === 'example' ? 9 : clean(spec.table!, needs.map((n) => n.col)))) {
      return { ok: false, error: `The model has ${r.value.params.length} parameters but there are not that many points.` };
    }
    L.push(...header(`Surface fit: z = f(x, y), ${r.value.title}`), ...data, '');
    L.push(...genBlock({ ...common, pfx: 'cf', kind: 'surface', xs: ['x', 'y'], ys: 'z', r: r.value, band: false, guard: false, wt: false, sigma: false }));
    blocks.push(infoOf('cf', r.value, { kind: 'surface', xv: 'x', xv2: 'y', yv: 'z', hasBand: false, robust: spec.robust !== 'none', modelId: spec.model.id }));
    return done(spec, L, blocks, 'surface');
  }

  if (spec.shape === 'yx') {
    if (spec.mode === 'compare') {
      if (spec.compare.length < 2) return { ok: false, error: 'Compare needs at least two models: tick two or more in the list.' };
      const rs: Array<Resolved> = [];
      for (const ref of spec.compare) {
        const r = resolveModel(ref, 'yx', 'x');
        if (!r.ok) return { ok: false, error: `${modelLabel(ref)}: ${r.error}` };
        rs.push(r.value);
      }
      L.push(...header(`Model comparison on one data set (${rs.length} models, ranked by ${spec.criterion.toUpperCase()})`), ...data, '');
      rs.forEach((r, i) => {
        const pfx = `cf${i + 1}`;
        L.push(...genBlock({ ...common, pfx, kind: 'curve', xs: ['x'], ys: 'y', r, band: false, guard: true }), '');
        blocks.push(infoOf(pfx, r, { kind: 'curve', xv: 'x', yv: 'y', hasBand: false, robust: spec.robust !== 'none', modelId: spec.compare[i].id }));
      });
      const ci = { aic: 9, aicc: 10, bic: 11 }[spec.criterion];
      const M = rs.length;
      L.push(
        `# ranking: lowest ${spec.criterion.toUpperCase()} first; models that failed to fit are ranked last`,
        `cmp_score = [${rs.map((_, i) => `cf${i + 1}_stat[${ci}]`).join(', ')}]`,
        `cmp_key = zeros(${M})`, `for i = 0 to ${M - 1}`, '  if isfinite(cmp_score[i]) then', '    cmp_key[i] = cmp_score[i]', '  else', '    cmp_key[i] = inf', '  end if', 'end for',
        'cmp_order = argsort(cmp_key)', 'cmp_delta = cmp_key - min(cmp_key)', 'cmp_weight = exp(-cmp_delta / 2)', 'cmp_weight = cmp_weight / sum(cmp_weight)',
        `print("ranking (best first), models numbered from 1:", cmp_order + 1)`, 'print("Akaike weights:", cmp_weight)',
      );
      return done(spec, L, blocks, 'compare');
    }
    const r = resolveModel(spec.model, 'yx', 'x');
    if (!r.ok) return r;
    L.push(...header(`Curve fit: y = f(x), ${r.value.title}`), ...data, '');
    L.push(...genBlock({ ...common, pfx: 'cf', kind: 'curve', xs: ['x'], ys: 'y', r: r.value, band: spec.band, guard: false }));
    blocks.push(infoOf('cf', r.value, { kind: 'curve', xv: 'x', yv: 'y', hasBand: spec.band, robust: spec.robust !== 'none', modelId: spec.model.id }));
    return done(spec, L, blocks, 'single');
  }

  // time series: several signals against one time column
  const sigs = spec.cols.signals;
  const svars = exampleSig && spec.source === 'example' ? exampleSig : sigs.map((s) => signalVar(spec, s));
  const names = spec.source === 'example' ? svars.map((v) => v.slice(2)) : sigs;
  if (names.length === 0) return { ok: false, error: 'Tick at least one signal column.' };
  const lines = [...data];
  if (spec.coupling === 'ode') {
    lines.push('# the ODE is integrated over time, so the samples must be in increasing time order', 'cf_ord = argsort(t)', 't = t[cf_ord]');
    svars.forEach((v) => lines.push(`${v} = ${v}[cf_ord]`));
  }
  const mcommon = { ...common, wt: false, sigma: false };
  if (spec.coupling === 'ode') {
    const r = resolveOde(spec);
    if (!r.ok) return r;
    L.push(...header(`Time-series fit: ${r.value.formula} for ${names.length} signal${names.length > 1 ? 's' : ''}`), ...lines, '');
    svars.forEach((v, i) => {
      const pfx = `cf${i + 1}`;
      L.push(...genBlock({ ...mcommon, pfx, kind: 'ode', xs: ['t'], ys: v, r: r.value, band: false, guard: true }), '');
      blocks.push(infoOf(pfx, r.value, { kind: 'ode', xv: 't', yv: v, hasBand: false, robust: false, signal: names[i], modelId: ODE_ID }));
    });
    return done(spec, L, blocks, 'ode', names);
  }
  const r = resolveModel(spec.model, 'multi', 't');
  if (!r.ok) return r;
  if (spec.coupling === 'separate') {
    L.push(...header(`Several signals against time, ${r.value.title} fitted to each separately`), ...lines, '');
    svars.forEach((v, i) => {
      const pfx = `cf${i + 1}`;
      L.push(...genBlock({ ...mcommon, pfx, kind: 'curve', xs: ['t'], ys: v, r: r.value, band: spec.band, guard: true }), '');
      blocks.push(infoOf(pfx, r.value, { kind: 'curve', xv: 't', yv: v, hasBand: spec.band, robust: spec.robust !== 'none', signal: names[i], modelId: spec.model.id }));
    });
    return done(spec, L, blocks, 'separate', names);
  }
  // shared: one joint fit, one residual vector over all signals
  if (names.length < 2) return { ok: false, error: 'A shared-parameter fit needs at least two signals.' };
  const rv = r.value;
  const sharedNames = rv.params.filter((p) => spec.shared.includes(p));
  if (sharedNames.length === 0) return { ok: false, error: 'Tick at least one parameter to share between the signals (for example the decay rate).' };
  const m = names.length;
  // layout of q: shared free parameters first, then each signal's own free parameters
  const labels: string[] = [];
  const jlo: number[] = [];
  const jhi: number[] = [];
  const fi = freeInfo(rv);
  const slotShared: Record<string, number> = {};
  fi.names.forEach((p, k) => {
    if (sharedNames.includes(p)) {
      slotShared[p] = labels.length;
      labels.push(`${p} (shared)`);
      jlo.push(fi.lo[k]);
      jhi.push(fi.hi[k]);
    }
  });
  const slotOwn: Array<Record<string, number>> = [];
  for (let s = 0; s < m; s++) {
    const slots: Record<string, number> = {};
    fi.names.forEach((p, k) => {
      if (!sharedNames.includes(p)) {
        slots[p] = labels.length;
        labels.push(`${p} (${names[s]})`);
        jlo.push(fi.lo[k]);
        jhi.push(fi.hi[k]);
      }
    });
    slotOwn.push(slots);
  }
  L.push(...header(`Joint fit of ${m} signals with shared parameter${sharedNames.length > 1 ? 's' : ''} ${sharedNames.join(', ')}: one residual vector over all signals`), ...lines, '');
  L.push(`function cf_model(t, ${rv.params.join(', ')})`, `  return ${subst(rv.body, { pfx: 'cf', xs: ['t'], ys: '', kind: 'curve' })}`, 'end function');
  const sumAll: string[] = [];
  svars.forEach((v, s) => {
    const pfx = `cf${s + 1}`;
    const sub = (t: string) => subst(t, { pfx, xs: ['t'], ys: v, kind: 'curve' });
    const used = rv.pre.join('\n') + rv.guess.join('\n');
    sumAll.push(`# summary of ${names[s]} for its starting values`, ...summaryLines('yx', used, ['x0', 'x1', 'xr']).map(sub));
    const args = rv.params.map((p, i) => {
      if (!isFree(rv, i)) return sub(rv.guess[i]);
      return `q[${sharedNames.includes(p) ? slotShared[p] : slotOwn[s][p]}]`;
    });
    L.push(`function cf_eval${s + 1}(t, q)`, `  return cf_model(t, ${args.join(', ')})`, 'end function');
  });
  L.push(...sumAll);
  L.push('function cf_resid(q)', `  return [${svars.map((v, s) => `cf_eval${s + 1}(t, q) - ${v}`).join(', ')}]`, 'end function');
  const q0: string[] = [];
  const preLines: string[] = [];
  labels.forEach((lab) => {
    const p = lab.split(' ')[0];
    const i = rv.params.indexOf(p);
    const sOwn = lab.includes('(shared)') ? 0 : names.indexOf(lab.slice(lab.indexOf('(') + 1, -1));
    const pfx = `cf${sOwn + 1}`;
    q0.push(subst(rv.guess[i], { pfx, xs: ['t'], ys: svars[sOwn], kind: 'curve' }));
  });
  svars.forEach((v, s) => preLines.push(...rv.pre.map((t) => subst(t, { pfx: `cf${s + 1}`, xs: ['t'], ys: v, kind: 'curve' }))));
  const hasBounds = jlo.some((v) => v > -Infinity) || jhi.some((v) => v < Infinity);
  const inner: string[] = [...preLines];
  inner.push(`cf_q0 = [${q0.join(', ')}]   # starting values: ${labels.join(', ')}`);
  inner.push(`cf_fit = least_squares("cf_resid", cf_q0${hasBounds ? `, lower=[${jlo.map(num).join(', ')}], upper=[${jhi.map(num).join(', ')}]` : ''}, max_iter=500)`);
  inner.push('cf_q = cf_fit.params', 'cf_se = cf_fit.stderr', 'cf_lo = cf_fit.ci[:, 0]', 'cf_hi = cf_fit.ci[:, 1]', 'cf_tv = cf_fit.t', 'cf_pv = cf_fit.p_values');
  const kOwn = (s: number) => `${Object.keys(slotShared).length + Object.keys(slotOwn[s]).length}`;
  svars.forEach((v, s) => {
    const pfx = `cf${s + 1}`;
    inner.push(`${pfx}_pred = cf_eval${s + 1}(t, cf_q)`, ...statLines(pfx, v, `${pfx}_pred`, kOwn(s), { wt: false, sigma: false, order: 't' }), statVector(pfx, 'cf_fit.converged * 1', 'cf_fit.iterations'));
  });
  inner.push(`cf_y = [${svars.join(', ')}]`, `cf_pred = [${svars.map((_, s) => `cf${s + 1}_pred`).join(', ')}]`);
  inner.push(...statLines('cf', 'cf_y', 'cf_pred', 'length(cf_q)', { wt: false, sigma: false, order: null }), statVector('cf', 'cf_fit.converged * 1', 'cf_fit.iterations'));
  inner.push(`cf_x0 = min(t)`, `cf_x1 = max(t)`, `cf_xr = cf_x1 - cf_x0`);
  inner.push(spec.logx ? `cf_gx = exp(linspace(log(cf_x0 * (cf_x1 / cf_x0) ^ (-${ext})), log(cf_x1 * (cf_x1 / cf_x0) ^ ${ext}), 400))` : `cf_gx = linspace(cf_x0 - ${ext} * cf_xr, cf_x1 + ${ext} * cf_xr, 400)`);
  svars.forEach((_, s) => inner.push(`cf${s + 1}_gx = cf_gx`, `cf${s + 1}_gy = cf_eval${s + 1}(cf_gx, cf_q)`));
  inner.push(`print("joint fit: R2 =", cf_r2, " RMSE =", cf_rmse, " AIC =", cf_aic, " BIC =", cf_bic)`);
  labels.forEach((lab, i) => inner.push(`print("  ${lab} =", cf_q[${i}], " +/- ", cf_se[${i}])`));
  inner.push('if cf_fit.converged == false then', '  print("CFWARN cf did not converge")', 'end if');
  L.push('try', ...inner.map((s) => `  ${s}`), 'catch e', '  print("CFERR cf " + e.message)', `  cf_stat = nan * ones(${NSTAT})`, 'end try');
  svars.forEach((v, s) => {
    blocks.push({
      pfx: `cf${s + 1}`, title: rv.title, formula: rv.formula, free: fi.names, lo: fi.lo, hi: fi.hi,
      fixed: [], signal: names[s], xv: 't', yv: v, kind: 'curve', hasBand: false, robust: false, modelId: spec.model.id,
    });
  });
  const plan = done(spec, L, blocks, 'shared', names);
  if (plan.ok) {
    plan.plan.jointLabels = labels;
    plan.plan.jointLo = jlo;
    plan.plan.jointHi = jhi;
  }
  return plan;
}

function EXAMPLE_LOOKUP(id: string): { columns: string[] } {
  return exampleById(id) ?? { columns: [] };
}

function done(spec: FitSpec, L: string[], blocks: BlockInfo[], layout: Plan['layout'], signals?: string[]): PlanResult {
  return { ok: true, plan: { script: L.join('\n') + '\n', layout, blocks, signals, criterion: spec.criterion } };
}

export const buildCurveFitScript = (spec: FitSpec, mode: 'show' | 'run' = 'show', runFile = ''): string => {
  const p = buildCurveFit(spec, mode, runFile);
  return p.ok ? p.plan.script : `# ${p.error}\n`;
};

/** Which temp CSV a run needs: the large-table case writes the needed columns to a file. */
export function needsDataFile(spec: FitSpec): { csv: string } | null {
  if (spec.source === 'example' || !spec.table) return null;
  const needs = needsFor(spec);
  if (!needsStaging(clean(spec.table, needs.map((n) => n.col)))) return null;
  return { csv: csvForScript(spec.table, needs) };
}

// ------------------------------------------------------------ reading results

export type Series = Record<string, (number | null)[]>;

export interface ParamRow {
  name: string;
  value: number;
  se: number;
  lo: number;
  hi: number;
  t: number;
  p: number;
  fixed?: boolean;
  atBound?: 'lower' | 'upper';
}

export interface BlockResult {
  info: BlockInfo;
  ok: boolean;
  converged: boolean;
  error?: string;
  stats: Stats | null;
  params: ParamRow[];
  gx: number[];
  gy: number[];
  gb: number[];
  gz: number[];
  pred: number[];
  res: number[];
  rw: number[];
}

const nums = (v: (number | null)[] | undefined): number[] => (v ?? []).map((x) => (x == null ? NaN : x));

/** `CFERR cf2 message` lines of the script's output, by block prefix. */
export function scriptErrors(output: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of output.split(/\r?\n/)) {
    const m = /^CFERR (\w+) (.*)$/.exec(line.trim());
    if (m) out[m[1]] = m[2];
  }
  return out;
}

export function readBlock(info: BlockInfo, s: Series, errors: Record<string, string>, jointLabels?: string[], jointBounds?: [number[], number[]]): BlockResult {
  const P = info.pfx;
  const stats = readStats(s[`${P}_stat`]);
  const q = nums(s[`${P}_q`]);
  const se = nums(s[`${P}_se`]);
  const lo = nums(s[`${P}_lo`]);
  const hi = nums(s[`${P}_hi`]);
  const tv = nums(s[`${P}_tv`]);
  const pv = nums(s[`${P}_pv`]);
  const useQ = jointLabels ? nums(s['cf_q']) : q;
  const ok = !!stats && (jointLabels ? useQ.length > 0 : q.length === info.free.length);
  const rows: ParamRow[] = [];
  if (ok) {
    const jq = jointLabels ? nums(s['cf_q']) : q;
    const jse = jointLabels ? nums(s['cf_se']) : se;
    const jlo = jointLabels ? nums(s['cf_lo']) : lo;
    const jhi = jointLabels ? nums(s['cf_hi']) : hi;
    const jt = jointLabels ? nums(s['cf_tv']) : tv;
    const jp = jointLabels ? nums(s['cf_pv']) : pv;
    const names = jointLabels ?? info.free;
    const los = jointBounds?.[0] ?? info.lo;
    const his = jointBounds?.[1] ?? info.hi;
    names.forEach((name, i) => {
      const v = jq[i];
      const atBound = Number.isFinite(los[i]) && Math.abs(v - los[i]) <= 1e-9 * Math.max(1, Math.abs(v)) ? 'lower' : Number.isFinite(his[i]) && Math.abs(v - his[i]) <= 1e-9 * Math.max(1, Math.abs(v)) ? 'upper' : undefined;
      rows.push({ name, value: v, se: jse[i], lo: jlo[i], hi: jhi[i], t: jt[i], p: jp[i], atBound });
    });
    if (!jointLabels) info.fixed.forEach((f) => rows.push({ name: f.name, value: Number(f.value), se: NaN, lo: NaN, hi: NaN, t: NaN, p: NaN, fixed: true }));
  }
  return {
    info, ok, converged: !!stats && stats.converged === 1, error: errors[P] ?? (ok ? undefined : errors['cf']), stats, params: rows,
    gx: nums(s[`${P}_gx`]), gy: nums(s[`${P}_gy`]), gb: nums(s[`${P}_gb`]), gz: nums(s[`${P}_gz`]), pred: nums(s[`${P}_pred`]), res: nums(s[`${P}_res`]), rw: nums(s[`${P}_ow`]),
  };
}

export interface Ranked {
  index: number;
  rank: number;
  delta: number;
  weight: number;
  status: 'ok' | 'not converged' | 'failed';
  equalFit: boolean;
}

/** Ranking of the comparison from the script's own `cmp_*` vectors. */
export function readRanking(s: Series, blocks: BlockResult[]): Ranked[] {
  const order = nums(s['cmp_order']);
  const delta = nums(s['cmp_delta']);
  const weight = nums(s['cmp_weight']);
  return order.map((idx, rank) => {
    const b = blocks[idx];
    const d = delta[idx];
    return {
      index: idx, rank: rank + 1, delta: d, weight: weight[idx],
      status: !b.ok ? 'failed' : b.converged ? 'ok' : 'not converged',
      equalFit: Number.isFinite(d) && d < 2,
    };
  });
}

/** Line number (1-based) an engine error points at, and that line of the script. */
export function errorContext(message: string, script: string): { line: number; text: string } | null {
  const m = /(?:\.qu|script)[:\s]+(\d+)|\bline (\d+)/i.exec(message) ?? /\bat [^\s:]+:(\d+)/.exec(message);
  if (!m) return null;
  const line = Number(m[1] ?? m[2]);
  const text = script.split('\n')[line - 1];
  return text == null ? null : { line, text: text.trim() };
}

/** Engine error text without the interpreter's boilerplate prefix. */
export function plainError(message: string): string {
  return message.replace(/^qu: (runtime |parse )?error:?\s*/im, '').split('\n')[0].trim();
}

export { isName };

// ------------------------------------------------------------------- defaults

export function defaultSpec(): FitSpec {
  return {
    shape: 'yx', source: 'example', table: null, example: 'decay',
    cols: { x: '', y: '', z: '', err: '', t: '', signals: [] },
    weight: 'none', mode: 'single', model: emptyRef('exp'),
    compare: [emptyRef('poly', 1), emptyRef('poly', 2), emptyRef('exp'), emptyRef('power')], criterion: 'aicc',
    coupling: 'separate', shared: [], odeRhs: '-k*y', odePs: {}, fitY0: true,
    robust: 'none', starts: 1, band: true, extrap: 0, logx: false, logy: false,
  };
}
