/** Plugins for the Loop tab: small, named, user-visible Qu snippets that hook
 *  the loop at a defined point. Pure TypeScript, no DOM: `buildLoopScript`
 *  (loopDesign.ts) calls `pluginBlocks` / `pluginStepCode` to write them into
 *  the one script the panel runs, and `loopPlugins.test.ts` tests them.
 *
 *  Hook points, in the order one step applies them:
 *
 *    reference   lpN_reference(r, t, dt)        -> r'     setpoint shaper
 *    sensor      lpN_sensor(y, t, dt)           -> y'     measurement y -> y'
 *    (controller sees r', y' and returns u; the log records r', y', u, e)
 *    monitor     lpN_monitor(t, r, y, u, e)     -> 0|1    records / checks; 1 stops the run
 *    actuator    lpN_actuator(u, t, dt)         -> u'     command u -> u'
 *    disturbance lpN_disturbance(t, dt)         -> d      added to the plant input u'
 *    (plant advances under u' + d)
 *
 *  `N` is the plugin's position in the list; every global a plugin owns is
 *  named `lpN_...`, so two plugins (or two copies of one) never share state
 *  and none can shadow a builtin or the plant/controller's own names. */

export type HookKind = 'reference' | 'sensor' | 'monitor' | 'actuator' | 'disturbance';

/** The order a step applies the hooks in, which is also how the panel groups them. */
export const HOOK_ORDER: HookKind[] = ['reference', 'sensor', 'monitor', 'actuator', 'disturbance'];

export const HOOK_LABEL: Record<HookKind, string> = {
  reference: 'reference shaper',
  sensor: 'sensor',
  monitor: 'monitor',
  actuator: 'actuator',
  disturbance: 'disturbance',
};

/** The function name a plugin of this kind defines (after `lpN_`), and its parameters. */
export const HOOK_SIGNATURE: Record<HookKind, { name: string; args: string; returns: string }> = {
  reference: { name: 'reference', args: 'r, t, dt', returns: "the shaped setpoint r'" },
  sensor: { name: 'sensor', args: 'y, t, dt', returns: "the measurement the controller sees, y'" },
  monitor: { name: 'monitor', args: 't, r, y, u, e', returns: '0 to carry on, 1 to stop the run' },
  actuator: { name: 'actuator', args: 'u, t, dt', returns: "the command the plant gets, u'" },
  disturbance: { name: 'disturbance', args: 't, dt', returns: 'the extra input d added at the plant' },
};

export interface PluginParam {
  id: string;
  label: string;
  default: number;
  min?: number;
  max?: number;
  integer?: boolean;
  /** Whether 0 is allowed even when `min` is above it (no). */
  step?: number;
  hint?: string;
}

export interface PluginDef {
  id: string;
  label: string;
  hook: HookKind;
  /** One line, for the library list. */
  summary: string;
  params: PluginParam[];
  /** The Qu that defines this plugin; `p` is the prefix (`lp3_`), `v` the parameter values. */
  code: (p: string, v: Record<string, number>) => string;
  /** Monitors that print a summary after the run define `<p>report()`. */
  report?: boolean;
}

/** A plugin in the loop's list. `def` is a PluginDef id, or `custom`. */
export interface PluginInstance {
  /** Stable key for the UI (not written into the script). */
  uid: string;
  def: string;
  enabled: boolean;
  params: Record<string, number>;
  /** custom only */
  hook?: HookKind;
  name?: string;
  code?: string;
}

const num = (v: number): string => (Object.is(v, -0) ? '0' : String(v));

// ---------------------------------------------------------------- library

