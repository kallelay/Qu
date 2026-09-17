/// <reference path="../types/react-plotly.d.ts" />
import React, { useState, useRef, useEffect, useMemo } from "react";
import Plot from "react-plotly.js";
import Plotly from "plotly.js/dist/plotly";
import {
  Download,
  Grid3X3,
  Hand,
  Maximize2,
  Minimize2,
  RotateCcw,
  ZoomIn,
  Lock,
  Unlock,
  LineChart,
  SlidersHorizontal,
} from "lucide-react";
import { cn } from "../utils/cn";
import { axisRange, plotTheme, targetTheme, PLOT_FONT, PLOT_METRICS, PLOT_COLORWAY, type PlotStyle, type PlotTarget } from "../utils/plotStyle";
import { FigureDesignPanel } from "./FigureDesignPanel";
import {
  DEFAULT_FIGURE_DESIGN,
  figureComposition,
  figureLayerKeys,
  plotText,
  styleFigureLayers,
} from "../utils/figureDesign";
import type {
  FigureDesign,
  FigureLabels,
  LayerDesign,
} from "../utils/figureDesign";
import "./inspector.css";

export type PlotType =
  | "line"
  | "scatter"
  | "scattergl"
  | "bar"
  | "histogram"
  | "heatmap"
  | "contour"
  | "surface"
  | "scatter3d"
  | "mesh3d";
export interface PlotData {
  uid?: string;
  visible?: boolean | "legendonly";
  opacity?: number;
  showlegend?: boolean;
  legendgroup?: string;
  x?: (number | null)[];
  y?: (number | null)[];
  z?: (number | null)[] | (number | null)[][];
  type?: PlotType;
  name?: string;
  mode?: string;
  line?: { width?: number; color?: string; dash?: string };
  marker?: { size?: number; color?: string; symbol?: string };
  colorscale?: string | [number, string][];
}
export interface PlotViewerProps {
  data?: PlotData | PlotData[];
  title?: string;
  subtitle?: string;
  caption?: string;
  legendTitle?: string;
  xlabel?: string;
  ylabel?: string;
  zlabel?: string;
  width?: string | number;
  height?: string | number;
  logx?: boolean;
  logy?: boolean;
  logz?: boolean;
  xlim?: [number, number];
  ylim?: [number, number];
  zlim?: [number, number];
  showLegend?: boolean;
  showGrid?: boolean;
  theme?: "light" | "dark" | "system";
  className?: string;
  onExport?: (format: "png" | "svg" | "pdf") => void;
  /** Animate compatible data updates through Plotly.react. */
  animate?: boolean;
  transitionDuration?: number;
  /** Equal physical units on both axes (for example, Nyquist plots). */
  equalAspect?: boolean;
  /** Change to reset the viewport; keep stable to retain a user's pan/zoom. */
  viewRevision?: string | number;
}

