# Sequential API: Qu (`mlp_classifier`/`sequential` + `.fit()`) vs Keras (`Sequential` + `.compile()` + `.fit()`)

`../mlp/README.md` benchmarks a **manual** `param()`/`grad()`/`stop_grad()`
training loop, written by hand in both languages, and found PyTorch ~7x
faster — real per-`call_builtin` interpreter-dispatch overhead rebuilding
Qu's autodiff tape from scratch every epoch, not matmul throughput. This is
a **different** question: how do the two languages' **high-level, one-liner**
training APIs compare — Qu's new Sequential API (`sequential(...)`/
`mlp_classifier(...)` + `net.fit(...)`, landed 2026-08-25/26,
`engine/crates/qu-interp/src/lib.rs`) vs Keras' `Sequential([...])` +
`model.compile()` + `model.fit()` — on **ease of use** (actual code, actual
line count) and on **speed**, given that `net.fit(...)`'s training loop now
runs entirely as ONE native-Rust builtin call (`sequential_fit`) instead of
an interpreted Qu `for`-loop re-executing `call_builtin` dispatch every
epoch.

TensorFlow 2.12 (with Keras) is installed on this machine
(`python -c "import tensorflow"` succeeds) — so this uses real Keras, not a
PyTorch stand-in.

Run:

```bash
qu run benchmarks/sequential_api/bench.qu
python benchmarks/sequential_api/bench.py
```

## Same architecture, same data as `../mlp/` — reused, not regenerated

- **Data**: `../mlp/classification_{train,test}.csv` (2400×20 train, 600×20
  test), read directly by both scripts — the exact same
  `make_classification(n_samples=3000, n_features=20, n_informative=12,
  n_redundant=4, n_classes=4, n_clusters_per_class=1, class_sep=1.4,
  flip_y=0.02, random_state=42)` split `../mlp/`'s own benchmark uses. No
  new dataset, no new `make_dataset.py` — a fair before/after against that
  benchmark's own numbers.
- **Model**: `20 -> 64 -> 32 -> 4`, ReLU hidden layers, raw-logit output,
  softmax + cross-entropy loss — identical shape to `../mlp/`.
- **Training**: full-batch gradient descent (`batch_size=n` on the Keras
  side — no mini-batching, matching `net.fit`'s own full-batch-only
  contract), 300 epochs, `lr=0.05`, **plain SGD, no momentum/Adam** on
  either side (`keras.optimizers.SGD(learning_rate=LR)` — Qu's
  `net.fit`/`sequential_fit` only ever does `w -= lr * grad`, so Adam would
  be comparing a different algorithm, not a different language).
- **Not reused**: bit-identical initial weights. `../mlp/`'s whole point was
  proving gradient correctness from numpy-literal-pasted, byte-identical
  floats; `sequential(..., seed=42)` uses Qu's own He/Xavier initializer
  (`random_matrix`) instead — different starting floats, same
  architecture/algorithm. This benchmark is about the API surface and the
  training loop's own speed, not re-litigating gradient correctness.

## The code, side by side

This is the entire "model definition through trained model" — everything
above/below it (CSV loading, one-hot encoding, accuracy computation) is
shared plumbing neither side gets credit or blame for.

**Qu — 2 statements/lines:**

```
net = mlp_classifier(20, [64, 32], N_CLASSES, seed=42)
trained = net.fit(Xtr, Ytr, epochs=EPOCHS, lr=LR, loss="cross_entropy")
```

**Keras — 3 statements, 9 lines pretty-printed (`../mlp/`-style one-layer-per-line):**

```python
model = keras.Sequential([
    keras.layers.Dense(64, activation="relu", input_shape=(N_FEATURES,)),
    keras.layers.Dense(32, activation="relu"),
    keras.layers.Dense(N_CLASSES),
])
model.compile(
    optimizer=keras.optimizers.SGD(learning_rate=LR),
    loss=keras.losses.CategoricalCrossentropy(from_logits=True),
)
history = model.fit(Xtr, Ytr, epochs=EPOCHS, batch_size=n, verbose=0)
```

