import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { CodeEditor } from '@qu/ui-components';
import { AlertCircle, AlertTriangle, Check, Copy, FileCode2, Play, X } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';
import { MatrixInput, type SessionVar } from './MatrixInput';
import { BarsView, ComplexPlane, MatrixTable, copyText } from './MatrixTable';
import {
  DECOMPS,
  DEFAULT_SPEC,
  EXAMPLES,
  INLINE_LIMIT,
  MATFUNS,
  METHOD_LABEL,
  SOLVE_METHODS,
  TASKS,
  adviseSolve,
  buildLinalgScript,
  buildSummaryScript,
  checkPasses,
  conditionWarning,
  correctDigits,
  describeEngineError,
  flagsFromVars,
  formatComplex,
  formatNumber,
  indexVars,
  matVar,
  matricesNeeded,
  matrixRole,
  methodProblem,
  parseMarkers,
  scalarOrInf,
  scalarVar,
  sourceDims,
  sourceProblem,
  stagedBytes,
  validateSpec,
  vectorVar,
  type CardSpec,
  type EngineError,
  type LinalgSpec,
  type Mat,
  type MatrixFlags,
  type MatrixName,
  type MatrixSource,
  type RunVars,
  type ScriptPlan,
  type SolveMethodId,
} from './linalgDesign';

export interface LinAlgPanelProps extends LiveSerialPlotPanelProps {
  /** Opens `code` as a new editor buffer named `name` in the Code tab. */
  onOpenInEditor?: (code: string, name: string) => void;
  /** Numeric variables of the Code tab's last run, usable as a matrix source. */
  numericVars?: SessionVar[];
  /** Every value of one of them (column-major for a matrix). */
  loadVar?: (name: string) => Promise<(number | null)[]>;
  /** Starts from this spec instead of the remembered one (tests, deep links). */
  initialSpec?: LinalgSpec;
}

interface ExecuteShape {
  success: boolean;
  output?: string;
  error: string | null;
  data?: Array<{ name: string; type?: string; shape?: number[]; data: (number | null)[] }>;
}

interface Summary {
  key: string;
  flags: MatrixFlags | null;
  resolved: Mat | null;
  error: string;
  running: boolean;
}

// The panel unmounts whenever another top-level tab is shown; this keeps the
// work for the trip back.
const remembered: { spec: LinalgSpec; detached: boolean; code: string } = { spec: DEFAULT_SPEC, detached: false, code: '' };

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border';
const ALL: MatrixName[] = ['A', 'b', 'B', 'C'];

/** A cheap identity for a matrix source (to know when to re-measure it). */
export function sourceKey(src: MatrixSource): string {
  if (src.kind === 'grid') return `g:${JSON.stringify(src.cells)}`;
  if (src.kind === 'builder') return `b:${JSON.stringify(src.builder)}`;
  let h = 0;
  for (let i = 0; i < src.values.length; i++) h = (h * 31 + Math.round(src.values[i] * 1e6) + i) % 1_000_000_007;
  return `d:${src.origin}|${src.rows}x${src.cols}|${h}|${src.file ?? ''}`;
}

async function stageToFile(values: Float64Array): Promise<string> {
  const { writeBinaryFile, createDir } = await import('@tauri-apps/api/fs');
  const { appCacheDir, join } = await import('@tauri-apps/api/path');
  const dir = await appCacheDir();
  await createDir(dir, { recursive: true }).catch(() => undefined);
  // Named by a hash of the bytes: the same matrix reuses its file, so staging does not pile up.
  const bytes = new Uint8Array(values.buffer, values.byteOffset, values.byteLength);
  let h1 = 0x811c9dc5, h2 = 0x01000193;
  for (let i = 0; i < bytes.length; i++) {
    h1 = Math.imul(h1 ^ bytes[i], 0x01000193) >>> 0;
    h2 = (Math.imul(h2, 31) + bytes[i]) >>> 0;
  }
  const file = await join(dir, `linalg-matrix-${h1.toString(16)}${h2.toString(16)}-${values.length}.f64`);
  await writeBinaryFile(file, new Uint8Array(values.buffer));
  return file;
}

