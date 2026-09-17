import React, { useEffect, useRef, useState } from 'react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { GuiForm, type GuiNode } from './GuiForm';

export interface GuiMessage { id: string; done?: boolean; error?: string; packet?: { gui: { protocol: number; nodes: GuiNode[] }; output: string; error: string | null } }
const subscribeNative = async (handler: (message: GuiMessage) => void) => {
  const { listen } = await import('@tauri-apps/api/event');
  return listen<GuiMessage>('qu-gui', ({ payload }) => handler(payload));
};
/** Opens the running form as its own native OS window (`GuiRunnerWindow`,
 *  mounted by `main.tsx` off `?guiRunner=1&session=`) instead of this
 *  panel rendering the node tree itself — a designed form is what the
 *  SCRIPT'S USER sees, and embedding it in the same window as the Studio's
 *  own editor/sidebar chrome made that boundary invisible. One window per
 *  session id, so re-running focuses the existing window rather than
 *  piling up duplicates. Used only by `mode="window"`; `mode="inline"`
 *  renders the same tree in-place with `GuiForm` instead. */
async function openRunnerWindow(sessionId: string, previousLabel: string | null, onError: (message: string) => void) {
  const { WebviewWindow } = await import('@tauri-apps/api/window');
  const existing = WebviewWindow.getByLabel(sessionId);
  if (existing) { void existing.setFocus(); return; }
  // Every Run mints a fresh session id, so `getByLabel` above NEVER
  // matches across runs and the previous run's window would just linger,
  // showing a dead "Finished." form -- re-running piled up one stale
  // window per run. Close the one this panel opened last, by label, so a
  // window belonging to any other panel or session is left alone.
  if (previousLabel && previousLabel !== sessionId) {
    const stale = WebviewWindow.getByLabel(previousLabel);
    if (stale) { try { await stale.close(); } catch { /* already closed by its reader */ } }
  }
  const runner = new WebviewWindow(sessionId, {
    url: `index.html?guiRunner=1&session=${sessionId}`,
    title: 'Qu GUI',
    width: 900,
    height: 700,
  });
  // `new WebviewWindow` reports a failed creation through a
  // `tauri://error` event rather than throwing, so without this a window
  // that never opened (allowlist, label clash, OS refusal) left the panel
  // claiming "GUI running in its own window" with no window and no error.
  void runner.once('tauri://error', event => onError(`Could not open the GUI window: ${String(event.payload)}`));
}

