/** The engine gate for the Loop tab's environments and plugins: skipped
 *  unless QU_BIN names a qu executable. Every case builds the script exactly
 *  as the panel does (`buildLoopScript`), runs it with the real binary
 *  (stdin ignored, a timeout), reads the run's own log vectors back through
 *  `--emit-data`, and compares them with truth worked out here: closed forms,
 *  or an independent RK4 integration written in this file. Nothing is
 *  compared with output copied from the code under test. */
import { describe, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DEFAULT_LOOP, buildLoopScript, environmentSpec, validateLoop, type LoopSpec, type ResolvedSources } from './loopDesign';
import { ENVIRONMENTS } from './loopEnvironments';
import { PLUGIN_LIBRARY, buildPluginCheckScript, newPlugin, parsePluginCheck, parsePluginLine, pluginProblem, type PluginInstance } from './loopPlugins';

const QU = process.env.QU_BIN;
/** The step most plugin cases use (s). */
const dt = 0.01;
const gate = QU ? describe : describe.skip;
const dir = QU ? mkdtempSync(join(tmpdir(), 'loop-gate-')) : '';
let seq = 0;
/** One row per case for the report (written when QU_GATE_TABLE names a file). */
const TABLE = process.env.QU_GATE_TABLE;
const row = (name: string, truth: string, measured: string) => {
  if (TABLE) appendFileSync(TABLE, `${name} | ${truth} | ${measured}\n`);
};
const f = (v: number, d = 6) => String(Number(v.toPrecision(d)));

interface Run {
  ok: boolean;
  stdout: string;
  stderr: string;
  vars: Record<string, number[]>;
}

function runScript(script: string, timeout = 120_000): Run {
  const id = ++seq;
  const file = join(dir, `s${id}.qu`);
  const out = join(dir, `s${id}.json`);
  writeFileSync(file, script);
  const r = spawnSync(QU!, ['run', file, '--emit-data', out], { stdio: ['ignore', 'pipe', 'pipe'], timeout, encoding: 'utf8' });
  const vars: Record<string, number[]> = {};
  try {
    for (const v of JSON.parse(readFileSync(out, 'utf8'))) vars[v.name] = (v.data as Array<number | null>).map((x) => (x == null ? NaN : x));
  } catch {
    /* a failed run writes no data */
  }
  return { ok: r.status === 0 && !/runtime error|parse error/.test(r.stderr ?? ''), stdout: r.stdout ?? '', stderr: r.stderr ?? '', vars };
}

const base = (patch: Partial<LoopSpec>): LoopSpec => ({ ...DEFAULT_LOOP, ...patch });

/** A plant/controller pair as editor buffers: exact, tiny fixtures. */
function fixture(plant: string, controller: string): { spec: Pick<LoopSpec, 'plant' | 'controller'>; src: ResolvedSources } {
  return {
    spec: { plant: { kind: 'editor', example: '', path: '', func: '' }, controller: { kind: 'editor', example: '', path: '', func: '' } },
    src: { plantEditor: { name: 'plant.qu', code: plant }, controllerEditor: { name: 'controller.qu', code: controller } },
  };
}

function runFixture(plant: string, controller: string, patch: Partial<LoopSpec>, tail = ''): Run & { script: string } {
  const fx = fixture(plant, controller);
  const script = buildLoopScript(base({ ...fx.spec, ...patch }), fx.src) + tail;
  return { ...runScript(script), script };
}

const ECHO = 'function plant(u, dt)\n    return u\nend function';
/** y_true at 0-based step i is exactly i. */
const COUNTER = 'pc_k = 0\nfunction plant(u, dt)\n    global pc_k\n    if dt > 0\n        pc_k = pc_k + 1\n    end if\n    return pc_k\nend function';
const CONST0 = 'function plant(u, dt)\n    return 0\nend function';
/** u_k = a sin(0.05 k) + b, deterministic and known to this file. */
const driver = (a: number, b = 0) => `drv_k = 0\nfunction controller(r, y, dt)\n    global drv_k\n    drv_k = drv_k + 1\n    return ${a} * sin(0.05 * drv_k) + ${b}\nend function`;
const asR = 'function controller(r, y, dt)\n    return r\nend function';

const withPlugins = (plugins: PluginInstance[]): Partial<LoopSpec> => ({ plugins });
const plug = (def: string, params: Record<string, number>): PluginInstance => {
  const p = newPlugin(def);
  p.params = { ...p.params, ...params };
  return p;
};
const maxAbs = (a: number[]) => a.reduce((m, v) => Math.max(m, Math.abs(v)), 0);
const maxErr = (a: number[], b: number[]) => a.reduce((m, v, i) => Math.max(m, Math.abs(v - b[i])), 0);
const mean = (a: number[]) => a.reduce((s, v) => s + v, 0) / a.length;
const std = (a: number[]) => {
  const m = mean(a);
  return Math.sqrt(a.reduce((s, v) => s + (v - m) ** 2, 0) / (a.length - 1));
};

