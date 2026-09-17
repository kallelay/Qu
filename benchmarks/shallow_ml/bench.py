# bench.py -- shallow (classical) ML train+predict timing: k-means, SVM
# (RBF), random forest, gradient boosting, and PCA, via scikit-learn.
# Same classification_{train,test}.csv / clustering_{train,test}.csv
# datasets as bench.qu/bench.m (generated once by make_dataset.py).
#
# Run: python benchmarks/shallow_ml/bench.py

import time
import numpy as np
import pandas as pd
from sklearn.cluster import KMeans
from sklearn.decomposition import PCA
from sklearn.svm import SVC
from sklearn.ensemble import RandomForestClassifier, GradientBoostingClassifier
from sklearn.metrics import accuracy_score

DATA = "benchmarks/shallow_ml/"
N_TRIALS = 3
FEAT_COLS_20 = [f"f{i}" for i in range(20)]
FEAT_COLS_8 = [f"f{i}" for i in range(8)]

df = pd.read_csv(DATA + "classification_train.csv")
dfte = pd.read_csv(DATA + "classification_test.csv")
X = df[FEAT_COLS_20].to_numpy()
y = df["y"].to_numpy()
Xte = dfte[FEAT_COLS_20].to_numpy()
yte = dfte["y"].to_numpy()
print(f"classification: train {X.shape}, test {Xte.shape}")

dfc = pd.read_csv(DATA + "clustering_train.csv")
dfcte = pd.read_csv(DATA + "clustering_test.csv")
Xc = dfc[FEAT_COLS_8].to_numpy()
Xcte = dfcte[FEAT_COLS_8].to_numpy()
print(f"clustering: train {Xc.shape}, test {Xcte.shape}")
print()


def inertia(X, centers):
    # sum of squared distances to the nearest center -- same quantity
    # sklearn's own `.inertia_` reports for the training set, computed by
    # hand here so it can also be applied to held-out test data (sklearn
    # has no built-in "score a KMeans on new data" beyond `.score`, which
    # already returns exactly this, negated -- kept explicit for clarity).
    d2 = ((X[:, None, :] - centers[None, :, :]) ** 2).sum(axis=2)
    return d2.min(axis=1).sum()


# =========================== k-means (k=6) ===================================
# Qu's kmeans_model is plain Lloyd's algorithm from a SINGLE random init --
# no k-means++ seeding, no multi-restart. sklearn's default changed to
# n_init="auto" (effectively 1 restart, but with a smarter k-means++ init)
# in 1.4+. Run BOTH n_init=1 (closest apples-to-apples timing match to what
# Qu actually does) and n_init=10 (sklearn's long-standing traditional
# default, restarts and keeps the best-inertia solution) so the timing
# comparison is fair AND the quality cost of Qu's simpler algorithm is
# honestly visible rather than hidden.
for n_init_label, n_init_val in [("n_init=1", 1), ("n_init=10", 10)]:
    km_fit, km_pred = [], []
    for _ in range(N_TRIALS):
        t0 = time.perf_counter()
        km = KMeans(n_clusters=6, random_state=42, n_init=n_init_val)
        km.fit(Xc)
        km_fit.append(time.perf_counter() - t0)
        t0 = time.perf_counter()
        labels_te = km.predict(Xcte)
        km_pred.append(time.perf_counter() - t0)
    train_inertia = km.inertia_
    test_inertia = inertia(Xcte, km.cluster_centers_)
    print(f"kmeans[{n_init_label}] fit  trials(s)={[round(x,4) for x in km_fit]}  mean={np.mean(km_fit):.4f}")
    print(f"kmeans[{n_init_label}] pred trials(s)={[round(x,4) for x in km_pred]}  mean={np.mean(km_pred):.4f}")
    print(f"kmeans[{n_init_label}] train_inertia={train_inertia:.4f}  test_inertia={test_inertia:.4f}")
print()

