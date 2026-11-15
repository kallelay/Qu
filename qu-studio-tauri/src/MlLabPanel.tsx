import React, { useEffect, useMemo, useState } from 'react';
import { create } from 'zustand';
import { AlertCircle, Copy, FileCode2, FolderOpen, Pencil, Play, RotateCcw, Square } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { DataInput, type SessionMatrix } from './MlLabDataInput';
import { BarList, Card, Columns, ConfusionGrid, LineChart, PALETTE, ScatterChart } from './MlLabCharts';
import { exampleById, levelCounts, numericHistogram, type ColType, type ParsedTable, type TableMeta } from './mlLabData';
import {
  COMPARE_METRICS,
  DEFAULT_ML,
  MODELS,
  OPTIMIZERS,
  TASKS,
  buildMlScript,
  buildPredictScript,
  classLevels,
  defaultModel,
  defaultSpec,
  emptyResults,
  errorLine,
  featureNames,
  isSupervised,
  modelById,
  modelsFor,
  paramValue,
  parseMlOutput,
  rangeText,
  rankCompare,
  splitChunk,
  ingestMlLine,
  tunableParams,
  validateMl,
  withTask,
  type MlResults,
  type MlSpec,
  type NetLayer,
  type ParamDef,
  type SourceSpec,
  type Task,
} from './mlLabDesign';

export interface TemplateEntry {
  id: string;
  name: string;
  category: string;
  template: string;
}

export interface MlLabPanelProps extends LiveSerialPlotPanelProps {
  /** Opens `code` as a new editor buffer named `name` in the Code tab. */
  onOpenInEditor?: (code: string, name: string) => void;
  /** The older ML example scripts, offered in the Examples menu. */
  templates?: TemplateEntry[];
  /** Numeric variables of the Code tab's session (a data source). */
  variables?: SessionMatrix[];
}

/** The lab runs in its own `qu kernel`, restarted before every run. */
export const ML_SESSION = 'qu-studio-ml';

interface ExecuteShape {
  success: boolean;
  output?: string;
  error: string | null;
  elapsed_ms?: number;
}

type RunState = 'idle' | 'running' | 'done' | 'stopped' | 'failed';

// ---------------------------------------------------------------- store
// Module-level so a run keeps streaming across a trip to another tab.

const STORE_KEY = 'qu-studio.mlLab.v1';

function loadSpec(): MlSpec {
  try {
    const raw = window.localStorage.getItem(STORE_KEY);
    if (raw) {
      const s = JSON.parse(raw) as MlSpec;
      if (s && s.source && s.meta && (s.source.kind === 'example' || s.source.kind === 'file') && modelById(s.model)) return s;
    }
  } catch {
    /* no storage: fine */
  }
  return DEFAULT_ML;
}
function saveSpec(spec: MlSpec) {
  try {
    if (spec.source.kind === 'example' || spec.source.kind === 'file') window.localStorage.setItem(STORE_KEY, JSON.stringify(spec));
  } catch {
    /* ignore */
  }
}

interface MlStore {
  spec: MlSpec;
  /** The parsed rows behind spec.meta (null for a built-in example). */
  parsed: ParsedTable | null;
  state: RunState;
  error: string;
  results: MlResults;
  /** What the results on screen were made from. */
  ranSpec: MlSpec | null;
  startedAt: number;
  endedAt: number;
  version: number;
  /** The script the user is editing by hand (null: the panel's own). */
  edited: string | null;
  setSpec: (patch: Partial<MlSpec>) => void;
  replaceSpec: (spec: MlSpec) => void;
}

export const useMlLab = create<MlStore>((set) => ({
  spec: typeof window === 'undefined' ? DEFAULT_ML : loadSpec(),
  parsed: null,
  state: 'idle',
  error: '',
  results: emptyResults(),
  ranSpec: null,
  startedAt: 0,
  endedAt: 0,
  version: 0,
  edited: null,
  setSpec: (patch) => set((s) => {
    const spec = { ...s.spec, ...patch };
    saveSpec(spec);
    return { spec };
  }),
  replaceSpec: (spec) => {
    saveSpec(spec);
    set({ spec });
  },
}));

let live: MlResults = emptyResults();
let pending = '';
let flushQueued = false;

function flush() {
  if (flushQueued) return;
  flushQueued = true;
  window.setTimeout(() => {
    flushQueued = false;
    useMlLab.setState((s) => ({ results: { ...live }, version: s.version + 1 }));
  }, 60);
}
function ingest(chunk: string) {
  const { lines, rest } = splitChunk(pending, chunk);
  pending = rest;
  for (const l of lines) ingestMlLine(live, l);
  if (lines.length) flush();
}

let listening = false;
async function ensureListener() {
  if (listening || typeof (window as any).__TAURI_IPC__ === 'undefined') return;
  listening = true;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<{ session: string; text: string }>('qu-run-output', (e) => {
      if (e.payload?.session === ML_SESSION) ingest(String(e.payload.text ?? ''));
    });
  } catch (err) {
    listening = false;
    console.error('ML lab: output listener failed', err);
  }
}

// ---------------------------------------------------------------- helpers

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border min-w-0';
const fmtNum = (v: number | null | undefined, digits = 4): string => (v == null || !Number.isFinite(v) ? 'n/a' : String(Number(v.toPrecision(digits))));

const METRIC_LABEL: Record<string, string> = {
  accuracy: 'Accuracy', precision: 'Precision (macro)', recall: 'Recall (macro)', f1: 'F1 (macro)', roc_auc: 'ROC-AUC', avg_precision: 'Average precision',
  r2: 'R-squared', rmse: 'RMSE', mae: 'MAE', inertia: 'Inertia (within-cluster SSE)', silhouette: 'Silhouette', purity: 'Agreement with label column',
  kept_components: 'Components kept', kept_variance: 'Variance kept', flagged: 'Rows flagged', threshold: 'Score threshold',
};
const METRIC_HELP: Record<string, string> = {
  accuracy: 'Share of rows with the right class', f1: 'Balance of precision and recall, averaged over classes', roc_auc: '1 = perfect ranking, 0.5 = chance',
  r2: '1 = perfect, 0 = no better than the mean', rmse: 'Typical error, in the target\'s units', silhouette: '1 = tight separated clusters, 0 = overlapping',
  purity: 'Share of rows whose cluster\'s majority label is their own (reading aid, not used to fit)',
};

const baseName = (p: string): string => p.split(/[\\/]/).pop() ?? p;

