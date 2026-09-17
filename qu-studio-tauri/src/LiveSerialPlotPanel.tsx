import React, { useCallback, useEffect, useRef, useState } from 'react';
import { CodeEditor, PlotViewer } from '@qu/ui-components';
import type { PlotData } from '@qu/ui-components';
import { cn } from '@qu/ui-components';
import { Play, Square, Radio, AlertCircle } from 'lucide-react';
import { SeriPlotPanel } from './SeriPlotPanel';

// ============ Types ============

export interface LiveSerialPlotPanelProps {
  theme: 'light' | 'dark';
  invoke: <T,>(command: string, args?: Record<string, any>) => Promise<T>;
}

/** Arduino Serial Plotter's own line format: one or more numbers per line,
 * separated by whitespace or a comma, optionally each prefixed with a
 * `label:` tag (`temp:21.5 humidity:44.2`) naming that value's own trace.
 * Matched here so a script that already prints Arduino-style output (the
 * common case if Ahmed already has sketches printing this shape) plots
 * correctly with zero changes, not just Qu-authored scripts. */
const VALUE_RE = /(?:([A-Za-z_][\w]*)\s*:\s*)?(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;

function parseLine(line: string): Array<{ label: string | null; value: number }> {
  const out: Array<{ label: string | null; value: number }> = [];
  for (const m of line.matchAll(VALUE_RE)) {
    out.push({ label: m[1] ?? null, value: Number(m[2]) });
  }
  return out;
}

// Keep a rolling window rather than an ever-growing array -- a live run is
// meant to run indefinitely (that's the whole point), so an unbounded
// buffer would eventually exhaust memory and slow the plot down as it
// grows. Matches Arduino Serial Plotter's own "scrolling window" feel.
const MAX_POINTS = 500;

// Every line here is kept to 45 characters or fewer on purpose. This
// script lives in a side rail, and Monaco word-wrap silently turns one
// long line into two rendered ones -- costing vertical space in a panel
// whose whole point is seeing the script at once. Measured against the
// real thing rather than guessed: the editor renders 453px wide in this
// rail at 14px Cascadia Mono (8.2px/char), which after the line-number
// gutter leaves room for ~48 characters, so 45 keeps a margin.
const DEFAULT_SCRIPT = `# Live plotter: print() draws instantly.
#
# No device? Comment the serial block,
# uncomment the demo block below.

ports = serial_ports()
print("ports: {ports}")

port = serial_open(ports[0], 9600)
while true
    line = port.read_line()
    # \`none\` means the port went quiet, so
    # skip it instead of plotting a gap.
    if line != none
        print(line)
    end
end

# ---- Hardware-free demo: uncomment ----
# t = 0
# while true
#     print(sin(t / 10))
#     t = t + 1
#     sleep(100)
# end
`;

// ============ Component ============

const ScriptPlotPanel: React.FC<LiveSerialPlotPanelProps> = ({ theme, invoke }) => {
  const [script, setScript] = useState(DEFAULT_SCRIPT);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string>('Idle');
  // One series per distinct label seen so far ("" for unlabeled values --
  // Arduino Serial Plotter's own convention for a bare, untagged number).
  const seriesRef = useRef<Map<string, number[]>>(new Map());
  const xCounterRef = useRef(0);
  const [, forceRender] = useState(0);
  const unlistenRef = useRef<Array<() => void>>([]);

  const appendPoint = useCallback((label: string | null, value: number) => {
    if (!Number.isFinite(value)) return;
    const key = label ?? '';
    const series = seriesRef.current;
    const arr = series.get(key) ?? [];
    arr.push(value);
    if (arr.length > MAX_POINTS) arr.splice(0, arr.length - MAX_POINTS);
    series.set(key, arr);
  }, []);

  const handleLine = useCallback(
    (line: string) => {
      const parsed = parseLine(line);
      if (parsed.length === 0) return; // a status/print line with no numbers -- ignore for plotting
      for (const { label, value } of parsed) appendPoint(label, value);
      xCounterRef.current += 1;
      forceRender((n) => n + 1);
    },
    [appendPoint]
  );

  const stop = useCallback(async () => {
    try {
      await invoke('run_live_stop');
    } catch {
      // Nothing was running -- fine, matches the backend's own tolerance.
    }
    setRunning(false);
    setStatus('Stopped');
  }, [invoke]);

  const start = useCallback(async () => {
    setError(null);
    seriesRef.current = new Map();
    xCounterRef.current = 0;
    forceRender((n) => n + 1);
    setStatus('Running...');
    setRunning(true);
    try {
      await invoke('run_live_start', { code: script });
    } catch (err: any) {
      setError(err?.message ?? String(err));
      setRunning(false);
      setStatus('Failed to start');
    }
  }, [invoke, script]);

  // Subscribes once (not per start/stop) to the three events the backend's
  // `run_live_start` emits for the lifetime of this panel, so a run
  // started just before switching away and back is still being listened
  // to. `@tauri-apps/api/event`'s `listen` is itself async (it resolves
  // once IPC registration completes) -- collected into a ref array so the
  // cleanup function can synchronously call every unlisten, since a
  // cleanup function itself can't be async.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const unlistenLine = await listen<string>('qu-live-line', (event) => {
        handleLine(event.payload);
      });
      const unlistenError = await listen<string>('qu-live-error', (event) => {
        setError(event.payload);
      });
      const unlistenDone = await listen<number | null>('qu-live-done', (event) => {
        setRunning(false);
        setStatus(event.payload === 0 || event.payload === null ? 'Finished' : `Exited (code ${event.payload})`);
      });
      if (cancelled) {
        unlistenLine();
        unlistenError();
        unlistenDone();
        return;
      }
      unlistenRef.current = [unlistenLine, unlistenError, unlistenDone];
    })();
    return () => {
      cancelled = true;
      unlistenRef.current.forEach((fn) => fn());
      // Don't leave a live run (potentially a real infinite loop) running
      // in the background just because the user navigated away from this
      // panel or closed the app -- best-effort, errors ignored the same
      // way `stop()` already tolerates "nothing was running."
      invoke('run_live_stop').catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const traces: PlotData[] = Array.from(seriesRef.current.entries()).map(([label, values]) => {
    const startX = xCounterRef.current - values.length;
    return {
      x: values.map((_, i) => startX + i),
      y: values,
      type: 'scattergl',
      mode: 'lines',
      name: label || 'value',
    };
  });

  const panelClass = cn(
    'rounded-xl border p-3',
    theme === 'dark' ? 'bg-[#161615] border-[#2c2c2a]' : 'bg-white border-[#e1e0d9]'
  );

  return (
    <div className="flex-1 flex overflow-hidden">
      <div
        className={cn(
          // Wider than the 384px (`w-96`) rail the other panels use, and
          // deliberately so: in THIS panel the script is the primary input,
          // not a secondary control beside sliders (InteractiveModePanel) or
          // a stack of compact dropdowns (SeriPlotPanel) -- a narrow rail
          // suits those, but not code. Measured at 384px: the editor got
          // 357px and Monaco's word-wrap turned a 9-line script into 15
          // rendered line-boxes, so ~40% of the height went to wrapping and
          // under half the script was visible at once. 480px clears the
          // demo script's longest line without wrapping.
          'w-[30rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3',
          theme === 'dark' ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]'
        )}
      >
        <div className={panelClass}>
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Radio size={14} className={running ? 'text-green-500 animate-pulse' : undefined} />
              Live Serial Plotter
            </div>
            {running ? (
              <button
                onClick={stop}
                className="flex items-center gap-1 text-xs px-2 py-1 rounded-lg bg-red-500/10 text-red-500 hover:bg-red-500/20 transition-colors"
              >
                <Square size={12} /> Stop
              </button>
            ) : (
              <button
                onClick={start}
                className="flex items-center gap-1 text-xs px-2 py-1 rounded-lg bg-green-500/10 text-green-600 hover:bg-green-500/20 transition-colors"
              >
                <Play size={12} /> Start
              </button>
            )}
          </div>
          {/* Sample count alongside the status: for a live tool, "is it
              actually receiving anything?" is the first question, and a
              bare "Running..." can't answer it -- a stalled port and a
              healthy one look identical without this. */}
          <div className={cn('text-xs flex items-center gap-2', theme === 'dark' ? 'text-[#898781]' : 'text-[#898781]')}>
            <span>{status}</span>
            {xCounterRef.current > 0 && (
              <span className="tabular-nums">
                · {xCounterRef.current} sample{xCounterRef.current === 1 ? '' : 's'}
                {traces.length > 1 ? ` · ${traces.length} traces` : ''}
              </span>
            )}
          </div>
        </div>

        {error && (
          <div className="flex items-start gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-sm text-red-500">
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words">{error}</span>
          </div>
        )}

        {/* Fixed height, NOT `flex-1 min-h-0` -- this sidebar is
            `overflow-y-auto` (content-driven, scrollable), and a flex-1
            child inside a scrollable column has no stable remaining space
            to claim: it gets squeezed as the rest of the column grows
            instead of scrolling. `InteractiveModePanel` already settled on
            the same fixed-height pattern (`h-56`) for its own editor in
            its own scrollable sidebar; this one is a little taller since
            the script IS the primary input here rather than a secondary
            control next to sliders. */}
        <div className="h-96 flex-shrink-0 rounded-xl overflow-hidden border" style={{ borderColor: theme === 'dark' ? '#2c2c2a' : '#e1e0d9' }}>
          <CodeEditor
            language="qu"
            value={script}
            onChange={setScript}
            theme={theme}
            readOnly={running}
            showMinimap={false}
          />
        </div>
        <div className={cn('text-xs', 'text-[var(--qu-muted)]')}>
          Each `print(...)` line is plotted the moment it's flushed -- see
          the script's own comments for a hardware-free demo signal if
          nothing is plugged in.
        </div>
      </div>

      <div className="flex-1 flex flex-col p-3 overflow-hidden">
        <div className="flex-1 min-h-0">
          <PlotViewer
            data={traces}
            title="Live signal"
            theme={theme}
            xlabel="sample"
            ylabel="value"
            showLegend={traces.length > 1}
            height="100%"
          />
        </div>
      </div>
    </div>
  );
};

