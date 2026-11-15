// The ML lab's brain: the spec, its validation and `buildMlScript(spec)`, the
// ordinary Qu script every click ends up as. No React. The panel only shows
// this script and runs it; everything on screen is parsed from lines the
// script prints as `@ml <tag> ...` (see `ingestMlLine`).
import { INLINE_ROW_LIMIT, needsStaging } from './dataTable';
import { MAX_LEVELS, exampleById, exampleMeta, isIdent, type ColType, type ColumnMeta, type TableMeta } from './mlLabData';

export type Task = 'classification' | 'regression' | 'clustering' | 'reduction' | 'anomaly';
export const TASKS: Array<{ id: Task; label: string; blurb: string }> = [
  { id: 'classification', label: 'Classification', blurb: 'Predict a category' },
  { id: 'regression', label: 'Regression', blurb: 'Predict a number' },
  { id: 'clustering', label: 'Clustering', blurb: 'Find groups without labels' },
  { id: 'reduction', label: 'Reduce dimensions', blurb: 'PCA and t-SNE' },
  { id: 'anomaly', label: 'Anomalies', blurb: 'Flag unusual rows' },
];
export const isSupervised = (t: Task): boolean => t === 'classification' || t === 'regression';

// ---------------------------------------------------------------- models

export interface ParamDef {
  id: string;
  label: string;
  kind: 'int' | 'float' | 'select';
  def: number | string;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  /** Default text of the Tune range (comma-separated); absent: not tunable. */
  tune?: string;
  help?: string;
}

export interface ModelDef {
  id: string;
  label: string;
  tasks: Task[];
  blurb: string;
  params: ParamDef[];
  /** Needs exactly two classes. */
  binaryOnly?: boolean;
  /** Part of the "compare models" lineup for its task. */
  comparable?: boolean;
}

export const MODELS: ModelDef[] = [
  { id: 'logistic', label: 'Logistic regression', tasks: ['classification'], binaryOnly: true, comparable: true, params: [],
    blurb: 'A straight-line boundary with probabilities. Fast and readable; fails when the classes are not linearly separable. Two classes only.' },
  { id: 'knn', label: 'k-nearest neighbours', tasks: ['classification', 'regression'], comparable: true,
    blurb: 'Votes (or averages) the k closest training rows. No training, follows any shape; scale the features first.',
    params: [{ id: 'k', label: 'Neighbours k', kind: 'int', def: 5, min: 1, max: 200, tune: '1, 3, 5, 9, 15' }] },
  { id: 'tree', label: 'Decision tree', tasks: ['classification', 'regression'], comparable: true,
    blurb: 'Axis-aligned yes/no splits. Readable, handles curves and interactions, overfits when deep.',
    params: [
      { id: 'max_depth', label: 'Max depth', kind: 'int', def: 5, min: 1, max: 30, tune: '2, 3, 5, 8' },
      { id: 'min_samples_split', label: 'Min samples to split', kind: 'int', def: 2, min: 2, max: 200, tune: '2, 5, 10' },
    ] },
  { id: 'forest', label: 'Random forest', tasks: ['classification', 'regression'], comparable: true,
    blurb: 'Many trees on bootstrap samples, averaged. A strong default for tabular data; little tuning needed.',
    params: [
      { id: 'n_trees', label: 'Trees', kind: 'int', def: 100, min: 5, max: 500, tune: '20, 50, 100' },
      { id: 'max_depth', label: 'Max depth', kind: 'int', def: 8, min: 1, max: 30, tune: '4, 8, 12' },
    ] },
  { id: 'bayes', label: 'Gaussian naive Bayes', tasks: ['classification'], comparable: true,
    blurb: 'Per-class Gaussians per feature, combined as if independent. Very fast; good when features are roughly independent.', params: [] },
  { id: 'lda', label: 'Linear discriminant analysis', tasks: ['classification'], comparable: true,
    blurb: 'Gaussian classes with a shared covariance: a linear boundary, stable on small samples.', params: [] },
  { id: 'qda', label: 'Quadratic discriminant analysis', tasks: ['classification'],
    blurb: 'Like LDA but each class has its own covariance, so the boundary can curve. Needs enough rows per class.',
    params: [{ id: 'reg', label: 'Regularisation (0..1)', kind: 'float', def: 0, min: 0, max: 1, step: 0.05, tune: '0, 0.1, 0.5' }] },
  { id: 'svm', label: 'Support vector machine', tasks: ['classification'], binaryOnly: true, comparable: true,
    blurb: 'Maximum-margin boundary, curved with the rbf kernel. Strong on small clean data; scale the features. Two classes only.',
    params: [
      { id: 'kernel', label: 'Kernel', kind: 'select', def: 'rbf', options: ['linear', 'rbf', 'poly'] },
      { id: 'C', label: 'C (margin vs errors)', kind: 'float', def: 1, min: 0.001, max: 1000, tune: '0.1, 1, 10' },
      { id: 'gamma', label: 'gamma (rbf/poly)', kind: 'float', def: 0.5, min: 0.001, max: 100, step: 0.1, tune: '0.1, 0.5, 2' },
    ] },
  { id: 'ols', label: 'Linear regression (OLS)', tasks: ['regression'], comparable: true,
    blurb: 'Least-squares straight-line fit with an intercept. The baseline every other regressor should beat.', params: [] },
  { id: 'ridge', label: 'Ridge regression', tasks: ['regression'], comparable: true,
    blurb: 'Least squares with an L2 penalty: stable when features are correlated.',
    params: [{ id: 'alpha', label: 'Penalty alpha', kind: 'float', def: 1, min: 0, max: 1000, step: 0.1, tune: '0.01, 0.1, 1, 10' }] },
  { id: 'gboost', label: 'Gradient boosting', tasks: ['regression'], comparable: true,
    blurb: 'Shallow trees added one at a time, each fixing the previous residuals. Accurate on curved relations.',
    params: [
      { id: 'n_trees', label: 'Trees', kind: 'int', def: 100, min: 5, max: 500, tune: '50, 100, 200' },
      { id: 'learning_rate', label: 'Learning rate', kind: 'float', def: 0.1, min: 0.001, max: 1, step: 0.01, tune: '0.03, 0.1, 0.3' },
      { id: 'max_depth', label: 'Max depth', kind: 'int', def: 3, min: 1, max: 12, tune: '2, 3, 5' },
    ] },
  { id: 'svr', label: 'Support vector regression', tasks: ['regression'], comparable: true,
    blurb: 'Support-vector fit of a smooth curve. Engine defaults; scale the features.', params: [] },
  { id: 'pls', label: 'Partial least squares', tasks: ['regression'],
    blurb: 'Regression on a few latent components: the chemometrics workhorse for many correlated features.',
    params: [{ id: 'k', label: 'Components', kind: 'int', def: 2, min: 1, max: 50, tune: '1, 2, 3, 5' }] },
  { id: 'net', label: 'Neural network (deep learning)', tasks: ['classification', 'regression'],
    blurb: 'A stack of dense layers trained with an optimiser. Build the layers below; the engine offers relu or linear activations, dropout, and full-batch training.', params: [] },
  { id: 'kmeans', label: 'k-means', tasks: ['clustering'],
    blurb: 'Spherical clusters around k centres. Fast; choose k with the elbow / silhouette sweep.',
    params: [{ id: 'k', label: 'Clusters k', kind: 'int', def: 3, min: 1, max: 50 }] },
  { id: 'kmedians', label: 'k-medians', tasks: ['clustering'],
    blurb: 'Like k-means with median centres and L1 distance: robust to outliers.',
    params: [{ id: 'k', label: 'Clusters k', kind: 'int', def: 3, min: 1, max: 50 }] },
  { id: 'kmedoids', label: 'k-medoids (PAM)', tasks: ['clustering'],
    blurb: 'Centres are real rows of the data. Robust, slower on big tables.',
    params: [{ id: 'k', label: 'Clusters k', kind: 'int', def: 3, min: 1, max: 50 }] },
  { id: 'gmm', label: 'Gaussian mixture', tasks: ['clustering'],
    blurb: 'Soft elliptical clusters fitted by EM, warm-started from k-means.',
    params: [{ id: 'k', label: 'Components k', kind: 'int', def: 3, min: 1, max: 50 }] },
  { id: 'dbscan', label: 'DBSCAN', tasks: ['clustering'],
    blurb: 'Density-based: finds any shape and labels noise -1. eps is in the (scaled) units of the features.',
    params: [
      { id: 'eps', label: 'Radius eps', kind: 'float', def: 0.5, min: 0.001, max: 1000, step: 0.1 },
      { id: 'min_samples', label: 'Min samples', kind: 'int', def: 5, min: 1, max: 200 },
    ] },
  { id: 'pca', label: 'PCA', tasks: ['reduction'],
    blurb: 'Rotates the data onto orthogonal directions of decreasing variance; the variance-explained curve says how many to keep.',
    params: [{ id: 'k', label: 'Components to keep', kind: 'int', def: 2, min: 1, max: 50 }] },
  { id: 'tsne', label: 't-SNE', tasks: ['reduction'],
    blurb: 'A 2-D map that keeps neighbours together: for looking at structure, not for measuring it.',
    params: [
      { id: 'perplexity', label: 'Perplexity', kind: 'float', def: 30, min: 2, max: 200, step: 1 },
      { id: 'max_iter', label: 'Iterations', kind: 'int', def: 300, min: 50, max: 2000 },
    ] },
  { id: 'iforest', label: 'Isolation forest', tasks: ['anomaly'],
    blurb: 'Random cuts isolate unusual rows in few steps; the score is higher for rarer points.',
    params: [
      { id: 'n_trees', label: 'Trees', kind: 'int', def: 100, min: 10, max: 500 },
      { id: 'max_samples', label: 'Rows per tree', kind: 'int', def: 256, min: 8, max: 5000 },
      { id: 'contamination', label: 'Expected anomaly share', kind: 'float', def: 0.05, min: 0.001, max: 0.5, step: 0.01 },
    ] },
];

export const modelById = (id: string): ModelDef | undefined => MODELS.find((m) => m.id === id);
export const modelsFor = (task: Task): ModelDef[] => MODELS.filter((m) => m.tasks.includes(task));
export const defaultModel = (task: Task): string =>
  ({ classification: 'forest', regression: 'ridge', clustering: 'kmeans', reduction: 'pca', anomaly: 'iforest' } as Record<Task, string>)[task];

/** Compare models: classification and regression lineups. */
export const COMPARE_METRICS: Record<'classification' | 'regression', Array<{ id: string; label: string; higherIsBetter: boolean }>> = {
  classification: [
    { id: 'accuracy', label: 'accuracy', higherIsBetter: true },
    { id: 'f1', label: 'F1 (macro)', higherIsBetter: true },
    { id: 'roc_auc', label: 'ROC-AUC (hard labels, binary)', higherIsBetter: true },
  ],
  regression: [
    { id: 'r2', label: 'R-squared', higherIsBetter: true },
    { id: 'rmse', label: 'RMSE', higherIsBetter: false },
    { id: 'mae', label: 'MAE', higherIsBetter: false },
  ],
};

// ---------------------------------------------------------------- spec

