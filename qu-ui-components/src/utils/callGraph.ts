/**
 * Call graph of one Qu source file: which functions the file defines, and
 * which of those each one calls. Framework-free like `outline.ts` (whose
 * function scan it reuses, so the sidebar's Outline and Call graph always
 * agree on what a function is and where it ends).
 *
 * Only calls to functions DEFINED IN THIS FILE are edges: builtins
 * (`sin`, `plot`, ...) would drown the picture and say nothing about the
 * file's own structure. Two kinds of edge:
 *
 *   - `call`: `name(` somewhere in the body;
 *   - `ref`:  the bare name passed as an argument (`fminsearch(cost, x0)`,
 *             `map(f, xs)`), which is how Qu code hands a function to an
 *             optimiser or a higher-order builtin.
 *
 * Calls outside every function body (the script's top level) are listed
 * separately as `script`, so "what does running this file actually start
 * with" is visible too.
 *
 * Strings and comments are blanked before scanning, with the same rules as
 * the lexer (`engine/crates/qu-lexer`): `#` to end of line, `"..."`,
 * `'...'` unless the `'` follows a value (then it is the transpose
 * operator), `"""..."""` across lines, and `r"..."` without escapes. A line
 * scan, not a parse, so it keeps working on a buffer that does not parse.
 */

import { scanOutline } from './outline';

export type CallKind = 'call' | 'ref';

export interface CallSite {
  /** The callee: always a function defined in this file. */
  name: string;
  kind: CallKind;
  /** 1-based line and column of the FIRST such site in the caller. */
  line: number;
  column: number;
  /** How many sites in the caller (calls and refs together). */
  count: number;
}

export interface CallGraphFunction {
  name: string;
  /** 1-based definition line and last line of the body. */
  line: number;
  endLine: number;
  /** Distinct callees, in order of first appearance. */
  calls: CallSite[];
  /** Names of the functions in this file that call or pass this one. */
  callers: string[];
  /** Calls itself (directly). */
  recursive: boolean;
}

export interface CallGraph {
  functions: CallGraphFunction[];
  /** Calls made from top-level script code, outside every function. */
  script: CallSite[];
}

/** `source` with every string literal and comment replaced by spaces, newlines
 *  kept, so line and column positions stay exactly where they were. */
export function blankStringsAndComments(source: string): string {
  const out = source.split('');
  const n = source.length;
  let i = 0;
  // Whether the last significant character ends a value, deciding what a `'`
  // means (lexer's `ends_expr`).
  let endsValue = false;
  const blank = (from: number, to: number) => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== '\n' && out[k] !== '\r') out[k] = ' ';
  };
  while (i < n) {
    const c = source[i];
    if (c === '#') {
      let j = i;
      while (j < n && source[j] !== '\n') j++;
      blank(i, j);
      i = j;
      continue;
    }
    if (c === '"' && source.startsWith('"""', i)) {
      const close = source.indexOf('"""', i + 3);
      const end = close < 0 ? n : close + 3;
      blank(i, end);
      i = end;
      endsValue = true;
      continue;
    }
    const raw = c === 'r' && source[i + 1] === '"' && !/[A-Za-z0-9_]/.test(source[i - 1] ?? '');
    if (raw || c === '"' || (c === "'" && !endsValue)) {
      const quote = raw ? '"' : c;
      let j = raw ? i + 2 : i + 1;
      while (j < n && source[j] !== quote && source[j] !== '\n') {
        if (!raw && source[j] === '\\') j++;
        j++;
      }
      const end = j < n && source[j] === quote ? j + 1 : j;
      blank(i, end);
      i = end;
      endsValue = true;
      continue;
    }
    if (!/\s/.test(c)) endsValue = /[A-Za-z0-9_)\]}'.]/.test(c);
    i++;
  }
  return out.join('');
}

const IDENT_RE = /[A-Za-z_][A-Za-z0-9_]*/g;

/** Build the call graph of `source`. */
export function scanCallGraph(source: string): CallGraph {
  const defs = scanOutline(source).filter((s) => s.kind === 'function');
  const clean = blankStringsAndComments(source).split(/\r\n|\r|\n/);

  // First definition wins for a name defined twice; the Outline still lists both.
  const byName = new Map<string, { name: string; line: number; endLine: number }>();
  for (const d of defs) {
    if (!byName.has(d.name)) byName.set(d.name, { name: d.name, line: d.line, endLine: d.endLine });
  }
  const fns = [...byName.values()];
  const names = new Set(fns.map((f) => f.name));

  // Which function's body a line belongs to: the innermost one containing it.
  // A block function's body is the lines strictly between its header and its
  // `end`; an inline one's is the text after `:=` on its own line.
  const ownerOf = (line: number): string | null => {
    let best: { name: string; line: number } | null = null;
    for (const f of defs) {
      const inside = f.line === f.endLine ? line === f.line : line > f.line && line < f.endLine;
      if (inside && (!best || f.line >= best.line)) best = f;
    }
    return best?.name ?? null;
  };

  const sites = new Map<string | null, Map<string, CallSite>>();
  const add = (owner: string | null, name: string, kind: CallKind, line: number, column: number) => {
    let m = sites.get(owner);
    if (!m) sites.set(owner, (m = new Map()));
    const prev = m.get(name);
    if (prev) {
      prev.count++;
      if (kind === 'call') prev.kind = 'call';
    } else m.set(name, { name, kind, line, column, count: 1 });
  };

  for (let i = 0; i < clean.length; i++) {
    const lineNo = i + 1;
    let text = clean[i];
    let offset = 0;
    const header = defs.find((d) => d.line === lineNo);
    if (header) {
      const assign = text.indexOf(':=');
      if (header.line === header.endLine && assign >= 0) {
        offset = assign + 2;
        text = text.slice(offset);
      } else {
        continue; // a block header: its parameter list calls nothing
      }
    }
    const owner = ownerOf(lineNo);
    IDENT_RE.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = IDENT_RE.exec(text))) {
      const name = m[0];
      if (!names.has(name)) continue;
      const before = text.slice(0, m.index).replace(/\s+$/, '');
      const after = text.slice(m.index + name.length).replace(/^\s+/, '');
      if (before.endsWith('.')) continue; // obj.name(...) is a method, not this function
      const column = offset + m.index + 1;
      if (after.startsWith('(')) {
        add(owner, name, 'call', lineNo, column);
      } else if (/[(,]$/.test(before) && /^[,)]/.test(after)) {
        add(owner, name, 'ref', lineNo, column);
      }
    }
  }

  const functions: CallGraphFunction[] = fns.map((f) => {
    const calls = [...(sites.get(f.name)?.values() ?? [])];
    return { name: f.name, line: f.line, endLine: f.endLine, calls, callers: [], recursive: calls.some((c) => c.name === f.name) };
  });
  const index = new Map(functions.map((f) => [f.name, f]));
  for (const f of functions) {
    for (const c of f.calls) {
      const callee = index.get(c.name);
      if (callee && c.name !== f.name && !callee.callers.includes(f.name)) callee.callers.push(f.name);
    }
  }
  return { functions, script: [...(sites.get(null)?.values() ?? [])] };
}
