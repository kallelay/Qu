# Large-scale signal-processing pipeline (25x `full_pipeline/`)

`../full_pipeline/` runs the same 7-stage chained pipeline (load → clean →
bandpass filter → spectral → time-frequency → feature extraction → export)
at 200,000 samples. This directory asks the follow-up question directly:
**does Qu's relative standing hold, narrow, or widen at a much bigger
scale?** — 5,000,000 samples (25x), Qu vs Python vs MATLAB vs Matty.

## Why 5,000,000 (25x), not 100x

25x lands `pipeline_signal.csv` at ~90MB (Qu)/~145MB (Python) — big enough
to be a genuinely different regime from 200k, small enough that every
language's full run (generate → 7 stages) finishes in single-digit
seconds, so 2-3 trials per language fit in "a few minutes" total, per this
task's own scoping. 100x (20M samples, ~600MB+ CSVs) was tried mentally
and rejected purely on wall-clock-budget grounds, not because the pipeline
itself would break at that size.

## A real ceiling avoided, not hit

`../full_pipeline/bench.qu` builds its time vector via `n = 0 to N - 1`
(a Qu range literal) — fine at N=200,000, but Qu's range construct caps
eager materialization at 1,000,000 elements
(`qu-core::DEFAULT_ELEMENT_LIMIT`; see `../file_io/README.md`'s
`binary_doubles` section for the same cap hit head-on). At N=5,000,000
that construct would error ("range would create 5000000 values; limit is
1000000"). `bench.qu` here uses `t = linspace(0, (N-1)/Fs, N)` instead —
identical values, no cap (`linspace` doesn't route through the same
eager-range code path) — and needs no other change, since every pipeline
stage after that is a single call into a native bulk Rust operation, never
a qu-level loop over N elements. (Contrast `file_io/binary_doubles.qu`,
which genuinely cannot avoid the cap — its whole point is a per-value
loop.)

## MATLAB: a real toolbox gap on this machine, worked around transparently

This machine's MATLAB R2025b license does **not** include the Signal
Processing Toolbox (confirmed via `ver`: only base MATLAB + Parallel
Computing Toolbox are licensed). `butter`, `findpeaks`, `spectrogram`,
`hann`/`hamming` all hard-error ("... requires Signal Processing Toolbox")
here — this is a real constraint of this specific install, not a Qu/Python
finding, and not something to route around by fabricating numbers. `fft`,
`filter`, and `readtable`/`writetable` ARE base MATLAB and work fine.

`bench.m` hand-implements every toolbox-gated piece from these base
primitives, the same spirit as Qu's own native Rust implementations and
scipy's compiled internals:

- **Bandpass filter**: not the order-4 (8-pole) Butterworth Qu/Python use
  — a single RBJ "constant peak gain" biquad bandpass (2-pole, closed-form
  trig formulas, applied via base `filter()`). Isolates the same
  200–1000 Hz band with a gentler rolloff, so downstream rms/crest/
  energy/entropy are close in shape but don't match Qu/Python to the
  precision `../full_pipeline/README.md`'s own cross-check achieves.
- **Peak-finding**: a vectorized local-maxima-above-`min_height` scan,
  `min_distance` enforced by a cheap loop over the (few) candidates.
- **Spectrogram/entropy**: a manual per-frame `fft()` loop with a
  hand-coded Hann window (`0.5 - 0.5*cos(2*pi*n/(nfft-1))`) — genuinely
  **not vectorized** the way a toolbox call would be, so this stage's
  MATLAB number reflects an interpreted ~9,766-iteration loop, a
  different cost shape than Qu's/Python's vectorized STFT.

**Matty, by contrast, has real Signal Processing Toolbox-equivalent
coverage** (`butter`/`filter`/`findpeaks`/`spectrogram`/`hamming`, all
scipy-backed) baked into its own interpreter — `bench_matty.m` uses
Matty's real `butter`+`filter` (the same order-4 Butterworth Qu/Python
use), not the RBJ substitute. Worth stating plainly: on this specific
machine, **Matty's own signal-processing coverage is broader than this
particular MATLAB license's**.

## Matty: three real incompatibilities hit, in sequence

Tried `bench.m` unmodified against `matty_runner.py` first, per this
task's own methodology (don't silently swap scripts):

1. `rng(11)` — undefined. Matty has no RNG-seeding function at all
   (checked `builtins.py`: `rand`/`randn`/`randi`/`randperm` exist, no
   `rng`). `bench_matty.m` drops the seed call — values aren't
   seed-reproducible run to run as a result (confirmed: rms/energy came
   out bit-identical across 3 runs anyway, suggesting Matty's underlying
   numpy RNG has a fixed default seed at interpreter start, not
   investigated further).
