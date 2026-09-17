import { describe, it, expect } from 'vitest';
import { assignedNamesInOrder, evalConstExpr, readFigureTarget, setFigureTarget, insertPlotCode } from './scriptVars';

describe('assignedNamesInOrder', () => {
  it('returns assignments in source order, not alphabetical', () => {
    // The whole reason this function exists. Alphabetically this is
    // current, gain, time, voltage -- which would plot `current` as x.
    expect(
      assignedNamesInOrder(
        ['gain = 2', 'time = linspace(0, 10, 128)', 'voltage = gain * sin(time)', 'current = voltage / 4'].join('\n')
      )
    ).toEqual(['gain', 'time', 'voltage', 'current']);
  });

  it('matches multi-character identifiers', () => {
    // Regression guard. A `\w` that does not survive its string literal
    // turns the pattern into "letter followed by w's", which matches `x`
    // but not `time` -- so a bug here looks like "works in the demo,
    // silently does nothing for real scripts".
    expect(assignedNamesInOrder('time = 1')).toEqual(['time']);
    expect(assignedNamesInOrder('sample_rate_hz = 48000')).toEqual(['sample_rate_hz']);
    expect(assignedNamesInOrder('v2 = 3')).toEqual(['v2']);
  });

  it('ignores comparisons, comments and blank lines', () => {
    expect(assignedNamesInOrder(['a = 1', '# x = 99', 'if b == 2', '', 'c = 3'].join('\n'))).toEqual(['a', 'c']);
  });

  it('handles CRLF and leading indentation', () => {
    expect(assignedNamesInOrder('p = 1\r\n  q = 2\r\n')).toEqual(['p', 'q']);
  });

  it('reports each name once, at its first assignment', () => {
    expect(assignedNamesInOrder('v = 1\nw = 2\nv = 3')).toEqual(['v', 'w']);
  });

  it('accepts an assignment that ends the line', () => {
    expect(assignedNamesInOrder('a =\nb = 2')).toEqual(['a', 'b']);
  });
});

describe('evalConstExpr', () => {
  const near = (got: number | null, want: number) => {
    expect(got).not.toBeNull();
    expect(Math.abs((got as number) - want)).toBeLessThan(1e-12);
  };

  it('parses plain numbers, including negatives and decimals', () => {
    near(evalConstExpr('0'), 0);
    near(evalConstExpr('2.5'), 2.5);
    near(evalConstExpr('-3'), -3);
  });

  it('parses the named constants the engine defines', () => {
    near(evalConstExpr('pi'), Math.PI);
    near(evalConstExpr('tau'), Math.PI * 2);
    near(evalConstExpr('e'), Math.E);
  });

  it('parses the expression that motivated it', () => {
    // `# @slider phase 0 2*pi 0.01` -- previously dropped in silence.
    near(evalConstExpr('2*pi'), Math.PI * 2);
  });

  it('honours precedence and parentheses', () => {
    near(evalConstExpr('1+2*3'), 7);
    near(evalConstExpr('(1+2)*3'), 9);
    near(evalConstExpr('pi/2'), Math.PI / 2);
    near(evalConstExpr('-pi/2'), -Math.PI / 2);
  });

  it('tolerates surrounding and internal whitespace', () => {
    near(evalConstExpr(' 2 * pi '), Math.PI * 2);
  });

  it('rejects malformed input rather than guessing', () => {
    for (const bad of ['', '2*', '*2', '2*pi)', '(1+2', 'nope', '2 pi', '1//2', 'pi..2']) {
      expect(evalConstExpr(bad)).toBeNull();
    }
  });

  it('rejects anything that would need code execution', () => {
    for (const bad of ['alert(1)', 'globalThis', '1;2', 'Math.PI', '[].length']) {
      expect(evalConstExpr(bad)).toBeNull();
    }
  });

  it('rejects division that is not finite', () => {
    expect(evalConstExpr('1/0')).toBeNull();
  });
});