interface GuiPanelProps extends LiveSerialPlotPanelProps {
  subscribe?: typeof subscribeNative;
  /** Real, on-disk-backed Qu source to run -- always supplied by
   *  `GuiDesignerPanel` (the merged `form.gen.qu`). Editing happens in
   *  the Designer's own Code tab, not here: this panel's only job is to
   *  run a script and show what it does. */
  initialCode?: string;
  /** `inline`: render the running form in place, right where this panel
   *  sits (the Designer's Code tab, beside the handler-code editor).
   *  `window`: pop the form out as its own native OS window (the
   *  Designer's Run tab) -- the two ways Ahmed asked for, 2026-09-17. */
  mode: 'inline' | 'window';
}
export function GuiPanel({ theme, invoke, subscribe = subscribeNative, initialCode, mode }: GuiPanelProps) {
  const [code, setCode] = useState(initialCode ?? '');
  const [nodes, setNodes] = useState<GuiNode[]>([]);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const [pending, setPending] = useState(false);
  const [ready, setReady] = useState(false);
  const id = useRef<string | null>(null);
  const mounted = useRef(true);
  // Label of the runner window this panel opened last, so the next Run
  // can close it instead of stacking another one (see openRunnerWindow).
  const runnerLabel = useRef<string | null>(null);
  // `initialCode` lets the host (the visual Designer) hand this runtime a
  // freshly generated script without owning a second `gui_start` caller.
  // Only applied on a real change, and never while a session is running,
  // so it can't clobber an in-progress run.
  const lastInitialCode = useRef(initialCode);
  useEffect(() => {
    if (initialCode === undefined || initialCode === lastInitialCode.current) return;
    // Do NOT advance the ref while running -- it used to be consumed
    // before this check, so a design edit made during a live session was
    // swallowed permanently and never re-applied once the session ended.
    // `running` is a dependency, so leaving it pending re-runs this the
    // moment the session stops.
    if (running) return;
    lastInitialCode.current = initialCode;
    setCode(initialCode);
  }, [initialCode, running]);
  useEffect(() => {
    mounted.current = true;
    let unlisten: (() => void) | undefined;
    subscribe(payload => {
      if (!mounted.current || payload.id !== id.current) return;
      if (payload.done) { setRunning(false); setPending(false); }
      if (payload.error) { setError(payload.error); setPending(false); }
      if (payload.packet) {
        setPending(false);
        setError(payload.packet.error ?? '');
        // In `window` mode the node tree is consumed by `GuiRunnerWindow`
        // itself (its own `qu-gui` listener, same session id); tracking it
        // here too is harmless, and `inline` mode needs it to render.
        if (payload.packet.gui?.protocol === 1 && Array.isArray(payload.packet.gui.nodes)) setNodes(payload.packet.gui.nodes);
      }
    }).then(stop => { if (mounted.current) { unlisten = stop; setReady(true); } else stop(); }).catch(error => { if (mounted.current) setError(String(error)); });
    return () => { mounted.current = false; unlisten?.(); const session = id.current; id.current = null; if (session) void invoke('gui_stop', { id: session }).catch(() => {}); };
  }, [invoke, subscribe]);
  async function start() {
    const session = crypto.randomUUID(); id.current = session;
    setError(''); setNodes([]); setRunning(true); setPending(true);
    try {
      await invoke('gui_start', { id: session, code });
      if (!mounted.current || id.current !== session) { await invoke('gui_stop', { id: session }); return; }
      if (mode === 'window') {
        await openRunnerWindow(session, runnerLabel.current, message => { if (mounted.current) setError(message); });
        runnerLabel.current = session;
      }
    } catch (error) { if (mounted.current) { setError(String(error)); setRunning(false); setPending(false); } }
  }
  async function stop() { if (id.current) await invoke('gui_stop', { id: id.current }).catch(error => setError(String(error))); setRunning(false); setPending(false); }
  async function send(node: GuiNode, event: string, value: unknown = null) {
    if (!id.current || pending || !running) return;
    setPending(true);
    try { await invoke('gui_event', { id: id.current, target: node.id, event, value }); }
    catch (err) { setError(String(err)); setPending(false); }
  }

  const controls = (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <button className="qu-gui-button" disabled={!ready || running || !code.trim()} onClick={() => void start()}>Run</button>
      <button className="qu-gui-button" disabled={!running} onClick={() => void stop()}>Stop</button>
      <span role="status" style={{ fontSize: 12, color: 'var(--qu-muted)' }}>
        {pending ? 'Executing Qu…' : running ? (mode === 'window' ? 'Running in its own window' : 'Running') : 'Not running'}
      </span>
    </div>
  );

  if (mode === 'window') {
    return <div className="qu-serial-workspace" style={{ flexDirection: 'row' }}>
      <aside style={{ width: '100%', maxWidth: 420, display: 'flex', flexDirection: 'column', padding: 16, gap: 12 }}>
        <h2>Run</h2>
        <p style={{ color: 'var(--qu-muted)' }}>Opens the form in its own window, separate from the Studio's own chrome.</p>
        {controls}
        {error && <p role="alert" className="qu-serial-error">{error}</p>}
      </aside>
    </div>;
  }

  // `mode === 'inline'`: no separate window, no code editor here (Design
  // and Code already own editing) -- just Run/Stop and the form itself,
  // meant to sit beside the Code tab's handler-body editor.
  return (
    <div className="qu-inspector qu-gui-run" data-theme={theme} style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1, padding: 16, gap: 12 }}>
      {controls}
      {error && <p role="alert" className="qu-serial-error">{error}</p>}
      {!running && !nodes.length && !error && <p style={{ color: 'var(--qu-muted)' }}>Run to see the form here.</p>}
      <div style={{ flex: 1, overflow: 'auto' }}>
        <GuiForm nodes={nodes} running={running} pending={pending} theme={theme} onEvent={(node, event, value) => void send(node, event, value)} />
      </div>
    </div>
  );
}
