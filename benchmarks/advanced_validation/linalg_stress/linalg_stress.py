# linalg_stress.py -- numpy.linalg's qr/svd/solve on the EXACT SAME
# matrices linalg_stress.qu just ran (a_n{n}.csv/b_n{n}.csv, written once by
# make_data.py), plus an elementwise diff against the Qu solve's own dumped
# `x` (x_qu_n{n}.csv) -- not just a residual-norm comparison, an actual
# value-for-value check that both languages landed on the SAME solution.
#
# Run: python benchmarks/advanced_validation/linalg_stress/linalg_stress.py
# (after linalg_stress.qu, which produces x_qu_n{n}.csv)

import os
import time
import numpy as np
import pandas as pd

OUT = os.path.dirname(os.path.abspath(__file__))

SIZES = [300, 1000]

for n in SIZES:
    a_flat = pd.read_csv(os.path.join(OUT, f"a_n{n}.csv"))["val"].to_numpy()
    b = pd.read_csv(os.path.join(OUT, f"b_n{n}.csv"))["val"].to_numpy()
    A = a_flat.reshape((n, n), order="F")  # same column-major convention as Qu's reshape()

    # -- QR: A = Q*R ---------------------------------------------------------
    t0 = time.perf_counter()
    Q, R = np.linalg.qr(A)
    t_qr = time.perf_counter() - t0
    qr_resid = float(np.max(np.abs(A - Q @ R)))

    # -- SVD: A = U*diag(s)*Vt ------------------------------------------------
    t0 = time.perf_counter()
    U, s, Vt = np.linalg.svd(A)
    t_svd = time.perf_counter() - t0
    svd_resid = float(np.max(np.abs(A - (U * s) @ Vt)))

    # -- Solve: x = A \ b ------------------------------------------------------
    t0 = time.perf_counter()
    x = np.linalg.solve(A, b)
    t_solve = time.perf_counter() - t0

    print(f"n={n}")
    print(f"  qr    : {t_qr*1000:.3f} ms   max|A-Q*R|     = {qr_resid:.3e}")
    print(f"  svd   : {t_svd*1000:.3f} ms   max|A-U*S*Vt| = {svd_resid:.3e}")
    print(f"  solve : {t_solve*1000:.3f} ms")

    qu_x_path = os.path.join(OUT, f"x_qu_n{n}.csv")
    if os.path.exists(qu_x_path):
        qu_x = pd.read_csv(qu_x_path)["x"].to_numpy()
        max_diff = float(np.max(np.abs(qu_x - x)))
        print(f"  max|Qu_x - numpy_x| over all {n} elements = {max_diff:.3e}")
    else:
        print("  (run linalg_stress.qu first for the Qu-vs-numpy elementwise solve diff)")
