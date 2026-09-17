import React, { useState, useEffect, useRef } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Search, X, ChevronRight } from 'lucide-react';
import './inspector.css';

export interface Command {
  id: string;
  label: string;
  description?: string;
  shortcut?: string[];
  icon?: React.ReactNode;
  action: () => void;
  category?: string;
}

export interface CommandPaletteProps {
  commands?: Command[];
  isOpen: boolean;
  onClose: () => void;
  theme?: 'light' | 'dark' | 'system';
}

export const CommandPalette: React.FC<CommandPaletteProps> = ({
  commands = [],
  isOpen,
  onClose,
  theme = 'system',
}) => {
  const [search, setSearch] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const resolvedTheme = theme === 'system'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    : theme;

  const filteredCommands = commands.filter(cmd =>
    cmd.label.toLowerCase().includes(search.toLowerCase()) ||
    cmd.description?.toLowerCase().includes(search.toLowerCase()) ||
    cmd.category?.toLowerCase().includes(search.toLowerCase())
  );

  useEffect(() => {
    if (isOpen && inputRef.current) {
      inputRef.current.focus();
    }
  }, [isOpen]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!isOpen) return;

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedIndex(prev => (prev + 1) % filteredCommands.length);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedIndex(prev => (prev - 1 + filteredCommands.length) % filteredCommands.length);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        if (filteredCommands[selectedIndex]) {
          filteredCommands[selectedIndex].action();
          onClose();
        }
      } else if (e.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, filteredCommands, selectedIndex, onClose]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [search]);

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="qu-scrim fixed inset-0 z-50"
            onClick={onClose}
          />

          {/* Palette */}
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: -20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -20 }}
            transition={{ duration: 0.15 }}
            className="fixed top-[18%] left-1/2 -translate-x-1/2 w-[600px] max-w-[92vw] max-h-[500px] rounded-xl overflow-hidden z-50 flex flex-col border"
            style={{
              background: 'var(--qu-bg)',
              borderColor: 'var(--qu-border)',
              color: 'var(--qu-text)',
              boxShadow: '0 24px 64px rgb(0 0 0 / 35%)',
            }}
          >
            {/* Search Input */}
            <div className="flex items-center gap-3 px-4 py-3 border-b" style={{ borderColor: 'var(--qu-border)' }}>
              <Search size={18} style={{ color: 'var(--qu-muted)' }} />
              <input
                ref={inputRef}
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Type a command or search..."
                className="qu-palette-input flex-1 bg-transparent outline-none text-[15px]"
              />
              <button onClick={onClose} className="qu-file-action" aria-label="Close command palette">
                <X size={16} />
              </button>
            </div>

            {/* Commands List */}
            <div className="flex-1 overflow-y-auto py-2">
              {filteredCommands.length === 0 ? (
                // Was a bare "No commands found". The useful half of an
                // empty state is what to do next, and here that is simply
                // "the text you typed matched nothing" -- so say which
                // text, the way every other empty state in the app now
                // names its own subject.
                <div className="qu-inspector qu-empty" data-theme={resolvedTheme}>
                  <Search size={22} />
                  <strong>No command matches</strong>
                  <p>
                    Nothing here is called &ldquo;{search}&rdquo;. Try a verb —
                    open, run, save, toggle.
                  </p>
                </div>
              ) : (
                filteredCommands.map((cmd, index) => (
                  <div
                    key={cmd.id}
                    // The non-selected hover was `bg-[#1c2740]/50` -- a
                    // half-opacity navy, i.e. a dark blue wash over a white
                    // palette in light mode. Selection is keyboard-driven
                    // here (arrow keys move `selectedIndex`), so the marker
                    // is the accent rule, not a grey fill that competes
                    // with hover.
                    className="qu-palette-row flex items-center gap-3 px-4 py-2.5 cursor-pointer"
                    data-selected={index === selectedIndex || undefined}
                    onClick={() => {
                      cmd.action();
                      onClose();
                    }}
                    onMouseEnter={() => setSelectedIndex(index)}
                  >
                    {cmd.icon && (
                      <div
                        className="p-1.5 rounded-md flex-shrink-0"
                        style={{ background: 'var(--qu-surface)', color: 'var(--qu-muted)' }}
                      >
                        {cmd.icon}
                      </div>
                    )}
                    <div className="flex-1 min-w-0">
                      <div className="text-sm font-medium" style={{ color: 'var(--qu-text)' }}>
                        {cmd.label}
                      </div>
                      {cmd.description && (
                        <div className="text-xs mt-0.5" style={{ color: 'var(--qu-muted)' }}>
                          {cmd.description}
                        </div>
                      )}
                    </div>
                    <div className="flex items-center gap-1">
                      {cmd.shortcut?.map((key, i) => (
                        <kbd key={i} className="qu-kbd">
                          {key}
                        </kbd>
                      ))}
                      <ChevronRight size={14} style={{ color: 'var(--qu-muted)' }} />
                    </div>
                  </div>
                ))
              )}
            </div>

            {/* Footer */}
            <div
              className="px-4 py-2 text-[11px] border-t flex items-center justify-between"
              style={{ borderColor: 'var(--qu-border)', color: 'var(--qu-muted)', background: 'var(--qu-surface)' }}
            >
              <span><kbd className="qu-kbd">↑↓</kbd> navigate</span>
              <span>
                <kbd className="qu-kbd">↵</kbd> select
                <kbd className="qu-kbd ml-1.5">esc</kbd> close
              </span>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
};

export default CommandPalette;
