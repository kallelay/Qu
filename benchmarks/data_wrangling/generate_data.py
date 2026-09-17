"""generate_data.py -- builds the ONE shared dataset used by bench.qu,
bench.py, and bench.m.

Unlike csv_filters/full_pipeline (where each language's script regenerates
its own copy from the same seed/formula), this scenario is explicitly about
Table/DataFrame operations across three different CSV readers/engines, so
correctness only means something if all three load *the exact same bytes*.
Run this once; it writes dataset.csv (gitignored, deterministic from the
seed below, so nothing is lost by not tracking it).

Run: python benchmarks/data_wrangling/generate_data.py
"""
import os

import numpy as np
import pandas as pd

HERE = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(HERE, "dataset.csv")

N = 1_000_000
N_GROUPS = 8

rng = np.random.default_rng(42)

category = np.array([f"grp_{i}" for i in rng.integers(0, N_GROUPS, size=N)])
x1 = rng.standard_normal(N)
# x2 deliberately correlated with x1 (~0.7) so the correlation-matrix
# cross-check has a non-trivial off-diagonal value to verify, not just a
# near-zero number that would "match" even if the computation were wrong.
x2 = 0.7 * x1 + np.sqrt(1 - 0.7**2) * rng.standard_normal(N)
x3 = rng.uniform(-10.0, 10.0, size=N)
x4 = rng.exponential(scale=2.0, size=N)

df = pd.DataFrame({"category": category, "x1": x1, "x2": x2, "x3": x3, "x4": x4})
df.to_csv(CSV_PATH, index=False, float_format="%.6f")

size_mb = os.path.getsize(CSV_PATH) / (1024 * 1024)
print(f"wrote {CSV_PATH}: {N} rows x {df.shape[1]} cols, {size_mb:.1f} MB")
print(f"true corr(x1,x2) target: 0.7 (measured: {np.corrcoef(x1, x2)[0,1]:.4f})")
