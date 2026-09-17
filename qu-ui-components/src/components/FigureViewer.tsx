import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  BookOpen,
  ChevronLeft,
  Columns,
  Code2,
  Copy,
  Crosshair,
  FilePlus2,
  Hand,
  Magnet,
  Maximize,
  MoveHorizontal,
  MoveVertical,
  MousePointer2,
  Trash2,
  Type,
  ChevronRight,
  Download,
  Expand,
  Image,
  Layers,
  Monitor,
  Save,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { cn } from "../utils/cn";
import { downloadBlob } from "../utils/variableData";
import { formatCoord } from "../utils/figureGeometry";
import { useFigureTools, type FigureTool } from "../utils/useFigureTools";
import "./inspector.css";

/** Keep rendered SVG inline so Qu's data-point <title> tooltips still work. */
function decodeSvgDataUri(src: string): string | null {
  const match = /^data:image\/svg\+xml;base64,(.+)$/.exec(src);
  if (!match) return null;
  try {
    const bytes = Uint8Array.from(atob(match[1]), (char) => char.charCodeAt(0));
    return new TextDecoder("utf-8").decode(bytes);
  } catch {
    return null;
  }
}

/** Rasterize each figure once, for the panel's thumbnails only.
 *
 * The side panel used to mount every figure as live inline SVG -- a full
 * DOM tree per figure, every one of Qu's data-point `<title>` tooltips
 * included. For a spectrogram that is thousands of nodes the panel is too
 * small to read anyway, and the cost is paid on every render. A raster
 * thumbnail is one `<img>`.
 *
 * The SVG itself is NOT thrown away: full screen still mounts it inline,
 * which is where hovering a mark for its value actually works. Raster is
 * the preview; vector is the real thing.
 *
 * A figure that fails to rasterize (a browser that dislikes the SVG, a
 * revoked object URL) falls back to its original data URI, so the panel
 * degrades to what it did before rather than showing a hole.
 */
function useThumbnails(images: string[], maxWidth = 520): Record<string, string> {
  const [thumbs, setThumbs] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    const pending = images.filter((src) => src && !(src in thumbs));
    if (!pending.length) return;

    void Promise.all(
      pending.map(
        (src) =>
          new Promise<[string, string]>((resolve) => {
            const img = new window.Image();
            // Resolve rather than reject on failure: one bad figure must
            // not stop the rest of the panel from getting thumbnails.
            img.onerror = () => resolve([src, src]);
            img.onload = () => {
              try {
                const scale = Math.min(1, maxWidth / (img.naturalWidth || maxWidth));
                const canvas = document.createElement("canvas");
                canvas.width = Math.max(1, Math.round((img.naturalWidth || maxWidth) * scale));
                canvas.height = Math.max(1, Math.round((img.naturalHeight || maxWidth) * scale));
                const ctx = canvas.getContext("2d");
                if (!ctx) return resolve([src, src]);
                // Figures are drawn on white; without this a transparent
                // SVG background rasterizes to black in dark mode.
                ctx.fillStyle = "#ffffff";
                ctx.fillRect(0, 0, canvas.width, canvas.height);
                ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
                resolve([src, canvas.toDataURL("image/png")]);
              } catch {
                resolve([src, src]);
              }
            };
            img.src = src;
          }),
      ),
    ).then((pairs) => {
      if (cancelled) return;
      setThumbs((prev) => {
        const next = { ...prev };
        for (const [src, png] of pairs) next[src] = png;
        return next;
      });
    });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [images, maxWidth]);

  return thumbs;
}

/** Where a figure is headed: a screen, or a printed page. */
export type FigureTarget = "screen" | "publication";

