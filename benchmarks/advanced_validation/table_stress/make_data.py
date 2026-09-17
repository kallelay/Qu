# make_data.py -- generates the ONE shared dataset table_stress.qu and
# table_stress.py both read (identically, no per-language regeneration),
# same convention as ../ml_stress/make_dataset.py.
#
# A realistic-shaped tabular-analytics table: 30,000 rows, one categorical
# grouping column (15 regions, unevenly weighted like real store/region
# data), and four numeric columns with real linear correlation structure
# built in (amount depends on price+qty+noise, score depends mildly on
# price+qty+noise) so `corrcoef` has non-trivial, non-near-zero numbers to
# get right, not just noise. `sortkey` is an independent continuous
# Uniform(0, 1000) draw with no ties at double precision, used only for the
# filter+sort chain so row order is unambiguous.
#
# Run once: python benchmarks/advanced_validation/table_stress/make_data.py

import csv
import os

import numpy as np

OUT = os.path.dirname(os.path.abspath(__file__))

N = 30000
N_REGIONS = 15
SEED = 42

rng = np.random.default_rng(SEED)

regions = [f"R{i:02d}" for i in range(N_REGIONS)]
# Unevenly weighted (a Dirichlet draw, fixed seed) -- real region/store
# tables never have perfectly even group sizes.
weights = rng.dirichlet(np.ones(N_REGIONS) * 4.0)
region = rng.choice(regions, size=N, p=weights)

price = rng.uniform(5.0, 500.0, N)
qty = rng.integers(1, 50, N).astype(np.float64)

# amount correlates with both price and qty (plus noise) -- a believable
# "amount = unit_price-ish combination + noise" relationship.
noise_amount = rng.normal(0.0, 20.0, N)
amount = 0.6 * price + 2.0 * qty + noise_amount

# score correlates mildly with price/qty (smaller coefficients, larger
# relative noise) -- gives corrcoef a mix of strong and weak correlations
# to get right, not just one obviously-correlated pair.
noise_score = rng.normal(0.0, 15.0, N)
score = 0.05 * price + 0.3 * qty + noise_score + 50.0

# Independent continuous sort key -- Uniform(0, 1000) draws are tie-free at
# double precision for 30k samples (this is the value the filter+sort chain
# sorts by, so ambiguous ties would make row order language-dependent).
sortkey = rng.uniform(0.0, 1000.0, N)

path = os.path.join(OUT, "dataset.csv")
with open(path, "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["region", "price", "qty", "amount", "score", "sortkey"])
    for i in range(N):
        w.writerow([region[i], price[i], qty[i], amount[i], score[i], sortkey[i]])

print(f"wrote {path}: {N} rows, {N_REGIONS} regions")
print(f"region counts:\n{np.unique(region, return_counts=True)}")
print(f"price range: [{price.min():.2f}, {price.max():.2f}]")
