# The Loop tab: environments and plugins

The **Loop** tab of Qu Studio runs a plant and a controller against each other,
in software (SIL, on a virtual clock) or paced against the wall clock (HIL).
Like the DSP filter designer and the Fit tab, it is a front end to ordinary Qu
code: whatever you click is translated into one readable script that you can
see, copy, open in the editor, edit and re-run. The panel never computes
anything itself; it runs the script and draws the lines the script prints.

The contract is two plain functions: `plant(u, dt)` advances the plant by `dt`
under the input `u` and returns the measured output `y`, and
`controller(r, y, dt)` returns the command `u` for setpoint `r` and measurement
`y`. State lives in globals, like `static` variables in a C controller.

## Ready-made environments

The **Environments...** picker on the left loads a complete setup: a plant model,
a controller that suits it, a setpoint, a sample time and a run length. Under the
picker a card says what the environment models, what to try, and the tolerance it
is expected to meet (the engine test checks each one).

| Environment | Physics | Controller | Expected |
|---|---|---|---|
| DC motor speed | armature circuit driving a rotor, about 5 rad/s per volt, 1 s lag | PI, 12 V limit, anti-windup | 2 % band within 1.2 s |
| DC motor position | the same motor, shaft angle as output | PID (Ki = 0: the plant integrates) | 2 % within 2 s |
| Mass-spring-damper | m = 2, c = 1.2, k = 8: lightly damped, 2 rad/s | PID, 100 N limit | 2 % within 3 s |
| Inverted pendulum on a cart | linearised, unstable pole near +5.6 rad/s | observer + state feedback from the cart position | 5 % within 3.5 s, falls without it |
| Cruise control | 1000 kg car, linear drag, 20 s lag, -2000 to 4000 N | PI, anti-windup | 2 % within 14 s |
| Two tanks in series | nonlinear outlets (c sqrt(h)), pump 0 to 0.08 m^3/s | PID on the tank-2 level | 2 % within 20 s |
| Room heating with a delay | 5 min lag, 30 s heater transport delay | PI, 0 to 3 kW | 2 % within 1500 s |
| Quadcopter altitude (1-D) | gravity, drag, a floor at z = 0 | PD(I) with hover-thrust feedforward | 2 % within 3 s |
| Magnetic levitation | linearised, unstable pole at +44 rad/s, 40 rad/s coil | observer + state feedback from the gap | 5 % within 0.25 s, falls without it |

The linear plants are advanced exactly (zero-order hold through `expm`), so the
answer does not depend on the step size; the tank uses RK4 and the room's delay
is a ring buffer. The two observer-based controllers design their gains with
Ackermann's formula at the loop's step (the engine has no `lqr`), from a copy of
the plant's own matrices that you can edit to design against a wrong model.
Every global is prefixed `env_` (plant) or `ctl_` (controller).

## Plugins

A plugin is a small, named Qu snippet that hooks the loop at one defined point.
The **Plugins** section lists them in order; each can be switched off (its
settings stay), moved up or down, removed, and has its parameters edited in the
panel. The parameters are written into the code as `lpN_name = value` lines, and
each plugin appears in the script as a commented block:

```
# ---- plugin 2: Sensor delay (n samples) (sensor hook: lp2_sensor(y, t, dt) -> ...) ----
```

`N` is the plugin's position in the list. Every name a plugin defines starts with
`lpN_`, so two plugins (or two copies of one, each with its own delay line) never
share state and none can shadow a builtin.

| Hook | Function | Applied |
|---|---|---|
| reference shaper | `lpN_reference(r, t, dt)` returns r' | right after the setpoint |
| sensor | `lpN_sensor(y, t, dt)` returns y' | before the controller sees y |
| monitor | `lpN_monitor(t, r, y, u, e)` returns 0 or 1 | after the step is logged; 1 stops the run |
| actuator | `lpN_actuator(u, t, dt)` returns u' | between the controller and the plant |
| disturbance | `lpN_disturbance(t, dt)` returns d | added to the plant input u' |

