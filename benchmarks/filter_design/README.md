# Filter design + large-scale filter application benchmark

Two things `csv_filters/` doesn't cover:

1. **Filter *design*** — the coefficient-computation step itself (`butter`/
   `cheby1`/`cheby2`/`ellip`/`fir1`/`firls`), timed on its own, not mixed
   into an application step. `csv_filters/bench.qu` designs its filters
   once, untimed, then only times *applying* them.
2. **Filter *application* at much larger scale** — `csv_filters/` applies
   `sosfilt`/`filtfilt` to 500k samples; this goes to 1M and 10M to see
   whether the picture changes at real "big data" scale.

Four engines compared, not three: **Qu**, **Python** (scipy.signal),
**MATLAB** (R2025b), and **Matty** (a separate sibling MATLAB/Octave-
compatible interpreter at `../matty` (a sibling checkout), its own
NumPy/SciPy-backed engine — not the `benchmarks/matty_suite/` JAX script,
which is stale/retired). See "MATLAB availability" below for a real,
machine-specific wrinkle that shows up immediately.

## Files

| File | Role |
|---|---|
| `design.qu` / `design.py` / `design.m` | Filter design timing (coefficients only) |
| `apply_scale.qu` / `apply_scale.py` / `apply_scale.m` | `sosfilt`/`filtfilt` (or equivalent) at 1M/10M samples |

`design.m` and `apply_scale.m` are each **one script that runs unmodified
under both real MATLAB and Matty** — see "MATLAB availability" and the
comments at the top of each `.m` file for exactly what each interpreter
does differently at runtime (guarded via `exist(...)`/`try`, not two
separate files).

Run:

```bash
cargo run --manifest-path engine/Cargo.toml -p qu-cli --release -- run benchmarks/filter_design/design.qu
cargo run --manifest-path engine/Cargo.toml -p qu-cli --release -- run benchmarks/filter_design/apply_scale.qu
python benchmarks/filter_design/design.py
python benchmarks/filter_design/apply_scale.py
matlab -batch "run('benchmarks/filter_design/design.m')"
matlab -batch "run('benchmarks/filter_design/apply_scale.m')"
# from a sibling `matty` checkout:
./.venv/Scripts/python.exe matty_runner.py <path-to-design.m-or-apply_scale.m>
```

## MATLAB availability — a real, discovered blocker, not a choice

This machine's licensed **MATLAB R2025b has no Signal Processing Toolbox
installed.** Confirmed directly, not assumed:

```
>> license('test','signal_toolbox')
ans = 1        % license entitlement says "yes"
>> ver
MATLAB                    Version 25.2   (R2025b)
Parallel Computing Toolbox Version 25.2   (R2025b)
% ... no Signal Processing Toolbox in the list
>> butter(4, 0.2)
Undefined function 'butter' ... requires one of: DSP System Toolbox, Signal Processing Toolbox
```

The license server reports the entitlement as available, but the toolbox
itself isn't installed on this machine — so `butter`/`cheby1`/`cheby2`/
`ellip`/`fir1`/`firls`/`filtfilt`/`sosfilt` are **all unavailable in real
MATLAB here**, and the **filter *design* benchmark has no real-MATLAB
column at all** (marked N/A in the table below, not fabricated). Base
MATLAB *does* still have `filter` (direct-form application) and `conv` —
core-language functions, not toolbox ones — confirmed working.

Matty fills the gap for design (it reimplements the toolbox's filter-design
family over scipy — see its `TODO.md`) and *is* a genuine fourth
comparison point, not a MATLAB substitute in disguise: it's MATLAB/Octave
*syntax* run through a different (NumPy/SciPy-backed) engine, and its
numbers include Matty's own interpreter overhead on top of the underlying
scipy calls it wraps.

