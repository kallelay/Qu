import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PlotViewer } from '@qu/ui-components';
import type { PlotData } from '@qu/ui-components';
import { AlertCircle, Copy, FileCode2 } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { SOLVE_EXAMPLES, buildSolve, defaultSolveSpec, readSolve, type SolveOutput, type SolveSpec } from './solveDesign';
import { errorContext, plainError } from './curveFitDesign';

export interface SolveEquationsProps extends LiveSerialPlotPanelProps {
  onOpenInEditor?: (code: string, name: string) => void;
}

interface ExecuteShape {
  success: boolean;
  error: string | null;
  output?: string;
  data?: Array<{ name: string; data: (number | null)[] }>;
}

let remembered: SolveSpec = defaultSolveSpec();

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border font-mono';
const fmt = (v: number) => (Number.isFinite(v) ? String(Number(v.toPrecision(10))) : 'n/a');

export const SolveEquations: React.FC<SolveEquationsProps> = ({ theme, invoke, onOpenInEditor }) => {
  const [spec, setSpec] = useState<SolveSpec>(remembered);
  useEffect(() => {
    remembered = spec;
  }, [spec]);
  const [out, setOut] = useState<{ o: SolveOutput; kind: string; text: string; warn: string[] } | null>(null);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const [copied, setCopied] = useState(false);
  const runId = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const built = useMemo(() => buildSolve(spec), [spec]);
  const set = (patch: Partial<SolveSpec>) => setSpec((s) => ({ ...s, ...patch }));
  const script = built.ok ? built.plan.script : '';

  const run = useCallback(async () => {
    if (!built.ok) return;
    const mine = ++runId.current;
    setRunning(true);
    try {
      const r = await invoke<ExecuteShape>('execute_code', { request: { code: built.plan.script, file_path: null } });
      if (mine !== runId.current) return;
      if (!r.success) {
        const ctx = errorContext(r.error ?? '', built.plan.script);
        setError(`${plainError(r.error ?? 'The script did not run')}${ctx ? ` (script line ${ctx.line}: ${ctx.text})` : ''}`);
        return;
      }
      const series: Record<string, (number | null)[]> = {};
      for (const v of r.data ?? []) series[v.name] = v.data;
      const text = r.output ?? '';
      setOut({ o: readSolve(built.plan, series), kind: built.plan.kind, text, warn: text.split(/\r?\n/).filter((l) => l.startsWith('CFWARN')).map((l) => l.replace(/^CFWARN\s*/, '')) });
      setError('');
    } catch (err) {
      if (mine === runId.current) setError(String(err));
    } finally {
      if (mine === runId.current) setRunning(false);
    }
  }, [built, invoke]);

  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    if (!built.ok) return;
    timer.current = setTimeout(() => void run(), 400);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [script, built.ok, run]);

  const problem = built.ok ? null : built.error;
  const plan = built.ok ? built.plan : null;

  const text = (label: string, key: keyof SolveSpec, placeholder = '') => (
    <label className="flex flex-col gap-1 text-xs flex-1 min-w-0">
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      <input value={spec[key] as string} placeholder={placeholder} onChange={(e) => set({ [key]: e.target.value } as Partial<SolveSpec>)} className={inputClass} style={{ borderColor: border }} spellCheck={false} />
    </label>
  );

  const plot = () => {
    if (!out || !plan || plan.kind === 'linear' || plan.kind === 'nonlinear' || plan.kind === 'multistart') return null;
    const o = out.o;
    const data: PlotData[] = [{ x: o.gx, y: o.gy, name: 'left side', type: 'line', mode: 'lines', line: { width: 2 } }];
    const rhsGiven = plan.twoSided || spec.rhs.trim() !== '' && spec.rhs.trim() !== '0' || spec.expr.includes('=');
    data.push({ x: o.gx, y: o.gy2, name: rhsGiven ? 'right side' : 'zero', type: 'line', mode: 'lines', line: { width: 1, color: '#888', dash: 'dash' } });
    const real = o.roots.filter((r) => Math.abs(r.im) < 1e-9);
    if (real.length) data.push({ x: real.map((r) => r.re), y: real.map((r) => r.y), name: 'solutions', type: 'scatter', mode: 'markers', marker: { size: 11, color: '#e45756' } });
    const mins = o.stationary.filter((s) => s.kind === 'minimum');
    const maxs = o.stationary.filter((s) => s.kind === 'maximum');
    if (mins.length) data.push({ x: mins.map((s) => s.x), y: mins.map((s) => s.y), name: 'minima', type: 'scatter', mode: 'markers', marker: { size: 10, color: '#54a24b', symbol: 'triangle-down' } });
    if (maxs.length) data.push({ x: maxs.map((s) => s.x), y: maxs.map((s) => s.y), name: 'maxima', type: 'scatter', mode: 'markers', marker: { size: 10, color: '#f58518', symbol: 'triangle-up' } });
    if (o.inflections.length) data.push({ x: o.inflections.map((s) => s.x), y: o.inflections.map((s) => s.y), name: 'inflections', type: 'scatter', mode: 'markers', marker: { size: 10, color: '#b279a2', symbol: 'diamond' } });
    const marks = [...real.map((r) => r.y), ...o.stationary.map((s) => s.y), ...o.inflections.map((s) => s.y)].filter(Number.isFinite);
    const curve = o.gy.filter(Number.isFinite);
    let ylim: [number, number] | undefined;
    if (marks.length && curve.length) {
      const lo = Math.min(...marks, ...(rhsGiven ? [] : [0]));
      const hi = Math.max(...marks, ...(rhsGiven ? [] : [0]));
      const pad = Math.max((hi - lo) * 0.6, 1e-3 + Math.abs(hi) * 0.05);
      ylim = [lo - pad, hi + pad];
    }
    return <PlotViewer data={data} theme={theme} showLegend height={380} xlabel={plan.unknown} ylabel="value" title="Curve with the solutions marked" {...(ylim ? { ylim } : {})} />;
  };

  const results = () => {
    if (!out || !plan) return <div className="p-6 text-sm" style={{ color: 'var(--qu-muted)' }}>{problem ? 'Fix the input on the left.' : 'Solving...'}</div>;
    const o = out.o;
    const row = (k: string, v: string, key?: string) => (
      <div key={key ?? k} className="flex justify-between gap-3 px-2 py-1.5 text-sm" style={{ borderBottom: '1px solid var(--qu-border)' }}>
        <span style={{ color: 'var(--qu-muted)' }}>{k}</span>
        <span className="font-mono text-right">{v}</span>
      </div>
    );
    const card = (title: string, rows: React.ReactNode, note = '') => (
      <div className="rounded-lg border mb-3" style={{ borderColor: border }}>
        <div className="px-2 py-1.5 text-xs font-medium" style={{ borderBottom: '1px solid var(--qu-border)' }}>{title}</div>
        {rows}
        {note && <div className="px-2 py-1.5 text-xs" style={{ color: 'var(--qu-muted)' }}>{note}</div>}
      </div>
    );
    const un = plan.unknowns;
    if (plan.kind === 'linear' || plan.kind === 'nonlinear' || plan.kind === 'multistart') {
      return (
        <>
          {out.warn.map((w, i) => (
            <div key={i} className="qu-banner mb-2" data-kind="warning" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', fontSize: 12.5, padding: '8px 10px' }}>
              <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
              <span>{w}</span>
            </div>
          ))}
          {plan.kind === 'linear' && plan.matrix && card('Coefficient matrix A and right-hand side b (A u = b)', (
            <div className="p-2 font-mono text-xs overflow-x-auto">
              <table>
                <tbody>
                  {plan.matrix.map((r, i) => (
                    <tr key={i}>
                      {r.map((v, j) => <td key={j} className="px-2">{fmt(v)}</td>)}
                      <td className="px-2" style={{ color: 'var(--qu-muted)' }}>|</td>
                      <td className="px-2">{fmt(plan.rhsVec![i])}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div style={{ color: 'var(--qu-muted)' }}>columns: {un.join(', ')}</div>
            </div>
          ))}
          {plan.kind === 'multistart'
            ? card(`Distinct solutions found (${o.solutions?.length ?? 0})`, (o.solutions ?? []).map((s, i) => row(`solution ${i + 1}`, un.map((u, k) => `${u} = ${fmt(s[k])}`).join(',  '))), 'Every solution comes from fsolve started on a grid of points; duplicates within 1e-6 are merged. A system can have solutions the grid never reached: widen the range or add points.')
            : card('Solution', [...un.map((u, i) => row(u, fmt(o.solution?.[i] ?? NaN))), row('residual norm', fmt(o.residual ?? NaN)), row('converged', o.converged ? 'yes' : 'NO'), ...(plan.kind === 'linear' ? [row('condition number', fmt(o.cond ?? NaN))] : [])])}
        </>
      );
    }
    const real = o.roots.filter((r) => Math.abs(r.im) < 1e-9);
    const cplx = o.roots.filter((r) => Math.abs(r.im) >= 1e-9);
    return (
      <>
        {spec.task === 'equation' &&
          card(`Solutions (${o.roots.length})${plan.kind === 'polynomial' ? ' - roots of the polynomial' : ` - sign changes found on [${plan.lo}, ${plan.hi}]`}`, [
            ...real.map((r, i) => row(`${plan.unknown} =`, fmt(r.re), `r${i}`)),
            ...cplx.map((r, i) => row(`${plan.unknown} =`, `${fmt(r.re)} ${r.im < 0 ? '-' : '+'} ${fmt(Math.abs(r.im))}j`, `c${i}`)),
            ...(o.roots.length === 0 ? [<div key="none" className="px-2 py-1.5 text-sm">No solution found{plan.kind === 'scan' ? ' in the interval: widen it, or the curve only touches zero' : ''}.</div>] : []),
          ])}
        {card(`Stationary points (${o.stationary.length})`, o.stationary.map((s, i) => row(`${s.kind} at ${plan.unknown} = ${fmt(s.x)}`, `f = ${fmt(s.y)}`, `s${i}`)), spec.task === 'extrema' ? 'Where the derivative is zero; minimum / maximum from the sign of the second derivative.' : '')}
        {spec.task === 'extrema' && card(`Inflection points (${o.inflections.length})`, o.inflections.map((s, i) => row(`${plan.unknown} = ${fmt(s.x)}`, `f = ${fmt(s.y)}`, `i${i}`)), 'Where the second derivative changes sign.')}
        {plan.kind === 'scan' && <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>Found by scanning the interval in 2000 steps and refining each sign change with fzero; derivatives are central differences of the generated function. Only the interval you set is searched.</p>}
      </>
    );
  };

  return (
    <>
      <div className="w-[22rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
        <span className="text-sm font-medium">Solve equations</span>
        <div className="flex gap-1">
          {([['equation', 'Roots / f = g'], ['extrema', 'Extrema'], ['system', 'System']] as const).map(([id, label]) => (
            <button key={id} className="qu-side-action flex-1" style={spec.task === id ? { background: 'var(--qu-accent)', color: '#fff' } : undefined} onClick={() => set({ task: id })}>{label}</button>
          ))}
        </div>
        <div className="flex flex-wrap gap-1">
          {SOLVE_EXAMPLES.map((e) => (
            <button key={e.label} className="qu-side-action text-xs" onClick={() => set(e.patch)}>{e.label}</button>
          ))}
        </div>
        {spec.task !== 'system' ? (
          <>
            {text('Expression f(x)  (or  f(x) = g(x))', 'expr', '2*x^2 + 3*x')}
            {spec.task === 'equation' && text('Right-hand side (a number or g(x))', 'rhs', '0')}
            <div className="flex gap-2">
              {text('Interval from', 'lo')}
              {text('to', 'hi')}
            </div>
            <label className="flex items-center gap-2 text-xs">
              <input type="checkbox" checked={spec.forceNumeric} onChange={(e) => set({ forceNumeric: e.target.checked })} />
              Always scan numerically (skip the polynomial shortcut)
            </label>
          </>
        ) : (
          <>
            <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>One equation per line, up to 4, in named unknowns (for example x^2 + y^2 = 4).</span>
            {spec.eqs.map((e, i) => (
              <input key={i} value={e} placeholder={`equation ${i + 1}`} onChange={(ev) => set({ eqs: spec.eqs.map((q, j) => (j === i ? ev.target.value : q)) })} className={inputClass} style={{ borderColor: border }} spellCheck={false} />
            ))}
            {plan && plan.kind !== 'linear' && (
              <div className="flex flex-col gap-1">
                <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>Starting guess per unknown (default 1)</span>
                <div className="flex gap-2">
                  {plan.unknowns.map((u) => (
                    <label key={u} className="flex flex-col gap-1 text-xs flex-1 min-w-0">
                      <span className="font-mono">{u}</span>
                      <input value={spec.guesses[u] ?? ''} placeholder="1" onChange={(e) => set({ guesses: { ...spec.guesses, [u]: e.target.value } })} className={inputClass} style={{ borderColor: border }} />
                    </label>
                  ))}
                </div>
                <label className="flex items-center gap-2 text-xs mt-1">
                  <input type="checkbox" checked={spec.multistart} onChange={(e) => set({ multistart: e.target.checked })} />
                  Try several starting points and list the distinct solutions
                </label>
                {spec.multistart && (
                  <div className="flex gap-2">
                    {text('range (+/-)', 'range')}
                    <label className="flex flex-col gap-1 text-xs flex-1 min-w-0">
                      <span style={{ color: 'var(--qu-muted)' }}>points per unknown</span>
                      <input type="number" min={2} max={9} value={spec.per} onChange={(e) => set({ per: Number(e.target.value) })} className={inputClass} style={{ borderColor: border }} />
                    </label>
                  </div>
                )}
              </div>
            )}
          </>
        )}
        {(problem || error) && (
          <div className="qu-banner" data-kind="error" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words">{problem ?? error}</span>
          </div>
        )}
        <div className="relative rounded-lg p-2 text-xs font-mono overflow-x-auto whitespace-pre" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)', maxHeight: 360 }}>
          <button
            title="Copy this script"
            className="absolute top-1 right-1 p-1 rounded"
            style={{ color: 'var(--qu-muted)' }}
            onClick={() => void navigator.clipboard?.writeText(script).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); })}
          >
            <Copy size={12} />
          </button>
          {script || '# fix the input to see the script'}
          {copied && <span style={{ color: 'var(--qu-ok)' }}> copied</span>}
        </div>
        {onOpenInEditor && (
          <button className="qu-side-action w-full" disabled={!built.ok} onClick={() => onOpenInEditor(script, 'solve.qu')}>
            <FileCode2 size={13} /> Open in editor
          </button>
        )}
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>{running ? 'Solving...' : 'Everything is computed by this script in real Qu; the typed text is parsed and re-written, never pasted in.'}</p>
      </div>
      <div className="flex-1 min-w-0 min-h-0 overflow-auto p-3">
        {plot()}
        {results()}
      </div>
    </>
  );
};

export default SolveEquations;
