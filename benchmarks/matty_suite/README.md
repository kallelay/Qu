# matty_suite — cross-language benchmark, ported from matty

`matty/benchmarks/benchmark_suite.py` is matty's (the sibling MATLAB-interpreter
project vendored under `matty/`) own standard 5-kernel benchmark: matrix
multiply, element-wise ops, FFT, array creation, loop performance — same
problem sizes it uses to compare its own Julia/JAX/Octave backends. Porting it
gives Qu a same-sizes, same-kernels comparison point against MATLAB, numpy,
and matty itself, per the standing "monitor performance by correctness and
speed against MATLAB, Octave, Matty, Python (variants) and our software"
brief.

Three same-code ports, one per target:

- `bench.qu` — Qu, using `tic()`/`toc()` (MATLAB-compatible real wall-clock).
- `bench.py` — plain numpy, timed with `time.perf_counter()`.
- `bench.m` — MATLAB, using `tic`/`toc`. The `R2026a` install has an expired
  trial license (Error -10.2); `R2025b` is also installed and works — run it
  via its own `matlab` binary (`/c/Program Files/MATLAB/R2025b/bin/matlab`
  vs. `.../R2026a/bin/matlab`) if you need a specific version. Octave was
  not found anywhere on this machine (checked `Program Files`, `PATH`,
  common install dirs) despite being expected — worth installing if an
  Octave data point matters; matty vendors an Octave *source* tree under
  `matty/octave-9.4.0/` but it isn't built.
- `bench_fair.qu` / `bench_fair.py` / `bench_fair.m` — the same five kernels,
  measured so the per-kernel numbers describe the per-kernel labels: data
  built OUTSIDE the timed region, one warm-up run discarded, best of three
  reported. Use THESE for any per-kernel claim; `bench.*` keeps its single
  cold trial with data generation inside the clock, which is a legitimate
  end-to-end "how long does this script take" figure and nothing more.
- `run_via_matty_jax.py` — drives matty's own `BenchmarkSuite` against its
  `JAXBackend` (CPU; GPU not exercised here), using matty's project-local
  venv. Run from `matty/`: `./.venv/Scripts/python.exe
  ../benchmarks/matty_suite/run_via_matty_jax.py`.

## Results

Not statistically rigorous (single trial each unless noted, shared machine,
`randn`/`rand` draw different data per language so element-wise/FFT results
aren't bit-comparable — only shapes and timing are; this machine also runs
other concurrent work at times, so absolute numbers move run to run — see
the note under the table). Four Qu snapshots across one day's work
(BOARD.md has the full writeup for each):

| kernel | Qu (Arc-COW only) | Qu (+ frames/fast loops) | Qu (+ parallel elementwise/RNG) | Qu (+ rustfft) | numpy | matty (JAX, CPU) | MATLAB R2025b |
|---|---:|---:|---:|---:|---:|---:|---:|
| matrix_multiply (1000×1000) | 0.138 s | 0.083 s | 0.066–0.16 s | — | 0.204 s | 0.091 s | 0.05–0.21 s |
| element_wise_ops (10000×10000) | 16.589 s | 8.893 s | **1.9–4.6 s** | — | 14.037 s | 10.940 s | 1.77–4.22 s |
| fft (100000-pt) | 0.078 s | 0.053 s | 0.05–0.13 s | **0.0086–0.0155 s** | 0.0315–0.0629 s | 0.185–0.295 s | 0.0769–0.1775 s |
| array_creation (5000×5000 ×4) | 0.227 s | 0.086 s | 0.05–0.26 s | — | 0.377 s | 0.281 s | 0.14–0.43 s |
| loop_performance (100000 iters) | 0.060 s | 0.0008 s | 0.0008–0.0011 s | — | 0.0049 s | 0.641 s | 0.0007–0.0112 s |

**Same-moment, back-to-back runs (fairest single comparison, immune to the
machine-load swings above):**
- `element_wise_ops`: Qu **3.2554 s** vs MATLAB **3.2520 s** — a 0.01%
  difference, i.e. matched, not "close." This is the kernel that was
  **~9.3x behind MATLAB** (16.589s vs 1.789s) at the start of this day's
  work.
- `fft`: Qu **0.0155 s** vs numpy **0.0315 s** vs MATLAB **0.0769 s** —
  Qu is now **2.0x faster than numpy and 5.0x faster than MATLAB**, from
  being the one kernel this suite hadn't touched. `qu-core`'s hand-written
  radix-2 + Bluestein FFT was swapped for `rustfft` (MIT/Apache-2.0, pure
  Rust, SIMD-dispatched) behind the same `fft_complex`/`ifft_complex`
  API — a real **~5x** speedup on Qu's own number (0.080s → 0.0155s).
  FFTW itself was considered and rejected: GPL-licensed against this
  workspace's MIT/Apache-2.0, and a C library via FFI breaks the wasm32
  target `rustfft` doesn't. See BOARD.md.