const emptySummary = (key = ''): Summary => ({ key, flags: null, resolved: null, error: '', running: false });

const Banner: React.FC<{ kind: 'error' | 'warning'; children: React.ReactNode }> = ({ kind, children }) => (
  <div className="qu-banner" data-kind={kind} style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
    {kind === 'error' ? <AlertCircle size={14} className="flex-shrink-0 mt-0.5" /> : <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />}
    <span className="break-words">{children}</span>
  </div>
);

const Card: React.FC<{ title: string; call: string; children: React.ReactNode; note?: string }> = ({ title, call, children, note }) => (
  <div className="rounded-lg border p-3 flex flex-col gap-1.5" style={{ borderColor: border }}>
    <div className="flex items-baseline justify-between gap-3">
      <span className="text-sm font-medium">{title}</span>
      <code className="text-xs" style={{ color: 'var(--qu-muted)' }} title="The Qu call that produced this">{call}</code>
    </div>
    {children}
    {note && <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>{note}</div>}
  </div>
);

export const LinAlgPanel: React.FC<LinAlgPanelProps> = ({ theme, invoke, onOpenInEditor, numericVars = [], loadVar, initialSpec }) => {
  const [spec, setSpecState] = useState<LinalgSpec>(initialSpec ?? remembered.spec);
  const [detached, setDetachedState] = useState(remembered.detached);
  const [codeText, setCodeTextState] = useState(remembered.code);
  const setSpec = useCallback((f: (s: LinalgSpec) => LinalgSpec) => setSpecState((s) => { const n = f(s); remembered.spec = n; return n; }), []);
  const setDetached = (v: boolean) => { remembered.detached = v; setDetachedState(v); };
  const setCodeText = (v: string) => { remembered.code = v; setCodeTextState(v); };

  const [summaries, setSummaries] = useState<Record<MatrixName, Summary>>({ A: emptySummary(), b: emptySummary(), B: emptySummary(), C: emptySummary() });
  const summariesRef = useRef(summaries);
  summariesRef.current = summaries;
  const [vars, setVars] = useState<RunVars>({});
  const [markers, setMarkers] = useState<Record<string, string>>({});
  const [error, setError] = useState<EngineError | null>(null);
  const [running, setRunning] = useState(false);
  const [ranPlan, setRanPlan] = useState<ScriptPlan | null>(null);
  const [copied, setCopied] = useState(false);
  const runId = useRef(0);
  const staging = useRef(new Set<string>());

  const needed = matricesNeeded(spec);
  const keyOf = (n: MatrixName) => sourceKey(spec.matrices[n]);
  const fresh = (n: MatrixName): Summary | null => (summaries[n].key === keyOf(n) ? summaries[n] : null);
  const flagsA = fresh('A')?.flags ?? null;

  const problem = useMemo(() => validateSpec(spec, { A: flagsA }), [spec, flagsA]);
  const waitingForFlags = spec.task === 'solve' && spec.solve.method === 'auto' && !flagsA && !fresh('A')?.error && !sourceProblem('A', spec.matrices.A);
  const plan: ScriptPlan | null = useMemo(() => {
    if (problem) return null;
    try {
      return buildLinalgScript(spec, flagsA);
    } catch {
      return null;
    }
  }, [spec, flagsA, problem]);
  const shownCode = detached ? codeText : plan?.script ?? '';

  // ---- measure each matrix the task uses (every flag is computed by Qu)
  const neededKeys = needed.map((n) => `${n}:${keyOf(n)}`).join('|');
  useEffect(() => {
    const timers: Array<ReturnType<typeof setTimeout>> = [];
    let live = true;
    for (const n of needed) {
      const src = spec.matrices[n];
      const key = sourceKey(src);
      if (sourceProblem(n, src)) continue;
      if (summariesRef.current[n].key === key && !summariesRef.current[n].running) continue;
      timers.push(setTimeout(async () => {
        if (!live) return;
        setSummaries((s) => ({ ...s, [n]: { ...s[n], key, running: true, error: '' } }));
        const script = buildSummaryScript(n, src);
        try {
          const r = await invoke<ExecuteShape>('execute_code', { request: { code: script, file_path: null } });
          if (!live) return;
          if (!r.success) {
            setSummaries((s) => ({ ...s, [n]: { key, flags: null, resolved: null, running: false, error: describeEngineError(r.error ?? 'The measurement did not run', script).message } }));
            return;
          }
          const v = indexVars(r.data);
          setSummaries((s) => ({ ...s, [n]: { key, flags: flagsFromVars(v), resolved: matVar(v, 'M'), running: false, error: '' } }));
        } catch (err) {
          if (live) setSummaries((s) => ({ ...s, [n]: { key, flags: null, resolved: null, running: false, error: String(err) } }));
        }
      }, 350));
    }
    return () => {
      live = false;
      timers.forEach(clearTimeout);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [neededKeys, invoke]);

  // ---- large pasted / file / variable data is staged to a file the script reads back
  useEffect(() => {
    for (const n of ALL) {
      const src = spec.matrices[n];
      if (src.kind !== 'data' || src.file || src.rows * src.cols <= INLINE_LIMIT) continue;
      const k = sourceKey(src);
      if (staging.current.has(k)) continue;
      staging.current.add(k);
      void (async () => {
        try {
          const file = await stageToFile(stagedBytes(src));
          setSpec((s) => {
            const cur = s.matrices[n];
            return cur.kind === 'data' && sourceKey(cur) === k ? { ...s, matrices: { ...s.matrices, [n]: { ...cur, file } } } : s;
          });
        } catch (err) {
          staging.current.delete(k);
          setError({ message: `Could not stage ${n} to a file (${err}). Large matrices need the desktop app.`, raw: String(err) });
        }
      })();
    }
  }, [spec.matrices, setSpec]);

  // ---- run
  const run = useCallback(async (code: string, planForRun: ScriptPlan | null) => {
    const mine = ++runId.current;
    setRunning(true);
    try {
      const r = await invoke<ExecuteShape>('execute_code', { request: { code, file_path: null } });
      if (mine !== runId.current) return;
      setRanPlan(planForRun);
      if (!r.success) {
        setError(describeEngineError(r.error ?? 'The script did not run', code));
        setVars({});
        setMarkers({});
        return;
      }
      setVars(indexVars(r.data));
      setMarkers(parseMarkers(r.output ?? ''));
      setError(null);
    } catch (err) {
      if (mine === runId.current) setError({ message: String(err), raw: String(err) });
    } finally {
      if (mine === runId.current) setRunning(false);
    }
  }, [invoke]);

  const activeCode = detached ? codeText : plan?.script ?? '';
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    if (detached) return; // an edited script runs when you press Run
    if (problem || !plan || waitingForFlags) return;
    timer.current = setTimeout(() => void run(plan.script, plan), 300);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan?.script, problem, waitingForFlags, detached, run]);

  // The cards belong to the run whose variables are on screen.
  const cardsPlan = ranPlan;

  // ---- spec helpers
  const set = (patch: Partial<LinalgSpec>) => setSpec((s) => ({ ...s, ...patch }));
  const setMatrix = (n: MatrixName, v: MatrixSource) => setSpec((s) => ({ ...s, matrices: { ...s.matrices, [n]: v } }));
  const setSolve = (patch: Partial<LinalgSpec['solve']>) => setSpec((s) => ({ ...s, solve: { ...s.solve, ...patch } }));
  const detach = () => {
    setCodeText(plan?.script ?? '');
    setDetached(true);
  };

  const advice = flagsA && spec.task === 'solve' ? adviseSolve(flagsA, sourceDims(spec.matrices.b)?.cols ?? 1) : null;
  const dA = sourceDims(spec.matrices.A);
  const maxK = dA ? Math.min(dA.rows, dA.cols) : 1;
  const bCols = sourceDims(spec.matrices.b)?.cols ?? 1;

  // ---- result cards
  const condVarName = ranPlan?.cards.find((c) => c.id === 'cond')?.vars[0] ?? 'condA';
  const cond = vars[condVarName] ? scalarOrInf(vars, condVarName) : NaN;
  const warning = spec.task === 'solve' ? conditionWarning(cond) : null;
  const converged = vars['converged'] ? scalarVar(vars, 'converged') : 1;
  const precision = spec.precision;

  const renderCard = (c: CardSpec) => {
    const missing = c.vars.some((v, i) => i === 0 && !vars[v]);
    if (missing) {
      return (
        <Card key={c.id} title={c.title} call={c.call}>
          <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
            {c.id === 'Ainv' ? markers['inv_error'] ?? 'Not produced.' : c.id === 'V' || c.id === 'err_av' || c.id === 'err_vo' ? 'Eigenvectors are available only for a symmetric matrix.' : error ? 'Not produced: see the error.' : 'Not produced.'}
          </div>
        </Card>
      );
    }
    switch (c.kind) {
      case 'matrix': {
        const m = matVar(vars, c.vars[0], c.vars[1]);
        return (
          <Card key={c.id} title={c.title} call={c.call} note={c.note}>
            {m ? <MatrixTable m={m} name={c.vars[0].replace(/_re$/, '')} precision={precision} /> : null}
          </Card>
        );
      }
      case 'scalar': {
        const isCond = /^cond/.test(c.vars[0]);
        const v = isCond ? scalarOrInf(vars, c.vars[0]) : scalarVar(vars, c.vars[0]);
        return (
          <Card key={c.id} title={c.title} call={c.call}>
            <div className="font-mono text-sm">{formatNumber(v, precision)}</div>
          </Card>
        );
      }
      case 'check': {
        const v = scalarVar(vars, c.vars[0]);
        const ok = checkPasses(v, c.tol ?? 1e-8);
        return (
          <Card key={c.id} title={c.title} call={c.call} note={c.note}>
            <div className="flex items-center gap-2 text-sm">
              {ok ? <Check size={14} style={{ color: 'var(--qu-ok)' }} /> : <X size={14} style={{ color: 'var(--qu-danger)' }} />}
              <span className="font-mono">{Number.isNaN(v) ? 'n/a' : formatNumber(v, precision)}</span>
              <span className="text-xs" style={{ color: ok ? 'var(--qu-ok)' : 'var(--qu-danger)' }}>{Number.isNaN(v) ? (markers['check_skipped'] ?? 'not computed') : ok ? `within ${formatNumber(c.tol ?? 1e-8, 2)}` : `above ${formatNumber(c.tol ?? 1e-8, 2)}: do not trust this result`}</span>
            </div>
          </Card>
        );
      }
      case 'values': {
        const re = vectorVar(vars, c.vars[0]);
        const im = vectorVar(vars, c.vars[1]);
        return (
          <Card key={c.id} title={c.title} call={c.call}>
            <div className="font-mono text-xs flex flex-col gap-0.5" style={{ maxHeight: 220, overflow: 'auto' }}>
              {re.map((r, i) => <div key={i}>{formatComplex(r, im[i] ?? 0, precision)}</div>)}
            </div>
          </Card>
        );
      }
      case 'plane':
        return (
          <Card key={c.id} title={c.title} call={c.call} note="Real part across, imaginary part up. A complex-conjugate pair sits symmetrically about the real axis.">
            <ComplexPlane re={vectorVar(vars, c.vars[0])} im={vectorVar(vars, c.vars[1])} />
          </Card>
        );
      case 'bars':
        return (
          <Card key={c.id} title={c.title} call={c.call} note="The ratio of the largest to the smallest is the condition number.">
            <BarsView values={vectorVar(vars, c.vars[0])} label="s" />
          </Card>
        );
      case 'compare': {
        const [en, eq, es, rn, rq, rs] = c.vars.map((v) => scalarVar(vars, v));
        const failed = markers['ne_error'];
        const rows: Array<[string, number, number, string]> = [
          ['Normal equations', en, rn, 'solve(A\'A, A\'b)'],
          ['QR', eq, rq, 'qr(A)'],
          ['SVD', es, rs, 'svd(A)'],
        ];
        return (
          <Card key={c.id} title={c.title} call={c.call} note={failed ? `The normal-equations solve failed: ${failed}` : 'Digits correct = -log10 of the relative error, out of the 16 a double holds. The residual can look fine even when the answer is not.'}>
            <div className="text-xs flex flex-col gap-1.5">
              {rows.map(([label, err, res, call]) => (
                <div key={label} className="flex items-center gap-2">
                  <span className="w-32" title={call}>{label}</span>
                  <div className="flex-1 h-3 rounded-sm" style={{ background: 'var(--qu-hover)' }}>
                    <div className="h-3 rounded-sm" style={{ width: `${(correctDigits(err) / 16) * 100}%`, background: correctDigits(err) > 8 ? 'var(--qu-ok)' : correctDigits(err) > 3 ? 'var(--qu-warning)' : 'var(--qu-danger)' }} />
                  </div>
                  <span className="w-44 font-mono text-right">{formatNumber(correctDigits(err), 3)} digits, residual {formatNumber(res, 3)}</span>
                </div>
              ))}
            </div>
          </Card>
        );
      }
    }
  };

  // ---- sidebar controls
  const field = (label: string, control: React.ReactNode) => (
    <label className="flex flex-col gap-1 text-xs flex-1 min-w-0">
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      {control}
    </label>
  );
  const lambdaExp = Math.round(Math.log10(spec.solve.lambda) * 2) / 2;

  const methodOptions = useMemo(() => {
    const f = flagsA ?? (dA ? ({ rows: dA.rows, cols: dA.cols } as MatrixFlags) : null);
    return SOLVE_METHODS.map((m) => ({ ...m, problem: f ? methodProblem(m.id, f, bCols, !!flagsA) : null }));
  }, [flagsA, dA, bCols]);

  return (
    <div className="qu-inspector flex flex-1 flex-col min-w-0 min-h-0" data-theme={theme}>
      <div className="flex flex-1 min-h-0 min-w-0">
        <div className="w-[26rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3 [&>*]:flex-shrink-0" style={{ borderColor: border }}>
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium">Linear algebra</span>
            <select
              value=""
              onChange={(e) => {
                const ex = EXAMPLES.find((x) => x.id === e.target.value);
                if (ex) {
                  setDetached(false);
                  setSpec((s) => ({ ...ex.spec, precision: s.precision }));
                }
              }}
              className={`${inputClass} text-xs`}
              style={{ borderColor: border, maxWidth: '15rem' }}
              aria-label="Load an example"
            >
              <option value="" disabled>Load an example...</option>
              {EXAMPLES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
            </select>
          </div>

          <div className="flex flex-col gap-1" role="tablist" aria-label="Task">
            {TASKS.map((t) => (
              <button
                key={t.id}
                role="tab"
                aria-selected={spec.task === t.id}
                onClick={() => { setDetached(false); set({ task: t.id }); }}
                className="text-left px-2 py-1.5 rounded-md text-sm border"
                style={{ borderColor: spec.task === t.id ? 'var(--qu-accent)' : border, color: spec.task === t.id ? 'var(--qu-accent)' : 'var(--qu-text)' }}
                title={t.blurb}
              >
                {t.label}
              </button>
            ))}
          </div>
          <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>{TASKS.find((t) => t.id === spec.task)!.blurb}</p>

          {spec.task === 'solve' && (
            <div className="flex flex-col gap-2">
              {advice && (
                <div className="rounded-md p-2 text-xs" style={{ background: 'var(--qu-hover)' }}>
                  <div><strong>Advisor:</strong> {METHOD_LABEL(advice.method)}</div>
                  <div style={{ color: 'var(--qu-muted)' }}>{advice.reason}</div>
                </div>
              )}
              {field('Method', (
                <select value={spec.solve.method} onChange={(e) => setSolve({ method: e.target.value as SolveMethodId | 'auto' })} className={inputClass} style={{ borderColor: border }}>
                  <option value="auto">Auto{advice ? `: ${METHOD_LABEL(advice.method)}` : ''}</option>
                  {methodOptions.map((m) => <option key={m.id} value={m.id} disabled={!!m.problem}>{m.label}{m.problem ? ' (not suitable here)' : ''}</option>)}
                </select>
              ))}
              {spec.solve.method !== 'auto' && methodOptions.find((m) => m.id === spec.solve.method)?.problem && (
                <div className="text-xs" style={{ color: 'var(--qu-danger)' }}>{methodOptions.find((m) => m.id === spec.solve.method)!.problem}</div>
              )}
              {spec.solve.method === 'tikhonov' && field(`Regularisation lambda = ${formatNumber(spec.solve.lambda, 3)}`, (
                <input type="range" min={-12} max={0} step={0.5} value={lambdaExp} onChange={(e) => setSolve({ lambda: Math.pow(10, Number(e.target.value)) })} />
              ))}
              {spec.solve.method === 'tsvd' && field(`Singular values kept: ${Math.min(spec.solve.tsvdK, maxK)} of ${maxK}`, (
                <input type="range" min={1} max={maxK} step={1} value={Math.min(spec.solve.tsvdK, maxK)} onChange={(e) => setSolve({ tsvdK: Number(e.target.value) })} />
              ))}
            </div>
          )}
          {spec.task === 'decomp' && field('Decomposition', (
            <select value={spec.decomp.kind} onChange={(e) => set({ decomp: { kind: e.target.value as LinalgSpec['decomp']['kind'] } })} className={inputClass} style={{ borderColor: border }}>
              {DECOMPS.map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
            </select>
          ))}
          {spec.task === 'lsq' && (
            <div className="flex flex-col gap-2 text-xs">
              <label className="flex items-center gap-2">
                <input type="radio" checked={spec.lsq.truth === 'ones'} onChange={() => set({ lsq: { ...spec.lsq, truth: 'ones' } })} />
                Make b = A times (1, ..., 1): the true answer is known
              </label>
              <label className="flex items-center gap-2">
                <input type="radio" checked={spec.lsq.truth === 'given'} onChange={() => set({ lsq: { ...spec.lsq, truth: 'given' } })} />
                Use my b (compare against the SVD solution)
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={spec.lsq.weighted} onChange={(e) => set({ lsq: { ...spec.lsq, weighted: e.target.checked } })} />
                Weighted: row weights from C
              </label>
            </div>
          )}
          {spec.task === 'matfun' && (
            <div className="flex gap-2">
              {field('Function', (
                <select value={spec.matfun.kind} onChange={(e) => set({ matfun: { ...spec.matfun, kind: e.target.value as LinalgSpec['matfun']['kind'] } })} className={inputClass} style={{ borderColor: border }}>
                  {MATFUNS.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
                </select>
              ))}
              {spec.matfun.kind === 'mpower' && field('Power p', (
                <input type="number" step={1} value={spec.matfun.power} onChange={(e) => set({ matfun: { ...spec.matfun, power: Number(e.target.value) } })} className={inputClass} style={{ borderColor: border }} />
              ))}
            </div>
          )}

          {needed.map((n) => (
            <MatrixInput
              key={n}
              name={n}
              role={`${matrixRole(spec, n)}`}
              value={spec.matrices[n]}
              onChange={(v) => setMatrix(n, v)}
              flags={fresh(n)?.flags ?? null}
              resolved={fresh(n)?.resolved ?? null}
              running={summaries[n].running}
              summaryError={fresh(n)?.error ?? ''}
              precision={precision}
              sessionVars={numericVars}
              loadVar={loadVar}
            />
          ))}

          <div className="flex items-center gap-2 text-xs">
            <span style={{ color: 'var(--qu-muted)' }}>Significant digits</span>
            <select value={precision} onChange={(e) => set({ precision: Number(e.target.value) })} className={inputClass} style={{ borderColor: border }} aria-label="Significant digits">
              {[3, 4, 5, 6, 8, 10, 12, 15].map((p) => <option key={p} value={p}>{p}</option>)}
            </select>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="font-medium">{detached ? 'Qu code (edited by you)' : 'Qu code (generated)'}</span>
            <div className="flex gap-1.5">
              {detached ? (
                <button className="px-1.5 py-0.5 rounded border" style={{ borderColor: border }} onClick={() => { setDetached(false); }} title="Throw away the edits and follow the settings again">Re-attach</button>
              ) : (
                <button className="px-1.5 py-0.5 rounded border" style={{ borderColor: border }} onClick={detach} disabled={!plan} title="Edit the script by hand; the panel then stops rewriting it">Edit the code</button>
              )}
            </div>
          </div>
          {detached ? (
            <div className="flex flex-col gap-2">
              <Banner kind="warning">Detached: the panel no longer rewrites this code when you change settings, and the matrices above are not used. Results are read from the variables your code leaves behind. Press Run.</Banner>
              <div className="rounded-xl overflow-hidden border" style={{ borderColor: border, height: 280 }}>
                <CodeEditor language="qu" value={codeText} onChange={(v: string) => setCodeText(v)} theme={theme} showMinimap={false} />
              </div>
              <button className="qu-side-action w-full" disabled={running} onClick={() => void run(codeText, ranPlan ?? plan)}>
                <Play size={13} /> {running ? 'Running...' : 'Run'}
              </button>
            </div>
          ) : (
            <div className="relative rounded-lg p-2 text-xs font-mono overflow-x-auto whitespace-pre" style={{ background: 'var(--qu-code-bg)', border: `1px solid ${border}`, color: 'var(--qu-text)', maxHeight: 340, overflowY: 'auto' }}>
              <button
                title="Copy this script"
                className="absolute top-1 right-1 p-1 rounded"
                style={{ color: 'var(--qu-muted)' }}
                onClick={() => void copyText(shownCode).then((ok) => { if (ok) { setCopied(true); setTimeout(() => setCopied(false), 1200); } })}
              >
                <Copy size={12} />
              </button>
              {shownCode || '# fix the problem shown on the right to see the script'}
              {copied && <span style={{ color: 'var(--qu-ok)' }}> copied</span>}
            </div>
          )}
          {onOpenInEditor && (
            <button className="qu-side-action w-full" disabled={!activeCode} onClick={() => onOpenInEditor(activeCode, `linalg_${spec.task}.qu`)} title="Open this script in a new editor tab">
              <FileCode2 size={13} /> Open in editor
            </button>
          )}
        </div>

        <div className="flex-1 min-w-0 min-h-0 overflow-auto p-3 flex flex-col gap-3">
          {!detached && problem && <Banner kind="error">{problem}</Banner>}
          {error && (
            <div className="flex flex-col gap-1">
              <Banner kind="error">{error.message}</Banner>
              {error.line && (
                <div className="text-xs font-mono rounded-md p-2 whitespace-pre-wrap" style={{ background: 'var(--qu-code-bg)', border: `1px solid ${border}` }}>
                  line {error.line} of the generated script:{'\n'}{error.source}
                </div>
              )}
            </div>
          )}
          {!detached && cardsPlan && !problem && (
            <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
              {running ? 'Running...' : cardsPlan.headline}
            </div>
          )}
          {warning && warning.level !== 'ok' && !error && (
            <div className="flex flex-col gap-2">
              <Banner kind="warning">{warning.text}</Banner>
              {spec.solve.method !== 'tikhonov' && spec.solve.method !== 'tsvd' && (
                <div className="flex gap-2 text-xs">
                  <button className="qu-side-action" onClick={() => setSolve({ method: 'tikhonov' })}>Try Tikhonov regularisation</button>
                  <button className="qu-side-action" onClick={() => setSolve({ method: 'tsvd' })}>Try truncated SVD</button>
                </div>
              )}
            </div>
          )}
          {converged === 0 && !error && <Banner kind="warning">The iterative solver did not reach its tolerance in the iteration limit: the answer is not converged. Try GMRES or the sparse LU method.</Banner>}
          {Object.entries(markers).filter(([k]) => k === 'inv_error' || k === 'check_skipped').map(([k, v]) => <div key={k} className="text-xs" style={{ color: 'var(--qu-muted)' }}>{v}</div>)}
          {(detached || !problem) && cardsPlan?.cards.map(renderCard)}
          {!cardsPlan && !problem && !error && !detached && <div className="text-sm" style={{ color: 'var(--qu-muted)' }}>Working...</div>}
          {Object.keys(vars).length > 0 && (cardsPlan?.cards.length ?? 0) === 0 && <div className="text-sm" style={{ color: 'var(--qu-muted)' }}>The code ran.</div>}
        </div>
      </div>
    </div>
  );
};

export default LinAlgPanel;