// The code-first "GUI editor" sub-tab that used to live here was removed
// 2026-09-16: the Designer tab's Run mode (`GuiDesignerPanel.tsx`) is the
// same `GuiPanel` runtime, reached from a proper authoring surface instead
// of being buried under "Live" -- see docs/design/qu-gui-designer-spec.md.
// `GuiPanel.tsx` itself stays; it's still used from there.
export const LiveSerialPlotPanel: React.FC<LiveSerialPlotPanelProps> = (props) => {
  const [workspace, setWorkspace] = useState<'seriplot' | 'script'>('seriplot');
  return <div className="qu-inspector flex flex-1 flex-col min-w-0 min-h-0" data-theme={props.theme}>
    <div className="qu-live-tabs" role="tablist" aria-label="SeriPlot workspace">
      <button role="tab" id="live-seriplot-tab" aria-selected={workspace === 'seriplot'} aria-controls="live-workspace-panel" onClick={() => setWorkspace('seriplot')}>SeriPlot</button>
      <button role="tab" id="live-script-tab" aria-selected={workspace === 'script'} aria-controls="live-workspace-panel" onClick={() => setWorkspace('script')}>Qu script plotter</button>
    </div>
    <div id="live-workspace-panel" role="tabpanel" aria-labelledby={`live-${workspace}-tab`} className="flex flex-1 min-h-0 min-w-0">
      {workspace === 'seriplot' ? <SeriPlotPanel {...props} /> : <ScriptPlotPanel {...props} />}
    </div>
  </div>;
};

export default LiveSerialPlotPanel;
