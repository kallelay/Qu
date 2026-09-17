# File I/O benchmark: binary, CSV, image

Ahmed asked for (1) file loading/saving benchmarks specifically (binary
packed data, CSV, images) and (2) a much larger-scale signal-processing
pipeline (see `../large_pipeline/`) — both **Qu vs Python vs MATLAB vs
Matty** (Matty added mid-task; see "Matty" notes throughout). This
directory covers (1): three scenarios, each with a real, honest reason for
the sizes chosen (not all "10M+" as originally suggested — see
"binary_doubles" below for why).

Existing benchmarks already touch CSV loading (`../csv_filters/`, 500k
rows/2 cols, focused on load-then-filter; `../data_wrangling/` — a
concurrent session's WIP, 1M rows, focused on table ops after load) and
image processing (`../image_processing/` — a concurrent session's WIP CV
pipeline). This directory is narrower and load/save-focused: pure I/O
throughput, nothing downstream.

Machine/methodology context: same machine as `../README.md`'s index (AMD
Ryzen, RTX 5080; MATLAB R2025b via `matlab -batch`). Single-trial-per-run
numbers, 3 trials per script where the run is fast enough, reported as
ranges — not statistically rigorous, same convention as every other
benchmark in this repo.

## 1. Packed float64 binary round trip (`binary_doubles.*`)

**Why not "10M+ values" as originally scoped**: Qu's file I/O library
(`IMPL.md` "File I/O library", 2026-08-24) is scalar-call-based —
`write_double(f, x)`/`read_double(f)`, one value per call, no bulk
`fread`/`fwrite`-style vectorized path. Building/reading a value into that
loop hits two *separate*, real ceilings:

1. Qu's `for k = 0 to N-1` range construct eagerly materializes the range
   as a vector, capped at 1,000,000 elements
   (`qu-core::DEFAULT_ELEMENT_LIMIT`, `NumericError::ElementLimit` — a
   deliberate safety cap, not a bug). **N=1,000,000 is the actual ceiling**
   for any Qu script that loops a fixed number of times via this syntax.
2. Storing each read value into a *vector* via `y[k] = read_double(f)`
   used to hit a second, independent, and much worse cost — **fixed as of
   2026-08-26** (see `IMPL.md`'s "`exec_index_assign` O(N²) → O(N)
   indexed-assignment fix" entry for the full root-cause writeup). The old
   `exec_index_assign` (`engine/crates/qu-interp/src/lib.rs`) resolved the
   assignment target via `self.var_get(name).cloned()`, which bumped the
   `Arc<Vec<f64>>`'s refcount to 2 (the original was still live in
   `self.env`) *before* calling `Arc::make_mut(&mut xs)`. `make_mut` only
   mutates in place at refcount 1 — it was always 2 at that point, so
   **every indexed assignment deep-cloned the entire vector**, making a
   "fill this vector in a loop" pattern O(N²), not O(N). Confirmed
   directly (not inferred) on the old code: reading 200,000 doubles into a
   vector this way took **126.06s**; reading the same file with a scalar
   running-sum accumulator instead of a vector store took **0.1585s** —
   roughly 800x faster for the *identical* 200,000 `read_double` calls,
   isolating the cost to the index-assignment, not the file read itself.
   The fix (`var_take`, `engine/crates/qu-interp/src/lib.rs`) removes the
   target from its scope instead of cloning it, so `Arc::make_mut` sees
   the *true* refcount — 1 when uniquely owned (in-place mutation), 2+
   only when another variable genuinely aliases the same buffer (still
   correctly copies then). Same root cause also existed in
   `exec_cvec_index_assign` (`Z[i] = ...` on complex vectors) and
   `exec_matrix_index_assign` (`M[i,j] = ...`) — both fixed the same way.
   Re-measured after the fix: the same 200,000-element `y[k] =
   read_double(f)` materialize loop now takes **~0.06s** (down from
   126.06s, a **~2100x** speedup), and scales linearly — see the updated
   `materialize_demo.qu` table below. `append()` is intentionally NOT part
   of this fix: it's Qu's designed *pure*, return-a-new-vector builtin
   (`y = append(y, v)`), not an in-place mutation path, so its
   `xs.as_ref().clone()`-then-push shape is by-design copy semantics, not
   an instance of this bug. Record/List have no in-place indexed-assignment
   path at all (`x[i] = v` only exists for `Vec`/`Signal`/`CVec`/`Mat`), so
   there was nothing to fix there.

