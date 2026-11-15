import React, { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { autoRange, niceTicks } from './loopDesign';

/** Small SVG charts for the ML lab. Everything is drawn from numbers the
 *  generated script printed; nothing here computes a model quantity. */

export const PALETTE = ['#4e79a7', '#f28e2b', '#59a14f', '#e15759', '#b07aa1', '#76b7b2', '#edc948', '#9c755f'];
const muted = 'var(--qu-muted)';
const grid = 'var(--qu-border)';

const fmt = (v: number): string => (Number.isFinite(v) ? String(Number(v.toPrecision(3))) : 'n/a');

/** A box whose drawing size follows its width. */
const useWidth = (min = 240): [React.RefObject<HTMLDivElement>, number] => {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(420);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(Math.max(min, el.clientWidth)));
    ro.observe(el);
    setW(Math.max(min, el.clientWidth));
    return () => ro.disconnect();
  }, [min]);
  return [ref, w];
};

export const Card: React.FC<{ title: string; note?: string; children: React.ReactNode; testId?: string }> = ({ title, note, children, testId }) => (
  <div className="rounded-lg border p-3 flex flex-col gap-2 min-w-0" style={{ borderColor: grid }} data-testid={testId}>
    <div className="flex items-baseline justify-between gap-2">
      <span className="text-sm font-medium">{title}</span>
      {note && <span className="text-[11px] text-right" style={{ color: muted }}>{note}</span>}
    </div>
    {children}
  </div>
);

export interface LineSeries {
  name: string;
  x: number[];
  y: number[];
  color?: string;
  dashed?: boolean;
  /** Draw dots as well as the line. */
  dots?: boolean;
}

export const LineChart: React.FC<{
  series: LineSeries[];
  xLabel?: string;
  yLabel?: string;
  height?: number;
  /** Fixed ranges (e.g. 0..1 for ROC). */
  xRange?: [number, number];
  yRange?: [number, number];
  diagonal?: boolean;
  step?: boolean;
  testId?: string;
}> = ({ series, xLabel, yLabel, height = 220, xRange, yRange, diagonal, step, testId }) => {
  const [box, W] = useWidth();
  const m = { l: 44, r: 10, t: 8, b: 32 };
  const view = useMemo(() => {
    const xs: number[] = [];
    const ys: number[] = [];
    for (const s of series) {
      xs.push(...s.x);
      ys.push(...s.y);
    }
    const [x0, x1] = xRange ?? autoRange(xs);
    const [y0, y1] = yRange ?? autoRange(ys);
    return { x0, x1, y0, y1 };
  }, [series, xRange, yRange]);
  const px = (x: number) => m.l + ((x - view.x0) / (view.x1 - view.x0 || 1)) * (W - m.l - m.r);
  const py = (y: number) => m.t + ((view.y1 - y) / (view.y1 - view.y0 || 1)) * (height - m.t - m.b);
  const xt = niceTicks(view.x0, view.x1, Math.max(2, Math.floor((W - m.l - m.r) / 70)));
  const yt = niceTicks(view.y0, view.y1, 4);
  const path = (s: LineSeries): string => {
    let d = '';
    let pen = false;
    let lastY = 0;
    for (let i = 0; i < s.x.length; i++) {
      const x = s.x[i];
      const y = s.y[i];
      if (!Number.isFinite(x) || !Number.isFinite(y)) {
        pen = false;
        continue;
      }
      if (step && pen) d += `L${px(x).toFixed(1)},${py(lastY).toFixed(1)}`;
      d += `${pen ? 'L' : 'M'}${px(x).toFixed(1)},${py(y).toFixed(1)}`;
      pen = true;
      lastY = y;
    }
    return d;
  };
  return (
    <div ref={box} className="min-w-0" data-testid={testId}>
      <svg width={W} height={height} role="img" aria-label={yLabel ?? 'chart'} style={{ color: 'var(--qu-text)' }}>
        {yt.map((v) => (
          <g key={`y${v}`}>
            <line x1={m.l} x2={W - m.r} y1={py(v)} y2={py(v)} stroke={grid} strokeWidth={0.5} />
            <text x={m.l - 5} y={py(v) + 4} textAnchor="end" fontSize={10.5} fill={muted}>{fmt(v)}</text>
          </g>
        ))}
        {xt.map((v) => (
          <g key={`x${v}`}>
            <line x1={px(v)} x2={px(v)} y1={m.t} y2={height - m.b} stroke={grid} strokeWidth={0.5} />
            <text x={px(v)} y={height - m.b + 14} textAnchor="middle" fontSize={10.5} fill={muted}>{fmt(v)}</text>
          </g>
        ))}
        {diagonal && <line x1={px(view.x0)} y1={py(view.y0)} x2={px(view.x1)} y2={py(view.y1)} stroke={muted} strokeDasharray="4 3" strokeWidth={1} />}
        {series.map((s, i) => (
          <g key={s.name}>
            <path d={path(s)} fill="none" stroke={s.color ?? PALETTE[i % PALETTE.length]} strokeWidth={1.75} strokeDasharray={s.dashed ? '5 3' : undefined} />
            {s.dots && s.x.map((x, j) => (Number.isFinite(x) && Number.isFinite(s.y[j]) ? <circle key={j} cx={px(x)} cy={py(s.y[j])} r={2.6} fill={s.color ?? PALETTE[i % PALETTE.length]} /> : null))}
          </g>
        ))}
        {xLabel && <text x={(m.l + W - m.r) / 2} y={height - 3} textAnchor="middle" fontSize={11} fill={muted}>{xLabel}</text>}
        {yLabel && <text x={11} y={(m.t + height - m.b) / 2} textAnchor="middle" fontSize={11} fill={muted} transform={`rotate(-90 11 ${(m.t + height - m.b) / 2})`}>{yLabel}</text>}
      </svg>
      {series.length > 1 && (
        <div className="flex flex-wrap gap-3 text-[11px] mt-1" style={{ color: muted }}>
          {series.map((s, i) => (
            <span key={s.name} className="flex items-center gap-1">
              <span style={{ display: 'inline-block', width: 14, height: 2, background: s.color ?? PALETTE[i % PALETTE.length] }} /> {s.name}
            </span>
          ))}
        </div>
      )}
    </div>
  );
};

