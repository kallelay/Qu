import React, { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Send, X, Sparkles } from 'lucide-react';
import { cn } from '../utils/cn';

export interface MascotMessage {
  role: 'user' | 'assistant' | 'error';
  text: string;
}

export interface MascotProps {
  /**
   * Sends `question` to the LLM bridge and resolves with its reply (plain
   * text). The host app owns the actual Tauri `invoke('llm_chat', ...)`
   * call (and whatever current-buffer/last-error context it wants to fold
   * in) -- this component only knows how to render a chat thread and hand
   * off the raw question, so it stays usable outside a Tauri window too
   * (e.g. Storybook) by swapping in a different `onAsk`.
   */
  onAsk: (question: string) => Promise<string>;
  /** Accepted for call-site symmetry, no longer read: the panel takes its
   *  colours from the `--qu-*` tokens on `:root`. It used to branch on
   *  this for nine separate pairs of literals -- several of which named
   *  the same colour on both sides of the ternary, and two of which used
   *  the retired navy palette, so the chat bubbles did not match the
   *  window they floated over. */
  theme?: 'light' | 'dark';
  /** Shown once, above the first message, when the panel is opened with an
   *  empty thread -- e.g. "Ask me about your code or your last error." */
  greeting?: string;
  className?: string;
  /**
   * One-shot request to open the chat panel from outside (e.g. a host app's
   * Command Palette "Open Mascot" action) -- bump this to any new value to
   * pop the panel open, same token-bump contract as `CodeEditor`'s
   * `insertRequest`/`replaceRangeRequest`. Omit entirely to leave the
   * mascot fully self-contained (click-to-open only), which is this
   * component's default, pre-existing behavior.
   */
  openRequest?: number;
}

/**
 * Qu's own mascot: a small rounded-square character built from the app's
 * existing "Q" glyph (see `docs/favicon.svg`) rather than a copy of any
 * other product's assistant -- same blue rounded-square body and the same
 * swash-tail silhouette as the wordmark, just given a face. Lives as a
 * small dockable bubble (bottom-right corner, per the brief) that expands
 * into a simple chat panel on click.
 */
export const Mascot: React.FC<MascotProps> = ({
  onAsk,
  greeting = "Hi! Ask me about your code, or why your last run failed.",
  className,
  openRequest,
}) => {
  const [open, setOpen] = useState(false);
  const [messages, setMessages] = useState<MascotMessage[]>([]);
  const [input, setInput] = useState('');
  const [thinking, setThinking] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const lastOpenRequestRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (openRequest === undefined) return;
    if (lastOpenRequestRef.current === openRequest) return;
    lastOpenRequestRef.current = openRequest;
    setOpen(true);
  }, [openRequest]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, thinking]);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  const send = async () => {
    const question = input.trim();
    if (!question || thinking) return;
    setInput('');
    setMessages((prev) => [...prev, { role: 'user', text: question }]);
    setThinking(true);
    try {
      const reply = await onAsk(question);
      setMessages((prev) => [...prev, { role: 'assistant', text: reply || '(no response)' }]);
    } catch (err: any) {
      setMessages((prev) => [
        ...prev,
        { role: 'error', text: err?.message ?? String(err) },
      ]);
    } finally {
      setThinking(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className={cn('fixed bottom-5 right-5 z-50 flex flex-col items-end', className)}>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: 12, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 12, scale: 0.96 }}
            transition={{ duration: 0.15 }}
            className="mb-3 w-80 h-[26rem] rounded-xl shadow-2xl flex flex-col overflow-hidden border bg-[var(--qu-bg)] border-[var(--qu-border)] text-[var(--qu-text)]"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-3 py-2 border-b flex-shrink-0 border-[var(--qu-border)]">
              <div className="flex items-center gap-2">
                <MascotFace size={20} blink />
                <span className="font-semibold text-sm">Qu-bot</span>
                <Sparkles size={12} className="text-[var(--qu-accent)]" />
              </div>
              <button
                onClick={() => setOpen(false)}
                className="p-1 rounded transition-colors hover:bg-[var(--qu-hover)]"
                aria-label="Close mascot chat"
              >
                <X size={14} />
              </button>
            </div>

            {/* Messages */}
            <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 py-2 space-y-2 text-sm">
              {messages.length === 0 && (
                <div className="text-xs text-[var(--qu-muted)]">{greeting}</div>
              )}
              {messages.map((m, i) => (
                <div
                  key={i}
                  className={cn(
                    'rounded-lg px-3 py-2 max-w-[90%] whitespace-pre-wrap break-words',
                    m.role === 'user'
                      ? 'ml-auto bg-[var(--qu-accent-solid)] text-white'
                      : m.role === 'error'
                      ? 'text-[var(--qu-danger)] border border-current'
                      : 'bg-[var(--qu-surface)] border border-[var(--qu-border)]'
                  )}
                >
                  {m.text}
                </div>
              ))}
              {thinking && (
                <div
                  className={cn(
                    'rounded-lg px-3 py-2 max-w-[90%] flex items-center gap-1',
                    'bg-[var(--qu-surface)] border border-[var(--qu-border)]'
                  )}
                >
                  <ThinkingDots />
                </div>
              )}
            </div>

            {/* Input */}
            <div className="p-2 border-t flex items-end gap-2 flex-shrink-0 border-[var(--qu-border)]">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={onKeyDown}
                rows={1}
                placeholder="Ask Qu-bot..."
                className={cn(
                  'flex-1 resize-none rounded-lg px-2 py-1.5 text-sm outline-none max-h-24',
                  'bg-[var(--qu-surface)] border border-[var(--qu-border)] placeholder:text-[var(--qu-muted)]'
                )}
              />
              <button
                onClick={send}
                disabled={thinking || !input.trim()}
                className="p-2 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed bg-[var(--qu-accent-solid)] text-white hover:bg-[var(--qu-accent-solid-hover)]"
                aria-label="Send"
              >
                <Send size={14} />
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Bubble */}
      <motion.button
        onClick={() => setOpen((o) => !o)}
        whileHover={{ scale: 1.08 }}
        whileTap={{ scale: 0.94 }}
        aria-label={open ? 'Close Qu-bot' : 'Open Qu-bot'}
        className="w-14 h-14 rounded-2xl shadow-xl flex items-center justify-center bg-[var(--qu-accent-solid)]"
      >
        <motion.div
          animate={{ y: [0, -3, 0] }}
          transition={{ duration: 2.2, repeat: Infinity, ease: 'easeInOut' }}
        >
          <MascotFace size={30} blink bounce={!open} />
        </motion.div>
      </motion.button>
    </div>
  );
};

