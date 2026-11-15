/** Equation-solving tools of the Fit tab ("Solve equations" mode): the Qu
 *  script they generate, the checks that keep a bad input from reaching the
 *  engine, and the reading of the vectors the script leaves behind. All typed
 *  expressions go through mathExpr (parse, then re-emit from the AST), so no
 *  typed text is ever pasted into a script. No DOM, no Tauri. */

import { collectNames, emitExpr, linearForm, parseExpr, polynomialCoeffs, splitEquation, stripLhs, type Node } from './mathExpr';

export type SolveTask = 'equation' | 'extrema' | 'system';

export interface SolveSpec {
  task: SolveTask;
  /** f(x), or `f(x) = g(x)`. */
  expr: string;
  /** Right-hand side when `expr` has no `=`: a number or an expression in x. */
  rhs: string;
  lo: string;
  hi: string;
  /** Skip the polynomial shortcut and always scan the interval. */
  forceNumeric: boolean;
  eqs: string[];
  guesses: Record<string, string>;
  multistart: boolean;
  /** Multistart box: every unknown from -range to +range. */
  range: string;
  per: number;
}

export const defaultSolveSpec = (): SolveSpec => ({
  task: 'equation', expr: '2*x^2 + 3*x', rhs: '0', lo: '-10', hi: '10', forceNumeric: false,
  eqs: ['x^2 + y^2 = 4', 'x = y', '', ''], guesses: {}, multistart: false, range: '5', per: 5,
});

export const SOLVE_EXAMPLES: Array<{ label: string; patch: Partial<SolveSpec> }> = [
  { label: '2x^2 + 3x = 0', patch: { task: 'equation', expr: '2*x^2 + 3*x', rhs: '0', lo: '-10', hi: '10' } },
  { label: 'x^3 - 6x^2 + 11x - 6 = 0', patch: { task: 'equation', expr: 'x^3 - 6*x^2 + 11*x - 6', rhs: '0', lo: '-1', hi: '5' } },
  { label: 'cos(x) = x', patch: { task: 'equation', expr: 'cos(x)', rhs: 'x', lo: '-5', hi: '5' } },
  { label: 'x^2 = 2x + 3', patch: { task: 'equation', expr: 'x^2', rhs: '2*x + 3', lo: '-5', hi: '5' } },
  { label: 'circle and line', patch: { task: 'system', eqs: ['x^2 + y^2 = 4', 'x = y', '', ''], guesses: {} } },
  { label: 'linear 2x2', patch: { task: 'system', eqs: ['2*x + 3*y = 5', 'x - y = 0', '', ''], guesses: {} } },
];

export type SolveKind = 'polynomial' | 'scan' | 'linear' | 'nonlinear' | 'multistart';

export interface SolvePlan {
  script: string;
  kind: SolveKind;
  unknowns: string[];
  /** Polynomial route: coefficients, highest power first. */
  coef?: number[];
  /** Linear route: the coefficient matrix and right-hand side shown to the user. */
  matrix?: number[][];
  rhsVec?: number[];
  /** Equation tasks: is the right-hand side a curve (drawn) or just a number. */
  twoSided: boolean;
  lo: number;
  hi: number;
  unknown?: string;
}

export type SolveResult = { ok: true; plan: SolvePlan } | { ok: false; error: string };

const fail = (error: string): SolveResult => ({ ok: false, error });
const lit = (v: number) => String(Number(v.toPrecision(15)));

function interval(spec: SolveSpec): { lo: number; hi: number } | string {
  const lo = Number(spec.lo);
  const hi = Number(spec.hi);
  if (spec.lo.trim() === '' || spec.hi.trim() === '' || !Number.isFinite(lo) || !Number.isFinite(hi)) return 'The interval ends must be numbers.';
  if (!(lo < hi)) return 'The interval needs from < to.';
  return { lo, hi };
}

