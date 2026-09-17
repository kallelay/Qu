# make_dataset.py -- generates the shared dataset for the shallow_ml
# benchmark, ONCE, so Qu/Python/MATLAB/Matty all train and predict on
# byte-identical data (same rows, same order, same train/test split).
#
# Two datasets:
#   1. classification_{train,test}.csv -- sklearn make_classification,
#      used for SVM and random-forest/gradient-boosting classification,
#      and (features only) for PCA.
#   2. clustering_{train,test}.csv -- sklearn make_blobs, used for k-means.
#      Carries a `true_label` column (which blob each point was drawn
#      from) for a qualitative sanity check only -- never fed to k-means
#      itself, which is unsupervised.
#
# Run once: python benchmarks/shallow_ml/make_dataset.py

import numpy as np
from sklearn.datasets import make_classification, make_blobs
from sklearn.model_selection import train_test_split
import csv
import os

OUT = os.path.dirname(os.path.abspath(__file__))

# --- classification dataset (SVM, random forest, gradient boosting, PCA) ----
X, y = make_classification(
    n_samples=4000,
    n_features=20,
    n_informative=10,
    n_redundant=5,
    n_repeated=0,
    n_classes=2,
    class_sep=1.2,
    flip_y=0.01,
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
            w.writerow(list(X[i]) + [y[i]])

def write_csv_nohdr(path, X, y):
    # No header row, numeric only -- for MATLAB-family `csvread`, which
    # (in both real MATLAB without an R1 offset and Matty's own
    # np.loadtxt-based implementation) can't skip a text header row.
    n = X.shape[0]
    data = np.hstack([X, y.reshape(n, 1)])
    np.savetxt(path, data, delimiter=",")

write_csv(os.path.join(OUT, "classification_train.csv"), Xtr, ytr)
write_csv(os.path.join(OUT, "classification_test.csv"), Xte, yte)
write_csv_nohdr(os.path.join(OUT, "classification_train_nohdr.csv"), Xtr, ytr)
write_csv_nohdr(os.path.join(OUT, "classification_test_nohdr.csv"), Xte, yte)
print(f"classification: train {Xtr.shape}, test {Xte.shape}, "
      f"class balance train={ytr.mean():.3f} test={yte.mean():.3f}")

# --- clustering dataset (k-means) -------------------------------------------
Xc, yc = make_blobs(
    n_samples=6000,
    n_features=8,
    centers=6,
    cluster_std=1.6,
    center_box=(-10.0, 10.0),
    random_state=42,
)
Xc_tr, Xc_te, yc_tr, yc_te = train_test_split(
    Xc, yc, test_size=0.2, random_state=42, stratify=yc
)
write_csv(os.path.join(OUT, "clustering_train.csv"), Xc_tr, yc_tr, label_name="true_label")
write_csv(os.path.join(OUT, "clustering_test.csv"), Xc_te, yc_te, label_name="true_label")
write_csv_nohdr(os.path.join(OUT, "clustering_train_nohdr.csv"), Xc_tr, yc_tr)
write_csv_nohdr(os.path.join(OUT, "clustering_test_nohdr.csv"), Xc_te, yc_te)
print(f"clustering: train {Xc_tr.shape}, test {Xc_te.shape}, {len(set(yc))} true blobs")
