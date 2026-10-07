/** Pan/zoom arithmetic for the SVG viewer. Pure, so it is tested in node. */

export interface View {
  /** Scale factor (1 = 100%). */
  k: number;
  /** Translation in container pixels. */
  x: number;
  y: number;
}

export const MIN_ZOOM = 0.02;
export const MAX_ZOOM = 64;

export function clampZoom(k: number): number {
  if (!Number.isFinite(k) || k <= 0) return 1;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));
}

/** The view that centres a `w`x`h` content box in a `cw`x`ch` container at
 *  a given scale. */
export function centered(cw: number, ch: number, w: number, h: number, k: number): View {
  return { k, x: (cw - w * k) / 2, y: (ch - h * k) / 2 };
}

/** Scale at which the content fits the container with a margin; never
 *  upscales tiny drawings past 1 unless `allowUpscale`. */
export function fitScale(cw: number, ch: number, w: number, h: number, margin = 24, allowUpscale = true): number {
  if (!(w > 0) || !(h > 0) || !(cw > 0) || !(ch > 0)) return 1;
  const k = Math.min((cw - 2 * margin) / w, (ch - 2 * margin) / h);
  const bounded = allowUpscale ? k : Math.min(1, k);
  return clampZoom(bounded > 0 ? bounded : 1);
}

export function fitView(cw: number, ch: number, w: number, h: number, margin = 24, allowUpscale = true): View {
  return centered(cw, ch, w, h, fitScale(cw, ch, w, h, margin, allowUpscale));
}

/** Zooms by `factor` keeping the content point under (px, py) fixed. */
export function zoomAt(v: View, factor: number, px: number, py: number): View {
  const k = clampZoom(v.k * factor);
  const f = k / v.k;
  return { k, x: px - (px - v.x) * f, y: py - (py - v.y) * f };
}

/** Wheel delta -> zoom factor (smooth for trackpads, steady for wheels). */
export function wheelFactor(deltaY: number, deltaMode = 0): number {
  const px = deltaMode === 1 ? deltaY * 16 : deltaMode === 2 ? deltaY * 400 : deltaY;
  return Math.exp(-Math.max(-200, Math.min(200, px)) * 0.0015);
}

/**
 * Intrinsic size of an SVG from its attributes: width/height when they are
 * plain numbers (px/pt), else the viewBox, else a 300x150 fallback (what a
 * browser uses for a bare <svg>).
 */
export function intrinsicSize(attrs: { width?: string | null; height?: string | null; viewBox?: string | null }): {
  w: number;
  h: number;
} {
  const num = (s?: string | null): number | null => {
    if (!s) return null;
    const m = /^\s*([0-9]*\.?[0-9]+(?:e[+-]?\d+)?)\s*(px|pt)?\s*$/i.exec(s);
    if (!m) return null;
    const v = parseFloat(m[1]);
    return m[2]?.toLowerCase() === 'pt' ? (v * 96) / 72 : v;
  };
  const vb = attrs.viewBox
    ? attrs.viewBox
        .trim()
        .split(/[\s,]+/)
        .map(Number)
    : [];
  const vbOk = vb.length === 4 && vb.every(Number.isFinite) && vb[2] > 0 && vb[3] > 0;
  let w = num(attrs.width);
  let h = num(attrs.height);
  if (vbOk) {
    if (w && !h) h = (w * vb[3]) / vb[2];
    else if (h && !w) w = (h * vb[2]) / vb[3];
    else if (!w && !h) {
      w = vb[2];
      h = vb[3];
    }
  }
  return { w: w ?? 300, h: h ?? 150 };
}
