"""bench_filter_accuracy.py -- numpy/scipy port of bench_filter_accuracy.qu.

Same exact closed-form ground truth, same explicit frequency axis passed
to sosfreqz rather than trusted to a default (scipy's default integer
worN excludes the Nyquist endpoint -- verified directly before writing
this: sosfreqz(sos, worN=5) stops short of pi, sosfreqz(sos, worN=array)
does not. Passing linspace(0, pi, n_pts) explicitly is what makes this
comparable point-for-point against Qu's freqz(filt, n), which always
includes both 0 and pi.
"""

import time

import numpy as np
from scipy import signal

Fs = 1000.0
fc = 100.0
n_pts = 513
wc = 2.0 * np.pi * fc / Fs
orders = [2, 4, 6, 8, 10, 12, 16, 20]
w = np.linspace(0.0, np.pi, n_pts)

print("order  max_err       rms_err       design+freqz time")
for N in orders:
    t0 = time.perf_counter()
    sos = signal.butter(N, fc, btype="low", fs=Fs, output="sos")
    _, H = signal.sosfreqz(sos, worN=w)
    t = time.perf_counter() - t0

    mag = np.abs(H)
    exact_mag = np.ones_like(w)
    nz = w > 0
    ratio = np.tan(w[nz] / 2.0) / np.tan(wc / 2.0)
    # High order + w near pi sends ratio**(2N) to +inf in float64 -- the
    # limit 1/(1+inf) = 0 is still the mathematically correct answer, so
    # this is silenced rather than avoided.
    with np.errstate(over="ignore"):
        exact_mag[nz] = np.sqrt(1.0 / (1.0 + ratio ** (2 * N)))

    err = np.abs(mag - exact_mag)
    max_err = np.max(err)
    rms_err = np.sqrt(np.mean(err ** 2))

    print(f"{N:2d}     {max_err:.6e}  {rms_err:.6e}  {t:.6f} s")
