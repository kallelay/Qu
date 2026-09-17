import type { PlotData } from "../components/PlotViewer";

export interface FigureLabels {
  title: string;
  subtitle: string;
  caption: string;
  xlabel: string;
  ylabel: string;
  zlabel: string;
  legendTitle: string;
}

export const FIGURE_PALETTES = {
  qu: ["#D96445", "#218C81", "#547DC5", "#A15A99", "#B58A18", "#36869A"],
  okabe_ito: [
    "#0072B2",
    "#D55E00",
    "#009E73",
    "#CC79A7",
    "#E69F00",
    "#56B4E9",
    "#000000",
    "#F0E442",
  ],
  tableau: [
    "#4E79A7",
    "#F28E2B",
    "#E15759",
    "#76B7B2",
    "#59A14F",
    "#EDC948",
    "#B07AA1",
    "#FF9DA7",
    "#9C755F",
    "#BAB0AC",
  ],
};

export interface FigureDesign {
  textSize: number;
  lineWidth: number | null;
  pointSize: number | null;
  palette: keyof typeof FIGURE_PALETTES;
  continuousScale?: "Viridis" | "Cividis" | "RdBu";
  legendPosition: "bottom" | "right" | "none";
}

export const DEFAULT_FIGURE_DESIGN: FigureDesign = {
  textSize: 12,
  lineWidth: null,
  pointSize: null,
  palette: "qu",
  legendPosition: "bottom",
};

export interface LayerDesign {
  name?: string;
  color?: string;
  visible?: boolean | "legendonly";
}

export function figurePaletteColor(palette: FigureDesign['palette'], slot: number, dark = false): string {
  const colors = FIGURE_PALETTES[palette];
  const color = colors[slot % colors.length];
  if (!dark) return color;
  if (color === '#000000') return '#d8e1ee';
  // Lift default strokes against a dark canvas; explicit colors stay exact.
  return '#' + [1, 3, 5].map(offset => {
    const channel = parseInt(color.slice(offset, offset + 2), 16);
    return Math.round(channel + (255 - channel) * .18).toString(16).padStart(2, '0');
  }).join('');
}

/** Explicit ids/names preserve layer edits when callers reorder their traces. */
export function figureLayerKeys(traces: PlotData[]): string[] {
  const counts = new Map<string, number>();
  return traces.map((trace) => {
    const identity = trace.uid ?? trace.name ?? trace.type ?? "scatter";
    const count = counts.get(identity) ?? 0;
    counts.set(identity, count + 1);
    return JSON.stringify([identity, count]);
  });
}

