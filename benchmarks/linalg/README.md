# Linear-algebra decompositions + a second optimization workload

Two things `benchmarks/README.md`'s existing scenarios didn't cover:

1. **Dense-matrix decompositions at scale** — `eig`, `svd`, `qr`, `lu`, `det`,
   `rank` at 100×100 / 500×500 / 1000×1000, against numpy/scipy, MATLAB
   R2025b, and **matty** (the sibling MATLAB-compatible interpreter at
   `../matty` (a sibling checkout), not the vendored/retired
   `Qu/matty/` JAX backend `matty_suite/` already uses).
2. **A second, different optimization workload** — `polyfit` (least-squares
   Vandermonde solve) at `N=3,000,000` points, picked specifically because,
   unlike an iterative algorithm (where "same iteration count/convergence
   criterion" is a real source of cross-language ambiguity), a least-squares
   fit is a **direct,
   non-iterative solve** — there's no iteration count or starting point to
   keep in sync, so a bit-identical (formula-generated, no RNG) input
   dataset makes the four languages' outputs directly, exactly comparable,
   not just their timings.

## Files

| File | Language / engine |
|---|---|
| `bench.qu` | Qu |
| `bench.py` | Python (numpy + scipy.linalg) |
| `bench.m` | Real MATLAB R2025b |
| `bench_matty.m` | matty (same operations, two real incompatibilities worked around — see below) |
| `polyfit_bench.qu` / `.py` / `.m` | The polyfit workload, Qu / numpy / MATLAB — `.m` also runs unmodified under matty |

Run:

```bash
qu run benchmarks/linalg/bench.qu
python benchmarks/linalg/bench.py
matlab -batch "run('benchmarks/linalg/bench.m')"
# from the matty/ repo root:
python matty_runner.py <path>/benchmarks/linalg/bench_matty.m

qu run benchmarks/linalg/polyfit_bench.qu
python benchmarks/linalg/polyfit_bench.py
matlab -batch "run('benchmarks/linalg/polyfit_bench.m')"
python matty_runner.py <path>/benchmarks/linalg/polyfit_bench.m
```

## Part 1 — decompositions

**Matrix**: one symmetric "Wigner-style" random matrix per size, `A = M +
M'` where `M = randn(n,n)` — symmetric so Qu's `eig` takes its real-
eigenvector path (nalgebra's `symmetric_eigen`; a non-symmetric input only
returns eigenvalues, nothing to reconstruct/verify against). `randn`/`rng`
draw different numbers per language/seed (same standing caveat as
`matty_suite/`) — this is a timing + per-language-correctness comparison,
not a cross-language bit-match (that discipline is reserved for Part 2's
`polyfit`, where the input is instead a deterministic formula, precisely so
it *can* be bit-matched).

**Correctness, checked per language, not asserted**: every decomposition
reconstructs `A` and reports the Frobenius-norm residual (`norm(A - Q*R)`,
`norm(P*A - L*U)`, `norm(A - U*diag(s)*V')`, `norm(A*V - V*diag(lambda))`).
Every engine's residual lands at **1e-9 to 1e-14** at every size — all four
engines' decompositions are numerically correct — with one confirmed
exception, matty's `lu`, detailed below.

### Results (release build; 3 trials per cell, median shown)

**This machine ran under sustained, uncontrolled 100% CPU load throughout
this session** (`Get-CimInstance Win32_Processor | Select LoadPercentage`
read 100 mid-run; `Get-Process` showed OneDrive/Dropbox sync, multiple
`claude` sessions, and a `python`/matty process all active simultaneously —
plus the standing fact that another live Claude session is doing engine
work in this same repo). The result: **5–20x swings between trials of the
same op/size in every language, MATLAB included** (e.g. real MATLAB's own
`eig` at n=1000 read 21.3s, 10.6s, then 0.96s across three back-to-back
trials). This is real noise, not fabricated smoothing — medians below are
the honest center of a very wide spread, not a precise ratio. Treat this
table as **order-of-magnitude / correctness**, not a precision speed claim
— a genuinely quiet-machine re-run would be needed before trusting any
specific multiplier here.

n=100:

| op | Qu | numpy/scipy | MATLAB R2025b | matty |
|---|---:|---:|---:|---:|
| eig | 0.0013 s | 0.0159 s | 0.2107 s | 0.0122 s |
| qr | 0.0005 s | 0.0052 s | 0.2289 s | 0.0043 s |
| lu | 0.0004 s | 0.0058 s | 0.0019 s | 0.2823 s |
| det | 0.0002 s | 0.0009 s | 0.0014 s | 0.0002 s |
| rank | 0.0025 s | 0.0025 s | 0.1020 s | 0.0036 s |
| svd | 0.0020 s | 0.0098 s | 0.7504 s | 0.0144 s |

n=500:

| op | Qu | numpy/scipy | MATLAB R2025b | matty |
|---|---:|---:|---:|---:|
| eig | 0.1563 s | 1.0813 s | 6.3321 s | 0.3897 s |
| qr | 0.0503 s | 0.2264 s | 0.0819 s | 0.1389 s |
| lu | 0.0156 s | 3.2132 s | 0.0196 s | 3.3260 s |
| det | 0.0105 s | 2.7329 s | 0.0130 s | 3.4891 s |
| rank | 0.3721 s | 0.0426 s | 0.5841 s | 0.1948 s |
| svd | 0.4037 s | 0.8173 s | 0.8502 s | 0.7894 s |

n=1000:

| op | Qu | numpy/scipy | MATLAB R2025b | matty |
|---|---:|---:|---:|---:|
| eig | 1.5547 s | 4.9411 s | 10.6426 s | 7.5950 s |
| qr | 0.6893 s | 2.0572 s | 0.2625 s | 2.9758 s |
| lu | 0.1976 s | 4.5118 s | 0.0240 s | 6.7448 s |
| det | 0.1364 s | 5.0851 s | 0.0185 s | 3.8810 s |
| rank | 5.2732 s | 0.3497 s | 0.8810 s | 0.5694 s |
| svd | 4.9752 s | 1.7336 s | 0.8266 s | 1.8897 s |

**Reading this honestly, given the noise**: Qu is competitive-to-faster at
n=100/500 on most ops, and is *not* uniformly behind MATLAB the way `fft`
used to be before the `rustfft` swap — but `rank`/`svd` at n=1000 (5.3s/
5.0s) are Qu's clearest soft spots here, consistent with `rank` and `svd`
both bottoming out in nalgebra's dense SVD, a pure-Rust implementation with
no BLAS/LAPACK acceleration underneath — the same root cause behind every
other "Qu trails a vendor-tuned library" gap this repo has found (`fft`
before `rustfft`, `element_wise_ops` before parallelization). Also numpy's
`det`/`lu` at n=500/1000 (2.7-5.1s) look like real load-noise outliers
given `scipy.linalg.lu`'s and `numpy.linalg.det`'s normal sub-100ms
performance at this size on an unloaded machine — not trusted as numpy's
real number here. `det` overflows to `+/-Inf` at n=500/1000 in **every**
engine (Qu, numpy, MATLAB, matty) — expected: a 500-1000-dimensional
Wigner-style random matrix's determinant is astronomically large
(`log|det| ~ O(n log n)`), genuinely outside `f64` range; all four agree,
so it's a property of the input, not a bug anywhere.

### matty: two incompatibilities, one confirmed bug

Running `bench.m` unmodified under matty failed twice before it ran at all
— both are real matty gaps, not benchmark-script issues, kept as `bench_
matty.m` with the workarounds documented inline:

1. **No `rng`/seeded-RNG support anywhere** (`src/builtins.py`'s `randn` is
   a bare `np.random.randn(...)`, no seed parameter). Dropped `rng(42)` —
   harmless here since timings were never cross-language bit-comparable
   anyway.
2. **No local/sub-function support in a script file.** Real MATLAB (since
   R2016b) allows a script to define helper functions below the top-level
   code; matty's parser doesn't recognize this at all (`bench.m`'s `function
   run_size(n) ... end` errored "Undefined function or variable
   'run_size'"). Every one of matty's own `examples/*.m` is a flat script
   with no function definitions — consistent with this being a real,
   confirmed gap. Worked around by inlining the loop body.

**A third thing, found while sanity-checking `lu`'s residual, is a genuine
matty correctness bug, confirmed by reading source, not just observed
behavior**: `[L, U, P] = lu(A)` in matty returns **completely mislabeled
outputs**. `src/builtins.py::f_lu` calls `scipy.linalg.lu(A)`, which
returns `(p, l, u)` satisfying `A = p @ l @ u`, and returns that tuple
unchanged for the 3-output case. matty's interpreter then binds
return-tuple position *i* to the *i*-th left-hand variable **by position,
not by what the factorization actually is** — so a script's `[L, U, P] =
lu(A)` receives `L <- p` (the permutation matrix!), `U <- l` (the lower
factor!), `P <- u` (the upper factor!). Checking the residual against real
MATLAB's documented convention (`P*A = L*U`) gives garbage (thousands, not
~1e-12) at every size; checking `norm(A - L*U*P, 'fro')` — i.e. `A =
(matty's-L)*(matty's-U)*(matty's-P)`, exactly what the swap predicts —
lands at **7.5e-14 / 1.5e-12 / 6.6e-12** for n=100/500/1000: the
factorization itself is numerically correct, matty just never adapted
scipy's `(P,L,U)` ordering to MATLAB's documented `[L,U,P]` contract. Any
real matty script relying on `[L,U,P]=lu(A)` silently gets wrong-but-
plausible-looking matrices today. `bench_matty.m` documents all three
findings inline with the exact residual values proving each one.

## Part 2 — `polyfit` (second optimization workload)

`x = linspace(-1,1,N)`, `y = polyval(true_coeffs, x) + 0.01*sin(1000*x)` —
the `sin` term is a **deterministic**, non-polynomial perturbation (not RNG
noise), so all four languages build the exact same `y` and there's a
genuine nonzero least-squares residual (a degree-5 fit can't reproduce a
high-frequency `sin` term) rather than a trivial exact-interpolation
problem with nothing to solve.

### Correctness: verified numerically matching before trusting any timing

`true_coeffs = [2, -4, 1, 0.5, -2, 3]`, `N=3,000,000`, degree 5. Fitted
coefficients, all four engines:

| engine | coefficients (highest-degree-first) |
|---|---|
| Qu | `1.999524, -4, 1.000432, 0.5, -2.000072, 3` |
| numpy | `1.999524, -4.000000, 1.000432, 0.500000, -2.000072, 3.000000` |
| MATLAB | `1.999524, -4.000000, 1.000432, 0.500000, -2.000072, 3.000000` |
| matty | (fitted correctly — see display caveat below) |

Identical to 6 decimal places across Qu/numpy/MATLAB, and the residual
**`||y - yhat||_2 = 12.244523`** and **`max_abs_err = 0.010110`** match
*exactly* (not just "close") across **all four** engines, Qu/numpy/MATLAB/
matty. This is a genuinely apples-to-apples, verified-before-timed
comparison — the standing bar this benchmark set out to clear.

(matty caveat: its `fprintf('%.6f ', c)` only printed the first of `c`'s 6
elements rather than cycling the format string across the whole vector the
way real MATLAB's `fprintf` does — a real, minor matty formatting gap, but
`resid`/`max_abs_err` printed via scalar `fprintf` calls came through fine
and matched exactly, so the *fit itself* is confirmed correct.)

### Speed (3 trials each, small/fast enough that the machine-load noise
above barely shows up here — spreads were under 20%, not 5-20x)

| engine | polyfit time (median of 3) |
|---|---:|
| Qu | 1.586 s |
| numpy | 0.354 s |
| MATLAB R2025b | 0.394 s |
| matty | 0.422 s |

**Qu is a real, consistently-reproduced ~4x slower here** — the one clean,
low-noise speed number from this whole session. `polyfit` builds an
`(N, 6)` Vandermonde matrix and solves it via `qu-core`'s `least_squares`
(nalgebra's dense SVD-based pseudoinverse) — the same "nalgebra has no
BLAS/LAPACK acceleration underneath" gap flagged in Part 1's `rank`/`svd`
numbers, now isolated on a workload without the load-noise problem.
`numpy.polyfit` also goes through an SVD-based least-squares solve
(LAPACK's `gelsd`) — the ~4x gap is a real, LAPACK-vs-plain-nalgebra
difference on a tall-skinny matrix, not a different algorithm. A
worthwhile future target if `polyfit`/`least_squares`/`pinv` performance on
large tall-skinny inputs matters — out of scope to fix in this benchmarking
pass.

## Known gaps / out of scope

- No Octave data point (same standing gap as every other scenario in
  `benchmarks/README.md` — Octave isn't installed on this machine).
- Decomposition numbers (Part 1) are order-of-magnitude only given the
  documented 5-20x same-cell variance — a quiet-machine re-run is the
  honest next step before treating any specific ratio there as real.
- The matty `lu` mislabeling bug is reported here, not fixed — it lives in
  `matty/src/builtins.py::f_lu`, a different repo/project than Qu.
- `cheby1`/`cheby2` and everything else already flagged as out of scope in
  `benchmarks/README.md`'s "Known gaps" is unaffected by this addition.
