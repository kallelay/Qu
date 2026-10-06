# perf-0.4.9 benchmark set (Python/NumPy/SciPy side). Same workload names as
# bench.qu; prints "BENCH <name> <seconds>" (wall clock via perf_counter, data
# built outside the clock).
import os, re, sys, tempfile, time
import numpy as np
import scipy.linalg as sla
import scipy.sparse as sp
import scipy.sparse.linalg as spla
import scipy.signal as sig
import scipy.integrate as si
import pandas as pd

T = time.perf_counter


def rep(name, dt):
    print(f"BENCH {name} {dt}")


def timeit(name, f):
    t = T()
    f()
    rep(name, T() - t)


def add1(a):
    return a + 1


def ucall_loop(n):
    s = 0.0
    for _ in range(n):
        s = add1(s)
    return s


def scalar_loop(n):
    s = 0.0
    for i in range(1, n + 1):
        s = s + i * 0.5
    return s


def fsys(t, y):
    return [y[1], -y[0]]


def integrand(x):
    return np.exp(-x * x) * np.cos(3 * x)


import math


def integrand_s(x):
    return math.exp(-x * x) * math.cos(3 * x)


rng = np.random.default_rng(1)
t = T()
rep("baseline_empty", T() - t)

for n in (500, 1000):
    A = rng.standard_normal((n, n)); B = rng.standard_normal((n, n))
    timeit(f"matmul_{n}", lambda: A @ B)
for n in (200, 500, 1000):
    A = rng.standard_normal((n, n)); b = rng.standard_normal(n); S = A + A.T
    timeit(f"solve_{n}", lambda: np.linalg.solve(A, b))
    timeit(f"inv_{n}", lambda: np.linalg.inv(A))
    timeit(f"lu_{n}", lambda: sla.lu_factor(A))
    timeit(f"qr_{n}", lambda: np.linalg.qr(A))
    if n <= 500:
        timeit(f"svd_{n}", lambda: np.linalg.svd(A))
        timeit(f"eig_sym_{n}", lambda: np.linalg.eigh(S))
        timeit(f"eig_gen_{n}", lambda: np.linalg.eig(A))

for n in (65536, 1048576, 1000, 4999, 10007, 100000, 1000003):
    x = rng.standard_normal(n)
    timeit(f"fft_{n}", lambda: np.fft.fft(x))

x = rng.standard_normal(1000)
timeit("fft_1000_x100", lambda: [np.fft.fft(x) for _ in range(100)])
x = rng.standard_normal(4999)
timeit("fft_4999_x100", lambda: [np.fft.fft(x) for _ in range(100)])
x = rng.standard_normal(1000000)
bb_, aa_ = sig.butter(4, 100, "low", fs=1000)
h = np.ones(64) / 64
timeit("filter_fir64_1e6", lambda: sig.lfilter(h, [1.0], x))
timeit("filtfilt_butter4_1e6", lambda: sig.filtfilt(bb_, aa_, x))
timeit("conv_64tap_1e6", lambda: np.convolve(x, h))

v = rng.standard_normal(10000000)
w = np.floor(rng.random(10000000) * 1000)
timeit("cumsum_1e7", lambda: np.cumsum(v))
timeit("sort_1e7", lambda: np.sort(v))
timeit("unique_1e7", lambda: np.unique(w))
timeit("sum_1e7", lambda: np.sum(v))
timeit("mean_1e7", lambda: np.mean(v))
timeit("std_1e7", lambda: np.std(v))

a = rng.standard_normal(4000000); b2 = rng.standard_normal(4000000)
timeit("elementwise_chain_4e6", lambda: np.sin(a) * np.cos(b2) + np.exp(-a * a) - np.sqrt(np.abs(b2)))
M = rng.standard_normal((2000, 2000)); r = rng.standard_normal((1, 2000)); cc = rng.standard_normal((2000, 1))
timeit("broadcast_2000x2000", lambda: M + r + cc)

M = rng.standard_normal((1000, 1000))


