# bench.py -- MLP (20->64->32->4, He-init, ReLU, softmax+cross-entropy)
# trained via PyTorch autograd with a manual (non torch.optim) SGD update
# loop -- the Python-side mirror of bench.qu's manual param()/grad()/
# stop_grad() training loop. Same architecture, same initial weights
# (loaded from init_weights.npz -- generated once by make_dataset.py with
# numpy RandomState(42), and pasted verbatim as literal matrices into
# bench.qu, so both languages start from bit-identical floats), same data
# (classification_{train,test}.csv, same make_dataset.py), same epochs/lr,
# same loss formula (-mean(sum(Y_onehot * log_softmax(logits), axis=1))),
# for an apples-to-apples timing + accuracy comparison.
#
# Run: python benchmarks/mlp/bench.py

import time
import os
import numpy as np
import pandas as pd
import torch

OUT = os.path.dirname(os.path.abspath(__file__))
torch.manual_seed(0)  # irrelevant to results (no randomness left once
# weights are loaded from init_weights.npz below), kept only so nothing
# else in this process accidentally depends on unseeded global RNG state.

EPOCHS = 300
LR = 0.05
N_CLASSES = 4

# --- load data (same CSVs bench.qu reads) -------------------------------
tr = pd.read_csv(os.path.join(OUT, "classification_train.csv"))
te = pd.read_csv(os.path.join(OUT, "classification_test.csv"))
feat_cols = [c for c in tr.columns if c != "y"]

Xtr = torch.tensor(tr[feat_cols].values, dtype=torch.float64)
ytr = tr["y"].values.astype(int)
Xte = torch.tensor(te[feat_cols].values, dtype=torch.float64)
yte = te["y"].values.astype(int)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n}x{Xtr.shape[1]}, test {nte}x{Xte.shape[1]}")


def one_hot(y, k):
    m = np.zeros((len(y), k))
    m[np.arange(len(y)), y] = 1
    return torch.tensor(m, dtype=torch.float64)


Ytr = one_hot(ytr, N_CLASSES)
Yte = one_hot(yte, N_CLASSES)

# --- shared initial weights (He init, numpy RandomState(42) -- see
# make_dataset.py; loaded verbatim so PyTorch and Qu start from the exact
# same floats) ------------------------------------------------------------
d = np.load(os.path.join(OUT, "init_weights.npz"))


def param(arr):
    return torch.tensor(arr, dtype=torch.float64, requires_grad=True)


W1 = param(d["W1"]); b1 = param(d["b1"])
W2 = param(d["W2"]); b2 = param(d["b2"])
W3 = param(d["W3"]); b3 = param(d["b3"])
params = [W1, b1, W2, b2, W3, b3]


def forward(X, W1, b1, W2, b2, W3, b3):
    h1 = torch.relu(X @ W1 + b1)
    h2 = torch.relu(h1 @ W2 + b2)
    logits = h2 @ W3 + b3
    return logits


def loss_fn(logits, Y):
    logp = torch.log_softmax(logits, dim=1)
    return -(Y * logp).sum(dim=1).mean()


# --- training: manual SGD update loop (no torch.optim), full-batch,
# mirroring bench.qu's param()/grad()/stop_grad() loop step for step ------
losses = []
t0 = time.perf_counter()
for epoch in range(EPOCHS):
    logits = forward(Xtr, W1, b1, W2, b2, W3, b3)
    loss = loss_fn(logits, Ytr)
    losses.append(loss.item())

    for p in params:
        if p.grad is not None:
            p.grad = None
    loss.backward()
    with torch.no_grad():
        for p in params:
            p -= LR * p.grad
t1 = time.perf_counter()
train_time = t1 - t0

print(f"first_loss={losses[0]:.6f}  last_loss={losses[-1]:.6f}")
print(f"train_time_s={train_time:.4f}  per_epoch_ms={1000 * train_time / EPOCHS:.4f}")


def accuracy(X, Y, W1, b1, W2, b2, W3, b3):
    with torch.no_grad():
        logits = forward(X, W1, b1, W2, b2, W3, b3)
        pred = logits.argmax(dim=1)
        actual = Y.argmax(dim=1)
        return (pred == actual).double().mean().item()


train_acc = accuracy(Xtr, Ytr, W1, b1, W2, b2, W3, b3)
test_acc = accuracy(Xte, Yte, W1, b1, W2, b2, W3, b3)
print(f"train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
