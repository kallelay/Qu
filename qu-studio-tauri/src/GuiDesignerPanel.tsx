import React, { useCallback, useEffect, useRef, useState } from 'react';
import { GuiDesigner, CodeEditor, mergeGenScript, reconcileCodeQu, requiredHandlerNames } from '@qu/ui-components';
import { PenTool, Code2, Play } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { GuiPanel } from './GuiPanel';

const BASE_PATH_KEY = 'qu-gui-designer-base-path';

/** `base` is whatever the save dialog returned for the form (e.g.
 *  `.../form.qu`) -- the three real, on-disk files this panel owns are
 *  named off its stem, matching Ahmed's own naming directly:
 *    form.design.qu  -- machine-owned, regenerated wholesale by Design
 *    form.code.qu    -- the user's own hand-written handler bodies
 *    form.gen.qu     -- the two merged; what Run actually executes
 */
function deriveThreeFiles(base: string) {
  const stem = base.replace(/\.qu$/i, '');
  return {
    designPath: `${stem}.design.qu`,
    codePath: `${stem}.code.qu`,
    genPath: `${stem}.gen.qu`,
  };
}

async function pickBasePath(): Promise<string | null> {
  const { save } = await import('@tauri-apps/api/dialog');
  const selection = await save({ filters: [{ name: 'Qu Script', extensions: ['qu'] }], defaultPath: 'form.qu' });
  return selection ?? null;
}

/** Host for the visual GUI designer: a Design/Code/Run trio.
 *
 * § Design/Code/Run split (2026-09-16, Ahmed's direct instruction):
 * NOT a one-way "draw emits code, read-only" pipe. Two independently
 * editable, REAL on-disk files (`deriveThreeFiles` above) -- Design
 * regenerates `form.design.qu` wholesale on every sync (VB6/WinForms'
 * own `Designer.cs` model: machine-owned, safe to overwrite completely);
 * Code is the user's own free-form `form.code.qu`, never touched by a
 * design edit except to ADD an empty stub for a genuinely new handler
 * name (`reconcileCodeQu` -- an existing body is never overwritten, a
 * no-longer-referenced one is never deleted). Run merges the two
 * (`mergeGenScript`, code first so `gui.rs`'s "function before its
 * `.on()`" requirement holds) into `form.gen.qu`, writes THAT to disk
 * too, and hands it to the EXISTING `GuiPanel` runtime -- one
 * `gui_start` caller in the app, not two. See
 * `docs/design/qu-gui-designer-spec.md` §5.
 */