export const PLUGIN_LIBRARY: PluginDef[] = [
  {
    id: 'sensor-noise',
    label: 'Sensor noise (seeded Gaussian)',
    hook: 'sensor',
    summary: 'y\' = y + sigma * N(0,1); the seed makes every run identical.',
    params: [
      { id: 'sigma', label: 'sigma (std dev)', default: 0.01, min: 0, step: 0.001 },
      { id: 'seed', label: 'seed', default: 1, integer: true, min: 0 },
    ],
    code: (p, v) => `${p}sigma = ${num(v.sigma)}
seed(${num(v.seed)})
function ${p}sensor(y, t, dt)
    return y + ${p}sigma * randn(1)
end function`,
  },
  {
    id: 'sensor-delay',
    label: 'Sensor delay (n samples)',
    hook: 'sensor',
    summary: 'The controller sees the measurement from n steps ago (the first value is held until the line fills).',
    params: [{ id: 'n', label: 'delay (samples)', default: 3, integer: true, min: 1, max: 10000 }],
    code: (p, v) => `${p}n = ${num(v.n)}
${p}buf = zeros(${p}n)
${p}i = 0
${p}primed = 0
function ${p}sensor(y, t, dt)
    global ${p}buf, ${p}i, ${p}primed
    if ${p}primed == 0
        for j = 0 to ${p}n - 1
            ${p}buf[j] = y
        end for
        ${p}primed = 1
    end if
    slot = ${p}i mod ${p}n
    out = ${p}buf[slot]
    ${p}buf[slot] = y
    ${p}i = ${p}i + 1
    return out
end function`,
  },
  {
    id: 'sensor-quantise',
    label: 'Sensor quantisation (ADC bits)',
    hook: 'sensor',
    summary: 'Rounds y to the nearest level of an ADC with the given bits over [lo, hi] (values outside clip).',
    params: [
      { id: 'bits', label: 'bits', default: 10, integer: true, min: 1, max: 32 },
      { id: 'lo', label: 'range low', default: -10 },
      { id: 'hi', label: 'range high', default: 10 },
    ],
    code: (p, v) => `${p}lo = ${num(v.lo)}
${p}hi = ${num(v.hi)}
${p}lsb = (${p}hi - ${p}lo) / (2 ^ ${num(v.bits)} - 1)
function ${p}sensor(y, t, dt)
    c = round((min(${p}hi, max(${p}lo, y)) - ${p}lo) / ${p}lsb)
    return ${p}lo + c * ${p}lsb
end function`,
  },
  {
    id: 'actuator-saturation',
    label: 'Actuator saturation',
    hook: 'actuator',
    summary: "u' = clip(u, lo, hi).",
    params: [
      { id: 'lo', label: 'lower limit', default: -10 },
      { id: 'hi', label: 'upper limit', default: 10 },
    ],
    code: (p, v) => `${p}lo = ${num(v.lo)}
${p}hi = ${num(v.hi)}
function ${p}actuator(u, t, dt)
    return min(${p}hi, max(${p}lo, u))
end function`,
  },
  {
    id: 'actuator-rate',
    label: 'Actuator rate limit',
    hook: 'actuator',
    summary: "u' may change by at most rate * dt per step (it starts at 0).",
    params: [{ id: 'rate', label: 'max rate (units/s)', default: 20, min: 0, step: 1 }],
    code: (p, v) => `${p}rate = ${num(v.rate)}
${p}prev = 0
function ${p}actuator(u, t, dt)
    global ${p}prev
    room = ${p}rate * dt
    ${p}prev = ${p}prev + min(room, max(-room, u - ${p}prev))
    return ${p}prev
end function`,
  },
  {
    id: 'actuator-deadzone',
    label: 'Actuator dead zone',
    hook: 'actuator',
    summary: "u' = 0 while |u| <= width, then u shrunk toward 0 by width (stiction, backlash).",
    params: [{ id: 'width', label: 'half width', default: 0.5, min: 0, step: 0.1 }],
    code: (p, v) => `${p}width = ${num(v.width)}
function ${p}actuator(u, t, dt)
    if abs(u) <= ${p}width
        return 0
    end if
    return u - sign(u) * ${p}width
end function`,
  },
  {
    id: 'load-step',
    label: 'Load disturbance: step',
    hook: 'disturbance',
    summary: 'Adds a constant d to the plant input from time `at` on.',
    params: [
      { id: 'amp', label: 'amplitude d', default: 1 },
      { id: 'at', label: 'at (s)', default: 2, min: 0, step: 0.5 },
    ],
    code: (p, v) => `${p}amp = ${num(v.amp)}
${p}at = ${num(v.at)}
function ${p}disturbance(t, dt)
    if t >= ${p}at
        return ${p}amp
    end if
    return 0
end function`,
  },
  {
    id: 'load-pulse',
    label: 'Load disturbance: pulse',
    hook: 'disturbance',
    summary: 'Adds d to the plant input for `width` seconds starting at `at`.',
    params: [
      { id: 'amp', label: 'amplitude d', default: 1 },
      { id: 'at', label: 'at (s)', default: 2, min: 0, step: 0.5 },
      { id: 'width', label: 'width (s)', default: 0.5, min: 0, step: 0.1 },
    ],
    code: (p, v) => `${p}amp = ${num(v.amp)}
${p}at = ${num(v.at)}
${p}width = ${num(v.width)}
function ${p}disturbance(t, dt)
    if t >= ${p}at and t < ${p}at + ${p}width
        return ${p}amp
    end if
    return 0
end function`,
  },
  {
    id: 'load-sine',
    label: 'Load disturbance: sine',
    hook: 'disturbance',
    summary: 'Adds amp * sin(2 pi t / period) to the plant input (vibration, waves, a periodic load).',
    params: [
      { id: 'amp', label: 'amplitude d', default: 0.5 },
      { id: 'period', label: 'period (s)', default: 1, min: 1e-9, step: 0.1 },
    ],
    code: (p, v) => `${p}amp = ${num(v.amp)}
${p}period = ${num(v.period)}
function ${p}disturbance(t, dt)
    return ${p}amp * sin(2 * pi * t / ${p}period)
end function`,
  },
  {
    id: 'ref-lowpass',
    label: 'Reference low-pass',
    hook: 'reference',
    summary: "r' follows r through a first-order filter with time constant tau (exact for a step).",
    params: [{ id: 'tau', label: 'tau (s)', default: 0.5, min: 1e-9, step: 0.1 }],
    code: (p, v) => `${p}tau = ${num(v.tau)}
${p}state = 0
function ${p}reference(r, t, dt)
    global ${p}state
    ${p}state = ${p}state + (1 - exp(-dt / ${p}tau)) * (r - ${p}state)
    return ${p}state
end function`,
  },
  {
    id: 'ref-rate',
    label: 'Reference rate limit',
    hook: 'reference',
    summary: "r' ramps toward r at no more than rate per second (it starts at 0).",
    params: [{ id: 'rate', label: 'max rate (units/s)', default: 1, min: 0, step: 0.1 }],
    code: (p, v) => `${p}rate = ${num(v.rate)}
${p}state = 0
function ${p}reference(r, t, dt)
    global ${p}state
    room = ${p}rate * dt
    ${p}state = ${p}state + min(room, max(-room, r - ${p}state))
    return ${p}state
end function`,
  },
  {
    id: 'metrics',
    label: 'Metrics recorder (overshoot, settling time)',
    hook: 'monitor',
    report: true,
    summary: 'Records overshoot %, settling time after the last setpoint change, steady-state error and IAE; prints them after the run.',
    params: [{ id: 'band', label: 'settling band (fraction)', default: 0.02, min: 1e-9, step: 0.01 }],
    code: (p, v) => `${p}band = ${num(v.band)}
${p}ymax = -inf
${p}r_last = 0
${p}r_change = 0
${p}out_t = 0
${p}iae = 0
${p}y_last = 0
function ${p}monitor(t, r, y, u, e)
    global ${p}ymax, ${p}r_last, ${p}r_change, ${p}out_t, ${p}iae, ${p}y_last
    if abs(r - ${p}r_last) > 1e-12
        ${p}r_change = t
        ${p}out_t = t
        ${p}ymax = -inf
    end if
    ${p}r_last = r
    ${p}ymax = max(${p}ymax, y)
    if abs(e) > ${p}band * max(abs(r), 1e-12)
        ${p}out_t = t
    end if
    ${p}iae = ${p}iae + abs(e) * dt
    ${p}y_last = y
    return 0
end function
function ${p}report()
    over = 0
    if abs(${p}r_last) > 1e-12
        over = 100 * max(0, (${p}ymax - ${p}r_last) / abs(${p}r_last))
    end if
    print("@plugin metrics overshoot_pct={over:.12g} settle_s={${p}out_t - ${p}r_change:.12g} steady_err={${p}r_last - ${p}y_last:.12g} iae={${p}iae:.12g}")
end function`,
  },
  {
    id: 'saturation-monitor',
    label: 'Saturation / windup monitor',
    hook: 'monitor',
    report: true,
    summary: 'Counts the steps where |u| reached a limit (where an integrator would wind up); prints the fraction after the run.',
    params: [{ id: 'limit', label: '|u| limit', default: 10, min: 0 }],
    code: (p, v) => `${p}limit = ${num(v.limit)}
${p}hits = 0
${p}steps = 0
function ${p}monitor(t, r, y, u, e)
    global ${p}hits, ${p}steps
    ${p}steps = ${p}steps + 1
    if abs(u) >= ${p}limit
        ${p}hits = ${p}hits + 1
    end if
    return 0
end function
function ${p}report()
    print("@plugin saturation steps={${p}steps} saturated={${p}hits} fraction={${p}hits / max(1, ${p}steps):.12g}")
end function`,
  },
  {
    id: 'watchdog',
    label: 'Watchdog (abort on |e| > limit)',
    hook: 'monitor',
    summary: 'Stops the run, with a message, the first step after `after` seconds where |e| exceeds the limit.',
    params: [
      { id: 'limit', label: '|e| limit', default: 5, min: 0 },
      { id: 'after', label: 'watch from (s)', default: 0, min: 0, step: 0.5 },
    ],
    code: (p, v) => `${p}limit = ${num(v.limit)}
${p}after = ${num(v.after)}
function ${p}monitor(t, r, y, u, e)
    if t >= ${p}after and abs(e) > ${p}limit
        return 1
    end if
    return 0
end function`,
  },
];