// ------------------------------------------------------- independent truth: RK4 with a held input

type Mat = number[][];
interface Lin {
  id: string;
  A: Mat;
  B: number[];
  out: number;
  x0: number[];
  /** constant subtracted from u (gravity) */
  uOffset?: number;
  floor?: boolean;
  amp: number;
  bias?: number;
  seconds: number;
}

const matvec = (A: Mat, x: number[]) => A.map((r) => r.reduce((s, v, j) => s + v * x[j], 0));
function rk4Hold(A: Mat, B: number[], x: number[], u: number, dt: number, sub = 40): number[] {
  const h = dt / sub;
  const fx = (z: number[]) => matvec(A, z).map((v, i) => v + B[i] * u);
  let z = x.slice();
  for (let s = 0; s < sub; s++) {
    const k1 = fx(z);
    const k2 = fx(z.map((v, i) => v + (h / 2) * k1[i]));
    const k3 = fx(z.map((v, i) => v + (h / 2) * k2[i]));
    const k4 = fx(z.map((v, i) => v + h * k3[i]));
    z = z.map((v, i) => v + (h / 6) * (k1[i] + 2 * k2[i] + 2 * k3[i] + k4[i]));
  }
  return z;
}

const motorA = (R: number, L: number, K: number, J: number, b: number): Mat => [[-R / L, -K / L], [K / J, -b / J]];
const pend = (() => {
  const M = 0.5, m = 0.2, b = 0.1, I = 0.006, g = 9.8, l = 0.3;
  const p = I * (M + m) + M * m * l * l;
  return {
    A: [[0, 1, 0, 0], [0, -(I + m * l * l) * b / p, (m * m * g * l * l) / p, 0], [0, 0, 0, 1], [0, -(m * l * b) / p, (m * g * l * (M + m)) / p, 0]] as Mat,
    B: [0, (I + m * l * l) / p, 0, (m * l) / p],
  };
})();

const LINEAR: Lin[] = [
  { id: 'motor-speed', A: motorA(1, 0.01, 0.1, 0.02, 0.01), B: [100, 0], out: 1, x0: [0, 0], amp: 6, seconds: 3 },
  {
    id: 'motor-position',
    A: [...motorA(1, 0.01, 0.1, 0.02, 0.01).map((r) => [...r, 0]), [0, 1, 0]] as Mat,
    B: [100, 0, 0],
    out: 2,
    x0: [0, 0, 0],
    amp: 6,
    seconds: 3,
  },
  { id: 'mass-spring-damper', A: [[0, 1], [-8 / 2, -1.2 / 2]], B: [0, 0.5], out: 0, x0: [0, 0], amp: 20, seconds: 6 },
  { id: 'pendulum', A: pend.A, B: pend.B, out: 0, x0: [0, 0, 0.05, 0], amp: 0.3, seconds: 2 },
  { id: 'cruise', A: [[-50 / 1000]], B: [1 / 1000], out: 0, x0: [0], amp: 2000, bias: 1000, seconds: 30 },
  { id: 'quad-altitude', A: [[0, 1], [0, -0.3 / 1.2]], B: [0, 1 / 1.2], out: 0, x0: [0, 0], uOffset: 1.2 * 9.81, floor: true, amp: 4, bias: 12.5, seconds: 6 },
  { id: 'maglev', A: [[0, 1, 0], [(2 * 9.81) / 0.01, 0, -(2 * 9.81) / 0.5], [0, 0, -2 / 0.05]], B: [0, 0, 20], out: 0, x0: [0.0005, 0, 0], amp: 0.5, seconds: 0.3 },
];

// ------------------------------------------------------- the cases

