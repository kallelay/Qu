"""bench.py -- CSV load + multi-filter benchmark, Python port of bench.qu.

Same dataset (written by bench.qu, or regenerated here if missing), same
five filters, same timing methodology (wall clock around each step only,
not import/setup), for a direct Qu-vs-Python comparison.

Run: python benchmarks/csv_filters/bench.py
"""
import os
import time

import numpy as np
import pandas as pd
from scipy.signal import butter, sosfilt, sosfiltfilt

HERE = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(HERE, "signal.csv")

Fs = 10000
N = 500_000

# --- dataset (untimed prep, matching bench.qu's own setup; regenerated
# every run, same as bench.qu, so neither script depends on run order or a
# leftover file from the other language) ------------------------------------
rng = np.random.default_rng(7)
t = np.arange(N) / Fs
x = (
    np.sin(2 * np.pi * 50 * t)
    + 0.5 * np.sin(2 * np.pi * 500 * t)
    + 0.25 * np.sin(2 * np.pi * 2000 * t)
    + 0.1 * rng.standard_normal(N)
)
pd.DataFrame({"t": t, "x": x}).to_csv(CSV_PATH, index=False)

# --- load ---------------------------------------------------------------
t0 = time.perf_counter()
df = pd.read_csv(CSV_PATH)
t_load = time.perf_counter() - t0
print(f"csv_load          : {t_load:.4f} s  ({len(df)} rows)")

xs = df["x"].to_numpy()

# filter designs (untimed)
lp = butter(4, 200, btype="low", fs=Fs, output="sos")
hp = butter(4, 1000, btype="high", fs=Fs, output="sos")
bp = butter(4, [300, 800], btype="band", fs=Fs, output="sos")
mavg_kernel = np.ones(51) / 51

t0 = time.perf_counter()
y_lp = sosfilt(lp, xs)
t_lp = time.perf_counter() - t0
print(f"butter_lowpass     : {t_lp:.4f} s")

t0 = time.perf_counter()
y_hp = sosfilt(hp, xs)
t_hp = time.perf_counter() - t0
print(f"butter_highpass    : {t_hp:.4f} s")

t0 = time.perf_counter()
y_bp = sosfilt(bp, xs)
t_bp = time.perf_counter() - t0
print(f"butter_bandpass    : {t_bp:.4f} s")

t0 = time.perf_counter()
y_ff = sosfiltfilt(lp, xs)
t_ff = time.perf_counter() - t0
print(f"butter_filtfilt    : {t_ff:.4f} s")

t0 = time.perf_counter()
y_ma = np.convolve(xs, mavg_kernel, mode="same")
t_ma = time.perf_counter() - t0
print(f"moving_average_fir : {t_ma:.4f} s")

total = t_load + t_lp + t_hp + t_bp + t_ff + t_ma
print(f"total              : {total:.4f} s")

print(f"rms(y_lp)          = {np.sqrt(np.mean(y_lp**2)):.4f}")
