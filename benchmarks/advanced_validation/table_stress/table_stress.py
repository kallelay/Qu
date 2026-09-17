# table_stress.py -- Python-side mirror of table_stress.qu's three sub-cases,
# on the SAME dataset.csv (generated once by make_data.py). Loads Qu's own
# dumped result CSVs (table_stress.qu must be run first) and diffs them
# ELEMENTWISE against pandas'/numpy's own computation of the same thing --
# not just eyeballing printed numbers, same discipline as
# ../ml_stress/ml_stress.py.
#
# Run: python benchmarks/advanced_validation/table_stress/table_stress.py

import os
import time

import numpy as np
import pandas as pd

OUT = os.path.dirname(os.path.abspath(__file__))
PRICE_THRESHOLD = 250.0


def path(name):
    return os.path.join(OUT, name)


df = pd.read_csv(path("dataset.csv"))
n = len(df)
print(f"load          : {n} rows, {df.shape[1]} cols")
print()

# =========================== Check 1: group_by_agg ============================
t0 = time.perf_counter()
g = df.groupby("region")["amount"].agg(["mean", "sum", "count"]).sort_index()
t_group = time.perf_counter() - t0
print(f"groupby.agg   : {t_group:.4f} s  ({len(g)} groups)")

qu_g = pd.read_csv(path("group_agg_qu.csv"))
if not (qu_g["region"].tolist() == g.index.tolist()):
    print("  MISMATCH: region label order differs between Qu and pandas!")
else:
    mean_diff = np.abs(qu_g["mean_amount"].to_numpy() - g["mean"].to_numpy())
    sum_diff = np.abs(qu_g["sum_amount"].to_numpy() - g["sum"].to_numpy())
    count_diff = np.abs(qu_g["count_amount"].to_numpy() - g["count"].to_numpy())
    print(f"  region order match: {len(g)}/{len(g)} labels identical, same alphabetical order")
    print(f"  mean_amount  max abs diff = {mean_diff.max():.8f}  (pandas mean range [{g['mean'].min():.4f}, {g['mean'].max():.4f}])")
    print(f"  sum_amount   max abs diff = {sum_diff.max():.8f}  (pandas sum range [{g['sum'].min():.2f}, {g['sum'].max():.2f}])")
    print(f"  count_amount max abs diff = {count_diff.max():.8f}  (total rows = {int(g['count'].sum())})")
    n_mismatch = int(np.sum(mean_diff > 1e-4) + np.sum(sum_diff > 1e-3) + np.sum(count_diff > 0))
    print(f"  mismatches beyond tolerance (Qu's CSV writer rounds to 6 decimals): {n_mismatch}")
print()

# =========================== Check 2: corrcoef =================================
num_cols = ["price", "qty", "amount", "score"]
t0 = time.perf_counter()
R_pd = df[num_cols].corr().to_numpy()
t_corr = time.perf_counter() - t0
R_np = np.corrcoef(df[num_cols].to_numpy(), rowvar=False)
print(f"corr (.corr())      : {t_corr:.4f} s  ({R_pd.shape[0]}x{R_pd.shape[1]} matrix)")
print(f"  pandas .corr() vs numpy.corrcoef max abs diff = {np.abs(R_pd - R_np).max():.2e}  (sanity check, should be ~0)")

qu_R = pd.read_csv(path("corrcoef_qu.csv")).to_numpy()
corr_diff = np.abs(qu_R - R_pd)
print(f"  Qu corrcoef(X) vs pandas .corr() max abs diff = {corr_diff.max():.8f}")
print(f"  pandas .corr() matrix:\n{np.array2string(R_pd, precision=6, suppress_small=True)}")
print(f"  Qu corrcoef(X) matrix:\n{np.array2string(qu_R, precision=6, suppress_small=True)}")
print()

# =========================== Check 3: filter + sort chain ======================
t0 = time.perf_counter()
dff = df[df["price"] > PRICE_THRESHOLD].sort_values("sortkey").reset_index(drop=True)
t_filtsort = time.perf_counter() - t0
print(f"filter+sort   : {t_filtsort:.4f} s  ({len(dff)} rows kept of {n})")

qu_fs = pd.read_csv(path("filter_sort_qu.csv"))
if len(qu_fs) != len(dff):
    print(f"  MISMATCH: row counts differ -- Qu {len(qu_fs)} vs pandas {len(dff)}")
else:
    region_match = int(np.sum(qu_fs["region"].to_numpy() == dff["region"].to_numpy()))
    price_diff = np.abs(qu_fs["price"].to_numpy() - dff["price"].to_numpy())
    qty_diff = np.abs(qu_fs["qty"].to_numpy() - dff["qty"].to_numpy())
    amount_diff = np.abs(qu_fs["amount"].to_numpy() - dff["amount"].to_numpy())
    score_diff = np.abs(qu_fs["score"].to_numpy() - dff["score"].to_numpy())
    sortkey_diff = np.abs(qu_fs["sortkey"].to_numpy() - dff["sortkey"].to_numpy())
    print(f"  row count match     : {len(qu_fs)}/{len(dff)}")
    print(f"  region label match  : {region_match}/{len(dff)} identical, same row order")
    print(f"  price   max abs diff = {price_diff.max():.8f}")
    print(f"  qty     max abs diff = {qty_diff.max():.8f}")
    print(f"  amount  max abs diff = {amount_diff.max():.8f}")
    print(f"  score   max abs diff = {score_diff.max():.8f}")
    print(f"  sortkey max abs diff = {sortkey_diff.max():.8f}  (monotonic check: is_sorted={bool(np.all(np.diff(qu_fs['sortkey'].to_numpy()) >= 0))})")
    n_mismatch = int(
        np.sum(price_diff > 1e-4)
        + np.sum(qty_diff > 1e-4)
        + np.sum(amount_diff > 1e-4)
        + np.sum(score_diff > 1e-4)
        + np.sum(sortkey_diff > 1e-4)
        + (len(dff) - region_match)
    )
    print(f"  total mismatches beyond tolerance (Qu's CSV writer rounds to 6 decimals): {n_mismatch}")
print()

total_pd = t_group + t_corr + t_filtsort
print(f"total (pandas ops only, excludes load): {total_pd:.4f} s")
