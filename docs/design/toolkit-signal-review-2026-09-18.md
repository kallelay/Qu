# Signal-toolkit review, 2026-09-18

Code review of all 7 branches from tonight's v0.3.0 signal-processing toolkit
push (all now merged into master), by Qu-Aspirer. Each branch was reviewed
with 4-5 finder passes (line-by-line diff scan; removed-behavior audit;
cross-file tracer; reuse/simplification/efficiency; altitude/conventions)
followed by an adversarial verification pass on every candidate — nothing
below is un-verified. 62 findings total, all CONFIRMED unless noted.

Severity is my own judgement, not a formal field: **P0** = silent wrong
answer or a crash, worth fixing soon; **P1** = real defect but lower blast
radius (misleading error, missing warning, edge case); **P2** = cleanup
(reuse/simplification/efficiency/documentation) with no behavioral bug.

Branch base for all diffs: `506c8733` (the commit all 7 branched from).

## Branch 1 — `claude/qu-aspirer-0.3.0-gaps` (mine: fft scaling=, spectrum_normalize/unnormalize, K-point goertzel)

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | P1 | `lib.rs:24818` | `band_power` refuses an amplitude/rms-scaled spectrum citing "out of scope", but this same commit's own `unapply_spectrum_norm` is the exact fix, unused |
| 2 | P0 | `lib.rs:21936` | `goertzel(x, [5])` (bracket typo) now silently succeeds as a length-1 `CVec` instead of erroring like it used to |
| 3 | P1 | `lib.rs:24733` | `spectrum_normalize` checks for unknown `scaling=` before checking "already normalized", so a bad call on an already-scaled spectrum reports the wrong problem |
| 4 | P2 | `lib.rs:12150` | Dead code: a `Signal`-handling branch in `eval_fft`'s general path can never execute given the fast-path routing |
| 5 | P1 | `lib.rs:21936` | `goertzel(x, "abc")` shows a misleading "expected a numeric vector" error instead of the clearer original scalar-type error |
| 6 | P2 | `lib.rs:12084` | `fft`'s `scaling=` parser duplicates `rfft`'s verbatim |
| 7 | P2 | `lib.rs:24733` | `spectrum_normalize` reimplements `SpectrumNorm::from_name` instead of calling it |
| 8 | P2 | `lib.rs:24724` | `spectrum_normalize`/`unnormalize` duplicate Spectrum-extraction boilerplate the adjacent combined arm already shares |
| 9 | P2 | `lib.rs:21931` | `goertzel`/`goertzel_freq` duplicate their scalar-vs-vector dispatch block |
| 10 | P2 | `builtin_docs.rs:350` | `idft`/`ifft`/`irfft` docs don't mention the new scaling guard they now enforce |

## Branch 2 — `claude/spectrum-psd-builtins` (spectrum(), psd())

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `lib.rs:24676` | `psd()`/`welch()` documented "bit-identical" but disagree by one sample on odd segment lengths (rounding vs. truncation) — numerically verified; the branch's own test only covers even lengths |
| 2 | P1 | `lib.rs:35652` | `welch`/`periodogram`/`csd`/`spectral_coherence` (pre-existing functions) now silently accept `"rectangular"`/`"boxcar"`/`"none"` window names, undocumented on those 4 functions' own doc comments |
| 3 | P1 | `lib.rs:35668` | Window error message omits the `"boxcar"`/`"none"` aliases it actually accepts |
| 4 | P2 | `lib.rs:24655` | `psd`'s segment-bounds-check duplicates `welch`'s near-verbatim |
| 5 | P2 | `lib.rs:24605` | `spectrum()`'s windowing loop does a redundant per-sample division instead of precomputing the reciprocal once |
| 6 | P2 (plausible) | `lib.rs:24573` | `spectrum()`'s `scaling=` parse duplicates `rfft`'s pattern (different defaults, so not a drop-in shared helper) |

## Branch 3 — `claude/block-processor` (block_process(), gain(), delay())

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `lib.rs:22170` | `delay()` panics on integer overflow for an extreme negative shift (`n_f as i64` saturates to `i64::MIN`) in debug/test builds; release silently wraps |
| 2 | P0 | `lib.rs:22128` | `gain()` never validates `db` for NaN — `gain(x, 0/0)` silently turns the whole signal to NaN with no error |
| 3 | P0 | `lib.rs:23933` | `block_process` decides Signal-vs-Vec tagging from aggregate output length only, not per-block consistency — a buggy per-block function with non-uniform lengths that happen to sum right gets silently mistagged |
| 4 | P1 | `builtin_docs.rs:100` | `help("block_process")` never mentions its no-state-between-blocks limitation — the doc-generator picks the first sentence of a shared table cell and the caveat is the last sentence |
| 5 | P1 | `lib.rs:23877` | None of `block_process`'s own error messages mention the statelessness limitation either |
| 6 | P2 | `lib.rs:22129` | `gain` duplicates `db2mag`'s formula instead of calling it, despite a comment claiming to reuse it |
| 7 | P2 | `lib.rs:22168` | `delay` clones its input unnecessarily (`to_cow().into_owned()` where a borrow would do) |
| 8 | P2 | `lib.rs:23899` | `block_process` clones its input unnecessarily too, even before `block=` is validated |
| 9 | P2 | `lib.rs:22122` | `gain`/`delay` duplicate an identical "required argument" guard instead of sharing one helper |

