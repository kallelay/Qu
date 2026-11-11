/**
 * Where in the user's file did a Qu run fail?
 *
 * The engine reports a location in two shapes (both observed from
 * `qu run`, 2026-11-10):
 *
 *   qu: runtime error: `nope` is not defined
 *     at e.qu:3 in f()
 *     called from e.qu:5 (top level)
 *
 *   qu: parse error at p.qu:4:1: expected `end`
 *
 * Pure text in, location out, so it is unit tested without an editor.
 */

export interface QuErrorLocation {
  /** The file as the engine named it (may be a full path, may be empty). */
  file: string;
  /** 1-based line. */
  line: number;
  /** 1-based column when the engine gave one (parse errors do). */
  column?: number;
  /** The error's headline, without the `qu: ` prefix or the trace lines. */
  message: string;
}

// A frame is introduced by `at` / `called from`. What follows is one of:
//   `line 12`          -- the kernel, when the buffer has no file name
//   `12:3:`            -- a parse error with no file name (`parse error at 12:3: ...`)
//   `<file>:12[:3]`    -- everything else; the file part is non-greedy up
//                         to the LAST `:digits` before a space/colon/end,
//                         so a Windows path's own `C:` is not the line.
const BARE_LINE_RE = /(?:\bat|called from)\s+line (\d+)\b/;
const BARE_PARSE_RE = /parse error at (\d+):(\d+):/;
const FILE_FRAME_RE = /(?:\bat|called from)\s+(\S.*?\.[A-Za-z0-9_]+):(\d+)(?::(\d+))?(?=[\s:]|$)/;

function frameOf(l: string): { file: string; line: number; column?: number } | null {
  let m = l.match(BARE_PARSE_RE);
  if (m) return { file: '', line: Number(m[1]), column: Number(m[2]) };
  m = l.match(BARE_LINE_RE);
  if (m) return { file: '', line: Number(m[1]) };
  m = l.match(FILE_FRAME_RE);
  if (m) return { file: m[1].trim(), line: Number(m[2]), column: m[3] ? Number(m[3]) : undefined };
  return null;
}

function baseName(path: string): string {
  return path.replace(/^.*[\\/]/, '').toLowerCase();
}

/**
 * The frame to mark for `errorText`. With `activeFile`, the innermost frame
 * that is IN that file (an error raised inside an imported module is marked
 * at the line of ours that called it); without, the innermost frame.
 * `null` when the error names no location, or none in `activeFile`.
 */
export function parseErrorLocation(errorText: string, activeFile?: string | null): QuErrorLocation | null {
  const lines = String(errorText ?? '').split(/\r?\n/).filter((l) => l.trim() !== '');
  if (lines.length === 0) return null;

  const frames: Array<{ file: string; line: number; column?: number }> = [];
  for (const l of lines) {
    const f = frameOf(l);
    if (f && f.line >= 1) frames.push(f);
  }
  if (frames.length === 0) return null;

  const want = activeFile ? baseName(activeFile) : '';
  // A frame with no file name is the script being run, i.e. the active one.
  const frame = want ? frames.find((f) => f.file === '' || baseName(f.file) === want) : frames[0];
  if (!frame) return null;

  const message = lines[0]
    .replace(/^qu:\s*/, '')
    // A parse error carries its location inline: keep only what follows it.
    // `qu kernel` (what Studio runs through) reports one as
    // `runtime error: parse error at 3:1: ...` -- prefix and no file name.
    .replace(/^(?:runtime error:\s*)?(parse error) at (?:.*?:)?\d+:\d+:\s*/, '$1: ')
    .trim();
  return { ...frame, message };
}
