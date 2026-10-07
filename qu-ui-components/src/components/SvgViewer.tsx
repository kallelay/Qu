import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Code2, Maximize, ZoomIn, ZoomOut } from 'lucide-react';
import { sanitizeSvg } from '../utils/svgSafe';
import { fitView, centered, zoomAt, wheelFactor, intrinsicSize, type View } from '../utils/viewTransform';
import './viewers.css';

export interface SvgViewerProps {
  /** Raw SVG text, exactly as read from disk. Sanitised here before use. */
  svg: string;
  /** File name shown in the header (truncated, full path in the tooltip). */
  name: string;
  path?: string | null;
  /** "View source": the host opens the file as text in the editor. */
  onViewSource?: () => void;
}

/**
 * Renders an SVG file with pan (drag) and zoom (wheel, buttons, keys).
 * The markup is sanitised with DOMPurify and injected inline, so no script
 * inside the file can run; a file that sanitises to nothing shows a notice
 * and the "view source" button still works.
 */
export const SvgViewer: React.FC<SvgViewerProps> = ({ svg, name, path, onViewSource }) => {
  const clean = useMemo(() => sanitizeSvg(svg), [svg]);
  const box = useMemo(() => {
    if (!clean) return { w: 300, h: 150 };
    const doc = new DOMParser().parseFromString(clean, 'image/svg+xml');
    const el = doc.documentElement;
    if (!el || el.nodeName.toLowerCase() !== 'svg' || doc.querySelector('parsererror')) return { w: 300, h: 150 };
    return intrinsicSize({
      width: el.getAttribute('width'),
      height: el.getAttribute('height'),
      viewBox: el.getAttribute('viewBox'),
    });
  }, [clean]);

  const hostRef = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<View>({ k: 1, x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [animate, setAnimate] = useState(false);
  const viewRef = useRef(view);
  viewRef.current = view;
  const fitted = useRef(true);

  const size = () => {
    const r = hostRef.current?.getBoundingClientRect();
    return { cw: r?.width ?? 0, ch: r?.height ?? 0 };
  };

  const fit = useCallback(
    (smooth = true) => {
      const { cw, ch } = size();
      if (!cw || !ch) return;
      fitted.current = true;
      setAnimate(smooth);
      setView(fitView(cw, ch, box.w, box.h));
    },
    [box.w, box.h],
  );
  const actual = useCallback(() => {
    const { cw, ch } = size();
    fitted.current = false;
    setAnimate(true);
    setView(centered(cw, ch, box.w, box.h, 1));
  }, [box.w, box.h]);
  const zoomBy = useCallback((f: number) => {
    const { cw, ch } = size();
    fitted.current = false;
    setAnimate(true);
    setView((v) => zoomAt(v, f, cw / 2, ch / 2));
  }, []);

  // Fit on first layout and whenever the file changes.
  useLayoutEffect(() => {
    fit(false);
  }, [fit, clean]);

  // Keep a fitted drawing fitted as the pane resizes (no animation: it
  // would lag the drag-resize).
  useEffect(() => {
    const el = hostRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      if (fitted.current) fit(false);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [fit]);

  // Wheel needs a non-passive listener to preventDefault page scroll.
  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = el.getBoundingClientRect();
      fitted.current = false;
      setAnimate(false);
      setView((v) => zoomAt(v, wheelFactor(e.deltaY, e.deltaMode), e.clientX - r.left, e.clientY - r.top));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  const drag = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, vx: viewRef.current.x, vy: viewRef.current.y };
    setDragging(true);
    setAnimate(false);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    fitted.current = false;
    setView((v) => ({ ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y }));
  };
  const endDrag = () => {
    drag.current = null;
    setDragging(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === '0') fit();
    else if (e.key === '1') actual();
    else if (e.key === '+' || e.key === '=') zoomBy(1.25);
    else if (e.key === '-') zoomBy(0.8);
  };

  return (
    <div className="qu-viewer" data-testid="svg-viewer">
      <div className="qu-viewer-bar">
        <span className="qu-viewer-name" title={path || name}>
          {name}
        </span>
        <span className="qu-viewer-zoom" aria-live="polite">
          {Math.round(view.k * 100)}%
        </span>
        <button type="button" className="qu-viewer-btn" onClick={() => zoomBy(0.8)} aria-label="Zoom out" title="Zoom out (-)">
          <ZoomOut size={15} />
        </button>
        <button type="button" className="qu-viewer-btn" onClick={() => zoomBy(1.25)} aria-label="Zoom in" title="Zoom in (+)">
          <ZoomIn size={15} />
        </button>
        <button type="button" className="qu-viewer-btn" onClick={() => fit()} title="Fit to window (0)">
          <Maximize size={15} />
          <span>Fit</span>
        </button>
        <button type="button" className="qu-viewer-btn" onClick={actual} title="Actual size (1)">
          100%
        </button>
        {onViewSource && (
          <button type="button" className="qu-viewer-btn" onClick={onViewSource} title="Open the SVG source as text in the editor">
            <Code2 size={15} />
            <span>View source</span>
          </button>
        )}
      </div>
      <div
        ref={hostRef}
        className="qu-viewer-stage"
        data-dragging={dragging || undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={() => fit()}
      >
        {clean ? (
          <div
            className="qu-viewer-content"
            data-animate={animate || undefined}
            style={{
              width: box.w,
              height: box.h,
              transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})`,
            }}
            // Sanitised above (DOMPurify, svg profile, no scripts/foreignObject).
            dangerouslySetInnerHTML={{ __html: clean }}
          />
        ) : (
          <div className="qu-viewer-empty">
            This file has no drawable SVG content (or it contained only scripts, which Studio never runs). Use
            View source to read it as text.
          </div>
        )}
      </div>
    </div>
  );
};