export const pluginDef = (id: string): PluginDef | undefined => PLUGIN_LIBRARY.find((d) => d.id === id);

let uidSeq = 0;
/** A new instance of a library plugin with its default parameters. */
export function newPlugin(def: string): PluginInstance {
  const d = pluginDef(def);
  const params: Record<string, number> = {};
  for (const p of d?.params ?? []) params[p.id] = p.default;
  return { uid: `pl${Date.now().toString(36)}${(uidSeq++).toString(36)}`, def, enabled: true, params };
}

/** A blank custom plugin for a hook, with a working example body. */
export function newCustomPlugin(hook: HookKind = 'sensor'): PluginInstance {
  return { uid: `pl${Date.now().toString(36)}${(uidSeq++).toString(36)}`, def: 'custom', enabled: true, params: {}, hook, name: 'my plugin', code: CUSTOM_TEMPLATES[hook] };
}

/** Starting text for "write your own": defines `hook(...)`, which the generator
 *  renames to `lpN_<kind>` so the copy in the script cannot clash. */
export const CUSTOM_TEMPLATES: Record<HookKind, string> = {
  reference: '# Smooth the setpoint: hook(r, t, dt) returns the shaped r.\nfunction hook(r, t, dt)\n    return r\nend function',
  sensor: '# Offset and gain error: hook(y, t, dt) returns what the controller sees.\nfunction hook(y, t, dt)\n    return 1.01 * y + 0.002\nend function',
  monitor: '# hook(t, r, y, u, e) returns 1 to stop the run, 0 to carry on.\nfunction hook(t, r, y, u, e)\n    if abs(y) > 100\n        return 1\n    end if\n    return 0\nend function',
  actuator: '# Weaker actuator: hook(u, t, dt) returns what the plant is driven with.\nfunction hook(u, t, dt)\n    return 0.9 * u\nend function',
  disturbance: '# hook(t, dt) returns the extra input added at the plant.\nfunction hook(t, dt)\n    return 0\nend function',
};

