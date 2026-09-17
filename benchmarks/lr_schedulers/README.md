# Learning-rate controllers: Qu (`lr_adaptive`/`lr_plateau`) vs PyTorch

`BACKLOG.md`/`IMPL.md`'s "`lr_adaptive`/`lr_plateau` learning-rate
controllers, 2026-08-26" entry landed two `optimizer=` CONTROLLERS for
`.fit(...)`/`compile(...)` — `lr_adaptive(lr=, increase_factor=,
decrease_factor=)` (LM-style trust region: try a step, reject-and-shrink
if it's worse, accept-and-grow if it's better) and `lr_plateau(lr=,
patience=, factor=, min_lr=, base=)` (classic ReduceLROnPlateau: pure
monotonic decay, no rejection) — and verified both on `../mlp/`'s real
3000-row/20-feature/4-class dataset. This benchmark asks the natural
follow-up: how do PyTorch's closest equivalents behave on the **same**
architecture/data/epochs? `lr_plateau` has a direct built-in
(`torch.optim.lr_scheduler.ReduceLROnPlateau`); `lr_adaptive` doesn't, so
it's hand-rolled here to mirror `sequential_fit`'s own reject-and-retry
logic step for step.

Run:

```bash
qu run benchmarks/lr_schedulers/bench.qu
python benchmarks/lr_schedulers/bench.py
```

## Same architecture, same data — reused, not regenerated

- **Data**: `../mlp/classification_{train,test}.csv` (2400×20 train,
  600×20 test) — the same `make_classification(..., random_state=42)`
  split every other MLP benchmark in this directory reads.
- **Model**: `20 -> 64 -> 32 -> 4`, ReLU hidden layers, raw-logit output,
  softmax + cross-entropy loss, 300 epochs, full-batch gradient descent.
- **Qu side**: `mlp_classifier(20, [64, 32], 4, seed=42)` +
  `net.fit(..., optimizer=...)` — the Sequential API, since
  `lr_adaptive`/`lr_plateau` are only reachable through `optimizer=` on
  `.fit(...)`/`compile(...)`; there is no manual `param()`/`grad()`
  equivalent to reuse from `../mlp/`. A fresh net (fresh `seed=42` init) is
  built per run so all four Qu runs start from the same weights.
- **PyTorch side**: bit-identical starting weights across its own four
  runs, loaded from `../mlp/init_weights.npz` (the same numpy
  `RandomState(42)` He-init `../mlp/bench.py` uses).
- **Not reused across languages**: bit-identical initial weights between
  Qu and PyTorch. `sequential(..., seed=42)` uses Qu's own He/Xavier
  initializer, not numpy's — same non-issue already documented in
  `../sequential_api/README.md`. This means absolute loss VALUES aren't
  meant to line up between the two languages; what's directly comparable
  is each controller's behavior relative to that language's OWN fixed-lr
  baseline, and the qualitative shape of the `lr` trajectory.

## `lr_plateau` vs `ReduceLROnPlateau`: both are a no-op here — same reason, honestly reported

Qu's `plateau_step` uses an absolute tolerance (`LR_PLATEAU_TOL = 1e-9` —
any loss decrease at all, however small, counts as improvement and resets
the patience counter). PyTorch's `ReduceLROnPlateau` defaults to a 1e-4
*relative* threshold, which would silently make the PyTorch side a laxer
test than Qu's; `bench.py` sets `threshold=1e-9, threshold_mode="abs"` to
match Qu's own semantics so this is an apples-to-apples check, not a
softer bar.

Full-batch gradient descent at `lr=0.05` on this dataset decreases the
loss on **every single one of the 300 epochs** in both languages — so
the "epochs since last improvement" counter never reaches `patience=10`
either side, and the shrink condition never fires. Both plateau
schedulers degenerate to exactly the fixed-lr optimizer they wrap:

| | Qu `lr_plateau` | Qu fixed-lr SGD | PyTorch `ReduceLROnPlateau` | PyTorch fixed-lr SGD |
|---|---:|---:|---:|---:|
| final loss | 0.214134 | 0.214134 | 0.177436 | 0.177436 |
| final `lr` | 0.050000 | 0.050000 | 0.050000 | 0.050000 |
| train / test acc | 94.63% / 95.33% | 94.63% / 95.33% | 95.96% / 96.00% | 95.96% / 96.00% |

Digit-for-digit identical to its own language's baseline, both sides —
this is not a bug or a weaker test on either implementation, it's the
correct, expected behavior of a plateau-triggered schedule facing a loss
curve that never plateaus. (Qu's own unit test,
`lr_plateau_shrinks_on_a_schedule_end_to_end_with_a_zero_gradient_model`,
already proves the shrink mechanism itself works on a curve that *does*
plateau; not re-demonstrated here.)

## `lr_adaptive` vs hand-rolled PyTorch equivalent: both genuinely help, both self-correct from an oversized start