export interface FigureViewerProps {
  /** Finished figure data URIs returned by execute_code, in render order. */
  images: string[];
  theme?: "light" | "dark";
  className?: string;
  /** Current target, as read from the script's own `theme(...)` line. */
  target?: FigureTarget;
  /** Keep figures from earlier runs instead of replacing them. When the
   *  host does not pass these, the toggle is hidden and behaviour is
   *  unchanged -- replace on every run. */
  keepFigures?: boolean;
  onKeepFiguresChange?: (keep: boolean) => void;
  /** Save the whole collection. The host chooses where and reports back;
   *  without it the button is hidden rather than doing nothing. */
  onSaveCollection?: (format: "svg" | "png") => void;
  /** Insert generated Qu lines into the open script. When absent the
   *  editor still generates and shows the code, so it can be copied --
   *  the viewer never silently edits a file the host did not offer. */
  onInsertCode?: (lines: string[]) => void;
  /** Set the target. The host writes it into the SCRIPT rather than
   *  restyling the rendered image: the engine draws the figure, so the
   *  code is the only place the choice can honestly live -- and it means
   *  the setting survives being saved, shared and re-run. */
  onTargetChange?: (target: FigureTarget) => void;
  /** What to tell the user when there is nothing to show YET. The default
   *  ("run a script with `plot` or `savefig`") is right for the Code tab,
   *  where you write the plotting call yourself -- and wrong everywhere
   *  else this viewer is now mounted: the DSP workbench draws the figure
   *  FOR you the moment you press its own Run button, so instructing the
   *  user to write `plot` sends them looking for something the panel does
   *  not have. An empty state that describes a different screen is worse
   *  than none, because it is confidently wrong. */
  emptyHint?: React.ReactNode;
}

