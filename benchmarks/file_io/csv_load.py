"""csv_load.py -- large multi-column CSV load benchmark, Python side.

Same shape as csv_load.qu (3,000,000 rows x 6 numeric columns), own RNG
stream (not bit-identical values, same statistics) -- pandas.read_csv is
the thing being timed.

Run: python benchmarks/file_io/csv_load.py
"""
import time
import numpy as np
import pandas as pd

N = 3_000_000
path = "benchmarks/file_io/py_wide_data.csv"

rng = np.random.default_rng(3)
df_out = pd.DataFrame({
    "c1": rng.standard_normal(N),
    "c2": rng.standard_normal(N),
    "c3": rng.standard_normal(N),
    "c4": rng.standard_normal(N),
    "c5": rng.standard_normal(N),
    "c6": rng.standard_normal(N),
})
df_out.to_csv(path, index=False)

t0 = time.perf_counter()
df = pd.read_csv(path)
t_load = time.perf_counter() - t0
print(f"pandas.read_csv ({N} rows x 6 cols) : {t_load:.4f} s")

print(f"mean(c1) = {df['c1'].mean():.6f}")
print(f"mean(c6) = {df['c6'].mean():.6f}")
print(f"nrow = {len(df)}  ncol = {df.shape[1]}")