def mal():
    for i in range(1000):
        for j in range(1000):
            M[i, j] = i + j


timeit("matrix_assign_loop_1e6", mal)


def rss():
    s = 0.0
    for i in range(1000):
        s = s + np.sum(M[i, :])


timeit("row_slice_sum_1000", rss)


def csa():
    for i in range(1000):
        M[:, i] = np.zeros(1000)


timeit("col_slice_assign_1000", csa)
timeit("scalar_loop_1e7", lambda: scalar_loop(10000000))
timeit("user_fn_call_1e6", lambda: ucall_loop(1000000))


def wl():
    s = 0.0
    i = 0
    while i < 3000000:
        s = s + i
        i = i + 1


timeit("while_loop_3e6", wl)

box = {}


def sbj():
    parts = []
    for i in range(1, 100001):
        parts.append("item" + str(i))
    box["j"] = ",".join(parts)


timeit("str_build_join_1e5", sbj)
timeit("str_split_1e5", lambda: box["j"].split(","))
txt = "".join(f"id={i} value={i*3}\n" for i in range(1, 20001))
timeit("regex_count_20k", lambda: len(re.findall(r"value=[0-9]+", txt)))
timeit("regex_replace_20k", lambda: re.sub(r"id=([0-9]+)", r"ID:\1", txt))

df = pd.DataFrame({"group": np.floor(rng.random(1000000) * 50), "value": rng.standard_normal(1000000), "weight": rng.random(1000000)})
p = os.path.join(tempfile.gettempdir(), "perf049_py.csv")
timeit("csv_write_1e6", lambda: df.to_csv(p, index=False))
timeit("csv_read_1e6", lambda: pd.read_csv(p))
os.remove(p)


def dsg():
    d = {}
    for i in range(5000):
        d[f"k{i}"] = i
    s = 0
    for i in range(5000):
        s = s + d[f"k{i}"]


timeit("dict_5e3_set_get", dsg)


def lag():
    l = []
    for i in range(100000):
        l.append(i)
    s = 0
    for i in range(100000):
        s = s + l[i]


timeit("list_1e5_append_get", lag)


class R:
    pass


rec = R(); rec.a = 1; rec.b = 2


def rfl():
    s = 0
    for _ in range(1000000):
        s = s + rec.b + rec.a


timeit("record_field_loop_1e6", rfl)

timeit("ode45_sho_t1000", lambda: si.solve_ivp(fsys, (0, 1000), [1, 0], method="RK45", rtol=1e-8, atol=1e-10))


def qd():
    q = 0.0
    for _ in range(200):
        q = q + si.quad(integrand_s, -10, 10, epsabs=1e-12, epsrel=1e-12, limit=200)[0]


timeit("quad_smooth_x200", qd)
y = rng.standard_normal(10000000)
timeit("trapz_1e7", lambda: np.trapezoid(y))

m = 100
I = sp.identity(m, format="csr")
D = sp.diags([-1, 2, -1], [-1, 0, 1], shape=(m, m), format="csr")
t0 = T()
P = (sp.kron(I, D) + sp.kron(D, I)).tocsc()
rep("sparse_build_poisson100", T() - t0)
bv = np.ones(m * m)
timeit("sparse_lu_poisson100", lambda: spla.spsolve(P, bv))
Pc = P.tocsr()
timeit("sparse_cg_poisson100", lambda: spla.cg(Pc, bv, rtol=1e-10, maxiter=20000))
dinv = 1.0 / Pc.diagonal()
Mj = spla.LinearOperator(Pc.shape, matvec=lambda z: dinv * z)
timeit("sparse_cgj_poisson100", lambda: spla.cg(Pc, bv, rtol=1e-10, maxiter=20000, M=Mj))

pn = os.path.join(tempfile.gettempdir(), "perf049_py.npy")
big = rng.standard_normal(10000000)
timeit("npy_write_1e7", lambda: np.save(pn, big))
timeit("npy_read_1e7", lambda: np.load(pn))
os.remove(pn)
