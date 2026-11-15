import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PlotViewer } from '@qu/ui-components';
import type { PlotData } from '@qu/ui-components';
import { AlertCircle, Copy, FileCode2, Play } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { CurveFitData, autoCols, type DataUi } from './CurveFitDataInput';
import { inDesktopApp, stageTextFile } from './DataSourcePicker';
import { SolveEquations } from './SolveEquations';
import {
  CUSTOM_ID, MODELS, buildCurveFit, defaultSpec, emptyRef, errorContext, modelDef, modelLabel, modelsFor, needsDataFile, plainError, readBlock, readRanking,
  resolveModel, resolveOde, scriptErrors, type BlockResult, type FitSpec, type ModelRef, type ParamSetting, type Plan, type Series,
} from './curveFitDesign';
import type { Shape } from './curveFitData';

export interface CurveFitPanelProps extends LiveSerialPlotPanelProps {
  /** Numeric vectors of the Code tab's last run, usable as columns. */
  signalVars?: Array<{ name: string; length: number }>;
  /** Every sample of one of `signalVars`. */
  loadSignal?: (name: string) => Promise<(number | null)[]>;
  /** Opens `code` as a new editor buffer named `name` in the Code tab. */
  onOpenInEditor?: (code: string, name: string) => void;
}

interface ExecuteShape {
  success: boolean;
  error: string | null;
  output?: string;
  data?: Array<{ name: string; data: (number | null)[] }>;
}

interface Ran {
  plan: Plan;
  series: Series;
  output: string;
  script: string;
}

type Mode = 'fit' | 'solve';
type Tab = 'fit' | 'residuals' | 'numbers' | 'ranking' | 'curves' | 'trajectory' | 'surface';

// The tab unmounts whenever another top-level tab is shown; without this a
// trip to the Code tab and back threw the work away.
let remembered: { spec: FitSpec; ui: DataUi; mode: Mode; tab: Tab; ran: Ran | null } = {
  spec: defaultSpec(), ui: { pasteText: '', varNames: [], note: '' }, mode: 'fit', tab: 'fit', ran: null,
};

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border';
const COLORS = ['#4c78a8', '#f58518', '#54a24b', '#e45756', '#72b7b2', '#b279a2', '#9d755d', '#bab0ac'];

const fmt = (v: number | null | undefined, digits = 4): string => {
  if (v == null || !Number.isFinite(v)) return v === Infinity ? 'inf' : 'n/a';
  if (v === 0) return '0';
  const a = Math.abs(v);
  return a < 1e-3 || a >= 1e6 ? v.toExponential(digits - 1) : String(Number(v.toPrecision(digits)));
};

const compareKey = (r: ModelRef) => (r.id === 'poly' ? `poly:${r.degree}` : r.id);
const COMPARE_CHOICES: Array<{ key: string; label: string; ref: ModelRef }> = [
  ...[1, 2, 3].map((d) => ({ key: `poly:${d}`, label: d === 1 ? 'Straight line' : d === 2 ? 'Quadratic' : 'Cubic', ref: emptyRef('poly', d) })),
  ...MODELS.filter((m) => m.shapes.includes('yx') && m.id !== 'poly').map((m) => ({ key: m.id, label: m.label, ref: emptyRef(m.id) })),
  { key: CUSTOM_ID, label: 'Custom expression', ref: emptyRef(CUSTOM_ID) },
];

const line = (x: number[], y: number[], name: string, color?: string, dash?: string, width = 2): PlotData => ({ x, y, name, type: 'line', mode: 'lines', line: { width, color, dash } });
const dots = (x: number[], y: number[], name: string, color?: string, size = 7): PlotData => ({ x, y, name, type: 'scatter', mode: 'markers', marker: { size, color } });

/** Residual map: points at (x, y) coloured by their residual (blue below the fit, red above). */
const ResidualMap: React.FC<{ x: number[]; y: number[]; r: number[] }> = ({ x, y, r }) => {
  const W = 420;
  const H = 340;
  const m = { l: 44, r: 14, t: 12, b: 34 };
  const fin = (v: number[]) => v.filter(Number.isFinite);
  const xl = [Math.min(...fin(x)), Math.max(...fin(x))];
  const yl = [Math.min(...fin(y)), Math.max(...fin(y))];
  const mx = Math.max(1e-300, ...fin(r).map(Math.abs));
  const px = (v: number) => m.l + ((v - xl[0]) / (xl[1] - xl[0] || 1)) * (W - m.l - m.r);
  const py = (v: number) => H - m.b - ((v - yl[0]) / (yl[1] - yl[0] || 1)) * (H - m.t - m.b);
  const col = (v: number) => {
    const t = Math.max(-1, Math.min(1, v / mx));
    const a = Math.round(255 * (1 - Math.abs(t)));
    return t >= 0 ? `rgb(255,${a},${a})` : `rgb(${a},${a},255)`;
  };
  return (
    <svg viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', maxWidth: 520, color: 'var(--qu-text)' }} role="img" aria-label="Residual map">
      <rect x={m.l} y={m.t} width={W - m.l - m.r} height={H - m.t - m.b} fill="none" stroke="var(--qu-border)" />
      {x.map((v, i) => (Number.isFinite(v) && Number.isFinite(r[i]) ? <circle key={i} cx={px(v)} cy={py(y[i])} r={5} fill={col(r[i])} stroke="var(--qu-border)" strokeWidth={0.5} /> : null))}
      <text x={(m.l + W - m.r) / 2} y={H - 6} textAnchor="middle" fontSize={11} fill="var(--qu-muted)">x ({fmt(xl[0], 3)} to {fmt(xl[1], 3)})</text>
      <text x={12} y={(m.t + H - m.b) / 2} textAnchor="middle" fontSize={11} fill="var(--qu-muted)" transform={`rotate(-90 12 ${(m.t + H - m.b) / 2})`}>y ({fmt(yl[0], 3)} to {fmt(yl[1], 3)})</text>
      <text x={W - m.r} y={m.t + 10} textAnchor="end" fontSize={10} fill="var(--qu-muted)">red: data above fit, blue: below, |max| = {fmt(mx, 3)}</text>
    </svg>
  );
};

