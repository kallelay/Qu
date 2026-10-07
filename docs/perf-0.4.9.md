# Performance: Qu vs NumPy/SciPy, and the v0.4.9 optimizations

Branch `claude/perf-0.4.9`, built on 0.4.8 (`0c875e95`). Everything here is
reproducible with `benchmarks/perf-0.4.9/` (see "How to reproduce").

## Environment

- CPU: AMD Ryzen 7 9700X, 8 cores / 16 threads, 61 GB RAM, Windows 11.
- Qu: release build (`opt-level 3`, thin LTO, 1 codegen unit) of this tree,
  `qu run`. Python 3.13.13, NumPy 2.4.5, SciPy 1.17.1, pandas 3.0.2
  (NumPy/SciPy use the bundled scipy-openblas64). Nothing was installed.
- Load: **this is a shared box and never quiet.** The baseline run was taken
  at 100% CPU load with other lanes building; the "after" run at ~45%. Raw
  per-replicate files are in `benchmarks/perf-0.4.9/results/`.
- MATLAB was **not run** (the local licence is expired/unusable for this
  lane) and no MATLAB number appears in this document. The README's MATLAB
  claims are untouched and unverified here.

## Method

- 5 replicates, Qu and Python alternating inside each replicate (and the
  order flipped each round) so drift is not charged to one side. Each
  replicate is a fresh process running every workload once; data is built
  outside the timed region (wall clock via `tic`/`toc` and `perf_counter`).
  Reported: median (min-max in `results/*/table.md`).
- A do-nothing `baseline_empty` row shows the timer floor (~10-20 us); every
  workload below is >= 0.3 ms so no subtraction is applied.
- **Two Python columns.** `Py` is NumPy/SciPy as shipped. `Py1` is the same
  with `OPENBLAS_NUM_THREADS=1`. On this loaded box default-threaded
  OpenBLAS LAPACK calls are absurdly slow (solve 200x200 took 0.21 s, versus
  0.0003 s single-threaded: spinning worker threads fighting other
  processes), so the `Py` LAPACK rows are not a fair reference. `Py1` is the
  honest "good BLAS on one core" reference. Qu's own matmul uses all cores
  (rayon), its LU/QR/SVD are single-threaded.
- CPU time was not used: Qu and OpenBLAS are multithreaded, so process CPU
  time would charge spinning threads, not work. Wall time with interleaving
  and replication is what is reported; noise (min-max) is typically 5-15%,
  up to 2x on the loaded-box rows.
- Interpreter-overhead rows (scalar loop, user-function calls, assignment
  loops) compare Qu's tree-walking interpreter with **CPython's** loop,
  which is itself slow; "Python" there is not NumPy, and a vectorized
  NumPy program would not run these loops at all. Read them as "cost of the
  loop construct", not "cost of the maths".

## Results (after the optimizations)

`Qu/Py` < 1 means Qu is faster. **Bold** = Qu more than 2x slower than the
`Py1` column (the fair one for linear algebra), "(>2x faster)" = Qu more than
2x faster.