gate('environments: every one runs, behaves physically and meets its stated tolerance', () => {
  for (const env of ENVIRONMENTS) {
    it(`${env.id}: closed loop settles within ${env.expect.band * 100}% by ${env.expect.settleBy} s`, () => {
      const spec = environmentSpec(env.id)!;
      expect(validateLoop(spec)).toBeNull();
      const run = runScript(buildLoopScript(spec));
      expect(run.ok, run.stderr).toBe(true);
      const { log_t: t, log_y: y, log_r: r, log_u: u } = run.vars;
      expect(t.length).toBe(Math.floor(env.duration / env.dt + 1e-9));
      expect(y.every(Number.isFinite)).toBe(true);
      const from = env.setpoint.time + env.expect.settleBy;
      const target = Math.abs(env.setpoint.amplitude);
      let worst = 0;
      for (let i = 0; i < t.length; i++) if (t[i] >= from) worst = Math.max(worst, Math.abs(y[i] - r[i]));
      expect(worst).toBeLessThanOrEqual(env.expect.band * target);
      expect(maxAbs(u)).toBeGreaterThan(0);
      row(`env ${env.id}`, `|y-r| <= ${f(env.expect.band * target)} after ${f(from)} s`, `worst ${f(worst)}`);
    });
  }

  for (const env of ENVIRONMENTS.filter((e) => e.expect.unstable)) {
    it(`${env.id}: unstable without the controller, stabilised with it`, () => {
      const spec = environmentSpec(env.id)!;
      const closed = runScript(buildLoopScript(spec));
      expect(closed.ok, closed.stderr).toBe(true);
      const fx = fixture('', 'function controller(r, y, dt)\n    return 0\nend function');
      const open = runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
      expect(open.ok, open.stderr).toBe(true);
      const closedMax = maxAbs(closed.vars.log_y);
      const openMax = maxAbs(open.vars.log_y);
      // Open loop it runs away by orders of magnitude; closed loop it never leaves the same scale as the setpoint.
      expect(openMax).toBeGreaterThan(100 * Math.abs(env.setpoint.amplitude));
      expect(closedMax).toBeLessThan(5 * Math.abs(env.setpoint.amplitude));
      row(`env ${env.id} unstable`, 'open loop diverges, closed bounded', `open max |y| ${f(openMax)}, closed ${f(closedMax)}`);
    });
  }
});

gate('environments: the plant physics against an independent integration', () => {
  for (const lin of LINEAR) {
    it(`${lin.id}: the plant under a sine input matches RK4 written in this file`, () => {
      const env = ENVIRONMENTS.find((e) => e.id === lin.id)!;
      const spec = base({ ...environmentSpec(lin.id)!, duration: lin.seconds });
      const fx = fixture('', driver(lin.amp, lin.bias ?? 0));
      const run = runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
      expect(run.ok, run.stderr).toBe(true);
      const { log_y: y, log_u: u } = run.vars;
      let x = lin.x0.slice();
      const truth: number[] = [];
      for (let i = 0; i < u.length; i++) {
        truth.push(x[lin.out]);
        x = rk4Hold(lin.A, lin.B, x, u[i] - (lin.uOffset ?? 0), env.dt);
        if (lin.floor && x[0] < 0) x = x.map(() => 0);
      }
      const err = maxErr(y, truth);
      const scale = Math.max(maxAbs(truth), 1e-12);
      expect(err / scale).toBeLessThan(1e-7);
      row(`plant ${lin.id}`, 'RK4 (this file), same input', `max |dy| ${f(err)} (${f(err / scale)} of range)`);
    });
  }

  const DC: Array<{ id: string; hold: number; seconds: number; steady: number; tol: number; what: string }> = [
    { id: 'motor-speed', hold: 1, seconds: 12, steady: 0.1 / (0.01 * 1 + 0.1 * 0.1), tol: 1e-3, what: 'Kt/(bR + Kt^2) = 5 rad/s per volt' },
    { id: 'cruise', hold: 500, seconds: 200, steady: 500 / 50, tol: 1e-3, what: 'F/b = 10 m/s' },
    { id: 'mass-spring-damper', hold: 8, seconds: 60, steady: 8 / 8, tol: 1e-3, what: 'F/k = 1 m' },
    { id: 'two-tank', hold: 0.02, seconds: 300, steady: (0.02 / 0.02) ** 2, tol: 1e-3, what: 'h2 = (q/c)^2 = 1 m' },
    { id: 'room-thermal', hold: 1500, seconds: 4000, steady: 5 + 1500 / 100, tol: 5e-3, what: 'T = T_out + P/UA = 20 C' },
  ];
  for (const c of DC) {
    it(`${c.id}: held at a constant input it settles at ${c.what}`, () => {
      const spec = base({ ...environmentSpec(c.id)!, duration: c.seconds });
      const fx = fixture('', `function controller(r, y, dt)\n    return ${c.hold}\nend function`);
      const run = runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
      expect(run.ok, run.stderr).toBe(true);
      const y = run.vars.log_y;
      expect(Math.abs(y[y.length - 1] - c.steady)).toBeLessThan(c.tol);
      row(`dc ${c.id}`, `${f(c.steady)} (${c.what})`, f(y[y.length - 1]));
    });
  }

  it('room-thermal: the heater delay is 30 s, exactly (T does not move before it)', () => {
    const spec = base({ ...environmentSpec('room-thermal')!, duration: 60 });
    const fx = fixture('', 'function controller(r, y, dt)\n    return 1500\nend function');
    const run = runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
    expect(run.ok, run.stderr).toBe(true);
    const { log_t: t, log_y: y } = run.vars;
    // dt = 1: step k is at t = k; the first input (step 1) reaches the room at step 31, and shows in y at step 32.
    for (let i = 0; i < t.length; i++) if (t[i] <= 31) expect(y[i]).toBe(5);
    expect(y[t.length - 1]).toBeGreaterThan(5.1);
    row('room delay', 'T = 5 exactly until t = 31 s', `y(31) = ${y[30]}, y(60) = ${f(y[59])}`);
  });

  it('quad-altitude: below hover thrust it stays on the ground; above it climbs at the drag terminal speed', () => {
    const spec = base({ ...environmentSpec('quad-altitude')!, duration: 30 });
    const mk = (u: number) => {
      const fx = fixture('', `function controller(r, y, dt)\n    return ${u}\nend function`);
      return runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
    };
    const low = mk(5);
    expect(low.ok, low.stderr).toBe(true);
    expect(maxAbs(low.vars.log_y)).toBe(0);
    const high = mk(1.2 * 9.81 + 3);
    expect(high.ok, high.stderr).toBe(true);
    const { log_t: t, log_y: z } = high.vars;
    const i1 = t.findIndex((v) => v >= 20);
    const i2 = t.length - 1;
    const slope = (z[i2] - z[i1]) / (t[i2] - t[i1]);
    // m v' = 3 - c v  ->  v_inf = 3 / 0.3 = 10 m/s.
    expect(Math.abs(slope - 10)).toBeLessThan(0.05);
    row('quad hover', 'z = 0 below hover; slope 3/c = 10 m/s above', `max z ${maxAbs(low.vars.log_y)}, slope ${f(slope)}`);
  });

  it('maglev: the open-loop pole is +sqrt(2g/x0) = 44.3 rad/s (growth rate of the gap)', () => {
    const spec = base({ ...environmentSpec('maglev')!, duration: 0.5 });
    const fx = fixture('', 'function controller(r, y, dt)\n    return 0\nend function');
    const run = runScript(buildLoopScript({ ...spec, controller: fx.spec.controller }, { controllerEditor: fx.src.controllerEditor }));
    expect(run.ok, run.stderr).toBe(true);
    const { log_t: t, log_y: y } = run.vars;
    const a = t.findIndex((v) => v >= 0.3);
    const b = t.findIndex((v) => v >= 0.5 - 1e-9);
    const rate = Math.log(y[b] / y[a]) / (t[b] - t[a]);
    const truth = Math.sqrt((2 * 9.81) / 0.01);
    expect(Math.abs(rate - truth) / truth).toBeLessThan(0.01);
    row('maglev pole', `${f(truth)} 1/s`, f(rate));
  });
});

