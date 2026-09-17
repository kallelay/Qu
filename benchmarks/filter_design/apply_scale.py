"""apply_scale.py -- filter APPLICATION at large scale, Python port of
apply_scale.qu. Same Butterworth lowpass (order 4, 500 Hz, Fs=10 kHz), same
1M/10M sample scales, same sosfilt (causal) / filtfilt (zero-phase) pair.

Run: python benchmarks/filter_design/apply_scale.py
"""
import time

import numpy as np
from scipy.signal import butter, sosfilt, sosfiltfilt

Fs = 10000
lp = butter(4, 500, btype="low", fs=Fs, output="sos")


def run_scale(N, seed_val):
    t = np.linspace(0, (N - 1) / Fs, N)
    rng = np.random.default_rng(seed_val)
    x = (
        np.sin(2 * np.pi * 50 * t)
        + 0.5 * np.sin(2 * np.pi * 500 * t)
        + 0.25 * np.sin(2 * np.pi * 2000 * t)
        + 0.1 * rng.standard_normal(N)
    )

    t0 = time.perf_counter()
    y_sos = sosfilt(lp, x)
    t_sos = time.perf_counter() - t0
    print(f"sosfilt   N={N}  : {t_sos:.4f} s")

    t0 = time.perf_counter()
    y_ff = sosfiltfilt(lp, x)
    t_ff = time.perf_counter() - t0
    print(f"filtfilt  N={N}  : {t_ff:.4f} s")

    print(f"rms(y_sos) = {np.sqrt(np.mean(y_sos**2)):.4f}")


run_scale(1_000_000, 21)
run_scale(10_000_000, 22)
