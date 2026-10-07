/**
 * Pure helpers for "somebody handed Studio a file" -- from the command line
 * (`Qu Studio.exe "C:\x\a.qu"`), from an OS file drop, or from the file
 * tree. No DOM, no Tauri: everything here is unit-tested in node.
 */

export type FileKind = 'qu' | 'svg' | 'pdf' | 'text' | 'unsupported';

/** Refuse to read anything larger than this into the webview (bytes). */
export const MAX_VIEWER_BYTES = 200 * 1024 * 1024;
/** Source files above this are refused too -- Monaco chokes long before. */
export const MAX_TEXT_BYTES = 50 * 1024 * 1024;

const QU_EXT = new Set(['qu']);
// Plain-text files Studio will open in the editor as-is (a drop of a README
// or a CSV is a reasonable thing to want to look at). Anything else is
// refused with a toast naming the file rather than guessed at.
const TEXT_EXT = new Set(['txt', 'md', 'csv', 'tsv', 'json', 'toml', 'log', 'yaml', 'yml']);

/**
 * Strips the Windows verbatim prefix `std::fs::canonicalize` produces:
 * `\\?\C:\dir\a.qu` -> `C:\dir\a.qu`, `\\?\UNC\srv\share\a.qu` ->
 * `\\srv\share\a.qu`. Other paths pass through untouched.
 */
export function normalizePath(p: string): string {
  if (p.startsWith('\\\\?\\UNC\\')) return '\\\\' + p.slice(8);
  if (p.startsWith('\\\\?\\')) return p.slice(4);
  // Tauri/WebView2 sometimes reports the same prefix with forward slashes.
  if (p.startsWith('//?/UNC/')) return '//' + p.slice(8);
  if (p.startsWith('//?/')) return p.slice(4);
  return p;
}

/** Final path component, for either separator. */
export function baseName(p: string): string {
  const n = normalizePath(p).replace(/[\\/]+$/, '');
  const i = Math.max(n.lastIndexOf('/'), n.lastIndexOf('\\'));
  return i >= 0 ? n.slice(i + 1) : n;
}

/** Lower-cased extension without the dot; '' when there is none. */
export function extOf(p: string): string {
  const name = baseName(p);
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

/** The file-type router: which viewer/editor does this path belong in. */
export function classifyPath(p: string): FileKind {
  const ext = extOf(p);
  if (QU_EXT.has(ext)) return 'qu';
  if (ext === 'svg') return 'svg';
  if (ext === 'pdf') return 'pdf';
  if (TEXT_EXT.has(ext)) return 'text';
  return 'unsupported';
}

/**
 * Splits a drop's paths into what Studio can open and what it must refuse
 * (the caller toasts the refused names). Order is preserved and duplicates
 * (same normalised path) are dropped.
 */
export function routeDroppedPaths(paths: string[]): {
  accepted: { path: string; kind: Exclude<FileKind, 'unsupported'> }[];
  rejected: string[];
} {
  const accepted: { path: string; kind: Exclude<FileKind, 'unsupported'> }[] = [];
  const rejected: string[] = [];
  const seen = new Set<string>();
  for (const raw of paths) {
    const path = normalizePath(raw);
    if (seen.has(path)) continue;
    seen.add(path);
    const kind = classifyPath(path);
    if (kind === 'unsupported') rejected.push(baseName(path));
    else accepted.push({ path, kind });
  }
  return { accepted, rejected };
}

/** Human-readable byte count for refusal messages. */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Null when `size` is acceptable for `kind`, else the refusal message. */
export function sizeRefusal(name: string, kind: FileKind, size: number): string | null {
  const limit = kind === 'svg' || kind === 'pdf' ? MAX_VIEWER_BYTES : MAX_TEXT_BYTES;
  return size > limit
    ? `${name} is ${formatBytes(size)}; Studio will not open files larger than ${formatBytes(limit)}.`
    : null;
}
