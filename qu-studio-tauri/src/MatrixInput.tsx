import React, { useEffect, useRef, useState } from 'react';
import { AlertCircle, Minus, Plus } from 'lucide-react';
import {
  BUILDERS,
  GRID_LIMIT,
  columnStats,
  describeFlags,
  emptyGrid,
  formatNumber,
  gridOf,
  parseCell,
  parseMatrixText,
  sourceDims,
  sourceFromTable,
  sourceProblem,
  sourceValues,
  type Builder,
  type BuilderKind,
  type Mat,
  type MatrixFlags,
  type MatrixName,
  type MatrixSource,
} from './linalgDesign';

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1 rounded-md text-xs bg-transparent border';

/** A session variable offered as a matrix source (the Code tab's last run). */
export interface SessionVar {
  name: string;
  type?: string;
  shape?: number[];
  data?: (number | null)[];
  truncated?: boolean;
  length?: number;
}

export interface MatrixInputProps {
  name: MatrixName;
  role: string;
  value: MatrixSource;
  onChange: (s: MatrixSource) => void;
  /** Engine-measured facts; null until the first summary run returns. */
  flags: MatrixFlags | null;
  /** The matrix as the engine built it (for "edit as grid" on a builder). */
  resolved: Mat | null;
  running: boolean;
  summaryError: string;
  precision: number;
  /** Numeric variables of the Code tab's last run. */
  sessionVars?: SessionVar[];
  /** Every value of a variable (column-major for a matrix). */
  loadVar?: (name: string) => Promise<(number | null)[]>;
}

type Mode = 'grid' | 'build' | 'paste' | 'file' | 'var';

const MODES: Array<{ id: Mode; label: string }> = [
  { id: 'grid', label: 'Grid' },
  { id: 'build', label: 'Builder' },
  { id: 'paste', label: 'Paste / CSV' },
  { id: 'file', label: 'File' },
  { id: 'var', label: 'Variable' },
];

const modeOf = (v: MatrixSource): Mode => (v.kind === 'grid' ? 'grid' : v.kind === 'builder' ? 'build' : v.origin.startsWith('file:') ? 'file' : v.origin.startsWith('var:') ? 'var' : 'paste');

/** A whole-number field that can be cleared while typing. */
const IntField: React.FC<{ label: string; value: number; min?: number; max?: number; onChange: (n: number) => void }> = ({ label, value, min = 1, max = 5000, onChange }) => {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <label className="flex flex-col gap-0.5 text-xs flex-1 min-w-0">
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        value={draft ?? value}
        onChange={(e) => {
          setDraft(e.target.value);
          const v = Number(e.target.value);
          if (e.target.value.trim() !== '' && Number.isInteger(v)) onChange(v);
        }}
        onBlur={() => setDraft(null)}
        className={inputClass}
        style={{ borderColor: border }}
      />
    </label>
  );
};

const TextField: React.FC<{ label: string; value: string; onChange: (s: string) => void; rows?: number; placeholder?: string }> = ({ label, value, onChange, rows, placeholder }) => (
  <label className="flex flex-col gap-0.5 text-xs flex-1 min-w-0">
    <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
    {rows ? (
      <textarea value={value} onChange={(e) => onChange(e.target.value)} rows={rows} placeholder={placeholder} className={`${inputClass} font-mono`} style={{ borderColor: border }} />
    ) : (
      <input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} className={`${inputClass} font-mono`} style={{ borderColor: border }} />
    )}
  </label>
);

