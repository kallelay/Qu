# QR, SVD, and linear solve at scale vs numpy.linalg

A fourth advanced-language stress test, deliberately in the same
numerics/linear-algebra neighborhood as
[`curve_kalman/`](../curve_kalman/README.md) but attacking a different
question: not "does a complex algorithm (LM, Kalman) converge to the right
answer," but "do Qu's core dense-matrix factorizations stay numerically
correct and reasonably fast once matrices get big" — well beyond
`catalog/qu_qr_svd.qu`'s hand-checkable 4x2 toy, at sizes in the same
ballpark as `benchmarks/linalg/bench.qu`'s own 100/500/1000 ladder (which
already stress-tests `eig`/`qr`/`lu`/`det`/`rank`/`svd` at scale, but never
cross-checks against numpy — that's the gap this fills). Three builtins,
two sizes:

- `qr(A)` — QR decomposition, `A = Q*R`
- `svd(A)` — singular value decomposition, `A = U*diag(s)*V'`
- `A \ b` — MATLAB-style left division (linear solve)

on **n=300** and **n=1000** square matrices, each checked against
`numpy.linalg.qr`/`svd`/`solve` on the byte-identical input.

Run:

```bash
python benchmarks/advanced_validation/linalg_stress/make_data.py       # once, writes a_n*.csv/b_n*.csv
qu run benchmarks/advanced_validation/linalg_stress/linalg_stress.qu   # also dumps x_qu_n*.csv
python benchmarks/advanced_validation/linalg_stress/linalg_stress.py   # diffs against it
```

## Getting the SAME matrix into both languages

Qu has no raw numeric-CSV-matrix loader and no dynamic/reflective column
access — table columns are referenced by a literal `df.colname` at parse
time, so there's no way to loop over hundreds or thousands of dynamically
named columns to rebuild a matrix one column at a time. Instead
`make_data.py` writes each `n x n` matrix as a **single-column CSV**,
flattened in **column-major order**:

```python
a_flat = A.flatten(order="F")   # COLUMN-MAJOR -- see below for why
pd.DataFrame({"val": a_flat}).to_csv(f"a_n{n}.csv", index=False)
```

and the Qu side reconstructs it with `reshape`:

```
a_df = read_csv(DATA + "a_n{n}.csv")
A = reshape(a_df.val, n, n)
```

Column-major is not a stylistic choice — it is required to avoid a silent
transpose. Confirmed directly against the interpreter source: `reshape`'s
handler (`reshape_to` in `engine/crates/qu-interp/src/lib.rs`) calls
`Matrix::from_col_major_checked` whenever the input vector's length equals
`rows*cols`. Flattening `A` row-major and then calling `reshape(v, n, n)`
would silently hand Qu `A^T` instead of `A` — for a generic (non-symmetric)
matrix this produces a *different*, still-plausible-looking QR/SVD, not an
obvious crash, which is exactly the kind of bug that would slip past a
casual look at the output. The right-hand-side vector `b` needs no such
care (`read_csv(...).val` is already a plain vector, and `A \ b` accepts a
bare `Vec` — `into_matrix()` treats it as a column).

`make_data.py` builds each matrix as `A = M @ M.T + n*np.eye(n)` for a
guaranteed-symmetric-positive-definite, well-conditioned system regardless
of the random draw (`cond(A)` came out at **4.99** for n=300 and **4.92**
for n=1000 — nowhere near ill-conditioned). `a_n300.csv`/`a_n1000.csv`
(1.8MB/19.9MB, 90,000/1,000,000 flattened values) are **not committed** —
`.gitignore` gained two new lines following the exact precedent already
set by `../peak_finding/`'s noise-signal CSVs (regenerate with
`make_data.py`); the small `b_n*.csv` vectors and Qu's dumped
`x_qu_n*.csv` solve results stay committed.

## Results — accuracy: double-precision round-off on both sides

All residuals below are `max(abs(...))` over every element of the
reconstructed matrix (`A - Q*R` / `A - U*diag(s)*V'`), matching
`catalog/qu_qr_svd.qu`'s own convention exactly (not the Frobenius norm
`benchmarks/linalg/bench.qu` uses — a stricter, per-element bar).

| n | Qu QR resid | numpy QR resid | Qu SVD resid | numpy SVD resid |
|---|---:|---:|---:|---:|
| 300 | 1.364e-12 | 4.547e-13 | 1.705e-12 | 1.592e-12 |
| 1000 | 7.731e-12 | 2.046e-12 | 9.550e-12 | 1.069e-11 |

Both languages land at **1e-13 to 1e-11** — double-precision round-off, an
order of magnitude tighter than the task's 1e-10 bar, on both engines, at
both sizes. Qu's residuals run consistently ~2-4x larger than numpy's,
which is unremarkable — different QR/SVD algorithms (Householder vs.
whatever LAPACK/OpenBLAS variant numpy dispatches to) accumulate
round-off differently, and both are still far inside "double-precision
correct."