## Branch 4 — `claude/signal-diagnostics` (is_clipped, find_clipping, detect_saturation, verify_signal)

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `qu-core/diagnostics.rs:89` | A constant (DC) nonzero signal is falsely flagged as clipped by `is_clipped`/`find_clipping` when no explicit threshold is given — `is_clipped([5,5,5,5])` returns `true` |
| 2 | **P0** | `lib.rs:33641` | `is_clipped`/`find_clipping`'s `full_scale=` doesn't share `adc`'s code geometry, so it can never detect real positive-rail ADC clipping (gate = `full_scale` itself, not `full_scale - lsb`); negative rail happens to still catch by coincidence. `detect_saturation` does NOT have this bug |
| 3 | P1 | `qu-core/diagnostics.rs:159` | `detect_saturation`'s rail-hit tolerance (`lsb * 1e-9`) requires near-bit-exact match with `adc()`'s own arithmetic — a real captured/processed signal that's visibly railed reports 0 saturated samples |
| 4 | P1 | `builtin_docs.rs:140` (approx) | `help("is_clipped")` doesn't explain the flat-run rationale — a single glitchy sample at the rail reasonably expected to read "clipped" silently reads `false` with no explanation at the point of use |
| 5 | P1 | `lib.rs:19427` | `detect_saturation`'s undocumented `bits=` alias for `adc_bits=` can trigger a false "unread keyword" error when both are passed together |
| 6 | P2 | `qu-core/diagnostics.rs:104` | `find_clipping`'s flat-run plateau scan duplicates `find_peaks`'s similar scan with different exactness semantics, unshared |
| 7 | P2 | `lib.rs:19347` | `is_clipped`/`find_clipping` duplicate setup boilerplate (the detection algorithm itself IS correctly shared) |
| 8 | P2 | `lib.rs:33640` | `clip_params` wastes a full-array scan computing an inferred threshold even when the caller supplied one explicitly |
| 9 | P2 | `lib.rs:19346` | `is_clipped` never early-exits its underlying scan even though it only needs a boolean |
| 10 | P2 | `qu-core/diagnostics.rs:212` | `verify()` (backing `verify_signal`) makes two full passes where one would do |

## Branch 5 — `claude/pulse-edge-metrics` (find_trigger, find_edges, rise_time, pulse_width, duty_cycle, overshoot, ...)

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `qu-core/transforms.rs:1149` | A NaN dropout between the true crossing and a hysteresis-confirmed sample makes the interpolated crossing position wrong by several samples — contradicts `find_edges`'s own documented promise that hysteresis "doesn't move the measurement"; propagates into `find_edges`/`find_pulses`/`pulse_period`/`duty_cycle`/`pulse_width` |
| 2 | **P0** | `builtin_docs.rs:260,594,864` | `help()` shows the WRONG function's signature for 3 builtins: `fall_time` shows `rise_time`'s docs ("rising"), `pulse_frequency` shows `pulse_period`'s, `undershoot` shows `overshoot`'s — literal copy-paste-under-wrong-name |
| 3 | P1 | `lib.rs:22112` | All ~12 new kwargs use the unchecked `style_num` instead of the existing `style_num_checked`, so a wrong-typed kwarg silently falls back to the default instead of erroring |
| 4 | P1 | `qu-core/transforms.rs:1300` | `duty_cycle` has no upper bound — can silently report >100% on an irregular pulse train (verified counterexample: 255%) |
| 5 | P1 | `lib.rs:22277` | `rise_time`/`fall_time` only reject `top==base`, never validate ordering — a natural "before/after" call for a falling step silently swaps thresholds and returns `None` with no diagnostic |
| 6 | P1 | `qu-core/transforms.rs:1197` | `find_edges`'s hysteresis path (positive `hysteresis=`) bypasses the shared `find_trigger` primitive entirely, contradicting the file's own "everything built on one primitive" banner comment |
| 7 | P1 | `qu-core/transforms.rs:1349` | `rise_time`'s own documented ringing-inflation bug has an unused fix sitting nearby in the same diff (`settled_levels`, used only by `overshoot`/`undershoot`) |
| 8 | P2 | `qu-core/transforms.rs:901` | New `crossing_position` interpolation duplicates pre-existing `peak_width`'s interpolation instead of unifying |
| 9 | P2 | `qu-core/transforms.rs:1300` | `pulse_width`/`duty_cycle`/`pulse_period` each independently re-scan; calling all three on one signal triggers 7 total O(n) scans |
| 10 | P2 | `qu-core/transforms.rs:1168` | The same 6-line min/max-over-finite loop is duplicated verbatim in 3 functions |

