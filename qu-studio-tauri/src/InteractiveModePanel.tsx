import React, { useState, useMemo, useRef, useEffect, useCallback } from 'react';
import { CodeEditor, PlotViewer } from '@qu/ui-components';
import type { PlotData } from '@qu/ui-components';
import { cn, assignedNamesInOrder, evalConstExpr } from '@qu/ui-components';
import { Play, AlertCircle, Box, ScatterChart, LineChart, Grid3x3, Lock, Crosshair, RefreshCw } from 'lucide-react';
import type { PlotVar } from './App';

// ============ Types ============

interface SliderDef {
  name: string;
  min: number;
  max: number;
  step: number;
  value: number;
}

interface ExecuteResponseShape {
  success: boolean;
  output: string;
  error: string | null;
  elapsed_ms: number;
  plots: string[];
  data: PlotVar[];
}

export interface InteractiveModePanelProps {
  theme: 'light' | 'dark';
  invoke: <T,>(command: string, args?: Record<string, any>) => Promise<T>;
}

// ============ Slider annotation parsing ============
//
// Convention: a `# @slider name min max step` comment line, immediately
// followed (anywhere later in the script, not necessarily the very next
// line) by a plain top-level `name = <number>` assignment. This keeps a
// script a script -- runnable as-is from the normal Code tab with no
// special syntax the interpreter has to understand -- while still letting
// Interactive Mode discover which numbers are meant to be swept, and their
// intended range, from ordinary comments.
// Bounds are captured as raw text rather than as numbers, so they can be
// small constant expressions. `# @slider phase 0 2*pi 0.01` is the obvious
// thing to write, and a digits-only pattern matched none of it -- the
// directive was dropped and the slider silently never appeared, which
// looks exactly like the feature being broken. `evalConstExpr` decides
// what is actually a valid bound.
const SLIDER_DIRECTIVE_RE =
  /^[ \t]*#\s*@slider\s+([A-Za-z_]\w*)\s+(\S+)\s+(\S+)\s+(\S+)[ \t]*$/gm;

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function assignmentRegex(name: string): RegExp {
  // Matches a top-level `name = <number>` line, capturing the prefix
  // (indentation + `name =` + inner spacing) and trailing whitespace
  // separately from the numeric literal, so a value substitution can
  // preserve everything else about the line untouched.
  return new RegExp(`^([ \\t]*${escapeRegExp(name)}[ \\t]*=[ \\t]*)-?\\d+(?:\\.\\d+)?([ \\t]*)$`, 'm');
}

function parseSliders(script: string, previous: SliderDef[]): SliderDef[] {
  const previousByName = new Map(previous.map((s) => [s.name, s.value]));
  const out: SliderDef[] = [];
  const seen = new Set<string>();
  for (const m of script.matchAll(SLIDER_DIRECTIVE_RE)) {
    const [, name, minStr, maxStr, stepStr] = m;
    if (seen.has(name)) continue; // a duplicate directive for the same name is just ignored
    const min = evalConstExpr(minStr);
    const max = evalConstExpr(maxStr);
    const step = evalConstExpr(stepStr);
    // Not three usable bounds: treat it as an ordinary comment that happens
    // to begin with @slider rather than as a broken directive. Note this
    // check sits BEFORE `seen.add`, so a malformed line does not shadow a
    // well-formed directive for the same name further down.
    if (min === null || max === null || step === null || !(min < max) || !(step > 0)) continue;
    seen.add(name);
    const assignMatch = script.match(assignmentRegex(name));
    const currentInScript = assignMatch ? parseFloat(script.match(new RegExp(`^[ \\t]*${escapeRegExp(name)}[ \\t]*=[ \\t]*(-?\\d+(?:\\.\\d+)?)`, 'm'))?.[1] ?? `${min}`) : min;
    const value = previousByName.has(name) ? previousByName.get(name)! : currentInScript;
    out.push({ name, min, max, step, value: Number.isFinite(value) ? value : min });
  }
  return out;
}

function setVarInScript(script: string, name: string, value: number): string {
  const re = assignmentRegex(name);
  if (!re.test(script)) return script;
  // format without unnecessary trailing zeros, but keep it a plain decimal
  // (never scientific notation) so it stays a valid Qu numeric literal
  const formatted = Number.isInteger(value) ? String(value) : value.toFixed(6).replace(/0+$/, '').replace(/\.$/, '');
  return script.replace(re, `$1${formatted}$2`);
}

// ============ Seed example ============
//
// Deliberately demonstrates all three sliders AND both plot modes at once
// (a 2D curve `y2` and a 3D grid `z`) from one script, so switching between
// 2D/3D/scatter3d/surface in a fresh Interactive Mode session always has
// something real to show rather than an empty axis. Verified end-to-end
// against a release `qu.exe run` before being baked in here.
const DEFAULT_SCRIPT = `# Interactive 3D surface demo -- drag the sliders to sweep freq/amp/phase.
# @slider freq 1 10 0.5
freq = 3
# @slider amp 0.1 3 0.1
amp = 1
# @slider phase 0 6.28 0.1
phase = 0

N = 40
xs = linspace(-3, 3, N)
ys = linspace(-3, 3, N)

Z = zeros(N, N)
for i = 1 to N
    for j = 1 to N
        r = sqrt(xs[i-1]^2 + ys[j-1]^2)
        Z[i-1, j-1] = amp * sin(freq * r + phase) / (r + 1)
    end for
end for

x = xs
y = ys
z = Z

# A plain 2D curve too, for 2D mode: x vs y2.
y2 = amp * sin(freq * xs + phase)
`;

