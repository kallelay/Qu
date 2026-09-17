# ml_stress.py -- Python-side mirror of ml_stress.qu's two sub-cases, on
# the SAME classification_{train,test}.csv (generated once by
# make_dataset.py):
#
#   1. sklearn KNeighborsClassifier(n_neighbors=7) -- deterministic, no
#      random init anywhere, so this is checked for an EXACT match against
#      Qu's knn_model on the same split.
#   2. A PyTorch MLP (12->32->16->4, ReLU, Adam(lr=0.001), cross-entropy),
#      same architecture/optimizer/hyperparameters/epochs as
#      ml_stress.qu's Sequential-API net, but its OWN random init --
#      there is no bit-identical-init shortcut across Qu's RNG and
#      PyTorch's without the make_dataset.py-style precomputed-weights
#      trick ../../mlp/ already uses (and duplicating that machinery here
#      would just re-test what ../../mlp/ already covers). This asks a
#      different, still-valid question: does Qu's own Adam-based
#      Sequential .fit converge to comparable accuracy as PyTorch's Adam,
#      each from its own random start, same data/architecture/epochs.
#
# Run: python benchmarks/advanced_validation/ml_stress/ml_stress.py

import os
import time
import numpy as np
import pandas as pd
import torch
from sklearn.neighbors import KNeighborsClassifier

OUT = os.path.dirname(os.path.abspath(__file__))
N_FEATURES = 12
N_CLASSES = 4

tr = pd.read_csv(os.path.join(OUT, "classification_train.csv"))
te = pd.read_csv(os.path.join(OUT, "classification_test.csv"))
feat_cols = [c for c in tr.columns if c != "y"]

Xtr_np = tr[feat_cols].values.astype(np.float64)
ytr_np = tr["y"].values.astype(int)
Xte_np = te[feat_cols].values.astype(np.float64)
yte_np = te["y"].values.astype(int)
n, nte = Xtr_np.shape[0], Xte_np.shape[0]
print(f"train {n}x{Xtr_np.shape[1]}, test {nte}x{Xte_np.shape[1]}, classes={N_CLASSES}")
print()

# =========================== Sub-case 1: k-NN (k=7) ==========================
t0 = time.perf_counter()
knn = KNeighborsClassifier(n_neighbors=7, metric="euclidean")
knn.fit(Xtr_np, ytr_np)
t1 = time.perf_counter()
knn_pred = knn.predict(Xte_np)
t2 = time.perf_counter()
knn_acc = knn.score(Xte_np, yte_np)
print(f"knn(k=7)      fit_time={(t1-t0)*1000:.4f} ms  pred_time={(t2-t1)*1000:.4f} ms")
print(f"knn(k=7)      test_accuracy={knn_acc:.4f}")
print(f"knn(k=7)      predictions (first 20) = {list(knn_pred[:20])}")

# Elementwise diff against Qu's own dumped predictions (ml_stress.qu must
# have been run first) -- a real per-sample check, not just "same accuracy".
qu_pred_path = os.path.join(OUT, "knn_qu_predictions.csv")
if os.path.exists(qu_pred_path):
    qu_pred = pd.read_csv(qu_pred_path)["qu_pred"].to_numpy().astype(int)
    n_mismatch = int(np.sum(qu_pred != knn_pred))
    print(f"knn(k=7)      elementwise diff vs Qu's predictions: {n_mismatch}/{len(knn_pred)} mismatches")
else:
    print("knn(k=7)      (run ml_stress.qu first for an elementwise prediction diff)")
print()

# =========================== Sub-case 2: PyTorch MLP (Adam) =================
torch.manual_seed(0)  # PyTorch's own init -- see module docstring above for
# why this is NOT the same starting point as Qu's seed=42 init.

Xtr = torch.tensor(Xtr_np, dtype=torch.float64)
ytr = torch.tensor(ytr_np, dtype=torch.long)
Xte = torch.tensor(Xte_np, dtype=torch.float64)
yte = torch.tensor(yte_np, dtype=torch.long)


class Mlp(torch.nn.Module):
    def __init__(self):
        super().__init__()
        self.fc1 = torch.nn.Linear(N_FEATURES, 32).double()
        self.fc2 = torch.nn.Linear(32, 16).double()
        self.fc3 = torch.nn.Linear(16, N_CLASSES).double()

    def forward(self, x):
        h1 = torch.relu(self.fc1(x))
        h2 = torch.relu(self.fc2(h1))
        return self.fc3(h2)


net = Mlp()
opt = torch.optim.Adam(net.parameters(), lr=0.001, betas=(0.9, 0.999), eps=1e-8)
loss_fn = torch.nn.CrossEntropyLoss()

EPOCHS = 150
losses = []
t0 = time.perf_counter()
for epoch in range(EPOCHS):
    logits = net(Xtr)
    loss = loss_fn(logits, ytr)
    losses.append(loss.item())
    opt.zero_grad()
    loss.backward()
    opt.step()
t1 = time.perf_counter()
train_time = t1 - t0

with torch.no_grad():
    train_acc = (net(Xtr).argmax(dim=1) == ytr).double().mean().item()
    test_acc = (net(Xte).argmax(dim=1) == yte).double().mean().item()

print(f"sequential_mlp epochs={EPOCHS}  train_time={train_time:.4f} s  per_epoch={1000*train_time/EPOCHS:.4f} ms")
print(f"sequential_mlp first_loss={losses[0]:.6f}  last_loss={losses[-1]:.6f}")
print(f"sequential_mlp train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
