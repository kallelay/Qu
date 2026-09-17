"""binary_doubles.py -- packed float64 binary file round trip, Python side.

Mirrors binary_doubles.qu's dataset (same N, same seed-free but same
distribution) using numpy's vectorized tofile/fromfile -- the natural,
idiomatic way to do this in Python, not a struct-per-value loop (that
would be the unfair, unidiomatic comparison; struct.pack/unpack per value
is shown too, separately, purely as an apples-to-apples curiosity against
Qu's own scalar-call API).

Run: python benchmarks/file_io/binary_doubles.py
"""
import time
import struct
import numpy as np

N = 1_000_000
path = "benchmarks/file_io/py_doubles.bin"

rng = np.random.default_rng(42)
x = rng.standard_normal(N)

# --- idiomatic numpy: vectorized tofile/fromfile -----------------------------
t0 = time.perf_counter()
x.tofile(path)
t_write = time.perf_counter() - t0
print(f"numpy tofile   ({N} values) : {t_write:.4f} s")

t0 = time.perf_counter()
y = np.fromfile(path, dtype=np.float64)
t_read = time.perf_counter() - t0
print(f"numpy fromfile ({N} values) : {t_read:.4f} s")

print(f"mean(written) = {x.mean():.6f}")
print(f"mean(read)    = {y.mean():.6f}")
print(f"sumsq(read)   = {float(np.sum(y * y)):.6f}")

# --- apples-to-apples with Qu's scalar-call API: struct.pack/unpack per value,
# on a much smaller N (10,000) -- purely to show what "one Python call per
# value" costs too, since Qu has no other option. Not the headline number.
N2 = 10_000
path2 = "benchmarks/file_io/py_doubles_scalar.bin"
x2 = rng.standard_normal(N2)

t0 = time.perf_counter()
with open(path2, "wb") as f:
    for v in x2:
        f.write(struct.pack("<d", v))
t_write_scalar = time.perf_counter() - t0
print(f"struct.pack per-value  ({N2} values) : {t_write_scalar:.4f} s")

t0 = time.perf_counter()
s = 0.0
with open(path2, "rb") as f:
    for _ in range(N2):
        (v,) = struct.unpack("<d", f.read(8))
        s += v
t_read_scalar = time.perf_counter() - t0
print(f"struct.unpack per-value ({N2} values) : {t_read_scalar:.4f} s")
