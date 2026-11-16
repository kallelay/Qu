/** Ready-made simulation environments for the Loop tab: each is a plant
 *  model, a controller that suits it, a setpoint, a sample time, the
 *  tolerance it is expected to meet, and a line on the physics and what to
 *  try. They are ordinary Qu: `ENV_PLANT_EXAMPLES` / `ENV_CONTROLLER_EXAMPLES`
 *  join the plant and controller example lists in loopDesign.ts, and
 *  `environmentSpec` turns one into a LoopSpec the generator writes out
 *  like any other. No React, no DOM.
 *
 *  Linear plants are advanced EXACTLY (zero-order hold through `expm`), so
 *  the answer does not depend on the step size; the nonlinear tank uses RK4.
 *  Every global is prefixed `env_` (plant) or `ctl_` (controller). */

import type { SetpointSpec } from './loopDesign';
import type { PluginInstance } from './loopPlugins';

const num = (v: number): string => (Object.is(v, -0) ? '0' : String(v));

// ---------------------------------------------------------------- plant templates

/** Lines that fill M = [A*dt, B*dt; 0, 0] element by element (`n` the state count).
 *  Block assignment of a matrix into a slice is avoided on purpose: the engine
 *  currently writes 2-D blocks transposed. `scalar`: A and B are plain numbers. */
function blockLoops(A: string, B: string, dt: string, pad: string, scalar: boolean, nName = 'env_n'): string[] {
  if (scalar) return [`${pad}M[0, 0] = ${A} * ${dt}`, `${pad}M[0, 1] = ${B} * ${dt}`];
  return [
    `${pad}for i = 0 to ${nName} - 1`,
    `${pad}    for j = 0 to ${nName} - 1`,
    `${pad}        M[i, j] = ${A}[i, j] * ${dt}`,
    `${pad}    end for`,
    `${pad}    M[i, ${nName}] = ${B}[i] * ${dt}`,
    `${pad}end for`,
  ];
}

interface StateSpace {
  /** Comment lines (without `# `) printed above the model. */
  about: string[];
  /** Qu lines that define the physical parameters. */
  params: string[];
  /** Qu expressions: the A matrix literal (rows separated by `;`) and the B column. */
  A: string;
  B: string;
  n: number;
  /** Index of the measured state (the plant's output y). */
  out: number;
  /** Initial state, a Qu column literal; default all zeros. */
  x0?: string;
  /** A constant force the model sees besides u (gravity): u - offset. */
  uOffset?: string;
  /** Keep the first state at or above 0 (a floor). */
  floor?: boolean;
}

/** A linear plant x' = A x + B u, y = x[out], stepped exactly with expm. */
function statePlant(m: StateSpace): string {
  const n = m.n;
  const last = n - 1;
  const u = m.uOffset ? `(u - ${m.uOffset})` : 'u';
  return [
    ...m.about.map((l) => `# ${l}`),
    '# Stepped exactly (zero-order hold through expm), so the result does not depend on dt.',
    ...m.params,
    `env_A = ${m.A}`,
    `env_B = ${m.B}`,
    `env_n = ${n}`,
    `env_out = ${m.out}`,
    `env_x = ${m.x0 ?? (n === 1 ? '0' : `zeros(${n}, 1)`)}`,
    'env_dt = 0',
    `env_Ad = ${n === 1 ? '1' : `eye(${n})`}`,
    `env_Bd = ${n === 1 ? '0' : `zeros(${n}, 1)`}`,
    'function plant(u, dt)',
    '    global env_x, env_dt, env_Ad, env_Bd',
    '    if dt > 0',
    '        if dt != env_dt',
    '            M = zeros(env_n + 1, env_n + 1)',
    ...blockLoops('env_A', 'env_B', 'dt', '            ', n === 1),
    '            E = expm(M)',
    `            env_Ad = E[0:${last}, 0:${last}]`,
    `            env_Bd = E[0:${last}, ${n}:${n}]`,
    '            env_dt = dt',
    '        end if',
    `        env_x = env_Ad * env_x + env_Bd * ${u}`,
    ...(m.floor ? ['        if env_x[0] < 0', '            env_x = zeros(env_n, 1)', '        end if'] : []),
    '    end if',
    n === 1 ? '    return env_x' : `    return env_x[${m.out}]`,
    'end function',
  ].join('\n');
}

