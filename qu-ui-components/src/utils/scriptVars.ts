/** Reading a Qu script for what the author meant its axes to be.
 *
 * A run tells you which variables exist and what shape they are, but not
 * which of them the script is *about*. Sorted alphabetically, a script
 * about `time` and `voltage` will happily offer you `amp` first. The one
 * signal the run throws away is the author's own ordering: someone who
 * writes `x`, then `y`, then `z` has already said what the axes are.
 *
 * So this reads the source, and the pickers combine both -- names from
 * here, types and shapes from the run.
 */

/** Top-level `name = ...` assignments, in the order they appear.
 *
 * Deliberately a line scanner and not a parser: it only has to be right
 * about the shape of a plotting script, and being wrong is survivable (the
 * caller falls back to the run's variable list). A parser here would be
 * more code and more ways to fail on syntax this never needs to understand.
 *
 * NOTE the regex literals. Written as `new RegExp("...\\w...")` the escape
 * has to survive a string literal first, and `"\w"` silently degrades to a
 * plain `w` -- which turns the identifier pattern into "a letter followed
 * by w's", matches almost nothing, and sends every caller down the
 * alphabetical fallback with no error anywhere. That exact bug shipped
 * once; the literals prevent it structurally.
 */
export function assignedNamesInOrder(script: string): string[] {
  const out: string[] = [];
  // `=(?:[^=]|$)` takes `a = 1` and a bare `a =` at end of line, but not
  // the `==` of a comparison.
  const lineRe = /^[ \t]*([A-Za-z_]\w*)[ \t]*=(?:[^=]|$)/;
  for (const line of script.split(/\r?\n/)) {
    const m = lineRe.exec(line);
    if (m && !out.includes(m[1])) out.push(m[1]);
  }
  return out;
}

/** Named constants a slider bound may refer to, matching the engine's own
 *  globals so `2*pi` means in a directive what it means in the script. */
const CONSTANTS: Record<string, number> = {
  pi: Math.PI,
  tau: Math.PI * 2,
  e: Math.E,
};

/** Evaluate a small constant expression, or return `null`.
 *
 * Slider bounds want to be written the way the rest of the script writes
 * numbers -- `# @slider phase 0 2*pi 0.01` is the obvious thing to type,
 * and before this it matched nothing, so the directive was dropped and the
 * slider simply never appeared. Silence for valid-looking input is the
 * worst of the available failure modes.
 *
 * A recursive-descent parser rather than `eval` or `new Function`: this
 * parses text out of a user's script, and handing that to the JS engine to
 * execute would be a code-execution path opened for the sake of arithmetic
 * it can do itself in thirty lines. Supports + - * / , unary minus,
 * parentheses, decimals and the constants above. Anything else is `null`,
 * which the caller treats as "not a slider directive".
 */
export function evalConstExpr(src: string): number | null {
  const tokens = src.match(/\d+(?:\.\d+)?|[A-Za-z_]\w*|[()+\-*/]/g);
  if (!tokens || tokens.join('') !== src.replace(/\s+/g, '')) return null;

  let i = 0;
  const peek = () => tokens[i];

  // expr := term (('+' | '-') term)*
  const expr = (): number | null => {
    let left = term();
    if (left === null) return null;
    while (peek() === '+' || peek() === '-') {
      const op = tokens[i++];
      const right = term();
      if (right === null) return null;
      left = op === '+' ? left + right : left - right;
    }
    return left;
  };

  // term := factor (('*' | '/') factor)*
  const term = (): number | null => {
    let left = factor();
    if (left === null) return null;
    while (peek() === '*' || peek() === '/') {
      const op = tokens[i++];
      const right = factor();
      if (right === null) return null;
      left = op === '*' ? left * right : left / right;
    }
    return left;
  };

  // factor := '-' factor | '(' expr ')' | number | constant
  const factor = (): number | null => {
    const t = tokens[i];
    if (t === undefined) return null;
    if (t === '-') {
      i++;
      const v = factor();
      return v === null ? null : -v;
    }
    if (t === '+') {
      i++;
      return factor();
    }
    if (t === '(') {
      i++;
      const v = expr();
      if (v === null || tokens[i] !== ')') return null;
      i++;
      return v;
    }
    i++;
    if (/^\d/.test(t)) {
      const n = parseFloat(t);
      return Number.isFinite(n) ? n : null;
    }
    const c = CONSTANTS[t.toLowerCase()];
    return c === undefined ? null : c;
  };

  const value = expr();
  // Trailing junk (`2*pi)`) means this was not a well-formed expression,
  // and guessing at the author's intent is worse than declining.
  if (value === null || i !== tokens.length || !Number.isFinite(value)) return null;
  return value;
}

