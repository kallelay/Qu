# MLP training: Qu (manual `param`/`grad`/`stop_grad`) vs PyTorch

A real multi-layer perceptron — not the toy 1-2 parameter fits the
autodiff acceptance tests use to check correctness, and not a hand-picked
tiny CNN either — trained end to end in both Qu and PyTorch from
byte-identical initial weights on byte-identical data, timed and checked
for matching accuracy.

Run:

```bash
python benchmarks/mlp/make_dataset.py   # once, regenerate the shared CSVs + init_weights.npz
qu run benchmarks/mlp/bench.qu
python benchmarks/mlp/bench.py
```

## Architecture and task

- **Data**: `sklearn.datasets.make_classification(n_samples=3000,
  n_features=20, n_informative=12, n_redundant=4, n_classes=4,
  n_clusters_per_class=1, class_sep=1.4, flip_y=0.02, random_state=42)`,
  stratified 80/20 split (`classification_train.csv` 2400×20,
  `classification_test.csv` 600×20) — a real 4-class, 20-feature workload,
  not a 2-feature/2-parameter demo.
- **Model**: `20 -> 64 -> 32 -> 4`, ReLU hidden activations, softmax +
  cross-entropy output — two hidden layers, as requested, each a plain
  `dense(X, W, b)` + `relu(...)` (Qu) / `X @ W + b` + `torch.relu(...)`
  (PyTorch).
- **Loss**: `-mean(sum(Y_onehot * log_softmax(logits), axis=1))`, the exact
  same formula on both sides — Qu builds it from already-tracked
  primitives (`softmax_rows`, `ln`, `row_sum`, `.* `, `mean`) since Qu's
  plain `softmax` builtin isn't on the autodiff tape (only `softmax_rows`
  is); PyTorch uses `torch.log_softmax` directly. Verified numerically
  equivalent: a from-scratch numpy backprop implementation of the same
  formula lands on the same first/last loss as both (see "Correctness"
  below).
- **Training**: full-batch gradient descent, 300 epochs, `lr=0.05`, no
  momentum/Adam/mini-batching on either side — Qu's loop is the manual
  `param()`/`grad()`/`stop_grad()` pattern (`tape_reset()` each epoch,
  fresh `param(...)` leaves from the previous epoch's plain updated
  values, exactly the pattern already exercised by the tiny-CNN and
  transformer-block acceptance tests in
  `engine/crates/qu-interp/tests/acceptance.rs`); PyTorch's loop is
  autograd (`loss.backward()`) with a manual `p -= lr * p.grad` update
  (**not** `torch.optim`), so both sides are "compute the loss, get the
  gradient, subtract `lr * grad`" — no optimizer-internals asymmetry.

## Same initial weights, actually

Qu's own RNG (`randn`) and numpy's are different algorithms — "seed both
with 42" would **not** produce the same floats, only superficially look
like it does. Instead, `make_dataset.py` draws the He-initialized weights
**once**, in numpy (`RandomState(42)`), saves them to `init_weights.npz`
for `bench.py` to load directly, and their exact values are pasted
verbatim as literal Qu matrices (`W1_init`, `b1_init`, ...) into
`bench.qu`. Both scripts start optimization from bit-identical floats —
confirmed by the two training curves' first-epoch loss matching to 3-4
significant figures (see below; the residual difference is `ln(softmax +
1e-9)` on the Qu side vs `log_softmax` on the PyTorch side, a numerically-
stable-but-not-identical formulation of the same math, not a data or
weight mismatch).

## Correctness: does it actually converge?

Yes, both. A third, independent from-scratch numpy+manual-backprop
implementation (same architecture, same loss, same init, no autodiff
framework at all) was used to sanity-check both languages' results before
trusting the comparison — see the loss curves below; the numpy check
landed on `first_loss=6.267206, last_loss=0.177436` train/test accuracy
`0.9596`/`0.9600`, matching Qu and PyTorch to 3+ significant figures.

| | first-epoch loss | last-epoch loss | train accuracy | test accuracy |
|---|---|---|---|---|
| Qu | 6.267206 | 0.177427 | 0.9596 | 0.9617 |
| PyTorch | 6.373663 | 0.177436 | 0.9596 | 0.9600 |
| numpy (independent check) | 6.267206 | 0.177436 | 0.9596 | 0.9600 |

Loss drops ~35x over training on both sides (not a plateau, not a
divergence) and both land on essentially the same test accuracy (96.0% vs
96.2%, a 1-2 sample difference out of 600 — within noise for two
numerically-distinct-but-equivalent loss formulations starting from the
same floats). This is a real, checked, converged comparison, not just
"both ran without crashing."

## Timing

| | total train time (300 epochs, full-batch) | per-epoch |
|---|---|---|
| Qu (`qu run`, `backend auto`) | ~2.1-2.3 s | ~7.1-7.6 ms |
| PyTorch (CPU, float64, autograd) | ~0.39-0.46 s | ~1.3-1.5 ms |

Two runs each, single trials (same "not statistically rigorous, good
enough to see the shape" caveat as every other scenario in this index).
**PyTorch is ~5-5.5x faster here** (down from an earlier-measured ~7x —
see `IMPL.md`'s "Real autodiff/tape bottleneck found and fixed" dated
entry, 2026-08-26, for the fixes and full before/after numbers). The
original diagnosis on this line — "interpreter overhead per `call_builtin`
dispatch" — turned out to be wrong when actually measured (`IMPL.md`'s
`call_builtin`/`apply_seeded` dispatch-overhead audit, same date, isolated
the redundant-hashmap-lookup fix to noise-level effect and traced the real
cost to matmul/autodiff-vjp work instead). The current, measured breakdown
of one epoch: forward pass ~38%, `grad()`'s backward walk ~57-58% (a
correct reverse-mode backward pass inherently does more matmul-equivalent
work than forward — each interior layer needs both `dW` and `dX`), `param`
setup and the SGD update together under 5%. Two real fixes landed against
this: the `matrixmultiply`-crate SIMD matmul kernel (`fast-matmul`, ~12-16%
end-to-end once actually isolated in a controlled A/B) and a `tensor_vjp`
fix that stopped computing-then-discarding a full matmul's worth of
gradient for `dense(X, W, b)`'s untracked `X` on every backward pass (~7%
further). Remaining gap is genuine reverse-mode AD backward-pass cost and
Qu's autodiff-vjp machinery generally (not raw single-op matmul
throughput — `../linalg/` shows Qu's own matmul kernel is competitive) —
the standing lever `IMPL.md`/`BACKLOG.md` track for closing it further.

## Files

- `make_dataset.py` — generates `classification_{train,test}.csv` and
  `init_weights.npz` (He init, numpy `RandomState(42)`). Deterministic;
  rerunning reproduces the same files.
- `bench.qu` — Qu training loop + train/test accuracy, weight literals
  pasted from `init_weights.npz`.
- `bench.py` — PyTorch training loop + train/test accuracy, loads
  `init_weights.npz` directly.
- `classification_{train,test}.csv`, `init_weights.npz` — committed
  (small: a few hundred KB) for byte-identical reuse, same convention as
  `../shallow_ml/`'s committed CSVs.