const DEBOUNCE_MS = 300;

// ============ Data helpers ============

function isVectorVar(v: PlotVar): boolean {
  return v.type === 'vector' && v.data.length > 1;
}

function isMatrixVar(v: PlotVar): boolean {
  return v.type === 'matrix';
}

/** `PlotVar.data` for a matrix is qu-core's own column-major flat buffer
 * (`data[row + col*rows]`); Plotly's `surface` trace wants a row-major
 * nested `z[row][col]` grid, so this re-indexes rather than re-orders. */
function matrixToRows(v: PlotVar): (number | null)[][] {
  const [rows, cols] = v.shape;
  const out: (number | null)[][] = [];
  for (let r = 0; r < rows; r++) {
    // `null` passes straight through rather than being coerced: a
    // non-finite sample (see `PlotVar.data`) is a hole in the surface, and
    // Plotly renders null as exactly that. Forcing it to 0 would invent a
    // data point that the script never produced.
    const row: (number | null)[] = [];
    for (let c = 0; c < cols; c++) {
      row.push(v.data[r + c * rows] ?? null);
    }
    out.push(row);
  }
  return out;
}


// ============ Component ============

export const InteractiveModePanel: React.FC<InteractiveModePanelProps> = ({ theme, invoke }) => {
  const [script, setScript] = useState(DEFAULT_SCRIPT);
  const [sliders, setSliders] = useState<SliderDef[]>(() => parseSliders(DEFAULT_SCRIPT, []));
  // 'heatmap' is a third mode rather than a 2D sub-kind: it plots a
  // MATRIX, like the 3D surface does, and shares none of the 2D line
  // mode's x/y-vector machinery. A spectrogram is the case that asked for
  // it -- `spectrogram(...)` returns a frequency-by-time matrix, and
  // viewing that as a surface is a worse picture of it than the image it
  // actually is.
  const [mode, setMode] = useState<'2d' | '3d' | 'heatmap'>('3d');
  const [plotKind3d, setPlotKind3d] = useState<'scatter3d' | 'surface'>('surface');
  const [xVar, setXVar] = useState<string>('x');
  // x / y / z -- the axes are named after the axes. (`y2` here was a
  // leftover from a demo script that happened to define one.)
  const [yVar, setYVar] = useState<string>('y');
  const [zVar, setZVar] = useState<string>('z');
  const [plotVars, setPlotVars] = useState<PlotVar[]>([]);
  const [running, setRunning] = useState(false);
  /** The displayed frame is from an earlier run than the latest one, because
   *  the latest produced nothing plottable -- see the double-buffer note in
   *  `runScript`. Surfaced in the UI so a stale-but-good plot is never
   *  mistaken for a current one. */
  const [stale, setStale] = useState(false);
  /** Target locked: the code and the 2D/3D configuration are put away, the
   *  script can no longer be edited, and the sliders drive the figure in
   *  real time. Unlocking hands the code back. */
  const [locked, setLocked] = useState(false);
  /** A run is in flight; used to coalesce rather than queue (see
   *  `scheduleRun`). */
  const inFlightRef = useRef(false);
  /** The script text a pending run should use, if one arrived while the
   *  previous was still running. Only the LATEST is kept -- superseded
   *  frames are dropped, never queued. */
  const pendingRef = useRef<string | null>(null);
  /** The exact source of the last run we actually dispatched, so an
   *  identical request can be skipped outright. */
  const lastRunSrcRef = useRef<string | null>(null);
  /** Frames dropped because a newer slider value arrived mid-run --
   *  surfaced so the update rate is honest rather than invisible. */
  const [dropped, setDropped] = useState(0);
  /** How updates are paced while locked. Each frame costs a real `qu.exe`
   *  run, so this is genuinely a choice rather than a preference:
   *
   *  smart    -- no clock. Run when something changed, one at a time,
   *              newest wins. Cheapest, and what you want by default.
   *  relaxed  -- tick at `fps`; if the engine is still busy the frame is
   *              dropped and life goes on. Steady cadence, no backlog.
   *  strict   -- tick at `fps` and REPORT frames the engine could not
   *              deliver, so a target rate that isn't being met is visible
   *              instead of silently pretended.
   *  sync     -- every change runs, in order, nothing dropped. Honest and
   *              slow; useful when each frame matters more than fluidity.
   */
  const [updateMode, setUpdateMode] = useState<'smart' | 'relaxed' | 'strict' | 'sync'>('relaxed');
  const [fps, setFps] = useState<30 | 60 | 120>(60);
  /** Frames a strict timer wanted but the engine could not deliver. */
  const [lostFrames, setLostFrames] = useState(0);
  /** Latest source awaiting a timer tick (relaxed/strict only). */
  const tickPendingRef = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const runIdRef = useRef(0);

  // Re-derive sliders whenever the script text changes (typing a new
  // `# @slider` line, editing min/max/step, or a substitution from
  // dragging a slider itself all funnel through here) -- `parseSliders`
  // keeps each slider's live value if its name still exists, so a slider
  // never jumps back to its script-file default just because the script
  // was re-parsed.
  useEffect(() => {
    setSliders((prev) => parseSliders(script, prev));
  }, [script]);

  const runScript = useCallback(
    async (code: string) => {
      const thisRun = ++runIdRef.current;
      setRunning(true);
      try {
        const response = await invoke<ExecuteResponseShape>('execute_code', {
          request: { code, file_path: null },
        });
        if (thisRun !== runIdRef.current) return; // a newer run already superseded this one
        // Double-buffered: the displayed frame is only ever replaced by a
        // COMPLETE one. A run that fails, or that comes back with nothing
        // plottable (a mid-edit script that doesn't parse, a slider dragged
        // through a value the script rejects), leaves the last good frame
        // on screen and reports the problem beside it -- instead of
        // blanking the plot and redrawing it a moment later, which is what
        // made dragging a slider flicker. Same reason a double buffer
        // exists anywhere: never show a half-built frame.
        const frame = response.data ?? [];
        if (frame.length > 0) {
          setPlotVars(frame);
        }
        setError(response.error ?? null);
        setStale(frame.length === 0);
      } catch (err: any) {
        if (thisRun !== runIdRef.current) return;
        setError(err?.message ?? String(err));
      } finally {
        if (thisRun === runIdRef.current) setRunning(false);
      }
    },
    [invoke]
  );

  // ---- Update scheduling ----------------------------------------------
  //
  // Each frame costs a real `qu.exe` run, so the question is not "how often
  // can we fire" but WHAT is worth recomputing. Three rules, in order:
  //
  //   Smart      -- if the source is byte-identical to the last dispatched
  //                 run, there is nothing to recompute. A slider nudged
  //                 back to where it started costs nothing.
  //   Coalescing -- at most one run is ever in flight. A newer slider value
  //                 arriving mid-run REPLACES the pending one instead of
  //                 queueing behind it, so dragging a slider chases the
  //                 latest value rather than replaying every intermediate
  //                 one. Superseded frames are counted, not hidden.
  //   Buffered   -- the displayed frame is only ever swapped for a complete
  //                 one (see `runScript`), so a drag never blanks the plot.
  //
  // This is the "relaxed" timing of the options Ahmed listed rather than a
  // strict fixed-rate clock: the engine sets the pace, frames that the
  // engine could not keep up with are dropped rather than accumulating
  // latency, and nothing is ever left half-drawn.
  const dispatchRun = useCallback(
    async (src: string) => {
      inFlightRef.current = true;
      lastRunSrcRef.current = src;
      try {
        await runScript(src);
      } finally {
        inFlightRef.current = false;
        const next = pendingRef.current;
        pendingRef.current = null;
        if (next !== null && next !== lastRunSrcRef.current) void dispatchRun(next);
      }
    },
    [runScript]
  );

  const scheduleRun = useCallback(
    (src: string) => {
      // The one rule every mode shares: identical source, nothing to do.
      // (`sync` included -- re-running a byte-identical script cannot
      // produce a different answer, it can only waste a frame.)
      if (src === lastRunSrcRef.current) return;

      // The timed modes pace a *stream* of updates -- a slider being
      // dragged while locked. Only then is there a clock running to drain
      // this. Unlocked, a run is a discrete act (Try, Refresh, an edit
      // settling), there is no stream to pace, and parking the source for a
      // clock that isn't ticking would simply never run it. Fall through to
      // the immediate path instead.
      if (locked && (updateMode === 'relaxed' || updateMode === 'strict')) {
        // Hand it to the clock; the tick below decides what actually runs.
        tickPendingRef.current = src;
        return;
      }

      if (updateMode === 'sync') {
        // Every change runs, in order. Queue rather than coalesce.
        if (inFlightRef.current) {
          pendingRef.current = src;
          return;
        }
        void dispatchRun(src);
        return;
      }

      // smart: one in flight, newest wins, superseded frames dropped.
      if (inFlightRef.current) {
        if (pendingRef.current !== null) setDropped((n) => n + 1);
        pendingRef.current = src;
        return;
      }
      void dispatchRun(src);
    },
    [dispatchRun, updateMode, locked]
  );

  // The clock, for the two timed modes. It never interrupts a run in
  // flight -- an interpreter mid-script cannot be preempted -- so a tick
  // that arrives while the engine is busy is a frame that will not happen.
  // `relaxed` shrugs and waits for the next one; `strict` counts it, which
  // is the entire difference between them: strict makes an unmet target
  // rate visible instead of quietly pretending it was met.
  useEffect(() => {
    if (!locked || (updateMode !== 'relaxed' && updateMode !== 'strict')) return;
    const period = 1000 / fps;
    const id = setInterval(() => {
      const src = tickPendingRef.current;
      if (src === null || src === lastRunSrcRef.current) return; // nothing new
      if (inFlightRef.current) {
        if (updateMode === 'strict') setLostFrames((n) => n + 1);
        else setDropped((n) => n + 1);
        return;
      }
      tickPendingRef.current = null;
      void dispatchRun(src);
    }, period);
    return () => clearInterval(id);
  }, [locked, updateMode, fps, dispatchRun]);

  // Unlocking (or switching away from a timed mode) stops that clock. A
  // frame still parked for it would otherwise be stranded, leaving the
  // figure showing data older than the sliders that produced it -- the one
  // failure this panel must never have. Drain it once the clock is gone.
  useEffect(() => {
    if (locked && (updateMode === 'relaxed' || updateMode === 'strict')) return;
    const src = tickPendingRef.current;
    tickPendingRef.current = null;
    if (src !== null && src !== lastRunSrcRef.current) void dispatchRun(src);
  }, [locked, updateMode, dispatchRun]);

  // Editing the script is debounced (nobody wants a run per keystroke);
  // slider drags bypass this and schedule immediately, since coalescing
  // already protects the engine and the extra 300ms just reads as lag.
  // Locked: sliders drive the figure with no debounce at all. Coalescing
  // already protects the engine, so the extra 300ms would only read as lag.
  useEffect(() => {
    if (!locked) return;
    scheduleRun(script);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script, locked]);

  useEffect(() => {
    if (locked) return; // locked: the code can't change, so nothing to watch
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      scheduleRun(script);
    }, DEBOUNCE_MS);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script, locked]);

  /** Re-derive the axis pickers from the script and the latest run, then
   *  run. Deliberately manual: silently re-picking axes underneath someone
   *  who has just chosen them is worse than leaving them alone, so this is
   *  a button rather than an effect. */
  const refreshVariables = useCallback(() => {
    const ordered = assignedNamesInOrder(script);
    const vectors = plotVars.filter(isVectorVar).map((v) => v.name);
    const matrices = plotVars.filter(isMatrixVar).map((v) => v.name);
    // A name the script assigns AND the run produced, in the script's own
    // order; literal x/y/z still win, since that is what they mean.
    // `skip` applies to the literal branch too. A script that defines `y`
    // but no `x` would otherwise give x the name `y` (source order) and
    // then hand y the literal `y` as well -- the same variable on both
    // axes.
    const pickVec = (preferred: string, skip: string[]) =>
      vectors.includes(preferred) && !skip.includes(preferred)
        ? preferred
        : ordered.find((n) => vectors.includes(n) && !skip.includes(n)) ??
          vectors.find((n) => !skip.includes(n)) ??
          '';
    const nextX = pickVec('x', []);
    const nextY = pickVec('y', [nextX]);
    setXVar(nextX);
    setYVar(nextY);
    if ((mode === '3d' && plotKind3d === 'surface') || mode === 'heatmap') {
      setZVar(matrices.includes('z') ? 'z' : ordered.find((n) => matrices.includes(n)) ?? matrices[0] ?? '');
    } else {
      setZVar(pickVec('z', [nextX, nextY]));
    }
    // Force a run even if the source is unchanged -- Refresh means "look
    // again", and the smart rule would otherwise skip it.
    lastRunSrcRef.current = null;
    scheduleRun(script);
  }, [script, plotVars, mode, plotKind3d, scheduleRun]);

  const handleSliderChange = (name: string, value: number) => {
    setSliders((prev) => prev.map((s) => (s.name === name ? { ...s, value } : s)));
    // Scheduling belongs in an effect, not in the state updater -- an
    // updater must stay pure (React may call it twice, or not at all).
    setScript((prev) => setVarInScript(prev, name, value));
  };

  const vectorVars = useMemo(() => plotVars.filter(isVectorVar), [plotVars]);
  const matrixVars = useMemo(() => plotVars.filter(isMatrixVar), [plotVars]);

  // Keep the axis pickers pointed at variables that still exist after each
  // run; fall back to a sensible default (a literal `x`/`y`/`z` binding if
  // present, else the first available candidate) the first time, or
  // whenever the previously-picked name disappeared from this run's output.
  // Z means a different KIND of variable depending on the plot: a matrix
  // for a surface, a vector for everything else. These two effects used to
  // both claim `zVar` unconditionally, so in surface mode the vector effect
  // could hijack it to a vector name -- and `traces` returns [] when the
  // chosen z is not a matrix, which emptied the figure while the title
  // still read "<name> surface". Each effect now owns z only for the modes
  // it is actually about.
  // Both the surface and the heatmap plot a MATRIX as z; the 2D line and
  // the 3D scatter plot a vector. Which kind z has to be is what these
  // effects branch on, so the two matrix modes share one flag rather than
  // the heatmap being forgotten wherever `surfaceMode` was tested.
  const surfaceMode = (mode === '3d' && plotKind3d === 'surface') || mode === 'heatmap';

  useEffect(() => {
    const names = new Set(vectorVars.map((v) => v.name));
    // Each axis avoids the names the axes before it already took. Without
    // this, an x chosen elsewhere (by Refresh, or by the user) and a y
    // falling back to positional order could land on the SAME variable --
    // which is how a figure ends up titled "time vs time", plotting a
    // variable against itself in a perfectly straight and perfectly
    // useless line. Preferring the literal names x/y/z stays first: a
    // script that defines them has already said what its axes are.
    const taken: string[] = [];
    const claim = (current: string, literal: string): string => {
      const keep =
        names.has(current) && !taken.includes(current)
          ? current
          : names.has(literal) && !taken.includes(literal)
            ? literal
            : vectorVars.map((v) => v.name).find((n) => !taken.includes(n)) ?? '';
      if (keep) taken.push(keep);
      return keep;
    };
    // In a matrix mode the axes are not free: a vector is this grid's x
    // only if it has one entry per COLUMN, and its y only if it has one
    // per ROW. Preferring the literal names `x`/`y` there picks whatever
    // the script happened to call something -- for a spectrogram that is
    // the 2001-sample signal, which matches neither dimension, so Plotly
    // silently falls back to bin indices and the figure reads 0..13
    // instead of 0..1.66 seconds. Matching the length first is what makes
    // `spectrogram` plot in real units with nothing to configure.
    const chosen = matrixVars.find((m) => m.name === zVar);
    const byLength = (len: number, skip: string[]): string | undefined =>
      vectorVars.find((v) => v.data.length === len && !skip.includes(v.name))?.name;

    let nextX: string;
    let nextY: string;
    if (surfaceMode && chosen) {
      const [rows, cols] = chosen.shape;
      // Still honour a pick that already fits -- the user may have chosen
      // between two equally valid axes, and re-deciding under them would
      // undo it on every run.
      const fitsX = vectorVars.some((v) => v.name === xVar && v.data.length === cols);
      const fitsY = vectorVars.some((v) => v.name === yVar && v.data.length === rows);
      nextX = fitsX ? xVar : byLength(cols, []) ?? '';
      nextY = fitsY ? yVar : byLength(rows, [nextX]) ?? '';
    } else {
      nextX = claim(xVar, 'x');
      nextY = claim(yVar, 'y');
    }
    if (nextX !== xVar) setXVar(nextX);
    if (nextY !== yVar) setYVar(nextY);
    // Only claim z where z IS a vector; in surface mode it is a matrix and
    // belongs to the effect below.
    if (!surfaceMode) {
      const nextZ = claim(zVar, 'z');
      if (nextZ !== zVar) setZVar(nextZ);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [vectorVars, matrixVars, surfaceMode, zVar]);

  useEffect(() => {
    if (!surfaceMode) return;
    const names = new Set(matrixVars.map((v) => v.name));
    // Runs whenever the mode changes too, not just when the matrix list
    // does -- switching INTO surface mode with a vector still selected was
    // the exact path that produced an empty figure.
    if (!names.has(zVar)) setZVar(names.has('z') ? 'z' : matrixVars[0]?.name ?? '');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matrixVars, surfaceMode, zVar]);

  // Pick the plot kind from what the workspace actually produced, once,
  // on the first run that returns anything. A script that computed a
  // matrix wants a surface; one with only vectors wants a 2D curve --
  // asking the user to say so when the data already answers it is a
  // question not worth posing. Only ever fires while UNLOCKED and only
  // until the user touches the controls themselves, so it guides the
  // first run without ever fighting a deliberate choice.
  const autoDetectedRef = useRef(false);
  useEffect(() => {
    if (locked || autoDetectedRef.current || plotVars.length === 0) return;
    autoDetectedRef.current = true;
    setMode(matrixVars.length > 0 ? '3d' : '2d');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plotVars, locked]);

  const traces = useMemo((): PlotData[] => {
    const byName = new Map(plotVars.map((v) => [v.name, v]));
    if (mode === '2d') {
      const xv = byName.get(xVar);
      const yv = byName.get(yVar);
      if (!xv || !yv) return [];
      return [
        {
          x: xv.data,
          y: yv.data,
          // 'scattergl' is Plotly's WebGL-accelerated 2D scatter/line trace
          // (vs plain SVG 'scatter') -- the "hardware accelerated 2D" half
          // of this panel's brief.
          type: 'scattergl',
          mode: 'lines+markers',
          name: `${yVar} vs ${xVar}`,
          line: { width: 2 },
          marker: { size: 4 },
        },
      ];
    }
    if (mode === 'heatmap') {
      const zv = byName.get(zVar);
      if (!zv || zv.type !== 'matrix') return [];
      const rows = zv.shape[0];
      const cols = zv.shape[1];
      // Same rule as the surface below: a workspace vector becomes an axis
      // only when its length matches that dimension. A spectrogram's
      // `freq` and `time` fit exactly, which is what turns the image from
      // bin indices into hertz and seconds.
      const axis = (name: string, len: number) => {
        const v = byName.get(name);
        return v && v.type === 'vector' && v.data.length === len ? v.data : undefined;
      };
      return [
        {
          z: matrixToRows(zv),
          x: axis(xVar, cols),
          y: axis(yVar, rows),
          type: 'heatmap',
          colorscale: 'Viridis',
          name: zVar,
        },
      ];
    }
    if (plotKind3d === 'scatter3d') {
      const xv = byName.get(xVar);
      const yv = byName.get(yVar);
      const zv = byName.get(zVar);
      if (!xv || !yv || !zv) return [];
      const n = Math.min(xv.data.length, yv.data.length, zv.data.length);
      return [
        {
          x: xv.data.slice(0, n),
          y: yv.data.slice(0, n),
          z: zv.data.slice(0, n),
          type: 'scatter3d',
          mode: 'markers',
          marker: { size: 3 },
          name: `${zVar}(${xVar}, ${yVar})`,
        },
      ];
    }
    // surface
    const zv = byName.get(zVar);
    if (!zv || zv.type !== 'matrix') return [];
    // Carry the workspace's own x/y vectors onto the surface when they fit
    // the grid, instead of leaving Plotly to label the axes 0..N-1. The
    // script already computed the real coordinates (the demo sweeps
    // -3..3); plotting against bare indices silently misreports them, and
    // there was previously no way to pick them at all in surface mode --
    // reported directly as "the x and y aren't spying from the workspace".
    // Mismatched lengths are ignored rather than truncated: a vector that
    // isn't this grid's axis is not a coordinate, and guessing at one
    // would relabel the plot with numbers that mean nothing.
    const rows = zv.shape[0];
    const cols = zv.shape[1];
    const axis = (name: string, len: number) => {
      const v = byName.get(name);
      return v && v.type === 'vector' && v.data.length === len ? v.data : undefined;
    };
    return [
      {
        z: matrixToRows(zv),
        x: axis(xVar, cols),
        y: axis(yVar, rows),
        type: 'surface',
        name: zVar,
      },
    ];
  }, [plotVars, mode, plotKind3d, xVar, yVar, zVar]);

  // Was two hand-written theme branches out of the old navy palette. On
  // tokens it is one class, and it matches the same control everywhere
  // else in the app rather than being this panel's own idea of a select.
  const selectClass =
    'text-xs rounded-lg px-2 py-1.5 border outline-none bg-[var(--qu-surface)] border-[var(--qu-border)] text-[var(--qu-text)]';

  const panelClass = cn(
    'rounded-xl border p-3',
    'bg-[var(--qu-shell-panel)] border-[var(--qu-border)]'
  );

  return (
    <div className="flex-1 flex overflow-hidden">
      {/* Left: script + sliders + axis controls */}
      <div
        className={cn(
          'w-96 flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3',
          theme === 'dark' ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]'
        )}
      >
        {/* Locked: the code and the render configuration are put away, and
            the sliders alone drive the figure in real time. Unlocking hands
            the code back. The script panel is hidden rather than merely
            disabled -- once the target is locked the code is no longer the
            thing you are working on, and the room is better spent on the
            figure. */}
        {!locked && (
          <div className={panelClass}>
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-semibold uppercase tracking-wide">Script</span>
              <div className="flex items-center gap-1.5">
                {/* Re-reads the axis variables from the code (assignment
                    order, x/y/z preferred) and runs. Manual on purpose:
                    silently re-picking axes underneath someone who just
                    chose them would be worse than leaving them be. */}
                <button
                  onClick={refreshVariables}
                  disabled={running}
                  title="Re-detect x / y / z from the code, then run"
                  className={cn(
                    'flex items-center gap-1.5 px-2 py-1 rounded-lg text-xs font-medium transition-all disabled:opacity-50',
                    theme === 'dark'
                      ? 'bg-[#2c2c2a] text-[#dfe7f2] hover:bg-[#3a3a37]'
                      : 'bg-[#e1e0d9] text-[#52514e] hover:bg-[#d5d4cc]'
                  )}
                >
                  <RefreshCw size={12} />
                  Refresh
                </button>
                <button
                  onClick={() => scheduleRun(script)}
                  disabled={running}
                  className={cn(
                    'flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium transition-all',
                    'bg-[#2a78d6] text-white hover:bg-[#3987e5] disabled:opacity-50'
                  )}
                >
                  <Play size={12} />
                  {running ? 'Running…' : 'Try'}
                </button>
              </div>
            </div>
            {/* Taller than it was: the script is the thing you are actually
                working on while unlocked, and 56 was cramped enough that a
                short demo did not fit without scrolling. */}
            <div className="h-80 rounded-lg overflow-hidden border" style={{ borderColor: theme === 'dark' ? '#2c2c2a' : '#e1e0d9' }}>
              <CodeEditor value={script} onChange={setScript} language="qu" theme={theme} showMinimap={false} />
            </div>
            <p className={cn('text-[11px] mt-2', theme === 'dark' ? 'text-[#898781]' : 'text-[#898781]')}>
              Mark a sweepable number with a comment: <code>{'# @slider name min max step'}</code> on the line
              above its assignment.
            </p>
          </div>
        )}

        <div className={panelClass}>
          <button
            onClick={() => {
              const next = !locked;
              setLocked(next);
              // Locking implies "this is the figure I want" -- make sure it
              // reflects the current script even if the last edit never ran.
              if (next) scheduleRun(script);
            }}
            className={cn(
              'w-full flex items-center justify-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all',
              locked
                ? 'bg-[#2a78d6] text-white hover:bg-[#3987e5]'
                : theme === 'dark'
                  ? 'bg-[#2c2c2a] text-[#dfe7f2] hover:bg-[#3a3a37]'
                  : 'bg-[#e1e0d9] text-[#52514e] hover:bg-[#d5d4cc]'
            )}
          >
            {locked ? <Lock size={12} /> : <Crosshair size={12} />}
            {locked ? 'Target locked -- click to edit code' : 'Lock target on'}
          </button>
          {locked && (
            <div className="mt-3 flex flex-col gap-2">
              <label className="text-[11px] flex items-center justify-between gap-2">
                Update
                <select
                  className={selectClass}
                  value={updateMode}
                  onChange={(e) => { setUpdateMode(e.target.value as typeof updateMode); setDropped(0); setLostFrames(0); }}
                  title="How updates are paced. Each frame is a real engine run, so this is a genuine trade-off, not a preference."
                >
                  <option value="smart">Smart (on change)</option>
                  <option value="relaxed">Timer (relaxed)</option>
                  <option value="strict">Timer (strict)</option>
                  <option value="sync">Synchronous</option>
                </select>
              </label>
              {(updateMode === 'relaxed' || updateMode === 'strict') && (
                <label className="text-[11px] flex items-center justify-between gap-2">
                  Rate
                  <select
                    className={selectClass}
                    value={fps}
                    onChange={(e) => { setFps(Number(e.target.value) as typeof fps); setLostFrames(0); }}
                  >
                    <option value={30}>30 fps</option>
                    <option value={60}>60 fps</option>
                    <option value={120}>120 fps</option>
                  </select>
                </label>
              )}
              <p className={cn('text-[11px]', 'text-[var(--qu-muted)]')}>
                {updateMode === 'strict' && lostFrames > 0 ? (
                  <span className="text-amber-500">
                    {lostFrames} frame{lostFrames === 1 ? '' : 's'} missed -- the engine cannot keep up with {fps} fps for this script.
                  </span>
                ) : updateMode === 'sync' ? (
                  'Every change runs, in order. Nothing is dropped.'
                ) : dropped > 0 ? (
                  `Sliders drive the figure live. ${dropped} superseded frame${dropped === 1 ? '' : 's'} skipped.`
                ) : (
                  'Sliders drive the figure live. Points move in place -- the figure is never rebuilt.'
                )}
              </p>
            </div>
          )}
        </div>

        <div className={panelClass}>
          <div className="text-xs font-semibold uppercase tracking-wide mb-2">Sliders</div>
          {sliders.length === 0 ? (
            <div className={cn('text-xs', 'text-[var(--qu-muted)]')}>
              No <code>@slider</code> directives found in the script yet.
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {sliders.map((s) => (
                <div key={s.name}>
                  <div className="flex items-center justify-between text-xs mb-1">
                    <span className="font-mono font-medium">{s.name}</span>
                    <span className={theme === 'dark' ? 'text-[#898781]' : 'text-[#898781]'}>
                      {s.value}
                    </span>
                  </div>
                  <input
                    type="range"
                    min={s.min}
                    max={s.max}
                    step={s.step}
                    value={s.value}
                    onChange={(e) => handleSliderChange(s.name, parseFloat(e.target.value))}
                    className="w-full accent-[#2a78d6]"
                  />
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Hidden while locked: once the target is locked you are tuning
            the figure, not deciding how to draw it -- and the axis roles are
            already settled by then. */}
        {!locked && (
          <div className={panelClass}>
            <div className="text-xs font-semibold uppercase tracking-wide mb-2">Render</div>
            <div className="flex gap-2 mb-3">
              <button
                onClick={() => setMode('2d')}
                className={cn(
                  'flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all',
                  mode === '2d'
                    ? 'bg-[var(--qu-selected)] text-[var(--qu-accent)] shadow-[inset_0_-2px_0_var(--qu-accent)]'
                    : 'text-[var(--qu-muted)] hover:bg-[var(--qu-hover)] hover:text-[var(--qu-text)]'
                )}
              >
                <LineChart size={13} />
                2D (WebGL)
              </button>
              <button
                onClick={() => setMode('3d')}
                className={cn(
                  'flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all',
                  mode === '3d'
                    ? 'bg-[var(--qu-selected)] text-[var(--qu-accent)] shadow-[inset_0_-2px_0_var(--qu-accent)]'
                    : 'text-[var(--qu-muted)] hover:bg-[var(--qu-hover)] hover:text-[var(--qu-text)]'
                )}
              >
                <Box size={13} />
                3D (WebGL)
              </button>
              <button
                onClick={() => setMode('heatmap')}
                className={cn(
                  'flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all',
                  mode === 'heatmap'
                    ? 'bg-[var(--qu-selected)] text-[var(--qu-accent)] shadow-[inset_0_-2px_0_var(--qu-accent)]'
                    : 'text-[var(--qu-muted)] hover:bg-[var(--qu-hover)] hover:text-[var(--qu-text)]'
                )}
              >
                <Grid3x3 size={13} />
                Heatmap
              </button>
            </div>

            {mode === 'heatmap' ? (
              // A matrix plus, optionally, the vectors that give its axes
              // real units. Exactly the surface controls -- the difference
              // between the two modes is how the same three variables are
              // drawn, not which ones they are.
              <div className="flex flex-col gap-2">
                <label className="text-xs flex flex-col gap-1">
                  Matrix
                  <select className={selectClass} value={zVar} onChange={(e) => setZVar(e.target.value)}>
                    {matrixVars.map((v) => (
                      <option key={v.name} value={v.name}>
                        {v.name} ({v.shape[0]}x{v.shape[1]})
                      </option>
                    ))}
                  </select>
                </label>
                <label className="text-xs flex flex-col gap-1">
                  X axis (columns)
                  <select className={selectClass} value={xVar} onChange={(e) => setXVar(e.target.value)}>
                    {vectorVars.map((v) => (
                      <option key={v.name} value={v.name}>{v.name}</option>
                    ))}
                  </select>
                </label>
                <label className="text-xs flex flex-col gap-1">
                  Y axis (rows)
                  <select className={selectClass} value={yVar} onChange={(e) => setYVar(e.target.value)}>
                    {vectorVars.map((v) => (
                      <option key={v.name} value={v.name}>{v.name}</option>
                    ))}
                  </select>
                </label>
                {matrixVars.length === 0 && (
                  // Naming the fix, not just the lack: a script that draws
                  // a spectrogram without keeping its return value has
                  // nothing here, and the reason is not guessable.
                  <div className="text-[11px] opacity-70 leading-snug">
                    No matrix in the workspace. Assign one -- e.g.
                    <code className="mx-1">s = spectrogram(x, 256, 128, Fs)</code>
                    then pick <code>s.db</code>.
                  </div>
                )}
              </div>
            ) : mode === '2d' ? (
              <div className="flex flex-col gap-2">
                <label className="text-xs flex flex-col gap-1">
                  X
                  <select className={selectClass} value={xVar} onChange={(e) => setXVar(e.target.value)}>
                    {vectorVars.map((v) => (
                      <option key={v.name} value={v.name}>{v.name}</option>
                    ))}
                  </select>
                </label>
                <label className="text-xs flex flex-col gap-1">
                  Y
                  <select className={selectClass} value={yVar} onChange={(e) => setYVar(e.target.value)}>
                    {vectorVars.map((v) => (
                      <option key={v.name} value={v.name}>{v.name}</option>
                    ))}
                  </select>
                </label>
              </div>
            ) : (
              <div className="flex flex-col gap-2">
                <div className="flex gap-2 mb-1">
                  <button
                    onClick={() => setPlotKind3d('scatter3d')}
                    className={cn(
                      'flex-1 flex items-center justify-center gap-1.5 px-2 py-1 rounded-lg text-[11px] font-medium transition-all',
                      plotKind3d === 'scatter3d'
                        ? 'bg-[var(--qu-selected)] text-[var(--qu-accent)] shadow-[inset_0_-2px_0_var(--qu-accent)]'
                        : 'text-[var(--qu-muted)] hover:bg-[var(--qu-hover)] hover:text-[var(--qu-text)]'
                    )}
                  >
                    <ScatterChart size={12} />
                    Scatter3D
                  </button>
                  <button
                    onClick={() => setPlotKind3d('surface')}
                    className={cn(
                      'flex-1 flex items-center justify-center gap-1.5 px-2 py-1 rounded-lg text-[11px] font-medium transition-all',
                      plotKind3d === 'surface'
                        ? 'bg-[var(--qu-selected)] text-[var(--qu-accent)] shadow-[inset_0_-2px_0_var(--qu-accent)]'
                        : 'text-[var(--qu-muted)] hover:bg-[var(--qu-hover)] hover:text-[var(--qu-text)]'
                    )}
                  >
                    <Box size={12} />
                    Surface
                  </button>
                </div>

                {plotKind3d === 'scatter3d' ? (
                  <>
                    <label className="text-xs flex flex-col gap-1">
                      X
                      <select className={selectClass} value={xVar} onChange={(e) => setXVar(e.target.value)}>
                        {vectorVars.map((v) => (
                          <option key={v.name} value={v.name}>{v.name}</option>
                        ))}
                      </select>
                    </label>
                    <label className="text-xs flex flex-col gap-1">
                      Y
                      <select className={selectClass} value={yVar} onChange={(e) => setYVar(e.target.value)}>
                        {vectorVars.map((v) => (
                          <option key={v.name} value={v.name}>{v.name}</option>
                        ))}
                      </select>
                    </label>
                    <label className="text-xs flex flex-col gap-1">
                      Z
                      <select className={selectClass} value={zVar} onChange={(e) => setZVar(e.target.value)}>
                        {vectorVars.map((v) => (
                          <option key={v.name} value={v.name}>{v.name}</option>
                        ))}
                      </select>
                    </label>
                  </>
                ) : (
                  <>
                    <label className="text-xs flex flex-col gap-1">
                      Z (matrix)
                      <select className={selectClass} value={zVar} onChange={(e) => setZVar(e.target.value)}>
                        {matrixVars.map((v) => (
                          <option key={v.name} value={v.name}>{v.name} ({v.shape[0]}×{v.shape[1]})</option>
                        ))}
                      </select>
                    </label>
                    {/* A surface has real axes, and the script usually already
                        computed them (the demo sweeps -3..3). Surface mode used
                        to offer no X/Y at all, so the grid was always plotted
                        against bare indices. Only vectors whose length matches
                        the grid are offered -- anything else isn't this
                        surface's axis. `auto` keeps the old index behaviour
                        available rather than forcing a choice. */}
                    <label className="text-xs flex flex-col gap-1">
                      X (columns)
                      <select className={selectClass} value={xVar} onChange={(e) => setXVar(e.target.value)}>
                        <option value="">auto (index)</option>
                        {vectorVars
                          .filter((v) => v.data.length === (matrixVars.find((m) => m.name === zVar)?.shape[1] ?? -1))
                          .map((v) => (
                            <option key={v.name} value={v.name}>{v.name}</option>
                          ))}
                      </select>
                    </label>
                    <label className="text-xs flex flex-col gap-1">
                      Y (rows)
                      <select className={selectClass} value={yVar} onChange={(e) => setYVar(e.target.value)}>
                        <option value="">auto (index)</option>
                        {vectorVars
                          .filter((v) => v.data.length === (matrixVars.find((m) => m.name === zVar)?.shape[0] ?? -1))
                          .map((v) => (
                            <option key={v.name} value={v.name}>{v.name}</option>
                          ))}
                      </select>
                    </label>
                  </>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Right: the plot itself */}
      <div className="flex-1 flex flex-col p-3 overflow-hidden">
        {error && (
          <div className="flex items-center gap-2 px-3 py-2 mb-3 rounded-lg bg-red-500/10 border border-red-500/30 flex-shrink-0">
            <AlertCircle size={14} className="text-red-400 flex-shrink-0" />
            <span className="text-xs text-red-400 font-mono truncate" title={error}>{error}</span>
          </div>
        )}
        <div className="flex-1 min-h-0">
          {/* The plot itself never unmounts or blanks between runs (see the
              double-buffer note in `runScript`); the only thing that changes
              while a re-run is in flight is this thin overlay, so a dragged
              slider reads as a plot updating rather than a plot vanishing
              and coming back. `animate` lets Plotly tween between the old
              and new frame instead of cutting to it. */}
          <div className="relative h-full w-full">
            <PlotViewer
              data={traces}
              title={mode === '3d' ? `${zVar || 'Response'} ${plotKind3d === 'surface' ? 'surface' : 'in 3D'}` : `${yVar || 'Value'} vs ${xVar || 'index'}`}
              theme={theme}
              height="100%"
              xlabel={xVar || undefined}
              ylabel={yVar || undefined}
              zlabel={mode === '3d' ? zVar : undefined}
              showLegend={mode === '2d'}
              animate
              transitionDuration={280}
            />
            {(running || stale) && (
              <div
                className={cn(
                  'absolute top-2 right-2 flex items-center gap-1.5 px-2 py-1 rounded-full text-[11px] pointer-events-none',
                  'backdrop-blur transition-opacity duration-200',
                  'bg-[var(--qu-bg)] text-[var(--qu-accent)] border border-[var(--qu-border)]'
                )}
              >
                <span
                  className={cn(
                    'inline-block w-1.5 h-1.5 rounded-full',
                    running ? 'bg-[#3987e5] animate-pulse' : 'bg-amber-500'
                  )}
                />
                {running ? 'Updating' : 'Showing last good frame'}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

export default InteractiveModePanel;