function defaultBuilder(kind: BuilderKind, prev?: Builder): Builder {
  const n = prev && 'n' in prev ? prev.n : 5;
  switch (kind) {
    case 'identity': return { kind, n };
    case 'zeros': return { kind, rows: 3, cols: 3 };
    case 'ones': return { kind, rows: 3, cols: 3 };
    case 'diagonal': return { kind, values: '1, 2, 3' };
    case 'random': return { kind, rows: 4, cols: 4, seed: 1, dist: 'normal' };
    case 'hilbert': return { kind, n: 8 };
    case 'vandermonde': return { kind, points: 20, cols: 4 };
    case 'toeplitz': return { kind, col: '4, -1, 0, 0', row: '' };
    case 'poisson1d': return { kind, n: 100 };
    case 'pattern': return { kind, rows: 5, cols: 5, entries: '0 0 4\n1 1 4\n2 2 4\n3 3 4\n4 4 4\n0 4 -1\n4 0 -1' };
  }
}

const BuilderEditor: React.FC<{ b: Builder; onChange: (b: Builder) => void }> = ({ b, onChange }) => {
  const set = (patch: Partial<Builder>) => onChange({ ...b, ...patch } as Builder);
  switch (b.kind) {
    case 'identity': case 'hilbert': case 'poisson1d':
      return <IntField label="n (size)" value={b.n} onChange={(n) => set({ n })} />;
    case 'zeros': case 'ones':
      return (
        <div className="flex gap-2">
          <IntField label="rows" value={b.rows} onChange={(rows) => set({ rows })} />
          <IntField label="columns" value={b.cols} onChange={(cols) => set({ cols })} />
        </div>
      );
    case 'diagonal':
      return <TextField label="diagonal entries" value={b.values} onChange={(values) => set({ values })} placeholder="1, 2, 3" />;
    case 'random':
      return (
        <div className="flex flex-col gap-2">
          <div className="flex gap-2">
            <IntField label="rows" value={b.rows} onChange={(rows) => set({ rows })} />
            <IntField label="columns" value={b.cols} onChange={(cols) => set({ cols })} />
            <IntField label="seed" value={b.seed} min={0} onChange={(seed) => set({ seed })} />
          </div>
          <label className="flex flex-col gap-0.5 text-xs">
            <span style={{ color: 'var(--qu-muted)' }}>distribution</span>
            <select value={b.dist} onChange={(e) => set({ dist: e.target.value as 'normal' | 'uniform' | 'spd' })} className={inputClass} style={{ borderColor: border }}>
              <option value="normal">normal (randn)</option>
              <option value="uniform">uniform (rand)</option>
              <option value="spd">symmetric positive definite (square, from rows)</option>
            </select>
          </label>
        </div>
      );
    case 'vandermonde':
      return (
        <div className="flex gap-2">
          <IntField label="points on [0, 1]" value={b.points} onChange={(points) => set({ points })} />
          <IntField label="columns (powers)" value={b.cols} onChange={(cols) => set({ cols })} />
        </div>
      );
    case 'toeplitz':
      return (
        <div className="flex flex-col gap-2">
          <TextField label="first column" value={b.col} onChange={(col) => set({ col })} placeholder="4, -1, 0, 0" />
          <TextField label="first row (blank = symmetric)" value={b.row} onChange={(row) => set({ row })} placeholder="4, -1, 0, 0" />
        </div>
      );
    case 'pattern':
      return (
        <div className="flex flex-col gap-2">
          <div className="flex gap-2">
            <IntField label="rows" value={b.rows} onChange={(rows) => set({ rows })} />
            <IntField label="columns" value={b.cols} onChange={(cols) => set({ cols })} />
          </div>
          <TextField label="entries: row col value (0-based), one per line" value={b.entries} onChange={(entries) => set({ entries })} rows={5} />
        </div>
      );
  }
};

/** Cells typed into the grid; arrow keys, Enter and Tab move between cells; a
 *  paste from a spreadsheet or CSV fills the grid from the focused cell. */
