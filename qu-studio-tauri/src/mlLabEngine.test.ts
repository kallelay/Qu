// The ML lab's ENGINE GATE: builds the script exactly as the panel would, runs
// it with the real Qu binary and checks the numbers against truth computed
// here, independently of the code under test. Skipped unless QU_BIN points at
// a qu executable:  QU_BIN=engine/target/release/qu.exe npm test
import { describe, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  buildMlScript,
  buildPredictScript,
  defaultSpec,
  parseMlOutput,
  rankCompare,
  specForExample,
  validateMl,
  withTask,
  type MlResults,
  type MlSpec,
} from './mlLabDesign';
import { EXAMPLES, canonicalCsv, exampleById, exampleMeta, parseTable, forFile } from './mlLabData';

const QU = process.env.QU_BIN;
const gate = QU ? describe : describe.skip;
const dir = QU ? mkdtempSync(join(tmpdir(), 'mllab-')) : '';

interface Ran {
  ok: boolean;
  out: string;
  err: string;
  script: string;
  res: MlResults;
}
let seq = 0;
function runScript(script: string, timeout = 240_000): Ran {
  const file = join(dir, `s${++seq}.qu`);
  writeFileSync(file, script);
  const r = spawnSync(QU!, ['run', file], { stdio: ['ignore', 'pipe', 'pipe'], timeout, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, cwd: dir });
  const out = r.stdout ?? '';
  const err = (r.stderr ?? '') + (r.error ? String(r.error) : '');
  return { ok: r.status === 0 && !/runtime error|parse error/i.test(err + out), out, err, script, res: parseMlOutput(out) };
}
function run(spec: MlSpec): Ran {
  const bad = validateMl(spec);
  if (bad) throw new Error(`spec invalid: ${bad}`);
  return runScript(buildMlScript(spec));
}
function must(r: Ran): Ran {
  if (!r.ok) throw new Error(`script failed:\n${r.err}\n${r.out.slice(-1500)}\n--- script ---\n${r.script}`);
  return r;
}
const metric = (r: Ran, name: string): number => {
  const m = r.res.metrics.find((x) => x.name === name);
  if (!m) throw new Error(`no metric ${name}; got ${r.res.metrics.map((x) => x.name).join(', ')}\n${r.out.slice(-800)}`);
  return m.value;
};

function ex(id: string, patch: Partial<MlSpec> = {}): MlSpec {
  const e = exampleById(id)!;
  return { ...specForExample(id, exampleMeta(e)), ...patch };
}
const holdout = (spec: MlSpec, model: string, extra: Partial<MlSpec> = {}): MlSpec => ({ ...spec, model, ...extra });

// A seeded generator for the data this file builds itself (independent of Qu).
function rng(seed: number) {
  let a = seed >>> 0;
  const u = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const n = () => Math.sqrt(-2 * Math.log(1 - u())) * Math.cos(2 * Math.PI * u());
  return { u, n };
}

/** AUC by the rank-sum (Mann-Whitney) formula: independent of roc_curve. */
function aucFromScores(y: number[], s: number[]): number {
  let wins = 0;
  let pairs = 0;
  for (let i = 0; i < y.length; i++) {
    if (y[i] !== 1) continue;
    for (let j = 0; j < y.length; j++) {
      if (y[j] !== 0) continue;
      pairs++;
      wins += s[i] > s[j] ? 1 : s[i] === s[j] ? 0.5 : 0;
    }
  }
  return wins / pairs;
}

