/** Pure helpers behind the Loop (SIL / HIL) panel: the one Qu script the
 *  panel generates from its choices, the checks that keep an invalid setup
 *  from reaching the engine, the reading of the `@`-tagged lines the script
 *  prints, and the plain-language verdict on a run. No DOM, no Tauri, so it
 *  is testable on its own (`loopDesign.test.ts`).
 *
 *  The contract a plant or controller script has to meet is deliberately
 *  two plain functions, the shape an embedded engineer already writes:
 *
 *    plant(u, dt)          advances the plant by dt under input u and returns
 *                          the measured output y. It is called once with
 *                          (0, 0) before the loop to read the initial output.
 *    controller(r, y, dt)  returns the actuator command u for setpoint r and
 *                          measurement y.
 *
 *  State lives in the script's own globals (`global name` inside the
 *  function), like `static` variables in a C controller -- which is also why
 *  a compiled C controller with `double controller(double, double, double)`
 *  drops into the same slot through `native_call`. */

export type LoopMode = 'sil' | 'hil';
export type SourceKind = 'example' | 'file' | 'editor' | 'clib';
export type SignalName = 't' | 'r' | 'y' | 'u' | 'e';
export type SetpointKind = 'step' | 'square' | 'sine';

/** Where a plant or controller comes from. `clib` is controller-only. */
export interface SourceSpec {
  kind: SourceKind;
  /** `example`: an id from PLANT_EXAMPLES / CONTROLLER_EXAMPLES. */
  example: string;
  /** `file`: a .qu file, imported. `clib`: the shared library. */
  path: string;
  /** `clib`: the exported C function, `double f(double r, double y, double dt)`. */
  func: string;
}

export interface SetpointSpec {
  kind: SetpointKind;
  amplitude: number;
  /** step: the time of the step (s). square/sine: the period (s). */
  time: number;
}

export interface LoopSpec {
  plant: SourceSpec;
  controller: SourceSpec;
  mode: LoopMode;
  /** Simulated seconds to run. */
  duration: number;
  /** Controller period (s). */
  dt: number;
  /** HIL real-time factor: 1 = real time. */
  pace: number;
  /** HIL: a step later than this many milliseconds counts as an overrun. */
  toleranceMs: number;
  setpoint: SetpointSpec;
  /** Signals drawn on the scope (all five are always logged). */
  watch: SignalName[];
  /** Write the run's five signals with trace_save. */
  record: { enabled: boolean; path: string };
  /** Compare the run against a saved trace with trace_diff, per signal. */
  reference: { enabled: boolean; path: string; tol: Record<SignalName, Tolerance> };
  /** HIL only: the plant is real hardware on a port. */
  io: IoSpec;
}

/** Binding the loop to a real port. Each step sends the controller output u
 *  as one text line and reads one text line back holding the measured y. */
export interface IoSpec {
  kind: 'none' | 'serial' | 'tcp';
  /** serial: OS device name, e.g. COM3 or /dev/ttyUSB0. */
  port: string;
  baud: number;
  /** serial: how long to wait for the reply line. */
  timeoutMs: number;
  host: string;
  tcpPort: number;
}

/** True when this run talks to hardware instead of the plant script. */
export const usesIo = (s: Pick<LoopSpec, 'mode' | 'io'>): boolean => s.mode === 'hil' && s.io.kind !== 'none';

export interface Tolerance {
  rtol: number;
  atol: number;
}

/** The editor buffers a source of kind `editor` reads, resolved by the caller. */
export interface ResolvedSources {
  plantEditor?: { name: string; code: string };
  controllerEditor?: { name: string; code: string };
}

export const SIGNALS: Array<{ id: SignalName; label: string; short: string; color: string }> = [
  { id: 'r', label: 'r  setpoint', short: 'setpoint', color: '#8a8a8a' },
  { id: 'y', label: 'y  measured output', short: 'output', color: '#2a78d6' },
  { id: 'u', label: 'u  controller output', short: 'control', color: '#e0a33a' },
  { id: 'e', label: 'e  error r - y', short: 'error', color: '#c0392f' },
];
/** Every logged signal, in trace order. */
export const TRACE_SIGNALS: SignalName[] = ['t', 'r', 'y', 'u', 'e'];

