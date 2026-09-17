"""polyfit_bench.py -- numpy port of polyfit_bench.qu/.m, bit-identical
(formula-generated, no RNG) dataset so the fitted coefficients can be
compared directly across languages, not just the timings.

Run: python benchmarks/linalg/polyfit_bench.py
"""
import time
import numpy as np

N = 3_000_000
degree = 5
true_coeffs = [2, -4, 1, 0.5, -2, 3]  # highest-degree-first

x = np.linspace(-1, 1, N)
y_true = np.polyval(true_coeffs, x)
noise = 0.01 * np.sin(1000 * x)
y = y_true + noise

t0 = time.perf_counter()
c = np.polyfit(x, y, degree)
t_fit = time.perf_counter() - t0

t0 = time.perf_counter()
yhat = np.polyval(c, x)
t_eval = time.perf_counter() - t0

resid = np.linalg.norm(y - yhat)
max_abs_err = np.max(np.abs(y - yhat))

print(f"N={N}  degree={degree}")
print(f"polyfit : {t_fit:.4f} s")
print(f"polyval : {t_eval:.4f} s")
print(f"coeffs  : {list(c)}")
print(f"resid (||y - yhat||_2) = {resid:.6f}")
print(f"max_abs_err            = {max_abs_err:.6f}")
