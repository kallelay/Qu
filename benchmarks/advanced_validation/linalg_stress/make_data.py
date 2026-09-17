# make_data.py -- generates the shared matrices/vectors this scenario's Qu
# and Python scripts both read, ONCE, so `qr`/`svd`/`\` on the Qu side and
# `numpy.linalg.qr`/`svd`/`solve` on the Python side operate on the exact
# same numbers (same convention as ../curve_kalman/make_data.py and
# ../peak_finding/make_signals.py).
#
# Sizes: n=300 and n=1000 -- beyond catalog/qu_qr_svd.qu's 4x2 toy example,
# in the same ballpark as benchmarks/linalg/bench.qu's own 100/500/1000
# ladder (which already exercises qr/svd/eig/lu/det/rank at these sizes).
#
# THE COLUMN-MAJOR TRICK (see README.md for the full explanation): Qu has
# no raw numeric-CSV-matrix loader and no dynamic/reflective column access,
# so a matrix can't be split across N named CSV columns and reassembled by
# looping over column names at parse time. Instead each matrix is written
# as a SINGLE-COLUMN CSV holding its values in COLUMN-MAJOR order
# (`A.flatten(order="F")`), and reconstructed on the Qu side with
# `reshape(df.val, n, n)` -- confirmed against the interpreter source
# (`engine/crates/qu-interp/src/lib.rs`'s `reshape_to`, which calls
# `Matrix::from_col_major_checked` whenever the input vector's length
# equals rows*cols). Flattening row-major instead would silently transpose
# the reconstructed matrix.
#
# Run once: python benchmarks/advanced_validation/linalg_stress/make_data.py

import os
import numpy as np
import pandas as pd

OUT = os.path.dirname(os.path.abspath(__file__))

SIZES = [300, 1000]

rng = np.random.default_rng(42)

for n in SIZES:
    # M @ M.T + n*I is symmetric positive-definite by construction (every
    # eigenvalue of M@M.T is >= 0, shifting by n*I pushes the smallest
    # eigenvalue comfortably positive) -- guaranteed non-singular and
    # well-conditioned regardless of the random draw, no post-hoc cond()
    # check-and-retry needed.
    M = rng.standard_normal((n, n))
    A = M @ M.T + n * np.eye(n)
    b = rng.standard_normal(n)

    cond_a = np.linalg.cond(A)

    a_flat = A.flatten(order="F")  # COLUMN-MAJOR -- see note above
    pd.DataFrame({"val": a_flat}).to_csv(os.path.join(OUT, f"a_n{n}.csv"), index=False)
    pd.DataFrame({"val": b}).to_csv(os.path.join(OUT, f"b_n{n}.csv"), index=False)

    print(f"n={n}: wrote a_n{n}.csv ({a_flat.size} values), b_n{n}.csv ({b.size} values), cond(A)={cond_a:.3e}")