export const PLANT_EXAMPLES: Array<{ id: string; label: string; code: string }> = [
  {
    id: 'lag1',
    label: 'First-order lag (K = 2, tau = 0.5 s)',
    code: `# First-order lag: tau * dy/dt = K * u - y, with K = 2, tau = 0.5 s.
# Forward Euler; plant(u, 0) just reads the output.
lag_y = 0
function plant(u, dt)
    global lag_y
    lag_y = lag_y + dt * (2 * u - lag_y) / 0.5
    return lag_y
end function`,
  },
  {
    id: 'msd',
    label: 'Mass-spring-damper (m = 1, c = 0.8, k = 4)',
    code: `# Mass-spring-damper: m x'' + c x' + k x = u, with m = 1, c = 0.8, k = 4.
# Semi-implicit Euler; the measured output is the position x.
msd_x = 0
msd_v = 0
function plant(u, dt)
    global msd_x, msd_v
    msd_v = msd_v + dt * (u - 0.8 * msd_v - 4 * msd_x)
    msd_x = msd_x + dt * msd_v
    return msd_x
end function`,
  },
];

export const CONTROLLER_EXAMPLES: Array<{ id: string; label: string; code: string }> = [
  {
    id: 'pid',
    label: 'PID (Kp = 2, Ki = 4, Kd = 0.05, |u| <= 10)',
    code: `# PID with the derivative on the measurement (no kick on a setpoint
# step) and the output clamped to +/- 10.
pid_i = 0
pid_prev_y = 0
function controller(r, y, dt)
    global pid_i, pid_prev_y
    e = r - y
    pid_i = pid_i + e * dt
    d = 0
    if dt > 0
        d = -(y - pid_prev_y) / dt
    end if
    pid_prev_y = y
    u = 2 * e + 4 * pid_i + 0.05 * d
    return min(10, max(-10, u))
end function`,
  },
  {
    id: 'pi',
    label: 'PI (Kp = 1, Ki = 2)',
    code: `# PI controller, no output limit.
pi_i = 0
function controller(r, y, dt)
    global pi_i
    e = r - y
    pi_i = pi_i + e * dt
    return 1 * e + 2 * pi_i
end function`,
  },
];

/** A C controller that fits the `clib` slot, for "Copy C template". */
export const C_CONTROLLER_TEMPLATE = `/* controller.c -- build as a shared library, e.g.
 *   cl /LD /O2 controller.c          (MSVC, gives controller.dll)
 *   gcc -shared -O2 -fPIC controller.c -o libcontroller.so
 * State lives in statics, exactly like the Qu controller's globals. */
#ifdef _WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif

static double integ = 0.0, prev_y = 0.0;

EXPORT double controller(double r, double y, double dt)
{
    double e = r - y, d = 0.0, u;
    integ += e * dt;
    if (dt > 0.0) d = -(y - prev_y) / dt;
    prev_y = y;
    u = 2.0 * e + 4.0 * integ + 0.05 * d;
    return u > 10.0 ? 10.0 : (u < -10.0 ? -10.0 : u);
}
`;

export const DEFAULT_LOOP: LoopSpec = {
  plant: { kind: 'example', example: 'lag1', path: '', func: '' },
  controller: { kind: 'example', example: 'pid', path: '', func: 'controller' },
  mode: 'sil',
  duration: 5,
  dt: 0.001,
  pace: 1,
  toleranceMs: 1,
  setpoint: { kind: 'step', amplitude: 1, time: 0.5 },
  watch: ['r', 'y', 'u'],
  record: { enabled: false, path: '' },
  reference: { enabled: false, path: '', tol: defaultTolerances() },
  io: { kind: 'none', port: '', baud: 115200, timeoutMs: 1000, host: '127.0.0.1', tcpPort: 5555 },
};

/** Tight by default: a back-to-back test of the same arithmetic should
 *  agree to rounding. Time is compared absolutely. */
export function defaultTolerances(): Record<SignalName, Tolerance> {
  return {
    t: { rtol: 0, atol: 1e-9 },
    r: { rtol: 1e-6, atol: 1e-9 },
    y: { rtol: 1e-6, atol: 1e-9 },
    u: { rtol: 1e-6, atol: 1e-9 },
    e: { rtol: 1e-6, atol: 1e-9 },
  };
}