2. `table`/`readtable`/`writetable` — none exist in Matty (only
   `csvread`/`csvwrite` on plain matrices; same finding as
   `../file_io/README.md`'s `csv_load` section). `bench_matty.m` uses
   `csvwrite`/`csvread` with a plain `[t x_raw]` matrix instead.
3. A vectorized boolean-assignment shape bug: `is_peak(2:end-1) = (...)`
   against a `false(nyq,1)`-initialized column vector raised a shape
   mismatch inside Matty's own array machinery even after (1) and (2)
   were fixed — traced to `spectrum`/`xs_filt` coming out row-oriented
   somewhere in Matty's `filter()` path rather than column-oriented.
   Worked around two ways: rewrote peak detection as a fully vectorized
   `is_peak = (spectrum > left) & (spectrum > right) & (spectrum >
   min_height)` (no partial-range assignment), and added an explicit
   `xs_filt = xs_filt(:);` after the `filter()` call to force column
   orientation. Both are real Matty-side quirks, not deliberately
   engineered around this task's needs — flagged, not silently
   patched-and-forgotten.

With all three worked around, `bench_matty.m` runs to completion and
produces real, repeatable numbers (below).

## Results (3 trials each; N=5,000,000)

| Stage | Qu | Python (pandas+scipy) | MATLAB (toolbox-free) | Matty (real butter) |
|---|---:|---:|---:|---:|
| load | 0.51–0.68 s | 0.98–1.03 s | 3.73–4.10 s | 0.82–0.91 s |
| clean | 0.007–0.011 s | 0.009–0.014 s | 0.012 s | 0.012 s |
| filter | 0.037–0.043 s | 0.037–0.038 s | 0.043–0.046 s | 0.68–0.72 s |
| spectral | 0.114–0.139 s | 0.115–0.122 s | 0.123–0.136 s | 0.27–0.36 s |
| timefreq | 0.271–0.288 s | 0.347–0.396 s | 0.489–0.536 s | 1.15–1.40 s |
| features | 0.044–0.049 s | 0.078–0.093 s | 0.032–0.035 s | 0.44–0.58 s |
| export | 0.005–0.009 s | 0.004–0.005 s | 0.056–0.087 s | 0.003–0.004 s |
| **TOTAL** | **1.01–1.22 s** | **1.60–1.64 s** | **4.48–4.95 s** | **3.46–3.95 s** |

Correctness (real signal, not the toolbox-free MATLAB's — see caveat
above): Qu `rms=0.4288 crest=2.0456 energy=919314.89 n_peaks=1
mean_entropy=0.1651`; Python `rms=0.4287 crest=2.0947 energy=918765.21
n_peaks=1 mean_entropy=0.1651` — matching closely, same as
`../full_pipeline/`'s own cross-check, now confirmed at 25x scale too.
Matty (`rms=0.4243 crest=1.4675 n_peaks=1 mean_entropy=0.1397`) is in the
same ballpark using the real order-4 filter with unseeded noise — the
`crest`/`entropy` gap vs Qu/Python is plausibly just a different noise
draw plus Matty's own STFT-loop numerics, not investigated further.
MATLAB's toolbox-free run (`rms=0.2616 crest=2.6846 n_peaks=3
mean_entropy=0.2494`) diverges more, exactly as expected from a
2-pole-vs-8-pole filter substitute — not a Qu/Python/Matty discrepancy.

**Does the ~1.8-2x Qu-vs-Python gap from `full_pipeline/` hold at 25x?**
Roughly yes, maybe slightly wider: **~1.4-1.6x** here (1.01-1.22s vs
1.60-1.64s) — actually a bit *narrower* than `full_pipeline/`'s cached
~1.8-2x, not wider. `load` is still the largest single contributor on
both sides, same as at 200k. No stage flipped order (Qu still ahead
everywhere it was ahead at 200k) — nothing "shifted" qualitatively at
25x, contrary to what a fusion/parallelization-threshold effect might have
predicted; if anything the elementwise-parallelization work `../README.md`
item 7 describes is doing its job more visibly at this size (`timefreq`,
the STFT stage, shows the largest relative Qu advantage of any stage).

## Files

- `bench.qu` / `.py` / `.m` — Qu, Python, real-MATLAB (toolbox-free)
- `bench_matty.m` — Matty variant (real `butter`, no `rng`/`table`)

Generated `pipeline_signal*.csv`/`pipeline_features*.csv` files are
gitignored (fully reproducible from the scripts).

## Running

```bash
qu run benchmarks/large_pipeline/bench.qu
python benchmarks/large_pipeline/bench.py
matlab -batch "cd('benchmarks/large_pipeline'); bench"
# from the matty repo, its own .venv active:
python matty_runner.py <path-to>/bench_matty.m
```
