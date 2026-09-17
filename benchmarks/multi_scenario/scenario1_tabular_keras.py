# scenario1_tabular_keras.py -- Scenario 1 (tabular/MLP), Keras side.
#
# Reuses ../mlp/'s already-committed dataset directly. Architecture
# 20 -> 64 -> 32 -> 4 (ReLU hidden layers), same epochs/lr as ../mlp/ and
# scenario1_tabular_qu.qu (300 epochs, lr=0.05), full-batch (batch_size=n),
# plain SGD (no momentum) -- Sequential + compile + fit, matching
# ../sequential_api/bench.py's own approach.
#
# Run: python benchmarks/multi_scenario/scenario1_tabular_keras.py

import time
import os
import numpy as np
import pandas as pd
import tensorflow as tf
from tensorflow import keras

HERE = os.path.dirname(os.path.abspath(__file__))
MLP_DATA = os.path.join(HERE, "..", "mlp")

EPOCHS = 300
LR = 0.05
N_CLASSES = 4
N_FEATURES = 20

tf.random.set_seed(42)
np.random.seed(42)

tr = pd.read_csv(os.path.join(MLP_DATA, "classification_train.csv"))
te = pd.read_csv(os.path.join(MLP_DATA, "classification_test.csv"))
feat_cols = [c for c in tr.columns if c != "y"]

Xtr = tr[feat_cols].values.astype(np.float32)
ytr = tr["y"].values.astype(int)
Xte = te[feat_cols].values.astype(np.float32)
yte = te["y"].values.astype(int)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n}x{Xtr.shape[1]}, test {nte}x{Xte.shape[1]}")

# =============================================================================
# THE ENTIRE MODEL: Sequential + compile + fit.
# =============================================================================
model = keras.Sequential([
    keras.layers.Dense(64, activation="relu", input_shape=(N_FEATURES,)),
    keras.layers.Dense(32, activation="relu"),
    keras.layers.Dense(N_CLASSES),
])
model.compile(
    optimizer=keras.optimizers.SGD(learning_rate=LR),
    loss=keras.losses.SparseCategoricalCrossentropy(from_logits=True),
)

t0 = time.perf_counter()
history = model.fit(Xtr, ytr, epochs=EPOCHS, batch_size=n, verbose=0)
t1 = time.perf_counter()
train_time = t1 - t0
# =============================================================================

losses = history.history["loss"]
print(f"first_loss={losses[0]:.6f}  last_loss={losses[-1]:.6f}")
print(f"train_time_s={train_time:.4f}  per_epoch_ms={1000 * train_time / EPOCHS:.4f}")


def accuracy(X, y):
    logits = model.predict(X, verbose=0)
    pred = logits.argmax(axis=1)
    return (pred == y).mean()


train_acc = accuracy(Xtr, ytr)
test_acc = accuracy(Xte, yte)
print(f"train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
