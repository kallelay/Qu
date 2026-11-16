import { describe, expect, it } from 'vitest';
import { DEFAULT_LOOP, CONTROLLER_EXAMPLES, PLANT_EXAMPLES, buildLoopScript, environmentSpec, validateLoop, type LoopSpec } from './loopDesign';
import { ENVIRONMENTS, environmentById } from './loopEnvironments';
import {
  CUSTOM_TEMPLATES,
  HOOK_ORDER,
  PLUGIN_LIBRARY,
  activePlugins,
  buildPluginCheckScript,
  canAbort,
  newCustomPlugin,
  newPlugin,
  parsePluginCheck,
  parsePluginLine,
  pluginBlocks,
  pluginDef,
  pluginProblem,
  pluginStepCode,
  pluginsProblem,
  renameCustom,
  type PluginInstance,
} from './loopPlugins';

const withPlugins = (plugins: PluginInstance[]): LoopSpec => ({ ...DEFAULT_LOOP, plugins });
const make = (def: string, params: Record<string, number> = {}): PluginInstance => {
  const p = newPlugin(def);
  p.params = { ...p.params, ...params };
  return p;
};

/** Qu builtins a plugin must never shadow (a user function with the same name wins silently). */
const BUILTINS = ['solve', 'fit', 'step', 'filter', 'sin', 'min', 'max', 'abs', 'round', 'exp', 'sign', 'zeros', 'print', 'seed', 'randn', 'poly', 'inv', 'expm', 'eye', 'plant', 'controller', 'setpoint'];