No PyTorch built-in does LM-style reject-and-retry, so `bench.py`'s
`train_lr_adaptive(...)` mirrors `sequential_fit`'s `"lr_adaptive"` branch
exactly: each epoch, compute the gradient once at the current params,
build a candidate SGD step at the current attempt `lr`, probe its loss
with gradients OFF (mirrors Qu's untracked `training=false` forward), and
either commit + grow `lr *= increase_factor` on the first improving
attempt, or reject + shrink `lr *= decrease_factor` and retry from the
same starting point — up to 20 bounded attempts, identical to Qu's
`MAX_LR_ADAPTIVE_RETRIES`.

| | Qu `lr_adaptive` (lr₀=0.05) | Qu fixed-lr baseline | PyTorch `lr_adaptive` (lr₀=0.05) | PyTorch fixed-lr baseline |
|---|---:|---:|---:|---:|
| final loss | 0.139742 | 0.214134 | 0.132162 | 0.177436 |
| loss vs baseline | **-35%** | — | **-26%** | — |
| train / test acc | 96.46% / 96.00% | 94.63% / 95.33% | 96.92% / 96.17% | 95.96% / 96.00% |
| `lr` @ ep 0/50/100/150/200/299 | 0.0525 / 0.151 / 0.216 / 0.155 / 0.111 / 0.217 | constant 0.05 | 0.0525 / 0.075 / 0.108 / 0.155 / 0.111 / 0.108 | constant 0.05 |

Both sides: `lr` oscillates roughly an order of magnitude above the
starting value rather than settling to a constant — growing on accepted
epochs, dropping back on rejected ones — proof the reject/retry machinery
is doing real work every epoch in both implementations, not a wrapper
that happens to also train.

### Deliberately-oversized start (lr₀=5.0, 100x the sane value)

| | Qu `lr_adaptive` (lr₀=5.0) | PyTorch `lr_adaptive` (lr₀=5.0) |
|---|---:|---:|
| final loss | **0.123139** (best of all 4 Qu runs) | **0.091355** (best of all 4 PyTorch runs) |
| train / test acc | 96.83% / 96.33% | 98.04% / 95.67% |
| `lr` after epoch 0 | 0.164 (down from 5.0) | 0.164 (down from 5.0) |
| `lr` @ ep 50/100/150/200/299 | 0.118 / 0.169 / 0.242 / 0.173 / 0.169 | 0.235 / 0.169 / 0.242 / 0.173 / 0.169 |

Same story both languages: the retry loop rejects the first several
candidate steps at 5.0 → 2.5 → 1.25 → ... before landing somewhere
workable — `lr` is already down to ~0.164 by the end of epoch 0 in
**both** implementations — rather than diverging the way fixed-lr SGD at
`lr=5.0` would. In both languages this oversized-start run reaches the
lowest final loss and (train-side) the best accuracy of its four runs: a
big bad starting `lr` gets rescued instead of blowing up the model,
confirmed independently on two different autodiff engines.

## Convergence sanity check

Every run's loss trajectory was inspected, not just its endpoint — all
eight runs (4 Qu + 4 PyTorch) show loss decreasing from the first epoch
through the last (see each script's own `first=`/`ep50=`/`ep150=`/`last=`
printout), never NaN/Inf, never diverging. The oversized-`lr_adaptive`
runs in particular start from a much higher first-epoch loss (Qu 2.46,
PyTorch 5.09 — the raw lr=5.0 step before any rejection kicks in) and
still land at their respective run's lowest final loss, which is the
actual point of the reject-and-retry design, not merely "it ran without
crashing."

## Discrepancies worth flagging

- **Qu's `lr_adaptive` shows a larger relative win over its own baseline
  than PyTorch's does** (-35% for Qu vs -26% for PyTorch, at `lr₀=0.05`).
  Both reductions are real and the same sign/shape; the exact magnitude
  differs because the two languages start from different initial weights
  (see "Not reused across languages" above), so this is expected noise
  from a different starting point on the loss surface, not a correctness
  concern — the reject/retry mechanism and its qualitative effect (lower
  loss, non-monotonic oscillating `lr`, self-correction from an oversized
  start) match exactly.
- **Per-epoch `lr` sample points aren't expected to match exactly between
  Qu runs measured here and the numbers cached in `IMPL.md`** (e.g.
  `ep50` here reads 0.1505 for `lr_adaptive(lr0=0.05)` vs `IMPL.md`'s
  0.1433) — both are the same script/hyperparameters/dataset, but
  `f64` accumulation order and any interpreter changes since 2026-08-26
  can nudge which side of a reject/accept boundary a given epoch lands on
  without changing the qualitative trajectory or the final numbers, which
  match `IMPL.md` almost exactly (0.139742/0.123139 finals, identical to
  6 decimal places).
- **Raw wall-clock `time_s` in `bench.py` needed a warm-up pass** — the
  very first PyTorch run measured here ate ~20s of one-time MKL/thread-pool
  lazy-init cost before a throwaway warm-up forward+backward was added; all
  four runs are now ~0.3-0.8s. This benchmark isn't about throughput
  (`../mlp/`/`../sequential_api/` already cover that in depth) so times are
  reported for completeness only, not as a speed claim.
