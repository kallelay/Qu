# make_data.py -- generates the ONE noisy multi-tone signal both
# filter_stress.qu and filter_stress.py read byte-identically (same
# convention as ../peak_finding/make_signals.py / ../curve_kalman/make_data.py
# -- a numerical accuracy comparison is only meaningful if both sides start
# from the exact same input, never two independent random draws).
#
# Scenario: a three-tone signal (5 Hz "slow drift", 50 Hz "mains-like
# interference" -- the tone we specifically need to REMOVE, and 120 Hz
# "high-frequency content" we need to KEEP) plus additive Gaussian
# measurement noise. fs and N are chosen so all three tones land on exact
# FFT bins (df = fs/N = 0.25 Hz; 5/0.25=20, 50/0.25=200, 120/0.25=480 --
# all integers), so the later FFT sanity-check isn't confounded by spectral
# leakage from a non-integer number of cycles.
#
# Run once: python benchmarks/advanced_validation/filter_stress/make_data.py

import os
import csv
import numpy as np

OUT = os.path.dirname(os.path.abspath(__file__))

Fs = 2000.0
N = 8000  # 4 seconds @ 2000 Hz -- "a few thousand to ~20k" stress size

t = np.arange(N) / Fs
clean = (
    1.0 * np.sin(2 * np.pi * 5 * t)
    + 0.8 * np.sin(2 * np.pi * 50 * t + 0.4)
    + 0.5 * np.sin(2 * np.pi * 120 * t + 1.1)
)

rng = np.random.RandomState(42)
noise_std = 0.35
noisy = clean + noise_std * rng.randn(N)

path = os.path.join(OUT, "noisy_signal.csv")
with open(path, "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["x"])
    for v in noisy:
        w.writerow([float(v)])

print(f"noisy_signal.csv: N={N}, Fs={Fs} Hz, tones=5/50/120 Hz (amps 1.0/0.8/0.5), "
      f"noise_std={noise_std}, seed=42")
print("target: remove the 50 Hz tone with a Butterworth band-stop filter, "
      "keep 5 Hz and 120 Hz.")
