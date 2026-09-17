# bench.py -- PyTorch equivalents of Qu's `lr_adaptive`/`lr_plateau`
# learning-rate CONTROLLERS (bench.qu), same `20 -> 64 -> 32 -> 4` MLP,
# He-init, ReLU, softmax+cross-entropy as ../mlp/bench.py -- reuses that
# script's own `classification_{train,test}.csv` and `init_weights.npz`
# directly (same `make_classification(n_samples=3000, ..., random_state=
# 42)` split, same numpy `RandomState(42)` He-init weights, loaded
# byte-for-byte so all four runs below start from the exact same floats,
# just like ../mlp/bench.py and bench.qu do for their own comparison) --
# not a new dataset/init in disguise.
#
# `lr_plateau` has a direct PyTorch built-in:
#   torch.optim.lr_scheduler.ReduceLROnPlateau(optimizer, mode="min",
#   patience=10, factor=0.5, threshold=1e-9, threshold_mode="abs")
# `threshold`/`threshold_mode` are set to match Qu's own `plateau_step`
# tolerance (`LR_PLATEAU_TOL = 1e-9`, an ABSOLUTE "did the loss go down by
# at least this much" test -- PyTorch's default is a 1e-4 RELATIVE
# threshold, which would silently make this comparison less strict than
# Qu's own controller and could produce a false "not a no-op" result for
# a different reason than Qu's).
#
# `lr_adaptive` has no PyTorch built-in (no shipped scheduler does
# reject-and-retry), so it's hand-rolled here to mirror `sequential_fit`'s
# own logic exactly (see `engine/crates/qu-interp/src/lib.rs`'s doc
# comment on `sequential_fit`, "lr_adaptive" match arm): each epoch, compute
# the gradient once at the CURRENT params, then repeatedly try a candidate
# SGD step at the current attempt lr, probe its loss with NO grad tracking
# (mirrors Qu's `sequential_forward(..., training=false, ...)` probe), and
# either commit + grow `lr *= increase_factor` (first improving attempt) or
# reject + shrink `lr *= decrease_factor` and retry from the SAME starting
# params/gradient, up to 20 bounded attempts -- identical factors/bound to
# Qu's `MAX_LR_ADAPTIVE_RETRIES = 20`.
#
# Four runs, same as bench.qu, matching the dated IMPL.md entry for these
# two controllers:
#   1. fixed-lr SGD (lr=0.05)                                    -- baseline
#   2. ReduceLROnPlateau(patience=10, factor=0.5)                on SGD(lr=0.05)
#   3. hand-rolled lr_adaptive(lr0=0.05, inc=1.05, dec=0.5)
#   4. hand-rolled lr_adaptive(lr0=5.0, inc=1.05, dec=0.5)  -- oversized start
#
# Run: python benchmarks/lr_schedulers/bench.py

import os
import time

import numpy as np
import pandas as pd
import torch

OUT = os.path.dirname(os.path.abspath(__file__))
MLP_DATA = os.path.join(OUT, "..", "mlp")
torch.manual_seed(0)  # irrelevant to results once weights load from
# init_weights.npz below -- kept only so nothing else in this process
# accidentally depends on unseeded global RNG state.

EPOCHS = 300
N_CLASSES = 4

# --- load data (same CSVs ../mlp/bench.py and bench.qu both read) ----------
tr = pd.read_csv(os.path.join(MLP_DATA, "classification_train.csv"))
te = pd.read_csv(os.path.join(MLP_DATA, "classification_test.csv"))
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

# --- shared initial weights (same init_weights.npz ../mlp/bench.py loads,
# generated once by ../mlp/make_dataset.py with numpy RandomState(42)) ------
D = np.load(os.path.join(MLP_DATA, "init_weights.npz"))


def load_init_params():
    """Fresh plain (non-leaf, requires_grad=False) tensors from the shared
    npz init -- every run below starts from exactly these same floats."""
    return [
        torch.tensor(D["W1"], dtype=torch.float64),
        torch.tensor(D["b1"], dtype=torch.float64),
        torch.tensor(D["W2"], dtype=torch.float64),
        torch.tensor(D["b2"], dtype=torch.float64),
        torch.tensor(D["W3"], dtype=torch.float64),
        torch.tensor(D["b3"], dtype=torch.float64),
    ]


def forward(X, W1, b1, W2, b2, W3, b3):
    h1 = torch.relu(X @ W1 + b1)
    h2 = torch.relu(h1 @ W2 + b2)
    logits = h2 @ W3 + b3
    return logits


def loss_fn(logits, Y):
    logp = torch.log_softmax(logits, dim=1)
    return -(Y * logp).sum(dim=1).mean()


def accuracy(params, X, Y):
    with torch.no_grad():
        logits = forward(X, *params)
        pred = logits.argmax(dim=1)
        actual = Y.argmax(dim=1)
        return (pred == actual).double().mean().item()


# --- warm-up: a throwaway forward+backward pass so the FIRST timed run
# below doesn't eat PyTorch/MKL/thread-pool lazy-init cost (~20s observed
# once, ~1-2s every run after) -- this benchmark is about convergence
# behavior/lr trajectories, not throughput (../mlp/ and ../sequential_api/
# already cover speed in depth), so timings are reported for completeness
# only, not as this script's main claim.
_wp = [p.clone().requires_grad_(True) for p in load_init_params()]
loss_fn(forward(Xtr, *_wp), Ytr).backward()
del _wp