/** The two worked examples the panel offers out of the box. */
export const WORKED_EXAMPLES: Array<{ id: string; label: string; spec: LoopSpec }> = [
  { id: 'sil-pid', label: 'SIL: first-order plant + PID', spec: DEFAULT_LOOP },
  {
    id: 'hil-pid',
    label: 'HIL: the same loop, paced at real time',
    spec: { ...DEFAULT_LOOP, mode: 'hil', duration: 3, dt: 0.002 },
  },
];

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;
/** Paths go into a Qu string literal: no quote, no brace (interpolation). */
const BAD_PATH_CHARS = /["{}\n\r]/;

/** Forward slashes (Qu accepts them on Windows, and a backslash would start
 *  an escape inside the string literal). */
export const quPath = (p: string): string => p.trim().split('\\').join('/');

/** Longest loop the panel will log, in steps. */
export const MAX_STEPS = 2_000_000;

function sourceProblem(role: 'Plant' | 'Controller', s: SourceSpec, editorAvailable: boolean): string | null {
  switch (s.kind) {
    case 'example': {
      const list = role === 'Plant' ? PLANT_EXAMPLES : CONTROLLER_EXAMPLES;
      return list.some((x) => x.id === s.example) ? null : `${role}: pick an example.`;
    }
    case 'file':
      if (!s.path.trim()) return `${role}: choose a .qu file.`;
      if (BAD_PATH_CHARS.test(s.path)) return `${role}: the file path cannot contain " { or }.`;
      return null;
    case 'editor':
      return editorAvailable ? null : `${role}: there is no Qu file open in the Code tab.`;
    case 'clib':
      if (role === 'Plant') return 'A compiled library can only be the controller.';
      if (!s.path.trim()) return 'Controller: choose the compiled library (.dll / .so / .dylib).';
      if (BAD_PATH_CHARS.test(s.path)) return 'Controller: the library path cannot contain " { or }.';
      if (!IDENT.test(s.func)) return 'Controller: the C function name must be a plain identifier.';
      return null;
  }
}

/** `null` when the setup can run, else one sentence saying what to change. */
export function validateLoop(s: LoopSpec, editor: { plant: boolean; controller: boolean } = { plant: true, controller: true }): string | null {
  if (usesIo(s)) {
    const io = ioProblem(s.io);
    if (io) return io;
  } else {
    const p = sourceProblem('Plant', s.plant, editor.plant);
    if (p) return p;
  }
  const c = sourceProblem('Controller', s.controller, editor.controller);
  if (c) return c;
  if (!(s.duration > 0) || !Number.isFinite(s.duration)) return 'Duration must be above 0 s.';
  if (!(s.dt > 0) || !Number.isFinite(s.dt)) return 'Step must be above 0 s.';
  if (s.dt > s.duration) return 'Step must not be longer than the duration.';
  if (s.duration / s.dt > MAX_STEPS) return `That is more than ${MAX_STEPS.toLocaleString('en-US')} steps; lengthen the step or shorten the run.`;
  if (s.mode === 'hil') {
    if (!(s.pace > 0) || !Number.isFinite(s.pace)) return 'Real-time factor must be above 0 (1 = real time).';
    if (!(s.toleranceMs >= 0) || !Number.isFinite(s.toleranceMs)) return 'Lateness tolerance must be 0 ms or more.';
    if (s.dt / s.pace < 0.0001) return 'Paced steps shorter than 0.1 ms are not realistic for an interpreted loop; lengthen the step.';
  }
  if (!Number.isFinite(s.setpoint.amplitude)) return 'Setpoint amplitude must be a number.';
  if (s.setpoint.kind === 'step' ? !(s.setpoint.time >= 0) : !(s.setpoint.time > 0)) {
    return s.setpoint.kind === 'step' ? 'Step time must be 0 s or later.' : 'Setpoint period must be above 0 s.';
  }
  if (s.record.enabled) {
    const p = tracePathProblem('Record', s.record.path);
    if (p) return p;
  }
  if (s.reference.enabled) {
    const p = tracePathProblem('Reference', s.reference.path);
    if (p) return p;
    const t = toleranceProblem(s.reference.tol);
    if (t) return t;
  }
  return null;
}

/** `null` when the port binding is usable. */
export function ioProblem(io: IoSpec): string | null {
  if (io.kind === 'serial') {
    if (!/^[A-Za-z0-9_./\\:-]+$/.test(io.port.trim())) return 'I/O: pick a serial port (e.g. COM3 or /dev/ttyUSB0).';
    if (!Number.isInteger(io.baud) || io.baud <= 0) return 'I/O: the baud rate must be a whole number above 0.';
    if (!(io.timeoutMs > 0) || !Number.isFinite(io.timeoutMs)) return 'I/O: the reply timeout must be above 0 ms.';
  } else if (io.kind === 'tcp') {
    if (!/^[A-Za-z0-9.:_-]+$/.test(io.host.trim())) return 'I/O: enter a host name or IP address.';
    if (!Number.isInteger(io.tcpPort) || io.tcpPort < 1 || io.tcpPort > 65535) return 'I/O: the TCP port must be 1 to 65535.';
  }
  return null;
}

/** The plant slot when it is real hardware: a `plant(u, dt)` that sends u
 *  and returns the line that comes back. */
function ioPlantBlock(io: IoSpec): string {
  const recv = 'to_float(trim(reply))';
  if (io.kind === 'serial') {
    const port = io.port.trim().split('\\').join('/');
    return `# ---- plant: REAL HARDWARE on serial ${port} at ${io.baud} baud ----
# Protocol: each step sends u as one text line ("0.5\\n") and waits up to
# ${n(io.timeoutMs)} ms for one line back holding the measured y. The first call,
# plant(0, 0), reads the initial output.
io = serial_open("${port}", ${io.baud}, timeout_ms=${n(io.timeoutMs)})
function plant(u, dt)
    write(io, "{u:.17g}\\n")
    reply = read_line(io)
    if reply == none
        error("no reply from ${port} within ${n(io.timeoutMs)} ms")
    end if
    return ${recv}
end function`;
  }
  const host = io.host.trim();
  return `# ---- plant: REAL HARDWARE over TCP ${host}:${io.tcpPort} ----
# Protocol: each step sends u as one text line ("0.5\\n") and waits for one
# line back holding the measured y. The first call, plant(0, 0), reads the
# initial output.
io = tcp_connect("${host}", ${io.tcpPort})
io_buf = ""
function plant(u, dt)
    global io_buf
    tcp_send(io, "{u:.17g}\\n")
    nl = index_of(io_buf, "\\n")
    while nl == none
        chunk = tcp_recv(io)
        if chunk == none
            error("${host}:${io.tcpPort} closed the connection")
        end if
        io_buf = io_buf + chunk
        nl = index_of(io_buf, "\\n")
    end while
    reply = substr(io_buf, 0, nl)
    io_buf = substr(io_buf, nl + 1)
    return ${recv}
end function`;
}

/** Lists the serial ports the OS reports (`serial_ports()`), one tagged line. */
export const PORT_LIST_SCRIPT = 'sep = ","\nprint("@ports " + join(serial_ports(), sep))\n';

/** The port names from PORT_LIST_SCRIPT's output; `null` if it did not run. */
export function parsePortList(output: string): string[] | null {
  const line = output.split(/\r?\n/).find((l) => l.startsWith('@ports'));
  if (line == null) return null;
  return line.slice(6).trim().split(',').map((p) => p.trim()).filter(Boolean);
}

/** A trace file the engine can write and read: .csv or .npz. */
export function tracePathProblem(role: string, path: string): string | null {
  if (!path.trim()) return `${role}: choose a trace file (.csv or .npz).`;
  if (BAD_PATH_CHARS.test(path)) return `${role}: the path cannot contain " { or }.`;
  if (!/\.(csv|npz)$/i.test(path.trim())) return `${role}: a trace file ends in .csv or .npz.`;
  return null;
}

/** Every tolerance a finite number of 0 or more. */
export function toleranceProblem(tol: Record<SignalName, Tolerance>): string | null {
  for (const id of TRACE_SIGNALS) {
    const t = tol[id];
    if (!t || !(t.rtol >= 0) || !(t.atol >= 0) || !Number.isFinite(t.rtol) || !Number.isFinite(t.atol)) {
      return `Tolerance for ${id}: rtol and atol must be numbers of 0 or more.`;
    }
  }
  return null;
}

/** A number as Qu source: JS's shortest round-trip form. */
const n = (v: number): string => (Object.is(v, -0) ? '0' : String(v));

function setpointFn(sp: SetpointSpec): string {
  const a = n(sp.amplitude);
  const t = n(sp.time);
  switch (sp.kind) {
    case 'step':
      return `function setpoint(t)\n    if t >= ${t}\n        return ${a}\n    end if\n    return 0\nend function`;
    case 'square':
      return `function setpoint(t)\n    if (t mod ${t}) < ${n(sp.time / 2)}\n        return ${a}\n    end if\n    return 0\nend function`;
    case 'sine':
      return `function setpoint(t)\n    return ${a} * sin(2 * pi * t / ${t})\nend function`;
  }
}

function sourceBlock(role: 'plant' | 'controller', s: SourceSpec, editor?: { name: string; code: string }): string {
  switch (s.kind) {
    case 'example': {
      const list = role === 'plant' ? PLANT_EXAMPLES : CONTROLLER_EXAMPLES;
      const ex = list.find((x) => x.id === s.example)!;
      return `# ---- ${role}: ${ex.label} ----\n${ex.code}`;
    }
    case 'file':
      return `# ---- ${role}: ${quPath(s.path)} (must define ${role === 'plant' ? 'plant(u, dt)' : 'controller(r, y, dt)'}) ----\nimport "${quPath(s.path)}"`;
    case 'editor':
      return `# ---- ${role}: ${editor?.name ?? 'editor buffer'} (from the Code tab) ----\n${(editor?.code ?? '').replace(/\s+$/, '')}`;
    case 'clib':
      return `# ---- controller: compiled C, ${s.func}() in ${quPath(s.path)} ----
ctrl_lib = load_library("${quPath(s.path)}")
function controller(r, y, dt)
    return native_call(ctrl_lib, "${s.func}", "double(double, double, double)", r, y, dt)
end function`;
  }
}

/** How many steps between two scope samples: about 4000 points for a whole
 *  SIL run, about 100 lines a second of wall clock for a paced one. */
export function decimation(s: LoopSpec): number {
  const steps = Math.max(1, Math.floor(s.duration / s.dt + 1e-9));
  if (s.mode === 'sil') return Math.max(1, Math.ceil(steps / 4000));
  return Math.max(1, Math.ceil(0.01 / (s.dt / s.pace) - 1e-9));
}

/** The Qu program for this setup: definitions, the timer loop, the run, and
 *  the `@`-tagged lines the panel reads. Shown, copied and opened in the
 *  editor exactly as it is run. */
export function buildLoopScript(s: LoopSpec, src: ResolvedSources = {}): string {
  const hil = s.mode === 'hil';
  const every = decimation(s);
  const watch = SIGNALS.map((x) => x.id).filter((id) => s.watch.includes(id));
  const sigVar: Record<SignalName, string> = { t: 't', r: 'r', y: 'y', u: 'u', e: 'err' };
  const scopeFields = watch.map((id) => `${id}={${sigVar[id]}}`).join(' ');
  const timing = hil ? ' late={late} over={n_over} lmax={late_max}' : '';
  const header = hil
    ? `# Loop, HIL: paced against the wall clock (real-time factor ${n(s.pace)})${usesIo(s) ? ', plant on a real port' : ''}.`
    : '# Loop, SIL: virtual clock, as fast as the machine can go.';
  const lines: string[] = [
    header,
    '# Generated by Qu Studio (Loop tab). Lines starting with @ feed its scope.',
    `duration = ${n(s.duration)}`,
    `dt = ${n(s.dt)}`,
    '',
    usesIo(s) ? ioPlantBlock(s.io) : sourceBlock('plant', s.plant, src.plantEditor),
    '',
    sourceBlock('controller', s.controller, src.controllerEditor),
    '',
    '# ---- setpoint ----',
    setpointFn(s.setpoint),
    '',
    '# ---- the loop: every dt, sample y, compute u, advance the plant ----',
    'n_max = floor(duration / dt) + 2',
    'log_t = zeros(n_max)',
    'log_r = zeros(n_max)',
    'log_y = zeros(n_max)',
    'log_u = zeros(n_max)',
    'log_e = zeros(n_max)',
    'k = 0',
    'y = plant(0, 0)',
  ];
  if (hil) {
    lines.push(`pace = ${n(s.pace)}`, `tol_ms = ${n(s.toleranceMs)}`, 'late = 0', 'late_max = 0', 'n_over = 0');
  }
  lines.push(
    'every dt do',
    '    k = k + 1',
    '    t = k * dt',
  );
  if (hil) {
    lines.push(
      '    late = max(0, toc() * 1000 - t * 1000 / pace)',
      '    late_max = max(late_max, late)',
      '    if late > tol_ms',
      '        n_over = n_over + 1',
      '    end if',
    );
  }
  lines.push(
    '    r = setpoint(t)',
    '    err = r - y',
    '    u = controller(r, y, dt)',
    '    log_t[k - 1] = t',
    '    log_r[k - 1] = r',
    '    log_y[k - 1] = y',
    '    log_u[k - 1] = u',
    '    log_e[k - 1] = err',
    `    if k mod ${every} == 0`,
    `        print("@loop k={k} t={t}${timing} ${scopeFields}")`,
    '    end if',
    '    y = plant(u, dt)',
    'end',
    '',
  );
  if (hil) {
    lines.push('tic()', `run_for(duration, pace=pace, tolerance=${n(s.toleranceMs)} ms)`);
  } else {
    lines.push('run_for(duration)');
  }
  if (usesIo(s)) lines.push(s.io.kind === 'serial' ? 'close(io)' : 'tcp_close(io)');
  lines.push(
    'log_t = log_t[0:k - 1]',
    'log_r = log_r[0:k - 1]',
    'log_y = log_y[0:k - 1]',
    'log_u = log_u[0:k - 1]',
    'log_e = log_e[0:k - 1]',
    'st = run_for_stats()',
    'print("@stats steps={st.steps} overruns={st.overruns} max_ms={st.max_lateness_ms} mean_ms={st.mean_lateness_ms} wall_ms={st.wall_ms} paced={st.paced * 1}")',
  );
  if (s.record.enabled) {
    lines.push(
      '',
      '# ---- record: the five signals, bit-exact (trace_save) ----',
      'run_trace = {t = log_t, r = log_r, y = log_y, u = log_u, e = log_e}',
      `trace_save("${quPath(s.record.path)}", run_trace)`,
      'print("@saved n={k}")',
    );
  }
  if (s.reference.enabled) lines.push('', buildCompareScript(s).trimEnd());
  return lines.join('\n') + '\n';
}

/** Prints a trace's samples as tagged lines (`@loop` to replay it on the
 *  scope, `@ref` to draw it behind a run), about 4000 of them. */
const EMIT_TRACE = `function emit_trace(tag, tr)
    names = keys(tr)
    tt = tr["t"]
    n = length(tt)
    every_n = max(1, ceil(n / 4000))
    for i = 0 to n - 1 step every_n
        txt = "{tag} k={i + 1} t={tt[i]}"
        for name in ("r", "y", "u", "e")
            if contains(names, name)
                v = tr[name]
                txt = txt + " {name}={v[i]}"
            end if
        end for
        print(txt)
    end for
end function`;

/** The comparison against the reference: one trace_diff per signal, with
 *  that signal's tolerance, then the reference drawn behind the run. Part
 *  of the run's script, and also sent on its own to the same interpreter
 *  after a run ("Check again" with new tolerances: the run's log_* vectors
 *  are still there). */
export function buildCompareScript(s: LoopSpec): string {
  const tol = s.reference.tol;
  const logName: Record<SignalName, string> = { t: 'log_t', r: 'log_r', y: 'log_y', u: 'log_u', e: 'log_e' };
  return [
    '# ---- compare against the reference: one trace_diff per signal ----',
    `ref = trace_load("${quPath(s.reference.path)}")`,
    'ref_names = keys(ref)',
    'function compare_signal(name, a, rt, at)',
    '    if not contains(ref_names, name)',
    '        print("@diff name={name} missing=1")',
    '        return 0',
    '    end if',
    '    b = ref[name]',
    '    if length(a) != length(b)',
    '        print("@diff name={name} len_run={length(a)} len_ref={length(b)} rtol={rt} atol={at}")',
    '        return 0',
    '    end if',
    '    d = trace_diff(a, b, rtol=rt, atol=at)',
    '    print("@diff name={name} ok={d.ok * 1} max_abs={d.max_abs} max_rel={d.max_rel} worst={d.worst_index} first_bad={d.first_bad_index} n_bad={d.n_bad} n={d.n} rtol={rt} atol={at}")',
    '    return d.ok',
    'end function',
    ...TRACE_SIGNALS.map((id) => `compare_signal("${id}", ${logName[id]}, ${n(tol[id].rtol)}, ${n(tol[id].atol)})`),
    EMIT_TRACE,
    'if contains(ref_names, "t")',
    '    emit_trace("@ref", ref)',
    'end if',
    '',
  ].join('\n');
}

/** Draws a saved trace on the scope without running anything. */
export function buildReplayScript(path: string): string {
  return [
    '# Replay a saved trace on the Loop scope.',
    `tr = trace_load("${quPath(path)}")`,
    EMIT_TRACE,
    'emit_trace("@loop", tr)',
    'n_rep = length(tr["t"])',
    'print("@replayed n={n_rep}")',
    '',
  ].join('\n');
}

// ---------------------------------------------------------------- reading

export interface LoopSample {
  k: number;
  t: number;
  late?: number;
  over?: number;
  lmax?: number;
  values: Partial<Record<SignalName, number>>;
}

export interface LoopStats {
  steps: number;
  overruns: number;
  maxMs: number;
  meanMs: number;
  wallMs: number;
  paced: boolean;
}

/** One signal's trace_diff, as the script printed it. */
export interface DiffRow {
  name: string;
  /** Not in the reference at all. */
  missing?: boolean;
  /** Lengths differ (trace_diff would refuse): run vs reference. */
  lenRun?: number;
  lenRef?: number;
  ok?: boolean;
  maxAbs?: number;
  maxRel?: number;
  worst?: number;
  firstBad?: number;
  nBad?: number;
  n?: number;
  rtol?: number;
  atol?: number;
}

export type LoopLine =
  | { kind: 'sample'; sample: LoopSample }
  | { kind: 'ref'; sample: LoopSample }
  | { kind: 'stats'; stats: LoopStats }
  | { kind: 'diff'; row: DiffRow }
  | { kind: 'saved'; n: number }
  | { kind: 'replayed'; n: number }
  | { kind: 'text'; text: string };

/** `key=value` pairs of one tagged line. A value Qu prints as `NaN`/`inf`
 *  arrives as NaN/Infinity, so a diverging loop still reads. */
function pairs(rest: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const m of rest.matchAll(/([A-Za-z_]+)=(\S+)/g)) {
    const raw = m[2].toLowerCase();
    out[m[1]] = raw === 'nan' ? NaN : raw === 'inf' ? Infinity : raw === '-inf' ? -Infinity : Number(m[2]);
  }
  return out;
}

