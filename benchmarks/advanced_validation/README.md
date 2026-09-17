# Advanced-language validation: is Qu "good enough"?

A validation exercise, not a feature-building one: seven genuine stress
tests of Qu's more advanced language surface, each checked against Python
for BOTH speed AND numerical accuracy — a fast wrong answer is a failure
here, not a win. Every existing `benchmarks/*/` scenario before this one
either measures raw kernel speed (`matty_suite/`, `linalg/`,
`filter_design/`, ...) or ML training speed/accuracy on one architecture at
a time (`mlp/`, `shallow_ml/`, `sequential_api/`, `multi_scenario/`); this
directory specifically goes looking for correctness problems in realistic,
multi-case scripts (not single happy-path calls) across seven different
domains:

| Scenario | Domain | vs. | Verdict |
|---|---|---|---|
| [`peak_finding/`](peak_finding/) | Signal processing — `findpeaks` at multiple SNR levels, a distance-suppression boundary case, an ambiguous near-tie, a flat-topped plateau | `scipy.signal.find_peaks` | Exact match on all 6 cases (plateau case only after a **same-day fix**). Speed gap also **fixed** (see below) — now at or faster than scipy on every noisy case |
| [`ml_stress/`](ml_stress/) | Classical + neural ML — `knn_model` and Sequential-API `.fit(optimizer=adam(...))` on a real 2000-sample/4-class dataset | `sklearn.neighbors.KNeighborsClassifier`, PyTorch `nn.Sequential` + `Adam` | k-NN: **zero mismatches across all 400** held-out predictions. MLP: converges, within ~2 points of PyTorch's test accuracy, ~2.1x slower/epoch |
| [`curve_kalman/`](curve_kalman/) | Nonlinear optimization + estimation theory — `curve_fit` (Levenberg-Marquardt, good/poor init, a harder 5-param model) and a 5,000-step 2D Kalman tracker | `scipy.optimize.curve_fit(method="lm")`, an independent from-scratch numpy Kalman filter | `curve_fit` matches scipy to **6 decimal places**, including reproducing the same two non-obvious alternate optima; Kalman matches an independent implementation to **5e-7**. Speed gap also **fixed** (see below) — now at parity with scipy |
| [`linalg_stress/`](linalg_stress/) | Linear algebra — `qr`/`svd` reconstruction and `\` solve at n=300/1000 | `numpy.linalg` | Accuracy clean (residuals 1e-13 to 1e-11, solve match ~5e-7). Found a real architectural gap: `\` always computes a full SVD pseudoinverse even for a plain square well-conditioned system, a **50-130x** speed gap — root-caused, fix queued |
| [`filter_stress/`](filter_stress/) | DSP — `butter`+`filtfilt`+`freqz` band-stop filtering | `scipy.signal` | Elementwise match to 5e-7. **Qu is 4-6x faster** here (vectorized Rust kernels, no interpreter overhead) — caught and avoided a scipy test-methodology trap along the way (see that dir's README) |
| [`table_stress/`](table_stress/) | Table/dataframe ops — `group_by_agg`/`corrcoef`/filter+sort on a 30,000-row table | `pandas`/`numpy` | Zero mismatches beyond CSV rounding. `group_by_agg` at parity; `corrcoef` ~8-13x slower, filter+sort ~3-4x slower — real gaps, no isolated silly root cause (BLAS-backed pandas/numpy vs. a young table engine) |
| [`image_stress/`](image_stress/) | Image processing — Otsu threshold + blob detection on a 9-shape synthetic image with 15 speckle outliers | `opencv` (`cv2`) | Blob count, centroids (0px error), area/bbox bit-identical to cv2 across 3 runs. Otsu threshold differs by a benign bin-edge convention only (zero effect on segmentation). Qu ~1.3-1.6x slower — real but modest |

Read each subdirectory's own `README.md` for full methodology, exact
commands, and the complete numbers — this page is the index, not a
substitute for them.

## The one finding worth leading with — found and fixed same-day

This pass found that **`findpeaks` silently dropped flat-topped (plateau)
peaks that both scipy's `find_peaks` and MATLAB's own `findpeaks` detect**
— a real, reproducible, root-caused behavioral gap, not a flaky one-off.
`find_peaks`'s peak candidate test in `qu-core/src/transforms.rs` was a
strict `x[i] > x[i-1] && x[i] > x[i+1]` local maximum, which never accepts
either sample of an exact-tie plateau — meaning any signal with a
hard-clipped/quantized/digitized plateau would silently lose that peak.

**Fixed the same day this was found**: the candidate scan now detects a
maximal run of equal values flanked by strictly smaller neighbors and
reports its midpoint (rounded down on an even-width plateau), matching
scipy's own convention exactly — re-verified against the real benchmark
data (`n_peaks=1, location=93`, exact match with scipy). 4 new unit tests
cover odd/even-width plateaus, an ascending run into a taller plateau, and
a plateau touching the signal's edge. See
[`peak_finding/README.md`](peak_finding/README.md#the-one-real-discrepancy-this-pass-found--fixed-not-just-reported)
for the full before/after.

No other correctness bugs were found across any of the seven domains —
every other discrepancy investigated (the two "wrong-looking" `curve_fit`
optima, the k-NN/Kalman/MLP-accuracy numbers, the Otsu threshold value)
turned out to be either an exact match or a fully explained, expected
difference (different random init, a model's inherent parameter symmetry,
a benign histogram-edge convention), not an unresolved mismatch.

## The speed gaps: two fixed, three documented

Two root-caused algorithmic-complexity gaps got the same-day fix treatment
as the plateau bug above, both with before/after numbers and
correctness-preserving regression tests:

- **`findpeaks`'s distance suppression** was `O(candidates × accepted)`;
  rewritten as an `O(n log n)` sort + linked-list sweep. Now at or faster
  than scipy on every noisy-tone case (was up to ~37x slower). See
  `peak_finding/README.md`'s "Results — speed" section.
- **`curve_fit`'s per-point interpreted calls** were replaced with one
  vectorized call per Jacobian column wherever the model function is
  elementwise-safe (with an automatic fallback to the old per-point path
  otherwise, so correctness never regresses). Closed a 29-147x gap to
  parity with scipy. See `curve_kalman/README.md`.

Three more gaps from the second round are real but left as documented,
unfixed backlog rather than forced through in this pass:

- **`\` (solve)** always routes through a full SVD pseudoinverse even for
  a plain square well-conditioned system, where Qu's own `lu()` would give
  a direct solve — a 50-130x gap with an obvious, well-scoped fix (a
  square-`A` fast path in `least_squares`) queued for a follow-up.
- **`corrcoef`** (~8-13x slower) and **table filter+sort** (~3-4x slower)
  vs. pandas/numpy have no isolated silly root cause the way the `\` gap
  does — closing them means competing with decades-tuned BLAS/C routines,
  a bigger and less clearly-scoped undertaking.
- **Image processing** (Otsu+blobs, ~1.3-1.6x slower than cv2) is a real
  but modest gap, lowest priority of the three.

## Regenerating everything

Each subdirectory's data is committed (small CSVs, same convention as
`../shallow_ml/`/`../mlp/`) but can be regenerated deterministically:

```bash
python benchmarks/advanced_validation/peak_finding/make_signals.py
python benchmarks/advanced_validation/ml_stress/make_dataset.py
python benchmarks/advanced_validation/curve_kalman/make_data.py
python benchmarks/advanced_validation/linalg_stress/make_data.py
python benchmarks/advanced_validation/filter_stress/make_data.py
python benchmarks/advanced_validation/table_stress/make_data.py
python benchmarks/advanced_validation/image_stress/generate_image.py