For `apply_scale.m`'s large-signal application step, base MATLAB's `filter`
still works without the toolbox — the script hardcodes a literal `[b,a]`
pair (a real order-4/500 Hz/10 kHz-Fs Butterworth lowpass, computed via
`scipy.signal.butter` since this MATLAB can't compute it itself) and uses a
hand-rolled forward+backward double `filter()` call in place of `filtfilt`
(labeled `manual_filtfilt`, not `filtfilt` — it skips the edge-padding /
initial-condition matching the real toolbox function does, so it's a
*similar*, not identical, zero-phase approximation). Matty has both
`butter` and a real `filtfilt` (scipy-backed), so it takes the full path
there.

## Dataset / filter parameters

Same lowpass target throughout both benchmarks: `Fs = 10 kHz`, cutoff
`500 Hz` (`Wn = 0.1` normalized, `Nyquist = 5 kHz`) — `rp = 1 dB` passband
ripple (`cheby1`/`ellip`), `rs = 40 dB` stopband attenuation (`cheby2`/
`ellip`). FIR `firls`'s transition band straddles the same cutoff
(`[0, 0.08, 0.12, 1]` normalized, desired `[1,1,0,0]`).

## Part 1: filter design timing

A single design call is fast enough (low microseconds for most IIR
families) that one `tic`/`toc` pair would mostly measure timer overhead —
every scenario runs the design call in a loop and reports the per-call
average. Reps are scaled down per scenario where the call itself is slow
(see `firls` below) so the whole script still finishes in reasonable time.
**2-3 trials per language**, run back-to-back on this same (shared, other
processes present) machine — ranges below span what was actually observed,
not error bars.

### IIR: `butter` / `cheby1` / `cheby2` / `ellip` (us per call)

| Order | Filter | Qu | Python (scipy) | Matty | MATLAB |
|---|---|---:|---:|---:|---:|
| 4 | butter | 1.4–2.0 | 470–490 (one noisy trial: 1275) | 350 | N/A |
| 4 | cheby1 | 1.8–2.2 | 470–530 (1630) | 266 | N/A |
| 4 | cheby2 | 1.7–4.4 | 510–540 (1730) | 295 | N/A |
| 4 | ellip | 5.6–12.3 | 605–715 (1740) | 380 | N/A |
| 8 | butter | 1.6–9.4 | 735–790 (2020) | 288 | N/A |
| 8 | cheby1 | 2.0–23.1 | 795–835 (2360) | 394 | N/A |
| 8 | cheby2 | 2.1–2.8 | 790–915 (2590) | 359 | N/A |
| 8 | ellip | 7.0–11.6 | 890–950 (1740) | 376 | N/A |
| 16 | butter | 2.2–2.6 | 1395–1400 (1975) | 352 | N/A |
| 16 | cheby1 | 2.4–4.4 | 1395–1440 (3170) | 408 | N/A |
| 16 | cheby2 | 2.6–4.2 | 1210–1370 (3100) | 430 | N/A |
| 16 | ellip | 8.6–9.7 | 1440–1560 (3865) | 654 | N/A |

The bracketed number in each Python cell is a single noisy first trial (see
"Surprises" — this machine had other MATLAB processes running in the
background at the time, confirmed via `tasklist`; the other two trials,
run back-to-back, were consistent with each other). Treat the un-bracketed
range as the trustworthy figure.

### FIR: `fir1` (windowed-sinc) — us per call

| Order (taps) | Qu | Python (`firwin`) | Matty |
|---|---:|---:|---:|
| 64 (65) | 3.7–4.3 | 95–230 | 120 |
| 256 (257) | 10.7–47.5 | 107–275 | 145 |
| 1024 (1025) | 32.7–53.0 | 150–180 | 185 |

MATLAB: N/A, same toolbox gap.

### FIR: `firls` (least-squares) — fixed, 2026-08-26

**Update, 2026-08-26: fixed.** The "genuine surprise" below was a real,
reproducible `O(n^3)`-with-a-huge-constant algorithmic gap, root-caused and
fixed the same day — see `qu-core/src/filter.rs`'s `firls` doc comment for
the full derivation, and `IMPL.md`'s
"`firls` O(n^3) → closed-form fix" entry for the before/after story.
Summary: the old code discretized each band into an `O(taps)`-point
frequency grid and ran a generic dense least-squares solve over it, so
*both* dimensions of that solve grew with the filter order, making the
whole thing `O(taps^3)`. The fix replaces the grid with the classical
closed-form (Parks & Burrus) analytic-integral normal equations — a compact
`(n/2+1)x(n/2+1)` system, independent of any frequency grid — solved with
the same pseudo-inverse solver as before, just over a far smaller matrix.
Verified to match the old grid-based algorithm (cross-checked at high grid
density, itself scipy-validated) to `<5e-6` absolute coefficient error
across 7 orders/band configurations before the grid path was removed
(`firls_matches_the_reference_grid_based_solution` in `filter.rs`).

| Order (taps) | Qu, before (3 trials) | Qu, after (3 trials) | Speedup | Python (`firls`) |
|---|---:|---:|---:|---:|
| 64 (65) | 8.0–19.4 **ms** | 58.8–61.9 **us** | ~130–330x | 254–550 **us** |
| 256 (257) | 308–576 **ms** | 1.79–2.00 **ms** | ~155–320x | 1.5–2.8 **ms** |
| 1024 (1025) | 26.0–39.6 **s** | 73.0–80.0 **ms** | ~325–540x | 85–490 **ms** |

("Before" is the original three-trial measurement further up; "after" is 3
fresh trials of the same `design.qu` script, release build, this machine,
REPS bumped up now that each call is cheap — see the script's own `firls`
section for exact REPS per order.) **Qu's `firls` is now in the same
ballpark as scipy's at every order tested, and faster than scipy at 64 and
1024** — the 50-500x gap reported below is closed, not just narrowed. The
fix is still a dense solve (`O((n/2)^3)`, not a `Toeplitz`/Levinson-Durbin
`O(n^2)` one): `Q` here is Toeplitz-*plus*-Hankel, not pure Toeplitz, so
plain Levinson-Durbin doesn't apply to it, and neither MATLAB's nor SciPy's
own `firls` bothers with a structured solve either — both build this same
kind of small dense system and hand it to a generic solver, because at
`n/2` (not `n`) the cubic cost is already fast enough that a further
`O(n^2)` solve isn't needed to be scipy-competitive. Matty still has no
`firls` builtin at all (checked `TODO.md` and `src/builtins.py` directly),
so it's still skipped in the table above, not silently faked.

<details>
<summary>Original finding (before the fix), kept verbatim for the record</summary>

| Order (taps) | Qu | Python (`firls`) | Matty |
|---|---:|---:|---:|
| 64 (65) | 8.0–19.4 **ms** | 254–550 **us** | not implemented |
| 256 (257) | 308–576 **ms** | 1.5–2.8 **ms** | not implemented |
| 1024 (1025) | 26.0–39.6 **s** | 85–490 **ms** | not implemented |

**Qu's `firls` is 50-500x slower than scipy's at order 64/256, and roughly
100-450x slower at order 1024 (tens of seconds vs under half a second).**
Scaling the order 64→128→1024 measurements against each other (a separate
probe run, not in the committed script) showed Qu's per-call cost growing
almost exactly as `O(n^3)` — consistent with a dense linear solve over the
full order-sized system with no exploitation of the Toeplitz/symmetric
structure real implementations use to cut this down. This is a genuine,
reproducible algorithmic gap, not measurement noise (three independent
trials all showed the same order-of-magnitude story) — flagged here as a
finding, not fixed as part of this benchmarking pass. Matty has no `firls`
builtin at all (checked `TODO.md` and `src/builtins.py` directly — every
other filter-design function in this benchmark is there, `firls` simply
isn't), so that scenario is skipped under Matty, not silently faked.

</details>

## Part 2: filter application at 1M / 10M samples

Butterworth lowpass (order 4, 500 Hz/10 kHz) applied to a synthetic
3-tone-plus-noise signal generated directly in memory (no CSV round trip —
`csv_filters/`'s own 500k-sample benchmark already covers that combination).
`t = linspace(0, (N-1)/Fs, N)` is used instead of Qu's `a to b` range
literal for the time axis — the range literal has a deliberate
1,000,000-element safety cap (`qu-core::DEFAULT_ELEMENT_LIMIT`, guarding
against e.g. a typo'd bound turning into a huge accidental allocation),
which the 10M-sample case trips; `linspace` has no such cap and produces
identical output.

**Correctness cross-check**: `rms(y)` on the causal output lands at
**0.7506–0.7507 in all four engines**, matching the analytically-expected
`1/sqrt(2)` for a filter isolating the unit-amplitude 50 Hz tone — same
check `csv_filters/README.md` uses, now confirmed across 4 implementations
instead of 2.

### Results (seconds, 2 trials per language)

| Step | N | Qu | Python (scipy) | MATLAB (`filter`/manual) | Matty |
|---|---:|---:|---:|---:|---:|
| causal (`sosfilt`/`filter`) | 1,000,000 | 0.0061–0.0074 | 0.0065–0.0074 | 0.0187–0.0298 | 0.0087–0.0097 |
| zero-phase (`filtfilt`) | 1,000,000 | 0.0135–0.0192 | 0.0155–0.0199 | 0.0254–0.0282 (manual) | 0.0173–0.0323 |
| causal (`sosfilt`/`filter`) | 10,000,000 | 0.0615–0.0918 | 0.0668–0.0918 | 0.0989–0.1070 | 0.0773–0.0857 |
| zero-phase (`filtfilt`) | 10,000,000 | 0.1341–0.1714 | 0.1930–0.2332 | 0.2100–0.2248 (manual) | 0.2322–0.2397 |

**Does the picture change at 10x the scale of `csv_filters/` (500k)?** Not
qualitatively — Qu, scipy, and Matty all stay within roughly 1.3x of each
other at both 1M and 10M, same "essentially tied" story `csv_filters/`
found at 500k (all four are ultimately dominated by a tight compiled
biquad-cascade or direct-form loop). MATLAB's numbers here use a
*different* code path (hardcoded direct-form `[b,a]` + a hand-rolled
double-filter, not real `sosfilt`/`filtfilt`) because of the toolbox gap
above, so its column is the least directly comparable of the four — it's
in a similar ballpark, not a clean apples-to-apples number. Scaling from 1M
to 10M is close to linear for every engine at both filter types (roughly
10x samples -> 8-13x time), as expected for an O(N) filtering algorithm.

## What's out of scope here

- **Filter design accuracy/stability at order 16** — this benchmark only
  timed the design calls; it didn't check e.g. pole-radius stability of
  the resulting order-16 elliptic/Chebyshev designs (a real, separate
  numerical-analysis question, not a benchmarking one).
- **Fixing Qu's `firls` scaling** — flagged as a finding, not addressed;
  a real fix (Toeplitz-aware solve) is a `qu-core::numeric::filter`
  algorithm change, out of scope for a benchmarking pass, and Ahmed should
  decide whether/when it's worth prioritizing given `firls` is the one
  filter-design function this large a gap shows up in.
- **A working MATLAB Signal Processing Toolbox comparison** — genuinely
  blocked on this machine (see above); would need either installing the
  toolbox or running on a machine that has it.
