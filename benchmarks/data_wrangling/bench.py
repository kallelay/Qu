"""bench.py -- Table/DataFrame operations benchmark, Python (pandas) side.

Same dataset (dataset.csv, built once by generate_data.py) and same six
steps as bench.qu/bench.m: load, filter, groupby+agg, sort, descriptive
stats, correlation matrix. Prints both timings and the actual computed
values so results can be cross-checked against the other two languages,
not just timed.

Run: python benchmarks/data_wrangling/bench.py
"""
import os
import time

import numpy as np
import pandas as pd

HERE = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(HERE, "dataset.csv")

NUM_COLS = ["x1", "x2", "x3", "x4"]

# --- 1. load --------------------------------------------------------------
t0 = time.perf_counter()
df = pd.read_csv(CSV_PATH)
t_load = time.perf_counter() - t0
print(f"load          : {t_load:.4f} s  ({len(df)} rows, {df.shape[1]} cols)")

# --- 2. filter: x1 > 0 -----------------------------------------------------
t0 = time.perf_counter()
filtered = df[df["x1"] > 0]
t_filter = time.perf_counter() - t0
print(f"filter        : {t_filter:.4f} s  ({len(filtered)} rows kept)")

# --- 3. groupby(category) + agg(mean/sum/count of x2) ----------------------
t0 = time.perf_counter()
grouped = df.groupby("category")["x2"].agg(["mean", "sum", "count"])
t_group = time.perf_counter() - t0
grouped_sorted = grouped.sort_index()
print(f"groupby_agg   : {t_group:.4f} s  ({len(grouped)} groups)")

# --- 4. sort by x3 -----------------------------------------------------------
t0 = time.perf_counter()
sorted_df = df.sort_values("x3")
t_sort = time.perf_counter() - t0
print(f"sort_by       : {t_sort:.4f} s")

# --- 5. descriptive stats: describe() on numeric columns --------------------
t0 = time.perf_counter()
desc = df[NUM_COLS].describe()
t_describe = time.perf_counter() - t0
print(f"describe      : {t_describe:.4f} s")

# --- 6. correlation matrix ---------------------------------------------------
t0 = time.perf_counter()
corr = df[NUM_COLS].corr()
t_corr = time.perf_counter() - t0
print(f"corr_matrix   : {t_corr:.4f} s")

total = t_load + t_filter + t_group + t_sort + t_describe + t_corr
print(f"total         : {total:.4f} s")

# --- correctness cross-check printout ---------------------------------------
print()
print("=== correctness cross-check values ===")
print(f"total_rows          = {len(df)}")
print(f"filtered_rows(x1>0) = {len(filtered)}")
print("group_counts (sorted by category):")
for cat, row in grouped_sorted.iterrows():
    print(f"  {cat}: mean={row['mean']:.6f} sum={row['sum']:.6f} count={int(row['count'])}")
print(f"sorted_first_x3     = {sorted_df['x3'].iloc[0]:.6f}")
print(f"sorted_last_x3      = {sorted_df['x3'].iloc[-1]:.6f}")
print(f"sorted_is_monotonic = {bool(sorted_df['x3'].is_monotonic_increasing)}")
print("describe():")
print(desc.to_string())
print("corr matrix:")
print(corr.to_string())