/** One printed line of the run, classified. Anything that is not one of the
 *  panel's tags (the controller's own prints, `run_for`'s summary) is text. */
export function parseLoopLine(line: string): LoopLine {
  const s = line.trim();
  const tag = s.slice(0, s.indexOf(' ') < 0 ? s.length : s.indexOf(' '));
  if (tag === '@loop' || tag === '@ref') {
    const p = pairs(s.slice(tag.length));
    if (Number.isFinite(p.k) && Number.isFinite(p.t)) {
      const values: Partial<Record<SignalName, number>> = {};
      for (const id of ['r', 'y', 'u', 'e'] as SignalName[]) if (id in p) values[id] = p[id];
      const sample = { k: p.k, t: p.t, late: p.late, over: p.over, lmax: p.lmax, values };
      return tag === '@loop' ? { kind: 'sample', sample } : { kind: 'ref', sample };
    }
  } else if (tag === '@diff') {
    const name = /\bname=(\S+)/.exec(s)?.[1];
    if (name) {
      const p = pairs(s.slice(5));
      const row: DiffRow = { name };
      if (p.missing === 1) row.missing = true;
      else if ('len_run' in p) Object.assign(row, { lenRun: p.len_run, lenRef: p.len_ref, rtol: p.rtol, atol: p.atol });
      else Object.assign(row, { ok: p.ok === 1, maxAbs: p.max_abs, maxRel: p.max_rel, worst: p.worst, firstBad: p.first_bad, nBad: p.n_bad, n: p.n, rtol: p.rtol, atol: p.atol });
      return { kind: 'diff', row };
    }
  } else if (tag === '@saved' || tag === '@replayed') {
    const p = pairs(s.slice(tag.length));
    if (Number.isFinite(p.n)) return tag === '@saved' ? { kind: 'saved', n: p.n } : { kind: 'replayed', n: p.n };
  } else if (tag === '@stats') {
    const p = pairs(s.slice(7));
    return {
      kind: 'stats',
      stats: { steps: p.steps, overruns: p.overruns, maxMs: p.max_ms, meanMs: p.mean_ms, wallMs: p.wall_ms, paced: p.paced === 1 },
    };
  }
  return { kind: 'text', text: line };
}

