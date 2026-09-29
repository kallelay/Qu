# Performance, accuracy and memory audit — 2026-09-28 (v0.4.4)

Benchmarks: `benchmarks/audit-2026-09-28/` (`run.sh` runs every script with
`qu run --profile` and reports peak RSS). Each case runs at two sizes so
the ratio exposes the complexity class. Accuracy probes compare against
references that are exact by construction. Release build, one Linux x86-64
container; absolute times are indicative, ratios are what matter.

## Accuracy — fixed

| Probe | Before | After | Exact |
|---|---|---|---|
| `sum([1e16] ++ 1e6 ones)` | 1e16 (lost 1e6) | 1e16 + 1e6 | 1e16 + 1e6 |
| `sum([1, 1e100, 1, -1e100])` | 0 | 2 | 2 |
| `sum` of 1e7 × 0.1 | error 1.6e-4 | exact | 1e6 |
| `cumsum` of 1e7 × 0.1, last | error 1.6e-4 | exact | 1e6 |
| `mean([1e308, 1e308])` | inf | 1e308 | 1e308 |
| `norm([3e200, 4e200])` | inf | 5e200 (1 ulp) | 5e200 |
| `norm([3e-200, 4e-200])` | 0 | 5e-200 | 5e-200 |
| `dot([1e16, 1, -1e16], ones)` | 0 | 1 | 1 |
| `var(1e12 + (0..99999))` | error 0.21 | exact | N(N+1)/12 |

How: Neumaier-compensated summation (`ksum`) for `sum`, `mean`, `dot`,
`cumsum` and the tensor `sum`/`mean`; an overflow-safe `mean` (`kmean`);
`norm` in one compensated pass with a BLAS-`nrm2`-style scaled fallback when
squares could overflow or underflow; `var`/`std` as a corrected two-pass
(Chan–Golub–LeVeque) on the compensated mean. Infinities and NaN propagate
as before. Already correct and unchanged: `var` with a 1e9 offset,
median/quantile definitions (NumPy "linear"), `logsumexp`, `log1p`, `expm1`.

## Performance — fixed

| Case | Size | Before | After | |
|---|---|---|---|---|
| `median` | 4M | 0.241 s | 0.049 s | 4.9× (selection, O(n)) |
| `quantile` | 4M | 0.220 s | 0.020 s | 10.9× |
| `table_describe` | 1M rows | 0.334 s | 0.080 s | 4.2× (uses quantiles) |
| `s = s + "x"` loop | 80k | 0.308 s | 0.037 s | 8.2×, now linear (was 11.5× per 4× n) |

`median`/`quantile` use `select_nth_unstable` with `total_cmp`, so NaN
placement and every result are identical to the old full sort (pinned by a
randomized test against the sorting definition). String building: `s = s +
...` where `s` holds a string and the terms provably run no user code
appends into `s`'s own buffer; byte-identical to the ordinary `+`, aliases
unaffected (copy-on-take), and numeric accumulators are turned away by a
syntactic check before any variable lookup (`s = s + abs(i)` stays at its
previous 20 ns/iteration).

Cost of the accuracy fixes: `sum`/`mean` 4M 6.3 → 9.0 ms, `norm` 6.7 → 9.3
ms, `std`/`var` 12.8 → 17 ms — about 1.3–1.5×, for exact answers.

## Memory — leak probes

Each probe ran N and 4N iterations under `--profile`; a leak shows as peak
RSS growing with N. `benchmarks/audit-2026-09-28/leaks/`.

| Probe | N | 4N |
|---|---|---|
| 80k streamed `print` lines | 7 MB | 8 MB |
| Office docx/xlsx new + edit + discard | 10 MB | 10 MB |
| repeated `curve_fit` with full uncertainty | 10 MB | 10 MB |
| tables, matrices, records, lists, recursion, string building | 9 MB | 9 MB |

No leaks. By design, not leaks: Office handles live until `discard` or the
end of the run; `load_library` libraries stay loaded (unloading is how a
use-after-free happens); `write_report` scripts keep a transcript of their
output. Streaming output (v0.4.3) removed the old behaviour of holding every
printed line in memory until exit.

## Measured and fine

Loops (scalar 25 ns/iter, builtin call 20 ns, indexed read/write ~35 ns),
elementwise vector math, `sort` (4M in 0.2 s), matmul (400² in 5 ms),
`solve`, FFT (power of two), `filtfilt`, `conv`, table filter/sort/group,
CSV read/write, regex and string ops, the distribution functions and the
LDA/PLS/ICA models — all scale as their algorithms should.

## Not fixed — ranked, with effort

1. **User-function call overhead** (~1 µs per call vs 20 ns for a builtin,
   `user_fn_call_loop`). The tree-walking call path allocates a frame and
   arguments per call. Biggest general win left; medium–large (frame
   pooling, argument slices).
2. **Non-power-of-two FFT** is 3.7× slower than power-of-two at 1M and
   scales worse (8.7× for 4× n at 4M). Likely the Bluestein path's
   allocation/cache behaviour; small–medium.
3. **`inv`** (400² in 0.24 s) is 25× slower than `solve` for the same
   size; a blocked LU-based inverse would close most of it; medium.
4. **`filter_ba` with long FIR** uses a direct form (0.59 s for 4M × 64);
   an FFT-based path for long filters (as `conv` already is) would be ~4×;
   small.
5. **`s = s + x[k]`** with no visibly textual term still takes the ordinary
   (quadratic) path — the price of keeping numeric accumulators free; a
   type-feedback cache per assignment site would remove it; medium.