(The wide ranges above — everything moved ~2-3x between quiet and loaded
moments, for Qu *and* MATLAB roughly equally — are machine-load noise, not
a real regression or improvement between runs; a same-moment comparison
like the one above is the only fair read at this scale. `loop_performance`
is sub-millisecond for both Qu and MATLAB now, so its spread is pure noise
too, not a real difference in either direction.)

## Fair-method results (`bench_fair.*`, re-measured 2026-09-16)

Qu built from `master` `ccd105cb`. Method: three interleaved rounds
Qu -> Python -> MATLAB R2025b, each script internally discarding a warm-up and
reporting best-of-three; the table is the minimum across rounds. Interleaving
matters on this shared box — machine-wide drift then hits all three languages
roughly equally instead of penalising whichever ran last. All nine runs exited
0 and all three printed `result = 60000003` from the loop kernel, which is the
cross-language correctness check.

| kernel | Qu | numpy 2.4.5 | MATLAB R2025b | Qu vs MATLAB |
|---|---:|---:|---:|---|
| matrix_multiply (1000×1000) | **0.0075 s** | 0.0051 s | 0.0090 s | **Qu 1.20x faster** |
| element_wise_ops (3000×3000) | 0.0678 s | 0.1586 s | **0.0378 s** | MATLAB 1.79x faster |
| fft (100000-pt) | 0.0021 s | 0.0033 s | **0.0009 s** | MATLAB 2.33x faster |
| array_creation (5000×5000 ×3) | **0.0255 s** | 0.0281 s | 0.0285 s | **Qu 1.12x faster** |
| loop_performance (20e6, non-foldable) | 0.4465 s | 1.1276 s | **0.0610 s** | MATLAB 7.3x faster |
| **four bulk-array kernels** | 0.1029 s | 0.1951 s | **0.0762 s** | MATLAB 1.35x faster |
| total (incl. loop) | 0.5494 s | 1.3227 s | **0.1372 s** | MATLAB 4.0x faster |

Read the **bulk-array subtotal**, not the total, for anything about array
throughput: the loop kernel is now 20,000,000 iterations and dominates the
total by construction. On bulk array work Qu is **1.35x behind MATLAB and
1.90x ahead of numpy**, winning `matrix_multiply` and `array_creation`
outright. On scalar loops Qu is **7.3x behind MATLAB and 2.5x ahead of
CPython**.

**Quote the ratios, not the absolute seconds.** Re-running the Qu column a few
hours later on a loaded box gave element_wise 0.1292 s and loop 0.8601 s — about
1.9x worse than the table above, which looks like a regression and is not.
MATLAB, whose code did not change, degraded by 2.2-2.9x in the same window
(element_wise 0.0378 -> 0.1094 s, loop 0.0610 -> 0.1357 s), and the Qu:MATLAB
ratios held or improved. Absolute seconds on this machine are only meaningful
against a baseline measured in the same window; that is what the interleaving
is for. If you see numbers unlike these, re-measure MATLAB before concluding
anything about Qu.

### Why `loop_performance` changed shape

The old kernel — 100,000 iterations of `result = result + i` — could not
support a claim in either direction, for two separately measured reasons:

1. **No headroom.** At n=100k all three languages land in the 0.2-1.9 ms band,
   at or near timer resolution. A saturated kernel can only ever show a
   regression.
2. **MATLAB's JIT folds the closed-form sum.** At n=20e6, `result = result + i`
   runs in **0.0117 s** (1.7e9 iterations/s, ~2 cycles/iteration) while
   `result = result + mod(i,7)` takes **0.5462 s** — **47x apart at the same
   trip count**. The plain-sum baseline was measuring constant folding, not
   loop throughput.

So the kernel is now n=20e6 with the body `result + i - 7*floor(i/7)`,
identical in all three ports and verified by all three printing
`result = 60000003`. (`mod(i, 7)` is not available in Qu: `mod` is a KEYWORD,
so it is a parse error in an expression — `unexpected Keyword("mod") in
expression`.)

Together these two distortions had been pulling the published number in
opposite directions: the small-N row read as a 4x MATLAB lead, the folded
n=20e6 baseline as 17x. Neither is right; **7.3x** is.

## Reading it