Within a hook, plugins run in list order. The scope and the trace record what the
controller saw: `r` is the shaped setpoint, `y` the measurement after the sensor
plugins, `u` the controller's command (before the actuator plugins) and `e = r - y`.
A monitor sees exactly those values.

Built in: sensor noise (seeded Gaussian), sensor delay (n samples), sensor
quantisation (ADC bits), actuator saturation, rate limit and dead zone, step,
pulse and sine load disturbance, reference low-pass and rate limit, a metrics
recorder (overshoot, settling time, steady-state error, IAE), a saturation / windup
monitor, and a watchdog that aborts the run on `|e| > limit`. Monitors that print a
summary do it after the run as `@plugin ...` lines in the output box. A monitor that
returns 1 raises an error tagged `lp_abort`; the script catches only that one and
carries on to print its statistics, so any other error still surfaces unchanged.

### Write your own

**Add plugin / Write your own** gives a text box with a starting snippet for the
chosen hook. Define a function named `hook` with the arguments in the table above
(a monitor may also define `report()` to print after the run); the generator writes
it into the script as `lpN_<hook>` so it cannot clash. Keep any state in globals
whose names start with `lpN_`. **Check by running** defines the plugin in a
throwaway interpreter and calls the hook on a few test values; an error comes back
in the engine's own words. A plugin cannot import files or start timers.

## Worked example

Cruise control with a hill: the **Cruise control** environment plus a step load of
-300 N at 20 s and the metrics recorder. This is the script the panel generates
(run with `qu run`: 4000 steps, y ends at 24.9998 m/s, overshoot 0.48 %, settling
8.87 s; after the hill the PI holds 1550 N, which is 50 N s/m x 25 m/s + 300 N):

