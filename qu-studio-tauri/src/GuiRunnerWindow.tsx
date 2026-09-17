import React, { useEffect, useRef, useState } from 'react';
import { useTheme } from '@qu/ui-components';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';
import { getCurrent } from '@tauri-apps/api/window';
import { GuiForm, type GuiNode } from './GuiForm';

interface Packet { gui: { protocol: number; nodes: GuiNode[] }; output: string; error: string | null }
interface GuiMessage { id: string; done?: boolean; error?: string; packet?: Packet }

/** The running side of a designed/hand-written GUI, as its own native OS
 *  window rather than a panel embedded inside the Studio's own chrome —
 *  a designed form is what the SCRIPT'S USER sees, not another tab of the
 *  IDE, and rendering it inline made that boundary invisible. Mounted by
 *  `main.tsx` when the page URL carries `?guiRunner=1&session=<id>` (see
 *  that file's own comment), which is how `GuiPanel`'s `mode="window"`
 *  opens this: a second `WebviewWindow` pointed at the same `index.html`
 *  with that query string, rather than a second Tauri command or a second
 *  copy of this component's markup living in two places. The widget-tree
 *  rendering itself lives in `GuiForm.tsx`, shared with `GuiPanel`'s
 *  `mode="inline"` embed, so the two run paths never visually drift.
 *
 *  Event wiring is otherwise identical to `GuiPanel`'s own (`qu-gui`
 *  events, `gui_event`/`gui_stop` invocations) — Tauri events and the
 *  backend `gui_start` session are both process-wide, not scoped to one
 *  webview, so a second window listening for the same session id works
 *  with no backend change. See `docs/design/qu-gui-designer-spec.md` §5.
 */
export function GuiRunnerWindow() {
  const { resolvedTheme: theme } = useTheme();
  const params = new URLSearchParams(window.location.search);
  const sessionId = params.get('session') ?? '';
  const [nodes, setNodes] = useState<GuiNode[]>([]);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(true);
  const [pending, setPending] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    let unlisten: (() => void) | undefined;
    // Whether the LIVE event stream has already delivered a packet. The
    // catch-up snapshot below must never overwrite newer live state.
    let sawLivePacket = false;
    const applyPacket = (packet: Packet) => {
      if (packet.gui?.protocol !== 1 || !Array.isArray(packet.gui.nodes)) { setError('Unsupported GUI protocol'); return; }
      setNodes(packet.gui.nodes);
      setPending(false);
      setError(packet.error ?? '');
    };
    listen<GuiMessage>('qu-gui', ({ payload }) => {
      if (!mounted.current || payload.id !== sessionId) return;
      if (payload.done) { setRunning(false); setPending(false); }
      if (payload.error) { setError(payload.error); setPending(false); }
      if (payload.packet) { sawLivePacket = true; applyPacket(payload.packet); }
    }).then(async stop => {
      if (!mounted.current) { stop(); return; }
      unlisten = stop;
      // Catch-up. `gui_start` broadcasts the engine's one and only
      // initial packet -- the whole widget tree -- before this window
      // exists, let alone before the listener above is registered, so
      // without this the window renders nothing and, having no widget to
      // click, can never provoke a second packet either. See
      // `gui_bridge.rs`'s `Session`/`gui_snapshot` for the full note.
      // Fetched AFTER the listener is up so the two cannot both miss.
      try {
        const cached = await invoke<Packet | null>('gui_snapshot', { id: sessionId });
        if (mounted.current && !sawLivePacket && cached) applyPacket(cached);
      } catch (err) { if (mounted.current && !sawLivePacket) setError(String(err)); }
    }).catch(err => { if (mounted.current) setError(String(err)); });
    // Closing this window (the reader closing the form) stops the session
    // on the backend too, rather than leaving an orphaned `gui_start` run
    // with nothing left listening to it.
    const currentWindow = getCurrent();
    const unlistenClose = currentWindow.onCloseRequested(() => {
      if (sessionId) void invoke('gui_stop', { id: sessionId }).catch(() => {});
    });
    return () => {
      mounted.current = false;
      unlisten?.();
      void unlistenClose.then(stop => stop());
    };
  }, [sessionId]);

  async function send(node: GuiNode, event: string, value: unknown = null) {
    if (!sessionId || pending || !running) return;
    setPending(true);
    try { await invoke('gui_event', { id: sessionId, target: node.id, event, value }); }
    catch (err) { setError(String(err)); setPending(false); }
  }

  return (
    <div className="qu-inspector qu-gui-run" data-theme={theme}>
      {!running && !error && <p role="status" style={{marginBottom:16,color:'var(--qu-muted)'}}>Finished.</p>}
      {error && <p role="alert" className="qu-serial-error" style={{marginBottom:16}}>{error}</p>}
      <GuiForm nodes={nodes} running={running} pending={pending} theme={theme} onEvent={(node, event, value) => void send(node, event, value)} onFrameClose={() => void getCurrent().close()} />
    </div>
  );
}

export default GuiRunnerWindow;