- **`element_wise_ops` went from Qu's biggest gap to matched with MATLAB,
  same day.** Two real fixes, not one, and finding the second one only
  showed up by profiling *after* the first:
  1. **`qu-core`'s `Matrix::map`/`Matrix::broadcast` were plain serial
     loops** — unlike `matmul`, never parallelized. Gave them the same
     rayon `par_iter`/`par_chunks_mut` treatment `matmul` already had
     (threshold-gated, same as before). This alone barely moved the
     number (~8.9s → ~9.2s, noise) — profiling *why* revealed the real
     cost was elsewhere.
  2. **Every matrix-boundary binary op (`.* + - / mod`) was deep-cloning
     both operands before doing any math**, regardless of whether the
     value was a fresh temporary about to be discarded anyway
     (`Value::to_matrix(&self)` always clones). Added `into_matrix(self)`
     (consumes `self`; `Arc::try_unwrap`s a `Value::Mat` instead of
     cloning when nothing else still holds it) and used it in
     `matrix_binop` — a chained expression like `sin(A) .* cos(B)` now
     moves data through instead of copying it at every step.
  3. **`randn`/`rand` generation turned out to be the single largest cost
     in the whole kernel** — ~2.7s serial *per* `randn(10000,10000)` call,
     2 calls, ~5.4s of the pre-fix ~8.9s. Parallelized the same way
     `parallel for`/`spawn`/`pmap` already do: draw one seed per chunk
     sequentially off the parent stream first (so the draw sequence is
     independent of thread scheduling — `seed(n)` reproducibility still
     holds exactly, new test:
     `deterministic_randn_above_the_parallel_threshold`), then generate
     each chunk on its own thread with its own fresh RNG, writing directly
     into disjoint slices of the output (an earlier version collected a
     `Vec<Vec<f64>>` per chunk and flattened them single-threaded
     afterward — quietly ate back a big chunk of the win; fixed to write
     straight into pre-allocated output slices instead). `randn(10000,10000)`:
     ~2.7s → ~0.42s, **~6.3x**.
  4. Combined: **element_wise_ops ~8.9s → ~1.9-3.3s depending on machine
     load, matching MATLAB back-to-back.** Full writeup, all four numbered
     fixes: `BOARD.md`'s "ELEMENTWISE PARALLELIZATION" entry.
- **Bulk array ops are competitive with numpy and matty across the board
  now** — Qu beats or matches numpy and matty on every kernel in this
  suite, `fft` included as of the `rustfft` swap. matty's JAX-CPU backend
  still edges out matmul specifically (XLA's tighter BLAS path).
- **loop_performance's fix (same day, earlier pass)**: the original run
  found Qu's interpreted `for`-loop 12.2x slower than plain CPython's, from
  scalar per-iteration interpretation cost (a `String` allocation plus a
  `HashMap` probe on every read/write) — not the array-copy cost the
  `Arc`-COW pass had just fixed. Call-stack-frames + slot-resolved-fast-loop
  work (Ahmed: "go ahead", BOARD.md) took it from 0.060s to ~0.0008s — a real
  and large win, and ~800x faster than matty's own interpreter.
  **CORRECTED 2026-09-16: the follow-on claim that this "matches MATLAB's own
  JIT-compiled loop" does not survive measurement.** Both landing in the same
  0.0007-0.01s band at n=100k was an artifact of a kernel with no headroom
  plus a MATLAB baseline its JIT was constant-folding — see "Why
  `loop_performance` changed shape" above. Measured at n=20e6 with a
  non-foldable body, **MATLAB is 7.3x faster than Qu** on scalar loops. Qu
  does beat CPython there by 2.5x. Scalar loop throughput is an open gap, not
  a closed one.

## Re-running

```bash
# Qu (release build recommended -- debug is markedly slower)
#
# `tools/build_qu.sh` is the canonical entry point (Qu-Build, 2026-09-16): it
# works from any directory in the checkout, refuses to build from a parked
# tree, verifies the artifact by RUNNING it rather than trusting cargo's
# summary line, and prints the exe path and profile on success.
#
# It exists because the obvious spelling fails: the `qu` bin is NOT in the
# repo-root workspace. The root Cargo.toml is a legacy workspace
# (qu-core/qu-dsp/qu-data/qu-ml/qu-gpu/qu-wasm) and
# `cargo build --release --bin qu` there dies with
#   error: no bin target named `qu` in default-run packages
# The CLI lives in engine/crates/qu-cli, a separate workspace under engine/.
bash tools/build_qu.sh              # release (default); --debug for debug
#
# Use the exe path the script prints. The `run` subcommand is required -- a
# bare path argument is rejected with a did-you-mean rather than executed.
<printed-exe-path> run benchmarks/matty_suite/bench_fair.qu

# numpy
python benchmarks/matty_suite/bench_fair.py

# MATLAB -- use R2025b EXPLICITLY. `matlab` on PATH resolves to R2026a, whose
# trial license is expired (Licensing Error 10 / -10.2). That failure prints to
# stdout and STILL EXITS 0, so a wrapper gating on the exit code will record an
# expired license as a successful run and silently drop MATLAB from the table.
# Assert on the output, not on $?.
"/c/Program Files/MATLAB/R2025b/bin/matlab" -sd "$(cygpath -w benchmarks/matty_suite)" -batch "bench_fair"

# All three must print `result = 60000003` from loop_performance. If one does
# not, that language did not actually run -- check its exit code per language,
# not just the combined runner's.

# matty (JAX backend, CPU)
cd matty
./.venv/Scripts/python.exe ../benchmarks/matty_suite/run_via_matty_jax.py
```