// ---------------------------------------------------------------- controller templates

interface Pid {
  about: string[];
  kp: number;
  ki: number;
  kd: number;
  lo: number;
  hi: number;
  /** Added to the output before limiting (a known constant load, e.g. hover thrust). */
  feedforward?: string;
}

/** PI/PID with the derivative on the measurement (no kick on a setpoint step)
 *  and the integrator frozen while the output is limited (anti-windup). */
function pidController(c: Pid): string {
  const ff = c.feedforward ? ` + ${c.feedforward}` : '';
  return [
    ...c.about.map((l) => `# ${l}`),
    '# Derivative on the measurement; the integrator is frozen while the output is limited.',
    `ctl_kp = ${num(c.kp)}`,
    `ctl_ki = ${num(c.ki)}`,
    `ctl_kd = ${num(c.kd)}`,
    `ctl_lo = ${num(c.lo)}`,
    `ctl_hi = ${num(c.hi)}`,
    'ctl_i = 0',
    'ctl_prev_y = 0',
    'function controller(r, y, dt)',
    '    global ctl_i, ctl_prev_y',
    '    e = r - y',
    '    d = 0',
    '    if dt > 0',
    '        d = -(y - ctl_prev_y) / dt',
    '    end if',
    '    ctl_prev_y = y',
    '    trial = ctl_i + e * dt',
    `    u = ctl_kp * e + ctl_ki * trial + ctl_kd * d${ff}`,
    '    if u > ctl_hi',
    '        u = ctl_hi',
    '    elseif u < ctl_lo',
    '        u = ctl_lo',
    '    else',
    '        ctl_i = trial',
    '    end if',
    '    return u',
    'end function',
  ].join('\n');
}

interface ObserverFeedback {
  about: string[];
  /** Closed-loop poles (1/s, real, distinct). */
  poles: number[];
  /** Observer poles, faster than the closed-loop ones. */
  observer: number[];
  /** Qu lines, inside the controller, that fill `xr` (the state the setpoint r asks for) and `ur` (the input holding it). */
  reference: string[];
}

/** State feedback u = ur - K (xhat - xr) from an observer that sees only y.
 *  K and the observer gain L come from Ackermann's formula in discrete time
 *  at the loop's dt (first call), with the poles mapped through exp(p dt). The
 *  design model is a copy of the plant's own matrices. */
