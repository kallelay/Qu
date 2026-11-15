/** One shared parser/validator for every math expression the Fit tab (custom
 *  models, ODE right-hand sides, starting guesses) and its Solve mode accept.
 *
 *  SAFETY CONTRACT: a typed expression is never pasted into a generated Qu
 *  script. It is parsed here into an AST (numbers, names, + - * / ^ and
 *  parentheses, calls to an allow-listed set of math functions) and the script
 *  receives `emitExpr(ast)`, text rebuilt from that AST. Anything that is not
 *  a plain math expression (statements, strings, `;`, newlines, `end`,
 *  backticks, brackets, assignments, calls outside the allow-list) is refused
 *  with a plain message, so it cannot reach the script. No React, no Tauri. */

export type Node =
  | { t: 'num'; v: string }
  | { t: 'id'; n: string }
  | { t: 'un'; op: '-' | '+'; a: Node }
  | { t: 'bin'; op: '+' | '-' | '*' | '/' | '^'; a: Node; b: Node }
  | { t: 'call'; f: string; args: Node[] };

/** Allow-listed functions and their argument counts. */
export const MATH_FUNCS: Record<string, number> = {
  sin: 1, cos: 1, tan: 1, asin: 1, acos: 1, atan: 1, atan2: 2,
  sinh: 1, cosh: 1, tanh: 1, exp: 1, log: 1, log10: 1, log2: 1, sqrt: 1, cbrt: 1,
  abs: 1, sign: 1, floor: 1, ceil: 1, round: 1, erf: 1, min: 2, max: 2,
};

export const CONSTANTS = ['pi', 'e'];

/** Words Qu reads as syntax: never usable as a name. */
export const RESERVED = [
  'and', 'or', 'not', 'mod', 'in', 'as', 'to', 'step', 'skip', 'by', 'if', 'then', 'else', 'elseif', 'end', 'for',
  'while', 'do', 'repeat', 'until', 'return', 'function', 'sub', 'class', 'new', 'true', 'false', 'none', 'backend',
  'with', 'import', 'const', 'constant', 'try', 'catch', 'unsafe', 'global', 'break', 'continue', 'inf', 'nan',
];

export const MAX_EXPR_LENGTH = 400;

export type ParseResult = { ok: true; ast: Node; vars: string[]; params: string[] } | { ok: false; error: string };

export interface ParseOptions {
  /** Names that are the independent variable(s) of the expression. */
  vars: string[];
  /** Any other identifier is a model parameter (custom models) when true; an error when false. */
  allowParams: boolean;
  /** Extra read-only names an expression may use (the data summary names of a starting guess). */
  extraNames?: string[];
  /** Names a parameter may not take (for example the ODE state). */
  forbiddenParams?: string[];
}

type Tok = { k: 'num' | 'id' | 'op' | 'lp' | 'rp' | 'comma'; s: string; at: number };

const NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
export const isName = (s: string): boolean => NAME_RE.test(s);

function describeChar(ch: string): string {
  if (ch === '\n' || ch === '\r') return 'a line break';
  if (ch === '\t') return 'a tab';
  if (ch === '`') return 'a backtick';
  if (ch === ';') return 'a semicolon';
  if (ch === '"' || ch === "'") return 'a quotation mark (no strings are allowed)';
  if (ch === '=') return "'=' (write only the expression, not an equation)";
  if ('[]{}'.includes(ch)) return `'${ch}' (indexing and lists are not allowed)`;
  return `'${ch}'`;
}

