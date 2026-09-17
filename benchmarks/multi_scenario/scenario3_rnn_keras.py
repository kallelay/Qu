# scenario3_rnn_keras.py -- Scenario 3 (Sequence/RNN), Keras side.
#
# Same data and matching architecture to scenario3_rnn_qu.qu /
# scenario3_rnn_pytorch.py: `layers.GRU(hidden_size=8)` (returns only the
# LAST hidden state by default, matching Qu's/PyTorch's "readout on the
# final hidden state" convention) + `Dense(3)` readout. Full-batch
# (batch_size=n), plain SGD, same epochs/lr.
#
# Run: python benchmarks/multi_scenario/scenario3_rnn_keras.py

import time
import os
import numpy as np
import tensorflow as tf
from tensorflow import keras

HERE = os.path.dirname(os.path.abspath(__file__))

EPOCHS = 200
LR = 0.3
N_CLASSES = 3
HIDDEN = 8

tf.random.set_seed(13)
np.random.seed(13)

tr = np.load(os.path.join(HERE, "rnn_train_sequences.npz"))
te = np.load(os.path.join(HERE, "rnn_test_sequences.npz"))
Xtr = tr["X"][..., np.newaxis]  # (n, T, 1)
ytr = tr["y"].astype(int)
Xte = te["X"][..., np.newaxis]
yte = te["y"].astype(int)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n} sequences len {Xtr.shape[1]}, test {nte} sequences")

# =============================================================================
# THE ENTIRE MODEL: GRU (last hidden state) -> Dense, plain SGD.
# =============================================================================
model = keras.Sequential([
    keras.layers.GRU(HIDDEN, input_shape=(Xtr.shape[1], 1)),
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
