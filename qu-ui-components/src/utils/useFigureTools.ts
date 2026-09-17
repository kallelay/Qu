import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  axisResolution,
  dataAt,
  formatSnapped,
  panelAt,
  pixelAt,
  readFigureGeometry,
  type FigureGeometry,
} from "./figureGeometry";

/** What a click means right now. */
export type FigureTool =
  | "pan"
  // Cursor and annotate are ONE tool. Reading a value and marking it are
  // the same activity -- you point at something because you care about it
  // -- and splitting them meant finding a peak with the cursor, then
  // switching tools and finding it again to mark it. The readout is now
  // always live while annotating:
  //   click        a labelled annotation at that value
  //   double-click a highlighted point, labelled with its own coordinates
  //   drag         an arrow from where you started to where you released,
  //                with text
  | "annotate"
  /** Free text anywhere on the figure, not tied to a data point. */
  | "text"
  | "select"
  // The shape tools. Each one places a figure element by POINTING at the
  // plot and emits the Qu line that would draw it, because the engine --
  // not the viewer -- is what renders a figure. A shape that existed only
  // in the viewer would vanish on the next run; a line of code is the
  // thing that survives, gets committed, and can be reviewed.
  | "vline"
  | "hline"
  | "xspan"
  | "inset";

/** The tools whose readout follows the pointer. */
const READOUT_TOOLS: FigureTool[] = ["annotate", "text"];

/** A figure element placed by pointing, and the Qu that reproduces it. */
export interface GeneratedShape {
  id: string;
  tool: Extract<
    FigureTool,
    "vline" | "hline" | "xspan" | "inset" | "annotate" | "text" | "point" | "arrow"
  >;
  code: string;
  /** Data coordinates, for drawing the preview. */
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  panel: number;
}

/** The tools that are placed by dragging out a region rather than clicking. */
const DRAG_TOOLS: FigureTool[] = ["xspan", "inset", "annotate"];

/** Round a coordinate for generated code.
 *
 * A line reading `vline(3.14159265358979)` is noise: the click was worth
 * about a pixel, and printing sixteen digits claims a precision the mouse
 * never had. Four significant figures is what a person would have typed.
 */
/** Quote a string as a Qu literal.
 *
 * Qu interpolates `{...}` inside strings, so a label containing a brace
 * would be read as an embedded expression and error at run time. Doubling
 * braces is Qu's own escape for that.
 */
// Exported for reuse anywhere else that generates Qu source from a UI
// action (e.g. `guiDesign.ts`'s widget-designer codegen) -- the escaping
// rules are Qu's, not this file's, so they belong to one shared function
// rather than being re-derived per caller.
export function quote(text: string): string {
  const bs = String.fromCharCode(92);
  return (
    '"' +
    text
      .split(bs).join(bs + bs)
      .split('"').join(bs + '"')
      .split("{").join("{{")
      .split("}").join("}}") +
    '"'
  );
}

export function forCode(v: number): string {
  if (!Number.isFinite(v)) return "0";
  return String(Number(v.toPrecision(4)));
}

export interface Annotation {
  id: string;
  panel: number;
  /** Which Qu builtin reproduces it:
   *    annotate  text WITH a marker dot at the point
   *    text      the label alone, placed freely
   *    point     a highlighted sample labelled with its own coordinates
   *    arrow     an arrow from (x, y) to (toX, toY), with text
   *  Kept on the annotation rather than inferred later, so the preview and
   *  the generated code can never disagree about what was placed. */
  kind: "annotate" | "text" | "point" | "arrow";
  /** Where an `arrow` points TO. */
  toX?: number;
  toY?: number;
  /** DATA coordinates, not pixels: an annotation means "this value", so it
   *  must land in the same place when the figure is re-rendered at another
   *  size, rather than drifting with the pixel it was clicked at. */
  x: number;
  y: number;
  text: string;
}

const OVERLAY_ID = "qu-tool-overlay";
/** How close, in SVG user units, the pointer must be to snap. Tight
 *  enough that you can still place a mark in empty space on purpose. */
const SNAP_RADIUS = 14;

