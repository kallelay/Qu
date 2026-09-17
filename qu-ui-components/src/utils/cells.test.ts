import { describe, expect, it } from 'vitest';
import { parseCells, findCellAtLine, getPrefixSource } from './cells';

describe('parseCells', () => {
  it('treats a file with no #%% markers as a single untitled cell', () => {
    const source = 'x = 1\ny = 2\nprint x + y';
    const cells = parseCells(source);
    expect(cells).toHaveLength(1);
    expect(cells[0]).toMatchObject({
      index: 0,
      title: '',
      startLine: 1,
      endLine: 3,
    });
    expect(cells[0].code).toBe(source);
  });

  it('splits on #%% markers and captures titles', () => {
    const source = [
      '#%% FFT Spectrum Analysis',
      'Fs = 10000',
      'N = 4096',
      '',
      '#%% Plot results',
      'plot(t, x)',
      'title("Signal")',
    ].join('\n');

    const cells = parseCells(source);
    expect(cells).toHaveLength(2);

    expect(cells[0].title).toBe('FFT Spectrum Analysis');
    expect(cells[0].startLine).toBe(1);
    expect(cells[0].endLine).toBe(4);
    expect(cells[0].code).toBe('#%% FFT Spectrum Analysis\nFs = 10000\nN = 4096\n');

    expect(cells[1].title).toBe('Plot results');
    expect(cells[1].startLine).toBe(5);
    expect(cells[1].endLine).toBe(7);
    expect(cells[1].code).toBe('#%% Plot results\nplot(t, x)\ntitle("Signal")');
  });

  it('gives an implicit untitled leading cell for content before the first marker', () => {
    const source = [
      '# just a regular comment, not a cell marker',
      'import stuff',
      '',
      '#%% Real cell',
      'x = 1',
    ].join('\n');

    const cells = parseCells(source);
    expect(cells).toHaveLength(2);
    expect(cells[0].title).toBe('');
    expect(cells[0].startLine).toBe(1);
    expect(cells[0].endLine).toBe(3);
    expect(cells[1].title).toBe('Real cell');
    expect(cells[1].startLine).toBe(4);
    expect(cells[1].endLine).toBe(5);
  });

  it('gives a bare "#%%" marker with no title an empty title, not undefined', () => {
    const source = '#%%\nx = 1';
    const cells = parseCells(source);
    expect(cells).toHaveLength(1);
    expect(cells[0].title).toBe('');
  });

  it('handles consecutive markers with no code between them (an empty cell)', () => {
    const source = '#%% First\n#%% Second\nx = 1';
    const cells = parseCells(source);
    expect(cells).toHaveLength(2);
    expect(cells[0].code).toBe('#%% First');
    expect(cells[0].startLine).toBe(1);
    expect(cells[0].endLine).toBe(1);
    expect(cells[1].title).toBe('Second');
    expect(cells[1].startLine).toBe(2);
    expect(cells[1].endLine).toBe(3);
  });

  it('does not treat an indented "#%%" or a mid-line "#%%" as a marker', () => {
    // Spec is `^#%%.*$` -- anchored to the start of the line.
    const source = '  #%% not a marker (indented)\nx = 1 # #%% also not a marker (not at line start)';
    const cells = parseCells(source);
    expect(cells).toHaveLength(1);
    expect(cells[0].title).toBe('');
  });
});

describe('findCellAtLine', () => {
  const source = '#%% A\nx = 1\ny = 2\n#%% B\nz = 3';
  const cells = parseCells(source);

  it('finds the cell containing a given line', () => {
    expect(findCellAtLine(cells, 1)?.title).toBe('A');
    expect(findCellAtLine(cells, 3)?.title).toBe('A');
    expect(findCellAtLine(cells, 4)?.title).toBe('B');
    expect(findCellAtLine(cells, 5)?.title).toBe('B');
  });

  it('returns undefined for an out-of-range line', () => {
    expect(findCellAtLine(cells, 999)).toBeUndefined();
    expect(findCellAtLine(cells, 0)).toBeUndefined();
  });
});

describe('getPrefixSource (state-persistence semantics)', () => {
  // Documents and proves the chosen behavior: since `execute_code` spawns a
  // fresh `qu.exe run <tempfile>` process per call (no persistent REPL),
  // "Run Cell" for cell N re-executes cells [0..N] concatenated, so that
  // cell N's process actually sees variables earlier cells defined. This
  // is a deliberate re-execution, not a real incremental kernel.
  const source = [
    '#%% Setup',
    'x = 1',
    '',
    '#%% Depends on Setup',
    'y = x + 1',
    '',
    '#%% Depends on both',
    'z = y + 1',
  ].join('\n');
  const cells = parseCells(source);

  it('running the first cell alone does not leak later cells\' code', () => {
    const prefix = getPrefixSource(source, cells, 0);
    expect(prefix).toBe('#%% Setup\nx = 1\n');
    expect(prefix).not.toContain('y = x + 1');
    expect(prefix).not.toContain('z = y + 1');
  });

  it('running the second cell includes the first cell\'s definitions (state carries forward)', () => {
    const prefix = getPrefixSource(source, cells, 1);
    expect(prefix).toContain('x = 1');
    expect(prefix).toContain('y = x + 1');
    expect(prefix).not.toContain('z = y + 1');
    // Order matters: x must be defined before y uses it when the
    // concatenated script actually runs top-to-bottom.
    expect(prefix.indexOf('x = 1')).toBeLessThan(prefix.indexOf('y = x + 1'));
  });

  it('running the third cell replays all three cells in order', () => {
    const prefix = getPrefixSource(source, cells, 2);
    expect(prefix).toBe(source);
    const xIdx = prefix.indexOf('x = 1');
    const yIdx = prefix.indexOf('y = x + 1');
    const zIdx = prefix.indexOf('z = y + 1');
    expect(xIdx).toBeGreaterThanOrEqual(0);
    expect(xIdx).toBeLessThan(yIdx);
    expect(yIdx).toBeLessThan(zIdx);
  });

  it('rejects an out-of-range cell index', () => {
    expect(() => getPrefixSource(source, cells, 99)).toThrow(RangeError);
  });
});