const SCAN_HELPERS = [
  '# scan an interval for sign changes of f and refine each one with fzero (a double root, where f only touches zero, has no sign change)',
  'function sv_scan(f, lo, hi, n)',
  '  s = linspace(lo, hi, n + 1)',
  '  v = f(s)',
  '  out = zeros(n)',
  '  c = 0',
  '  for i = 0 to n - 1',
  '    if isfinite(v[i]) and isfinite(v[i + 1]) then',
  '      if v[i] == 0 then',
  '        out[c] = s[i]',
  '        c = c + 1',
  '      elseif v[i] * v[i + 1] < 0 then',
  '        out[c] = fzero(f, s[i], s[i + 1])',
  '        c = c + 1',
  '      end if',
  '    end if',
  '  end for',
  '  if v[n] == 0 then',
  '    out[c] = s[n]',
  '    c = c + 1',
  '  end if',
  '  if c == 0 then',
  '    return zeros(0)',
  '  end if',
  '  return out[0:c - 1]',
  'end function',
];

/** Parses the one-unknown input; returns the pieces both tasks need. */
function oneVariable(spec: SolveSpec): { name: string; lhs: Node; rhs: Node | null; lhsConst: boolean; rhsConst: boolean } | string {
  const sp = splitEquation(spec.expr);
  if (!sp.ok) return sp.error;
  let lhsText = sp.lhs;
  let rhsText: string | null = sp.rhs;
  if (rhsText == null) {
    const r = spec.rhs.trim();
    rhsText = r === '' || r === '0' ? null : r;
  } else if (spec.rhs.trim() !== '' && spec.rhs.trim() !== '0') {
    return "The expression already has an '=': clear the right-hand side field (or drop the '=' from the expression).";
  }
  lhsText = stripLhs(lhsText).trim();
  const names = collectNames([lhsText, ...(rhsText ? [rhsText] : [])]);
  if (names.error) return names.error;
  if (names.names.length === 0) return 'There is no unknown: the expression needs a name such as x.';
  if (names.names.length > 1) return `More than one unknown (${names.names.join(', ')}): this tool solves for one. Use "System of equations" for several.`;
  const name = names.names[0];
  if (name.startsWith('sv_')) return `'${name}' is reserved for the generated script: rename the unknown.`;
  const a = parseExpr(lhsText, { vars: [name], allowParams: false });
  if (!a.ok) return a.error;
  let rhs: Node | null = null;
  let rhsConst = true;
  if (rhsText != null) {
    const b = parseExpr(rhsText, { vars: [name], allowParams: false });
    if (!b.ok) return `Right-hand side: ${b.error}`;
    rhs = b.ast;
    rhsConst = b.vars.length === 0;
  }
  return { name, lhs: a.ast, rhs, lhsConst: a.vars.length === 0, rhsConst };
}

