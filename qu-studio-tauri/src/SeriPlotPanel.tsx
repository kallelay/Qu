import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PlotViewer, serialFigures, parseIndexFilter, IMPEDANCE_VIEWS } from '@qu/ui-components';
import type { SerialMode, SerialFrame, ImpedanceView } from '@qu/ui-components';
import { Activity, Cable, Circle, Download, Pause, Play, RefreshCw, Square, Terminal, X } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import './seriplot.css';

interface Snapshot {
  running: boolean; mode: SerialMode | null; lines: number; version: number; pending: number;
  ignored: number; discarded: number; recording: string | null; error: string | null;
  console: string[]; signal: SerialFrame[] | null; impedance: SerialFrame[] | null;
}
interface Port { name: string; description: string }
const EMPTY: Snapshot = { running: false, mode: null, lines: 0, version: 0, pending: 0, ignored: 0, discarded: 0, recording: null, error: null, console: [], signal: [], impedance: [] };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function SeriPlotPanel({ theme, invoke }: LiveSerialPlotPanelProps) {
  const [ports, setPorts] = useState<Port[]>([]);
  const [port, setPort] = useState('');
  const [source, setSource] = useState('serial');
  const [baud, setBaud] = useState(115200);
  const [snapshot, setSnapshot] = useState<Snapshot>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [dismissedError, setDismissedError] = useState('');
  const [notice, setNotice] = useState('');
  const [paused, setPaused] = useState(false);
  const [fading, setFading] = useState(false);
  const [autoscale, setAutoscale] = useState(true);
  const [modeChoice, setModeChoice] = useState<'auto' | SerialMode>('auto');
  const [view, setView] = useState<ImpedanceView>('nyquist-bode');
  const [filterText, setFilterText] = useState('');
  const [filter, setFilter] = useState('');
  const [filterError, setFilterError] = useState('');
  const [display, setDisplay] = useState({ signal: [] as SerialFrame[], impedance: [] as SerialFrame[], mode: null as SerialMode | null, version: 0 });
  const [consoleOpen, setConsoleOpen] = useState(false);
  const [autoscroll, setAutoscroll] = useState(true);
  const [consoleText, setConsoleText] = useState('');
  const consoleLines = useRef<string[]>([]);
  const consoleNode = useRef<HTMLPreElement>(null);
  const session = useRef<string | null>(null);
  const mounted = useRef(true);
  const latest = useRef<Snapshot>(EMPTY);
  const timeout = useRef<ReturnType<typeof setTimeout>>();

  const refreshPorts = useCallback(async () => {
    try {
      const found = await invoke<Port[]>('seriplot_ports');
      if (!mounted.current) return;
      setPorts(found);
      setPort(current => found.some(item => item.name === current) ? current : found[0]?.name ?? '');
    } catch (error) { if (mounted.current) setError(message(error)); }
  }, [invoke]);

  useEffect(() => {
    mounted.current = true;
    void refreshPorts();
    return () => {
      mounted.current = false;
      clearTimeout(timeout.current);
      const id = session.current;
      session.current = null;
      if (id) void invoke('seriplot_stop', { id }).catch(() => {});
    };
  }, [invoke, refreshPorts]);

  const poll = useCallback(async function read(id: string) {
    try {
      const incoming = await invoke<Snapshot>('seriplot_poll', { id, version: latest.current.version });
      if (!mounted.current || session.current !== id) return;
      const next = { ...incoming, signal: incoming.signal ?? latest.current.signal, impedance: incoming.impedance ?? latest.current.impedance };
      latest.current = next;
      consoleLines.current = [...consoleLines.current, ...incoming.console].slice(-5000);
      setSnapshot(next);
      if (incoming.running) timeout.current = setTimeout(() => void read(id), 200);
    } catch (error) {
      if (!mounted.current || session.current !== id) return;
      setError(message(error));
      // A failed poll must not leave a device running without a working UI.
      await invoke('seriplot_stop', { id }).catch(() => {});
      if (mounted.current && session.current === id) setSnapshot(current => ({ ...current, running: false, recording: null }));
    }
  }, [invoke]);

  useEffect(() => {
    if (!paused) setDisplay({ signal: snapshot.signal ?? [], impedance: snapshot.impedance ?? [], mode: snapshot.mode, version: snapshot.version });
    if (consoleOpen) setConsoleText(consoleLines.current.join('\n'));
  }, [snapshot, paused, consoleOpen]);
  useEffect(() => {
    if (autoscroll && consoleNode.current) consoleNode.current.scrollTop = consoleNode.current.scrollHeight;
  }, [consoleText, autoscroll]);

  const start = async () => {
    setBusy(true); setError(''); setDismissedError(''); setNotice('');
    clearTimeout(timeout.current);
    const id = crypto.randomUUID();
    session.current = id;
    try {
      await invoke('seriplot_start', { id, port: source === 'serial' ? port : null, baud, demo: source === 'serial' ? null : source });
      if (!mounted.current || session.current !== id) { await invoke('seriplot_stop', { id }); return; }
      latest.current = { ...EMPTY, running: true };
      consoleLines.current = [];
      setSnapshot(latest.current); setPaused(false);
      setDisplay({ signal: [], impedance: [], mode: null, version: 0 });
      void poll(id);
    } catch (error) { if (mounted.current) { setError(message(error)); session.current = null; } }
    finally { if (mounted.current) setBusy(false); }
  };
  const stop = async () => {
    const id = session.current; if (!id) return;
    setBusy(true);
    try { await invoke('seriplot_stop', { id }); }
    catch (error) { setError(message(error)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const mode: SerialMode = modeChoice === 'auto' ? display.mode ?? 'impedance' : modeChoice;
  const history = display[mode];
  const figures = useMemo(() => serialFigures(mode, view, history, fading, parseIndexFilter(filter)), [mode, view, history, fading, filter]);
  const latestFrame = history[history.length - 1];
  const displayedPoints = latestFrame?.rows.filter(row => mode === 'signal' || parseIndexFilter(filter)(row[0])).length ?? 0;

  const record = async () => {
    const id = session.current; if (!id) return;
    setBusy(true); setError('');
    try {
      if (snapshot.recording) {
        await invoke('seriplot_record', { id, path: null });
        setNotice('Recording saved.');
      } else {
        const { save } = await import('@tauri-apps/api/dialog');
        const path = await save({ title: 'Record incoming serial lines', defaultPath: `qu-seriplot-${Date.now()}.csv`, filters: [{ name: 'CSV', extensions: ['csv'] }] });
        if (!path) return;
        await invoke('seriplot_record', { id, path });
        setNotice('Recording every incoming line, including while the display is paused.');
      }
    } catch (error) { setError(message(error)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const saveBuffer = async () => {
    const id = session.current; if (!id) return;
    setBusy(true); setError('');
    try {
      // Capture the complete, unfiltered native buffer before opening the dialog.
      const content = await invoke<string>('seriplot_buffer', { id, mode });
      const { save } = await import('@tauri-apps/api/dialog');
      const path = await save({ title: `Save ${mode} buffer`, defaultPath: `qu-${mode}-buffer.csv`, filters: [{ name: 'CSV', extensions: ['csv'] }] });
      if (path) { await invoke('save_file', { path, content }); setNotice('Saved all retained complete frames, without display filtering.'); }
    } catch (error) { setError(message(error)); }
    finally { if (mounted.current) setBusy(false); }
  };

  return <div className="qu-serial-workspace qu-inspector" data-theme={theme}>
    <header className="qu-serial-heading">
      <div><span className="qu-serial-eyebrow"><Activity size={14} /> MEASUREMENT WORKSPACE</span><h1>SeriPlot <span>Native in Qu</span></h1><p>Signals and impedance, from acquisition to a finished figure.</p></div>
      <div className="qu-serial-status" role="status"><i data-active={snapshot.running} />{snapshot.running ? source === 'serial' ? 'Connected' : 'Demo running' : snapshot.lines ? 'Disconnected' : 'Ready'}{paused && <span>Display paused</span>}</div>
    </header>
    <div className="qu-serial-content">
      <aside className="qu-serial-controls" aria-label="Acquisition controls">
        <section><h2><Cable size={15} /> Acquisition</h2>
          <label>Source<select aria-label="Acquisition source" value={source} disabled={snapshot.running || busy} onChange={e => setSource(e.target.value)}><option value="serial">Serial device</option><option value="impedance">Demo · impedance sweep</option><option value="signal">Demo · two-channel signal</option></select></label>
          {source === 'serial' ? <>
            <label>Port<div className="qu-serial-port"><select aria-label="Serial port" value={port} disabled={snapshot.running || busy} onChange={e => setPort(e.target.value)}>{!ports.length && <option value="">No ports found</option>}{ports.map(port => <option key={port.name} value={port.name}>{port.name}{port.description ? ` · ${port.description}` : ''}</option>)}</select><button aria-label="Refresh serial ports" title="Refresh ports" disabled={snapshot.running || busy} onClick={() => void refreshPorts()}><RefreshCw size={14} /></button></div></label>
            <label>Baud rate<select aria-label="Baud rate" value={baud} disabled={snapshot.running || busy} onChange={e => setBaud(Number(e.target.value))}>{[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1000000, 2000000, 3000000].map(rate => <option key={rate} value={rate}>{rate.toLocaleString()}</option>)}</select></label>
          </> : <p className="qu-serial-hint">Simulated data passes through the native wire-protocol decoder. No device is opened.</p>}
          <button className="qu-serial-primary" disabled={busy || (!snapshot.running && source === 'serial' && !port)} onClick={() => void (snapshot.running ? stop() : start())}>{snapshot.running ? <Square size={14} /> : <Play size={14} />}{busy ? 'Working…' : snapshot.running ? 'Disconnect' : source === 'serial' ? 'Connect' : 'Start demo'}</button>
          <div className="qu-serial-metrics"><div><strong>{snapshot.lines.toLocaleString()}</strong><span>received lines</span></div><div><strong>{snapshot.version.toLocaleString()}</strong><span>complete frames</span></div></div>
          <p className="qu-serial-hint">{snapshot.mode === 'signal' ? `${snapshot.pending} / 2048 samples in the next frame` : snapshot.mode === 'impedance' ? `${snapshot.pending} points in the current sweep` : 'Mode is detected from incoming data.'}</p>
        </section>
        <section><h2>Display</h2>
          <label>Measurement<select aria-label="Measurement mode" value={modeChoice} onChange={e => setModeChoice(e.target.value as typeof modeChoice)}><option value="auto">Auto-detect</option><option value="signal">Signal · ADC 1 + ADC 2</option><option value="impedance">Impedance</option></select></label>
          {mode === 'impedance' && <label>Layout<select aria-label="Impedance layout" value={view} onChange={e => setView(e.target.value as ImpedanceView)}>{IMPEDANCE_VIEWS.map(view => <option key={view.value} value={view.value}>{view.label}</option>)}</select></label>}
          <label className="qu-serial-check"><input type="checkbox" checked={fading} onChange={e => setFading(e.target.checked)} />Fade previous 10 frames</label>
          <label className="qu-serial-check"><input type="checkbox" checked={autoscale} onChange={e => setAutoscale(e.target.checked)} />Autoscale incoming frames</label>
          <button disabled={!snapshot.lines} aria-pressed={paused} onClick={() => setPaused(value => !value)}>{paused ? <Play size={14} /> : <Pause size={14} />}{paused ? 'Resume display' : 'Pause display'}</button>
          {paused && <p className="qu-serial-hint">Acquisition and recording continue.</p>}
          {mode === 'impedance' && <form onSubmit={event => { event.preventDefault(); try { parseIndexFilter(filterText); setFilter(filterText.trim()); setFilterError(''); } catch (error) { setFilterError(message(error)); } }}>
            <label>Frequency indices<input aria-label="Frequency index filter" value={filterText} placeholder="0-5,8 or skip:3,7" onChange={e => setFilterText(e.target.value)} aria-invalid={!!filterError} /></label>
            <div className="qu-serial-buttons"><button type="submit">Apply filter</button><button type="button" disabled={!filter && !filterText} onClick={() => { setFilter(''); setFilterText(''); setFilterError(''); }}>Clear</button></div>
            {filterError && <p role="alert" className="qu-serial-error">{filterError}</p>}
            <p className="qu-serial-hint">Indices are sent by the device. Frequency in Hz is not part of this protocol.</p>
          </form>}
        </section>
        <section><h2>Capture & export</h2>
          <button disabled={!snapshot.running || busy} className={snapshot.recording ? 'qu-serial-recording' : ''} onClick={() => void record()}>{snapshot.recording ? <Square size={14} /> : <Circle size={14} />}{snapshot.recording ? 'Stop recording' : 'Record raw CSV'}</button>
          {snapshot.recording && <p className="qu-serial-hint qu-serial-path" title={snapshot.recording}>{snapshot.recording}</p>}
          <button disabled={!(snapshot[mode]?.length) || busy} onClick={() => void saveBuffer()}><Download size={14} />Save {mode} buffer</button>
          <p className="qu-serial-hint">Keeps 15 complete frames per mode. CSV includes all retained data; figure exports use the current view.</p>
        </section>
      </aside>
      <main className="qu-serial-main">
        {(error || (snapshot.error && snapshot.error !== dismissedError)) && <div className="qu-serial-banner qu-serial-error" role="alert">{error || snapshot.error}<button aria-label="Dismiss error" onClick={() => { setError(''); setDismissedError(snapshot.error ?? ''); }}><X size={14} /></button></div>}
        {notice && <div className="qu-serial-banner" role="status">{notice}<button aria-label="Dismiss notice" onClick={() => setNotice('')}><X size={14} /></button></div>}
        <div className="qu-serial-view-heading"><div><h2>{mode === 'signal' ? 'Two-channel signal' : 'Impedance spectrum'}</h2><p>{latestFrame ? `${displayedPoints.toLocaleString()} points · ${history.length} retained ${mode === 'signal' ? 'frames' : 'sweeps'}${fading ? ' · fading history' : ' · latest frame'}` : 'Waiting for a complete frame'}{filter && mode === 'impedance' ? ` · filter: ${filter}` : ''}</p></div><button aria-expanded={consoleOpen} onClick={() => setConsoleOpen(value => !value)}><Terminal size={15} />Console</button></div>
        {!latestFrame ? <div className="qu-serial-empty"><Activity size={40} /><h2>{snapshot.running ? 'Listening to your data' : 'A clear view of every measurement'}</h2><p>{snapshot.running ? snapshot.mode === 'signal' ? 'The first signal frame appears after 2048 samples.' : 'A complete impedance sweep appears when the next zero index arrives.' : 'Connect a serial device, or choose a demo to explore signals, Nyquist plots, and Bode views.'}</p><div><span>2048 samples / signal frame</span><span>15 frames of history</span><span>SVG & PNG figures</span></div></div> : <>
          {!displayedPoints && <p className="qu-serial-banner">No points match this filter. Clear it to see the full sweep.</p>}
          <div className={`qu-serial-figures ${mode === 'signal' ? 'is-signal' : figures.length === 3 ? view === 'nyquist-components' ? 'is-components' : 'is-bode' : 'is-single'}`}>
            {figures.map(figure => <div className="qu-serial-figure" key={figure.key}><PlotViewer data={figure.data} title={figure.title} subtitle={source !== 'serial' ? 'DEMO · simulated measurement' : undefined} xlabel={figure.xlabel} ylabel={figure.ylabel} equalAspect={figure.equalAspect} showLegend={mode === 'impedance' && ['phase', 'magnitude'].includes(figure.key)} animate={false} theme={theme} height="100%" viewRevision={autoscale ? display.version : undefined} /></div>)}
          </div>
        </>}
        {consoleOpen && <section className="qu-serial-console"><header><h2>Serial console <span>{consoleLines.current.length.toLocaleString()} / 5,000 lines</span></h2><label className="qu-serial-check"><input type="checkbox" checked={autoscroll} onChange={e => setAutoscroll(e.target.checked)} />Autoscroll</label><button onClick={() => { consoleLines.current = []; setConsoleText(''); }}>Clear console</button></header><pre ref={consoleNode} tabIndex={0} aria-label="Raw serial output">{consoleText || 'Incoming lines will appear here.'}</pre></section>}
        <footer className="qu-serial-footer"><span>{snapshot.ignored.toLocaleString()} non-data lines ignored · {snapshot.discarded.toLocaleString()} partial samples discarded</span><span>{snapshot.running ? 'Acquisition active' : snapshot.pending ? `${snapshot.pending} pending rows not included in complete-frame exports` : 'Native serial · no Python runtime'}</span></footer>
      </main>
    </div>
  </div>;
}