/** Points coloured by an integer class / cluster label (-1 = noise, grey). */
export const ScatterChart: React.FC<{
  x: number[];
  y: number[];
  c?: number[];
  labels?: string[];
  centers?: Array<{ x: number; y: number }>;
  xLabel?: string;
  yLabel?: string;
  height?: number;
  /** Plot y against x with the diagonal (predicted vs actual). */
  diagonal?: boolean;
  testId?: string;
}> = ({ x, y, c, labels, centers, xLabel, yLabel, height = 260, diagonal, testId }) => {
  const [box, W] = useWidth();
  const m = { l: 44, r: 10, t: 8, b: 32 };
  const view = useMemo(() => {
    const xs = x.concat((centers ?? []).map((p) => p.x));
    const ys = y.concat((centers ?? []).map((p) => p.y));
    let [x0, x1] = autoRange(xs);
    let [y0, y1] = autoRange(ys);
    if (diagonal) {
      x0 = y0 = Math.min(x0, y0);
      x1 = y1 = Math.max(x1, y1);
    }
    return { x0, x1, y0, y1 };
  }, [x, y, centers, diagonal]);
  const px = (v: number) => m.l + ((v - view.x0) / (view.x1 - view.x0 || 1)) * (W - m.l - m.r);
  const py = (v: number) => m.t + ((view.y1 - v) / (view.y1 - view.y0 || 1)) * (height - m.t - m.b);
  const xt = niceTicks(view.x0, view.x1, Math.max(2, Math.floor((W - m.l - m.r) / 70)));
  const yt = niceTicks(view.y0, view.y1, 5);
  const color = (k: number | undefined) => (k === undefined || !Number.isFinite(k) ? muted : k < 0 ? '#9aa0a6' : PALETTE[Math.round(k) % PALETTE.length]);
  const classes = c ? Array.from(new Set(c.filter((v) => Number.isFinite(v)))).sort((a, b) => a - b) : [];
  return (
    <div ref={box} className="min-w-0" data-testid={testId} data-points={x.length}>
      <svg width={W} height={height} role="img" aria-label="scatter plot" style={{ color: 'var(--qu-text)' }}>
        {yt.map((v) => (
          <g key={`y${v}`}>
            <line x1={m.l} x2={W - m.r} y1={py(v)} y2={py(v)} stroke={grid} strokeWidth={0.5} />
            <text x={m.l - 5} y={py(v) + 4} textAnchor="end" fontSize={10.5} fill={muted}>{fmt(v)}</text>
          </g>
        ))}
        {xt.map((v) => (
          <g key={`x${v}`}>
            <line x1={px(v)} x2={px(v)} y1={m.t} y2={height - m.b} stroke={grid} strokeWidth={0.5} />
            <text x={px(v)} y={height - m.b + 14} textAnchor="middle" fontSize={10.5} fill={muted}>{fmt(v)}</text>
          </g>
        ))}
        {diagonal && <line x1={px(view.x0)} y1={py(view.y0)} x2={px(view.x1)} y2={py(view.y1)} stroke={muted} strokeDasharray="4 3" />}
        {x.map((v, i) => (Number.isFinite(v) && Number.isFinite(y[i]) ? <circle key={i} cx={px(v)} cy={py(y[i])} r={2.6} fill={color(c?.[i])} fillOpacity={0.75} /> : null))}
        {(centers ?? []).map((p, i) => (
          <g key={`c${i}`} stroke="var(--qu-text)" strokeWidth={2}>
            <line x1={px(p.x) - 6} x2={px(p.x) + 6} y1={py(p.y) - 6} y2={py(p.y) + 6} />
            <line x1={px(p.x) - 6} x2={px(p.x) + 6} y1={py(p.y) + 6} y2={py(p.y) - 6} />
          </g>
        ))}
        {xLabel && <text x={(m.l + W - m.r) / 2} y={height - 3} textAnchor="middle" fontSize={11} fill={muted}>{xLabel}</text>}
        {yLabel && <text x={11} y={(m.t + height - m.b) / 2} textAnchor="middle" fontSize={11} fill={muted} transform={`rotate(-90 11 ${(m.t + height - m.b) / 2})`}>{yLabel}</text>}
      </svg>
      {classes.length > 1 && (
        <div className="flex flex-wrap gap-3 text-[11px] mt-1" style={{ color: muted }}>
          {classes.map((k) => (
            <span key={k} className="flex items-center gap-1">
              <span style={{ display: 'inline-block', width: 9, height: 9, borderRadius: 5, background: color(k) }} /> {k < 0 ? 'noise' : labels?.[k] ?? `cluster ${k}`}
            </span>
          ))}
        </div>
      )}
    </div>
  );
};

