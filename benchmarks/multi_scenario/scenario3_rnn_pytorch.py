# scenario3_rnn_pytorch.py -- Scenario 3 (Sequence/RNN), PyTorch side.
#
# Same data (scenario3_rnn_make_dataset.py's rnn_{train,test}_sequences.npz,
# loaded directly) and matching architecture to scenario3_rnn_qu.qu's
# `simple_rnn_classifier(1, hidden_size=8, 3, seed=)`: a single-layer
# `nn.GRU(input_size=1, hidden_size=8)` + `nn.Linear(8, 3)` readout on the
# GRU's LAST hidden state (matching Qu's `dense(tt(h_last), out_w, out_b)`
# on `gru_forward`'s final hidden state). Full-batch, plain SGD (no
# momentum), same epochs/lr as the Qu side.
#
# Run: python benchmarks/multi_scenario/scenario3_rnn_pytorch.py

import time
import os
import numpy as np
import torch
import torch.nn as nn

HERE = os.path.dirname(os.path.abspath(__file__))

EPOCHS = 200
LR = 0.3
N_CLASSES = 3
HIDDEN = 8

torch.manual_seed(13)

tr = np.load(os.path.join(HERE, "rnn_train_sequences.npz"))
te = np.load(os.path.join(HERE, "rnn_test_sequences.npz"))
# GRU expects (batch, seq_len, input_size)
Xtr = torch.tensor(tr["X"], dtype=torch.float32).unsqueeze(-1)
ytr = torch.tensor(tr["y"], dtype=torch.long)
Xte = torch.tensor(te["X"], dtype=torch.float32).unsqueeze(-1)
yte = torch.tensor(te["y"], dtype=torch.long)
n, nte = Xtr.shape[0], Xte.shape[0]
print(f"train {n} sequences len {Xtr.shape[1]}, test {nte} sequences")


# =============================================================================
# THE ENTIRE MODEL: GRU + Linear readout on the last hidden state, plain SGD.
# =============================================================================
class RnnClassifier(nn.Module):
    def __init__(self, input_size, hidden_size, n_classes):
        super().__init__()
        self.gru = nn.GRU(input_size, hidden_size, batch_first=True)
        self.out = nn.Linear(hidden_size, n_classes)

    def forward(self, x):
        _, h_last = self.gru(x)  # h_last: (1, batch, hidden)
        return self.out(h_last.squeeze(0))


model = RnnClassifier(1, HIDDEN, N_CLASSES)
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
