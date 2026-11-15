import { describe, expect, it } from 'vitest';
import {
  DEFAULT_LOOP,
  TRACE_SIGNALS,
  buildCompareScript,
  buildReplayScript,
  compareVerdict,
  defaultTolerances,
  rowReason,
  toleranceProblem,
  tracePathProblem,
  WORKED_EXAMPLES,
  PORT_LIST_SCRIPT,
  autoRange,
  buildLoopScript,
  decimation,
  ioProblem,
  niceTicks,
  parseLoopLine,
  parsePortList,
  quPath,
  splitChunk,
  usesIo,
  validateLoop,
  verdict,
  type LoopSpec,
} from './loopDesign';

const sil = DEFAULT_LOOP;
const hil: LoopSpec = { ...DEFAULT_LOOP, mode: 'hil', duration: 3, dt: 0.002 };

describe('validateLoop', () => {
  it('accepts both worked examples', () => {
    for (const ex of WORKED_EXAMPLES) expect(validateLoop(ex.spec)).toBeNull();
  });

  it('rejects bad timing', () => {
    expect(validateLoop({ ...sil, duration: 0 })).toMatch(/Duration/);
    expect(validateLoop({ ...sil, dt: -1 })).toMatch(/Step/);
    expect(validateLoop({ ...sil, dt: 10 })).toMatch(/longer than the duration/);
    expect(validateLoop({ ...sil, duration: 1000, dt: 1e-6 })).toMatch(/steps/);
    expect(validateLoop({ ...hil, pace: 0 })).toMatch(/Real-time factor/);
    expect(validateLoop({ ...hil, toleranceMs: -1 })).toMatch(/tolerance/);
    expect(validateLoop({ ...hil, dt: 0.00005 })).toMatch(/0.1 ms/);
    // The same short step is fine on the virtual clock.
    expect(validateLoop({ ...sil, dt: 0.00005, duration: 1 })).toBeNull();
  });

  it('needs a usable source for each slot', () => {
    expect(validateLoop({ ...sil, plant: { ...sil.plant, kind: 'file', path: '' } })).toMatch(/Plant: choose a .qu file/);
    expect(validateLoop({ ...sil, plant: { ...sil.plant, kind: 'file', path: 'C:/a"b.qu' } })).toMatch(/cannot contain/);
    expect(validateLoop({ ...sil, plant: { ...sil.plant, kind: 'clib', path: 'x.dll' } })).toMatch(/only be the controller/);
    expect(validateLoop({ ...sil, controller: { ...sil.controller, kind: 'clib', path: 'pid.dll', func: '2bad' } })).toMatch(/identifier/);
    expect(validateLoop({ ...sil, controller: { ...sil.controller, kind: 'editor' } }, { plant: true, controller: false })).toMatch(/no Qu file open/);
    expect(validateLoop({ ...sil, plant: { ...sil.plant, example: 'nope' } })).toMatch(/pick an example/);
  });

  it('checks the setpoint', () => {
    expect(validateLoop({ ...sil, setpoint: { kind: 'sine', amplitude: 1, time: 0 } })).toMatch(/period/);
    expect(validateLoop({ ...sil, setpoint: { kind: 'step', amplitude: 1, time: 0 } })).toBeNull();
    expect(validateLoop({ ...sil, setpoint: { kind: 'step', amplitude: NaN, time: 0 } })).toMatch(/amplitude/);
  });
});

