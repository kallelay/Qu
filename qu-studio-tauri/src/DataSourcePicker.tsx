// The source-picker UI shared by the Fit tab and the ML lab: choose where a
// table comes from (a built-in example, pasted text, a CSV file from disk, or
// numeric variables of the Code tab's session), plus the notice/banner and the
// preview table that follow, and the two Tauri helpers every source needs (the
// file dialog, and writing a big table to a temp CSV the generated script
// reads back). What each app does WITH the table (column roles, types,
// examples) stays in the app's own file; `dataTable.ts` is the pure side.
import React, { useRef } from 'react';
import { AlertCircle, FileUp, FolderOpen } from 'lucide-react';

export type SourceKind = 'example' | 'paste' | 'file' | 'variable';

const border = 'var(--qu-border)';
const muted = { color: 'var(--qu-muted)' } as const;

// --------------------------------------------------------------- Tauri helpers

/** Is this the desktop app (the file dialog and file system exist)? */
export const inDesktopApp = (): boolean => typeof (window as any).__TAURI_IPC__ !== 'undefined';

/** Opens the native file dialog for a table; the chosen path, or null when the
 *  user cancelled. Throws when there is no dialog (not the desktop app). */
export async function pickTableFile(filterName: string, extensions: string[]): Promise<string | null> {
  const { open } = await import('@tauri-apps/api/dialog');
  const picked = await open({ multiple: false, filters: [{ name: filterName, extensions }] });
  const path = Array.isArray(picked) ? picked[0] : picked;
  return typeof path === 'string' && path ? path : null;
}

/** Writes `text` to `<app data|cache dir>[/subdir]/<file>`, replacing what is
 *  there (one fixed name, so staged tables never pile up), and returns the
 *  path for the script's read_csv. Throws when it cannot. */
export async function stageTextFile(text: string, where: { base: 'data' | 'cache'; subdir?: string; file: string }): Promise<string> {
  const { appDataDir, appCacheDir, join } = await import('@tauri-apps/api/path');
  const { createDir, writeTextFile } = await import('@tauri-apps/api/fs');
  const root = where.base === 'data' ? await appDataDir() : await appCacheDir();
  const dir = where.subdir ? await join(root, where.subdir) : root;
  await createDir(dir, { recursive: true }).catch(() => undefined);
  const file = await join(dir, where.file);
  await writeTextFile(file, text);
  return file;
}

// ---------------------------------------------------------------------- pieces

/** A warning/error banner with an alert icon. */
export const NoticeBanner: React.FC<{ kind: 'error' | 'warning'; text: string; compact?: boolean }> = ({ kind, text, compact }) => (
  <div className="qu-banner" data-kind={kind} style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12, padding: compact ? '6px 8px' : '6px 9px' }}>
    <AlertCircle size={compact ? 14 : 13} className="flex-shrink-0 mt-0.5" />
    <span className="break-words">{text}</span>
  </div>
);

/** The first rows of a table as text, with optional muted footer rows (statistics).
 *  `dense` is the compact look the ML lab uses. */
export const PreviewTable: React.FC<{ columns: string[]; rows: string[][]; footer?: string[][]; dense?: boolean; testId?: string }> = ({ columns, rows, footer = [], dense, testId }) => (
  <div className={dense ? 'overflow-x-auto rounded border' : 'rounded-lg border text-xs overflow-x-auto'} style={{ borderColor: border }} data-testid={testId}>
    <table className={dense ? 'text-[11px] w-full font-mono' : 'w-full font-mono'} style={dense ? { borderCollapse: 'collapse' } : undefined}>
      <thead>
        <tr style={dense ? muted : undefined}>
          {columns.map((c, j) => <th key={j} className={dense ? 'font-normal px-1 py-1 text-left whitespace-nowrap' : 'px-2 py-1 text-left font-medium'}>{c}</th>)}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i} style={dense ? { borderTop: `1px solid ${border}` } : undefined}>
            {r.map((c, j) => <td key={j} className={dense ? 'px-1 whitespace-nowrap' : 'px-2 py-0.5'}>{c}</td>)}
          </tr>
        ))}
        {footer.map((r, i) => (
          <tr key={`f${i}`} style={muted}>{r.map((c, j) => <td key={j} className="px-2 py-0.5">{c}</td>)}</tr>
        ))}
      </tbody>
    </table>
  </div>
);

// ---------------------------------------------------------------------- picker

export interface PasteSource {
  value: string;
  onChange: (text: string) => void;
  placeholder: string;
  rows: number;
  className: string;
  ariaLabel?: string;
  spellCheck?: boolean;
}

/** A file chosen through a button (the Fit tab) or typed/browsed as a path (the ML lab). */
export type FileSource =
  | {
      mode: 'button';
      label: string;
      /** Extensions the browser fallback input accepts, e.g. ".csv,.txt". */
      accept: string;
      /** Handle the click (the native dialog). Resolve 'fallback' when the desktop dialog is not available. */
      onChoose: () => Promise<'handled' | 'fallback'>;
      /** The browser fallback picked a file. */
      onBrowserFile: (name: string, text: string) => void;
    }
  | {
      mode: 'path';
      path: string;
      onPath: (path: string) => void;
      /** Enter or blur in the path box. */
      onCommit: () => void;
      /** The browse button. */
      onBrowse: () => void;
      placeholder: string;
      ariaLabel: string;
    };