/** Splits streamed output into whole lines; the remainder waits for its
 *  newline. Returns the complete lines and the new remainder. */
export function splitChunk(pending: string, chunk: string): { lines: string[]; rest: string } {
  const parts = (pending + chunk).split(/\r?\n/);
  const rest = parts.pop() ?? '';
  return { lines: parts.filter((l) => l.length > 0), rest };
}

/** A row passes only when trace_diff said so; a missing signal or a length
 *  mismatch is a failure, not a skip. */
export const rowPasses = (r: DiffRow): boolean => r.ok === true;

/** The comparison's one-line result. `null` until every expected signal
 *  has a row. */
export function compareVerdict(rows: DiffRow[], expected: string[] = TRACE_SIGNALS): { pass: boolean; text: string } | null {
  if (expected.some((id) => !rows.some((r) => r.name === id))) return null;
  const bad = expected.filter((id) => !rowPasses(rows.find((r) => r.name === id)!));
  return bad.length === 0
    ? { pass: true, text: `PASS: all ${expected.length} signals within tolerance` }
    : { pass: false, text: `FAIL: ${bad.join(', ')} ${bad.length === 1 ? 'is' : 'are'} outside tolerance` };
}

/** Why a row failed, in a few words. */
export function rowReason(r: DiffRow): string {
  if (r.missing) return 'not in the reference';
  if (r.lenRun != null) return `length ${r.lenRun} vs reference ${r.lenRef}`;
  if (r.ok) return r.maxAbs === 0 ? 'identical' : 'within tolerance';
  return `${r.nBad} of ${r.n} samples out, first at sample ${r.firstBad}`;
}

