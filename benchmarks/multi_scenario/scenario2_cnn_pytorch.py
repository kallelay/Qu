# scenario2_cnn_pytorch.py -- Scenario 2 (Image/CNN), PyTorch side.
#
# Same data (scenario2_cnn_make_dataset.py's cnn_{train,test}_images.npz,
# loaded directly -- exact floats, no CSV round trip needed on this side)
# and same architecture as scenario2_cnn_qu.qu's `simple_cnn([16,16], 3,
# seed=)`: one 3x3 "valid" Conv2d (1 in-channel, 1 out-channel, matching
# Qu's single learned kernel) + bias -> ReLU -> 2x2 non-overlapping
# MaxPool2d -> Flatten -> Linear(7*7, 3) (16x16 -conv3x3-> 14x14 -pool2-> 7x7).
# Full-batch (whole train set every step), plain SGD (no momentum), same
# epochs/lr as the Qu side.
#
# Run: python benchmarks/multi_scenario/scenario2_cnn_pytorch.py

import time
import os
import numpy as np
import torch
import torch.nn as nn

HERE = os.path.dirname(os.path.abspath(__file__))

EPOCHS = 250
LR = 0.3
N_CLASSES = 3

torch.manual_seed(11)

tr = np.load(os.path.join(HERE, "cnn_train_images.npz"))
te = np.load(os.path.join(HERE, "cnn_test_images.npz"))
Xtr = torch.tensor(tr["X"], dtype=torch.float32).unsqueeze(1)  # (n,1,16,16)
ytr = torch.tensor(tr["y"], dtype=torch.long)
Xte = torch.tensor(te["X"], dtype=torch.float32).unsqueeze(1)
yte = torch.tensor(te["y"], dtype=torch.long)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n} images {Xtr.shape[2]}x{Xtr.shape[3]}, test {nte} images")

# =============================================================================
# THE ENTIRE MODEL: Conv2d -> ReLU -> MaxPool2d -> Flatten -> Linear, plain SGD.
# =============================================================================
model = nn.Sequential(
    nn.Conv2d(1, 1, kernel_size=3),   # "valid" padding (PyTorch default)
    nn.ReLU(),
    nn.MaxPool2d(2),
    nn.Flatten(),
    nn.Linear(7 * 7, N_CLASSES),
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
