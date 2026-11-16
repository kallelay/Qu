import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { create } from 'zustand';
import { AlertCircle, Copy, Eraser, FileCode2, FolderOpen, Pause, Play, Square } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import {
  CONTROLLER_EXAMPLES,
  C_CONTROLLER_TEMPLATE,
  DEFAULT_LOOP,
  PLANT_EXAMPLES,
  SIGNALS,
  WORKED_EXAMPLES,
  TRACE_SIGNALS,
  autoRange,
  buildCompareScript,
  buildLoopScript,
  buildReplayScript,
  compareVerdict,
  niceTicks,
  parseLoopLine,
  parsePortList,
  PORT_LIST_SCRIPT,
  usesIo,
  type IoSpec,
  rowReason,
  splitChunk,
  tracePathProblem,
  validateLoop,
  verdict,
  type DiffRow,
  type LoopSample,
  type LoopSpec,
  type LoopStats,
  type ResolvedSources,
  type SignalName,
  type SourceSpec,
} from './loopDesign';
import { environmentSpec } from './loopDesign';
import { EnvironmentPicker, PluginsSection } from './LoopPluginsPanel';

/** The Code tab's active file, offered as a plant or controller source. */
export interface EditorFile {
  name: string;
  code: string;
}

export interface LoopPanelProps extends LiveSerialPlotPanelProps {
  editorFile?: EditorFile | null;
  /** Opens `code` as a new editor buffer named `name` in the Code tab. */
  onOpenInEditor?: (code: string, name: string) => void;
}

/** The loop runs in its own `qu kernel` (never the Code tab's session), and
 *  is restarted before every run so no timer or global survives into the
 *  next one. */
export const LOOP_SESSION = 'qu-studio-loop';

interface ExecuteShape {
  success: boolean;
  error: string | null;
  elapsed_ms?: number;
}

type RunState = 'idle' | 'running' | 'done' | 'stopped' | 'failed';

// ---------------------------------------------------------------- run store
// Module-level so a run keeps streaming into the panel across a trip to
// another tab (the panel unmounts; the store and its listener do not).

const MAX_SAMPLES = 200_000;
const MAX_TEXT = 200;

interface LoopStore {
  spec: LoopSpec;
  state: RunState;
  error: string;
  stats: LoopStats | null;
  /** Spec of the run on screen (the verdict reads its mode and tolerance). */
  ranSpec: LoopSpec | null;
  last: LoopSample | null;
  text: string[];
  startedAt: number;
  endedAt: number;
  /** Bumped when `samples` changes; the scope redraws on it. */
  version: number;
  /** What is on the scope: a loop run, or a saved trace replayed. */
  shows: 'run' | 'replay';
  /** The replayed file and its length. */
  replayed: { path: string; n: number } | null;
  /** Per-signal trace_diff rows of the last comparison. */
  diffs: DiffRow[];
  /** Samples the last run's trace_save wrote (null: nothing recorded). */
  saved: number | null;
  /** The interpreter still holds the last run's log_* vectors. */
  canRecheck: boolean;
  setSpec: (patch: Partial<LoopSpec>) => void;
  replaceSpec: (spec: LoopSpec) => void;
}

const samples: LoopSample[] = [];
/** The reference trace, drawn faintly behind the run. */
const refSamples: LoopSample[] = [];
let pending = '';

const useLoop = create<LoopStore>((set) => ({
  spec: DEFAULT_LOOP,
  state: 'idle',
  error: '',
  stats: null,
  ranSpec: null,
  last: null,
  text: [],
  startedAt: 0,
  endedAt: 0,
  version: 0,
  shows: 'run',
  replayed: null,
  diffs: [],
  saved: null,
  canRecheck: false,
  setSpec: (patch) => set((s) => ({ spec: { ...s.spec, ...patch } })),
  replaceSpec: (spec) => set({ spec }),
}));

let flushQueued = false;
let queuedText: string[] = [];
function ingest(chunk: string) {
  const { lines, rest } = splitChunk(pending, chunk);
  pending = rest;
  let last: LoopSample | null = null;
  let stats: LoopStats | null = null;
  const diffs: DiffRow[] = [];
  let saved: number | null = null;
  let replayed: number | null = null;
  for (const line of lines) {
    const p = parseLoopLine(line);
    if (p.kind === 'sample') {
      samples.push(p.sample);
      last = p.sample;
    } else if (p.kind === 'ref') {
      refSamples.push(p.sample);
    } else if (p.kind === 'stats') {
      stats = p.stats;
    } else if (p.kind === 'diff') {
      diffs.push(p.row);
    } else if (p.kind === 'saved') {
      saved = p.n;
    } else if (p.kind === 'replayed') {
      replayed = p.n;
    } else {
      queuedText.push(p.text);
    }
  }
  if (samples.length > MAX_SAMPLES) samples.splice(0, samples.length - MAX_SAMPLES);
  if (last || stats || diffs.length || saved != null || replayed != null) {
    useLoop.setState((s) => ({
      last: last ?? s.last,
      stats: stats ?? s.stats,
      diffs: diffs.length ? s.diffs.filter((d) => !diffs.some((x) => x.name === d.name)).concat(diffs) : s.diffs,
      saved: saved ?? s.saved,
      replayed: replayed != null && s.replayed ? { ...s.replayed, n: replayed } : s.replayed,
    }));
  }
  // One React update per 50 ms, however fast lines arrive. (A timer, not
  // requestAnimationFrame: that one pauses while the window is hidden.)
  if (!flushQueued) {
    flushQueued = true;
    window.setTimeout(() => {
      flushQueued = false;
      const add = queuedText;
      queuedText = [];
      useLoop.setState((s) => ({ version: s.version + 1, text: add.length ? s.text.concat(add).slice(-MAX_TEXT) : s.text }));
    }, 50);
  }
}