export function buildSolve(spec: SolveSpec): SolveResult {
  if (spec.task === 'system') return buildSystem(spec);
  const iv = interval(spec);
  if (typeof iv === 'string') return fail(iv);
  const one = oneVariable(spec);
  if (typeof one === 'string') return fail(one);
  const { name, lhs, rhs } = one;
  const roots = spec.task === 'equation';
  const diff: Node = rhs ? { t: 'bin', op: '-', a: lhs, b: rhs } : lhs;
  const coef = spec.forceNumeric ? null : polynomialCoeffs(diff, name);
  const poly = !!coef && coef.length >= 2;
  if (coef && coef.length < 2 && !spec.forceNumeric) {
    return fail(rhs ? 'The two sides differ by a constant: there is no solution (or every x is one if the constant is zero).' : 'The expression is constant: there is nothing to solve.');
  }
  const L: string[] = [];
  const title = `${emitExpr(lhs)}${rhs ? ' = ' + emitExpr(rhs) : ' = 0'}`;
  L.push(`# ${spec.task === 'equation' ? 'Solve' : 'Extrema and inflections of'} ${title}`, '# Generated by the Solve tool: edit and re-run freely.');
  const zero = (c: boolean) => (c ? ' + 0 * ' + name : '');
  L.push(`function sv_lhs(${name})`, `  return ${emitExpr(lhs)}${zero(one.lhsConst)}`, 'end function');
  L.push(`function sv_rhs(${name})`, `  return ${rhs ? emitExpr(rhs) : '0'}${zero(one.rhsConst)}`, 'end function');
  L.push(`function sv_f(${name})`, `  return sv_lhs(${name}) - sv_rhs(${name})`, 'end function');
  L.push(`sv_lo = ${lit(iv.lo)}`, `sv_hi = ${lit(iv.hi)}`);
  const numericDerivs = [
    'function sv_d1(x)',
    '  h = 1e-5 * max(abs(x), 1)',
    '  return (sv_f(x + h) - sv_f(x - h)) / (2 * h)',
    'end function',
    'function sv_d2(x)',
    '  h = 1e-3 * max(abs(x), 1)',
    '  return (sv_f(x + h) - 2 * sv_f(x) + sv_f(x - h)) / (h * h)',
    'end function',
  ];
  if (poly) {
    // the difference f - g gives the roots; stationary points are those of the left-hand curve (f), as the plot shows
    const lc = coef;
    L.push(`sv_coef = [${coef!.map(lit).join(', ')}]   # ${title.replace(/ = .*/, '')} as a polynomial, highest power first${rhs ? ' (left minus right)' : ''}`);
    if (roots) {
      L.push('sv_rt = roots(sv_coef)', 'sv_rt_re = real(sv_rt)', 'sv_rt_im = imag(sv_rt)');
    } else {
      L.push('sv_rt_re = zeros(0)', 'sv_rt_im = zeros(0)');
    }
    L.push('sv_rt_y = sv_lhs(sv_rt_re)');
    if (lc && lc.length >= 3) {
      L.push(`sv_c1 = [${lc.map(lit).join(', ')}]   # f - g`, 'sv_p1 = polyder(sv_c1)', 'sv_e = roots(sv_p1)', 'sv_ex_x = real(sv_e)[find(abs(imag(sv_e)) < 1e-9)]', 'sv_ex_y = sv_lhs(sv_ex_x)', 'sv_ex_d2 = polyval(polyder(sv_p1), sv_ex_x)');
    } else L.push('sv_ex_x = zeros(0)', 'sv_ex_y = zeros(0)', 'sv_ex_d2 = zeros(0)');
    if (!roots && lc && lc.length >= 4) {
      L.push('sv_p2 = polyder(polyder(sv_c1))', 'sv_i = roots(sv_p2)', 'sv_if_x = real(sv_i)[find(abs(imag(sv_i)) < 1e-9)]', 'sv_if_y = sv_lhs(sv_if_x)');
    } else L.push('sv_if_x = zeros(0)', 'sv_if_y = zeros(0)');
  } else {
    L.push(...SCAN_HELPERS, ...numericDerivs);
    if (roots) {
      L.push('sv_rt_re = sv_scan(sv_f, sv_lo, sv_hi, 2000)', 'sv_rt_im = zeros(length(sv_rt_re))', 'sv_rt_y = sv_lhs(sv_rt_re)');
    } else L.push('sv_rt_re = zeros(0)', 'sv_rt_im = zeros(0)', 'sv_rt_y = zeros(0)');
    L.push('# stationary points: f\'(x) = 0 by central differences; sv_ex_d2 > 0 is a minimum, < 0 a maximum', 'sv_ex_x = sv_scan(sv_d1, sv_lo, sv_hi, 2000)', 'sv_ex_y = sv_lhs(sv_ex_x)', 'sv_ex_d2 = sv_d2(sv_ex_x)');
    if (!roots) L.push('# inflection points: f\'\'(x) changes sign', 'sv_if_x = sv_scan(sv_d2, sv_lo, sv_hi, 2000)', 'sv_if_y = sv_lhs(sv_if_x)');
    else L.push('sv_if_x = zeros(0)', 'sv_if_y = zeros(0)');
  }
  L.push('sv_gx = linspace(sv_lo, sv_hi, 600)', 'sv_gy = sv_lhs(sv_gx)', 'sv_gy2 = sv_rhs(sv_gx)');
  if (roots) {
    L.push(`print("solutions of ${title.replace(/"/g, '')}:")`, 'for i = 0 to length(sv_rt_re) - 1', `  print("  ${name} =", sv_rt_re[i], " + ", sv_rt_im[i], "j")`, 'end for', 'if length(sv_rt_re) == 0 then', `  print("  no solution in the interval")`, 'end if');
  }
  L.push('for i = 0 to length(sv_ex_x) - 1', `  print("  stationary point at ${name} =", sv_ex_x[i], " f =", sv_ex_y[i], " f'' =", sv_ex_d2[i])`, 'end for');
  if (!roots) L.push('for i = 0 to length(sv_if_x) - 1', `  print("  inflection at ${name} =", sv_if_x[i], " f =", sv_if_y[i])`, 'end for');
  return {
    ok: true,
    plan: { script: L.join('\n') + '\n', kind: poly ? 'polynomial' : 'scan', unknowns: [name], unknown: name, coef: poly ? coef! : undefined, twoSided: !!rhs && !one.rhsConst, lo: iv.lo, hi: iv.hi },
  };
}

