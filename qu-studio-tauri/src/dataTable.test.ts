import { describe, expect, it } from 'vitest';
import {
  INLINE_ROW_LIMIT, badCellLines, columnStats, contentLines, detectDecimal, detectDelimiter, formatSig, headerLooksPresent, isMissingToken,
  needsStaging, quNumberText, readGrid, scanColumn, splitLine, toNumber,
} from './dataTable';
import { parseTable as fitParse } from './curveFitData';
import { parseTable as mlParse } from './mlLabData';

const grid = (text: string, opts?: { sep?: string; header?: boolean }) => {
  const r = readGrid(text, opts);
  if (!r.ok) throw new Error('empty');
  return r.grid;
};

describe('toNumber', () => {
  it('reads plain, signed, scientific and fractional spellings', () => {
    expect(toNumber('3.5')).toBe(3.5);
    expect(toNumber('1e-3')).toBe(0.001);
    expect(toNumber('1E3')).toBe(1000);
    expect(toNumber('-.5')).toBe(-0.5);
    expect(toNumber('+.5')).toBe(0.5);
    expect(toNumber('5.')).toBe(5);
    expect(toNumber('+7')).toBe(7);
  });
  it('ignores leading and trailing spaces, and rejects empties and text', () => {
    expect(toNumber('  4.25  ')).toBe(4.25);
    expect(toNumber('')).toBeNull();
    expect(toNumber('   ')).toBeNull();
    expect(toNumber('abc')).toBeNull();
    expect(toNumber('1.2.3')).toBeNull();
    expect(toNumber('1 2')).toBeNull();
  });
  it('does not take hex, infinity or NaN for numbers', () => {
    expect(toNumber('0x10')).toBeNull();
    expect(toNumber('inf')).toBeNull();
    expect(toNumber('Infinity')).toBeNull();
    expect(toNumber('NaN')).toBeNull();
    expect(toNumber('1e999')).toBeNull();
  });
  it('under a decimal comma: 1,5 and -,5 and 1,5e2 are numbers', () => {
    expect(toNumber('1,5', ',')).toBe(1.5);
    expect(toNumber('-,5', ',')).toBe(-0.5);
    expect(toNumber('1,5e2', ',')).toBe(150);
    expect(toNumber('7', ',')).toBe(7);
  });
  it('under a decimal comma a dotted thousands group is dropped, but 1.5 stays 1.5', () => {
    expect(toNumber('1.234,5', ',')).toBe(1234.5);
    expect(toNumber('1.234.567', ',')).toBe(1234567);
    expect(toNumber('1.5', ',')).toBe(1.5);
    expect(toNumber('12.5', ',')).toBe(12.5);
  });
  it('more than one decimal comma is not a number; a comma under a decimal point is not either', () => {
    expect(toNumber('1,2,3', ',')).toBeNull();
    expect(toNumber('1,5', '.')).toBeNull();
  });
});

describe('small helpers', () => {
  it('missing tokens', () => {
    for (const t of ['', ' ', 'NA', 'n/a', 'NaN', '?', 'null', ' - ', '.']) expect(isMissingToken(t), t).toBe(true);
    for (const t of ['0', 'x', '1']) expect(isMissingToken(t), t).toBe(false);
  });
  it('number spelling and significant digits', () => {
    expect(quNumberText(1)).toBe('1');
    expect(quNumberText(0.1 + 0.2)).toBe('0.3');
    expect(formatSig(3.14159265, 4)).toBe('3.142');
    expect(formatSig(NaN)).toBe('n/a');
    expect(formatSig(2, 6)).toBe('2');
  });
  it('the inline-or-file rule: 200 rows inline, 201 go to a file', () => {
    expect(INLINE_ROW_LIMIT).toBe(200);
    expect(needsStaging(0)).toBe(false);
    expect(needsStaging(200)).toBe(false);
    expect(needsStaging(201)).toBe(true);
  });
  it('bad-cell lines are offset by where the data starts', () => {
    expect(badCellLines([1, NaN, 3, NaN], 2)).toEqual([3, 5]);
    expect(badCellLines([1, 2], 1)).toEqual([]);
  });
});