def sample(name, loss_hist, lr_hist, params, t):
    print(f"== {name} ==")
    print(
        f"  loss: first={loss_hist[0]:.6f} ep50={loss_hist[50]:.6f} "
        f"ep150={loss_hist[150]:.6f} last={loss_hist[-1]:.6f}"
    )
    print(
        f"  lr:   ep0={lr_hist[0]:.6f} ep50={lr_hist[50]:.6f} "
        f"ep100={lr_hist[100]:.6f} ep150={lr_hist[150]:.6f} "
        f"ep200={lr_hist[200]:.6f} ep299={lr_hist[-1]:.6f}"
    )
    train_acc = accuracy(params, Xtr, Ytr)
    test_acc = accuracy(params, Xte, Yte)
    print(f"  train_accuracy={train_acc:.4f}  test_accuracy={test_acc:.4f}")
    print(f"  time_s={t:.4f}")


# =============================================================================
# 1. fixed-lr SGD (lr=0.05) baseline -- plain manual GD, mirrors
#    ../mlp/bench.py's own loop and bench.qu's sgd(lr=0.05) run.
# =============================================================================
LR = 0.05
params = [p.clone().requires_grad_(True) for p in load_init_params()]
loss_hist, lr_hist = [], []
t0 = time.perf_counter()
for epoch in range(EPOCHS):
    logits = forward(Xtr, *params)
    loss = loss_fn(logits, Ytr)
    loss_hist.append(loss.item())
    lr_hist.append(LR)
    for p in params:
        if p.grad is not None:
            p.grad = None
    loss.backward()
    with torch.no_grad():
        for p in params:
            p -= LR * p.grad
t1 = time.perf_counter()
sample("fixed-lr SGD (lr=0.05)", loss_hist, lr_hist, [p.detach() for p in params], t1 - t0)

# =============================================================================
# 2. ReduceLROnPlateau(patience=10, factor=0.5) on SGD(lr=0.05) -- PyTorch's
#    direct built-in equivalent of Qu's lr_plateau(...). threshold=1e-9,
#    threshold_mode="abs" to match Qu's own tolerance (see module doc
#    comment above for why the PyTorch default wouldn't be a fair test).
# =============================================================================
params = [p.clone().requires_grad_(True) for p in load_init_params()]
optimizer = torch.optim.SGD(params, lr=0.05, momentum=0.0)
scheduler = torch.optim.lr_scheduler.ReduceLROnPlateau(
    optimizer, mode="min", patience=10, factor=0.5, threshold=1e-9, threshold_mode="abs"
)
loss_hist, lr_hist = [], []
t0 = time.perf_counter()
for epoch in range(EPOCHS):
    logits = forward(Xtr, *params)
    loss = loss_fn(logits, Ytr)
    loss_hist.append(loss.item())
    optimizer.zero_grad()
    loss.backward()
    optimizer.step()
    scheduler.step(loss.item())
    lr_hist.append(optimizer.param_groups[0]["lr"])
t1 = time.perf_counter()
sample(
    "ReduceLROnPlateau (lr0=0.05, patience=10, factor=0.5)",
    loss_hist, lr_hist, [p.detach() for p in params], t1 - t0,
)


# =============================================================================
# 3 & 4. hand-rolled lr_adaptive -- no PyTorch built-in does LM-style
#    reject-and-retry, so this mirrors sequential_fit's own "lr_adaptive"
#    branch step for step (see module doc comment above).
# =============================================================================
def train_lr_adaptive(lr0, increase_factor=1.05, decrease_factor=0.5, max_retries=20):
    params = load_init_params()  # plain tensors, requires_grad=False
    lr = lr0
    loss_hist, lr_hist = [], []
    t0 = time.perf_counter()
    for epoch in range(EPOCHS):
        leaves = [p.clone().requires_grad_(True) for p in params]
        logits = forward(Xtr, *leaves)
        loss = loss_fn(logits, Ytr)
        starting_loss = loss.item()
        grads = torch.autograd.grad(loss, leaves)

        attempt_lr = lr
        accepted = False
        accepted_loss = starting_loss
        for _ in range(max_retries):
            candidate = [(p - attempt_lr * g).detach() for p, g in zip(params, grads)]
            with torch.no_grad():
                logits_c = forward(Xtr, *candidate)
                loss_c = loss_fn(logits_c, Ytr).item()
            if loss_c < starting_loss:
                params = candidate
                lr = attempt_lr * increase_factor
                accepted_loss = loss_c
                accepted = True
                break
            attempt_lr *= decrease_factor
        if not accepted:
            # every retry this epoch still made things worse -- weights
            # stay exactly as they were entering the epoch, lr is left at
            # its final (shrunk) value for the next epoch to try from.
            lr = attempt_lr
            accepted_loss = starting_loss

        loss_hist.append(accepted_loss)
        lr_hist.append(lr)
    t1 = time.perf_counter()
    return params, loss_hist, lr_hist, t1 - t0


params, loss_hist, lr_hist, t = train_lr_adaptive(lr0=0.05, increase_factor=1.05, decrease_factor=0.5)
sample("lr_adaptive (lr0=0.05, inc=1.05, dec=0.5)", loss_hist, lr_hist, params, t)

params, loss_hist, lr_hist, t = train_lr_adaptive(lr0=5.0, increase_factor=1.05, decrease_factor=0.5)
sample("lr_adaptive (deliberately large lr0=5.0)", loss_hist, lr_hist, params, t)
