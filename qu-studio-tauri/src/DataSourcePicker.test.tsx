// Render tests for the shared source picker (server-side render, no DOM) and
// the two Tauri helpers it carries, with the Tauri modules stubbed.
import React from 'react';
import { renderToString } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const written: Array<{ file: string; text: string }> = [];
const made: string[] = [];
let picked: unknown = '/data/t.csv';
vi.mock('@tauri-apps/api/path', () => ({
  appDataDir: async () => '/appdata',
  appCacheDir: async () => '/appcache',
  join: async (...p: string[]) => p.join('/'),
}));
vi.mock('@tauri-apps/api/fs', () => ({
  createDir: async (d: string) => { made.push(d); },
  writeTextFile: async (file: string, text: string) => { written.push({ file, text }); },
}));
vi.mock('@tauri-apps/api/dialog', () => ({ open: async () => picked }));

import { DataSourcePicker, NoticeBanner, PreviewTable, pickTableFile, stageTextFile, type DataSourcePickerProps, type SourceKind } from './DataSourcePicker';

const KINDS: Array<{ id: SourceKind; label: string }> = [
  { id: 'example', label: 'Built-in example' },
  { id: 'paste', label: 'Paste text' },
  { id: 'file', label: 'CSV file' },
  { id: 'variable', label: 'Session variable' },
];
const noop = () => undefined;

const props = (over: Partial<DataSourcePickerProps> = {}): DataSourcePickerProps => ({
  kind: 'example',
  onKind: noop,
  kinds: KINDS,
  chooser: 'select',
  example: <span>EXAMPLE-BODY</span>,
  paste: { value: 'x,y\n1,2', onChange: noop, placeholder: 'PASTE-HERE', rows: 4, className: 'pasteclass' },
  file: { mode: 'button', label: 'Choose a CSV file...', accept: '.csv', onChoose: async () => 'handled', onBrowserFile: noop },
  variable: { mode: 'multi', options: [{ name: 'sigA', hint: '40' }, { name: 'sigB', hint: '50' }], selected: ['sigA'], onChange: noop, emptyText: 'NO-VARS' },
  ...over,
});
const render = (over: Partial<DataSourcePickerProps> = {}) => renderToString(<DataSourcePicker {...props(over)} />);

describe('DataSourcePicker chooser', () => {
  it('a select lists every source and marks the current one', () => {
    const html = render({ kind: 'paste' });
    expect(html).toContain('Data source');
    for (const k of KINDS) expect(html).toContain(k.label);
    expect(html).toMatch(/<option value="paste" selected/);
    expect(html).not.toContain('role="tablist"');
  });

  it('tabs mark the current source selected', () => {
    const html = render({ chooser: 'tabs', kind: 'file' });
    expect(html).toContain('role="tablist"');
    expect(html).toMatch(/aria-selected="true"[^>]*>CSV file</);
    expect(html).toMatch(/aria-selected="false"[^>]*>Paste text</);
  });

  it('what comes before the chooser is drawn first', () => {
    const html = render({ before: <i>BEFORE-STRIP</i> });
    expect(html.indexOf('BEFORE-STRIP')).toBeLessThan(html.indexOf('Data source'));
  });
});

