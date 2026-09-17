/**
 * Detecting that an open file changed underneath the editor.
 *
 * Qu Studio has no filesystem watcher and deliberately does not gain one
 * here: a `notify`-style watcher costs a crate, a background thread and a
 * burst of events per keystroke from whatever external tool is writing,
 * to answer a question this app only ever asks at two moments -- when its
 * window regains focus, and on a slow poll while it holds focus. A `stat`
 * is cheap enough to just ask then.
 *
 * The part worth testing, and the reason this module is framework-free
 * (no React, no Tauri -- same rationale as `cells.ts`), is that a stat
 * alone is NOT sufficient evidence of a change:
 *
 *  - `mtime` moves when a file is merely rewritten with identical bytes.
 *    A `git checkout` that restores the same content, a formatter that
 *    made no change, a build step that regenerates a file verbatim -- all
 *    tick mtime. Banner on that and the user learns to dismiss the banner
 *    without reading it, which costs more than never showing it.
 *  - `len` is unchanged by any edit that preserves length, which is the
 *    common case for the edits that matter most (a digit, a sign, a
 *    single character in a constant).
 *
 * So the stat is a *gate*, not a verdict: `classifyStat` says only
 * "worth re-reading", and the caller confirms against the bytes it last
 * knew to be on disk before showing the user anything. Both halves are
 * exercised below.
 */

/** What `file_stat` (see `src-tauri/src/main.rs`) reports for one path. */
export interface DiskStat {
  exists: boolean;
  /**
   * Milliseconds since the Unix epoch, or `null` when the platform or
   * filesystem does not report one. `null` must never be read as
   * "unchanged" -- it means fall back to `len`.
   */
  mtime_ms: number | null;
  len: number;
}

/** What the editor last knew to be true of a path on disk. */
export interface DiskBaseline {
  stat: DiskStat;
  /** The exact bytes last read from (or written to) that path. */
  content: string;
}

/**
 * Whether a fresh stat is worth acting on. `'recheck'` means only that
 * something moved -- the caller must still read the file and compare
 * content before deciding a change is real. See `confirmChange`.
 */
export type StatVerdict = 'unchanged' | 'recheck' | 'deleted' | 'recreated';

/** The user-facing conclusion, after content has actually been compared. */
export type DiskChange = 'none' | 'modified' | 'deleted' | 'recreated';

/**
 * Stage one: the cheap gate.
 *
 * With no baseline the answer is always `'unchanged'` -- the first stat of
 * a path establishes what "unchanged" means for it, and must never itself
 * raise a banner. That bootstrap is why nothing has to hook every place a
 * file can be opened.
 */
export function classifyStat(baseline: DiskBaseline | undefined, current: DiskStat): StatVerdict {
  if (!baseline) return 'unchanged';

  if (baseline.stat.exists && !current.exists) return 'deleted';
  if (!baseline.stat.exists && current.exists) return 'recreated';
  if (!baseline.stat.exists && !current.exists) return 'unchanged';

  if (current.len !== baseline.stat.len) return 'recheck';

  // Only compare mtimes when BOTH are real. A filesystem that reports no
  // mtime gives us nothing here, and treating `null === null` as "same"
  // would silently disable detection for equal-length edits on it -- so
  // fall through to a recheck instead of claiming they match.
  if (current.mtime_ms === null || baseline.stat.mtime_ms === null) {
    return current.mtime_ms === baseline.stat.mtime_ms ? 'unchanged' : 'recheck';
  }

  return current.mtime_ms !== baseline.stat.mtime_ms ? 'recheck' : 'unchanged';
}

/**
 * Stage two: the verdict, once the file has actually been re-read.
 *
 * `diskContent` is what the file now holds. Identical bytes mean the stat
 * moved for a reason the user does not care about, and the caller should
 * quietly re-baseline rather than say anything.
 */
export function confirmChange(baseline: DiskBaseline | undefined, diskContent: string): DiskChange {
  if (!baseline) return 'none';
  return diskContent === baseline.content ? 'none' : 'modified';
}

/**
 * Does the user actually have something to lose if this file is reloaded?
 *
 * Drives which affordances the banner offers: with no unsaved edits a
 * reload is free and can be the obvious default, whereas a dirty buffer
 * means both versions matter and the diff is the honest thing to lead
 * with. Compares against the bytes last known on disk, not a dirty flag,
 * because a buffer can be marked dirty and still be byte-identical to the
 * file (type a character, delete it again).
 *
 * Takes the baseline CONTENT rather than a whole `DiskBaseline` so it can
 * be evaluated at RENDER time against the live buffer. It was originally
 * computed once, when the change was first detected, and stored beside
 * the alert -- which was wrong, and wrong in the dangerous direction:
 * type into the buffer while the banner is up and it went on claiming you
 * had nothing unsaved, while Reload sat there ready to discard exactly
 * that typing.
 */
export function hasLocalEdits(baselineContent: string | undefined, bufferContent: string): boolean {
  if (baselineContent === undefined) return false;
  return bufferContent !== baselineContent;
}
