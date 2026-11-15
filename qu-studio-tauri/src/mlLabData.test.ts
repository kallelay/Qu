import { describe, expect, it } from 'vitest';
import {
  EXAMPLES,
  canonicalCsv,
  detectDelimiter,
  exampleMeta,
  forFile,
  isIdent,
  isMissingToken,
  levelCounts,
  matrixToCsv,
  needsPositional,
  numericHistogram,
  parseTable,
  quNumberText,
  sanitizeIdent,
  splitLine,
  toNumber,
  withType,
} from './mlLabData';

describe('delimited text', () => {
  it('detects the separator', () => {
    expect(detectDelimiter('a,b,c\n1,2,3\n4,5,6')).toBe(',');
    expect(detectDelimiter('a;b;c\n1;2;3')).toBe(';');
    expect(detectDelimiter('a\tb\n1\t2')).toBe('\t');
  });

  it('splits quoted fields', () => {
    expect(splitLine('a,"b,c",d', ',')).toEqual(['a', 'b,c', 'd']);
    expect(splitLine('"say ""hi""",2', ',')).toEqual(['say "hi"', '2']);
  });

  it('reads numbers with a decimal point or comma', () => {
    expect(toNumber('3.5')).toBe(3.5);
    expect(toNumber('1e-3')).toBe(0.001);
    expect(toNumber('3,5', ',')).toBe(3.5);
    expect(toNumber('1.234,5', ',')).toBe(1234.5);
    expect(toNumber('abc')).toBeNull();
    expect(toNumber('')).toBeNull();
  });

  it('spells numbers the way Qu prints them', () => {
    expect(quNumberText(1)).toBe('1');
    expect(quNumberText(0.1 + 0.2)).toBe('0.3');
  });

  it('knows the missing-value tokens', () => {
    for (const t of ['', 'NA', 'n/a', 'NaN', '?', 'null', ' - ']) expect(isMissingToken(t)).toBe(true);
    expect(isMissingToken('0')).toBe(false);
    expect(isMissingToken('x')).toBe(false);
  });
});

describe('parseTable', () => {
  const text = 'height,weight,species\n1.2,3.4,cat\n1.5,,dog\n1.1,2.9,cat\n1.7,4.0,dog\n';

  it('finds the header, types, levels and missing cells', () => {
    const p = parseTable(text);
    expect(p.meta.hasHeader).toBe(true);
    expect(p.meta.rows).toBe(4);
    expect(p.meta.columns.map((c) => c.name)).toEqual(['height', 'weight', 'species']);
    expect(p.meta.columns.map((c) => c.type)).toEqual(['numeric', 'numeric', 'category']);
    expect(p.meta.columns[1].missing).toBe(1);
    expect(p.meta.columns[2].levels).toEqual(['cat', 'dog']);
    expect(p.meta.columns[0].min).toBe(1.1);
    expect(p.meta.columns[0].max).toBe(1.7);
    expect(p.head.length).toBe(4);
  });

  it('works without a header and names the columns col1..', () => {
    const p = parseTable('1,2\n3,4\n5,6');
    expect(p.meta.hasHeader).toBe(false);
    expect(p.meta.columns.map((c) => c.name)).toEqual(['col1', 'col2']);
    expect(p.meta.rows).toBe(3);
  });

  it('reports ragged lines and empty input in plain words', () => {
    const p = parseTable('a,b,c\n1,2,3\n4,5\n6,7,8,9');
    expect(p.problems.join(' ')).toMatch(/different number of fields/);
    expect(parseTable('   \n').problems[0]).toMatch(/empty/);
    // a lone line is data, not a header: validation then says there are too few rows
    expect(parseTable('a,b\n').meta.rows).toBe(1);
  });

  it('treats a mostly-text column as category, free text as ignore, numbers-as-text as numeric', () => {
    const rows = ['id,note,score'];
    for (let i = 0; i < 60; i++) rows.push(`${i},note number ${i},${i % 5}`);
    const p = parseTable(rows.join('\n'));
    expect(p.meta.columns[1].type).toBe('ignore');
    expect(p.meta.columns[2].type).toBe('numeric');
    // a numeric column with few distinct values keeps its levels so it can become a category
    expect(p.meta.columns[2].levels).toEqual(['0', '1', '2', '3', '4']);
  });

  it('makes odd header names safe and unique, and flags the file for positional reading', () => {
    const p = parseTable('"temp C","temp C","class","a b"\n1,2,x,4\n3,4,y,5');
    const idents = p.meta.columns.map((c) => c.ident);
    expect(new Set(idents).size).toBe(4);
    expect(idents.every(isIdent)).toBe(true);
    expect(needsPositional(p.meta)).toBe(true);
    expect(forFile(p).positional).toBe(true);
    expect(forFile(parseTable('a,b\n1,2\n3,4')).positional).toBe(false);
  });

  it('reserved words and digits do not survive as identifiers', () => {
    const used = new Set<string>();
    expect(sanitizeIdent('end', used, 'c1')).toBe('end_');
    expect(sanitizeIdent('2nd', used, 'c2')).toBe('c_2nd');
    expect(sanitizeIdent('', used, 'c3')).toBe('c3');
    expect(isIdent('for')).toBe(false);
    expect(isIdent('nrows')).toBe(false);
  });

  it('flipping a column to category recomputes its levels', () => {
    const p = parseTable('rank,v\n1,5\n2,6\n3,7\n1,8\n');
    const meta = withType(p, p.meta, 'rank', 'category');
    expect(meta.columns[0].type).toBe('category');
    expect(meta.columns[0].levels).toEqual(['1', '2', '3']);
    expect(withType(p, p.meta, 'rank', 'ignore').columns[0].levels).toEqual([]);
  });

  it('counts classes and bins a numeric column', () => {
    const p = parseTable('x,c\n1,a\n2,a\n3,b\n4,a\n');
    expect(levelCounts(p, 'c')).toEqual({ levels: ['a', 'b'], counts: [3, 1] });
    const h = numericHistogram(p, 'x', 4)!;
    expect(h.counts.reduce((a, b) => a + b, 0)).toBe(4);
    expect(h.lo).toBe(1);
    expect(h.hi).toBe(4);
    expect(numericHistogram(p, 'c')).toBeNull();
  });

  it('handles decimal commas with a semicolon separator', () => {
    const p = parseTable('a;b\n1,5;2,5\n3,5;4,5');
    expect(p.meta.sep).toBe(';');
    expect(p.meta.decimal).toBe(',');
    expect(p.meta.columns[0].type).toBe('numeric');
    expect(p.meta.columns[0].mean).toBeCloseTo(2.5, 6);
  });
});

