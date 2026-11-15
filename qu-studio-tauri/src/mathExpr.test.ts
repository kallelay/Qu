import { describe, expect, it } from 'vitest';
import { collectNames, emitExpr, linearForm, parseExpr, polynomialCoeffs, splitEquation, stripLhs } from './mathExpr';

const parse = (s: string, vars = ['x'], allowParams = true) => parseExpr(s, { vars, allowParams });
const ok = (s: string, vars = ['x']) => {
  const r = parse(s, vars);
  if (!r.ok) throw new Error(r.error);
  return r;
};

describe('parseExpr: what is accepted', () => {
  it('finds the parameters in order of appearance and leaves x, constants and functions alone', () => {
    const r = ok('a*exp(-b*x)+c');
    expect(r.params).toEqual(['a', 'b', 'c']);
    expect(r.vars).toEqual(['x']);
    expect(ok('A*sin(w*x+p)+pi*k').params).toEqual(['A', 'w', 'p', 'k']);
    expect(ok('atan2(a, x) + max(b, 2)').params).toEqual(['a', 'b']);
  });

  it('accepts numbers in all spellings', () => {
    for (const s of ['1', '1.5', '.5', '2.', '1e-3*x', '2.5E+4 + x']) expect(parse(s).ok, s).toBe(true);
  });

  it('re-emits from the AST: spacing is normalised, precedence is preserved', () => {
    const e = (s: string) => emitExpr(ok(s).ast);
    expect(e('a  *x+b')).toBe('a * x + b');
    expect(e('(a+b)*x')).toBe('(a + b) * x');
    expect(e('a-(b-c)')).toBe('a - (b - c)');
    expect(e('a/(b*c)')).toBe('a / (b * c)');
    expect(e('2^3^2')).toBe('2^3^2');
    expect(e('(2^3)^2')).toBe('(2^3)^2');
    expect(e('-x^2')).toBe('-x^2');
    expect(e('(-x)^2')).toBe('(-x)^2');
    expect(e('x^-2')).toBe('x^(-2)');
    expect(e('a*-b')).toBe('a * (-b)');
    expect(e('x.^2 .* a')).toBe('x^2 * a');
  });

  it('strips a leading y = or f(x) =', () => {
    expect(stripLhs('y = a*x + b')).toBe('a*x + b');
    expect(stripLhs('f(x)=a*x')).toBe('a*x');
    expect(stripLhs('a*x + b')).toBe('a*x + b');
  });

  it('applies the extra names allowed in a starting value', () => {
    expect(parseExpr('d_ymax / 2', { vars: [], allowParams: false, extraNames: ['d_ymax'] }).ok).toBe(true);
    expect(parseExpr('d_nope', { vars: [], allowParams: false, extraNames: ['d_ymax'] }).ok).toBe(false);
  });
});

describe('parseExpr: syntax errors are explained', () => {
  const err = (s: string, vars = ['x']) => {
    const r = parse(s, vars);
    expect(r.ok, s).toBe(false);
    return r.ok ? '' : r.error;
  };
  it('unbalanced and empty parentheses, dangling operators, missing operators', () => {
    expect(err('a*exp(-b*x')).toMatch(/closing parenthesis/);
    expect(err('a*x)')).toMatch(/closing parenthesis/);
    expect(err('()')).toMatch(/Empty/);
    expect(err('a*')).toMatch(/ends too early/);
    expect(err('*a')).toMatch(/operand/);
    expect(err('2x')).toMatch(/write 2\*x/);
    expect(err('a b')).toMatch(/Missing operator/);
    expect(err('(a)(b)')).toMatch(/Missing operator/);
    expect(err('a**b')).toMatch(/\*\*/);
    expect(err('a,b')).toMatch(/','/);
    expect(err('')).toMatch(/empty/);
    expect(err('exp(a, b)')).toMatch(/takes 1 argument/);
    expect(err('exp()')).toMatch(/needs an argument/);
  });
  it('a function name used as a value, a reserved word as a parameter', () => {
    expect(err('sin + x')).toMatch(/is a function/);
    expect(err('to*x')).toMatch(/reserved/);
    expect(err('step*x + a')).toMatch(/reserved/);
  });
  it('names that would collide with the generated script', () => {
    expect(err('cf1*x')).toMatch(/reserved for the generated/);
    expect(err('d_x0*x')).toMatch(/reserved for the generated/);
    expect(err('wt*x')).toMatch(/reserved for the generated/);
  });
});

