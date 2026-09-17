/** Reading data coordinates back out of a rendered Qu figure.
 *
 * The engine draws a figure as a flat list of lines and text -- nothing in
 * the picture says which pixel means which data value. It also emits the
 * mapping it used into the SVG's `<metadata id="qu-figure">` element, and
 * this is the reader for it. The alternative, parsing tick labels off the
 * rendered axes, guesses wrong the moment an axis is logarithmic, broken,
 * or has ticks the label formatter rounded.
 */

export interface PanelGeometry {
  panel: number;
  /** Plot area in SVG user units. */
  left: number;
  right: number;
  top: number;
  bottom: number;
  /** Data limits, POST-transform: already log10 when the axis is log. */
  xmin: number;
  xmax: number;
  ymin: number;
  ymax: number;
  logX: boolean;
  logY: boolean;
  xBreak: [number, number] | null;
  yBreak: [number, number] | null;
}

export interface FigureGeometry {
  version: number;
  width: number;
  height: number;
  panels: PanelGeometry[];
}

/** Parse the mapping out of a figure's SVG, or `null` if it carries none
 *  (a figure rendered by an older engine, or a raster image). */
export function readFigureGeometry(svg: string | Document | null): FigureGeometry | null {
  if (!svg) return null;
  let text: string | null = null;
  if (typeof svg === "string") {
    // Deliberately a regex rather than DOMParser: this runs on every
    // figure the viewer opens, and parsing a large SVG twice (once here,
    // once by the browser when it mounts) is pure waste.
    const m = /<metadata\b[^>]*id="qu-figure"[^>]*>([\s\S]*?)<\/metadata>/.exec(svg);
    text = m ? m[1] : null;
  } else {
    text = svg.querySelector("metadata#qu-figure")?.textContent ?? null;
  }
  if (!text) return null;
  try {
    // The engine XML-escapes the JSON; a string extracted by regex still
    // carries those entities, so undo the five XML predefined ones.
    const decoded = text
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&amp;/g, "&");
    const parsed = JSON.parse(decoded) as FigureGeometry;
    return Array.isArray(parsed?.panels) ? parsed : null;
  } catch {
    return null;
  }
}

/** The panel containing a point, in SVG user units, or `null` outside all
 *  of them. Later panels win, matching draw order, so an inset over a
 *  panel reports the inset. */
export function panelAt(geo: FigureGeometry, x: number, y: number): PanelGeometry | null {
  for (let i = geo.panels.length - 1; i >= 0; i--) {
    const p = geo.panels[i];
    if (x >= p.left && x <= p.right && y >= p.top && y <= p.bottom) return p;
  }
  return null;
}

/** Invert one axis: a 0..1 fraction across the plot area -> a data value.
 *
 * Mirrors the engine's `axis_frac`. Inside a break the mapping is
 * many-to-one -- every value in the gap is drawn at the same place -- so
 * the honest inverse is the middle of the gap, not a precise-looking
 * number the figure cannot actually support.
 */
function invertAxis(frac: number, lo: number, hi: number, brk: [number, number] | null): number {
  if (brk) {
    const [blo, bhi] = brk;
    const shown = hi - lo - (bhi - blo);
    if (shown > 0 && bhi > blo && blo > lo && bhi < hi) {
      // The engine gives the break zone a fixed slice of the axis; the
      // rest is split in proportion to the data each side spans.
      const zoneFrac = 0.06;
      const leftFrac = ((blo - lo) / shown) * (1 - zoneFrac);
      const rightFrac = 1 - zoneFrac - leftFrac;
      if (frac <= leftFrac) return lo + (frac / Math.max(leftFrac, 1e-12)) * (blo - lo);
      if (frac >= leftFrac + zoneFrac) {
        return bhi + ((frac - leftFrac - zoneFrac) / Math.max(rightFrac, 1e-12)) * (hi - bhi);
      }
      return (blo + bhi) / 2;
    }
  }
  return lo + frac * (hi - lo);
}

/** SVG user-unit point -> data coordinates for a panel. */
export function dataAt(p: PanelGeometry, x: number, y: number): { x: number; y: number } {
  const fx = (x - p.left) / Math.max(p.right - p.left, 1e-12);
  const fy = (p.bottom - y) / Math.max(p.bottom - p.top, 1e-12);
  const dx = invertAxis(fx, p.xmin, p.xmax, p.xBreak);
  const dy = invertAxis(fy, p.ymin, p.ymax, p.yBreak);
  // Limits are stored post-transform, so undo the log here.
  return { x: p.logX ? 10 ** dx : dx, y: p.logY ? 10 ** dy : dy };
}

/** Data coordinates -> SVG user units, for placing an annotation back on
 *  the figure at the value it refers to. */
export function pixelAt(p: PanelGeometry, x: number, y: number): { x: number; y: number } {
  const tx = p.logX ? Math.log10(Math.max(x, 1e-300)) : x;
  const ty = p.logY ? Math.log10(Math.max(y, 1e-300)) : y;
  const fx = (tx - p.xmin) / Math.max(p.xmax - p.xmin, 1e-12);
  const fy = (ty - p.ymin) / Math.max(p.ymax - p.ymin, 1e-12);
  return { x: p.left + fx * (p.right - p.left), y: p.bottom - fy * (p.bottom - p.top) };
}

/** Format a coordinate for a readout: enough significant figures to be
 *  useful, without the noise of full float precision. */
export function formatCoord(v: number): string {
  if (!Number.isFinite(v)) return "—";
  const a = Math.abs(v);
  if (a !== 0 && (a < 1e-3 || a >= 1e6)) return v.toExponential(3);
  return String(Number(v.toPrecision(6)));
}

/** The finest data step a figure can actually resolve on one axis.
 *
 * The renderer writes coordinates with two decimals, so a drawn point
 * carries at most that much positional information. Reading a value back
 * off the picture therefore cannot be more precise than this -- and
 * printing `9.00005` for a point that is exactly 9 is false precision
 * produced by the round trip, not a real measurement.
 */
export function axisResolution(p: PanelGeometry, axis: "x" | "y"): number {
  const span = axis === "x" ? p.right - p.left : p.bottom - p.top;
  const range = axis === "x" ? p.xmax - p.xmin : p.ymax - p.ymin;
  const SVG_COORD_STEP = 0.01; // two decimals, as the renderer writes them
  return Math.abs(range / Math.max(span, 1e-12)) * SVG_COORD_STEP;
}

/** Format a value read back off the figure, at the precision the figure
 *  can actually support. One decimal coarser than the raw resolution, so
 *  a point that really is at 9 reads as `9` rather than `9.0001`. */
export function formatSnapped(value: number, resolution: number): string {
  if (!Number.isFinite(value)) return "—";
  if (!Number.isFinite(resolution) || resolution <= 0) return formatCoord(value);
  const decimals = Math.max(0, Math.ceil(-Math.log10(resolution)) - 1);
  const rounded = Number(value.toFixed(Math.min(decimals, 15)));
  // Very large or very small magnitudes still read better in exponent
  // form, matching `formatCoord`.
  const a = Math.abs(rounded);
  if (a !== 0 && (a < 1e-3 || a >= 1e6)) return rounded.toExponential(3);
  return String(rounded);
}
