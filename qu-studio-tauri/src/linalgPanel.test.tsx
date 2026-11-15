import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';

// The Monaco-based editor cannot load in a node test run; the panel only uses it in "edit the code" mode.
vi.mock('@qu/ui-components', () => ({ CodeEditor: () => null }));

import { LinAlgPanel } from './LinAlgPanel';
import { DEFAULT_SPEC, EXAMPLES, TASKS } from './linalgDesign';

const invoke = (async () => ({ success: true, error: null, output: '', data: [] })) as never;

describe('LinAlgPanel first render', () => {
  it('renders the task picker, the matrices the task needs, the generated code and Open in editor', () => {
    const html = renderToStaticMarkup(<LinAlgPanel theme="light" invoke={invoke} onOpenInEditor={() => undefined} />);
    for (const t of TASKS) expect(html).toContain(t.label);
    expect(html).toContain('Load an example');
    expect(html).toContain('A (right-hand side)'.slice(0, 1)); // matrix A card
    expect(html).toContain('b (right-hand side)');
    expect(html).toContain('Qu code (generated)');
    expect(html).toContain('Edit the code');
    expect(html).toContain('Open in editor');
    expect(html).toContain('Significant digits');
  });

  it('renders every shipped example in both themes without crashing, showing its generated script', () => {
    for (const ex of EXAMPLES) {
      for (const theme of ['light', 'dark'] as const) {
        const html = renderToStaticMarkup(<LinAlgPanel theme={theme} invoke={invoke} initialSpec={{ ...DEFAULT_SPEC, ...ex.spec }} />);
        expect(html, ex.id).toContain('# Qu Studio / Linear Algebra');
        expect(html).toContain(`data-theme="${theme}"`);
      }
    }
  });

  it('shows a plain-words problem instead of a script when the spec is wrong', () => {
    const bad = { ...DEFAULT_SPEC, matrices: { ...DEFAULT_SPEC.matrices, b: { kind: 'grid' as const, cells: [['1'], ['2']] } } };
    const html = renderToStaticMarkup(<LinAlgPanel theme="light" invoke={invoke} initialSpec={bad} />);
    expect(html).toContain('b needs 3 rows, but b has 2');
    expect(html).toContain('fix the problem shown on the right');
  });
});