export interface NetLayer {
  units: number;
  activation: 'relu' | 'none';
  /** Dropout rate after this layer (0 = none). */
  dropout: number;
}
export const OPTIMIZERS = ['adam', 'sgd', 'rmsprop', 'adamw', 'nadam', 'adagrad', 'adamax'] as const;
export interface NetSpec {
  layers: NetLayer[];
  optimizer: (typeof OPTIMIZERS)[number];
  lr: number;
  epochs: number;
  /** Validate and print a curve point every this many epochs (the engine's
   *  fit() runs in chunks of this size; the optimiser's state restarts each chunk). */
  reportEvery: number;
  /** Share of the training rows held back for validation (0 = none). */
  valFraction: number;
  /** Stop when the validation loss has not improved for this many reports (0 = off). */
  patience: number;
}

export interface SourceSpec {
  kind: 'example' | 'file' | 'paste' | 'variable';
  exampleId: string;
  path: string;
  varName: string;
  /** Canonical CSV (header = identifiers) for pasted text or a variable. */
  csv: string;
  csvRows: number;
  /** Where the panel wrote `csv` when it is too big to inline. */
  tempPath: string;
}

export interface MlSpec {
  source: SourceSpec;
  meta: TableMeta;
  task: Task;
  mode: 'single' | 'compare';
  /** Column names. */
  target: string;
  features: string[];
  encoding: 'onehot' | 'label';
  missing: 'drop' | 'mean' | 'median';
  scaling: 'none' | 'standard' | 'minmax';
  model: string;
  /** `model.param` -> value. */
  params: Record<string, number | string>;
  tune: { enabled: boolean; ranges: Record<string, string> };
  eval: { method: 'holdout' | 'cv'; testFraction: number; folds: number; seed: number; learningCurve: boolean; importance: boolean };
  compare: { models: string[]; folds: number; metric: string };
  net: NetSpec;
  /** Clustering: also sweep k for the elbow / silhouette curves. */
  sweepK: boolean;
  save: { enabled: boolean; path: string };
}

export const DEFAULT_NET: NetSpec = {
  layers: [
    { units: 16, activation: 'relu', dropout: 0 },
    { units: 8, activation: 'relu', dropout: 0 },
  ],
  optimizer: 'adam',
  lr: 0.01,
  epochs: 100,
  reportEvery: 5,
  valFraction: 0.2,
  patience: 5,
};

const EMPTY_SOURCE: SourceSpec = { kind: 'example', exampleId: 'blobs2', path: '', varName: '', csv: '', csvRows: 0, tempPath: '' };

export function emptyMeta(): TableMeta {
  return { columns: [], rows: 0, hasHeader: true, positional: false, sep: ',', decimal: '.' };
}

/** A spec for a built-in example, with sensible defaults for its task. */
export function specForExample(id: string, meta: TableMeta): MlSpec {
  const ex = exampleById(id);
  const task: Task = ex?.task ?? 'classification';
  const target = ex?.target ?? meta.columns[meta.columns.length - 1]?.name ?? '';
  return defaultSpec({ ...EMPTY_SOURCE, exampleId: id }, meta, task, target);
}

export function defaultSpec(source: SourceSpec, meta: TableMeta, task: Task, target?: string): MlSpec {
  const last = meta.columns[meta.columns.length - 1]?.name ?? '';
  const tgt = target ?? last;
  const features = meta.columns.filter((c) => c.name !== tgt && c.type !== 'ignore').map((c) => c.name);
  return {
    source,
    meta,
    task,
    mode: 'single',
    target: tgt,
    features,
    encoding: 'onehot',
    missing: 'drop',
    scaling: 'standard',
    model: defaultModel(task),
    params: {},
    tune: { enabled: false, ranges: {} },
    eval: { method: 'holdout', testFraction: 0.25, folds: 5, seed: 1, learningCurve: false, importance: true },
    compare: { models: [], folds: 5, metric: task === 'regression' ? 'r2' : 'accuracy' },
    net: DEFAULT_NET,
    sweepK: true,
    save: { enabled: false, path: '' },
  };
}

export const DEFAULT_ML: MlSpec = specForExample('blobs2', exampleMeta(exampleById('blobs2')!));

export const paramValue = (spec: MlSpec, modelId: string, p: ParamDef): number | string => spec.params[`${modelId}.${p.id}`] ?? p.def;

/** Task changed: pick that task's default model and a fitting target. */
export function withTask(spec: MlSpec, task: Task): MlSpec {
  const m = modelById(spec.model);
  const model = m && m.tasks.includes(task) ? spec.model : defaultModel(task);
  const col = (n: string) => spec.meta.columns.find((c) => c.name === n);
  let target = spec.target;
  if (task === 'regression') {
    if (col(target)?.type !== 'numeric') target = [...spec.meta.columns].reverse().find((c) => c.type === 'numeric')?.name ?? target;
  } else if (task === 'classification') {
    const t = col(target);
    if (!t || t.type === 'ignore') target = [...spec.meta.columns].reverse().find((c) => c.type === 'category')?.name ?? target;
  }
  const features = spec.features.filter((f) => f !== target);
  return {
    ...spec,
    task,
    model,
    target,
    features: features.length ? features : spec.meta.columns.filter((c) => c.name !== target && c.type !== 'ignore').map((c) => c.name),
    compare: { ...spec.compare, metric: task === 'regression' ? 'r2' : 'accuracy', models: [] },
    mode: isSupervised(task) ? spec.mode : 'single',
  };
}

// ---------------------------------------------------------------- features

export interface FeatureCol {
  /** The label shown in importance charts. */
  label: string;
  kind: 'num' | 'hot' | 'code';
  col: ColumnMeta;
  /** Index into col.levels for a 'hot' column. */
  level: number;
}

const colOf = (spec: MlSpec, name: string): ColumnMeta | undefined => spec.meta.columns.find((c) => c.name === name);

/** The model's input columns, after encoding. */
export function featureCols(spec: MlSpec): FeatureCol[] {
  const out: FeatureCol[] = [];
  for (const name of spec.features) {
    const col = colOf(spec, name);
    if (!col || col.type === 'ignore') continue;
    if (col.type === 'numeric') out.push({ label: col.name, kind: 'num', col, level: 0 });
    else if (spec.encoding === 'label') out.push({ label: col.name, kind: 'code', col, level: 0 });
    else for (let j = 1; j < col.levels.length; j++) out.push({ label: `${col.name}=${col.levels[j]}`, kind: 'hot', col, level: j });
  }
  return out;
}
export const featureNames = (spec: MlSpec): string[] => featureCols(spec).map((f) => f.label);
export const featureCount = (spec: MlSpec): number => featureCols(spec).length;

/** Class names of a classification target (or the colour/label column of an
 *  unsupervised task). */
export function classLevels(spec: MlSpec): string[] {
  const t = colOf(spec, spec.target);
  return t && t.type !== 'ignore' ? t.levels.slice(0, MAX_LEVELS) : [];
}

// ---------------------------------------------------------------- validation

