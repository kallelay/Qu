import React, { useEffect, useRef, useState } from 'react';
import {
  EXAMPLES,
  canonicalCsv,
  exampleById,
  exampleMeta,
  forFile,
  matrixToCsv,
  parseTable,
  withType,
  type ColType,
  type ParsedTable,
  type TableMeta,
} from './mlLabData';
import type { SourceSpec } from './mlLabDesign';
import { INLINE_ROW_LIMIT, needsStaging } from './dataTable';
import { DataSourcePicker, PreviewTable, pickTableFile, stageTextFile } from './DataSourcePicker';

/** A numeric variable of the Code tab's session (a matrix is read column-major). */
export interface SessionMatrix {
  name: string;
  type: string;
  shape: number[];
  data: (number | null)[];
  /** Only a preview was sent: the full values were not loaded. */
  truncated?: boolean;
}

export interface DataInputProps {
  source: SourceSpec;
  meta: TableMeta;
  /** The parsed rows behind `meta` (null for a built-in example). */
  parsed: ParsedTable | null;
  /** Numeric variables of the session, offered as a source. */
  variables?: SessionMatrix[];
  /** A new table was loaded. */
  onLoad: (source: SourceSpec, meta: TableMeta, parsed: ParsedTable | null) => void;
  /** The user changed a column's type. */
  onMeta: (meta: TableMeta) => void;
}

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border min-w-0';
const KINDS: Array<{ id: SourceSpec['kind']; label: string }> = [
  { id: 'example', label: 'Example' },
  { id: 'file', label: 'CSV file' },
  { id: 'variable', label: 'Variable' },
  { id: 'paste', label: 'Paste' },
];

const baseSource = (kind: SourceSpec['kind']): SourceSpec => ({ kind, exampleId: '', path: '', varName: '', csv: '', csvRows: 0, tempPath: '' });

/** Writes big data to a temp file the script can read_csv. '' when that is not possible here. */
const writeTemp = (text: string): Promise<string> => stageTextFile(text, { base: 'data', subdir: 'ml-lab', file: 'data.csv' }).catch(() => '');

/** The native file dialog; null when cancelled or unavailable. */
const pickCsv = (): Promise<string | null> => pickTableFile('Table', ['csv', 'tsv', 'txt']).catch(() => null);

const fmt = (v: number | null): string => (v === null ? '' : String(Number(v.toPrecision(4))));

