# ML stress test: k-NN (exact-match check) + Sequential-API MLP (Adam) vs scikit-learn/PyTorch

Goes well beyond `catalog/qu_sequential_xor.qu` (4-sample toy) and
`catalog/qu_classification_report.qu` (120-point 2-class toy): a real
2000-sample, 12-feature, 4-class classification task
(`sklearn.datasets.make_classification`), stratified 80/20 split (1600
train / 400 test), exercised through two DIFFERENT Qu ML code paths not
covered by the existing `../../shallow_ml/` or `../../mlp/` benchmarks:

1. **`knn_model`** — deterministic (no RNG anywhere in k-NN), so this is a
   real exact-match check against `sklearn.neighbors.KNeighborsClassifier`
   on the identical split, not just a "close enough" comparison.
2. **The Sequential API's Adam optimizer** (`sequential`/`dense_layer`/
   `net.fit(..., optimizer=adam(lr=0.001))`) — `../../mlp/` already
   stress-tested the older manual `param()`/`grad()`/`stop_grad()` loop
   with bit-identical PyTorch init; `../../sequential_api/` stress-tested
   the Sequential API but with plain SGD. Neither exercises Sequential +
   Adam + `cross_entropy` together at this scale, so this fills that gap.

Run:

```bash
python benchmarks/advanced_validation/ml_stress/make_dataset.py   # once, regenerate the shared CSVs
qu run benchmarks/advanced_validation/ml_stress/ml_stress.qu       # also dumps knn_qu_predictions.csv
python benchmarks/advanced_validation/ml_stress/ml_stress.py       # diffs against it
```

## Dataset

`make_dataset.py`: `make_classification(n_samples=2000, n_features=12,
n_informative=9, n_redundant=2, n_classes=4, n_clusters_per_class=1,
class_sep=1.3, flip_y=0.02, random_state=42)`, stratified 80/20 split →
`classification_train.csv` (1600×12), `classification_test.csv` (400×12).
Both `ml_stress.qu` and `ml_stress.py` read these same CSVs — no
per-language regeneration.

## Sub-case 1: k-NN — exact-match result

`knn_model(Xtr, ytr, 7, kind="classification")` vs
`KNeighborsClassifier(n_neighbors=7, metric="euclidean")`, same split, no
random component on either side.

| | Qu | scikit-learn |
|---|---:|---:|
| test accuracy | 0.9450 | 0.9450 |
| elementwise prediction diff | **0 / 400 mismatches** | — |

`ml_stress.qu` dumps its own 400 test-set predictions to
`knn_qu_predictions.csv`; `ml_stress.py` loads that file and diffs it
element-by-element against sklearn's own predictions (not just comparing
accuracy numbers, which could hide compensating errors) — **zero
mismatches out of 400**. This is the strongest possible correctness signal
for this sub-case: Qu's k-NN implementation (Euclidean distance, majority
vote) is behaviorally identical to sklearn's on real data at this scale.

Timing (fit + predict, single-trial, shared machine — not statistically
rigorous, same caveat as the rest of this benchmark suite):

| | Qu | scikit-learn |
|---|---:|---:|
| fit | 0.06 ms | 2.26 ms |
| predict (400 queries) | 16.9 ms | 6.1 ms |

Qu's `knn_model(...)` call is near-instant (`k`-NN has no real "fit" step,
just storing the data — sklearn's higher fit cost here is likely
`KDTree`/`BallTree` index construction overhead that Qu's brute-force
implementation skips). Predict is the opposite: Qu takes ~2.8x longer for
400 brute-force queries against 1600 training points, consistent with
sklearn defaulting to a tree-based nearest-neighbor search (sub-linear per
query) against Qu's straightforward brute-force distance scan (linear per
query) — expected, not a red flag, at this small a scale.

## Sub-case 2: Sequential MLP with Adam — converges, close but not identical

Architecture `12→32→16→4`, ReLU hidden layers, raw-logit output layer,
`loss="cross_entropy"` (one-hot targets, `-mean(row_sum(y .* ln(softmax_rows(logits)
+ 1e-9)))` — see `sequential_loss`'s own doc comment in
`engine/crates/qu-interp/src/lib.rs`), `adam(lr=0.001, beta1=0.9,
beta2=0.999, eps=1e-8)` — **Qu's own Adam defaults match PyTorch's
`torch.optim.Adam` defaults exactly**, so both sides run the literal same
optimizer hyperparameters. 150 full-batch epochs on both sides.

**Deliberately NOT bit-identical initial weights** — unlike `../../mlp/`,
which pastes numpy-generated weights verbatim into both scripts to get a
lockstep comparison, this test lets each side use its own random init (Qu
`seed=42`'s He/Xavier init; PyTorch `torch.manual_seed(0)`'s default
`nn.Linear` init) and asks a different, still-valid question: starting
from its own random point, does Qu's Adam-based Sequential `.fit` converge
to comparable quality as PyTorch's Adam, on the same data/architecture/
epochs? Building the `../../mlp/`-style precomputed-weights bridge here
would only re-test what that scenario already covers.

| | Qu | PyTorch |
|---|---:|---:|
| first-epoch loss | 2.173001 | 1.354488 |
| last-epoch loss (150 ep) | 0.271828 | 0.237339 |
| train accuracy | 0.9194 | 0.9419 |
| test accuracy | 0.8850 | 0.9025 |
| train time (150 epochs) | 0.355 s | 0.173 s |
| per-epoch | 2.37 ms | 1.15 ms |

Both converge to a real, working classifier (loss drops ~8x on Qu, ~5.7x
on PyTorch; nothing plateaus or diverges) and land within **1.75 points of
test accuracy** of each other (88.50% vs 90.25%) — different starting
losses are expected and not a bug (different random init, exactly as
documented above), and the converged quality is genuinely comparable, not
"both ran without crashing." Qu is **~2.1x slower per epoch** here — a much
narrower gap than `../../mlp/`'s manual-autodiff-loop comparison found
(~5-7x, tracked as a real interpreter/autodiff-overhead gap in
`IMPL.md`/`BACKLOG.md`) — consistent with `../../sequential_api/README.md`'s
own finding that moving the epoch loop into one native `sequential_fit`
Rust call (rather than an interpreted per-epoch Qu loop) closes a real
chunk of that gap, though it doesn't eliminate it.

## Honest verdict

**k-NN: as good as it gets** — exact elementwise match with sklearn, not
just similar accuracy. **Sequential+Adam MLP: converges correctly and
lands close to PyTorch on accuracy** (within ~2 points, both real
converged models, not toy runs), trailing on speed by ~2x per epoch — a
real but modest gap, not a correctness concern. No bugs found in either
code path during this pass.