/** Treat figure text as plain text, not Plotly's HTML subset. */
export function plotText(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

export function wrapFigureText(text: string, columns: number): string[] {
  if (!text) return [];
  const width = Math.max(8, Math.floor(columns));
  const lines: string[] = [];
  for (const paragraph of text.split("\n")) {
    let line = "";
    for (const word of paragraph.split(/\s+/).filter(Boolean)) {
      if (line && line.length + word.length + 1 > width) {
        lines.push(line);
        line = "";
      }
      const characters = Array.from(word);
      while (characters.length > width) {
        if (line) {
          lines.push(line);
          line = "";
        }
        lines.push(characters.splice(0, width).join(""));
      }
      if (characters.length) line += `${line ? " " : ""}${characters.join("")}`;
    }
    lines.push(line);
  }
  return lines;
}

export function styleFigureLayers(
  traces: PlotData[],
  keys: string[],
  layers: Record<string, LayerDesign>,
  design: FigureDesign,
  colorSlots: Record<string, number> = {},
  dark = false,
) {
  return traces.map((trace, index) => {
    const edit = layers[keys[index]] ?? {};
    const continuous = ["heatmap", "contour", "surface"].includes(
      trace.type ?? "",
    );
    const color =
      edit.color ??
      trace.line?.color ??
      trace.marker?.color ??
      figurePaletteColor(design.palette, colorSlots[keys[index]] ?? index, dark);
    return {
      ...trace,
      // Plotly interpolates uid into CSS selectors during purge/update.
      // Encode the full identity without punctuation or lossy slug collisions.
      uid: 'qu-' + Array.from(keys[index]).map(char => char.codePointAt(0)!.toString(16)).join('-'),
      type: trace.type === "line" ? "scatter" : (trace.type ?? "scatter"),
      name: plotText(edit.name ?? trace.name ?? `Layer ${index + 1}`),
      visible: edit.visible ?? trace.visible ?? true,
      ...(trace.type === "line" ? { mode: trace.mode ?? "lines" } : {}),
      line: {
        color,
        ...trace.line,
        width: design.lineWidth ?? trace.line?.width ?? 2,
        ...(edit.color ? { color: edit.color } : {}),
      },
      marker: {
        color,
        ...trace.marker,
        size: design.pointSize ?? trace.marker?.size ?? 6,
        ...(edit.color ? { color: edit.color } : {}),
      },
      ...(continuous
        ? {
            colorscale: design.continuousScale ?? trace.colorscale ?? "Viridis",
          }
        : {}),
    };
  });
}

/** Reserve real figure space for text so the exact composition also exports. */
export function figureComposition(
  labels: FigureLabels,
  design: FigureDesign,
  width: number,
  legendNames: string[],
  showLegend: boolean,
  ink: string,
  muted: string,
) {
  const font = design.textSize;
  const rightLegend =
    showLegend && design.legendPosition === "right" && width >= 640;
  const legendWidth = rightLegend
    ? Math.min(
        240,
        Math.max(
          100,
          ...legendNames.map((name) => name.length * font * 0.56 + 42),
        ),
      )
    : 0;
  const left = 58;
  const right = 24 + legendWidth;
  const innerWidth = Math.max(100, width - left - right);
  const titleSize = font * 1.5;
  const title = wrapFigureText(labels.title, innerWidth / (titleSize * 0.58));
  const subtitle = wrapFigureText(labels.subtitle, innerWidth / (font * 0.55));
  const caption = wrapFigureText(labels.caption, innerWidth / (font * 0.5));
  const titleHeight = title.length * titleSize * 1.2;
  const subtitleHeight = subtitle.length * font * 1.45;
  const top = Math.ceil(
    18 +
      titleHeight +
      (title.length && subtitle.length ? 6 : 0) +
      subtitleHeight +
      (title.length || subtitle.length ? 18 : 0),
  );
  let rows = 0,
    rowWidth = 0;
  if (showLegend && !rightLegend) {
    for (const name of legendNames) {
      const itemWidth = Math.min(innerWidth, name.length * font * 0.56 + 48);
      if (!rows || rowWidth + itemWidth > innerWidth) {
        rows++;
        rowWidth = 0;
      }
      rowWidth += itemWidth;
    }
  }
  const legendHeight =
    rows * (font + 12) + (rows && labels.legendTitle ? font + 10 : 0);
  const axisBottom = labels.xlabel ? font * 2 + 28 : font + 22;
  const bottom = Math.ceil(
    axisBottom +
      legendHeight +
      caption.length * font * 1.3 +
      (caption.length ? 16 : 0) +
      8,
  );
  const annotations: Record<string, unknown>[] = [];
  if (title.length)
    annotations.push({
      x: 0,
      y: 1,
      yshift: top - 14,
      xref: "paper",
      yref: "paper",
      xanchor: "left",
      yanchor: "top",
      showarrow: false,
      align: "left",
      text: `<b>${title.map(plotText).join("<br>")}</b>`,
      font: { size: titleSize, color: ink },
    });
  if (subtitle.length)
    annotations.push({
      x: 0,
      y: 1,
      yshift: top - 14 - titleHeight - (title.length ? 6 : 0),
      xref: "paper",
      yref: "paper",
      xanchor: "left",
      yanchor: "top",
      showarrow: false,
      align: "left",
      text: subtitle.map(plotText).join("<br>"),
      font: { size: font, color: muted },
    });
  if (caption.length)
    annotations.push({
      x: 0,
      y: 0,
      yshift: -axisBottom - legendHeight - 8,
      xref: "paper",
      yref: "paper",
      xanchor: "left",
      yanchor: "top",
      showarrow: false,
      align: "left",
      text: caption.map(plotText).join("<br>"),
      font: { size: font * 0.9, color: muted },
    });
  return {
    margin: { l: left, r: right, t: top, b: bottom },
    annotations,
    legend: {
      orientation: rightLegend ? "v" : "h",
      x: rightLegend ? 1.03 : 0,
      y: rightLegend ? 1 : 0, // The caller converts the bottom offset to paper coordinates.
      xanchor: "left",
      yanchor: "top",
      title: { text: plotText(labels.legendTitle), font: { size: font } },
      font: { size: font },
      bgcolor: "rgba(0,0,0,0)",
      borderwidth: 0,
      tracegroupgap: 8,
      itemsizing: "constant",
    },
    axisBottom,
    rightLegend,
  };
}
