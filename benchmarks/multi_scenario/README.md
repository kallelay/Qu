# Multi-scenario: Qu vs PyTorch vs Keras (tabular / image / sequence)

Three classification scenarios, each trained end to end in all three
languages/frameworks with matching architecture, optimizer, epochs, and
learning rate, so accuracy is genuinely comparable and only wall-clock and
ergonomics differ. Both PyTorch (2.12.0+cu130) and TensorFlow/Keras (2.21.0)
are installed on this machine — confirmed via `python -c "import torch"` /
`python -c "import tensorflow"` before starting, per the task's own
instruction — so every scenario has all three columns, no framework gaps.

Every Qu script was actually run via the release binary
(`engine/target/release/qu.exe run <script>`, rebuilt fresh from this
session's `cargo build --release` before any numbers were taken) — not just
read for plausibility. 2-3 runs per script per language; the tables below
report the **median** time (Qu's and Keras's own runs vary by a few percent
run to run; PyTorch's are close to identical) and note when accuracy varies
across runs.

## Summary table

| Scenario | Qu accuracy (train/test) | Qu time | PyTorch accuracy (train/test) | PyTorch time | Keras accuracy (train/test) | Keras time |
|---|---|---:|---|---:|---|---:|
| 1. Tabular/MLP (20→64→32→4, 2400/600, 300 ep) | 94.63% / 95.33% | **2.13 s** | 94.75% / 94.00% | **0.21 s** | 95.5-95.75% / 95.83-96.33% | **6.59 s** |
| 2. Image/CNN (16×16, 3 classes, 240/60, 250 ep) | 95.83% / 88.33% | **5.14 s** | 92.92% / 90.00% | **0.24 s** | 95.83-99.17% / 86.67-93.33% | **8.12 s** |
| 3. Sequence/RNN (GRU, len 20, 3 classes, 144/36, 200 ep) | 100% / 100% | **1.00 s** (was 13.97 s — see below) | 100% / 100% | **0.93 s** | 100% / 100% | **6.28 s** |

**Reading it honestly**: PyTorch is fastest on every scenario by a wide
margin (its autograd + BLAS backend is a mature, compiled system; Qu's
per-epoch interpreted loop — see below — pays real dispatch overhead).
Keras is slower than Qu on all three scenarios now (its own per-step
Python/graph-dispatch overhead dominates at this small scale, the same
finding `../sequential_api/README.md` made) — scenario 3 used to be the
exception (Qu's uniquely expensive per-sample-per-timestep GRU loop made it
the slowest of the three) until the batching fix described below closed
most of that gap. Accuracy is essentially tied across all three languages
on every scenario — this is a speed/ergonomics comparison, not a
correctness gap.

## Backend comparison: Qu-native vs Qu+TorchBackend vs Qu+TensorFlowBackend (added 2026-08-26)

Same three scenarios, run through Qu's own `engine="torch"` model-zoo
wiring (`simple_cnn`/`simple_rnn_classifier(..., engine="torch")` —
`scenario2_cnn_qu_torch.qu`/`scenario3_rnn_qu_torch.qu`, new this pass) —
routes the SAME `.qu` script through real libtorch autograd instead of
Qu's own tape. Requires a `qu.exe` built with `--features backend-torch`
(`LIBTORCH_USE_PYTORCH=1 LIBTORCH_BYPASS_VERSION_CHECK=1` on this box —
this pip `torch` install is 2.12.0 but `tch 0.26` expects 2.13.0, the
version check bypass is safe here since it's a minor-version gap, not a
missing install; the torch DLL directory also needs to be on `PATH` at
RUN time, not just build time, a Windows-specific gotcha — see IMPL.md's
matching dated entry). Every number below is from an actual run of the
release binary, not estimated.

| Scenario | Qu-native time | Qu+Torch(CPU) time | Qu+Torch(CUDA) | Qu+TensorFlow | Accuracy (all engines) |
|---|---:|---:|---|---|---|
| 1. Tabular/MLP | 2.13 s | not supported¹ | not supported¹ | not supported² | n/a |
| 2. Image/CNN | 6.12 s (fresh re-run; 5.14 s original) | **46.74 s** (7.6x slower) | not supported³ | not supported² | 95.83% / 88.33% (native and torch match exactly) |
| 3. Sequence/RNN | 0.89 s (fresh re-run; 1.00 s original) | **170.52 s** (192x slower)⁴ | not supported³ | not supported² | 100% / 100% (native and torch match exactly) |

¹ `mlp_classifier` (what Scenario 1 uses) always trains with `loss=
"cross_entropy"` and the `adam` optimizer internally, neither of which
`TorchBackend`'s current surface supports (`sequential_fit_torch` only
covers `loss="mse"` + plain `sgd`, see the original `backend-torch`
IMPL.md entry) — `mlp_classifier` itself doesn't even accept `engine=` as
a parameter. Not attempted rather than forced through a different,
non-equivalent architecture.

² `TensorFlowBackend` only implements the original `dense`/`relu`/`mse`/
plain-SGD slice for real — every method this pass's CNN/GRU work needed
(`conv2d`, `gru_cell`, `softmax_cross_entropy`, ...) is a stub that
errors clearly (`tf_unimplemented`) rather than silently running wrong
math. `simple_cnn`/`simple_rnn_classifier` don't accept `engine=
"tensorflow"` at all (only `"native"|"torch"`).

³ CUDA is unreachable from this build regardless of scenario — see
IMPL.md's dated entry for the full investigation (`tch::Cuda::
is_available()` returns `false` on this box despite Python's `torch`
confirming a working RTX 5080). Re-checked fresh this pass, unchanged.

⁴ **Not an apples-to-apples engine comparison** — `simple_rnn_fit`'s
NATIVE path has a batched-GRU fast path (one `gru_forward` call per epoch
over all 144 samples at once, see the section below); `simple_rnn_fit_
torch` (new this pass) always trains ONE SAMPLE AT A TIME, matching
`train_gru_classifier`'s own per-sample loop — no batched-GRU path exists
on the torch side yet. So this 192x gap is dominated by "unbatched vs
batched" (the exact 144x-ish factor the batching fix below closed for
native), not by "torch is inherently much slower than native at GRU
training" — the CNN row (both sides unbatched, per-image) is the fairer
per-op comparison, and shows the same DIRECTION but a far smaller gap
(7.6x), consistent with IMPL.md's original conv2d/GRU/attention finding
(native wins 2.2x-7.1x at comparable unbatched scales). Loss trajectories
match to 5-6 significant figures between native and torch on both
scenarios (see the Rust-level parity tests in IMPL.md's entry) — the gap
here is pure wall-clock, not a correctness difference.

## Architecture matching — what's identical, what honestly isn't

**Scenario 1 (tabular)**: `20 -> 64 -> 32 -> 4`, ReLU hidden layers, raw-logit
output, softmax/categorical cross-entropy loss, full-batch (`batch_size=n`),
plain SGD (no momentum/Adam), 300 epochs, `lr=0.05` — identical on all three
sides. Qu: `mlp_classifier(20, [64, 32], 4, seed=42)` + `net.fit(...,
loss="cross_entropy")` (same one-liner API `../sequential_api/` already
benchmarks against Keras — reused here, now with a PyTorch column added via
`nn.Sequential` + `nn.CrossEntropyLoss` + `optim.SGD`, the high-level API the
task asked for, **not** `../mlp/bench.py`'s manual `param()`/`backward()`
loop). Reuses `../mlp/`'s already-committed
`classification_{train,test}.csv` directly — no new dataset. Initial weights
are **not** byte-identical across languages (each framework's own random
init) — `../mlp/README.md` already did that byte-identical-weights rigor for
the *manual*-loop comparison; this one is about the high-level training APIs
and reuses the same data, not that specific proof again.

**Scenario 2 (CNN)**: one 3x3 "valid" conv (single in/out channel, i.e. one
learned kernel — Qu's `simple_cnn` only ever has one) + bias → ReLU → 2x2
non-overlapping max-pool → flatten → `Linear`/`Dense(3)` readout — identical
on all three sides (`Conv2d(1,1,3)`/`Conv2D(1,3)` in PyTorch/Keras exactly
mirror Qu's single-kernel `simple_cnn([16,16], 3, seed=)`). Full-batch, plain
SGD, 250 epochs, `lr=0.3`, identical on all three. Data: a new synthetic
16x16, 3-class (square / plus / diagonal bar) task,
`scenario2_cnn_make_dataset.py`, generated once and shared byte-for-byte —
each framework loads the exact same pixels (Qu via a stacked headerless CSV,
PyTorch/Keras via the companion `.npz`, both written from the same numpy
array in the same script run). **Honest caveat**: weight initialization is
each framework's own default, not shared — same situation as scenario 1, and
for the same reason (this task is about the trained *result*, not
byte-identical gradients from a common start).

**Scenario 3 (RNN)**: single-layer GRU (`hidden_size=8`, `input_size=1`) +
a `Linear`/`Dense(3)` readout on the GRU's **last** hidden state — identical
on all three (Qu's `simple_rnn_classifier` reads `gru_forward`'s final
hidden state the same way `nn.GRU`'s returned `h_n` and Keras'
`layers.GRU(...)` default `return_sequences=False` output both do). Data: a
3-class synthetic sequence task (rising ramp / falling ramp / oscillating
sine, length 20, phase/amplitude jitter + noise) extending
`simple_rnn_classifier`'s own 2-class rising-vs-falling acceptance test to 3
classes and 180 total samples. Full-batch, plain SGD, 200 epochs, `lr=0.3`,
identical on all three. **One real, unavoidable asymmetry, flagged rather
than hidden**: Keras' `GRU` layer defaults to `reset_after=True` (the
"CuDNN-compatible" gate formula, an extra bias term split differently from
the classic 1406.1078v3 GRU equations `gru_cell_compute`/PyTorch's `nn.GRU`
both implement) — mathematically a slightly different parameterization of
the same gating idea, not a bug, and not reasonably avoidable without
patching Keras' own layer internals. All three still converge to 100%/100%
accuracy at 200 epochs, so this asymmetry doesn't visibly affect the
comparison here, but it means the three GRUs are not bit-for-bit the same
recurrence, only architecturally equivalent (same hidden size, same "readout
on last state" contract).

## Why WAS Qu's RNN scenario so much slower (relatively) than its CNN/MLP scenarios? (fixed 2026-08-26)

`simple_cnn_fit`/`simple_rnn_fit` (`engine/crates/qu-interp/src/lib.rs`) used
to both train with a **per-sample** Rust loop (no batched tensor op across
samples), unlike `sequential_fit`'s batched `(n,features)` matrix ops. The
RNN case was strictly worse than the CNN case: each of its per-sample
forward passes was *itself* a `gru_forward` loop over every one of the T=20
timesteps (a `gru_cell` autodiff-tracked op per step), so one epoch did
`n_samples x T` tracked cell evaluations versus the CNN's `n_samples x 1`
conv+pool evaluations — at 144 samples x 20 steps x 200 epochs, that's
576,000 tracked GRU-cell forward calls (plus their backward pass) in this
one scenario, dwarfing scenario 2's 240 x 250 = 60,000 conv calls. This was
the same `call_builtin`-dispatch-per-tracked-op overhead `../mlp/README.md`
and `../sequential_api/README.md` already found — it just compounded
multiplicatively here because of the extra timestep loop, which neither of
those two earlier scenarios has.

**The fix**: `gru_forward`/`gru_cell_compute` were already documented to
accept a BATCHED `(input_size, batch)` matrix per timestep (one column per
sample) instead of a single `(input_size, 1)` column — the batch dimension
rides on `Matrix::matmul`'s/`Matrix::broadcast`'s own existing rayon
parallelism — but `simple_rnn_fit`'s training loop never actually used that
shape; it always called `gru_forward` once per sample. `simple_rnn_fit` now
stacks every sample into one `(input_size, batch)` matrix per timestep
(`build_batched_gru_sequence`, falling back to the original per-sample loop
only when sequences have different lengths and can't be stacked) and runs
ONE `gru_forward` + `dense` + `cross_entropy` call per epoch over the whole
144-sample batch, instead of 144 separate calls. That turns 576,000 tracked
per-timestep `gru_cell_compute` calls (144 samples x 20 steps x 200 epochs)
into 4,000 (20 steps x 200 epochs) — a 144x reduction in interpreter/tape
dispatch count. **Measured**: `net.fit()` wall-clock on this exact scenario
dropped from 13.97 s to a median of **1.00 s** across 4 release-binary runs
(0.986-1.024 s) — a ~14x speedup — with identical accuracy (100%/100%
train/test, unchanged) and the same convergence shape (loss still drops
from ~1.0 to ~0.02 over 200 epochs). At this scenario's small
`hidden_size=8`, the win is entirely from cutting dispatch count, not from
newly crossing `Matrix::matmul`'s `PARALLEL_MATMUL_THRESHOLD` (even
batched, `hidden_size(8) x batch(144) x input_size(1) = 1152` stays far
below the `1<<16` threshold) — a larger `hidden_size`/batch combination
would additionally pick up rayon matmul parallelism for free once `m*n*k`
clears that threshold. See `build_batched_gru_sequence`'s and
`simple_rnn_fit`'s own doc comments in `engine/crates/qu-interp/src/lib.rs`
for the implementation, and its unit tests (`batched_gru_forward_matches_
per_sample_forward` and friends) for the correctness proof that batching is
a pure reshape of the same computation, not an approximation.

## Files

- `scenario1_tabular_{qu.qu,pytorch.py,keras.py}` — reuses `../mlp/`'s
  committed CSVs directly, no new data files here.
- `scenario2_cnn_make_dataset.py` — generates `cnn_{train,test}_images.
  {csv,npz}` + `cnn_{train,test}_labels.csv` (committed, small: a few
  hundred KB). `scenario2_cnn_{qu.qu,pytorch.py,keras.py}` — the three
  training scripts.
- `scenario3_rnn_make_dataset.py` — generates `rnn_{train,test}_sequences.
  {csv,npz}` + `rnn_{train,test}_labels.csv` (committed). `scenario3_rnn_
  {qu.qu,pytorch.py,keras.py}` — the three training scripts.
- `scenario2_cnn_qu_torch.qu`/`scenario3_rnn_qu_torch.qu` (2026-08-26) —
  IDENTICAL to `scenario2_cnn_qu.qu`/`scenario3_rnn_qu.qu` except
  `engine="torch"` on the model constructor; same data files, no new ones.
  Needs a `qu.exe` built with `--features backend-torch` — see the
  "Backend comparison" section above for the exact env vars this machine
  needs and why Scenario 1 has no `_torch` variant.

Run everything (from the repo root):

```bash
python benchmarks/multi_scenario/scenario2_cnn_make_dataset.py   # once
python benchmarks/multi_scenario/scenario3_rnn_make_dataset.py   # once

engine/target/release/qu.exe run benchmarks/multi_scenario/scenario1_tabular_qu.qu
python benchmarks/multi_scenario/scenario1_tabular_pytorch.py
python benchmarks/multi_scenario/scenario1_tabular_keras.py

engine/target/release/qu.exe run benchmarks/multi_scenario/scenario2_cnn_qu.qu
python benchmarks/multi_scenario/scenario2_cnn_pytorch.py
python benchmarks/multi_scenario/scenario2_cnn_keras.py

engine/target/release/qu.exe run benchmarks/multi_scenario/scenario3_rnn_qu.qu
python benchmarks/multi_scenario/scenario3_rnn_pytorch.py
python benchmarks/multi_scenario/scenario3_rnn_keras.py

# Backend comparison (needs a --features backend-torch build, see above):
engine/target/release/qu.exe run benchmarks/multi_scenario/scenario2_cnn_qu_torch.qu
engine/target/release/qu.exe run benchmarks/multi_scenario/scenario3_rnn_qu_torch.qu
```