// ---------------------------------------------------------------- checks

const IDENT = /\bhook\b/;
/** Matches the definition line of `hook` in a custom snippet. */
const HOOK_DEF = /^[ \t]*function[ \t]+hook[ \t]*\(([^)]*)\)/m;
const REPORT_DEF = /^[ \t]*function[ \t]+report[ \t]*\(\s*\)/m;

const argCount = (args: string): number => (args.trim() === '' ? 0 : args.split(',').length);

/** Why a plugin cannot be written into the script, or `null`. Library
 *  plugins are checked against their parameter limits; custom ones for the
 *  shape of their `hook` function (the running check is the engine's). */
export function pluginProblem(pl: PluginInstance): string | null {
  if (pl.def === 'custom') {
    const label = pl.name?.trim() || 'custom plugin';
    if (!pl.hook || !(pl.hook in HOOK_SIGNATURE)) return `${label}: pick a hook point.`;
    const code = pl.code ?? '';
    if (!code.trim()) return `${label}: write the hook function.`;
    const m = HOOK_DEF.exec(code);
    const sig = HOOK_SIGNATURE[pl.hook];
    if (!m) return `${label}: define a function named hook(${sig.args}) that returns ${sig.returns}.`;
    if (argCount(m[1]) !== argCount(sig.args)) return `${label}: hook must take (${sig.args}) for a ${HOOK_LABEL[pl.hook]} plugin.`;
    if (/^[ \t]*(import|run_for|every)\b/m.test(code)) return `${label}: a plugin cannot import files or start timers.`;
    if (!/\breturn\b/.test(code)) return `${label}: hook must return a value.`;
    return null;
  }
  const d = pluginDef(pl.def);
  if (!d) return `Unknown plugin "${pl.def}".`;
  for (const prm of d.params) {
    const v = pl.params[prm.id];
    if (typeof v !== 'number' || !Number.isFinite(v)) return `${d.label}: ${prm.label} must be a number.`;
    if (prm.integer && !Number.isInteger(v)) return `${d.label}: ${prm.label} must be a whole number.`;
    if (prm.min != null && v < prm.min) return `${d.label}: ${prm.label} must be at least ${prm.min}.`;
    if (prm.max != null && v > prm.max) return `${d.label}: ${prm.label} must be at most ${prm.max}.`;
  }
  return null;
}