gate('plugins: each against a closed form', () => {

  it('actuator saturation clips exactly at the limits', () => {
    const run = runFixture(ECHO, 'function controller(r, y, dt)\n    return 3 * r\nend function', {
      duration: 6, dt,
      setpoint: { kind: 'sine', amplitude: 10, time: 3 },
      ...withPlugins([plug('actuator-saturation', { lo: -4, hi: 6 })]),
    });
    expect(run.ok, run.stderr).toBe(true);
    const { log_u: u, log_y: y } = run.vars;
    // The plant echoes the input it gets: y[i + 1] is what the actuator let through at step i.
    const truth = u.slice(0, -1).map((v) => Math.min(6, Math.max(-4, v)));
    expect(maxErr(y.slice(1), truth)).toBeLessThan(1e-12);
    expect(Math.max(...y)).toBe(6);
    expect(Math.min(...y)).toBe(-4);
    row('saturation', 'y = clip(u, -4, 6) exactly', `max |dy| ${maxErr(y.slice(1), truth)}, range [${Math.min(...y)}, ${Math.max(...y)}]`);
  });

  it('rate limit moves at most rate*dt per step, and ramps 0 -> 10 in exactly 50 steps', () => {
    const run = runFixture(ECHO, 'function controller(r, y, dt)\n    return 10\nend function', { duration: 2, dt, setpoint: { kind: 'step', amplitude: 1, time: 0 }, ...withPlugins([plug('actuator-rate', { rate: 20 })]) });
    expect(run.ok, run.stderr).toBe(true);
    const y = run.vars.log_y;
    for (let i = 1; i < y.length; i++) expect(Math.abs(y[i] - y[i - 1])).toBeLessThanOrEqual(20 * dt + 1e-12);
    // u'[i] = min(10, 0.2 (i + 1)); y[i + 1] = u'[i].
    for (let i = 0; i < 60; i++) expect(Math.abs(y[i + 1] - Math.min(10, 0.2 * (i + 1)))).toBeLessThan(1e-9);
    expect(y.findIndex((v) => v >= 10 - 1e-9)).toBe(50);
    row('rate limit', '0.2 per step, 10 reached at sample 50', `first y >= 10 at sample ${y.findIndex((v) => v >= 10 - 1e-9)}`);
  });

  it('dead zone: 0 inside +-w, shrunk by w outside', () => {
    const run = runFixture(ECHO, 'function controller(r, y, dt)\n    return r\nend function', { duration: 4, dt, setpoint: { kind: 'sine', amplitude: 2, time: 2 }, ...withPlugins([plug('actuator-deadzone', { width: 0.5 })]) });
    expect(run.ok, run.stderr).toBe(true);
    const { log_u: u, log_y: y } = run.vars;
    const truth = u.slice(0, -1).map((v) => (Math.abs(v) <= 0.5 ? 0 : v - Math.sign(v) * 0.5));
    expect(maxErr(y.slice(1), truth)).toBeLessThan(1e-12);
    expect(truth.some((v) => v === 0)).toBe(true);
    row('dead zone', 'u - sign(u) w outside, 0 inside', `max |dy| ${maxErr(y.slice(1), truth)}`);
  });

  for (const bits of [3, 8]) {
    it(`quantiser (${bits} bits over [-10, 10]): every output lies on the ADC grid, within half an LSB of the truth`, () => {
      const run = runFixture(ECHO, driver(9.3), { duration: 8, dt, ...withPlugins([plug('sensor-quantise', { bits, lo: -10, hi: 10 })]) });
      expect(run.ok, run.stderr).toBe(true);
      const { log_y: y, log_u: u } = run.vars;
      const lsb = 20 / (2 ** bits - 1);
      let offGrid = 0;
      for (const v of y) offGrid = Math.max(offGrid, Math.abs((v + 10) / lsb - Math.round((v + 10) / lsb)));
      expect(offGrid).toBeLessThan(1e-9);
      // The sensor sees the echoed u of the previous step.
      let worst = 0;
      for (let i = 1; i < y.length; i++) worst = Math.max(worst, Math.abs(y[i] - u[i - 1]));
      expect(worst).toBeLessThanOrEqual(lsb / 2 + 1e-9);
      expect(new Set(y.map((v) => v.toFixed(9))).size).toBeGreaterThan(2);
      row(`quantiser ${bits} bit`, `grid step ${f(lsb)}, error <= lsb/2`, `off-grid ${f(offGrid, 3)}, worst error ${f(worst)}`);
    });
  }

  for (const n of [1, 3, 7]) {
    it(`sensor delay of ${n} samples shifts the trace by exactly ${n}`, () => {
      const run = runFixture(COUNTER, asR, { duration: 1, dt, ...withPlugins([plug('sensor-delay', { n })]) });
      expect(run.ok, run.stderr).toBe(true);
      const y = run.vars.log_y;
      // The counter plant reads i at step i; delayed, step i shows max(0, i - n).
      const truth = y.map((_, i) => Math.max(0, i - n));
      expect(maxErr(y, truth)).toBe(0);
      row(`delay n=${n}`, 'y[i] = max(0, i - n)', `max |dy| ${maxErr(y, truth)}`);
    });
  }

  it('two delays in one list add up (names do not clash)', () => {
    const run = runFixture(COUNTER, asR, { duration: 1, dt, ...withPlugins([plug('sensor-delay', { n: 2 }), plug('sensor-delay', { n: 3 })]) });
    expect(run.ok, run.stderr).toBe(true);
    const y = run.vars.log_y;
    expect(maxErr(y, y.map((_, i) => Math.max(0, i - 5)))).toBe(0);
    row('two delays', 'shift 2 + 3 = 5', 'exact');
  });

  it('sensor noise: same seed reproduces bit for bit, another seed differs, and the std is sigma', () => {
    const go = (seed: number) => runFixture(CONST0, asR, { duration: 200, dt, setpoint: { kind: 'step', amplitude: 0, time: 0 }, ...withPlugins([plug('sensor-noise', { sigma: 0.25, seed })]) });
    const a = go(7);
    const b = go(7);
    const c = go(8);
    for (const r of [a, b, c]) expect(r.ok, r.stderr).toBe(true);
    expect(a.vars.log_y).toEqual(b.vars.log_y);
    expect(a.vars.log_y).not.toEqual(c.vars.log_y);
    const y = a.vars.log_y;
    expect(y.length).toBe(20000);
    const s = std(y);
    expect(Math.abs(s - 0.25) / 0.25).toBeLessThan(0.03);
    expect(Math.abs(mean(y))).toBeLessThan(4 * 0.25 / Math.sqrt(y.length));
    row('noise', 'std 0.25, mean 0, same seed identical', `std ${f(s)}, mean ${f(mean(y))}`);
  });

  // P-only loop around the existing first-order example (K = 2, tau = 0.5), so every number is a closed form.
  const lagLoop = (plugins: PluginInstance[], extra: Partial<LoopSpec> = {}) =>
    runFixture(
      'lag_y = 0\nfunction plant(u, dt)\n    global lag_y\n    lag_y = lag_y + dt * (2 * u - lag_y) / 0.5\n    return lag_y\nend function',
      'function controller(r, y, dt)\n    return 4 * (r - y)\nend function',
      { duration: 12, dt: 0.001, setpoint: { kind: 'step', amplitude: 1, time: 0 }, ...withPlugins(plugins), ...extra },
    );

  it('step load disturbance moves the steady state by K d / (1 + K Kp)', () => {
    const none = lagLoop([]);
    const dist = lagLoop([plug('load-step', { amp: 0.5, at: 1 })]);
    for (const r of [none, dist]) expect(r.ok, r.stderr).toBe(true);
    const a = none.vars.log_y;
    const b = dist.vars.log_y;
    const ss0 = a[a.length - 1];
    const ss1 = b[b.length - 1];
    expect(ss0).toBeCloseTo((2 * 4) / (1 + 2 * 4), 3);
    const expected = (2 * 0.5) / (1 + 2 * 4);
    expect(Math.abs(ss1 - ss0 - expected)).toBeLessThan(1e-4);
    row('step load', `shift K d/(1+K Kp) = ${f(expected)}`, f(ss1 - ss0));
  });

  it('pulse load: the output comes back to the undisturbed value after the pulse', () => {
    const none = lagLoop([]);
    const pulse = lagLoop([plug('load-pulse', { amp: 1, at: 2, width: 0.5 })]);
    expect(pulse.ok, pulse.stderr).toBe(true);
    const a = none.vars.log_y;
    const b = pulse.vars.log_y;
    const during = b[Math.round(2.4 / 0.001)] - a[Math.round(2.4 / 0.001)];
    // Closed-loop time constant tau / (1 + K Kp) = 1/18 s; by 2.4 s the pulse is fully felt: K d / (1 + K Kp).
    expect(Math.abs(during - (2 * 1) / 9)).toBeLessThan(2e-3);
    expect(Math.abs(b[b.length - 1] - a[a.length - 1])).toBeLessThan(1e-6);
    row('pulse load', `during ${f(2 / 9)}, after 0`, `during ${f(during)}, after ${f(b[b.length - 1] - a[a.length - 1], 3)}`);
  });

  it('sine load: the loop output oscillates with amplitude K d / sqrt((tau w)^2 + (1 + K Kp)^2)', () => {
    const period = 0.5;
    const w = (2 * Math.PI) / period;
    const none = lagLoop([]);
    const sine = lagLoop([plug('load-sine', { amp: 1, period })]);
    expect(sine.ok, sine.stderr).toBe(true);
    const t = sine.vars.log_t;
    const diff = sine.vars.log_y.map((v, i) => v - none.vars.log_y[i]);
    const tail = diff.filter((_, i) => t[i] > 8);
    const amp = (Math.max(...tail) - Math.min(...tail)) / 2;
    const truth = 2 / Math.sqrt((0.5 * w) ** 2 / 1 ** 2 + 9 ** 2) / 1;
    // tau dy/dt = 2 (Kp (r - y) + d) - y  ->  (tau s + 1 + K Kp) y = K d; tau = 0.5, K Kp = 8.
    expect(Math.abs(amp - truth) / truth).toBeLessThan(0.01);
    row('sine load', `amplitude ${f(truth)}`, f(amp));
  });

  it('reference low-pass follows 1 - exp(-t/tau) after a step', () => {
    const tau = 0.25;
    const run = runFixture(CONST0, asR, { duration: 3, dt, setpoint: { kind: 'step', amplitude: 1, time: 0.5 }, ...withPlugins([plug('ref-lowpass', { tau })]) });
    expect(run.ok, run.stderr).toBe(true);
    const { log_t: t, log_r: r } = run.vars;
    let worst = 0;
    t.forEach((tt, i) => {
      // The first sample with setpoint 1 is at t = 0.5 (update number 1).
      const m = tt >= 0.5 - 1e-9 ? Math.round((tt - 0.5) / dt) + 1 : 0;
      worst = Math.max(worst, Math.abs(r[i] - (1 - Math.exp((-m * dt) / tau))));
    });
    expect(worst).toBeLessThan(1e-9);
    row('reference low-pass', "r' = 1 - exp(-t/tau)", `max |dr| ${f(worst, 3)}`);
  });

  it('reference rate limit ramps at exactly rate per second, then holds the setpoint', () => {
    const run = runFixture(CONST0, asR, { duration: 4, dt, setpoint: { kind: 'step', amplitude: 1, time: 0 }, ...withPlugins([plug('ref-rate', { rate: 0.5 })]) });
    expect(run.ok, run.stderr).toBe(true);
    const { log_t: t, log_r: r } = run.vars;
    let worst = 0;
    t.forEach((tt, i) => (worst = Math.max(worst, Math.abs(r[i] - Math.min(1, 0.5 * tt)))));
    expect(worst).toBeLessThan(1e-9);
    row('reference rate limit', 'r = min(1, 0.5 t)', `max |dr| ${f(worst, 3)}`);
  });

  it('metrics recorder agrees with overshoot, settling time and IAE computed from the log', () => {
    const run = runFixture(
      'lag_y = 0\nfunction plant(u, dt)\n    global lag_y\n    lag_y = lag_y + dt * (2 * u - lag_y) / 0.5\n    return lag_y\nend function',
      'ctl_i = 0\nfunction controller(r, y, dt)\n    global ctl_i\n    ctl_i = ctl_i + (r - y) * dt\n    return 0.5 * (r - y) + 4 * ctl_i\nend function',
      { duration: 8, dt: 0.001, setpoint: { kind: 'step', amplitude: 1, time: 1 }, ...withPlugins([plug('metrics', { band: 0.02 })]) },
    );
    expect(run.ok, run.stderr).toBe(true);
    const line = run.stdout.split(/\r?\n/).map(parsePluginLine).find((x) => x && x.name === 'metrics');
    expect(line).toBeTruthy();
    const { log_t: t, log_y: y, log_e: e } = run.vars;
    let ymax = -Infinity;
    let lastOut = 0;
    for (let i = 0; i < t.length; i++) {
      if (t[i] >= 1) ymax = Math.max(ymax, y[i]);
      if (t[i] >= 1 && Math.abs(e[i]) > 0.02) lastOut = t[i];
    }
    const iae = e.reduce((s, v) => s + Math.abs(v) * 0.001, 0);
    expect(Math.abs(line!.values.overshoot_pct - 100 * (ymax - 1))).toBeLessThan(1e-6);
    expect(Math.abs(line!.values.settle_s - (lastOut - 1))).toBeLessThan(1e-9);
    expect(Math.abs(line!.values.iae - iae)).toBeLessThan(1e-9);
    // PI on a first-order lag with zeta = 0.5: about 16 % overshoot, settled well before the end.
    expect(line!.values.overshoot_pct).toBeGreaterThan(10);
    expect(line!.values.settle_s).toBeGreaterThan(0.5);
    expect(line!.values.settle_s).toBeLessThan(5);
    row('metrics', `overshoot ${f(100 * (ymax - 1))} %, settle ${f(lastOut - 1)} s, IAE ${f(iae)}`, `${f(line!.values.overshoot_pct)} %, ${f(line!.values.settle_s)} s, ${f(line!.values.iae)}`);
  });

  it('saturation monitor counts exactly the steps where |u| reached the limit', () => {
    const run = runFixture(ECHO, 'function controller(r, y, dt)\n    return 3 * r\nend function', {
      duration: 6, dt, setpoint: { kind: 'sine', amplitude: 10, time: 3 }, ...withPlugins([plug('saturation-monitor', { limit: 20 })]),
    });
    expect(run.ok, run.stderr).toBe(true);
    const line = run.stdout.split(/\r?\n/).map(parsePluginLine).find((x) => x && x.name === 'saturation');
    const u = run.vars.log_u;
    const hits = u.filter((v) => Math.abs(v) >= 20).length;
    expect(line!.values.saturated).toBe(hits);
    expect(line!.values.steps).toBe(u.length);
    expect(hits).toBeGreaterThan(0);
    row('saturation monitor', `${hits} of ${u.length} steps`, `${line!.values.saturated} of ${line!.values.steps}`);
  });

  it('watchdog stops the run at the first |e| > limit, and the script still reports', () => {
    // A plant that runs away: y doubles every step from 1, with r = 0, so e = -y.
    const run = runFixture('wd_y = 1\nfunction plant(u, dt)\n    global wd_y\n    if dt > 0\n        wd_y = wd_y * 2\n    end if\n    return wd_y\nend function', asR, {
      duration: 5, dt, setpoint: { kind: 'step', amplitude: 0, time: 0 }, ...withPlugins([plug('watchdog', { limit: 100, after: 0 })]),
    });
    expect(run.ok, run.stderr).toBe(true);
    const { log_y: y, log_e: e } = run.vars;
    // y_k = 2^(k-1) for step k: first |e| > 100 at 2^7 = 128, step 8.
    expect(y.length).toBe(8);
    expect(Math.abs(e[e.length - 1])).toBe(128);
    expect(e.slice(0, -1).every((v) => Math.abs(v) <= 100)).toBe(true);
    expect(run.stdout).toMatch(/run stopped by a monitor: .*watchdog/i);
    expect(run.stdout).toMatch(/@stats steps=8 /);
    row('watchdog', 'stops at step 8 (first |e| = 128 > 100)', `${y.length} samples, last |e| ${Math.abs(e[e.length - 1])}`);
  });
});