## Branch 6 — `claude/signal-indexing` (`s[0.5 s : 1.2 s]`, `X[440 Hz]`)

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `lib.rs:10164` | Index-**assignment** (`s[1 s] = value`) never got the unit-classification fix — silently discards the unit and writes to the wrong sample. Read path (`s[1 s]`) is correct; write path isn't, and no test exercises index-assignment at all |
| 2 | P1 | `lib.rs:10682` | Slice bounds are now evaluated unconditionally before validation (regression from the old short-circuit order) — a side-effecting later bound now fires even when an earlier bound is invalid |
| 3 | **P0** | `lib.rs:4440` | Unit classification only recognizes `s`/`Hz`; any other unit (`kg`, `V`, ...) silently falls through to plain sample-index interpretation — `s[5 kg]` silently becomes sample 5, the same bug class this branch claims to have closed, just not closed generally |
| 4 | P1 | `lib.rs:4505` | The mixed-unit slice error says "has no unit" when the bound actually has a unit, just an unrecognized one — misleads debugging |
| 5 | P1 | `lib.rs:12515` | The CVec-not-Spectrum design for band slices (meant to stop wrong answers) doesn't reach `ifft`/`fft(inverse)`, which accepts any CVec unconditionally — `ifft(X[20 Hz:200 Hz])` silently computes a wrong result |
| 6 | P1 | `lib.rs:10903` | Spectrum's frequency-range indexing silently clamps a negative frequency instead of erroring, inconsistent with Signal's time-range indexing which explicitly rejects negative time |
| 7 | P2 | `lib.rs:10871` | Spectrum's multi-index gather reimplements `numeric::selection::gather` instead of calling it (Signal's arm one line above does call it) |
| 8 | P2 | `lib.rs:10650` | Stale doc comment on `resolve_sel` references a function (`resolve_sel_inner`) that no longer exists |

## Branch 7 — `claude/rolling-stats-cleanup` (rolling_mean/rms/std, find_outliers, fill_missing)

| # | Sev | File:Line | Summary |
|---|---|---|---|
| 1 | **P0** | `qu-core/noise.rs:1009` | `hampel_flags` (now reachable via the much more discoverable `find_outliers(method="hampel")`) has no NaN/Inf filtering unlike its 3 siblings — a NaN corrupts the local median for every neighboring sample, not just itself |
| 2 | P1 | `lib.rs:20340` | `find_outliers`'s zscore method has a documented `(N-1)/sqrt(N)` ceiling with no runtime warning, despite the engine already having a `warn_once` mechanism built and used for exactly this class of problem elsewhere |
| 3 | P1 | `qu-core/noise.rs:1218` | `rolling_std`/`rolling_rms` give no reliability signal for their (smaller-N) shrunk edge windows, despite docs suggesting live-trigger use |
| 4 | P2 | `lib.rs:20296` | `rolling_mean(x, 5)` duplicates `smooth(x, 5, method="moving")` in substance (same stat, same default window, only edge handling differs), no cross-reference |
| 5 | P2 | `qu-core/noise.rs:1182` | `rolling_mean`/`rolling_rms`/`rolling_std` are O(n·window) with no shared O(n) sliding accumulator |
| 6 | P2 | `qu-core/noise.rs:1182` | The 5 `rolling_*` functions duplicate the same outer skeleton, only the reduction varies |
| 7 | P2 | `lib.rs:20382` | `replace_outliers`'s constant-fill path does 2 full allocations where 1 would do (writes NaN then immediately overwrites with the constant) |
| 8 | P2 | `lib.rs:20347` | Undocumented `method="local"` alias for `"hampel"` |
| 9 | P2 | `lib.rs:35119` | `row_mean`/`rolling_mean` naming proximity in `BUILTIN_NAMES` (cosmetic, unrelated functions) |

## Summary by severity

- **P0 (9)**: goertzel bracket-typo type-widening (B1#2), psd/welch overlap mismatch (B2#1), delay integer-overflow panic (B3#1), gain NaN propagation (B3#2), block_process mislabeling (B3#3), is_clipped DC false-positive (B4#1), is_clipped full_scale ADC miss (B4#2), pulse-edge NaN crossing-position error (B5#1), help() wrong-signature x3 (B5#2), index-assignment unit bug (B6#1), unit classification only s/Hz (B6#3), hampel NaN asymmetry (B7#1). *(12 items tagged P0 above; some judgement calls on the P0/P1 line — read the tables, not just this count.)*
- **P1 (~20)**: real defects, lower blast radius — misleading errors, missing warnings/guards, undocumented behavior changes.
- **P2 (~30)**: reuse/simplification/efficiency/documentation, no behavioral bug.

None of these block the merge that already happened — all 7 branches pass their own test suites, and these are follow-up fixes, not regressions introduced by merging. Flagging per Ahmed's/QuMaster's ask so they're durable and assignable rather than living only in a session transcript.
