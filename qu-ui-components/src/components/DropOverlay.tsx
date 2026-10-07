import React, { useEffect } from 'react';
import { Upload } from 'lucide-react';
import './viewers.css';

/** Full-window "drop here" target, shown while an OS file drag hovers. */
export const DropOverlay: React.FC<{ visible: boolean }> = ({ visible }) => {
  if (!visible) return null;
  return (
    <div className="qu-drop-overlay" data-testid="drop-overlay" role="presentation">
      <div className="qu-drop-card">
        <Upload size={30} />
        <strong>Drop to open</strong>
        <span>.qu scripts, .svg drawings and .pdf documents</span>
      </div>
    </div>
  );
};

export interface ToastItem {
  id: number;
  kind: 'info' | 'warning' | 'error';
  text: string;
}

/** Stack of short-lived messages (auto-dismiss after `ttl` ms). */
export const ToastStack: React.FC<{ toasts: ToastItem[]; onDismiss: (id: number) => void; ttl?: number }> = ({
  toasts,
  onDismiss,
  ttl = 6000,
}) => {
  useEffect(() => {
    if (!toasts.length) return;
    const timers = toasts.map((t) => window.setTimeout(() => onDismiss(t.id), ttl));
    return () => timers.forEach(window.clearTimeout);
  }, [toasts, onDismiss, ttl]);
  if (!toasts.length) return null;
  return (
    <div className="qu-toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="qu-toast" data-kind={t.kind} onClick={() => onDismiss(t.id)}>
          {t.text}
        </div>
      ))}
    </div>
  );
};
