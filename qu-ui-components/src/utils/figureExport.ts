/** Saving a collection of figures, in the format the destination wants.
 *
 * The formats are not interchangeable and the choice is not cosmetic:
 *
 *   - **LaTeX** takes TikZ or PDF (EPS in older workflows). TikZ is set by
 *     LaTeX itself, so its type matches the document exactly -- same font,
 *     same size, same maths -- which no exported image can promise.
 *   - **Word** takes SVG or PNG. SVG stays sharp at any zoom; PNG is the
 *     safe fallback for anything that mishandles vector.
 *
 * Only SVG and PNG can be produced from what the panel already holds. PDF
 * and TikZ have to come from the engine, because they are rendered, not
 * converted -- so those are offered as a script line to add rather than
 * pretended into existence here.
 */
export type FigureFormat = "svg" | "png" | "pdf" | "tikz";

export const FIGURE_FORMATS: { id: FigureFormat; label: string; hint: string; fromPanel: boolean }[] = [
  { id: "svg", label: "SVG", hint: "Word, web — sharp at any size", fromPanel: true },
  { id: "png", label: "PNG", hint: "Word, slides — safe everywhere", fromPanel: true },
  { id: "pdf", label: "PDF", hint: "LaTeX — vector, embeds cleanly", fromPanel: false },
  { id: "tikz", label: "TikZ", hint: "LaTeX — typeset by LaTeX, matches the document", fromPanel: false },
];

/** Rasterize an SVG data URI to a PNG one, at a scale factor.
 *
 * `scale` exists because a figure saved for print needs more pixels than
 * the screen has: journals ask for 600-1200 dpi on line art, and a 1:1
 * raster of a screen-sized figure is nowhere near that.
 */
export async function svgToPng(dataUri: string, scale = 3): Promise<string> {
  if (!dataUri.startsWith("data:image/svg+xml")) return dataUri;
  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("could not decode the figure"));
    img.src = dataUri;
  });
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round((img.naturalWidth || 900) * scale));
  canvas.height = Math.max(1, Math.round((img.naturalHeight || 600) * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no 2D canvas available");
  // Figures are drawn on white; without this a transparent SVG ground
  // rasterizes to black wherever the page would have shown through.
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/png");
}

/** Turn the panel's collection into files ready for `save_figures`. */
export async function buildFigureFiles(
  images: string[],
  format: Extract<FigureFormat, "svg" | "png">,
  prefix = "figure",
): Promise<{ name: string; data_uri: string }[]> {
  const out: { name: string; data_uri: string }[] = [];
  for (const [i, src] of images.entries()) {
    // 1-based and zero-padded, so a directory listing sorts the way the
    // figures were produced rather than 1, 10, 11, 2.
    const n = String(i + 1).padStart(2, "0");
    out.push({
      name: `${prefix}-${n}.${format}`,
      data_uri: format === "png" ? await svgToPng(src) : src,
    });
  }
  return out;
}

/** The Qu lines that would save this collection in an engine-only format.
 *
 * Offered instead of a silent no-op: PDF and TikZ are rendered by the
 * engine from the figure's own draw operations, not converted from an
 * image, which is exactly why they are worth using for LaTeX.
 */
export function engineSaveSnippet(format: Extract<FigureFormat, "pdf" | "tikz">, count: number): string {
  const lines = [`# add before each figure is finished, then re-run`];
  for (let i = 1; i <= Math.max(count, 1); i++) {
    lines.push(`savefig("figure-${String(i).padStart(2, "0")}.${format}")`);
  }
  return lines.join("\n");
}