gate('ML lab engine gate', () => {
  it('every classifier reaches > 0.95 test accuracy on two separated Gaussian blobs', () => {
    for (const model of ['logistic', 'knn', 'tree', 'forest', 'bayes', 'lda', 'qda', 'svm']) {
      const r = must(run(holdout(ex('blobs2'), model)));
      expect(metric(r, 'accuracy'), `${model} accuracy`).toBeGreaterThan(0.95);
    }
  });

  it('the neural network also separates the blobs (and streams a training curve)', () => {
    const r = must(run(holdout(ex('blobs2'), 'net')));
    expect(metric(r, 'accuracy')).toBeGreaterThan(0.95);
    expect(r.res.epochs.length).toBeGreaterThan(2);
    const first = r.res.epochs[0];
    const last = r.res.epochs[r.res.epochs.length - 1];
    expect(last.loss).toBeLessThan(first.loss);
    expect(Number.isFinite(last.valLoss)).toBe(true);
  });

  it('XOR: a linear model fails (about 50%) while kNN, a forest, an RBF SVM and a network succeed', () => {
    const lin = must(run(holdout(ex('xor'), 'logistic')));
    const accLin = metric(lin, 'accuracy');
    expect(accLin).toBeGreaterThan(0.3);
    expect(accLin).toBeLessThan(0.7);
    for (const model of ['knn', 'forest', 'svm']) {
      const r = must(run(holdout(ex('xor'), model)));
      expect(metric(r, 'accuracy'), `${model} on XOR`).toBeGreaterThan(0.85);
    }
    const net = must(run(holdout(ex('xor'), 'net', { net: { ...ex('xor').net, epochs: 300, layers: [{ units: 16, activation: 'relu', dropout: 0 }, { units: 16, activation: 'relu', dropout: 0 }], lr: 0.02 } })));
    expect(metric(net, 'accuracy'), 'network on XOR').toBeGreaterThan(0.85);
  });

  it('noisy linear regression: R-squared matches the theoretical 13 / 13.25', () => {
    // y = 3 x1 - 2 x2 + 1 + noise(sd 0.5), x ~ N(0,1): signal variance 13, noise variance 0.25.
    const theory = 13 / 13.25;
    for (const model of ['ols', 'ridge', 'pls']) {
      const r = must(run(holdout(ex('linreg'), model)));
      expect(metric(r, 'r2'), model).toBeGreaterThan(theory - 0.02);
      expect(metric(r, 'r2'), model).toBeLessThan(theory + 0.01);
      // RMSE of an unbiased fit is about the noise sd
      expect(metric(r, 'rmse'), `${model} rmse`).toBeGreaterThan(0.4);
      expect(metric(r, 'rmse'), `${model} rmse`).toBeLessThan(0.62);
    }
    // x3 is irrelevant: its permutation importance is the smallest
    const r = must(run(holdout(ex('linreg'), 'ols')));
    const imp = r.res.series.importance;
    expect(imp.length).toBe(3);
    expect(imp[2]).toBeLessThan(imp[0] / 10);
    expect(imp[2]).toBeLessThan(imp[1] / 10);
  });

  it('known polynomial: a line explains the analytic 0.43, trees / forest / boosting recover it', () => {
    // y = 1 + 2x - 3x^2 on x in [-1.5, 1.5]: best-line R^2 = var(2x) / (var(2x) + var(3x^2)) = 3 / 7.05.
    const lin = must(run(holdout(ex('poly'), 'ols')));
    expect(metric(lin, 'r2')).toBeGreaterThan(0.36);
    expect(metric(lin, 'r2')).toBeLessThan(0.49);
    for (const model of ['forest', 'tree', 'gboost', 'knn']) {
      const r = must(run(holdout(ex('poly'), model)));
      expect(metric(r, 'r2'), model).toBeGreaterThan(0.95);
    }
  });

  it('k-means recovers the three known centres (scaled and unscaled)', () => {
    const truth = [[0, 4], [4, -2], [-4, -2]];
    for (const scaling of ['none', 'standard'] as const) {
      const spec = ex('blobs3', { task: 'clustering', model: 'kmeans', target: 'label', features: ['x1', 'x2'], scaling });
      const r = must(run(spec));
      expect(r.res.centers.length).toBe(3);
      for (const t of truth) {
        const near = Math.min(...r.res.centers.map((c) => Math.hypot(c.values[0] - t[0], c.values[1] - t[1])));
        expect(near, `centre near ${t} (${scaling})`).toBeLessThan(0.5);
      }
      expect(r.res.clusterSizes.reduce((a, c) => a + c.n, 0)).toBe(240);
      expect(metric(r, 'silhouette')).toBeGreaterThan(0.6);
      expect(metric(r, 'purity')).toBeGreaterThan(0.98);
      expect(r.res.series.sweep_k.length).toBeGreaterThan(3);
      // the elbow: inertia falls steeply to k = 3 and slowly after
      const k = r.res.series.sweep_k;
      const inertia = r.res.series.sweep_inertia;
      const at = (kk: number) => inertia[k.indexOf(kk)];
      expect(at(2) - at(3)).toBeGreaterThan(4 * (at(3) - at(4)));
    }
  });

  it('the other clusterers run and find three groups on the blobs', () => {
    for (const model of ['kmedians', 'kmedoids', 'gmm']) {
      const r = must(run(ex('blobs3', { task: 'clustering', model, target: 'label', features: ['x1', 'x2'] })));
      expect(metric(r, 'purity'), model).toBeGreaterThan(0.95);
    }
    const db = must(run(ex('blobs3', { task: 'clustering', model: 'dbscan', target: 'label', features: ['x1', 'x2'], params: { 'dbscan.eps': 0.35, 'dbscan.min_samples': 5 } })));
    expect(db.res.clusters!.k).toBeGreaterThanOrEqual(3);
  });

  it('PCA on data with a known dominant direction: leading loading aligned, variance near the analytic value', () => {
    // x = t * u + e, u = (0.6, 0.8), t ~ N(0, 3^2), e ~ N(0, 0.3^2 I). Population eigenvalues 9.09 and 0.09.
    const g = rng(7);
    const rows: string[] = ['a,b'];
    for (let i = 0; i < 600; i++) {
      const t = 3 * g.n();
      rows.push(`${(0.6 * t + 0.3 * g.n()).toFixed(6)},${(0.8 * t + 0.3 * g.n()).toFixed(6)}`);
    }
    const parsed = parseTable(rows.join('\n'));
    const csv = canonicalCsv(parsed).text;
    expect(parsed.meta.rows).toBe(600);
    const spec = defaultSpec({ kind: 'paste', exampleId: '', path: '', varName: '', csv, csvRows: 600, tempPath: join(dir, 'pca_data.csv') }, parsed.meta, 'reduction', '');
    spec.model = 'pca';
    spec.scaling = 'none';
    spec.features = ['a', 'b'];
    writeFileSync(spec.source.tempPath, csv);
    const r = must(run(spec));
    const ratio = r.res.series.var_ratio;
    expect(ratio[0]).toBeGreaterThan(9.09 / 9.18 - 0.005);
    expect(ratio[0]).toBeLessThan(9.09 / 9.18 + 0.005);
    const l0 = r.res.series.loading_0;
    expect(Math.abs(l0[0] * 0.6 + l0[1] * 0.8)).toBeGreaterThan(0.999);
    expect(Math.hypot(l0[0], l0[1])).toBeCloseTo(1, 4);
    expect(r.res.series.cum_var[1]).toBeCloseTo(1, 4);
    expect(metric(r, 'kept_variance')).toBeCloseTo(ratio[0] + ratio[1], 4);
  });

  it('PCA on five correlated features: two components carry the variance', () => {
    const r = must(run(ex('wide', { task: 'reduction', model: 'pca', target: 'group', features: ['f1', 'f2', 'f3', 'f4', 'f5'] })));
    expect(r.res.series.cum_var[1]).toBeGreaterThan(0.95);
    expect(r.res.series.scatter_x.length).toBe(200);
    must(run(ex('wide', { task: 'reduction', model: 'tsne', target: 'group', features: ['f1', 'f2', 'f3', 'f4', 'f5'], params: { 'tsne.max_iter': 100, 'tsne.perplexity': 15 } })));
  });

  it('the ROC-AUC the script reports equals one computed independently from the scores it printed', () => {
    for (const model of ['logistic', 'lda', 'bayes', 'net']) {
      const r = must(run(holdout(ex('moons'), model)));
      const y = r.res.series.y_true;
      const s = r.res.series.y_score;
      expect(y.length).toBeGreaterThanOrEqual(74);
      expect(y.length).toBeLessThanOrEqual(77);
      expect(s.length).toBe(y.length);
      expect(metric(r, 'roc_auc'), model).toBeCloseTo(aucFromScores(y, s), 5);
    }
    // a model with only hard labels still gets a (degenerate) ROC and says so
    const r = must(run(holdout(ex('moons'), 'knn')));
    expect(r.res.notes.join(' ')).toMatch(/only returns labels/);
  });

  it('the confusion matrix counts every test row', () => {
    const r = must(run(holdout(ex('blobs3'), 'forest')));
    const total = r.res.cm.flat().reduce((a, b) => a + b, 0);
    expect(r.res.cm.length).toBe(3);
    expect(total).toBe(60);
    // accuracy from the diagonal equals the reported accuracy
    const diag = r.res.cm.reduce((a, row, i) => a + row[i], 0);
    expect(diag / total).toBeCloseTo(metric(r, 'accuracy'), 5);
  });

  it('compare: the linear model ranks last on XOR; every model sees the same folds', () => {
    const spec = ex('xor', { mode: 'compare', compare: { models: ['logistic', 'knn', 'tree', 'forest', 'svm'], folds: 5, metric: 'accuracy' } });
    const r = must(run(spec));
    const ranked = rankCompare(r.res.compare, 'accuracy', 'classification');
    expect(ranked.length).toBe(5);
    expect(ranked[ranked.length - 1].id).toBe('logistic');
    expect(ranked[0].mean).toBeGreaterThan(0.85);
    expect(ranked[ranked.length - 1].mean).toBeLessThan(0.65);
    for (const c of ranked) expect(c.folds.length).toBe(5);
    // regression compare, error metric ranks ascending
    const rr = must(run(ex('poly', { mode: 'compare', compare: { models: ['ols', 'ridge', 'forest', 'gboost'], folds: 4, metric: 'rmse' } })));
    const rk = rankCompare(rr.res.compare, 'rmse', 'regression');
    expect(['ols', 'ridge']).toContain(rk[rk.length - 1].id);
  });

  it('seeds make two runs byte-identical', () => {
    const specs: MlSpec[] = [
      holdout(ex('moons'), 'forest'),
      holdout(ex('moons'), 'net'),
      holdout(ex('linreg'), 'gboost'),
      ex('blobs3', { task: 'clustering', model: 'kmeans', target: 'label', features: ['x1', 'x2'] }),
      ex('anomaly', { task: 'anomaly', model: 'iforest', target: 'outlier', features: ['x1', 'x2'] }),
      ex('xor', { mode: 'compare', compare: { models: ['knn', 'forest'], folds: 3, metric: 'accuracy' } }),
    ];
    for (const spec of specs) {
      const a = must(run(spec));
      const b = must(run(spec));
      expect(b.out, `${spec.task}/${spec.model}`).toBe(a.out);
    }
  });

  it('rejects wrong specs before running, with a clear message', () => {
    const base = ex('blobs2');
    expect(validateMl({ ...base, features: ['x1', 'x2', 'label'] })).toMatch(/also in the features/);
    expect(validateMl({ ...base, meta: { ...base.meta, rows: 0 } })).toMatch(/zero rows/);
    const reg = ex('mixed', { task: 'regression', model: 'ols', target: 'segment', features: ['age', 'income'] });
    expect(validateMl(reg)).toMatch(/Regression needs a numeric target/);
    expect(validateMl({ ...base, model: 'logistic', ...{} , target: 'label' })).toBeNull();
    expect(validateMl(holdout(ex('blobs3'), 'svm'))).toMatch(/exactly two classes/);
    expect(validateMl({ ...base, features: [] })).toMatch(/at least one feature/);
  });

  it('a spec that slips past validation fails loudly in the engine (the gate can fail)', () => {
    const base = ex('blobs2');
    // the panel thinks the table has a column the data does not: validation cannot see that, the engine must
    const meta = { ...base.meta, columns: base.meta.columns.map((c) => (c.name === 'x2' ? { ...c, ident: 'nope' } : c)) };
    const r = runScript(buildMlScript({ ...base, meta }));
    expect(r.ok).toBe(false);
    expect(r.err + r.out).toMatch(/nope|no column/i);
  });

  it('mixed types: category features are one-hot encoded and missing ages are dropped or imputed', () => {
    for (const missing of ['drop', 'mean', 'median'] as const) {
      const r = must(run(holdout(ex('mixed'), 'forest', { missing })));
      expect(metric(r, 'accuracy'), missing).toBeGreaterThan(0.8);
      if (missing === 'drop') expect(r.res.data!.used).toBeLessThan(r.res.data!.total);
      else expect(r.res.data!.used).toBe(r.res.data!.total);
    }
    const lab = must(run(holdout(ex('mixed'), 'tree', { encoding: 'label' })));
    expect(lab.res.series.importance.length).toBe(3);
    const hot = must(run(holdout(ex('mixed'), 'tree')));
    expect(hot.res.series.importance.length).toBe(4); // age, income, city=south, city=east
  });

  it('every supervised model runs on its task', () => {
    for (const model of ['logistic', 'knn', 'tree', 'forest', 'bayes', 'lda', 'qda', 'svm']) {
      must(run(holdout(ex('moons'), model, { eval: { ...ex('moons').eval, learningCurve: true } })));
    }
    for (const model of ['ols', 'ridge', 'knn', 'tree', 'forest', 'gboost', 'svr', 'pls']) {
      const r = must(run(holdout(ex('linreg'), model, { eval: { ...ex('linreg').eval, learningCurve: true } })));
      expect(r.res.series.lc_train.length).toBeGreaterThan(3);
      expect(r.res.series.y_pred.length).toBeGreaterThan(10);
    }
  });

  it('k-fold cross-validation reports mean +/- std and out-of-fold curves', () => {
    const spec = holdout(ex('moons'), 'knn', { eval: { ...ex('moons').eval, method: 'cv', folds: 5 } });
    const r = must(run(spec));
    const cv = r.res.cvMetrics.find((m) => m.name === 'accuracy')!;
    expect(cv.mean).toBeGreaterThan(0.9);
    expect(cv.std).toBeGreaterThanOrEqual(0);
    expect(r.res.cm.flat().reduce((a, b) => a + b, 0)).toBe(300); // every row predicted exactly once
    const reg = must(run(holdout(ex('linreg'), 'ridge', { eval: { ...ex('linreg').eval, method: 'cv', folds: 4 } })));
    expect(reg.res.cvMetrics.find((m) => m.name === 'r2')!.mean).toBeGreaterThan(0.97);
  });

  it('Tune runs grid_search on the training rows and refits the best combination', () => {
    const spec = holdout(ex('moons'), 'knn', { tune: { enabled: true, ranges: { 'knn.k': '1, 5, 15, 40' } } });
    const r = must(run(spec));
    expect(r.res.grid.length).toBe(4);
    expect(r.res.best).not.toBeNull();
    const bestMean = Math.max(...r.res.grid.map((g) => g.mean));
    expect(r.res.best!.score).toBeCloseTo(bestMean, 5);
    expect(r.res.grid.map((g) => g.params[0])).toEqual([1, 5, 15, 40]);
    const two = holdout(ex('linreg'), 'gboost', { tune: { enabled: true, ranges: { 'gboost.n_trees': '20, 60', 'gboost.learning_rate': '0.05, 0.2', 'gboost.max_depth': '2' } } });
    const t = must(run(two));
    expect(t.res.grid.length).toBe(4);
  });

  it('early stopping ends training before the epoch budget and keeps the best weights', () => {
    const base = ex('moons');
    const spec = holdout(base, 'net', { net: { ...base.net, layers: [{ units: 32, activation: 'relu', dropout: 0 }, { units: 32, activation: 'relu', dropout: 0 }], epochs: 400, reportEvery: 5, patience: 3, lr: 0.05 } });
    const r = must(run(spec));
    expect(r.res.stop).not.toBeNull();
    expect(r.res.epochs.length).toBeLessThan(80);
    // it stopped patience reports after the best one, and the curve ends at the stop
    expect(r.res.stop!.epoch).toBe(r.res.epochs[r.res.epochs.length - 1].epoch);
    expect(r.res.stop!.epoch - r.res.stop!.best).toBe(3 * 5);
    const best = r.res.epochs.find((e) => e.epoch === r.res.stop!.best)!;
    expect(best.valLoss).toBe(Math.min(...r.res.epochs.map((e) => e.valLoss)));
    expect(metric(r, 'accuracy')).toBeGreaterThan(0.9);
  });

  it('neural-network regression (dropout, other optimisers)', () => {
    const base = ex('linreg');
    for (const optimizer of ['adam', 'sgd', 'rmsprop'] as const) {
      const spec = holdout(base, 'net', { net: { ...base.net, optimizer, lr: optimizer === 'sgd' ? 0.01 : 0.01, epochs: 120, layers: [{ units: 12, activation: 'relu', dropout: 0.1 }], patience: 0 } });
      const r = must(run(spec));
      expect(metric(r, 'r2'), optimizer).toBeGreaterThan(0.85);
    }
  });

  it('anomaly detection finds the planted outliers', () => {
    const r = must(run(ex('anomaly', { task: 'anomaly', model: 'iforest', target: 'outlier', features: ['x1', 'x2'] })));
    expect(metric(r, 'roc_auc')).toBeGreaterThan(0.93);
    expect(metric(r, 'flagged')).toBeGreaterThan(5);
    expect(r.res.top.length).toBe(10);
    // the most anomalous rows are among the 15 planted ones (row index >= 300)
    expect(r.res.top.filter((t) => t.row >= 300).length).toBeGreaterThanOrEqual(7);
  });

  it('a CSV file source, a header with odd names (positional), and a decimal comma all work', () => {
    const g = rng(3);
    const lines = ['"Sample id";"temp C";"pH value";"class"'];
    for (let i = 0; i < 80; i++) {
      const c = i % 2;
      const t = (c ? 30 : 20) + 2 * g.n();
      const ph = (c ? 6 : 8) + 0.5 * g.n();
      lines.push(`s${i};${t.toFixed(3).replace('.', ',')};${ph.toFixed(3).replace('.', ',')};${c ? 'fresh' : 'old'}`);
    }
    const file = join(dir, 'odd.csv');
    writeFileSync(file, lines.join('\n'));
    const parsed = parseTable(lines.join('\n'));
    expect(parsed.meta.sep).toBe(';');
    expect(parsed.meta.decimal).toBe(',');
    const meta = forFile(parsed);
    expect(meta.positional).toBe(true);
    const spec = defaultSpec({ kind: 'file', exampleId: '', path: file, varName: '', csv: '', csvRows: 0, tempPath: '' }, meta, 'classification', 'class');
    spec.features = ['temp C', 'pH value'];
    spec.model = 'logistic';
    const r = must(run(spec));
    expect(metric(r, 'accuracy')).toBeGreaterThan(0.95);
    expect(r.script).toContain('headers = false');
  });

  it('more than 200 pasted rows go through a temp file; blank cells become missing', () => {
    const g = rng(5);
    const lines = ['u,v,w'];
    for (let i = 0; i < 260; i++) lines.push(`${(g.n() + 3 * (i % 2)).toFixed(4)},${i % 13 === 0 ? '' : g.n().toFixed(4)},${i % 2}`);
    const parsed = parseTable(lines.join('\n'));
    expect(parsed.meta.columns[1].missing).toBeGreaterThan(10);
    const csv = canonicalCsv(parsed).text;
    const tempPath = join(dir, 'paste.csv');
    writeFileSync(tempPath, csv);
    const spec = defaultSpec({ kind: 'paste', exampleId: '', path: '', varName: '', csv, csvRows: 260, tempPath }, parsed.meta, 'classification', 'w');
    spec.model = 'lda';
    spec.missing = 'mean';
    const r = must(run(spec));
    expect(r.script).toContain('read_csv(');
    expect(r.res.data!.used).toBe(260);
    expect(metric(r, 'accuracy')).toBeGreaterThan(0.8);
  });

  it('Save model + Predict on new data: the saved model predicts the same labels as the live one', () => {
    const base = holdout(ex('blobs2'), 'forest');
    const modelPath = join(dir, 'model.json');
    const spec: MlSpec = { ...base, save: { enabled: true, path: modelPath } };
    const trained = must(run(spec));
    expect(trained.res.saved).toContain('model.json');
    // new data: a small CSV with the same columns
    const g = rng(11);
    const rows = ['x1,x2'];
    const truth: number[] = [];
    for (let i = 0; i < 30; i++) {
      const c = i % 2;
      truth.push(c);
      rows.push(`${((c ? -2 : 2) + 0.3 * g.n()).toFixed(4)},${((c ? -2 : 2) + 0.3 * g.n()).toFixed(4)}`);
    }
    const newCsv = join(dir, 'new.csv');
    const outCsv = join(dir, 'pred.csv');
    writeFileSync(newCsv, rows.join('\n'));
    const p = must(runScript(buildPredictScript(spec, { modelPath, newCsv, outCsv })) as Ran);
    expect(p.out).toMatch(/wrote 30 predictions/);
    const text = readFileSync(outCsv, 'utf8') as string;
    const preds = text.trim().split('\n').slice(1).map((l) => l.split(',')[1].trim());
    // class "1" was centred at (-2,-2) in the training data
    const right = preds.filter((v, i) => v === String(truth[i])).length;
    expect(right).toBeGreaterThanOrEqual(28);
    // a network saves and reloads too
    const netPath = join(dir, 'net.json');
    const netSpec: MlSpec = { ...holdout(ex('linreg'), 'net'), save: { enabled: true, path: netPath } };
    must(run(netSpec));
    const newReg = join(dir, 'newreg.csv');
    writeFileSync(newReg, 'x1,x2,x3\n1,0,0\n0,1,0\n');
    const outReg = join(dir, 'predreg.csv');
    must(runScript(buildPredictScript(netSpec, { modelPath: netPath, newCsv: newReg, outCsv: outReg })) as Ran);
    const regPreds = (readFileSync(outReg, 'utf8') as string).trim().split('\n').slice(1).map((l) => Number(l.split(',')[1]));
    // y = 3 x1 - 2 x2 + 1: at (1,0,0) -> 4, at (0,1,0) -> -1 (loose: a small network)
    expect(Math.abs(regPreds[0] - 4)).toBeLessThan(1.2);
    expect(Math.abs(regPreds[1] + 1)).toBeLessThan(1.2);
  });

  it('every built-in example loads and is described correctly', () => {
    for (const e of EXAMPLES) {
      const task = e.task;
      const spec = ex(e.id, { task, model: task === 'classification' ? 'knn' : task === 'regression' ? 'ols' : task === 'clustering' ? 'kmeans' : task === 'reduction' ? 'pca' : 'iforest' });
      if (task === 'clustering' || task === 'reduction' || task === 'anomaly') spec.features = spec.meta.columns.filter((c) => c.name !== e.target).map((c) => c.name);
      const r = must(run(spec));
      expect(r.res.data!.total, e.id).toBe(e.rows);
    }
  });

  void rmSync;
  void withTask;
});
