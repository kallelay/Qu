export interface NumericVariable {
  name: string;
  type: string;
  shape: number[];
  /** Matrices use Qu's column-major storage. Null denotes a non-finite value. */
  data: (number | null)[];
}

export function formatNumber(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value === 0) return "0";
  const magnitude = Math.abs(value);
  return magnitude >= 1e6 || magnitude < 1e-4
    ? value.toExponential(4)
    : Number(value.toPrecision(7)).toString();
}

export function numericSummary(data: (number | null)[]) {
  let min = Infinity,
    max = -Infinity,
    count = 0;
  for (const value of data) {
    if (value == null || !Number.isFinite(value)) continue;
    min = Math.min(min, value);
    max = Math.max(max, value);
    count++;
  }
  if (!count)
    return { min: null, max: null, mean: null, count, missing: data.length };
  // Normalize before summing so finite values near MAX_VALUE have a finite mean.
  const scale = Math.max(Math.abs(min), Math.abs(max));
  let mean = 0;
  if (scale)
    for (const value of data) {
      if (value != null && Number.isFinite(value))
        mean += value / scale / count;
    }
  return {
    min,
    max,
    mean: Math.max(-1, Math.min(1, mean)) * scale,
    count,
    missing: data.length - count,
  };
}

export function matrixValue(
  variable: NumericVariable,
  row: number,
  column: number,
) {
  return variable.data[column * (variable.shape[0] ?? 0) + row];
}

/** Bounded preview preserving each bucket's extrema, order, and missing gaps. */
export function sampleSeries(data: (number | null)[], buckets = 80) {
  const samples: { x: number; y: number | null }[] = [];
  const step = Math.max(1, Math.ceil(data.length / Math.max(1, buckets)));
  for (let start = 0; start < data.length; start += step) {
    let lo = start,
      hi = start,
      missing = -1;
    for (let i = start; i < Math.min(start + step, data.length); i++) {
      const value = data[i];
      if (value == null || !Number.isFinite(value)) {
        missing = i;
        continue;
      }
      if (data[lo] == null || !Number.isFinite(data[lo]) || value < data[lo]!)
        lo = i;
      if (data[hi] == null || !Number.isFinite(data[hi]) || value > data[hi]!)
        hi = i;
    }
    const indices = new Set([
      start,
      lo,
      hi,
      Math.min(start + step, data.length) - 1,
    ]);
    if (missing >= 0) indices.add(missing);
    for (const i of [...indices].sort((a, b) => a - b)) {
      samples.push({
        x: i + 1,
        y: data[i] != null && Number.isFinite(data[i]) ? data[i] : null,
      });
    }
  }
  return samples;
}

export function variableCsv(variable: NumericVariable): string {
  const [rows = 0, columns = 1] = variable.shape;
  const lines = [
    Array.from({ length: columns }, (_, c) => `Column ${c + 1}`).join(","),
  ];
  for (let row = 0; row < rows; row++) {
    lines.push(
      Array.from({ length: columns }, (_, column) => {
        const value = matrixValue(variable, row, column);
        return value != null && Number.isFinite(value) ? String(value) : "";
      }).join(","),
    );
  }
  return lines.join("\r\n");
}

export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
