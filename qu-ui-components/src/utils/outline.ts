/**
 * Document outline for a Qu source file: the named places a reader would
 * want to jump to. Framework-free (no Monaco, no React) for the same reason
 * `cells.ts` is -- the scan is unit tested on its own, and both the Monaco
 * document-symbol provider (Ctrl+Shift+O, breadcrumbs) and the Studio
 * sidebar's outline list consume this one function, so they cannot disagree.
 *
 * What counts as a symbol, taken from how Qu is actually written in
 * `catalog/*.qu` rather than from another language's habits:
 *
 *   - `function name(args)` ... `end function`   (block function)
 *   - `name(args) := expr`                        (one-line function)
 *   - `#%% title`                                 (runnable cell marker)
 *   - `# --- title ---`, `# === title ===`        (section comment)
 *
 * This is a line scan, not a parse: it has to keep working on a buffer that
 * does not currently parse, which is most of the time while typing.
 */

export type QuSymbolKind = 'function' | 'cell' | 'section';

export interface QuSymbol {
  kind: QuSymbolKind;
  /** Function name, cell title, or section title. Never empty. */
  name: string;
  /** Parameter list for functions (`(a, b)`), otherwise ''. */
  detail: string;
  /** 1-based line the symbol is declared on. */
  line: number;
  /** 1-based, inclusive last line the symbol covers (>= `line`). */
  endLine: number;
  /** 1-based column where `name` starts on `line`. */
  column: number;
}

const CELL_RE = /^#%%(.*)$/;
// `# --- title`, `# === title ===`, `#--- title ----------`: a comment whose
// text opens with a run of at least three `-` or `=`.
const SECTION_RE = /^\s*#\s*[-=]{3,}\s*(.*?)\s*[-=]*\s*$/;
const BLOCK_FN_RE = /^(\s*)(?:export\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*(\([^)]*\)?)?/;
const INLINE_FN_RE = /^(\s*)([A-Za-z_][A-Za-z0-9_]*)\s*(\([^()]*\))\s*:=/;
const END_FN_RE = /^\s*end\s+function\b/;
// A bare `end` also closes a function (`function f()` ... `end` runs under
// `qu run`), but a bare `end` closes every other block too. Without a full
// block parser the one that closes the function is taken to be the first
// bare `end` at the header's own indentation -- how Qu is laid out.
const BARE_END_RE = /^(\s*)end\s*(?:#.*)?$/;

/** Scan `source` for outline symbols, in document order. */
export function scanOutline(source: string): QuSymbol[] {
  const lines = source.split(/\r\n|\r|\n/);
  const symbols: QuSymbol[] = [];

  for (let i = 0; i < lines.length; i++) {
    const text = lines[i];

    const cell = text.match(CELL_RE);
    if (cell) {
      symbols.push({
        kind: 'cell',
        name: cell[1].trim() || 'Cell',
        detail: '',
        line: i + 1,
        endLine: i + 1,
        column: 1,
      });
      continue;
    }

    const section = text.match(SECTION_RE);
    if (section) {
      // A bare rule (`# ----------`) separates, but names nothing.
      if (section[1]) {
        symbols.push({
          kind: 'section',
          name: section[1],
          detail: '',
          line: i + 1,
          endLine: i + 1,
          column: text.indexOf(section[1]) + 1,
        });
      }
      continue;
    }

    const block = text.match(BLOCK_FN_RE);
    if (block) {
      let end = i;
      for (let j = i + 1; j < lines.length; j++) {
        // Another header before any `end function` means this one is
        // unterminated (mid-edit): do not let it swallow the next function.
        if (BLOCK_FN_RE.test(lines[j])) break;
        if (END_FN_RE.test(lines[j])) {
          end = j;
          break;
        }
        const bare = lines[j].match(BARE_END_RE);
        if (bare && bare[1] === block[1]) {
          end = j;
          break;
        }
      }
      symbols.push({
        kind: 'function',
        name: block[2],
        detail: block[3] ?? '',
        line: i + 1,
        endLine: end + 1,
        // Measured, not searched for: `indexOf('f')` would find the `f`
        // of the keyword `function` itself.
        column: (text.match(/^\s*(?:export\s+)?function\s+/)?.[0].length ?? 0) + 1,
      });
      continue;
    }

    const inline = text.match(INLINE_FN_RE);
    if (inline) {
      symbols.push({
        kind: 'function',
        name: inline[2],
        detail: inline[3],
        line: i + 1,
        endLine: i + 1,
        column: inline[1].length + 1,
      });
    }
  }

  // A cell or section runs until the next cell/section marker (a section
  // also stops at the next cell: cells are the coarser unit), so that
  // breadcrumbs can say which one the caret is inside.
  const lastLine = lines.length;
  for (let k = 0; k < symbols.length; k++) {
    const s = symbols[k];
    if (s.kind === 'function') continue;
    let end = lastLine;
    for (let m = k + 1; m < symbols.length; m++) {
      const next = symbols[m];
      if (next.kind === 'cell' || (s.kind === 'section' && next.kind === 'section')) {
        end = next.line - 1;
        break;
      }
    }
    s.endLine = Math.max(s.line, end);
  }

  return symbols;
}

export interface QuSymbolNode extends QuSymbol {
  children: QuSymbolNode[];
}

/**
 * Nest the flat scan by containment (cell > section > function) -- the
 * shape Monaco's breadcrumbs and outline want. Order is preserved.
 */
export function nestOutline(symbols: QuSymbol[]): QuSymbolNode[] {
  const roots: QuSymbolNode[] = [];
  const stack: QuSymbolNode[] = [];
  for (const s of symbols) {
    const node: QuSymbolNode = { ...s, children: [] };
    while (stack.length && stack[stack.length - 1].endLine < node.line) stack.pop();
    const parent = stack[stack.length - 1];
    // Never let a child outrun its parent: Monaco rejects such trees.
    if (parent && node.endLine > parent.endLine) node.endLine = parent.endLine;
    (parent ? parent.children : roots).push(node);
    stack.push(node);
  }
  return roots;
}