export const CurveFitPanel: React.FC<CurveFitPanelProps> = ({ theme, invoke, signalVars = [], loadSignal, onOpenInEditor }) => {
  const [mode, setMode] = useState<Mode>(remembered.mode);
  const [spec, setSpec] = useState<FitSpec>(remembered.spec);
  const [ui, setUi] = useState<DataUi>(remembered.ui);
  const [tab, setTab] = useState<Tab>(remembered.tab);
  const [ran, setRan] = useState<Ran | null>(remembered.ran);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [codeText, setCodeText] = useState('');
  const [surface3d, setSurface3d] = useState(false);
  const [pick, setPick] = useState(0);
  const runId = useRef(0);
  useEffect(() => {
    remembered = { spec, ui, mode, tab, ran };
  }, [spec, ui, mode, tab, ran]);

  const set = (patch: Partial<FitSpec>) => setSpec((s) => ({ ...s, ...patch }));
  const setModel = (patch: Partial<ModelRef>) => setSpec((s) => ({ ...s, model: { ...s.model, ...patch } }));
  const setParam = (name: string, patch: Partial<ParamSetting>) =>
    setSpec((s) =>
      s.shape === 'multi' && s.coupling === 'ode'
        ? { ...s, odePs: { ...s.odePs, [name]: { ...s.odePs[name], ...patch } } }
        : { ...s, model: { ...s.model, ps: { ...s.model.ps, [name]: { ...s.model.ps[name], ...patch } } } },
    );

  const planR = useMemo(() => buildCurveFit(spec, 'show'), [spec]);
  const shown = planR.ok ? planR.plan.script : '';
  const problem = planR.ok ? null : planR.error;
  const stale = !!ran && ran.script !== shown && !editing;

  // the model's parameters, for the settings table
  const resolved = useMemo(() => {
    if (spec.shape === 'multi' && spec.coupling === 'ode') return resolveOde(spec);
    return resolveModel(spec.model, spec.shape, spec.shape === 'multi' ? 't' : 'x');
  }, [spec]);

  const changeShape = (shape: Shape) => {
    const model = shape === 'surface' ? emptyRef('quad', 2) : emptyRef(spec.shape === 'surface' ? 'exp' : spec.model.id === 'quad' || spec.model.id === 'plane' || spec.model.id === 'gauss2d' ? 'exp' : spec.model.id, spec.model.degree, spec.model.expr);
    set({ shape, model, mode: 'single' });
    setRan(null);
    setTab(shape === 'surface' ? 'surface' : shape === 'multi' ? 'fit' : 'fit');
  };

  const runFit = useCallback(async () => {
    const p0 = buildCurveFit(spec, 'show');
    if (!p0.ok) {
      setError(p0.error);
      return;
    }
    const mine = ++runId.current;
    setRunning(true);
    setError('');
    try {
      let file = '';
      const need = needsDataFile(spec);
      if (need && !editing) {
        if (!inDesktopApp()) throw new Error('Large data tables go through a temporary file: that needs the desktop app.');
        // one fixed name, overwritten each run, so staged tables never pile up
        file = await stageTextFile(need.csv, { base: 'cache', file: 'curve-fit-data.csv' });
      }
      const p = buildCurveFit(spec, 'run', file);
      if (!p.ok) throw new Error(p.error);
      const code = editing ? codeText : p.plan.script;
      const r = await invoke<ExecuteShape>('execute_code', { request: { code, file_path: null } });
      if (mine !== runId.current) return;
      if (!r.success) {
        const ctx = errorContext(r.error ?? '', code);
        setError(`${plainError(r.error ?? 'The fit did not run')}${ctx ? `\nScript line ${ctx.line}: ${ctx.text}` : ''}`);
        return;
      }
      const series: Series = {};
      for (const v of r.data ?? []) series[v.name] = v.data;
      setRan({ plan: p.plan, series, output: r.output ?? '', script: p0.plan.script });
      setTab((t) => (spec.mode === 'compare' && spec.shape === 'yx' ? (t === 'ranking' || t === 'curves' ? t : 'ranking') : t === 'ranking' || t === 'curves' ? 'fit' : t));
    } catch (err) {
      if (mine === runId.current) setError(String(err));
    } finally {
      if (mine === runId.current) setRunning(false);
    }
  }, [spec, editing, codeText, invoke]);

  const blocks: BlockResult[] = useMemo(() => {
    if (!ran) return [];
    const errors = scriptErrors(ran.output);
    const joint = ran.plan.layout === 'shared';
    return ran.plan.blocks.map((b) => readBlock(b, ran.series, errors, joint ? ran.plan.jointLabels : undefined, joint ? [ran.plan.jointLo!, ran.plan.jointHi!] : undefined));
  }, [ran]);
  const warnings = useMemo(() => (ran ? ran.output.split(/\r?\n/).filter((l) => l.startsWith('CFWARN')).map((l) => l.replace(/^CFWARN\s*/, '')) : []), [ran]);

  // ------------------------------------------------------------------ plots
  const col = (name: string) => (ran?.series[name] ?? []).map((v) => (v == null ? NaN : v));
  const pv = (data: PlotData[], extra: Partial<React.ComponentProps<typeof PlotViewer>> = {}) => (
    <PlotViewer data={data} theme={theme} showLegend height={380} logx={spec.logx} logy={spec.logy} {...extra} />
  );
  const layout = ran?.plan.layout;
  const multiBlocks = layout === 'separate' || layout === 'shared' || layout === 'ode';

  const fitPlot = () => {
    if (!ran || blocks.length === 0) return null;
    if (layout === 'surface') return surfacePlot();
    const data: PlotData[] = [];
    blocks.forEach((b, i) => {
      if (!b.ok) return;
      const c = COLORS[i % COLORS.length];
      const label = b.info.signal ?? 'data';
      const xs = col(b.info.xv);
      const ys = col(b.info.yv);
      const lowW = b.rw.length ? ys.map((_, k) => b.rw[k] < 0.5) : [];
      if (b.rw.length) {
        data.push(dots(xs.filter((_, k) => !lowW[k]), ys.filter((_, k) => !lowW[k]), `${label} data`, c));
        data.push({ ...dots(xs.filter((_, k) => lowW[k]), ys.filter((_, k) => lowW[k]), `${label} outliers`, '#d33', 9), marker: { size: 9, color: '#d33', symbol: 'x' } });
      } else data.push(dots(xs, ys, multiBlocks ? `${label} data` : 'data', c));
      data.push(line(b.gx, b.gy, multiBlocks ? `${label} fit` : 'fit', c));
      if (b.gb.length) {
        data.push(line(b.gx, b.gy.map((v, k) => v + b.gb[k]), '95% band', c, 'dot', 1));
        data.push(line(b.gx, b.gy.map((v, k) => v - b.gb[k]), '95% band (lower)', c, 'dot', 1));
      }
    });
    return pv(data, { xlabel: spec.shape === 'multi' ? 'time' : spec.cols.x || 'x', ylabel: spec.shape === 'multi' ? 'signal' : spec.cols.y || 'y', title: 'Data and fitted curve' });
  };

  const surfacePlot = () => {
    const b = blocks[0];
    if (!b || !b.ok || b.gz.length < 900) return null;
    const ux = Array.from({ length: 30 }, (_, i) => b.gx[i * 30]);
    const uy = Array.from({ length: 30 }, (_, j) => b.gy[j]);
    const Z = uy.map((_, j) => ux.map((__, i) => b.gz[i * 30 + j]));
    const xs = col('x');
    const ys = col('y');
    const zs = col('z');
    const data: PlotData[] = surface3d
      ? [
          { x: ux, y: uy, z: Z as unknown as number[][], name: 'fit', type: 'surface', opacity: 0.8 },
          { x: xs, y: ys, z: zs, name: 'data', type: 'scatter3d', mode: 'markers', marker: { size: 3, color: '#e45756' } },
        ]
      : [
          { x: ux, y: uy, z: Z as unknown as number[][], name: 'fit', type: 'contour' },
          dots(xs, ys, 'data points', '#e45756', 6),
        ];
    return (
      <div>
        <label className="flex items-center gap-2 text-xs mb-2">
          <input type="checkbox" checked={surface3d} onChange={(e) => setSurface3d(e.target.checked)} /> 3-D surface (otherwise contour with the data points)
        </label>
        <PlotViewer data={data} theme={theme} showLegend height={460} xlabel={spec.cols.x || 'x'} ylabel={spec.cols.y || 'y'} title={`Fitted surface for ${spec.cols.z || 'z'}`} />
      </div>
    );
  };

  const residualPlots = () => {
    const bs = blocks.filter((b) => b.ok);
    if (bs.length === 0) return null;
    const b = bs[Math.min(pick, bs.length - 1)];
    const x = col(b.info.xv);
    const zero = (xs: number[]) => line([Math.min(...xs.filter(Number.isFinite)), Math.max(...xs.filter(Number.isFinite))], [0, 0], 'zero', '#888', 'dash', 1);
    return (
      <>
        {bs.length > 1 && (
          <select className={`${inputClass} mb-2`} style={{ borderColor: border }} value={pick} onChange={(e) => setPick(Number(e.target.value))}>
            {bs.map((c, i) => <option key={c.info.pfx} value={i}>{c.info.signal ?? c.info.title}</option>)}
          </select>
        )}
        {layout !== 'surface' && pv([dots(x, b.res, 'residual'), zero(x)], { xlabel: spec.shape === 'multi' ? 'time' : spec.cols.x || 'x', ylabel: 'residual (data - fit)', title: 'Residuals versus x', height: 320, logy: false })}
        {pv([dots(b.pred, b.res, 'residual'), zero(b.pred)], { xlabel: 'fitted value', ylabel: 'residual (data - fit)', title: 'Residuals versus fitted value', height: 320, logx: false, logy: false })}
        {layout === 'surface' && (
          <div className="mt-2">
            <div className="text-sm font-medium mb-1">Residual map</div>
            <ResidualMap x={col('x')} y={col('y')} r={b.res} />
          </div>
        )}
      </>
    );
  };

  const trajectory = () => {
    const bs = blocks.filter((b) => b.ok);
    if (bs.length < 2 || bs.length > 3) return <div className="p-4 text-sm" style={{ color: 'var(--qu-muted)' }}>Tick two or three signals to see their trajectory.</div>;
    const names = bs.map((b) => b.info.signal ?? b.info.yv);
    const d = bs.map((b) => col(b.info.yv));
    const f = bs.map((b) => b.gy);
    if (bs.length === 2) {
      return pv([dots(d[0], d[1], 'data', COLORS[0]), line(f[0], f[1], 'fit', COLORS[1])], { xlabel: names[0], ylabel: names[1], title: `Trajectory: ${names[1]} against ${names[0]}`, logx: false, logy: false, height: 460 });
    }
    return pv(
      [
        { x: d[0], y: d[1], z: d[2], name: 'data', type: 'scatter3d', mode: 'markers', marker: { size: 3, color: COLORS[0] } },
        { x: f[0], y: f[1], z: f[2], name: 'fit', type: 'scatter3d', mode: 'lines', line: { width: 4, color: COLORS[1] } },
      ],
      { xlabel: names[0], ylabel: names[1], zlabel: names[2], title: 'Trajectory (x, y, z)', logx: false, logy: false, height: 520 },
    );
  };

  const ranking = () => {
    if (!ran) return null;
    const ranked = readRanking(ran.series, blocks);
    const crit = ran.plan.criterion;
    const errors = scriptErrors(ran.output);
    return (
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr style={{ color: 'var(--qu-muted)' }} className="text-left text-xs">
              <th className="px-2 py-1">#</th><th className="px-2 py-1">Model</th><th className="px-2 py-1">params</th><th className="px-2 py-1">R2</th>
              <th className="px-2 py-1">AIC</th><th className="px-2 py-1">AICc</th><th className="px-2 py-1">BIC</th>
              <th className="px-2 py-1">delta {crit.toUpperCase()}</th><th className="px-2 py-1">weight</th><th className="px-2 py-1">status</th>
            </tr>
          </thead>
          <tbody>
            {ranked.map((r) => {
              const b = blocks[r.index];
              const s = b.stats;
              const bad = r.status !== 'ok';
              return (
                <tr key={r.index} style={{ borderTop: '1px solid var(--qu-border)', opacity: r.status === 'failed' ? 0.7 : 1 }}>
                  <td className="px-2 py-1">{r.status === 'failed' ? '-' : r.rank}</td>
                  <td className="px-2 py-1"><span style={{ color: COLORS[r.index % COLORS.length] }}>{'■'} </span>{b.info.title}<div className="text-xs font-mono" style={{ color: 'var(--qu-muted)' }}>{b.info.formula}</div></td>
                  <td className="px-2 py-1 font-mono">{s ? s.k : ''}</td>
                  <td className="px-2 py-1 font-mono">{s ? fmt(s.r2) : ''}</td>
                  <td className="px-2 py-1 font-mono">{s ? fmt(s.aic, 5) : ''}</td>
                  <td className="px-2 py-1 font-mono">{s ? fmt(s.aicc, 5) : ''}</td>
                  <td className="px-2 py-1 font-mono">{s ? fmt(s.bic, 5) : ''}</td>
                  <td className="px-2 py-1 font-mono">{Number.isFinite(r.delta) ? fmt(r.delta, 3) : ''}</td>
                  <td className="px-2 py-1 font-mono">{Number.isFinite(r.weight) ? fmt(r.weight, 3) : ''}</td>
                  <td className="px-2 py-1" style={{ color: bad ? 'var(--qu-danger)' : 'var(--qu-ok)' }}>
                    {r.status === 'failed' ? `FAILED: ${errors[b.info.pfx] ?? 'no result'}` : r.status === 'not converged' ? 'did not converge' : r.equalFit && r.rank > 1 ? 'equal fit to the best' : r.rank === 1 ? 'best' : 'ok'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="text-xs mt-2" style={{ color: 'var(--qu-muted)' }}>
          Ranked by {crit.toUpperCase()} (lower is better); delta is the distance from the best, weights are Akaike weights (exp(-delta/2), normalised). A difference under 2 means the data cannot tell the models apart: prefer the simpler one.
          AIC = n ln(SSR/n) + 2k, or chi-square + 2k when y errors are given; k counts the free parameters. AICc adds a small-sample correction. Failed models are shown, not hidden.
        </p>
      </div>
    );
  };

  const card = (b: BlockResult) => {
    const s = b.stats;
    const sigmaWeights = spec.shape === 'yx' && spec.weight === 'sigma';
    const rows = !s
      ? []
      : ([
          ['R2', fmt(s.r2, 5)], ['adjusted R2', fmt(s.r2adj, 5)], ['RMSE', fmt(s.rmse)], ['sigma-hat (std. error of the fit)', fmt(s.sigma)],
          [sigmaWeights ? 'reduced chi-square' : 'residual variance s2', fmt(s.chi2red)], ['AIC', fmt(s.aic, 6)], ['AICc', fmt(s.aicc, 6)], ['BIC', fmt(s.bic, 6)],
          ['points n / parameters k / dof', `${s.n} / ${s.k} / ${s.dof}`], ['iterations', fmt(s.iterations, 3)], ['converged', s.converged === 1 ? 'yes' : 'NO'],
        ] as Array<[string, string]>);
    const res = !s
      ? []
      : ([
          ['mean', fmt(s.resMean)], ['standard deviation', fmt(s.resStd)], ['skewness', fmt(s.skew, 3)], ['excess kurtosis', fmt(s.exkurt, 3)],
          ['Jarque-Bera p', fmt(s.jbP, 3)], ['lag-1 autocorrelation', fmt(s.lag1, 3)],
        ] as Array<[string, string]>);
    const hints: string[] = [];
    if (s) {
      if (Number.isFinite(s.jbP)) hints.push(s.jbP < 0.05 ? 'The residuals depart from a normal distribution (Jarque-Bera p < 0.05): outliers or a model that misses structure.' : 'The residuals look normal (Jarque-Bera p >= 0.05; with few points this test has little power).');
      if (Number.isFinite(s.lag1) && Math.abs(s.lag1) > 0.3) hints.push('Neighbouring residuals are correlated (lag-1 above 0.3 in absolute value): the curve is missing a systematic trend.');
    }
    const t = (cells: React.ReactNode[], key: string, head = false) => (
      <tr key={key} style={{ borderTop: head ? undefined : '1px solid var(--qu-border)', color: head ? 'var(--qu-muted)' : undefined }} className={head ? 'text-xs text-left' : ''}>
        {cells.map((c, i) => <td key={i} className="px-2 py-1 font-mono">{c}</td>)}
      </tr>
    );
    return (
      <div key={b.info.pfx} className="rounded-lg border mb-4 p-2" style={{ borderColor: border }}>
        <div className="text-sm font-medium mb-1">{b.info.signal ? `${b.info.signal}: ` : ''}{b.info.title}</div>
        <div className="text-xs font-mono mb-2" style={{ color: 'var(--qu-muted)' }}>{b.info.formula}</div>
        {!b.ok ? (
          <div className="text-sm" style={{ color: 'var(--qu-danger)' }}>This fit failed{b.error ? `: ${b.error}` : ''}.</div>
        ) : (
          <>
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>{t(['parameter', 'value', 'std. error', '95% confidence interval', 't', 'p'], 'h', true)}</thead>
                <tbody>
                  {b.params.map((p) =>
                    t(
                      [p.name + (p.atBound ? ` *` : ''), fmt(p.value, 6), p.fixed ? 'fixed' : fmt(p.se), p.fixed ? '' : `${fmt(p.lo)} to ${fmt(p.hi)}`, p.fixed ? '' : fmt(p.t, 3), p.fixed ? '' : fmt(p.p, 3)],
                      p.name,
                    ),
                  )}
                </tbody>
              </table>
            </div>
            {b.params.some((p) => p.atBound) && <p className="text-xs mt-1" style={{ color: 'var(--qu-warning)' }}>* sits on its bound: its standard error is not valid there (the bound, not the data, decided it).</p>}
            <p className="text-xs mt-1" style={{ color: 'var(--qu-muted)' }}>
              Standard errors are the square roots of the diagonal of cov = s2 (J'J)^-1, with J the Jacobian of the residuals at the solution and s2 = SSR/(n-k): a linearised estimate that is exact for models linear in their parameters and an approximation otherwise. The 95% intervals are value +/- t(0.975, n-k) x std. error.
              {sigmaWeights ? ' With y errors given the residuals are weighted by 1/sigma; the covariance still uses the observed scatter (like scipy with absolute_sigma off), and reduced chi-square near 1 says the errors were honest.' : ''}
            </p>
            <div className="grid grid-cols-2 gap-x-6 mt-3">
              <div>{rows.map(([k, v]) => <div key={k} className="flex justify-between gap-2 text-xs py-0.5" style={{ borderBottom: '1px solid var(--qu-border)' }}><span style={{ color: 'var(--qu-muted)' }}>{k}</span><span className="font-mono">{v}</span></div>)}</div>
              <div>
                <div className="text-xs font-medium mb-0.5">Residuals (data - fit)</div>
                {res.map(([k, v]) => <div key={k} className="flex justify-between gap-2 text-xs py-0.5" style={{ borderBottom: '1px solid var(--qu-border)' }}><span style={{ color: 'var(--qu-muted)' }}>{k}</span><span className="font-mono">{v}</span></div>)}
              </div>
            </div>
            {hints.map((h) => <p key={h} className="text-xs mt-1">{h}</p>)}
          </>
        )}
      </div>
    );
  };

  const tabs: Array<{ id: Tab; label: string }> = (() => {
    if (!ran) return [];
    if (layout === 'compare') return [{ id: 'ranking', label: 'Ranking' }, { id: 'curves', label: 'Curves' }];
    if (layout === 'surface') return [{ id: 'surface', label: 'Surface' }, { id: 'residuals', label: 'Residuals' }, { id: 'numbers', label: 'Parameters and fit' }];
    const t: Array<{ id: Tab; label: string }> = [{ id: 'fit', label: spec.shape === 'multi' ? 'Signals' : 'Fit' }];
    if (multiBlocks && blocks.filter((b) => b.ok).length >= 2 && blocks.filter((b) => b.ok).length <= 3) t.push({ id: 'trajectory', label: 'Trajectory' });
    t.push({ id: 'residuals', label: 'Residuals' }, { id: 'numbers', label: 'Parameters and fit' });
    return t;
  })();
  const activeTab: Tab = tabs.some((t) => t.id === tab) ? tab : tabs[0]?.id ?? 'fit';

  const body = (() => {
    if (!ran) return <div className="p-6 text-sm" style={{ color: 'var(--qu-muted)' }}>{problem ? 'Fix the input on the left, then press Fit.' : 'Pick a model and press Fit: the data, the fitted curve, the residuals and the parameters with their uncertainty all come from the script on the left.'}</div>;
    switch (activeTab) {
      case 'ranking':
        return ranking();
      case 'curves': {
        const data: PlotData[] = [dots(col('x'), col('y'), 'data', '#888', 6)];
        blocks.forEach((b, i) => b.ok && data.push(line(b.gx, b.gy, b.info.title, COLORS[i % COLORS.length], b.converged ? undefined : 'dot')));
        return pv(data, { xlabel: spec.cols.x || 'x', ylabel: spec.cols.y || 'y', title: 'Every model on the same data (dotted: did not converge)', height: 440 });
      }
      case 'surface':
        return surfacePlot();
      case 'residuals':
        return residualPlots();
      case 'trajectory':
        return trajectory();
      case 'numbers':
        return <>{blocks.map(card)}{layout === 'shared' && blocks[0]?.ok && <p className="text-xs">The joint fit is one least-squares problem over every signal together; its parameter table is shown on the first signal.</p>}</>;
      default:
        return fitPlot();
    }
  })();

  // ------------------------------------------------------------ left column
  const modelOptions = modelsFor(spec.shape).map((m) => ({ id: m.id, label: m.label }));
  const def = modelDef(spec.model.id);
  const odeMode = spec.shape === 'multi' && spec.coupling === 'ode';
  const params = resolved.ok ? resolved.value.params : [];
  const settings = (name: string): ParamSetting => (odeMode ? spec.odePs[name] : spec.model.ps[name]) ?? {};
  const defaultGuess = (i: number) => (resolved.ok ? resolved.value.guess[i] : '');
  const sel = (value: string, onChange: (v: string) => void, options: Array<{ id: string; label: string }>) => (
    <select value={value} onChange={(e) => onChange(e.target.value)} className={inputClass} style={{ borderColor: border }}>
      {options.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
    </select>
  );
  const field = (label: string, control: React.ReactNode) => (
    <label className="flex flex-col gap-1 text-xs flex-1 min-w-0">
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      {control}
    </label>
  );
  const numField = (value: number, onChange: (n: number) => void, extra: React.InputHTMLAttributes<HTMLInputElement> = {}) => (
    <input type="number" value={value} onChange={(e) => onChange(Number(e.target.value))} className={inputClass} style={{ borderColor: border }} {...extra} />
  );
  const small = 'px-1.5 py-1 rounded text-xs bg-transparent border font-mono min-w-0 w-full';

  const modelSection = () => (
    <div className="flex flex-col gap-2">
      <span className="text-sm font-medium">Model</span>
      {spec.shape === 'yx' && (
        <div className="flex gap-1">
          {(['single', 'compare'] as const).map((m) => (
            <button key={m} className="qu-side-action flex-1" style={spec.mode === m ? { background: 'var(--qu-accent)', color: '#fff' } : undefined} onClick={() => { set({ mode: m }); setTab(m === 'compare' ? 'ranking' : 'fit'); }}>
              {m === 'single' ? 'One model' : 'Compare models'}
            </button>
          ))}
        </div>
      )}
      {spec.shape === 'multi' && (
        <>
          {field('How the signals are fitted', sel(spec.coupling, (v) => set({ coupling: v as FitSpec['coupling'] }), [
            { id: 'separate', label: 'Each signal separately (own parameters)' },
            { id: 'shared', label: 'Jointly, with shared parameters' },
            { id: 'ode', label: "ODE y' = f(t, y, p) per signal" },
          ]))}
        </>
      )}
      {spec.mode === 'compare' && spec.shape === 'yx' ? (
        <div className="flex flex-col gap-1">
          <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>Tick the models to fit to the same data</span>
          {COMPARE_CHOICES.map((c) => {
            const on = spec.compare.some((r) => compareKey(r) === c.key);
            return (
              <label key={c.key} className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={on}
                  onChange={(e) => set({ compare: e.target.checked ? [...spec.compare, c.key === CUSTOM_ID ? { ...c.ref, expr: spec.model.expr } : c.ref] : spec.compare.filter((r) => compareKey(r) !== c.key) })}
                />
                <span>{c.label}</span>
              </label>
            );
          })}
          {spec.compare.some((r) => r.id === CUSTOM_ID) && (
            <input
              value={spec.compare.find((r) => r.id === CUSTOM_ID)!.expr}
              onChange={(e) => set({ compare: spec.compare.map((r) => (r.id === CUSTOM_ID ? { ...r, expr: e.target.value } : r)) })}
              className={`${inputClass} font-mono`}
              style={{ borderColor: border }}
              placeholder="a*exp(-b*x) + c"
              spellCheck={false}
            />
          )}
          {field('Rank by', sel(spec.criterion, (v) => set({ criterion: v as FitSpec['criterion'] }), [{ id: 'aic', label: 'AIC' }, { id: 'aicc', label: 'AICc (small samples)' }, { id: 'bic', label: 'BIC (prefers simpler)' }]))}
        </div>
      ) : odeMode ? (
        <>
          <label className="flex flex-col gap-1 text-xs">
            <span style={{ color: 'var(--qu-muted)' }}>{"y' = f(t, y) with named parameters"}</span>
            <input value={spec.odeRhs} onChange={(e) => set({ odeRhs: e.target.value })} className={`${inputClass} font-mono`} style={{ borderColor: border }} placeholder="-k*y   or   r*y*(1 - y/K)" spellCheck={false} />
          </label>
          <label className="flex items-center gap-2 text-xs">
            <input type="checkbox" checked={spec.fitY0} onChange={(e) => set({ fitY0: e.target.checked })} /> Fit the initial value y(0) too (otherwise the first sample is used)
          </label>
          <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>The residual function integrates the ODE with ode45 at the sample times. One state variable y; samples are sorted by time.</p>
        </>
      ) : (
        <>
          {field('Model', sel(spec.model.id, (v) => setModel({ id: v }), [...modelOptions, { id: CUSTOM_ID, label: 'Custom expression...' }]))}
          {spec.model.id === CUSTOM_ID ? (
            <>
              <label className="flex flex-col gap-1 text-xs">
                <span style={{ color: 'var(--qu-muted)' }}>Expression in {spec.shape === 'surface' ? 'x, y' : spec.shape === 'multi' ? 't' : 'x'} and named parameters</span>
                <input value={spec.model.expr} onChange={(e) => setModel({ expr: e.target.value })} className={`${inputClass} font-mono`} style={{ borderColor: border }} placeholder={spec.shape === 'surface' ? 'a*x^2 + b*x*y + c*y^2 + d' : 'a*exp(-b*x) + c'} spellCheck={false} />
              </label>
              <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>Names other than {spec.shape === 'surface' ? 'x and y' : spec.shape === 'multi' ? 't' : 'x'}, pi, e and the math functions (sin cos tan exp log sqrt abs ...) become parameters. Only a plain math expression is accepted.</p>
            </>
          ) : (
            <>
              <div className="text-xs font-mono" style={{ color: 'var(--qu-muted)' }}>{resolved.ok ? resolved.value.formula : def?.formula}</div>
              {def?.degree && field(`Degree (${def.degree.min} to ${def.degree.max})`, numField(spec.model.degree, (n) => setModel({ degree: n }), { min: def.degree.min, max: def.degree.max, step: 1 }))}
              {def?.note && <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>{def.note}</p>}
            </>
          )}
          {spec.shape === 'multi' && spec.coupling === 'shared' && resolved.ok && (
            <div className="flex flex-col gap-1">
              <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>Parameters shared by all signals (the others are fitted per signal)</span>
              <div className="flex flex-wrap gap-3">
                {resolved.value.params.map((p) => (
                  <label key={p} className="flex items-center gap-1 text-xs font-mono">
                    <input type="checkbox" checked={spec.shared.includes(p)} onChange={(e) => set({ shared: e.target.checked ? [...spec.shared, p] : spec.shared.filter((s) => s !== p) })} />
                    {p}
                  </label>
                ))}
              </div>
            </div>
          )}
        </>
      )}
      {!(spec.mode === 'compare' && spec.shape === 'yx') && resolved.ok && params.length > 0 && (
        <details>
          <summary className="text-xs cursor-pointer" style={{ color: 'var(--qu-muted)' }}>Starting values, fixed parameters and bounds</summary>
          <div className="grid gap-1 mt-1 text-xs" style={{ gridTemplateColumns: 'auto 1fr auto 3.5rem 3.5rem' }}>
            <span /><span style={{ color: 'var(--qu-muted)' }}>start</span><span style={{ color: 'var(--qu-muted)' }}>fixed</span><span style={{ color: 'var(--qu-muted)' }}>min</span><span style={{ color: 'var(--qu-muted)' }}>max</span>
            {params.map((p, i) => {
              const s = settings(p);
              return (
                <React.Fragment key={p}>
                  <span className="font-mono py-1">{p}</span>
                  <input className={small} style={{ borderColor: border }} value={s.guess ?? ''} placeholder={defaultGuess(i)} onChange={(e) => setParam(p, { guess: e.target.value })} spellCheck={false} />
                  <input type="checkbox" checked={!!s.fixed} onChange={(e) => setParam(p, { fixed: e.target.checked })} title="hold this parameter at its start value" />
                  <input className={small} style={{ borderColor: border }} value={s.lo ?? ''} placeholder="-inf" onChange={(e) => setParam(p, { lo: e.target.value })} />
                  <input className={small} style={{ borderColor: border }} value={s.hi ?? ''} placeholder="inf" onChange={(e) => setParam(p, { hi: e.target.value })} />
                </React.Fragment>
              );
            })}
          </div>
          <p className="text-xs mt-1" style={{ color: 'var(--qu-muted)' }}>
            Starts may use the data summary (d_x0, d_x1, d_xr, d_y0, d_y1, d_ylo, d_yhi, d_ym, d_xpk, d_ypk, d_xh for y(x); d_zm, d_xpk ... for surfaces) and math. Bounds are enforced by clamping each trial point (least_squares lower=/upper=); a parameter on its bound is flagged.
          </p>
        </details>
      )}
    </div>
  );

  const optionsSection = () => (
    <details>
      <summary className="text-sm font-medium cursor-pointer">Options</summary>
      <div className="flex flex-col gap-2 mt-2">
        {spec.shape !== 'surface' && !odeMode && field('Robust fitting', sel(spec.robust, (v) => set({ robust: v as FitSpec['robust'] }), [{ id: 'none', label: 'off (plain least squares)' }, { id: 'huber', label: 'Huber weights (IRLS)' }, { id: 'bisquare', label: 'Bisquare weights (IRLS)' }]))}
        {spec.shape === 'surface' && field('Robust fitting', sel(spec.robust, (v) => set({ robust: v as FitSpec['robust'] }), [{ id: 'none', label: 'off (plain least squares)' }, { id: 'huber', label: 'Huber weights (IRLS)' }, { id: 'bisquare', label: 'Bisquare weights (IRLS)' }]))}
        <div className="flex gap-2">
          {field('Multi-start (1-9)', numField(spec.starts, (n) => set({ starts: n }), { min: 1, max: 9, step: 1 }))}
          {field('Extrapolate by (x range)', numField(spec.extrap, (n) => set({ extrap: n }), { min: 0, max: 5, step: 0.1 }))}
        </div>
        {spec.shape !== 'surface' && (
          <>
            <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={spec.band} onChange={(e) => set({ band: e.target.checked })} /> 95% confidence band of the curve</label>
            <div className="flex gap-4 text-xs">
              <label className="flex items-center gap-1"><input type="checkbox" checked={spec.logx} onChange={(e) => set({ logx: e.target.checked })} /> log x axis</label>
              <label className="flex items-center gap-1"><input type="checkbox" checked={spec.logy} onChange={(e) => set({ logy: e.target.checked })} /> log y axis</label>
            </div>
          </>
        )}
      </div>
    </details>
  );

  const fitLeft = (
    <div className="w-[22rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
      <CurveFitData
        value={spec}
        ui={ui}
        onChange={(patch) => {
          if (patch.shape && patch.shape !== spec.shape) changeShape(patch.shape);
          const { shape: _ignored, ...rest } = patch;
          void _ignored;
          setSpec((s) => ({ ...s, ...rest, ...(patch.shape ? { shape: patch.shape } : {}) }));
        }}
        onUi={(patch) => setUi((u) => ({ ...u, ...patch }))}
        invoke={invoke}
        signalVars={signalVars}
        loadSignal={loadSignal}
      />
      {modelSection()}
      {optionsSection()}
      <button className="qu-side-action w-full" style={{ background: 'var(--qu-accent)', color: '#fff' }} disabled={running || !!problem} onClick={() => void runFit()} title={problem ?? 'Run the script and draw the result'}>
        <Play size={13} /> {running ? 'Fitting...' : 'Fit'}
      </button>
      {(problem || error) && (
        <div className="qu-banner" data-kind="error" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
          <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
          <span className="break-words whitespace-pre-wrap">{problem ?? error}</span>
        </div>
      )}
      {warnings.map((w) => (
        <div key={w} className="qu-banner" data-kind="warning" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', fontSize: 12.5, padding: '8px 10px' }}>
          <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
          <span>{w}</span>
        </div>
      ))}
      {stale && <p className="text-xs" style={{ color: 'var(--qu-warning)' }}>The settings changed since the last fit: press Fit to refresh the results.</p>}
      <div className="flex items-center gap-2 text-xs">
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={editing}
            onChange={(e) => {
              setEditing(e.target.checked);
              if (e.target.checked) setCodeText(shown);
            }}
          />
          Edit the code
        </label>
        {editing && <span style={{ color: 'var(--qu-warning)' }}>The panel no longer drives the script; Fit runs your text and reads the same variable names.</span>}
      </div>
      {editing ? (
        <textarea value={codeText} onChange={(e) => setCodeText(e.target.value)} rows={16} className="rounded-lg p-2 text-xs font-mono w-full" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }} spellCheck={false} />
      ) : (
        <div className="relative rounded-lg p-2 text-xs font-mono overflow-x-auto whitespace-pre" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)', maxHeight: 420 }}>
          <button
            title="Copy this fit as Qu code"
            className="absolute top-1 right-1 p-1 rounded"
            style={{ color: 'var(--qu-muted)' }}
            onClick={() => void navigator.clipboard?.writeText(shown).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); })}
          >
            <Copy size={12} />
          </button>
          {shown || `# ${problem}`}
          {copied && <span style={{ color: 'var(--qu-ok)' }}> copied</span>}
        </div>
      )}
      {onOpenInEditor && (
        <button className="qu-side-action w-full" disabled={!planR.ok && !editing} onClick={() => onOpenInEditor(editing ? codeText : shown, 'curve_fit.qu')}>
          <FileCode2 size={13} /> Open in editor
        </button>
      )}
      <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
        Fits use least_squares (Levenberg-Marquardt) on the residual function above. Plots are drawn by the Studio plot viewer from the vectors the script leaves behind.
      </p>
    </div>
  );

  return (
    <div className="flex-1 min-w-0 min-h-0 flex flex-col">
      <div className="qu-live-tabs" role="tablist" aria-label="Fit mode">
        <button role="tab" aria-selected={mode === 'fit'} onClick={() => setMode('fit')}>Fit</button>
        <button role="tab" aria-selected={mode === 'solve'} onClick={() => setMode('solve')}>Solve equations</button>
      </div>
      <div className="flex-1 min-h-0 flex">
        {mode === 'solve' ? (
          <SolveEquations theme={theme} invoke={invoke} onOpenInEditor={onOpenInEditor} />
        ) : (
          <>
            {fitLeft}
            <div className="flex-1 min-w-0 min-h-0 flex flex-col">
              {tabs.length > 0 && (
                <div className="qu-live-tabs" role="tablist" aria-label="Fit views">
                  {tabs.map((t) => <button key={t.id} role="tab" aria-selected={activeTab === t.id} onClick={() => setTab(t.id)}>{t.label}</button>)}
                </div>
              )}
              <div className="flex-1 min-h-0 overflow-auto p-3">{body}</div>
            </div>
          </>
        )}
      </div>
    </div>
  );
};

export { modelLabel, autoCols };
export default CurveFitPanel;