So `binary_doubles.qu` runs two variants at **N=1,000,000** (the range-cap
ceiling): a write loop (`write_double`, no array mutation — just
indexing/reading `x[k]`, which is cheap) and a **streaming** read loop
(`read_double` + running scalar sum, no array store — the fast path).
`materialize_demo.qu` separately times the vector-store variant at
N=20,000/40,000/80,000 to show the O(N²) shape without the multi-minute
wait a 200k+ run would need.

Python/MATLAB/Matty use their natural vectorized `tofile`/`fromfile` and
`fwrite`/`fread` — one call, not a loop — since that's the idiomatic way
to do this in those languages; `binary_doubles.py` also includes a
`struct.pack`/`unpack`-per-value variant (N=10,000 only) purely to show
what "one Python call per value" costs too, since that's the fairer
apples-to-apples against Qu's *only* available API shape.

### Results (3 trials each; N=1,000,000 unless noted)

| Operation | Qu | Python (numpy) | MATLAB | Matty |
|---|---:|---:|---:|---:|
| write (vectorized where available) | 14.0–19.5 s | 0.013–0.016 s | 0.022–0.040 s | 0.009–0.011 s |
| read (streaming/vectorized) | 0.67–1.67 s | 0.003–0.031 s | 0.003–0.005 s | 0.003–0.005 s |

Python's own per-value loop, N=10,000 only (not the same N — shown for
scale, not a row-for-row comparison): `struct.pack` **0.0074–0.0116 s**,
`struct.unpack` **0.0054–0.0091 s** — i.e. Python's *own* per-call
overhead for this pattern is ~0.7–1.2 μs/call, vs Qu's write path at
~14–20 μs/call and its streaming-read path at ~0.7–1.7 μs/call (comparable
to Python's per-call cost once the O(N²) vector-store trap is avoided).

**Honest read**: Qu's per-call file I/O is not itself pathologically slow
(the streaming read is in the same ballpark as Python's own per-value
loop) — the write side's ~14–20 μs/call is dominated by `fopen("wb")`
opening a raw, **unbuffered** `std::fs::File` (`FileHandleState.writer`),
so every `write_double` is a real, uncached OS write syscall, not a
buffered one. The dramatically larger gap against numpy/MATLAB/Matty is
architectural: those call one vectorized bulk operation; Qu has no bulk
binary read/write API at all today, only per-value calls, so N calls
always means N syscalls/interpreter round-trips, regardless of how fast
any single one is. Correctness: `mean(written)` == `mean(read)` and
`sumsq(read)` cross-checked exactly (bit-identical) on every Qu run.

**`materialize_demo.qu` — FIXED, 2026-08-26.** Was the O(N²) vector-store
cost; now genuinely O(N). Real runs, not extrapolated, before/after the
`exec_index_assign`/`var_take` fix (see `IMPL.md`):

| N | time (before fix) | time (after fix, this script re-run as-is) |
|---:|---:|---:|
| 20,000 | 0.0722 s | 0.0145 s |
| 40,000 | 0.2510 s | 0.0253 s |
| 80,000 | 1.1661 s | 0.0425 s |

Separately, at the sizes this whole finding was originally measured at
(200,000-element reads, via a one-off script mirroring this one): **126.06
s before the fix → 0.0616 s after**, a real **~2050x** speedup. Scaling
check across a wider range after the fix (50,000 / 100,000 / 200,000 /
400,000 / 800,000): 0.0209 s / 0.0349 s / 0.0616 s / 0.1387 s / 0.2484 s —
every doubling ~1.7–2.2x, consistent with O(N); before the fix each
doubling in the table above was ~3.5–4.6x, consistent with O(N²) (a clean
4x is exactly quadratic). The underlying bug (`exec_index_assign` always
seeing refcount 2 because `var_get(name).cloned()` itself created the
second `Arc` owner) is fixed by `var_take`, which removes the target from
its scope instead of cloning it, so `Arc::make_mut` sees the real
refcount. Aliasing safety (`z = y; y[k] = ...` must leave `z` unchanged)
is preserved and covered by existing/new unit tests in `qu-interp` (see
`IMPL.md`).

**Matty note**: `binary_doubles.m` fails immediately under
`matty_runner.py` — Matty has no `rng()` (`Undefined function or variable
'rng'`). `binary_doubles_matty.m` is the same script with the seed call
removed (values aren't seed-reproducible run to run as a result, but the
I/O mechanics are identical) — Matty's `fopen`/`fwrite`/`fread`/`fclose`
are real and land in the same performance league as MATLAB/numpy.

## 2. Large multi-column CSV load (`csv_load.*`)

3,000,000 rows x 6 numeric columns (bigger and wider than
`../csv_filters/`'s 500k x 2) — pure `read_csv`/`read_csv`-equivalent
throughput, no downstream filtering/table-ops.

### Results (3 trials each)

| | Qu (`read_csv`) | Python (`pandas.read_csv`) | MATLAB (`readtable`) | Matty (`csvread`) |
|---|---:|---:|---:|---:|
| load | 0.93–1.14 s | 1.88–2.92 s | 6.24–7.16 s | 1.88–2.32 s |

**Qu is fastest of all four** on this shape — ahead of pandas by ~2x and
of MATLAB's `readtable` by ~6-7x. Consistent with `../csv_filters/`'s own
finding that the `Table::from_csv` single-pass-parse rewrite "now edges
out pandas." MATLAB's `readtable` builds a full table object (type
inference, variable names, etc.) rather than a plain numeric matrix, which
likely explains a meaningful part of its gap here — not investigated
further (out of scope: this is a benchmark, not a MATLAB optimization
task). Matty's `csvread` (plain-matrix, no table machinery, scipy/
numpy-backed) lands close to pandas.