describe('canonicalCsv and matrixToCsv', () => {
  it('writes identifier headers, NaN for missing numbers and keeps text', () => {
    const p = parseTable('Sample id,val,cls\n1,2.5,a\n2,,b\n3,4,a');
    const { text } = canonicalCsv(p);
    const lines = text.split('\n');
    expect(lines[0]).toBe('Sample_id,val,cls');
    expect(lines[2]).toBe('2,NaN,b');
  });

  it('turns decimal commas into points', () => {
    const p = parseTable('a;b\n1,5;x\n2,5;y');
    expect(canonicalCsv(p).text.split('\n')[1]).toBe('1.5,x');
  });

  it('replaces characters that cannot live inside a Qu string literal', () => {
    const p = parseTable('a,b\n1,"x{y}z"\n2,"q\\r"');
    const { text, warnings } = canonicalCsv(p);
    expect(text).not.toMatch(/[{}\\]/);
    expect(warnings.join(' ')).toMatch(/replaced/);
  });

  it('reads a session matrix column-major (the kernel sends it that way)', () => {
    // M = [1,2,3;4,5,6] arrives as shape [2,3], data [1,4,2,5,3,6]
    expect(matrixToCsv([2, 3], [1, 4, 2, 5, 3, 6])).toBe('x1,x2,x3\n1,2,3\n4,5,6');
    expect(matrixToCsv([3, 1], [7, null, 9], 'v')).toBe('v\n7\nNaN\n9');
  });
});

describe('built-in examples', () => {
  it('each has a target that is a real column and levels for category columns', () => {
    for (const e of EXAMPLES) {
      const m = exampleMeta(e);
      expect(m.columns.some((c) => c.name === e.target), e.id).toBe(true);
      expect(m.rows).toBe(e.rows);
      for (const c of m.columns) if (c.type === 'category') expect(c.levels.length, `${e.id}.${c.name}`).toBeGreaterThan(1);
      expect(new Set(m.columns.map((c) => c.ident)).size).toBe(m.columns.length);
      expect(e.code).toContain('ml_t = table(');
    }
  });
});