describe('buildLoopScript', () => {
  it('SIL: a virtual run_for, no pacing, no lateness bookkeeping', () => {
    const s = buildLoopScript(sil);
    expect(s).toContain('run_for(duration)\n');
    expect(s).not.toContain('pace=');
    expect(s).not.toContain('toc()');
    expect(s).toContain('every dt do');
    expect(s).toContain('function plant(u, dt)');
    expect(s).toContain('function controller(r, y, dt)');
    expect(s).toContain('y = plant(0, 0)');
    expect(s).toContain('print("@stats ');
  });

  it('HIL: paced run_for with the tolerance, and live lateness from the stopwatch', () => {
    const s = buildLoopScript({ ...hil, pace: 2, toleranceMs: 0.5 });
    expect(s).toContain('tic()\nrun_for(duration, pace=pace, tolerance=0.5 ms)');
    expect(s).toContain('pace = 2');
    expect(s).toContain('late = max(0, toc() * 1000 - t * 1000 / pace)');
    expect(s).toMatch(/@loop k=\{k\} t=\{t\} late=\{late\} over=\{n_over\} lmax=\{late_max\}/);
  });

  it('prints only the watched signals, in a fixed order', () => {
    const s = buildLoopScript({ ...sil, watch: ['u', 'e'] });
    expect(s).toContain('print("@loop k={k} t={t} u={u} e={err}")');
  });

  it('writes the setpoint as a Qu function', () => {
    expect(buildLoopScript({ ...sil, setpoint: { kind: 'step', amplitude: 2, time: 0.25 } })).toContain('if t >= 0.25\n        return 2');
    expect(buildLoopScript({ ...sil, setpoint: { kind: 'square', amplitude: 1, time: 2 } })).toContain('if (t mod 2) < 1');
    expect(buildLoopScript({ ...sil, setpoint: { kind: 'sine', amplitude: 3, time: 4 } })).toContain('return 3 * sin(2 * pi * t / 4)');
  });

  it('imports a file, inlines an editor buffer, wraps a C library', () => {
    const s = buildLoopScript(
      { ...sil, plant: { ...sil.plant, kind: 'file', path: 'C:\\work\\plant.qu' }, controller: { kind: 'clib', example: '', path: 'D:\\lib\\pid.dll', func: 'pid_step' } },
    );
    expect(s).toContain('import "C:/work/plant.qu"');
    expect(s).toContain('ctrl_lib = load_library("D:/lib/pid.dll")');
    expect(s).toContain('native_call(ctrl_lib, "pid_step", "double(double, double, double)", r, y, dt)');
    const e = buildLoopScript({ ...sil, controller: { ...sil.controller, kind: 'editor' } }, { controllerEditor: { name: 'my_ctl.qu', code: 'function controller(r, y, dt)\n    return 0\nend function\n\n' } });
    expect(e).toContain('# ---- controller: my_ctl.qu (from the Code tab) ----\nfunction controller(r, y, dt)\n    return 0\nend function\n');
  });
});

describe('decimation', () => {
  it('keeps a SIL run near 4000 scope points and a paced one near 100 lines a second', () => {
    expect(decimation(sil)).toBe(2); // 5000 steps
    expect(decimation({ ...sil, duration: 1, dt: 0.01 })).toBe(1);
    expect(decimation(hil)).toBe(5); // 2 ms steps -> every 10 ms
    expect(decimation({ ...hil, pace: 2 })).toBe(10);
    expect(decimation({ ...hil, dt: 0.05 })).toBe(1);
  });
});

describe('parseLoopLine', () => {
  it('reads a sample, with and without timing', () => {
    expect(parseLoopLine('@loop k=10 t=0.01 r=1 y=0.5 u=2')).toEqual({ kind: 'sample', sample: { k: 10, t: 0.01, late: undefined, over: undefined, lmax: undefined, values: { r: 1, y: 0.5, u: 2 } } });
    const p = parseLoopLine('@loop k=5 t=0.01 late=0.0365 over=2 lmax=1.5 e=-0.25');
    expect(p.kind === 'sample' && p.sample).toMatchObject({ late: 0.0365, over: 2, lmax: 1.5, values: { e: -0.25 } });
  });

  it('keeps NaN and infinity from a diverging loop', () => {
    const p = parseLoopLine('@loop k=1 t=0.1 y=NaN u=inf e=-inf');
    expect(p.kind === 'sample' && p.sample.values).toEqual({ y: NaN, u: Infinity, e: -Infinity });
  });

  it('reads the stats line', () => {
    expect(parseLoopLine('@stats steps=1500 overruns=3 max_ms=2.5 mean_ms=0.01 wall_ms=3000.02 paced=1')).toEqual({
      kind: 'stats',
      stats: { steps: 1500, overruns: 3, maxMs: 2.5, meanMs: 0.01, wallMs: 3000.02, paced: true },
    });
  });

  it('leaves everything else as text', () => {
    expect(parseLoopLine('· run_for(5s): 4999 timer firing(s)')).toEqual({ kind: 'text', text: '· run_for(5s): 4999 timer firing(s)' });
    expect(parseLoopLine('@loop garbage')).toEqual({ kind: 'text', text: '@loop garbage' });
  });
});

