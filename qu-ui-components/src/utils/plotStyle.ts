export type PlotStyle = "grey" | "minimal" | "classic";

/** Limits are data values; Plotly expects log10 coordinates for log axes. */
export function axisRange(
  limits: [number, number] | undefined,
  logarithmic: boolean,
): [number, number] | undefined {
  if (
    !limits ||
    limits.some(
      (value) => !Number.isFinite(value) || (logarithmic && value <= 0),
    )
  )
    return undefined;
  return logarithmic
    ? [Math.log10(limits[0]), Math.log10(limits[1])]
    : [...limits];
}

/** Non-data styling inspired by ggplot2's grey, minimal and classic themes. */
export function plotTheme(style: PlotStyle, theme: "light" | "dark") {
  const dark = theme === "dark";
  return {
    paper: dark ? "#171b22" : "#ffffff",
    panel:
      style === "grey"
        ? dark
          ? "#252d39"
          : "#ebedf0"
        : dark
          ? "#171b22"
          : "#ffffff",
    grid:
      style === "grey"
        ? dark
          ? "#3d4858"
          : "#ffffff"
        : dark
          ? "#303947"
          : "#e3e7ed",
    ink: dark ? "#d8e1ee" : "#445267",
  };
}

/**
 * What the figure is FOR, which is a different question from which theme
 * it wears. `screen` is a figure you are reading on a monitor right now;
 * `publication` is one destined for a paper, where it will be printed at
 * column width in black and white as often as not.
 *
 * They genuinely need different type and different weights -- a figure
 * tuned to look crisp on a backlit display is usually too light and too
 * loose once it is 8cm wide on paper -- which is why this is its own axis
 * rather than another entry in `PlotStyle`.
 */
export type PlotTarget = "screen" | "publication";

/**
 * Type stacks per target.
 *
 * `publication` leads with **Latin Modern Roman** deliberately: it is the
 * modern OpenType successor to Computer Modern, i.e. the typeface a LaTeX
 * document is already set in. A figure using it drops into a paper looking
 * like it belongs to the text around it rather than like a screenshot
 * pasted in -- which is the single most common tell of an amateur figure.
 * It is bundled with the engine (see `TIKZ_FONT_FAMILY` in `plotting.rs`),
 * so it is present rather than merely hoped for, and the fallbacks stay on
 * the same skeleton: CMU Serif is the same design, then a Times-class
 * serif, which is what a journal would substitute anyway.
 *
 * `screen` keeps Inter: a grotesque drawn for screen rendering, with a tall
 * x-height that stays legible at the small sizes axis labels live at.
 */
export const PLOT_FONT: Record<PlotTarget, string> = {
  screen: 'Inter, system-ui, -apple-system, "Segoe UI", sans-serif',
  publication:
    '"Latin Modern Roman", "CMU Serif", "Times New Roman", Times, Georgia, serif',
};

/**
 * Line weights and type sizes per target, in points.
 *
 * Publication numbers are heavier, not lighter, and that is the point: a
 * hairline that reads as elegant on a display can disappear entirely in
 * print or in a photocopy, so axis and data lines both thicken while the
 * type grows just enough to survive being reduced to column width.
 */
export const PLOT_METRICS: Record<
  PlotTarget,
  { font: number; title: number; tick: number; line: number; axis: number; marker: number }
> = {
  screen: { font: 11, title: 13, tick: 10, line: 2, axis: 1, marker: 6 },
  publication: { font: 13, title: 15, tick: 12, line: 2.4, axis: 1.4, marker: 7 },
};

/**
 * A figure bound for print is drawn on white and inked in black, whatever
 * theme the IDE happens to be wearing -- the paper it lands on has no dark
 * mode. Screen keeps the theme-aware palette from `plotTheme`.
 */
export function targetTheme(
  target: PlotTarget,
  style: PlotStyle,
  theme: "light" | "dark",
) {
  if (target === "screen") return plotTheme(style, theme);
  return {
    paper: "#ffffff",
    panel: style === "grey" ? "#f2f2f2" : "#ffffff",
    grid: style === "grey" ? "#ffffff" : "#d9d9d9",
    ink: "#1a1a1a",
  };
}

/**
 * Okabe & Ito's colourblind-safe qualitative set (Color Universal Design;
 * popularised by Wong, *Nature Methods* 8, 441, 2011) for publication --
 * roughly one reader in twelve men has a colour vision deficiency, and a
 * journal figure has no way to ask. Screen keeps the warmer house palette.
 */
export const PLOT_COLORWAY: Record<PlotTarget, string[]> = {
  screen: ["#E76F51", "#2A9D8F", "#6488D0", "#B478AE", "#D6A535", "#56A5B7"],
  publication: [
    "#0072B2", "#D55E00", "#009E73", "#CC79A7", "#E69F00", "#56B4E9", "#F0E442", "#000000",
  ],
};
