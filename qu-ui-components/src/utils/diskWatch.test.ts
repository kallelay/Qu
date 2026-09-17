import { describe, expect, it } from 'vitest';
import {
  classifyStat,
  confirmChange,
  hasLocalEdits,
  type DiskBaseline,
  type DiskStat,
} from './diskWatch';

const stat = (over: Partial<DiskStat> = {}): DiskStat => ({
  exists: true,
  mtime_ms: 1_000_000,
  len: 10,
  ...over,
});

const baseline = (over: Partial<DiskBaseline> = {}): DiskBaseline => ({
  stat: stat(),
  content: 'a = 1\nb = 2',
  ...over,
});

describe('classifyStat', () => {
  it('never fires on the first stat of a path', () => {
    // The bootstrap case. If this ever returned anything else, merely
    // opening a file would raise a "changed on disk" banner, and nothing
    // would have to hook the open path to prevent it.
    expect(classifyStat(undefined, stat())).toBe('unchanged');
    expect(classifyStat(undefined, stat({ exists: false, len: 0, mtime_ms: null }))).toBe('unchanged');
  });

  it('reports an identical stat as unchanged', () => {
    expect(classifyStat(baseline(), stat())).toBe('unchanged');
  });

  it('rechecks when the length changed but the mtime did not', () => {
    // A filesystem with coarse mtime resolution, or a write inside the
    // same tick. Length is the only signal left, so it has to be enough.
    expect(classifyStat(baseline(), stat({ len: 11 }))).toBe('recheck');
  });

  it('rechecks when the mtime moved but the length did not', () => {
    // The edit that matters most and is easiest to miss: one character
    // swapped for another, so the file is exactly as long as before.
    expect(classifyStat(baseline(), stat({ mtime_ms: 2_000_000 }))).toBe('recheck');
  });

  it('reports a file that disappeared as deleted', () => {
    expect(classifyStat(baseline(), stat({ exists: false, mtime_ms: null, len: 0 }))).toBe('deleted');
  });

  it('reports a file that came back as recreated', () => {
    const gone = baseline({ stat: stat({ exists: false, mtime_ms: null, len: 0 }) });
    expect(classifyStat(gone, stat())).toBe('recreated');
  });

  it('stays quiet while a file remains absent', () => {
    const gone = baseline({ stat: stat({ exists: false, mtime_ms: null, len: 0 }) });
    expect(classifyStat(gone, stat({ exists: false, mtime_ms: null, len: 0 }))).toBe('unchanged');
  });

  it('does not treat two missing mtimes as proof of no change', () => {
    // A filesystem reporting no mtime tells us nothing. Reading
    // `null === null` as "same" would silently disable detection there
    // for every equal-length edit, which is exactly the failure mode
    // that looks like the feature working.
    const noMtime = baseline({ stat: stat({ mtime_ms: null }) });
    expect(classifyStat(noMtime, stat({ mtime_ms: null }))).toBe('unchanged');
    expect(classifyStat(noMtime, stat({ mtime_ms: null, len: 12 }))).toBe('recheck');
    // And a baseline with no mtime against a stat that HAS one is a
    // recheck, not a match.
    expect(classifyStat(noMtime, stat({ mtime_ms: 5 }))).toBe('recheck');
  });
});

describe('confirmChange', () => {
  it('reports identical bytes as no change however much the stat moved', () => {
    // `git checkout` restoring the same content, a formatter that
    // changed nothing, a build regenerating a file verbatim. The stat
    // gate fires for all of these; the content check is what stops the
    // user being interrupted by them.
    expect(confirmChange(baseline(), 'a = 1\nb = 2')).toBe('none');
  });

  it('reports differing bytes as a real modification', () => {
    expect(confirmChange(baseline(), 'a = 1\nb = 3')).toBe('modified');
  });

  it('is silent with no baseline', () => {
    expect(confirmChange(undefined, 'anything at all')).toBe('none');
  });

  it('notices a same-length edit', () => {
    // Guards the case `len` alone cannot see -- one digit for another.
    expect(confirmChange(baseline(), 'a = 1\nb = 9')).toBe('modified');
  });
});

describe('hasLocalEdits', () => {
  const onDisk = 'a = 1\nb = 2';

  it('is false when the buffer still matches disk', () => {
    expect(hasLocalEdits(onDisk, 'a = 1\nb = 2')).toBe(false);
  });

  it('is true when the buffer has diverged', () => {
    expect(hasLocalEdits(onDisk, 'a = 1\nb = 2\nc = 3')).toBe(true);
  });

  it('is false for a buffer edited back to its original bytes', () => {
    // Typing a character and deleting it again leaves the tab flagged
    // dirty, but there is nothing to lose by reloading -- which is why
    // this compares bytes rather than reading `isDirty`.
    expect(hasLocalEdits(onDisk, 'a = 1\nb = 2')).toBe(false);
  });

  it('is false with no baseline', () => {
    expect(hasLocalEdits(undefined, 'anything')).toBe(false);
  });

  it('turns true as soon as the buffer is typed into', () => {
    // The regression this signature exists to prevent: the banner is
    // already up, THEN the user types. Evaluated against the live buffer
    // this flips to true, so the banner re-words itself and Reload stops
    // being presented as free. Computed once at detection time it stayed
    // false, and Reload silently discarded the new typing.
    expect(hasLocalEdits(onDisk, onDisk)).toBe(false);
    expect(hasLocalEdits(onDisk, onDisk + '\n# typed after the banner appeared')).toBe(true);
  });
});
