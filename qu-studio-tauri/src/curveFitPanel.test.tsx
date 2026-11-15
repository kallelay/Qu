import { describe, expect, it, vi } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';

vi.mock('@qu/ui-components', () => ({ PlotViewer: () => React.createElement('div', { 'data-plot': true }) }));

import { CurveFitPanel } from './CurveFitPanel';
import { SolveEquations } from './SolveEquations';

const invoke = (async () => ({ success: true, error: null, data: [], output: '' })) as never;

describe('panels render without a running engine', () => {
  it('the Fit panel renders its first state (built-in example, generated script visible)', () => {
    const html = renderToString(<CurveFitPanel theme="dark" invoke={invoke} />);
    expect(html).toContain('Solve equations');
    expect(html).toContain('least_squares');
    expect(html).toContain('Compare models');
    expect(html).toContain('Exponential decay with noise');
  });

  it('the Solve panel renders its first state with the generated polynomial script', () => {
    const html = renderToString(<SolveEquations theme="light" invoke={invoke} />);
    expect(html).toContain('roots(sv_coef)');
    expect(html).toContain('sv_coef = [2, 3, 0]');
  });
});