const GridEditor: React.FC<{ cells: string[][]; onChange: (cells: string[][]) => void }> = ({ cells, onChange }) => {
  const rows = cells.length;
  const cols = cells[0]?.length ?? 0;
  const refs = useRef<Record<string, HTMLInputElement | null>>({});
  const focus = (i: number, j: number) => refs.current[`${i}:${j}`]?.focus();
  const setCell = (i: number, j: number, v: string) => onChange(cells.map((r, ri) => (ri === i ? r.map((c, ci) => (ci === j ? v : c)) : r)));
  const key = (e: React.KeyboardEvent<HTMLInputElement>, i: number, j: number) => {
    const el = e.currentTarget;
    const atStart = el.selectionStart === 0 && el.selectionEnd === 0;
    const atEnd = el.selectionStart === el.value.length;
    if (e.key === 'ArrowDown' || e.key === 'Enter') { e.preventDefault(); focus(Math.min(rows - 1, i + 1), j); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); focus(Math.max(0, i - 1), j); }
    else if (e.key === 'ArrowRight' && atEnd) { e.preventDefault(); focus(i, Math.min(cols - 1, j + 1)); }
    else if (e.key === 'ArrowLeft' && atStart) { e.preventDefault(); focus(i, Math.max(0, j - 1)); }
  };
  const paste = (e: React.ClipboardEvent<HTMLInputElement>, i: number, j: number) => {
    const text = e.clipboardData.getData('text');
    if (!/[\t\n,;]/.test(text.trim())) return; // a single value: let the input take it
    e.preventDefault();
    const t = parseMatrixText(text);
    if (!t.rows) return;
    const needR = Math.min(GRID_LIMIT, Math.max(rows, i + t.rows));
    const needC = Math.min(GRID_LIMIT, Math.max(cols, j + t.cols));
    const next = Array.from({ length: needR }, (_, r) => Array.from({ length: needC }, (_, c) => cells[r]?.[c] ?? '0'));
    for (let r = 0; r < t.rows && i + r < needR; r++) for (let c = 0; c < t.cols && j + c < needC; c++) {
      const v = t.values[r][c];
      next[i + r][j + c] = Number.isFinite(v) ? String(v) : '0';
    }
    onChange(next);
  };
  const resize = (dr: number, dc: number) => {
    const nr = Math.max(1, Math.min(GRID_LIMIT, rows + dr));
    const nc = Math.max(1, Math.min(GRID_LIMIT, cols + dc));
    onChange(Array.from({ length: nr }, (_, r) => Array.from({ length: nc }, (_, c) => cells[r]?.[c] ?? '0')));
  };
  const btn = 'inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded border text-xs';
  return (
    <div>
      <div className="overflow-auto rounded-md border" style={{ borderColor: border, maxHeight: 300 }}>
        <table style={{ borderCollapse: 'collapse' }}>
          <tbody>
            {cells.map((r, i) => (
              <tr key={i}>
                {r.map((c, j) => (
                  <td key={j} style={{ padding: 0 }}>
                    <input
                      ref={(el) => { refs.current[`${i}:${j}`] = el; }}
                      value={c}
                      onChange={(e) => setCell(i, j, e.target.value)}
                      onKeyDown={(e) => key(e, i, j)}
                      onPaste={(e) => paste(e, i, j)}
                      onFocus={(e) => e.currentTarget.select()}
                      aria-label={`row ${i + 1}, column ${j + 1}`}
                      className="font-mono text-xs text-right bg-transparent"
                      style={{ width: '4.4rem', padding: '3px 5px', border: 0, borderBottom: `1px solid ${border}`, borderRight: `1px solid ${border}`, outline: 'none', color: parseCell(c) === null ? 'var(--qu-danger)' : 'var(--qu-text)' }}
                    />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex flex-wrap items-center gap-1.5 mt-1" style={{ color: 'var(--qu-muted)' }}>
        <span className="text-xs">{rows} x {cols}</span>
        <button className={btn} style={{ borderColor: border }} onClick={() => resize(1, 0)} disabled={rows >= GRID_LIMIT} title="Add a row"><Plus size={10} /> row</button>
        <button className={btn} style={{ borderColor: border }} onClick={() => resize(-1, 0)} disabled={rows <= 1} title="Remove the last row"><Minus size={10} /> row</button>
        <button className={btn} style={{ borderColor: border }} onClick={() => resize(0, 1)} disabled={cols >= GRID_LIMIT} title="Add a column"><Plus size={10} /> col</button>
        <button className={btn} style={{ borderColor: border }} onClick={() => resize(0, -1)} disabled={cols <= 1} title="Remove the last column"><Minus size={10} /> col</button>
        <span className="text-xs">arrows move, paste a spreadsheet block to fill</span>
      </div>
    </div>
  );
};

/** What a pasted/file/variable table turned into, with its parse problems. */
const TableReport: React.FC<{ src: Extract<MatrixSource, { kind: 'data' }>; problems: string[] }> = ({ src, problems }) => {
  const stats = columnStats(src);
  const shown = Math.min(src.rows, 6);
  const cols = Math.min(src.cols, 8);
  return (
    <div className="text-xs">
      <div style={{ color: 'var(--qu-muted)' }}>
        {src.rows} x {src.cols}{src.header ? ', header row detected' : ''}; first {shown} rows{src.cols > cols ? ` and ${cols} columns` : ''}:
      </div>
      <div className="overflow-auto rounded-md border mt-1" style={{ borderColor: border }}>
        <table className="font-mono" style={{ borderCollapse: 'collapse' }}>
          <thead>
            <tr>{stats.slice(0, cols).map((s, j) => <th key={j} className="px-2 py-0.5 text-right font-medium" style={{ borderBottom: `1px solid ${border}` }}>{s.name}</th>)}</tr>
          </thead>
          <tbody>
            {Array.from({ length: shown }, (_, i) => (
              <tr key={i}>{stats.slice(0, cols).map((_s, j) => <td key={j} className="px-2 py-0.5 text-right">{formatNumber(src.values[i * src.cols + j], 5)}</td>)}</tr>
            ))}
            <tr style={{ color: 'var(--qu-muted)' }}>{stats.slice(0, cols).map((s, j) => <td key={j} className="px-2 py-0.5 text-right" title="min / mean / max">{formatNumber(s.min, 3)}..{formatNumber(s.max, 3)}</td>)}</tr>
          </tbody>
        </table>
      </div>
      {problems.length > 0 && (
        <div className="mt-1" style={{ color: 'var(--qu-danger)' }}>
          {problems.slice(0, 4).map((p, i) => <div key={i}>{p}</div>)}
          {problems.length > 4 && <div>...and {problems.length - 4} more.</div>}
        </div>
      )}
    </div>
  );
};

export const MatrixInput: React.FC<MatrixInputProps> = ({ name, role, value, onChange, flags, resolved, running, summaryError, precision, sessionVars = [], loadVar }) => {
  const [mode, setMode] = useState<Mode>(modeOf(value));
  const [text, setText] = useState('');
  const [problems, setProblems] = useState<string[]>([]);
  const [fileName, setFileName] = useState('');
  const [busy, setBusy] = useState('');
  const [chosenVar, setChosenVar] = useState('');
  useEffect(() => {
    // a value replaced from outside (an example) moves the tab with it
    setMode(modeOf(value));
  }, [value.kind]); // eslint-disable-line react-hooks/exhaustive-deps

  const problem = sourceProblem(name, value);
  const dims = sourceDims(value);

  const useText = (t: string, origin: string) => {
    const parsed = parseMatrixText(t);
    setProblems(parsed.problems);
    if (!parsed.rows || parsed.problems.length) return; // fix the problems listed below, then try again
    onChange(sourceFromTable(parsed, origin));
  };

  const chooseFile = async () => {
    setBusy('Reading the file...');
    try {
      const { open } = await import('@tauri-apps/api/dialog');
      const { readTextFile } = await import('@tauri-apps/api/fs');
      const r = await open({ multiple: false, filters: [{ name: 'CSV / text', extensions: ['csv', 'txt', 'tsv', 'dat'] }] });
      if (typeof r !== 'string') return;
      const content = await readTextFile(r);
      setFileName(r.split(/[\\/]/).pop() ?? r);
      useText(content, `file:${r}`);
    } catch (err) {
      setProblems([`Could not read the file: ${err}. (Choosing a file needs the desktop app.)`]);
    } finally {
      setBusy('');
    }
  };

  const useVariable = async (v: SessionVar) => {
    setBusy(`Loading ${v.name}...`);
    try {
      const shape = v.shape && v.shape.length === 2 ? v.shape : [v.length ?? v.data?.length ?? 0, 1];
      const full = !v.truncated && v.data ? v.data : loadVar ? await loadVar(v.name) : null;
      if (!full) throw new Error('the values are not available');
      const [r, c] = shape;
      if (full.length !== r * c) throw new Error(`expected ${r * c} values, got ${full.length}`);
      const values: number[] = [];
      for (let i = 0; i < r; i++) for (let j = 0; j < c; j++) {
        const x = full[i + j * r];
        values.push(x == null ? NaN : x);
      }
      if (values.some((x) => Number.isNaN(x))) throw new Error('it holds NaN or infinite entries');
      const rowsArr = Array.from({ length: r }, (_, i) => values.slice(i * c, (i + 1) * c));
      onChange(sourceFromTable({ rows: r, cols: c, values: rowsArr, header: null, problems: [] }, `var:${v.name}`));
      setProblems([]);
    } catch (err) {
      setProblems([`Could not use ${v.name}: ${err}`]);
    } finally {
      setBusy('');
    }
  };

  const toGrid = () => {
    const vals = value.kind === 'builder' ? sourceValues(value) ?? resolved?.re : null;
    if (vals && vals.length <= GRID_LIMIT && vals[0].length <= GRID_LIMIT) onChange(gridOf(vals));
  };
  const canGrid = value.kind === 'builder' && dims && dims.rows <= GRID_LIMIT && dims.cols <= GRID_LIMIT && (sourceValues(value) || resolved);

  const switchMode = (m: Mode) => {
    setMode(m);
    if (m === 'grid' && value.kind !== 'grid') {
      const vals = sourceValues(value) ?? resolved?.re;
      if (vals && vals.length <= GRID_LIMIT && vals[0].length <= GRID_LIMIT) onChange(gridOf(vals));
      else if (!dims || dims.rows > GRID_LIMIT || dims.cols > GRID_LIMIT) onChange(emptyGrid(3, 3));
    }
    if (m === 'build' && value.kind !== 'builder') onChange({ kind: 'builder', builder: defaultBuilder('identity') });
  };

  return (
    <div className="rounded-lg border p-2 flex flex-col gap-2" style={{ borderColor: border }}>
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium">{role}</span>
        {running && <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>measuring...</span>}
      </div>
      <div className="flex flex-wrap gap-1" role="tablist" aria-label={`${name} source`}>
        {MODES.map((m) => (
          <button
            key={m.id}
            role="tab"
            aria-selected={mode === m.id}
            onClick={() => switchMode(m.id)}
            className="px-2 py-0.5 rounded-md text-xs border"
            style={{ borderColor: mode === m.id ? 'var(--qu-accent)' : border, color: mode === m.id ? 'var(--qu-accent)' : 'var(--qu-muted)' }}
          >
            {m.label}
          </button>
        ))}
      </div>

      {mode === 'grid' && value.kind === 'grid' && <GridEditor cells={value.cells} onChange={(cells) => onChange({ kind: 'grid', cells })} />}
      {mode === 'grid' && value.kind !== 'grid' && (
        <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          This matrix is {dims ? `${dims.rows} x ${dims.cols}` : 'not defined yet'}, too large for the grid.{' '}
          {canGrid && <button className="underline" onClick={toGrid}>Edit as a grid</button>}
        </div>
      )}

      {mode === 'build' && value.kind === 'builder' && (
        <div className="flex flex-col gap-2">
          <label className="flex flex-col gap-0.5 text-xs">
            <span style={{ color: 'var(--qu-muted)' }}>builder</span>
            <select
              value={value.builder.kind}
              onChange={(e) => onChange({ kind: 'builder', builder: defaultBuilder(e.target.value as BuilderKind, value.builder) })}
              className={inputClass}
              style={{ borderColor: border }}
            >
              {BUILDERS.map((b) => <option key={b.id} value={b.id}>{b.label}</option>)}
            </select>
          </label>
          <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>{BUILDERS.find((b) => b.id === value.builder.kind)!.note}</div>
          <BuilderEditor b={value.builder} onChange={(builder) => onChange({ kind: 'builder', builder })} />
          {canGrid && <button className="text-xs underline text-left" onClick={toGrid}>Convert to an editable grid</button>}
        </div>
      )}
      {mode === 'build' && value.kind !== 'builder' && <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>Pick the Builder tab again to choose one.</div>}

      {mode === 'paste' && (
        <div className="flex flex-col gap-1.5">
          <TextField label="paste numbers (spreadsheet, CSV, or one row per line)" value={text} onChange={setText} rows={5} placeholder={'4 1 0\n1 3 1\n0 1 2'} />
          <button className="qu-side-action w-full" disabled={!text.trim()} onClick={() => useText(text, 'paste')}>Use these numbers</button>
        </div>
      )}
      {mode === 'file' && (
        <div className="flex flex-col gap-1.5">
          <button className="qu-side-action w-full" onClick={() => void chooseFile()}>Choose a CSV file...</button>
          <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>{fileName ? `Read ${fileName}. A header line is detected and skipped.` : 'Numbers only; comma, semicolon, tab or space separated.'}</div>
        </div>
      )}
      {mode === 'var' && (
        <div className="flex flex-col gap-1.5">
          {sessionVars.length === 0 ? (
            <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>No numeric variables yet: run a script in the Code tab that creates a matrix or vector.</div>
          ) : (
            <>
              <select value={chosenVar} onChange={(e) => setChosenVar(e.target.value)} className={inputClass} style={{ borderColor: border }}>
                <option value="">Pick a variable...</option>
                {sessionVars.map((v) => <option key={v.name} value={v.name}>{v.name} ({v.shape && v.shape.length === 2 ? `${v.shape[0]} x ${v.shape[1]}` : v.length ?? v.data?.length ?? '?'})</option>)}
              </select>
              <button className="qu-side-action w-full" disabled={!chosenVar} onClick={() => { const v = sessionVars.find((x) => x.name === chosenVar); if (v) void useVariable(v); }}>Use this variable</button>
            </>
          )}
        </div>
      )}

      {busy && <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>{busy}</div>}
      {value.kind === 'data' && <TableReport src={value} problems={problems} />}
      {value.kind !== 'data' && problems.length > 0 && (
        <div className="text-xs" style={{ color: 'var(--qu-danger)' }}>{problems.slice(0, 4).map((p, i) => <div key={i}>{p}</div>)}</div>
      )}
      {problem && (
        <div className="text-xs flex items-start gap-1" style={{ color: 'var(--qu-danger)' }}>
          <AlertCircle size={12} className="flex-shrink-0 mt-0.5" /> <span>{problem}</span>
        </div>
      )}
      {summaryError && !problem && <div className="text-xs" style={{ color: 'var(--qu-danger)' }}>{summaryError}</div>}
      {flags && !problem && (
        <div className="rounded-md text-xs" style={{ background: 'var(--qu-hover)' }} title="Measured by Qu, not guessed">
          {describeFlags(flags, Math.min(precision, 4)).map(([k, v]) => (
            <div key={k} className="flex justify-between gap-3 px-2 py-0.5">
              <span style={{ color: 'var(--qu-muted)' }}>{k}</span>
              <span className="font-mono text-right">{v}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

export default MatrixInput;
