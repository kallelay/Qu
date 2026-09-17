# Qu performance benchmarks — index

Qu's standing goal is to get faster than MATLAB/Python/Octave (`IMPL.md` §1),
tracked value-for-value on `BOARD.md`/`BACKLOG.md`, not just argued. Work on
this has been spread across several contributors and sessions (a "Codex"
agent, multiple Claude sessions, Ahmed himself) and several different
directories. This is the consolidated index — every benchmark scenario that
exists in this repo, where it lives, its latest cached numbers, and whether
it's runnable right now on this machine or needs extra setup (GPU hardware,
a MATLAB/Octave license, matty's own Python venv).

Numbers below are **cached from the session that produced them** unless
marked "fresh" (re-run while assembling this index, 2026-08-23). None of
this is statistically rigorous — single trials on a shared machine — good
enough to see the shape of where Qu stands, not a publishable claim.

## Scenario index

| # | Scenario | Measures | Runnable now? |
|---|---|---|---|
| 1 | [`matty_suite/`](matty_suite/) | 5 standard kernels vs numpy/MATLAB/matty | ✅ Qu, numpy · MATLAB needs a licensed install · matty needs its venv |
| 3 | [`qu-gpu` multisine + matmul](#3-gpu-multisine-synthesis--matmul) | GPU vs CPU vs scalar-oracle | ✅ this machine has an RTX 5080; `QU_REQUIRE_GPU=1` to force it |
| 4 | [`qu-core/tests/matmul_bench.rs`](../qu-core/tests/matmul_bench.rs) | rayon-parallel vs naive matmul | ✅ `--features parallel` |
| 5 | [`parallel_for_scaling.qu`](parallel_for_scaling.qu) | `parallel for` vs plain `for`, real multi-core | ✅ |
| 6 | matty's own Rust-vs-Python numbers | context/prior art only, not a Qu benchmark | n/a — read-only reference |
| 7 | [`matty_suite/`](matty_suite/) `element_wise_ops` | elementwise parallelization + redundant-clone fix | ✅ Qu, numpy · same as #1 |
| 8 | [`matty_suite/`](matty_suite/) `fft` | `rustfft`-backed FFT vs numpy/MATLAB/matty | ✅ Qu, numpy · same as #1 |
| 9 | [`csv_filters/`](csv_filters/) | CSV load + 5 filters (Butterworth low/high/band, filtfilt, FIR moving-average) vs pandas/scipy | ✅ Qu, Python |
| 10 | [`full_pipeline/`](full_pipeline/) | end-to-end 7-stage chained pipeline (load→clean→filter→spectral→time-freq→features→export) vs pandas/scipy | ✅ Qu, Python |
| 11 | [`data_wrangling/`](data_wrangling/) | Table/DataFrame ops (load, filter, groupby+agg, sort, describe, corr matrix) on a shared 1M-row CSV, vs pandas/MATLAB `table` | ✅ Qu, Python, MATLAB · Matty confirmed unable to run this scenario (no table/DataFrame type) |
| 12 | [`image_processing/`](image_processing/) | CV pipeline: grayscale→Otsu→morphology→blob labeling→affine warp→histeq vs OpenCV | ✅ Qu, Python (OpenCV) · MATLAB Image Processing Toolbox not installed on this machine · Matty has no image-processing builtins |
| 13 | [`linalg/`](linalg/) | `eig`/`svd`/`qr`/`lu`/`det`/`rank` at 100/500/1000, plus `polyfit` (large-N least squares) — vs numpy/scipy, MATLAB, and matty | ✅ Qu, numpy/scipy, MATLAB · matty needs its own venv (`matty/.venv`) |
| 14 | [`filter_design/`](filter_design/) | filter DESIGN (`butter`/`cheby1`/`cheby2`/`ellip`/`fir1`/`firls` coefficient computation) + filter APPLICATION at 1M/10M samples, vs scipy/MATLAB/Matty | ✅ Qu, Python, Matty · MATLAB design N/A (no Signal Processing Toolbox on this machine, application-only via a workaround) |
| 15 | [`shallow_ml/`](shallow_ml/) | k-means/PCA/SVM(RBF)/random forest/gradient boosting train+predict timing, vs scikit-learn and matty | ✅ Qu, scikit-learn · matty needs its own venv (`matty/.venv`), kmeans/PCA only · MATLAB licensed but Statistics and Machine Learning Toolbox not installed on this machine |
| 17 | [`file_io/`](file_io/) | binary packed-double, wide-CSV, and image save/load throughput vs numpy/pandas/Pillow/MATLAB/Matty | ✅ Qu, Python, MATLAB · Matty partial (no `table`/`readtable`, no image codec at all) |
| 18 | [`large_pipeline/`](large_pipeline/) | `full_pipeline/`'s 7-stage pipeline at 25x scale (5M samples) vs pandas/scipy/MATLAB (toolbox-free)/Matty | ✅ Qu, Python, MATLAB (toolbox-free, hand-rolled filter/STFT), Matty (real `butter`) |
| 19 | [`mlp/`](mlp/) | real MLP (20→64→32→4, 2 hidden layers) trained via manual `param`/`grad`/`stop_grad`, same init/data/loss, vs PyTorch autograd | ✅ Qu, PyTorch (CPU) |
| 20 | [`sequential_api/`](sequential_api/) | same MLP, high-level one-liner API (`mlp_classifier`+`.fit()` vs Keras `Sequential`+`.compile()`+`.fit()`) — ease-of-use + speed | ✅ Qu, TensorFlow/Keras (CPU) |
| 21 | [`multi_scenario/`](multi_scenario/) | 3 scenarios (tabular MLP, image CNN, sequence RNN) via the model-zoo `mlp_classifier`/`simple_cnn`/`simple_rnn_classifier`, each vs BOTH PyTorch (`nn.Sequential`/`nn.Conv2d`/`nn.GRU`) and Keras (`Sequential`/`Conv2D`/`GRU`) | ✅ Qu, PyTorch (CPU), TensorFlow/Keras (CPU) |
| 22 | [`lr_schedulers/`](lr_schedulers/) | `lr_adaptive`/`lr_plateau` learning-rate controllers vs PyTorch (`ReduceLROnPlateau` + a hand-rolled reject-and-retry equivalent), fixed-lr baseline + a deliberately-oversized-lr stress case | ✅ Qu, PyTorch (CPU) |
| 23 | [`advanced_validation/`](advanced_validation/) | Correctness + speed validation (not just speed) across 3 domains: `findpeaks` at multiple SNR levels, `knn_model`/Sequential-API-Adam ML on a real dataset, `curve_fit`/Kalman filtering — vs scipy/scikit-learn/PyTorch/independent numpy | ✅ Qu, scipy, scikit-learn, PyTorch |
| 24 | [`multisine_acceleration/`](multisine_acceleration/) | Same 51-tone/1.5M-sample multisine, 5 acceleration methods head to head: baseline loop, full broadcast/matmul vectorization, `parallel for`, pool/queue/workers, `gpu_matmul` | ✅ Qu only (all 5 methods, `--features gpu` needed for #5) |

---

## 1. Cross-language kernel suite (`matty_suite/`)

Ports matty's own 5-kernel `BenchmarkSuite` (matrix_multiply, element_wise_ops,
fft, array_creation, loop_performance — same sizes matty uses to compare its
own Julia/JAX/Octave backends) to Qu, numpy, MATLAB, and matty's own JAX
backend. Full methodology, re-run instructions, and the "how to read it"
analysis live in [`matty_suite/README.md`](matty_suite/README.md) — the
numbers move fast enough (60x on `loop_performance`, then `element_wise_ops`
going from ~9x behind MATLAB to matched, both in one day) that duplicating
the table here would just go stale.

> **Correction, 2026-09-10.** This section previously read: *"Qu beats
> numpy/matty on every kernel but `fft`, matches MATLAB's own JIT on
> `loop_performance`, and … is now matched with MATLAB on
> `element_wise_ops`."* **Those claims do not survive a fair measurement**,
> and the reason is the harness, not the engine — recorded here rather than
> deleted, because how a number came to be wrong is worth more than its
> absence.
>
> `bench.qu`/`.py`/`.m` time each kernel **once, cold, with `randn` inside
> the timed region**. Two consequences, both measured:
>
> - **MATLAB's first call is warm-up.** `fft` takes 0.0237 s cold and
>   0.0007 s on its third call — 34×. The old "Qu 0.0062 vs MATLAB 0.1045,
>   17× faster" was timing MATLAB's library load. Qu's own warm-up is mild
>   (1.8× on fft, flat on matmul), so the distortion is one-sided.
> - **`randn` dominates some kernels.** Python's `matrix_multiply` row is
>   **82%** random number generation; numpy's actual matmul is ~0.0098 s,
>   not the 0.0539 s the row shows.
>
> `bench_fair.qu`/`.py`/`.m` build data outside the clock, discard a warm-up
> run and report best-of-three. On the same machine, same session
> (element-wise at 3000×3000 rather than 10000×10000 so three repetitions
> finish; the others are the original sizes):
>
> | kernel | size | Qu | Python | MATLAB |
> |---|---|---|---|---|
> | matrix_multiply | 1000² *(same as cold)* | 0.0069 | **0.0047** | 0.0095 |
> | element_wise_ops | **3000² — NOT the cold suite's 10000²** | 0.0608 | 0.1422 | **0.0339** |
> | fft | 100k *(same as cold)* | 0.0018 | 0.0028 | **0.0006** |
> | array_creation | 5000² *(same as cold, minus `rand`)* | 0.0256 | 0.0245 | **0.0208** |
> | loop_performance | 100k *(same as cold)* | 0.0007 | 0.0016 | **0.0002** |
> | **total** | — | 0.0958 | 0.1759 | **0.0650** |
>
> **Do not compare the `element_wise_ops` row against the cold suite's
> figure**: this one runs 3000×3000 so three repetitions finish, the cold
> one runs 10000×10000. They are ~11× different in element count and are
> not the same measurement. Comparing them would be the exact mistake this
> whole correction is about — a number read against a label that does not
> describe it — repeated one layer up. Every other row is like-for-like on
> size; `array_creation` also drops `rand` so it measures allocation rather
> than RNG.
>
> Measured this way **Qu wins no kernel**: MATLAB takes four and numpy takes
> `matrix_multiply`. Qu is still comfortably ahead of numpy overall (0.0958
> vs 0.1759) and behind MATLAB by 47%. `loop_performance` is the sharpest
> reversal — Qu looked 2.4× faster than MATLAB cold and is 3.5× *slower*
> warm, because MATLAB's JIT needs a call to warm up and then wins.
>
> The old numbers are not fabricated and `bench.*` is not deleted: as an
> end-to-end *cold script* measurement it is a real thing to want, and Qu
> genuinely does start faster. What it cannot support is any claim of the
> form "Qu beats X at FFT", because the per-kernel labels do not describe
> what the per-kernel numbers contain. Use `bench_fair.*` for kernel claims
> and `bench.*` for start-up-inclusive script claims, and say which.
>
> **What the `loop_performance` row is actually measuring, and what would
> close it.** 0.0007 s against MATLAB's 0.0002 s is not a library gap —
> there is no BLAS in a scalar accumulation loop. It is Qu's tree-walking
> interpreter against MATLAB's JIT, which is why the cold number flattered
> Qu (MATLAB's JIT had not warmed up yet) and the warm number does not.
>
> That is precisely the gap **M4 (Typed IR → Cranelift)** exists to close,
> and M4 is already fully scoped in `IMPL.md` (§"M4 Typed IR — scoped
> design, 2026-08-23") and has never been started — verified: no
> `cranelift-*` dependency appears in any `Cargo.toml` in the repo, exactly
> as that section says. So this row is the empirical case for a milestone
> that was previously argued only from first principles. "Qu is 47% behind
> MATLAB overall" and "the deficit is concentrated in interpreted scalar
> loops, and the already-designed fix for that is M4" are the same fact and
> very different messages.

`fft` and general elementwise/matmul throughput trail MATLAB's tuned
BLAS/LAPACK by a real margin on quiet-machine numbers; nothing in this
index closes that further yet.


## 3. GPU multisine synthesis + matmul

`qu-gpu/tests/multisine_gpu.rs` and `qu-gpu/tests/matmul_gpu.rs` (real
`Instant::now()` timing + correctness-vs-CPU parity, `wgpu`/WebGPU), plus
`qu-core/tests/signal_advanced.rs`'s CPU-side accelerated-vs-scalar-oracle
comparison. This machine has a real GPU (RTX 5080), so this is runnable here
today — most dev machines won't have one, so `QU_REQUIRE_GPU=1` forces a hard
failure instead of a silent skip when you need the number, not just a
pass/fail. **Fresh numbers, 2026-08-23** (this session, same machine as the
cached BOARD.md figures from 2026-08-21/22):

| | cached (2026-08-22) | fresh (2026-08-23) |
|---|---|---|
| CPU accelerated/waveform | 0.107 ms (3.16x over oracle) | 0.107 ms (3.30x over oracle) |
| GPU population (80×15×2048), Vulkan/RTX 5080 | 0.159 ms (1.984 us/waveform) | 0.385 ms (4.813 us/waveform) |

CPU number reproduced almost exactly. The GPU number moved ~2.4x between
runs — expected for a single-trial GPU dispatch measurement (driver JIT
warm-up, thermal/clock state, no repeated-trial averaging in the test
itself); worth several repeated runs before treating either number as "the"
GPU figure. Both are still comfortably inside the test's own budget
assertions (`gpu_population_matches_cpu_and_meets_dispatch_budget`,
`accelerated_cpu_kernel_is_fast_and_faster_than_oracle`).

Run: `cargo test -p qu-gpu -p qu-core --release -- --nocapture` (add
`QU_REQUIRE_GPU=1` to hard-fail instead of skip without a GPU); or
`scripts/test-signal.ps1 -RequireGpu` for the full gate (adds wasm32 compile
check + engine workspace tests + browser/doc node tests — heavier, this is
the actual CI-shaped gate, not just the GPU numbers).

## 4. Parallel matmul (`qu-core/tests/matmul_bench.rs` — new this session)

The rayon-parallel-matmul speedup (BACKLOG.md, "Multi-core matmul via
rayon", 2026-08-25 dev session) was a one-off manual measurement — real, but
not captured in any runnable test, so it couldn't be reproduced without
redoing the work by hand. Added `qu-core/tests/matmul_bench.rs` (mirrors the
existing `signal_advanced.rs` printed-timing pattern: naive triple loop vs
`Matrix::matmul`'s rayon path, correctness-checked, timing printed via
`eprintln!`, release-only speedup assertion) so this is now a permanent,
re-runnable regression check instead of a session note.

| | cached (BACKLOG.md, 600×600) | fresh (this session, 600×600) |
|---|---|---|
| naive | 0.0352 s | 0.2003 s |
| parallel | 0.0084 s | 0.0097 s |
| speedup | 4.2x | 20.6x |

The **parallel** number reproduces closely (0.0084s → 0.0097s); the naive
baseline doesn't (0.0352s → 0.2003s) — most likely the original measurement
used a different naive loop/access pattern than this test's row-outer,
bounds-checked `Matrix::get` triple loop, not a real regression in
`matmul`'s parallel path itself. Treat the parallel-path number as the
trustworthy one; the naive baseline is this test's own reference, not a
reproduction of whatever the original session measured. Run: `cargo test -p
qu-core --release --features parallel --test matmul_bench -- --nocapture`.

## 5. `parallel for` vs plain `for` (`parallel_for_scaling.qu` — new this session)

Same situation as #4: the `parallel for` speedup (BACKLOG.md, "Real
multithreaded `parallel for`", 2026-08-25 dev session — 16 iterations of a
synthetic CPU-heavy function, 6.500s serial → 1.017s parallel, 6.4x) came
from two separate manual `qu run` invocations, not a script. Added
`parallel_for_scaling.qu`: a builtin-call-heavy (`sin`/`cos`, not the plain
arithmetic the `for`/`while` fast path from earlier this session covers) CPU
workload, run once as a plain `for` and once as `parallel for`, with
`tic()`/`toc()` timing and a correctness check (`max(abs(serial - parallel))`
should be ~0 — isolated per-iteration work, no shared state).

Fresh run, this session: **serial 1.827 s, parallel 0.293 s, speedup
6.23x**, max diff `0.00e0` (bit-identical). Close to the cached 6.4x
(different workload size — this one's tuned to finish in ~2s total instead
of ~7s, for a faster benchmark-suite run). Run: `qu run
benchmarks/parallel_for_scaling.qu` (release build recommended).

## 6. matty's own prior art (context only, not a Qu benchmark)

`matty/` (the vendored sibling MATLAB-interpreter project) has its own
Rust-native-vs-Python-backend numbers in `matty/NATIVE_STATUS_REPORT.md`
("~1000x faster than Python `eval()` for scalars", per-example timings
~1-2ms) and `matty/EVALUATION_REPORT.md` ("100% (20/20) pass, ~1.2ms avg
execution time"). These measure matty's *own* two backends against each
other, not Qu — relevant only as prior art on what a fast MATLAB-compatible
interpreter can look like, not part of Qu's own benchmark surface. Not
included in any table above.

## 7. Elementwise parallelization + redundant-clone fix (new this session)

`element_wise_ops` went from Qu's biggest cross-language gap (~9.3x behind
MATLAB) to matched with it (0.01% apart, same-moment run), via three
compounding fixes, found in sequence — the second and third only became
visible by profiling *after* the first didn't move the number much:

1. `qu-core::Matrix::map`/`::broadcast` were plain serial loops, unlike
   `matmul` (which already had a rayon path). Parallelized both the same
   way, threshold-gated like `matmul`. Barely moved the benchmark on its
   own (~8.9s → ~9.2s, noise) — the real cost was elsewhere.
2. Every matrix-boundary binary op (`.* + - / mod`) deep-cloned **both**
   operands via `Value::to_matrix(&self)`, even when an operand was a
   fresh temporary about to be discarded (e.g. the `sin(A)` in
   `sin(A) .* cos(B)`). Added a consuming `into_matrix(self)` that
   `Arc::try_unwrap`s instead of cloning when nothing else holds the
   `Arc`, used it in `matrix_binop`.
3. `randn`/`rand` generation was the single largest cost in the kernel —
   ~2.7s serial per `randn(10000,10000)` call, 2 calls, ~5.4s of the
   pre-fix ~8.9s total. Parallelized with the same "draw per-chunk seeds
   sequentially off the parent stream first, then generate each chunk on
   its own thread with a fresh `Rng`" pattern `parallel for`/`spawn`/`pmap`
   already use — `seed(n)` reproducibility holds exactly (new test:
   `deterministic_randn_above_the_parallel_threshold`). `randn(10000,10000)`:
   ~2.7s → ~0.42s.

Combined: `element_wise_ops` ~8.9s → ~1.9-3.3s depending on machine load,
matching MATLAB back-to-back (3.2554s vs 3.2520s). Full writeup:
`BOARD.md`'s "ELEMENTWISE PARALLELIZATION" entry and
`benchmarks/matty_suite/README.md`. New correctness tests: 2 in
`qu-core/src/matrix.rs` (`large_map_takes_the_parallel_path_and_still_
matches_a_serial_reference`, `large_broadcast_...`) proving the parallel
paths match a serial reference at scale, plus the RNG-reproducibility test
above. Full workspace suite (233+61+8+29 = 331 tests) verified green across
debug, release, `--features gpu`, `--no-default-features`, and the wasm32
compile check throughout.

## 8. `fft`: hand-written radix-2/Bluestein swapped for `rustfft` (new this session)

The one kernel item 7 flagged as untouched. Ahmed asked directly: "can't we
implement fftw?" — answer: no, but yes to the underlying idea. FFTW is
GPL-licensed (or a paid commercial license) against this workspace's
MIT/Apache-2.0, and it's a C library via FFI, which breaks the wasm32
target `qu-wasm` treats as first-class. `rustfft` (pure Rust, MIT/
Apache-2.0, SIMD-dispatched where the target supports runtime feature
detection) was already sitting in the workspace `Cargo.toml`, declared but
unused — wired it into `qu-core::fft_complex`/`::ifft_complex` behind the
exact same public API, replacing the hand-written radix-2 + Bluestein
implementation entirely (~140 lines deleted, not just supplemented). A
thread-local `FftPlanner` is cached per thread so repeated same-length
transforms (e.g. `stft`'s per-frame FFTs) only pay planning cost once.

**Same-moment result**: Qu **0.0155s**, numpy **0.0315s**, MATLAB
**0.0769s** — Qu is now **2.0x faster than numpy and 5.0x faster than
MATLAB** on this kernel, up from being the one gap left. Own-number
speedup: **~5x** (0.080s → 0.0155s). Full workspace suite (349 tests)
verified green across debug, release, `--features gpu`,
`--no-default-features`, and the wasm32 compile check. Full writeup:
`BOARD.md`'s "RUSTFFT" entry, `benchmarks/matty_suite/README.md`.

## 9. CSV load + multi-filter benchmark (new this session)

Ahmed asked for a "file loading as csv and applying different filters"
benchmark — a real analysis-workflow scenario (load, then filter) rather
than another isolated kernel. `csv_filters/bench.qu`/`bench.py` build the
same 500k-sample, 3-tone-plus-noise dataset, write it to CSV, then time
loading it back and running it through five filters: Butterworth low/high/
bandpass (causal, `sosfilt`), Butterworth lowpass zero-phase (`filtfilt`),
and a 51-tap FIR moving average (`conv`). Full writeup, exact commands, and
the honest per-step breakdown: [`csv_filters/README.md`](csv_filters/).

**Result**: the four IIR/FIR filter steps are essentially tied with scipy
(both bottom out in a tight compiled loop) — Qu **~0.02–0.03s** filter
total vs Python **~0.02s**. Correctness cross-checked, not just timed: the
lowpass output's RMS lands at 0.7077 (Qu) / 0.7072 (Python), both matching
the analytically-expected `1/sqrt(2)`.

`read_csv` itself originally trailed pandas by ~2–3x (0.13–0.16s vs
0.05–0.08s for 500k rows) — Ahmed noticed and asked to close it (also
flagging polars, itself Rust, as a sharper long-term target: 0.004s warm,
architecturally a different league — parallel memory-mapped chunked
parsing, not attempted here). `Table::from_csv` was buffering every cell
as an owned `String` before a second pass parsed it to `f64` — a million
throwaway allocations for this file. Rewritten to parse numeric fields
directly from borrowed `&str` into a pre-sized `Vec<f64>` in one pass.
**0.13–0.16s → 0.038–0.044s, ~3.6x — now edges out pandas** on this file.
Full writeup: [`csv_filters/README.md`](csv_filters/).

## 10. Interpreter allocation-hotspot pass (Ahmed: "continue accelerating the language")

Same "stop allocating what you're about to discard" pattern as the CSV
fix, applied to two more genuinely hot paths, found via a targeted sweep
(env/function lookup, string-building, redundant clones, unsized
`Vec::push` loops) rather than a guess — the sweep also confirmed
`self.env`/`self.funcs`/`self.frames` are already plain `HashMap`s (O(1),
no fix needed) and `matrix_binop`/`map1`/`map2` already avoid loop-internal
clones.

- **SVG/TikZ polyline & polygon rendering**
  (`engine/crates/qu-interp/src/plotting.rs`, `write_svg_op`/
  `write_tikz_op`): every `Polyline`/`Polygon` draw op — which is the
  *entire point series* for a line/area plot, so one call pays a per-point
  cost — was building `points.iter().map(|(x,y)| format!(...)).collect::<Vec<String>>().join(...)`:
  one `String` allocation per point plus a throwaway `Vec`, discarded
  immediately after the join. Rewritten to write each point straight into
  the shared output buffer. All 69 plotting tests pass byte-for-byte
  (output format unchanged, just how it's built). Measured after the fix,
  a 200,000-point signal plot: `savefig(.svg)` **0.187s**, `savefig(.tikz)`
  **0.037s** (no comparable "before" number kept — the fix was applied
  in-place on the working tree rather than benchmarked via a stash, to
  avoid touching git state on a repo with concurrent live edits from
  another session; the removed-allocation-count reasoning above is the
  honest basis for expecting this was a real win, not an unverified
  speedup multiplier).
- **`Table::group_by_agg`** (`engine/crates/qu-interp/src/table.rs`): was
  `buckets.contains_key(k)` immediately followed by a separate
  `buckets.entry(k.clone())` — two `HashMap` lookups per row where one
  suffices. Rewritten to a single `Entry::Vacant`/`Entry::Occupied` match.
  Deliberately did *not* also switch numeric group columns to bucket on
  the raw `f64` bit pattern instead of the formatted display string (a
  larger, real speedup for numeric `group_by` — skips `fmt_num`-ing every
  row just to hash it) — that changes grouping *semantics* at the margins
  (two floats that display identically today merge into one group; a
  bit-exact bucket would split them), which isn't a decision to make
  silently inside a "go faster" pass. Flagged here, not implemented.

## 11. Full signal-processing pipeline (new this session)

Ahmed's follow-up to §9/§10 ("full signal processing pipeline"): a
chained, end-to-end workflow rather than isolated kernels or a single
load+filter step — `full_pipeline/bench.qu`/`bench.py` run the same
200k-sample signal through 7 stages (load → clean → bandpass filter →
spectral analysis → time-frequency/spectral-entropy → feature extraction
→ export), each feeding the next. Full writeup, per-stage table, and the
correctness cross-check: [`full_pipeline/README.md`](full_pipeline/).

**Result: Qu ~1.8–2x faster end to end** (0.032–0.040s vs 0.063–0.074s).
No single stage dominates the way `read_csv` did before its fix — `load`
is still the largest contributor on both sides, but every stage holds its
own. Correctness cross-checked: `mean_entropy` (Shannon entropy of each
STFT frame's normalized power spectrum — Python's version hand-implemented
to match Qu's own definition, since `scipy` has no direct equivalent)
matches to 4 decimal places between the two independently-generated
datasets.

Also includes a fresh re-run of the `matty_suite/` 5-kernel comparison
against numpy (a lot changed in the interpreter this session): **Qu ahead
of numpy on every kernel, 2.3x–3.3x, 3.2x on the whole suite total** — a
wider margin than that benchmark's own cached snapshot, consistent with
the accumulated parallelization/rustfft/soft-compile work landing since
those numbers were taken. Not re-checked against MATLAB this pass.

## 12. Table/DataFrame operations + descriptive statistics (new this session)

Ahmed asked for a Table/DataFrame benchmark: load, filter, groupby+aggregate,
sort, and descriptive stats/correlation matrix on a realistically large
(1,000,000-row, mixed-numeric-plus-categorical) dataset, Qu vs pandas vs
MATLAB `table` (Matty as a requested 4th language too). Unlike `csv_filters`/
`full_pipeline` above, this scenario is specifically about comparing
Table *engines*, so all three scripts read one shared CSV built once by
`data_wrangling/generate_data.py`, rather than each regenerating their own
copy. Full methodology, exact commands, and the full correctness writeup:
[`data_wrangling/README.md`](data_wrangling/).

**Result**: pandas wins every step, often by 10-40x (its home turf — compiled
vectorized group/sort/corr kernels). Qu beats MATLAB's `readtable` clearly
(3.7-6.5s vs Qu's 0.58-0.64s) and lands in the same total-time ballpark
(4.18-6.39s vs MATLAB's 4.82-8.29s), but loses to MATLAB on every step after
load — `group_by_agg` and `corr` both only take one `(value_col, agg)`/
`(x,y)` pair per call (no multi-aggregate or matrix-correlation builtin yet),
so getting mean+sum+count means 3 full grouping passes and a 4-column
correlation matrix means 10 pairwise calls (each re-cloning its column),
where pandas/MATLAB do the equivalent in one vectorized call. A real,
structural interpreter-surface gap — flagged, not fixed this session
(`engine/crates/qu-interp/src/lib.rs` was off-limits, being actively edited
by a concurrent autodiff/training session).

**Correctness cross-checked, not just timed**: row counts, group aggregates,
sort order, `describe()` stats, and the correlation matrix all match across
Qu/pandas/MATLAB to at least 6 decimal places (small, expected percentile-
interpolation-convention differences from MATLAB's `prctile` aside). The
cross-check caught a real bug along the way: an early `bench.m` draft used
`groupsummary`'s `'nnz'` (nonzero-count) method as a stand-in for row count,
which silently undercounted one group by 1 row (a genuine `x2` value in the
dataset rounds to exactly `0.000000` at the CSV's precision) — caught by
diffing against pandas' `.agg(["count"])`, not by inspection. Fixed by using
`groupsummary`'s always-present `GroupCount` column instead.

**Matty**: confirmed, not assumed, unable to run this scenario — no `table`/
`readtable`/`groupsummary`/DataFrame type anywhere in its builtins, and its
only CSV reader (`csvread`, a thin numeric-only `np.loadtxt` wrapper) fails
immediately on the shared dataset's header row and text `category` column
(`could not convert string 'category' to float64...`). No workaround forced
(e.g. stripping the categorical column) since that would silently benchmark
a different, easier scenario.

## 13. CV pipeline: grayscale → Otsu → morphology → blobs → affine → histeq (new this session)

Ahmed's ask: benchmark a representative CV pipeline (not an isolated
kernel) on today's earlier-landed image/CV builtins
(`grayscale`/`otsu_threshold`/`threshold`/`imopen`/`label_blobs`/
`blob_stats`/`imrotate`/`imscale`/`imequalize`), Qu vs Python vs MATLAB vs
Matty, on the SAME 2000x2000 synthetic image across every language. Full
methodology, pipeline mapping table, and per-stage numbers:
[`image_processing/README.md`](image_processing/).

**Result: OpenCV ~13.5x faster end to end** (Qu 0.90–1.39s vs Python
0.039–0.143s, 3 trials each). Stage-by-stage the gap ranges from tied
(`grayscale`, ~1.0x — a single linear pixel pass leaves little room to
differ) to ~59x (`otsu_threshold`) and ~38-39x (`imopen`, the affine
warp) — expected, OpenCV's kernels are decades-optimized hand-vectorized
C++ against Qu's correctness-first, not-yet-SIMD image builtins landed
earlier the same day. **Correctness cross-checked, not just timed**: blob
count (9 — the seeded 6 circles + 3 rectangles, with all 20 bright
speckle-noise pixels correctly erased by the opening step) and total
blob area (240609 px) match **exactly** between Qu and OpenCV, every
trial — the strongest possible signal neither side is silently doing less
work. **MATLAB**: no numbers — this machine's MATLAB R2025b has no Image
Processing Toolbox installed (`imbinarize`/`bwlabel`/`regionprops`/
`imopen`/`imwarp`/`histeq` all `exist(...) == 0`; only base-MATLAB
`rgb2gray`/`imresize` are present); a believed-correct `bench.m` is
included but explicitly marked not-run. **Matty**: no column at all —
grepped Matty's own `src/`, `README.md`, `TODO.md` for every relevant
function name and found zero image-processing builtins; it's a numeric
MATLAB-language interpreter, not an Image Processing Toolbox
reimplementation. Also flagged (not fixed — touches the shared, actively-
mutating `qu-interp/src/lib.rs`): `otsu_threshold` redundantly re-runs a
full BT.601 grayscale conversion over all 4M pixels even when its input
is already grayscale, on exactly this pipeline's own call pattern
(`otsu_threshold(gray)` right after `gray = grayscale(img)`).

## 14. Linear-algebra decompositions at scale + a second optimization workload (new this session)

Two additions `benchmarks/README.md` didn't have: dense decompositions
(`eig`/`svd`/`qr`/`lu`/`det`/`rank`) at 100/500/1000, and a second, simpler
optimization workload (`polyfit`, least squares) as a second real data
point in that category — deliberately a *direct, non-iterative* solve this
time, so
there's no "same iteration count/starting point" ambiguity to get wrong
across languages, unlike an iterative method. Ahmed's follow-up ask added a
fourth language, matty, to this one specifically (Qu/numpy-scipy/MATLAB/
matty). Full methodology, results, and honest noise disclosure:
[`linalg/README.md`](linalg/).

**Part 1 (decompositions)**: this session's machine ran under sustained,
uncontrolled 100% CPU load (confirmed via `Get-CimInstance
Win32_Processor`/`Get-Process` — OneDrive/Dropbox sync, concurrent `claude`
sessions, and the standing fact that another live session is doing engine
work in this same repo, all at once) — real MATLAB's own `eig` at n=1000
read **21.3s, 10.6s, then 0.96s** across three back-to-back trials, a 20x
spread on the exact same operation/size/engine. Every table in
`linalg/README.md` is reported as a 3-trial median with this noise
disclosed explicitly, not smoothed over — treat those numbers as order-of-
magnitude/correctness, not a precision speed claim. What *did* survive the
noise: every engine's decomposition residual (`norm(A - Q*R)`, etc.) lands
at 1e-9 to 1e-14 — all four are numerically correct — `det` overflows to
`+/-Inf` at n>=500 in **all four** engines identically (an expected property
of a large random matrix's determinant, not a bug), and `rank`/`svd` at
n=1000 are Qu's clearest soft spots (nalgebra's dense SVD has no BLAS/LAPACK
acceleration underneath — same root cause as every other "Qu trails a
vendor-tuned library" gap this index has found before).

**A genuine matty correctness bug found and confirmed by reading source,
not just observed behavior**: `[L, U, P] = lu(A)` returns completely
mislabeled outputs — `src/builtins.py::f_lu` forwards `scipy.linalg.lu(A)`'s
raw `(p, l, u)` tuple (satisfying `A = p@l@u`), and matty's interpreter
binds it to the script's `(L, U, P)` names *by position*, not by what the
factorization actually is. Checking the residual against real MATLAB's
documented `P*A = L*U` convention gives garbage (thousands); checking `A =
L*U*P` — exactly what the positional-swap theory predicts — lands at 1e-12
at every size, confirming the factorization itself is correct and it's
purely an output-labeling bug. Two more, smaller matty gaps found and
worked around (documented inline in `linalg/bench_matty.m`): no
`rng`/seeded-RNG support anywhere, and no local/sub-function support in a
script file (every one of matty's own `examples/*.m` is a flat script,
consistent with this being a real, confirmed gap, not a one-off).

**Part 2 (`polyfit`, N=3,000,000, degree 5)**: a bit-identical
(deterministic-formula, no RNG) dataset let every language's fitted
coefficients be checked against each other directly, not just timed —
**identical to 6 decimal places across Qu/numpy/MATLAB**, and the residual
(`||y-yhat|| = 12.244523`) matches **exactly across all four engines**
including matty, verified *before* trusting any timing comparison, per the
standing brief. This workload is small/fast enough that the machine-load
noise above barely showed up (under 20% spread, not 5-20x), making it the
one clean speed number from this session: **Qu ~1.586s vs numpy ~0.354s vs
MATLAB ~0.394s vs matty ~0.422s — Qu a real, consistently-reproduced ~4x
slower**, isolating the same nalgebra-has-no-LAPACK gap Part 1's `rank`/
`svd` numbers hinted at, without the confounding noise.

## 16. Shallow ML: k-means/PCA/SVM/random forest/gradient boosting train+predict timing (new this session)

Ahmed's ask: benchmark shallow (classical) ML train **and** predict timing
separately, k-means/SVM/random-forest-or-gradient-boosting/PCA at minimum,
Qu vs scikit-learn vs MATLAB — plus matty, if it actually has real ML
support (checked, not assumed: `kmeans`/`pca` only, confirmed by grepping
`matty/TODO.md` and `matty/src/builtins.py`, no SVM/tree/ensemble
equivalent). Same `make_classification`/`make_blobs` datasets (4000×20
binary classification, 6000×8 six-blob clustering, both stratified 80/20
split, seed 42) reused byte-identically across every language. Full
methodology, hyperparameter mapping, and every result:
[`shallow_ml/README.md`](shallow_ml/).

**MATLAB got a real surprise**: `license('test','statistics_toolbox')`
*and* `license('checkout',...)` both report `1` (licensed), but `ver` lists
only base MATLAB + Parallel Computing Toolbox, and `kmeans`/`pca`/
`fitcsvm`/`fitctree`/`fitcensemble` are all `exist(...) == 0` — the
Statistics and Machine Learning Toolbox is **licensed but not installed**
on this machine, a different failure mode than "no license" a naive check
would have missed. No MATLAB column at all this time, honestly reported
rather than guessed.

**Result — timing**: Qu trails scikit-learn on every fit (PCA/k-means are
sub-millisecond-scale on both and noise-dominated; SVM/random
forest/gradient boosting show a real, consistent ~2-6x gap, e.g. SVM fit
1.01–1.40s Qu vs 0.17–0.43s sklearn on 3200 rows), tied or ahead on
predict for most models. One timing number was actively misleading and
excluded from the headline comparison rather than reported at face value:
sklearn's first `KMeans.fit` call in a fresh process pays a one-time BLAS/
OpenMP thread-pool spin-up ranging from 0.004s to **2.79s** for the
identical call — a >600x spread that's pure warm-up noise, not algorithm
speed (Qu has no such effect, staying in a tight 0.0018–0.0040s band).

**Result — correctness/quality, the more interesting half**: SVM matches
essentially exactly (98.00% test accuracy, both engines, 1989 vs 1988
support vectors). PCA's explained-variance ratio matches **to 5 decimal
places across Qu, scikit-learn, AND matty's independent SVD** — the
strongest possible signal. Random forest is close (95.75% vs 96.00%).
**Gradient boosting shows a real, unresolved 6-point test-accuracy gap**
(88.88% Qu vs 95.00% sklearn, same hyperparameters, same algorithm shape,
visible in *training* accuracy too so it isn't overfitting) — flagged
honestly rather than explained away, worth a dedicated follow-up into
`TreeBuilder`'s split search on residual targets. **k-means produced this
session's best finding**: Qu's single-random-init Lloyd's algorithm (no
k-means++, no restarts) landed on the *exact* global-optimum inertia
(98502.1528, matching the true-blob-center inertia to 4 decimal places,
ARI=1.0 against the known blob labels) — matching matty's `kmeans2` and
sklearn's own *traditional* `n_init=10` default exactly, while sklearn's
*current* `n_init="auto"` single-restart default landed in a visibly worse
local optimum (inertia 152028.83, ARI 0.77 — it confuses two blobs).
Read honestly: not proof Qu's simpler algorithm is better, just that it
got lucky on this particular well-separated dataset — a harder one would
likely expose the missing restarts.

## 15. Filter design + large-scale filter application benchmark

Two gaps `csv_filters/` (#9) didn't cover: the filter-design (coefficient
computation) step timed on its own rather than mixed into application
timing, and filter *application* (`sosfilt`/`filtfilt`) at 1M/10M samples
rather than `csv_filters/`'s 500k. Four engines this time, not three —
Ahmed's ask specifically added Matty (the sibling MATLAB/Octave-compatible
interpreter at `../matty` (a sibling checkout), its own NumPy/
SciPy-backed engine, not `matty_suite/`'s retired JAX script) alongside
Qu/Python/MATLAB. Full methodology, exact commands, and the complete
results tables: [`filter_design/README.md`](filter_design/).

**A real, machine-specific blocker found immediately, not worked around
silently**: this machine's licensed MATLAB R2025b has no Signal Processing
Toolbox installed — confirmed directly (`ver` lists only base MATLAB +
Parallel Computing Toolbox; `license('test','signal_toolbox')` reports the
entitlement as available, but the toolbox itself isn't there), so
`butter`/`cheby1`/`cheby2`/`ellip`/`fir1`/`firls`/`filtfilt`/`sosfilt` all
throw "Undefined function" in real MATLAB here. The filter-*design*
benchmark has **no real-MATLAB column at all** as a result (marked N/A, not
fabricated) — Matty fills that comparison point instead (it reimplements
the toolbox's design family over scipy). For the large-scale
*application* benchmark, base MATLAB's `filter` (a core-language function,
not toolbox) still works, so that one script hardcodes a literal
scipy-computed `[b,a]` pair and substitutes a hand-rolled forward+backward
double `filter()` call (labeled `manual_filtfilt`, not `filtfilt` — a
similar but not identical zero-phase approximation) to get a real
base-MATLAB application number despite the gap.

**Design timing result**: Qu's IIR design (`butter`/`cheby1`/`cheby2`/
`ellip`) is dramatically faster than scipy's at every order tested (4/8/16)
— low single-digit-to-teens of microseconds in Qu vs several hundred to a
few thousand microseconds in scipy (confirmed independently via Python's
own `timeit`, not just the benchmark script, since the gap looked large
enough to double-check) — scipy's design functions carry real per-call
Python-level overhead that dominates over the actual numerics at these
sizes. `fir1` design is a smaller Qu win (~5-10x). **`firls` inverts this
completely**: Qu is 50-500x *slower* than scipy, reaching ~26-40 **seconds**
per call at order 1024 (taps=1025) vs well under half a second in scipy —
a separate probe run showed Qu's `firls` cost scaling almost exactly
`O(n^3)` with order, consistent with a dense linear solve that doesn't
exploit the Toeplitz/symmetric structure a production implementation
would. Matty has no `firls` builtin at all (confirmed by reading
`src/builtins.py`, not just a failed call), so that scenario is reported as
unavailable there rather than skipped silently.

**Application-at-scale result**: at both 1M and 10M samples, Qu/scipy/Matty
stay within roughly 1.3x of each other for both causal (`sosfilt`/`filter`)
and zero-phase (`filtfilt`) application — the same "essentially tied,
dominated by a tight compiled loop" picture `csv_filters/` found at 500k,
still holding 20x larger. **Correctness cross-checked across all four
engines**, not just two: `rms(y)` on the causal output lands at
0.7506-0.7507 in Qu, Python, MATLAB, and Matty alike, matching the
analytically-expected `1/sqrt(2)`.

## 17. File I/O throughput: binary packed doubles, wide CSV, image round trip (new this session)

Ahmed's ask, split from `csv_filters/`/`full_pipeline`'s load-then-process
shape into pure I/O: (1) a packed float64 binary round trip, (2) a large
multi-column CSV load, (3) an image save/load round trip — Qu vs Python vs
MATLAB vs Matty. Full methodology and every number:
[`file_io/README.md`](file_io/).

**The binary round trip is this section's real finding**: Qu's file I/O
library (`fopen`/`read_double`/`write_double`, IMPL.md 2026-08-24) is
scalar-call-only, no bulk `fread`/`fwrite` path, so N values always means
N syscalls/interpreter round-trips — capped at N=1,000,000 by the same
`for`-range eager-materialization limit (`qu-core::DEFAULT_ELEMENT_LIMIT`)
that would otherwise need a separate justification. Worse, and genuinely
new: storing each read value into a vector via `y[k] = read_double(f)`
hits an **O(N²)** cost, confirmed by reading `exec_index_assign`'s source,
not just observed — `Arc::make_mut` always sees refcount 2 there (the
value is still live in `self.env` when the local clone bumps it), so it
deep-clones the whole vector on *every* indexed assignment. Isolated by
comparison: reading 200,000 doubles into a vector took 126.06s; the exact
same 200,000 `read_double` calls accumulated into a scalar sum instead
took 0.1585s — ~800x faster for identical I/O, proving the cost is the
assignment, not the read. Not fixed (`lib.rs` off-limits this session),
but pinned down precisely enough that whoever picks it up next won't need
to rediscover it.

**CSV**: Qu **fastest of all four** languages on a 3M-row x 6-col load
(0.93-1.14s vs pandas 1.88-2.92s vs MATLAB `readtable` 6.24-7.16s vs Matty
`csvread` 1.88-2.32s) — consistent with `csv_filters/`'s own `read_csv`
fix "now edges out pandas," holding at 6x the row count and 3x the column
count.

**Image**: Qu fastest on BMP save/load and PNG save too (though Qu's PNG
encoder is uncompressed "stored block" DEFLATE, doing measurably less work
than Pillow's/MATLAB's real compression — flagged, not hidden). Matty has
no image codec of any kind (confirmed via source grep) — this scenario
cannot run there at all, a hard capability gap rather than a missing
seed/API-shape mismatch.

**Matty, consolidated**: every one of this section's scripts fails
immediately on `rng()` (still missing, same as `linalg/`/`filter_design/`
above); CSV additionally has no `table`/`readtable`/`writetable` (only
`csvread`/`csvwrite`); image has no codec at all. Binary and CSV both got
a real from-scratch Matty variant once the seed call was dropped and got
real, comparable numbers; image did not, because there's nothing to call.

## 18. Large-scale (25x) full pipeline: does the Qu-vs-Python gap hold at scale? (new this session)

`full_pipeline/`'s own 7-stage chained benchmark ran at 200,000 samples.
Ahmed's follow-up: build a genuinely larger version (10-100x) and see if
Qu's standing holds, narrows, or widens. `large_pipeline/bench.qu` runs
the identical 7 stages at **5,000,000 samples (25x)**. Full methodology,
per-stage table, and the MATLAB-toolbox/Matty story below:
[`large_pipeline/README.md`](large_pipeline/).

**Answer: holds, and narrows slightly** — Qu **~1.4-1.6x** faster than
Python end to end at 25x (1.01-1.22s vs 1.60-1.64s), versus
`full_pipeline/`'s own cached ~1.8-2x at 200k. Nothing shifted
qualitatively — Qu is still ahead on every stage it was ahead on at 200k,
`load` is still the largest single contributor on both sides — but the
margin compressed rather than widened, the opposite of what a naive
"parallelization helps more at scale" guess would predict. Correctness
cross-checked at the new scale too: `rms`/`n_peaks`/`mean_entropy` match
Qu-vs-Python to the same precision `full_pipeline/README.md` established
at 200k.

**A real MATLAB constraint, worked around transparently, not hidden**:
this machine's MATLAB R2025b license has no Signal Processing Toolbox
(confirmed via `ver`, same finding as `filter_design/README.md` and
`shallow_ml/README.md` before it) — `butter`/`findpeaks`/`spectrogram`/
`hann` all hard-error here. `large_pipeline/bench.m` hand-implements each
from base `fft`/`filter`: a 2-pole RBJ biquad bandpass standing in for
Qu/Python's order-4 (8-pole) Butterworth, a vectorized local-maxima peak
scan, and a manual per-frame FFT loop with a hand-coded Hann window for
the spectrogram/entropy stage. Isolates the same 200-1000 Hz band, but
with a visibly different downstream `rms`/`crest`/`n_peaks` (a gentler
filter passes more), flagged explicitly rather than presented as a
matching cross-check. MATLAB's total (4.48-4.95s) is markedly slower than
Qu/Python here, but that number reflects "an interpreted MATLAB script
hand-rolling toolbox functionality," not "MATLAB's own toolbox is slow" —
not a fair toolbox-vs-toolbox reading.

**Matty ran the REAL order-4 Butterworth** (its `butter`/`filter` are
real, scipy-backed, unlike this machine's MATLAB) via `bench_matty.m`,
after three incompatibilities found and worked around in sequence: no
`rng()` (dropped, values not seed-reproducible), no `table`/`readtable`/
`writetable` (swapped for `csvwrite`/`csvread` on a plain matrix), and a
row-vs-column orientation bug in Matty's own array machinery that broke a
vectorized boolean assignment (worked around with an explicit `(:)`
reshape and a fully-vectorized rewrite of the peak-detection line — a
genuine Matty-side quirk, documented inline in the script, not silently
patched over). Total: 3.46-3.95s, with the real-filter correctness numbers
(`rms=0.4243`, `n_peaks=1`) landing much closer to Qu/Python's own
(`0.4287-0.4288`, `n_peaks=1`) than MATLAB's toolbox-free substitute did —
worth stating plainly: **on this specific machine, Matty's own
signal-processing coverage is broader than this particular MATLAB
license's.**

## 19. MLP training: manual autodiff (`param`/`grad`/`stop_grad`) vs PyTorch

Every prior autodiff correctness test in `engine/crates/qu-interp/tests/
acceptance.rs` (linear fits, tiny CNN, GRU, transformer block) checks that
gradients are *right*, on deliberately small toy problems. This scenario
asks a different question: on a **real** multi-layer perceptron — 4-class,
20-feature, 2400-sample classification, `20→64→32→4` with two ReLU hidden
layers, 300 full-batch epochs — trained with the exact same manual
`param()`/`grad()`/`stop_grad()` loop pattern, how does Qu's autodiff
compare to PyTorch's, both in wall-clock and in whether it actually
converges to a real model? Full methodology (same data, same He-init
weights pasted byte-identical into both scripts since Qu's RNG and
numpy's don't share values across a shared seed, same loss formula) in
[`mlp/README.md`](mlp/).

**Converges, and matches**: loss drops ~35x (6.27 → 0.177) on both sides,
landing on 95.96%/96.17% (Qu) vs 95.96%/96.00% (PyTorch) train/test
accuracy — cross-checked against a third, independent from-scratch numpy
backprop implementation that lands on the same numbers to 3+ significant
figures, so this is a verified-converged comparison, not "both ran."

**Timing: PyTorch ~7x faster** — Qu 2.96-3.10s total / ~10ms per epoch vs
PyTorch 0.39-0.46s total / ~1.4ms per epoch (two single-trial runs each,
same caveat as the rest of this index). Unlike `shallow_ml/` (where Qu is
competitive because the classical-ML fit/predict math itself dominates)
or `linalg/` (where Qu's raw matmul is competitive), this scenario's gap
is autodiff-loop overhead specifically: `call_builtin` dispatch plus
`tape_reset()`-and-rebuild six tracked ops (two `dense`, two `relu`,
`softmax_rows`, `row_sum`) every epoch, on a workload too small for the
underlying matmuls themselves to dominate wall-clock the way they do at
`linalg/`'s 500-1000 scale. A real, measured gap for `IMPL.md`/
`BACKLOG.md` to track — not a red flag on the gradients (those are
correct), a red flag on the interpreter overhead around them.

## 20. Sequential API: high-level one-liner (`mlp_classifier`+`.fit()` vs Keras) (new this session)

Item 19's own comparison used a **manual** `param()`/`grad()`/`stop_grad()`
loop, written by hand in both languages, and found PyTorch ~7x faster —
real per-`call_builtin` interpreter-dispatch overhead re-running Qu's
autodiff tape from scratch every epoch. Ahmed's follow-up asked a different
question: how do the two languages' **high-level** training APIs compare —
Qu's new Sequential API (`sequential`/`mlp_classifier` + `net.fit(...)`,
landed 2026-08-25/26) vs Keras' `Sequential([...])` + `.compile()` +
`.fit()` — on both ease-of-use and speed, given that `net.fit(...)`'s
training loop now runs as ONE native-Rust builtin call
(`sequential_fit`) rather than an interpreted Qu `for`-loop. Same
architecture and data as `mlp/` (reused directly, not regenerated). Full
methodology, side-by-side code, and honest caveats:
[`sequential_api/README.md`](sequential_api/).

**Ease of use**: Qu's `net = mlp_classifier(20, [64, 32], 4, seed=42);
trained = net.fit(Xtr, Ytr, epochs=300, lr=0.05, loss="cross_entropy")` is
2 statements; Keras needs 3 (`Sequential([...])`, then a separate mandatory
`.compile()` for optimizer/loss, then `.fit()`) — `net.fit`'s `loss=`
keyword folds what Keras splits into two calls into one.

**Timing — Qu ~3.7x faster than Keras here** (2.135 s vs 7.831 s total,
300 epochs, 3 runs each averaged) — the **opposite direction** from item
19's PyTorch comparison. Not a matmul-throughput story: at this small
scale (2400×20 full-batch), TensorFlow's own per-step Python/graph-dispatch
overhead dominates wall-clock more than Qu's does. This is **not** a direct
"closed the 7x gap" result — item 19 compared manual loops against
PyTorch specifically, this compares Sequential APIs against TensorFlow
(what's actually installed on this machine), a different library. The
honest same-question proxy that IS comparable: moving the epoch loop out
of interpreted Qu script into native Rust (`net.fit`) dropped Qu's own
per-epoch cost from `mlp/`'s ~9.9-10.3 ms (manual interpreted loop) to
~7.0-7.3 ms (native `net.fit` loop) on the same-shaped workload — a real
but partial ~30% reduction, not a full close of the autodiff-overhead gap
(still nowhere near PyTorch's ~1.3-1.5 ms/epoch from item 19).

**Accuracy — genuinely comparable**: both converge to mid-90s% train/test
accuracy (Qu 94.63%/95.33%; Keras 95.0-96.1% test across 3 runs) on the
same real 4-class/20-feature/2400-sample problem, same architecture, same
plain-SGD optimizer (no Adam on either side, to keep the algorithm
identical), same epoch count. One asymmetry flagged honestly: Qu's runs
are bit-for-bit reproducible (`seed=42` deterministically reseeds Qu's
RNG); Keras' initial loss and final accuracy visibly vary run to run
despite `tf.random.set_seed(42)` — TensorFlow's global seed doesn't fully
pin every op's RNG stream here, a Keras-side quirk noted rather than
papered over.

## 21. Multi-scenario model zoo: tabular/image/sequence, Qu vs PyTorch AND Keras (new this session)

Items 19/20 above each compare Qu against ONE other framework (PyTorch,
Keras respectively) on ONE architecture (the tabular MLP). Ahmed's follow-up
asked for 3 scenarios — tabular/MLP, image/CNN, sequence/RNN — each against
**both** PyTorch and Keras, using the model-zoo constructors
(`mlp_classifier`/`simple_cnn`/`simple_rnn_classifier`, item d0e2224) that
didn't exist when items 19/20 were written. Both frameworks confirmed
installed first (`python -c "import torch"` / `"import tensorflow"`, per the
task's own instruction), so all three scenarios have all three columns, no
gaps to report. Full methodology, architecture-matching detail, and the
honest caveats (per-framework weight init, Keras' `GRU(reset_after=True)`
gate-formula difference): [`multi_scenario/README.md`](multi_scenario/).

**Result**:

| Scenario | Qu acc (train/test) | Qu time | PyTorch acc (train/test) | PyTorch time | Keras acc (train/test) | Keras time |
|---|---|---:|---|---:|---|---:|
| Tabular/MLP (20→64→32→4, 300 ep) | 94.63%/95.33% | 2.13s | 94.75%/94.00% | 0.21s | 95.5-95.75%/95.83-96.33% | 6.59s |
| Image/CNN (16×16, 3 classes, 250 ep) | 95.83%/88.33% | 5.14s | 92.92%/90.00% | 0.24s | 95.83-99.17%/86.67-93.33% | 8.12s |
| Sequence/RNN (GRU, len 20, 3 classes, 200 ep) | 100%/100% | 13.97s | 100%/100% | 0.93s | 100%/100% | 6.28s |

PyTorch wins every scenario on speed by a wide margin (mature compiled
autograd + BLAS). Keras trails Qu on the tabular/CNN scenarios (its own
per-step Python/graph-dispatch overhead at this small scale — the same
finding item 20 made) but beats Qu on the RNN scenario, where Qu pays a
uniquely steep cost: `simple_rnn_fit`'s per-sample training loop is ALSO a
per-timestep loop internally (`gru_forward` over T=20 steps, each step its
own tracked `call_builtin` dispatch), so one epoch does `n_samples x T`
tracked op evaluations versus the CNN scenario's `n_samples x 1` — 576,000
tracked GRU-cell calls total at this scenario's scale, compounding the
per-`call_builtin` dispatch overhead items 19/20 already identified rather
than introducing a new gap. Accuracy is essentially tied across all three
languages on every scenario (RNN scenario: literally 100%/100% on all
three) — every gap here is a speed story, not a correctness one.

## 22. Learning-rate controllers: `lr_adaptive`/`lr_plateau` vs PyTorch

`BACKLOG.md`/`IMPL.md`'s "`lr_adaptive`/`lr_plateau` learning-rate
controllers, 2026-08-26" entry verified both controllers on-Qu only. This
asks the follow-up: how do PyTorch's closest equivalents behave on the
same `../mlp/` dataset/architecture/epochs? `lr_plateau` has a direct
built-in (`ReduceLROnPlateau`, tolerance matched to Qu's own
`LR_PLATEAU_TOL=1e-9` via `threshold=1e-9, threshold_mode="abs"`);
`lr_adaptive`'s LM-style reject-and-retry has no PyTorch built-in, so it's
hand-rolled to mirror `sequential_fit`'s own logic step for step. Full
methodology and honest caveats (different Qu/PyTorch initial weights, a
PyTorch timing artifact that needed a warm-up pass to fix):
[`lr_schedulers/README.md`](lr_schedulers/README.md).

**Result — both plateau schedulers are a genuine no-op on this dataset,
same reason, on both languages** (full-batch GD decreases the loss every
single epoch here, so the "epochs since improvement" counter never
reaches `patience=10`):

| | Qu `lr_plateau` | Qu fixed-lr | PyTorch `ReduceLROnPlateau` | PyTorch fixed-lr |
|---|---:|---:|---:|---:|
| final loss / final lr | 0.214134 / 0.05 | 0.214134 / 0.05 | 0.177436 / 0.05 | 0.177436 / 0.05 |
| test accuracy | 95.33% | 95.33% | 96.00% | 96.00% |

**`lr_adaptive` genuinely helps and self-corrects from a 100x-oversized
start, confirmed on both languages independently**:

| | Qu (lr₀=0.05) | Qu baseline | Qu (lr₀=5.0) | PyTorch (lr₀=0.05) | PyTorch baseline | PyTorch (lr₀=5.0) |
|---|---:|---:|---:|---:|---:|---:|
| final loss | 0.139742 | 0.214134 | **0.123139** | 0.132162 | 0.177436 | **0.091355** |
| test accuracy | 96.00% | 95.33% | 96.33% | 96.17% | 96.00% | 95.67% |

In both languages the deliberately-oversized `lr₀=5.0` run rejects its
first several candidate steps (5.0 → 2.5 → 1.25 → ...), lands `lr` around
~0.16 by the end of epoch 0, and goes on to reach the LOWEST final loss of
its language's four runs — the reject-and-retry trust region rescues a
bad starting `lr` instead of diverging, independently verified on two
different autodiff engines. Absolute loss values don't match across
languages (different initial weights — Qu's own `seed=42` He/Xavier init
vs PyTorch's numpy-loaded `init_weights.npz`, same non-issue
`multi_scenario/README.md`/`sequential_api/README.md` already document)
but the qualitative shape — real improvement over fixed-lr, non-monotonic
oscillating `lr`, self-correction from an oversized start — matches
exactly.

## 23. Advanced-language validation: correctness across 3 domains, not just speed (new this session)

Every scenario above measures speed (with correctness as a supporting
check); Ahmed's ask here inverted the emphasis: build genuine stress tests
— multiple sub-cases and edge conditions, not single happy-path calls — of
Qu's more advanced features specifically to find out honestly whether they
hold up, checked against Python for BOTH speed and accuracy. Full
methodology and every number: [`advanced_validation/README.md`](advanced_validation/README.md).

**Domain 1 (`findpeaks` vs `scipy.signal.find_peaks`)**: exact match
(locations + heights) on 5 of 6 stress cases — three SNR levels on the same
200,000-sample signal with fixed detector settings, a 15-cluster
distance-suppression boundary test, and a genuinely ambiguous near-tied
pair. **One real, root-caused behavioral gap found**: `findpeaks` misses
flat-topped (plateau) peaks entirely (strict `x[i] > x[i-1] && x[i] >
x[i+1]`, `qu-core/src/transforms.rs:782-786`) that scipy's plateau-aware
algorithm detects — a genuine gap from scipy/MATLAB parity, not fixed this
pass but pinned to the exact line. Also found: Qu's distance-suppression
step is measurably slower and scales worse than scipy's as candidate-peak
count grows (up to ~23x on the heaviest case), traced to an `O(candidates
× accepted)` inner loop vs. scipy's `O(n log n)` sort-and-sweep.

**Domain 2 (k-NN + Sequential-API Adam MLP vs scikit-learn/PyTorch)**: a
real 2000-sample/12-feature/4-class dataset. `knn_model` matches
`KNeighborsClassifier` with **0/400 elementwise prediction mismatches** —
the strongest possible signal, not just matching accuracy numbers. The
Sequential API's Adam optimizer (untested at this scale before — `mlp/`
covers manual-autodiff+bit-identical-init, `sequential_api/` covers
Sequential+SGD) converges to within ~2 points of PyTorch's test accuracy
from its own random init, ~2.1x slower per epoch — a much narrower gap
than `mlp/`'s ~5-7x manual-loop finding.

**Domain 3 (`curve_fit`/Kalman vs scipy/independent numpy)**: chosen as
the third domain specifically to avoid repeating signal-processing or ML a
third time — a numerically demanding optimization + estimation-theory
pair. `curve_fit` (Levenberg-Marquardt) matches
`scipy.optimize.curve_fit(method="lm")` to **6 decimal places** on every
case, including independently reproducing the SAME two non-obvious
alternate optima (a sign/phase ambiguity in a damped sinusoid, a
term-swap symmetry in a sum-of-two-exponentials model) from the same
starting points on both sides — strong evidence the algorithm itself is
correct, not approximately correct. A 5,000-step 2D Kalman filter matches
an independent from-scratch numpy implementation of the same linear
update equations to `5×10⁻⁷` (measurement-precision level). **Speed is
the honest downside**: `curve_fit` is 29-147x slower than scipy, traced to
a specific architectural cause (Qu calls the model function once per data
point per residual evaluation — an interpreted dispatch per point — where
scipy's numpy-vectorized model function evaluates all points in one call);
Kalman's ~2.7x gap is unremarkable interpreter overhead by comparison.

**Overall verdict**: one real correctness bug found and clearly flagged
(the `findpeaks` plateau gap) plus two real, root-caused speed gaps
(`find_peaks`'s distance-suppression complexity, `curve_fit`'s per-point
call architecture) — reported honestly rather than glossed over, per the
standing "a benchmark suite that only reports wins isn't trustworthy"
brief. Everything else checked — k-NN, the Sequential MLP's convergence,
the Kalman filter, curve_fit's actual fitted values — either matched
exactly or matched to full displayed precision.

## 24. Multisine acceleration showcase: 5 methods, 1 signal, all cross-checked (new this session)

Ahmed's ask: a real, verified showcase comparing every acceleration method
Qu offers, all computing the identical 51-tone/1,500,000-sample multisine
— not a demo that runs each variant once, but a genuine wall-clock
comparison with every signal cross-checked against the baseline before any
timing number is trusted. Full methodology, the exact `parallel for`
reduction investigation, and the GPU buffer-limit story:
[`multisine_acceleration/README.md`](multisine_acceleration/README.md).

**Result**: all 5 methods produce the numerically identical signal (diffs
from `1e-15` to `1e-6`, the latter exactly matching `gpu_matmul`'s
documented f32 precision, not a bug). Pool/queue/workers and full
vectorization both beat the baseline solidly (1.6-3.8x) and are within
noise of each other across 4 runs; `parallel for`'s only *safe* pattern
here (disjoint per-tone column writes into one shared 612 MB matrix, since
a `x = x + ...` shared-vector accumulator was confirmed to silently
compute the WRONG answer with no error — a real gap between `parallel
for`'s scalar-reassignment check, which does reject that, and its vector/
matrix one, which doesn't) turns out ~30x **slower** than doing nothing
extra, root-caused to repeated full-matrix copy-on-write clones of that
612 MB shared matrix. GPU (RTX 5080, confirmed real and correct) needed a
3-way chunked `gpu_matmul` workaround — a literal single call exceeds this
GPU's measured 128 MiB `max_*_buffer_binding_size` and crashes the whole
process with an uncatchable wgpu panic rather than a clean Qu error,
another real gap flagged, not fixed — and even chunked, GPU lands ~3x
slower than the CPU baseline here, despite this workload's `m*n*k` sitting
well above `gpu_probe_info()`'s own measured square-matmul crossover: the
crossover doesn't transfer to this workload's "skinny" matrix-vector shape,
where 3 full upload/dispatch/readback round trips dominate wall-clock in a
way a dense square GEMM at the same multiply-add count never would.

## Known gaps

- No Octave data point anywhere — Octave isn't installed on this machine
  (checked `Program Files`, `PATH`, common install dirs); matty vendors an
  Octave *source* tree (`matty/octave-9.4.0/`) but it isn't built. Worth
  installing a real Octave binary if that comparison matters.
- `qu-core`'s elementwise kernels now parallelize, but there's no fusion —
  a chained expression like `A .* B + sin(A) .* cos(B)` still does 6
  separate full-array passes (one per operator/call), each independently
  parallelized, rather than one fused pass touching each input element
  once. A real expression-fusion compiler would close more of the gap but
  is a much larger undertaking (a new optimization pass, not a kernel
  tweak) — not attempted this session; the redundant-clone fix above
  captured the cheap, safe win from the same investigation without it.
- ~~`conv`/`xcorr` FFT-crossover~~ — **done** (concurrently, later this
  session): `qu-core::transforms::full_conv` is a measured three-way
  dispatch — direct `O(n·m)` below `CONV_FFT_MIN_DIM=512`, overlap-add for
  a long signal against a short/medium kernel, whole-signal `rfft`/`irfft`
  for comparable-length pairs — each threshold picked from an actual
  release-build benchmark, documented inline in `transforms.rs`, not
  guessed. Butterworth (`butter`), elliptic/Cauer (`ellip`), Chebyshev
  types I/II (`cheby1`/`cheby2` — landed since this note was first written;
  confirmed present and benchmarked in
  [`filter_design/`](#15-filter-design--large-scale-filter-application-benchmark)
  below, so the "still genuinely open" claim this bullet used to end on is
  now stale and removed), and FIR (`fir1` windowed-sinc, `firls`
  least-squares) filter design have all landed — see the
  [CSV + multi-filter benchmark](#9-csv-load--multi-filter-benchmark-new-this-session)
  above for `sosfilt`/`filtfilt` timings at 500k samples, and
  [`filter_design/`](filter_design/) for design-step timing and
  application timing at 1M/10M samples. One real gap `filter_design/`
  itself found: `firls`'s design-step cost scales roughly `O(n^3)` in Qu
  (tens of seconds at order 1024 vs well under a second in scipy) — a
  genuine, unfixed algorithmic gap, not a missing feature.

## Should performance work continue?

Every item this index has flagged as a next step is now closed —
`for`/`while` loops, `element_wise_ops`, and `fft` all now match or beat
MATLAB, from starting gaps of 12.2x, ~9.3x, and (implicitly) behind on
`fft` respectively. What's left in the *performance* column is smaller and
more speculative: true kernel fusion for chained elementwise expressions
is a new compiler pass, not a scoped kernel change, and worth scoping
deliberately rather than starting as a drive-by. The larger open work now
is *surface area*, not speed — `conv`/`xcorr`/filter design above — which
is a different kind of decision (what to build, not how to make it fast)
and a good point to check in with Ahmed before picking the next target.