export const FigureViewer: React.FC<FigureViewerProps> = ({
  images,
  theme = "light",
  className,
  target,
  onTargetChange,
  onInsertCode,
  keepFigures,
  onKeepFiguresChange,
  onSaveCollection,
  emptyHint,
}) => {
  const [selected, setSelected] = useState(0);
  const [zoom, setZoom] = useState(0); // 0 fits both dimensions; otherwise scale the viewport width.
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState("");
  const dialogRef = useRef<HTMLDialogElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const dialogStageRef = useRef<HTMLDivElement>(null);
  /** Timestamp of the last right-press, for double-right-click-to-return. */
  const lastRightClick = useRef(0);
  const [tool, setTool] = useState<FigureTool>("pan");
  const [snap, setSnap] = useState(true);
  const panRef = useRef<{
    pointer: number;
    x: number;
    y: number;
    left: number;
    top: number;
  } | null>(null);
  const index = Math.min(selected, Math.max(0, images.length - 1));
  const thumbnails = useThumbnails(images);
  const src = images[index];
  // Only decode the active figure, keeping large collections inexpensive.
  const svg = useMemo(() => (src ? decodeSvgDataUri(src) : null), [src]);
  const format = src?.startsWith("data:image/svg+xml") ? "SVG" : "PNG";
  const canvasGestures = {
    onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => {
      // Only the pan tool drags. Otherwise a click meant for the cursor or
      // an annotation would be swallowed by a pan that starts on
      // pointer-down and eats the click on release.
      if (!panEnabled || !zoom || event.button !== 0) return;
      const canvas = event.currentTarget;
      panRef.current = {
        pointer: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        left: canvas.scrollLeft,
        top: canvas.scrollTop,
      };
      canvas.setPointerCapture(event.pointerId);
      canvas.dataset.dragging = "true";
      event.preventDefault();
    },
    onPointerMove: (event: React.PointerEvent<HTMLDivElement>) => {
      const pan = panRef.current;
      if (!pan || pan.pointer !== event.pointerId) return;
      event.currentTarget.scrollLeft = pan.left + pan.x - event.clientX;
      event.currentTarget.scrollTop = pan.top + pan.y - event.clientY;
    },
    onLostPointerCapture: (event: React.PointerEvent<HTMLDivElement>) => {
      panRef.current = null;
      delete event.currentTarget.dataset.dragging;
    },
    onPointerUp: (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.currentTarget.hasPointerCapture(event.pointerId))
        event.currentTarget.releasePointerCapture(event.pointerId);
    },
    onKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (event.key === "+" || event.key === "=") {
        event.preventDefault();
        setZoom((value) => Math.min(4, (value || 1) + 0.5));
      }
      if (event.key === "-") {
        event.preventDefault();
        setZoom((value) => Math.max(0.5, (value || 1) - 0.5));
      }
      if (event.key === "0") {
        event.preventDefault();
        setZoom(0);
      }
    },
  };

  // Wheel zoom, anchored under the pointer -- the map/image-viewer
  // convention, and the first thing anyone tries on a figure.
  //
  // Attached natively rather than as an `onWheel` prop because React
  // registers wheel listeners on its root as PASSIVE, where
  // `preventDefault()` does nothing: the figure would zoom and the page
  // would scroll at the same time. Non-passive is the only way to own the
  // gesture.
  //
  // Anchoring is the part that makes it feel right. Zooming about the
  // centre walks whatever you were looking at off screen, so you end up
  // zoom, pan back, zoom, pan back. Here the point under the cursor stays
  // under the cursor.
  useEffect(() => {
    const stage = expanded ? dialogStageRef.current : null;
    if (!stage) return;
    const onWheel = (event: WheelEvent) => {
      // Shift+wheel stays a horizontal scroll: the standard way to read a
      // wide figure without changing scale.
      if (event.shiftKey) return;
      event.preventDefault();
      const rect = stage.getBoundingClientRect();
      // Where the pointer sits over the CONTENT, as a fraction, before the
      // scale changes.
      const fx = (stage.scrollLeft + event.clientX - rect.left) / Math.max(1, stage.scrollWidth);
      const fy = (stage.scrollTop + event.clientY - rect.top) / Math.max(1, stage.scrollHeight);
      setZoom((current) => {
        const from = current || 1;
        // Multiplicative, so a notch feels the same at every scale; a
        // fixed step is imperceptible at 4x and enormous at 0.5x.
        const next = Math.min(4, Math.max(0.5, from * (event.deltaY < 0 ? 1.15 : 1 / 1.15)));
        if (Math.abs(next - from) < 1e-6) return current;
        // Put the anchor back once the browser has laid out the new size.
        requestAnimationFrame(() => {
          stage.scrollLeft = fx * stage.scrollWidth - (event.clientX - rect.left);
          stage.scrollTop = fy * stage.scrollHeight - (event.clientY - rect.top);
        });
        return next;
      });
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, [expanded]);

  useEffect(() => {
    setSelected((value) => Math.min(value, Math.max(0, images.length - 1)));
    if (!images.length) setExpanded(false);
  }, [images.length]);
  useEffect(() => {
    setZoom(0);
    setError("");
    stageRef.current?.scrollTo(0, 0);
    dialogStageRef.current?.scrollTo(0, 0);
  }, [src]);
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (expanded && !dialog.open) dialog.showModal();
    if (!expanded && dialog.open) dialog.close();
  }, [expanded]);

  const select = (next: number) => {
    setSelected(Math.max(0, Math.min(next, images.length - 1)));
    setZoom(0);
  };
  /** Open one figure full screen -- the panel's only interaction. */
  const open = (next: number) => {
    select(next);
    setExpanded(true);
  };
  const tools = useFigureTools({
    hostRef: dialogStageRef,
    svg,
    tool,
    active: expanded,
    snap,
  });
  // Panning is a drag; every other tool wants the click for itself.
  const panEnabled = tool === "pan";

  const download = async () => {
    try {
      // Annotations and renames are part of the figure now, so what is
      // saved is what is on screen. Falls back to the original bytes when
      // nothing has been changed, so an untouched figure downloads byte
      // for byte as the engine wrote it.
      const edited = tools.edited ? tools.exportSvg() : null;
      const blob = edited
        ? new Blob([edited], { type: "image/svg+xml" })
        : await (await fetch(src)).blob();
      downloadBlob(
        blob,
        `qu-figure-${index + 1}${tools.edited ? "-edited" : ""}.${
          edited ? "svg" : format.toLowerCase()
        }`,
      );
      setError("");
    } catch {
      setError("Could not download this figure. Please try again.");
    }
  };
  const zoomControls = (
    <>
      <button
        className="qu-icon-button"
        title="Zoom out"
        aria-label="Zoom out figure"
        disabled={zoom !== 0 && zoom <= 0.5}
        onClick={() => setZoom(Math.max(0.5, (zoom || 1) - 0.5))}
      >
        <ZoomOut size={14} />
      </button>
      <button
        className="qu-text-button"
        title="Fit figure to view"
        aria-label="Fit figure to view"
        onClick={() => setZoom(0)}
      >
        <Expand size={12} />
        {zoom === 0 ? "Fit" : `${Math.round(zoom * 100)}%`}
      </button>
      <button
        className="qu-icon-button"
        title="Zoom in"
        aria-label="Zoom in figure"
        disabled={zoom >= 4}
        onClick={() => setZoom(Math.min(4, (zoom || 1) + 0.5))}
      >
        <ZoomIn size={14} />
      </button>
    </>
  );
  const figure = (
    <div
      className="qu-figure-paper"
      style={
        zoom
          ? { width: `${zoom * 100}%`, minWidth: `${zoom * 100}%` }
          : undefined
      }
    >
      {svg ? (
        <div
          role="img"
          aria-label={`Figure ${index + 1}`}
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      ) : (
        <img src={src} alt={`Figure ${index + 1}`} />
      )}
    </div>
  );
  const navigation = (
    <>
      <button
        className="qu-icon-button"
        title="Previous figure"
        aria-label="Previous figure"
        disabled={index === 0}
        onClick={() => select(index - 1)}
      >
        <ChevronLeft size={14} />
      </button>
      <span className="qu-note" aria-live="polite">
        {index + 1} / {images.length}
      </span>
      <button
        className="qu-icon-button"
        title="Next figure"
        aria-label="Next figure"
        disabled={index + 1 >= images.length}
        onClick={() => select(index + 1)}
      >
        <ChevronRight size={14} />
      </button>
    </>
  );

  return (
    <section
      className={cn("qu-inspector qu-figures", className)}
      data-theme={theme}
      aria-label="Figure viewer"
    >
      <div className="qu-section-header">
        <Image size={15} />
        <h2>Figures</h2>
        <span className="qu-count">{images.length}</span>
        <span className="qu-spacer" />
        {onKeepFiguresChange && (
          // What happens to a figure on the NEXT run is a property of the
          // panel, not of the script, so it lives here rather than in the
          // code -- unlike the screen/publication target beside it.
          <button
            className={cn("qu-text-button", keepFigures && "is-active")}
            aria-pressed={!!keepFigures}
            title={
              keepFigures
                ? "Keeping figures from earlier runs — click to replace them instead"
                : "Replacing figures on each run — click to keep earlier ones for comparison"
            }
            onClick={() => onKeepFiguresChange(!keepFigures)}
          >
            <Layers size={12} />
            Keep
          </button>
        )}
        {onSaveCollection && images.length > 0 && (
          // Saving the collection is the other half of "Keep": the run
          // directory each figure came from is deleted as soon as its
          // bytes are read, so a kept collection lives only in memory
          // until it is written somewhere.
          <div className="qu-save-group" role="group" aria-label="Save all figures">
            <button
              className="qu-text-button"
              title="Save every figure as SVG — sharp at any size, good for Word and the web"
              onClick={() => onSaveCollection("svg")}
            >
              <Save size={12} />
              SVG
            </button>
            <button
              className="qu-text-button"
              title="Save every figure as PNG at 3x — safe in any document"
              onClick={() => onSaveCollection("png")}
            >
              PNG
            </button>
          </div>
        )}
        {onTargetChange && (
          // Writes a `theme(...)` line into the script rather than
          // restyling the picture: the engine renders the figure, so this
          // is the only place the choice can be made honestly -- and it
          // then survives saving, sharing and re-running the file.
          <div className="qu-target-toggle" role="group" aria-label="Figure target">
            <button
              className={cn("qu-text-button", target !== "publication" && "is-active")}
              aria-pressed={target !== "publication"}
              title="Screen: brighter, for a display"
              onClick={() => onTargetChange("screen")}
            >
              <Monitor size={12} />
              Screen
            </button>
            <button
              className={cn("qu-text-button", target === "publication" && "is-active")}
              aria-pressed={target === "publication"}
              title="Publication: print-weight frame, paler grid, colourblind-safe palette"
              onClick={() => onTargetChange("publication")}
            >
              <BookOpen size={12} />
              Publication
            </button>
          </div>
        )}
      </div>
      {!images.length ? (
        <div className="qu-empty">
          <Image size={28} />
          <strong>Give your data a view</strong>
          <p>
            {emptyHint ?? (
              <>
                Run a script with <code>plot</code> or <code>savefig</code>.
                Your figures will appear here.
              </>
            )}
          </p>
        </div>
      ) : (
        <>
          {/* Thumbnails only. Zoom, pan and inspection live in full
              screen, where there is room for them -- a zoom control on a
              200px-wide preview is a control for reading nothing. */}
          <div className="qu-figure-grid" aria-label="Figures">
            {images.map((image, i) => (
              <button
                key={i}
                className="qu-figure-thumb"
                aria-label={`Open figure ${i + 1} full screen`}
                onClick={() => open(i)}
              >
                <img src={thumbnails[image] ?? image} alt="" loading="lazy" />
                <span className="qu-figure-thumb-label">
                  Figure {i + 1}
                  <Expand size={11} aria-hidden />
                </span>
              </button>
            ))}
          </div>
          {error && (
            <p className="qu-plot-error" role="alert">
              {error}
            </p>
          )}
        </>
      )}
      <dialog
        ref={dialogRef}
        className="qu-inspector qu-figure-dialog"
        data-theme={theme}
        aria-label="Enlarged figure viewer"
        onCancel={() => setExpanded(false)}
        onClose={() => setExpanded(false)}
        onClick={(event) => {
          if (event.target !== dialogRef.current) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          if (
            event.clientX < bounds.left ||
            event.clientX > bounds.right ||
            event.clientY < bounds.top ||
            event.clientY > bounds.bottom
          )
            setExpanded(false);
        }}
        onKeyDown={(event) => {
          // Do not override native arrow-key behavior on focused controls.
          if ((event.target as HTMLElement).closest("button, select, input"))
            return;
          if (event.key === "ArrowLeft") {
            event.preventDefault();
            select(index - 1);
          }
          if (event.key === "ArrowRight") {
            event.preventDefault();
            select(index + 1);
          }
        }}
      >
        {expanded && images.length > 0 && (
          <>
            <div className="qu-toolbar">
              <Image size={16} />
              <strong>Figure {index + 1}</strong>
              <span className="qu-badge">{format}</span>
              {navigation}
              <span className="qu-spacer" />
              {/* Tools live here, in full screen, where there is room to
                  use them. The cursor and annotate tools need the figure's
                  data<->pixel mapping, which only figures rendered by an
                  engine that emits it carry -- so they are disabled, with a
                  reason, rather than silently reporting nonsense. */}
              <div className="qu-tool-group" role="group" aria-label="Figure tools">
                {([
                  ["pan", Hand, "Pan", "Drag to move the figure", true],
                  [
                    "annotate",
                    Crosshair,
                    "Annotate",
                    "Reads values as you move · click to label · double-click for a marked point · drag for an arrow",
                    tools.hasGeometry,
                  ],
                  ["text", Type, "Text", "Click anywhere to place free text", tools.hasGeometry],
                  ["select", MousePointer2, "Select", "Click text to select, double-click to retype", true],
                ] as const).map(([id, Icon, label, hint, enabled]) => (
                  <button
                    key={id}
                    className={cn("qu-icon-button", tool === id && "is-active")}
                    aria-pressed={tool === id}
                    aria-label={label}
                    disabled={!enabled}
                    title={enabled ? `${label} — ${hint}` : `${label} — this figure carries no coordinate data`}
                    onClick={() => setTool(id)}
                  >
                    <Icon size={14} />
                  </button>
                ))}
              </div>
              <div className="qu-tool-group" role="group" aria-label="Figure elements">
                {([
                  ["vline", MoveVertical, "V-line", "Click to place a vertical reference line"],
                  ["hline", MoveHorizontal, "H-line", "Click to place a horizontal reference line"],
                  ["xspan", Columns, "X-span", "Drag across to shade a band"],
                  ["inset", Maximize, "Inset", "Drag a rectangle to magnify it in an inset"],
                ] as const).map(([id, Icon, label, hint]) => (
                  <button
                    key={id}
                    className={cn("qu-icon-button", tool === id && "is-active")}
                    aria-pressed={tool === id}
                    aria-label={label}
                    disabled={!tools.hasGeometry}
                    title={
                      tools.hasGeometry
                        ? `${label} — ${hint}, and get the Qu line for it`
                        : `${label} — this figure carries no coordinate data`
                    }
                    onClick={() => setTool(id)}
                  >
                    <Icon size={14} />
                  </button>
                ))}
              </div>
              {tool === "annotate" && (
                <button
                  className={cn("qu-text-button", snap && "is-active")}
                  aria-pressed={snap}
                  title={
                    snap
                      ? "Snapping to plotted points — click to read any position instead"
                      : "Reading any position — click to snap to plotted points"
                  }
                  onClick={() => setSnap((v) => !v)}
                >
                  <Magnet size={12} />
                  Snap
                </button>
              )}
              {tools.annotations.length > 0 && (
                <button
                  className="qu-icon-button"
                  aria-label="Remove all annotations"
                  title={`Remove ${tools.annotations.length} annotation(s)`}
                  onClick={tools.clearAnnotations}
                >
                  <Trash2 size={14} />
                </button>
              )}
              {zoomControls}
              <button className="qu-text-button" onClick={download}>
                <Download size={14} />
                Download
              </button>
              <button
                className="qu-icon-button"
                aria-label="Close figure viewer"
                title="Close (Esc)"
                onClick={() => setExpanded(false)}
              >
                <X size={18} />
              </button>
            </div>
            <div
              ref={dialogStageRef}
              {...canvasGestures}
              // Double right-click returns to the panel. Plotly uses
              // double-click to reset a zoom, so the right button keeps
              // "go back" from fighting "fit to view".
              onContextMenu={(event) => event.preventDefault()}
              onDoubleClick={(event) => {
                if (event.button === 2) setExpanded(false);
              }}
              onPointerDownCapture={(event) => {
                if (event.button !== 2) return;
                const now = Date.now();
                if (now - lastRightClick.current < 400) setExpanded(false);
                lastRightClick.current = now;
              }}
              data-zoomed={zoom > 0}
              data-tool={tool}
              tabIndex={0}
              aria-label="Figure canvas; use left and right arrows to navigate"
              className={cn("qu-figure-stage", zoom === 0 && "qu-figure-fit")}
            >
              {figure}
            </div>
            {error && (
              <p className="qu-plot-error" role="alert">
                {error}
              </p>
            )}
            {/* Generated code. Shown BEFORE it is inserted, always: the
                point of pointing at a figure is to get the line you would
                have written, and you should read it before it lands in
                your script. Insert is offered only when the host provided
                somewhere to insert it. */}
            {tools.shapeCode.length > 0 && (
              <div className="qu-codegen" aria-label="Generated Qu">
                <div className="qu-codegen-head">
                  <Code2 size={13} />
                  <strong>Qu for these {tools.shapeCode.length} element(s)</strong>
                  <span className="qu-spacer" />
                  {onInsertCode && (
                    <button
                      className="qu-text-button is-active"
                      title="Add these lines to the open script"
                      onClick={() => onInsertCode(tools.shapeCode)}
                    >
                      <FilePlus2 size={12} />
                      Insert into script
                    </button>
                  )}
                  <button
                    className="qu-text-button"
                    title="Copy the generated lines"
                    onClick={() => void navigator.clipboard?.writeText(tools.shapeCode.join("\n"))}
                  >
                    <Copy size={12} />
                    Copy
                  </button>
                  <button
                    className="qu-icon-button"
                    aria-label="Discard the generated elements"
                    title="Discard"
                    onClick={tools.clearShapes}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
                <pre className="qu-codegen-body">{tools.shapeCode.join(String.fromCharCode(10))}</pre>
              </div>
            )}
            <div className="qu-figure-footer">
              <span>
                {/* The readout is live whenever a tool is pointing at data,
                    which is the whole reason cursor and annotate merged:
                    you read a value BECAUSE you are about to mark it. */}
                {(tool === "annotate" || tool === "text") && tools.readout ? (
                  <span className="qu-readout">
                    x ={" "}
                    {tools.readout.label ??
                      `${formatCoord(tools.readout.x)} · y = ${formatCoord(tools.readout.y)}`}
                    {tools.snappedTo ? " · on a data point" : ""}
                  </span>
                ) : tool === "annotate" ? (
                  "Click to label · double-click for a marked point · drag for an arrow"
                ) : tool === "text" ? (
                  "Click anywhere to place text · double-click it to retype"
                ) : tool === "vline" || tool === "hline" ? (
                  "Click the plot to place a reference line"
                ) : tool === "xspan" ? (
                  "Drag across the plot to shade a band"
                ) : tool === "inset" ? (
                  "Drag a rectangle to magnify it in an inset"
                ) : tool === "select" ? (
                  "Double-click any text to retype it · Enter commits, Esc cancels"
                ) : (
                  "Wheel to zoom · drag to pan · 0 to fit · double right-click to return"
                )}
              </span>
              <span>Esc to close</span>
            </div>
          </>
        )}
      </dialog>
    </section>
  );
};
export default FigureViewer;
