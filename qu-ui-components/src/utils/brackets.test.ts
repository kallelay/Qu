import { describe, expect, it } from 'vitest';
import { findUnclosedBracket } from './brackets';

describe('findUnclosedBracket', () => {
  it('finds the open paren on an earlier line, not where the parser noticed', () => {
    const r = findUnclosedBracket('x = 1\ny = (2 +\nz = 3\n');
    expect(r).toEqual({ line: 2, column: 5, open: '(', close: ')' });
  });

  it('reports the innermost bracket still open', () => {
    const r = findUnclosedBracket('f(a, [1, 2\n');
    expect(r).toMatchObject({ line: 1, column: 6, open: '[' });
  });

  it('returns null for balanced source, including nested and mixed brackets', () => {
    expect(findUnclosedBracket('a = f(g([1, 2], {x: 3}))\n')).toBeNull();
    expect(findUnclosedBracket('')).toBeNull();
  });

  it('ignores brackets in comments and strings', () => {
    expect(findUnclosedBracket('x = 1 # ( not a bracket\ns = "(" + \')\'\n')).toBeNull();
  });

  it('treats a quote after a value as transpose, not a string start', () => {
    // the ' after b is transpose; the real unclosed paren is the one on line 2
    const r = findUnclosedBracket("a = b'\nc = (d'\n");
    expect(r).toMatchObject({ line: 2, column: 5, open: '(' });
  });

  it('does not let a stray closer cancel an opener of another kind', () => {
    expect(findUnclosedBracket('(1 ]\n')).toMatchObject({ line: 1, column: 1, open: '(' });
  });
});