export const GuiDesignerPanel: React.FC<LiveSerialPlotPanelProps> = (props) => {
  const { invoke } = props;
  const [mode, setMode] = useState<'design' | 'code' | 'run'>('design');
  const [designCode, setDesignCode] = useState('');
  const [userCode, setUserCode] = useState('');
  const [basePath, setBasePath] = useState<string | null>(() => localStorage.getItem(BASE_PATH_KEY));
  const [status, setStatus] = useState('');
  const codeSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // `userCode` read from a callback that must not re-create itself every
  // time the user types a character (`syncToDisk` below).
  const userCodeRef = useRef(userCode);
  userCodeRef.current = userCode;

  // Load whatever's already on disk for a base path restored from a
  // previous session -- a missing file (first run for this form) is not
  // an error, just an empty starting point.
  //
  // `form.design.qu` is deliberately NOT read back here: `GuiDesigner`
  // restores the design tree from its own localStorage and mirrors it
  // out through `onDesignCodeChange` on mount, so the canvas is the
  // authority for the design. Reading the file too raced that mirror --
  // whichever of the two async paths landed second won, and the disk
  // copy landing second would silently replace the live canvas's source
  // with a stale form's.
  useEffect(() => {
    if (!basePath) return;
    const { codePath } = deriveThreeFiles(basePath);
    (async () => {
      try {
        const onDisk = await invoke<string>('open_file', { path: codePath });
        // Only if nothing has been typed or stubbed in meanwhile.
        if (!userCodeRef.current.trim()) setUserCode(onDisk);
      } catch { /* not written yet */ }
    })();
    // Only on mount / when a restored basePath first appears -- this
    // panel is the only writer of these two paths afterward, so re-
    // reading on every basePath identity change (there isn't one after
    // mount) would just re-read what this panel itself last wrote.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function ensureBasePath(): Promise<string | null> {
    if (basePath) return basePath;
    const picked = await pickBasePath();
    if (!picked) return null;
    localStorage.setItem(BASE_PATH_KEY, picked);
    setBasePath(picked);
    return picked;
  }

  /** Reconcile handler stubs for `design`, write all three files, and
   *  return the merged `form.gen.qu` source.
   *
   *  Shared by the Design tab's explicit sync button AND by Run. Run used
   *  to write only `form.gen.qu`, built from a `designCode` that nothing
   *  but that button ever set -- so "place widgets, press Run" merged an
   *  EMPTY design with an empty code file, `mergeGenScript` returned a
   *  lone "\n", and the Run tab came up with a blank editor that executed
   *  nothing. That is the whole of "GUI not working" for anyone who did
   *  not know to click the unlabelled icon in the code strip first.
   *  Running a design now syncs it, which is what pressing Run in a form
   *  designer has always meant. */
  const syncToDisk = useCallback(async (design: string): Promise<string> => {
    const reconciled = reconcileCodeQu(userCodeRef.current, requiredHandlerNamesFromDesignQu(design));
    if (reconciled !== userCodeRef.current) { userCodeRef.current = reconciled; setUserCode(reconciled); }
    const gen = mergeGenScript(reconciled, design);
    const base = await ensureBasePath();
    if (!base) return gen;
    const { designPath, codePath, genPath } = deriveThreeFiles(base);
    try {
      await invoke('save_file', { path: designPath, content: design });
      await invoke('save_file', { path: codePath, content: reconciled });
      await invoke('save_file', { path: genPath, content: gen });
      setStatus(`Saved ${[designPath, codePath, genPath].map(p => p.split(/[\\/]/).pop()).join(', ')}`);
    } catch (err) {
      setStatus(`Save failed: ${String(err)}`);
    }
    return gen;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [basePath, invoke]);

  /** Design -> Code sync, from the code strip's own button. */
  const handleDesignSync = useCallback(async (code: string) => {
    setDesignCode(code);
    await syncToDisk(code);
    setMode('code');
  }, [syncToDisk]);

  // `requiredHandlerNames` (the exported utility) walks a DesignNode[],
  // which this panel never sees -- only the already-rendered
  // `form.design.qu` text. Its own `.on(...)` bindings name every
  // handler it needs just as completely, so extracting from the text is
  // equivalent here and avoids widening GuiDesigner's own prop surface
  // just to hand a parallel copy of the same information up.
  function requiredHandlerNamesFromDesignQu(designQu: string): string[] {
    const seen = new Set<string>();
    const names: string[] = [];
    const re = /\.on\(\s*"(?:[^"\\]|\\.)*"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(designQu)) !== null) {
      if (!seen.has(m[1])) { seen.add(m[1]); names.push(m[1]); }
    }
    return names;
  }

  function handleCodeChange(next: string) {
    setUserCode(next);
    if (codeSaveTimer.current) clearTimeout(codeSaveTimer.current);
    codeSaveTimer.current = setTimeout(async () => {
      if (!basePath) return; // nothing to save to yet -- Design hasn't synced once
      const { codePath } = deriveThreeFiles(basePath);
      try {
        await invoke('save_file', { path: codePath, content: next });
        setStatus(`Saved ${codePath.split(/[\\/]/).pop()}`);
      } catch (err) {
        setStatus(`Save failed: ${String(err)}`);
      }
    }, 600);
  }

  const genCode = mergeGenScript(userCode, designCode);

  async function enterRun() {
    await syncToDisk(designCode);
    setMode('run');
  }

  return (
    <div className="qu-inspector flex flex-1 flex-col min-w-0 min-h-0" data-theme={props.theme}>
      <div className="qu-live-tabs" role="tablist" aria-label="GUI designer workspace">
        <button
          role="tab"
          id="designer-design-tab"
          aria-selected={mode === 'design'}
          aria-controls="designer-workspace-panel"
          onClick={() => setMode('design')}
        >
          <PenTool size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />
          Design
        </button>
        <button
          role="tab"
          id="designer-code-tab"
          aria-selected={mode === 'code'}
          aria-controls="designer-workspace-panel"
          onClick={() => setMode('code')}
        >
          <Code2 size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />
          Code
        </button>
        <button
          role="tab"
          id="designer-run-tab"
          aria-selected={mode === 'run'}
          aria-controls="designer-workspace-panel"
          onClick={() => void enterRun()}
        >
          <Play size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />
          Run
        </button>
        {status && <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--qu-muted)', padding: '0 10px' }}>{status}</span>}
      </div>
      <div
        id="designer-workspace-panel"
        role="tabpanel"
        aria-labelledby={`designer-${mode}-tab`}
        className="flex flex-1 min-h-0 min-w-0"
      >
        {mode === 'design' ? (
          <GuiDesigner
            theme={props.theme}
            onInsertCode={(code) => void handleDesignSync(code)}
            onDesignCodeChange={setDesignCode}
          />
        ) : mode === 'code' ? (
          <div className="flex flex-1 min-w-0 min-h-0" style={{ gap: 0 }}>
            {/* Left: form.code.qu -- your own handler bodies. Design only
                ever ADDS an empty stub here for a new handler; it never
                edits or removes what you write. Right: the same script,
                actually running, right where you can see it while you
                edit -- one of the two ways to run a form (the other is
                the Run tab's own window), not a duplicate of it. */}
            <div className="flex flex-1 flex-col min-w-0 min-h-0" style={{ padding: 16, gap: 12, borderRight: '1px solid var(--qu-border)' }}>
              <p style={{ color: 'var(--qu-muted)' }}>
                form.code.qu — your own handler bodies. Design only ever ADDS an
                empty stub here for a new handler; it never edits or removes what
                you write.
              </p>
              <div style={{ flex: 1, minHeight: 0 }}>
                <CodeEditor
                  value={userCode}
                  onChange={handleCodeChange}
                  language="qu"
                  theme={props.theme}
                  showMinimap={false}
                />
              </div>
            </div>
            <div className="flex flex-1 min-w-0 min-h-0">
              <GuiPanel {...props} initialCode={genCode} mode="inline" />
            </div>
          </div>
        ) : (
          <GuiPanel {...props} initialCode={genCode} mode="window" />
        )}
      </div>
    </div>
  );
};

export default GuiDesignerPanel;
