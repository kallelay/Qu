import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PlotViewer } from '@qu/ui-components';
import type { PlotData } from '@qu/ui-components';
import { AlertCircle, Copy, FileCode2 } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import {
  FAMILIES,
  FILTER_KINDS,
  SIGNALS,
  buildFilterScript,
  parseFilterNumbers,
  validateDesign,
  type DesignSpec,
  type FilterNumbers,
  type SignalVariable,
} from './filterDesign';

export interface FilterDesignerProps extends LiveSerialPlotPanelProps {
  /** Numeric vectors of the Code tab's last run, usable as the input signal. */
  signalVars?: SignalVariable[];
  /** Every sample of one of `signalVars` (from the session's interpreter). */
  loadSignal?: (name: string) => Promise<(number | null)[]>;
  /** Opens `code` as a new editor buffer named `name` in the Code tab. */
  onOpenInEditor?: (code: string, name: string) => void;
}

const DEFAULT_SPEC: DesignSpec = {
  family: 'butter', kind: 'low', order: 4, f1: 100, f2: 200, fs: 1000, ripple: 1, atten: 40, signal: 'tones', source: 'example', varName: '',
};
// The designer unmounts whenever another top-level tab is shown; without this
// a trip to the Code tab and back threw the design away.
let remembered: { spec: DesignSpec; tab: Tab } = { spec: DEFAULT_SPEC, tab: 'response' };
let signalFileSeq = 0;

interface VarShape {
  name: string;
  data: (number | null)[];
}
interface ExecuteShape {
  success: boolean;
  error: string | null;
  data?: VarShape[];
}

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border';

type Tab = 'response' | 'time' | 'poles' | 'nyquist' | 'phase' | 'signal';
const TABS: Array<{ id: Tab; label: string }> = [
  { id: 'response', label: 'Response' },
  { id: 'time', label: 'Impulse / step' },
  { id: 'poles', label: 'Poles & zeros' },
  { id: 'nyquist', label: 'Nyquist' },
  { id: 'phase', label: 'Phase & delay' },
  { id: 'signal', label: 'Test signal' },
];

type Series = Record<string, (number | null)[]>;

/** Magnitude response with draggable -3 dB edges. The cutoff handles move
 *  immediately under the pointer (a local ghost), and each move also updates the
 *  design so the real curve follows a moment later. */