let listening = false;
async function ensureListener() {
  if (listening || typeof (window as any).__TAURI_IPC__ === 'undefined') return;
  listening = true;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<{ session: string; text: string }>('qu-run-output', (e) => {
      if (e.payload?.session === LOOP_SESSION) ingest(String(e.payload.text ?? ''));
    });
  } catch (err) {
    listening = false;
    console.error('Loop panel: output listener failed', err);
  }
}

// ---------------------------------------------------------------- scope

type WindowChoice = 'all' | 1 | 5 | 10;

const LoopScope: React.FC<{ watch: SignalName[]; duration: number; windowS: WindowChoice; frozen: LoopSample[] | null; version: number }> = ({ watch, duration, windowS, frozen, version }) => {
  const box = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ w: 640, h: 360 });
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: Math.max(240, el.clientWidth), h: Math.max(200, el.clientHeight) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const data = frozen ?? samples;
  const view = useMemo(() => {
    const tEnd = data.length ? data[data.length - 1].t : 0;
    const refEnd = refSamples.length ? refSamples[refSamples.length - 1].t : 0;
    const x0 = windowS === 'all' ? 0 : Math.max(0, tEnd - windowS);
    const x1 = windowS === 'all' ? Math.max(duration, tEnd, refEnd) : Math.max(x0 + windowS, tEnd);
    // Samples are in time order: binary-search the window's start.
    const from = (arr: LoopSample[]) => {
      let lo = 0;
      let hi = arr.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid].t < x0) lo = mid + 1;
        else hi = mid;
      }
      let end = lo;
      while (end < arr.length && arr[end].t <= x1) end++;
      return arr.slice(lo, end);
    };
    const vis = from(data);
    const ref = from(refSamples);
    function* vals() {
      for (const arr of [vis, ref]) for (const s of arr) for (const id of watch) {
        const v = s.values[id];
        if (v != null) yield v;
      }
    }
    const [y0, y1] = autoRange(vals());
    return { vis, ref, x0, x1, y0, y1 };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, version, windowS, duration, watch]);

  const m = { l: 56, r: 14, t: 12, b: 34 };
  const W = size.w;
  const H = size.h;
  const px = (t: number) => m.l + ((t - view.x0) / (view.x1 - view.x0 || 1)) * (W - m.l - m.r);
  const py = (v: number) => m.t + ((view.y1 - v) / (view.y1 - view.y0 || 1)) * (H - m.t - m.b);
  // At most ~2 points per pixel column: a 200 000-sample buffer still draws fast.
  const pathOf = (arr: LoopSample[], id: SignalName) => {
    const stride = Math.max(1, Math.floor(arr.length / ((W - m.l - m.r) * 2)));
    let d = '';
    let pen = false;
    for (let i = 0; i < arr.length; i += stride) {
      const s = arr[i];
      const v = s.values[id];
      if (v == null || !Number.isFinite(v)) {
        pen = false;
        continue;
      }
      d += `${pen ? 'L' : 'M'}${px(s.t).toFixed(1)},${py(v).toFixed(1)}`;
      pen = true;
    }
    return d;
  };
  const paths = watch.map((id) => ({ id, d: pathOf(view.vis, id) }));
  const refPaths = view.ref.length ? watch.filter((id) => id !== 'r').map((id) => ({ id, d: pathOf(view.ref, id) })) : [];
  const xt = niceTicks(view.x0, view.x1, Math.max(2, Math.floor((W - m.l - m.r) / 90)));
  const yt = niceTicks(view.y0, view.y1, Math.max(2, Math.floor((H - m.t - m.b) / 50)));
  const color = (id: SignalName) => SIGNALS.find((s) => s.id === id)?.color ?? 'currentColor';

  return (
    <div ref={box} className="flex-1 min-h-0 relative" data-testid="loop-scope" data-samples={view.vis.length}>
      <svg width={W} height={H} style={{ position: 'absolute', inset: 0, color: 'var(--qu-text)' }} role="img" aria-label="Live scope of the loop signals">
        {yt.map((v) => (
          <g key={`y${v}`}>
            <line x1={m.l} x2={W - m.r} y1={py(v)} y2={py(v)} stroke="var(--qu-border)" strokeWidth={v === 0 ? 1 : 0.5} />
            <text x={m.l - 6} y={py(v) + 4} textAnchor="end" fontSize={11} fill="var(--qu-muted)">{v}</text>
          </g>
        ))}
        {xt.map((v) => (
          <g key={`x${v}`}>
            <line x1={px(v)} x2={px(v)} y1={m.t} y2={H - m.b} stroke="var(--qu-border)" strokeWidth={0.5} />
            <text x={px(v)} y={H - m.b + 15} textAnchor="middle" fontSize={11} fill="var(--qu-muted)">{v}</text>
          </g>
        ))}
        <text x={(m.l + W - m.r) / 2} y={H - 4} textAnchor="middle" fontSize={11} fill="var(--qu-muted)">simulated time t (s)</text>
        {refPaths.map((p) => (
          <path key={`ref-${p.id}`} d={p.d} fill="none" stroke={color(p.id)} strokeOpacity={0.45} strokeWidth={4} data-ref={p.id} />
        ))}
        {paths.map((p) => (
          <path key={p.id} d={p.d} fill="none" stroke={color(p.id)} strokeWidth={p.id === 'r' ? 1.25 : 1.75} strokeDasharray={p.id === 'r' ? '5 3' : undefined} />
        ))}
      </svg>
      {data.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center text-sm" style={{ color: 'var(--qu-muted)' }}>
          Press Run: the chosen signals are drawn here as the loop runs.
        </div>
      )}
    </div>
  );
};