describe('splitLine', () => {
  it('splits plain and trims cells', () => {
    expect(splitLine(' a , b ,c', ',')).toEqual(['a', 'b', 'c']);
  });
  it('keeps a separator inside quotes, and doubled quotes', () => {
    expect(splitLine('a,"b,c",d', ',')).toEqual(['a', 'b,c', 'd']);
    expect(splitLine('"c;d";2', ';')).toEqual(['c;d', '2']);
    expect(splitLine('"say ""hi""",2', ',')).toEqual(['say "hi"', '2']);
  });
  it('keeps empty cells', () => {
    expect(splitLine('1,,3,', ',')).toEqual(['1', '', '3', '']);
  });
  it('whitespace mode collapses runs and ignores leading and trailing whitespace', () => {
    expect(splitLine('  1   2\t3  ', ' ')).toEqual(['1', '2', '3']);
  });
});

describe('contentLines', () => {
  it('drops a BOM, CRLF, blank lines and # comments', () => {
    expect(contentLines('﻿# note\r\nx,y\r\n\r\n1,2\r\n  \r\n')).toEqual(['x,y', '1,2']);
  });
});

describe('detectDelimiter', () => {
  it('finds comma, semicolon, tab and pipe', () => {
    expect(detectDelimiter('a,b,c\n1,2,3')).toBe(',');
    expect(detectDelimiter('a;b;c\n1;2;3')).toBe(';');
    expect(detectDelimiter('a\tb\n1\t2')).toBe('\t');
    expect(detectDelimiter('a|b\n1|2')).toBe('|');
  });
  it('a semicolon file full of decimal commas is semicolon-separated, header or not', () => {
    expect(detectDelimiter('1,5;2,5\n3,5;4,5')).toBe(';');
    expect(detectDelimiter('a;b\n1,5;2,5')).toBe(';');
  });
  it('a tab file full of decimal commas is tab-separated', () => {
    expect(detectDelimiter('1,5\t2,5\n3,5\t4,5')).toBe('\t');
  });
  it('a comma inside quotes does not count as a separator', () => {
    expect(detectDelimiter('"a,b";c\n"d,e";f')).toBe(';');
  });
  it('plain whitespace falls back to a space; one column is a comma', () => {
    expect(detectDelimiter('1 2\n3 4')).toBe(' ');
    expect(detectDelimiter('1\n2\n3')).toBe(',');
    expect(detectDelimiter('')).toBe(',');
  });
  it('ignores comment lines when deciding', () => {
    expect(detectDelimiter('# a b c\nx;y\n1;2')).toBe(';');
  });
});

describe('detectDecimal and headerLooksPresent', () => {
  it('a comma separator never has a decimal comma', () => {
    expect(detectDecimal([['1,5']], ',')).toBe('.');
  });
  it('otherwise the more common spelling wins', () => {
    expect(detectDecimal([['1,5', '2,5'], ['3,5', '4']], ';')).toBe(',');
    expect(detectDecimal([['1.5', '2.5']], ';')).toBe('.');
    expect(detectDecimal([['1', '2']], ';')).toBe('.');
  });
  it('a name row above numbers is a header; numbers on top are not', () => {
    expect(headerLooksPresent([['x', 'y'], ['1', '2']], '.')).toBe(true);
    expect(headerLooksPresent([['1', '2'], ['3', '4']], '.')).toBe(false);
    expect(headerLooksPresent([['x', 'y']], '.')).toBe(false);
  });
  it('a header with one numeric-looking name is still a header', () => {
    expect(headerLooksPresent([['a', 'b', '3'], ['1', '2', '3']], '.')).toBe(true);
  });
  it('a label column above text-and-number rows is not mistaken for a header', () => {
    expect(headerLooksPresent([['a', '1'], ['b', '2'], ['c', '3']], '.')).toBe(false);
  });
});