### Solve: not just low-residual, the actual elementwise-identical `x`

| n | max\|Qu_x − numpy_x\| |
|---|---:|
| 300 | 4.959e-07 |
| 1000 | 4.999e-07 |

Both engines converge on the **same** solution vector `x`, to ~5e-7 —
exactly the same order of magnitude as `../curve_kalman/README.md`'s own
Kalman-filter finding (5e-7), and for the identical reason: `write_csv`
round-trips through ~6 decimal digits of text precision, and `x` here is
dumped to CSV specifically so this script could diff it elementwise against
numpy's own `x` rather than trusting a residual norm. 5e-7 is the CSV
format's floor, not a solver disagreement — Qu and numpy agree to the
precision the comparison method itself allows.

## Results — speed

Real numbers, same machine, same freshly-built release `qu.exe`
(`engine/target/release/qu`), Qu timed with
`tic()`/`toc()`, numpy with `time.perf_counter()`. **This machine was under
real, uncontrolled load while these numbers were taken** — multiple
concurrent Claude sessions plus Dropbox sync, the same caveat
`benchmarks/linalg/README.md` documents for its own 100/500/1000 run — and
it hit numpy's *default multi-threaded* OpenBLAS path much harder than
Qu's single-threaded interpreter: `np.linalg.solve` alone measured
**580–940 ms at n=300** across five repeated calls on a *freshly generated,
unrelated* matrix (isolated outside this script, to rule out anything CSV
or Qu-specific) — over 100x its normal cost for a 300x300 solve. Pinning
`OPENBLAS_NUM_THREADS=1` dropped that same call to **0.5–0.8 ms**, and
every other numpy timing in this section improved too (n=1000 `svd` alone
went from ~1.2-1.5s multi-threaded to ~0.2s single-threaded) — clear
evidence this is OpenBLAS's threaded code path thrashing under core
contention on a busy shared machine, not a property of the algorithm or
the data. **Both readings are reported below**; the single-threaded column
is the fairer "quiet machine" proxy given this environment's real
constraints, following the same spirit as `linalg/README.md` discounting
its own noisy `det`/`lu` outliers.

| n | op | Qu | numpy (multi-thread, noisy) | numpy (`OPENBLAS_NUM_THREADS=1`) | ratio (vs. single-thread) |
|---|---|---:|---:|---:|---:|
| 300 | qr | 8.2 ms | 7.5–10.4 ms | 2.5–4.2 ms | ~2-3x slower |
| 300 | svd | 42.0 ms | 18.7–27.8 ms | 9.2–9.8 ms | ~4x slower |
| 300 | solve | 42.1 ms (BEFORE fix) | 775–997 ms | 0.79–0.89 ms | ~50x slower (BEFORE fix) |
| 1000 | qr | 267 ms | 765–1031 ms | 51–56 ms | ~5x slower |
| 1000 | svd | 1441 ms | 1204–1483 ms | 198–209 ms | ~7x slower |
| 1000 | solve | 1515 ms (BEFORE fix) | 1191–1627 ms | 11.4–11.7 ms | ~130x slower (BEFORE fix) |

### `solve` BEFORE/AFTER: the LU fast-path fix (2026-08-31)

**Fixed 2026-08-31** (see the root-cause section below): `A \ b` no longer
routes through a full SVD pseudoinverse for a square system — it takes a
direct LU solve instead, falling back to SVD only when `A` isn't square or
LU reports a singular system. Real before/after numbers, same machine,
same freshly-built release `qu.exe`, same CSVs, `tic()`/`toc()` timing on
the Qu side both times:

| n | Qu solve BEFORE | Qu solve AFTER | numpy (`OPENBLAS_NUM_THREADS=1`) | ratio BEFORE | ratio AFTER |
|---|---:|---:|---:|---:|---:|
| 300 | 42.1 ms | 1.66 ms | 0.79–0.89 ms | ~50x slower | **~2x slower** |
| 1000 | 1515 ms | 68.2 ms | 11.4–11.7 ms | ~130x slower | **~6x slower** |

A ~25x (n=300) to ~22x (n=1000) speedup from the LU fast path alone,
closing almost all of the architectural gap — the residual ~2-6x against
numpy's single-threaded OpenBLAS is the same already-documented,
already-understood `qr`/`svd` gap (nalgebra's dense LU is pure-Rust, no
vendored BLAS/LAPACK underneath), not a new finding. Accuracy is
unchanged: the freshly re-dumped `x_qu_n300.csv`/`x_qu_n1000.csv` after
the fix are byte-identical to the committed pre-fix files, and re-running
`linalg_stress.py` reproduces the exact same `max|Qu_x - numpy_x|` as
before (4.959e-07 at n=300, 4.999e-07 at n=1000).