async function pickSavePath(current: string, ext: string): Promise<string | null> {
  try {
    const { save } = await import('@tauri-apps/api/dialog');
    const r = await save({ defaultPath: current || undefined, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    return typeof r === 'string' ? r : null;
  } catch {
    return null;
  }
}
async function pickOpenPath(): Promise<string | null> {
  try {
    const { open } = await import('@tauri-apps/api/dialog');
    const r = await open({ multiple: false, filters: [{ name: 'Table', extensions: ['csv', 'tsv', 'txt'] }] });
    return typeof r === 'string' ? r : null;
  } catch {
    return null;
  }
}
async function defaultModelPath(): Promise<string> {
  try {
    const { appDataDir, join } = await import('@tauri-apps/api/path');
    const { createDir } = await import('@tauri-apps/api/fs');
    const dir = await join(await appDataDir(), 'ml-lab');
    await createDir(dir, { recursive: true }).catch(() => undefined);
    return await join(dir, 'model.json');
  } catch {
    return '';
  }
}

// ---------------------------------------------------------------- panel

export const MlLabPanel: React.FC<MlLabPanelProps> = ({ theme, invoke, onOpenInEditor, templates = [], variables = [] }) => {
  const spec = useMlLab((s) => s.spec);
  const setSpec = useMlLab((s) => s.setSpec);
  const parsed = useMlLab((s) => s.parsed);
  const state = useMlLab((s) => s.state);
  const error = useMlLab((s) => s.error);
  const results = useMlLab((s) => s.results);
  const ranSpec = useMlLab((s) => s.ranSpec);
  const startedAt = useMlLab((s) => s.startedAt);
  const endedAt = useMlLab((s) => s.endedAt);
  const edited = useMlLab((s) => s.edited);
  const [copied, setCopied] = useState('');
  const [now, setNow] = useState(() => performance.now());
  const [showEdit, setShowEdit] = useState(false);
  const [predict, setPredict] = useState({ newCsv: '', outCsv: '', code: '', output: '', error: '', running: false });

  useEffect(() => {
    void ensureListener();
  }, []);
  useEffect(() => {
    if (state !== 'running') return;
    const id = window.setInterval(() => setNow(performance.now()), 200);
    return () => window.clearInterval(id);
  }, [state]);

  const running = state === 'running';
  const problem = useMemo(() => validateMl(spec), [spec]);
  const script = useMemo(() => (problem ? '' : buildMlScript(spec)), [spec, problem]);
  const shown = edited ?? script;
  const sup = isSupervised(spec.task);
  const model = modelById(spec.model);
  const classes = classLevels(spec);

  // -------- loading data
  const adopt = (source: SourceSpec, meta: TableMeta, p: ParsedTable | null) => {
    const cur = useMlLab.getState().spec;
    const ex = source.kind === 'example' ? exampleById(source.exampleId) : undefined;
    const task: Task = ex ? ex.task : cur.task;
    let next: MlSpec = defaultSpec(source, meta, task, ex ? ex.target : isSupervised(task) ? undefined : '');
    next = {
      ...next,
      eval: cur.eval,
      net: cur.net,
      encoding: cur.encoding,
      missing: cur.missing,
      scaling: cur.scaling,
      save: cur.save,
      sweepK: cur.sweepK,
      params: cur.params,
      tune: cur.tune,
      mode: cur.mode,
      model: ex ? defaultModel(task) : modelById(cur.model)?.tasks.includes(task) ? cur.model : defaultModel(task),
    };
    next = withTask(next, task);
    if (ex) next = { ...next, model: defaultModel(task), compare: { ...next.compare, models: [] } };
    useMlLab.setState({ parsed: p, spec: next, edited: null });
    saveSpec(next);
  };
  const adoptMeta = (meta: TableMeta) => {
    const cur = useMlLab.getState().spec;
    const features = cur.features.filter((f) => meta.columns.find((c) => c.name === f)?.type !== 'ignore');
    useMlLab.getState().replaceSpec({ ...cur, meta, features });
    const p = useMlLab.getState().parsed;
    if (p) useMlLab.setState({ parsed: { ...p, meta } });
  };

  // -------- running
  const execute = async (code: string): Promise<void> => {
    pending = '';
    live = emptyResults();
    useMlLab.setState({ state: 'running', error: '', results: emptyResults(), ranSpec: spec, startedAt: performance.now(), endedAt: 0, version: useMlLab.getState().version + 1 });
    try {
      await invoke('repl_restart', { session: ML_SESSION });
      const r = await invoke<ExecuteShape>('repl_run', { code, session: ML_SESSION, cwd: null, file: 'ml_lab.qu' });
      await new Promise((res) => setTimeout(res, 80));
      if (pending) ingest('\n');
      // The response carries the whole output: parse it again so a dropped event cannot lose a line.
      const full = typeof r.output === 'string' && r.output.length ? parseMlOutput(r.output) : null;
      const stopped = !r.success && /^Stopped\./.test(r.error ?? '');
      useMlLab.setState((s) => ({
        state: r.success ? 'done' : stopped ? 'stopped' : 'failed',
        error: r.success || stopped ? '' : r.error ?? 'The script did not run',
        results: full ?? { ...live },
        endedAt: performance.now(),
        version: s.version + 1,
      }));
    } catch (err) {
      useMlLab.setState({ state: 'failed', error: String(err), endedAt: performance.now(), results: { ...live } });
    }
  };
  const run = () => {
    if (running) return;
    if (edited === null && problem) return;
    void execute(shown);
  };
  const stop = async () => {
    try {
      await invoke('repl_stop', { session: ML_SESSION });
    } catch (err) {
      useMlLab.setState({ error: `Could not stop the run: ${err}` });
    }
  };
  const copy = (what: string, value: string) => {
    void navigator.clipboard?.writeText(value).then(() => {
      setCopied(what);
      setTimeout(() => setCopied(''), 1200);
    });
  };

  // -------- predict on new data
  const modelPath = spec.save.path;
  const predictReady = sup && spec.save.enabled && !!modelPath && predict.newCsv.trim().length > 0 && !problem;
  const buildPredict = () => {
    const out = predict.outCsv.trim() || predict.newCsv.replace(/\.[^./\\]+$/, '') + '_predictions.csv';
    const code = buildPredictScript(spec, { modelPath, newCsv: predict.newCsv.trim(), outCsv: out });
    setPredict((p) => ({ ...p, code, outCsv: out, output: '', error: '' }));
  };
  const runPredict = async () => {
    setPredict((p) => ({ ...p, running: true, output: '', error: '' }));
    try {
      const r = await invoke<ExecuteShape>('execute_code', { request: { code: predict.code, file_path: null } });
      setPredict((p) => ({ ...p, running: false, output: r.output ?? '', error: r.success ? '' : r.error ?? 'The predict script did not run' }));
    } catch (err) {
      setPredict((p) => ({ ...p, running: false, error: String(err) }));
    }
  };

  // -------- small controls
  const field = (label: string, control: React.ReactNode, title?: string) => (
    <label className="flex flex-col gap-1 text-xs flex-1 min-w-0" title={title}>
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      {control}
    </label>
  );
  const select = (value: string, onChange: (v: string) => void, options: Array<{ id: string; label: string }>, aria?: string) => (
    <select value={value} onChange={(e) => onChange(e.target.value)} className={inputClass} style={{ borderColor: border }} aria-label={aria}>
      {options.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
    </select>
  );
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const numInput = (key: string, value: number | string, onValue: (v: number) => void, extra: React.InputHTMLAttributes<HTMLInputElement> = {}) => (
    <input
      type="number"
      value={drafts[key] ?? value}
      onChange={(e) => {
        const raw = e.target.value;
        setDrafts((d) => ({ ...d, [key]: raw }));
        const v = Number(raw);
        if (raw.trim() !== '' && Number.isFinite(v)) onValue(v);
      }}
      onBlur={() => setDrafts((d) => {
        const { [key]: _, ...rest } = d;
        return rest;
      })}
      className={inputClass}
      style={{ borderColor: border }}
      aria-label={key}
      {...extra}
    />
  );
  const section = (title: string, children: React.ReactNode, hint?: string) => (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between">
        <span className="text-sm font-medium">{title}</span>
        {hint && <span className="text-[11px]" style={{ color: 'var(--qu-muted)' }}>{hint}</span>}
      </div>
      {children}
    </div>
  );

  // -------- columns
  const cols = spec.meta.columns;
  const targetOptions = [
    ...(sup ? [] : [{ id: '', label: 'none' }]),
    ...cols.filter((c) => c.type !== 'ignore' && (spec.task !== 'regression' || c.type === 'numeric')).map((c) => ({ id: c.name, label: c.name })),
  ];
  const setTarget = (t: string) => setSpec({ target: t, features: spec.features.filter((f) => f !== t) });
  const toggleFeature = (name: string, on: boolean) => setSpec({ features: on ? [...spec.features, name].sort((a, b) => cols.findIndex((c) => c.name === a) - cols.findIndex((c) => c.name === b)) : spec.features.filter((f) => f !== name) });

  const balance = useMemo(() => {
    if (!spec.target) return null;
    const t = cols.find((c) => c.name === spec.target);
    if (!t) return null;
    if (parsed && sup && spec.task === 'classification') return { kind: 'classes' as const, ...(levelCounts(parsed, spec.target) ?? { levels: [], counts: [] }) };
    if (parsed && spec.task === 'regression') {
      const h = numericHistogram(parsed, spec.target);
      return h ? { kind: 'hist' as const, ...h } : null;
    }
    return null;
  }, [parsed, spec.target, spec.task, cols, sup]);

  // -------- model section
  const renderParam = (p: ParamDef) => {
    const v = paramValue(spec, spec.model, p);
    const key = `${spec.model}.${p.id}`;
    if (p.kind === 'select') {
      return field(p.label, select(String(v), (x) => setSpec({ params: { ...spec.params, [key]: x } }), p.options!.map((o) => ({ id: o, label: o })), p.label), p.help);
    }
    return field(p.label, numInput(key, v, (x) => setSpec({ params: { ...spec.params, [key]: x } }), { min: p.min, max: p.max, step: p.step ?? (p.kind === 'int' ? 1 : 0.1) }), p.help);
  };

  const net = spec.net;
  const setNet = (patch: Partial<typeof net>) => setSpec({ net: { ...net, ...patch } });
  const setLayer = (i: number, patch: Partial<NetLayer>) => setNet({ layers: net.layers.map((l, j) => (j === i ? { ...l, ...patch } : l)) });
  const netBuilder = (
    <div className="flex flex-col gap-2" data-testid="ml-net">
      <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
        This engine builds dense layers with relu or linear activation, optional dropout, and trains full-batch (no mini-batches); there are no convolutional or other layer types for tabular data here.
      </div>
      {net.layers.map((l, i) => (
        <div key={i} className="flex items-end gap-1">
          {field(`Layer ${i + 1} units`, numInput(`layer${i}units`, l.units, (x) => setLayer(i, { units: Math.round(x) }), { min: 1, step: 1 }))}
          {field('Activation', select(l.activation, (a) => setLayer(i, { activation: a as 'relu' | 'none' }), [{ id: 'relu', label: 'relu' }, { id: 'none', label: 'linear' }]))}
          {field('Dropout', numInput(`layer${i}drop`, l.dropout, (x) => setLayer(i, { dropout: x }), { min: 0, max: 0.9, step: 0.05 }))}
          <button className="px-2 py-1.5 rounded-md border text-xs" style={{ borderColor: border }} disabled={net.layers.length <= 1} onClick={() => setNet({ layers: net.layers.filter((_, j) => j !== i) })} title="Remove this layer">x</button>
        </div>
      ))}
      <button className="text-xs underline self-start" style={{ color: 'var(--qu-accent)' }} onClick={() => setNet({ layers: [...net.layers, { units: 8, activation: 'relu', dropout: 0 }] })}>Add a layer</button>
      <div className="flex gap-2">
        {field('Optimiser', select(net.optimizer, (o) => setNet({ optimizer: o as typeof net.optimizer }), OPTIMIZERS.map((o) => ({ id: o, label: o }))))}
        {field('Learning rate', numInput('lr', net.lr, (x) => setNet({ lr: x }), { min: 0, step: 0.001 }))}
      </div>
      <div className="flex gap-2">
        {field('Epochs', numInput('epochs', net.epochs, (x) => setNet({ epochs: Math.round(x) }), { min: 1, step: 10 }))}
        {field('Report every', numInput('report', net.reportEvery, (x) => setNet({ reportEvery: Math.round(x) }), { min: 1, step: 1 }), 'A curve point and validation check every N epochs; the engine trains in chunks of N and the optimiser restarts each chunk')}
      </div>
      <div className="flex gap-2">
        {field('Validation share', numInput('val', net.valFraction, (x) => setNet({ valFraction: x }), { min: 0, max: 0.5, step: 0.05 }))}
        {field('Early stop patience', numInput('patience', net.patience, (x) => setNet({ patience: Math.round(x) }), { min: 0, step: 1 }), '0 = train all epochs; otherwise stop after this many reports without a better validation loss and keep the best weights')}
      </div>
    </div>
  );

  const tp = sup && spec.mode === 'single' && spec.model !== 'net' ? tunableParams(spec.model) : [];
  const compareList = MODELS.filter((m) => m.comparable && m.tasks.includes(spec.task) && (!m.binaryOnly || classes.length === 2));
  const compareMetrics = sup ? COMPARE_METRICS[spec.task as 'classification' | 'regression'].filter((m) => !(m.id === 'roc_auc' && classes.length !== 2) && !(m.id === 'accuracy' && spec.task === 'classification' && classes.length > 2)) : [];

  // -------- results
  const res = results;
  const rs = ranSpec ?? spec;
  const rClasses = classLevels(rs);
  const rNames = featureNames(rs);
  const metricOf = (n: string) => res.metrics.find((m) => m.name === n)?.value;
  const wallMs = (endedAt || now) - startedAt;
  const hasResults = res.metrics.length + res.cvMetrics.length + res.compare.length + res.epochs.length + res.grid.length > 0 || Object.keys(res.series).length > 0 || res.clusters !== null;
  const S = res.series;
  const pair = (a: string, b: string) => (S[a] && S[b] && S[a].length === S[b].length ? [S[a], S[b]] : null);
  const roc = pair('roc_fpr', 'roc_tpr');
  const pr = pair('pr_recall', 'pr_precision');
  const imp = S.importance && S.importance.length === rNames.length ? rNames.map((n, i) => ({ label: n, value: S.importance[i] })).sort((a, b) => b.value - a.value) : null;
  const yPair = pair('y_true', 'y_pred');
  const resid = pair('y_pred', 'residual');
  const scat = S.scatter_x && S.scatter_y ? { x: S.scatter_x, y: S.scatter_y, c: S.scatter_c } : null;
  const ranked = res.compare.length ? rankCompare(res.compare, rs.compare.metric, rs.task === 'regression' ? 'regression' : 'classification') : [];
  const ev = res.epochs;
  const epochX = ev.map((e) => e.epoch);
  const centersProjected = undefined;
  void centersProjected;
  const tunedNames = rs.model ? tunableParams(rs.model).map((p) => p.label) : [];

  const scoreHist = useMemo(() => {
    if (!S.scores) return null;
    const lo = Math.min(...S.scores);
    const hi = Math.max(...S.scores);
    const counts = new Array(15).fill(0);
    for (const v of S.scores) counts[Math.min(14, Math.floor(((v - lo) / (hi - lo + 1e-12)) * 15))]++;
    return { lo, hi, counts };
  }, [S.scores]);

  const stateLabel: Record<RunState, string> = { idle: 'Idle', running: 'Running', done: 'Done', stopped: 'Stopped', failed: 'Failed' };
  const headline = (): string => {
    if (state === 'idle') return 'Press Run';
    const bits: string[] = [stateLabel[state]];
    if (res.data) bits.push(`${res.data.used} of ${res.data.total} rows used`);
    if (res.split) bits.push(`${res.split.train} train / ${res.split.test} test`);
    if (startedAt) bits.push(`${fmtNum(wallMs / 1000, 3)} s`);
    return bits.join(' - ');
  };
  const errLine = error ? errorLine(error, shown) : null;

  const taskTabs = (
    <div className="flex flex-wrap gap-1" role="tablist" aria-label="Task">
      {TASKS.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={spec.task === t.id}
          title={t.blurb}
          className="px-2 py-1 rounded-md border text-xs"
          style={{ borderColor: spec.task === t.id ? 'var(--qu-accent)' : border, background: spec.task === t.id ? 'var(--qu-hover)' : 'transparent' }}
          onClick={() => useMlLab.getState().replaceSpec(withTask(spec, t.id))}
        >
          {t.label}
        </button>
      ))}
    </div>
  );

  return (
    <div className="qu-inspector flex flex-1 min-w-0 min-h-0" data-theme={theme} data-testid="ml-panel">
      {/* ---------------- left: setup */}
      <div className="w-[23rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-4 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm font-medium" title="From a table to a trained, evaluated model: everything is the generated Qu script">Machine learning lab</span>
          <select
            value=""
            onChange={(e) => {
              const t = templates.find((x) => x.id === e.target.value);
              if (t && onOpenInEditor) onOpenInEditor(t.template, `${t.name.replace(/[^A-Za-z0-9]+/g, '_')}.qu`);
            }}
            className="px-2 py-1 rounded-md text-xs bg-transparent border"
            style={{ borderColor: border }}
            aria-label="Examples"
            disabled={!templates.length || !onOpenInEditor}
          >
            <option value="" disabled>Examples…</option>
            {templates.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </select>
        </div>

        {section('1. Data', (
          <DataInput source={spec.source} meta={spec.meta} parsed={parsed} variables={variables} onLoad={adopt} onMeta={adoptMeta} />
        ))}

        {cols.length > 0 && section('2. Columns', (
          <div className="flex flex-col gap-2">
            {field(sup ? 'Target (what to predict)' : 'Label column (optional: colours and scoring only)', select(spec.target, setTarget, targetOptions, 'target column'))}
            {balance?.kind === 'classes' && balance.levels.length > 0 && <Columns counts={balance.counts} labels={balance.levels} testId="ml-balance" xNote="class balance" />}
            {balance?.kind === 'hist' && <Columns counts={balance.counts} height={70} testId="ml-balance" xNote={`target histogram, ${fmtNum(balance.lo)} .. ${fmtNum(balance.hi)}`} />}
            <div className="flex flex-col gap-1 text-xs">
              <div className="flex items-center justify-between">
                <span style={{ color: 'var(--qu-muted)' }}>Features ({spec.features.length})</span>
                <span className="flex gap-2">
                  <button className="underline" style={{ color: 'var(--qu-accent)' }} onClick={() => setSpec({ features: cols.filter((c) => c.name !== spec.target && c.type !== 'ignore').map((c) => c.name) })}>all</button>
                  <button className="underline" style={{ color: 'var(--qu-accent)' }} onClick={() => setSpec({ features: [] })}>none</button>
                </span>
              </div>
              <div className="max-h-32 overflow-y-auto flex flex-col gap-0.5 rounded border p-1" style={{ borderColor: border }}>
                {cols.map((c) => (
                  <label key={c.name} className="flex items-center gap-1" style={{ opacity: c.type === 'ignore' || c.name === spec.target ? 0.45 : 1 }}>
                    <input type="checkbox" checked={spec.features.includes(c.name)} disabled={c.type === 'ignore' || c.name === spec.target} onChange={(e) => toggleFeature(c.name, e.target.checked)} />
                    <span className="font-mono truncate" title={c.name}>{c.name}</span>
                    <span style={{ color: 'var(--qu-muted)' }}>{c.type}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="flex gap-2">
              {field('Category encoding', select(spec.encoding, (v) => setSpec({ encoding: v as MlSpec['encoding'] }), [{ id: 'onehot', label: 'one-hot' }, { id: 'label', label: 'label codes' }]), 'One-hot makes a 0/1 column per level (the first is dropped); label codes use one column of 0, 1, 2...')}
              {field('Missing values', select(spec.missing, (v) => setSpec({ missing: v as MlSpec['missing'] }), [{ id: 'drop', label: 'drop rows' }, { id: 'mean', label: 'mean impute' }, { id: 'median', label: 'median impute' }]))}
            </div>
            {field('Feature scaling', select(spec.scaling, (v) => setSpec({ scaling: v as MlSpec['scaling'] }), [{ id: 'none', label: 'none' }, { id: 'standard', label: 'standardise (z-score)' }, { id: 'minmax', label: 'min-max to [0, 1]' }]), 'fit_scaler on the training rows, applied unchanged to the rest')}
          </div>
        ))}

        {cols.length > 0 && section('3. Task and model', (
          <div className="flex flex-col gap-2">
            {taskTabs}
            {sup && (
              <div className="qu-live-tabs" role="tablist" aria-label="Mode" style={{ borderRadius: 8 }}>
                <button role="tab" aria-selected={spec.mode === 'single'} onClick={() => setSpec({ mode: 'single' })}>One model</button>
                <button role="tab" aria-selected={spec.mode === 'compare'} onClick={() => setSpec({ mode: 'compare', compare: { ...spec.compare, models: spec.compare.models.length ? spec.compare.models : compareList.slice(0, 5).map((m) => m.id), metric: spec.task === 'classification' && classes.length > 2 ? 'f1' : spec.compare.metric } })} title="Run several models through the same cross-validation folds and rank them">Compare models</button>
              </div>
            )}
            {spec.mode === 'single' || !sup ? (
              <>
                {select(spec.model, (m) => setSpec({ model: m, tune: { ...spec.tune, enabled: false } }), modelsFor(spec.task).map((m) => ({ id: m.id, label: m.label })), 'model')}
                {model && <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>{model.blurb}</span>}
                {spec.model === 'net' ? netBuilder : (
                  <>
                    {model && model.params.length > 0 && <div className="grid grid-cols-2 gap-2">{model.params.map((p) => <React.Fragment key={p.id}>{renderParam(p)}</React.Fragment>)}</div>}
                    {tp.length > 0 && (
                      <div className="flex flex-col gap-1">
                        <label className="flex items-center gap-2 text-xs" title="grid_search cross-validates every combination on the training rows and refits the best one">
                          <input type="checkbox" checked={spec.tune.enabled} onChange={(e) => setSpec({ tune: { ...spec.tune, enabled: e.target.checked } })} aria-label="Tune" />
                          Tune with grid_search (5-fold cross-validation on the training rows)
                        </label>
                        {spec.tune.enabled && tp.map((p) => field(`${p.label}: values to try`, (
                          <input
                            value={rangeText(spec, spec.model, p)}
                            onChange={(e) => setSpec({ tune: { ...spec.tune, ranges: { ...spec.tune.ranges, [`${spec.model}.${p.id}`]: e.target.value } } })}
                            className={`${inputClass} font-mono text-xs`}
                            style={{ borderColor: border }}
                            aria-label={`range ${p.id}`}
                          />
                        )))}
                      </div>
                    )}
                  </>
                )}
              </>
            ) : (
              <div className="flex flex-col gap-1 text-xs" data-testid="ml-compare-setup">
                <span style={{ color: 'var(--qu-muted)' }}>Models to compare (default settings)</span>
                {compareList.map((m) => (
                  <label key={m.id} className="flex items-center gap-1" title={m.blurb}>
                    <input
                      type="checkbox"
                      checked={spec.compare.models.includes(m.id)}
                      onChange={(e) => setSpec({ compare: { ...spec.compare, models: e.target.checked ? [...spec.compare.models, m.id] : spec.compare.models.filter((x) => x !== m.id) } })}
                    />
                    {m.label}
                  </label>
                ))}
                <div className="flex gap-2 mt-1">
                  {field('Folds', numInput('cmpfolds', spec.compare.folds, (x) => setSpec({ compare: { ...spec.compare, folds: Math.round(x) } }), { min: 2, step: 1 }))}
                  {field('Rank by', select(spec.compare.metric, (v) => setSpec({ compare: { ...spec.compare, metric: v } }), compareMetrics.map((m) => ({ id: m.id, label: m.label }))))}
                </div>
                <div className="flex gap-1 mt-1">
                  <button className="underline" style={{ color: 'var(--qu-accent)' }} onClick={() => setSpec({ compare: { ...spec.compare, models: compareList.map((m) => m.id) } })}>all</button>
                  <button className="underline" style={{ color: 'var(--qu-accent)' }} onClick={() => setSpec({ compare: { ...spec.compare, models: [] } })}>none</button>
                </div>
              </div>
            )}
          </div>
        ))}

        {cols.length > 0 && sup && spec.mode === 'single' && section('4. Evaluation', (
          <div className="flex flex-col gap-2">
            <div className="qu-live-tabs" role="tablist" aria-label="Evaluation" style={{ borderRadius: 8 }}>
              <button role="tab" aria-selected={spec.eval.method === 'holdout'} onClick={() => setSpec({ eval: { ...spec.eval, method: 'holdout' } })}>Train / test split</button>
              <button role="tab" aria-selected={spec.eval.method === 'cv'} onClick={() => setSpec({ eval: { ...spec.eval, method: 'cv' }, tune: { ...spec.tune, enabled: false } })} disabled={spec.model === 'net'} title={spec.model === 'net' ? 'The network trains once with a validation split' : ''}>k-fold</button>
            </div>
            {spec.eval.method === 'holdout' ? (
              <label className="flex flex-col gap-1 text-xs">
                <span style={{ color: 'var(--qu-muted)' }}>Test share: {Math.round(spec.eval.testFraction * 100)}%</span>
                <input type="range" min={5} max={50} step={5} value={Math.round(spec.eval.testFraction * 100)} onChange={(e) => setSpec({ eval: { ...spec.eval, testFraction: Number(e.target.value) / 100 } })} aria-label="Test share" />
              </label>
            ) : field('Folds', numInput('folds', spec.eval.folds, (x) => setSpec({ eval: { ...spec.eval, folds: Math.round(x) } }), { min: 2, step: 1 }))}
            <div className="flex gap-2">
              {field('Seed', numInput('seed', spec.eval.seed, (x) => setSpec({ eval: { ...spec.eval, seed: Math.round(x) } }), { step: 1 }), 'Fixes the split, the folds and every random draw: the same seed gives the same numbers')}
            </div>
            <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={spec.eval.importance} onChange={(e) => setSpec({ eval: { ...spec.eval, importance: e.target.checked } })} /> Feature importance (permutation)</label>
            {spec.eval.method === 'holdout' && <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={spec.eval.learningCurve} onChange={(e) => setSpec({ eval: { ...spec.eval, learningCurve: e.target.checked } })} /> Learning curve (8 extra fits)</label>}
          </div>
        ))}

        {cols.length > 0 && !sup && section('4. Settings', (
          <div className="flex flex-col gap-2">
            {field('Seed', numInput('seed', spec.eval.seed, (x) => setSpec({ eval: { ...spec.eval, seed: Math.round(x) } }), { step: 1 }))}
            {spec.task === 'clustering' && spec.model !== 'dbscan' && <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={spec.sweepK} onChange={(e) => setSpec({ sweepK: e.target.checked })} /> Elbow and silhouette sweep over k (2 to 10)</label>}
          </div>
        ))}

        {cols.length > 0 && sup && spec.mode === 'single' && section('5. Save', (
          <div className="flex flex-col gap-1">
            <label className="flex items-center gap-2 text-xs" title="Adds save(...) of the fitted model and its preprocessing to the end of the script">
              <input
                type="checkbox"
                checked={spec.save.enabled}
                onChange={async (e) => {
                  const enabled = e.target.checked;
                  const path = spec.save.path || (enabled ? await defaultModelPath() : '');
                  setSpec({ save: { enabled, path } });
                }}
                aria-label="Save model"
              />
              Save the model at the end of the script
            </label>
            {spec.save.enabled && (
              <div className="flex gap-1">
                <input value={spec.save.path} onChange={(e) => setSpec({ save: { ...spec.save, path: e.target.value } })} placeholder="C:/path/model.json" className={`${inputClass} flex-1 font-mono text-xs`} style={{ borderColor: border }} aria-label="model path" />
                <button className="px-2 rounded-md border" style={{ borderColor: border }} title="Save as…" onClick={async () => {
                  const p = await pickSavePath(spec.save.path, 'json');
                  if (p) setSpec({ save: { ...spec.save, path: p } });
                }}><FolderOpen size={13} /></button>
              </div>
            )}
            <span className="text-[11px]" style={{ color: 'var(--qu-muted)' }}>save() writes the model as readable JSON (every model kind here round-trips); the engine's HDF5 save_model is not enabled in this build.</span>
          </div>
        ))}

        {running ? (
          <button className="qu-side-action w-full" style={{ background: 'var(--qu-danger)' }} onClick={() => void stop()}><Square size={13} /> Stop</button>
        ) : (
          <button className="qu-side-action w-full" disabled={edited === null && !!problem} onClick={run} title={edited === null && problem ? problem : 'Generate the script and run it'} data-testid="ml-run">
            <Play size={13} /> Run
          </button>
        )}
        {edited === null && problem && cols.length > 0 && (
          <div className="qu-banner" data-kind="error" role="alert" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }} data-testid="ml-problem">
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words whitespace-pre-wrap">{problem}</span>
          </div>
        )}
      </div>

      {/* ---------------- centre: results */}
      <div className="flex-1 min-w-0 min-h-0 overflow-y-auto p-3 flex flex-col gap-3" data-testid="ml-results">
        <div className="flex items-center justify-between text-sm">
          <span className="font-medium" data-testid="ml-status">{headline()}</span>
          {rs && state !== 'idle' && <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>{TASKS.find((t) => t.id === rs.task)?.label}{rs.mode === 'compare' ? ', comparison' : modelById(rs.model) ? `, ${modelById(rs.model)!.label}` : ''}</span>}
        </div>
        {error && (
          <div className="qu-banner" data-kind="error" role="alert" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }} data-testid="ml-error">
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <div className="flex flex-col gap-1 min-w-0">
              <span className="break-words whitespace-pre-wrap">{error}</span>
              {errLine && <code className="text-[11px] break-all">script line {errLine.line}: {errLine.text.trim()}</code>}
            </div>
          </div>
        )}
        {res.notes.length > 0 && (
          <div className="text-xs flex flex-col gap-0.5" style={{ color: 'var(--qu-warning)' }} data-testid="ml-notes">
            {res.notes.map((n, i) => <span key={i}>Note: {n}</span>)}
          </div>
        )}
        {state === 'idle' && !hasResults && (
          <div className="text-sm" style={{ color: 'var(--qu-muted)' }}>
            Pick a data source, choose what to predict and press Run: the panel writes a Qu script (on the right), runs it, and draws what it prints. The default example needs two clicks.
          </div>
        )}

        <div className="grid grid-cols-1 2xl:grid-cols-2 gap-3 items-start">
          {(res.metrics.length > 0 || res.cvMetrics.length > 0) && (
            <Card title="Metrics" note={rs.eval.method === 'cv' && sup ? `${rs.eval.folds}-fold cross-validation` : sup && rs.mode === 'single' ? 'held-out test rows' : undefined} testId="ml-metrics">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <tbody>
                  {res.cvMetrics.map((m) => (
                    <tr key={`cv${m.name}`} style={{ borderTop: `1px solid ${border}` }} title={METRIC_HELP[m.name]}>
                      <td className="py-1">{METRIC_LABEL[m.name] ?? m.name} (folds)</td>
                      <td className="font-mono text-right" data-metric={`cv_${m.name}`}>{fmtNum(m.mean)} ± {fmtNum(m.std)}</td>
                    </tr>
                  ))}
                  {res.metrics.map((m) => (
                    <tr key={m.name} style={{ borderTop: `1px solid ${border}` }} title={METRIC_HELP[m.name]}>
                      <td className="py-1">{METRIC_LABEL[m.name] ?? m.name}{res.cvMetrics.length > 0 ? ' (all out-of-fold rows)' : ''}</td>
                      <td className="font-mono text-right" data-metric={m.name}>{fmtNum(m.value)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          )}

          {(S.class_counts || S.target_hist) && state !== 'idle' && (
            <Card title={S.class_counts ? 'Class balance' : 'Target distribution'} note={S.target_range ? `${fmtNum(S.target_range[0])} .. ${fmtNum(S.target_range[1])}` : undefined}>
              <Columns counts={S.class_counts ?? S.target_hist} labels={S.class_counts ? rClasses : undefined} />
            </Card>
          )}

          {res.cm.length > 0 && (
            <Card title="Confusion matrix" note="rows: actual, columns: predicted"><ConfusionGrid cm={res.cm} labels={rClasses.length ? rClasses : res.cm.map((_, i) => String(i))} testId="ml-confusion" /></Card>
          )}
          {roc && (
            <Card title="ROC curve" note={metricOf('roc_auc') !== undefined ? `AUC ${fmtNum(metricOf('roc_auc'))}` : undefined}>
              <LineChart series={[{ name: 'ROC', x: roc[0], y: roc[1] }]} xRange={[0, 1]} yRange={[0, 1]} diagonal xLabel="false positive rate" yLabel="true positive rate" testId="ml-roc" />
            </Card>
          )}
          {pr && (
            <Card title="Precision-recall curve" note={metricOf('avg_precision') !== undefined ? `AP ${fmtNum(metricOf('avg_precision'))}` : undefined}>
              <LineChart series={[{ name: 'PR', x: pr[0], y: pr[1] }]} xRange={[0, 1]} yRange={[0, 1.02]} xLabel="recall" yLabel="precision" testId="ml-pr" />
            </Card>
          )}
          {imp && (
            <Card title="Feature importance" note="drop in score when the column is shuffled"><BarList rows={imp} testId="ml-importance" /></Card>
          )}
          {yPair && (
            <Card title="Predicted vs actual" note="the dashed line is a perfect fit"><ScatterChart x={yPair[0]} y={yPair[1]} diagonal xLabel="actual" yLabel="predicted" testId="ml-pred-actual" /></Card>
          )}
          {resid && (
            <Card title="Residuals" note="actual minus predicted: no pattern is good"><ScatterChart x={resid[0]} y={resid[1]} xLabel="predicted" yLabel="residual" height={220} testId="ml-residuals" /></Card>
          )}
          {S.lc_n && S.lc_train && S.lc_test && (
            <Card title="Learning curve" note="score against training rows">
              <LineChart series={[{ name: 'training', x: S.lc_n, y: S.lc_train, dots: true }, { name: 'test', x: S.lc_n, y: S.lc_test, dots: true }]} xLabel="training rows" yLabel={rs.task === 'regression' ? 'R-squared' : 'accuracy'} testId="ml-learning-curve" />
            </Card>
          )}

          {ev.length > 0 && (
            <>
              <Card title="Training loss" note={res.stop ? `early stop at epoch ${res.stop.epoch}, best weights from epoch ${res.stop.best}` : `${ev[ev.length - 1].epoch} epochs`}>
                <LineChart series={[{ name: 'training loss', x: epochX, y: ev.map((e) => e.loss) }, ...(ev.some((e) => Number.isFinite(e.valLoss)) ? [{ name: 'validation loss', x: epochX, y: ev.map((e) => e.valLoss) }] : [])]} xLabel="epoch" yLabel={rs.task === 'regression' ? 'mse (standardised target)' : 'cross-entropy'} testId="ml-loss-curve" />
              </Card>
              <Card title={rs.task === 'regression' ? 'R-squared per epoch' : 'Accuracy per epoch'}>
                <LineChart series={[{ name: 'training', x: epochX, y: ev.map((e) => e.metric) }, ...(ev.some((e) => Number.isFinite(e.valMetric)) ? [{ name: 'validation', x: epochX, y: ev.map((e) => e.valMetric) }] : [])]} xLabel="epoch" testId="ml-metric-curve" />
              </Card>
            </>
          )}

          {res.grid.length > 0 && (
            <Card title="Tuning (grid_search)" note={`${res.gridMetric || 'score'}, mean ± std over 5 folds of the training rows`} testId="ml-grid">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <thead>
                  <tr style={{ color: 'var(--qu-muted)' }} className="text-left"><th className="font-normal">#</th>{tunedNames.map((n) => <th key={n} className="font-normal">{n}</th>)}<th className="font-normal">score</th></tr>
                </thead>
                <tbody>
                  {res.grid.map((g, i) => {
                    const best = res.best && Math.abs(g.mean - res.best.score) < 1e-9 && g.params.every((p, j) => p === res.best!.params[j]);
                    return (
                      <tr key={i} style={{ borderTop: `1px solid ${border}`, fontWeight: best ? 600 : 400, color: best ? 'var(--qu-ok)' : undefined }}>
                        <td className="py-0.5">{i + 1}</td>{g.params.map((p, j) => <td key={j} className="font-mono">{fmtNum(p)}</td>)}<td className="font-mono">{fmtNum(g.mean)} ± {fmtNum(g.std)}{best ? '  best' : ''}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </Card>
          )}

          {ranked.length > 0 && (
            <Card title="Which model should I use?" note={`${rs.compare.folds}-fold cross-validation, the same folds for every model, ranked by ${COMPARE_METRICS[rs.task === 'regression' ? 'regression' : 'classification'].find((m) => m.id === rs.compare.metric)?.label ?? rs.compare.metric}`} testId="ml-compare">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <thead><tr style={{ color: 'var(--qu-muted)' }} className="text-left"><th className="font-normal">rank</th><th className="font-normal">model</th><th className="font-normal">mean ± std</th><th className="font-normal">folds</th></tr></thead>
                <tbody>
                  {ranked.map((r, i) => (
                    <tr key={r.id} style={{ borderTop: `1px solid ${border}`, fontWeight: i === 0 ? 600 : 400 }} data-model={r.id}>
                      <td className="py-1">{i + 1}</td>
                      <td>{modelById(r.id)?.label ?? r.id}</td>
                      <td className="font-mono">{fmtNum(r.mean)} ± {fmtNum(r.std)}</td>
                      <td className="font-mono" style={{ color: 'var(--qu-muted)' }}>{r.folds.map((f) => fmtNum(f, 3)).join('  ')}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <BarList rows={ranked.map((r) => ({ label: modelById(r.id)?.label ?? r.id, value: r.mean, extra: r.std }))} min={0} />
              {res.compareFail.map((f) => <span key={f.id} className="text-xs" style={{ color: 'var(--qu-danger)' }}>{modelById(f.id)?.label ?? f.id} did not run: {f.message}</span>)}
            </Card>
          )}

          {rs.task === 'clustering' && scat && (
            <Card title="Clusters" note={res.clusters ? `${res.clusters.k} clusters${res.clusters.noise > 0 ? `, ${res.clusters.noise} noise rows` : ''}${rNames.length > 2 ? ', first two principal components' : ''}` : undefined}>
              <ScatterChart x={scat.x} y={scat.y} c={scat.c} xLabel={rNames.length > 2 ? 'PC 1' : rNames[0]} yLabel={rNames.length > 2 ? 'PC 2' : rNames[1]} testId="ml-clusters" />
            </Card>
          )}
          {res.centers.length > 0 && (
            <Card title="Cluster centres" note="original units">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <thead><tr style={{ color: 'var(--qu-muted)' }} className="text-left"><th className="font-normal">cluster</th><th className="font-normal">rows</th>{rNames.map((n) => <th key={n} className="font-normal">{n}</th>)}</tr></thead>
                <tbody>
                  {res.centers.map((c) => (
                    <tr key={c.id} style={{ borderTop: `1px solid ${border}` }} data-cluster={c.id}>
                      <td className="py-1"><span style={{ color: PALETTE[c.id % PALETTE.length] }}>●</span> {c.id}</td>
                      <td className="font-mono">{res.clusterSizes.find((s) => s.id === c.id)?.n ?? ''}</td>
                      {c.values.map((v, i) => <td key={i} className="font-mono">{fmtNum(v)}</td>)}
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          )}
          {S.sweep_k && S.sweep_inertia && (
            <Card title="Elbow: inertia against k" note="a bend marks a good k"><LineChart series={[{ name: 'inertia', x: S.sweep_k, y: S.sweep_inertia, dots: true }]} xLabel="k" yLabel="inertia" testId="ml-elbow" /></Card>
          )}
          {S.sweep_k && S.sweep_silhouette && (
            <Card title="Silhouette against k" note="higher is better"><LineChart series={[{ name: 'silhouette', x: S.sweep_k, y: S.sweep_silhouette, dots: true }]} xLabel="k" yLabel="silhouette" testId="ml-silhouette" /></Card>
          )}

          {rs.task === 'reduction' && S.var_ratio && (
            <Card title="Variance explained" note={`${fmtNum(metricOf('kept_variance'), 3)} kept with ${fmtNum(metricOf('kept_components'), 2)} components`} testId="ml-variance">
              <LineChart
                series={[
                  { name: 'per component', x: S.var_ratio.map((_, i) => i + 1), y: S.var_ratio, dots: true },
                  { name: 'cumulative', x: S.cum_var.map((_, i) => i + 1), y: S.cum_var, dots: true, dashed: true },
                ]}
                yRange={[0, 1.02]}
                xLabel="component"
                yLabel="share of variance"
              />
              <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>
                {(() => {
                  const k = S.cum_var.findIndex((v) => v >= 0.95);
                  return k >= 0 ? `${k + 1} component${k ? 's' : ''} reach 95% of the variance.` : 'No number of components reaches 95%.';
                })()}
              </span>
            </Card>
          )}
          {rs.task === 'reduction' && Object.keys(S).some((k) => k.startsWith('loading_')) && (
            <Card title="Loadings" note="how each feature enters a component">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <thead><tr style={{ color: 'var(--qu-muted)' }} className="text-left"><th className="font-normal">PC</th>{rNames.map((n) => <th key={n} className="font-normal">{n}</th>)}</tr></thead>
                <tbody>
                  {Object.keys(S).filter((k) => k.startsWith('loading_')).sort().map((k) => (
                    <tr key={k} style={{ borderTop: `1px solid ${border}` }}><td className="py-1">{Number(k.slice(8)) + 1}</td>{S[k].map((v, i) => <td key={i} className="font-mono">{fmtNum(v, 3)}</td>)}</tr>
                  ))}
                </tbody>
              </table>
            </Card>
          )}
          {rs.task === 'reduction' && scat && (
            <Card title={rs.model === 'tsne' ? 't-SNE map' : 'First two principal components'} note={rs.target ? `coloured by ${rs.target}` : undefined}>
              <ScatterChart x={scat.x} y={scat.y} c={rs.target ? scat.c : undefined} labels={rClasses} xLabel={rs.model === 'tsne' ? 'dimension 1' : 'PC 1'} yLabel={rs.model === 'tsne' ? 'dimension 2' : 'PC 2'} testId="ml-embedding" />
            </Card>
          )}

          {rs.task === 'anomaly' && scoreHist && (
            <Card title="Anomaly scores" note={`${fmtNum(metricOf('flagged'), 3)} rows flagged above ${fmtNum(metricOf('threshold'), 3)}`}>
              <Columns counts={scoreHist.counts} xNote={`score ${fmtNum(scoreHist.lo, 3)} .. ${fmtNum(scoreHist.hi, 3)} (higher = more unusual)`} testId="ml-score-hist" />
            </Card>
          )}
          {rs.task === 'anomaly' && scat && (
            <Card title="Flagged rows" note="grey: normal, coloured: flagged">
              <ScatterChart x={scat.x} y={scat.y} c={scat.c.map((v) => (v > 0 ? 1 : -1))} xLabel={rNames.length > 2 ? 'PC 1' : rNames[0]} yLabel={rNames.length > 2 ? 'PC 2' : rNames[1]} testId="ml-anomalies" />
            </Card>
          )}
          {res.top.length > 0 && (
            <Card title="Most unusual rows" note="0-based row numbers of the table">
              <table className="text-xs w-full" style={{ borderCollapse: 'collapse' }}>
                <tbody>{res.top.map((t) => <tr key={t.row} style={{ borderTop: `1px solid ${border}` }}><td className="py-0.5">row {t.row}</td><td className="font-mono text-right">{fmtNum(t.score)}</td></tr>)}</tbody>
              </table>
            </Card>
          )}
        </div>

        {res.text.length > 0 && (
          <div className="rounded border max-h-32 overflow-auto px-3 py-2 font-mono text-[11px] whitespace-pre" style={{ borderColor: border, color: 'var(--qu-muted)' }} aria-label="Run output">
            {res.text.slice(-40).join('\n')}
          </div>
        )}
      </div>

      {/* ---------------- right: the script */}
      <div className="w-[24rem] flex-shrink-0 border-l overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">Generated Qu</span>
          <div className="flex gap-1">
            <button className="p-1 rounded" style={{ color: 'var(--qu-muted)' }} title="Copy the script" disabled={!shown} onClick={() => copy('script', shown)}><Copy size={13} /></button>
            <button className="p-1 rounded" style={{ color: showEdit ? 'var(--qu-accent)' : 'var(--qu-muted)' }} title="Edit the code by hand (detaches the script from the panel's settings)" onClick={() => {
              if (!showEdit && edited === null) useMlLab.setState({ edited: script });
              setShowEdit(!showEdit);
            }}><Pencil size={13} /></button>
          </div>
        </div>
        {edited !== null && (
          <div className="qu-banner" data-kind="warning" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12, padding: '6px 9px' }} data-testid="ml-detached">
            <AlertCircle size={13} className="flex-shrink-0 mt-0.5" />
            <span>You are editing the code: the panel's settings no longer change it. Run uses your text.{' '}
              <button className="underline inline-flex items-center gap-1" onClick={() => { useMlLab.setState({ edited: null }); setShowEdit(false); }}><RotateCcw size={11} /> Back to generated</button>
            </span>
          </div>
        )}
        {showEdit && edited !== null ? (
          <textarea value={edited} onChange={(e) => useMlLab.setState({ edited: e.target.value })} spellCheck={false} className="rounded-lg p-2 text-[11px] leading-snug font-mono w-full h-[28rem] resize-y" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }} aria-label="Script editor" />
        ) : (
          <div className="rounded-lg p-2 text-[11px] leading-snug font-mono overflow-x-auto whitespace-pre max-h-[28rem] overflow-y-auto" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }} data-testid="ml-script">
            {shown || '# Fix the setup on the left to see the script.'}
            {copied === 'script' && <span style={{ color: 'var(--qu-ok)' }}>{'\n'}copied</span>}
          </div>
        )}
        {onOpenInEditor && (
          <button className="qu-side-action w-full" disabled={!shown} onClick={() => onOpenInEditor(shown, `ml_${spec.task}.qu`)} title="Open this exact script in a new editor tab">
            <FileCode2 size={13} /> Open in editor
          </button>
        )}

        {sup && spec.mode === 'single' && (
          <div className="flex flex-col gap-2 border-t pt-3" style={{ borderColor: border }} data-testid="ml-predict">
            <span className="text-sm font-medium">Predict on new data</span>
            {!spec.save.enabled || !spec.save.path ? (
              <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>Tick "Save the model" on the left and run first; then a script that loads the model and writes predictions for a new CSV is generated here.</span>
            ) : (
              <>
                <div className="flex gap-1">
                  <input value={predict.newCsv} onChange={(e) => setPredict((p) => ({ ...p, newCsv: e.target.value, code: '' }))} placeholder="C:/path/new_rows.csv" className={`${inputClass} flex-1 font-mono text-xs`} style={{ borderColor: border }} aria-label="new data path" />
                  <button className="px-2 rounded-md border" style={{ borderColor: border }} title="Browse" onClick={async () => {
                    const p = await pickOpenPath();
                    if (p) setPredict((x) => ({ ...x, newCsv: p, code: '' }));
                  }}><FolderOpen size={13} /></button>
                </div>
                <input value={predict.outCsv} onChange={(e) => setPredict((p) => ({ ...p, outCsv: e.target.value, code: '' }))} placeholder={`${predict.newCsv ? baseName(predict.newCsv).replace(/\.[^.]+$/, '') : 'new_rows'}_predictions.csv (output)`} className={`${inputClass} font-mono text-xs`} style={{ borderColor: border }} aria-label="predictions path" />
                <button className="qu-side-action w-full" disabled={!predictReady} onClick={buildPredict}>Generate predict script</button>
                {predict.code && (
                  <>
                    <div className="rounded-lg p-2 text-[11px] leading-snug font-mono overflow-x-auto whitespace-pre max-h-60 overflow-y-auto" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)' }} data-testid="ml-predict-script">{predict.code}</div>
                    <div className="flex gap-1">
                      <button className="qu-side-action flex-1" disabled={predict.running} onClick={() => void runPredict()}><Play size={13} /> Run</button>
                      {onOpenInEditor && <button className="qu-side-action flex-1" onClick={() => onOpenInEditor(predict.code, 'ml_predict.qu')}><FileCode2 size={13} /> Editor</button>}
                      <button className="qu-side-action" onClick={() => copy('predict', predict.code)}>{copied === 'predict' ? 'copied' : 'Copy'}</button>
                    </div>
                  </>
                )}
                {predict.output && <div className="text-xs font-mono whitespace-pre-wrap" style={{ color: 'var(--qu-ok)' }}>{predict.output.trim()}</div>}
                {predict.error && <div className="text-xs whitespace-pre-wrap" style={{ color: 'var(--qu-danger)' }}>{predict.error}</div>}
              </>
            )}
          </div>
        )}
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          The panel only writes this script and runs it in its own Qu interpreter; every number and picture comes from lines it prints. Copy it, edit it, rerun it anywhere.
        </p>
      </div>
    </div>
  );
};

void (undefined as unknown as ColType);
export default MlLabPanel;