/** Horizontal bars with signed values (feature importance, compare). */
export const BarList: React.FC<{
  rows: Array<{ label: string; value: number; extra?: number; note?: string }>;
  /** Draw +/- extra as a whisker. */
  testId?: string;
  format?: (v: number) => string;
  min?: number;
  max?: number;
}> = ({ rows, testId, format = fmt, min, max }) => {
  const lo = Math.min(0, min ?? Math.min(...rows.map((r) => r.value - (r.extra ?? 0)), 0));
  const hi = Math.max(max ?? Math.max(...rows.map((r) => r.value + (r.extra ?? 0)), 1e-9), 1e-9);
  const span = hi - lo || 1;
  const pos = (v: number) => `${((v - lo) / span) * 100}%`;
  return (
    <div className="flex flex-col gap-1" data-testid={testId}>
      {rows.map((r, i) => (
        <div key={r.label} className="flex items-center gap-2 text-xs" data-row={r.label}>
          <span className="w-32 truncate text-right font-mono" title={r.label}>{r.label}</span>
          <div className="relative flex-1 h-4 rounded" style={{ background: 'var(--qu-shell-sunken)' }}>
            <div
              className="absolute top-0 bottom-0 rounded"
              style={{ left: pos(Math.min(0, r.value)), width: `${(Math.abs(r.value) / span) * 100}%`, background: r.value < 0 ? 'var(--qu-danger)' : PALETTE[0], opacity: i === 0 ? 1 : 0.85 }}
            />
            {r.extra !== undefined && Number.isFinite(r.extra) && r.extra > 0 && (
              <div className="absolute top-1/2" style={{ left: pos(r.value - r.extra), width: `${((2 * r.extra) / span) * 100}%`, height: 1, background: 'var(--qu-text)' }} />
            )}
          </div>
          <span className="w-24 font-mono text-[11px]" style={{ color: muted }}>{format(r.value)}{r.extra !== undefined && Number.isFinite(r.extra) ? ` ± ${format(r.extra)}` : ''}</span>
        </div>
      ))}
    </div>
  );
};