`qr`/`svd` losing to numpy by ~2-7x is the same, already-documented,
already-explained gap `benchmarks/linalg/README.md` found: nalgebra's dense
SVD/QR are pure-Rust with no BLAS/LAPACK acceleration underneath, the same
root cause behind every other "Qu trails a vendor-tuned library" gap this
repo has found (`fft` before the `rustfft` swap, `element_wise_ops` before
parallelization). Not a new finding — reconfirmed at a new size.

## A real, root-caused performance finding: `A \ b` pays for a full SVD it doesn't need (fixed 2026-08-31)

**Status: fixed.** `qu-core::linalg::least_squares` (`qu-core/src/linalg.rs`)
now tries a direct LU solve first whenever `coefficients` is square,
finite, non-empty, and no explicit `relative_tolerance` was requested
(`relative_tolerance` given at all is treated as "the caller wants SVD's
tolerance-based rank handling," e.g. `eis.rs`'s regularized DRT fit,
`Some(1e-10)`, which is left untouched); LU declining — non-square, or a
zero pivot after partial pivoting, exactly the failure signal `lu()`
already relies on elsewhere — falls straight back to the original
`pseudo_inverse` path, unchanged. See the BEFORE/AFTER table above for
real timings and the confirmation that `x` is unchanged to the byte.

`solve`'s ~50-130x gap is **not** load noise or algorithmic parity with a
harder problem — it reproduces identically across every repeated run
(Qu's own solve time is essentially indistinguishable from Qu's own `svd()`
time at the same `n`: 42.1 ms vs. 42.0 ms at n=300, 1515 ms vs. 1441 ms at
n=1000). That's because it *is* the same computation: tracing the `\`
operator (`"\\" =>` arm, `engine/crates/qu-interp/src/lib.rs` line ~19886)
shows it calls `numeric::linalg::least_squares` (`qu-core/src/linalg.rs`
line ~157), which **unconditionally** computes a full `pseudo_inverse`
(same file, line ~62) — and `pseudo_inverse` **unconditionally** takes a
full SVD (`source.svd(true, true)`, line ~79) regardless of whether `A` is
square and well-conditioned. For an `n x n` well-conditioned system (this
scenario's whole test case, and the overwhelmingly common case for `A \ b`
in practice), a direct LU-based solve is the textbook-correct choice —
`O(n^3/3)` flops with a small constant, exactly what `numpy.linalg.solve`
and MATLAB's `\` both dispatch to — versus SVD's `O(n^3)` with a
substantially larger constant (multiple bidiagonalization + iterative QR
sweeps per factor). Qu already has a working, verified LU decomposition
(`lu(A)`, exercised at this same scale in `benchmarks/linalg/bench.qu`) —
the fix is not "add a new algorithm," it's "give `\` a fast path: LU-solve
when `A` is square, fall back to the existing SVD-based pseudoinverse only
for a genuinely rectangular/rank-deficient `A`," mirroring exactly the kind
of dispatch numpy/MATLAB already do. This is **not a correctness bug** —
the answer `x` matches numpy to CSV round-trip precision (5e-7) at both
sizes — purely a performance/architecture gap, reported here rather than
fixed directly per this exercise's rule about not touching shared engine
source, for a central session to pick up.

## Honest verdict

**Accuracy is a clean pass**: `qr`, `svd`, and `\` all land at
double-precision round-off (1e-13 to 1e-11 residuals, comfortably inside
the 1e-10 bar) at both n=300 and n=1000, and the solved vector `x` matches
numpy's elementwise to the precision the CSV comparison method itself
allows (5e-7). No correctness bug found in any of the three builtins.
**Speed is a mixed, but honestly-reported, picture**: `qr`/`svd` trail
numpy by a re-confirmed, already-understood ~2-7x (no vendored
BLAS/LAPACK under nalgebra's dense factorizations) — unsurprising and
consistent with `benchmarks/linalg/README.md`. `A \ b` was a genuine,
root-caused, and (as of 2026-08-31) **fixed** finding: it used to cost as
much as a full SVD because that is literally what it computed even for a
plain square solve, a ~50-130x gap against numpy's LU-based `solve` that
had nothing to do with machine noise (verified by pinning BLAS threads and
by Qu's own solve time tracking its own `svd()` time almost exactly) and
everything to do with `qu-core::linalg::least_squares` never checking
whether a cheaper direct solve applies before reaching for the
pseudoinverse. Giving `\` an LU fast path for the square case cut the gap
to ~2-6x — in the same ballpark as the already-understood `qr`/`svd` gap —
a ~22-25x real speedup, with accuracy unchanged to the byte.