qu run benchmarks/advanced_validation/peak_finding/peak_finding_stress.qu
qu run benchmarks/advanced_validation/ml_stress/ml_stress.qu
qu run benchmarks/advanced_validation/curve_kalman/curve_kalman_stress.qu
qu run benchmarks/advanced_validation/linalg_stress/linalg_stress.qu
qu run benchmarks/advanced_validation/filter_stress/filter_stress.qu
qu run benchmarks/advanced_validation/table_stress/table_stress.qu
qu run benchmarks/advanced_validation/image_stress/image_stress.qu

python benchmarks/advanced_validation/peak_finding/peak_finding_stress.py
python benchmarks/advanced_validation/ml_stress/ml_stress.py
python benchmarks/advanced_validation/curve_kalman/curve_kalman_stress.py
python benchmarks/advanced_validation/linalg_stress/linalg_stress.py
python benchmarks/advanced_validation/filter_stress/filter_stress.py
python benchmarks/advanced_validation/table_stress/table_stress.py
python benchmarks/advanced_validation/image_stress/image_stress.py
```

(Run the `.qu` script before the matching `.py` script — each dumps its
own results/predictions/estimates to CSV for the Python side to diff
elementwise, not just compare summary numbers.)

All numbers in every README under this directory come from an actual run
performed while building this exercise (`cargo build --release -p qu-cli`
from `engine/`, system Python with numpy/scipy/pandas/scikit-learn/
PyTorch all confirmed installed) — none are estimated. Single-trial timing
numbers, same "not statistically rigorous, good enough to see the shape"
caveat as every other scenario in `../README.md`.
