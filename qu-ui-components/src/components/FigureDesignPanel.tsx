import React from "react";
import { Eye, EyeOff, RotateCcw, X } from "lucide-react";
import type {
  FigureDesign,
  FigureLabels,
  LayerDesign,
} from "../utils/figureDesign";
import { FIGURE_PALETTES, figurePaletteColor } from "../utils/figureDesign";
import type { PlotData } from "./PlotViewer";

interface FigureDesignPanelProps {
  labels: FigureLabels;
  design: FigureDesign;
  traces: PlotData[];
  layerKeys: string[];
  layers: Record<string, LayerDesign>;
  colorSlots: Record<string, number>;
  dark: boolean;
  is3d: boolean;
  onLabel: (key: keyof FigureLabels, value: string) => void;
  onDesign: (patch: Partial<FigureDesign>) => void;
  onLayer: (key: string, patch: LayerDesign) => void;
  onReset: () => void;
  onClose: () => void;
}

export function FigureDesignPanel({
  labels,
  design,
  traces,
  layerKeys,
  layers,
  colorSlots,
  dark,
  is3d,
  onLabel,
  onDesign,
  onLayer,
  onReset,
  onClose,
}: FigureDesignPanelProps) {
  const textField = (
    label: string,
    key: keyof FigureLabels,
    placeholder: string,
    multiline = false,
  ) => (
    <label className="qu-design-field" key={key}>
      <span>{label}</span>
      {multiline ? (
        <textarea
          aria-label={label}
          value={labels[key]}
          rows={2}
          placeholder={placeholder}
          onChange={(event) => onLabel(key, event.target.value)}
        />
      ) : (
        <input
          aria-label={label}
          value={labels[key]}
          placeholder={placeholder}
          onChange={(event) => onLabel(key, event.target.value)}
        />
      )}
    </label>
  );
  return (
    <aside
      className="qu-figure-design"
      aria-label="Figure design"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="qu-toolbar">
        <strong>Figure design</strong>
        <span className="qu-spacer" />
        <button
          className="qu-icon-button"
          aria-label="Reset figure design"
          title="Restore defaults and supplied labels"
          onClick={onReset}
        >
          <RotateCcw size={14} />
        </button>
        <button
          className="qu-icon-button"
          aria-label="Close figure design"
          onClick={onClose}
        >
          <X size={16} />
        </button>
      </div>
      <div className="qu-design-scroll">
        <p className="qu-note">Edits apply to this viewer and its exports.</p>
        <details open>
          <summary>Labels & context</summary>
          {textField("Title", "title", "What does this figure show?")}
          {textField("Subtitle", "subtitle", "Add the context", true)}
          {textField(
            "Caption",
            "caption",
            "Source, method, or a useful note",
            true,
          )}
          <div className="qu-design-pair">
            {textField("X axis", "xlabel", "Label / units")}
            {textField("Y axis", "ylabel", "Label / units")}
          </div>
          {is3d && textField("Z axis", "zlabel", "Label / units")}
        </details>
        <details open>
          <summary>Appearance</summary>
          <label className="qu-design-field">
            <span>Palette</span>
            <select
              aria-label="Palette"
              value={design.palette}
              onChange={(event) =>
                onDesign({
                  palette: event.target.value as FigureDesign["palette"],
                })
              }
            >
              <option value="qu">Qu</option>
              <option value="okabe_ito">Okabe–Ito</option>
              <option value="tableau">Tableau</option>
            </select>
          </label>
          <div
            className="qu-palette-swatches"
            aria-label={`${design.palette} palette preview`}
          >
            {FIGURE_PALETTES[design.palette].map((color) => (
              <span key={color} style={{ background: color }} />
            ))}
          </div>
          <label className="qu-design-field">
            <span>Continuous scale</span>
            <select
              aria-label="Continuous scale"
              value={design.continuousScale ?? ""}
              onChange={(event) =>
                onDesign({
                  continuousScale: (event.target.value ||
                    undefined) as FigureDesign["continuousScale"],
                })
              }
            >
              <option value="">From data / Viridis</option>
              <option>Viridis</option>
              <option>Cividis</option>
              <option>RdBu</option>
            </select>
          </label>
          {(
            [
              ["Text size", "textSize", 10, 18, 1, "px"],
              ["Line weight", "lineWidth", 1, 5, 0.5, "px"],
              ["Point size", "pointSize", 3, 12, 1, "px"],
            ] as const
          ).map(([label, key, min, max, step, unit]) => (
            <label className="qu-design-field" key={key}>
              <span>
                {label}
                <output>
                  {design[key] == null ? "From data" : `${design[key]} ${unit}`}
                </output>
              </span>
              <input
                type="range"
                aria-label={label}
                min={min}
                max={max}
                step={step}
                value={design[key] ?? (key === "lineWidth" ? 2 : 6)}
                onChange={(event) =>
                  onDesign({ [key]: Number(event.target.value) })
                }
              />
            </label>
          ))}
        </details>
        <details open>
          <summary>
            Legend & layers <span className="qu-count">{traces.length}</span>
          </summary>
          <label className="qu-design-field">
            <span>Legend position</span>
            <select
              aria-label="Legend position"
              value={design.legendPosition}
              onChange={(event) =>
                onDesign({
                  legendPosition: event.target
                    .value as FigureDesign["legendPosition"],
                })
              }
            >
              <option value="bottom">Below the plot</option>
              <option value="right">Right · below on narrow views</option>
              <option value="none">Hidden</option>
            </select>
          </label>
          {textField(
            "Legend title",
            "legendTitle",
            "What distinguishes the layers?",
          )}
          <div className="qu-design-layers">
            {traces.map((trace, index) => {
              const key = layerKeys[index];
              const layer = layers[key] ?? {};
              const name = layer.name ?? trace.name ?? `Layer ${index + 1}`;
              const color =
                layer.color ??
                trace.line?.color ??
                trace.marker?.color ??
                figurePaletteColor(design.palette, colorSlots[key] ?? index, dark);
              const visible = (layer.visible ?? trace.visible ?? true) === true;
              const continuous = ["heatmap", "contour", "surface"].includes(
                trace.type ?? "",
              );
              return (
                <div className="qu-design-layer" key={key}>
                  <button
                    className="qu-icon-button"
                    aria-label={`${visible ? "Hide" : "Show"} layer ${index + 1}`}
                    title={`${visible ? "Hide" : "Show"} ${name}`}
                    onClick={() =>
                      onLayer(key, { visible: visible ? "legendonly" : true })
                    }
                  >
                    {visible ? <Eye size={14} /> : <EyeOff size={14} />}
                  </button>
                  {!continuous && (
                    <input
                      type="color"
                      aria-label={`Layer ${index + 1} color`}
                      value={/^#[\da-f]{6}$/i.test(color) ? color : "#547dc5"}
                      onChange={(event) =>
                        onLayer(key, { color: event.target.value })
                      }
                    />
                  )}
                  <input
                    aria-label={`Layer ${index + 1} label`}
                    value={name}
                    onChange={(event) =>
                      onLayer(key, { name: event.target.value })
                    }
                  />
                  <span className="qu-note">{trace.type ?? "scatter"}</span>
                </div>
              );
            })}
          </div>
        </details>
      </div>
    </aside>
  );
}