/** Session variables as a source: tick any number (the Fit tab) or pick one (the ML lab). */
export type VariableSource =
  | { mode: 'multi'; options: Array<{ name: string; hint: string }>; selected: string[]; onChange: (names: string[]) => void; emptyText: string }
  | { mode: 'single'; options: Array<{ name: string; label: string }>; value: string; onSelect: (name: string) => void; emptyText: string; ariaLabel: string; placeholder: string };

export interface DataSourcePickerProps {
  kind: SourceKind;
  onKind: (kind: SourceKind) => void;
  /** The chooser's choices, in display order. */
  kinds: Array<{ id: SourceKind; label: string }>;
  /** A labelled drop-down (the Fit tab) or a tab strip (the ML lab). */
  chooser: 'select' | 'tabs';
  /** The built-in-example controls (app-specific), shown for the example source. */
  example?: React.ReactNode;
  paste: PasteSource;
  file: FileSource;
  variable: VariableSource;
  /** Shown before the chooser (the Fit tab's data-shape strip). */
  before?: React.ReactNode;
  busy?: boolean;
  /** A plain muted line under the source. */
  note?: string;
  banner?: { kind: 'error' | 'warning'; text: string; compact?: boolean } | null;
}

const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border';

export const DataSourcePicker: React.FC<DataSourcePickerProps> = ({ kind, onKind, kinds, chooser, example, paste, file, variable, before, busy, note, banner }) => {
  const fileInput = useRef<HTMLInputElement | null>(null);
  return (
    <>
      {before}
      {chooser === 'select' ? (
        <label className="flex flex-col gap-1 text-xs">
          <span style={muted}>Data source</span>
          <select value={kind} onChange={(e) => onKind(e.target.value as SourceKind)} className={inputClass} style={{ borderColor: border }}>
            {kinds.map((k) => <option key={k.id} value={k.id}>{k.label}</option>)}
          </select>
        </label>
      ) : (
        <div className="qu-live-tabs" role="tablist" aria-label="Data source" style={{ borderRadius: 8 }}>
          {kinds.map((k) => (
            <button key={k.id} role="tab" aria-selected={kind === k.id} onClick={() => onKind(k.id)}>{k.label}</button>
          ))}
        </div>
      )}

      {kind === 'example' && example}

      {kind === 'paste' && (
        <textarea
          value={paste.value}
          onChange={(e) => paste.onChange(e.target.value)}
          placeholder={paste.placeholder}
          rows={paste.rows}
          className={paste.className}
          style={{ borderColor: border }}
          aria-label={paste.ariaLabel}
          spellCheck={paste.spellCheck}
        />
      )}

      {kind === 'file' && file.mode === 'button' && (
        <>
          <button
            className="qu-side-action w-full"
            onClick={async () => {
              if ((await file.onChoose()) === 'fallback') fileInput.current?.click();
            }}
          >
            <FileUp size={13} />{` ${file.label}`}
          </button>
          <input
            ref={fileInput}
            type="file"
            accept={file.accept}
            className="hidden"
            onChange={async (e) => {
              const f = e.target.files?.[0];
              if (f) file.onBrowserFile(f.name, await f.text());
            }}
          />
        </>
      )}
      {kind === 'file' && file.mode === 'path' && (
        <div className="flex gap-1">
          <input
            value={file.path}
            onChange={(e) => file.onPath(e.target.value)}
            onBlur={file.onCommit}
            onKeyDown={(e) => { if (e.key === 'Enter') file.onCommit(); }}
            placeholder={file.placeholder}
            className="px-2 py-1.5 rounded-md text-sm bg-transparent border min-w-0 flex-1 font-mono text-xs"
            style={{ borderColor: border }}
            aria-label={file.ariaLabel}
          />
          <button className="px-2 rounded-md border" style={{ borderColor: border }} title="Browse" onClick={file.onBrowse}>
            <FolderOpen size={13} />
          </button>
        </div>
      )}

      {kind === 'variable' && variable.mode === 'multi' && (
        <div className="flex flex-col gap-1 text-xs">
          {variable.options.length === 0 && <span style={muted}>{variable.emptyText}</span>}
          {variable.options.map((v) => (
            <label key={v.name} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={variable.selected.includes(v.name)}
                onChange={(e) => variable.onChange(e.target.checked ? [...variable.selected, v.name] : variable.selected.filter((n) => n !== v.name))}
              />
              <span className="font-mono">{v.name}</span>
              <span style={muted}>({v.hint})</span>
            </label>
          ))}
        </div>
      )}
      {kind === 'variable' && variable.mode === 'single' && (
        variable.options.length ? (
          <select value={variable.value} onChange={(e) => variable.onSelect(e.target.value)} className={`${inputClass} min-w-0`} style={{ borderColor: border }} aria-label={variable.ariaLabel}>
            <option value="" disabled>{variable.placeholder}</option>
            {variable.options.map((v) => <option key={v.name} value={v.name}>{v.label}</option>)}
          </select>
        ) : (
          <span className="text-xs" style={muted}>{variable.emptyText}</span>
        )
      )}

      {busy && <span className="text-xs" style={muted}>Reading…</span>}
      {note && <p className="text-xs" style={muted}>{note}</p>}
      {banner && <NoticeBanner kind={banner.kind} text={banner.text} compact={banner.compact} />}
    </>
  );
};

export default DataSourcePicker;
