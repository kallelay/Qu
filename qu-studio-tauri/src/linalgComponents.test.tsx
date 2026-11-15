import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { MatrixTable, ComplexPlane, BarsView } from './MatrixTable';
import { MatrixInput } from './MatrixInput';
import { gridOf, columnGrid, matFromRows, flagsFromVars, indexVars } from './linalgDesign';

describe('MatrixTable', () => {
  it('shows an aligned table with the chosen precision and scientific notation for tiny values', () => {
    const html = renderToStaticMarkup(<MatrixTable m={matFromRows([[1 / 3, 2], [1e-9, 4]])} name="A" precision={3} />);
    expect(html).toContain('0.333');
    expect(html).toContain('1e-9');
    expect(html).toContain('2 x 2');
    expect(html).toContain('Qu literal');
    expect(html).toContain('CSV');
  });
  it('shows complex entries as a + bj', () => {
    const html = renderToStaticMarkup(<MatrixTable m={matFromRows([[1, 2]], [[0, -3]])} name="Z" precision={4} />);
    expect(html).toContain('2 - 3j');
  });
  it('says so for an empty matrix and trims a huge one to a corner', () => {
    expect(renderToStaticMarkup(<MatrixTable m={{ rows: 3, cols: 0, re: [[], [], []] }} name="N" precision={4} />)).toContain('An empty matrix');
    const big = matFromRows(Array.from({ length: 300 }, (_, i) => Array.from({ length: 60 }, (_, j) => i + j)));
    expect(renderToStaticMarkup(<MatrixTable m={big} name="B" precision={4} />)).toContain('showing 200 x 40');
  });
  it('draws eigenvalues and bars without crashing, including non-finite values', () => {
    expect(renderToStaticMarkup(<ComplexPlane re={[0, 0, 5]} im={[2, -2, 0]} />)).toContain('<circle');
    expect(renderToStaticMarkup(<ComplexPlane re={[NaN]} im={[0]} />)).toContain('not finite');
    expect(renderToStaticMarkup(<BarsView values={[5, 1e-6]} label="s" />)).toContain('log scale');
  });
});

describe('MatrixInput', () => {
  const base = { name: 'A' as const, role: 'A', flags: null, resolved: null, running: false, summaryError: '', precision: 4, onChange: () => undefined };
  it('renders the grid editor with one input per cell and the live summary underneath', () => {
    const f = flagsFromVars(indexVars(
      ['m_rows', 'm_cols', 'm_nnz', 'm_density', 'm_lower', 'm_upper', 'm_uband', 'm_lband', 'm_sym', 'm_spd', 'm_rank', 'm_cond', 'm_det', 'm_heavy'].map((name, i) => ({
        name, type: 'number', shape: [1, 1], data: [[2, 2, 4, 1, 0, 0, 1, 1, 1, 1, 2, 3, 5, 0][i]],
      })),
    ))!;
    const html = renderToStaticMarkup(<MatrixInput {...base} value={gridOf([[2, 1], [1, 2]])} flags={f} />);
    expect((html.match(/<input/g) ?? []).length).toBe(4);
    expect(html).toContain('row 2, column 2');
    expect(html).toContain('positive definite');
    expect(html).toContain('condition number');
    expect(html).toContain('Paste / CSV');
    expect(html).toContain('Builder');
  });
  it('flags a bad cell in plain words', () => {
    const html = renderToStaticMarkup(<MatrixInput {...base} value={{ kind: 'grid', cells: [['1', 'oops'], ['3', '4']] }} />);
    expect(html).toContain('row 1, column 2');
  });
  it('shows a builder with its parameters and a data source with a preview', () => {
    expect(renderToStaticMarkup(<MatrixInput {...base} value={{ kind: 'builder', builder: { kind: 'hilbert', n: 8 } }} />)).toContain('Hilbert');
    const data = { kind: 'data' as const, rows: 20, cols: 2, values: Array.from({ length: 40 }, (_, i) => i), origin: 'paste', header: ['x', 'y'] };
    const html = renderToStaticMarkup(<MatrixInput {...base} value={data} />);
    expect(html).toContain('header row detected');
    expect(html).toContain('<th');
  });
  it('a column vector source renders as a one-column grid', () => {
    const html = renderToStaticMarkup(<MatrixInput {...base} name="b" role="b" value={columnGrid([1, 2, 3])} />);
    expect((html.match(/<input/g) ?? []).length).toBe(3);
  });
});