| workload | Qu median s | Qu min-max | Py median s | Py1 median s | Qu/Py | Qu/Py1 |
|---|---|---|---|---|---|---|
| baseline_empty | 0.00001 | 0.00001-0.00001 | 0.00000 | 0.00000 | 35.33 | 26.50 **>2x slower** |
| matmul_500 | 0.00159 | 0.00137-0.00189 | 0.00103 | 0.00291 | 1.55 | 0.55 |
| matmul_1000 | 0.00685 | 0.00620-0.00731 | 0.00536 | 0.01848 | 1.28 | 0.37 (>2x faster) |
| solve_200 | 0.00063 | 0.00061-0.00067 | 0.21335 | 0.00029 | 0.00 | 2.15 **>2x slower** |
| inv_200 | 0.00175 | 0.00163-0.00180 | 0.17122 | 0.00067 | 0.01 | 2.62 **>2x slower** |
| lu_200 | 0.00069 | 0.00063-0.00075 | 0.00137 | 0.00034 | 0.50 | 2.04 **>2x slower** |
| qr_200 | 0.00183 | 0.00172-0.00205 | 0.00283 | 0.00086 | 0.65 | 2.13 **>2x slower** |
| svd_200 | 0.00870 | 0.00825-0.00951 | 0.00718 | 0.00350 | 1.21 | 2.49 **>2x slower** |
| eig_sym_200 | 0.00438 | 0.00425-0.00541 | 0.00392 | 0.00186 | 1.12 | 2.36 **>2x slower** |
| eig_gen_200 | 0.01631 | 0.01562-0.01682 | 0.01270 | 0.00958 | 1.28 | 1.70 |
| solve_500 | 0.00469 | 0.00447-0.00532 | 0.61236 | 0.00192 | 0.01 | 2.44 **>2x slower** |
| inv_500 | 0.01440 | 0.01374-0.01457 | 0.48879 | 0.00673 | 0.03 | 2.14 **>2x slower** |
| lu_500 | 0.00508 | 0.00490-0.00580 | 0.42072 | 0.00199 | 0.01 | 2.55 **>2x slower** |
| qr_500 | 0.02389 | 0.02339-0.02438 | 0.08192 | 0.00692 | 0.29 | 3.45 **>2x slower** |
| svd_500 | 0.12303 | 0.11691-0.12906 | 0.11065 | 0.02851 | 1.11 | 4.31 **>2x slower** |
| eig_sym_500 | 0.05906 | 0.05695-0.06601 | 0.06071 | 0.01582 | 0.97 | 3.73 **>2x slower** |
| eig_gen_500 | 0.22556 | 0.21664-0.23224 | 0.18967 | 0.07592 | 1.19 | 2.97 **>2x slower** |
| solve_1000 | 0.02155 | 0.02024-0.02357 | 0.36679 | 0.01021 | 0.06 | 2.11 **>2x slower** |
| inv_1000 | 0.06393 | 0.05814-0.07834 | 0.25850 | 0.04089 | 0.25 | 1.56 |
| lu_1000 | 0.02270 | 0.02130-0.02471 | 0.42752 | 0.01081 | 0.05 | 2.10 **>2x slower** |
| qr_1000 | 0.18920 | 0.18118-0.19420 | 0.19121 | 0.04011 | 0.99 | 4.72 **>2x slower** |
| fft_65536 | 0.00125 | 0.00095-0.00147 | 0.00104 | 0.00105 | 1.20 | 1.20 |
| fft_1048576 | 0.01361 | 0.01297-0.01483 | 0.02134 | 0.02080 | 0.64 | 0.65 |
| fft_1000 | 0.00035 | 0.00034-0.00060 | 0.00009 | 0.00010 | 4.04 | 3.64 **>2x slower** |
| fft_4999 | 0.00030 | 0.00026-0.00034 | 0.00026 | 0.00025 | 1.17 | 1.17 |
| fft_10007 | 0.00089 | 0.00077-0.00139 | 0.00051 | 0.00045 | 1.73 | 1.97 |
| fft_100000 | 0.00131 | 0.00125-0.00154 | 0.00172 | 0.00173 | 0.76 | 0.76 |
| fft_1000003 | 0.10278 | 0.09733-0.11769 | 0.11829 | 0.12603 | 0.87 | 0.82 |
| fft_1000_x100 | 0.00093 | 0.00079-0.00132 | 0.00101 | 0.00107 | 0.92 | 0.87 |
| fft_4999_x100 | 0.00493 | 0.00460-0.00521 | 0.02428 | 0.02439 | 0.20 | 0.20 (>2x faster) |
| filter_fir64_1e6 | 0.01725 | 0.01439-0.01951 | 0.01087 | 0.01107 | 1.59 | 1.56 |
| filtfilt_butter4_1e6 | 0.01118 | 0.01091-0.01301 | 0.01231 | 0.01254 | 0.91 | 0.89 |
| conv_64tap_1e6 | 0.00732 | 0.00650-0.00878 | 0.00989 | 0.00966 | 0.74 | 0.76 |
| cumsum_1e7 | 0.04108 | 0.03841-0.04206 | 0.03057 | 0.03173 | 1.34 | 1.29 |
| sort_1e7 | 0.04793 | 0.04337-0.05466 | 0.07972 | 0.07810 | 0.60 | 0.61 |
| unique_1e7 | 0.03735 | 0.03461-0.04049 | 0.04597 | 0.04404 | 0.81 | 0.85 |
| sum_1e7 | 0.00981 | 0.00896-0.01075 | 0.00456 | 0.00442 | 2.15 | 2.22 **>2x slower** |
| mean_1e7 | 0.00921 | 0.00882-0.00949 | 0.00467 | 0.00442 | 1.97 | 2.08 **>2x slower** |
| std_1e7 | 0.01345 | 0.01301-0.01449 | 0.02423 | 0.02305 | 0.56 | 0.58 |
| elementwise_chain_4e6 | 0.09268 | 0.09115-0.09788 | 0.09268 | 0.08843 | 1.00 | 1.05 |
| broadcast_2000x2000 | 0.00972 | 0.00900-0.01015 | 0.00933 | 0.00904 | 1.04 | 1.08 |
| matrix_assign_loop_1e6 | 0.28463 | 0.28238-0.29050 | 0.06012 | 0.06033 | 4.73 | 4.72 **>2x slower** |
| row_slice_sum_1000 | 0.00431 | 0.00393-0.00514 | 0.00243 | 0.00242 | 1.78 | 1.78 |
| col_slice_assign_1000 | 0.00316 | 0.00313-0.00468 | 0.00121 | 0.00120 | 2.60 | 2.63 **>2x slower** |
| scalar_loop_1e7 | 0.11971 | 0.11785-0.12107 | 0.22294 | 0.21348 | 0.54 | 0.56 |
| user_fn_call_1e6 | 0.45448 | 0.42764-0.49057 | 0.03078 | 0.02970 | 14.77 | 15.30 **>2x slower** |
| while_loop_3e6 | 0.05054 | 0.04530-0.06157 | 0.08236 | 0.09006 | 0.61 | 0.56 |
| str_build_join_1e5 | 0.08177 | 0.07834-0.08720 | 0.00774 | 0.00841 | 10.57 | 9.72 **>2x slower** |
| str_split_1e5 | 0.00405 | 0.00377-0.00490 | 0.00232 | 0.00283 | 1.75 | 1.43 |
| regex_count_20k | 0.00115 | 0.00078-0.00127 | 0.00114 | 0.00114 | 1.01 | 1.01 |
| regex_replace_20k | 0.00267 | 0.00255-0.00426 | 0.00260 | 0.00257 | 1.03 | 1.04 |
| csv_write_1e6 | 0.75925 | 0.73639-0.80615 | 1.49136 | 1.48776 | 0.51 | 0.51 |
| csv_read_1e6 | 0.08687 | 0.08444-0.08949 | 0.20093 | 0.20297 | 0.43 | 0.43 (>2x faster) |
| dict_5e3_set_get | 0.37180 | 0.35376-0.41516 | 0.00101 | 0.00088 | 366.42 | 423.66 **>2x slower** |
| list_1e5_append_get | 0.04048 | 0.04015-0.04410 | 0.00380 | 0.00412 | 10.66 | 9.83 **>2x slower** |
| record_field_loop_1e6 | 0.29641 | 0.28758-0.30986 | 0.03069 | 0.02700 | 9.66 | 10.98 **>2x slower** |
| ode45_sho_t1000 | 0.05216 | 0.05089-0.05392 | 0.28371 | 0.31297 | 0.18 | 0.17 (>2x faster) |
| quad_smooth_x200 | 0.01949 | 0.01879-0.02251 | 0.01013 | 0.01138 | 1.92 | 1.71 |
| trapz_1e7 | 0.01929 | 0.01861-0.01988 | 0.03476 | 0.03687 | 0.55 | 0.52 |
| npy_write_1e7 | 0.06898 | 0.06518-0.21568 | 0.04247 | 0.04241 | 1.62 | 1.63 |
| npy_read_1e7 | 0.02864 | 0.02745-0.03071 | 0.01943 | 0.01910 | 1.47 | 1.50 |
| sparse_build_poisson100 | 0.00106 | 0.00104-0.00121 | 0.00109 | 0.00116 | 0.97 | 0.92 |
| sparse_lu_poisson100 | 0.10531 | 0.10176-0.11442 | 0.01370 | 0.01378 | 7.69 | 7.64 **>2x slower** |
| sparse_cg_poisson100 | 0.00802 | 0.00773-0.01123 | 0.00706 | 0.00701 | 1.14 | 1.14 |
| sparse_cgj_poisson100 | 0.00856 | 0.00804-0.01046 | 0.00699 | 0.00737 | 1.22 | 1.16 |