describe('splitChunk', () => {
  it('holds a partial line until its newline arrives', () => {
    const a = splitChunk('', '@loop k=1 t=0.1\n@loop k=2');
    expect(a).toEqual({ lines: ['@loop k=1 t=0.1'], rest: '@loop k=2' });
    expect(splitChunk(a.rest, ' t=0.2\r\n')).toEqual({ lines: ['@loop k=2 t=0.2'], rest: '' });
  });
});

describe('verdict', () => {
  it('SIL reports how much faster than real time', () => {
    const v = verdict(sil, null, { steps: 4999, overruns: 0, maxMs: 0, meanMs: 0, wallMs: 25, paced: false });
    expect(v).toMatchObject({ kind: 'fast', text: '200x faster than real time' });
    expect(verdict(sil, null, null).kind).toBe('idle');
  });

  it('HIL on time, and late with the worst lateness', () => {
    expect(verdict(hil, null, { steps: 1500, overruns: 0, maxMs: 0.18, meanMs: 0.001, wallMs: 3000, paced: true })).toMatchObject({ kind: 'ok', text: 'on time' });
    const late = verdict(hil, null, { steps: 1500, overruns: 12, maxMs: 23.4, meanMs: 0.4, wallMs: 3020, paced: true });
    expect(late).toMatchObject({ kind: 'late', text: 'running late by 23.4 ms' });
    expect(late.detail).toMatch(/12 of 1500 steps/);
  });

  it('reads the live counters while the run is going', () => {
    expect(verdict(hil, { simT: 1, wallMs: 1000, over: 0, lmax: 0.2, steps: 500 }, null).text).toBe('on time');
    expect(verdict(hil, { simT: 1, wallMs: 1000, over: 4, lmax: 7.25, steps: 500 }, null).text).toBe('running late by 7.25 ms');
  });
});