function tokenize(src: string): Tok[] | string {
  const out: Tok[] = [];
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === ' ') {
      i++;
      continue;
    }
    if (/[0-9]/.test(ch) || (ch === '.' && /[0-9]/.test(src[i + 1] ?? ''))) {
      const m = /^(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/.exec(src.slice(i))!;
      out.push({ k: 'num', s: m[0], at: i });
      i += m[0].length;
      continue;
    }
    if (/[A-Za-z_]/.test(ch)) {
      const m = /^[A-Za-z_][A-Za-z0-9_]*/.exec(src.slice(i))!;
      out.push({ k: 'id', s: m[0], at: i });
      i += m[0].length;
      continue;
    }
    if (ch === '.' && '*/^'.includes(src[i + 1] ?? '#')) {
      // MATLAB-style elementwise operators mean the same as the plain ones here.
      out.push({ k: 'op', s: src[i + 1], at: i });
      i += 2;
      continue;
    }
    if ('+-*/^'.includes(ch)) {
      if (ch === '*' && src[i + 1] === '*') return "'**' is not an operator in Qu: write ^ for powers";
      out.push({ k: 'op', s: ch, at: i });
      i++;
      continue;
    }
    if (ch === '(') out.push({ k: 'lp', s: ch, at: i });
    else if (ch === ')') out.push({ k: 'rp', s: ch, at: i });
    else if (ch === ',') out.push({ k: 'comma', s: ch, at: i });
    else return `Unexpected ${describeChar(ch)}: only numbers, names, + - * / ^, parentheses and a few math functions are allowed.`;
    i++;
  }
  return out;
}

/** Removes a leading `y =` / `f(x) =` a user may type out of habit. */
export function stripLhs(src: string): string {
  const m = /^\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*(?:\([A-Za-z_, ]*\))?)\s*=(?!=)\s*(.*)$/s.exec(src);
  return m && !src.slice(src.indexOf('=') + 1).includes('=') ? m[1] : src;
}