const ResponseChart: React.FC<{
  f: (number | null)[];
  magDb: (number | null)[];
  fs: number;
  edges: number[];
  onEdge: (index: number, hz: number) => void;
}> = ({ f, magDb, fs, edges, onEdge }) => {
  const W = 760;
  const H = 360;
  const m = { l: 52, r: 16, t: 14, b: 38 };
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const yMin = -100;
  const yMax = 5;
  const nyq = fs / 2;
  const x = (hz: number) => m.l + (hz / nyq) * (W - m.l - m.r);
  const y = (db: number) => m.t + ((yMax - Math.min(yMax, Math.max(yMin, db))) / (yMax - yMin)) * (H - m.t - m.b);
  const path = useMemo(() => {
    let d = '';
    let pen = false;
    for (let i = 0; i < f.length; i++) {
      const fv = f[i];
      const mv = magDb[i];
      if (fv == null || mv == null) {
        pen = false;
        continue;
      }
      d += `${pen ? 'L' : 'M'}${x(fv).toFixed(1)},${y(mv).toFixed(1)}`;
      pen = true;
    }
    return d;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [f, magDb, fs]);

  const hzAt = (clientX: number) => {
    const r = svgRef.current!.getBoundingClientRect();
    const px = ((clientX - r.left) / r.width) * W;
    return Math.min(nyq * 0.995, Math.max(nyq * 0.002, ((px - m.l) / (W - m.l - m.r)) * nyq));
  };
  const xTicks = [0, 0.25, 0.5, 0.75, 1].map((t) => t * nyq);
  const yTicks = [0, -20, -40, -60, -80, -100];

  return (
    <svg
      ref={svgRef}
      viewBox={`0 0 ${W} ${H}`}
      style={{ width: '100%', maxHeight: 420, touchAction: 'none', color: 'var(--qu-text)' }}
      onPointerMove={(e) => {
        if (drag !== null) onEdge(drag, Math.round(hzAt(e.clientX) * 10) / 10);
      }}
      onPointerUp={() => setDrag(null)}
      onPointerLeave={() => setDrag(null)}
      role="img"
      aria-label="Magnitude response in dB with draggable cutoff"
    >
      {yTicks.map((t) => (
        <g key={t}>
          <line x1={m.l} x2={W - m.r} y1={y(t)} y2={y(t)} stroke="var(--qu-border)" strokeWidth={t === 0 ? 1 : 0.5} />
          <text x={m.l - 6} y={y(t) + 4} textAnchor="end" fontSize={11} fill="var(--qu-muted)">{t}</text>
        </g>
      ))}
      {xTicks.map((t) => (
        <text key={t} x={x(t)} y={H - m.b + 16} textAnchor="middle" fontSize={11} fill="var(--qu-muted)">
          {Math.round(t)}
        </text>
      ))}
      <line x1={m.l} x2={W - m.r} y1={y(-3)} y2={y(-3)} stroke="var(--qu-muted)" strokeDasharray="3 4" strokeWidth={0.8} />
      <text x={W - m.r - 2} y={y(-3) - 4} textAnchor="end" fontSize={10} fill="var(--qu-muted)">-3 dB</text>
      <path d={path} fill="none" stroke="var(--qu-accent)" strokeWidth={2} />
      {edges.map((hz, i) => (
        <g key={i} style={{ cursor: 'ew-resize' }} onPointerDown={(e) => { (e.target as Element).setPointerCapture?.(e.pointerId); setDrag(i); }}>
          <line x1={x(hz)} x2={x(hz)} y1={m.t} y2={H - m.b} stroke="var(--qu-warning)" strokeWidth={1.5} strokeDasharray="5 3" />
          <rect x={x(hz) - 7} y={m.t} width={14} height={H - m.t - m.b} fill="transparent" />
          <rect x={x(hz) - 5} y={m.t + 2} width={10} height={16} rx={3} fill="var(--qu-warning)" />
          <text x={x(hz)} y={H - m.b - 6} textAnchor="middle" fontSize={11} fill="var(--qu-warning)">{hz} Hz</text>
        </g>
      ))}
      <text x={(m.l + W - m.r) / 2} y={H - 4} textAnchor="middle" fontSize={11} fill="var(--qu-muted)">Frequency (Hz)</text>
      <text x={12} y={(m.t + H - m.b) / 2} textAnchor="middle" fontSize={11} fill="var(--qu-muted)" transform={`rotate(-90 12 ${(m.t + H - m.b) / 2})`}>Magnitude (dB)</text>
    </svg>
  );
};

const fmt = (v: number | null | undefined, unit = '', digits = 4): string => {
  if (v == null || !Number.isFinite(v)) return 'n/a';
  // A gain of exactly zero arrives as 20*log10(1e-300) = -6000 dB.
  if (unit === 'dB' && v <= -300) return '-inf dB';
  if (Math.abs(v) < 1e-9) return unit ? `0 ${unit}` : '0';
  const a = Math.abs(v);
  const s = a !== 0 && (a < 1e-3 || a >= 1e5) ? v.toExponential(2) : String(Number(v.toPrecision(digits)));
  return unit ? `${s} ${unit}` : s;
};

const ms = (v: number | null): number | null => (v == null ? null : v * 1000);

const NumbersCard: React.FC<{ n: FilterNumbers | null; fs: number }> = ({ n, fs }) => {
  const rows: Array<[string, string, string]> = n
    ? [
        ['Delay', fmt(n.delay * 1000, 'ms') + ` (${fmt(n.delaySamples)} samples)`, 'Group delay at the passband centre: how late the filtered signal arrives.'],
        ['Delay spread', fmt(n.delaySpread, 'samples'), 'How much the delay varies across the passband (0 = linear phase, no waveform distortion).'],
        ['Impulse response', fmt(n.impulseLength, 'samples') + ` (${fmt((n.impulseLength / fs) * 1000, 'ms')})`, 'Samples holding 99.9 % of the impulse energy: how long the filter rings.'],
        ['Time constant tau', fmt(ms(n.tau), 'ms'), 'Step response reaches 63.2 % of its final value. n/a when the filter has no DC gain (high/band-pass).'],
        ['Slowest pole tau', fmt(ms(n.tauPole), 'ms'), 'From the pole closest to the unit circle. n/a for FIR.'],
        ['Rise / settle', `${fmt(ms(n.rise), 'ms')} / ${fmt(ms(n.settle), 'ms')}`, 'Step 10-90 % rise time, and time to stay within 2 % of final.'],
        ['Overshoot', fmt(n.overshoot, '%'), 'Step response peak above its final value.'],
        ['-3 dB cutoff', n.edgeLow != null && n.edgeHigh != null ? `${fmt(n.edgeLow, 'Hz')} to ${fmt(n.edgeHigh, 'Hz')}` : fmt(n.cutoff, 'Hz'), 'Where the magnitude falls 3 dB below its peak.'],
        ['DC gain', fmt(n.dcGainDb, 'dB'), 'Gain at 0 Hz.'],
        ['Stability margin', `${n.stable ? 'stable' : 'UNSTABLE'}, ${fmt(n.stabilityMargin)}`, 'One minus the largest pole radius: how far the poles are from the unit circle.'],
      ]
    : [];
  return (
    <div className="rounded-lg border text-xs" style={{ borderColor: border }}>
      {rows.length === 0 && <div className="p-2" style={{ color: 'var(--qu-muted)' }}>Numbers appear after the first run.</div>}
      {rows.map(([k, v, tip]) => (
        <div key={k} className="flex justify-between gap-3 px-2 py-1.5" style={{ borderBottom: '1px solid var(--qu-border)' }} title={tip}>
          <span style={{ color: 'var(--qu-muted)' }}>{k}</span>
          <span className="font-mono text-right" style={{ color: k === 'Stability margin' && n && !n.stable ? 'var(--qu-danger)' : 'var(--qu-text)' }}>{v}</span>
        </div>
      ))}
    </div>
  );
};

export const FilterDesigner: React.FC<FilterDesignerProps> = ({ theme, invoke, signalVars = [], loadSignal, onOpenInEditor }) => {
  const [spec, setSpec] = useState<DesignSpec>(remembered.spec);
  const [tab, setTab] = useState<Tab>(remembered.tab);
  useEffect(() => {
    remembered = { spec, tab };
  }, [spec, tab]);
  const [series, setSeries] = useState<Series>({});
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const [copied, setCopied] = useState(false);
  // What a number field shows while it is being typed in. The design only
  // takes values that parse, so without this an emptied field snapped back
  // to its old value at once and could not be cleared to type a new one.
  const [drafts, setDrafts] = useState<Partial<Record<keyof DesignSpec, string>>>({});
  // `source: 'variable'`: the variable's samples, written to a file the
  // hidden run reads (see buildFilterScript).
  const [signalFile, setSignalFile] = useState('');
  const [signalNote, setSignalNote] = useState('');
  const runId = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const own = spec.source === 'variable';
  const problem = useMemo(() => validateDesign(spec), [spec]);
  const code = useMemo(() => buildFilterScript(spec, true, signalFile), [spec, signalFile]);
  const shown = useMemo(() => buildFilterScript(spec, false), [spec]);
  const family = FAMILIES.find((f) => f.id === spec.family)!;
  const twoBand = spec.kind === 'band' || spec.kind === 'stop';
  const set = (patch: Partial<DesignSpec>) => setSpec((s) => ({ ...s, ...patch }));
  const num = (key: keyof DesignSpec) => (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value;
    setDrafts((d) => ({ ...d, [key]: raw }));
    if (raw.trim() === '') return;
    const v = Number(raw);
    if (Number.isFinite(v)) set({ [key]: v } as Partial<DesignSpec>);
  };
  const setFamily = (id: DesignSpec['family']) => {
    // An order-4 FIR is a 5-tap filter that barely filters at all, and a
    // high-order IIR is numerically fragile: moving between the two kinds
    // of family starts from an order that suits the new one.
    const toFir = FAMILIES.find((f) => f.id === id)!.fir;
    setDrafts((d) => ({ ...d, order: undefined }));
    setSpec((s) => {
      const wasFir = FAMILIES.find((f) => f.id === s.family)!.fir;
      let order = s.order;
      if (toFir && !wasFir && order < 16) order = 32;
      if (!toFir && wasFir && order > 12) order = 4;
      return { ...s, family: id, order };
    });
  };

  // The chosen variable's data, re-read after every Code-tab run (a new
  // `signalVars` list) so the designer filters what the variable holds now.
  const chosen = own ? signalVars.find((v) => v.name === spec.varName) : undefined;
  useEffect(() => {
    if (!own || !spec.varName || !loadSignal) {
      setSignalFile('');
      setSignalNote('');
      return;
    }
    let live = true;
    setSignalNote(`Loading ${spec.varName}…`);
    (async () => {
      try {
        const data = await loadSignal(spec.varName!);
        const buf = new Float64Array(data.length);
        data.forEach((x, i) => (buf[i] = x == null ? NaN : x));
        const { writeBinaryFile, createDir, removeFile } = await import('@tauri-apps/api/fs');
        const { appCacheDir, join } = await import('@tauri-apps/api/path');
        const dir = await appCacheDir();
        await createDir(dir, { recursive: true }).catch(() => undefined);
        const file = await join(dir, `filter-designer-signal-${++signalFileSeq}.f64`);
        await writeBinaryFile(file, new Uint8Array(buf.buffer));
        if (!live) return;
        setSignalFile((prev) => {
          if (prev) void removeFile(prev).catch(() => undefined);
          return file;
        });
        setSignalNote(`${spec.varName}: ${data.length} samples, filtered at fs = ${spec.fs} Hz`);
      } catch (err) {
        if (live) setSignalNote(`Could not read ${spec.varName}: ${err}`);
      }
    })();
    return () => {
      live = false;
    };
    // spec.fs only feeds the note; re-reading the data for it is not needed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [own, spec.varName, signalVars, loadSignal]);

  const run = useCallback(async (source: string) => {
    const mine = ++runId.current;
    setRunning(true);
    try {
      const r = await invoke<ExecuteShape>('execute_code', { request: { code: source, file_path: null } });
      if (mine !== runId.current) return; // a newer design already superseded this one
      if (!r.success || !r.data?.length) {
        setError(r.error ?? 'The design did not run');
        return; // keep the last good plots on screen
      }
      const next: Series = {};
      for (const v of r.data) next[v.name] = v.data;
      setSeries(next);
      setError('');
    } catch (err) {
      if (mine === runId.current) setError(String(err));
    } finally {
      if (mine === runId.current) setRunning(false);
    }
  }, [invoke]);

  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    if (problem || (own && !signalFile)) return;
    timer.current = setTimeout(() => void run(code), 300);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [code, problem, run, own, signalFile]);

  const numbers = useMemo(() => parseFilterNumbers(series['fi']), [series]);
  const col = (name: string) => series[name] ?? [];
  const pv = (data: PlotData[], extra: Partial<React.ComponentProps<typeof PlotViewer>> = {}) => (
    <PlotViewer data={data} theme={theme} showLegend height={380} {...extra} />
  );
  const line = (x: (number | null)[], y: (number | null)[], name: string, color?: string): PlotData => ({ x, y, name, type: 'line', mode: 'lines', line: { width: 2, color } });

  const body = (() => {
    if (!series['f']) return <div className="p-6 text-sm" style={{ color: 'var(--qu-muted)' }}>Designing…</div>;
    const f = col('f');
    switch (tab) {
      case 'response':
        return (
          <ResponseChart
            f={f}
            magDb={col('mag_db')}
            fs={spec.fs}
            edges={twoBand ? [spec.f1, spec.f2] : [spec.f1]}
            onEdge={(i, hz) => set(i === 0 ? { f1: hz } : { f2: hz })}
          />
        );
      case 'time': {
        const k = col('h').map((_, i) => i);
        return (
          <>
            {pv([{ x: k, y: col('h'), name: 'impulse', type: 'bar' }], { xlabel: 'sample n', ylabel: 'amplitude', title: 'Impulse response', height: 330 })}
            {pv([line(k, col('s'), 'step')], { xlabel: 'sample n', ylabel: 'amplitude', title: 'Step response', height: 330 })}
          </>
        );
      }
      case 'poles': {
        const t = Array.from({ length: 181 }, (_, i) => (i / 180) * 2 * Math.PI);
        return pv(
          [
            { x: t.map(Math.cos), y: t.map(Math.sin), name: 'unit circle', type: 'line', mode: 'lines', line: { width: 1, color: '#888' } },
            { x: col('zr'), y: col('zim'), name: 'zeros', type: 'scatter', mode: 'markers', marker: { size: 10, symbol: 'circle-open' } },
            { x: col('pr'), y: col('pim'), name: 'poles', type: 'scatter', mode: 'markers', marker: { size: 11, symbol: 'x' } },
          ],
          { equalAspect: true, xlabel: 'real', ylabel: 'imaginary', title: 'Poles and zeros: inside the circle = stable', height: 560 },
        );
      }
      case 'nyquist':
        return pv(
          [
            line(col('hr'), col('hi'), 'H(e^jw)'),
            { x: [-1], y: [0], name: '-1', type: 'scatter', mode: 'markers', marker: { size: 9, color: '#d33' } },
          ],
          { equalAspect: true, xlabel: 'Re H', ylabel: 'Im H', title: 'Nyquist plot of H(e^jw)', height: 560 },
        );
      case 'phase':
        return (
          <>
            {pv([line(f, col('ph'), 'phase (rad)')], { xlabel: 'Hz', ylabel: 'rad', title: 'Phase (wrapped to +/- pi)', height: 330 })}
            {pv([line(f, col('gd'), 'group delay (samples)')], { xlabel: 'Hz', ylabel: 'samples', title: 'Group delay', height: 330 })}
          </>
        );
      case 'signal': {
        const t = col('x');
        return (
          <>
            {own
              ? pv([line(t, col('sig'), spec.varName ?? 'input'), line(t, col('y'), 'filtered', '#f5a524')], { xlabel: 'time (s)', ylabel: 'amplitude', title: `${spec.varName}, before and after`, height: 340 })
              : pv([line(t, col('sig'), 'input'), line(t, col('y'), 'filtered', '#f5a524')], { xlabel: 'time (s)', ylabel: 'amplitude', title: 'Test signal, before and after (first 0.2 s)', xlim: [0, 0.2], height: 340 })}
            {pv([line(col('sf'), col('sx'), 'input'), line(col('sf'), col('sy'), 'filtered')], { xlabel: 'Hz', ylabel: '|X|', title: 'Spectrum, before and after', height: 340 })}
          </>
        );
      }
    }
  })();

  const select = (value: string, onChange: (v: string) => void, options: Array<{ id: string; label: string }>) => (
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
  const numInput = (key: keyof DesignSpec, extra: React.InputHTMLAttributes<HTMLInputElement> = {}) => (
    <input
      type="number"
      value={drafts[key] ?? (spec[key] as number)}
      onChange={num(key)}
      onBlur={() => setDrafts((d) => ({ ...d, [key]: undefined }))}
      className={inputClass}
      style={{ borderColor: border }}
      {...extra}
    />
  );
  const sourceOptions = [
    { id: 'example', label: 'Example' },
    { id: 'variable', label: 'Variable from last run' },
  ];
  const varOptions = [
    ...(spec.varName && !chosen ? [{ id: spec.varName, label: `${spec.varName} (not in the last run)` }] : []),
    ...signalVars.map((v) => ({ id: v.name, label: `${v.name} (${v.length})` })),
  ];

  return (
    <>
      {/* Every child keeps its natural height (flex-shrink-0): in a column
          that scrolls, shrinking squashed the code box to two clipped lines. */}
      <div className="w-[22rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <span className="text-sm font-medium">Filter design</span>
        <div className="flex gap-2">
          {field('Family', select(spec.family, (v) => setFamily(v as DesignSpec['family']), FAMILIES))}
          {field('Type', select(spec.kind, (v) => set({ kind: v as DesignSpec['kind'] }), FILTER_KINDS))}
        </div>
        <div className="flex gap-2">
          {field(family.fir ? 'Order (even)' : 'Order', numInput('order', { min: 1, max: 200 }))}
          {field('Sample rate fs (Hz)', numInput('fs', { min: 1 }))}
        </div>
        <div className="flex gap-2">
          {field(twoBand ? 'Edge f1 (Hz)' : 'Cutoff (Hz)', numInput('f1', { min: 0 }))}
          {twoBand && field('Edge f2 (Hz)', numInput('f2', { min: 0 }))}
        </div>
        {(family.ripple || family.atten) && (
          <div className="flex gap-2">
            {family.ripple && field('Passband ripple (dB)', numInput('ripple', { min: 0.01, step: 0.1 }))}
            {family.atten && field('Stopband atten. (dB)', numInput('atten', { min: 1 }))}
          </div>
        )}
        <div className="flex gap-2">
          {field('Signal source', select(spec.source ?? 'example', (v) => {
            const source = v as DesignSpec['source'];
            // Default to the first usable variable rather than to nothing.
            set(source === 'variable' && !spec.varName && signalVars[0] ? { source, varName: signalVars[0].name } : { source });
          }, sourceOptions))}
          {own
            ? field('Variable', varOptions.length
              ? select(spec.varName ?? '', (v) => set({ varName: v }), spec.varName ? varOptions : [{ id: '', label: 'Pick one…' }, ...varOptions])
              : <span className="text-xs py-1.5" style={{ color: 'var(--qu-muted)' }}>None yet: run a script in the Code tab.</span>)
            : field('Test signal', select(spec.signal, (v) => set({ signal: v as DesignSpec['signal'] }), SIGNALS))}
        </div>
        {own && signalNote && (
          <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>{signalNote}. Set fs above to the rate it was sampled at.</p>
        )}
        {(problem || error) && (
          <div className="qu-banner" data-kind="error" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words">{problem ?? error}</span>
          </div>
        )}
        <NumbersCard n={numbers} fs={spec.fs} />
        <div className="relative rounded-lg p-2 text-xs font-mono overflow-x-auto whitespace-pre" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }}>
          <button
            title="Copy this design as Qu code"
            className="absolute top-1 right-1 p-1 rounded"
            style={{ color: 'var(--qu-muted)' }}
            onClick={() => {
              void navigator.clipboard?.writeText(shown).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); });
            }}
          >
            <Copy size={12} />
          </button>
          {shown}
          {copied && <span style={{ color: 'var(--qu-ok)' }}> copied</span>}
        </div>
        {onOpenInEditor && (
          <button
            className="qu-side-action w-full"
            disabled={!!problem}
            title={own ? `Open this design in a new editor tab; it filters ${spec.varName} from the session` : 'Open this design in a new editor tab'}
            onClick={() => onOpenInEditor(shown, `filter_${spec.family}_${spec.kind}.qu`)}
          >
            <FileCode2 size={13} /> Open in editor
          </button>
        )}
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          {running ? 'Updating…' : 'Drag a dashed cutoff on the Response tab, or edit a field: everything redraws from real Qu.'}
        </p>
      </div>
      <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <div className="qu-live-tabs" role="tablist" aria-label="Filter views">
          {TABS.map((t) => (
            <button key={t.id} role="tab" aria-selected={tab === t.id} onClick={() => setTab(t.id)}>{t.label}</button>
          ))}
        </div>
        <div className="flex-1 min-h-0 overflow-auto p-3">{body}</div>
      </div>
    </>
  );
};

export default FilterDesigner;