(`baseline_empty` is the timer floor; its ratio is meaningless.)

### What the table says

Where Qu is **faster** than NumPy/SciPy on this machine (after): `ode45` on a
2-state problem to t=1000 (0.05 s vs 0.28 s, ~5x), `read_csv` of 1e6 rows
(2.3x) and `write_csv` (2x), `trapz` 1e7 (1.8x), `std` 1e7 (1.8x),
non-power-of-two FFT repeated on the same length (4999 x100: 5x, the cached
plan), `sort` 1e7 (1.7x), `unique` 1e7, `conv`, `filtfilt`, large power-of-two
FFT and the scalar `for`/`while` loop (~2x CPython). Matmul is within 0.4-1.5x
of the OpenBLAS result (faster than single-threaded OpenBLAS at 1000x1000,
slower than the all-threads run).

Where Qu is **more than 2x slower** than the fair reference:

| Case | Qu / reference | Why |
|---|---|---|
| `qr`, `svd`, `eig` (200-1000) | 2-5x vs `Py1` (about 1x vs default threads) | nalgebra's unblocked Householder/QR-iteration kernels; single threaded. Not fixed (below). |
| `solve`/`lu`/`inv` 200-1000 | ~2x vs `Py1` (after the fix; was 20-50x) | blocked LU in safe Rust is still about half LAPACK's single-core speed |
| `sum`/`mean` 1e7 | 2x | deliberate: compensated (Neumaier) summation from the 0.4.4 accuracy audit; exact answers instead of 1.6e-4 drift |
| `fft` 1000 (first call) | 4x | rustfft plans per length on first use (0.35 ms once); repeated calls are 1.0x |
| `matrix_assign_loop` 1e6 (`M[i,j] = x`) | 4.7x CPython | interpreter cost per assignment (0.28 us); CPython/NumPy scalar assignment is 0.06 us |
| `col_slice_assign` | 2.6x | small (3 ms), per-call overhead |
| `user_fn_call` 1e6 | 15x CPython | 0.45 us per Qu call versus 0.03 us for a CPython call to a trivial function (CPython's inlined call path is very cheap) |
| `str_build_join` 1e5 / `list_append_get` 1e5 | ~10x | per-iteration interpreter cost (0.4-0.8 us), `append` allocates a new list value |
| `record_field_loop` 1e6 | 10x | each `rec.a` read costs ~0.15 us |
| **`dict` 5000 `set`+`get`** | **~370x** | **dict is an association list: `get` is a linear scan and `set` copies the whole dict, so n inserts are O(n^2).** 0.37 s for 5000; 1e5 did not finish in 2 minutes. |
| sparse LU, 2-D Poisson 100x100 | 7.7x | Gilbert-Peierls with reverse Cuthill-McKee ordering, versus SuperLU with COLAMD; CG (1.1x) is fine |

Honest reading of the interpreter rows: a script that loops 1e6-1e7 times
over scalars runs at 0.1-0.5 us/iteration. That is faster than CPython for
plain arithmetic loops and 5-15x slower for anything that goes through a
function call, a record, a dict or a matrix cell. Vectorized code (the normal
way to write Qu, as in NumPy) is at parity or ahead.

## What was optimized (paired before/after)

"Before" is the 0.4.8 binary, "after" this branch's, run **alternately,
5 pairs, in the same minute** (`paired.sh`; `results/paired/table.md`), so
box load cancels. The absolute numbers differ from the table above because the
box load differed between the runs; the paired ratio is the claim.

| Optimization | Case | Before | After | Paired ratio |
|---|---|---|---|---|
| Blocked LU inverse (`inv`), guarded (below) | 1000x1000 | 1.253 s | 0.091 s | **13.8x** |
| | 500x500 | 0.144 s | 0.018 s | 8x |
| | 200x200 | 10.7 ms | 2.4 ms | 4.4x |
| Blocked LU for `solve` | 1000x1000 | 62 ms | 31 ms | 2.0x |
| Blocked LU for `lu` | 1000x1000 | 71 ms | 30 ms | 2.4x |
| | 500x500 | 10.2 ms | 6.2 ms | 1.7x |
| Parallel sort (`sort`, `unique`, >= 65536 values) | 1e7 | 0.281 s | 0.058 s | **4.9x** |
| | `unique` 1e7 | 0.148 s | 0.049 s | 3.0x |
| `filter_ba` direct form without per-tap lookups | 64 taps x 1e6 | 40 ms | 19 ms | 2.1x |
| User-function call: no AST deep copy per call | 1e6 calls of a 1-line function | 0.65 s | 0.47 s | 1.4x |
| | 1e5 calls of a ~15-statement function | 1.0 s | 0.52 s | **2.0x** |
| | `quad` x200 (callbacks) | 93 ms | 21 ms | 4.4x |
| | `ode45` t=1000 | 92 ms | 61 ms | 1.5x |
| `M[i, j] = x` direct cell write | 1e6 assignments | 0.41 s | 0.30 s | 1.4x |
| `read_npy` float64 fast path | 1e7 doubles | 0.121 s | 0.050 s | 2.4x |

(The user-call row's gain depends on function size because the removed cost
was a deep copy of the function's parameter list and body AST on every call:
`resolve_method` cloned the whole `MethodEntry`.)

### What each change does, and its safety argument

1. **Dense LU (`qu-core/src/dense_lu.rs`)**: right-looking blocked LU with
   partial pivoting (64-column panels; the trailing update and the
   triangular solves are `Matrix::matmul` calls on the existing SIMD/rayon
   GEMM). Same pivot rule as nalgebra/LAPACK (first maximal |entry|), same
   "exact zero pivot means decline" contract as nalgebra's `solve`. No new
   dependency, no `unsafe`. Used by `solve`/`\` (square), `lu` (square) and
   `det`; rectangular `lu` still uses nalgebra.
2. **`inv`** previously computed a full SVD pseudo-inverse (that is what made
   it 25x slower than `solve`). It now tries the blocked LU first and **only
   accepts the answer when the matrix is provably nonsingular by the very
   test the SVD route used**: the old code reported "singular" when
   `sigma_min <= n*eps*sigma_max`, and for an n x n matrix
   `cond_2 <= n * cond_1`, with `cond_1` exactly computable from the inverse in
   hand, so `cond_1 * n^2 * eps < 1/4` guarantees the SVD route would have
   returned full rank. Anything else (exact zero pivot, borderline or
   singular conditioning, non-finite values) falls through to the old SVD
   code unchanged, so `inv([1,2;2,4])` and the 3x3 `[1..9]` matrix still raise
   the same "matrix is singular (rank r of n)" error. Accepted results differ
   from the SVD route only in rounding (residual `max|A*inv(A)-I|` < 1e-9 at
   n=150 is pinned in a test; typically ~1e-13).
3. **`sort`/`unique`**: `par_sort_unstable_by(total_cmp)` above 65536
   elements. Elements equal under `total_cmp` are bit-identical (it orders
   -0.0 < +0.0 and NaN payloads), so the output is byte-identical to the
   stable sort; verified (below).
4. **`filter_ba`**: padded coefficient vectors and a slice loop instead of
   `bn.get(k+1)` / `an.get(k+1)` per tap per sample. Identical operations in
   identical order, so results are bit-identical (verified).
5. **User calls**: `MethodEntry` holds `Arc<Vec<Param>>` and
   `Arc<MethodBody>`, so the clone `resolve_method` hands out per call is two
   refcount increments instead of a deep AST copy. Redefinition still replaces
   the entry (tested).
6. **`M[i, j] = x`**: when both indices are scalars and the operator is
   plain `=` and the value is a number, write the cell directly (same
   evaluation order: row, column, value; same copy-on-write via
   `Arc::make_mut`; same error for out-of-bounds; compound `+=`, slices and
   block assignments take the old path).
7. **`read_npy`**: little-endian float64 decoded with `from_le_bytes` over
   chunks instead of the per-element dtype dispatcher (bit-identical; every
   other dtype keeps the old path).

### Verification

- `ident.qu`-style check (old and new binaries on the same inputs, 17
  significant digits): `filter_ba` (64-tap FIR, IIR, trivial), `sort`,
  `unique`, `read_npy` of a written file: **identical output**; `det` of a
  60x60 identical to 10 digits.
- New tests: `qu-core/src/dense_lu.rs` (5 unit tests: residuals across the
  64-column block boundary at n = 1, 2, 3, 17, 63, 64, 65, 130, 200;
  inverse x matrix = identity; determinant; singular/borderline matrices
  decline; `P*A = L*U` reconstruction) and
  `engine/crates/qu-interp/tests/perf_0_4_9.rs` (7 language-level tests:
  inverse accuracy and `det(A)*det(inv A) = 1`, singular errors preserved,
  `lu` factors, a 200000-element sort with NaN and signed zeros, `filter_ba`
  against a hand-written recurrence, matrix cell-assignment semantics
  including aliasing, user-function redefinition).
- Existing suites (`cargo test --release --workspace --no-fail-fast`): see the
  test record at the end of this file.

## What was NOT fixed, and why

1. **`dict` is O(n) per operation** (association list; `get` scans, `set`
   copies the whole dict). The largest ratio in the table (~370x at 5000
   keys) and the real scalability cliff found by this audit. The fix is a
   representation change (an insertion-ordered hash map behind the
   `Arc`, plus an in-place `d = set(d, k, v)` path like the string-append
   one); it touches every `Value::Dict` consumer (about 20 match sites), so it
   was not attempted as a low-risk change. Highest-value follow-up.
2. **`qr`, `svd`, `eig`** (2-5x slower than single-thread LAPACK): nalgebra
   kernels. A blocked/Householder-WY QR on the new GEMM path is the
   natural next step for `qr`; `svd`/`eig` need bidiagonalization/Hessenberg
   reductions, which is a larger project. No heavy dependency (`faer`) was
   added: it is not in the vendored registry here and adding it was not
   justified by 2x.
3. **Sparse LU** (7.7x slower than SuperLU): needs a COLAMD-class ordering
   (documented in the 0.4.7 notes). CG/BiCGSTAB are at parity.
4. **User-function call cost** is down 1.4-2x but still ~0.45 us. What remains
   is structural: call frames are `HashMap<String, Value>` (a `String` clone
   and SipHash per parameter), `call_tracked` allocates the callee name for the
   error-trace stack on every call, `reject_unknown_kwargs`, and the
   `resolve_method` + dispatch-cache double lookup. Fixing it properly means
   slot-resolved frames; large.
5. **Record field read** (`rec.a`, ~0.15 us), **list append** and **string
   join** loops: same class (interpreter allocation per iteration).
6. **Compensated `sum`/`mean`** are deliberately 2x slower than NumPy's
   pairwise sum (they are exact where NumPy's is not); kept.
7. **Non-power-of-two FFT**: the audit's "3.7x slower than power of two" is
   rustfft's algorithm cost for those lengths (mixed radix / Bluestein), at
   parity with or ahead of NumPy at every size measured (4999, 10007, 100000,
   1000003); the apparent 4x at n=1000 is first-call planning (0.35 ms once).
8. Not measured: CPU time per process, GPU, memory use, and MATLAB.

## How to reproduce

```
cd engine && cargo build --release -p qu-cli          # ~10 min
bash benchmarks/perf-0.4.9/run.sh 5 mytag             # Qu vs Python vs Python 1-thread BLAS
bash benchmarks/perf-0.4.9/paired.sh old.exe new.exe 5 pair   # before/after of two Qu binaries
```

`run.sh` writes `results/<tag>/{qu,py,py1}_<k>.txt` and prints the table via
`agg.qu` (aggregation is itself a Qu script). Scripts: `bench.qu` +
`bench_sparse.qu` (separate file because `import sparse` makes `solve` and
`transpose` ambiguous with the dense builtins) and `bench.py`; workload names
match one to one. Needs Python with numpy, scipy and pandas (not installed by
the scripts).

## Test record

`cd engine && RUST_MIN_STACK=134217728 cargo test --release --workspace --no-fail-fast < /dev/null`:
56 test binaries, 3342 passed, 1 failed. The one failure,
`every_repo_qu_file_formats_idempotently_and_preserves_tokens`, was caused by
the new `benchmarks/perf-0.4.9/bench_sparse.qu` itself (a stray blank line with
CRLF endings made `qu fmt` refuse it, "internal error: formatting would change
the token stream"); after fixing the file it passes alone:

```
test every_repo_qu_file_formats_idempotently_and_preserves_tokens ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 86.97s
```

The qu-interp library suite in the full run:
`test result: ok. 2272 passed; 0 failed; 1 ignored` (including
`tests::string_self_append_is_in_place_and_identical ... ok`; the known
wall-clock flake did not trigger). New tests:
`test result: ok. 7 passed; 0 failed` (`perf_0_4_9`) and
`test result: ok. 5 passed` filtered to qu-core `dense_lu`
(`5 passed; 0 failed; 0 ignored; 0 measured; 397 filtered out`).

Side finding for the `qu fmt` owner: a file ending in a blank line with CRLF
endings makes `qu fmt` fail with the token-stream error instead of trimming
the line.

## Addendum: `dict` (fixed after the first write-up)

The first version of this document listed `dict` as the largest unfixed cliff: it was an
association list, so `get` was a linear scan and every `set` copied the whole dict (5000
`set` + `get` took ~370x longer than Python; 100,000 inserts did not finish in two
minutes).

`Value::Dict` is now `Arc<DictData>` (`src/dict_data.rs`): insertion-ordered pairs plus a
lazily built hash index (only for dicts over 16 entries). `get` uses the index. The
statement `d = set(d, k, v)` takes the dict out of its variable, so it is uniquely owned and
`Arc::make_mut` updates it in place, keeping the index current (`try_selfrebind_set` in
`lib.rs`, modelled on the existing `append` fast path; it is skipped when the script
defines its own `set`). Any other `set` call still returns a copy, as documented.
`dict(keys, values)` no longer does a linear duplicate check per key.

Measured on the same machine (one run each, `D:\qu-project\qu-audit-0.4.8\dict_bench.qu`):

| workload | before | after |
|---|---|---|
| 100,000 `d = set(d, "k"+str(i), i)` | > 120 s (did not finish) | 0.18 s |
| 100,000 `get(d, "k"+str(i))` | (quadratic) | 0.21 s |

The remaining per-operation cost (~2 us) is the interpreter loop and the string
concatenation for the key, not the dict. Tests: `tests/dict_scale.rs` (aliases never see an
in-place update, a user-defined `set` is respected, key order, number-key normalisation,
records keep their type) and the unit tests in `dict_data.rs`.
