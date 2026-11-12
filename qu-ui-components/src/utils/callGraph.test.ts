import { describe, expect, it } from 'vitest';
import { blankStringsAndComments, scanCallGraph } from './callGraph';

const src = (...lines: string[]) => lines.join('\n');

describe('scanCallGraph', () => {
  it('links block functions to the defined functions they call, not builtins', () => {
    const g = scanCallGraph(src(
      'function helper(x)',
      '  return sin(x) * 2',
      'end function',
      'function main(v)',
      '  a = helper(v)',
      '  b = helper(a) + max(v)',
      '  return b',
      'end function',
      'r = main([1, 2])',
    ));
    expect(g.functions.map((f) => f.name)).toEqual(['helper', 'main']);
    const [helper, main] = g.functions;
    expect(helper.calls).toEqual([]);
    expect(helper.callers).toEqual(['main']);
    expect(main.calls).toEqual([{ name: 'helper', kind: 'call', line: 5, column: 7, count: 2 }]);
    expect(g.script).toEqual([{ name: 'main', kind: 'call', line: 9, column: 5, count: 1 }]);
  });

  it('reads one-line := functions, their bodies after :=, and recursion', () => {
    const g = scanCallGraph(src('sq(x) := x * x', 'fact(n) := n <= 1 ? 1 : n * fact(n - 1)', 'norm2(v) := sq(v[0]) + sq(v[1])'));
    const by = Object.fromEntries(g.functions.map((f) => [f.name, f]));
    expect(by.fact.recursive).toBe(true);
    expect(by.fact.callers).toEqual([]);
    expect(by.norm2.calls.map((c) => [c.name, c.count, c.column])).toEqual([['sq', 2, 13]]);
    expect(by.sq.callers).toEqual(['norm2']);
    expect(g.script).toEqual([]);
  });

  it('counts a function passed as an argument as a ref edge', () => {
    const g = scanCallGraph(src(
      'cost(p) := sum((p - 3) ^ 2)',
      'function fit()',
      '  return fminsearch(cost, [0])',
      'end',
    ));
    expect(g.functions[1].calls).toEqual([{ name: 'cost', kind: 'ref', line: 3, column: 21, count: 1 }]);
  });

  it('ignores names inside strings and comments, and method calls', () => {
    const g = scanCallGraph(src(
      'function f(x)',
      '  print("f(x) calls g(x)")  # g(1) here is a comment',
      "  s = 'g(2)'",
      '  return x.g(1)',
      'end',
      'function g(x)',
      '  return x',
      'end',
    ));
    expect(g.functions[0].calls).toEqual([]);
    expect(g.functions[1].callers).toEqual([]);
  });

  it('ends a body at its own end, so code after it belongs to the script', () => {
    const g = scanCallGraph(src(
      'function outer()',
      '  if true',
      '    return leaf()',
      '  end',
      '  return 0',
      'end',
      'leaf() := 1',
      'y = outer() + leaf()',
    ));
    const by = Object.fromEntries(g.functions.map((f) => [f.name, f]));
    expect(by.outer.calls.map((c) => c.name)).toEqual(['leaf']);
    expect(by.leaf.callers).toEqual(['outer']);
    expect(g.script.map((c) => c.name)).toEqual(['outer', 'leaf']);
  });

  it('is empty for a file with no functions', () => {
    expect(scanCallGraph('x = sin(1)\nplot(x)')).toEqual({ functions: [], script: [] });
  });
});

describe('blankStringsAndComments', () => {
  it('keeps positions and newlines, treats a quote after a value as transpose', () => {
    const s = 'a = b\' * "x#y" # tail\nc = r"\\d(" + \'q\'';
    const out = blankStringsAndComments(s);
    expect(out.length).toBe(s.length);
    expect(out.split('\n')).toEqual(["a = b' *".padEnd(21), 'c = ' + ' '.repeat(6) + ' + ' + ' '.repeat(3)]);
  });

  it('blanks a triple-quoted string across lines', () => {
    expect(blankStringsAndComments('x = """\nf(1)\n"""\ny')).toBe('x =    \n    \n   \ny');
  });
});