/** First problem in a plugin list, or `null`. */
export function pluginsProblem(list: PluginInstance[] | undefined): string | null {
  for (const pl of list ?? []) {
    if (!pl.enabled) continue;
    const p = pluginProblem(pl);
    if (p) return p;
  }
  return null;
}

// ---------------------------------------------------------------- generation

export interface ActivePlugin {
  /** 1-based position in the whole list (names stay put when others are switched off). */
  index: number;
  prefix: string;
  hook: HookKind;
  label: string;
  /** Has a `report()` to call after the run. */
  report: boolean;
  /** Can stop the run. */
  monitor: boolean;
  instance: PluginInstance;
}

export function activePlugins(list: PluginInstance[] | undefined): ActivePlugin[] {
  const out: ActivePlugin[] = [];
  (list ?? []).forEach((pl, i) => {
    if (!pl.enabled) return;
    const prefix = `lp${i + 1}_`;
    if (pl.def === 'custom') {
      const hook = pl.hook ?? 'sensor';
      out.push({ index: i + 1, prefix, hook, label: (pl.name ?? '').split(/\s+/).join(' ').trim() || 'custom plugin', report: REPORT_DEF.test(pl.code ?? ''), monitor: hook === 'monitor', instance: pl });
    } else {
      const d = pluginDef(pl.def);
      if (d) out.push({ index: i + 1, prefix, hook: d.hook, label: d.label, report: !!d.report, monitor: d.hook === 'monitor', instance: pl });
    }
  });
  return out;
}

