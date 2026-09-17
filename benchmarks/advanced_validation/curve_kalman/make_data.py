# make_data.py -- generates every dataset this scenario's Qu and Python
# scripts both read, ONCE, so `curve_fit`/`scipy.optimize.curve_fit` and
# the two Kalman-filter implementations see byte-identical input (same
# convention as ../peak_finding/make_signals.py and ../ml_stress/
# make_dataset.py).
#
# Two sub-scenarios:
#
# 1. Nonlinear curve fitting -- three cases stressing DIFFERENT axes of
#    difficulty for Levenberg-Marquardt:
#      damped_sine_good_init.csv / damped_sine_poor_init*.csv -- same
#        4-parameter damped-sinusoid data (N=1000), the difficulty is in
#        the STARTING GUESS the fitting scripts use (good vs. deliberately
#        far off), not the data itself, so the same CSV serves both.
#      double_exp.csv -- a genuinely harder 5-parameter sum-of-two-
#        exponentials model (A1*exp(-t/tau1) + A2*exp(-t/tau2) + c),
#        N=1000, moderate noise.
#
# 2. Linear Kalman filtering -- a real stress size (N=5,000 steps, 25x
#    catalog/qu_kalman_tracking.qu's N=60 1D toy), 2D constant-velocity
#    target with 2D position-only noisy measurements.
#
# Run once: python benchmarks/advanced_validation/curve_kalman/make_data.py

import numpy as np
import csv
import os

OUT = os.path.dirname(os.path.abspath(__file__))


def write_csv(path, cols):
    names = list(cols.keys())
    n = len(next(iter(cols.values())))
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(names)
        for i in range(n):
            w.writerow([float(cols[name][i]) for name in names])


# =========================== curve fitting data =============================
rng = np.random.RandomState(42)
N1 = 1000
x1 = np.linspace(0, 4, N1)
true_params_sine = [2.0, 1.5, 1.2, 0.3]  # amplitude, decay tau, freq, phase


def damped_sine(x, A, tau, f, phi):
    return A * np.exp(-x / tau) * np.sin(2 * np.pi * f * x + phi)


y1_clean = damped_sine(x1, *true_params_sine)
y1 = y1_clean + 0.08 * rng.randn(N1)
write_csv(os.path.join(OUT, "damped_sine.csv"), {"x": x1, "y": y1})
print(f"damped_sine: N={N1}, true_params={true_params_sine}, noise std=0.08")

rng2 = np.random.RandomState(7)
N2 = 1000
x2 = np.linspace(0, 5, N2)
true_params_exp = [3.0, 0.8, 1.5, 3.0, 0.2]  # A1, tau1, A2, tau2, offset


def double_exp(x, A1, tau1, A2, tau2, c):
    return A1 * np.exp(-x / tau1) + A2 * np.exp(-x / tau2) + c


y2_clean = double_exp(x2, *true_params_exp)
y2 = y2_clean + 0.05 * rng2.randn(N2)
write_csv(os.path.join(OUT, "double_exp.csv"), {"x": x2, "y": y2})
print(f"double_exp: N={N2}, true_params={true_params_exp}, noise std=0.05")

# =========================== Kalman filter data ==============================
# 2D constant-velocity target: state [px, py, vx, vy]. True trajectory is a
# gentle curve (constant-ish velocity plus a small sinusoidal wobble in vy,
# so the constant-velocity MODEL is slightly mismatched to the true motion
# -- a real, not-perfectly-linear tracking scenario, not a scripted straight
# line a constant-velocity filter would fit trivially). Position measured
# every step with 2D Gaussian sensor noise.
rng3 = np.random.RandomState(123)
N3 = 5000
dt = 0.02
t3 = np.arange(N3) * dt
true_vx = 3.0
true_vy = 1.0 + 0.5 * np.sin(0.3 * t3)  # slowly varying vy -- mild model mismatch
true_px = np.cumsum(true_vx * dt * np.ones(N3)) - true_vx * dt
true_py = np.cumsum(true_vy * dt) - true_vy[0] * dt
meas_std = 0.5
meas_px = true_px + meas_std * rng3.randn(N3)
meas_py = true_py + meas_std * rng3.randn(N3)
write_csv(
    os.path.join(OUT, "kalman_track.csv"),
    {"t": t3, "true_px": true_px, "true_py": true_py, "meas_px": meas_px, "meas_py": meas_py},
)
print(f"kalman_track: N={N3}, dt={dt}, measurement noise std={meas_std}")
