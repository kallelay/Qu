# Shallow (classical) ML: train + predict timing, Qu vs scikit-learn vs MATLAB vs matty

k-means, PCA, an RBF SVM classifier, a random forest, and gradient boosting
— five classical ML algorithms, timed separately for **fit** and **predict**
(a train-only number hides a model that's slow to use, and vice versa),
cross-checked for correctness/quality (not just speed), on the same data
across every language.

Run:

```bash
python benchmarks/shallow_ml/make_dataset.py   # once, regenerate the shared CSVs
qu run benchmarks/shallow_ml/bench.qu
python benchmarks/shallow_ml/bench.py
```

Matty (from `matty/`'s own repo root, its own `.venv`):

```bash
./.venv/Scripts/python.exe matty_runner.py ../Qu/benchmarks/shallow_ml/bench_matty.m
```

## Dataset

Generated once by `make_dataset.py` (`sklearn.datasets.make_classification`/
`make_blobs`, seed 42) and reused byte-identically by every language —
nothing is regenerated per-script, unlike `csv_filters`/`full_pipeline`.

- **Classification** (SVM, random forest, gradient boosting, and — features
  only — PCA): `make_classification(n_samples=4000, n_features=20,
  n_informative=10, n_redundant=5, n_classes=2, class_sep=1.2, flip_y=0.01)`,
  stratified 80/20 split → `classification_train.csv` (3200×20) /
  `classification_test.csv` (800×20).
- **Clustering** (k-means): `make_blobs(n_samples=6000, n_features=8,
  centers=6, cluster_std=1.6, center_box=(-10,10))`, stratified 80/20 split
  → `clustering_train.csv` (4800×8) / `clustering_test.csv` (1200×8), with a
  `true_label` column (which blob each point was drawn from) carried along
  for a quality sanity check only — never fed to k-means itself.
- `*_nohdr.csv` variants (no header row, numeric-only) exist alongside the
  headered ones for MATLAB-family `csvread`, which — like real MATLAB's
  without an `R1` row offset — can't skip a text header row; Matty's own
  `csvread` is a thin `np.loadtxt` wrapper with the same limitation.
- CSVs are committed (small enough: ~1-2 MB total) rather than gitignored,
  since exact byte-identical reuse across four languages is the entire
  point of this scenario, unlike `csv_filters`' regenerate-per-run signal.

## Algorithm → builtin mapping and hyperparameters

| Algorithm | Qu | scikit-learn | MATLAB | matty |
|---|---|---|---|---|
| k-means (k=6) | `kmeans_model(X, 6, seed=42)` | `KMeans(n_clusters=6, random_state=42)` | `kmeans(X, 6)` | `kmeans(X, 6)` |
| PCA (k=5) | `pca_model(X, 5)` | `PCA(n_components=5)` | `pca(X)`, keep top 5 | `pca(X)`, keep top 5 |
| SVM (RBF) | `svm_model(X, y, kernel="rbf", C=1.0, gamma=0.05)` | `SVC(kernel="rbf", C=1.0, gamma=0.05)` | `fitcsvm(..., 'KernelFunction','rbf', 'BoxConstraint',1)` | not implemented |
| Random forest | `random_forest_model(X, y, 100, kind="classification", max_depth=10, seed=42)` | `RandomForestClassifier(n_estimators=100, max_depth=10, random_state=42)` | `fitcensemble(..., 'Method','Bag', 'NumLearningCycles',100)` | not implemented |
| Gradient boosting | `gradient_boosting_model(X, y, 100, kind="classification", learning_rate=0.1, max_depth=3)` | `GradientBoostingClassifier(n_estimators=100, learning_rate=0.1, max_depth=3, random_state=42)` | `fitcensemble(..., 'Method','LogitBoost', 'NumLearningCycles',100, 'LearnRate',0.1)` | not implemented |

`gamma=0.05` is Qu's own default (`1/n_features = 1/20`), pinned explicitly
on the sklearn side too (not `gamma="scale"`) so both engines use the exact
same RBF kernel, not two different heuristics that happen to share a name.

## MATLAB: Statistics and Machine Learning Toolbox checked, found NOT actually available

Per the standing instruction to verify rather than guess: `license('test',
'statistics_toolbox')` **and** `license('checkout','statistics_toolbox')`
both return `1` (the license entitlement exists) — but `ver` on this
machine lists only `MATLAB` and `Parallel Computing Toolbox`, and every
toolbox function this benchmark needs is undefined:

```
>> exist('fitcsvm')      ans = 0
>> exist('kmeans')       ans = 0
>> exist('pca')          ans = 0
>> exist('fitctree')     ans = 0
>> exist('fitcensemble') ans = 0
>> kmeans(rand(10,2),2)
Undefined function 'kmeans' for input arguments of type 'double'.
```

The toolbox is **licensed but not installed** on this machine — a different
failure mode than "no license," and worth distinguishing since a naive
`license('test',...)` check alone would have wrongly suggested it was
available. **MATLAB has no column in this benchmark's results at all** —
not guessed, not silently skipped, not worked around. (Contrast
`benchmarks/linalg/README.md`/`image_processing/README.md`'s MATLAB
columns, which use base-MATLAB/Image Processing Toolbox functions this
install does have.)

## matty: confirmed real ML support is `kmeans`/`pca` only

Checked `matty/TODO.md` and `matty/src/builtins.py`'s own builtin-name
dictionary before assuming anything: `pca` (SVD-based, MATLAB's documented
sign convention, verified against Lindsay-Smith reference data per
`TODO.md`) and `kmeans` (`scipy.cluster.vq.kmeans2`, `minit='++'`) are both
real, but grepping for `svm`/`fitcsvm`/`tree`/`fitctree`/`ensemble`/
`fitensemble`/`forest`/`boosting` across `TODO.md` and `builtins.py` found
**nothing** — matty is a MATLAB-*language* interpreter, not a
reimplementation of the Statistics and Machine Learning Toolbox's
classifiers. It gets a column for k-means and PCA only; SVM/random
forest/gradient boosting are correctly out of scope for it, not a gap in
this benchmark. (One lexer limitation hit and worked around: matty's
parser doesn't handle MATLAB's `...` line-continuation inside a
multi-string `disp([...])` call — `Lexer error ... Unterminated string
literal` — `bench_matty.m` keeps those lines unwrapped instead.)

## Results

Three independent measurements: two full `qu run`/`python bench.py`
process invocations (each internally looping 3 trials per algorithm, so 6
data points per cell) plus a third confirmation run via the exact required
`cargo run --manifest-path engine/Cargo.toml -p qu-cli --release -- run
benchmarks/shallow_ml/bench.qu` command. This machine is genuinely shared
(Dropbox/OneDrive sync, another live Claude/Codex session doing concurrent
`cargo build`s in the same repo throughout), and it shows: ranges below are
min–max across all trials, not a single clean number.

| Algorithm | Qu fit (s) | Qu predict (s) | sklearn fit (s) | sklearn predict (s) |
|---|---:|---:|---:|---:|
| k-means (k=6) | 0.0018–0.0040 | 0.0001–0.0013 | see note below | see note below |
| PCA (k=5) | 0.0028–0.0059 | 0.0002–0.0005 | 0.0004–0.0069 | 0.0001–0.0008 |
| SVM (RBF, n=3200) | 1.01–1.40 | 0.023–0.033 | 0.17–0.43 | 0.062–0.15 |
| Random forest (100 trees) | 1.84–3.06 | 0.008–0.026 | 0.59–1.16 | 0.008–0.029 |
| Gradient boosting (100 trees) | 0.64–1.06 | 0.0015–0.0028 | 1.56–2.27 | 0.0008–0.0013 |

**k-means timing is a special case, called out rather than folded into the
table above**: sklearn's `KMeans.fit`'s *first* call in a fresh process
pays a one-time BLAS/OpenMP thread-pool spin-up that dwarfs the actual
algorithm — observed anywhere from 0.004s to **2.79s** for the exact same
`n_init=1` call depending on process/machine state, a >600x spread that is
pure warm-up noise, not the algorithm. Qu has no such effect (no BLAS
dependency for this kernel) and stays in a tight 0.0018–0.0040s band across
every trial. Reporting a single "sklearn k-means fit" number here would be
actively misleading; see the k-means quality section below instead, where
the real, honest story is about solution quality, not speed.

Matty (kmeans + PCA only — no SVM/random forest/gradient boosting):

| Algorithm | Matty fit (s) | Matty predict (s) |
|---|---:|---:|
| k-means (k=6) | 0.0040–0.0041 (excl. one 0.45s first-call outlier, same warm-up effect as sklearn) | 0.0005–0.0007 |
| PCA (k=5) | 0.0055–0.0077 | 0.0001–0.0004 |

## Correctness and quality cross-checks

**SVM/random forest/gradient boosting — held-out test accuracy** (800 test
rows, never seen during fit):

| Model | Qu test acc | sklearn test acc | Qu train acc | sklearn train acc |
|---|---:|---:|---:|---:|
| SVM (RBF) | **0.9800** | **0.9800** | — | — |
| Random forest | 0.9575 | 0.9600 | 0.9884 | 0.9938 |
| Gradient boosting | **0.8888** | **0.9500** | 0.9119 | 0.9775 |

SVM matches essentially exactly (1989 vs 1988 support vectors — one-point
difference from Qu's simplified-SMO vs libsvm's solver, immaterial).
Random forest is close (1-2 points). **Gradient boosting shows a real,
unresolved 6-point accuracy gap** — same hyperparameters (100 trees,
`max_depth=3`, `learning_rate=0.1`), same log-odds-space binary-deviance
algorithm shape (verified by reading `gradient_boosting_model`'s own doc
comment in `engine/crates/qu-interp/src/lib.rs`), yet Qu's boosted trees
underfit relative to sklearn's (train accuracy 0.9119 vs 0.9775 — the gap
shows up in *training* accuracy too, so it isn't a generalization/overfit
difference, it's the per-tree fit itself). Flagged honestly rather than
explained away with an unverified guess — worth its own investigation into
`TreeBuilder`'s split-search (candidate threshold selection, tie-breaking)
on residual targets specifically, separate from this benchmarking task.

**k-means — inertia and cluster-recovery quality, not just speed**: this
surfaced the most interesting finding of the whole scenario. Qu's
`kmeans_model` is **plain single-random-init Lloyd's algorithm — no
k-means++ seeding, no multi-restart** (confirmed by reading `kmeans_fit`'s
own doc comment). sklearn's `KMeans` defaults changed in 1.4+ to
`n_init="auto"`, which for k-means++ init means **exactly one restart**
too — so naively comparing "each engine's own default" is comparing two
single-shot runs, and it showed:

| Config | Train inertia | Test inertia | ARI vs true blobs |
|---|---:|---:|---:|
| Qu (`kmeans_model`, single random init, seed=42) | **98502.15** | 24206.62 | **1.0000** |
| sklearn `n_init=1` (its own current default) | 152028.83 | 37224.54 | 0.7730 |
| sklearn `n_init=10` (its traditional default pre-1.4) | **98502.15** | 24206.62 | **1.0000** |
| matty (`kmeans2`, `minit='++'`) | **98502.15** | 24206.62 | **1.0000** |

sklearn's own current single-restart default (`n_init="auto"`) landed in a
visibly worse local optimum on this dataset (ARI 0.77 — it visibly
confuses two of the six blobs) — while Qu's *simpler*, restart-free
algorithm, matty's `kmeans2`, and sklearn's own *old* multi-restart default
all converged to the exact same global optimum, confirmed by computing
what the inertia would be at the true blob centers directly:
**98502.1528**, matching all three to 4 decimal places. This isn't a case
of Qu getting lucky on an easy dataset either — sklearn's newer default
genuinely underperforms here. Read honestly: Qu's k-means is simpler
(single-shot, no restarts) but this run happened to land on the global
optimum; a harder/less-separated dataset would likely need Qu's own
restarts (not currently implemented) to match reliably. `bench.qu`/
`bench_matty.m` each dump their fitted labels (`kmeans_qu_labels.csv`/
`kmeans_matty_labels.csv`) alongside the dataset's own `true_label` for
this Adjusted-Rand-Index check, computed via
`sklearn.metrics.adjusted_rand_score`.

**PCA — explained variance ratio**, top 5 components, computed
independently by three different SVD implementations (Qu's `qu_core::
linalg::svd`, numpy/scikit-learn's LAPACK-backed SVD, matty's own
`np.linalg.svd` call):

| | PC1 | PC2 | PC3 | PC4 | PC5 | sum |
|---|---:|---:|---:|---:|---:|---:|
| Qu | 0.34016 | 0.20750 | 0.17235 | 0.06315 | 0.04504 | 0.8282 |
| scikit-learn | 0.34016 | 0.20750 | 0.17235 | 0.06315 | 0.04504 | 0.8282 |
| matty | 0.34016 | 0.20750 | 0.17235 | 0.06315 | 0.04504 | 0.8282 |

**Exact match to 5 decimal places across all three** — the strongest
possible correctness signal for this scenario.

## Out of scope / known gaps

- **MATLAB**: entirely excluded — Statistics and Machine Learning Toolbox
  licensed but not installed on this machine (see above). No numbers
  fabricated or estimated in its place.
- **matty**: SVM, random forest, and gradient boosting are not implemented
  (confirmed by grep, not assumed) — k-means and PCA only.
- **SVM/logistic regression multi-class**: `svm_model` is binary-only
  (one-vs-rest not implemented); this dataset is binary by construction so
  it doesn't bite here, but it's a real scope limit worth remembering.
- **k-means quality gap**: flagged above, not fixed — `kmeans_model` has no
  k-means++ initialization or multi-restart (`n_init=`-equivalent)
  parameter; adding either would be a `qu-interp/src/lib.rs` change, out of
  scope for a benchmarking task (and that file was mid-edit by a concurrent
  session for part of this task's runtime).
- **Gradient boosting accuracy gap**: flagged above with training-accuracy
  evidence ruling out overfitting as the explanation, but the root cause
  in `TreeBuilder`'s split search was not investigated further this pass.
- Build note: `engine/crates/qu-interp/src/lib.rs` briefly failed to
  compile mid-task from a concurrent session's in-progress edit (a real
  borrow-checker conflict, not this benchmark's own code) — verified via
  an isolated `git worktree`/`git archive` checkout at the same commit
  while that was in flight, then re-verified with the exact required
  `cargo run --manifest-path engine/Cargo.toml -p qu-cli --release -- run
  benchmarks/shallow_ml/bench.qu` command once the other session committed
  its fix (results above include that run as the third data point).
