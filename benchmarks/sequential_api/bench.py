# bench.py -- Sequential-API ease-of-use + speed benchmark: Keras'
# `Sequential([...])` + `model.compile()` + `model.fit()` one-liner
# high-level API, the Python-side mirror of bench.qu's `mlp_classifier(...)`
# + `net.fit(...)`. Same architecture, same data, same optimizer (plain SGD,
# no momentum/Adam -- matching Qu's `net.fit`, which only does a plain
# `w -= lr * grad` update, see `sequential_fit`'s own doc comment in
# `engine/crates/qu-interp/src/lib.rs`), same epochs/lr, same full-batch (no
# mini-batching -- `batch_size=len(Xtr)`), same one-hot + categorical
# cross-entropy loss formula, for an apples-to-apples ease-of-use AND speed
# comparison. Reuses ../mlp/'s already-committed
# classification_{train,test}.csv directly (same make_classification split
# ../mlp/bench.py itself reads) -- not a new dataset in disguise.
#
# Run: python benchmarks/sequential_api/bench.py

import time
import os
import numpy as np
import pandas as pd
import tensorflow as tf
from tensorflow import keras

OUT = os.path.dirname(os.path.abspath(__file__))
MLP_DATA = os.path.join(OUT, "..", "mlp")

EPOCHS = 300
LR = 0.05
N_CLASSES = 4
N_FEATURES = 20

tf.random.set_seed(42)
np.random.seed(42)

# --- load data (same CSVs ../mlp/bench.py reads) -------------------------
tr = pd.read_csv(os.path.join(MLP_DATA, "classification_train.csv"))
te = pd.read_csv(os.path.join(MLP_DATA, "classification_test.csv"))
feat_cols = [c for c in tr.columns if c != "y"]

Xtr = tr[feat_cols].values.astype(np.float64)
ytr = tr["y"].values.astype(int)
Xte = te[feat_cols].values.astype(np.float64)
yte = te["y"].values.astype(int)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n}x{Xtr.shape[1]}, test {nte}x{Xte.shape[1]}")

Ytr = keras.utils.to_categorical(ytr, N_CLASSES)
Yte = keras.utils.to_categorical(yte, N_CLASSES)

# =============================================================================
# THE ENTIRE MODEL: construction through trained model -- 3 statements
# (Keras needs an explicit compile() step bench.qu's net.fit(..., loss=...)
# folds into the fit call itself; the model-definition Sequential([...])
# call itself is 1 statement/expression, pretty-printed across several
# lines below purely for the usual one-layer-per-line Keras style). Compare
# against bench.qu's net = mlp_classifier(20, [64, 32], 4, seed=42);
# trained = net.fit(Xtr, Ytr, epochs=EPOCHS, lr=LR, loss="cross_entropy")
# -- 2 statements.
# =============================================================================
model = keras.Sequential([
    keras.layers.Dense(64, activation="relu", input_shape=(N_FEATURES,)),
    keras.layers.Dense(32, activation="relu"),
    keras.layers.Dense(N_CLASSES),
])
model.compile(
    optimizer=keras.optimizers.SGD(learning_rate=LR),
    loss=keras.losses.CategoricalCrossentropy(from_logits=True),
)

t0 = time.perf_counter()
history = model.fit(Xtr, Ytr, epochs=EPOCHS, batch_size=n, verbose=0)
t1 = time.perf_counter()
train_time = t1 - t0
# =============================================================================

losses = history.history["loss"]
print(f"first_loss={losses[0]:.6f}  last_loss={losses[-1]:.6f}")
print(f"train_time_s={train_time:.4f}  per_epoch_ms={1000 * train_time / EPOCHS:.4f}")


def accuracy(X, Y):
    logits = model.predict(X, verbose=0)
    pred = logits.argmax(axis=1)
    actual = Y.argmax(axis=1)
    return (pred == actual).mean()


train_acc = accuracy(Xtr, Ytr)
test_acc = accuracy(Xte, Yte)
print(f"train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