**Reading it honestly**: Qu's `mlp_classifier(in_dim, hidden_dims,
n_classes, seed=)` collapses "build a dense+relu stack" into one call with
no per-layer input-shape bookkeeping (Keras' first `Dense` needs
`input_shape=(20,)` spelled out explicitly — Qu's `dense_layer` chain
already knows each layer's `in_dim` from the previous one). The bigger
structural difference is `compile()`: Keras separates "what loss/optimizer"
from "run it" into its own mandatory call; `net.fit(X, Y, epochs=, lr=,
loss=)` folds both into the one call that actually trains. 2 statements vs
3 — a real, if modest, ergonomics edge for Qu here, not just a wash.

## Timing

Three single-trial runs each (same "not statistically rigorous, good enough
to see the shape" caveat as the rest of `benchmarks/README.md`):

| | run 1 | run 2 | run 3 | avg total | avg per-epoch |
|---|---:|---:|---:|---:|---:|
| Qu (`net.fit`, native-Rust loop) | 2.1837 s | 2.1331 s | 2.0894 s | **2.135 s** | **7.12 ms** |
| Keras (TensorFlow 2.12, CPU) | 7.6074 s | 8.1989 s | 7.6865 s | **7.831 s** | **26.10 ms** |

**Qu is ~3.7x faster than Keras here** — the opposite direction from
`../mlp/`'s manual-loop finding. The reason isn't that Qu's underlying
matmuls got faster; it's architectural: `net.fit(...)` is ONE call into
`sequential_fit` (`engine/crates/qu-interp/src/lib.rs`), which runs its own
300-epoch loop as plain Rust — no Qu bytecode/AST dispatch per epoch at
all, just the tape-reset + forward + one batched `eval_grad` call + SGD
update, all native. Keras' `model.fit(...)`, despite also being "one call"
from the script's point of view, carries real per-step Python/graph-
dispatch overhead internally (callback bookkeeping, `tf.function`
retracing/execution machinery) that dominates wall-clock on a workload
this small (2400×20 full-batch matmuls are microseconds of actual compute
— TensorFlow's own step overhead is what's being measured here, not its
BLAS throughput).

**This does not directly "close the ~7x PyTorch gap" from `../mlp/`** —
that number compared hand-written manual autodiff loops against PyTorch
specifically; this compares high-level *Sequential* APIs against
TensorFlow/Keras (the framework actually installed on this machine, per
the task's own instructions), a different library with different
per-step overhead characteristics. What this DOES show, as an honest
proxy for the same underlying question ("does Qu's autodiff-loop overhead
show up here too?"): moving the epoch loop out of interpreted Qu script
and into a native Rust builtin (`net.fit`) measurably helps *Qu's own*
per-epoch cost — `../mlp/`'s manual interpreted loop ran ~9.9-10.3 ms/epoch
on the same-shaped 2400-sample workload; `net.fit`'s native loop here runs
~7.0-7.3 ms/epoch, roughly a **30% reduction** (not a controlled
before/after — different initial weights, see above — but the same
architecture, data size, and epoch count). It didn't come close to
PyTorch's ~1.3-1.5 ms/epoch on the manual-loop side of that comparison, so
Qu's autodiff-loop overhead is still real; it's just measured here against
a different, and in this instance slower, competitor.

## Accuracy — genuinely comparable

| | first-epoch loss | last-epoch loss | train accuracy | test accuracy |
|---|---:|---:|---:|---:|
| Qu (`mlp_classifier` + `net.fit`) | 2.581579 | 0.214134 | 0.9463 | 0.9533 |
| Keras run 1 | 2.432636 | 0.184272 | 0.9592 | 0.9600 |
| Keras run 2 | 1.757800 | 0.182273 | 0.9546 | 0.9500 |
| Keras run 3 | 1.751508 | 0.173182 | 0.9613 | 0.9567 |

Both converge to essentially the same place — mid-90s% train/test accuracy
on a real 4-class, 20-feature, 2400-sample problem, same architecture, same
optimizer, same epoch count. Qu's numbers are exactly reproducible run to
run (`sequential(..., seed=42)` deterministically reseeds Qu's RNG); Keras'
initial loss and final accuracy visibly vary between runs (1.75-2.58 first
loss, 95.0-96.1% test accuracy) despite `tf.random.set_seed(42)` — TensorFlow's
global seed doesn't fully pin every op's RNG stream on this setup, a
Keras-side quirk, not a Qu-side one; noted honestly rather than
cherry-picking the closest-looking run.

## Files

- `bench.qu` — loads `../mlp/`'s CSVs, builds+trains via
  `mlp_classifier(...)` + `net.fit(...)`, computes train/test accuracy via
  `net.predict(...)` + `argmax`.
- `bench.py` — same data/architecture/optimizer via
  `keras.Sequential([...])` + `.compile()` + `.fit()`.
- No committed data files here — both scripts read `../mlp/`'s already-committed
  `classification_{train,test}.csv` directly.