export const DataInput: React.FC<DataInputProps> = ({ source, meta, parsed, variables = [], onLoad, onMeta }) => {
  const [pasteText, setPasteText] = useState('');
  const [filePath, setFilePath] = useState(source.kind === 'file' ? source.path : '');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);

  const usable = variables.filter((v) => (v.type === 'matrix' || v.type === 'vector') && v.shape[0] >= 2);

  /** Parses `text`, stores big tables in a temp file and reports the result. */
  const ingest = async (kind: 'paste' | 'variable', text: string, extra: Partial<SourceSpec>) => {
    const mine = ++seq.current;
    const p = parseTable(text);
    if (!p.meta.columns.length) {
      setNote(p.problems.join(' '));
      return;
    }
    const { text: csv, warnings } = canonicalCsv(p);
    let tempPath = '';
    if (needsStaging(p.meta.rows)) {
      setBusy(true);
      tempPath = await writeTemp(csv);
      setBusy(false);
      if (mine !== seq.current) return;
    }
    setNote([...p.problems, ...warnings, ...(needsStaging(p.meta.rows) && !tempPath ? ['Could not write the temp file the script needs for more than ' + INLINE_ROW_LIMIT + ' rows.'] : [])].join(' '));
    onLoad({ ...baseSource(kind), ...extra, csv, csvRows: p.meta.rows, tempPath }, p.meta, p);
  };

  const loadFile = async (path: string) => {
    if (!path.trim()) return;
    const mine = ++seq.current;
    setBusy(true);
    try {
      const { readTextFile } = await import('@tauri-apps/api/fs');
      const text = await readTextFile(path);
      if (mine !== seq.current) return;
      const p = parseTable(text);
      setNote(p.problems.join(' '));
      if (!p.meta.columns.length) return;
      const meta2 = forFile(p);
      onLoad({ ...baseSource('file'), path }, meta2, { ...p, meta: meta2 });
    } catch (err) {
      if (mine === seq.current) setNote(`Could not read ${path}: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const loadExample = (id: string) => {
    const ex = exampleById(id);
    if (!ex) return;
    setNote('');
    onLoad({ ...baseSource('example'), exampleId: id }, exampleMeta(ex), null);
  };

  const loadVariable = (name: string) => {
    const v = usable.find((x) => x.name === name);
    if (!v) return;
    if (v.truncated) {
      setNote(`${name} was sent as a preview only; load its full values in the Variables panel first.`);
      return;
    }
    void ingest('variable', matrixToCsv(v.shape, v.data, 'x'), { varName: name });
  };

  // A restored file source has no parsed rows yet: read it again.
  useEffect(() => {
    if (source.kind === 'file' && source.path && !parsed) void loadFile(source.path);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounced paste parsing.
  useEffect(() => {
    if (source.kind !== 'paste' || !pasteText.trim()) return;
    const id = window.setTimeout(() => void ingest('paste', pasteText, {}), 350);
    return () => window.clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pasteText]);

  const switchKind = (kind: SourceSpec['kind']) => {
    setNote('');
    if (kind === 'example') loadExample(source.exampleId || EXAMPLES[0].id);
    else if (kind === 'file') {
      if (filePath) void loadFile(filePath);
      else onLoad({ ...baseSource('file') }, { ...meta, columns: [], rows: 0 }, null);
    } else if (kind === 'paste') {
      if (pasteText.trim()) void ingest('paste', pasteText, {});
      else onLoad(baseSource('paste'), { ...meta, columns: [], rows: 0 }, null);
    } else {
      onLoad(baseSource('variable'), { ...meta, columns: [], rows: 0 }, null);
      if (usable.length) loadVariable(source.varName || usable[0].name);
    }
  };

  const setType = (name: string, type: ColType) => {
    onMeta(parsed ? withType(parsed, meta, name, type) : { ...meta, columns: meta.columns.map((c) => (c.name === name ? { ...c, type } : c)) });
  };

  const ex = source.kind === 'example' ? exampleById(source.exampleId) : undefined;

  return (
    <div className="flex flex-col gap-2" data-testid="ml-data">
      <DataSourcePicker
        chooser="tabs"
        kinds={KINDS}
        kind={source.kind}
        onKind={switchKind}
        example={
          <div className="flex flex-col gap-1">
            <select value={source.exampleId} onChange={(e) => loadExample(e.target.value)} className={inputClass} style={{ borderColor: border }} aria-label="Built-in example">
              {EXAMPLES.map((e) => <option key={e.id} value={e.id}>{e.label}</option>)}
            </select>
            {ex && <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>{ex.blurb} Generated by Qu code in the script, seeded: the same table every run.</span>}
          </div>
        }
        paste={{
          value: pasteText,
          onChange: setPasteText,
          placeholder: 'Paste CSV or tab-separated text with a header line:\nheight, weight, species\n1.2, 3.4, a',
          rows: 5,
          className: `${inputClass} font-mono text-xs w-full`,
          ariaLabel: 'Pasted data',
        }}
        file={{
          mode: 'path',
          path: filePath,
          onPath: setFilePath,
          onCommit: () => void loadFile(filePath),
          onBrowse: async () => {
            const p = await pickCsv();
            if (p) {
              setFilePath(p);
              void loadFile(p);
            }
          },
          placeholder: 'C:/data/table.csv (a path from the Files tab works too)',
          ariaLabel: 'CSV path',
        }}
        variable={{
          mode: 'single',
          options: usable.map((v) => ({ name: v.name, label: `${v.name} (${v.shape.join(' x ')})` })),
          value: source.varName,
          onSelect: loadVariable,
          emptyText: "No numeric matrix or vector in the Code tab's session yet: run a script there that defines one (columns become x1, x2...).",
          ariaLabel: 'Session variable',
          placeholder: 'Choose a matrix or vector…',
        }}
        busy={busy}
        banner={note ? { kind: 'warning', text: note } : null}
      />

      {meta.columns.length > 0 && (
        <>
          <div className="text-xs" style={{ color: 'var(--qu-muted)' }} data-testid="ml-data-summary">
            {meta.rows} rows, {meta.columns.length} columns
            {source.kind === 'file' && meta.positional ? ' (header names are not valid Qu identifiers: read by position)' : ''}
            {parsed && meta.sep !== ',' ? `, separator "${meta.sep === '\t' ? 'tab' : meta.sep}"` : ''}
            {parsed && meta.decimal === ',' ? ', decimal comma' : ''}
          </div>
          <div className="overflow-x-auto rounded border" style={{ borderColor: border }}>
            <table className="text-[11px] w-full" style={{ borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ color: 'var(--qu-muted)' }} className="text-left">
                  <th className="font-normal px-1 py-1">column</th>
                  <th className="font-normal px-1">type</th>
                  <th className="font-normal px-1">missing</th>
                  <th className="font-normal px-1">summary</th>
                </tr>
              </thead>
              <tbody>
                {meta.columns.map((c) => (
                  <tr key={c.name} style={{ borderTop: `1px solid ${border}` }}>
                    <td className="px-1 py-0.5 font-mono max-w-[7rem] truncate" title={c.name}>{c.name}</td>
                    <td className="px-1">
                      <select value={c.type} onChange={(e) => setType(c.name, e.target.value as ColType)} className="bg-transparent border rounded px-1" style={{ borderColor: border }} aria-label={`type of ${c.name}`}>
                        <option value="numeric">numeric</option>
                        <option value="category">category</option>
                        <option value="ignore">ignore</option>
                      </select>
                    </td>
                    <td className="px-1 font-mono">{parsed ? c.missing : ''}</td>
                    <td className="px-1 font-mono" style={{ color: 'var(--qu-muted)' }}>
                      {c.type === 'numeric' && c.mean !== null ? `${fmt(c.min)} .. ${fmt(c.max)}, mean ${fmt(c.mean)}` : c.type === 'numeric' ? 'number' : c.levels.length ? `${c.levels.slice(0, 4).join(', ')}${c.levels.length > 4 ? ', …' : ''} (${c.levels.length})` : c.type === 'ignore' ? 'not used' : ''}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {parsed && parsed.head.length > 0 && (
            <PreviewTable dense testId="ml-data-preview" columns={meta.columns.map((c) => c.name)} rows={parsed.head.map((r) => meta.columns.map((c) => r[c.index] ?? ''))} />
          )}
        </>
      )}
    </div>
  );
};

export default DataInput;
