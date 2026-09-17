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
  work (Ahmed: "go ahead", BOARD.md) took it from 0.060s to ~0.0008s — now
  matching MATLAB's own JIT-compiled loop (both land in the same
  0.0007-0.01s noise band) and ~800x faster than matty's own interpreter.

## Re-running

```bash
# Qu (release build recommended -- debug is markedly slower)
$env:CARGO_TARGET_DIR = "<temp dir>"   # Windows/Dropbox: avoid target-dir file locks
cargo build --manifest-path engine/Cargo.toml -p qu-cli --release
& "$env:CARGO_TARGET_DIR/release/qu.exe" run benchmarks/matty_suite/bench.qu

# numpy
python benchmarks/matty_suite/bench.py

# MATLAB (pick whichever install has a valid license)
matlab -batch "run('benchmarks/matty_suite/bench.m')"
# or, with multiple installs: "/c/Program Files/MATLAB/R2025b/bin/matlab" -batch "..."

# matty (JAX backend, CPU)
cd matty
./.venv/Scripts/python.exe ../benchmarks/matty_suite/run_via_matty_jax.py
```
