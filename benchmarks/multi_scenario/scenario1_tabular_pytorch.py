# scenario1_tabular_pytorch.py -- Scenario 1 (tabular/MLP), PyTorch side.
#
# Reuses ../mlp/'s already-committed dataset directly (classification_
# {train,test}.csv). Architecture 20 -> 64 -> 32 -> 4 (ReLU hidden layers),
# same epochs/lr as ../mlp/ and scenario1_tabular_qu.qu (300 epochs,
# lr=0.05), full-batch, plain SGD (no momentum) -- but via the HIGH-LEVEL
# nn.Sequential + nn.CrossEntropyLoss + optim.SGD API (per the task spec),
# not ../mlp/bench.py's manual param()/backward()/grad-update loop. This is
# the fairer "one-liner API vs one-liner API" comparison against
# scenario1_tabular_qu.qu's mlp_classifier()+net.fit() and
# scenario1_tabular_keras.py's Sequential()+compile()+fit().
#
# Run: python benchmarks/multi_scenario/scenario1_tabular_pytorch.py

import time
import os
import numpy as np
import pandas as pd
import torch
import torch.nn as nn

HERE = os.path.dirname(os.path.abspath(__file__))
MLP_DATA = os.path.join(HERE, "..", "mlp")

EPOCHS = 300
LR = 0.05
N_CLASSES = 4
N_FEATURES = 20

torch.manual_seed(42)

tr = pd.read_csv(os.path.join(MLP_DATA, "classification_train.csv"))
te = pd.read_csv(os.path.join(MLP_DATA, "classification_test.csv"))
feat_cols = [c for c in tr.columns if c != "y"]

Xtr = torch.tensor(tr[feat_cols].values, dtype=torch.float32)
ytr = torch.tensor(tr["y"].values.astype(int), dtype=torch.long)
Xte = torch.tensor(te[feat_cols].values, dtype=torch.float32)
yte = torch.tensor(te["y"].values.astype(int), dtype=torch.long)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n}x{Xtr.shape[1]}, test {nte}x{Xte.shape[1]}")

# =============================================================================
# THE ENTIRE MODEL: nn.Sequential + nn.CrossEntropyLoss + optim.SGD.
# =============================================================================
model = nn.Sequential(
    nn.Linear(N_FEATURES, 64),
    nn.ReLU(),
    nn.Linear(64, 32),
    nn.ReLU(),
    nn.Linear(32, N_CLASSES),
)
loss_fn = nn.CrossEntropyLoss()
optimizer = torch.optim.SGD(model.parameters(), lr=LR)

t0 = time.perf_counter()
losses = []
for epoch in range(EPOCHS):
    optimizer.zero_grad()
    logits = model(Xtr)
    loss = loss_fn(logits, ytr)
    loss.backward()
    optimizer.step()
    losses.append(loss.item())
t1 = time.perf_counter()
train_time = t1 - t0
# =============================================================================

print(f"first_loss={losses[0]:.6f}  last_loss={losses[-1]:.6f}")
print(f"train_time_s={train_time:.4f}  per_epoch_ms={1000 * train_time / EPOCHS:.4f}")


def accuracy(X, y):
    with torch.no_grad():
        logits = model(X)
        pred = logits.argmax(dim=1)
        return (pred == y).float().mean().item()


train_acc = accuracy(Xtr, ytr)
test_acc = accuracy(Xte, yte)
print(f"train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