// ---------------------------------------------------------------- scope

/** About `count` round tick values covering [lo, hi] (1, 2 or 5 times a
 *  power of ten apart). */
export function niceTicks(lo: number, hi: number, count = 5): number[] {
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return [];
  if (hi < lo) [lo, hi] = [hi, lo];
  if (hi === lo) return [lo];
  const raw = (hi - lo) / Math.max(1, count);
  const pow = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 5, 10].map((m) => m * pow).find((s) => s >= raw) ?? 10 * pow;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step - 1e-9) * step; v <= hi + step * 1e-9; v += step) {
    out.push(Math.abs(v) < step * 1e-9 ? 0 : Number(v.toPrecision(12)));
  }
  return out;
}

/** The y range of the scope: the visible values, padded 8 %, never empty. */
export function autoRange(values: Iterable<number>): [number, number] {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  if (lo === Infinity) return [-1, 1];
  if (hi - lo < 1e-12) {
    const pad = Math.max(Math.abs(hi) * 0.1, 0.5);
    return [lo - pad, hi + pad];
  }
  const pad = (hi - lo) * 0.08;
  return [lo - pad, hi + pad];
}

// ---------------------------------------------------------------- verdict

export type VerdictKind = 'idle' | 'ok' | 'late' | 'fast';
export interface Verdict {
  kind: VerdictKind;
  text: string;
  detail: string;
}