// ---------------------------------------------------------------- compare

const CompareTable: React.FC<{
  spec: LoopSpec;
  diffs: DiffRow[];
  onTol: (id: SignalName, patch: Partial<{ rtol: number; atol: number }>) => void;
  canRecheck: boolean;
  onRecheck: () => void;
}> = ({ spec, diffs, onTol, canRecheck, onRecheck }) => {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const overall = compareVerdict(diffs);
  const tolInput = (id: SignalName, which: 'rtol' | 'atol') => {
    const key = `${id}.${which}`;
    return (
      <input
        value={drafts[key] ?? fmtTol(spec.reference.tol[id][which])}
        onChange={(e) => {
          const raw = e.target.value;
          setDrafts((d) => ({ ...d, [key]: raw }));
          const v = Number(raw);
          if (raw.trim() !== '' && Number.isFinite(v) && v >= 0) onTol(id, { [which]: v });
        }}
        onBlur={() => setDrafts((d) => {
          const { [key]: _, ...rest } = d;
          return rest;
        })}
        className="w-20 px-1 py-0.5 rounded border bg-transparent font-mono text-[11px]"
        style={{ borderColor: 'var(--qu-border)' }}
        aria-label={`${which} ${id}`}
      />
    );
  };
  return (
    <div className="border-t px-3 py-2 text-xs" style={{ borderColor: 'var(--qu-border)' }} data-testid="loop-compare">
      <div className="flex items-center gap-3 mb-1">
        <span className="font-medium text-sm">Reference check</span>
        <span className="font-mono truncate" style={{ color: 'var(--qu-muted)' }} title={spec.reference.path}>{baseName(spec.reference.path) || 'no file'}</span>
        <span className="flex-1" />
        {overall && (
          <span className="font-medium" style={{ color: overall.pass ? 'var(--qu-ok)' : 'var(--qu-danger)' }} data-testid="loop-compare-verdict">{overall.text}</span>
        )}
        <button className="px-2 py-0.5 rounded border" style={{ borderColor: 'var(--qu-border)' }} disabled={!canRecheck} onClick={onRecheck} title="Compare the last run again with these tolerances (no re-run)">
          Check again
        </button>
      </div>
      <table className="w-full" style={{ borderCollapse: 'collapse' }}>
        <thead>
          <tr style={{ color: 'var(--qu-muted)' }} className="text-left">
            <th className="font-normal py-1">signal</th>
            <th className="font-normal">result</th>
            <th className="font-normal">max |a - b|</th>
            <th className="font-normal">max rel</th>
            <th className="font-normal">detail</th>
            <th className="font-normal">rtol</th>
            <th className="font-normal">atol</th>
          </tr>
        </thead>
        <tbody>
          {TRACE_SIGNALS.map((id) => {
            const r = diffs.find((d) => d.name === id);
            return (
              <tr key={id} style={{ borderTop: '1px solid var(--qu-border)' }} data-signal={id}>
                <td className="py-1 font-mono">{id}</td>
                <td>
                  {r ? (
                    <span className="font-medium" style={{ color: r.ok ? 'var(--qu-ok)' : 'var(--qu-danger)' }}>{r.ok ? 'pass' : 'FAIL'}</span>
                  ) : (
                    <span style={{ color: 'var(--qu-muted)' }}>-</span>
                  )}
                </td>
                <td className="font-mono">{r?.maxAbs != null ? fmtNum(r.maxAbs) : ''}</td>
                <td className="font-mono">{r?.maxRel != null ? fmtNum(r.maxRel) : ''}</td>
                <td style={{ color: 'var(--qu-muted)' }}>{r ? rowReason(r) : 'run to compare'}</td>
                <td>{tolInput(id, 'rtol')}</td>
                <td>{tolInput(id, 'atol')}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};

// ---------------------------------------------------------------- panel

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border min-w-0';

const fmtNum = (v: number | null | undefined, digits = 4): string =>
  v == null || !Number.isFinite(v) ? 'n/a' : String(Number(v.toPrecision(digits)));
const baseName = (p: string): string => p.split(/[\\/]/).pop() ?? p;
/** 1e-6 rather than 0.000001. */
const fmtTol = (v: number): string => (v !== 0 && Math.abs(v) < 1e-3 ? v.toExponential() : String(v));

/** Where a recording goes when none was chosen: the app's data folder. */
async function defaultTracePath(): Promise<string> {
  try {
    const { appDataDir, join } = await import('@tauri-apps/api/path');
    const { createDir } = await import('@tauri-apps/api/fs');
    const dir = await join(await appDataDir(), 'loop-traces');
    await createDir(dir, { recursive: true }).catch(() => undefined);
    return await join(dir, 'run.csv');
  } catch {
    return '';
  }
}

async function pickSavePath(current: string): Promise<string | null> {
  try {
    const { save } = await import('@tauri-apps/api/dialog');
    const r = await save({ defaultPath: current || undefined, filters: [{ name: 'Trace', extensions: ['csv', 'npz'] }] });
    return typeof r === 'string' ? r : null;
  } catch {
    return null;
  }
}

async function pickFile(filters: Array<{ name: string; extensions: string[] }>): Promise<string | null> {
  try {
    const { open } = await import('@tauri-apps/api/dialog');
    const r = await open({ multiple: false, filters });
    return typeof r === 'string' ? r : null;
  } catch {
    return null;
  }
}

export const LoopPanel: React.FC<LoopPanelProps> = ({ theme, invoke, editorFile, onOpenInEditor }) => {
  const spec = useLoop((s) => s.spec);
  const setSpec = useLoop((s) => s.setSpec);
  const state = useLoop((s) => s.state);
  const error = useLoop((s) => s.error);
  const stats = useLoop((s) => s.stats);
  const ranSpec = useLoop((s) => s.ranSpec);
  const last = useLoop((s) => s.last);
  const text = useLoop((s) => s.text);
  const version = useLoop((s) => s.version);
  const startedAt = useLoop((s) => s.startedAt);
  const endedAt = useLoop((s) => s.endedAt);
  const shows = useLoop((s) => s.shows);
  const replayed = useLoop((s) => s.replayed);
  const diffs = useLoop((s) => s.diffs);
  const saved = useLoop((s) => s.saved);
  const canRecheck = useLoop((s) => s.canRecheck);
  const [windowS, setWindowS] = useState<WindowChoice>('all');
  const [frozen, setFrozen] = useState<LoopSample[] | null>(null);
  const [copied, setCopied] = useState('');
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [now, setNow] = useState(() => performance.now());
  const [ports, setPorts] = useState<string[] | null>(null);
  const [portsNote, setPortsNote] = useState('');

  useEffect(() => {
    void ensureListener();
  }, []);
  // The wall-clock readout ticks while a run is in flight.
  useEffect(() => {
    if (state !== 'running') return;
    const id = window.setInterval(() => setNow(performance.now()), 100);
    return () => window.clearInterval(id);
  }, [state]);

  const editorOk = !!editorFile && editorFile.code.trim().length > 0;
  const problem = useMemo(() => validateLoop(spec, { plant: editorOk, controller: editorOk }), [spec, editorOk]);
  const sources: ResolvedSources = useMemo(
    () => ({
      plantEditor: spec.plant.kind === 'editor' && editorFile ? editorFile : undefined,
      controllerEditor: spec.controller.kind === 'editor' && editorFile ? editorFile : undefined,
    }),
    [spec.plant.kind, spec.controller.kind, editorFile],
  );
  const script = useMemo(() => (problem ? '' : buildLoopScript(spec, sources)), [spec, sources, problem]);
  const running = state === 'running';

  /** Sends `code` to the loop's interpreter and follows it to the end.
   *  `fresh`: restart the interpreter first (a run or a replay); a
   *  re-check keeps it, since it reads the last run's log_* vectors. */
  const execute = async (code: string, fresh: boolean, file: string): Promise<boolean> => {
    pending = '';
    queuedText = [];
    try {
      if (fresh) await invoke('repl_restart', { session: LOOP_SESSION });
      const r = await invoke<ExecuteShape>('repl_run', { code, session: LOOP_SESSION, cwd: null, file });
      // Output events can trail the response by a moment.
      await new Promise((res) => setTimeout(res, 60));
      if (pending) ingest('\n');
      const stopped = !r.success && /^Stopped\./.test(r.error ?? '');
      useLoop.setState({ state: r.success ? 'done' : stopped ? 'stopped' : 'failed', error: r.success || stopped ? '' : r.error ?? 'The script did not run', endedAt: performance.now() });
      return r.success;
    } catch (err) {
      useLoop.setState({ state: 'failed', error: String(err), endedAt: performance.now() });
      return false;
    }
  };
  const clearScope = () => {
    samples.length = 0;
    refSamples.length = 0;
    setFrozen(null);
  };
  const run = async () => {
    if (problem || running) return;
    clearScope();
    useLoop.setState({ state: 'running', shows: 'run', replayed: null, error: '', stats: null, last: null, text: [], diffs: [], saved: null, canRecheck: false, ranSpec: spec, startedAt: performance.now(), endedAt: 0, version: useLoop.getState().version + 1 });
    const ok = await execute(script, true, 'loop.qu');
    useLoop.setState({ canRecheck: ok });
  };
  const replay = async (path: string) => {
    if (running || tracePathProblem('Replay', path)) return;
    clearScope();
    const code = buildReplayScript(path);
    // Echo what runs: the replay script is not the one in the script box.
    const echo = ['# ran this replay script:', ...code.trimEnd().split('\n')];
    useLoop.setState({ state: 'running', shows: 'replay', replayed: { path, n: 0 }, error: '', stats: null, last: null, text: echo, diffs: [], saved: null, canRecheck: false, ranSpec: spec, startedAt: performance.now(), endedAt: 0, version: useLoop.getState().version + 1 });
    await execute(code, true, 'loop_replay.qu');
  };
  const recheck = async () => {
    if (running || problem || !useLoop.getState().canRecheck) return;
    refSamples.length = 0;
    useLoop.setState({ state: 'running', diffs: [], error: '' });
    await execute(buildCompareScript(spec), false, 'loop_compare.qu');
  };
  const stop = async () => {
    try {
      await invoke('repl_stop', { session: LOOP_SESSION });
    } catch (err) {
      useLoop.setState({ error: `Could not stop the loop: ${err}` });
    }
  };

  const copy = (what: string, value: string) => {
    void navigator.clipboard?.writeText(value).then(() => {
      setCopied(what);
      setTimeout(() => setCopied(''), 1200);
    });
  };

  // -------- controls
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
  const numInput = (key: string, value: number, onValue: (v: number) => void, extra: React.InputHTMLAttributes<HTMLInputElement> = {}) => (
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

  /** serial_ports(), run once in a throwaway interpreter. */
  const listPorts = async () => {
    setPortsNote('Listing…');
    try {
      const r = await invoke<ExecuteShape & { output: string }>('execute_code', { request: { code: PORT_LIST_SCRIPT, file_path: null } });
      const list = r.success ? parsePortList(r.output ?? '') : null;
      if (list == null) {
        setPorts(null);
        setPortsNote(r.error ?? 'serial_ports() did not run');
        return;
      }
      setPorts(list);
      setPortsNote(list.length ? `${list.length} port${list.length > 1 ? 's' : ''} reported by the OS` : 'The OS reports no serial ports.');
      if (list.length && !spec.io.port) setSpec({ io: { ...spec.io, port: list[0] } });
    } catch (err) {
      setPortsNote(String(err));
    }
  };
  const io = spec.io;
  const setIo = (patch: Partial<IoSpec>) => setSpec({ io: { ...io, ...patch } });
  const ioSection = (
    <div className="flex flex-col gap-2" aria-label="I/O">
      <span className="text-sm font-medium">I/O (real hardware)</span>
      {select(io.kind, (k) => setIo({ kind: k as IoSpec['kind'] }), [
        { id: 'none', label: 'None: the plant script is the plant' },
        { id: 'serial', label: 'Serial port' },
        { id: 'tcp', label: 'TCP address' },
      ], 'io kind')}
      {io.kind === 'serial' && (
        <>
          <div className="flex gap-1 items-end">
            {field('Port', ports && ports.length ? select(io.port, (p) => setIo({ port: p }), (io.port && !ports.includes(io.port) ? [io.port, ...ports] : ports).map((p) => ({ id: p, label: p })), 'serial port')
              : <input value={io.port} onChange={(e) => setIo({ port: e.target.value })} placeholder="COM3" className={`${inputClass} font-mono text-xs`} style={{ borderColor: border }} aria-label="serial port" />)}
            <button className="px-2 py-1.5 rounded-md border text-xs" style={{ borderColor: border }} onClick={() => void listPorts()} title="serial_ports(): what the OS reports">List ports</button>
          </div>
          {portsNote && <span className="text-xs" style={{ color: 'var(--qu-muted)' }} data-testid="loop-ports">{portsNote}{ports && ports.length ? `: ${ports.join(', ')}` : ''}</span>}
          <div className="flex gap-2">
            {field('Baud', numInput('baud', io.baud, (x) => setIo({ baud: x }), { min: 1, step: 1 }))}
            {field('Reply timeout (ms)', numInput('iotimeout', io.timeoutMs, (x) => setIo({ timeoutMs: x }), { min: 1 }))}
          </div>
        </>
      )}
      {io.kind === 'tcp' && (
        <div className="flex gap-2">
          {field('Host', <input value={io.host} onChange={(e) => setIo({ host: e.target.value })} className={`${inputClass} font-mono text-xs`} style={{ borderColor: border }} aria-label="tcp host" />)}
          {field('Port', numInput('tcpport', io.tcpPort, (x) => setIo({ tcpPort: x }), { min: 1, max: 65535, step: 1 }))}
        </div>
      )}
      {io.kind !== 'none' && (
        <div className="qu-banner" data-kind="warning" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12, padding: '6px 9px' }}>
          <AlertCircle size={13} className="flex-shrink-0 mt-0.5" />
          <span>
            Needs real hardware (or a stand-in) on the other end. Each step sends u as one text line and waits for one line back with the measured y; the plant script is not used.
          </span>
        </div>
      )}
    </div>
  );

  const sourceEditor = (role: 'plant' | 'controller') => {
    const s = spec[role];
    if (role === 'plant' && usesIo(spec)) {
      return (
        <div className="flex flex-col gap-1">
          <span className="text-sm font-medium">Plant</span>
          <span className="text-xs" style={{ color: 'var(--qu-warning)' }}>
            Real hardware on {io.kind === 'serial' ? `serial ${io.port || '(no port yet)'}` : `TCP ${io.host}:${io.tcpPort}`} (see I/O below).
          </span>
        </div>
      );
    }
    const upd = (patch: Partial<SourceSpec>) => setSpec({ [role]: { ...s, ...patch } } as Partial<LoopSpec>);
    const kinds = [
      { id: 'example', label: 'Example' },
      { id: 'file', label: '.qu file' },
      { id: 'editor', label: editorFile ? `Active editor file (${editorFile.name})` : 'Active editor file' },
      ...(role === 'controller' ? [{ id: 'clib', label: 'Compiled C library' }] : []),
    ];
    const examples = role === 'plant' ? PLANT_EXAMPLES : CONTROLLER_EXAMPLES;
    return (
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">{role === 'plant' ? 'Plant' : 'Controller'}</span>
          <code className="text-[11px]" style={{ color: 'var(--qu-muted)' }}>{role === 'plant' ? 'plant(u, dt) -> y' : 'controller(r, y, dt) -> u'}</code>
        </div>
        {select(s.kind, (v) => upd({ kind: v as SourceSpec['kind'] }), kinds, `${role} source`)}
        {s.kind === 'example' && select(s.example, (v) => upd({ example: v }), examples, `${role} example`)}
        {(s.kind === 'file' || s.kind === 'clib') && (
          <div className="flex gap-1">
            <input
              value={s.path}
              onChange={(e) => upd({ path: e.target.value })}
              placeholder={s.kind === 'file' ? 'C:/path/plant.qu' : 'C:/path/controller.dll'}
              className={`${inputClass} flex-1 font-mono text-xs`}
              style={{ borderColor: border }}
              aria-label={`${role} path`}
            />
            <button
              className="px-2 rounded-md border"
              style={{ borderColor: border }}
              title="Browse"
              onClick={async () => {
                const p = await pickFile(s.kind === 'file' ? [{ name: 'Qu script', extensions: ['qu'] }] : [{ name: 'Shared library', extensions: ['dll', 'so', 'dylib'] }]);
                if (p) upd({ path: p });
              }}
            >
              <FolderOpen size={13} />
            </button>
          </div>
        )}
        {s.kind === 'clib' && (
          <div className="flex gap-2 items-end">
            {field('C function: double f(double r, double y, double dt)', (
              <input value={s.func} onChange={(e) => upd({ func: e.target.value })} className={`${inputClass} font-mono text-xs`} style={{ borderColor: border }} aria-label="C function name" />
            ))}
            <button className="text-xs underline pb-1.5" style={{ color: 'var(--qu-accent)' }} onClick={() => copy('c', C_CONTROLLER_TEMPLATE)} title="Copy a C controller that fits this slot">
              {copied === 'c' ? 'copied' : 'C template'}
            </button>
          </div>
        )}
        {s.kind === 'editor' && !editorOk && (
          <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>Open a Qu file in the Code tab first.</span>
        )}
      </div>
    );
  };

  // -------- status
  const wallMs = stats?.wallMs ?? (startedAt ? (endedAt || now) - startedAt : 0);
  const shownSpec = ranSpec ?? spec;
  const v = shows === 'replay'
    ? { kind: 'idle' as const, text: 'replay', detail: replayed ? `${replayed.n || '…'} samples from ${baseName(replayed.path)}, drawn as saved (nothing simulated)` : '' }
    : verdict(shownSpec, last ? { simT: last.t, wallMs, over: last.over, lmax: last.lmax, steps: last.k } : null, stats);
  const verdictColor = v.kind === 'ok' ? 'var(--qu-ok)' : v.kind === 'late' ? 'var(--qu-warning)' : 'var(--qu-text)';
  const stateLabel: Record<RunState, string> = { idle: 'Idle', running: 'Running', done: 'Done', stopped: 'Stopped', failed: 'Failed' };
  const hilShown = shownSpec.mode === 'hil' && shows === 'run';
  const rows: Array<[string, string, string?]> = shows === 'replay' ? [['State', stateLabel[state]], ['Samples', replayed?.n ? String(replayed.n) : 'n/a']] : [
    ['State', stateLabel[state]],
    ['Mode', hilShown ? `HIL, paced x${shownSpec.pace}` : 'SIL, virtual clock'],
    ['Simulated time', last ? `${fmtNum(last.t)} / ${shownSpec.duration} s` : 'n/a'],
    ['Wall time', startedAt ? `${fmtNum(wallMs / 1000, 3)} s` : 'n/a'],
    ['Steps', stats ? String(stats.steps) : last ? String(last.k) : 'n/a'],
  ];
  if (saved != null && shows === 'run') rows.push(['Recorded', `${saved} samples`, ranSpec?.record.path]);
  if (hilShown) {
    rows.push(
      ['Overruns', stats ? String(stats.overruns) : last?.over != null ? `${last.over} (live)` : 'n/a', `Steps that started more than ${shownSpec.toleranceMs} ms after their deadline`],
      ['Max lateness', stats ? `${fmtNum(stats.maxMs)} ms` : last?.lmax != null ? `${fmtNum(last.lmax)} ms (live)` : 'n/a'],
      ['Mean lateness', stats ? `${fmtNum(stats.meanMs)} ms` : 'at the end'],
    );
  }

  const scopeWatch = (ranSpec ?? spec).watch;

  return (
    <div className="qu-inspector flex flex-1 min-w-0 min-h-0" data-theme={theme} data-testid="loop-panel">
      {/* ---------------- left: setup */}
      <div className="w-[21rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium" title="Software- and hardware-in-the-loop">Loop</span>
          <select
            value=""
            onChange={(e) => {
              const ex = WORKED_EXAMPLES.find((x) => x.id === e.target.value);
              if (ex) {
                setDrafts({});
                useLoop.getState().replaceSpec(ex.spec);
              }
            }}
            className="px-2 py-1 rounded-md text-xs bg-transparent border"
            style={{ borderColor: border }}
            aria-label="Worked examples"
          >
            <option value="" disabled>Worked examples…</option>
            {WORKED_EXAMPLES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
          </select>
        </div>
        <EnvironmentPicker
          plantId={spec.plant.kind === 'example' ? spec.plant.example : ''}
          controllerId={spec.controller.kind === 'example' ? spec.controller.example : ''}
          onPick={(id) => {
            const next = environmentSpec(id);
            if (next) {
              setDrafts({});
              // Keep the run mode and I/O the user chose; the environment sets the rest.
              useLoop.getState().replaceSpec({ ...next, mode: spec.mode, pace: spec.pace, toleranceMs: spec.toleranceMs, io: spec.io, record: spec.record, reference: spec.reference });
            }
          }}
        />
        {sourceEditor('plant')}
        {sourceEditor('controller')}
        <PluginsSection plugins={spec.plugins ?? []} onChange={(plugins) => setSpec({ plugins })} invoke={invoke} />
        <div className="flex flex-col gap-2">
          <span className="text-sm font-medium">Run</span>
          <div className="qu-live-tabs" role="tablist" aria-label="Mode" style={{ borderRadius: 8 }}>
            <button role="tab" aria-selected={spec.mode === 'sil'} onClick={() => setSpec({ mode: 'sil' })} title="Virtual clock: as fast as the machine can go">SIL</button>
            <button role="tab" aria-selected={spec.mode === 'hil'} onClick={() => setSpec({ mode: 'hil' })} title="Paced against the wall clock">HIL (real time)</button>
          </div>
          <div className="flex gap-2">
            {field('Duration (s)', numInput('duration', spec.duration, (x) => setSpec({ duration: x }), { min: 0, step: 0.5 }))}
            {field('Step dt (s)', numInput('dt', spec.dt, (x) => setSpec({ dt: x }), { min: 0, step: 0.001 }), 'The controller period')}
          </div>
          {spec.mode === 'hil' && (
            <div className="flex gap-2">
              {field('Real-time factor', numInput('pace', spec.pace, (x) => setSpec({ pace: x }), { min: 0, step: 0.5 }), '1 = real time, 2 = twice as fast')}
              {field('Late after (ms)', numInput('tolerance', spec.toleranceMs, (x) => setSpec({ toleranceMs: x }), { min: 0, step: 0.5 }), 'A step that starts later than this counts as an overrun')}
            </div>
          )}
          <div className="flex gap-2">
            {field('Setpoint', select(spec.setpoint.kind, (k) => setSpec({ setpoint: { ...spec.setpoint, kind: k as LoopSpec['setpoint']['kind'], time: k === 'step' ? 0.5 : 2 } }), [
              { id: 'step', label: 'Step' },
              { id: 'square', label: 'Square wave' },
              { id: 'sine', label: 'Sine' },
            ], 'setpoint kind'))}
            {field('Amplitude', numInput('amplitude', spec.setpoint.amplitude, (x) => setSpec({ setpoint: { ...spec.setpoint, amplitude: x } }), { step: 0.1 }))}
            {field(spec.setpoint.kind === 'step' ? 'At (s)' : 'Period (s)', numInput('sptime', spec.setpoint.time, (x) => setSpec({ setpoint: { ...spec.setpoint, time: x } }), { min: 0, step: 0.1 }))}
          </div>
          <div className="flex flex-col gap-1 text-xs">
            <span style={{ color: 'var(--qu-muted)' }}>Watch on the scope</span>
            <div className="flex gap-3 flex-wrap">
              {SIGNALS.map((sg) => (
                <label key={sg.id} className="flex items-center gap-1" title={sg.label}>
                  <input
                    type="checkbox"
                    checked={spec.watch.includes(sg.id)}
                    onChange={(e) => setSpec({ watch: e.target.checked ? [...spec.watch, sg.id] : spec.watch.filter((w) => w !== sg.id) })}
                  />
                  <span className="font-mono" style={{ color: sg.color }}>{sg.id}</span>
                </label>
              ))}
            </div>
          </div>
        </div>
        {spec.mode === 'hil' && ioSection}
        <div className="flex flex-col gap-2" aria-label="Trace">
          <span className="text-sm font-medium">Trace</span>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={spec.record.enabled}
              aria-label="Record"
              onChange={async (e) => {
                const enabled = e.target.checked;
                const path = spec.record.path || (enabled ? await defaultTracePath() : '');
                setSpec({ record: { enabled, path } });
              }}
            />
            Record this run (trace_save: t, r, y, u, e)
          </label>
          {spec.record.enabled && (
            <div className="flex gap-1">
              <input value={spec.record.path} onChange={(e) => setSpec({ record: { ...spec.record, path: e.target.value } })} placeholder="C:/path/run.csv" className={`${inputClass} flex-1 font-mono text-xs`} style={{ borderColor: border }} aria-label="record path" />
              <button className="px-2 rounded-md border" style={{ borderColor: border }} title="Save as…" onClick={async () => {
                const p = await pickSavePath(spec.record.path);
                if (p) setSpec({ record: { ...spec.record, path: p } });
              }}>
                <FolderOpen size={13} />
              </button>
              <button className="px-2 rounded-md border text-xs" style={{ borderColor: border }} disabled={running || !!tracePathProblem('Replay', spec.record.path)} title="Draw this saved trace on the scope (nothing is simulated)" onClick={() => void replay(spec.record.path)}>
                Replay
              </button>
            </div>
          )}
          <label className="flex items-center gap-2 text-xs">
            <input type="checkbox" checked={spec.reference.enabled} aria-label="Compare" onChange={(e) => setSpec({ reference: { ...spec.reference, enabled: e.target.checked } })} />
            Compare with a reference trace (trace_diff per signal)
          </label>
          {spec.reference.enabled && (
            <>
              <div className="flex gap-1">
                <input value={spec.reference.path} onChange={(e) => setSpec({ reference: { ...spec.reference, path: e.target.value } })} placeholder="C:/path/reference.csv" className={`${inputClass} flex-1 font-mono text-xs`} style={{ borderColor: border }} aria-label="reference path" />
                <button className="px-2 rounded-md border" style={{ borderColor: border }} title="Load reference…" onClick={async () => {
                  const p = await pickFile([{ name: 'Trace', extensions: ['csv', 'npz'] }]);
                  if (p) setSpec({ reference: { ...spec.reference, path: p } });
                }}>
                  <FolderOpen size={13} />
                </button>
                <button className="px-2 rounded-md border text-xs" style={{ borderColor: border }} disabled={running || !!tracePathProblem('Replay', spec.reference.path)} title="Draw the reference on the scope" onClick={() => void replay(spec.reference.path)}>
                  Replay
                </button>
              </div>
              {spec.record.path && spec.record.path !== spec.reference.path && (
                <button className="text-xs underline self-start" style={{ color: 'var(--qu-accent)' }} onClick={() => setSpec({ reference: { ...spec.reference, path: spec.record.path }, record: { ...spec.record, enabled: false } })} title="Compare the next runs against the last recording (and stop overwriting it)">
                  Use the recording as the reference
                </button>
              )}
            </>
          )}
        </div>
        {running ? (
          <button className="qu-side-action w-full" style={{ background: 'var(--qu-danger)' }} onClick={() => void stop()}>
            <Square size={13} /> Stop
          </button>
        ) : (
          <button className="qu-side-action w-full" disabled={!!problem} onClick={() => void run()} title={problem ?? 'Generate the script and run it'}>
            <Play size={13} /> Run {spec.mode === 'hil' ? 'HIL' : 'SIL'}
          </button>
        )}
        {(problem || error) && (
          <div className="qu-banner" data-kind="error" role="alert" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words whitespace-pre-wrap">{problem ?? error}</span>
          </div>
        )}
      </div>

      {/* ---------------- centre: scope */}
      <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <div className="flex items-center gap-2 px-3 py-2 border-b text-xs" style={{ borderColor: border }}>
          <span className="font-medium text-sm mr-2">Scope</span>
          {scopeWatch.map((id) => {
            const sg = SIGNALS.find((s) => s.id === id)!;
            return (
              <span key={id} className="flex items-center gap-1 whitespace-nowrap" style={{ color: 'var(--qu-muted)' }} title={sg.label}>
                <span style={{ display: 'inline-block', width: 14, height: 2, background: sg.color }} />
                <span className="font-mono" style={{ color: 'var(--qu-text)' }}>{sg.id}</span> {sg.short}
              </span>
            );
          })}
          {spec.reference.enabled && shows === 'run' && (
            <span className="flex items-center gap-1 whitespace-nowrap" style={{ color: 'var(--qu-muted)' }} title="The reference trace, drawn behind the run">
              <span style={{ display: 'inline-block', width: 14, height: 5, background: 'var(--qu-muted)', opacity: 0.45 }} />
              reference
            </span>
          )}
          <span className="flex-1" />
          <label className="flex items-center gap-1" style={{ color: 'var(--qu-muted)' }}>
            Window
            <select value={String(windowS)} onChange={(e) => setWindowS(e.target.value === 'all' ? 'all' : (Number(e.target.value) as WindowChoice))} className="px-1 py-0.5 rounded border bg-transparent" style={{ borderColor: border }} aria-label="Scope window">
              <option value="all">whole run</option>
              <option value="1">last 1 s</option>
              <option value="5">last 5 s</option>
              <option value="10">last 10 s</option>
            </select>
          </label>
          <button className="flex items-center gap-1 px-2 py-0.5 rounded border" style={{ borderColor: border }} onClick={() => setFrozen(frozen ? null : samples.slice())} aria-pressed={!!frozen} title="Freeze the picture (the run keeps going)">
            {frozen ? <Play size={12} /> : <Pause size={12} />} {frozen ? 'Resume' : 'Pause'}
          </button>
          <button
            className="flex items-center gap-1 px-2 py-0.5 rounded border"
            style={{ borderColor: border }}
            onClick={() => {
              clearScope();
              useLoop.setState((s) => ({ version: s.version + 1 }));
            }}
            title="Clear the scope"
          >
            <Eraser size={12} /> Clear
          </button>
        </div>
        <LoopScope watch={scopeWatch} duration={(ranSpec ?? spec).duration} windowS={windowS} frozen={frozen} version={version} />
        {spec.reference.enabled && (
          <CompareTable
            spec={spec}
            diffs={diffs}
            onTol={(id, patch) => setSpec({ reference: { ...spec.reference, tol: { ...spec.reference.tol, [id]: { ...spec.reference.tol[id], ...patch } } } })}
            canRecheck={canRecheck && !running && !problem}
            onRecheck={() => void recheck()}
          />
        )}
        {text.length > 0 && (
          <div className="border-t max-h-32 overflow-auto px-3 py-2 font-mono text-[11px] whitespace-pre" style={{ borderColor: border, color: 'var(--qu-muted)' }} aria-label="Run output">
            {text.slice(-40).join('\n')}
          </div>
        )}
      </div>

      {/* ---------------- right: status + script */}
      <div className="w-[21rem] flex-shrink-0 border-l overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <div className="rounded-lg border" style={{ borderColor: border }} data-testid="loop-status">
          <div className="px-3 py-2 border-b" style={{ borderColor: border }}>
            <div className="text-[11px]" style={{ color: 'var(--qu-muted)' }}>Verdict</div>
            <div className="text-base font-medium" style={{ color: verdictColor }} data-testid="loop-verdict">{v.text}</div>
            {v.detail && <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>{v.detail}</div>}
          </div>
          {rows.map(([k, val, tip]) => (
            <div key={k} className="flex justify-between gap-3 px-3 py-1.5 text-xs" style={{ borderBottom: '1px solid var(--qu-border)' }} title={tip}>
              <span style={{ color: 'var(--qu-muted)' }}>{k}</span>
              <span className="font-mono text-right" style={{ color: k === 'Overruns' && stats && stats.overruns > 0 ? 'var(--qu-warning)' : 'var(--qu-text)' }}>{val}</span>
            </div>
          ))}
        </div>
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">Generated Qu</span>
          <div className="flex gap-1">
            <button className="p-1 rounded" style={{ color: 'var(--qu-muted)' }} title="Copy the script" disabled={!script} onClick={() => copy('script', script)}>
              <Copy size={13} />
            </button>
          </div>
        </div>
        <div className="rounded-lg p-2 text-[11px] leading-snug font-mono overflow-x-auto whitespace-pre max-h-[28rem] overflow-y-auto" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }} data-testid="loop-script">
          {script || '# Fix the setup on the left to see the script.'}
          {copied === 'script' && <span style={{ color: 'var(--qu-ok)' }}>{'\n'}copied</span>}
        </div>
        {onOpenInEditor && (
          <button className="qu-side-action w-full" disabled={!script} onClick={() => onOpenInEditor(script, `loop_${spec.mode}.qu`)} title="Open this exact script in a new editor tab">
            <FileCode2 size={13} /> Open in editor
          </button>
        )}
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          The panel only writes this script and runs it in its own Qu interpreter; every number on screen comes from it.
        </p>
      </div>
    </div>
  );
};

export default LoopPanel;