/** A custom snippet with `hook` -> `lpN_<kind>` and `report` -> `lpN_report`. */
export function renameCustom(code: string, prefix: string, hook: HookKind): string {
  return code
    .replace(/\s+$/, '')
    .split('\n')
    .map((line) => {
      // Leave comments alone so the user's prose does not get rewritten.
      const hash = line.indexOf('#');
      const body = hash < 0 ? line : line.slice(0, hash);
      const tail = hash < 0 ? '' : line.slice(hash);
      return body.replace(/\bhook\b/g, `${prefix}${HOOK_SIGNATURE[hook].name}`).replace(/\breport\b(?=\s*\()/g, `${prefix}report`) + tail;
    })
    .join('\n');
}

/** The delimited, commented blocks that define the active plugins. */
export function pluginBlocks(list: PluginInstance[] | undefined): string[] {
  const out: string[] = [];
  for (const a of activePlugins(list)) {
    const sig = HOOK_SIGNATURE[a.hook];
    const head = `# ---- plugin ${a.index}: ${a.label} (${HOOK_LABEL[a.hook]} hook: ${a.prefix}${sig.name}(${sig.args}) -> ${sig.returns}) ----`;
    let body: string;
    if (a.instance.def === 'custom') {
      body = `# Your snippet; "hook" is written as ${a.prefix}${sig.name} so it cannot clash with another plugin.\n${renameCustom(a.instance.code ?? '', a.prefix, a.hook)}`;
    } else {
      const d = pluginDef(a.instance.def)!;
      const v: Record<string, number> = {};
      for (const prm of d.params) v[prm.id] = a.instance.params[prm.id] ?? prm.default;
      body = `# ${d.summary}\n${d.code(a.prefix, v)}`;
    }
    out.push(`${head}\n${body}`);
  }
  return out;
}

const ofHook = (act: ActivePlugin[], k: HookKind): ActivePlugin[] => act.filter((a) => a.hook === k);

/** The per-step lines the plugins add. `before` goes between the setpoint and
 *  the controller (reference then sensor), `monitors` after the log is written,
 *  and `actuate` between the controller and the plant. All are 4-space indented. */
export function pluginStepCode(list: PluginInstance[] | undefined): { before: string[]; monitors: string[]; actuate: string[] } {
  const act = activePlugins(list);
  const before: string[] = [];
  for (const a of ofHook(act, 'reference')) before.push(`    r = ${a.prefix}reference(r, t, dt)`);
  before.push('    y = y_true');
  for (const a of ofHook(act, 'sensor')) before.push(`    y = ${a.prefix}sensor(y, t, dt)`);
  const monitors: string[] = [];
  for (const a of ofHook(act, 'monitor')) {
    monitors.push(
      `    if ${a.prefix}monitor(t, r, y, u, err) != 0`,
      `        error("lp_abort: plugin ${a.index} (${a.label.replace(/[^A-Za-z0-9 ()|<>=.,-]/g, '')}) stopped the run at t={t}, step {k}")`,
      '    end if',
    );
  }
  const actuate: string[] = ['    u_act = u'];
  for (const a of ofHook(act, 'actuator')) actuate.push(`    u_act = ${a.prefix}actuator(u_act, t, dt)`);
  for (const a of ofHook(act, 'disturbance')) actuate.push(`    u_act = u_act + ${a.prefix}disturbance(t, dt)`);
  return { before, monitors, actuate };
}

/** Lines after the run: one `report()` per monitor that has one. */
export function pluginReportCode(list: PluginInstance[] | undefined): string[] {
  return activePlugins(list).filter((a) => a.report).map((a) => `${a.prefix}report()`);
}

/** True when some active plugin can stop the run (the script then wraps run_for in try/catch). */
export const canAbort = (list: PluginInstance[] | undefined): boolean => activePlugins(list).some((a) => a.monitor);

export const hasPlugins = (list: PluginInstance[] | undefined): boolean => activePlugins(list).length > 0;

// ---------------------------------------------------------------- "write your own": validate by running

/** A small script that defines the plugin (with its parameters/state) and calls
 *  the hook a few times with plausible arguments, printing `@check ok=1 value=..`
 *  or letting the engine's own error through. The panel runs it in a throwaway
 *  interpreter and shows the error in plain words. */
export function buildPluginCheckScript(pl: PluginInstance): string {
  const bad = pluginProblem(pl);
  if (bad) throw new Error(bad);
  const blocks = pluginBlocks([{ ...pl, enabled: true }]);
  // Position 1 in a one-item list: prefix lp1_.
  const a = activePlugins([{ ...pl, enabled: true }])[0];
  const fn = `${a.prefix}${HOOK_SIGNATURE[a.hook].name}`;
  const call: Record<HookKind, string> = {
    reference: `${fn}(1, 0.01 * i, 0.01)`,
    sensor: `${fn}(0.5, 0.01 * i, 0.01)`,
    monitor: `${fn}(0.01 * i, 1, 0.5, 0.2, 0.5)`,
    actuator: `${fn}(0.5, 0.01 * i, 0.01)`,
    disturbance: `${fn}(0.01 * i, 0.01)`,
  };
  const lines = [
    '# Check run for a plugin: define it, then call its hook a few times.',
    // The metrics monitor reads dt; give the check the same global the loop has.
    'dt = 0.01',
    ...blocks,
    'chk_last = 0',
    'for i = 0 to 4',
    `    chk_last = ${call[a.hook]}`,
    'end for',
  ];
  if (a.report) lines.push(`${a.prefix}report()`);
  lines.push('print("@check ok=1 value={chk_last}")');
  return lines.join('\n') + '\n';
}

/** Reads `buildPluginCheckScript`'s output: the last value the hook returned, or null if it did not run. */
export function parsePluginCheck(output: string): { ok: boolean; value: number } | null {
  const line = output.split(/\r?\n/).find((l) => l.startsWith('@check'));
  if (!line) return null;
  const m = /value=(\S+)/.exec(line);
  const v = m ? Number(m[1]) : NaN;
  return { ok: /ok=1/.test(line), value: v };
}

/** Parses the `@plugin name key=value ...` summary lines a monitor prints. */
export function parsePluginLine(line: string): { name: string; values: Record<string, number> } | null {
  const m = /^@plugin (\S+)(.*)$/.exec(line.trim());
  if (!m) return null;
  const values: Record<string, number> = {};
  for (const kv of m[2].matchAll(/([A-Za-z_]+)=(\S+)/g)) values[kv[1]] = Number(kv[2]);
  return { name: m[1], values };
}

/** Exposed for the tests: does this snippet mention `hook` as a bare word. */
export const mentionsHook = (code: string): boolean => IDENT.test(code);
