"""bench.py -- numpy port of matty's BenchmarkSuite, same kernels/sizes as bench.qu/bench.m.

Run: python benchmarks/matty_suite/bench.py
"""
import time
import numpy as np


def timed(fn):
    start = time.perf_counter()
    fn()
    return time.perf_counter() - start


def bench_matrix_multiply():
    A = np.random.randn(1000, 1000)
    B = np.random.randn(1000, 1000)
    C = A @ B


def bench_element_wise_ops():
    A = np.random.randn(10000, 10000)
    B = np.random.randn(10000, 10000)
    C = A * B + np.sin(A) * np.cos(B)
    D = np.abs(C)


def bench_fft():
    x = np.random.randn(100000)
    X = np.fft.fft(x)
    xr = np.fft.ifft(X)


def bench_array_creation():
    A = np.zeros((5000, 5000))
    B = np.ones((5000, 5000))
    C = np.eye(5000)
    D = np.random.rand(5000, 5000)


def bench_loop_performance():
    result = 0
    for i in range(1, 100001):
        result = result + i
    return result


if __name__ == "__main__":
    t_matmul = timed(bench_matrix_multiply)
    print(f"matrix_multiply   : {t_matmul:.4f} s")

    t_elemwise = timed(bench_element_wise_ops)
    print(f"element_wise_ops  : {t_elemwise:.4f} s")

    t_fft = timed(bench_fft)
    print(f"fft               : {t_fft:.4f} s")

    t_arrcreate = timed(bench_array_creation)
    print(f"array_creation    : {t_arrcreate:.4f} s")

    start = time.perf_counter()
    result = bench_loop_performance()
    t_loop = time.perf_counter() - start
    print(f"loop_performance  : {t_loop:.4f} s  (result = {result})")

    total = t_matmul + t_elemwise + t_fft + t_arrcreate + t_loop
    print(f"total             : {total:.4f} s")
