# make_signals.py -- generates every signal this scenario's Qu and Python
# scripts both read, ONCE, so `findpeaks`/`find_peaks` see byte-identical
# input on both sides (an accuracy comparison against two independently-
# drawn random signals would be meaningless -- same convention as
# ../mlp/make_dataset.py / ../../shallow_ml/make_dataset.py).
#
# Six CSVs (single column "x"), each stressing a different aspect of
# peak-finding correctness:
#   low_noise.csv     -- 3-tone signal, N=200,000, small noise (SNR high)
#   medium_noise.csv  -- same tones, moderate noise
#   high_noise.csv    -- same tones, heavy noise (SNR low, spurious local
#                        maxima expected -- stresses min_peak_height/
#                        min_peak_distance actually doing their job)
#   close_peaks.csv   -- deterministic Gaussian bumps at KNOWN locations,
#                        some closer than a chosen min_peak_distance, one
#                        pair placed EXACTLY at the threshold (inclusive-
#                        boundary check), 30 total bumps for a real
#                        distance-suppression stress, not a 2-bump toy
#   ambiguous_pair.csv -- two nearly-equal-height peaks close together
#                        (height differs by 1e-6) -- a genuinely ambiguous
#                        tie for min_peak_distance's greedy-by-height rule
#   plateau.csv        -- a flat-topped (2-sample plateau) peak: known to
#                        separate strict-local-maximum peak finders (Qu's
#                        `find_peaks`, x[i] > both neighbors, STRICT) from
#                        plateau-aware ones (scipy's `find_peaks`, which
#                        reports the plateau's midpoint) -- included
#                        deliberately to surface and document that
#                        difference rather than let it hide.
#
# Run once: python benchmarks/advanced_validation/peak_finding/make_signals.py

import numpy as np
import csv
import os

OUT = os.path.dirname(os.path.abspath(__file__))


def write_csv(path, x):
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["x"])
        for v in x:
            w.writerow([float(v)])


# --- three-tone signal, shared shape across noise levels -------------------
Fs = 1000.0
N = 200_000
t = np.arange(N) / Fs
clean = (
    1.0 * np.sin(2 * np.pi * 5 * t)
    + 0.6 * np.sin(2 * np.pi * 17 * t + 0.3)
    + 0.3 * np.sin(2 * np.pi * 41 * t)
)

rng = np.random.RandomState(42)
low = clean + 0.05 * rng.randn(N)
rng = np.random.RandomState(42)  # same noise DRAW shape, different scale --
medium = clean + 0.30 * rng.randn(N)  # deliberately independent RandomState
rng = np.random.RandomState(42)  # per case (not a running stream) so each
high = clean + 0.80 * rng.randn(N)  # case is reproducible on its own.

write_csv(os.path.join(OUT, "low_noise.csv"), low)
write_csv(os.path.join(OUT, "medium_noise.csv"), medium)
write_csv(os.path.join(OUT, "high_noise.csv"), high)
print(f"low/medium/high_noise: N={N}, Fs={Fs}, tones=5/17/41 Hz, noise std=0.05/0.30/0.80")

# --- close_peaks: deterministic Gaussian bumps at known centers ------------
# 15 CLUSTERS of two bumps each, alternating a "too close" pair (3 samples
# apart -- strictly below the min_peak_distance=5 both scripts use, so only
# the taller bump of the pair should survive) and an "exactly at threshold"
# pair (5 samples apart -- the inclusive boundary; both should survive,
# per Qu's own `find_peaks_min_distance_keeps_the_tallest_of_a_close_
# cluster` unit test, which explicitly checks "exactly at the required
# distance ... both must still be kept"). Clusters are 60 samples apart so
# they never interact with each other. Expected peak count after distance=5
# suppression: 7 "too close" clusters -> 1 survivor each, 8 "at threshold"
# clusters -> 2 survivors each = 7*1 + 8*2 = 23 (out of 30 raw bumps).
n2 = 4000
x2 = np.zeros(n2)
centers = []
heights = []
cluster_start = 200
rng2 = np.random.RandomState(7)
n_too_close_survivors = 0
n_at_threshold_survivors = 0
for i in range(15):
    sep = 3 if i % 2 == 0 else 5  # alternate: too-close / exactly-at-threshold
    h1 = 1.0 + 0.5 * rng2.rand()
    h2 = 1.0 + 0.5 * rng2.rand()
    c1, c2 = cluster_start, cluster_start + sep
    centers += [c1, c2]
    heights += [h1, h2]
    x2 += h1 * np.exp(-0.5 * ((np.arange(n2) - c1) / 1.2) ** 2)
    x2 += h2 * np.exp(-0.5 * ((np.arange(n2) - c2) / 1.2) ** 2)
    if sep == 3:
        n_too_close_survivors += 1
    else:
        n_at_threshold_survivors += 2
    cluster_start += 60
write_csv(os.path.join(OUT, "close_peaks.csv"), x2)
expected = n_too_close_survivors + n_at_threshold_survivors
print(
    f"close_peaks: N={n2}, 15 clusters (30 raw bumps), "
    f"expected survivors after distance=5 suppression = {expected}"
)

# --- ambiguous_pair: two nearly-equal-height peaks, close together --------
n3 = 500
x3 = np.zeros(n3)
c1, c2 = 200, 204  # 4 samples apart
h1, h2 = 1.000000, 1.000001  # differ by 1e-6 -- a genuine near-tie
x3 += h1 * np.exp(-0.5 * ((np.arange(n3) - c1) / 1.5) ** 2)
x3 += h2 * np.exp(-0.5 * ((np.arange(n3) - c2) / 1.5) ** 2)
write_csv(os.path.join(OUT, "ambiguous_pair.csv"), x3)
print(f"ambiguous_pair: N={n3}, peaks at {c1} (h={h1}) and {c2} (h={h2}), 4 samples apart")

# --- plateau: a flat-topped peak (2 equal-height samples at the top) ------
n4 = 200
x4 = np.array([0.0] * 90 + [0.0, 0.3, 0.7, 1.0, 1.0, 0.7, 0.3, 0.0] + [0.0] * 102)
assert len(x4) == n4
write_csv(os.path.join(OUT, "plateau.csv"), x4)
print(f"plateau: N={n4}, flat top at indices 93-94 (both value 1.0)")