describe('DataSourcePicker sources', () => {
  it('example shows only the app-supplied controls', () => {
    const html = render({ kind: 'example' });
    expect(html).toContain('EXAMPLE-BODY');
    expect(html).not.toContain('PASTE-HERE');
  });

  it('paste shows the textarea with its text and class', () => {
    const html = render({ kind: 'paste' });
    expect(html).toContain('PASTE-HERE');
    expect(html).toContain('pasteclass');
    expect(html).toContain('x,y\n1,2');
    expect(html).not.toContain('EXAMPLE-BODY');
  });

  it('file as a button draws the button and the hidden browser input', () => {
    const html = render({ kind: 'file' });
    expect(html).toContain('Choose a CSV file...');
    expect(html).toContain('type="file"');
    expect(html).toContain('accept=".csv"');
  });

  it('file as a path draws the path box with the typed path and a browse button', () => {
    const html = render({
      kind: 'file',
      file: { mode: 'path', path: 'C:/d/t.csv', onPath: noop, onCommit: noop, onBrowse: noop, placeholder: 'PATH-HINT', ariaLabel: 'CSV path' },
    });
    expect(html).toContain('value="C:/d/t.csv"');
    expect(html).toContain('placeholder="PATH-HINT"');
    expect(html).toContain('title="Browse"');
    expect(html).not.toContain('type="file"');
  });

  it('variables as checkboxes: ticked ones are checked, with their sizes', () => {
    const html = render({ kind: 'variable' });
    expect(html).toMatch(/checked=""[^>]*\/><span class="font-mono">sigA/);
    expect(html).toContain('sigB');
    expect(html).toContain('(<!-- -->50<!-- -->)');
  });

  it('no variables says why, in either style', () => {
    expect(render({ kind: 'variable', variable: { mode: 'multi', options: [], selected: [], onChange: noop, emptyText: 'NO-VARS' } })).toContain('NO-VARS');
    expect(render({ kind: 'variable', variable: { mode: 'single', options: [], value: '', onSelect: noop, emptyText: 'NO-VARS-2', ariaLabel: 'v', placeholder: 'pick' } })).toContain('NO-VARS-2');
  });

  it('variables as one drop-down', () => {
    const html = render({
      kind: 'variable',
      variable: { mode: 'single', options: [{ name: 'M', label: 'M (3 x 2)' }], value: 'M', onSelect: noop, emptyText: '', ariaLabel: 'Session variable', placeholder: 'Choose...' },
    });
    expect(html).toContain('aria-label="Session variable"');
    expect(html).toContain('M (3 x 2)');
    expect(html).toContain('Choose...');
  });
});

describe('DataSourcePicker notices', () => {
  it('shows the busy line, the muted note and the banner when given', () => {
    const html = render({ busy: true, note: 'Read f.csv', banner: { kind: 'error', text: 'Column x is bad' } });
    expect(html).toContain('Reading…');
    expect(html).toContain('Read f.csv');
    expect(html).toContain('data-kind="error"');
    expect(html).toContain('Column x is bad');
  });

  it('shows none of them by default', () => {
    const html = render();
    expect(html).not.toContain('Reading…');
    expect(html).not.toContain('qu-banner');
  });

  it('a banner can be a warning', () => {
    expect(renderToString(<NoticeBanner kind="warning" text="careful" />)).toContain('data-kind="warning"');
  });
});

describe('PreviewTable', () => {
  it('draws the header, the rows and the footer lines', () => {
    const html = renderToString(<PreviewTable columns={['x', 'y']} rows={[['1', '2'], ['3', '4']]} footer={[['n=2', 'n=2']]} testId="pv" />);
    expect(html).toContain('data-testid="pv"');
    expect(html).toContain('>x</th>');
    expect(html).toContain('>4</td>');
    expect(html).toContain('>n=2</td>');
  });

  it('the dense look is the compact one', () => {
    expect(renderToString(<PreviewTable dense columns={['a']} rows={[['1']]} />)).toContain('text-[11px]');
    expect(renderToString(<PreviewTable columns={['a']} rows={[['1']]} />)).not.toContain('text-[11px]');
  });
});

describe('Tauri helpers', () => {
  beforeEach(() => {
    written.length = 0;
    made.length = 0;
    picked = '/data/t.csv';
  });

  it('stageTextFile writes under the app data dir and returns the path', async () => {
    const path = await stageTextFile('a,b\n1,2', { base: 'data', subdir: 'ml-lab', file: 'data.csv' });
    expect(path).toBe('/appdata/ml-lab/data.csv');
    expect(made).toEqual(['/appdata/ml-lab']);
    expect(written).toEqual([{ file: '/appdata/ml-lab/data.csv', text: 'a,b\n1,2' }]);
  });

  it('stageTextFile can use the cache dir with no subdirectory', async () => {
    expect(await stageTextFile('x', { base: 'cache', file: 'curve-fit-data.csv' })).toBe('/appcache/curve-fit-data.csv');
    expect(made).toEqual(['/appcache']);
  });

  it('pickTableFile returns the chosen path, or null on cancel', async () => {
    expect(await pickTableFile('Tables', ['csv'])).toBe('/data/t.csv');
    picked = ['/data/first.csv', '/data/second.csv'];
    expect(await pickTableFile('Tables', ['csv'])).toBe('/data/first.csv');
    picked = null;
    expect(await pickTableFile('Tables', ['csv'])).toBeNull();
  });
});
