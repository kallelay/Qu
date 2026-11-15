import React, { useState } from 'react';
import { Copy } from 'lucide-react';
import { formatComplex, formatNumber, matToCsv, matToQuLiteral, type Mat } from './linalgDesign';

const border = 'var(--qu-border)';
const MAX_SHOWN_ROWS = 200;
const MAX_SHOWN_COLS = 40;

export const copyText = async (text: string): Promise<boolean> => {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
};

/** A matrix as an aligned, scrollable table with copy buttons. Complex
 *  entries read `a + bj`, as the engine prints them. Very large matrices
 *  show their top-left corner; the copy buttons always copy everything. */
export const MatrixTable: React.FC<{ m: Mat; name: string; precision: number; maxHeight?: number }> = ({ m, name, precision, maxHeight = 320 }) => {
  const [note, setNote] = useState('');
  const rows = Math.min(m.rows, MAX_SHOWN_ROWS);
  const cols = Math.min(m.cols, MAX_SHOWN_COLS);
  const cell = (i: number, j: number) => (m.im ? formatComplex(m.re[i][j], m.im[i][j], precision) : formatNumber(m.re[i][j], precision));
  const body: string[][] = [];
  for (let i = 0; i < rows; i++) body.push(Array.from({ length: cols }, (_, j) => cell(i, j)));
  const flash = (msg: string) => {
    setNote(msg);
    setTimeout(() => setNote(''), 1400);
  };
  return (
    <div>
      {m.cols === 0 || m.rows === 0 ? (
        <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>An empty matrix ({m.rows} x {m.cols}).</div>
      ) : (
        <div className="overflow-auto rounded-md border" style={{ borderColor: border, maxHeight }}>
          <table className="text-xs font-mono" style={{ borderCollapse: 'collapse' }}>
            <tbody>
              {body.map((r, i) => (
                <tr key={i}>
                  {r.map((c, j) => (
                    <td key={j} className="px-2 py-0.5 text-right whitespace-pre" style={{ borderBottom: `1px solid ${border}`, color: 'var(--qu-text)' }}>
                      {c}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="flex items-center gap-2 mt-1 text-xs" style={{ color: 'var(--qu-muted)' }}>
        <span>
          {m.rows} x {m.cols}
          {(m.rows > rows || m.cols > cols) && ` (showing ${rows} x ${cols})`}
        </span>
        <button
          className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded border"
          style={{ borderColor: border }}
          title="Copy the whole matrix as a Qu literal"
          onClick={() => void copyText(matToQuLiteral(name, m)).then((ok) => flash(ok ? 'copied Qu literal' : 'copy failed'))}
        >
          <Copy size={11} /> Qu literal
        </button>
        <button
          className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded border"
          style={{ borderColor: border }}
          title="Copy the whole matrix as CSV"
          onClick={() => void copyText(matToCsv(m)).then((ok) => flash(ok ? 'copied CSV' : 'copy failed'))}
        >
          <Copy size={11} /> CSV
        </button>
        {note && <span style={{ color: 'var(--qu-ok)' }}>{note}</span>}
      </div>
    </div>
  );
};

/** Eigenvalues (or any numbers with an optional imaginary part) in the complex plane. */
export const ComplexPlane: React.FC<{ re: number[]; im: number[] }> = ({ re, im }) => {
  const W = 360;
  const H = 260;
  const pad = 34;
  const finite = re.every(Number.isFinite) && im.every(Number.isFinite);
  const span = Math.max(1e-12, ...re.map(Math.abs), ...im.map(Math.abs)) * 1.25;
  const xr = Math.max(span, ...re.map(Math.abs)) ;
  const yr = Math.max(span * 0.6, ...im.map(Math.abs)) * 1.0;
  const sx = (v: number) => pad + ((v + xr) / (2 * xr)) * (W - 2 * pad);
  const sy = (v: number) => H - pad - ((v + yr) / (2 * yr)) * (H - 2 * pad);
  if (!finite) return <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>Some eigenvalues are not finite, so they cannot be drawn.</div>;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', maxWidth: 480, color: 'var(--qu-text)' }} role="img" aria-label="Eigenvalues in the complex plane">
      <line x1={pad} x2={W - pad} y1={sy(0)} y2={sy(0)} stroke="var(--qu-border)" />
      <line x1={sx(0)} x2={sx(0)} y1={pad} y2={H - pad} stroke="var(--qu-border)" />
      <text x={W - pad} y={sy(0) - 4} textAnchor="end" fontSize={10} fill="var(--qu-muted)">real</text>
      <text x={sx(0) + 4} y={pad - 4} fontSize={10} fill="var(--qu-muted)">imag</text>
      <text x={pad} y={sy(0) + 14} fontSize={10} fill="var(--qu-muted)">{formatNumber(-xr, 3)}</text>
      <text x={W - pad} y={sy(0) + 14} textAnchor="end" fontSize={10} fill="var(--qu-muted)">{formatNumber(xr, 3)}</text>
      {re.map((r, i) => (
        <g key={i}>
          <circle cx={sx(r)} cy={sy(im[i])} r={5} fill="var(--qu-accent)" fillOpacity={0.85} stroke="var(--qu-bg)" />
          <title>{formatComplex(r, im[i], 6)}</title>
        </g>
      ))}
    </svg>
  );
};

/** Horizontal bars; a log scale when the values span more than three decades. */
export const BarsView: React.FC<{ values: number[]; label: string }> = ({ values, label }) => {
  const pos = values.filter((v) => v > 0 && Number.isFinite(v));
  const max = Math.max(...pos, 0);
  const min = Math.min(...pos, max);
  const log = max > 0 && min > 0 && max / min > 1e3;
  const len = (v: number) => {
    if (!(v > 0) || !Number.isFinite(v)) return 0;
    if (!log) return (v / max) * 100;
    const lo = Math.log10(min) - 0.3;
    return ((Math.log10(v) - lo) / (Math.log10(max) - lo)) * 100;
  };
  return (
    <div className="text-xs">
      {values.map((v, i) => (
        <div key={i} className="flex items-center gap-2 py-0.5">
          <span className="w-12 text-right" style={{ color: 'var(--qu-muted)' }}>{label}{i + 1}</span>
          <div className="flex-1 h-3 rounded-sm" style={{ background: 'var(--qu-hover)' }}>
            <div className="h-3 rounded-sm" style={{ width: `${len(v)}%`, background: 'var(--qu-accent)' }} />
          </div>
          <span className="w-24 font-mono text-right">{formatNumber(v, 5)}</span>
        </div>
      ))}
      {log && <div style={{ color: 'var(--qu-muted)' }}>Bars are on a log scale: the values span {formatNumber(max / min, 2)}x.</div>}
    </div>
  );
};