```qu
# Loop, SIL: virtual clock, as fast as the machine can go.
# Generated by Qu Studio (Loop tab). Lines starting with @ feed its scope.
duration = 40
dt = 0.01

# ---- plant: Car on a road (force -> speed) ----
# Cruise control: one state, the speed v (m/s); input drive force F (N).
# Stepped exactly (zero-order hold through expm), so the result does not depend on dt.
env_m = 1000   # car mass (kg)
env_b = 50      # linear drag (N s/m)
env_A = -env_b / env_m
env_B = 1 / env_m
env_n = 1
env_out = 0
env_x = 0
env_dt = 0
env_Ad = 1
env_Bd = 0
function plant(u, dt)
    global env_x, env_dt, env_Ad, env_Bd
    if dt > 0
        if dt != env_dt
            M = zeros(env_n + 1, env_n + 1)
            M[0, 0] = env_A * dt
            M[0, 1] = env_B * dt
            E = expm(M)
            env_Ad = E[0:0, 0:0]
            env_Bd = E[0:0, 1:1]
            env_dt = dt
        end if
        env_x = env_Ad * env_x + env_Bd * u
    end if
    return env_x
end function

# ---- controller: PI for cruise (Kp = 800, Ki = 200, -2000..4000 N) ----
# PI for the speed: the integrator is frozen while the engine is at its limit.
# Derivative on the measurement; the integrator is frozen while the output is limited.
ctl_kp = 800
ctl_ki = 200
ctl_kd = 0
ctl_lo = -2000
ctl_hi = 4000
ctl_i = 0
ctl_prev_y = 0
function controller(r, y, dt)
    global ctl_i, ctl_prev_y
    e = r - y
    d = 0
    if dt > 0
        d = -(y - ctl_prev_y) / dt
    end if
    ctl_prev_y = y
    trial = ctl_i + e * dt
    u = ctl_kp * e + ctl_ki * trial + ctl_kd * d
    if u > ctl_hi
        u = ctl_hi
    elseif u < ctl_lo
        u = ctl_lo
    else
        ctl_i = trial
    end if
    return u
end function

# ---- plugin 1: Load disturbance: step (disturbance hook: lp1_disturbance(t, dt) -> the extra input d added at the plant) ----
# Adds a constant d to the plant input from time `at` on.
lp1_amp = -300
lp1_at = 20
function lp1_disturbance(t, dt)
    if t >= lp1_at
        return lp1_amp
    end if
    return 0
end function

# ---- plugin 2: Metrics recorder (overshoot, settling time) (monitor hook: lp2_monitor(t, r, y, u, e) -> 0 to carry on, 1 to stop the run) ----
# Records overshoot %, settling time after the last setpoint change, steady-state error and IAE; prints them after the run.
lp2_band = 0.02
lp2_ymax = -inf
lp2_r_last = 0
lp2_r_change = 0
lp2_out_t = 0
lp2_iae = 0
lp2_y_last = 0
function lp2_monitor(t, r, y, u, e)
    global lp2_ymax, lp2_r_last, lp2_r_change, lp2_out_t, lp2_iae, lp2_y_last
    if abs(r - lp2_r_last) > 1e-12
        lp2_r_change = t
        lp2_out_t = t
        lp2_ymax = -inf
    end if
    lp2_r_last = r
    lp2_ymax = max(lp2_ymax, y)
    if abs(e) > lp2_band * max(abs(r), 1e-12)
        lp2_out_t = t
    end if
    lp2_iae = lp2_iae + abs(e) * dt
    lp2_y_last = y
    return 0
end function
function lp2_report()
    over = 0
    if abs(lp2_r_last) > 1e-12
        over = 100 * max(0, (lp2_ymax - lp2_r_last) / abs(lp2_r_last))
    end if
    print("@plugin metrics overshoot_pct={over:.12g} settle_s={lp2_out_t - lp2_r_change:.12g} steady_err={lp2_r_last - lp2_y_last:.12g} iae={lp2_iae:.12g}")
end function

# ---- setpoint ----
function setpoint(t)
    if t >= 1
        return 25
    end if
    return 0
end function

# ---- the loop: every dt, sample y, compute u, advance the plant ----
n_max = floor(duration / dt) + 2
log_t = zeros(n_max)
log_r = zeros(n_max)
log_y = zeros(n_max)
log_u = zeros(n_max)
log_e = zeros(n_max)
k = 0
y_true = plant(0, 0)
every dt do
    k = k + 1
    t = k * dt
    r = setpoint(t)
    y = y_true
    err = r - y
    u = controller(r, y, dt)
    log_t[k - 1] = t
    log_r[k - 1] = r
    log_y[k - 1] = y
    log_u[k - 1] = u
    log_e[k - 1] = err
    if k mod 1 == 0
        print("@loop k={k} t={t} r={r} y={y} u={u}")
    end if
    if lp2_monitor(t, r, y, u, err) != 0
        error("lp_abort: plugin 2 (Metrics recorder (overshoot, settling time)) stopped the run at t={t}, step {k}")
    end if
    u_act = u
    u_act = u_act + lp1_disturbance(t, dt)
    y_true = plant(u_act, dt)
end

lp_aborted = 0
try
    run_for(duration)
catch lp_err
    if contains(lp_err.message, "lp_abort:")
        lp_aborted = 1
        print("run stopped by a monitor: " + lp_err.message)
    else
        error(lp_err.message)
    end if
end
log_t = log_t[0:k - 1]
log_r = log_r[0:k - 1]
log_y = log_y[0:k - 1]
log_u = log_u[0:k - 1]
log_e = log_e[0:k - 1]
if lp_aborted == 0
    st = run_for_stats()
    print("@stats steps={st.steps} overruns={st.overruns} max_ms={st.max_lateness_ms} mean_ms={st.mean_lateness_ms} wall_ms={st.wall_ms} paced={st.paced * 1}")
else
    print("@stats steps={k} overruns=0 max_ms=0 mean_ms=0 wall_ms=0 paced=0")
end if
lp2_report()
```

Everything above can be edited and re-run in the Code tab: change `ctl_kp`,
`ctl_ki`, the plugin parameters, or add another `lpN_` block by hand.
