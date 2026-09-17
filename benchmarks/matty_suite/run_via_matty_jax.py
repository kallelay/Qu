"""Run matty's own BenchmarkSuite (matty/benchmarks/benchmark_suite.py) against its
JAX backend (CPU), for a direct comparison against bench.qu/bench.py/bench.m on the
same 5 kernels/sizes.

Must run from the matty/ directory with its project-local venv, since matty's
backend modules use relative imports rooted there:

    cd matty
    ./.venv/Scripts/python.exe ../benchmarks/matty_suite/run_via_matty_jax.py
"""
import json
import sys

sys.path.insert(0, ".")
from backends.jax_backend import JAXBackend
from benchmarks.benchmark_suite import BenchmarkSuite

backend = JAXBackend()
print("JAX available:", backend.available, "GPU available:", getattr(backend, "gpu_available", None))

suite = BenchmarkSuite()
results = suite.run_for_backend(backend)
print(json.dumps({k: v for k, v in results.items() if k != "system"}, indent=2))
