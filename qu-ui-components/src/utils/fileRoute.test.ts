import { describe, it, expect } from 'vitest';
import {
  normalizePath,
  baseName,
  extOf,
  classifyPath,
  routeDroppedPaths,
  sizeRefusal,
  formatBytes,
  MAX_VIEWER_BYTES,
} from './fileRoute';

describe('normalizePath', () => {
  it('strips the verbatim prefix', () => {
    expect(normalizePath('\\\\?\\C:\\Users\\Ahmed\\my file.qu')).toBe('C:\\Users\\Ahmed\\my file.qu');
  });
  it('maps verbatim UNC back to a UNC path', () => {
    expect(normalizePath('\\\\?\\UNC\\srv\\share\\a.qu')).toBe('\\\\srv\\share\\a.qu');
  });
  it('leaves ordinary paths alone, spaces and unicode included', () => {
    expect(normalizePath('/home/ü/Meine Skripte/ä.qu')).toBe('/home/ü/Meine Skripte/ä.qu');
    expect(normalizePath('C:\\x\\a.qu')).toBe('C:\\x\\a.qu');
  });
});

describe('baseName / extOf', () => {
  it('handles both separators and trailing ones', () => {
    expect(baseName('C:\\a\\b\\c.qu')).toBe('c.qu');
    expect(baseName('/a/b/c.qu')).toBe('c.qu');
    expect(baseName('\\\\?\\C:\\a\\c d.svg')).toBe('c d.svg');
  });
  it('extension is lower-cased; dotfiles have none', () => {
    expect(extOf('X.PDF')).toBe('pdf');
    expect(extOf('/a/.hidden')).toBe('');
    expect(extOf('noext')).toBe('');
  });
});

describe('classifyPath', () => {
  it('routes by extension, case-insensitively', () => {
    expect(classifyPath('a.qu')).toBe('qu');
    expect(classifyPath('A.SVG')).toBe('svg');
    expect(classifyPath('/x/y.Pdf')).toBe('pdf');
    expect(classifyPath('notes.md')).toBe('text');
    expect(classifyPath('a.exe')).toBe('unsupported');
    expect(classifyPath('a.svg.exe')).toBe('unsupported');
  });
});

describe('routeDroppedPaths', () => {
  it('splits accepted and rejected, dedupes, normalises', () => {
    const r = routeDroppedPaths(['\\\\?\\C:\\a\\x.qu', 'C:\\a\\x.qu', 'C:\\a\\pic.svg', 'C:\\a\\virus.exe', 'C:\\a\\d.pdf']);
    expect(r.accepted.map((a) => [a.path, a.kind])).toEqual([
      ['C:\\a\\x.qu', 'qu'],
      ['C:\\a\\pic.svg', 'svg'],
      ['C:\\a\\d.pdf', 'pdf'],
    ]);
    expect(r.rejected).toEqual(['virus.exe']);
  });
});

describe('sizeRefusal', () => {
  it('refuses oversize viewer files with the size in the message', () => {
    expect(sizeRefusal('big.pdf', 'pdf', MAX_VIEWER_BYTES + 1)).toMatch(/big\.pdf is 200\.0 MB/);
    expect(sizeRefusal('ok.pdf', 'pdf', 1024)).toBeNull();
  });
  it('formats bytes', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 KB');
  });
});