export const PlotViewer: React.FC<PlotViewerProps> = ({
  data = [],
  title = "",
  subtitle = "",
  caption = "",
  legendTitle = "",
  xlabel = "",
  ylabel = "",
  zlabel = "",
  width = "100%",
  height = 400,
  logx = false,
  logy = false,
  logz = false,
  xlim,
  ylim,
  zlim,
  showLegend = true,
  showGrid = true,
  theme = "system",
  className,
  onExport,
  animate = true,
  transitionDuration = 300,
  equalAspect = false,
  viewRevision = 0,
}) => {
  const [systemDark, setSystemDark] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [style, setStyle] = useState<PlotStyle>("grey");
  // This viewer backs Interactive and Live -- tools you drive on a monitor,
  // in real time. It is always a SCREEN figure. Publication styling belongs
  // to the figures a script renders for export (the engine's own plotting),
  // not here: switching a live exploration view to print type and print
  // weights makes it worse at the only job it has.
  const target: PlotTarget = "screen";
  /** Frozen: the data shown is a snapshot, and incoming `data` updates are
   *  ignored until unfrozen. The workflow this exists for: run the script
   *  once, freeze the result, then shape its APPEARANCE without the risk of
   *  a re-run recomputing (possibly expensively) underneath you or throwing
   *  away the styling you just set. Unfreezing hands control back to the
   *  code. Only data is frozen -- every design control stays live, which is
   *  the whole point. */
  const [frozen, setFrozen] = useState(false);
  const frozenTraces = useRef<PlotData[] | null>(null);
  const [grid, setGrid] = useState(showGrid);
  const [drag, setDrag] = useState<"zoom" | "pan">("zoom");
  const [revision, setRevision] = useState(0);
  const [format, setFormat] = useState<"png" | "svg">("png");
  const [exporting, setExporting] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [designOpen, setDesignOpen] = useState(false);
  const [design, setDesign] = useState<FigureDesign>(DEFAULT_FIGURE_DESIGN);
  const [labelEdits, setLabelEdits] = useState<Partial<FigureLabels>>({});
  const [layers, setLayers] = useState<Record<string, LayerDesign>>({});
  const [canvasSize, setCanvasSize] = useState({ width: 800, height: 360 });
  const colorSlotsRef = useRef<Record<string, number>>({});
  const nextColorSlotRef = useRef(0);
  const designButtonRef = useRef<HTMLButtonElement>(null);
  // Memoized because `layout` depends on it: an object literal rebuilt each
  // render would make the layout memo recompute every time, which hands
  // <Plot> a new layout and sends the whole figure back through
  // Plotly.react -- exactly the full invalidation a data-only update is
  // supposed to avoid.
  const labels: FigureLabels = useMemo(
    () => ({ title, subtitle, caption, xlabel, ylabel, zlabel, legendTitle, ...labelEdits }),
    [title, subtitle, caption, xlabel, ylabel, zlabel, legendTitle, labelEdits],
  );
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const graphRef = useRef<HTMLElement | null>(null);
  const resolvedTheme =
    theme === "system" ? (systemDark ? "dark" : "light") : theme;
  // Freeze gate. While frozen the figure keeps exactly the traces it had at
  // the moment of freezing and ignores whatever the caller sends after --
  // so a re-run (a nudged slider, a watcher firing, an expensive recompute)
  // cannot replace the figure you are in the middle of preparing for a
  // paper. Styling is deliberately NOT frozen: every design control below
  // still applies live, which is the entire point of the mode.
  const sourceTraces = useMemo(() => {
    const live = Array.isArray(data) ? data : [data];
    if (!frozen) {
      frozenTraces.current = null;
      return live;
    }
    if (frozenTraces.current === null) frozenTraces.current = live;
    return frozenTraces.current;
  }, [data, frozen]);
  const layerKeys = useMemo(
    () => figureLayerKeys(sourceTraces),
    [sourceTraces],
  );
  const colorSlots = useMemo(() => {
    const next: Record<string, number> = {};
    for (const key of layerKeys) next[key] = colorSlotsRef.current[key] ?? nextColorSlotRef.current++;
    colorSlotsRef.current = next;
    return next;
  }, [layerKeys]);
  const traces = useMemo(
    () => styleFigureLayers(sourceTraces, layerKeys, layers, design, colorSlots, resolvedTheme === 'dark'),
    [sourceTraces, layerKeys, layers, design, colorSlots, resolvedTheme],
  );
  const is3d = traces.some((trace) =>
    ["surface", "scatter3d", "mesh3d"].includes(trace.type),
  );
  const hasData = traces.some(
    (trace) =>
      (trace.x?.length ?? 0) + (trace.y?.length ?? 0) + (trace.z?.length ?? 0) >
      0,
  );

  // ---- Data-only updates -----------------------------------------------
  //
  // When only the NUMBERS change -- a slider moved, the same curves with new
  // values -- the figure must not be handed a new `data` prop, because that
  // sends the whole thing back through `Plotly.react` to be diffed and
  // re-laid-out. Instead the existing traces are mutated in place with
  // `Plotly.restyle`, which moves the points (and their colours) on the
  // live figure and touches nothing else: no re-inserted graph, no rebuilt
  // scene, no reset camera, and no work proportional to anything but the
  // data itself.
  //
  // `structureSignature` is what decides which path applies. Trace count,
  // types, names and visibility ARE structure; the numeric arrays are not.
  // A structural change (a new series, a switch from surface to scatter3d)
  // still goes through React and `Plotly.react`, because that genuinely is
  // a different figure.
  const structureSignature = useMemo(
    () =>
      traces
        .map((t) => `${t.type}|${t.name ?? ""}|${t.visible ?? true}|${t.mode ?? ""}`)
        .join("~"),
    [traces],
  );
  const lastSignatureRef = useRef<string | null>(null);
  // What `<Plot>` is actually given. Held stable across data-only updates on
  // purpose: keeping the same array identity is what stops react-plotly
  // from re-reacting behind our back after we have already restyled.
  const [plotData, setPlotData] = useState<PlotData[]>(traces);

  useEffect(() => {
    const gd = graphRef.current;
    const structural = structureSignature !== lastSignatureRef.current;
    if (structural || !gd || !ready) {
      lastSignatureRef.current = structureSignature;
      setPlotData(traces);
      return;
    }
    // Data-only: move the points where they now belong.
    //
    // `Plotly.animate` is preferred over `restyle` here because it TWEENS
    // between the old and new values -- the points travel to their new
    // positions instead of snapping -- while still touching only trace
    // data. `redraw: false` keeps it a pure data tween: no relayout, no
    // re-inserted graph, no rebuilt scene.
    //
    // WebGL traces are excluded deliberately. Plotly's animation machinery
    // covers SVG cartesian traces; handed a gl trace it either no-ops or
    // throws, depending on type and version. That covers gl3d
    // (surface/scatter3d/mesh3d) and equally `scattergl`, which is what
    // this panel's own "2D (WebGL)" mode produces -- so the throw landed on
    // the single most common path here, on every slider move, and took the
    // whole figure down with it. Those take `restyle`: still an in-place
    // data swap that moves the points and rebuilds nothing, just an
    // instant one rather than a tweened one.
    const usesGl = traces.some((t) => typeof t.type === "string" && t.type.endsWith("gl"));
    const tweenable = animate && !is3d && !usesGl;
    const update = {
      x: traces.map((t) => t.x),
      y: traces.map((t) => t.y),
      z: traces.map((t) => t.z),
      "marker.color": traces.map((t) => t.marker?.color),
      "line.color": traces.map((t) => t.line?.color),
      // Cast: Plotly's typings describe these value arrays more narrowly
      // than the API accepts (one entry per trace, each itself an array).
    } as unknown as Partial<Record<string, unknown>>;

    // Both branches can fail SYNCHRONOUSLY -- Plotly throws rather than
    // returning a rejected promise for several shapes it won't accept. A
    // throw here escapes the effect and React unmounts the panel, so the
    // failure mode is a dead figure and an error banner, not a stale
    // frame. Catch both kinds and fall back to the honest full update.
    let inPlace: unknown;
    try {
      inPlace = tweenable
        ? Plotly.animate(
            gd,
            { data: traces as unknown as Plotly.Data[] },
            {
              transition: { duration: transitionDuration, easing: "cubic-in-out" },
              frame: { duration: transitionDuration, redraw: false },
            },
          )
        : Plotly.restyle(gd, update);
    } catch {
      setPlotData(traces);
      return;
    }

    void Promise.resolve(inPlace).catch(() => {
      setPlotData(traces);
    });
  }, [traces, structureSignature, ready, animate, is3d, transitionDuration]);

  // Same reasoning as `labels`: a fresh palette object each render would
  // invalidate the layout memo and therefore the figure.
  const colors = useMemo(
    () => targetTheme(target, style, resolvedTheme),
    [target, style, resolvedTheme],
  );
  const metrics = PLOT_METRICS[target];
  const legendVisible = showLegend && design.legendPosition !== "none";
  // The trace NAMES matter to composition (legend sizing), the trace DATA
  // does not -- so this is keyed on the names, letting a data-only update
  // leave the composition, and therefore the layout, untouched.
  const layerNames = useMemo(
    () =>
      sourceTraces.map(
        (trace, index) =>
          layers[layerKeys[index]]?.name ?? trace.name ?? `Layer ${index + 1}`,
      ),
    [sourceTraces, layers, layerKeys],
  );
  const layerNameKey = layerNames.join("~");
  const composition = useMemo(
    () =>
      figureComposition(
        labels,
        design,
        canvasSize.width,
        layerNames,
        legendVisible,
        colors.ink,
        resolvedTheme === "dark" ? "#a2afc1" : "#667386",
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [labels, design, canvasSize.width, layerNameKey, legendVisible, colors.ink, resolvedTheme],
  );
  const plotHeight = Math.max(
    50,
    canvasSize.height - composition.margin.t - composition.margin.b,
  );
  const axis = (label: string, log: boolean, limits?: [number, number]) => ({
    title: {
      text: plotText(label),
      standoff: 12,
      font: { size: design.textSize },
    },
    type: log ? "log" : "linear",
    range: axisRange(limits, log),
    autorange: limits ? false : true,
    automargin: true,
    showgrid: grid && style !== "classic",
    gridcolor: colors.grid,
    zeroline: false,
    showline: style === "classic",
    linewidth: metrics.axis,
    linecolor: colors.ink,
    ticks: style === "classic" ? "outside" : "",
    ticklen: 3,
    tickwidth: metrics.axis,
    tickcolor: colors.grid,
    tickfont: { size: design.textSize - 1 },
    nticks: Math.max(4, Math.min(8, Math.floor(canvasSize.width / 100))),
  });
  // Memoized so a data-only update doesn't hand Plotly a brand-new layout
  // object every render. `Plotly.react` diffs what it's given; a fresh
  // layout each time invites needless relayout work on a figure that is
  // supposed to be having only its data swapped.
  const layout = useMemo(() => ({
    autosize: true,
    paper_bgcolor: colors.paper,
    plot_bgcolor: colors.panel,
    // Publication swaps to Okabe-Ito, which is colourblind-safe -- a
    // printed figure can't ask its reader. See `PLOT_COLORWAY`.
    colorway: PLOT_COLORWAY[target],
    font: {
      family: PLOT_FONT[target],
      size: design.textSize,
      color: colors.ink,
    },
    margin: composition.margin,
    annotations: composition.annotations,
    xaxis: axis(labels.xlabel, logx, xlim),
    yaxis: { ...axis(labels.ylabel, logy, ylim), ...(equalAspect ? { scaleanchor: 'x', scaleratio: 1 } : {}) },
    showlegend: legendVisible,
    legend: {
      ...composition.legend,
      y: composition.rightLegend ? 1 : -composition.axisBottom / plotHeight,
    },
    hovermode: "closest",
    hoverlabel: {
      bgcolor: colors.paper,
      bordercolor: colors.grid,
      font: { color: colors.ink },
    },
    dragmode: drag,
    uirevision: `${revision}:${viewRevision}:${equalAspect}:${is3d}:${logx}:${logy}:${logz}:${xlabel}:${ylabel}:${zlabel}`,
    // 3D scenes also need labels/ranges when the caller provides one trace
    // or omits zlabel (neither case should disable the entire scene setup).
    ...(is3d
      ? {
          scene: {
            xaxis: axis(labels.xlabel, logx, xlim),
            yaxis: axis(labels.ylabel, logy, ylim),
            zaxis: axis(labels.zlabel, logz, zlim),
            bgcolor: colors.panel,
            dragmode: drag === "pan" ? "pan" : "orbit",
          },
        }
      : {}),
    ...(animate
      ? { transition: { duration: transitionDuration, easing: "cubic-in-out" } }
      : {}),
  }), [
    title, xlabel, ylabel, zlabel, logx, logy, logz, xlim, ylim, zlim,
    showLegend, grid, drag, revision, is3d, colors, style, animate, transitionDuration,
    target, metrics, design, labels, composition, legendVisible, equalAspect,
  ]);

  useEffect(() => {
    setGrid(showGrid);
  }, [showGrid]);
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    const update = () =>
      setIsFullscreen(document.fullscreenElement === containerRef.current);
    document.addEventListener("fullscreenchange", update);
    return () => document.removeEventListener("fullscreenchange", update);
  }, []);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || typeof ResizeObserver === "undefined") return;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (canvas.clientWidth && canvas.clientHeight) {
          const width = Math.round(canvas.clientWidth),
            height = Math.round(canvas.clientHeight);
          setCanvasSize((previous) =>
            previous.width === width && previous.height === height
              ? previous
              : { width, height },
          );
        }
        if (graphRef.current && canvas.clientWidth && canvas.clientHeight) {
          void Promise.resolve(Plotly.Plots.resize(graphRef.current)).catch(
            () => {
              /* Hidden during a tab change. */
            },
          );
        }
      });
    });
    observer.observe(canvas);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [hasData]);

  const fullscreen = async () => {
    try {
      if (document.fullscreenElement === containerRef.current)
        await document.exitFullscreen();
      else await containerRef.current?.requestFullscreen();
    } catch {
      setError("Fullscreen is unavailable in this window.");
    }
  };
  const reset = async () => {
    if (!graphRef.current) return;
    setRevision((value) => value + 1);
    try {
      const updates: Record<string, unknown> = {};
      for (const [key, limits, log] of [
        ["xaxis", xlim, logx],
        ["yaxis", ylim, logy],
        ["zaxis", zlim, logz],
      ] as const) {
        if (!is3d && key === "zaxis") continue;
        const prefix = is3d ? `scene.${key}` : key;
        const range = axisRange(limits, log);
        updates[`${prefix}.autorange`] = !range;
        if (range) updates[`${prefix}.range`] = range;
      }
      if (is3d)
        updates["scene.camera"] = {
          eye: { x: 1.25, y: 1.25, z: 1.25 },
          center: { x: 0, y: 0, z: 0 },
          up: { x: 0, y: 0, z: 1 },
        };
      await Plotly.relayout(graphRef.current, updates);
    } catch {
      setError("Could not reset the plot view.");
    }
  };
  const exportImage = async () => {
    if (!graphRef.current) return;
    setExporting(true);
    setError("");
    try {
      const bounds = graphRef.current.getBoundingClientRect();
      const exportOptions = {
        format,
        width: Math.round(bounds.width || 900),
        height: Math.round(bounds.height || 600),
        // Scale the complete composition, including fonts and line weights.
        scale: Math.max(1, 1600 / (bounds.width || 900)),
        filename:
          labels.title.replace(/[^a-zA-Z0-9_-]+/g, "-").replace(/^-|-$/g, "") ||
          "qu-plot",
      };
      await Plotly.downloadImage(graphRef.current, exportOptions);
      onExport?.(format);
    } catch {
      setError("Export failed. Please try again.");
    } finally {
      setExporting(false);
    }
  };
  return (
    <div
      ref={containerRef}
      className={cn("qu-inspector qu-plot", className)}
      data-theme={resolvedTheme}
      style={{ width, height }}
      aria-label={labels.title || "Interactive plot"}
    >
      <div className="qu-toolbar">
        <LineChart size={14} />
        <span className="qu-plot-title qu-truncate">Figure</span>
        <span className="qu-spacer" />
        <button
          ref={designButtonRef}
          className="qu-text-button"
          aria-label="Edit figure design"
          aria-expanded={designOpen}
          onClick={() => setDesignOpen(!designOpen)}
        >
          <SlidersHorizontal size={14} />
          Design
        </button>
        <select
          aria-label="Plot theme"
          value={style}
          onChange={(event) => setStyle(event.target.value as PlotStyle)}
        >
          <option value="grey">Grey</option>
          <option value="minimal">Minimal</option>
          <option value="classic">Classic</option>
        </select>
        {/* Freeze pins the DATA and lets you keep shaping the appearance:
            run once, freeze, then style it without a re-run recomputing
            underneath you or discarding the styling you just set. */}
        <button
          className="qu-icon-button"
          aria-label={frozen ? "Unfreeze data" : "Freeze data"}
          aria-pressed={frozen}
          title={
            frozen
              ? "Frozen: showing a snapshot. Styling still applies; re-runs are ignored. Click to unfreeze."
              : "Freeze this data, then shape the figure without re-runs replacing it."
          }
          onClick={() => setFrozen((value) => !value)}
        >
          {frozen ? <Lock size={16} /> : <Unlock size={16} />}
        </button>
        <button
          className="qu-icon-button"
          aria-label="Toggle grid"
          title="Toggle grid"
          aria-pressed={grid && style !== "classic"}
          disabled={style === "classic"}
          onClick={() => setGrid(!grid)}
        >
          <Grid3X3 size={14} />
        </button>
        <button
          className="qu-icon-button"
          aria-label={is3d ? "Rotate plot" : "Zoom plot"}
          title={is3d ? "Drag to rotate" : "Drag to zoom"}
          aria-pressed={drag === "zoom"}
          onClick={() => setDrag("zoom")}
        >
          <ZoomIn size={14} />
        </button>
        <button
          className="qu-icon-button"
          aria-label="Pan plot"
          title="Drag to pan"
          aria-pressed={drag === "pan"}
          onClick={() => setDrag("pan")}
        >
          <Hand size={14} />
        </button>
        <button
          className="qu-icon-button"
          aria-label="Reset plot view"
          title="Reset view"
          disabled={!ready || !hasData}
          onClick={reset}
        >
          <RotateCcw size={14} />
        </button>
        <select
          aria-label="Plot export format"
          value={format}
          onChange={(event) => setFormat(event.target.value as "png" | "svg")}
        >
          <option value="png">PNG</option>
          <option value="svg">SVG</option>
        </select>
        <button
          className="qu-icon-button"
          aria-label="Download plot"
          title={exporting ? "Exporting…" : `Download ${format.toUpperCase()}`}
          disabled={exporting || !ready || !hasData}
          onClick={exportImage}
        >
          <Download size={14} />
        </button>
        <button
          className="qu-icon-button"
          aria-label={isFullscreen ? "Exit fullscreen" : "Fullscreen plot"}
          title={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
          onClick={fullscreen}
        >
          {isFullscreen ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </button>
      </div>
      {error && (
        <div className="qu-plot-error" role="alert">
          {error}
        </div>
      )}
      <div className="qu-plot-body">
        <div ref={canvasRef} className="qu-plot-canvas">
          {/* The figure is mounted ONCE and kept. It used to be rendered
              conditionally on `hasData`, which meant every transition
              through "no data" unmounted the whole plot -- tearing down the
              WebGL scene of a 3D surface and, with it, the camera the user
              had set. A re-run then rebuilt the figure from scratch instead
              of swapping its data, so nothing could be tweened between and
              the view reset on every slider move. Keeping <Plot> mounted
              lets `Plotly.react` do what it is for: diff the new data
              against the live figure and update in place. The empty state
              is now an overlay on top rather than a replacement for it. */}
          <Plot
              data={plotData}
              layout={layout}
              config={{
                responsive: true,
                displayModeBar: false,
                displaylogo: false,
                scrollZoom: false,
                doubleClick: "reset",
              }}
              style={{ width: "100%", height: "100%" }}
              useResizeHandler
              onInitialized={(_, graph) => {
                graphRef.current = graph;
                setReady(true);
              }}
              onUpdate={(_, graph) => {
                graphRef.current = graph;
              }}
              onPurge={() => {
                graphRef.current = null;
                setReady(false);
              }}
              onError={(err: Error) => setError(err.message)}
              onRestyle={([update, indices]: [
                Record<string, unknown>,
                number[],
              ]) => {
                if (!("visible" in update)) return;
                setLayers((previous) => {
                  const next = { ...previous };
                  indices.forEach((index, position) => {
                    const visible = Array.isArray(update.visible)
                      ? update.visible[position % update.visible.length]
                      : update.visible;
                    if (
                      layerKeys[index] &&
                      (typeof visible === "boolean" || visible === "legendonly")
                    )
                      next[layerKeys[index]] = {
                        ...next[layerKeys[index]],
                        visible,
                      };
                  });
                  return next;
                });
              }}
            />
          {!hasData && (
            <div className="qu-empty qu-empty-overlay">
              <LineChart size={28} />
              <strong>A canvas for your data</strong>
              <p>
                Run your code and choose numeric variables to start plotting.
              </p>
            </div>
          )}
        </div>
        {designOpen && (
          <FigureDesignPanel
            labels={labels}
            design={design}
            traces={sourceTraces}
            layerKeys={layerKeys}
            layers={layers}
            colorSlots={colorSlots}
            dark={resolvedTheme === 'dark'}
            is3d={is3d}
            onLabel={(key, value) =>
              setLabelEdits((previous) => ({ ...previous, [key]: value }))
            }
            onDesign={(patch) =>
              setDesign((previous) => ({ ...previous, ...patch }))
            }
            onLayer={(key, patch) =>
              setLayers((previous) => ({
                ...previous,
                [key]: { ...previous[key], ...patch },
              }))
            }
            onReset={() => {
              setLabelEdits({});
              setDesign(DEFAULT_FIGURE_DESIGN);
              setLayers({});
            }}
            onClose={() => {
              setDesignOpen(false);
              designButtonRef.current?.focus();
            }}
          />
        )}
      </div>
      <div className="qu-plot-footer">
        {hasData
          ? `${is3d ? "Drag to rotate or pan" : "Drag to zoom or pan"} · Double-click to reset`
          : "Waiting for data"}
        {is3d && format === "svg" ? " · 3D layers are rasterized in SVG" : ""}
      </div>
    </div>
  );
};
export default PlotViewer;