const ms = (v: number): string =>
  v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v >= 0.01 || v === 0 ? v.toFixed(2) : String(Number(v.toPrecision(2)));

/** The status card's plain-language line. Live (no stats yet) it reads the
 *  running counters of the last sample; at the end, `run_for_stats()`. */
export function verdict(
  spec: Pick<LoopSpec, 'mode' | 'duration' | 'toleranceMs'>,
  live: { simT: number; wallMs: number; over?: number; lmax?: number; steps?: number } | null,
  stats: LoopStats | null,
): Verdict {
  if (spec.mode === 'sil') {
    const wall = stats?.wallMs ?? live?.wallMs;
    const sim = stats ? spec.duration : live?.simT;
    if (wall == null || sim == null || !(sim > 0)) return { kind: 'idle', text: 'not run yet', detail: '' };
    const factor = wall > 0 ? (sim * 1000) / wall : Infinity;
    return {
      kind: 'fast',
      text: Number.isFinite(factor) ? `${factor >= 10 ? factor.toFixed(0) : factor.toFixed(1)}x faster than real time` : 'instant',
      detail: `${sim.toPrecision(3)} s simulated in ${ms(wall)} ms of wall clock (virtual clock, nothing waits)`,
    };
  }
  const over = stats ? stats.overruns : live?.over;
  const maxLate = stats ? stats.maxMs : live?.lmax;
  const steps = stats ? stats.steps : live?.steps;
  if (over == null || maxLate == null) return { kind: 'idle', text: 'not run yet', detail: '' };
  if (over === 0) {
    return {
      kind: 'ok',
      text: 'on time',
      detail: `no step later than ${n(spec.toleranceMs)} ms; worst ${ms(maxLate)} ms${stats ? `, mean ${ms(stats.meanMs)} ms` : ''}`,
    };
  }
  return {
    kind: 'late',
    text: `running late by ${ms(maxLate)} ms`,
    detail: `${over}${steps ? ` of ${steps}` : ''} steps started more than ${n(spec.toleranceMs)} ms after their deadline`,
  };
}
