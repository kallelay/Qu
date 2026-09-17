# make_dataset.py -- generates the shared dataset + shared weight
# initialization for the mlp benchmark (Qu vs PyTorch), ONCE, so both
# languages train on byte-identical data starting from byte-identical
# weights. Same pattern as benchmarks/shallow_ml/make_dataset.py (CSVs
# committed, regenerate by rerunning this script -- deterministic, seed 42
# throughout).
#
# Two outputs:
#   1. classification_{train,test}.csv -- sklearn make_classification,
#      features f0..f19 + integer label y in {0,1,2,3}.
#   2. init_weights.npz -- He-initialized weights for a 20->64->32->4 MLP
#      (numpy RandomState(42)), consumed by bench.py directly and pasted
#      as literal Qu matrices into bench.qu by gen_qu_literals.py (this
#      guarantees bit-identical starting weights across languages, since
#      Qu's own RNG and numpy's are different algorithms -- there is no way
#      to get the same numbers from "same seed" in two different RNGs, so
#      the actual float values are generated once, here, and shared).
#
# Run once: python benchmarks/mlp/make_dataset.py

import numpy as np
from sklearn.datasets import make_classification
from sklearn.model_selection import train_test_split
import csv
import os

OUT = os.path.dirname(os.path.abspath(__file__))

N_FEATURES = 20
HIDDEN1 = 64
HIDDEN2 = 32
N_CLASSES = 4

# --- classification dataset --------------------------------------------
X, y = make_classification(
    n_samples=3000,
    n_features=N_FEATURES,
    n_informative=12,
    n_redundant=4,
    n_repeated=0,
    n_classes=N_CLASSES,
    n_clusters_per_class=1,
    class_sep=1.4,
    flip_y=0.02,
    random_state=42,
)
Xtr, Xte, ytr, yte = train_test_split(
    X, y, test_size=0.2, random_state=42, stratify=y
)


def write_csv(path, X, y, feat_prefix="f", label_name="y"):
    n, d = X.shape
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow([f"{feat_prefix}{i}" for i in range(d)] + [label_name])
        for i in range(n):
            w.writerow(list(X[i]) + [int(y[i])])


write_csv(os.path.join(OUT, "classification_train.csv"), Xtr, ytr)
write_csv(os.path.join(OUT, "classification_test.csv"), Xte, yte)
print(
    f"classification: train {Xtr.shape}, test {Xte.shape}, "
    f"classes train={np.bincount(ytr)} test={np.bincount(yte)}"
)

# --- shared weight init (He init, numpy RandomState(42)) ----------------
rng = np.random.RandomState(42)


def he(fan_in, fan_out):
    return rng.randn(fan_in, fan_out) * np.sqrt(2.0 / fan_in)


W1 = he(N_FEATURES, HIDDEN1)
b1 = np.zeros((1, HIDDEN1))
W2 = he(HIDDEN1, HIDDEN2)
b2 = np.zeros((1, HIDDEN2))
W3 = he(HIDDEN2, N_CLASSES)
b3 = np.zeros((1, N_CLASSES))

np.savez(
    os.path.join(OUT, "init_weights.npz"),
    W1=W1, b1=b1, W2=W2, b2=b2, W3=W3, b3=b3,
)
print(f"init weights: W1{W1.shape} W2{W2.shape} W3{W3.shape} (He init, seed 42)")
