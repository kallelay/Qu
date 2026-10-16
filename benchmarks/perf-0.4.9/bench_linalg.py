"""NumPy side of bench_linalg.qu: the same workloads under the same names.

qr_n        np.linalg.qr(A)                  (reduced Q, R)
svd_n       np.linalg.svd(A)                 (thin U, s, Vt; full_matrices=False)
eig_sym_n   np.linalg.eigh(S)                (values and vectors)
eig_gen_n   np.linalg.eig(A)                 (values AND vectors: more work than Qu's
                                              eig(A), which returns eigenvalues only)
eigvals_n   np.linalg.eigvals(A)             (values only: the like-for-like reference)

Run with OPENBLAS_NUM_THREADS=1 for the single-thread reference.
"""
import time

import numpy as np

rng = np.random.default_rng(1)


def rep(name, dt):
    print(f"BENCH {name} {dt}", flush=True)


for n in (100, 200, 500, 1000):
    A = rng.standard_normal((n, n))
    S = A + A.T
    t = time.perf_counter(); np.linalg.qr(A); rep(f"qr_{n}", time.perf_counter() - t)
    t = time.perf_counter(); np.linalg.svd(A, full_matrices=False); rep(f"svd_{n}", time.perf_counter() - t)
    t = time.perf_counter(); np.linalg.eigh(S); rep(f"eig_sym_{n}", time.perf_counter() - t)
    t = time.perf_counter(); np.linalg.eig(A); rep(f"eig_gen_{n}", time.perf_counter() - t)
    t = time.perf_counter(); np.linalg.eigvals(A); rep(f"eigvals_{n}", time.perf_counter() - t)
