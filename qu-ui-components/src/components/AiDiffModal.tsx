import React, { useMemo } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Check, X, Sparkles, Loader } from 'lucide-react';
import { cn } from '../utils/cn';

/** One rendered line of the diff -- 'same' lines appear on both sides. */
interface DiffLine {
  kind: 'same' | 'removed' | 'added';
  text: string;
}

/**
 * A minimal LCS (longest-common-subsequence) line diff -- no external
 * dependency needed for this package's one use case (reviewing a short
 * AI-suggested script/snippet before applying it, never a huge file).
 * O(n*m) time/space, same algorithm `diff`/git's own line-mode diff is
 * built on, just without the extra move-detection/word-diff refinements
 * those tools layer on top -- more than enough fidelity for a few dozen
 * lines of Qu code.
 */
function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split(/\r\n|\r|\n/);
  const b = after.split(/\r\n|\r|\n/);
  const n = a.length;
  const m = b.length;

  // dp[i][j] = length of the LCS of a[i..] and b[j..]
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: 'same', text: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: 'removed', text: a[i] });
      i++;
    } else {
      out.push({ kind: 'added', text: b[j] });
      j++;
    }
  }
  while (i < n) {
    out.push({ kind: 'removed', text: a[i] });
    i++;
  }
  while (j < m) {
    out.push({ kind: 'added', text: b[j] });
    j++;
  }
  return out;
}

export interface AiDiffModalProps {
  /** Shown in the header, e.g. "Fix suggestion" or "Transform suggestion". */
  title: string;
  /** The code as it exists now, before the AI's suggestion. */
  original: string;
  /**
   * The AI's suggested replacement. `null` while a request is still in
   * flight (renders a busy state instead of a diff) -- keeps this one
   * component covering both "waiting for the model" and "reviewing its
   * answer" so the caller doesn't need a separate loading overlay.
   */
  suggested: string | null;
  /** True while the request is in flight; pairs with `suggested === null`. */
  loading?: boolean;
  /** A short note on what's being waited for/reviewed, e.g. the instruction
   *  the user typed, or "this can take up to a minute on this local model". */
  subtitle?: string;
  /** Any error from the request itself (model not loaded, feature off, ...). */
  error?: string | null;
  onApply: () => void;
  onDiscard: () => void;
  theme?: 'light' | 'dark';
}

/**
 * Review-before-apply panel for every AI-suggested code change in QuStudio
 * (the "Fix with AI" button and the "Generate/Transform" button both use
 * this) -- a local model can and does produce wrong or non-Qu code, so
 * nothing from it ever overwrites the buffer directly; this is the one
 * place a user sees the actual diff and explicitly accepts or discards it.
 */
export const AiDiffModal: React.FC<AiDiffModalProps> = ({
  title,
  original,
  suggested,
  loading = false,
  subtitle,
  error,
  onApply,
  onDiscard,
  theme = 'dark',
}) => {
  const dark = theme === 'dark';
  const lines = useMemo(() => (suggested !== null ? diffLines(original, suggested) : []), [original, suggested]);
  const hasChanges = lines.some((l) => l.kind !== 'same');

  return (
    <AnimatePresence>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-6"
        onClick={onDiscard}
      >
        <motion.div
          initial={{ opacity: 0, y: 12, scale: 0.97 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 12, scale: 0.97 }}
          transition={{ duration: 0.15 }}
          onClick={(e) => e.stopPropagation()}
          className={cn(
            'w-full max-w-2xl max-h-[80vh] rounded-xl shadow-2xl flex flex-col overflow-hidden border',
            dark ? 'bg-[#161615] border-[#2c2c2a] text-[#dfe7f2]' : 'bg-white border-[#e1e0d9] text-[#52514e]'
          )}
        >
          {/* Header */}
          <div
            className={cn(
              'flex items-center justify-between px-4 py-3 border-b flex-shrink-0',
              dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]'
            )}
          >
            <div className="flex items-center gap-2 min-w-0">
              <Sparkles size={16} className={dark ? 'text-[#3987e5]' : 'text-[#2a78d6]'} />
              <div className="min-w-0">
                <div className="font-semibold text-sm truncate">{title}</div>
                {subtitle && (
                  <div className={cn('text-xs truncate', dark ? 'text-[#898781]' : 'text-[#898781]')}>{subtitle}</div>
                )}
              </div>
            </div>
            <button
              onClick={onDiscard}
              className={cn('p-1 rounded transition-colors flex-shrink-0', dark ? 'hover:bg-[#2c2c2a]' : 'hover:bg-[#e1e0d9]')}
              aria-label="Close"
            >
              <X size={14} />
            </button>
          </div>

          {/* Body */}
          <div className="flex-1 overflow-auto px-4 py-3 font-mono text-xs">
            {loading || suggested === null ? (
              <div className={cn('flex flex-col items-center justify-center gap-2 py-10', dark ? 'text-[#898781]' : 'text-[#898781]')}>
                <Loader size={20} className="animate-spin" />
                <span className="text-sm">
                  Asking the local model... this can take up to a minute on CPU-only inference.
                </span>
              </div>
            ) : error ? (
              <div className="rounded-lg px-3 py-2 bg-red-500/15 text-red-400 border border-red-500/30 whitespace-pre-wrap">
                {error}
              </div>
            ) : !hasChanges ? (
              <div className={cn('italic py-6 text-center', dark ? 'text-[#898781]' : 'text-[#898781]')}>
                The suggestion is identical to the current code.
              </div>
            ) : (
              <pre className="whitespace-pre-wrap break-words leading-5">
                {lines.map((line, i) => (
                  <div
                    key={i}
                    className={cn(
                      'px-2 -mx-2',
                      line.kind === 'removed' && (dark ? 'bg-red-500/15 text-red-300' : 'bg-red-100 text-red-700'),
                      line.kind === 'added' && (dark ? 'bg-emerald-500/15 text-emerald-300' : 'bg-emerald-100 text-emerald-700')
                    )}
                  >
                    <span className="select-none inline-block w-4 opacity-60">
                      {line.kind === 'removed' ? '-' : line.kind === 'added' ? '+' : ' '}
                    </span>
                    {line.text || ' '}
                  </div>
                ))}
              </pre>
            )}
          </div>

          {/* Footer */}
          <div className={cn('flex items-center justify-end gap-2 px-4 py-3 border-t flex-shrink-0', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}>
            <button
              onClick={onDiscard}
              className={cn(
                'px-3 py-1.5 rounded-lg text-sm font-medium transition-colors',
                dark ? 'hover:bg-[#2c2c2a]' : 'hover:bg-[#e1e0d9]'
              )}
            >
              Discard
            </button>
            <button
              onClick={onApply}
              disabled={loading || suggested === null || !!error || !hasChanges}
              className={cn(
                'flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-all',
                'disabled:opacity-40 disabled:cursor-not-allowed',
                'bg-[#2a78d6] text-white hover:bg-[#3987e5]'
              )}
            >
              <Check size={14} />
              Apply
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
};

export default AiDiffModal;
