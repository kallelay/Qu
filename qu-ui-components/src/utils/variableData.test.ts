import { describe, expect, it } from "vitest";
import {
  formatNumber,
  matrixValue,
  numericSummary,
  sampleSeries,
  variableCsv,
} from "./variableData";
import { axisRange } from "./plotStyle";

describe("workspace numeric inspection", () => {
  it("reads and exports a non-square Qu matrix in row order", () => {
    const matrix = {
      name: "A",
      type: "matrix",
      shape: [2, 3],
      data: [1, 2, 3, 4, 5, 6],
    };
    expect(matrixValue(matrix, 1, 2)).toBe(6);
    expect(matrixValue(matrix, 0, 1)).toBe(3);
    expect(variableCsv(matrix)).toBe(
      "Column 1,Column 2,Column 3\r\n1,3,5\r\n2,4,6",
    );
  });
  it("excludes missing values from statistics without turning them into zeros", () => {
    expect(numericSummary([null, -3, 3, Infinity, NaN])).toEqual({
      min: -3,
      max: 3,
      mean: 0,
      count: 2,
      missing: 3,
    });
    expect(numericSummary([null, NaN])).toEqual({
      min: null,
      max: null,
      mean: null,
      count: 0,
      missing: 2,
    });
    expect(numericSummary([]).mean).toBeNull();
  });
  it("keeps large finite means finite and preserves CSV precision", () => {
    expect(numericSummary([1e308, 1e308]).mean).toBe(1e308);
    const value = 1.2345678901234567;
    expect(
      variableCsv({
        name: "x",
        type: "vector",
        shape: [2, 1],
        data: [value, null],
      }),
    ).toBe(`Column 1\r\n${value}\r\n`);
  });
  it("bounds a large preview while preserving spikes, gaps, endpoints and index order", () => {
    const values: (number | null)[] = Array(100_000).fill(0);
    values[573] = 99;
    values[597] = -99;
    values[600] = null;
    const points = sampleSeries(values);
    expect(points.length).toBeLessThanOrEqual(400);
    expect(points).toContainEqual({ x: 574, y: 99 });
    expect(points).toContainEqual({ x: 598, y: -99 });
    expect(points).toContainEqual({ x: 601, y: null });
    expect(points[0].x).toBe(1);
    expect(points[points.length - 1]?.x).toBe(100_000);
    expect(points.every((p, i) => i === 0 || p.x > points[i - 1].x)).toBe(true);
  });
  it("handles empty and scalar data previews", () => {
    expect(sampleSeries([])).toEqual([]);
    expect(sampleSeries([5])).toEqual([{ x: 1, y: 5 }]);
    expect(formatNumber(null)).toBe("—");
    expect(formatNumber(Infinity)).toBe("—");
    expect(formatNumber(0)).toBe("0");
  });
});

describe("plot axis limits", () => {
  it("converts positive data limits to Plotly log coordinates, including reversed axes", () => {
    expect(axisRange([1, 1000], true)).toEqual([0, 3]);
    expect(axisRange([100, 0.01], true)).toEqual([2, -2]);
    expect(axisRange([-3, 7], false)).toEqual([-3, 7]);
  });
  it("falls back to automatic range for invalid logarithmic limits", () => {
    expect(axisRange([0, 10], true)).toBeUndefined();
    expect(axisRange([-1, 10], true)).toBeUndefined();
    expect(axisRange([1, Infinity], false)).toBeUndefined();
    expect(axisRange(undefined, false)).toBeUndefined();
  });
});