describe('scope helpers', () => {
  it('niceTicks lands on round numbers', () => {
    expect(niceTicks(0, 5, 5)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(niceTicks(-0.13, 1.08, 5)).toEqual([0, 0.5, 1]);
    expect(niceTicks(2, 2)).toEqual([2]);
    expect(niceTicks(NaN, 1)).toEqual([]);
  });

  it('autoRange pads, ignores non-finite values, and never collapses', () => {
    expect(autoRange([0, 1])).toEqual([-0.08, 1.08]);
    expect(autoRange([NaN, Infinity])).toEqual([-1, 1]);
    expect(autoRange([3, 3])).toEqual([2.5, 3.5]);
  });

  it('quPath turns backslashes into forward slashes', () => {
    expect(quPath(' C:\\a\\b.qu ')).toBe('C:/a/b.qu');
  });
});

describe('record and compare', () => {
  const rec: LoopSpec = { ...sil, record: { enabled: true, path: 'C:\\traces\\run.csv' } };
  const cmp: LoopSpec = { ...sil, reference: { enabled: true, path: 'C:/traces/ref.npz', tol: { ...defaultTolerances(), y: { rtol: 1e-3, atol: 0.01 } } } };

  it('validates the trace paths and the tolerances', () => {
    expect(validateLoop(rec)).toBeNull();
    expect(validateLoop({ ...rec, record: { enabled: true, path: '' } })).toMatch(/Record: choose a trace file/);
    expect(validateLoop({ ...rec, record: { enabled: true, path: 'run.txt' } })).toMatch(/\.csv or \.npz/);
    // A disabled recording with no path is fine.
    expect(validateLoop({ ...sil, record: { enabled: false, path: '' } })).toBeNull();
    expect(validateLoop(cmp)).toBeNull();
    expect(validateLoop({ ...cmp, reference: { ...cmp.reference, path: '' } })).toMatch(/Reference/);
    expect(validateLoop({ ...cmp, reference: { ...cmp.reference, tol: { ...cmp.reference.tol, u: { rtol: -1, atol: 0 } } } })).toMatch(/Tolerance for u/);
    expect(toleranceProblem({ ...defaultTolerances(), e: { rtol: NaN, atol: 0 } })).toMatch(/for e/);
    expect(tracePathProblem('X', 'a.NPZ')).toBeNull();
  });

  it('records all five signals with trace_save after the stats', () => {
    const s = buildLoopScript(rec);
    expect(s).toContain('run_trace = {t = log_t, r = log_r, y = log_y, u = log_u, e = log_e}\ntrace_save("C:/traces/run.csv", run_trace)\nprint("@saved n={k}")');
    expect(s.indexOf('trace_save(')).toBeGreaterThan(s.indexOf('print("@stats'));
    expect(buildLoopScript(sil)).not.toContain('trace_save');
  });

  it('compares each signal with its own tolerance and draws the reference', () => {
    const s = buildLoopScript(cmp);
    expect(s).toContain('ref = trace_load("C:/traces/ref.npz")');
    expect(s).toContain('compare_signal("t", log_t, 0, 1e-9)');
    expect(s).toContain('compare_signal("y", log_y, 0.001, 0.01)');
    expect(s).toContain('d = trace_diff(a, b, rtol=rt, atol=at)');
    expect(s).toContain('emit_trace("@ref", ref)');
    // The same block on its own, for "Check again".
    expect(buildCompareScript(cmp)).toContain('compare_signal("e", log_e, 0.000001, 1e-9)');
    expect(buildCompareScript(cmp)).not.toContain('run_for');
  });

  it('replays a trace through trace_load, as @loop lines', () => {
    const s = buildReplayScript('C:\\t\\run.csv');
    expect(s).toContain('tr = trace_load("C:/t/run.csv")');
    expect(s).toContain('emit_trace("@loop", tr)');
    expect(s).toContain('print("@replayed n={n_rep}")');
  });

  it('parses diff, reference, saved and replayed lines', () => {
    expect(parseLoopLine('@diff name=y ok=0 max_abs=0.202878 max_rel=0.5 worst=708 first_bad=500 n_bad=4499 n=4999 rtol=1e-6 atol=1e-9')).toEqual({
      kind: 'diff',
      row: { name: 'y', ok: false, maxAbs: 0.202878, maxRel: 0.5, worst: 708, firstBad: 500, nBad: 4499, n: 4999, rtol: 1e-6, atol: 1e-9 },
    });
    expect(parseLoopLine('@diff name=t ok=1 max_abs=0 max_rel=0 worst=-1 first_bad=-1 n_bad=0 n=4999 rtol=0 atol=1e-9')).toMatchObject({ row: { name: 't', ok: true } });
    expect(parseLoopLine('@diff name=u missing=1')).toEqual({ kind: 'diff', row: { name: 'u', missing: true } });
    expect(parseLoopLine('@diff name=r len_run=4999 len_ref=2499 rtol=0 atol=1')).toEqual({ kind: 'diff', row: { name: 'r', lenRun: 4999, lenRef: 2499, rtol: 0, atol: 1 } });
    expect(parseLoopLine('@ref k=3 t=0.003 y=0.1')).toMatchObject({ kind: 'ref', sample: { k: 3, t: 0.003, values: { y: 0.1 } } });
    expect(parseLoopLine('@saved n=4999')).toEqual({ kind: 'saved', n: 4999 });
    expect(parseLoopLine('@replayed n=12')).toEqual({ kind: 'replayed', n: 12 });
  });

  it('passes only when every signal passed, and says which failed', () => {
    const ok = (name: string) => ({ name, ok: true, maxAbs: 0, n: 10, nBad: 0 });
    const all = TRACE_SIGNALS.map(ok);
    expect(compareVerdict(all)).toEqual({ pass: true, text: 'PASS: all 5 signals within tolerance' });
    expect(compareVerdict(all.slice(0, 4))).toBeNull();
    const bad = [...all.slice(0, 2), { name: 'y', ok: false, n: 10, nBad: 3, firstBad: 4 }, { name: 'u', missing: true }, ok('e')];
    expect(compareVerdict(bad)).toEqual({ pass: false, text: 'FAIL: y, u are outside tolerance' });
    expect(rowReason(bad[2])).toBe('3 of 10 samples out, first at sample 4');
    expect(rowReason(bad[3])).toBe('not in the reference');
    expect(rowReason({ name: 'r', lenRun: 5, lenRef: 6 })).toBe('length 5 vs reference 6');
    expect(rowReason(ok('t'))).toBe('identical');
    expect(rowReason({ name: 't', ok: true, maxAbs: 1e-12 })).toBe('within tolerance');
  });
});

describe('HIL I/O', () => {
  const tcp: LoopSpec = { ...hil, io: { ...hil.io, kind: 'tcp', host: '127.0.0.1', tcpPort: 5591 } };
  const serial: LoopSpec = { ...hil, io: { ...hil.io, kind: 'serial', port: 'COM3', baud: 115200, timeoutMs: 250 } };

  it('only binds a port in HIL', () => {
    expect(usesIo(tcp)).toBe(true);
    expect(usesIo({ ...tcp, mode: 'sil' })).toBe(false);
    expect(usesIo(hil)).toBe(false);
    expect(buildLoopScript({ ...tcp, mode: 'sil' })).not.toContain('tcp_connect');
  });

  it('validates the binding, and skips the plant source when hardware is the plant', () => {
    expect(validateLoop(tcp)).toBeNull();
    expect(validateLoop({ ...tcp, plant: { ...tcp.plant, kind: 'file', path: '' } })).toBeNull();
    expect(validateLoop({ ...tcp, io: { ...tcp.io, tcpPort: 70000 } })).toMatch(/1 to 65535/);
    expect(validateLoop({ ...tcp, io: { ...tcp.io, host: 'a b' } })).toMatch(/host/);
    expect(validateLoop(serial)).toBeNull();
    expect(ioProblem({ ...serial.io, port: '' })).toMatch(/serial port/);
    expect(ioProblem({ ...serial.io, port: 'COM3"' })).toMatch(/serial port/);
    expect(ioProblem({ ...serial.io, baud: 9600.5 })).toMatch(/baud/);
    expect(ioProblem({ ...serial.io, timeoutMs: 0 })).toMatch(/timeout/);
  });

  it('TCP: connects, sends u as a line, reads a framed line back, closes', () => {
    const s = buildLoopScript(tcp);
    expect(s).toContain('io = tcp_connect("127.0.0.1", 5591)');
    expect(s).toContain('tcp_send(io, "{u:.17g}\\n")');
    expect(s).toContain('while nl == none');
    expect(s).toContain('return to_float(trim(reply))');
    expect(s).toContain('tcp_close(io)');
    expect(s).not.toContain('lag_y');
    expect(s).toContain('plant on a real port');
  });

  it('serial: opens with the timeout, writes u, errors on a silent device', () => {
    const s = buildLoopScript(serial);
    expect(s).toContain('io = serial_open("COM3", 115200, timeout_ms=250)');
    expect(s).toContain('write(io, "{u:.17g}\\n")');
    expect(s).toContain('reply = read_line(io)');
    expect(s).toContain('error("no reply from COM3 within 250 ms")');
    expect(s).toContain('close(io)');
  });

  it('lists ports through serial_ports()', () => {
    expect(PORT_LIST_SCRIPT).toContain('serial_ports()');
    expect(parsePortList('@ports COM1,COM3\n')).toEqual(['COM1', 'COM3']);
    expect(parsePortList('@ports \n')).toEqual([]);
    expect(parsePortList('error')).toBeNull();
  });
});
