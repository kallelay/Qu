# make_dataset.py -- generates the ONE shared dataset both ml_stress.qu and
# ml_stress.py use for both sub-cases (knn classification + Sequential-API
# MLP classification), so every accuracy number is a real apples-to-apples
# comparison, not two different random draws. Same convention as
# ../../shallow_ml/make_dataset.py / ../../mlp/make_dataset.py.
#
# A real, non-trivial multi-class classification task -- 2000 samples, 12
# features, 4 classes -- well beyond catalog/qu_classification_report.qu's
# 120-point 2-class toy, stratified 80/20 split (1600 train / 400 test).
#
# Run once: python benchmarks/advanced_validation/ml_stress/make_dataset.py

import numpy as np
from sklearn.datasets import make_classification
from sklearn.model_selection import train_test_split
import csv
import os

OUT = os.path.dirname(os.path.abspath(__file__))

X, y = make_classification(
    n_samples=2000,
    n_features=12,
    n_informative=9,
    n_redundant=2,
    n_repeated=0,
    n_classes=4,
    n_clusters_per_class=1,
    class_sep=1.3,
    flip_y=0.02,
    random_state=42,
)
Xtr, Xte, ytr, yte = train_test_split(X, y, test_size=0.2, random_state=42, stratify=y)


def write_csv(path, X, y):
    n, d = X.shape
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow([f"f{i}" for i in range(d)] + ["y"])
        for i in range(n):
            w.writerow(list(X[i]) + [int(y[i])])


write_csv(os.path.join(OUT, "classification_train.csv"), Xtr, ytr)
write_csv(os.path.join(OUT, "classification_test.csv"), Xte, yte)
print(
    f"classification: train {Xtr.shape}, test {Xte.shape}, "
    f"classes train={np.bincount(ytr)} test={np.bincount(yte)}"
)
