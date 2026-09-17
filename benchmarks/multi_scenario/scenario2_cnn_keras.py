# scenario2_cnn_keras.py -- Scenario 2 (Image/CNN), Keras side.
#
# Same data and architecture as scenario2_cnn_qu.qu / scenario2_cnn_pytorch.py:
# Conv2D(filters=1, kernel_size=3, "valid") -> ReLU -> MaxPooling2D(2) ->
# Flatten -> Dense(3). Full-batch (batch_size=n), plain SGD, same epochs/lr.
#
# Run: python benchmarks/multi_scenario/scenario2_cnn_keras.py

import time
import os
import numpy as np
import tensorflow as tf
from tensorflow import keras

HERE = os.path.dirname(os.path.abspath(__file__))

EPOCHS = 250
LR = 0.3
N_CLASSES = 3

tf.random.set_seed(11)
np.random.seed(11)

tr = np.load(os.path.join(HERE, "cnn_train_images.npz"))
te = np.load(os.path.join(HERE, "cnn_test_images.npz"))
Xtr = tr["X"][..., np.newaxis]  # (n,16,16,1) -- channels-last
ytr = tr["y"].astype(int)
Xte = te["X"][..., np.newaxis]
yte = te["y"].astype(int)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n} images {Xtr.shape[1]}x{Xtr.shape[2]}, test {nte} images")

# =============================================================================
# THE ENTIRE MODEL: Conv2D -> ReLU -> MaxPooling2D -> Flatten -> Dense, plain SGD.
# =============================================================================
model = keras.Sequential([
    keras.layers.Conv2D(1, 3, activation="relu", input_shape=(16, 16, 1)),  # "valid" (Keras default)
    keras.layers.MaxPooling2D(2),
    keras.layers.Flatten(),
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