const BAD_LITERAL = /["{}\\\r\n]/;

/** Parses "1, 3, 5" into numbers; null when anything is not a finite number. */
export function parseRange(text: string): number[] | null {
  const parts = text.split(/[,;\s]+/).filter((s) => s.length > 0);
  if (!parts.length) return null;
  const nums = parts.map(Number);
  return nums.every((n) => Number.isFinite(n)) ? nums : null;
}

export const tunableParams = (modelId: string): ParamDef[] => (modelById(modelId)?.params ?? []).filter((p) => p.tune !== undefined);
export const rangeText = (spec: MlSpec, modelId: string, p: ParamDef): string => spec.tune.ranges[`${modelId}.${p.id}`] ?? p.tune ?? '';

/** The reason the spec cannot run, in plain words, or null. */
export function validateMl(spec: MlSpec): string | null {
  const m = modelById(spec.model);
  if (!m) return `Unknown model "${spec.model}".`;
  if (!m.tasks.includes(spec.task)) return `${m.label} does not do ${spec.task}.`;
  const src = spec.source;
  if (src.kind === 'file' && !src.path.trim()) return 'Choose a CSV file.';
  if (src.kind === 'file' && /["{}]/.test(src.path)) return 'The file path cannot contain quotes or braces.';
  if (src.kind === 'paste' && !src.csv.trim()) return 'Paste some data first.';
  if (src.kind === 'variable' && !src.csv.trim()) return 'Pick a session variable (a matrix or vector) first.';
  if ((src.kind === 'paste' || src.kind === 'variable') && needsStaging(src.csvRows) && !src.tempPath) {
    return `More than ${INLINE_ROW_LIMIT} rows: the panel needs a temp file for them, but could not write one.`;
  }
  const meta = spec.meta;
  if (!meta.columns.length) return 'No data loaded yet.';
  if (meta.rows <= 0) return 'The table has zero rows: there is nothing to learn from.';
  if (meta.rows < 10) return `Only ${meta.rows} rows: too few to train and test anything (at least 10).`;

  const sup = isSupervised(spec.task);
  const target = colOf(spec, spec.target);
  if (sup) {
    if (!spec.target) return 'Choose the target column (what to predict).';
    if (!target) return `The target column "${spec.target}" is not in the table.`;
    if (target.type === 'ignore') return `The target column "${spec.target}" is set to ignore.`;
    if (spec.features.includes(spec.target)) return `The target column "${spec.target}" is also in the features: the model would just read the answer. Remove it from the features.`;
    if (spec.task === 'regression' && target.type !== 'numeric') {
      return `Regression needs a numeric target, but "${spec.target}" is a category column${target.levels.length ? ` (${target.levels.slice(0, 4).join(', ')}${target.levels.length > 4 ? ', ...' : ''})` : ''}. Choose a numeric column or switch to Classification.`;
    }
    if (spec.task === 'classification') {
      if (target.type === 'numeric' && target.distinct > MAX_LEVELS) {
        return `"${spec.target}" has ${target.distinct} distinct numeric values: it looks continuous. Use Regression, or set it to category to treat each value as a class.`;
      }
      const k = target.levels.length;
      if (k < 2) return `Classification needs at least two classes, but "${spec.target}" has ${k}.`;
      if (k > MAX_LEVELS) return `"${spec.target}" has more than ${MAX_LEVELS} classes.`;
      if (m.binaryOnly && k !== 2) return `${m.label} handles exactly two classes, but "${spec.target}" has ${k}. Pick another model (forest, k-nearest neighbours, naive Bayes, LDA or the neural network).`;
      for (const l of target.levels) if (BAD_LITERAL.test(l)) return `The class "${l}" contains characters that cannot be written in a Qu string (quotes, braces, backslash). Clean the data first.`;
    }
  } else if (spec.target) {
    // optional label/colour column: only checked when it is used
    if (spec.features.includes(spec.target)) return `The label column "${spec.target}" is also in the features. Remove it from the features.`;
  }
  if (!spec.features.length) return 'Choose at least one feature column.';
  const seen = new Set<string>();
  for (const f of spec.features) {
    const c = colOf(spec, f);
    if (!c) return `The feature column "${f}" is not in the table.`;
    if (c.type === 'ignore') return `The feature "${f}" is set to ignore; remove it from the features.`;
    if (seen.has(f)) return `The feature "${f}" is listed twice.`;
    seen.add(f);
    if (c.type === 'category') {
      if (c.levels.length > MAX_LEVELS) return `The category column "${f}" has more than ${MAX_LEVELS} levels: set it to ignore, or to numeric if the values are numbers.`;
      for (const l of c.levels) if (BAD_LITERAL.test(l)) return `The value "${l}" in column "${f}" contains characters that cannot be written in a Qu string (quotes, braces, backslash).`;
    }
  }
  const d = featureCount(spec);
  if (d < 1) return 'The chosen features give no input columns (a category column with one level carries no information).';
  if (spec.task === 'reduction' && d < 2) return 'Dimensionality reduction needs at least two input columns.';
  if (spec.meta.columns.some((c) => !c.ident && spec.features.includes(c.name))) return 'A feature column has no usable name.';
  if (!Number.isInteger(spec.eval.seed)) return 'The seed must be a whole number.';

  if (sup || spec.task === 'anomaly' || spec.task === 'clustering' || spec.task === 'reduction') {
    for (const p of m.params) {
      const v = paramValue(spec, m.id, p);
      if (p.kind === 'select') {
        if (!p.options!.includes(String(v))) return `${p.label}: choose one of ${p.options!.join(', ')}.`;
        continue;
      }
      const n = Number(v);
      if (!Number.isFinite(n)) return `${m.label}: ${p.label} must be a number.`;
      if (p.min !== undefined && n < p.min) return `${m.label}: ${p.label} must be at least ${p.min}.`;
      if (p.max !== undefined && n > p.max) return `${m.label}: ${p.label} must be at most ${p.max}.`;
      if (p.kind === 'int' && !Number.isInteger(n)) return `${m.label}: ${p.label} must be a whole number.`;
    }
  }
  if (sup) {
    if (spec.eval.method === 'holdout') {
      const f = spec.eval.testFraction;
      if (!(f >= 0.05 && f <= 0.5)) return 'The test share must be between 5% and 50%.';
      if (Math.floor(meta.rows * f) < 2) return 'The test set would have fewer than 2 rows: raise the test share or add data.';
    } else {
      const k = spec.eval.folds;
      if (!Number.isInteger(k) || k < 2) return 'Cross-validation needs at least 2 folds.';
      if (k > meta.rows / 2) return `${k} folds on ${meta.rows} rows leaves fewer than 2 rows per fold.`;
    }
    if (spec.mode === 'compare') {
      if (spec.compare.models.length < 2) return 'Tick at least two models to compare.';
      const k = spec.compare.folds;
      if (!Number.isInteger(k) || k < 2 || k > meta.rows / 2) return `Compare: folds must be a whole number from 2 to ${Math.floor(meta.rows / 2)}.`;
      for (const id of spec.compare.models) {
        const cm = modelById(id);
        if (!cm || !cm.comparable || !cm.tasks.includes(spec.task)) return `"${id}" cannot be compared for ${spec.task}.`;
        if (cm.binaryOnly && target && target.levels.length !== 2) return `${cm.label} handles exactly two classes, but "${spec.target}" has ${target.levels.length}: untick it.`;
      }
      if (!COMPARE_METRICS[spec.task as 'classification' | 'regression'].some((x) => x.id === spec.compare.metric)) return 'Pick a comparison metric that fits the task.';
      if (spec.compare.metric === 'roc_auc' && target && target.levels.length !== 2) return 'ROC-AUC needs two classes: pick accuracy or F1.';
      if (spec.task === 'classification' && spec.compare.metric === 'accuracy' && target && target.levels.length > 2) return 'The engine cross_val accuracy mistakes class labels above 1 for scores, so it is wrong for more than two classes. Compare with F1 (macro) instead.';
    } else if (spec.model === 'net') {
      const n = spec.net;
      if (!n.layers.length) return 'Add at least one hidden layer.';
      for (const [i, l] of n.layers.entries()) {
        if (!Number.isInteger(l.units) || l.units < 1 || l.units > 1024) return `Layer ${i + 1}: units must be a whole number from 1 to 1024.`;
        if (!(l.dropout >= 0 && l.dropout < 1)) return `Layer ${i + 1}: dropout must be from 0 up to (not including) 1.`;
      }
      if (!(n.lr > 0)) return 'The learning rate must be positive.';
      if (!Number.isInteger(n.epochs) || n.epochs < 1 || n.epochs > 5000) return 'Epochs must be a whole number from 1 to 5000.';
      if (!Number.isInteger(n.reportEvery) || n.reportEvery < 1 || n.reportEvery > n.epochs) return 'Report every N epochs: N must be a whole number from 1 to the epoch count.';
      if (!(n.valFraction >= 0 && n.valFraction <= 0.5)) return 'The validation share must be from 0 to 50%.';
      if (n.patience > 0 && n.valFraction === 0) return 'Early stopping needs a validation share above 0.';
      if (spec.eval.method === 'cv') return 'The neural network trains once with a validation split: use the hold-out evaluation.';
    } else if (spec.tune.enabled) {
      if (spec.eval.method === 'cv') return 'Tuning picks parameters on the training rows, then evaluates on the test rows: use the hold-out evaluation (or compare models).';
      const tp = tunableParams(spec.model);
      if (!tp.length) return `${m.label} has nothing to tune.`;
      let combos = 1;
      for (const p of tp) {
        const r = parseRange(rangeText(spec, spec.model, p));
        if (!r) return `Tune range for ${p.label}: write numbers separated by commas.`;
        if (r.length > 10) return `Tune range for ${p.label}: at most 10 values.`;
        if (p.kind === 'int' && r.some((x) => !Number.isInteger(x))) return `Tune range for ${p.label}: whole numbers only.`;
        if (p.min !== undefined && r.some((x) => x < p.min!)) return `Tune range for ${p.label}: values must be at least ${p.min}.`;
        combos *= r.length;
      }
      if (combos > 60) return `The tune grid has ${combos} combinations (at most 60): shorten a range.`;
    }
  }
  if (spec.task === 'reduction' && spec.model === 'pca' && Number(paramValue(spec, 'pca', modelById('pca')!.params[0])) > d) return `PCA cannot keep more components (${paramValue(spec, 'pca', modelById('pca')!.params[0])}) than there are input columns (${d}).`;
  if (spec.task === 'clustering' && m.params.some((p) => p.id === 'k') && Number(paramValue(spec, m.id, m.params[0])) > meta.rows / 2) return 'k is more than half the number of rows.';
  if (spec.save.enabled) {
    if (!sup) return 'Saving a model is available for classification and regression.';
    if (!spec.save.path.trim()) return 'Choose where to save the model.';
    if (/["{}]/.test(spec.save.path)) return 'The save path cannot contain quotes or braces.';
    if (spec.mode === 'compare') return 'Save a model from a single-model run.';
  }
  return null;
}

// ---------------------------------------------------------------- script pieces

const q = (s: string): string => `"${s}"`;
const tuple = (xs: string[]): string => (xs.length === 1 ? `(${q(xs[0])},)` : `(${xs.map(q).join(', ')})`);
const fnum = (v: number): string => String(Number(v.toPrecision(12)));
const fpath = (p: string): string => p.replace(/\\/g, '/');

function readCsvCall(path: string, meta: TableMeta): string {
  const extra: string[] = [];
  if (meta.positional || !meta.hasHeader) extra.push('headers = false');
  if (meta.sep !== ',') extra.push(`sep = ${q(meta.sep === '\t' ? '\\t' : meta.sep)}`);
  if (meta.decimal !== '.') extra.push(`decimal = ${q(meta.decimal)}`);
  return `read_csv(${q(fpath(path))}${extra.length ? `, ${extra.join(', ')}` : ''})`;
}

function dataBlock(spec: MlSpec): string[] {
  const s = spec.source;
  if (s.kind === 'example') {
    const ex = exampleById(s.exampleId);
    return [`# ---- Data: built-in example "${ex?.label ?? s.exampleId}" (generated here, seeded)`, ex?.code ?? 'ml_t = table(x = [0.0])'];
  }
  if (s.kind === 'file') return [`# ---- Data: CSV file`, `ml_t = ${readCsvCall(s.path, spec.meta)}`];
  if (needsStaging(s.csvRows) && s.tempPath) {
    const what = s.kind === 'variable' ? `session variable ${s.varName}` : 'pasted text';
    return [`# ---- Data: ${what} (${s.csvRows} rows, kept in a temp file)`, `ml_t = read_csv(${q(fpath(s.tempPath))})`];
  }
  const what = s.kind === 'variable' ? `session variable ${s.varName}` : 'pasted text';
  return [`# ---- Data: ${what} (${s.csvRows} rows, inline)`, `ml_t = table("`, s.csv, `")`];
}

/** `ml_t.col` for a column, honouring the positional fallback. */
function rawCol(spec: MlSpec, c: ColumnMeta): string {
  const m = spec.meta;
  if (m.positional || !m.hasHeader) return m.positional && m.hasHeader ? `ml_skip1(ml_t.col${c.index + 1})` : `ml_t.col${c.index + 1}`;
  return `ml_t.${c.ident}`;
}

const HELPERS: Record<string, string> = {
  emit: `function ml_emit(tag, v)
    s = "@ml " + tag
    for i = 0 to length(v) - 1
        s = s + " " + str(v[i])
    end for
    print(s)
end function`,
  thin: `function ml_thin(v, m)
    n = length(v)
    if n <= m
        return v
    end if
    out = zeros(m)
    for i = 0 to m - 1
        out[i] = v[round(i * (n - 1) / (m - 1))]
    end for
    return out
end function`,
  skip1: `function ml_skip1(c)
    return c[1:len(c) - 1]
end function`,
  num: `function ml_num(c)
    if type(c) == "vector"
        return c
    end if
    n = len(c)
    v = zeros(n)
    for i = 0 to n - 1
        v[i] = nan
        try
            v[i] = to_float(c[i])
        catch ml_e
            try
                v[i] = to_float(replace(c[i], ",", "."))
            catch ml_e2
                v[i] = nan
            end try
        end try
    end for
    return v
end function`,
  codes: `function ml_codes(c, levels)
    n = len(c)
    out = zeros(n)
    for i = 0 to n - 1
        out[i] = nan
        x = c[i]
        if type(c) == "vector"
            x = str(c[i])
        end if
        for j = 0 to len(levels) - 1
            if x == levels[j]
                out[i] = j
            end if
        end for
    end for
    return out
end function`,
  hot: `function ml_hot(codes, j)
    n = length(codes)
    out = zeros(n)
    for i = 0 to n - 1
        if isfinite(codes[i])
            if codes[i] == j
                out[i] = 1
            end if
        else
            out[i] = nan
        end if
    end for
    return out
end function`,
  okrows: `function ml_ok_rows(X, y, strict)
    ok = []
    for i = 0 to rows(X) - 1
        good = isfinite(y[i])
        if strict
            for j = 0 to cols(X) - 1
                if not isfinite(X[i, j])
                    good = false
                end if
            end for
        end if
        if good
            ok = append(ok, i)
        end if
    end for
    return ok
end function`,
  fill: `function ml_fill_values(X, how)
    f = zeros(cols(X))
    for j = 0 to cols(X) - 1
        v = X[:, j]
        v = v[isfinite(v)]
        if length(v) > 0
            if how == "median"
                f[j] = median(v)
            else
                f[j] = mean(v)
            end if
        end if
    end for
    return f
end function

function ml_fillna(X, f)
    for j = 0 to cols(X) - 1
        for i = 0 to rows(X) - 1
            if not isfinite(X[i, j])
                X[i, j] = f[j]
            end if
        end for
    end for
    return X
end function`,
  confusion: `function ml_confusion(y, p, k)
    cm = zeros(k, k)
    for i = 0 to length(y) - 1
        cm[y[i], p[i]] = cm[y[i], p[i]] + 1
    end for
    return cm
end function`,
  metric: `function ml_metric(name, v)
    print("@ml metric " + name + " " + str(v))
end function

function ml_cv_metric(name, v)
    print("@ml cvmetric " + name + " " + str(mean(v)) + " " + str(std(v)))
end function`,
  acc: `function ml_acc(y, p)
    return sum(y == p) / length(y)
end function`,
  quality: `function ml_pred(kind, model, X)
    if kind == "net_classification"
        return argmax(softmax_rows(model.predict(X)), axis = 1)
    end if
    return model.predict(X)
end function

function ml_quality(kind, y, p)
    if kind == "classification" or kind == "net_classification"
        return ml_acc(y, p)
    end if
    return r2(y, p)
end function

function ml_pimp(model, X, y, kind, reps, sd)
    out = zeros(cols(X))
    base = ml_quality(kind, y, ml_pred(kind, model, X))
    for j = 0 to cols(X) - 1
        acc = 0
        for r = 1 to reps
            idx = argsort(rand(rows(X), seed = sd + 31 * j + r))
            Xp = X
            Xp[:, j] = X[idx, j]
            acc = acc + base - ml_quality(kind, y, ml_pred(kind, model, Xp))
        end for
        out[j] = acc / reps
    end for
    return out
end function`,
  silhouette: `function ml_silhouette(X, lab)
    n = rows(X)
    if n > 400
        idx = round(linspace(0, n - 1, 400))
        X = X[idx, :]
        lab = lab[idx]
        n = 400
    end if
    groups = unique(lab)
    total = 0
    used = 0
    for i = 0 to n - 1
        if lab[i] >= 0
            d = sqrt(sum((X - X[i, :]) .^ 2, axis = 1))
            own = sum(lab == lab[i])
            if own > 1
                a = sum(d[lab == lab[i]]) / (own - 1)
                b = inf
                for g = 0 to length(groups) - 1
                    if groups[g] >= 0 and groups[g] != lab[i]
                        b = min(b, mean(d[lab == groups[g]]))
                    end if
                end for
                if isfinite(b)
                    total = total + (b - a) / max(a, b)
                    used = used + 1
                end if
            end if
        end if
    end for
    if used == 0
        return nan
    end if
    return total / used
end function`,
};

// ---------------------------------------------------------------- model calls

const taskKind = (spec: MlSpec): string => (spec.task === 'regression' ? 'regression' : 'classification');

/** The Qu expression that fits `model` on (X, y). `pv(id)` gives each
 *  hyperparameter's source text (a literal, or `p.id` when tuned). */
export function fitCall(spec: MlSpec, id: string, pv: (pid: string) => string): string {
  const kind = taskKind(spec);
  const seed = spec.eval.seed;
  switch (id) {
    case 'logistic': return 'logistic_model(X, y)';
    case 'knn': return `knn_model(X, y, ${pv('k')}, kind = ${q(kind)})`;
    case 'tree': return `tree_model(X, y, max_depth = ${pv('max_depth')}, min_samples_split = ${pv('min_samples_split')}, kind = ${q(kind)})`;
    case 'forest': return `random_forest_model(X, y, ${pv('n_trees')}, max_depth = ${pv('max_depth')}, seed = ${seed}, kind = ${q(kind)})`;
    case 'bayes': return 'naive_bayes_model(X, y)';
    case 'lda': return 'lda_model(X, y)';
    case 'qda': return `qda_model(X, y, reg = ${pv('reg')})`;
    case 'svm': return `svm_model(X, y, kernel = ${pv('kernel')}, C = ${pv('C')}, gamma = ${pv('gamma')})`;
    case 'ols': return 'ols_model(X, y)';
    case 'ridge': return `ridge_model(X, y, ${pv('alpha')})`;
    case 'gboost': return `gradient_boosting_model(X, y, ${pv('n_trees')}, learning_rate = ${pv('learning_rate')}, max_depth = ${pv('max_depth')})`;
    case 'svr': return 'svr_model(X, y)';
    case 'pls': return `pls_model(X, y, ${pv('k')})`;
    default: throw new Error(`fitCall: no supervised fit for ${id}`);
  }
}

function literalParams(spec: MlSpec, id: string, tuned: Set<string> = new Set()): (pid: string) => string {
  const m = modelById(id)!;
  return (pid) => {
    if (tuned.has(pid)) return `p.${pid}`;
    const p = m.params.find((x) => x.id === pid)!;
    const v = paramValue(spec, id, p);
    if (p.kind === 'select') return q(String(v));
    return fnum(Number(v));
  };
}

/** Lines that set `ml_score` (a real-valued score for class 1) from `ml_model`
 *  and `Xte`, or null when the model only gives labels. */
function scoreLines(id: string): string[] | null {
  switch (id) {
    case 'logistic':
      return ['ml_score = 1 ./ (1 + exp(-(Xte * ml_model.coef + ml_model.intercept)))'];
    case 'lda':
      return [
        'ml_w = ml_model.precision * (ml_model.means[1, :] - ml_model.means[0, :])\'',
        'ml_score = (Xte - 0.5 * (ml_model.means[0, :] + ml_model.means[1, :])) * ml_w + log(ml_model.priors[1] / ml_model.priors[0])',
      ];
    case 'bayes':
      return [
        'ml_ll0 = -0.5 * sum(log(ml_model.variances[0, :])) - 0.5 * sum((Xte - ml_model.means[0, :]) .^ 2 ./ ml_model.variances[0, :], axis = 1)',
        'ml_ll1 = -0.5 * sum(log(ml_model.variances[1, :])) - 0.5 * sum((Xte - ml_model.means[1, :]) .^ 2 ./ ml_model.variances[1, :], axis = 1)',
        'ml_score = ml_ll1 - ml_ll0 + log(ml_model.priors[1] / ml_model.priors[0])',
      ];
    case 'net':
      return ['ml_score = softmax_rows(ml_model.predict(Xte))[:, 1]'];
    default:
      return null;
  }
}

// ---------------------------------------------------------------- the script

interface Need {
  set: Set<string>;
}
const need = (n: Need, ...keys: string[]) => keys.forEach((k) => n.set.add(k));

function helperBlock(n: Need): string[] {
  const order = ['emit', 'thin', 'skip1', 'num', 'codes', 'hot', 'okrows', 'fill', 'confusion', 'metric', 'acc', 'quality', 'silhouette'];
  if (n.set.has('quality') || n.set.has('metric')) n.set.add('acc');
  const out: string[] = ['# ---- Helpers: generated plumbing. The "@ml" lines they print are what the panel draws.'];
  for (const k of order) if (n.set.has(k)) out.push(HELPERS[k], '');
  return out;
}

/** Feature matrix, target, missing-value handling: shared by training and prediction. */
function featureBlock(spec: MlSpec, n: Need, withTarget: boolean): string[] {
  const fc = featureCols(spec);
  const L: string[] = [];
  need(n, 'num', 'emit');
  const numN = fc.filter((f) => f.kind === 'num').length;
  const catCols = Array.from(new Set(fc.filter((f) => f.kind !== 'num').map((f) => f.col.name)));
  L.push(`# ---- Features: ${numN} numeric${catCols.length ? `, ${catCols.length} category (${spec.encoding === 'onehot' ? 'one-hot, first level dropped' : 'label-encoded'})` : ''}`);
  const names: string[] = [];
  const catVar = new Map<string, string>();
  fc.forEach((f, i) => {
    const nm = `ml_f${i}`;
    if (f.kind === 'num') {
      L.push(`${nm} = ml_num(${rawCol(spec, f.col)})`);
    } else {
      need(n, 'codes');
      if (!catVar.has(f.col.name)) {
        const cv = `ml_k${catVar.size}`;
        catVar.set(f.col.name, cv);
        L.push(`${cv} = ml_codes(${rawCol(spec, f.col)}, ${tuple(f.col.levels)})   # ${f.col.name}`);
      }
      const cv = catVar.get(f.col.name)!;
      if (f.kind === 'code') L.push(`${nm} = ${cv}`);
      else {
        need(n, 'hot');
        L.push(`${nm} = ml_hot(${cv}, ${f.level})   # ${f.label}`);
      }
    }
    names.push(nm);
  });
  if (names.length === 1) L.push(`X = ${names[0]} as matrix(length(${names[0]}), 1)`);
  else L.push(`X = vstack(${names.join(', ')})'`);
  if (withTarget) {
    const t = colOf(spec, spec.target);
    if (spec.task === 'regression') {
      L.push(`y = ml_num(${rawCol(spec, t!)})   # target: ${spec.target}`);
    } else if (t) {
      need(n, 'codes');
      L.push(`ml_classes = ${tuple(classLevels(spec))}`);
      L.push(`y = ml_codes(${rawCol(spec, t)}, ml_classes)   # target: ${spec.target}`);
    }
  }
  return L;
}

function missingBlock(spec: MlSpec, n: Need, hasY: boolean, yDrives = true): string[] {
  need(n, 'okrows');
  const strict = spec.missing === 'drop';
  const L = [`# ---- Missing values: ${strict ? 'rows with any missing value are dropped' : `a missing feature value is replaced by the column ${spec.missing} of the training rows`}`];
  L.push('ml_n0 = rows(X)');
  L.push(hasY && yDrives ? `ml_ok = ml_ok_rows(X, y, ${strict})` : `ml_ok = ml_ok_rows(X, zeros(rows(X)), ${strict})`);
  L.push('if length(ml_ok) < 10', '    error("Fewer than 10 usable rows remain (" + str(length(ml_ok)) + " of " + str(ml_n0) + "): check the column types, or impute instead of dropping missing values")', 'end if');
  L.push('X = X[ml_ok, :]');
  if (hasY) L.push('y = y[ml_ok]');
  L.push('print("@ml data {rows(X)} {ml_n0} {cols(X)}")');
  return L;
}

/** Class counts (classification) or a target histogram (regression), printed once. */
function balanceBlock(spec: MlSpec, n: Need): string[] {
  need(n, 'emit');
  if (spec.task === 'classification') {
    const K = classLevels(spec).length;
    return [`ml_cc = zeros(${K})`, 'for ml_i = 0 to length(y) - 1', '    ml_cc[y[ml_i]] = ml_cc[y[ml_i]] + 1', 'end for', 'ml_emit("class_counts", ml_cc)'];
  }
  return ['ml_lo = min(y)', 'ml_hi = max(y)', 'ml_hc = zeros(12)', 'for ml_i = 0 to length(y) - 1', '    ml_b = min(floor((y[ml_i] - ml_lo) / (ml_hi - ml_lo + 1e-12) * 12), 11)', '    ml_hc[ml_b] = ml_hc[ml_b] + 1', 'end for', 'ml_emit("target_hist", ml_hc)', 'ml_emit("target_range", [ml_lo, ml_hi])'];
}

function colorBlock(spec: MlSpec, n: Need): string[] {
  // Unsupervised tasks: an optional category column used to colour points / score.
  const t = colOf(spec, spec.target);
  if (!spec.target || !t || t.type === 'ignore' || t.levels.length < 2 || t.levels.length > MAX_LEVELS) return [];
  need(n, 'codes');
  return [`ml_classes = ${tuple(classLevels(spec))}`, `y = ml_codes(${rawCol(spec, t)}, ml_classes)   # label column: ${spec.target} (colours / scoring only, never a model input)`];
}

function prepFns(spec: MlSpec, n: Need): string[] {
  need(n, 'fill');
  const how = spec.missing === 'median' ? 'median' : 'mean';
  const L: string[] = ['# ---- Preprocessing, fitted on the rows a model is trained on and applied unchanged to the rest'];
  L.push('function ml_prep_fit(Xtr)');
  L.push(`    fill = ml_fill_values(Xtr, ${q(how)})`);
  if (spec.scaling === 'none') L.push('    return {fill = fill}');
  else {
    L.push(`    sc = fit_scaler(ml_fillna(Xtr, fill), method = ${q(spec.scaling === 'standard' ? 'standard' : 'minmax')})`);
    L.push('    return {fill = fill, scaler = sc}');
  }
  L.push('end function', '', 'function ml_prep_apply(P, X)');
  L.push(spec.scaling === 'none' ? '    return ml_fillna(X, P.fill)' : '    return P.scaler.transform(ml_fillna(X, P.fill))');
  L.push('end function');
  return L;
}

function emitSeedLine(spec: MlSpec): string {
  return `seed(${spec.eval.seed})   # the global generator too, so any unseeded draw repeats`;
}

function metricsClassification(spec: MlSpec, n: Need, yv: string, pv: string, scoreVar: string | null, K: number, tag = 'ml_metric'): string[] {
  need(n, 'metric', 'emit', 'confusion', 'thin');
  const L: string[] = [];
  L.push(`${tag}("accuracy", ml_acc(${yv}, ${pv}))`);
  L.push(`${tag}("precision", precision(${yv}, ${pv}))`);
  L.push(`${tag}("recall", recall(${yv}, ${pv}))`);
  L.push(`${tag}("f1", f1(${yv}, ${pv}))`);
  L.push(`ml_cm = ml_confusion(${yv}, ${pv}, ${K})`);
  L.push(`for ml_i = 0 to ${K - 1}`, '    ml_emit("cm_row " + str(ml_i), ml_cm[ml_i, :])', 'end for');
  if (K === 2) {
    const sv = scoreVar ?? pv;
    if (!scoreVar) L.push('print("@ml note this model only returns labels, so the ROC and PR curves have a single operating point")');
    L.push('try');
    L.push(`    ml_roc = roc_curve(${yv}, ${sv})`);
    L.push(`    ${tag}("roc_auc", ml_roc.auc)`);
    L.push('    ml_emit("roc_fpr", ml_thin(ml_roc.fpr, 200))', '    ml_emit("roc_tpr", ml_thin(ml_roc.tpr, 200))');
    L.push(`    ml_pr = pr_curve(${yv}, ${sv})`);
    L.push(`    ${tag}("avg_precision", ml_pr.ap)`);
    L.push('    ml_emit("pr_recall", ml_thin(ml_pr.recall, 200))', '    ml_emit("pr_precision", ml_thin(ml_pr.precision, 200))');
    L.push('catch ml_e', '    print("@ml note ROC/PR unavailable: " + ml_e.message)', 'end try');
    L.push(`ml_emit("y_true", ml_thin(${yv}, 2000))`, `ml_emit("y_score", ml_thin(${sv}, 2000))`);
  }
  return L;
}

function metricsRegression(n: Need, yv: string, pv: string, tag = 'ml_metric'): string[] {
  need(n, 'metric', 'emit', 'thin');
  return [
    `${tag}("r2", r2(${yv}, ${pv}))`,
    `${tag}("rmse", rmse(${yv}, ${pv}))`,
    `${tag}("mae", mae(${yv}, ${pv}))`,
    `ml_emit("y_true", ml_thin(${yv}, 600))`,
    `ml_emit("y_pred", ml_thin(${pv}, 600))`,
    `ml_emit("residual", ml_thin(${yv} - ${pv}, 600))`,
  ];
}

function importanceLines(spec: MlSpec, n: Need, modelV: string, Xv: string, yv: string, kind: string, into: string): string[] {
  need(n, 'quality', 'emit');
  if (kind.startsWith('net_')) return [`${into} = ml_pimp(${modelV}, ${Xv}, ${yv}, ${q(kind)}, 5, ${spec.eval.seed})`];
  return ['try', `    ${into} = permutation_importance(${modelV}, ${Xv}, ${yv}, n_repeats = 5)`, 'catch ml_e', `    ${into} = ml_pimp(${modelV}, ${Xv}, ${yv}, ${q(kind)}, 5, ${spec.eval.seed})`, 'end try'];
}

function fitFunction(spec: MlSpec, id: string, name: string, tuned?: Set<string>): string[] {
  if (tuned && tuned.size) {
    return [`function ${name}(X, y, p)`, `    return ${fitCall(spec, id, literalParams(spec, id, tuned))}`, 'end function'];
  }
  return [`function ${name}(X, y)`, `    return ${fitCall(spec, id, literalParams(spec, id))}`, 'end function'];
}

function splitLines(spec: MlSpec): string[] {
  const e = spec.eval;
  const fn = spec.task === 'classification' ? 'stratified_split' : 'train_test_split';
  return [
    `# ---- Hold-out split: ${Math.round(e.testFraction * 100)}% of the rows are never seen while fitting${spec.task === 'classification' ? ' (stratified: each class keeps its share)' : ''}`,
    `ml_sp = ${fn}(X, y, test_size = ${fnum(e.testFraction)}, seed = ${e.seed})`,
    'Xtr = ml_sp.X_train',
    'ytr = ml_sp.y_train',
    'Xte = ml_sp.X_test',
    'yte = ml_sp.y_test',
    'ml_P = ml_prep_fit(Xtr)',
    'Xtr = ml_prep_apply(ml_P, Xtr)',
    'Xte = ml_prep_apply(ml_P, Xte)',
    'print("@ml split {rows(Xtr)} {rows(Xte)}")',
  ];
}

function saveLines(spec: MlSpec): string[] {
  if (!spec.save.enabled) return [];
  const names = ['ml_model', 'ml_P'];
  if (spec.model === 'net' && spec.task === 'regression') names.push('ml_ym', 'ml_ys');
  return ['', '# ---- Save the fitted model and its preprocessing (load them again with load(...))', `save(${q(fpath(spec.save.path))}, ${names.map(q).join(', ')})`, 'print("@ml saved " + ' + q(fpath(spec.save.path)) + ')'];
}

// ----- supervised, single model, hold-out
function supervisedHoldout(spec: MlSpec, n: Need): string[] {
  const id = spec.model;
  const m = modelById(id)!;
  const L: string[] = [];
  const cls = spec.task === 'classification';
  const K = classLevels(spec).length;
  L.push(...splitLines(spec), '');
  const tp = spec.tune.enabled ? tunableParams(id) : [];
  if (tp.length) {
    const tuned = new Set(tp.map((p) => p.id));
    const grid = tp.map((p) => `${p.id} = [${(parseRange(rangeText(spec, id, p)) ?? []).map(fnum).join(', ')}]`).join(', ');
    const metric = cls ? 'accuracy' : 'r2';
    L.push(`# ---- Tune ${m.label}: grid_search cross-validates every combination on the TRAINING rows only (same folds for all), then the best one is refit`);
    L.push(...fitFunction(spec, id, 'ml_fit_p', tuned));
    L.push(`ml_gs = grid_search(ml_fit_p, Xtr, ytr, {${grid}}, folds = 5, seed = ${spec.eval.seed}, metric = ${q(metric)})`);
    L.push('for ml_i = 0 to length(ml_gs.results) - 1', '    ml_r = ml_gs.results[ml_i]');
    L.push(`    print("@ml grid " + str(ml_r.mean) + " " + str(ml_r.std)${tp.map((p) => ` + " " + str(ml_r.params.${p.id})`).join('')})`, 'end for');
    L.push(`print("@ml best " + str(ml_gs.best_score)${tp.map((p) => ` + " " + str(ml_gs.best_params.${p.id})`).join('')})`);
    L.push(`print("@ml gridmetric ${metric}")`);
    L.push('ml_model = ml_fit_p(Xtr, ytr, ml_gs.best_params)');
  } else {
    L.push(`# ---- Model: ${m.label}`, ...fitFunction(spec, id, 'ml_fit'), 'ml_model = ml_fit(Xtr, ytr)');
  }
  L.push('', '# ---- Evaluation on the held-out rows');
  L.push('ml_pred = ml_model.predict(Xte)');
  if (cls) {
    const sl = K === 2 ? scoreLines(id) : null;
    if (sl) L.push(...sl);
    L.push(...metricsClassification(spec, n, 'yte', 'ml_pred', sl ? 'ml_score' : null, K));
  } else {
    L.push(...metricsRegression(n, 'yte', 'ml_pred'));
  }
  if (spec.eval.importance) {
    L.push('', '# ---- Feature importance: how much the test score drops when one column is shuffled');
    L.push(...importanceLines(spec, n, 'ml_model', 'Xte', 'yte', taskKind(spec), 'ml_imp'));
    L.push('ml_emit("importance", ml_imp)');
  }
  if (spec.eval.learningCurve) L.push('', ...learningCurve(spec, n, tp.length > 0));
  return L;
}

function learningCurve(spec: MlSpec, n: Need, tuned: boolean): string[] {
  need(n, 'quality', 'emit');
  const kind = taskKind(spec);
  const fit = tuned ? 'ml_fit_p(Xtr[0:ml_m - 1, :], ytr[0:ml_m - 1], ml_gs.best_params)' : 'ml_fit(Xtr[0:ml_m - 1, :], ytr[0:ml_m - 1])';
  return [
    '# ---- Learning curve: score against the number of training rows',
    'ml_lc_n = []',
    'ml_lc_tr = []',
    'ml_lc_te = []',
    'for ml_i = 1 to 8',
    '    ml_m = max(round(rows(Xtr) * ml_i / 8), 8)',
    '    try',
    `        ml_mm = ${fit}`,
    `        ml_lc_tr = append(ml_lc_tr, ml_quality(${q(kind)}, ytr[0:ml_m - 1], ml_mm.predict(Xtr[0:ml_m - 1, :])))`,
    `        ml_lc_te = append(ml_lc_te, ml_quality(${q(kind)}, yte, ml_mm.predict(Xte)))`,
    '        ml_lc_n = append(ml_lc_n, ml_m)',
    '    catch ml_e',
    '        print("@ml note learning curve point skipped: " + ml_e.message)',
    '    end try',
    'end for',
    'ml_emit("lc_n", ml_lc_n)',
    'ml_emit("lc_train", ml_lc_tr)',
    'ml_emit("lc_test", ml_lc_te)',
  ];
}

// ----- supervised, single model, k-fold
function supervisedCv(spec: MlSpec, n: Need): string[] {
  const id = spec.model;
  const m = modelById(id)!;
  const cls = spec.task === 'classification';
  const K = classLevels(spec).length;
  const k = spec.eval.folds;
  const kind = taskKind(spec);
  need(n, 'emit', 'thin', 'quality', 'metric');
  const L: string[] = [];
  L.push(`# ---- Model: ${m.label}`, ...fitFunction(spec, id, 'ml_fit'), '');
  L.push(`# ---- ${k}-fold cross-validation: every row is predicted once, by a model that never saw it`);
  L.push(`ml_perm = argsort(rand(rows(X), seed = ${spec.eval.seed}))`);
  L.push('ml_oof = zeros(rows(X))');
  if (cls && K === 2) L.push('ml_oof_s = zeros(rows(X))');
  L.push(`ml_imp = zeros(cols(X))`);
  if (cls) L.push(`ml_fold_acc = zeros(${k})`, `ml_fold_f1 = zeros(${k})`);
  else L.push(`ml_fold_r2 = zeros(${k})`, `ml_fold_rmse = zeros(${k})`);
  L.push(`for ml_f = 0 to ${k - 1}`, '    ml_tr = []', '    ml_te = []');
  L.push('    for ml_i = 0 to rows(X) - 1', `        if ml_i mod ${k} == ml_f`, '            ml_te = append(ml_te, ml_perm[ml_i])', '        else', '            ml_tr = append(ml_tr, ml_perm[ml_i])', '        end if', '    end for');
  L.push('    Xtr = X[ml_tr, :]', '    ytr = y[ml_tr]', '    Xte = X[ml_te, :]', '    yte = y[ml_te]');
  L.push('    ml_P = ml_prep_fit(Xtr)', '    Xtr = ml_prep_apply(ml_P, Xtr)', '    Xte = ml_prep_apply(ml_P, Xte)');
  L.push('    ml_model = ml_fit(Xtr, ytr)', '    ml_pred = ml_model.predict(Xte)', '    ml_oof[ml_te] = ml_pred');
  if (cls && K === 2) {
    const sl = scoreLines(id);
    if (sl) L.push(...sl.map((s) => `    ${s}`), '    ml_oof_s[ml_te] = ml_score');
    else L.push('    ml_oof_s[ml_te] = ml_pred');
  }
  if (cls) L.push('    ml_fold_acc[ml_f] = ml_acc(yte, ml_pred)', '    ml_fold_f1[ml_f] = f1(yte, ml_pred)');
  else L.push('    ml_fold_r2[ml_f] = r2(yte, ml_pred)', '    ml_fold_rmse[ml_f] = rmse(yte, ml_pred)');
  if (spec.eval.importance) {
    L.push('    try', '        ml_imp = ml_imp + permutation_importance(ml_model, Xte, yte, n_repeats = 3) / ' + k, '    catch ml_e', `        ml_imp = ml_imp + ml_pimp(ml_model, Xte, yte, ${q(kind)}, 3, ${spec.eval.seed}) / ${k}`, '    end try');
  }
  L.push('end for', '');
  L.push('# ---- Mean +/- std over the folds');
  if (cls) L.push('ml_cv_metric("accuracy", ml_fold_acc)', 'ml_cv_metric("f1", ml_fold_f1)');
  else L.push('ml_cv_metric("r2", ml_fold_r2)', 'ml_cv_metric("rmse", ml_fold_rmse)');
  L.push('', '# ---- All out-of-fold predictions together');
  L.push('ml_pred = ml_oof');
  if (cls) L.push(...metricsClassification(spec, n, 'y', 'ml_pred', K === 2 ? 'ml_oof_s' : null, K));
  else L.push(...metricsRegression(n, 'y', 'ml_pred'));
  if (spec.eval.importance) L.push('ml_emit("importance", ml_imp)');
  L.push('', '# ---- Final model on all rows (for saving / predicting new data)', 'ml_P = ml_prep_fit(X)', 'ml_model = ml_fit(ml_prep_apply(ml_P, X), y)');
  return L;
}

// ----- compare
function compareBlock(spec: MlSpec, n: Need): string[] {
  const cm = spec.compare;
  need(n, 'emit');
  const L: string[] = [];
  L.push(`# ---- Compare ${cm.models.length} models on the SAME ${cm.folds} folds (seed ${spec.eval.seed}); each is scored by ${cm.metric}`);
  L.push('# Preprocessing is fitted on all rows here (folds share it): a small optimistic bias, equal for every model.');
  L.push('ml_P = ml_prep_fit(X)', 'Xs = ml_prep_apply(ml_P, X)', '');
  for (const id of cm.models) L.push(...fitFunction(spec, id, `ml_fit_${id}`), '');
  for (const id of cm.models) {
    L.push('try');
    L.push(`    ml_cv = cross_val(ml_fit_${id}, Xs, y, folds = ${cm.folds}, seed = ${spec.eval.seed}, metric = ${q(cm.metric)})`);
    L.push(`    ml_s = "@ml compare ${id} " + str(ml_cv.mean) + " " + str(ml_cv.std)`);
    L.push('    for ml_i = 0 to length(ml_cv.scores) - 1', '        ml_s = ml_s + " " + str(ml_cv.scores[ml_i])', '    end for', '    print(ml_s)');
    L.push('catch ml_e', `    print("@ml compare_fail ${id} " + ml_e.message)`, 'end try');
  }
  return L;
}

// ----- neural network
function netBlock(spec: MlSpec, n: Need): string[] {
  const nt = spec.net;
  const cls = spec.task === 'classification';
  const K = classLevels(spec).length;
  const D = featureCount(spec);
  need(n, 'emit', 'quality', 'metric', 'thin', 'confusion');
  const L: string[] = [];
  L.push(...splitLines(spec).filter((l) => !l.startsWith('print("@ml split')));
  if (nt.valFraction > 0) {
    L.push(`ml_vs = train_test_split(Xtr, ytr, test_size = ${fnum(nt.valFraction)}, seed = ${spec.eval.seed + 1})`);
    L.push('Xfit = ml_vs.X_train', 'yfit = ml_vs.y_train', 'Xval = ml_vs.X_test', 'yval = ml_vs.y_test');
  } else L.push('Xfit = Xtr', 'yfit = ytr');
  L.push('print("@ml split {rows(Xfit)} {rows(Xte)}")');
  L.push('');
  const kind = cls ? 'net_classification' : 'net_regression';
  if (cls) {
    L.push('# ---- One-hot targets (the network outputs one logit per class)');
    L.push(`Yfit = zeros(rows(Xfit), ${K})`, 'for ml_i = 0 to rows(Xfit) - 1', '    Yfit[ml_i, yfit[ml_i]] = 1', 'end for');
    if (nt.valFraction > 0) L.push(`Yval = zeros(rows(Xval), ${K})`, 'for ml_i = 0 to rows(Xval) - 1', '    Yval[ml_i, yval[ml_i]] = 1', 'end for');
  } else {
    L.push('# ---- The target is standardised for training and mapped back for every prediction');
    L.push('ml_ym = mean(yfit)', 'ml_ys = std(yfit)', 'Yfit = (yfit - ml_ym) / ml_ys');
    if (nt.valFraction > 0) L.push('Yval = (yval - ml_ym) / ml_ys');
  }
  const layers: string[] = [];
  let width = D;
  for (const l of nt.layers) {
    layers.push(`dense_layer(${width}, ${l.units}, activation = ${q(l.activation)})`);
    if (l.dropout > 0) layers.push(`dropout_layer(${fnum(l.dropout)})`);
    width = l.units;
  }
  layers.push(`dense_layer(${width}, ${cls ? K : 1})`);
  L.push('', `# ---- Network: ${D} inputs -> ${nt.layers.map((l) => `${l.units} ${l.activation === 'relu' ? 'relu' : 'linear'}${l.dropout > 0 ? ` (dropout ${l.dropout})` : ''}`).join(' -> ')} -> ${cls ? `${K} logits` : '1 output'}`);
  L.push(`ml_net = sequential((${layers.join(', ')}), seed = ${spec.eval.seed})`);
  const rounds = Math.ceil(nt.epochs / nt.reportEvery);
  const loss = cls ? 'cross_entropy' : 'mse';
  const opt = `${nt.optimizer}(lr = ${fnum(nt.lr)})`;
  L.push('', `# ---- Training: ${nt.epochs} epochs in chunks of ${nt.reportEvery} (the engine's fit() is full-batch; the optimiser restarts each chunk)`);
  L.push('ml_best = ml_net', 'ml_best_val = inf', 'ml_wait = 0', 'ml_done = 0', 'ml_best_ep = 0');
  L.push(`for ml_r = 1 to ${rounds}`);
  L.push(`    ml_net = ml_net.fit(Xfit, Yfit, epochs = ${nt.reportEvery}, loss = ${q(loss)}, optimizer = ${opt})`);
  L.push(`    ml_done = ml_done + ${nt.reportEvery}`, '    ml_lh = ml_net.loss_history', '    ml_tl = ml_lh[length(ml_lh) - 1]');
  if (cls) {
    L.push('    ml_ta = ml_acc(yfit, argmax(softmax_rows(ml_net.predict(Xfit)), axis = 1))');
    if (nt.valFraction > 0) {
      L.push('    ml_vp = ml_net.predict(Xval)', '    ml_vl = -mean(sum(Yval .* log(softmax_rows(ml_vp) + 1e-12), axis = 1))', '    ml_va = ml_acc(yval, argmax(softmax_rows(ml_vp), axis = 1))');
    } else L.push('    ml_vl = nan', '    ml_va = nan');
  } else {
    L.push('    ml_ta = r2(yfit, ml_net.predict(Xfit) * ml_ys + ml_ym)');
    if (nt.valFraction > 0) L.push('    ml_vp = ml_net.predict(Xval)', '    ml_vl = mean((ml_vp - Yval) .^ 2)', '    ml_va = r2(yval, ml_vp * ml_ys + ml_ym)');
    else L.push('    ml_vl = nan', '    ml_va = nan');
  }
  L.push('    print("@ml epoch {ml_done} {ml_tl} {ml_vl} {ml_ta} {ml_va}")');
  if (nt.valFraction > 0) {
    L.push('    if ml_vl < ml_best_val', '        ml_best_val = ml_vl', '        ml_best = ml_net', '        ml_best_ep = ml_done', '        ml_wait = 0', '    else', '        ml_wait = ml_wait + 1', '    end if');
    if (nt.patience > 0) L.push(`    if ml_wait >= ${nt.patience}`, '        print("@ml stop {ml_done} {ml_best_ep}")', '        break', '    end if');
  }
  L.push('end for');
  if (nt.valFraction > 0 && nt.patience > 0) L.push('ml_model = ml_best   # early stopping keeps the weights with the lowest validation loss');
  else L.push('ml_model = ml_net');
  L.push('', '# ---- Evaluation on the held-out rows');
  if (cls) {
    L.push('ml_prob = softmax_rows(ml_model.predict(Xte))', 'ml_pred = argmax(ml_prob, axis = 1)');
    if (K === 2) L.push('ml_score = ml_prob[:, 1]');
    L.push(...metricsClassification(spec, n, 'yte', 'ml_pred', K === 2 ? 'ml_score' : null, K));
  } else {
    L.push('ml_pred = ml_model.predict(Xte) * ml_ys + ml_ym');
    L.push(...metricsRegression(n, 'yte', 'ml_pred'));
  }
  if (spec.eval.importance) {
    L.push('', '# ---- Feature importance by permutation');
    if (cls) L.push('ml_imp = ml_pimp(ml_model, Xte, yte, "net_classification", 5, ' + spec.eval.seed + ')');
    else L.push('ml_imp = ml_pimp(ml_model, Xte, (yte - ml_ym) / ml_ys, "net_regression", 5, ' + spec.eval.seed + ')');
    L.push('ml_emit("importance", ml_imp)');
  }
  void kind;
  return L;
}

// ----- unsupervised
function clusterBlock(spec: MlSpec, n: Need): string[] {
  const id = spec.model;
  const m = modelById(id)!;
  const seed = spec.eval.seed;
  const hasY = classLevels(spec).length >= 2 && !!spec.target && !!colOf(spec, spec.target) && colOf(spec, spec.target)!.type !== 'ignore';
  need(n, 'emit', 'thin', 'silhouette', 'metric');
  const kP = Number(paramValue(spec, id, m.params.find((p) => p.id === 'k') ?? { id: 'k', label: '', kind: 'int', def: 3 }));
  const L: string[] = [];
  const algo = (kx: string): string => {
    switch (id) {
      case 'kmeans': return `kmeans(Z, ${kx}, seed = ${seed})`;
      case 'kmedians': return `kmedians_model(Z, ${kx}, seed = ${seed}).predict(Z)`;
      case 'kmedoids': return `kmedoids_model(Z, ${kx}, seed = ${seed}).predict(Z)`;
      case 'gmm': return `gmm_model(Z, ${kx}, seed = ${seed}).predict(Z)`;
      default: return `dbscan(Z, ${fnum(Number(paramValue(spec, id, m.params[0])))}, ${fnum(Number(paramValue(spec, id, m.params[1])))})`;
    }
  };
  L.push('# ---- Preprocessing on all rows (there is no test set when clustering)', 'ml_P = ml_prep_fit(X)', 'Z = ml_prep_apply(ml_P, X)', '');
  if (id === 'dbscan') L.push('function ml_cluster(Z)', `    return ${algo('')}`, 'end function', '', `# ---- ${m.label}`, 'ml_labels = ml_cluster(Z)');
  else L.push('function ml_cluster(Z, k)', `    return ${algo('k')}`, 'end function', '', `# ---- ${m.label} with k = ${kP}`, `ml_labels = ml_cluster(Z, ${kP})`);
  L.push('ml_groups = unique(ml_labels)', 'ml_k = sum(ml_groups >= 0)', 'ml_noise = sum(ml_labels < 0)', 'print("@ml clusters {ml_k} {ml_noise}")');
  L.push('# Centres (mean of each cluster, in the original units) and sizes', 'ml_inertia = 0', 'for ml_g = 0 to length(ml_groups) - 1', '    ml_c = ml_groups[ml_g]', '    if ml_c >= 0', '        ml_cz = mean(Z[ml_labels == ml_c, :], axis = 0) as matrix(1, cols(Z))', '        ml_inertia = ml_inertia + sum(sum((Z[ml_labels == ml_c, :] - ml_cz) .^ 2, axis = 1))');
  L.push(spec.scaling === 'none' ? '        ml_co = ml_cz' : '        ml_co = ml_P.scaler.inverse_transform(ml_cz)');
  L.push('        ml_emit("center " + str(ml_c), ml_co)', '        print("@ml cluster_size " + str(ml_c) + " " + str(sum(ml_labels == ml_c)))', '    end if', 'end for');
  L.push('ml_metric("inertia", ml_inertia)', 'ml_metric("silhouette", ml_silhouette(Z, ml_labels))');
  if (hasY) L.push('# Agreement with the label column (only for reading the result, never used to fit)', 'ml_purity = 0', 'for ml_g = 0 to length(ml_groups) - 1', '    if ml_groups[ml_g] >= 0', '        ml_yy = y[ml_labels == ml_groups[ml_g]]', '        ml_best = 0', '        for ml_c = 0 to ' + (classLevels(spec).length - 1), '            ml_best = max(ml_best, sum(ml_yy == ml_c))', '        end for', '        ml_purity = ml_purity + ml_best', '    end if', 'end for', 'ml_metric("purity", ml_purity / rows(Z))');
  L.push('', '# ---- Picture: the first two principal components when there are more than two features');
  L.push(...scatterLines(spec, 'Z', hasY, id !== 'dbscan'));
  if (spec.sweepK && id !== 'dbscan') {
    const kmax = Math.max(2, Math.min(10, Math.floor(spec.meta.rows / 4)));
    L.push('', '# ---- Elbow / silhouette sweep over k', 'ml_sk = []', 'ml_si = []', 'ml_ss = []', `for ml_kk = 2 to ${kmax}`, '    ml_l2 = ml_cluster(Z, ml_kk)', '    ml_in2 = 0', '    for ml_c = 0 to ml_kk - 1', '        if sum(ml_l2 == ml_c) > 0', '            ml_cz = mean(Z[ml_l2 == ml_c, :], axis = 0)', '            ml_in2 = ml_in2 + sum(sum((Z[ml_l2 == ml_c, :] - ml_cz) .^ 2, axis = 1))', '        end if', '    end for', '    ml_sk = append(ml_sk, ml_kk)', '    ml_si = append(ml_si, ml_in2)', '    ml_ss = append(ml_ss, ml_silhouette(Z, ml_l2))', 'end for', 'ml_emit("sweep_k", ml_sk)', 'ml_emit("sweep_inertia", ml_si)', 'ml_emit("sweep_silhouette", ml_ss)');
  }
  return L;
}

function scatterLines(spec: MlSpec, Zv: string, hasY: boolean, centresToo: boolean): string[] {
  const L: string[] = [];
  L.push(`if cols(${Zv}) > 2`, `    ml_xy = pca(${Zv}, 2)`, 'else if cols(' + Zv + ') == 2', `    ml_xy = ${Zv}`, 'else', `    ml_xy = vstack(${Zv}[:, 0], zeros(rows(${Zv})))'`, 'end if');
  L.push('ml_ix = round(linspace(0, rows(ml_xy) - 1, min(rows(ml_xy), 1500)))', 'ml_emit("scatter_x", ml_xy[ml_ix, 0])', 'ml_emit("scatter_y", ml_xy[ml_ix, 1])');
  L.push(spec.task === 'clustering' ? 'ml_emit("scatter_c", ml_labels[ml_ix])' : hasY ? 'ml_emit("scatter_c", y[ml_ix])' : 'ml_emit("scatter_c", zeros(length(ml_ix)))');
  void centresToo;
  return L;
}

function reductionBlock(spec: MlSpec, n: Need): string[] {
  const id = spec.model;
  const m = modelById(id)!;
  const hasY = classLevels(spec).length >= 2 && !!spec.target && !!colOf(spec, spec.target) && colOf(spec, spec.target)!.type !== 'ignore';
  need(n, 'emit', 'metric', 'thin');
  const D = featureCount(spec);
  const L: string[] = ['# ---- Preprocessing on all rows', 'ml_P = ml_prep_fit(X)', 'Z = ml_prep_apply(ml_P, X)', ''];
  if (id === 'pca') {
    const k = Math.min(Number(paramValue(spec, id, m.params[0])), D);
    L.push(`# ---- PCA: variance explained by each component`);
    L.push(`ml_kmax = min(cols(Z), rows(Z) - 1)`, 'ml_ev = pca_explained_variance(Z, ml_kmax)', 'ml_emit("var_ratio", ml_ev)', 'ml_emit("cum_var", cumsum(ml_ev))');
    L.push(`ml_comp = pca_components(Z, min(${k}, ml_kmax))`, 'for ml_i = 0 to rows(ml_comp) - 1', '    ml_emit("loading " + str(ml_i), ml_comp[ml_i, :])', 'end for');
    L.push(`ml_keep = min(${k}, ml_kmax)`, 'ml_metric("kept_components", ml_keep)', 'ml_metric("kept_variance", sum(ml_ev[0:ml_keep - 1]))');
    L.push('', '# ---- Picture: the data on the first two components', 'ml_xy = pca(Z, 2)');
  } else {
    L.push('# ---- t-SNE: a 2-D map that keeps neighbours together', `ml_xy = tsne(Z, n_components = 2, perplexity = ${fnum(Number(paramValue(spec, id, m.params[0])))}, seed = ${spec.eval.seed}, max_iter = ${fnum(Number(paramValue(spec, id, m.params[1])))})`);
  }
  L.push('ml_ix = round(linspace(0, rows(ml_xy) - 1, min(rows(ml_xy), 1500)))', 'ml_emit("scatter_x", ml_xy[ml_ix, 0])', 'ml_emit("scatter_y", ml_xy[ml_ix, 1])');
  L.push(hasY ? 'ml_emit("scatter_c", y[ml_ix])' : 'ml_emit("scatter_c", zeros(length(ml_ix)))');
  return L;
}

function anomalyBlock(spec: MlSpec, n: Need): string[] {
  const m = modelById('iforest')!;
  const pv = (pid: string) => fnum(Number(paramValue(spec, 'iforest', m.params.find((p) => p.id === pid)!)));
  const t = colOf(spec, spec.target);
  const hasY = !!t && t.type !== 'ignore' && t.levels.length === 2;
  need(n, 'emit', 'metric', 'thin');
  const L: string[] = ['# ---- Preprocessing on all rows', 'ml_P = ml_prep_fit(X)', 'Z = ml_prep_apply(ml_P, X)', ''];
  L.push('# ---- Isolation forest: unusual rows are cut off from the rest in few random splits');
  L.push(`ml_iso = isolation_forest(Z, n_trees = ${pv('n_trees')}, max_samples = ${pv('max_samples')}, contamination = ${pv('contamination')}, seed = ${spec.eval.seed})`);
  L.push('ml_scores = ml_iso.scores', 'ml_flag = ml_iso.labels', 'ml_metric("flagged", sum(ml_flag))', 'ml_metric("threshold", ml_iso.threshold)');
  L.push('ml_emit("scores", ml_thin(ml_scores, 2000))', 'ml_emit("flag", ml_thin(ml_flag, 2000))');
  L.push('ml_order = argsort(-ml_scores)', 'for ml_i = 0 to min(10, rows(Z)) - 1', '    print("@ml top " + str(ml_ok[ml_order[ml_i]]) + " " + str(ml_scores[ml_order[ml_i]]))', 'end for');
  if (hasY) {
    L.push('# ---- Against the label column (1 = anomaly): read-only, never used to fit');
    L.push('ml_metric("roc_auc", roc_auc(y, ml_scores))', 'ml_metric("precision", sum(ml_flag .* y) / max(sum(ml_flag), 1))', 'ml_metric("recall", sum(ml_flag .* y) / max(sum(y), 1))');
  }
  L.push('', '# ---- Picture: the first two principal components when there are more than two features');
  L.push('if cols(Z) > 2', '    ml_xy = pca(Z, 2)', 'else if cols(Z) == 2', '    ml_xy = Z', 'else', "    ml_xy = vstack(Z[:, 0], zeros(rows(Z)))'", 'end if');
  L.push('ml_ix = round(linspace(0, rows(ml_xy) - 1, min(rows(ml_xy), 1500)))', 'ml_emit("scatter_x", ml_xy[ml_ix, 0])', 'ml_emit("scatter_y", ml_xy[ml_ix, 1])', 'ml_emit("scatter_c", ml_flag[ml_ix])');
  return L;
}

/** The whole experiment as one Qu script. Throws nothing: validate first. */
export function buildMlScript(spec: MlSpec): string {
  const n: Need = { set: new Set() };
  const body: string[] = [];
  const sup = isSupervised(spec.task);
  const m = modelById(spec.model);
  const head = [
    `# Qu Studio machine learning lab: ${spec.task}${sup || spec.task === 'clustering' || spec.task === 'anomaly' || spec.task === 'reduction' ? '' : ''}${spec.mode === 'compare' && sup ? ', model comparison' : m ? ` with ${m.label}` : ''}`,
    '# Generated from the panel; edit it freely and press Run. Every number on screen is printed by this script.',
    emitSeedLine(spec),
    '',
  ];
  body.push(...dataBlock(spec), '');
  body.push(...featureBlock(spec, n, sup), '');
  if (sup) {
    body.push(...missingBlock(spec, n, true), ...balanceBlock(spec, n), '');
  } else {
    body.push(...colorBlock(spec, n));
    body.push(...missingBlock(spec, n, colorBlock(spec, n).length > 0, false), '');
  }
  body.push(...prepFns(spec, n), '');
  if (sup && spec.mode === 'compare') body.push(...compareBlock(spec, n));
  else if (sup && spec.model === 'net') body.push(...netBlock(spec, n), ...saveLines(spec));
  else if (sup && spec.eval.method === 'cv') body.push(...supervisedCv(spec, n), ...saveLines(spec));
  else if (sup) body.push(...supervisedHoldout(spec, n), ...saveLines(spec));
  else if (spec.task === 'clustering') body.push(...clusterBlock(spec, n));
  else if (spec.task === 'reduction') body.push(...reductionBlock(spec, n));
  else body.push(...anomalyBlock(spec, n));
  need(n, 'emit');
  if (spec.meta.positional && spec.meta.hasHeader) need(n, 'skip1');
  return [...head, ...helperBlock(n), ...body, ''].join('\n');
}

// ---------------------------------------------------------------- predict on new data

/** A script that loads a saved model and writes predictions for a new CSV. */
export function buildPredictScript(spec: MlSpec, io: { modelPath: string; newCsv: string; outCsv: string }): string {
  const n: Need = { set: new Set() };
  const cls = spec.task === 'classification';
  need(n, 'emit', 'num', 'fill');
  const meta = spec.meta;
  const L: string[] = [
    `# Predict on new data with the model saved by the ML lab (${modelById(spec.model)?.label ?? spec.model})`,
    '# The new CSV needs the same feature columns as the training data' + (meta.positional || !meta.hasHeader ? ' in the same positions.' : ' (same names).'),
    '',
  ];
  const feat = featureBlock({ ...spec, task: 'regression' }, n, false);
  L.push(`# ---- Data: the new rows`, `ml_t = ${readCsvCall(io.newCsv, { ...meta, positional: meta.positional })}`, '', ...feat, '');
  L.push('# ---- The saved model and its preprocessing', `load(${q(fpath(io.modelPath))})`);
  L.push('X = ml_fillna(X, ml_P.fill)   # a missing value is replaced by the training-set statistic');
  L.push(spec.scaling === 'none' ? 'Xn = X' : 'Xn = ml_P.scaler.transform(X)');
  if (cls) {
    const classes = classLevels(spec);
    if (spec.model === 'net') L.push('ml_pred = argmax(softmax_rows(ml_model.predict(Xn)), axis = 1)');
    else L.push('ml_pred = ml_model.predict(Xn)');
    L.push(`ml_classes = ${tuple(classes)}`, 'ml_names = lines("")', 'for ml_i = 0 to length(ml_pred) - 1', '    ml_names = append(ml_names, ml_classes[ml_pred[ml_i]])', 'end for');
    L.push('ml_out = table(row = linspace(0, length(ml_pred) - 1, length(ml_pred)), prediction = ml_names)');
  } else {
    if (spec.model === 'net') L.push('ml_pred = ml_model.predict(Xn) * ml_ys + ml_ym');
    else L.push('ml_pred = ml_model.predict(Xn)');
    L.push('ml_out = table(row = linspace(0, length(ml_pred) - 1, length(ml_pred)), prediction = ml_pred)');
  }
  L.push(`write_csv(ml_out, ${q(fpath(io.outCsv))})`, 'print("wrote {length(ml_pred)} predictions")');
  if (meta.positional && meta.hasHeader) need(n, 'skip1');
  return [...helperBlock(n), ...L, ''].join('\n');
}

// ---------------------------------------------------------------- reading the output

export interface EpochRow {
  epoch: number;
  loss: number;
  valLoss: number;
  metric: number;
  valMetric: number;
}
export interface GridRow {
  mean: number;
  std: number;
  params: number[];
}
export interface CompareRow {
  id: string;
  mean: number;
  std: number;
  folds: number[];
}
export interface MlResults {
  metrics: Array<{ name: string; value: number }>;
  cvMetrics: Array<{ name: string; mean: number; std: number }>;
  cm: number[][];
  series: Record<string, number[]>;
  epochs: EpochRow[];
  stop: { epoch: number; best: number } | null;
  grid: GridRow[];
  best: { score: number; params: number[] } | null;
  gridMetric: string;
  compare: CompareRow[];
  compareFail: Array<{ id: string; message: string }>;
  centers: Array<{ id: number; values: number[] }>;
  clusterSizes: Array<{ id: number; n: number }>;
  clusters: { k: number; noise: number } | null;
  top: Array<{ row: number; score: number }>;
  notes: string[];
  data: { used: number; total: number; features: number } | null;
  split: { train: number; test: number } | null;
  saved: string;
  text: string[];
}

export const emptyResults = (): MlResults => ({
  metrics: [], cvMetrics: [], cm: [], series: {}, epochs: [], stop: null, grid: [], best: null, gridMetric: '', compare: [], compareFail: [],
  centers: [], clusterSizes: [], clusters: null, top: [], notes: [], data: null, split: null, saved: '', text: [],
});

/** One output token as a number: NaN and inf as the engine prints them. */
export function qnum(tok: string): number {
  const t = tok.trim();
  if (t === 'inf' || t === '+inf') return Infinity;
  if (t === '-inf') return -Infinity;
  const v = Number(t);
  return Number.isNaN(v) ? NaN : v;
}

/** Folds one output line into `res` (mutating it). Returns true when the line was a tagged one. */
export function ingestMlLine(res: MlResults, line: string): boolean {
  if (!line.startsWith('@ml ')) {
    res.text.push(line);
    return false;
  }
  const rest = line.slice(4);
  const sp = rest.indexOf(' ');
  const tag = sp < 0 ? rest : rest.slice(0, sp);
  const body = sp < 0 ? '' : rest.slice(sp + 1);
  const toks = body.split(/\s+/).filter((t) => t.length > 0);
  const nums = (a: string[]): number[] => a.map(qnum);
  switch (tag) {
    case 'metric': res.metrics = res.metrics.filter((m) => m.name !== toks[0]).concat({ name: toks[0], value: qnum(toks[1]) }); break;
    case 'cvmetric': res.cvMetrics = res.cvMetrics.filter((m) => m.name !== toks[0]).concat({ name: toks[0], mean: qnum(toks[1]), std: qnum(toks[2]) }); break;
    case 'series': res.series[toks[0]] = nums(toks.slice(1)); break;
    case 'data': res.data = { used: qnum(toks[0]), total: qnum(toks[1]), features: qnum(toks[2]) }; break;
    case 'split': res.split = { train: qnum(toks[0]), test: qnum(toks[1]) }; break;
    case 'epoch': {
      const v = nums(toks);
      res.epochs.push({ epoch: v[0], loss: v[1], valLoss: v[2], metric: v[3], valMetric: v[4] });
      break;
    }
    case 'stop': res.stop = { epoch: qnum(toks[0]), best: qnum(toks[1]) }; break;
    case 'grid': { const v = nums(toks); res.grid.push({ mean: v[0], std: v[1], params: v.slice(2) }); break; }
    case 'best': { const v = nums(toks); res.best = { score: v[0], params: v.slice(1) }; break; }
    case 'gridmetric': res.gridMetric = toks[0] ?? ''; break;
    case 'compare': { const v = nums(toks.slice(1)); res.compare = res.compare.filter((c) => c.id !== toks[0]).concat({ id: toks[0], mean: v[0], std: v[1], folds: v.slice(2) }); break; }
    case 'compare_fail': res.compareFail.push({ id: toks[0], message: toks.slice(1).join(' ') }); break;
    case 'clusters': res.clusters = { k: qnum(toks[0]), noise: qnum(toks[1]) }; break;
    case 'cluster_size': res.clusterSizes.push({ id: qnum(toks[0]), n: qnum(toks[1]) }); break;
    case 'top': res.top.push({ row: qnum(toks[0]), score: qnum(toks[1]) }); break;
    case 'note': res.notes.push(body); break;
    case 'saved': res.saved = body; break;
    default: {
      // "cm_row 0 v...", "center 2 v...", "loading 0 v...", plain "importance v..."
      const m = /^(cm_row|center|loading)$/.exec(tag);
      if (m && toks.length) {
        const idx = qnum(toks[0]);
        const v = nums(toks.slice(1));
        if (tag === 'cm_row') res.cm[idx] = v;
        else if (tag === 'center') res.centers.push({ id: idx, values: v });
        else res.series[`loading_${idx}`] = v;
      } else res.series[tag] = nums(toks);
    }
  }
  return true;
}

// The emit helper prints "@ml <tag> v1 v2 ..." where <tag> may be "cm_row 0":
// handled above because the tag is the first word and the index the first token.

/** Folds a whole output text. */
export function parseMlOutput(text: string): MlResults {
  const res = emptyResults();
  for (const line of text.split(/\r?\n/)) if (line.length) ingestMlLine(res, line);
  return res;
}

/** The Compare table, best first. */
export function rankCompare(rows: CompareRow[], metric: string, task: 'classification' | 'regression'): CompareRow[] {
  const def = COMPARE_METRICS[task].find((m) => m.id === metric);
  const hi = def ? def.higherIsBetter : true;
  return rows.slice().sort((a, b) => (hi ? b.mean - a.mean : a.mean - b.mean));
}

/** The script line an engine error points at, when it names one. */
export function errorLine(error: string, script: string): { line: number; text: string } | null {
  const m = /(?:\bat [^\s:]+:|\bline )(\d+)/.exec(error);
  if (!m) return null;
  const line = Number(m[1]);
  const text = script.split('\n')[line - 1];
  return text === undefined ? null : { line, text };
}

/** Splits streamed output into whole lines; the remainder waits for its newline. */
export function splitChunk(pending: string, chunk: string): { lines: string[]; rest: string } {
  const parts = (pending + chunk).split(/\r?\n/);
  const rest = parts.pop() ?? '';
  return { lines: parts.filter((l) => l.length > 0), rest };
}

export { isIdent };
export type { ColType };