gate('plugins: combined with environments, and the library as a whole', () => {
  for (const def of PLUGIN_LIBRARY) {
    it(`${def.id}: runs on an environment with its default parameters, finite output`, () => {
      const spec = { ...environmentSpec('motor-speed')!, plugins: [newPlugin(def.id)] };
      expect(validateLoop(spec)).toBeNull();
      const run = runScript(buildLoopScript(spec));
      expect(run.ok, run.stderr).toBe(true);
      expect(run.vars.log_y.every(Number.isFinite)).toBe(true);
      expect(run.stdout).toMatch(/@stats steps=/);
    });
  }

  it('a realistic stack (reference filter, noise, delay, rate limit, saturation, load, metrics) runs and still settles', () => {
    const spec = {
      ...environmentSpec('motor-speed')!,
      plugins: [plug('ref-lowpass', { tau: 0.1 }), plug('sensor-noise', { sigma: 0.02, seed: 3 }), plug('sensor-delay', { n: 2 }), plug('actuator-rate', { rate: 400 }), plug('actuator-saturation', { lo: -12, hi: 12 }), plug('load-step', { amp: -0.5, at: 2 }), plug('metrics', { band: 0.05 })],
    };
    const run = runScript(buildLoopScript(spec));
    expect(run.ok, run.stderr).toBe(true);
    const y = run.vars.log_y;
    expect(Math.abs(mean(y.slice(-300)) - 10)).toBeLessThan(0.3);
    expect(run.stdout).toMatch(/@plugin metrics/);
  });

  it('a custom sensor plugin (gain 2) and a custom monitor run through the same hooks', () => {
    const gain: PluginInstance = { uid: 'c1', def: 'custom', enabled: true, params: {}, hook: 'sensor', name: 'gain 2', code: 'function hook(y, t, dt)\n    return 2 * y\nend function' };
    const stop: PluginInstance = { uid: 'c2', def: 'custom', enabled: true, params: {}, hook: 'monitor', name: 'stop at 40', code: 'function hook(t, r, y, u, e)\n    if y >= 40\n        return 1\n    end if\n    return 0\nend function' };
    const run = runFixture(COUNTER, asR, { duration: 5, dt, setpoint: { kind: 'step', amplitude: 0, time: 0 }, ...withPlugins([gain, stop]) });
    expect(run.ok, run.stderr).toBe(true);
    const y = run.vars.log_y;
    expect(maxErr(y, y.map((_, i) => 2 * i))).toBe(0);
    expect(y[y.length - 1]).toBe(40);
    expect(y.length).toBe(21);
    row('custom plugins', 'y = 2 i, stops at y = 40 (21 samples)', `${y.length} samples, last ${y[y.length - 1]}`);
  });

  it('"write your own" is validated by running it: a good snippet passes, runtime and syntax errors are caught', () => {
    const good = newPlugin('sensor-noise');
    good.def = 'custom';
    good.hook = 'sensor';
    good.code = 'function hook(y, t, dt)\n    return y + 1\nend function';
    const ok = runScript(buildPluginCheckScript(good));
    expect(ok.ok, ok.stderr).toBe(true);
    expect(parsePluginCheck(ok.stdout)).toEqual({ ok: true, value: 1.5 });
    const crash = { ...good, code: 'function hook(y, t, dt)\n    return no_such_function(y)\nend function' };
    const bad = runScript(buildPluginCheckScript(crash));
    expect(bad.ok).toBe(false);
    expect(parsePluginCheck(bad.stdout)).toBeNull();
    const syntax = { ...good, code: 'function hook(y, t, dt)\n    return y +\nend function' };
    expect(runScript(buildPluginCheckScript(syntax)).ok).toBe(false);
    row('write your own', 'good passes (value 1.5); runtime + syntax errors rejected', 'as expected');
  });

  it('the deliberately wrong specs are rejected (this gate can fail)', () => {
    // Rejected before it ever reaches the engine:
    const zeroDelay = { ...environmentSpec('motor-speed')!, plugins: [plug('sensor-delay', { n: 0 })] };
    expect(validateLoop(zeroDelay)).toMatch(/at least 1/);
    expect(pluginProblem({ ...newPlugin('actuator-saturation'), params: { lo: 'x' as unknown as number, hi: 1 } })).toMatch(/number/);
    // And the engine, not the validator, is what stops a wrong custom snippet that passes the shape check:
    const wrong: PluginInstance = { uid: 'w', def: 'custom', enabled: true, params: {}, hook: 'sensor', name: 'wrong', code: 'function hook(y, t, dt)\n    return not_defined_anywhere + y\nend function' };
    const spec = { ...environmentSpec('motor-speed')!, plugins: [wrong] };
    expect(validateLoop(spec)).toBeNull();
    const run = runScript(buildLoopScript(spec));
    expect(run.ok).toBe(false);
    expect(run.stderr).toMatch(/not_defined_anywhere/);
    row('wrong specs', 'delay n=0 and a custom hook calling an undefined name are rejected', 'rejected');
  });
});