function observerController(c: ObserverFeedback): string {
  const lit = (v: number[]) => `[${v.map(num).join(', ')}]`;
  return [
    ...c.about.map((l) => `# ${l}`),
    '# Design model: a copy of the plant\'s own matrices (edit these two lines to design against a wrong model).',
    'ctl_A = env_A',
    'ctl_B = env_B',
    'ctl_n = env_n',
    `ctl_poles = ${lit(c.poles)}`,
    `ctl_obs = ${lit(c.observer)}`,
    'ctl_dt = 0',
    'ctl_xhat = zeros(ctl_n, 1)',
    'ctl_K = zeros(1, ctl_n)',
    'ctl_L = zeros(ctl_n, 1)',
    'ctl_Ad = eye(ctl_n)',
    'ctl_Bd = zeros(ctl_n, 1)',
    '# Ackermann: the row K with eig(F - G K) = z   (G a column).',
    'function ctl_acker(F, G, z)',
    '    n = length(z)',
    '    c = poly(z)',
    '    Wc = zeros(n, n)',
    '    col = G',
    '    for j = 0 to n - 1',
    '        Wc[0:n - 1, j:j] = col',
    '        col = F * col',
    '    end for',
    '    P = zeros(n, n)',
    '    for j = 0 to n',
    '        P = P * F + c[j] * eye(n)',
    '    end for',
    '    Wi = inv(Wc)',
    '    return Wi[n - 1:n - 1, 0:n - 1] * P',
    'end function',
    'function ctl_design(dt)',
    '    global ctl_K, ctl_L, ctl_Ad, ctl_Bd, ctl_dt',
    '    n = ctl_n',
    '    M = zeros(n + 1, n + 1)',
    ...blockLoops('ctl_A', 'ctl_B', 'dt', '    ', false, 'n'),
    '    E = expm(M)',
    '    ctl_Ad = E[0:n - 1, 0:n - 1]',
    '    ctl_Bd = E[0:n - 1, n:n]',
    '    ctl_K = ctl_acker(ctl_Ad, ctl_Bd, exp(ctl_poles * dt))',
    '    Cc = zeros(n, 1)',
    '    Cc[env_out] = 1',
    '    Lt = ctl_acker(ctl_Ad\', Cc, exp(ctl_obs * dt))',
    '    ctl_L = Lt\'',
    '    ctl_dt = dt',
    'end function',
    'function controller(r, y, dt)',
    '    global ctl_xhat',
    '    if dt != ctl_dt',
    '        ctl_design(dt)',
    '    end if',
    '    xr = zeros(ctl_n, 1)',
    '    ur = 0',
    ...c.reference.map((l) => `    ${l}`),
    '    u = ur',
    '    for j = 0 to ctl_n - 1',
    '        u = u - ctl_K[0, j] * (ctl_xhat[j] - xr[j])',
    '    end for',
    '    innov = y - ctl_xhat[env_out]',
    '    ctl_xhat = ctl_Ad * ctl_xhat + ctl_Bd * u + ctl_L * innov',
    '    return u',
    'end function',
  ].join('\n');
}

// ---------------------------------------------------------------- the environments

export interface EnvExpect {
  /** For t >= step time + settleBy, |y - r| <= band * |r|. */
  band: number;
  settleBy: number;
  /** The plant is unstable without the controller (the gate checks both ways). */
  unstable: boolean;
}

export interface Environment {
  id: string;
  label: string;
  /** One line: the physics. */
  physics: string;
  /** One line: what to try. */
  tryThis: string;
  plant: { id: string; label: string; code: string };
  controller: { id: string; label: string; code: string };
  setpoint: SetpointSpec;
  dt: number;
  duration: number;
  expect: EnvExpect;
  plugins?: PluginInstance[];
}

const motorParams = [
  'env_R = 1        # armature resistance (ohm)',
  'env_L = 0.01     # armature inductance (H)',
  'env_Kt = 0.1     # torque / back-EMF constant (N m/A, V s/rad)',
  'env_J = 0.02     # rotor inertia (kg m^2)',
  'env_b = 0.01     # viscous friction (N m s/rad)',
];

