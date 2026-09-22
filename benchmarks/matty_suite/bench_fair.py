"""bench_fair.py -- numpy port of bench_fair.qu.

Same five kernels as bench.py, but with data built OUTSIDE the timed region,
one discarded warm-up run, and best-of-three reported. See bench_fair.qu's
header for why: bench.py's per-kernel numbers include their own `randn`
(82% of its matrix_multiply row) and a single cold trial, so they cannot
support a per-kernel claim.
"""

import math
import time

import numpy as np

REPS = 3


def best_of(fn, reps=REPS):
    fn()  # warm-up, discarded
    best = float("inf")
    for _ in range(reps):
        t = time.perf_counter()
        fn()
        best = min(best, time.perf_counter() - t)
    return best


# matmul -- data pre-built
A = np.random.randn(1000, 1000)
B = np.random.randn(1000, 1000)
t_mm = best_of(lambda: A @ B)
print(f"matrix_multiply   : {t_mm:.4f} s   (data pre-built, best of {REPS})")

# elementwise -- 3000x3000 to match bench_fair.qu
A2 = np.random.randn(3000, 3000)
B2 = np.random.randn(3000, 3000)
t_ew = best_of(lambda: np.abs(A2 * B2 + np.sin(A2) * np.cos(B2)))
print(f"element_wise_ops  : {t_ew:.4f} s   (3000x3000, data pre-built, best of {REPS})")

# fft -- data pre-built
x = np.random.randn(100000)
t_fft = best_of(lambda: np.fft.ifft(np.fft.fft(x)))
print(f"fft               : {t_fft:.4f} s   (data pre-built, best of {REPS})")


# array creation -- no rand, matching bench_fair.qu
def make_arrays():
    np.zeros((5000, 5000))
    np.ones((5000, 5000))
    np.eye(5000)


t_ac = best_of(make_arrays)
print(f"array_creation    : {t_ac:.4f} s   (no rand, best of {REPS})")


# Two deliberate departures from bench.py's loop kernel, both measured; see
# bench_fair.qu's loop_performance comment for the numbers. Short version:
# n=100_000 is at timer resolution, and MATLAB's JIT folds the closed-form
# sum (47x apart from a non-foldable body at the same trip count), so a
# plain `r += i` loop measures constant folding rather than throughput.
# All three ports must print result = 60000003.
N_LP = 20_000_000


def loop():
    r = 0
    for i in range(1, N_LP + 1):
        r += i - 7 * math.floor(i / 7)
    return r


t_lp = best_of(loop)
print(
    f"loop_performance  : {t_lp:.4f} s   "
    f"(n={N_LP}, non-foldable, best of {REPS}, result = {loop()})"
)

print(f"total             : {t_mm + t_ew + t_fft + t_ac + t_lp:.4f} s")
