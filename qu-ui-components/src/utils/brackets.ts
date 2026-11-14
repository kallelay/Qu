/**
 * Where is the bracket that was never closed?
 *
 * The parser reports an unclosed `(`, `[` or `{` where it finally notices
 * (usually the end of the file or a later line), which puts the squiggle far
 * from the actual mistake. This scans the source for the innermost bracket
 * still open at the end, so the editor can mark that one instead.
 *
 * Skips `#` comments, "double" strings and 'single' strings; a `'` right
 * after a value (identifier, number, `)`, `]`, `}` or another `'`) is the
 * transpose operator, not a string.
 */

export interface OpenBracket {
  /** 1-based line. */
  line: number;
  /** 1-based column. */
  column: number;
  /** The opening character. */
  open: '(' | '[' | '{';
  /** The closer the parser is waiting for. */
  close: ')' | ']' | '}';
}

const CLOSER: Record<string, OpenBracket['close']> = { '(': ')', '[': ']', '{': '}' };

export function findUnclosedBracket(source: string): OpenBracket | null {
  const stack: OpenBracket[] = [];
  let line = 1;
  let col = 1;
  let prev = ''; // previous non-space character on the current line
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    if (c === '\n') {
      line++;
      col = 1;
      prev = '';
      continue;
    }
    if (c === '#') {
      while (i + 1 < source.length && source[i + 1] !== '\n') i++;
      continue;
    }
    const startCol = col;
    const isString = c === '"' || (c === "'" && !/[A-Za-z0-9_)\]}']/.test(prev));
    if (isString) {
      // consume to the matching quote on this line
      let j = i + 1;
      while (j < source.length && source[j] !== c && source[j] !== '\n') {
        if (source[j] === '\\') j++;
        j++;
      }
      col += j - i + (source[j] === c ? 1 : 0);
      i = source[j] === c ? j : j - 1;
      prev = c;
      continue;
    }
    if (c === '(' || c === '[' || c === '{') {
      stack.push({ line, column: startCol, open: c, close: CLOSER[c] });
    } else if (c === ')' || c === ']' || c === '}') {
      // a closer matches the nearest opener of its own kind
      for (let k = stack.length - 1; k >= 0; k--) {
        if (stack[k].close === c) {
          stack.length = k;
          break;
        }
      }
    }
    if (c !== ' ' && c !== '\t' && c !== '\r') prev = c;
    col++;
  }
  return stack.length ? stack[stack.length - 1] : null;
}