/** Every plotted point in a figure, in SVG user units.
 *
 * Taken from the drawn geometry itself -- polyline vertices and marker
 * circles -- because that IS the data: the renderer put each vertex where
 * its value maps to, so reading them back needs no extra channel from the
 * engine and cannot disagree with what is on screen.
 *
 * Legend swatches are excluded. They are ordinary circles and short lines
 * drawn inside the plot area, so without this a click near the legend
 * would snap to its little sample marker rather than to the data. The
 * legend box is the rounded white panel the renderer draws for it
 * (`rx="6"`), which is what identifies it here.
 */
function collectPoints(root: SVGSVGElement): Array<{ x: number; y: number }> {
  const overlay = root.querySelector("#" + OVERLAY_ID);
  const boxes: Array<{ x: number; y: number; w: number; h: number }> = [];
  root.querySelectorAll("rect[rx='6']").forEach((r) => {
    boxes.push({
      x: parseFloat(r.getAttribute("x") ?? "0"),
      y: parseFloat(r.getAttribute("y") ?? "0"),
      w: parseFloat(r.getAttribute("width") ?? "0"),
      h: parseFloat(r.getAttribute("height") ?? "0"),
    });
  });
  const inLegend = (x: number, y: number) =>
    boxes.some((b) => x >= b.x - 6 && x <= b.x + b.w + 6 && y >= b.y - 6 && y <= b.y + b.h + 6);

  const points: Array<{ x: number; y: number }> = [];
  const push = (x: number, y: number) => {
    if (Number.isFinite(x) && Number.isFinite(y) && !inLegend(x, y)) points.push({ x, y });
  };
  root.querySelectorAll("circle").forEach((c) => {
    if (overlay?.contains(c)) return;
    push(parseFloat(c.getAttribute("cx") ?? ""), parseFloat(c.getAttribute("cy") ?? ""));
  });
  root.querySelectorAll("polyline, polygon").forEach((line) => {
    if (overlay?.contains(line)) return;
    const raw = line.getAttribute("points") ?? "";
    for (const pair of raw.trim().split(/\s+/)) {
      const [px, py] = pair.split(",");
      push(parseFloat(px), parseFloat(py));
    }
  });
  return points;
}

const CROSSHAIR_CLASS = "qu-crosshair";

/** Interactive tools over a live, inline figure SVG.
 *
 * Everything is drawn INTO the figure's own SVG rather than into an HTML
 * layer floating above it. That way the crosshair and the annotations
 * scale, pan and zoom with the figure for free, land in the exported file,
 * and cannot drift out of alignment with the picture -- which an overlay
 * positioned in screen pixels does the moment anything scrolls.
 */