/** Counts as vertical bars (class balance, histograms, score distributions). */
export const Columns: React.FC<{ counts: number[]; labels?: string[]; height?: number; testId?: string; xNote?: string }> = ({ counts, labels, height = 90, testId, xNote }) => {
  const top = Math.max(...counts, 1);
  return (
    <div data-testid={testId}>
      <div className="flex items-end gap-1" style={{ height }}>
        {counts.map((n, i) => (
          <div key={i} className="flex-1 flex flex-col items-center justify-end min-w-0" title={`${labels?.[i] ?? i}: ${n}`}>
            <span className="text-[10px]" style={{ color: muted }}>{n}</span>
            <div style={{ height: `${(n / top) * (height - 16)}px`, width: '100%', background: PALETTE[labels ? i % PALETTE.length : 0], borderRadius: 2 }} />
          </div>
        ))}
      </div>
      {labels && (
        <div className="flex gap-1 text-[10px]" style={{ color: muted }}>
          {labels.map((l, i) => <span key={i} className="flex-1 text-center truncate" title={l}>{l}</span>)}
        </div>
      )}
      {xNote && <div className="text-[10px] mt-0.5" style={{ color: muted }}>{xNote}</div>}
    </div>
  );
};

/** The confusion matrix as a coloured grid (rows: actual, columns: predicted). */
export const ConfusionGrid: React.FC<{ cm: number[][]; labels: string[]; testId?: string }> = ({ cm, labels, testId }) => {
  const k = cm.length;
  const top = Math.max(1, ...cm.flat());
  return (
    <div className="overflow-x-auto" data-testid={testId}>
      <table className="text-xs" style={{ borderCollapse: 'separate', borderSpacing: 2 }}>
        <thead>
          <tr>
            <th />
            <th colSpan={k} className="font-normal pb-1" style={{ color: muted }}>predicted</th>
          </tr>
          <tr>
            <th className="font-normal text-right pr-1" style={{ color: muted }}>actual</th>
            {labels.slice(0, k).map((l) => <th key={l} className="font-mono font-normal px-1 max-w-[5rem] truncate" title={l}>{l}</th>)}
          </tr>
        </thead>
        <tbody>
          {cm.map((row, i) => {
            const total = row.reduce((a, b) => a + b, 0) || 1;
            return (
              <tr key={i}>
                <td className="font-mono text-right pr-1 max-w-[6rem] truncate" title={labels[i]}>{labels[i]}</td>
                {row.map((v, j) => (
                  <td
                    key={j}
                    className="text-center font-mono rounded"
                    style={{
                      width: 52,
                      height: 38,
                      background: `color-mix(in srgb, ${i === j ? 'var(--qu-ok)' : 'var(--qu-danger)'} ${Math.round((v / top) * 70)}%, transparent)`,
                      border: '1px solid var(--qu-border)',
                    }}
                    title={`${v} of ${total} actual "${labels[i]}" predicted "${labels[j]}" (${Math.round((v / total) * 100)}%)`}
                  >
                    {v}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};