function buildSystem(spec: SolveSpec): SolveResult {
  const lines = spec.eqs.map((s) => s.trim()).filter((s) => s !== '');
  if (lines.length === 0) return fail('Type at least one equation, one per line (for example x^2 + y^2 = 4).');
  if (lines.length > 4) return fail('At most 4 equations.');
  const sides: Array<{ l: Node; r: Node }> = [];
  const texts: string[] = [];
  for (const [i, ln] of lines.entries()) {
    const sp = splitEquation(ln);
    if (!sp.ok) return fail(`Equation ${i + 1}: ${sp.error}`);
    texts.push(sp.lhs, ...(sp.rhs != null ? [sp.rhs] : []));
  }
  const names = collectNames(texts);
  if (names.error) return fail(names.error);
  if (names.names.length === 0) return fail('The equations have no unknown names.');
  if (names.names.length > 4) return fail(`More than 4 unknowns (${names.names.join(', ')}).`);
  const bad = names.names.find((n) => n.startsWith('sv_'));
  if (bad) return fail(`'${bad}' is reserved for the generated script: rename the unknown.`);
  const unknowns = names.names;
  for (const [i, ln] of lines.entries()) {
    const sp = splitEquation(ln);
    if (!sp.ok) return fail(sp.error);
    const a = parseExpr(sp.lhs, { vars: unknowns, allowParams: false });
    if (!a.ok) return fail(`Equation ${i + 1}: ${a.error}`);
    let r: Node = { t: 'num', v: '0' };
    if (sp.rhs != null) {
      const b = parseExpr(sp.rhs, { vars: unknowns, allowParams: false });
      if (!b.ok) return fail(`Equation ${i + 1}: ${b.error}`);
      r = b.ast;
    }
    sides.push({ l: a.ast, r });
  }
  const m = sides.length;
  const n = unknowns.length;
  const forms = sides.map((s) => linearForm({ t: 'bin', op: '-', a: s.l, b: s.r }, unknowns));
  const linear = forms.every((f) => f !== null);
  const L: string[] = [`# System of ${m} equation${m > 1 ? 's' : ''} in ${unknowns.join(', ')}`, '# Generated by the Solve tool: edit and re-run freely.'];
  const eqTitle = sides.map((s) => `${emitExpr(s.l)} = ${emitExpr(s.r)}`);
  eqTitle.forEach((t) => L.push(`#   ${t}`));
  if (linear) {
    const A = forms.map((f) => f!.a);
    const b = forms.map((f) => (f!.c === 0 ? 0 : -f!.c));
    L.push(
      `sv_A = [${A.map((row) => row.map(lit).join(', ')).join('; ')}]   # coefficient matrix, columns ${unknowns.join(', ')}`,
      `sv_b = [${b.map(lit).join(', ')}]`, 'sv_cond = cond(sv_A)',
      m === n ? 'sv_sol = solve(sv_A, sv_b)' : '# not square: the least-squares / minimum-norm solution', m === n ? 'sv_x = sv_sol[:, 0]' : 'sv_x = lstsq(sv_A, sv_b)',
      'sv_norm = norm(sv_A * sv_x - sv_b)', 'sv_conv = 1',
      'if sv_cond > 1e12 then', '  print("CFWARN the coefficient matrix is singular or nearly so: the solution is not unique")', 'end if',
    );
    unknowns.forEach((u, i) => L.push(`print("${u} =", sv_x[${i}])`));
    L.push('print("residual norm", sv_norm)');
    return { ok: true, plan: { script: L.join('\n') + '\n', kind: 'linear', unknowns, matrix: A, rhsVec: b, twoSided: false, lo: 0, hi: 0 } };
  }
  L.push('function sv_sys(sv_u)', ...unknowns.map((u, i) => `  ${u} = sv_u[${i}]`), `  return [${sides.map((s) => `${emitExpr(s.l)} - (${emitExpr(s.r)})`).join(', ')}]`, 'end function');
  // starting guess
  const guess: string[] = [];
  for (const u of unknowns) {
    const g = (spec.guesses[u] ?? '').trim() === '' ? '1' : spec.guesses[u];
    const p = parseExpr(g, { vars: [], allowParams: false });
    if (!p.ok) return fail(`Starting value of ${u}: ${p.error}`);
    guess.push(emitExpr(p.ast));
  }
  if (!spec.multistart) {
    L.push(
      `sv_x0 = [${guess.join(', ')}]   # starting guess for ${unknowns.join(', ')}`, 'sv_sol = fsolve("sv_sys", sv_x0, max_iter=500)',
      'sv_x = sv_sol.x', 'sv_norm = sv_sol.norm', 'sv_conv = sv_sol.converged * 1',
      'if sv_conv == 0 then', '  print("CFWARN fsolve did not converge: try another starting guess or multiple starting points")', 'end if',
    );
    unknowns.forEach((u, i) => L.push(`print("${u} =", sv_x[${i}])`));
    L.push('print("residual norm", sv_norm)');
    return { ok: true, plan: { script: L.join('\n') + '\n', kind: 'nonlinear', unknowns, twoSided: false, lo: 0, hi: 0 } };
  }
  const R = Number(spec.range);
  if (!(R > 0) || !Number.isFinite(R)) return fail('The multistart range must be a positive number.');
  if (!Number.isInteger(spec.per) || spec.per < 2 || spec.per > 9) return fail('Starting points per unknown must be a whole number from 2 to 9.');
  const per = n > 3 ? Math.min(spec.per, 4) : spec.per;
  L.push(
    `# multistart: a grid of ${per}^${n} starting points between -${lit(R)} and ${lit(R)}; keep each distinct solution once`,
    `sv_K = ${per}`, `sv_R = ${lit(R)}`, 'sv_S = zeros(32, ' + n + ')', 'sv_ns = 0', `sv_start = zeros(${n})`,
    `for sv_i = 0 to sv_K ^ ${n} - 1`, '  sv_t = sv_i',
    `  for sv_j = 0 to ${n - 1}`, '    sv_dgt = sv_t - sv_K * floor(sv_t / sv_K)', '    sv_start[sv_j] = -sv_R + 2 * sv_R * sv_dgt / (sv_K - 1)', '    sv_t = floor(sv_t / sv_K)', '  end for',
    '  try',
    '    sv_try = fsolve("sv_sys", sv_start, max_iter=200)',
    '    if sv_try.converged and sv_try.norm < 1e-8 and sv_ns < 32 then',
    '      sv_new = 1',
    '      for sv_k = 0 to sv_ns - 1',
    '        if norm(sv_S[sv_k, :] - sv_try.x) < 1e-6 * max(1, norm(sv_try.x)) then',
    '          sv_new = 0',
    '        end if',
    '      end for',
    '      if sv_new == 1 then',
    '        sv_S[sv_ns, :] = sv_try.x',
    '        sv_ns = sv_ns + 1',
    '      end if',
    '    end if',
    '  catch e',
    '    # this start failed; the others still run',
    '  end try',
    'end for',
    `sv_x0 = [${guess.join(', ')}]`, 'sv_sol = fsolve("sv_sys", sv_x0, max_iter=500)', 'sv_x = sv_sol.x', 'sv_norm = sv_sol.norm', 'sv_conv = sv_sol.converged * 1',
    'print("distinct solutions found:", sv_ns)', 'for sv_k = 0 to sv_ns - 1', '  print(sv_S[sv_k, :])', 'end for',
    'if sv_ns == 0 then', '  print("CFWARN no starting point converged: widen the range or check that the system has a solution")', 'end if',
  );
  return { ok: true, plan: { script: L.join('\n') + '\n', kind: 'multistart', unknowns, twoSided: false, lo: 0, hi: 0 } };
}