describe('readGrid', () => {
  it('reads a header table', () => {
    const g = grid('x,y\n1,2\n3,4');
    expect(g).toMatchObject({ names: ['x', 'y'], rows: [['1', '2'], ['3', '4']], width: 2, hasHeader: true, sep: ',', decimal: '.', ragged: 0 });
  });
  it('reads a header-less table and names the columns col1..', () => {
    const g = grid('1,2\n3,4\n5,6');
    expect(g.hasHeader).toBe(false);
    expect(g.names).toEqual(['col1', 'col2']);
    expect(g.rows).toHaveLength(3);
  });
  it('reads a single column', () => {
    const g = grid('1\n2\n3');
    expect(g.width).toBe(1);
    expect(g.rows).toEqual([['1'], ['2'], ['3']]);
  });
  it('fills blank header cells with colN', () => {
    expect(grid('a,,c\n1,2,3').names).toEqual(['a', 'col2', 'c']);
  });
  it('survives a BOM, CRLF line ends and trailing blank lines', () => {
    const g = grid('﻿x,y\r\n1,2\r\n3,4\r\n\r\n\r\n');
    expect(g.names).toEqual(['x', 'y']);
    expect(g.rows).toEqual([['1', '2'], ['3', '4']]);
  });
  it('reads semicolons with a decimal comma', () => {
    const g = grid('a;b\n1,5;2,5\n3,5;4,5');
    expect(g.sep).toBe(';');
    expect(g.decimal).toBe(',');
  });
  it('quoted fields may hold the separator', () => {
    const g = grid('name,v\n"a,b",1\n"c;d",2');
    expect(g.rows).toEqual([['a,b', '1'], ['c;d', '2']]);
    expect(g.ragged).toBe(0);
  });
  it('pads short rows, cuts long ones, and counts them', () => {
    const g = grid('a,b,c\n1,2,3\n4,5\n6,7,8,9');
    expect(g.ragged).toBe(2);
    expect(g.rows).toEqual([['1', '2', '3'], ['4', '5', ''], ['6', '7', '8']]);
  });
  it('a ragged header counts as ragged too', () => {
    expect(grid('a,b\n1,2,3\n4,5,6').ragged).toBe(2);
  });
  it('whitespace-separated numbers, with leading blanks and a comment above the header', () => {
    const g = grid('# comment\n  x   y\n  1   2\n  3   4\n');
    expect(g).toMatchObject({ sep: ' ', hasHeader: true, names: ['x', 'y'], rows: [['1', '2'], ['3', '4']] });
  });
  it('honours an explicit separator and header flag', () => {
    const g = grid('a;b\n1;2', { sep: ';', header: false });
    expect(g.hasHeader).toBe(false);
    expect(g.rows).toHaveLength(2);
  });
  it('empty and comment-only text is not ok', () => {
    expect(readGrid('')).toEqual({ ok: false, reason: 'empty' });
    expect(readGrid('  \n\n')).toEqual({ ok: false, reason: 'empty' });
    expect(readGrid('# only a comment')).toEqual({ ok: false, reason: 'empty' });
  });
  it('keeps empty cells as empty strings', () => {
    expect(grid('x,y\n1,\n,2\n3,4').rows).toEqual([['1', ''], ['', '2'], ['3', '4']]);
  });
  it('reads a 5000-row table', () => {
    const lines = ['t,v'];
    for (let i = 0; i < 5000; i++) lines.push(`${i},${(i * 0.5).toFixed(1)}`);
    const g = grid(lines.join('\n'));
    expect(g.rows).toHaveLength(5000);
    expect(g.rows[4999]).toEqual(['4999', '2499.5']);
  });
});

describe('scanColumn', () => {
  it('counts missing, numbers and distinct values', () => {
    const s = scanColumn(['1', '2', '', 'NA', '2', 'x'], '.', 0.95);
    expect(s.missing).toBe(2);
    expect(s.present).toEqual(['1', '2', '2', 'x']);
    expect(s.numericCount).toBe(3);
    expect(s.distinct).toBe(3);
    expect(s.isNumeric).toBe(false);
  });
  it('the numeric threshold is the caller to set', () => {
    const cells = ['1', '2', '3', 'x'];
    expect(scanColumn(cells, '.', 0.6).isNumeric).toBe(true);
    expect(scanColumn(cells, '.', 0.95).isNumeric).toBe(false);
  });
  it('an all-missing column is not numeric', () => {
    expect(scanColumn(['', 'NA', ' '], '.', 0.6).isNumeric).toBe(false);
  });
  it('reads decimal commas', () => {
    expect(scanColumn(['1,5', '2,5'], ',', 0.95).isNumeric).toBe(true);
  });
});

