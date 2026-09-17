import { describe, expect, it } from "vitest";
import {
  DEFAULT_FIGURE_DESIGN,
  figureComposition,
  figureLayerKeys,
  plotText,
  styleFigureLayers,
  wrapFigureText,
} from "./figureDesign";
import type { FigureLabels } from "./figureDesign";
import type { PlotData } from "../components/PlotViewer";

const labels: FigureLabels = {
  title: "Measured response",
  subtitle: "Three independent runs",
  caption: "Source: experiment α",
  xlabel: "Time (s)",
  ylabel: "Amplitude",
  zlabel: "",
  legendTitle: "Run",
};

describe("figure composition", () => {
  it("puts the title, subtitle, and caption inside the exportable figure", () => {
    const composition = figureComposition(
      labels,
      DEFAULT_FIGURE_DESIGN,
      800,
      ["A", "B"],
      true,
      "#222",
      "#666",
    );
    expect(composition.annotations.map((a) => a.text)).toEqual([
      "<b>Measured response</b>",
      "Three independent runs",
      "Source: experiment α",
    ]);
    expect(composition.margin.t).toBeGreaterThan(60);
    expect(composition.margin.b).toBeGreaterThan(90);
    expect(composition.legend.title.text).toBe("Run");
  });
  it("reserves extra space for wrapped labels on narrow figures", () => {
    const longLabels = {
      ...labels,
      title:
        "A measured response over time that needs multiple lines in the narrow variable inspector",
    };
    const wide = figureComposition(
      longLabels,
      DEFAULT_FIGURE_DESIGN,
      1000,
      [],
      false,
      "#222",
      "#666",
    );
    const narrow = figureComposition(
      longLabels,
      DEFAULT_FIGURE_DESIGN,
      320,
      [],
      false,
      "#222",
      "#666",
    );
    expect(narrow.margin.t).toBeGreaterThan(wide.margin.t);
    expect(narrow.annotations[0].text).toContain("<br>");
  });
  it("moves a right legend below a narrow plot instead of collapsing the data area", () => {
    const design = {
      ...DEFAULT_FIGURE_DESIGN,
      legendPosition: "right" as const,
    };
    expect(
      figureComposition(labels, design, 900, ["A"], true, "#222", "#666")
        .rightLegend,
    ).toBe(true);
    expect(
      figureComposition(labels, design, 360, ["A"], true, "#222", "#666")
        .rightLegend,
    ).toBe(false);
  });
  it("keeps literal markup, explicit newlines and long identifiers readable", () => {
    expect(plotText("signal < threshold & x > 0")).toBe(
      "signal &lt; threshold &amp; x &gt; 0",
    );
    expect(wrapFigureText("first\nsecond", 40)).toEqual(["first", "second"]);
    expect(wrapFigureText("abcdefghijklmnopqr", 8)).toEqual([
      "abcdefgh",
      "ijklmnop",
      "qr",
    ]);
  });
});

describe("figure layers", () => {
  const data: PlotData[] = [
    {
      name: "Measured",
      type: "line",
      x: [0, 1],
      y: [1, 2],
      line: { width: 4, color: "#112233" },
    },
    { name: "Reference", type: "line", y: [2, 3] },
  ];
  it("keeps named edits attached to the same layer after reordering", () => {
    const keys = figureLayerKeys(data);
    const reordered = [data[1], data[0]];
    const styled = styleFigureLayers(
      reordered,
      figureLayerKeys(reordered),
      {
        [keys[0]]: {
          name: "Observed <x>",
          color: "#abcdef",
          visible: "legendonly",
        },
      },
      DEFAULT_FIGURE_DESIGN,
    );
    expect(styled[1].name).toBe("Observed &lt;x&gt;");
    expect(styled[1].line.color).toBe("#abcdef");
    expect(styled[1].visible).toBe("legendonly");
    expect(styled[0].name).toBe("Reference");
  });
  it("preserves supplied styles until the user overrides them without mutating input", () => {
    const keys = figureLayerKeys(data);
    expect(
      styleFigureLayers(data, keys, {}, DEFAULT_FIGURE_DESIGN)[0].line.width,
    ).toBe(4);
    const styled = styleFigureLayers(
      data,
      keys,
      {},
      { ...DEFAULT_FIGURE_DESIGN, lineWidth: 1.5 },
    );
    expect(styled[0].line.width).toBe(1.5);
    expect(data[0].line?.width).toBe(4);
    expect(styled[0].type).toBe("scatter");
    expect(styled[0].mode).toBe("lines");
    expect(styled[0].uid).toMatch(/^qu-[\da-f-]+$/);
  });
  it("keeps duplicate named and unnamed layer keys unique", () => {
    const keys = figureLayerKeys([{ name: "A" }, { name: "A" }, {}, {}]);
    expect(new Set(keys).size).toBe(4);
  });
});