export function parseExpr(raw: string, opt: ParseOptions): ParseResult {
  const src = raw;
  if (src.trim() === '') return { ok: false, error: 'The expression is empty.' };
  if (src.length > MAX_EXPR_LENGTH) return { ok: false, error: `The expression is longer than ${MAX_EXPR_LENGTH} characters.` };
  const toks = tokenize(src);
  if (typeof toks === 'string') return { ok: false, error: toks };
  const vars: string[] = [];
  const params: string[] = [];
  const extra = new Set(opt.extraNames ?? []);
  let pos = 0;
  let depth = 0;
  const fail = (m: string): never => {
    throw new Error(m);
  };
  const peek = () => toks[pos];
  const classify = (name: string, isCall: boolean): void => {
    if (isCall) {
      if (!(name in MATH_FUNCS)) {
        fail(`Unknown function '${name}'. Allowed: ${Object.keys(MATH_FUNCS).join(', ')}.`);
      }
      return;
    }
    if (RESERVED.includes(name)) fail(`'${name}' is a reserved word in Qu and cannot be used here.`);
    if (CONSTANTS.includes(name)) return;
    if (name in MATH_FUNCS) fail(`'${name}' is a function: write ${name}(...), or choose another parameter name.`);
    if (opt.vars.includes(name)) {
      if (!vars.includes(name)) vars.push(name);
      return;
    }
    if (extra.has(name)) return;
    if (!opt.allowParams) fail(`Unknown name '${name}'. Allowed here: ${[...opt.vars, ...extra].join(', ') || 'numbers only'}.`);
    if ((opt.forbiddenParams ?? []).includes(name)) fail(`'${name}' cannot be a parameter name here.`);
    if (name.startsWith('cf') || name.startsWith('d_') || name === 'wt' || name === 'rw') {
      fail(`'${name}' is reserved for the generated script's own variables: rename the parameter.`);
    }
    if (!params.includes(name)) params.push(name);
  };
  const atom = (): Node => {
    const t = peek();
    if (!t) return fail('The expression ends too early: something is missing at the end.');
    if (t.k === 'num') {
      pos++;
      const nx = peek();
      if (nx && (nx.k === 'num' || nx.k === 'id' || nx.k === 'lp')) fail(`Missing operator after ${t.s}: write ${t.s}*${nx.s}.`);
      return { t: 'num', v: t.s };
    }
    if (t.k === 'id') {
      pos++;
      const nx = peek();
      if (nx && nx.k === 'lp') {
        classify(t.s, true);
        pos++;
        const args: Node[] = [];
        if (peek()?.k === 'rp') fail(`${t.s}() needs an argument.`);
        for (;;) {
          depth++;
          if (depth > 40) fail('The expression is nested too deeply.');
          args.push(expr());
          depth--;
          const c = peek();
          if (c?.k === 'comma') {
            pos++;
            continue;
          }
          if (c?.k === 'rp') {
            pos++;
            break;
          }
          fail(`Missing closing parenthesis in the call to ${t.s}.`);
        }
        if (args.length !== MATH_FUNCS[t.s]) fail(`${t.s} takes ${MATH_FUNCS[t.s]} argument${MATH_FUNCS[t.s] > 1 ? 's' : ''}, not ${args.length}.`);
        return { t: 'call', f: t.s, args };
      }
      classify(t.s, false);
      if (nx && (nx.k === 'num' || nx.k === 'id')) fail(`Missing operator between ${t.s} and ${nx.s}: write ${t.s}*${nx.s}.`);
      return { t: 'id', n: t.s };
    }
    if (t.k === 'lp') {
      pos++;
      depth++;
      if (depth > 40) fail('The expression is nested too deeply.');
      if (peek()?.k === 'rp') fail('Empty parentheses ().');
      const e = expr();
      depth--;
      if (peek()?.k !== 'rp') fail('Missing closing parenthesis ).');
      pos++;
      const nx = peek();
      if (nx && (nx.k === 'num' || nx.k === 'id' || nx.k === 'lp')) fail(`Missing operator after ): write * before ${nx.s}.`);
      return e;
    }
    if (t.k === 'rp') return fail('Unexpected closing parenthesis ).');
    if (t.k === 'comma') return fail("Unexpected ',' outside a function call.");
    return fail(`Unexpected '${t.s}': an operand (a number or a name) is expected here.`);
  };
  const unary = (): Node => {
    const t = peek();
    if (t?.k === 'op' && (t.s === '-' || t.s === '+')) {
      pos++;
      return { t: 'un', op: t.s, a: unary() };
    }
    return power();
  };
  const power = (): Node => {
    const base = atom();
    const t = peek();
    if (t?.k === 'op' && t.s === '^') {
      pos++;
      return { t: 'bin', op: '^', a: base, b: unary() };
    }
    return base;
  };
  const term = (): Node => {
    let a = unary();
    for (;;) {
      const t = peek();
      if (t?.k === 'op' && (t.s === '*' || t.s === '/')) {
        pos++;
        a = { t: 'bin', op: t.s, a, b: unary() };
      } else return a;
    }
  };
  function expr(): Node {
    let a = term();
    for (;;) {
      const t = peek();
      if (t?.k === 'op' && (t.s === '+' || t.s === '-')) {
        pos++;
        a = { t: 'bin', op: t.s, a, b: term() };
      } else return a;
    }
  }
  try {
    const ast = expr();
    if (pos < toks.length) {
      const t = toks[pos];
      fail(t.k === 'rp' ? 'Unexpected closing parenthesis ).' : `Unexpected '${t.s}' after the end of the expression.`);
    }
    return { ok: true, ast, vars, params };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
}

/** Qu text for an AST, rebuilt token by token (never the user's own text).
 *  Precedence in Qu: + - < * / < unary minus < ^ (so -2^2 is -4), ^ right-associative. */
export function emitExpr(n: Node, rename: Record<string, string> = {}): string {
  const go = (m: Node): [string, number] => {
    const wrap = (c: Node, need: number): string => {
      const [s, p] = go(c);
      return p < need ? `(${s})` : s;
    };
    switch (m.t) {
      case 'num':
        return [m.v, 5];
      case 'id':
        return [rename[m.n] ?? m.n, 5];
      case 'call':
        return [`${m.f}(${m.args.map((a) => go(a)[0]).join(', ')})`, 5];
      case 'un':
        return [`${m.op}${wrap(m.a, 3.5)}`, 3];
      case 'bin':
        if (m.op === '+' || m.op === '-') return [`${wrap(m.a, 1)} ${m.op} ${wrap(m.b, 2)}`, 1];
        if (m.op === '*' || m.op === '/') return [`${wrap(m.a, 2)} ${m.op} ${wrap(m.b, 3.5)}`, 2];
        return [`${wrap(m.a, 4.5)}^${wrap(m.b, 4)}`, 4];
    }
  };
  return go(n)[0];
}

/** Numeric value of a variable-free subtree (JS math), or null. */
export function evalConst(n: Node): number | null {
  switch (n.t) {
    case 'num':
      return Number(n.v);
    case 'id':
      return n.n === 'pi' ? Math.PI : n.n === 'e' ? Math.E : null;
    case 'un': {
      const a = evalConst(n.a);
      return a == null ? null : n.op === '-' ? -a : a;
    }
    case 'bin': {
      const a = evalConst(n.a);
      const b = evalConst(n.b);
      if (a == null || b == null) return null;
      return n.op === '+' ? a + b : n.op === '-' ? a - b : n.op === '*' ? a * b : n.op === '/' ? a / b : Math.pow(a, b);
    }
    case 'call': {
      const a = n.args.map(evalConst);
      if (a.some((v) => v == null)) return null;
      const v = a as number[];
      const f: Record<string, (...z: number[]) => number> = {
        sin: Math.sin, cos: Math.cos, tan: Math.tan, asin: Math.asin, acos: Math.acos, atan: Math.atan, atan2: Math.atan2,
        sinh: Math.sinh, cosh: Math.cosh, tanh: Math.tanh, exp: Math.exp, log: Math.log, log10: Math.log10, log2: Math.log2,
        sqrt: Math.sqrt, cbrt: Math.cbrt, abs: Math.abs, sign: Math.sign, floor: Math.floor, ceil: Math.ceil, round: Math.round,
        min: Math.min, max: Math.max,
      };
      return f[n.f] ? f[n.f](...v) : null;
    }
  }
}

/** Coefficients (highest power first) when `n` is certainly a polynomial in
 *  `v` with numeric coefficients; null otherwise. Conservative: any name other
 *  than `v`, any function call, a non-integer or negative power, or division by
 *  something that depends on `v` gives null (no guessing). */
export function polynomialCoeffs(n: Node, v: string): number[] | null {
  const MAXDEG = 30;
  const add = (a: number[], b: number[], s: number) => {
    const out = Array(Math.max(a.length, b.length)).fill(0);
    a.forEach((c, i) => (out[i] += c));
    b.forEach((c, i) => (out[i] += s * c));
    return out;
  };
  const mul = (a: number[], b: number[]) => {
    const out = Array(a.length + b.length - 1).fill(0);
    a.forEach((c, i) => b.forEach((d, j) => (out[i + j] += c * d)));
    return out;
  };
  const go = (m: Node): number[] | null => {
    switch (m.t) {
      case 'num':
        return [Number(m.v)];
      case 'id':
        if (m.n === v) return [0, 1];
        if (m.n === 'pi') return [Math.PI];
        if (m.n === 'e') return [Math.E];
        return null;
      case 'un': {
        const a = go(m.a);
        return a && (m.op === '-' ? a.map((c) => -c) : a);
      }
      case 'call': {
        const c = evalConst(m);
        return c == null ? null : [c];
      }
      case 'bin': {
        const a = go(m.a);
        if (!a) return null;
        if (m.op === '^') {
          const e = evalConst(m.b);
          if (e == null || !Number.isInteger(e) || e < 0 || e > MAXDEG) return null;
          let r = [1];
          for (let i = 0; i < e; i++) r = mul(r, a);
          return r.length > MAXDEG + 1 ? null : r;
        }
        const b = go(m.b);
        if (!b) return null;
        if (m.op === '+') return add(a, b, 1);
        if (m.op === '-') return add(a, b, -1);
        if (m.op === '*') return mul(a, b);
        if (b.length === 1 && b[0] !== 0) return a.map((c) => c / b[0]);
        return null;
      }
    }
  };
  const asc = go(n);
  if (!asc) return null;
  let hi = asc.length - 1;
  while (hi > 0 && asc[hi] === 0) hi--;
  const coeffs = asc.slice(0, hi + 1).reverse();
  return coeffs.every(Number.isFinite) ? coeffs : null;
}

export interface LinearForm {
  c: number;
  a: number[];
}

/** `c + sum(a[i]*vars[i])` when `n` is exactly linear in `vars`, else null. */
export function linearForm(n: Node, vars: string[]): LinearForm | null {
  const zero = (): LinearForm => ({ c: 0, a: vars.map(() => 0) });
  const isConst = (f: LinearForm) => f.a.every((v) => v === 0);
  const go = (m: Node): LinearForm | null => {
    switch (m.t) {
      case 'num':
        return { ...zero(), c: Number(m.v) };
      case 'id': {
        const i = vars.indexOf(m.n);
        if (i >= 0) {
          const f = zero();
          f.a[i] = 1;
          return f;
        }
        const k = evalConst(m);
        return k == null ? null : { ...zero(), c: k };
      }
      case 'un': {
        const f = go(m.a);
        return f && (m.op === '-' ? { c: -f.c, a: f.a.map((v) => -v) } : f);
      }
      case 'call': {
        const k = evalConst(m);
        return k == null ? null : { ...zero(), c: k };
      }
      case 'bin': {
        const f = go(m.a);
        const g = go(m.b);
        if (!f || !g) return null;
        if (m.op === '+' || m.op === '-') {
          const s = m.op === '+' ? 1 : -1;
          return { c: f.c + s * g.c, a: f.a.map((v, i) => v + s * g.a[i]) };
        }
        if (m.op === '*') {
          if (isConst(g)) return { c: f.c * g.c, a: f.a.map((v) => v * g.c) };
          if (isConst(f)) return { c: g.c * f.c, a: g.a.map((v) => v * f.c) };
          return null;
        }
        if (m.op === '/') {
          return isConst(g) && g.c !== 0 ? { c: f.c / g.c, a: f.a.map((v) => v / g.c) } : null;
        }
        if (isConst(f) && isConst(g)) return { ...zero(), c: Math.pow(f.c, g.c) };
        if (isConst(g) && g.c === 1) return f;
        if (isConst(g) && g.c === 0) return { ...zero(), c: 1 };
        return null;
      }
    }
  };
  const r = go(n);
  return r && Number.isFinite(r.c) && r.a.every(Number.isFinite) ? r : null;
}

export type EquationSplit = { ok: true; lhs: string; rhs: string | null } | { ok: false; error: string };

/** Splits `f(x) = g(x)` at its single `=`; a line without `=` is `f(x)` alone. */
export function splitEquation(src: string): EquationSplit {
  const parts = src.split('=');
  if (parts.length > 2) return { ok: false, error: "More than one '=' in one equation." };
  if (parts.length === 2) {
    if (parts[0].trim() === '' || parts[1].trim() === '') return { ok: false, error: "An equation needs an expression on both sides of '='." };
    return { ok: true, lhs: parts[0], rhs: parts[1] };
  }
  return { ok: true, lhs: parts[0], rhs: null };
}

/** The unknowns an equation list mentions, in order of appearance, for a system. */
export function collectNames(srcs: string[]): { names: string[]; error?: string } {
  const names: string[] = [];
  for (const s of srcs) {
    const toks = tokenize(s);
    if (typeof toks === 'string') return { names, error: toks };
    toks.forEach((t, i) => {
      if (t.k !== 'id') return;
      const call = toks[i + 1]?.k === 'lp';
      if (call || t.s in MATH_FUNCS || CONSTANTS.includes(t.s)) return;
      if (!names.includes(t.s)) names.push(t.s);
    });
  }
  return { names };
}