describe('columnStats', () => {
  it('ignores non-finite values', () => {
    const s = columnStats([1, NaN, 3, Infinity]);
    expect(s).toMatchObject({ n: 2, min: 1, max: 3, mean: 2 });
    expect(s.std).toBeCloseTo(Math.SQRT2, 12);
  });
  it('one value has zero spread, none is all NaN', () => {
    expect(columnStats([5])).toEqual({ n: 1, min: 5, max: 5, mean: 5, std: 0 });
    expect(columnStats([NaN]).n).toBe(0);
    expect(Number.isNaN(columnStats([]).mean)).toBe(true);
  });
  it('handles a long column (no spread-argument limit)', () => {
    const v = Array.from({ length: 300_000 }, (_, i) => i);
    expect(columnStats(v)).toMatchObject({ n: 300_000, min: 0, max: 299_999 });
  });
});

// The two table shapes built on the grid agree on every spelling.
describe('both parsers on the shared edge cases', () => {
  const both = (text: string) => ({ fit: fitParse(text), ml: mlParse(text) });

  it('numbers like 1e-3, -.5, +3 and 5. are numeric in both', () => {
    const { fit, ml } = both('x,y\n1e-3,-.5\n2E2,+3\n.5,5.');
    expect(fit.columns[0].values).toEqual([0.001, 200, 0.5]);
    expect(fit.columns[1].values).toEqual([-0.5, 3, 5]);
    expect(ml.meta.columns.map((c) => c.type)).toEqual(['numeric', 'numeric']);
    expect(ml.meta.columns[1].min).toBe(-0.5);
  });

  it('decimal commas under semicolons, with and without a header', () => {
    for (const t of ['a;b\n1,5;2,5\n3,5;4,5', '1,5;2,5\n3,5;4,5']) {
      const { fit, ml } = both(t);
      expect(fit.delimiter).toBe('semicolon');
      expect(fit.columns.map((c) => c.values)).toEqual([[1.5, 3.5], [2.5, 4.5]]);
      expect(ml.meta.sep).toBe(';');
      expect(ml.meta.decimal).toBe(',');
      expect(ml.meta.columns.every((c) => c.type === 'numeric')).toBe(true);
    }
  });

  it('a tab file with decimal commas (the Fit tab used to drop every column)', () => {
    const { fit, ml } = both('a\tb\n1,5\t2,5\n3,5\t4,5');
    expect(fit.columns.map((c) => c.values)).toEqual([[1.5, 3.5], [2.5, 4.5]]);
    expect(ml.meta.decimal).toBe(',');
  });

  it('a dot inside a decimal-comma file is still a decimal point (the ML lab used to read 1.5 as 15)', () => {
    const { fit, ml } = both('a;b\n1.5;2,5\n3,5;4,5');
    expect(fit.columns[0].values).toEqual([1.5, 3.5]);
    expect(ml.meta.columns[0].mean).toBe(2.5);
  });

  it('quoted fields with separators (the Fit tab used to split them)', () => {
    const { fit, ml } = both('name,v\n"a,b",1\n"c;d",2\n"e",3');
    expect(fit.columns.map((c) => c.name)).toEqual(['v']);
    expect(fit.columns[0].values).toEqual([1, 2, 3]);
    expect(fit.problems.join(' ')).not.toMatch(/different number/);
    expect(ml.meta.columns[0].levels).toEqual(['a,b', 'c;d', 'e']);
  });

  it('quoted numbers are numbers', () => {
    expect(fitParse('"x","y"\n"1","2"\n"3","4"').columns.map((c) => c.values)).toEqual([[1, 3], [2, 4]]);
  });

  it('a BOM and CRLF with trailing blank lines', () => {
    const { fit, ml } = both('﻿x,y\r\n1,2\r\n3,4\r\n\r\n');
    expect(fit.columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(fit.rows).toBe(2);
    expect(ml.meta.rows).toBe(2);
    expect(ml.meta.columns[0].name).toBe('x');
  });

  it('a # comment line first (the ML lab used to find no columns)', () => {
    const { fit, ml } = both('# hi\nx,y\n1,2\n3,4');
    expect(fit.header).toBe(true);
    expect(ml.meta.hasHeader).toBe(true);
    expect(ml.meta.rows).toBe(2);
  });

  it('whitespace with leading blanks (the ML lab used to see an empty first column)', () => {
    const { fit, ml } = both('  1   2\n  3   4\n');
    expect(fit.columns).toHaveLength(2);
    expect(ml.meta.columns).toHaveLength(2);
  });

  it('a pipe separator (the Fit tab used to fail)', () => {
    const { fit, ml } = both('a|b\n1|2\n3|4');
    expect(fit.delimiter).toBe('pipe');
    expect(fit.columns.map((c) => c.name)).toEqual(['a', 'b']);
    expect(ml.meta.sep).toBe('|');
  });

  it('a header with one numeric-looking name (the Fit tab used to read it as data)', () => {
    const { fit, ml } = both('a,b,3\n1,2,3\n4,5,6');
    expect(fit.header).toBe(true);
    expect(fit.rows).toBe(2);
    expect(ml.meta.hasHeader).toBe(true);
  });

  it('hex is text, not a number (the Fit tab used to read 0x10 as 16)', () => {
    const { fit } = both('x,y\n0x10,1\n2,2');
    expect(fit.columns.map((c) => c.name)).toEqual(['y']);
  });

  it('ragged rows are reported in each tool words and still read', () => {
    const { fit, ml } = both('a,b,c\n1,2,3\n4,5\n6,7,8,9');
    expect(fit.problems.join(' ')).toMatch(/2 lines have a different number of fields/);
    expect(fit.rows).toBe(3);
    expect(ml.problems.join(' ')).toMatch(/2 lines have a different number of fields/);
    expect(ml.meta.rows).toBe(3);
    expect(ml.meta.columns[2].missing).toBe(1);
  });

  it('leading and trailing spaces around cells', () => {
    const { fit, ml } = both('x , y\n 1 , 2 \n 3 , 4 ');
    expect(fit.columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(fit.columns[0].values).toEqual([1, 3]);
    expect(ml.meta.columns.map((c) => c.type)).toEqual(['numeric', 'numeric']);
  });

  it('empty cells are missing, not poison', () => {
    const { fit, ml } = both('x,y\n1,\n,2\n3,4\n5,6');
    expect(fit.columns[0].values).toEqual([1, NaN, 3, 5]);
    expect(fit.problems.join(' ')).toMatch(/not a number/);
    expect(ml.meta.columns[0].missing).toBe(1);
    expect(ml.meta.columns[0].type).toBe('numeric');
  });

  it('a column that is mostly blank is still numeric in the Fit tab, and its holes are reported', () => {
    const t = fitParse('x,y\n1,\n2,\n3,\n4,9\n5,\n6,');
    expect(t.columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(t.problems.join(' ')).toMatch(/Column 'y': 5 cells are not a number/);
  });

  it('a header-less table', () => {
    const { fit, ml } = both('1,2\n3,4\n5,6');
    expect(fit.header).toBe(false);
    expect(fit.columns.map((c) => c.name)).toEqual(['col1', 'col2']);
    expect(ml.meta.hasHeader).toBe(false);
    expect(ml.meta.columns.map((c) => c.name)).toEqual(['col1', 'col2']);
  });

  it('a single column: the Fit tab says it is too narrow, the ML lab reads it', () => {
    const { fit, ml } = both('1\n2\n3');
    expect(fit.problems.join(' ')).toMatch(/at least two numeric columns/);
    expect(fit.columns).toHaveLength(1);
    expect(ml.meta.columns).toHaveLength(1);
    expect(ml.meta.rows).toBe(3);
  });

  it('a text column: left out by the Fit tab, a category in the ML lab', () => {
    const { fit, ml } = both('id,x,y\na,1,2\nb,2,3\nc,3,4');
    expect(fit.columns.map((c) => c.name)).toEqual(['x', 'y']);
    expect(fit.problems.join(' ')).toMatch(/'id' is not numeric/);
    expect(ml.meta.columns[0].type).toBe('category');
    expect(ml.meta.columns[0].levels).toEqual(['a', 'b', 'c']);
  });

  it('a 5000-row table through both', () => {
    const lines = ['t,v'];
    for (let i = 0; i < 5000; i++) lines.push(`${i},${i % 7}`);
    const { fit, ml } = both(lines.join('\n'));
    expect(fit.rows).toBe(5000);
    expect(fit.columns[0].values[4999]).toBe(4999);
    expect(ml.meta.rows).toBe(5000);
    expect(needsStaging(ml.meta.rows)).toBe(true);
  });
});
