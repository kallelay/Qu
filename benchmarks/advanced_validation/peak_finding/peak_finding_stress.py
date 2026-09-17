# peak_finding_stress.py -- scipy.signal.find_peaks on the SAME six CSVs
# peak_finding_stress.qu reads (generated once by make_signals.py), same
# height/distance parameters passed to both, so this is a real accuracy +
# speed cross-check, not two independent random draws.
#
# Run: python benchmarks/advanced_validation/peak_finding/peak_finding_stress.py

import os
import time
import numpy as np
import pandas as pd
from scipy.signal import find_peaks

OUT = os.path.dirname(os.path.abspath(__file__))


def load(name):
    return pd.read_csv(os.path.join(OUT, name))["x"].to_numpy()


def report(name, locs, heights, t):
    print(f"{name}: n_peaks={len(locs)}  time={t*1000:.3f} ms")
    print(f"  locations (first 10) = {list(locs[:10])}")
    print(f"  heights   (first 10) = {[round(float(h), 6) for h in heights[:10]]}")


total_t0 = time.perf_counter()

# --- low/medium/high noise: SAME detector settings across all three,
# height=0.5, distance=10, so any degradation with SNR is due to the
# signal/noise itself, not re-tuned thresholds. -----------------------------
for name in ("low_noise", "medium_noise", "high_noise"):
    x = load(f"{name}.csv")
    t0 = time.perf_counter()
    locs, props = find_peaks(x, height=0.5, distance=10)
    t1 = time.perf_counter()
    report(name, locs, props["peak_heights"], t1 - t0)

# --- close_peaks: distance=5 -- 3-sample-apart bumps must be suppressed
# (keeping the taller of each pair), 5-sample-apart (at the threshold) must
# both survive. ---------------------------------------------------------
x = load("close_peaks.csv")
t0 = time.perf_counter()
locs, props = find_peaks(x, distance=5)
t1 = time.perf_counter()
report("close_peaks (distance=5)", locs, x[locs], t1 - t0)

# --- ambiguous_pair: distance=6 forces exactly one of the two near-tied
# peaks (heights 1.000000 vs 1.000001, 4 samples apart) to be dropped. -----
x = load("ambiguous_pair.csv")
t0 = time.perf_counter()
locs, props = find_peaks(x, distance=6)
t1 = time.perf_counter()
report("ambiguous_pair (distance=6)", locs, x[locs], t1 - t0)

# --- plateau: no filters -- does a flat-topped (2-sample) peak get found,
# and where does the reported location land? -------------------------------
x = load("plateau.csv")
t0 = time.perf_counter()
locs, props = find_peaks(x)
t1 = time.perf_counter()
report("plateau (no filters)", locs, x[locs], t1 - t0)

total_t1 = time.perf_counter()
print(f"\ntotal_time_s={total_t1 - total_t0:.4f}")