describe('figure target round trip', () => {
  it('reads screen when the script says nothing', () => {
    expect(readFigureTarget('plot(x, y)')).toBe('screen');
  });

  it('reads publication from a theme call', () => {
    expect(readFigureTarget('theme("publication")\nplot(x, y)')).toBe('publication');
    expect(readFigureTarget("theme('publication')")).toBe('publication');
  });

  it('does not mistake another theme for publication', () => {
    expect(readFigureTarget('theme("minimal")')).toBe('screen');
  });

  it('inserts a theme call below the leading comment block', () => {
    const out = setFigureTarget('# FFT Spectrum Analysis\n# second line\nFs = 10000', 'publication');
    expect(out.split('\n')).toEqual([
      '# FFT Spectrum Analysis',
      '# second line',
      'theme("publication")',
      'Fs = 10000',
    ]);
    expect(readFigureTarget(out)).toBe('publication');
  });

  it('replaces another theme rather than stacking a second call', () => {
    const out = setFigureTarget('theme("minimal")\nplot(x)', 'publication');
    expect(out).toBe('theme("publication")\nplot(x)');
    expect(out.match(/theme\(/g)).toHaveLength(1);
  });

  it('keeps the author quote style and indentation', () => {
    expect(setFigureTarget("  theme('classic')", 'publication')).toBe("  theme('publication')");
  });

  it('removes the call when switching back to screen', () => {
    const out = setFigureTarget('theme("publication")\nplot(x)', 'screen');
    expect(out).toBe('plot(x)');
    expect(readFigureTarget(out)).toBe('screen');
  });

  it('leaves a non-publication theme alone when switching to screen', () => {
    // The user chose `minimal` deliberately; screen is the default target
    // and does not require deleting an unrelated style choice.
    expect(setFigureTarget('theme("minimal")\nplot(x)', 'screen')).toBe('theme("minimal")\nplot(x)');
  });

  it('is a no-op when the script already says the right thing', () => {
    const src = 'theme("publication")\nplot(x)';
    expect(setFigureTarget(src, 'publication')).toBe(src);
    const plain = 'plot(x)';
    expect(setFigureTarget(plain, 'screen')).toBe(plain);
  });

  it('survives a publication -> screen -> publication round trip', () => {
    const src = '# title\nplot(x)';
    const pub = setFigureTarget(src, 'publication');
    expect(readFigureTarget(pub)).toBe('publication');
    const back = setFigureTarget(pub, 'screen');
    expect(readFigureTarget(back)).toBe('screen');
    expect(back).toBe(src);
  });
});

describe('insertPlotCode', () => {
  it('inserts before the first savefig, not at the end of the file', () => {
    // Appending would put a `vline` after the figure had already been
    // written out: the code runs, reports nothing, and changes nothing.
    const src = ['x = linspace(0, 10, 50)', 'plot(x, sin(x))', 'savefig("out.svg")'].join('\n');
    expect(insertPlotCode(src, ['vline(5, dash=true)']).split('\n')).toEqual([
      'x = linspace(0, 10, 50)',
      'plot(x, sin(x))',
      'vline(5, dash=true)',
      'savefig("out.svg")',
    ]);
  });

  it('inserts before `show`, which now ends a figure', () => {
    const src = ['plot(x, y)', 'show plot', 'plot(a, b)'].join('\n');
    expect(insertPlotCode(src, ['hline(0)']).split('\n')[1]).toBe('hline(0)');
  });

  it('appends when there is no terminator', () => {
    const src = 'x = 1\nplot(x, x)';
    expect(insertPlotCode(src, ['vline(1)'])).toBe('x = 1\nplot(x, x)\nvline(1)');
  });

  it('ignores a savefig that is only mentioned in a comment', () => {
    const src = ['plot(x, y)', '# savefig("later.svg")', 'z = 2'].join('\n');
    expect(insertPlotCode(src, ['vline(1)']).split('\n')).toEqual([
      'plot(x, y)',
      '# savefig("later.svg")',
      'z = 2',
      'vline(1)',
    ]);
  });

  it('keeps the surrounding indentation', () => {
    const src = ['for i = 0 to 2', '    plot(x, y)', '    savefig("a.svg")', 'end'].join('\n');
    expect(insertPlotCode(src, ['vline(1)']).split('\n')[2]).toBe('    vline(1)');
  });

  it('inserts several lines in order and is a no-op for none', () => {
    const src = 'plot(x, y)';
    expect(insertPlotCode(src, ['vline(1)', 'hline(2)'])).toBe('plot(x, y)\nvline(1)\nhline(2)');
    expect(insertPlotCode(src, [])).toBe(src);
  });

  it('does not leave the insertion after a trailing blank line', () => {
    const src = 'plot(x, y)\n\n';
    expect(insertPlotCode(src, ['vline(1)'])).toBe('plot(x, y)\nvline(1)\n\n');
  });
});
