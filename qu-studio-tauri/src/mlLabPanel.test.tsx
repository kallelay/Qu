// A render smoke test for the panel: server-side render (no DOM needed) with
// canned run output, to prove every card draws and nothing throws on the
// shapes the generated script prints. The real UI check is done by hand.
import React from 'react';
import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { MlLabPanel, useMlLab } from './MlLabPanel';
import { parseMlOutput, specForExample, withTask } from './mlLabDesign';
import { exampleById, exampleMeta } from './mlLabData';

// zustand's server render reads the INITIAL state object, so the test writes into that one too.
const put = (patch: Record<string, unknown>) => {
  useMlLab.setState(patch);
  Object.assign(useMlLab.getInitialState() as object, patch);
};

const render = () =>
  renderToString(
    <MlLabPanel
      theme="dark"
      invoke={async () => ({}) as never}
      templates={[{ id: 't1', name: 'Linear Regression', category: 'supervised', template: '# t' }]}
      variables={[{ name: 'M', type: 'matrix', shape: [3, 2], data: [1, 2, 3, 4, 5, 6] }]}
      onOpenInEditor={() => undefined}
    />,
  );

const ex = (id: string) => specForExample(id, exampleMeta(exampleById(id)!));

describe('MlLabPanel', () => {
  it('renders the first screen: data, columns, model, script and the Examples menu', () => {
    put({ spec: ex('blobs2'), parsed: null, state: 'idle', results: parseMlOutput(''), edited: null });
    const html = render();
    expect(html).toContain('Machine learning lab');
    expect(html).toContain('Two Gaussian blobs');
    expect(html).toContain('Linear Regression'); // an old template reachable from the Examples menu
    expect(html).toContain('stratified_split');
    expect(html).toContain('data-testid="ml-run"');
    expect(html).not.toContain('data-testid="ml-problem"');
  });

  it('shows the reason a spec cannot run instead of a script', () => {
    put({ spec: { ...ex('blobs2'), features: ['x1', 'x2', 'label'] }, edited: null });
    const html = render();
    expect(html).toContain('data-testid="ml-problem"');
    expect(html).toContain('also in the features');
    expect(html).toContain('Fix the setup on the left');
  });

  it('draws every classification result card from a canned run', () => {
    const out = [
      '@ml data 200 200 2', '@ml split 150 50', '@ml class_counts 100 100',
      '@ml metric accuracy 0.96', '@ml metric roc_auc 0.99', '@ml metric avg_precision 0.98',
      '@ml cm_row 0 24 1', '@ml cm_row 1 1 24',
      '@ml roc_fpr 0 0.1 1', '@ml roc_tpr 0 0.9 1', '@ml pr_recall 1 0.9 0', '@ml pr_precision 0.5 0.9 1',
      '@ml importance 0.4 0.1', '@ml lc_n 20 40 60', '@ml lc_train 1 1 1', '@ml lc_test 0.8 0.9 0.95',
      '@ml epoch 5 0.7 0.8 0.5 0.4', '@ml epoch 10 0.4 0.5 0.8 0.7', '@ml stop 10 5',
      '@ml grid 0.9 0.01 3', '@ml best 0.9 3', '@ml note only labels',
    ].join('\n');
    put({ spec: ex('blobs2'), ranSpec: ex('blobs2'), state: 'done', results: parseMlOutput(out), startedAt: 1, endedAt: 2, edited: null });
    const html = render();
    for (const id of ['ml-metrics', 'ml-confusion', 'ml-roc', 'ml-pr', 'ml-importance', 'ml-learning-curve', 'ml-loss-curve', 'ml-metric-curve', 'ml-grid', 'ml-notes']) {
      expect(html, id).toContain(`data-testid="${id}"`);
    }
    expect(html).toContain('0.96');
    expect(html).toContain('early stop at epoch 10');
  });

  it('draws regression, compare, clustering, reduction and anomaly results', () => {
    const reg = ex('linreg');
    put({ spec: reg, ranSpec: reg, state: 'done', edited: null, results: parseMlOutput(['@ml metric r2 0.98', '@ml y_true 1 2 3', '@ml y_pred 1.1 1.9 3.2', '@ml residual -0.1 0.1 -0.2'].join('\n')) });
    let html = render();
    expect(html).toContain('data-testid="ml-pred-actual"');
    expect(html).toContain('data-testid="ml-residuals"');

    const cmp = { ...ex('xor'), mode: 'compare' as const, compare: { models: ['knn', 'logistic'], folds: 5, metric: 'accuracy' } };
    put({ spec: cmp, ranSpec: cmp, results: parseMlOutput(['@ml compare knn 0.9 0.02 0.9 0.9', '@ml compare logistic 0.5 0.03 0.5 0.5'].join('\n')) });
    html = render();
    expect(html).toContain('data-testid="ml-compare"');
    expect(html.indexOf('data-model="knn"')).toBeLessThan(html.indexOf('data-model="logistic"'));

    const cl = withTask(ex('blobs3'), 'clustering');
    put({ spec: cl, ranSpec: cl, results: parseMlOutput(['@ml clusters 3 0', '@ml center 0 0 4', '@ml cluster_size 0 80', '@ml scatter_x 0 1 2', '@ml scatter_y 0 1 2', '@ml scatter_c 0 1 2', '@ml sweep_k 2 3 4', '@ml sweep_inertia 9 3 2', '@ml sweep_silhouette 0.5 0.7 0.6'].join('\n')) });
    html = render();
    for (const id of ['ml-clusters', 'ml-elbow', 'ml-silhouette']) expect(html, id).toContain(`data-testid="${id}"`);

    const red = withTask(ex('wide'), 'reduction');
    put({ spec: red, ranSpec: red, results: parseMlOutput(['@ml var_ratio 0.7 0.25 0.05', '@ml cum_var 0.7 0.95 1', '@ml loading_0 0.5 0.5', '@ml scatter_x 1 2', '@ml scatter_y 1 2', '@ml scatter_c 0 1'].join('\n')) });
    html = render();
    expect(html).toContain('data-testid="ml-variance"');
    expect(html).toContain('2 components reach 95%');

    const an = withTask(ex('anomaly'), 'anomaly');
    put({ spec: an, ranSpec: an, results: parseMlOutput(['@ml scores 0.4 0.5 0.9', '@ml scatter_x 1 2 3', '@ml scatter_y 1 2 3', '@ml scatter_c 0 0 1', '@ml top 2 0.9'].join('\n')) });
    html = render();
    for (const id of ['ml-score-hist', 'ml-anomalies']) expect(html, id).toContain(`data-testid="${id}"`);
  });

  it('shows an engine error with the script line it names', () => {
    const s = ex('blobs2');
    put({ spec: s, ranSpec: s, state: 'failed', edited: null, results: parseMlOutput(''), error: 'qu: runtime error: boom\n  at ml_lab.qu:3' });
    const html = render();
    expect(html).toContain('data-testid="ml-error"');
    expect(html).toMatch(/script line (<!-- -->)?3/);
  });

  it('marks the script as detached while it is being edited by hand', () => {
    put({ spec: ex('blobs2'), state: 'idle', error: '', edited: '# my own script\nprint(1)' });
    const html = render();
    expect(html).toContain('data-testid="ml-detached"');
    expect(html).toContain('my own script');
    put({ edited: null });
  });
});
