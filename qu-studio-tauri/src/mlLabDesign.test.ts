import { describe, expect, it } from 'vitest';
import {
  COMPARE_METRICS,
  DEFAULT_ML,
  MODELS,
  buildMlScript,
  buildPredictScript,
  classLevels,
  defaultSpec,
  emptyResults,
  errorLine,
  featureCols,
  featureNames,
  fitCall,
  ingestMlLine,
  modelById,
  modelsFor,
  parseMlOutput,
  parseRange,
  qnum,
  rankCompare,
  specForExample,
  splitChunk,
  validateMl,
  withTask,
  type MlSpec,
} from './mlLabDesign';
import { EXAMPLES, exampleById, exampleMeta, parseTable } from './mlLabData';

const ex = (id: string, patch: Partial<MlSpec> = {}): MlSpec => ({ ...specForExample(id, exampleMeta(exampleById(id)!)), ...patch });

describe('validateMl', () => {
  it('accepts the default and every example with its own task and model', () => {
    expect(validateMl(DEFAULT_ML)).toBeNull();
    for (const e of EXAMPLES) expect(validateMl(ex(e.id)), e.id).toBeNull();
  });

  it('rejects a target that is also a feature', () => {
    const s = ex('blobs2');
    expect(validateMl({ ...s, features: ['x1', 'x2', 'label'] })).toMatch(/also in the features/);
  });

  it('rejects zero rows and too few rows', () => {
    const s = ex('blobs2');
    expect(validateMl({ ...s, meta: { ...s.meta, rows: 0 } })).toMatch(/zero rows/);
    expect(validateMl({ ...s, meta: { ...s.meta, rows: 5 } })).toMatch(/too few/);
    expect(validateMl({ ...s, meta: { ...s.meta, columns: [] } })).toMatch(/No data/);
  });

  it('rejects a categorical target for regression and a continuous one for classification', () => {
    expect(validateMl(ex('mixed', { task: 'regression', model: 'ols', target: 'segment', features: ['age', 'income'] }))).toMatch(/numeric target/);
    const p = parseTable(['a,y'].concat(Array.from({ length: 80 }, (_, i) => `${i},${i * 1.37}`)).join('\n'));
    const s = defaultSpec({ kind: 'paste', exampleId: '', path: '', varName: '', csv: 'a,y\n1,2', csvRows: 80, tempPath: '' }, p.meta, 'classification', 'y');
    s.features = ['a'];
    s.source.csvRows = 5;
    expect(validateMl(s)).toMatch(/looks continuous/);
  });

  it('checks the class count against binary-only models and one-class targets', () => {
    expect(validateMl(ex('blobs3', { model: 'svm' }))).toMatch(/exactly two classes/);
    expect(validateMl(ex('blobs3', { model: 'logistic' }))).toMatch(/exactly two classes/);
    expect(validateMl(ex('blobs3', { model: 'forest' }))).toBeNull();
    const s = ex('blobs2');
    const meta = { ...s.meta, columns: s.meta.columns.map((c) => (c.name === 'label' ? { ...c, levels: ['0'] } : c)) };
    expect(validateMl({ ...s, meta })).toMatch(/at least two classes/);
  });

  it('needs features, valid hyper-parameters and a sane split', () => {
    const s = ex('blobs2');
    expect(validateMl({ ...s, features: [] })).toMatch(/at least one feature/);
    expect(validateMl({ ...s, model: 'knn', params: { 'knn.k': 0 } })).toMatch(/at least 1/);
    expect(validateMl({ ...s, model: 'knn', params: { 'knn.k': 2.5 } })).toMatch(/whole number/);
    expect(validateMl({ ...s, eval: { ...s.eval, testFraction: 0.9 } })).toMatch(/between 5% and 50%/);
    expect(validateMl({ ...s, eval: { ...s.eval, method: 'cv', folds: 1 } })).toMatch(/at least 2 folds/);
    expect(validateMl({ ...s, eval: { ...s.eval, method: 'cv', folds: 150 } })).toMatch(/fewer than 2 rows/);
    expect(validateMl({ ...s, eval: { ...s.eval, seed: 1.5 } })).toMatch(/whole number/);
  });

  it('checks tune ranges, compare lists, saving and the network', () => {
    const s = ex('moons', { model: 'knn' });
    expect(validateMl({ ...s, tune: { enabled: true, ranges: { 'knn.k': '1, x' } } })).toMatch(/numbers separated by commas/);
    expect(validateMl({ ...s, tune: { enabled: true, ranges: { 'knn.k': '1, 2.5' } } })).toMatch(/whole numbers/);
    expect(validateMl({ ...s, tune: { enabled: true, ranges: {} } })).toBeNull();
    expect(validateMl({ ...s, model: 'logistic', tune: { enabled: true, ranges: {} } })).toMatch(/nothing to tune/);
    expect(validateMl({ ...s, tune: { enabled: true, ranges: {} }, eval: { ...s.eval, method: 'cv' } })).toMatch(/hold-out/);
    expect(validateMl({ ...s, mode: 'compare', compare: { models: ['knn'], folds: 5, metric: 'accuracy' } })).toMatch(/at least two/);
    expect(validateMl({ ...s, mode: 'compare', compare: { models: ['knn', 'forest'], folds: 5, metric: 'r2' } })).toMatch(/metric that fits/);
    expect(validateMl({ ...s, save: { enabled: true, path: '' } })).toMatch(/where to save/);
    const net = { ...s, model: 'net' };
    expect(validateMl({ ...net, net: { ...net.net, layers: [] } })).toMatch(/at least one hidden layer/);
    expect(validateMl({ ...net, net: { ...net.net, patience: 3, valFraction: 0 } })).toMatch(/validation share/);
    expect(validateMl({ ...net, net: { ...net.net, reportEvery: 500 } })).toMatch(/Report every/);
  });

  it('refuses cross_val accuracy for more than two classes (the engine mislabels it)', () => {
    const s = ex('blobs3', { mode: 'compare', compare: { models: ['knn', 'forest'], folds: 4, metric: 'accuracy' } });
    expect(validateMl(s)).toMatch(/F1/);
    expect(validateMl({ ...s, compare: { ...s.compare, metric: 'f1' } })).toBeNull();
  });

  it('wants a file path, pasted text, or a variable for those sources', () => {
    const s = ex('blobs2');
    expect(validateMl({ ...s, source: { ...s.source, kind: 'file', path: '' } })).toMatch(/Choose a CSV/);
    expect(validateMl({ ...s, source: { ...s.source, kind: 'paste', csv: '' } })).toMatch(/Paste/);
    expect(validateMl({ ...s, source: { ...s.source, kind: 'variable', csv: '' } })).toMatch(/variable/);
    expect(validateMl({ ...s, source: { ...s.source, kind: 'paste', csv: 'a,b\n1,2', csvRows: 500, tempPath: '' } })).toMatch(/temp file/);
  });

  it('PCA cannot keep more components than there are columns; k-means k is bounded by the rows', () => {
    expect(validateMl(ex('wide', { task: 'reduction', model: 'pca', params: { 'pca.k': 9 } }))).toMatch(/cannot keep more/);
    expect(validateMl(ex('blobs3', { task: 'clustering', model: 'kmeans', params: { 'kmeans.k': 200 } }))).toMatch(/half the number of rows|at most/);
  });
});