# =========================== PCA (k=5) =======================================
pca_fit, pca_pred = [], []
for _ in range(N_TRIALS):
    t0 = time.perf_counter()
    pca = PCA(n_components=5)
    pca.fit(X)
    pca_fit.append(time.perf_counter() - t0)
    t0 = time.perf_counter()
    proj_te = pca.transform(Xte)
    pca_pred.append(time.perf_counter() - t0)
print(f"pca           fit  trials(s)={[round(x,4) for x in pca_fit]}  mean={np.mean(pca_fit):.4f}")
print(f"pca           pred trials(s)={[round(x,4) for x in pca_pred]}  mean={np.mean(pca_pred):.4f}")
print(f"pca           explained_variance_ratio(top5)={np.round(pca.explained_variance_ratio_,6).tolist()}")
print(f"pca           sum_explained(top5)={pca.explained_variance_ratio_.sum():.4f}")
print()

# =========================== SVM (RBF, C=1, gamma=1/n_features) =============
svm_fit_t, svm_pred_t = [], []
for _ in range(N_TRIALS):
    t0 = time.perf_counter()
    svm = SVC(kernel="rbf", C=1.0, gamma=0.05)
    svm.fit(X, y)
    svm_fit_t.append(time.perf_counter() - t0)
    t0 = time.perf_counter()
    pred = svm.predict(Xte)
    svm_pred_t.append(time.perf_counter() - t0)
svm_acc = accuracy_score(yte, pred)
print(f"svm(rbf)      fit  trials(s)={[round(x,4) for x in svm_fit_t]}  mean={np.mean(svm_fit_t):.4f}")
print(f"svm(rbf)      pred trials(s)={[round(x,4) for x in svm_pred_t]}  mean={np.mean(svm_pred_t):.4f}")
print(f"svm(rbf)      test_accuracy={svm_acc:.4f}  n_support={svm.n_support_.sum()}")
print()

# =========================== Random Forest (100 trees, depth 10) ============
rf_fit_t, rf_pred_t = [], []
for _ in range(N_TRIALS):
    t0 = time.perf_counter()
    rf = RandomForestClassifier(n_estimators=100, max_depth=10, random_state=42)
    rf.fit(X, y)
    rf_fit_t.append(time.perf_counter() - t0)
    t0 = time.perf_counter()
    pred = rf.predict(Xte)
    rf_pred_t.append(time.perf_counter() - t0)
rf_acc = accuracy_score(yte, pred)
rf_train_acc = accuracy_score(y, rf.predict(X))
print(f"random_forest fit  trials(s)={[round(x,4) for x in rf_fit_t]}  mean={np.mean(rf_fit_t):.4f}")
print(f"random_forest pred trials(s)={[round(x,4) for x in rf_pred_t]}  mean={np.mean(rf_pred_t):.4f}")
print(f"random_forest test_accuracy={rf_acc:.4f}  train_accuracy={rf_train_acc:.4f}")
print()

# =========================== Gradient Boosting (100 trees, depth 3, lr 0.1) ==
gb_fit_t, gb_pred_t = [], []
for _ in range(N_TRIALS):
    t0 = time.perf_counter()
    gb = GradientBoostingClassifier(n_estimators=100, learning_rate=0.1, max_depth=3, random_state=42)
    gb.fit(X, y)
    gb_fit_t.append(time.perf_counter() - t0)
    t0 = time.perf_counter()
    pred = gb.predict(Xte)
    gb_pred_t.append(time.perf_counter() - t0)
gb_acc = accuracy_score(yte, pred)
gb_train_acc = accuracy_score(y, gb.predict(X))
print(f"grad_boosting fit  trials(s)={[round(x,4) for x in gb_fit_t]}  mean={np.mean(gb_fit_t):.4f}")
print(f"grad_boosting pred trials(s)={[round(x,4) for x in gb_pred_t]}  mean={np.mean(gb_pred_t):.4f}")
print(f"grad_boosting test_accuracy={gb_acc:.4f}  train_accuracy={gb_train_acc:.4f}")