/** Names a block of Qu defines at its top level: `name = ...` and `function name(`. */
function topLevelNames(code: string): string[] {
  const names: string[] = [];
  let depth = 0;
  for (const raw of code.split('\n')) {
    const line = raw.replace(/#.*$/, '');
    const fn = /^function\s+([A-Za-z_]\w*)\s*\(/.exec(line);
    if (depth === 0) {
      if (fn) names.push(fn[1]);
      else {
        const asg = /^([A-Za-z_]\w*)\s*=[^=]/.exec(line);
        if (asg) names.push(asg[1]);
      }
    }
    if (/^function\b/.test(line)) depth++;
    if (/^end function\b/.test(line)) depth--;
  }
  return names;
}

describe('the loop is unchanged without plugins', () => {
  it('an empty or missing plugin list writes exactly the same script', () => {
    const plain = buildLoopScript(DEFAULT_LOOP);
    expect(buildLoopScript({ ...DEFAULT_LOOP, plugins: [] })).toBe(plain);
    expect(buildLoopScript(withPlugins([{ ...make('actuator-saturation'), enabled: false }]))).toBe(plain);
    expect(plain).toContain('y = plant(u, dt)');
    expect(plain).not.toContain('lp1_');
    expect(plain).not.toContain('y_true');
  });
});

describe('library', () => {
  it('has the ten kinds the brief asks for, at every hook point', () => {
    const ids = PLUGIN_LIBRARY.map((d) => d.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const want of ['sensor-noise', 'sensor-delay', 'sensor-quantise', 'actuator-saturation', 'actuator-rate', 'actuator-deadzone', 'load-step', 'load-pulse', 'load-sine', 'ref-lowpass', 'ref-rate', 'metrics', 'watchdog']) {
      expect(ids).toContain(want);
    }
    for (const hook of HOOK_ORDER) expect(PLUGIN_LIBRARY.some((d) => d.hook === hook)).toBe(true);
  });

  it('every default parameter set is valid, and every plugin only defines names under its own lpN_ prefix', () => {
    for (const d of PLUGIN_LIBRARY) {
      const pl = make(d.id);
      expect(pluginProblem(pl), d.id).toBeNull();
      const code = pluginBlocks([pl]).join('\n');
      const names = topLevelNames(code);
      expect(names.length, d.id).toBeGreaterThan(0);
      for (const nm of names) expect(nm.startsWith('lp1_'), `${d.id}: ${nm}`).toBe(true);
      for (const nm of names) expect(BUILTINS).not.toContain(nm);
    }
  });

  it('parameters are written into the code and follow the instance', () => {
    const code = pluginBlocks([make('actuator-saturation', { lo: -3.5, hi: 7.25 })]).join('\n');
    expect(code).toContain('lp1_lo = -3.5');
    expect(code).toContain('lp1_hi = 7.25');
    expect(code).toContain('function lp1_actuator(u, t, dt)');
  });
});

describe('the generated script', () => {
  const stack = withPlugins([
    make('ref-lowpass'),
    make('sensor-noise', { sigma: 0.1, seed: 5 }),
    make('sensor-delay', { n: 2 }),
    make('metrics'),
    make('actuator-saturation'),
    make('load-step'),
  ]);
  const script = buildLoopScript(stack);

  it('writes each plugin as a delimited, commented block, in list order, after the controller', () => {
    const heads = script.split('\n').filter((l) => l.startsWith('# ---- plugin '));
    expect(heads.map((h) => /plugin (\d+):/.exec(h)![1])).toEqual(['1', '2', '3', '4', '5', '6']);
    expect(heads[1]).toMatch(/Sensor noise.*sensor hook: lp2_sensor\(y, t, dt\)/);
    expect(script.indexOf('# ---- plugin 1')).toBeGreaterThan(script.indexOf('# ---- controller'));
    expect(script.indexOf('# ---- plugin 1')).toBeLessThan(script.indexOf('# ---- setpoint'));
  });

  it('applies the hooks in a fixed order each step: reference, sensor, controller, log, monitor, actuator, disturbance, plant', () => {
    const body = script.slice(script.indexOf('every dt do'));
    const order = [
      'r = setpoint(t)',
      'r = lp1_reference(r, t, dt)',
      'y = y_true',
      'y = lp2_sensor(y, t, dt)',
      'y = lp3_sensor(y, t, dt)',
      'u = controller(r, y, dt)',
      'log_u[k - 1] = u',
      'lp4_monitor(t, r, y, u, err)',
      'u_act = u',
      'u_act = lp5_actuator(u_act, t, dt)',
      'u_act = u_act + lp6_disturbance(t, dt)',
      'y_true = plant(u_act, dt)',
    ];
    let at = -1;
    for (const frag of order) {
      const i = body.indexOf(frag);
      expect(i, frag).toBeGreaterThan(at);
      at = i;
    }
    expect(script).toContain('y_true = plant(0, 0)');
  });

  it('wraps the run in try/catch only when a monitor can stop it, and only catches its own abort', () => {
    expect(canAbort(stack.plugins)).toBe(true);
    expect(script).toContain('catch lp_err');
    expect(script).toContain('contains(lp_err.message, "lp_abort:")');
    expect(script).toContain('error(lp_err.message)');
    expect(script).toContain('lp4_report()');
    const quiet = buildLoopScript(withPlugins([make('actuator-saturation')]));
    expect(quiet).not.toContain('catch');
    expect(quiet).toContain('st = run_for_stats()');
  });

  it('numbering stays put when a plugin is switched off, and the disabled one leaves no code', () => {
    const list = [make('sensor-delay'), { ...make('sensor-noise'), enabled: false }, make('actuator-rate')];
    const s = buildLoopScript(withPlugins(list));
    expect(s).toContain('# ---- plugin 1:');
    expect(s).not.toContain('# ---- plugin 2:');
    expect(s).toContain('# ---- plugin 3:');
    expect(s).toContain('lp3_actuator');
    expect(s).not.toContain('lp2_');
  });

  it('two copies of one plugin keep separate state', () => {
    const s = buildLoopScript(withPlugins([make('sensor-delay', { n: 2 }), make('sensor-delay', { n: 5 })]));
    expect(s).toContain('lp1_n = 2');
    expect(s).toContain('lp2_n = 5');
    expect(s).toContain('global lp1_buf, lp1_i, lp1_primed');
    expect(s).toContain('global lp2_buf, lp2_i, lp2_primed');
  });

  it('keeps the HIL pacing and the scope lines', () => {
    const s = buildLoopScript({ ...stack, mode: 'hil', duration: 3, dt: 0.002 });
    expect(s).toContain('run_for(duration, pace=pace, tolerance=1 ms)');
    expect(s).toContain('print("@loop k={k} t={t} late={late}');
    expect(s.indexOf('tic()')).toBeLessThan(s.indexOf('try'));
  });
});

describe('validation', () => {
  it('rejects out-of-range and non-integer parameters, saying which', () => {
    expect(pluginProblem(make('sensor-delay', { n: 0 }))).toMatch(/at least 1/);
    expect(pluginProblem(make('sensor-delay', { n: 2.5 }))).toMatch(/whole number/);
    expect(pluginProblem(make('sensor-quantise', { bits: 40 }))).toMatch(/at most 32/);
    expect(pluginProblem(make('sensor-noise', { sigma: -1 }))).toMatch(/at least 0/);
    expect(pluginProblem(make('sensor-noise', { sigma: NaN }))).toMatch(/number/);
    expect(pluginProblem({ ...make('sensor-noise'), def: 'nope' })).toMatch(/Unknown plugin/);
  });

  it('validateLoop reports a bad enabled plugin and ignores a disabled one', () => {
    expect(validateLoop(withPlugins([make('sensor-delay', { n: 0 })]))).toMatch(/Sensor delay/);
    expect(validateLoop(withPlugins([{ ...make('sensor-delay', { n: 0 }), enabled: false }]))).toBeNull();
    expect(pluginsProblem(undefined)).toBeNull();
  });

  it('checks the shape of a custom snippet', () => {
    const c = (hook: PluginInstance['hook'], code: string): PluginInstance => ({ ...newCustomPlugin(hook), code });
    expect(pluginProblem(newCustomPlugin('sensor'))).toBeNull();
    for (const h of HOOK_ORDER) expect(pluginProblem(newCustomPlugin(h)), h).toBeNull();
    expect(pluginProblem(c('sensor', ''))).toMatch(/write the hook/);
    expect(pluginProblem(c('sensor', 'function other(y, t, dt)\n return y\nend function'))).toMatch(/define a function named hook\(y, t, dt\)/);
    expect(pluginProblem(c('sensor', 'function hook(y)\n return y\nend function'))).toMatch(/must take \(y, t, dt\)/);
    expect(pluginProblem(c('monitor', 'function hook(t, r, y, u, e)\n print(1)\nend function'))).toMatch(/must return/);
    expect(pluginProblem(c('sensor', 'import "x.qu"\nfunction hook(y, t, dt)\n return y\nend function'))).toMatch(/cannot import/);
    expect(pluginProblem(c('sensor', 'every dt do\nend\nfunction hook(y, t, dt)\n return y\nend function'))).toMatch(/cannot import files or start timers/);
  });
});

describe('write your own', () => {
  it('renames hook (and report) per plugin so two custom plugins cannot clash, leaving comments alone', () => {
    const code = '# hook is the entry\nfunction hook(y, t, dt)\n    return y + 1  # hook comment\nend function\nfunction report()\n    print("hook {y}")\nend function';
    const out = renameCustom(code, 'lp3_', 'sensor');
    expect(out).toContain('function lp3_sensor(y, t, dt)');
    expect(out).toContain('function lp3_report()');
    expect(out).toContain('# hook is the entry');
    expect(out).toContain('# hook comment');
    expect(out).toContain('"lp3_sensor {y}"');
  });

  it('puts a custom plugin into the script like a library one', () => {
    const a = newCustomPlugin('sensor');
    const b = { ...newCustomPlugin('monitor'), name: 'stop\nnow' };
    const s = buildLoopScript(withPlugins([a, b]));
    expect(s).toContain('# ---- plugin 1: my plugin (sensor hook: lp1_sensor(y, t, dt)');
    expect(s).toContain('# ---- plugin 2: stop now (monitor hook: lp2_monitor(t, r, y, u, e)');
    expect(s).toContain('function lp1_sensor(y, t, dt)');
    expect(s).toContain('function lp2_monitor(t, r, y, u, e)');
    expect(s).not.toMatch(/^function hook/m);
    expect(activePlugins([a, b]).map((x) => x.prefix)).toEqual(['lp1_', 'lp2_']);
  });

  it('detects a custom report() and calls it after the run', () => {
    const m = { ...newCustomPlugin('monitor'), code: `${CUSTOM_TEMPLATES.monitor}\nfunction report()\n    print("@plugin mine x=1")\nend function` };
    expect(buildLoopScript(withPlugins([m]))).toContain('lp1_report()');
  });

  it('builds a check script that defines the plugin and calls its hook, and parses its answer', () => {
    const s = buildPluginCheckScript({ ...newCustomPlugin('sensor'), code: 'function hook(y, t, dt)\n    return y * 2\nend function' });
    expect(s).toContain('function lp1_sensor(y, t, dt)');
    expect(s).toContain('lp1_sensor(0.5, 0.01 * i, 0.01)');
    expect(s).toContain('print("@check ok=1 value={chk_last}")');
    expect(parsePluginCheck('@check ok=1 value=1')).toEqual({ ok: true, value: 1 });
    expect(parsePluginCheck('nothing')).toBeNull();
    expect(() => buildPluginCheckScript({ ...newCustomPlugin('sensor'), code: '' })).toThrow(/write the hook/);
  });

  it('reads the summary lines a monitor prints', () => {
    expect(parsePluginLine('@plugin metrics overshoot_pct=16.3 settle_s=1.9 iae=0.33')).toEqual({ name: 'metrics', values: { overshoot_pct: 16.3, settle_s: 1.9, iae: 0.33 } });
    expect(parsePluginLine('@loop k=1')).toBeNull();
  });
});

describe('step hooks', () => {
  it('lists the hooks of each kind in order', () => {
    const h = pluginStepCode([make('ref-rate'), make('sensor-delay'), make('watchdog'), make('actuator-rate'), make('load-sine')]);
    expect(h.before).toEqual(['    r = lp1_reference(r, t, dt)', '    y = y_true', '    y = lp2_sensor(y, t, dt)']);
    expect(h.monitors.join('\n')).toContain('lp3_monitor(t, r, y, u, err) != 0');
    expect(h.monitors.join('\n')).toContain('lp_abort: plugin 3 (Watchdog (abort on |e| > limit))');
    expect(h.actuate).toEqual(['    u_act = u', '    u_act = lp4_actuator(u_act, t, dt)', '    u_act = u_act + lp5_disturbance(t, dt)']);
  });
});

describe('environments', () => {
  it('there are nine, with unique ids, each fully described', () => {
    expect(ENVIRONMENTS.length).toBe(9);
    expect(new Set(ENVIRONMENTS.map((e) => e.id)).size).toBe(9);
    for (const e of ENVIRONMENTS) {
      expect(e.physics.length, e.id).toBeGreaterThan(40);
      expect(e.tryThis.length, e.id).toBeGreaterThan(20);
      expect(e.dt).toBeGreaterThan(0);
      expect(e.duration).toBeGreaterThan(e.setpoint.time + e.expect.settleBy);
      expect(e.expect.band).toBeGreaterThan(0);
      expect(environmentById(e.id)).toBe(e);
    }
  });

  it('every environment is a valid loop whose plant and controller are in the example lists', () => {
    for (const e of ENVIRONMENTS) {
      const spec = environmentSpec(e.id)!;
      expect(validateLoop(spec), e.id).toBeNull();
      expect(PLANT_EXAMPLES.some((p) => p.id === e.plant.id)).toBe(true);
      expect(CONTROLLER_EXAMPLES.some((p) => p.id === e.controller.id)).toBe(true);
      const script = buildLoopScript(spec);
      expect(script).toContain(`# ---- plant: ${e.plant.label} ----`);
      expect(script).toContain(`# ---- controller: ${e.controller.label} ----`);
      expect(script).toContain('function plant(u, dt)');
      expect(script).toContain('function controller(r, y, dt)');
    }
    expect(environmentSpec('nope')).toBeNull();
  });

  it('the two original examples are still first, and the environments do not shadow builtins', () => {
    expect(PLANT_EXAMPLES.slice(0, 2).map((p) => p.id)).toEqual(['lag1', 'msd']);
    expect(CONTROLLER_EXAMPLES.slice(0, 2).map((p) => p.id)).toEqual(['pid', 'pi']);
    for (const e of ENVIRONMENTS) {
      for (const code of [e.plant.code, e.controller.code]) {
        for (const nm of topLevelNames(code)) {
          if (nm === 'plant' || nm === 'controller') continue;
          expect(/^(env|ctl)_/.test(nm), `${e.id}: ${nm}`).toBe(true);
        }
      }
    }
  });

  it('plugins can be attached to an environment and ride along into its script', () => {
    const spec: LoopSpec = { ...environmentSpec('cruise')!, plugins: [make('load-step', { amp: -300, at: 20 })] };
    expect(validateLoop(spec)).toBeNull();
    const s = buildLoopScript(spec);
    expect(s).toContain('lp1_amp = -300');
    expect(s).toContain('y_true = plant(u_act, dt)');
    expect(pluginDef('load-step')!.hook).toBe('disturbance');
  });
});