export interface SolveOutput {
  roots: Array<{ re: number; im: number; y: number }>;
  stationary: Array<{ x: number; y: number; kind: 'minimum' | 'maximum' | 'flat' }>;
  inflections: Array<{ x: number; y: number }>;
  gx: number[];
  gy: number[];
  gy2: number[];
  solution?: number[];
  residual?: number;
  converged?: boolean;
  cond?: number;
  solutions?: number[][];
}

type Series = Record<string, (number | null)[]>;
const nv = (v: (number | null)[] | undefined) => (v ?? []).map((x) => (x == null ? NaN : x));

export function readSolve(plan: SolvePlan, s: Series): SolveOutput {
  const rx = nv(s['sv_rt_re']);
  const ri = nv(s['sv_rt_im']);
  const ry = nv(s['sv_rt_y']);
  const ex = nv(s['sv_ex_x']);
  const ey = nv(s['sv_ex_y']);
  const ed = nv(s['sv_ex_d2']);
  const ix = nv(s['sv_if_x']);
  const iy = nv(s['sv_if_y']);
  const out: SolveOutput = {
    roots: rx.map((re, i) => ({ re, im: ri[i] ?? 0, y: ry[i] })),
    stationary: ex.map((x, i) => ({ x, y: ey[i], kind: Math.abs(ed[i]) < 1e-6 ? 'flat' : ed[i] > 0 ? 'minimum' : 'maximum' })),
    inflections: ix.map((x, i) => ({ x, y: iy[i] })),
    gx: nv(s['sv_gx']), gy: nv(s['sv_gy']), gy2: nv(s['sv_gy2']),
  };
  if (plan.kind === 'linear' || plan.kind === 'nonlinear' || plan.kind === 'multistart') {
    out.solution = nv(s['sv_x']);
    out.residual = nv(s['sv_norm'])[0];
    out.converged = nv(s['sv_conv'])[0] === 1;
    out.cond = nv(s['sv_cond'])[0];
  }
  if (plan.kind === 'multistart') {
    const flat = nv(s['sv_S']);
    const n = plan.unknowns.length;
    const count = nv(s['sv_ns'])[0] ?? 0;
    const sols: number[][] = [];
    // sv_S is 32 x n, stored column by column
    for (let r = 0; r < count; r++) sols.push(Array.from({ length: n }, (_, c) => flat[c * 32 + r]));
    out.solutions = sols;
  }
  return out;
}

export const solveScript = (spec: SolveSpec): string => {
  const r = buildSolve(spec);
  return r.ok ? r.plan.script : `# ${r.error}\n`;
};
