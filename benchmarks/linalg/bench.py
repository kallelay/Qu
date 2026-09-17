"""bench.py -- numpy/scipy port of bench.qu/bench.m, same sizes/operations.

eig/svd/matrix_rank/det come from numpy.linalg (the exact functions named in
the benchmark ask); qr/lu come from scipy.linalg (numpy.linalg has no lu, and
its own qr doesn't expose the P/L/U split lu needs). scipy.linalg.lu's
convention is A = P @ L @ U (not P @ A = L @ U like MATLAB/Qu/nalgebra) --
handled explicitly in the residual check below, not silently mismatched.

Run: python benchmarks/linalg/bench.py
"""
import time
import numpy as np
import scipy.linalg as sla


def run_size(n, rng):
    M = rng.standard_normal((n, n))
    A = M + M.T

    t0 = time.perf_counter()
    lam, V = np.linalg.eig(A)
    t_eig = time.perf_counter() - t0
    lam_r = lam.real
    V_r = V.real
    eig_resid = np.linalg.norm(A @ V_r - V_r @ np.diag(lam_r))

    # eigh is the numerically-appropriate call for a symmetric input (the
    # same symmetric-dispatch path Qu's `eig` takes internally) -- timed
    # separately since `eig` vs `eigh` isn't an apples-to-apples pair on its
    # own, flagged in the README rather than only reporting the slower one.
    t0 = time.perf_counter()
    lam_h, V_h = np.linalg.eigh(A)
    t_eigh = time.perf_counter() - t0

    t0 = time.perf_counter()
    Q, R = np.linalg.qr(A)
    t_qr = time.perf_counter() - t0
    qr_resid = np.linalg.norm(A - Q @ R)

    t0 = time.perf_counter()
    P, L, U = sla.lu(A)
    t_lu = time.perf_counter() - t0
    lu_resid = np.linalg.norm(A - P @ L @ U)

    t0 = time.perf_counter()
    d = np.linalg.det(A)
    t_det = time.perf_counter() - t0

    t0 = time.perf_counter()
    r = np.linalg.matrix_rank(A)
    t_rank = time.perf_counter() - t0

    t0 = time.perf_counter()
    U2, S, Vt = np.linalg.svd(A)
    t_svd = time.perf_counter() - t0
    svd_resid = np.linalg.norm(A - U2 @ np.diag(S) @ Vt)

    print(f"n={n}")
    print(f"  eig  : {t_eig:.4f} s   resid={eig_resid:.3e}   (eigh: {t_eigh:.4f} s)")
    print(f"  qr   : {t_qr:.4f} s   resid={qr_resid:.3e}")
    print(f"  lu   : {t_lu:.4f} s   resid={lu_resid:.3e}")
    print(f"  det  : {t_det:.4f} s   d={d:.4e}")
    print(f"  rank : {t_rank:.4f} s   r={r}")
    print(f"  svd  : {t_svd:.4f} s   resid={svd_resid:.3e}")


if __name__ == "__main__":
    rng = np.random.default_rng(42)
    for n in (100, 500, 1000):
        run_size(n, rng)