**Matty note**: `csv_load.m` (the real-MATLAB script, using
`rng`+`table`+`readtable`+`writetable`) fails immediately under Matty —
`rng` is missing, and separately, **Matty has no `table`/`readtable`/
`writetable` at all** (checked `builtins.py`: only `csvread`/`csvwrite` on
plain matrices exist). `csv_load_matty.m` is a from-scratch equivalent
using those instead (numbers above) — not the same script, a genuine API
gap, not a one-line skip.

## 3. Image save/load round trip (`image_io.*`)

2000x2000 grayscale-from-random image (4,000,000 pixels). Qu's
`load_image` is BMP-only by design (see its own doc comment in `lib.rs` —
no PNG/JPEG decoder), so the round trip is BMP→BMP; `.png` is timed
save-only (encode), with no Qu-side decode to compare against Python/
MATLAB's `imread(.png)`.

### Results (3 trials each)

| Operation | Qu | Python (Pillow) | MATLAB |
|---|---:|---:|---:|
| save `.bmp` | 0.019–0.098 s | 0.031–0.056 s | 0.105–0.192 s |
| save `.png` | 0.121–0.220 s | 0.369–0.414 s | 0.966–1.149 s |
| load `.bmp` | 0.012–0.020 s | 0.012–0.017 s | 0.115–0.304 s |

Qu is fastest on every row here, including `.png` save — consistent with
`../README.md` item 10's note that Qu's PNG encoder is a real,
spec-correct **but uncompressed** ("stored block" DEFLATE) encoder, so
it's doing meaningfully less work than Pillow's/MATLAB's real DEFLATE
compression, not a fully apples-to-apples "PNG encoder speed" comparison —
flagged, not hidden. `.bmp` (uncompressed in every language) is the fairer
comparison and Qu still leads there. Correctness: round-tripped mean pixel
value matches the pre-save matrix mean to 3+ decimal places in every
language (e.g. Qu: 127.4575 vs 127.4572).

**Matty note**: no `imwrite`/`imread`/any image-file codec exists in Matty
at all (checked `src/graphics.py`/`builtins.py` — only a matplotlib-style
`imshow` for interactive plotting). `image_io.m` also fails immediately on
`rng` regardless. This scenario **cannot run under Matty** — not a missing
seed or a workaround-able gap, a genuine capability that doesn't exist
there yet.

## Files

- `binary_doubles.qu` / `.py` / `.m` — packed float64 round trip (§1)
- `binary_doubles_matty.m` — Matty variant (no `rng`) for §1
- `materialize_demo.qu` — the O(N²) vector-store cost demo (§1)
- `csv_load.qu` / `.py` / `.m` — wide CSV load (§2)
- `csv_load_matty.m` — Matty variant (no `rng`/`table`) for §2
- `image_io.qu` / `.py` / `.m` — image round trip (§3; no Matty variant, see above)

Generated data files (`.bin`/`.csv`/`.bmp`/`.png` produced by running any
of the above) are gitignored, same convention as every other benchmark
here — fully reproducible from the scripts themselves.

## Running

```bash
qu run benchmarks/file_io/binary_doubles.qu
qu run benchmarks/file_io/materialize_demo.qu
qu run benchmarks/file_io/csv_load.qu
qu run benchmarks/file_io/image_io.qu

python benchmarks/file_io/binary_doubles.py
python benchmarks/file_io/csv_load.py
python benchmarks/file_io/image_io.py

matlab -batch "cd('benchmarks/file_io'); binary_doubles"
matlab -batch "cd('benchmarks/file_io'); csv_load"
matlab -batch "cd('benchmarks/file_io'); image_io"

# from the matty repo, its own .venv active:
python matty_runner.py <path-to>/binary_doubles_matty.m
python matty_runner.py <path-to>/csv_load_matty.m
```