export function useFigureTools(options: {
  hostRef: React.RefObject<HTMLElement | null>;
  /** The figure source; tools reset when it changes. */
  svg: string | null;
  tool: FigureTool;
  active: boolean;
  /** Snap the cursor and new annotations to the nearest plotted point.
   *  On by default: pointing at a plot almost always means pointing at a
   *  value, and a mark that is nearly on the point reads as a mistake. */
  snap?: boolean;
}) {
  const { hostRef, svg, tool, active, snap = true } = options;
  const [readout, setReadout] = useState<
    { x: number; y: number; panel: number; label?: string | null } | null
  >(null);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<string | null>(null);
  const editingRef = useRef<HTMLInputElement | null>(null);
  /** Plotted points for snapping, rebuilt when the figure changes. */
  const pointsRef = useRef<Array<{ x: number; y: number }> | null>(null);
  const [snappedTo, setSnappedTo] = useState<{ x: number; y: number } | null>(null);
  const [shapes, setShapes] = useState<GeneratedShape[]>([]);
  /** In-progress drag for the region tools, in data coordinates. */
  const [dragging, setDragging] = useState<{ x0: number; y0: number; x1: number; y1: number; panel: number } | null>(null);
  const dragStart = useRef<{ x: number; y: number; panel: number } | null>(null);
  const geometry: FigureGeometry | null = useMemo(() => readFigureGeometry(svg), [svg]);

  // A different figure is a different set of annotations. Keeping them
  // would silently move marks onto data they were never about.
  useEffect(() => {
    setAnnotations([]);
    setEdits({});
    setSelected(null);
    setReadout(null);
    setSnappedTo(null);
    setShapes([]);
    setDragging(null);
    pointsRef.current = null;
  }, [svg]);

  const svgEl = useCallback(
    () => (hostRef.current?.querySelector("svg") as SVGSVGElement | null) ?? null,
    [hostRef],
  );

  /** Client (screen) point -> SVG user units, honouring whatever scroll,
   *  zoom or scaling the viewer has applied. `getScreenCTM` is the only way
   *  to get this right without duplicating the viewer's own layout maths
   *  and then drifting from it. */
  const toUser = useCallback(
    (clientX: number, clientY: number): { x: number; y: number } | null => {
      const el = svgEl();
      const ctm = el?.getScreenCTM();
      if (!el || !ctm) return null;
      const pt = el.createSVGPoint();
      pt.x = clientX;
      pt.y = clientY;
      const p = pt.matrixTransform(ctm.inverse());
      return { x: p.x, y: p.y };
    },
    [svgEl],
  );

  /** Nearest plotted point within `SNAP_RADIUS`, or the raw point.
   *
   * The index is built lazily from the live SVG and cached: a figure with
   * a few thousand vertices is cheap to scan once per figure and wasteful
   * to rebuild on every mouse move.
   */
  const withSnap = useCallback(
    (u: { x: number; y: number }): { point: { x: number; y: number }; snapped: boolean } => {
      if (!snap) return { point: u, snapped: false };
      const el = svgEl();
      if (!el) return { point: u, snapped: false };
      if (!pointsRef.current) pointsRef.current = collectPoints(el);
      let best: { x: number; y: number } | null = null;
      let bestDist = SNAP_RADIUS * SNAP_RADIUS;
      for (const p of pointsRef.current) {
        const dx = p.x - u.x;
        const dy = p.y - u.y;
        const d = dx * dx + dy * dy;
        if (d < bestDist) {
          bestDist = d;
          best = p;
        }
      }
      return best ? { point: best, snapped: true } : { point: u, snapped: false };
    },
    [snap, svgEl],
  );

  /** Redraw the overlay: crosshair, annotations. Rebuilt wholesale each
   *  time -- it is a handful of nodes, and diffing would be more code than
   *  it saves. */
  useEffect(() => {
    const el = svgEl();
    if (!el) return;
    el.querySelector("#" + OVERLAY_ID)?.remove();
    if (!active) return;

    const NS = "http://www.w3.org/2000/svg";
    const g = document.createElementNS(NS, "g");
    g.setAttribute("id", OVERLAY_ID);
    // The overlay must never swallow the events the tools depend on.
    g.setAttribute("pointer-events", "none");

    if (READOUT_TOOLS.includes(tool) && readout && geometry) {
      const p = geometry.panels.find((q) => q.panel === readout.panel);
      if (p) {
        const at = pixelAt(p, readout.x, readout.y);
        const spans = [
          [p.left, at.y, p.right, at.y],
          [at.x, p.top, at.x, p.bottom],
        ];
        for (const [x1, y1, x2, y2] of spans) {
          const line = document.createElementNS(NS, "line");
          line.setAttribute("x1", String(x1));
          line.setAttribute("y1", String(y1));
          line.setAttribute("x2", String(x2));
          line.setAttribute("y2", String(y2));
          line.setAttribute("stroke", "#2a78d6");
          line.setAttribute("stroke-width", "1");
          line.setAttribute("stroke-dasharray", "4 3");
          line.setAttribute("class", CROSSHAIR_CLASS);
          g.appendChild(line);
        }
      }
    }

    // A ring on the point the cursor has locked onto, so it is obvious
    // that the readout is a real data value and not an interpolation.
    if (snappedTo && READOUT_TOOLS.includes(tool)) {
      const ring = document.createElementNS(NS, "circle");
      ring.setAttribute("cx", String(snappedTo.x));
      ring.setAttribute("cy", String(snappedTo.y));
      ring.setAttribute("r", "6");
      ring.setAttribute("fill", "none");
      ring.setAttribute("stroke", "#2a78d6");
      ring.setAttribute("stroke-width", "1.75");
      ring.setAttribute("class", CROSSHAIR_CLASS);
      g.appendChild(ring);
    }

    // Placed shapes, and the region being dragged out right now. Drawn
    // in the same accent as the tools so they read as "not yet part of the
    // figure" -- the figure only gains them when the code is run.
    const drawShape = (sh: {
      tool: string;
      x0: number;
      y0: number;
      x1: number;
      y1: number;
      panel: number;
    }, preview: boolean) => {
      const p = geometry?.panels.find((q) => q.panel === sh.panel);
      if (!p) return;
      const a = pixelAt(p, sh.x0, sh.y0);
      const b = pixelAt(p, sh.x1, sh.y1);
      const stroke = preview ? "#2a78d6" : "#8a4fbd";
      if (sh.tool === "vline" || sh.tool === "hline") {
        const line = document.createElementNS(NS, "line");
        const vertical = sh.tool === "vline";
        line.setAttribute("x1", String(vertical ? a.x : p.left));
        line.setAttribute("y1", String(vertical ? p.top : a.y));
        line.setAttribute("x2", String(vertical ? a.x : p.right));
        line.setAttribute("y2", String(vertical ? p.bottom : a.y));
        line.setAttribute("stroke", stroke);
        line.setAttribute("stroke-width", "1.6");
        line.setAttribute("stroke-dasharray", "6 4");
        g.appendChild(line);
        return;
      }
      const rect = document.createElementNS(NS, "rect");
      const x = Math.min(a.x, b.x);
      const w = Math.abs(b.x - a.x);
      // An x-span covers the panel's full height whatever was dragged
      // vertically, which is what `xspan` actually draws.
      const spanning = sh.tool === "xspan";
      rect.setAttribute("x", String(x));
      rect.setAttribute("y", String(spanning ? p.top : Math.min(a.y, b.y)));
      rect.setAttribute("width", String(w));
      rect.setAttribute("height", String(spanning ? p.bottom - p.top : Math.abs(b.y - a.y)));
      rect.setAttribute("fill", stroke);
      rect.setAttribute("fill-opacity", "0.12");
      rect.setAttribute("stroke", stroke);
      rect.setAttribute("stroke-width", "1.2");
      if (sh.tool === "inset") rect.setAttribute("stroke-dasharray", "5 3");
      g.appendChild(rect);
    };
    for (const sh of shapes) drawShape(sh, false);
    if (dragging) {
      drawShape({ ...dragging, tool }, true);
    }

    for (const a of annotations) {
      const p = geometry?.panels.find((q) => q.panel === a.panel);
      if (!p) continue;
      const at = pixelAt(p, a.x, a.y);

      // An arrow is drawn from where the drag started to where it ended,
      // with a head, so the preview matches what `arrowtext` renders.
      if (a.kind === "arrow" && a.toX !== undefined && a.toY !== undefined) {
        const to = pixelAt(p, a.toX, a.toY);
        const shaft = document.createElementNS(NS, "line");
        shaft.setAttribute("x1", String(at.x));
        shaft.setAttribute("y1", String(at.y));
        shaft.setAttribute("x2", String(to.x));
        shaft.setAttribute("y2", String(to.y));
        shaft.setAttribute("stroke", "#d1495b");
        shaft.setAttribute("stroke-width", "1.6");
        g.appendChild(shaft);
        const dx = to.x - at.x;
        const dy = to.y - at.y;
        const len = Math.hypot(dx, dy);
        if (len > 1e-6) {
          const [ux, uy] = [dx / len, dy / len];
          const [nx, ny] = [-uy, ux];
          const HEAD = 9;
          const HALF = 3.6;
          const bx = to.x - ux * HEAD;
          const by = to.y - uy * HEAD;
          const head = document.createElementNS(NS, "polygon");
          head.setAttribute(
            "points",
            `${to.x},${to.y} ${bx + nx * HALF},${by + ny * HALF} ${bx - nx * HALF},${by - ny * HALF}`,
          );
          head.setAttribute("fill", "#d1495b");
          g.appendChild(head);
        }
      }

      // `text` is a caption with no marker; everything else marks a point.
      if (a.kind !== "text" && a.kind !== "arrow") {
        const dot = document.createElementNS(NS, "circle");
        dot.setAttribute("cx", String(at.x));
        dot.setAttribute("cy", String(at.y));
        dot.setAttribute("r", "4");
        dot.setAttribute("fill", "#d1495b");
        dot.setAttribute("stroke", "#ffffff");
        dot.setAttribute("stroke-width", "1.5");
        g.appendChild(dot);
      }

      const label = document.createElementNS(NS, "text");
      label.setAttribute("x", String(at.x + 8));
      label.setAttribute("y", String(at.y - 8));
      label.setAttribute("font-size", "13");
      label.setAttribute("fill", "#d1495b");
      label.setAttribute("data-qu-annotation", a.id);
      // Annotations are editable like any other text on the figure.
      label.setAttribute("pointer-events", "auto");
      label.textContent = a.text;
      g.appendChild(label);
    }
    el.appendChild(g);
  }, [svgEl, active, tool, readout, annotations, geometry, snappedTo, shapes, dragging]);

/** Grow or shrink a legend box to fit the labels currently in it.
 *
 * The engine sizes the legend when it draws the figure, from the labels
 * the script gave it. Rename one in the viewer and the box keeps its old
 * width -- a longer label spills straight out through the right-hand
 * border, a shorter one leaves a gap. Since the point of editing a legend
 * is usually to fix the wording, the box has to follow.
 *
 * The legend is the renderer's rounded panel (`rx="6"`); its rows are the
 * text nodes sitting inside it. Only the WIDTH is adjusted: row height and
 * count are unchanged by a rename, and moving rows around would fight the
 * engine's own layout for no benefit.
 */
function reflowLegendBoxes(root: SVGSVGElement, overlayId: string): void {
  const overlay = root.querySelector("#" + overlayId);
  const boxes = Array.from(root.querySelectorAll("rect[rx='6']")) as SVGRectElement[];
  for (const box of boxes) {
    const bx = parseFloat(box.getAttribute("x") ?? "NaN");
    const by = parseFloat(box.getAttribute("y") ?? "NaN");
    const bw = parseFloat(box.getAttribute("width") ?? "NaN");
    const bh = parseFloat(box.getAttribute("height") ?? "NaN");
    if (![bx, by, bw, bh].every(Number.isFinite)) continue;

    // Text nodes whose anchor point lies inside this box are its rows.
    let widest = -Infinity;
    let leftmost = Infinity;
    for (const node of Array.from(root.querySelectorAll("text"))) {
      if (overlay?.contains(node)) continue;
      const tx = parseFloat(node.getAttribute("x") ?? "NaN");
      const ty = parseFloat(node.getAttribute("y") ?? "NaN");
      if (!Number.isFinite(tx) || !Number.isFinite(ty)) continue;
      if (tx < bx || tx > bx + bw + 400 || ty < by || ty > by + bh) continue;
      let width = 0;
      try {
        width = (node as SVGTextElement).getBBox().width;
      } catch {
        // getBBox throws on a detached or hidden node; skip it rather
        // than resizing the box off one unmeasurable row.
        continue;
      }
      leftmost = Math.min(leftmost, tx);
      widest = Math.max(widest, tx + width);
    }
    if (!Number.isFinite(widest) || !Number.isFinite(leftmost)) continue;

    // Keep the padding the renderer used on the left of the text, and
    // mirror it on the right.
    const pad = Math.max(leftmost - bx, 6);
    const next = Math.max(widest - bx + pad, 24);
    if (Math.abs(next - bw) > 0.5) box.setAttribute("width", String(next));
  }
}

  /** Apply committed renames to the live SVG. Keyed by the element's index
   *  among `<text>` nodes, which is stable for a given figure. */
  useEffect(() => {
    const el = svgEl();
    if (!el) return;
    const texts = el.querySelectorAll("text");
    for (const [key, value] of Object.entries(edits)) {
      const node = texts[Number(key)];
      if (node && node.textContent !== value) node.textContent = value;
    }
    // A renamed legend entry has to be able to change the box it sits in.
    reflowLegendBoxes(el, OVERLAY_ID);
  }, [svgEl, edits, svg]);

  /** Start editing one `<text>` node in place. */
  const beginEdit = useCallback(
    (node: SVGTextElement) => {
      const host = hostRef.current;
      if (!host) return;
      const texts = Array.from(svgEl()?.querySelectorAll("text") ?? []);
      const index = texts.indexOf(node);
      const rect = node.getBoundingClientRect();
      const hostRect = host.getBoundingClientRect();
      const input = document.createElement("input");
      input.type = "text";
      input.value = node.textContent ?? "";
      input.className = "qu-figure-text-edit";
      // Positioned over the text it replaces, in the host's own coordinate
      // space, so it tracks the figure however it is scrolled or zoomed.
      input.style.left = (rect.left - hostRect.left + host.scrollLeft - 4) + "px";
      input.style.top = (rect.top - hostRect.top + host.scrollTop - 3) + "px";
      input.style.minWidth = Math.max(rect.width + 24, 90) + "px";
      const annotationId = node.getAttribute("data-qu-annotation");
      const finish = (commit: boolean) => {
        if (editingRef.current !== input) return;
        editingRef.current = null;
        const next = input.value;
        input.remove();
        if (!commit) return;
        if (annotationId) {
          setAnnotations((prev) =>
            prev.map((a) => (a.id === annotationId ? { ...a, text: next } : a)),
          );
        } else if (index >= 0) {
          setEdits((prev) => ({ ...prev, [index]: next }));
        }
      };
      input.addEventListener("keydown", (event) => {
        // Enter commits, Escape abandons -- the pairing every in-place
        // rename uses, and what was asked for here.
        if (event.key === "Enter") {
          event.preventDefault();
          finish(true);
        } else if (event.key === "Escape") {
          event.preventDefault();
          finish(false);
        }
        event.stopPropagation();
      });
      // Clicking away keeps what you typed: losing a rename because you
      // looked elsewhere is the more annoying of the two failures.
      input.addEventListener("blur", () => finish(true));
      host.appendChild(input);
      editingRef.current = input;
      input.focus();
      input.select();
    },
    [hostRef, svgEl],
  );

  /** Pointer wiring, attached to the host element rather than through React
   *  props: the figure is injected with `dangerouslySetInnerHTML`, so React
   *  does not own those nodes and cannot attach handlers to them. */
  useEffect(() => {
    const host = hostRef.current;
    if (!host || !active) return;

    const onMove = (event: MouseEvent) => {
      if (!READOUT_TOOLS.includes(tool) || !geometry) return;
      const raw = toUser(event.clientX, event.clientY);
      const p = raw ? panelAt(geometry, raw.x, raw.y) : null;
      if (!raw || !p) {
        setReadout(null);
        setSnappedTo(null);
        return;
      }
      const { point, snapped } = withSnap(raw);
      const d = dataAt(p, point.x, point.y);
      setReadout({
        x: d.x,
        y: d.y,
        panel: p.panel,
        // A snapped value came off a drawn vertex, so it carries only the
        // renderer's two-decimal positional precision; an unsnapped one is
        // a genuine pointer position and can be shown in full.
        label: snapped
          ? formatSnapped(d.x, axisResolution(p, "x")) +
            " · y = " +
            formatSnapped(d.y, axisResolution(p, "y"))
          : null,
      });
      setSnappedTo(snapped ? point : null);
    };
    const onLeave = () => setReadout(null);

    const onClick = (event: MouseEvent) => {
      if (tool === "annotate" || tool === "text") {
        if (!geometry) return;
        const raw = toUser(event.clientX, event.clientY);
        const p = raw ? panelAt(geometry, raw.x, raw.y) : null;
        if (!raw || !p) return;
        // Annotations snap, so a mark lands ON the point it is about
        // rather than a few pixels off it -- which reads as a mistake in a
        // finished figure. Free TEXT never snaps: it is a caption placed
        // where there is room, not a claim about a data point.
        const { point } = tool === "text" ? { point: raw } : withSnap(raw);
        const d = dataAt(p, point.x, point.y);
        setAnnotations((prev) =>
          prev.concat({
            id: "a" + Date.now() + prev.length,
            panel: p.panel,
            kind: tool === "text" ? "text" : "annotate",
            x: d.x,
            y: d.y,
            // An annotation is seeded with the value it marks, which is
            // what one on a plot is usually for, at the precision the
            // figure can actually resolve -- so a mark on the point at
            // x = 5 says "5", not "5.00003". Free text starts as a
            // placeholder to retype. Double-click either to change it.
            text:
              tool === "text"
                ? "Text"
                : "(" +
                  formatSnapped(d.x, axisResolution(p, "x")) +
                  ", " +
                  formatSnapped(d.y, axisResolution(p, "y")) +
                  ")",
          }),
        );
        return;
      }
      if (tool === "select") {
        const target = event.target as Element;
        setSelected(target.tagName.toLowerCase() === "text" ? target.textContent ?? "" : null);
        return;
      }
      // Reference lines are placed with a single click.
      if (tool === "vline" || tool === "hline") {
        if (!geometry) return;
        const raw = toUser(event.clientX, event.clientY);
        const p = raw ? panelAt(geometry, raw.x, raw.y) : null;
        if (!raw || !p) return;
        const { point } = withSnap(raw);
        const d = dataAt(p, point.x, point.y);
        const at = tool === "vline" ? d.x : d.y;
        setShapes((prev) =>
          prev.concat({
            id: "s" + Date.now() + prev.length,
            tool,
            // Dashed by default: a reference line is a threshold or a
            // limit, not the subject of the figure, and drawing it solid
            // makes it compete with the data.
            code: `${tool}(${forCode(at)}, dash=true)`,
            x0: tool === "vline" ? d.x : p.xmin,
            y0: tool === "hline" ? d.y : p.ymin,
            x1: tool === "vline" ? d.x : p.xmax,
            y1: tool === "hline" ? d.y : p.ymax,
            panel: p.panel,
          }),
        );
      }
    };

    const onDown = (event: MouseEvent) => {
      if (!DRAG_TOOLS.includes(tool) || !geometry || event.button !== 0) return;
      const raw = toUser(event.clientX, event.clientY);
      const p = raw ? panelAt(geometry, raw.x, raw.y) : null;
      if (!raw || !p) return;
      event.preventDefault();
      const d = dataAt(p, raw.x, raw.y);
      dragStart.current = { x: d.x, y: d.y, panel: p.panel };
      setDragging({ x0: d.x, y0: d.y, x1: d.x, y1: d.y, panel: p.panel });
    };

    const onDrag = (event: MouseEvent) => {
      const start = dragStart.current;
      if (!start || !geometry) return;
      const raw = toUser(event.clientX, event.clientY);
      const p = raw ? geometry.panels.find((q) => q.panel === start.panel) : null;
      if (!raw || !p) return;
      const d = dataAt(p, raw.x, raw.y);
      setDragging({ x0: start.x, y0: start.y, x1: d.x, y1: d.y, panel: start.panel });
    };

    const onUp = () => {
      const start = dragStart.current;
      dragStart.current = null;
      const region = dragging;
      setDragging(null);
      if (!start || !region) return;
      const [lo, hi] = [Math.min(region.x0, region.x1), Math.max(region.x0, region.x1)];
      const [ylo, yhi] = [Math.min(region.y0, region.y1), Math.max(region.y0, region.y1)];
      // A click that barely moved is not a region. Without this every
      // stray click on the plot would add a zero-width span.
      if (Math.abs(hi - lo) < 1e-12 && Math.abs(yhi - ylo) < 1e-12) return;
      // Dragging in annotate mode is an ARROW: from where you pressed to
      // where you released, which is how you point at a feature from a
      // clear patch of the plot.
      if (tool === "annotate") {
        setAnnotations((prev) =>
          prev.concat({
            id: "a" + Date.now() + prev.length,
            panel: region.panel,
            kind: "arrow",
            x: region.x0,
            y: region.y0,
            toX: region.x1,
            toY: region.y1,
            text: "Label",
          }),
        );
        return;
      }
      const code =
        tool === "xspan"
          ? `xspan(${forCode(lo)}, ${forCode(hi)})`
          : `zoom_inset(${forCode(lo)}, ${forCode(hi)}, ${forCode(ylo)}, ${forCode(yhi)})`;
      setShapes((prev) =>
        prev.concat({
          id: "s" + Date.now() + prev.length,
          tool: tool as GeneratedShape["tool"],
          code,
          x0: lo,
          y0: ylo,
          x1: hi,
          y1: yhi,
          panel: region.panel,
        }),
      );
    };

    const onDoubleClick = (event: MouseEvent) => {
      // Double-click in annotate mode drops a highlighted point labelled
      // with its own coordinates -- `point(x, y)` in Qu. It is the "this
      // exact sample" mark, as opposed to a click's free-text annotation.
      if (tool === "annotate") {
        if (!geometry) return;
        const raw = toUser(event.clientX, event.clientY);
        const p = raw ? panelAt(geometry, raw.x, raw.y) : null;
        if (!raw || !p) return;
        event.preventDefault();
        const { point } = withSnap(raw);
        const d = dataAt(p, point.x, point.y);
        setAnnotations((prev) => {
          // A double-click also fires a click, which already added a plain
          // annotation at this spot. Replace it rather than stacking two
          // marks on one point.
          const near = prev.findIndex(
            (a) =>
              a.panel === p.panel &&
              Math.abs(a.x - d.x) < 1e-9 &&
              Math.abs(a.y - d.y) < 1e-9,
          );
          const mark: Annotation = {
            id: "a" + Date.now() + prev.length,
            panel: p.panel,
            kind: "point",
            x: d.x,
            y: d.y,
            text:
              "(" +
              formatSnapped(d.x, axisResolution(p, "x")) +
              ", " +
              formatSnapped(d.y, axisResolution(p, "y")) +
              ")",
          };
          if (near >= 0) {
            const next = prev.slice();
            next[near] = mark;
            return next;
          }
          return prev.concat(mark);
        });
        return;
      }
      if (tool !== "select") return;
      const target = event.target as Element;
      if (target.tagName.toLowerCase() !== "text") return;
      event.preventDefault();
      event.stopPropagation();
      beginEdit(target as SVGTextElement);
    };

    host.addEventListener("mousedown", onDown);
    host.addEventListener("mousemove", onDrag);
    host.addEventListener("mouseup", onUp);
    host.addEventListener("mousemove", onMove);
    host.addEventListener("mouseleave", onLeave);
    host.addEventListener("click", onClick);
    host.addEventListener("dblclick", onDoubleClick);
    return () => {
      host.removeEventListener("mousedown", onDown);
      host.removeEventListener("mousemove", onDrag);
      host.removeEventListener("mouseup", onUp);
      host.removeEventListener("mousemove", onMove);
      host.removeEventListener("mouseleave", onLeave);
      host.removeEventListener("click", onClick);
      host.removeEventListener("dblclick", onDoubleClick);
    };
  }, [hostRef, active, tool, geometry, toUser, beginEdit, withSnap, dragging]);

  /** The figure as it now stands -- annotations and renames included -- so
   *  what gets downloaded is what is on screen. The crosshair is a pointer,
   *  not part of the figure, so it is stripped. */
  const exportSvg = useCallback((): string | null => {
    const el = svgEl();
    if (!el) return null;
    const clone = el.cloneNode(true) as SVGSVGElement;
    clone.querySelectorAll("." + CROSSHAIR_CLASS).forEach((n) => n.remove());
    return new XMLSerializer().serializeToString(clone);
  }, [svgEl]);

  return {
    readout,
    annotations,
    hasGeometry: !!geometry,
    snappedTo,
    shapes,
    /** The generated Qu for everything placed, in order.
     *
     * Annotations are included, not just shapes. A mark that lives only in
     * the viewer vanishes on the next run; the line of code is what makes
     * it part of the figure. Each maps to the builtin that draws exactly
     * what the preview showed.
     */
    shapeCode: shapes
      .map((s) => s.code)
      .concat(
        annotations.map((a) => {
          const t = quote(a.text);
          switch (a.kind) {
            case "point":
              // `point` labels itself with its own coordinates, so passing
              // text would duplicate what it already prints.
              return `point(${forCode(a.x)}, ${forCode(a.y)})`;
            case "arrow":
              return `arrowtext(${forCode(a.x)}, ${forCode(a.y)}, ${forCode(a.toX ?? a.x)}, ${forCode(a.toY ?? a.y)}, ${t})`;
            case "text":
              return `text(${forCode(a.x)}, ${forCode(a.y)}, ${t})`;
            default:
              return `annotate(${forCode(a.x)}, ${forCode(a.y)}, ${t})`;
          }
        }),
      ),
    clearShapes: () => {
      setShapes([]);
      setAnnotations([]);
    },
    removeShape: (id: string) => setShapes((prev) => prev.filter((s) => s.id !== id)),
    selected,
    edited: Object.keys(edits).length > 0 || annotations.length > 0,
    clearAnnotations: useCallback(() => setAnnotations([]), []),
    exportSvg,
  };
}
