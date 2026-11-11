import { describe, expect, it } from 'vitest';
import { parseErrorLocation } from './errorLocation';

const RUNTIME = ['qu: runtime error: `nope` is not defined', '  at e.qu:3 in f()', '  called from e.qu:5 (top level)'].join('\n');

describe('parseErrorLocation', () => {
  it('takes the innermost frame of a runtime trace', () => {
    expect(parseErrorLocation(RUNTIME)).toEqual({
      file: 'e.qu',
      line: 3,
      column: undefined,
      message: 'runtime error: `nope` is not defined',
    });
  });

  it('reads line and column from a parse error', () => {
    expect(parseErrorLocation('qu: parse error at p.qu:4:1: expected `end`')).toEqual({
      file: 'p.qu',
      line: 4,
      column: 1,
      message: 'parse error: expected `end`',
    });
  });

  it('prefers the frame inside the active file over an imported module', () => {
    const trace = ['runtime error: boom', '  at C:\\lib\\helpers.qu:12 in g()', '  called from D:\\work\\main.qu:7 (top level)'].join('\r\n');
    expect(parseErrorLocation(trace, 'D:/work/Main.qu')).toMatchObject({ file: 'D:\\work\\main.qu', line: 7 });
    expect(parseErrorLocation(trace)).toMatchObject({ file: 'C:\\lib\\helpers.qu', line: 12 });
  });

  it('reads the file-less shapes the kernel uses for an unnamed buffer', () => {
    const trace = ['runtime error: boom', '  at line 3 in f()', '  called from line 5 (top level)'].join('\n');
    expect(parseErrorLocation(trace, 'whatever.qu')).toMatchObject({ file: '', line: 3, message: 'runtime error: boom' });
    expect(parseErrorLocation('parse error at 3:1: expected `end`')).toEqual({
      file: '',
      line: 3,
      column: 1,
      message: 'parse error: expected `end`',
    });
  });

  it('reads the parse-error shape `qu kernel` actually returns (probed 2026-11-10)', () => {
    // {"op":"run","file":"D:\\work\\Main.qu","code":"x = 1\nif x\n"} -> this, with no file part.
    expect(parseErrorLocation('runtime error: parse error at 3:1: expected `end`', 'D:\\work\\Main.qu')).toEqual({
      file: '',
      line: 3,
      column: 1,
      message: 'parse error: expected `end`',
    });
  });

  it('returns null when nothing in the trace is in the active file', () => {
    expect(parseErrorLocation(RUNTIME, 'other.qu')).toBeNull();
  });

  it('returns null for an error with no location', () => {
    expect(parseErrorLocation('Stopped.')).toBeNull();
    expect(parseErrorLocation('look at 3:15 for details')).toBeNull();
    expect(parseErrorLocation('')).toBeNull();
  });
});