describe('features and encoding', () => {
  it('one-hot drops the first level; label encoding keeps one column', () => {
    const s = ex('mixed');
    expect(featureNames(s)).toEqual(['age', 'income', 'city=south', 'city=east']);
    expect(featureNames({ ...s, encoding: 'label' })).toEqual(['age', 'income', 'city']);
    expect(featureCols(s).length).toBe(4);
  });
  it('class levels come from the target column', () => {
    expect(classLevels(ex('mixed'))).toEqual(['basic', 'premium']);
    expect(classLevels(ex('blobs3'))).toEqual(['0', '1', '2']);
  });
  it('switching task picks a model and target that fit', () => {
    const r = withTask(ex('mixed'), 'regression');
    expect(r.target).toBe('income');
    expect(r.features).not.toContain('income');
    expect(modelById(r.model)!.tasks).toContain('regression');
    const c = withTask(ex('linreg'), 'classification');
    expect(modelById(c.model)!.tasks).toContain('classification');
  });
});

describe('buildMlScript', () => {
  it('is deterministic and readable: same spec, same text', () => {
    const s = ex('moons', { model: 'forest' });
    expect(buildMlScript(s)).toBe(buildMlScript({ ...s }));
    const t = buildMlScript(s);
    expect(t).toContain('stratified_split(X, y, test_size = 0.25, seed = 1)');
    expect(t).toContain('random_forest_model(X, y, 100, max_depth = 8, seed = 1, kind = "classification")');
    expect(t).toContain('fit_scaler(');
    expect(t).toContain('permutation_importance(');
    expect(t).toContain('roc_curve(');
    expect(t).toContain('seed(1)');
    // generated helpers never take a builtin's name
    for (const m of t.matchAll(/^function (\w+)\(/gm)) expect(m[1].startsWith('ml_')).toBe(true);
  });

  it('uses the engine functions the task needs', () => {
    expect(buildMlScript(ex('moons', { model: 'knn', tune: { enabled: true, ranges: {} } }))).toContain('grid_search(ml_fit_p, Xtr, ytr, {k = [1, 3, 5, 9, 15]}');
    expect(buildMlScript(ex('xor', { mode: 'compare', compare: { models: ['knn', 'tree'], folds: 5, metric: 'accuracy' } }))).toContain('cross_val(ml_fit_knn, Xs, y, folds = 5, seed = 1, metric = "accuracy")');
    expect(buildMlScript(ex('blobs3', { task: 'clustering', model: 'kmeans' }))).toContain('kmeans(Z, k, seed = 1)');
    expect(buildMlScript(ex('wide', { task: 'reduction', model: 'pca' }))).toContain('pca_explained_variance');
    expect(buildMlScript(ex('anomaly', { task: 'anomaly', model: 'iforest' }))).toContain('isolation_forest(Z,');
    expect(buildMlScript(ex('moons', { model: 'net' }))).toContain('sequential((dense_layer(2, 16, activation = "relu")');
  });

  it('never calls the engine accuracy() (it misreads labels above 1) but its own ml_acc', () => {
    for (const model of ['knn', 'net']) expect(buildMlScript(ex('blobs3', { model }))).not.toMatch(/[^_a-z]accuracy\(/);
    expect(buildMlScript(ex('blobs3', { model: 'knn' }))).toContain('ml_acc(yte, ml_pred)');
  });

  it('includes only the helpers it needs', () => {
    const t = buildMlScript(ex('linreg', { model: 'ols' }));
    expect(t).toContain('function ml_emit');
    expect(t).not.toContain('function ml_silhouette');
    expect(t).not.toContain('function ml_confusion');
    expect(buildMlScript(ex('blobs3', { task: 'clustering', model: 'kmeans' }))).toContain('function ml_silhouette');
  });

  it('inlines small pasted tables and points at a temp file for large ones', () => {
    const p = parseTable('a,b,y\n1,2,0\n3,4,1\n5,6,0\n7,8,1\n9,1,0\n2,3,1\n4,5,0\n6,7,1\n8,9,0\n1,5,1\n');
    const s = defaultSpec({ kind: 'paste', exampleId: '', path: '', varName: '', csv: 'a,b,y\n1,2,0', csvRows: 10, tempPath: '' }, p.meta, 'classification', 'y');
    expect(buildMlScript(s)).toContain('ml_t = table("');
    const big = { ...s, source: { ...s.source, csvRows: 5000, tempPath: 'C:\\tmp\\data.csv' } };
    expect(buildMlScript(big)).toContain('read_csv("C:/tmp/data.csv")');
  });

  it('reads a CSV with a semicolon separator and decimal comma, by position when names are unsafe', () => {
    const p = parseTable('"a b";"c d";cls\n1,5;2,5;x\n3,5;4,5;y\n1,1;2,2;x\n3,3;4,4;y\n1,2;3,4;x\n5,5;6,6;y\n1,7;2,8;x\n3,9;4,1;y\n1,3;2,4;x\n7,7;8,8;y');
    const s = defaultSpec({ kind: 'file', exampleId: '', path: 'C:\\d\\f.csv', varName: '', csv: '', csvRows: 0, tempPath: '' }, { ...p.meta, positional: true }, 'classification', 'cls');
    s.features = ['a b', 'c d'];
    const t = buildMlScript(s);
    expect(t).toContain('read_csv("C:/d/f.csv", headers = false, sep = ";", decimal = ",")');
    expect(t).toContain('ml_num(ml_skip1(ml_t.col1))');
    expect(t).toContain('function ml_skip1');
  });

  it('writes the model save lines last when asked', () => {
    const t = buildMlScript(ex('blobs2', { model: 'knn', save: { enabled: true, path: 'C:\\m\\model.json' } }));
    expect(t.trimEnd().split('\n').slice(-2)[0]).toContain('save("C:/m/model.json", "ml_model", "ml_P")');
    const reg = buildMlScript(ex('linreg', { model: 'net', save: { enabled: true, path: 'm.json' } }));
    expect(reg).toContain('"ml_model", "ml_P", "ml_ym", "ml_ys"');
  });

  it('every model has a fit call and a description', () => {
    for (const m of MODELS) {
      expect(m.blurb.length).toBeGreaterThan(20);
      if (m.tasks.some((t) => t === 'classification' || t === 'regression') && m.id !== 'net') {
        const task = m.tasks.includes('classification') ? 'classification' : 'regression';
        const pv = (pid: string) => String(m.params.find((p) => p.id === pid)?.def ?? 1);
        expect(fitCall(ex('moons', { task }), m.id, pv), m.id).toMatch(/_model\(/);
      }
    }
    expect(modelsFor('clustering').map((m) => m.id)).toEqual(['kmeans', 'kmedians', 'kmedoids', 'gmm', 'dbscan']);
  });

  it('builds a predict script that loads the saved model and writes a CSV', () => {
    const s = ex('mixed', { model: 'forest', save: { enabled: true, path: 'C:/m.json' } });
    const t = buildPredictScript(s, { modelPath: 'C:/m.json', newCsv: 'C:/new.csv', outCsv: 'C:/out.csv' });
    expect(t).toContain('load("C:/m.json")');
    expect(t).toContain('read_csv("C:/new.csv")');
    expect(t).toContain('write_csv(ml_out, "C:/out.csv")');
    expect(t).toContain('ml_classes = ("basic", "premium")');
    expect(t).not.toContain('train_test_split');
  });
});

describe('reading the output', () => {
  it('parses tagged lines and keeps plain ones as text', () => {
    const r = parseMlOutput([
      '@ml data 90 100 3',
      '@ml split 70 20',
      '@ml metric accuracy 0.95',
      '@ml cvmetric f1 0.9 0.02',
      '@ml cm_row 0 8 2',
      '@ml cm_row 1 1 9',
      '@ml roc_fpr 0 0.5 1',
      '@ml importance 0.3 -0.01',
      '@ml epoch 5 0.7 0.8 0.5 0.4',
      '@ml stop 40 25',
      '@ml grid 0.9 0.01 3 0.1',
      '@ml best 0.9 3 0.1',
      '@ml compare knn 0.9 0.02 0.9 0.91',
      '@ml compare_fail qda singular covariance',
      '@ml center 0 1.5 2.5',
      '@ml cluster_size 0 40',
      '@ml clusters 3 2',
      '@ml top 7 0.81',
      '@ml note something odd',
      '@ml saved C:/m.json',
      'plain line',
    ].join('\n'));
    expect(r.data).toEqual({ used: 90, total: 100, features: 3 });
    expect(r.split).toEqual({ train: 70, test: 20 });
    expect(r.metrics).toEqual([{ name: 'accuracy', value: 0.95 }]);
    expect(r.cvMetrics[0]).toEqual({ name: 'f1', mean: 0.9, std: 0.02 });
    expect(r.cm).toEqual([[8, 2], [1, 9]]);
    expect(r.series.roc_fpr).toEqual([0, 0.5, 1]);
    expect(r.series.importance).toEqual([0.3, -0.01]);
    expect(r.epochs[0]).toEqual({ epoch: 5, loss: 0.7, valLoss: 0.8, metric: 0.5, valMetric: 0.4 });
    expect(r.stop).toEqual({ epoch: 40, best: 25 });
    expect(r.grid[0]).toEqual({ mean: 0.9, std: 0.01, params: [3, 0.1] });
    expect(r.best).toEqual({ score: 0.9, params: [3, 0.1] });
    expect(r.compare[0]).toEqual({ id: 'knn', mean: 0.9, std: 0.02, folds: [0.9, 0.91] });
    expect(r.compareFail[0]).toEqual({ id: 'qda', message: 'singular covariance' });
    expect(r.centers[0]).toEqual({ id: 0, values: [1.5, 2.5] });
    expect(r.clusterSizes[0]).toEqual({ id: 0, n: 40 });
    expect(r.clusters).toEqual({ k: 3, noise: 2 });
    expect(r.top[0]).toEqual({ row: 7, score: 0.81 });
    expect(r.notes).toEqual(['something odd']);
    expect(r.saved).toBe('C:/m.json');
    expect(r.text).toEqual(['plain line']);
  });

  it('reads NaN and inf the way the engine prints them', () => {
    expect(qnum('NaN')).toBeNaN();
    expect(qnum('inf')).toBe(Infinity);
    expect(qnum('-inf')).toBe(-Infinity);
    expect(qnum('1e-7')).toBe(1e-7);
  });

  it('a metric printed twice keeps the last value', () => {
    const r = emptyResults();
    ingestMlLine(r, '@ml metric r2 0.5');
    ingestMlLine(r, '@ml metric r2 0.7');
    expect(r.metrics).toEqual([{ name: 'r2', value: 0.7 }]);
  });

  it('ranks higher-is-better descending and error metrics ascending', () => {
    const rows = [
      { id: 'a', mean: 0.5, std: 0, folds: [] },
      { id: 'b', mean: 0.9, std: 0, folds: [] },
      { id: 'c', mean: 0.7, std: 0, folds: [] },
    ];
    expect(rankCompare(rows, 'accuracy', 'classification').map((r) => r.id)).toEqual(['b', 'c', 'a']);
    expect(rankCompare(rows, 'rmse', 'regression').map((r) => r.id)).toEqual(['a', 'c', 'b']);
    expect(COMPARE_METRICS.regression.find((m) => m.id === 'rmse')!.higherIsBetter).toBe(false);
  });

  it('parses ranges and error locations and stream chunks', () => {
    expect(parseRange('1, 3;5 9')).toEqual([1, 3, 5, 9]);
    expect(parseRange('1, x')).toBeNull();
    expect(parseRange('')).toBeNull();
    const script = 'a = 1\nb = oops(2)\nc = 3';
    expect(errorLine('qu: runtime error: unknown function `oops`\n  at ml_lab.qu:2', script)).toEqual({ line: 2, text: 'b = oops(2)' });
    expect(errorLine('failed at line 3', script)?.text).toBe('c = 3');
    expect(errorLine('no location', script)).toBeNull();
    const a = splitChunk('', '@ml a 1\n@ml b');
    expect(a).toEqual({ lines: ['@ml a 1'], rest: '@ml b' });
    expect(splitChunk(a.rest, ' 2\n').lines).toEqual(['@ml b 2']);
  });
});
