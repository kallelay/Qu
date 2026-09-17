# scenario3_rnn_make_dataset.py -- generates the shared synthetic sequence
# dataset for Scenario 3 (Sequence/RNN), ONCE, so Qu/PyTorch/Keras all train
# on the exact same sequences. Deterministic (numpy RandomState(5));
# rerunning reproduces the same files.
#
# Extends `simple_rnn_classifier`'s own rising-vs-falling ramp acceptance
# test (`engine/crates/qu-interp/tests/acceptance.rs`,
# `simple_rnn_classifier_fit_learns_rising_vs_falling_sequences`, 2 classes,
# 5-step sequences, 10 samples) to a slightly harder 3-class task at a real
# "few hundred samples" scale, per the task spec: rising ramp / falling ramp
# / oscillating (sine) sequence, length 20, each with per-sample phase/
# amplitude jitter and additive noise. input_size=1 (one scalar per
# timestep), matching the acceptance test's own convention.
#
# input_size=1 means each sequence is naturally just ONE row of T=20
# numbers -- unlike Scenario 2's images, no vertical-stacking trick is
# needed: `rnn_{train,test}_sequences.csv` is a plain (n_samples, 20)
# headerless CSV, one sequence per row, read directly by both `read_csv
# (..., headers=false)` (Qu) and `pandas`/`numpy` (Python).
#
# Run once: python benchmarks/multi_scenario/scenario3_rnn_make_dataset.py

import os
import numpy as np
from sklearn.model_selection import train_test_split

OUT = os.path.dirname(os.path.abspath(__file__))

T = 20  # sequence length
N_PER_CLASS = 60  # 180 total
NOISE_STD = 0.05
rng = np.random.RandomState(5)

t = np.linspace(0.0, 1.0, T)


def make_rising(rng):
    slope = 0.6 + 0.4 * rng.rand()
    start = 0.1 * rng.rand()
    return start + slope * t


def make_falling(rng):
    slope = 0.6 + 0.4 * rng.rand()
    start = 0.9 - 0.1 * rng.rand()
    return start - slope * t


def make_oscillating(rng):
    freq = 1.5 + rng.rand()  # cycles over the window
    phase = rng.rand() * 2 * np.pi
    amp = 0.3 + 0.15 * rng.rand()
    center = 0.5
    return center + amp * np.sin(2 * np.pi * freq * t + phase)


makers = [make_rising, make_falling, make_oscillating]

sequences = []
labels = []
for label, maker in enumerate(makers):
    for _ in range(N_PER_CLASS):
        seq = maker(rng) + rng.normal(0.0, NOISE_STD, size=T)
        sequences.append(seq.astype(np.float32))
        labels.append(label)

sequences = np.stack(sequences, axis=0)  # (N, T)
labels = np.array(labels, dtype=int)

Xtr, Xte, ytr, yte = train_test_split(
    sequences, labels, test_size=0.2, random_state=5, stratify=labels
)
print(f"sequences: train {Xtr.shape}, test {Xte.shape}, classes train={np.bincount(ytr)} test={np.bincount(yte)}")


def write_seq_csv(path, X):
    np.savetxt(path, X, delimiter=",", fmt="%.8g")


def write_labels_csv(path, y):
    with open(path, "w", newline="") as f:
        f.write("y\n")
        for v in y:
            f.write(f"{int(v)}\n")


write_seq_csv(os.path.join(OUT, "rnn_train_sequences.csv"), Xtr)
write_seq_csv(os.path.join(OUT, "rnn_test_sequences.csv"), Xte)
write_labels_csv(os.path.join(OUT, "rnn_train_labels.csv"), ytr)
write_labels_csv(os.path.join(OUT, "rnn_test_labels.csv"), yte)

np.savez(os.path.join(OUT, "rnn_train_sequences.npz"), X=Xtr, y=ytr)
np.savez(os.path.join(OUT, "rnn_test_sequences.npz"), X=Xte, y=yte)

print("wrote rnn_{train,test}_sequences.{csv,npz} + rnn_{train,test}_labels.csv")