/**
 * The mascot's face: reuses the exact silhouette of the app's own "Q" glyph
 * (a rounded body with the Q's diagonal swash reinterpreted as a stubby
 * tail) plus two blinking eyes and a small smile -- Qu's own character, not
 * a reskin of any other assistant.
 */
const MascotFace: React.FC<{ size: number; blink?: boolean; bounce?: boolean }> = ({ size, blink }) => {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
      {/* Body: rounded square, matching docs/favicon.svg's silhouette */}
      <rect x="4" y="4" width="56" height="56" rx="16" fill="#ffffff" fillOpacity="0.16" />
      <rect x="4" y="4" width="56" height="56" rx="16" fill="none" stroke="#ffffff" strokeOpacity="0.9" strokeWidth="2" />
      {/* Tail: the Q's diagonal swash, reused as a little wag */}
      <motion.path
        d="M40 40 L52 50"
        stroke="#ffffff"
        strokeWidth="4"
        strokeLinecap="round"
        animate={{ rotate: [0, 8, -8, 0] }}
        transition={{ duration: 1.6, repeat: Infinity, ease: 'easeInOut' }}
        style={{ transformOrigin: '40px 40px' }}
      />
      {/* Eyes */}
      <motion.circle
        cx="24"
        cy="27"
        r="4"
        fill="#ffffff"
        animate={blink ? { scaleY: [1, 1, 0.1, 1, 1] } : undefined}
        transition={{ duration: 3.2, repeat: Infinity, times: [0, 0.85, 0.9, 0.95, 1] }}
        style={{ transformOrigin: '24px 27px' }}
      />
      <motion.circle
        cx="40"
        cy="27"
        r="4"
        fill="#ffffff"
        animate={blink ? { scaleY: [1, 1, 0.1, 1, 1] } : undefined}
        transition={{ duration: 3.2, repeat: Infinity, times: [0, 0.85, 0.9, 0.95, 1] }}
        style={{ transformOrigin: '40px 27px' }}
      />
      {/* Smile */}
      <path d="M22 38 Q32 46 42 38" stroke="#ffffff" strokeWidth="3" strokeLinecap="round" fill="none" />
    </svg>
  );
};

const ThinkingDots: React.FC = () => (
  <div className="flex items-center gap-1">
    {[0, 1, 2].map((i) => (
      <motion.span
        key={i}
        className="w-1.5 h-1.5 rounded-full bg-[var(--qu-muted)]"
        animate={{ opacity: [0.3, 1, 0.3] }}
        transition={{ duration: 1, repeat: Infinity, delay: i * 0.15 }}
      />
    ))}
  </div>
);

export default Mascot;