/** Where a figure is headed: a screen, or a printed page. */
export type FigureTarget = 'screen' | 'publication';

// A `theme(...)` call with a quoted argument. `theme` in Qu sets ONE
// figure style at a time -- publication, minimal, classic -- so there is
// at most one of these that matters, and switching target replaces the
// argument rather than adding a second call that would just override it.
const THEME_CALL_RE = /^([ \t]*)theme\s*\(\s*(['"])([^'"]*)\2\s*\)[ \t]*$/m;

/** Read the figure target out of a script.
 *
 * `screen` is the default in the engine, so its absence is not ambiguous:
 * a script with no `theme("publication")` renders for a screen.
 */
export function readFigureTarget(code: string): FigureTarget {
  const m = THEME_CALL_RE.exec(code);
  return m && m[3].toLowerCase() === 'publication' ? 'publication' : 'screen';
}

/** Set the figure target in a script, returning the new source.
 *
 * The target is written into the CODE, not applied to the rendered image,
 * because the engine is what draws the figure -- and because a setting
 * that lives in the file survives being saved, shared and re-run, which a
 * toggle in a panel does not.
 *
 * Returns the input unchanged when it already says the right thing, so a
 * host can compare by identity and skip a no-op edit.
 */
export function setFigureTarget(code: string, target: FigureTarget): string {
  const existing = THEME_CALL_RE.exec(code);

  if (target === 'publication') {
    if (!existing) {
      // Insert after any leading comment block -- a script usually opens
      // with a title comment, and shoving a call above it reads badly.
      const lines = code.split(/\r?\n/);
      let at = 0;
      while (at < lines.length && /^[ \t]*(#.*)?$/.test(lines[at])) at++;
      lines.splice(at, 0, 'theme("publication")');
      return lines.join('\n');
    }
    if (existing[3].toLowerCase() === 'publication') return code;
    // Replace the argument, keeping the author's indentation and quote
    // style rather than reformatting their line.
    return code.replace(
      THEME_CALL_RE,
      (_m, indent: string, quote: string) => `${indent}theme(${quote}publication${quote})`,
    );
  }

  // Screen is the engine default, so it is expressed by NOT asking for
  // publication. Only a publication theme is removed: a script that says
  // `theme("minimal")` has made a different choice, and dropping it here
  // would be editing something the user did not ask to change.
  if (!existing || existing[3].toLowerCase() !== 'publication') return code;
  const lines = code.split(/\r?\n/);
  const idx = lines.findIndex((l) => THEME_CALL_RE.test(l));
  if (idx === -1) return code;
  lines.splice(idx, 1);
  // Leave no double blank line behind.
  if (idx > 0 && idx < lines.length && lines[idx - 1].trim() === '' && lines[idx].trim() === '') {
    lines.splice(idx, 1);
  }
  return lines.join('\n');
}

/** Insert generated plotting lines where they will actually take effect.
 *
 * Order matters in a Qu script: `savefig` writes the figure as it stands,
 * and `show` now ENDS a figure and starts the next one. Appending to the
 * end of the file would put a `vline` after the figure it belongs to had
 * already been written out or closed -- the code would run, report no
 * error, and change nothing visible. That is the worst kind of "it didn't
 * work".
 *
 * So the lines go before the first `savefig`/`show`/`write_report` if
 * there is one, and at the end if there is not.
 */
export function insertPlotCode(code: string, lines: string[]): string {
  if (!lines.length) return code;
  const src = code.split(/\r?\n/);
  // Only a call at the START of a line counts: `# savefig(...)` in a
  // comment, or the word inside a string, is not a call.
  const terminator = /^[ \t]*(savefig|write_report|show)\b/;
  let at = src.findIndex((l) => terminator.test(l));
  if (at === -1) {
    at = src.length;
    // Keep a trailing blank line trailing.
    while (at > 0 && src[at - 1].trim() === "") at--;
  }
  const indent = /^([ \t]*)/.exec(src[Math.min(at, src.length - 1)] ?? "")?.[1] ?? "";
  const block = lines.map((l) => indent + l);
  return [...src.slice(0, at), ...block, ...src.slice(at)].join("\n");
}
