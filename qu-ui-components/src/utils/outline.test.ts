import { describe, expect, it } from 'vitest';
import { nestOutline, scanOutline } from './outline';

describe('scanOutline', () => {
  it('finds block functions with their parameter list and extent', () => {
    const src = ['x = 1', 'function damped_sine(x, p)', '  return x', 'end function', 'y = 2'].join('\n');
    expect(scanOutline(src)).toEqual([
      { kind: 'function', name: 'damped_sine', detail: '(x, p)', line: 2, endLine: 4, column: 10 },
    ]);
  });

  it('ends a function at a bare `end` at its own indentation, not at an inner block\'s', () => {
    const src = ['function f(x)', '  if x > 0', '    x = 1', '  end', '  return x', 'end', 'y = f(2)'].join('\n');
    expect(scanOutline(src)).toEqual([
      { kind: 'function', name: 'f', detail: '(x)', line: 1, endLine: 6, column: 10 },
    ]);
  });

  it('finds one-line := functions', () => {
    const [s] = scanOutline('crest(v) := max(abs(v)) / rms(v)');
    expect(s).toMatchObject({ kind: 'function', name: 'crest', detail: '(v)', line: 1, endLine: 1, column: 1 });
  });

  it('does not mistake calls, assignments or `fn = ...` for functions', () => {
    const src = ['fn = length(where(a == 0))', 'print("f(x) := 3")', 'y = f(x)', 'function_count = 3'].join('\n');
    expect(scanOutline(src)).toEqual([]);
  });

  it('finds #%% cells, naming an untitled one', () => {
    const syms = scanOutline('#%% Load data\nx = 1\n#%%\ny = 2');
    expect(syms.map((s) => [s.kind, s.name, s.line, s.endLine])).toEqual([
      ['cell', 'Load data', 1, 2],
      ['cell', 'Cell', 3, 4],
    ]);
  });

  it('finds # --- and # === section comments and strips the trailing rule', () => {
    const src = [
      '# --- biquad lowpass coefficients (bilinear transform) -------------------',
      'a = 1',
      '# === Results ===',
      '# ------------------------',
      '# an ordinary comment',
    ].join('\n');
    const syms = scanOutline(src);
    expect(syms.map((s) => [s.kind, s.name, s.line])).toEqual([
      ['section', 'biquad lowpass coefficients (bilinear transform)', 1],
      ['section', 'Results', 3],
    ]);
    expect(syms[0].endLine).toBe(2);
    expect(syms[1].endLine).toBe(5);
  });

  it('keeps an unterminated function from swallowing the next one', () => {
    const src = ['function a()', '  x = 1', 'function b()', 'end function'].join('\n');
    const syms = scanOutline(src);
    expect(syms.map((s) => [s.name, s.line, s.endLine])).toEqual([
      ['a', 1, 1],
      ['b', 3, 4],
    ]);
  });

  it('handles CRLF and an empty buffer', () => {
    expect(scanOutline('')).toEqual([]);
    expect(scanOutline('#%% A\r\nfunction f()\r\nend function\r\n').map((s) => s.name)).toEqual(['A', 'f']);
  });
});

describe('nestOutline', () => {
  it('nests functions and sections inside the cell that contains them', () => {
    const src = [
      'function top()',
      'end function',
      '#%% One',
      '# --- helpers ---',
      'sq(x) := x * x',
      '#%% Two',
      'function g(a)',
      'end function',
    ].join('\n');
    const tree = nestOutline(scanOutline(src));
    expect(tree.map((n) => n.name)).toEqual(['top', 'One', 'Two']);
    expect(tree[1].children.map((n) => n.name)).toEqual(['helpers']);
    expect(tree[1].children[0].children.map((n) => n.name)).toEqual(['sq']);
    expect(tree[2].children.map((n) => n.name)).toEqual(['g']);
  });
});
