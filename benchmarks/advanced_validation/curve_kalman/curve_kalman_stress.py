# curve_kalman_stress.py -- scipy.optimize.curve_fit (Levenberg-Marquardt,
# method="lm", numerical Jacobian, same algorithm family as Qu's own
# curve_fit) on the SAME damped_sine.csv/double_exp.csv, plus an
# independent from-scratch numpy Kalman filter implementing the identical
# linear-algebra update equations, run on the SAME kalman_track.csv --
# cross-checked against curve_kalman_stress.qu's own dumped estimates.
#
# Run: python benchmarks/advanced_validation/curve_kalman/curve_kalman_stress.py

import os
import time
import numpy as np
import pandas as pd
from scipy.optimize import curve_fit

OUT = os.path.dirname(os.path.abspath(__file__))


def rms(x):
    return float(np.sqrt(np.mean(np.asarray(x) ** 2)))


# =========================== Part 1: curve_fit ===============================
def damped_sine(x, A, tau, f, phi):
    return A * np.exp(-x / tau) * np.sin(2 * np.pi * f * x + phi)


def double_exp(x, A1, tau1, A2, tau2, c):
    return A1 * np.exp(-x / tau1) + A2 * np.exp(-x / tau2) + c


sine_df = pd.read_csv(os.path.join(OUT, "damped_sine.csv"))
xs, ys = sine_df["x"].to_numpy(), sine_df["y"].to_numpy()
true_params_sine = [2.0, 1.5, 1.2, 0.3]

p0_good = [1.0, 1.0, 1.0, 0.0]
t0 = time.perf_counter()
popt_good, _ = curve_fit(damped_sine, xs, ys, p0=p0_good, method="lm", maxfev=300 * (len(p0_good) + 1))
t1 = time.perf_counter()
cost_good = float(np.sum((damped_sine(xs, *popt_good) - ys) ** 2))
print(f"damped_sine (good init):  cost={cost_good:.6f}  time={(t1-t0)*1000:.3f} ms")
print(f"  true_params   = {true_params_sine}")
print(f"  fitted_params = {list(np.round(popt_good, 6))}")

p0_poor = [10.0, 0.1, 5.0, 3.0]
t0 = time.perf_counter()
popt_poor, _ = curve_fit(damped_sine, xs, ys, p0=p0_poor, method="lm", maxfev=300 * (len(p0_poor) + 1))
t1 = time.perf_counter()
cost_poor = float(np.sum((damped_sine(xs, *popt_poor) - ys) ** 2))
print(f"damped_sine (poor init):  cost={cost_poor:.6f}  time={(t1-t0)*1000:.3f} ms")
print(f"  fitted_params = {list(np.round(popt_poor, 6))}")
print()

exp_df = pd.read_csv(os.path.join(OUT, "double_exp.csv"))
xe, ye = exp_df["x"].to_numpy(), exp_df["y"].to_numpy()
true_params_exp = [3.0, 0.8, 1.5, 3.0, 0.2]
p0_exp = [1.0, 1.0, 1.0, 1.0, 0.0]
t0 = time.perf_counter()
popt_exp, _ = curve_fit(double_exp, xe, ye, p0=p0_exp, method="lm", maxfev=500 * (len(p0_exp) + 1))
t1 = time.perf_counter()
cost_exp = float(np.sum((double_exp(xe, *popt_exp) - ye) ** 2))
print(f"double_exp (5 params):    cost={cost_exp:.6f}  time={(t1-t0)*1000:.3f} ms")
print(f"  true_params   = {true_params_exp}")
print(f"  fitted_params = {list(np.round(popt_exp, 6))}")
print()

# =========================== Part 2: Kalman filter ============================
# Independent from-scratch implementation of the SAME linear KF update
# equations Qu's kalman_init/.predict/.update use (x'=Fx, P'=FPF'+Q,
# y=z-Hx, S=HPH'+R, K=PH'S^-1, x=x+Ky, P=(I-KH)P) -- no filterpy/pykalman
# dependency, since the algorithm itself is just linear algebra and this
# keeps the reference implementation fully auditable in this one file.
track = pd.read_csv(os.path.join(OUT, "kalman_track.csv"))
N = len(track)
dt = float(track["t"][1] - track["t"][0])
meas_std = 0.5

F = np.array([[1, 0, dt, 0], [0, 1, 0, dt], [0, 0, 1, 0], [0, 0, 0, 1]])
Q = np.diag([0.001, 0.001, 0.01, 0.01])
H = np.array([[1, 0, 0, 0], [0, 1, 0, 0]])
R = np.diag([meas_std**2, meas_std**2])
I4 = np.eye(4)

x = np.zeros(4)
P = np.diag([10.0, 10.0, 10.0, 10.0])
est_px = np.zeros(N)
est_py = np.zeros(N)

meas_px = track["meas_px"].to_numpy()
meas_py = track["meas_py"].to_numpy()

t0 = time.perf_counter()
for k in range(N):
    # predict
    x = F @ x
    P = F @ P @ F.T + Q
    # update
    z = np.array([meas_px[k], meas_py[k]])
    y = z - H @ x
    S = H @ P @ H.T + R
    K = P @ H.T @ np.linalg.inv(S)
    x = x + K @ y
    P = (I4 - K @ H) @ P
    est_px[k] = x[0]
    est_py[k] = x[1]
t1 = time.perf_counter()
t_kalman = t1 - t0

true_px = track["true_px"].to_numpy()
true_py = track["true_py"].to_numpy()
raw_err = rms(meas_px - true_px) + rms(meas_py - true_py)
filt_err = rms(est_px - true_px) + rms(est_py - true_py)
print(f"kalman: N={N} steps  time={t_kalman:.4f} s  per_step={1000*t_kalman/N:.4f} ms")
print(f"kalman: raw sensor RMS error (x+y) = {raw_err:.4f}")
print(f"kalman: filtered RMS error (x+y)   = {filt_err:.4f}")

# Cross-check against Qu's own dumped estimates -- elementwise, not just
# "both produced a lower error than raw."
qu_path = os.path.join(OUT, "kalman_qu_estimates.csv")
if os.path.exists(qu_path):
    qu_est = pd.read_csv(qu_path)
    max_diff_px = float(np.max(np.abs(qu_est["qu_est_px"].to_numpy() - est_px)))
    max_diff_py = float(np.max(np.abs(qu_est["qu_est_py"].to_numpy() - est_py)))
    print(f"kalman: max|Qu - numpy| over all {N} steps: px={max_diff_px:.3e}  py={max_diff_py:.3e}")
else:
    print("kalman: (run curve_kalman_stress.qu first for the Qu-vs-numpy elementwise diff)")