export const ENVIRONMENTS: Environment[] = [
  {
    id: 'motor-speed',
    label: 'DC motor speed (PI)',
    physics: 'Armature circuit (L di/dt = V - R i - Kt w) driving a rotor (J dw/dt = Kt i - b w): about 5 rad/s per volt with a 1 s mechanical lag.',
    tryThis: 'Raise Kp for a faster rise, then add the actuator rate-limit or sensor-noise plugin; lower the 12 V limit and watch the anti-windup hold the integrator.',
    plant: {
      id: 'env-motor-speed',
      label: 'DC motor, speed output (V -> rad/s)',
      code: statePlant({
        about: ['DC motor: states [armature current i, speed w]; input voltage V; output speed w (rad/s).'],
        params: motorParams,
        A: '[-env_R / env_L, -env_Kt / env_L; env_Kt / env_J, -env_b / env_J]',
        B: '[1 / env_L; 0]',
        n: 2,
        out: 1,
      }),
    },
    controller: {
      id: 'env-motor-speed-pi',
      label: 'PI for motor speed (Kp = 1, Ki = 1, |V| <= 12)',
      code: pidController({ about: ['PI for the DC motor speed: Ki/Kp sits on the 1 s mechanical pole, so the loop is close to a pure integrator.'], kp: 1, ki: 1, kd: 0, lo: -12, hi: 12 }),
    },
    setpoint: { kind: 'step', amplitude: 10, time: 0.2 },
    dt: 0.001,
    duration: 3,
    expect: { band: 0.02, settleBy: 1.2, unstable: false },
  },
  {
    id: 'motor-position',
    label: 'DC motor position (PID)',
    physics: 'The same motor with the shaft angle as output: an integrator behind the speed lag, so a position error needs the controller to brake as well as push.',
    tryThis: 'Set Kd to 0 and watch it ring; add a 0.3 V dead zone on the actuator and see the position stall short of the target, then set Ki = 1 and watch it hunt.',
    plant: {
      id: 'env-motor-position',
      label: 'DC motor, angle output (V -> rad)',
      code: statePlant({
        about: ['DC motor: states [armature current i, speed w, angle th]; input voltage V; output angle th (rad).'],
        params: motorParams,
        A: '[-env_R / env_L, -env_Kt / env_L, 0; env_Kt / env_J, -env_b / env_J, 0; 0, 1, 0]',
        B: '[1 / env_L; 0; 0]',
        n: 3,
        out: 2,
      }),
    },
    controller: {
      id: 'env-motor-position-pid',
      label: 'PID for motor angle (Kp = 7, Ki = 0, Kd = 2.2, |V| <= 12)',
      code: pidController({ about: ['PID for the motor angle: Kp sets the stiffness, Kd the damping; Ki is 0 because the plant already integrates (raise it to fight a constant load).'], kp: 7, ki: 0, kd: 2.2, lo: -12, hi: 12 }),
    },
    setpoint: { kind: 'step', amplitude: 1, time: 0.2 },
    dt: 0.001,
    duration: 4,
    expect: { band: 0.02, settleBy: 2, unstable: false },
  },
  {
    id: 'mass-spring-damper',
    label: 'Mass-spring-damper (PID)',
    physics: 'm x\'\' + c x\' + k x = F with m = 2 kg, c = 1.2 N s/m, k = 8 N/m: lightly damped (about 15 %), natural frequency 2 rad/s.',
    tryThis: 'Look at the open-loop ringing first (set Kp = Ki = Kd = 0), then compare with the loop; try a 50 ms sensor delay and raise Kd.',
    plant: {
      id: 'env-msd',
      label: 'Mass-spring-damper (F -> position)',
      code: statePlant({
        about: ['Mass-spring-damper: states [position x, velocity v]; input force F; output x (m).'],
        params: ['env_m = 2      # mass (kg)', 'env_c = 1.2    # damping (N s/m)', 'env_k = 8      # spring (N/m)'],
        A: '[0, 1; -env_k / env_m, -env_c / env_m]',
        B: '[0; 1 / env_m]',
        n: 2,
        out: 0,
      }),
    },
    controller: {
      id: 'env-msd-pid',
      label: 'PID for the mass (Kp = 40, Ki = 40, Kd = 10, |F| <= 100)',
      code: pidController({ about: ['PID for the mass-spring-damper position.'], kp: 40, ki: 40, kd: 10, lo: -100, hi: 100 }),
    },
    setpoint: { kind: 'step', amplitude: 1, time: 0.5 },
    dt: 0.001,
    duration: 6,
    expect: { band: 0.02, settleBy: 3, unstable: false },
  },
  {
    id: 'pendulum',
    label: 'Inverted pendulum on a cart (state feedback)',
    physics: 'Linearised about upright (cart M = 0.5 kg, pendulum m = 0.2 kg, l = 0.3 m): one pole near +5.6 rad/s, so it falls in about half a second without control. Output is the cart position only.',
    tryThis: 'Switch the controller off (return 0) and watch it fall; slow the closed-loop poles or add 20 ms of sensor delay and see how little margin there is.',
    plant: {
      id: 'env-pendulum',
      label: 'Inverted pendulum on a cart, linearised (F -> cart position)',
      code: statePlant({
        about: ['Linearised inverted pendulum on a cart: states [x, x_dot, phi, phi_dot]; input force F; output cart position x.', 'It starts with the pendulum 0.05 rad off upright.'],
        params: [
          'env_M = 0.5     # cart mass (kg)',
          'env_m = 0.2     # pendulum mass (kg)',
          'env_bc = 0.1    # cart friction (N s/m)',
          'env_I = 0.006   # pendulum inertia about its centre (kg m^2)',
          'env_g = 9.8',
          'env_l = 0.3     # pivot to centre of mass (m)',
          'env_p = env_I * (env_M + env_m) + env_M * env_m * env_l ^ 2',
        ],
        A: '[0, 1, 0, 0; 0, -(env_I + env_m * env_l ^ 2) * env_bc / env_p, env_m ^ 2 * env_g * env_l ^ 2 / env_p, 0; 0, 0, 0, 1; 0, -env_m * env_l * env_bc / env_p, env_m * env_g * env_l * (env_M + env_m) / env_p, 0]',
        B: '[0; (env_I + env_m * env_l ^ 2) / env_p; 0; env_m * env_l / env_p]',
        n: 4,
        out: 0,
        x0: '[0; 0; 0.05; 0]',
      }),
    },
    controller: {
      id: 'env-pendulum-sf',
      label: 'Observer + state feedback for the pendulum (poles -3 ... -4.5)',
      code: observerController({
        about: ['Pendulum controller: u = -K (xhat - xref), xhat from an observer that sees only the cart position.'],
        poles: [-3, -3.5, -4, -4.5],
        observer: [-12, -13, -14, -15],
        reference: ['xr[0] = r'],
      }),
    },
    setpoint: { kind: 'step', amplitude: 0.2, time: 1 },
    dt: 0.002,
    duration: 8,
    expect: { band: 0.05, settleBy: 3.5, unstable: true },
  },
  {
    id: 'cruise',
    label: 'Cruise control (PI)',
    physics: 'm v\' = F - b v with m = 1000 kg and linear drag b = 50 N s/m: gain 0.02 (m/s)/N and a 20 s lag; the engine gives -2000 to 4000 N.',
    tryThis: 'Add the step load disturbance (-300 N for a hill) and see the PI recover; add the saturation monitor to count steps at full throttle.',
    plant: {
      id: 'env-cruise',
      label: 'Car on a road (force -> speed)',
      code: statePlant({
        about: ['Cruise control: one state, the speed v (m/s); input drive force F (N).'],
        params: ['env_m = 1000   # car mass (kg)', 'env_b = 50      # linear drag (N s/m)'],
        A: '-env_b / env_m',
        B: '1 / env_m',
        n: 1,
        out: 0,
      }),
    },
    controller: {
      id: 'env-cruise-pi',
      label: 'PI for cruise (Kp = 800, Ki = 200, -2000..4000 N)',
      code: pidController({ about: ['PI for the speed: the integrator is frozen while the engine is at its limit.'], kp: 800, ki: 200, kd: 0, lo: -2000, hi: 4000 }),
    },
    setpoint: { kind: 'step', amplitude: 25, time: 1 },
    dt: 0.01,
    duration: 40,
    expect: { band: 0.02, settleBy: 14, unstable: false },
  },
  {
    id: 'two-tank',
    label: 'Two tanks in series, level control (PID)',
    physics: 'Nonlinear: each outlet flows c sqrt(h), so the plant slows down as the level drops. A pump fills tank 1 (0 to 0.08 m^3/s); tank 1 drains into tank 2, whose level is controlled.',
    tryThis: 'Raise the setpoint to 2 m (the pump saturates and the time constant doubles) or add the pulse load as a leak.',
    plant: {
      id: 'env-two-tank',
      label: 'Two tanks, pump flow -> tank-2 level (nonlinear, RK4)',
      code: [
        '# Two tanks in series: A dh1/dt = q - c sqrt(h1),  A dh2/dt = c sqrt(h1) - c sqrt(h2).',
        '# Input q is the pump flow (clipped to 0..0.08 m^3/s); output is the level h2 (m). RK4 steps.',
        'env_A = 0.05     # tank cross-section (m^2)',
        'env_c = 0.02     # outlet coefficient (m^2.5/s)',
        'env_qmax = 0.08',
        'env_h = zeros(2, 1)',
        'function env_flow(h, q)',
        '    s1 = env_c * sqrt(max(h[0], 0))',
        '    s2 = env_c * sqrt(max(h[1], 0))',
        '    out = zeros(2, 1)',
        '    out[0] = (q - s1) / env_A',
        '    out[1] = (s1 - s2) / env_A',
        '    return out',
        'end function',
        'function plant(u, dt)',
        '    global env_h',
        '    if dt > 0',
        '        q = min(env_qmax, max(0, u))',
        '        k1 = env_flow(env_h, q)',
        '        k2 = env_flow(env_h + k1 * (dt / 2), q)',
        '        k3 = env_flow(env_h + k2 * (dt / 2), q)',
        '        k4 = env_flow(env_h + k3 * dt, q)',
        '        env_h = env_h + (k1 + 2 * k2 + 2 * k3 + k4) * (dt / 6)',
        '        env_h[0] = max(env_h[0], 0)',
        '        env_h[1] = max(env_h[1], 0)',
        '    end if',
        '    return env_h[1]',
        'end function',
      ].join('\n'),
    },
    controller: {
      id: 'env-two-tank-pid',
      label: 'PID for the tank level (Kp = 0.05, Ki = 0.005, Kd = 0.1, 0..0.08)',
      code: pidController({ about: ['PID for the tank-2 level; the output is the pump flow.'], kp: 0.05, ki: 0.005, kd: 0.1, lo: 0, hi: 0.08 }),
    },
    setpoint: { kind: 'step', amplitude: 1, time: 1 },
    dt: 0.05,
    duration: 60,
    expect: { band: 0.02, settleBy: 20, unstable: false },
  },
  {
    id: 'room-thermal',
    label: 'Room heating with a delay (PI)',
    physics: 'C dT/dt = UA (T_out - T) + P with C = 30 kJ/K, UA = 100 W/K (5 min lag) and a 30 s transport delay between the heater command and the room.',
    tryThis: 'Double Kp and watch the delay turn the loop oscillatory; open a window with a step load of -500 W.',
    plant: {
      id: 'env-room',
      label: 'Room with heater delay (W -> deg C)',
      code: [
        '# Room: C dT/dt = UA (T_out - T) + P, heater power P (W) acting after a transport delay.',
        '# Exact step for the lag; the delay is a ring buffer of delay/dt samples. Output: T (deg C).',
        'env_C = 30000    # heat capacity (J/K)',
        'env_UA = 100     # loss to outside (W/K)',
        'env_Tout = 5     # outside temperature (deg C)',
        'env_delay = 30   # heater transport delay (s)',
        'env_T = env_Tout',
        'env_buf = zeros(1)',
        'env_i = 0',
        'env_len = 0',
        'function plant(u, dt)',
        '    global env_T, env_buf, env_i, env_len',
        '    if dt > 0',
        '        if env_len == 0',
        '            env_len = max(1, round(env_delay / dt))',
        '            env_buf = zeros(env_len)',
        '        end if',
        '        slot = env_i mod env_len',
        '        p = env_buf[slot]',
        '        env_buf[slot] = u',
        '        env_i = env_i + 1',
        '        a = exp(-env_UA * dt / env_C)',
        '        env_T = env_Tout + (env_T - env_Tout) * a + (p / env_UA) * (1 - a)',
        '    end if',
        '    return env_T',
        'end function',
      ].join('\n'),
    },
    controller: {
      id: 'env-room-pi',
      label: 'PI for the room (Kp = 100, Ki = 0.6, 0..3000 W)',
      code: pidController({ about: ['PI for the room temperature; the heater is 0 to 3 kW.'], kp: 100, ki: 0.6, kd: 0, lo: 0, hi: 3000 }),
    },
    setpoint: { kind: 'step', amplitude: 20, time: 5 },
    dt: 1,
    duration: 3000,
    expect: { band: 0.02, settleBy: 1500, unstable: false },
  },
  {
    id: 'quad-altitude',
    label: 'Quadcopter altitude, 1-D (PID + hover thrust)',
    physics: 'm z\'\' = T - m g - c z\' with m = 1.2 kg: it falls unless thrust T holds up its weight (11.8 N), and the ground stops it at z = 0.',
    tryThis: 'Change the mass in the plant (say 1.5 kg) without touching the controller: it sags short of the target; set Ki = 3 and the integrator takes over the missing hover thrust.',
    plant: {
      id: 'env-quad',
      label: 'Quadcopter altitude (thrust N -> height m)',
      code: statePlant({
        about: ['Quadcopter, vertical axis only: states [height z, climb rate v]; input total thrust T (N); output z (m).'],
        params: ['env_m = 1.2    # mass (kg)', 'env_g = 9.81', 'env_cd = 0.3   # linear air drag (N s/m)'],
        A: '[0, 1; 0, -env_cd / env_m]',
        B: '[0; 1 / env_m]',
        n: 2,
        out: 0,
        uOffset: 'env_m * env_g',
        floor: true,
      }),
    },
    controller: {
      id: 'env-quad-pid',
      label: 'PID for altitude (Kp = 11, Ki = 0, Kd = 6.5, hover 11.77 N, 0..30 N)',
      code: pidController({ about: ['PD(I) for altitude with the hover thrust (1.2 kg x 9.81) added as feedforward; Ki = 0 until the mass is wrong.'], kp: 11, ki: 0, kd: 6.5, lo: 0, hi: 30, feedforward: '11.772' }),
    },
    setpoint: { kind: 'step', amplitude: 1, time: 0.5 },
    dt: 0.002,
    duration: 6,
    expect: { band: 0.02, settleBy: 3, unstable: false },
  },
  {
    id: 'maglev',
    label: 'Magnetic levitation (state feedback)',
    physics: 'A steel ball under an electromagnet, linearised about a 10 mm gap and 0.5 A: the force grows as the ball nears the coil, giving an unstable pole at +44 rad/s (it falls or sticks in about 20 ms) plus a 40 rad/s coil.',
    tryThis: 'Add 1 ms of sensor delay, then 3 ms, and see the margin go; add sensor noise and watch the coil voltage.',
    plant: {
      id: 'env-maglev',
      label: 'Magnetic levitation, linearised (V -> gap deviation)',
      code: statePlant({
        about: ['Magnetic levitation, deviations from the operating point: states [gap x, speed v, coil current i]; input coil voltage; output x (m).', 'The ball starts 0.5 mm off.'],
        params: [
          'env_g = 9.81',
          'env_x0 = 0.01   # operating gap (m)',
          'env_i0 = 0.5    # operating current (A)',
          'env_R = 2       # coil resistance (ohm)',
          'env_L = 0.05    # coil inductance (H)',
        ],
        A: '[0, 1, 0; 2 * env_g / env_x0, 0, -2 * env_g / env_i0; 0, 0, -env_R / env_L]',
        B: '[0; 0; 1 / env_L]',
        n: 3,
        out: 0,
        x0: '[0.0005; 0; 0]',
      }),
    },
    controller: {
      id: 'env-maglev-sf',
      label: 'Observer + state feedback for the levitated ball',
      code: observerController({
        about: ['Maglev controller: state feedback on [gap, speed, current] estimated from the gap alone; the reference also asks for the coil current that holds the new gap.'],
        poles: [-30, -60, -90],
        observer: [-150, -200, -250],
        reference: ['xr[0] = r', 'xr[2] = r * env_i0 / env_x0', 'ur = env_R * xr[2]'],
      }),
    },
    setpoint: { kind: 'step', amplitude: 0.001, time: 0.1 },
    dt: 0.0005,
    duration: 0.8,
    expect: { band: 0.05, settleBy: 0.25, unstable: true },
  },
];

export const ENV_PLANT_EXAMPLES: Array<{ id: string; label: string; code: string }> = ENVIRONMENTS.map((e) => ({ ...e.plant }));
export const ENV_CONTROLLER_EXAMPLES: Array<{ id: string; label: string; code: string }> = ENVIRONMENTS.map((e) => ({ ...e.controller }));

export const environmentById = (id: string): Environment | undefined => ENVIRONMENTS.find((e) => e.id === id);