describe('parseExpr: injection attempts are refused', () => {
  const attacks: Array<[string, RegExp]> = [
    ['a*x; print(1)', /semicolon/],
    ['a*x\nprint(1)', /line break/],
    ['a*x\r\nend function', /line break/],
    ['a*x end', /reserved|Missing operator/],
    ['a*x`ls`', /backtick/],
    ['system(a)', /Unknown function 'system'/],
    ['a*foo(x)', /Unknown function 'foo'/],
    ['shell("rm -rf /")', /Unknown function|quotation/],
    ['"abc"', /quotation/],
    ["'abc'", /quotation/],
    ['a*x[0]', /indexing/],
    ['{a}', /indexing/],
    ['a = 1', /'='/],
    ['a*x # comment', /'#'/],
    ['a && b', /'&'/],
    ['a || b', /'\|'/],
    ['a:b', /':'/],
    ['a\\b', /'\\'/],
    ['a@b', /'@'/],
    ['a$b', /'\$'/],
    ['function f(x) return x end function', /Unknown function|reserved|Missing operator/],
    ['x + end', /reserved/],
    ['import os', /reserved|Missing operator/],
    ['read_csv(a)', /Unknown function/],
    ['exp(x) % 2', /'%'/],
    ['a\u0000b', /Unexpected/],
    ['\u00e9*x', /Unexpected/],
  ];
  for (const [src, re] of attacks) {
    it(`refuses ${JSON.stringify(src)}`, () => {
      const r = parse(src);
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toMatch(re);
    });
  }
  it('limits the length and the nesting depth', () => {
    expect(parse('a*x+'.repeat(200) + 'b').ok).toBe(false);
    expect(parse('('.repeat(60) + 'x' + ')'.repeat(60)).ok).toBe(false);
  });
});

describe('polynomialCoeffs: certain or null', () => {
  const pc = (s: string) => {
    const r = parseExpr(s, { vars: ['x'], allowParams: true });
    return r.ok ? polynomialCoeffs(r.ast, 'x') : 'parse error';
  };
  it('reads polynomials with + - * ^ and parentheses', () => {
    expect(pc('2*x^2+3*x')).toEqual([2, 3, 0]);
    expect(pc('x^3-6*x^2+11*x-6')).toEqual([1, -6, 11, -6]);
    expect(pc('(x-1)*(x-2)*(x-3)')).toEqual([1, -6, 11, -6]);
    expect(pc('(x+1)^2')).toEqual([1, 2, 1]);
    expect(pc('-x^2 + 4')).toEqual([-1, 0, 4]);
    expect(pc('x/2 + 1')).toEqual([0.5, 1]);
    expect(pc('3')).toEqual([3]);
    expect(pc('x - x + 2*x')).toEqual([2, 0]);
    expect(pc('x^2')).toEqual([1, 0, 0]);
  });
  it('declines anything it is not sure about', () => {
    expect(pc('cos(x) - x')).toBeNull();
    expect(pc('1/x')).toBeNull();
    expect(pc('x^0.5')).toBeNull();
    expect(pc('x^-1')).toBeNull();
    expect(pc('x^x')).toBeNull();
    expect(pc('a*x')).toBeNull();
    expect(pc('exp(x)')).toBeNull();
    expect(pc('x^100')).toBeNull();
    expect(pc('x/(x+1)')).toBeNull();
  });
  it('treats constant calls as numbers', () => {
    expect(pc('sqrt(4)*x + 1')).toEqual([2, 1]);
  });
});

describe('linearForm and equation helpers', () => {
  const lf = (s: string, vars: string[]) => {
    const r = parseExpr(s, { vars, allowParams: false });
    return r.ok ? linearForm(r.ast, vars) : 'parse error';
  };
  it('reads linear forms and refuses products of unknowns', () => {
    expect(lf('2*x + 3*y - 5', ['x', 'y'])).toEqual({ c: -5, a: [2, 3] });
    expect(lf('(x - y)/2', ['x', 'y'])).toEqual({ c: 0, a: [0.5, -0.5] });
    expect(lf('3*(x + 2) - y', ['x', 'y'])).toEqual({ c: 6, a: [3, -1] });
    expect(lf('x*y', ['x', 'y'])).toBeNull();
    expect(lf('x^2', ['x', 'y'])).toBeNull();
    expect(lf('sin(x)', ['x', 'y'])).toBeNull();
    expect(lf('sqrt(4)*x', ['x', 'y'])).toEqual({ c: 0, a: [2, 0] });
  });
  it('splits equations at one =', () => {
    expect(splitEquation('x^2 = 4')).toEqual({ ok: true, lhs: 'x^2 ', rhs: ' 4' });
    expect(splitEquation('x^2')).toEqual({ ok: true, lhs: 'x^2', rhs: null });
    expect(splitEquation('a = b = c').ok).toBe(false);
    expect(splitEquation('= 3').ok).toBe(false);
  });
  it('collects unknown names, not functions or constants', () => {
    expect(collectNames(['x^2 + y^2', 'sin(x) - pi*z']).names).toEqual(['x', 'y', 'z']);
  });
});
