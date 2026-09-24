# Statistics & Machine Learning

Every statistics, machine-learning and estimation builtin that runs today.
The list is read off the interpreter's own dispatch table, so nothing here
is aspirational. Plain descriptive statistics — `mean`, `std`, `var`,
`median` — are in [Core Maths](core-math.md); this chapter starts at
correlation and goes up.

## Example data

A regression problem and a held-out set. Every example below runs against
these.

```qu
X    = randn(120, 3, seed = 3)   # 120 samples, 3 features
beta = [2.0, -1.0, 0.5]
y    = X * beta + 0.1 * randn(120, seed = 4)
Xnew = randn(30, 3, seed = 5)
ynew = Xnew * beta
```

## Descriptive statistics

| Function | Signature | Description |
|---|---|---|
| `corr` | `corr(x, y)` | Pearson correlation between two length-`N` numeric vectors `x` and `y`. Returns one number in `[-1, 1]`. `xcorr` is the lag-by-lag version. |
| `cov` | `cov(x, y)` | Sample covariance between two length-`N` numeric vectors `x` and `y`, `N-1` denominator — the same convention `var` and `std` use. Returns a single number. |
| `mode` | `mode(x)` | The most frequent value in a numeric or categorical vector `x` of length `N`. Returns one scalar; ties go to the smallest, matching MATLAB. |
| `skewness` | `skewness(x)` | Third standardized moment, population form, of a length-`N` numeric vector `x`. Returns a single number: zero if symmetric, positive for a long upper tail. |
| `kurtosis` | `kurtosis(x)` | Fourth standardized moment, population form, of a length-`N` numeric vector `x`. Returns a single number — **not** excess kurtosis, so a normal distribution scores 3. |

Two conventions worth knowing before you compare a number against another
tool: `kurtosis` follows MATLAB (normal = 3), not SciPy's default
(`fisher=True`, normal = 0); and `corr(x, y)` is exactly
`cov(x, y) / (std(x) * std(y))`, because all three share the `N-1`
denominator.

`corr` and `cov` both answer "how do these two vectors move together",
just on different scales: `cov` keeps the original units (so it changes if
you rescale `a` or `b`), while `corr` divides that out and always lands in
`[-1, 1]`, which is why `corr` is the one worth reporting and `cov` is the
one worth using inside a formula. The example below builds `b` as a known
mix of `a` and independent noise, prints both numbers, then plots the
scatter so the number and the shape it describes sit side by side.

```qu
a = randn(200, seed = 11)
b = 0.8 * a + 0.6 * randn(200, seed = 12)
print("corr(a, b): {round(corr(a, b), 3)}")
print("cov(a, b):  {round(cov(a, b), 3)}")
scatter(a, b, color = "#4169e1", alpha = 0.6)
title("corr = {round(corr(a, b), 3)}, cov = {round(cov(a, b), 3)}")
xlabel("a")
ylabel("b")
```

`mode`, `skewness` and `kurtosis` describe the shape of one sample rather
than a relationship between two. `mode` just answers "which value showed up
most", useful on counts or categories where a mean would land between
categories that do not average into anything meaningful. `skewness` and
`kurtosis` are about the tails: a skewed sample has one tail longer than
the other, and a sample with heavy or light tails relative to a normal
distribution shows up as a `kurtosis` above or below 3. The example below
takes nine scores with one obvious outlier and reports all three in one
go:

```qu
scores = [3, 5, 5, 5, 6, 7, 7, 9, 20]
print("mode: {mode(scores)}")
print("skewness: {round(skewness(scores), 3)}")
print("kurtosis: {round(kurtosis(scores), 3)}")
```

The one outlier at 20 pulls the tail out to the right, which is exactly
what a positive `skewness` means, and pushes `kurtosis` well past the
normal distribution's 3.

## Regression

| Function | Signature | Description |
|---|---|---|
| `polyfit` | `polyfit(x, y, degree)` | Fits a least-squares polynomial of the given integer `degree` to a length-`N` vector `x` and a length-`N` vector `y`. Returns a length-`degree + 1` coefficient vector, highest-degree term first, ready for `polyval`. |
| `polyval` | `polyval(coeffs, x)` | Evaluates a coefficient vector `coeffs` (as returned by `polyfit`, highest-degree first) at `x`, either a single number or a length-`N` vector, by Horner's method. Returns a number or a length-`N` vector matching the shape of `x`. |
| `ridge` | `ridge(X, y, alpha)` | Tikhonov-regularized (L2) least squares on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y`, with regularization strength `alpha` (a non-negative number; plain OLS at `alpha = 0`). Returns only the length-`D` coefficient vector, with **no intercept term**. |

`ridge` adds **no intercept** — append a column of ones yourself if you
want one. `ridge_model` does add one, which is the difference between the
two names.

`polyfit` and `polyval` are a matched pair: fit once, then evaluate the
same coefficients anywhere you like, whether that is the original `x`
values (to check the fit) or a finer grid (to draw a smooth curve). The
example below fits a noisy cubic, prints the recovered coefficients, and
overlays the fitted curve on the noisy points so you can see how well a
degree-3 polynomial tracks a signal that really is degree 3:

```qu
xs = -3 to 3 step 0.25
truth = 0.5 * xs .^ 3 - 2 * xs
noisy = truth + 1.5 * randn(len(xs), seed = 7)
c = polyfit(xs, noisy, 3)
print("polyfit coefficients (highest degree first): {round(c, 3)}")
scatter(xs, noisy, color = "#94a3b8", label = "measured")
plot(xs, polyval(c, xs), color = "#4169e1", width = 2, label = "cubic fit")
legend()
title("polyfit degree 3")
```

`ridge` is for when a plain OLS fit would be unstable — collinear columns,
or more columns than rows — and a small amount of shrinkage toward zero
buys back stability at the cost of a little bias. Because `ridge` never
learns an intercept, the example below sets up a design matrix `X` with
zero mean by construction (`randn`), so the "true" coefficients really are
recoverable without one:

```qu
X    = randn(120, 3, seed = 3)
beta = [2.0, -1.0, 0.5]
y    = X * beta + 0.1 * randn(120, seed = 4)
c = ridge(X, y, 0.5)
print("ridge coef: {round(c, 3)}")   # close to [2.0, -1.0, 0.5]
```

## Time series: autoregressive models

An AR(`p`) model forecasts its own future from its own fitted history, so
`predict` takes a horizon rather than new data — the one place in this
chapter where `predict` does not mean "apply to new rows".

| Function | Signature | Description |
|---|---|---|
| `ar_model` | `ar_model(x, order)` | Fits an autoregressive AR(`p`) model by Yule-Walker to a length-`N` numeric vector `x`, using the integer lag order `order` (this is `p`). Returns a fitted model Record with fields `coef` (a length-`p` vector of AR coefficients), `order` (`p` back), `mean` (the scalar series mean subtracted before fitting), `history` (the last `p` values of `x`, used to seed forecasting), `fitted` (a length-`N` vector of in-sample fitted values), and `residuals` (a length-`N` vector, `x` minus `fitted`). |
| `predict` (AR) | `ar.predict(n_ahead)` | Forecasts `n_ahead` (a positive integer) values beyond the end of the series the model was fitted on, by recursively applying the AR coefficients. Returns a length-`n_ahead` vector. |
| `arima` | `arima(x, [p=1], [d=0], [q=0])` | Fits an ARIMA(`p`, `d`, `q`) model to a length-`N` numeric vector `x`: `d` (integer, default 0) rounds of differencing, then an ARMA fit with `p` (integer, default 1) autoregressive lags and `q` (integer, default 0) moving-average lags. Estimated by Hannan-Rissanen — a long pure-AR pass to recover usable shock estimates, then one linear least-squares solve on the lagged series and those shocks — **not** exact maximum likelihood, so coefficients differ slightly from R's `arima()` on short series. Returns a fitted model Record with fields `ar` (length-`p`), `ma` (length-`q`), `mu` (the differenced series' mean), `p`, `d`, `q`, `fitted` and `residuals` (in the differenced series' space), plus `history`, `shock_history` and `levels`, which seed forecasting. `arima(x, p=k, d=0, q=0)` models the same thing `ar_model(x, k)` does, by a different estimator. Added in v0.3.0. |
| `predict` (ARIMA) | `arima.predict(n_ahead)` | Forecasts `n_ahead` (a positive integer) values beyond the end of the series, recursively, with future shocks set to their expectation of zero and past shocks taken from the fit's own residual tail. Values come back in the **original** series' units — the `d` rounds of differencing are undone. Returns a length-`n_ahead` vector. Added in v0.3.0. |

Past the first `p` steps the recursion is conditioning on its own earlier
forecasts, because an out-of-sample future is not available to condition
on. That is not a shortcut — it is what a multi-step AR forecast is — but
it does mean the uncertainty grows with the horizon in a way the returned
numbers do not show.

```qu
x = zeros(500)
x[0] = randn(seed = 21)
x[1] = randn(seed = 22)
for t = 2 to 499
    x[t] = 0.6 * x[t - 1] - 0.3 * x[t - 2] + 0.1 * randn()
end for
m = ar_model(x, 2)
print("generating: [0.6, -0.3]")
print("recovered:  {round(m.coef, 3)}")

future = m.predict(30)
plot(460 to 499, x[460:499], color = "#0f172a", label = "measured")
plot(500 to 529, future, color = "#e11d48", width = 2, label = "forecast")
legend()
title("AR(2), 30 steps ahead")
```

How close the recovery gets depends on how much record you give it. At 500
samples it lands within a few hundredths; at 100 it does not, and the
forecast inherits that error compounded once per step.

## Clustering and dimensionality reduction

These describe the dataset they were given and have no `.predict`. Where a
reusable model makes sense there is a `_model` sibling — see
[the fitted-model protocol](#the-fitted-model-protocol).

| Function | Signature | Description |
|---|---|---|
| `kmeans` | `kmeans(X, k, [seed=])` | Lloyd's algorithm on an `N`-row, `D`-column data matrix `X`, partitioning it into the integer number of clusters `k`, starting from randomly chosen initial centroids (an optional integer `seed=` fixes the draw). Returns a length-`N` vector of integer cluster labels in `0..k-1`, one per row of `X`. |
| `kmeans_centers` | `kmeans_centers(X, k, [seed=])` | The same Lloyd's-algorithm run as `kmeans` on `X`/`k`/`seed=`, returning the `(k, D)` centroid matrix instead of the labels. Passing the same `seed=` as a `kmeans` call on the same `X`/`k` reproduces the matching run. Returns a `(k, D)` `Mat`, one centroid per row. |
| `dbscan` | `dbscan(X, eps, min_samples)` | Density-based clustering on an `N`-row, `D`-column data matrix `X`: a number `eps` (the neighbourhood radius, in `X`'s own units) and an integer `min_samples` (the minimum neighbourhood size to count as a dense core point). Returns a length-`N` vector of integer cluster labels; points that fall in no dense cluster get label `-1`. |
| `pca` | `pca(X, k)` | Projects an `N`-row, `D`-column matrix `X` onto its top `k` principal components (columns mean-centered first). Returns the `(N, k)` projected coordinate matrix. |
| `pca_components` | `pca_components(X, k)` | Fits the same PCA as `pca` on `X` and `k`, returning the `(k, D)` loading matrix instead — each row is a unit-length direction in the original `D`-dimensional feature space. Returns a `(k, D)` `Mat`. |
| `pca_explained_variance` | `pca_explained_variance(X, k)` | Fits the same PCA as `pca` on `X` and `k`, returning a length-`k` vector giving the fraction of the total variance in `X` that each kept component carries, in decreasing order. Returns a length-`k` `Vec`. |
| `tsne` | `tsne(X, [n_components=2], [perplexity=], [seed=], [max_iter=300])` | t-SNE embedding of an `N`-row, `D`-column matrix `X` into `n_components` dimensions (an integer, default 2), controlled by `perplexity` (a number balancing local vs. global structure; unset uses the implementation's default), an optional integer `seed=`, and `max_iter` (an integer iteration budget, default 300). Returns the `(N, n_components)` embedded coordinate matrix — for looking at structure rather than measuring it. |

Qu has no multiple return, so getting both labels and centroids means
calling `kmeans` and `kmeans_centers` with the same seed. `dbscan` has no
`_model` sibling on purpose: a point's cluster depends on the whole
dataset's local density, so there is no honest `.predict(Xnew)` to write.
`eps` is in `X`'s own units and nothing is auto-scaled, which is the usual
reason a first DBSCAN run returns one cluster or all noise.

```qu
G = vstack(randn(40, 2, seed = 31) + 4,
           randn(40, 2, seed = 32) - 4,
           randn(40, 2, seed = 33))
labels = kmeans(G, 3, seed = 5)
centers = kmeans_centers(G, 3, seed = 5)
xs = G[:, 0]
ys = G[:, 1]
for k in 0 to 2
    inside = labels == k
    print("cluster {k}: {len(xs[inside])} points")
    scatter(xs[inside], ys[inside], label = "cluster {k}")
end for
scatter(centers[:, 0], centers[:, 1], marker = "x", color = "#0f172a", size = 12, label = "centroids")
legend()
title("kmeans, k = 3")
```

The three blobs really do have 40 points each; k-means recovers 40, 39 and
41, and the one it misplaces sits between two clusters. A mask indexes a
vector directly -- `xs[labels == k]` -- which is how you draw one cluster
at a time, since `color =` takes one colour for the whole series rather
than one per point.

`dbscan` needs an `eps` sized to the data rather than a cluster count;
`pca`/`pca_components`/`pca_explained_variance` describe the same fit
three different ways; `tsne` is for a quick look at a handful of points,
not a metric to report:

```qu
G = vstack(randn(40, 2, seed = 31) + 4,
           randn(40, 2, seed = 32) - 4,
           randn(40, 2, seed = 33))
d = dbscan(G, 1.5, 4)
print("dbscan labels: {distinct(d)}")

proj  = pca(G, 2)                    # the (n, 2) projected coordinates
comps = pca_components(G, 2)         # the (2, 2) loading matrix
ev    = pca_explained_variance(G, 2)
print("pca projected shape: {rows(proj)} x {cols(proj)}")
print("pca components:\n{round(comps, 3)}")
print("explained variance: {round(ev, 3)}")

small = vstack(randn(6, 2, seed = 41) + 3, randn(6, 2, seed = 42) - 3)
low = tsne(small, n_components = 2, perplexity = 3, seed = 1, max_iter = 100)
print("tsne output rows: {rows(low)}")
```

`G` already carries three well-separated blobs, so `eps = 1.5` finds them
without also finding noise; a smaller `eps` on the same data returns
mostly `-1`. `pca` on a two-column input is not reducing anything -- it is
here to show the three functions agree with each other -- the real use is
`k < d`.

## The fitted-model protocol

Every estimator fits with a constructor call and returns a handle: named
read-only fields, plus `predict`/`score` (and `update`/`estimate` for the
filters) reached through ordinary method sugar.

```qu
m = ols_model(X, y)
print(round(m.coef, 3))            # a field, not a method call
yhat = m.predict(Xnew)
print(round(m.score(Xnew, ynew), 4))
```

`predict`, `update`, `estimate`, `score`, `fit` and `pipeline` are each one
builtin that switches on the model's own `.kind`, not a separate builtin
per model type. That is why the same `predict` means "apply to new rows"
for a regression and "time update" for a Kalman filter.

### Constructors

| Function | Signature | Description |
|---|---|---|
| `ols_model` | `ols_model(X, y)` | Fits ordinary least squares with an automatic intercept on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y`. Returns a fitted model Record with fields `coef` (length-`D` slope vector), `intercept` (a number), `fitted` (length-`N` in-sample predictions), `residuals` (length-`N`), and `r2` (a number, the in-sample R²). |
| `ridge_model` | `ridge_model(X, y, alpha)` | The same fit as `ols_model` on an `N`-row, `D`-column `X` and a length-`N` `y`, but with L2 regularization strength `alpha` (a non-negative number) shrinking the coefficients toward zero; `ols_model` is this at `alpha = 0`. Returns a fitted model Record with the same fields as `ols_model`. |
| `kmeans_model` | `kmeans_model(X, k, [seed=])` | k-means as a reusable handle: fits on an `N`-row, `D`-column matrix `X` into `k` clusters (optional integer `seed=`), so `.predict(Xnew)` later assigns new rows to the nearest centroid rather than only labelling the fitting data. Returns a fitted model Record whose `.predict` gives a length-`M` integer label vector for an `M`-row `Xnew`. |
| `kmedians_model` | `kmedians_model(X, k, [seed=])` | The outlier-resistant sibling of `kmeans_model`, same `X`/`k`/`seed=` shape: clusters by L1 distance around coordinate-wise median centres instead of means. Returns a fitted model Record with a `.predict` of the same shape as `kmeans_model`. |
| `kmedoids_model` | `kmedoids_model(X, k, [seed=])` | PAM clustering on the same `X`/`k`/`seed=` shape as `kmeans_model` — every one of the `k` centres is an actual row of `X`, never an average. Returns a fitted model Record whose `.predict` gives a length-`M` integer label vector for an `M`-row input. |
| `gmm_model` | `gmm_model(X, k, [seed=], [max_iter=100])` | Gaussian mixture model with `k` components fitted by EM on an `N`-row, `D`-column matrix `X`, warm-started from a k-means run (optional integer `seed=`, and `max_iter` capping the EM iterations, default 100). Returns a fitted model Record with fields `means` (`(k, D)`), `covariances` (one `(D, D)` matrix per component), `weights` (length-`k` mixing proportions), and `log_likelihood` (a number, the total training log-likelihood). |
| `isolation_forest` | `isolation_forest(X, [n_trees=100], [max_samples=256], [contamination=], [seed=])` | Anomaly detection on an `N`-row, `D`-column matrix `X` by random partitioning (Liu, Ting & Zhou 2008): grows `n_trees` (integer, default 100) trees, each on its own subsample of `max_samples` rows (integer, default 256, capped at `N`) drawn without replacement, splitting on a uniformly random feature at a uniformly random cut point, and measures how few cuts it takes to isolate each row. It does not model "normal" and flag the misfits — a point in a sparse region is simply cut off sooner. `contamination` (optional number strictly between 0 and 1) is the expected fraction of anomalies and sets `.threshold` to the matching score quantile; without it the threshold is the paper's conventional `0.5`. An optional integer `seed=` fixes every draw. Returns a fitted model Record with fields `scores` (length-`N`, on `(0, 1)`, higher is more anomalous), `labels` (length-`N`, `1` anomalous / `0` normal), `threshold`, `n_trees`, `max_samples`, `n_features`, and the stored forest (`nodes`, `roots`). `.predict(Xnew)` gives the same score for `M` rows the forest was never fit on. Added in v0.3.0. |
| `gaussian_process` | `gaussian_process(X, y, [kernel="rbf"], [length_scale=], [sigma_f=1], [noise=1e-8])` | Gaussian-process regression on an `N`-row, `D`-column matrix `X` and a length-`N` target `y`. Unlike `ols_model`/`ridge_model` it reports how much it does not know at each input, which is the reason to use it: `.predict(Xnew, variance=true)` returns an `(M, 2)` matrix, column 0 the posterior mean and column 1 the posterior **variance** — small near the training rows, rising toward `sigma_f^2` far from all of them — while plain `.predict(Xnew)` returns just the length-`M` mean vector so a GP still drops into `pipeline`/`score` like any other regressor. `kernel` (string) is `"rbf"` (default, squared exponential) or `"matern32"` (once differentiable, better for data that is continuous but not glass-smooth). `length_scale` (number) defaults to the median pairwise distance in `X`, which keeps it in the data's own units instead of assuming order-1 inputs; `sigma_f` (number, default 1) is the prior standard deviation; `noise` (number, default 1e-8) is the observation-noise/jitter added to the kernel diagonal — raise it for noisy measurements. `y` is centered on its own mean before fitting and the mean is added back on prediction, so the posterior reverts to the data's level far from it, not to zero. The training covariance is factored once with a Cholesky decomposition, so prediction costs two triangular solves. Returns a fitted model Record with fields `x`, `y`, `l` (the Cholesky factor), `alpha`, `y_mean`, `kernel`, `length_scale`, `sigma_f`, `noise` (the level actually used, which may have been raised to keep the kernel matrix factorable), `log_marginal_likelihood`, `fitted`, `fitted_variance` and `n_features`. Added in v0.3.0. |
| `nmf` | `nmf(X, [n_components=], [max_iter=200], [tol=1e-4], [seed=])` | Non-negative matrix factorization `X ≈ W H` of an `N`-row, `D`-column **non-negative** matrix `X`, by Lee & Seung's multiplicative updates on the Frobenius objective (a negative or non-finite entry is a named error, not silently clipped). Because nothing may be negative, components can only add to a reconstruction and never cancel, so the factors read as parts that make up the whole rather than the signed directions `pca_model` returns — which is what makes it usable on spectra, concentrations and counts. `n_components` (integer) defaults to `min(N, D)`; `max_iter` (integer, default 200) caps the iterations and `tol` (number, default 1e-4) stops early once the relative improvement falls below it; an optional integer `seed=` fixes the random initialization. Returns a fitted model Record with fields `w` (`(N, k)`), `h` (`(k, D)`), `n_components`, `n_iter`, `reconstruction_error` (the final `‖X − WH‖_F`), `errors` (its value at every iteration, non-increasing by construction) and `n_features`. `.predict(Xnew)` returns the `(M, k)` non-negative encoding of new rows against the fitted `H` — a transform, not a reconstruction; multiply by `.h` to approximate `Xnew` itself. Added in v0.3.0. |
| `pca_model` | `pca_model(X, k)` | PCA as a reusable handle: fits the top `k` principal components on an `N`-row, `D`-column matrix `X`, so `.predict(Xnew)` projects new rows onto the same fitted components instead of refitting. Returns a fitted model Record whose `.predict` gives an `(M, k)` matrix for an `M`-row `Xnew`. |
| `tree_model` | `tree_model(X, y, [max_depth=20], [min_samples_split=2], [kind=])` | One CART decision tree on an `N`-row, `D`-column matrix `X` and a length-`N` target `y`, with `max_depth` (integer, default 20) limiting tree depth, `min_samples_split` (integer, default 2) the minimum node size to keep splitting, and `kind` a string, `"regression"` or `"classification"` (never inferred — must be given explicitly when it matters). Splits by variance reduction for regression, Gini impurity for classification. Returns a fitted model Record with a `.predict`/`.score`. |
| `random_forest_model` | `random_forest_model(X, y, n_trees, [max_depth=20], [seed=], [kind=])` | Bagged ensemble of `n_trees` (a positive integer) CART trees on an `N`-row, `D`-column `X` and length-`N` `y`, each tree grown on its own bootstrap resample of the rows and a `ceil(sqrt(D))`-column feature subsample at each split, `max_depth` per tree (default 20), an optional integer `seed=` that reproducibly seeds every tree's own draw, and `kind` (`"regression"` or `"classification"`, never inferred). Returns a fitted model Record with `.predict`/`.score`. |
| `gradient_boosting_model` | `gradient_boosting_model(X, y, n_trees, [learning_rate=0.1], [max_depth=3])` | Regression-only boosted ensemble on an `N`-row, `D`-column `X` and length-`N` `y`: `n_trees` (positive integer) shallow trees fitted in sequence, each to the running residual, scaled by `learning_rate` (a number, default 0.1) and capped at `max_depth` (default 3, shallower than `tree_model`'s 20 because a boosting weak learner is supposed to be weak). Returns a fitted model Record with `.predict`/`.score`. |
| `svm_model` | `svm_model(X, y, [kernel=], [C=1.0], [gamma=], [degree=3], [coef0=1])` | Binary-classification support-vector machine by simplified SMO on an `N`-row, `D`-column `X` and a length-`N` target `y` that must take exactly two distinct values (no one-vs-rest wrapper). `kernel` is a string, one of `"linear"`, `"rbf"`, `"poly"`; `C` (number, default 1.0) is the regularization strength; `gamma` (number) scales the `"rbf"`/`"poly"` kernels; `degree` (integer, default 3) and `coef0` (number, default 1) apply to `"poly"`. Returns a fitted model Record with `.predict`/`.score`. |
| `knn_model` | `knn_model(X, y, k, [metric=], [kind=])` | k-nearest-neighbours on an `N`-row, `D`-column `X` and a length-`N` target `y`, with integer neighbour count `k`, an optional `metric` string (distance function name), and `kind` (`"regression"` or `"classification"`, never inferred). Fitting only stores `X`/`y`; all the work happens inside `.predict`, which returns a length-`M` vector for an `M`-row query matrix. Returns a fitted `Model` of kind `"knn"` — the handle `.predict`/`.score` are called on. |
| `naive_bayes_model` | `naive_bayes_model(X, y)` | Gaussian naive Bayes classifier on an `N`-row, `D`-column `X` and a length-`N` categorical/integer target `y`, scored in log space so a product of small per-feature densities cannot underflow. Returns a fitted model Record whose `.predict(Xnew)` gives a length-`M` label vector; it defines `predict` but not `score`. |
| `logistic_model` | `logistic_model(X, y)` | Binary logistic regression on an `N`-row, `D`-column `X` and a length-`N` 0/1 target `y`. Returns a fitted model Record with `.predict`/`.score`, `.score` giving classification accuracy. |
| `svr_model` | `svr_model(X, y)` | Support-vector regression on an `N`-row, `D`-column `X` and a length-`N` numeric target `y`. Returns a fitted model Record with `.predict`/`.score`, `.score` giving R². |

Four things about this table are easy to get wrong:

- **`kind=` is never inferred.** A 0/1 target and a small-integer count
  target look identical to the type system, so `tree_model` and friends
  make you say `kind = "classification"` when that is what you mean.
- **`gradient_boosting_model` defaults to `max_depth = 3`**, shallower
  than `tree_model`'s 20. A boosting weak learner is supposed to be weak.
- **`svm_model` is binary only.** `y` must have exactly two distinct
  values; there is no one-vs-rest wrapper.
- **One `seed=` reproduces a whole forest.** It seeds a master stream that
  each tree draws its own seed from, so the forest is deterministic and no
  two trees share a stream.

```qu
Xc = randn(200, 2, seed = 41)
yc = zeros(200)
for i in 0 to 199
    if Xc[i, 0] + Xc[i, 1] > 0
        yc[i] = 1
    end if
end for
s = train_test_split(Xc, yc, test_size = 0.3, seed = 9)

function accuracy(model)
    yhat = model.predict(s.X_test)
    hit = yhat == s.y_test
    return len(yhat[hit]) / len(yhat)
end function

names = ("knn", "tree", "forest", "bayes")
scores = (
    accuracy(knn_model(s.X_train, s.y_train, 5, kind = "classification")),
    accuracy(tree_model(s.X_train, s.y_train, kind = "classification")),
    accuracy(random_forest_model(s.X_train, s.y_train, 40, seed = 3, kind = "classification")),
    accuracy(naive_bayes_model(s.X_train, s.y_train))
)
for i in 0 to 3
    print("{names[i]}: {round(scores[i], 3)}")
end for
bar(0 to 3, scores, color = "#4169e1", values = true)
xticks(0 to 3)
xticklabels(names)
ylim(0, 1)
ylabel("test accuracy")
title("held-out accuracy, same split")
```

Accuracy is computed here rather than taken from `.score` because
`naive_bayes` defines `predict` and not `score`, and a comparison where one
model is measured differently from the others is not a comparison. The
boundary is a straight line, which is why the two that can draw one do
best and the forest's extra capacity buys nothing.

The remaining constructors round out the table: two more clustering
handles, a mixture model, a boosted regressor and two more binary
classifiers.

```qu
km = kmedians_model(G, 3, seed = 5)
kd = kmedoids_model(G, 3, seed = 5)
gm = gmm_model(G, 3, seed = 5)
print("kmedians label for row 0: {km.predict(G)[0]}")
print("kmedoids label for row 0: {kd.predict(G)[0]}")
print("gmm log-likelihood: {round(gm.log_likelihood, 1)}")

gb = gradient_boosting_model(X, y, 20, learning_rate = 0.1, max_depth = 3)
print("gradient boosting r2: {round(gb.score(X, y), 3)}")

sv = svm_model(Xc, yc, kernel = "linear", C = 1.0)
lg = logistic_model(Xc, yc)
print("svm accuracy:      {round(sv.score(Xc, yc), 3)}")
print("logistic accuracy: {round(lg.score(Xc, yc), 3)}")

svr = svr_model(X, y)
print("svr r2: {round(svr.score(X, y), 3)}")
```

`kmedoids_model` picks its centres from among the rows it was given, so
`kd.predict` can only ever return a label matching an actual data point's
neighbourhood -- never an averaged one the way `kmeans_model` would.

#### Overloads: `tree_model`

#### Case: regression

`kind = "regression"` splits each node by variance reduction and predicts
the mean of a leaf's training targets -- a continuous number.

```qu
Xtr = randn(80, 2, seed = 53)
ytr = Xtr[:, 0] * 3 + Xtr[:, 1] * -2 + 0.1 * randn(80, seed = 55)
Xte = randn(20, 2, seed = 54)
yte = Xte[:, 0] * 3 + Xte[:, 1] * -2
trm = tree_model(Xtr, ytr, max_depth = 4, kind = "regression")
print("tree_model regression R2 on new data: {round(trm.score(Xte, yte), 3)}")
```

#### Case: classification

`kind = "classification"` splits each node by Gini impurity instead and
predicts the majority class among a leaf's training labels.

```qu
Xtrc = randn(80, 2, seed = 56)
ytrc = zeros(80)
for i in 0 to 79
    if Xtrc[i, 0] + Xtrc[i, 1] > 0
        ytrc[i] = 1
    end if
end for
Xtec = randn(20, 2, seed = 57)
ytec = zeros(20)
for i in 0 to 19
    if Xtec[i, 0] + Xtec[i, 1] > 0
        ytec[i] = 1
    end if
end for
tcm = tree_model(Xtrc, ytrc, max_depth = 4, kind = "classification")
print("tree_model classification accuracy on new data: {round(tcm.score(Xtec, ytec), 3)}")
```

`random_forest_model` takes the same `kind=` and branches the same way,
tree by tree.

#### Overloads: `svm_model`

#### Case: linear

`kernel = "linear"` fits a straight decision boundary -- fast, but blind
to a relationship that isn't linearly separable.

```qu
Xx = [0.0, 0.0; 0.0, 1.0; 1.0, 0.0; 1.0, 1.0; 0.1, 0.1; 0.1, 0.9; 0.9, 0.1; 0.9, 0.9]
Yx = [0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]
svl = svm_model(Xx, Yx, kernel = "linear", C = 1.0)
print("linear kernel accuracy on XOR: {round(svl.score(Xx, Yx), 3)}")
```

#### Case: rbf

`kernel = "rbf"` maps into an implicit, much higher-dimensional space
through a Gaussian kernel scaled by `gamma`, so it can separate a boundary
no straight line can.

```qu
Xx = [0.0, 0.0; 0.0, 1.0; 1.0, 0.0; 1.0, 1.0; 0.1, 0.1; 0.1, 0.9; 0.9, 0.1; 0.9, 0.9]
Yx = [0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]
svr = svm_model(Xx, Yx, kernel = "rbf", gamma = 2.0, C = 1.0)
print("rbf kernel accuracy on XOR:    {round(svr.score(Xx, Yx), 3)}")
```

#### Case: poly

`kernel = "poly"` raises the dot product to `degree` (shifted by
`coef0`) -- a middle ground between `"linear"` and `"rbf"`.

```qu
Xx = [0.0, 0.0; 0.0, 1.0; 1.0, 0.0; 1.0, 1.0; 0.1, 0.1; 0.1, 0.9; 0.9, 0.1; 0.9, 0.9]
Yx = [0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]
svp = svm_model(Xx, Yx, kernel = "poly", degree = 3, coef0 = 1.0, C = 1.0)
print("poly kernel accuracy on XOR:   {round(svp.score(Xx, Yx), 3)}")
```

#### Overloads: `knn_model`

#### Case: regression

`kind = "regression"` predicts the mean of the `k` nearest neighbours'
target values -- a number that can fall between any two training targets.

```qu
Xk = [0.0; 1.0; 2.0; 3.0]
yk = [10.0, 12.0, 30.0, 32.0]
knr = knn_model(Xk, yk, 2, kind = "regression")
q = [1.5] as matrix(1, 1)
print("knn_model regression predict(1.5): {knr.predict(q)}")   # mean of the 2 nearest y's
```

#### Case: classification

`kind = "classification"` predicts the majority label among the `k`
nearest neighbours instead -- always one of the training labels, never an
average.

```qu
Xkc = [0.0; 1.0; 2.0; 3.0]
ykc = [0, 0, 1, 1]
knc = knn_model(Xkc, ykc, 3, kind = "classification")
q = [1.5] as matrix(1, 1)
print("knn_model classification predict(1.5): {knc.predict(q)}")   # majority vote among 3 nearest labels
```

### The shared verbs

| Verb | Signature | Description |
|---|---|---|
| `predict` | `model.predict(Xnew)` | Applies a fitted model Record `model` to a new `M`-row (same `D` columns as it was fitted on) matrix or vector `Xnew`. Returns a length-`M` prediction vector for a supervised or clustering model. For a Kalman or particle filter, called instead as `state.predict(...)` with filter-specific arguments, this is the time-update step, returning a new filter state Record rather than a prediction vector. |
| `score` | `model.score(X, [y])` | Scores a fitted model Record `model` against an `N`-row feature matrix `X` and, for a supervised kind, the matching length-`N` target vector `y`. Returns a single number: R² for a regression, accuracy for a classifier, negative inertia (within-cluster sum of squared distances) for k-means, an explained-variance sum for PCA, or total log-likelihood for a GMM; `y` is required for a supervised kind and refused for a clustering one. |
| `estimate` | `state.estimate()` | Estimation-family only: reads a point estimate off a filter state Record `state` with no arguments. Returns the particle filter's weighted-mean particle vector, or a Kalman/EKF/UKF filter's `state.x` vector directly. |
| `update` | `state.update(...)` | Estimation-family only: the measurement-update half of a filter, called on a state Record `state` with filter-specific arguments (an observation matrix/function, a measurement vector, and a noise covariance, see the Estimation section below). Returns a new filter state Record; nothing is mutated in place. |
| `pipeline` | `pipeline(stage1, ..., finalStage)` | Composes one or more stage names (strings naming functions already defined in the program) into a single unfitted pipeline spec — every stage but the last must be a function `(X) -> X`; the last must be `(X, y) -> Model`. Returns an unfitted pipeline Record ready for `.fit`. |
| `fit` | `pipe.fit(X, y)` | Runs a pipeline Record `pipe` (from `pipeline(...)`) on an `N`-row feature matrix `X` and a length-`N` target vector `y`: applies every transform stage in order, then calls the final estimator stage. Returns a fitted pipeline Record whose `.predict`/`.score` replay the same transform stages before delegating to the fitted model inside. |

`score` needs `y` for a supervised kind and refuses it for a clustering
one; the GMM returns a total log-likelihood, not scikit-learn's
mean-per-sample. Stages are named by string, the same convention `pmap`
and `spawn` use.

#### Overloads: `predict`

#### Case: supervised or clustering model

`model.predict(Xnew)` returns a length-`M` vector -- one prediction per
row of `Xnew`.

```qu
Xp = randn(30, 2, seed = 71)
yp = Xp * [1.0, -1.0] + 0.05 * randn(30, seed = 72)
mp = ols_model(Xp, yp)
print("model .predict returns a prediction vector: {round(mp.predict(Xp[0:1, :]), 3)}")
```

#### Case: estimation filter state

`state.predict(...)` on a Kalman/EKF/UKF/particle state instead returns a
whole new state Record -- the time-update step, not a prediction vector.
The Estimation section below covers the filter-specific argument shapes.

```qu
s0 = kalman_init([0.0, 0.0], [1.0, 0, 0, 1.0] as matrix(2, 2))
F = [1, 0, 1, 1] as matrix(2, 2)
Q = [0.01, 0, 0, 0.01] as matrix(2, 2)
s1 = s0.predict(F, Q)
print("filter .predict returns a new state Record; its x field: {round(s1.x, 3)}")
```

#### Overloads: `score`

#### Case: regression

On a regression model (`ols_model`, `ridge_model`, `svr_model`, ...),
`.score(X, y)` returns R².

```qu
Xr = randn(60, 2, seed = 61)
beta = [1.5, -0.7]
yr = Xr * beta + 0.05 * randn(60, seed = 62)
om = ols_model(Xr, yr)
print("regression score is R2: {round(om.score(Xr, yr), 4)}")
```

#### Case: classification

On a classifier (`logistic_model`, `tree_model(kind="classification")`,
...), `.score(X, y)` returns accuracy instead.

```qu
Xc2 = randn(60, 2, seed = 63)
yc2 = zeros(60)
for i in 0 to 59
    if Xc2[i, 0] + Xc2[i, 1] > 0
        yc2[i] = 1
    end if
end for
lgm = logistic_model(Xc2, yc2)
print("classification score is accuracy: {round(lgm.score(Xc2, yc2), 4)}")
```

#### Case: k-means

On a `kmeans_model`, `.score(X)` takes no `y` and returns negative inertia
(the negated within-cluster sum of squared distances -- negative so that
"higher is better" still holds).

```qu
Gs = vstack(randn(20, 2, seed = 64) + 4, randn(20, 2, seed = 65) - 4)
km = kmeans_model(Gs, 2, seed = 5)
print("kmeans score is negative inertia: {round(km.score(Gs), 4)}")
```

#### Case: PCA

On a `pca_model`, `.score(X)` returns the summed explained-variance
fraction of the fitted components.

```qu
Gs = vstack(randn(20, 2, seed = 64) + 4, randn(20, 2, seed = 65) - 4)
pm = pca_model(Gs, 1)
print("pca score is summed explained variance: {round(pm.score(Gs), 4)}")
```

#### Case: GMM

On a `gmm_model`, `.score(X)` returns the total log-likelihood of `X`
under the fitted mixture.

```qu
Gs = vstack(randn(20, 2, seed = 64) + 4, randn(20, 2, seed = 65) - 4)
gm = gmm_model(Gs, 2, seed = 5)
print("gmm score is total log-likelihood: {round(gm.score(Gs), 2)}")
```

## Recurrent cells: LSTM

Part of the tracked-tensor family with `dense`, `relu`, `conv2d` and
`gru_cell`: a weight passed through `param(...)` gets a real gradient from
`grad(loss, wrt=...)`, backpropagated through time by the ordinary tape. No
separate recurrent machinery.

| Function | Signature | Description |
|---|---|---|
| `lstm_init` | `lstm_init(input_size, hidden_size, [seed=])` | Builds fresh LSTM weights for an input of size `input_size` (integer, number of features per timestep) and a hidden state of size `hidden_size` (integer): Xavier-scaled weight matrices and zero biases for all four gates (forget, input, output, candidate), plus an optional integer `seed=`. Returns a plain, untracked Record with fields `wf`, `uf`, `bf`, `wi`, `ui`, `bi`, `wo`, `uo`, `bo`, `wg`, `ug`, `bg` — wrap the ones you want trained in `param(...)` before use. |
| `lstm_cell` | `lstm_cell(x_t, h_prev, c_prev, weights)` | One LSTM timestep: `x_t` (length-`input_size` input vector), `h_prev`/`c_prev` (length-`hidden_size` previous hidden/cell state vectors), and `weights` (a Record with the twelve gate fields `lstm_init` produces, `param`-wrapped where you want gradients). Returns a Record `{h, c}` — two length-`hidden_size` states thread through an LSTM, unlike GRU's single `h`. |
| `lstm_forward` | `lstm_forward(x_sequence, h0, c0, weights)` | Unrolls `lstm_cell` over a whole sequence: `x_sequence` an `(input_size, seq_len)` matrix (one column per timestep), `h0`/`c0` length-`hidden_size` initial states, `weights` the same gate-weights Record as `lstm_cell`. Returns a length-`seq_len` list of length-`hidden_size` hidden-state vectors, one per timestep; the cell state threads through internally and is not returned. |

`lstm_forward` is what you reach for when you have a whole sequence up
front and just want the hidden states out the other end; it is also how
gradients flow back through time here — because every step reuses the
*same* tracked weight Tensors, `grad` sees the whole unrolled computation
as one graph and backpropagates through every timestep in one call. The
example below runs a 3-step sequence through a freshly initialized LSTM,
computes a toy loss on the last hidden state, and takes the gradient with
respect to one of the twelve weight matrices:

```qu
raw = lstm_init(2, 3, seed = 1)
w = {wf=param(raw.wf), uf=param(raw.uf), bf=param(raw.bf),
     wi=param(raw.wi), ui=param(raw.ui), bi=param(raw.bi),
     wo=param(raw.wo), uo=param(raw.uo), bo=param(raw.bo),
     wg=param(raw.wg), ug=param(raw.ug), bg=param(raw.bg)}
Xseq = [0.5, -0.3, 0.8; 0.2, 0.4, -0.1]   # (input_size, seq_len)
hs = lstm_forward(Xseq, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], w)
h_last = hs[length(hs) - 1]
loss = sum(h_last .^ 2)
g = grad(loss, wrt = w.wf)                # through time, on the same tape
print("loss: {round(loss, 4)}")
print("dL/dwf: {round(g, 4)}")
```

`lstm_cell` is the single timestep `lstm_forward` unrolls — useful on its
own when you are stepping a sequence by hand, e.g. inside a control loop
that needs to look at `h`/`c` between steps:

```qu
w1 = {wf=0.5, uf=0.5, bf=0.0, wi=0.3, ui=0.2, bi=0.0,
      wo=0.4, uo=0.1, bo=0.0, wg=0.6, ug=0.3, bg=0.0}
hc = lstm_cell(1.0, 0.0, 0.0, w1)
print("h = {round(hc.h, 4)}, c = {round(hc.c, 4)}")
```

## Recurrent cells: GRU

The gated recurrent unit: fewer gates than an LSTM (one hidden state, not
two), so `gru_cell` returns `h` directly rather than a `{h, c}` record.

| Function | Signature | Description |
|---|---|---|
| `gru_init` | `gru_init(input_size, hidden_size, [seed=])` | Builds fresh GRU weights for an input of size `input_size` (integer) and a hidden state of size `hidden_size` (integer): Xavier-scaled weight matrices and zero biases for the update, reset and candidate gates, plus an optional integer `seed=`. Returns a plain, untracked Record with fields `wz`, `uz`, `bz`, `wr`, `ur`, `br`, `wh`, `uh`, `bh` — wrap the ones you want trained in `param(...)`. |
| `gru_cell` | `gru_cell(x_t, h_prev, weights)` | One GRU timestep: `x_t` (length-`input_size` input vector), `h_prev` (length-`hidden_size` previous hidden state), `weights` (a Record with the nine gate fields `gru_init` produces). Returns the length-`hidden_size` new hidden state vector `h` directly, not a record — a GRU has no separate cell state to carry alongside it. |
| `gru_forward` | `gru_forward(x_sequence, h0, weights)` | Unrolls `gru_cell` over a whole sequence: `x_sequence` an `(input_size, seq_len)` matrix, `h0` a length-`hidden_size` initial hidden state, `weights` the same Record `gru_cell` takes. Returns a length-`seq_len` list of length-`hidden_size` hidden-state vectors, one per timestep. |

The example below mirrors the LSTM one: unroll a GRU over a short
sequence with `gru_forward`, take a gradient of a toy loss on the last
hidden state back through every timestep, then reset the tape and show
`gru_cell` stepping a single timestep by hand:

```qu
raw = gru_init(2, 3, seed = 1)
w = {wz=param(raw.wz), uz=param(raw.uz), bz=param(raw.bz),
     wr=param(raw.wr), ur=param(raw.ur), br=param(raw.br),
     wh=param(raw.wh), uh=param(raw.uh), bh=param(raw.bh)}
Xseq = [0.5, -0.3, 0.8; 0.2, 0.4, -0.1]   # (input_size, seq_len)
hs = gru_forward(Xseq, [0.0, 0.0, 0.0], w)
h_last = hs[length(hs) - 1]
print("gru_forward last hidden state: {round(h_last, 4)}")

loss = sum(h_last .^ 2)
g = grad(loss, wrt = w.wz)
print("dL/dwz: {round(g, 4)}")
tape_reset()

h1 = gru_cell([1.0], [1.0], {wz=0.5, uz=0.5, bz=0.0, wr=0.3, ur=0.3, br=0.0, wh=0.2, uh=0.2, bh=0.0})
print("gru_cell one step: {round(h1, 4)}")
```

## Local LLM inference (experimental)

Only in builds with `--features llm`. Runs on-device, CPU only, no API key.
This build of the engine was compiled without that feature, so the two
functions below are documented but not exercised by a runnable example
here — calling either one on a plain build fails with a message naming the
missing feature flag, the same pattern `save_model`/`load_model` use for
`--features h5-models` further down this chapter.

| Function | Signature | Description |
|---|---|---|
| `llm_load` | `llm_load(name)` | Loads a chat model, identified by `name`: a string, either a bundled name like `"tinyllama"` or a local filesystem path to a `.gguf` weights file with a `tokenizer.json` beside it. Returns a model handle Record with a `.generate` method; loading is eager, so the call blocks until the whole model is read into memory. |
| `generate` | `model.generate(prompt, [max_tokens=64])` | Runs greedy (argmax, no sampling knobs, no temperature) text completion on a model handle `model` from `llm_load`, given a string `prompt` and an integer cap `max_tokens` (default 64) on how many tokens to generate. Returns the completion as a plain string. There is no streaming and no hidden conversation state — each call is independent, so a multi-turn chat has to concatenate prior turns into `prompt` itself. |

## Feedforward layers and activations

The plain building blocks every network above is made from. All of them
are tracked-tensor aware: pass a `param(...)`-wrapped weight and `grad`
sees through them.

| Function | Signature | Description |
|---|---|---|
| `param`, `track` | `param(v)` / `track(v)` | Wraps a plain number, vector, or matrix `v` as a fresh, differentiable Tensor — a new leaf node on the autodiff tape with no inputs of its own, so `grad` treats it as something to differentiate *with respect to* rather than something computed. `param` and `track` do exactly the same thing; the two names exist so a program can say which values are trainable weights (`param`) versus intermediate values you also want a gradient through (`track`) — a note to the reader, not a mechanical difference. Returns a Tensor of the same shape as `v` (a bare vector is promoted to a column matrix, matching `to_matrix`'s convention). |
| `dense` | `dense(x, w, b)` | One fully-connected layer on an `N`-row, `D`-column input matrix `x`, a `(D, H)` weight matrix `w`, and a length-`H` bias vector `b`: computes `x * w + b`, broadcasting `b` over every row. Returns an `(N, H)` matrix. |
| `relu`, `sigmoid` | `relu(x)` | Elementwise activation applied to a number, vector, or matrix `x` of any shape — `relu` clips negative values to zero, `sigmoid` squashes to `(0, 1)`. Returns a value of the same shape as `x`, differentiable through the tape. |
| `softmax_rows` | `softmax_rows(M)` | Softmax along each row of an `N`-row, `D`-column matrix `M` of scores, shifted by the row maximum first so a large score cannot overflow the exponential. Returns an `(N, D)` matrix where each row sums to 1. |
| `dropout` | `dropout(x, rate)` | Zeroes a random share of the entries of `x` (a vector or matrix of any shape) at probability `rate` (a number in `[0, 1)`), and scales the surviving entries up by `1 / (1 - rate)`, so the expected value of the output equals `x`. Returns a value the same shape as `x`, freshly randomized on every call. |
| `tape_reset` | `tape_reset()` | Discards every entry recorded so far on the autodiff tape, freeing the memory a chain of `param`/`grad` calls has built up. Takes no arguments; call it between independent computations, since a program that never calls it keeps every tracked operation alive for as long as it runs. Returns `Nothing`. |

The next example threads a small computation through every piece here at
once: a `dense` layer feeds `relu` then `sigmoid`, `grad` differentiates a
toy loss with respect to the weight matrix, `tape_reset` clears that
computation before an unrelated `softmax_rows` call, and a final block
shows `dropout` zeroing roughly half of eight ones and rescaling the rest.

```qu
w = param([0.5, -0.3; 0.2, 0.8])
b = param([0.1, -0.1])
x = [1.0, 2.0; -0.5, 1.5]      # two examples, two features each
z = dense(x, w, b)
a = relu(z)
p = sigmoid(a)
loss = sum(p .^ 2)
g = grad(loss, wrt = w)
print("relu:    {round(a, 4)}")
print("sigmoid: {round(p, 4)}")
print("dL/dw:   {round(g, 4)}")
tape_reset()                    # clear the tape before the next computation

scores = [2.0, 1.0, 0.1; 0.5, 0.5, 0.5]
probs = softmax_rows(scores)
print("softmax row 0: {round(probs[0, :], 4)}")

seed(3)
kept = dropout(ones(1, 8), 0.5)
print("dropout output: {kept}")
print("survivors:      {sum(kept > 0)} of 8, each scaled by 1/(1-rate)")
```

## Convolution and pooling

`conv1d` slides one learnable kernel over a signal; `conv2d` does the same
over an image. `maxpool2d`/`avgpool2d` downsample by window.

| Function | Signature | Description |
|---|---|---|
| `conv1d` | `conv1d(x, kernel, [stride=], [padding=])` | Slides a length-`K` learnable `kernel` vector over a length-`N` signal `x`, moving `stride` positions at a time (integer, default 1) with `padding` (`"valid"` — no padding, output shrinks — or `"same"` — zero-padded to keep the output length equal to `N`; default `"valid"`). Returns the length-`M` convolved output vector, `M` depending on `N`, `K`, `stride`, and `padding`. |
| `conv2d` | `conv2d(x, kernel, [stride], [padding])` | Slides a `(Kh, Kw)` learnable `kernel` matrix over an `(H, W)` image matrix `x`, moving `stride` positions at a time (integer, default 1) with `padding` — `"valid"` (no padding) or `"same"` (output keeps `x`'s `(H, W)` shape); default `"valid"`. Returns an `(H', W')` convolved output matrix. |
| `maxpool2d` | `maxpool2d(x, size, [stride=])` | Downsamples an `(H, W)` matrix `x` by taking the largest value in each `size` x `size` window (integer `size`), moving `stride` positions at a time (integer, defaults to `size` — non-overlapping windows). Returns a smaller `(H', W')` matrix. |
| `avgpool2d` | `avgpool2d(x, size, [stride=])` | Downsamples an `(H, W)` matrix `x` the same way as `maxpool2d` — `size` x `size` windows, `stride` defaulting to `size` — but takes the mean of each window instead of the max. Returns a smaller `(H', W')` matrix. |

The four functions below cover both dimensionalities you meet in
practice: `conv1d` for a signal, `conv2d` for an image, and the two
pooling functions for shrinking a feature map afterward. The example
convolves a short signal with an edge-detecting kernel, convolves a small
image with a diagonal-edge kernel, and then shows `maxpool2d`/`avgpool2d`
shrinking the same image by taking the largest and the average value in
each 2x2 block.

```qu
x1d = [1.0, 2.0, 3.0, 4.0, 5.0]
edge_kernel = [1.0, 0.0, -1.0]
y1d = conv1d(x1d, edge_kernel)
print("conv1d edge response: {y1d}")

img = [1, 5, 2, 8; 4, 3, 9, 1; 7, 2, 6, 0; 1, 1, 1, 1]
k2d = [1, 0; 0, -1]
y2d = conv2d(img, k2d, 1, "valid")
print("conv2d output:\n{y2d}")

print("maxpool2d: {maxpool2d(img, 2)}")
print("avgpool2d: {avgpool2d(img, 2)}")
```

## Layer normalization and attention

The transformer building blocks: `layer_norm` normalizes each row of its
input independently (unlike batch norm, it needs no other rows in the
batch to work); `scaled_dot_product_attention` is the core attention
computation; `multi_head_attention` runs several of those in parallel over
learned projections; `transformer_block` wraps attention and a
feed-forward layer, each behind a residual connection and a layer norm.

| Function | Signature | Description |
|---|---|---|
| `layer_norm` | `layer_norm(x, gamma, beta, [eps=])` | Normalizes each row of an `N`-row, `D`-column matrix `x` independently to zero mean and unit variance, then applies a learned length-`D` per-feature scale `gamma` and shift `beta`, with `eps` (a small positive number, default a tiny constant) added inside the variance's square root for numerical stability. Returns an `(N, D)` matrix, the same shape as `x`. |
| `scaled_dot_product_attention` | `scaled_dot_product_attention(q, k, v, [mask=])` | Core attention computation: an `(Nq, D)` query matrix `q`, an `(Nk, D)` key matrix `k`, and an `(Nk, Dv)` value matrix `v` (`k` and `v` share the same row count `Nk`). Scores are `q * kᵀ` scaled by `1/sqrt(D)` before the softmax, so the softmax does not saturate as `D` grows; an optional `mask` matrix (same shape as the score matrix, `(Nq, Nk)`) adds large negative values to positions that should not be attended to before the softmax. Returns an `(Nq, Dv)` matrix. |
| `multi_head_attention` | `multi_head_attention(x, heads, n_heads)` | Runs several attention heads over the same `(N, D)` input `x` in parallel: `heads` is a tuple of `n_heads` (integer) Records, each with fields `wq`, `wk`, `wv` (per-head projection matrices) and `wo` (the head's output projection), and self-attention (`q = k = v = x` projected through each head's own weights) is computed per head, then the results are combined through each head's `wo` and summed. Returns an `(N, D)` matrix. |
| `transformer_block` | `transformer_block(x, weights)` | One transformer block on an `(N, D)` input `x`: multi-head self-attention plus a two-layer feed-forward network, each wrapped in a residual connection and a `layer_norm`. `weights` is a Record with fields `heads` (the tuple `multi_head_attention` expects), `ln1_gamma`/`ln1_beta` and `ln2_gamma`/`ln2_beta` (length-`D` layer-norm parameters for the two sublayers), and `w1`/`b1`/`w2`/`b2` (the feed-forward layer's weights and biases). Returns an `(N, D)` matrix, the same shape as `x`. |

The example below builds up from the smallest piece to the whole block:
`layer_norm` on a two-row matrix with very different scales per row shows
that each row is normalized independently of the other; a hand-built
`q`/`k`/`v` triple runs through `scaled_dot_product_attention` directly;
wrapping the identity matrix as a single attention head shows that
`multi_head_attention` with one identity head reduces to plain
self-attention, which is a useful sanity check when you are debugging a
custom set of head weights; and a full `transformer_block` runs the same
`q` through attention, a feed-forward layer, and two layer norms in one
call.

```qu
x = [1.0, 2.0, 3.0; 10.0, 20.0, 30.0]
normed = layer_norm(x, [1.0, 1.0, 1.0], [0.0, 0.0, 0.0])
print("layer_norm (scale-invariant per row): {normed}")

q = [1.0, 0.5; -0.5, 1.0; 0.2, -0.3]
k = [1.0, 0.0; 0.0, 1.0; 0.5, 0.5]
v = [1.0, 2.0; 3.0, 4.0; 5.0, 6.0]
attn = scaled_dot_product_attention(q, k, v)
print("attention output: {round(attn, 3)}")

eye2 = [1.0, 0.0; 0.0, 1.0]
head = {wq = eye2, wk = eye2, wv = eye2, wo = eye2}
mh = multi_head_attention(q, (head,), 1)
plain = scaled_dot_product_attention(q, q, q)
print("multi_head_attention with 1 identity head matches plain self-attention: {sum(abs(mh - plain)) < 1e-9}")

weights = {heads = (head,), ln1_gamma = [1.0, 1.0], ln1_beta = [0.0, 0.0],
           w1 = [0.2, -0.1; 0.1, 0.3], b1 = [0.0, 0.0],
           w2 = [0.1, -0.2; 0.3, 0.1], b2 = [0.0, 0.0],
           ln2_gamma = [1.0, 1.0], ln2_beta = [0.0, 0.0]}
block_out = transformer_block(q, weights)
print("transformer_block: {round(block_out, 3)}")
```

## Training loops and optimizers

Three ways to train, from least to most control. `quick_mlp` fits a whole
network in one call. `compile`/`.fit` builds a `sequential` pipeline and
trains it with a chosen optimizer. `train_loop`/`optimizer_step` run
underneath both, and are also callable directly when you are writing the
loop yourself.

| Function | Signature | Description |
|---|---|---|
| `compile` | `compile(pipeline, [optimizer=], [loss=])` | Attaches an optimizer Record (from `sgd`/`adam`/etc., default `adam()`) and a loss name string (`"mse"` or `"cross_entropy"`, default `"mse"`) to an `input(n) \|> dense(...) \|> ...` pipeline Record (built by piping through `input`/`dense`, see the layer-spec subsection below). Returns a fitted-model-ready `kind="sequential"` Model whose weights are already initialized (but not yet trained) — the same Model `sequential(...)` itself returns. Returns a `Model` of kind `"sequential"`. |
| `train_loop` | `train_loop(loss_fn_name, params, epochs, optimizer=)` | Runs the fit loop inside the engine rather than stepping it from Qu: `loss_fn_name` a string naming an in-scope function `(params) -> number`, `params` the Tensor (from `param(...)`) or Record of Tensors to optimize, `epochs` an integer number of iterations, and `optimizer=` an optimizer Record (from `sgd`/`adam`/etc., referenced here by the string name of the variable holding it). Returns a length-`epochs` vector of the loss value after each epoch. |
| `optimizer_step` | `optimizer_step(opt, grads)` | One optimizer update, for when you are writing the training loop yourself instead of calling `train_loop`: `opt` an optimizer Record (from `sgd`/`adam`/etc.), `grads` the gradient value (same shape as the parameter being optimized) from `grad(...)`. Returns a new optimizer Record with the updated parameter value in its `._params` field and any internal state (momentum, running averages) advanced by one step. |
| `sgd`, `nesterov_sgd` | `sgd(params, [lr=0.01])` | Plain stochastic gradient descent (`sgd`) and its look-ahead variant (`nesterov_sgd`, which evaluates the gradient after applying the momentum step rather than before it), given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.01; `nesterov_sgd` also takes a `momentum=` number). Returns an optimizer Record ready for `optimizer_step` or `optimizer=`. Called with no positional argument at all (e.g. `adam(lr=0.01)`) any of these instead returns a spec-only "recipe" Record for passing as `optimizer=` to `compile`/`train_loop`, without binding to a specific parameter yet. |
| `adam`, `adamw`, `nadam`, `adamax` | `adam(params, [lr=0.001])` | The Adam family of per-parameter adaptive-learning-rate optimizers, given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.001). `adamw` decouples weight decay from the gradient update; `nadam` folds in Nesterov-style look-ahead momentum; `adamax` swaps Adam's second-moment estimate for an infinity norm. Returns an optimizer Record, or (with no `params`) a recipe Record for `optimizer=`. |
| `rmsprop`, `adagrad`, `adadelta` | `rmsprop(params, [lr=0.001])` | Per-parameter learning rates derived from the running gradient history, given `params` (a Tensor to optimize) and `lr` (a number, default 0.001). `adagrad` accumulates the full gradient history and so its effective rate only shrinks over time; `rmsprop` and `adadelta` use a decaying average instead, so they keep adapting late in training. Returns an optimizer Record, or a recipe Record when called with no `params`. |
| `lr_adaptive`, `lr_plateau` | `lr_plateau([lr=], [patience=], [factor=], [base=])` | Learning-rate controllers that wrap a base optimizer rather than replacing it: `lr=` the starting learning rate (number), `patience` an integer number of epochs to wait for improvement before shrinking the rate, `factor` the multiplicative shrink (a number in `(0, 1)`), and `base` a string naming the wrapped optimizer (e.g. `"adam"`). Passed as `optimizer=` wherever a plain optimizer recipe would go. Returns an optimizer-recipe Record. |
| `quick_mlp` | `quick_mlp(X, y, [seed=], [epochs=])` | Fits a single-hidden-layer multilayer perceptron in one call on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y` (or an `(N, n_classes)` one-hot matrix for multi-class `y`), with an optional integer `seed=`, `epochs` (integer, default 200), and an implied hidden width of `clamp(2*D, 8, 64)`. Returns an already-fitted `kind="sequential"` Model with a `loss_history` field (a length-`epochs` vector) plus `.predict`. |

`sgd(w, 0.01)`/`adam(w, lr=0.01)` take the tensor to optimize as their
first argument; called with no positional argument at all (`adam(lr=0.01)`)
they instead return a spec-only "recipe" for `optimizer=`. Every other
update rule below plugs into the exact same `optimizer_step(opt, grad)`
call — only the constructor changes:

```qu
x = [1.0, 2.0, 3.0, 4.0]
y_true = [2.0, 4.0, 6.0, 8.0]
w = param(0.0)
function loss_fn(w)
    pred = w * x
    diff = pred - y_true
    return mean(diff .^ 2)
end function
opt = sgd(w, 0.01)
losses = train_loop("loss_fn", w, 100, optimizer = "opt")
print("train_loop (sgd): loss {round(losses[0], 3)} -> {round(losses[99], 5)}")
tape_reset()

w2v = 0.0
opt2 = adam(param(w2v), lr = 0.1)
for t = 0 to 19
    tape_reset()
    w2 = param(w2v)
    pred = w2 * x
    loss2 = mean((pred - y_true) .^ 2)
    g = grad(loss2, wrt = w2)
    opt2 = optimizer_step(opt2, g)
    w2v = opt2._params[0]
end for
print("optimizer_step (adam): w -> {round(w2v, 4)} (target 2.0)")
tape_reset()

# One step each from w=1.0, grad=3.0 -- same optimizer_step(opt, grad) call.
w3 = param(1.0)
grad3 = [3.0]
r1 = optimizer_step(nesterov_sgd(w3, lr = 0.05, momentum = 0.9), grad3)
r2 = optimizer_step(adamw(w3, lr = 0.05), grad3)
r3 = optimizer_step(nadam(w3, lr = 0.05), grad3)
r4 = optimizer_step(adamax(w3, lr = 0.05), grad3)
r5 = optimizer_step(rmsprop(w3, lr = 0.05), grad3)
r6 = optimizer_step(adagrad(w3, lr = 0.05), grad3)
r7 = optimizer_step(adadelta(w3), grad3)
print("nesterov_sgd -> {round(r1._params[0], 4)}, adamw -> {round(r2._params[0], 4)}, nadam -> {round(r3._params[0], 4)}")
print("adamax -> {round(r4._params[0], 4)}, rmsprop -> {round(r5._params[0], 4)}, adagrad -> {round(r6._params[0], 4)}, adadelta -> {round(r7._params[0], 4)}")

Xxor = [0.0, 0.0; 0.0, 1.0; 1.0, 0.0; 1.0, 1.0]
Yxor = [0.0; 1.0; 1.0; 2.0]
net = input(2) |> dense(4, activation = "relu") |> dense(1, activation = "none") |>
      compile(optimizer = lr_plateau(lr = 0.1, patience = 5, base = "adam"), loss = "mse", seed = 7)
net = net.fit(Xxor, Yxor, epochs = 60)
n = length(net.loss_history)
print("compile + fit(lr_plateau): loss {round(net.loss_history[0], 4)} -> {round(net.loss_history[n - 1], 4)}")

net2 = input(2) |> dense(4, activation = "relu") |> dense(1, activation = "none") |>
       compile(optimizer = lr_adaptive(lr = 0.05), loss = "mse", seed = 7)
net2 = net2.fit(Xxor, Yxor, epochs = 60)
n2 = length(net2.loss_history)
print("compile + fit(lr_adaptive): loss {round(net2.loss_history[0], 4)} -> {round(net2.loss_history[n2 - 1], 4)}")

quick = quick_mlp(Xxor, Yxor, seed = 3)
nq = length(quick.loss_history)
print("quick_mlp: loss {round(quick.loss_history[0], 4)} -> {round(quick.loss_history[nq - 1], 4)}")
```

#### Overloads: `sgd`

| Call | What it returns |
|---|---|
| `sgd(params, [lr=])` | An optimizer Record bound to `params`, with `_params` holding the tensor's current value -- ready for `optimizer_step`. |
| `sgd([lr=])` (no positional argument) | An unbound "recipe" Record with no `_params` -- for passing as `optimizer=` to `compile`/`train_loop`, to be bound to a parameter later. |

`nesterov_sgd`, `adam`, `adamw`, `nadam`, `adamax`, `rmsprop`, `adagrad`
and `adadelta` all share this same two-call-shape overload.

```qu
wv = param(1.0)
bound = sgd(wv, lr = 0.1)
print("bound optimizer's _params: {bound._params}")   # [1] -- ready for optimizer_step

recipe = sgd(lr = 0.1)
print(recipe)                                          # model(sgd) {lr} -- no _params yet
```

### Building the network: layer specs and `sequential`

The `input(2) |> dense(4, activation = "relu") |> ... |> compile(...)`
pipeline used above is sugar over a smaller set of primitives: `dense_layer`
and `dropout_layer` build plain, unfitted layer-spec Records (no weights
yet), and `sequential` takes a list of them and returns an initialized —
but not yet trained — `kind="sequential"` Model, exactly the same shape
`compile` itself returns. `mlp_classifier` is a shortcut over the same two
steps for the common "stack of dense+relu layers ending in raw class
logits" case.

| Function | Signature | Description |
|---|---|---|
| `input` | `input(n)` | Starts the pipeline sugar: `n` (a positive integer) is the number of input features. Returns a `pipeline_input` Record tracking the running output width and an (initially empty) layer list, ready to be piped into `dense(...)` stages and finished with `compile(...)`. |
| `dense_layer` | `dense_layer(in_dim, out_dim, [activation=])` | Builds one unfitted dense-layer spec: `in_dim`/`out_dim` (positive integers) the layer's input and output widths, `activation` a string, `"relu"` or `"none"` (default `"none"`, a bare affine layer). Returns a layer-spec Record with no weights yet — `sequential` does the actual initialization. Piping several calls together (`dense_layer(2, 8, activation = "relu") \|> dense_layer(8, 1)`) accumulates them into a single list, ready for `sequential`. |
| `dropout_layer` | `dropout_layer(rate)` | Builds one unfitted dropout-layer spec for use inside a `sequential` stack: `rate` a number in `[0, 1)`, the fraction of activations to zero during training. Returns a layer-spec Record with no weights (dropout has none) — `sequential_forward` turns it into a `dropout(...)` call during training and a no-op passthrough during `.predict`. |
| `sequential` | `sequential(layers, [seed=])` | Initializes a stack of layer specs (a list of `dense_layer`/`dropout_layer` Records, or a single one) into a trainable model: weights use He initialization (`sqrt(2/in_dim)`) on a layer whose activation is `"relu"`, Xavier-style (`sqrt(1/in_dim)`) otherwise, biases start at zero, and an optional integer `seed=` makes the draw reproducible. Returns an initialized, not-yet-fitted `kind="sequential"` Model with `.fit`/`.predict`. |
| `mlp_classifier` | `mlp_classifier(in_dim, hidden_dims, n_classes, [seed=])` | Shortcut that builds a `dense_layer(..., activation="relu")` for every size in `hidden_dims` (a vector of positive integers, e.g. `[64, 32]`) followed by a final `dense_layer(..., activation="none")` down to `n_classes` (a positive integer) raw logits, then initializes them with `sequential` (optional integer `seed=`). Returns an initialized, not-yet-fitted `kind="sequential"` Model; pair `.fit(X, Y, loss = "cross_entropy")` (one-hot `Y`) with `softmax_rows`/`argmax` on `.predict`'s output yourself. |

The example below builds the *same kind* of network two different ways.
First, directly from primitives: two `dense_layer` calls with a
`dropout_layer` in between, assembled by `sequential`, then fitted like
any other `kind="sequential"` model. Second, via `mlp_classifier`, which
skips writing out the layer list by hand for the common classification
shape — its output is raw per-class logits, so a real classifier would
still run them through `softmax_rows` before reading off a class.

```qu
dl1 = dense_layer(2, 8, activation = "relu")
dl2 = dropout_layer(0.3)
dl3 = dense_layer(8, 1, activation = "none")
net = sequential((dl1, dl2, dl3), seed = 3)
print("layer count: {length(net.layers)}")

Xxor = [0.0, 0.0; 0.0, 1.0; 1.0, 0.0; 1.0, 1.0]
Yxor = [0.0; 1.0; 1.0; 2.0]
net = net.fit(Xxor, Yxor, epochs = 60, optimizer = adam(lr = 0.05), loss = "mse")
n = length(net.loss_history)
print("sequential + dropout_layer: loss {round(net.loss_history[0], 4)} -> {round(net.loss_history[n - 1], 4)}")

mc = mlp_classifier(2, [8], 2, seed = 3)
Yc = [0, 1, 1, 0]
mc = mc.fit(Xxor, Yc, epochs = 60, loss = "cross_entropy")
nc = length(mc.loss_history)
print("mlp_classifier: loss {round(mc.loss_history[0], 4)} -> {round(mc.loss_history[nc - 1], 4)}")
preds = mc.predict(Xxor)
print("mlp_classifier raw logits shape: {rows(preds)} x {cols(preds)}")
```

## Ready-made architectures

One call each fits a whole network, for when the question is about your
data rather than about the network.

| Function | Signature | Description |
|---|---|---|
| `simple_cnn` | `simple_cnn([height, width], n_classes, [seed=])` | Builds an untrained conv → relu → maxpool → dense image classifier: `[height, width]` a 2-element vector giving the image size, `n_classes` a positive integer, and an optional integer `seed=`. Returns a `kind="simple_cnn"` Model with `.fit(images, labels, [epochs=], [lr=])`, where `images` is a tuple of `height` x `width` matrices (one per sample) and `labels` a length-`M` integer vector, `epochs` an integer and `lr` a number (learning rate); `.fit` returns the trained Model, and `.predict(images)` on it returns a length-`M` vector of predicted classes. |
| `simple_rnn_classifier` | `simple_rnn_classifier(input_size, hidden_size, n_classes, [seed=])` | Builds an untrained GRU-backed sequence classifier: `input_size`/`hidden_size`/`n_classes` positive integers (features per timestep, hidden width, and number of classes), plus an optional integer `seed=`. Returns a `kind="simple_rnn_classifier"` Model with `.fit(sequences, labels, [epochs=], [lr=])`, where `sequences` is a tuple of `(input_size, seq_len)` matrices and `labels` a length-`M` integer vector; `.predict(sequences)` returns a length-`M` vector of predicted classes. |

The two examples below build the smallest possible dataset for each
architecture — two hand-drawn 6x6 images with different dot patterns for
`simple_cnn`, and two short rising/falling sequences for
`simple_rnn_classifier` — fit for a handful of epochs, and check that
`.predict` comes back with one row per input sample:

```qu
net = simple_cnn([6, 6], 2, seed = 3)
img1 = zeros(6, 6)
img1[2, 2] = 1.0
img1[2, 3] = 1.0
img1[3, 2] = 1.0
img1[3, 3] = 1.0
img2 = zeros(6, 6)
img2[1, 2] = 1.0
img2[2, 1] = 1.0
img2[2, 3] = 1.0
img2[3, 2] = 1.0
net = net.fit((img1, img2), [0, 1], epochs = 5, lr = 0.1)
print("simple_cnn predict rows: {rows(net.predict((img1, img2)))}")

rnn = simple_rnn_classifier(1, 6, 2, seed = 5)
seq1 = [0.1, 0.3, 0.5, 0.7, 0.9] as matrix(1, 5)
seq2 = [0.9, 0.7, 0.5, 0.3, 0.1] as matrix(1, 5)
rnn = rnn.fit((seq1, seq2), [0, 1], epochs = 5, lr = 0.1)
print("simple_rnn_classifier predict rows: {rows(rnn.predict((seq1, seq2)))}")
```

## Reinforcement learning: gridworld

A square grid, four actions, tabular temporal-difference learning.

| Function | Signature | Description |
|---|---|---|
| `gridworld_env` | `gridworld_env(n, [start=], [goal=], [step_reward=-1], [goal_reward=10])` | Builds an `n x n` grid environment (integer `n`), with `start`/`goal` (optional integer state indices, defaulting to opposite corners), `step_reward` (number, default -1, given on every non-terminal move) and `goal_reward` (number, default 10, given on reaching the goal). Actions are the integers 0 to 3: up, right, down, left; walking into an edge leaves the agent where it was. Returns a `kind="gridworld_env"` Model (an environment handle, not a fitted model) with `.reset()`/`.step(state, action)`. |
| `reset` | `reset(env)` or `env.reset()` | Returns the environment `env`'s starting state — a plain integer state index. The environment holds no live position of its own to reset; the returned index is the one to thread through your own loop. |
| `step` | `env.step(state, action)` | Applies one `action` (an integer 0-3) from the current integer `state` index in environment `env`. Returns a Record with `next_state` (integer), `reward` (a number, `step_reward` or `goal_reward`), and `done` (a boolean, true once the goal is reached). |
| `q_learning`, `sarsa` | `q_learning(env, episodes, [alpha=0.1], [gamma=0.9], [epsilon=0.1], [seed=])` | Tabular temporal-difference learning on a `gridworld_env` `env` for an integer number of `episodes`, with learning rate `alpha`, discount factor `gamma`, and epsilon-greedy exploration rate `epsilon` (all numbers), plus an optional integer `seed=`. `q_learning` is off-policy (bootstraps off the greedy next action); `sarsa` is on-policy (bootstraps off the action actually taken next). Both return a fitted model Record with `.q_table` (an `(n_states, n_actions)` matrix) and `.rewards` (a length-`episodes` vector, one total reward per episode — it should trend upward as training progresses), plus `.predict(state)` giving the greedy action (an integer) for a given state index. Returns that fitted `Model`. |

The example below trains both algorithms on the same 4x4 grid for 300
episodes, compares their reward trend from the first 20 episodes to the
last 20 (the real check that either one learned anything, since a
constant reward with no crash proves nothing), then runs the greedy
policy `q_learning` produced from the start state to the goal and counts
the steps it takes:

```qu
env = gridworld_env(4)
start = env.reset()
print("start state: {start}")

q = q_learning(env, 300, alpha = 0.3, gamma = 0.9, epsilon = 0.1, seed = 7)
s = sarsa(env, 300, alpha = 0.3, gamma = 0.9, epsilon = 0.1, seed = 7)
print("q_learning reward: early {round(mean(q.rewards[0:19]), 2)} -> late {round(mean(q.rewards[280:299]), 2)}")
print("sarsa reward:      early {round(mean(s.rewards[0:19]), 2)} -> late {round(mean(s.rewards[280:299]), 2)}")

state = env.reset()
steps = 0
done = false
while done == false and steps < 20
    action = q.predict(state)
    result = env.step(state, action)
    state = result.next_state
    steps = steps + 1
    done = result.done
end while
print("greedy policy reached the goal in {steps} steps")
```

## Estimation: Kalman, EKF, UKF, particle filters

The whole family goes through three verbs — `predict`, `update`,
`estimate` — dispatching on the state's `.kind` and, within `"kalman"`, on
whether the second argument is a matrix (linear) or a function name
(nonlinear, `method="ekf"` by default or `"ukf"`). Each call returns a
**new** state; nothing is mutated.

| Function | Signature | Description |
|---|---|---|
| `kalman_init` | `kalman_init(x0, P0)` | Initial linear-Kalman state: `x0` a length-`D` initial state estimate vector, `P0` a `(D, D)` initial covariance matrix. Returns a `kind="kalman"` state Record with fields `x` (length-`D`) and `P` (`(D, D)`). No control input. |
| `particle_filter_init` | `particle_filter_init(x0, n, [spread=1.0], [seed=])` | Generic particle filter state: `x0` a length-`D` centre vector, `n` (integer) the particle count scattered around it with spread `spread` (a number, default 1.0), and an optional integer `seed=`. Returns a `kind="particle_filter"` state Record with `(n, D)` `particles` and a length-`n` uniform `weights` vector; you supply the transition and likelihood functions by name on `predict`/`update`. |
| `particle_filter` | `particle_filter(n_particles, initial_state, [process_noise=0.05], [obs_noise=1.0], [spread=1.0], [seed=])` | The specialized constant-velocity tracker: `n_particles` (integer), `initial_state` (a length-`D` position/velocity vector), `process_noise`/`obs_noise` (numbers, the built-in noise levels — no `Q`/`R` argument needed at `predict`/`update` time), `spread` (number, initial scatter), and an optional integer `seed=`. Returns a `kind="particle_tracker"` state Record whose predict step is a single matmul across all particles, so it can go to the GPU. |
| `predict` (linear) | `state.predict(F, Q)` | On a `kind="kalman"` state, `F` a `(D, D)` transition matrix and `Q` a `(D, D)` process-noise covariance. Computes `x' = F·x`, `P' = F·P·Fᵀ + Q`. Returns a new state Record. |
| `predict` (EKF/UKF) | `state.predict("process_fn", Q, [jac=], [method=])` | On a `kind="kalman"` state, `"process_fn"` a string naming an in-scope function `(x) -> x'`, `Q` a `(D, D)` process-noise covariance, `jac=` an optional string naming the process function's Jacobian `(x) -> (D, D)` matrix (EKF only), and `method=` `"ekf"` (default) or `"ukf"`. The function itself is evaluated exactly; only the covariance propagation linearizes for EKF. `"ukf"` pushes sigma points through instead and rejects `jac=` outright, since a UKF has no Jacobian to take. Returns a new state Record. |
| `predict` (particle) | `state.predict("process_fn")` | On a `kind="particle_filter"` state, `"process_fn"` a string naming an in-scope function `(particle) -> particle'` applied to every one of the `n` particles; the function is expected to inject its own noise — there is no `Q` argument here. Returns a new state Record with updated `particles`. |
| `predict`/`move` (tracker) | `state.predict(dt)` / `state.move(dt)` | On a `kind="particle_tracker"` state, `dt` a number (the timestep). Advances every particle by one constant-velocity step (one matmul across all of them) then adds process noise scaled by `dt`. `move` is an alias for the same call — it exists because for this state a "predict" really is a concrete motion update. Returns a new state Record. |
| `move` | `state.move(dt)` | Exactly `predict(dt)` on a `kind="particle_tracker"` state — the row above — under a verb that says what the step is: for this filter the predict step really is a concrete constant-velocity motion update, not the estimation family's generic `predict(transition, Q)`. `dt` is a number, the timestep. Calling it on any other kind of state errors rather than silently falling back to `predict`. Returns a new `kind="particle_tracker"` state Record; the original `state` is left untouched. |
| `update` (linear) | `state.update(H, z, R)` | On a `kind="kalman"` state, `H` a `(M, D)` observation matrix, `z` a length-`M` measurement vector, `R` an `(M, M)` measurement-noise covariance. Applies the standard Kalman gain, state, and covariance correction. Returns a new state Record. |
| `update` (EKF/UKF) | `state.update("obs_fn", z, R, [jac=], [method=])` | On a `kind="kalman"` state, `"obs_fn"` a string naming an in-scope observation function `(x) -> z`, `z` a length-`M` measurement vector, `R` an `(M, M)` measurement-noise covariance, `jac=`/`method=` the same EKF/UKF choice `predict` takes, applied here to the observation model instead. Returns a new state Record. |
| `update` (particle) | `state.update("likelihood_fn", z, [resample_threshold=0.5], [seed=])` | On a `kind="particle_filter"` state, `"likelihood_fn"` a string naming an in-scope function `(particle, z) -> weight`, `z` the measurement (shape matching what `likelihood_fn` expects), `resample_threshold` a number in `(0, 1]` (fraction of `n` below which resampling triggers, default 0.5), and an optional integer `seed=`. Reweights every particle, renormalizes, and resamples once the effective sample size falls below the threshold. Returns a new state Record with `neff` (a number) and `resampled` (a boolean) added. |
| `update` (tracker) | `state.update(observation, [resample_threshold=0.5], [seed=])` | On a `kind="particle_tracker"` state, `observation` a position-only measurement (velocity is unobserved), using the `obs_noise` stored on the state rather than a fresh `R` argument; `resample_threshold`/`seed=` as above. Returns a new state Record. |
| `estimate` | `state.estimate()` | Reads a point estimate off any estimation-family state with no arguments. Returns the particle weighted-mean vector (length-`D`) for a particle filter/tracker, or a Kalman/EKF/UKF state's `x` vector directly. |

Two things that cost time if you meet them the hard way. **`as matrix(r, c)`
fills column by column**, so `[1, 0, 1, 1] as matrix(2, 2)` is the matrix
whose first row is `1 1` — write the transition down and then read it back
before trusting it. And `jac=` with `method="ukf"` is a rejected error
rather than a silently ignored argument, because a UKF has no Jacobian to
take.

The example below is the linear case — the one every other variant in the
table above specializes: a constant-velocity `[position, velocity]` state
tracked from noisy position-only measurements. Each loop iteration calls
`predict` then `update` and keeps the new state, exactly the "returns a
new state, nothing is mutated" contract every estimation function in this
section follows; `rmse` at the end compares the raw noisy measurements
against the filtered estimate, so you can see the filter is actually doing
something rather than just running:

```qu
truth = zeros(60)
meas = zeros(60)
for t = 0 to 59
    truth[t] = 0.5 * t
    meas[t] = truth[t] + randn() * 2
end for

F = [1, 0, 1, 1] as matrix(2, 2)     # column-major: rows are [1, 1] and [0, 1]
Q = [0.01, 0, 0, 0.01] as matrix(2, 2)
H = [1, 0] as matrix(1, 2)
R = [4] as matrix(1, 1)

s = kalman_init([0, 0], [10, 0, 0, 10] as matrix(2, 2))
est = zeros(60)
for t = 0 to 59
    s = s.predict(F, Q)
    s = s.update(H, [meas[t]], R)
    est[t] = s.estimate()[0]
end for

print("measurement error: {round(rmse(truth, meas), 3)}")
print("filtered error:    {round(rmse(truth, est), 3)}")
scatter(0 to 59, meas, color = "#94a3b8", label = "measured")
plot(0 to 59, truth, color = "#0f172a", label = "truth")
plot(0 to 59, est, color = "#e11d48", width = 2, label = "Kalman")
legend()
title("constant-velocity Kalman filter")
```

## Markov chains and hidden Markov models

Their own verbs — `simulate`/`stationary`, `forward`/`viterbi` — because
neither model has an `X` to predict from.

| Function | Signature | Description |
|---|---|---|
| `markov_chain` | `markov_chain(P, [initial_state=0])` | Builds a discrete Markov chain from an `(S, S)` row-stochastic transition matrix `P` (every row must sum to 1 — rows that don't are rejected outright, never quietly renormalized) and an integer `initial_state` (default 0, a state index in `0..S-1`). Returns a `kind="markov_chain"` Record with field `transition` (the `(S, S)` matrix `P` itself) and methods `.simulate`/`.stationary`. |
| `simulate` | `chain.simulate(n_steps, [seed=])` | Draws one random trajectory from a `markov_chain` `chain`, starting at its `initial_state`, for an integer `n_steps` transitions (with an optional integer `seed=`). Returns a length-`n_steps + 1` vector of integer state indices. |
| `stationary` | `chain.stationary()` | Computes the long-run state distribution of a `markov_chain` `chain` by power iteration; converges for any irreducible, aperiodic chain. Returns a length-`S` probability vector. |
| *(n-step)* | `chain.transition ^ n` | The `n`-step transition matrix needs no dedicated builtin — `^` is already matrix power, so raising `chain.transition` (an `(S, S)` matrix) to an integer `n` gives the `(S, S)` matrix of `n`-step transition probabilities directly. |
| `hmm` | `hmm(transition, emission, initial)` | Builds a discrete hidden Markov model from an `(S, S)` row-stochastic hidden-state transition matrix `transition`, an `(S, O)` row-stochastic emission matrix `emission` (`O` the number of observable symbols), and a length-`S` initial-state distribution `initial`. Returns a `kind="hmm"` Record with methods `.forward`/`.viterbi`. Parameter fitting (Baum-Welch) is **not** implemented — the transition/emission matrices must already be known. |
| `forward` | `hmm.forward(obs)` | Runs the forward algorithm on an `hmm` model `hmm` for a length-`T` integer observation-symbol sequence `obs` (each entry a 0-based column index into `emission`). Returns a single number: the total probability of that observation sequence, summed over every possible hidden-state path. |
| `viterbi` | `hmm.viterbi(obs)` | Runs the Viterbi algorithm on an `hmm` model `hmm` for a length-`T` integer observation sequence `obs` (same encoding as `forward`). Returns a length-`T` vector of integer hidden-state indices: the single most likely path, not a distribution over paths. |

The example below covers both models. First, a two-state weather chain:
`stationary` gives the long-run fraction of time in each state, and
`chain.transition ^ 3` shows the 3-step transition probabilities computed
with plain matrix power, no dedicated builtin needed. Second, a classic
two-state HMM (healthy/sick, observing normal/cold/dizzy symptoms):
`forward` totals the probability of one specific three-day observation
sequence over every possible hidden-health-state path, and `viterbi`
picks out the single most likely path instead.

```qu
P = [0.9, 0.5, 0.1, 0.5] as matrix(2, 2)   # column-major: rows [0.9 0.1], [0.5 0.5]
chain = markov_chain(P, 0)
print("stationary: {round(chain.stationary(), 4)}")
print("3-step:     {round(chain.transition ^ 3, 4)}")

trans = [0.7, 0.4, 0.3, 0.6] as matrix(2, 2)
emit  = [0.5, 0.1, 0.4, 0.3, 0.1, 0.6] as matrix(2, 3)
h = hmm(trans, emit, [0.6, 0.4])
print("P(normal, cold, dizzy) = {round(h.forward([0, 1, 2]), 5)}")
print("most likely path       = {h.viterbi([0, 1, 2])}")
```

## Computational geometry

| Function | Signature | Description |
|---|---|---|
| `voronoi` | `voronoi(points)` | Computes the Voronoi diagram of an `(N, 2)` point matrix `points`, built on a real Delaunay triangulation. Returns a Record with fields `points` (the input `(N, 2)` matrix, unchanged), `cells` (a length-`N` list, one entry per input point, each a list of `(x, y)` vertex coordinates outlining that point's Voronoi cell), and `bounded` (a length-`N` vector of 0/1 flags, one per point). |

A cell that reaches infinity has no finite vertex list to give you, so
`cells` is empty or partial there and `bounded` is `0`. On a small point
set that is most of them. The example below computes the Voronoi diagram
of a perfect square's four corners — every corner cell is unbounded,
because with only four points there is nothing to close any of them off —
and prints the one interior vertex that does show up: the square's centre,
shared by all four unbounded cells.

```qu
pts = [0, 2, 0, 2, 0, 0, 2, 2] as matrix(4, 2)   # the corners of a square
v = voronoi(pts)
print("bounded: {v.bounded}")      # every corner cell is unbounded
print("cell 0:  {v.cells[0]}")     # the one finite vertex: the centre
```

## Feature scaling

`fit_scaler` with `.transform`/`.inverse_transform` is the fit-once path —
fit on training data, apply unchanged to a test set. The four one-shot
verbs fit and apply in the same call, with nothing kept. All of them work
column-wise on a matrix, whole on a vector, and per numeric column on a
table.

| Function | Signature | Description |
|---|---|---|
| `normalize` | `normalize(X)` | Min-max scaling of `X` (a vector, an `N`-row/`D`-column matrix scaled column-wise, or a table scaled per numeric column) to `[0, 1]`: `(x - min) / (max - min)`. Returns a value the same shape as `X`. |
| `standardize` | `standardize(X)` | z-score scaling of `X` (vector, matrix column-wise, or table per numeric column): `(x - mean) / std`. Returns a value the same shape as `X`, mean 0 and unit variance per column/vector. |
| `robust_scale` | `robust_scale(X)` | Outlier-resistant scaling of `X` (vector, matrix column-wise, or table per numeric column): `(x - median) / IQR`. Neither the median nor the interquartile range moves much when one extreme value arrives, unlike `standardize`'s mean/std. Returns a value the same shape as `X`. |
| `quantile_normalize` | `quantile_normalize(X)` | Rank-based scaling of `X` (vector, matrix column-wise, or table per numeric column): each value is replaced by its rank against its own column, rescaled uniform onto `[0, 1]`. Returns a value the same shape as `X`; tied input values land on the same output rank. |
| `fit_scaler` | `fit_scaler(X, [method=])` | Fits and stores scaling parameters from an `N`-row, `D`-column matrix (or vector, or table) `X`, for reuse on new data — the fit-once path, as opposed to the four one-shot verbs above. `method` is a string, one of `"standard"`, `"minmax"`, `"robust"`, `"quantile"` (default `"standard"`). Returns a fitted scaler Record carrying the per-column parameters (e.g. `mean`/`std` for `"standard"`) plus `.transform`/`.inverse_transform`. |
| `transform` | `scaler.transform(newX)` | Applies a fitted scaler Record `scaler`'s stored parameters to a new matrix/vector/table `newX` with the same columns it was fitted on. Never refits, even if `newX`'s own statistics differ. Returns a value the same shape as `newX`. |
| `inverse_transform` | `scaler.inverse_transform(newX)` | Undoes a fitted scaler Record `scaler`'s transform on an already-scaled matrix/vector/table `newX`, returning it to the original units. Returns a value the same shape as `newX`; lossy for a `"quantile"`-method scaler when the originally-fitted data had ties. |

One outlier is enough to show why the choice matters. `normalize` and
`standardize` are the two everyday defaults — min-max to a fixed range, or
z-score to mean 0 and unit variance — but both let a single extreme value
dominate the whole scale, which is exactly what the example below shows by
comparing `standardize` against the outlier-resistant `robust_scale` on
the same eight numbers:

```qu
raw = [2, 4, 4, 4, 5, 5, 7, 900]
print("standardize:  {round(standardize(raw), 2)}")
print("robust_scale: {round(robust_scale(raw), 2)}")
```

`standardize` squeezes every real value into a band a hundredth of a unit
wide, because the outlier owns the standard deviation. `robust_scale`
leaves them spread out and lets the outlier be the outlier.

`quantile_normalize` throws away the values entirely and keeps only the
rank, so the outlier stops mattering at all:

```qu
print("quantile_normalize: {round(quantile_normalize(raw), 3)}")
```

The four tied `4`s and `5`s land on the same rank, which is why the
result has runs of equal values rather than eight evenly spaced ones.

#### Overloads: `standardize`

#### Case: vector

A plain `Vec` is standardized as a whole -- one mean, one std, across
every element.

```qu
sv = [2.0, 4.0, 6.0, 8.0]
print("standardize on a vector: {round(standardize(sv), 3)}")
```

#### Case: matrix

An `N`-row, `D`-column `Mat` is standardized column by column -- each
column gets its own mean and std, independent of the others.

```qu
sm = [1.0, 100.0; 2.0, 200.0; 3.0, 300.0]
print("standardize on a matrix, per column: {round(standardize(sm), 3)}")
```

#### Case: table

A table is standardized per numeric column, exactly like a matrix -- a
non-numeric column (a label, say) passes through untouched instead of
erroring.

```qu
st = table(a = [1.0, 2.0, 3.0], b = [10.0, 20.0, 30.0], label = ["x", "y", "z"])
print("standardize on a table, per numeric column (label untouched):")
print(standardize(st))
```

`normalize`, `robust_scale` and `quantile_normalize` all branch the same
three ways.

## Splitting and evaluation

| Function | Signature | Description |
|---|---|---|
| `train_test_split` | `train_test_split(X, y, [test_size=0.2], [seed=], [shuffle=true])` | Splits an `N`-row feature matrix `X` and a length-`N` target vector `y` into training and test sets, `X` and `y` permuted together by the same random order (unless `shuffle=false`). `test_size` is a number in `(0, 1)` giving the test fraction (default 0.2), and `seed=` an optional integer. Returns a Record with `X_train`, `X_test` (matrices), `y_train`, `y_test` (vectors). |
| `sequential_split` | `sequential_split(X, y, [test_size=0.2])` | The same shape as `train_test_split` on `X`/`y`/`test_size`, but never shuffles: the first rows become the training set, the last rows the test set, in their original order. Returns a Record with `X_train`, `X_test`, `y_train`, `y_test`. |
| `stratified_split` | `stratified_split(X, y, [test_size=0.2], [seed=])` | The same shape as `train_test_split` on `X`/`y`/`test_size`/`seed=`, but splits each class of `y` separately before combining, so a rare class keeps its proportion in both sets every time rather than only on average. Returns a Record with `X_train`, `X_test`, `y_train`, `y_test`. |
| `train_val_test_split` | `train_val_test_split(X, y, [val_size=0.2], [test_size=0.2], [seed=], [shuffle=true])` | Three-way split of `X`/`y` into training, validation, and test sets: `val_size`/`test_size` are numbers in `(0, 1)` (both default 0.2); `val_size` is rescaled against the remainder left after removing the test set, so the final validation share really is what you asked for rather than a share of a share. Returns a Record with `X_train`/`X_val`/`X_test` (matrices) and `y_train`/`y_val`/`y_test` (vectors). |
| `confusion_matrix` | `confusion_matrix(actual, predicted, [n_classes])` | Draws a confusion-matrix heatmap comparing a length-`N` vector of true labels `actual` against a length-`N` vector of predicted labels `predicted`, over an optional integer `n_classes` (inferred from the data if omitted). This is a plotting side effect, not a computed matrix handed back to the caller. Returns `Nothing`. |

`sequential_split` exists under its own name rather than as
`train_test_split(..., shuffle = false)` because shuffling time-ordered
rows before splitting leaks the future into training, and that bug is
invisible in the results — the model just looks good. The example below
runs all three split strategies plus the three-way split, printing the
row counts each one produces, then trains a quick classifier on the
stratified split and hands its predictions to `confusion_matrix` to draw:

```qu
seq = sequential_split(X, y, test_size = 0.2)
print("sequential_split train rows: {rows(seq.X_train)}")

strat = stratified_split(Xc, yc, test_size = 0.3, seed = 9)
print("stratified_split train rows: {rows(strat.X_train)}")

tvt = train_val_test_split(Xc, yc, val_size = 0.2, test_size = 0.2, seed = 9)
print("train/val/test rows: {rows(tvt.X_train)}, {rows(tvt.X_val)}, {rows(tvt.X_test)}")

m = knn_model(strat.X_train, strat.y_train, 5, kind = "classification")
confusion_matrix(strat.y_test, m.predict(strat.X_test), 2)
```

`confusion_matrix` draws its heatmap and returns nothing, which is why it
sits alone on its own line rather than feeding a `print`.

## Random distributions

The parameters come first and the size (`rows`, `cols`, both defaulting to
1) after. An explicit `seed=` gives a fresh reproducible stream; without
one, the shared stream advances.

| Function | Signature | Description |
|---|---|---|
| `rand` | `rand([rows], [cols], [seed=])` | Draws from the uniform distribution on `[0, 1)`. `rows`/`cols` are integers, both defaulting to 1, giving the output shape (a number if both are 1, otherwise a vector or `(rows, cols)` matrix); an optional integer `seed=` starts a fresh reproducible stream, otherwise the shared stream advances. Returns a scalar `Num` when `rows` and `cols` are both 1, a `Vec` when one of them is, and a `(rows, cols)` `Mat` otherwise. |
| `randn` | `randn([rows], [cols], [seed=])` | Draws from the standard normal distribution (mean 0, std 1), same `rows`/`cols`/`seed=` shape as `rand`. Returns a number, vector, or `(rows, cols)` matrix. |
| `seed` | `seed(n)` | Reseeds the shared random stream with integer `n`, so subsequent calls to `rand`/`randn`/etc. without their own `seed=` become reproducible from this point on. Returns nothing. |
| `uniform` | `uniform(a, b, [rows], [cols], [seed=])` | Draws from the uniform distribution on `[a, b)`, two numbers `a < b`, with the same `rows`/`cols`/`seed=` shape as `rand`. Returns a number, vector, or `(rows, cols)` matrix. |
| `normal` | `normal(mu, sigma, [rows], [cols], [seed=])` | Draws from a Gaussian. `mu` (number) is the mean; `sigma` (number) is the standard deviation; `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and two for a matrix; `seed=` (number, optional) makes the draw repeatable. Returns a number, vector, or `(rows, cols)` matrix. |
| `randi` | `randi(lo, hi, [rows], [cols], [seed=])` | Draws random integers. `lo` (number) is the smallest value that can come out; `hi` (number) is the largest, **inclusive at both ends** (MATLAB's convention, not NumPy's half-open one, so `randi(1, 6)` really can return `6`); `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and two for a matrix; `seed=` (number, optional) makes the draw repeatable. Returns a number, vector, or `(rows, cols)` matrix. |
| `poisson` | `poisson(lambda, [rows], [cols], [seed=])` | Exact Poisson draws. `lambda` (a positive number) is the rate, which is also the mean and the variance of the result; `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and two for a matrix; `seed=` (number, optional) makes the draw repeatable. Slow, not wrong, for very large `lambda`. Returns a number, vector, or `(rows, cols)` matrix of non-negative integers. |
| `exponential` | `exponential(rate, [rows], [cols], [seed=])` | Exponential draws. `rate` (a positive number) is the **rate**, not the scale: the mean is `1/rate`, and NumPy instead takes `scale = 1/rate` directly, so a value copied from NumPy needs inverting; `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and two for a matrix; `seed=` (number, optional) makes the draw repeatable.  Returns a number, vector, or `(rows, cols)` matrix. |
| `binomial` | `binomial(n_trials, p, [rows], [cols], [seed=])` | Exact binomial draws. `n_trials` (a positive integer) is how many trials each draw counts successes over; `p` (a number in `[0, 1]`) is the success probability of one trial; `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and two for a matrix; `seed=` (number, optional) makes the draw repeatable. Returns a number, vector, or `(rows, cols)` matrix of non-negative integers. |
| `chisquare` | `chisquare(k, [rows], [cols], [seed=])` | Draws from the chi-square distribution with `k` degrees of freedom (a positive integer), same `rows`/`cols`/`seed=` shape as `rand`. Returns a number, vector, or `(rows, cols)` matrix. |
| `chi2pdf` | `chi2pdf(x, k)` | Chi-square probability density at `x` (a number or vector, elementwise) with `k` degrees of freedom (a positive number), computed in log space so it survives modest `k` without overflowing. Returns a number or vector the same shape as `x`. |
| `chi2cdf` | `chi2cdf(x, k)` | Chi-square cumulative distribution `P(X <= x)` at `x` (a number or vector, elementwise) with `k` degrees of freedom. Returns a number or vector in `[0, 1]`, the same shape as `x`. |
| `normpdf` | `normpdf(x, [mu=0], [sigma=1])` | Gaussian probability density at `x` (a number or vector, elementwise) with mean `mu` (default 0) and standard deviation `sigma` (default 1). Returns a number or vector the same shape as `x`. |
| `mvnpdf` | `mvnpdf(x, mu, Sigma)` | Multivariate Gaussian density at a length-`D` point `x`, with a length-`D` mean vector `mu` and a `(D, D)` covariance matrix `Sigma`, evaluated through `Sigma`'s Cholesky factor rather than an explicit matrix inverse (faster, and numerically stabler for a near-singular covariance). Returns a single number. |
| `random_walk` | `random_walk(n_steps, [drift=0], [volatility=1], [seed=])` | Generates a discretized Wiener-process path: `n_steps` (a positive integer) increments, each drawn `~ Normal(drift, volatility)`, cumulatively summed, with an optional integer `seed=`. Returns a length-`n_steps + 1` vector starting at 0, where `Var(x[n]) = n * volatility^2`. |

The first example draws 4000 samples from a Gaussian with a known mean and
standard deviation, confirms the sample statistics land close to the
parameters that generated them, and plots the resulting histogram:

```qu
draws = normal(10, 2, 4000, seed = 77)
print("mean {round(mean(draws), 3)}, std {round(std(draws), 3)}")
hist(draws, bins = 40, color = "#4169e1")
title("normal(mu = 10, sigma = 2), 4000 draws")
xlabel("value")
```

`rand` and `seed` are the plain uniform stream and its reset — reseeding
with `seed(42)` fixes what every later unseeded call draws, while an
explicit `seed=` on a single call always wins over whatever the shared
stream is doing. `chi2pdf` and `normpdf` are densities rather than draws:
they take a point and return how likely it is, with no randomness
involved at all. `random_walk` is a cumulative path, not an independent
sample each step, so its last value is the sum of every increment before
it:

```qu
seed(42)                             # reseed the shared stream
u = rand(3, seed = 1)                # an explicit seed still wins
print("rand draws: {round(u, 3)}")
print("chi2pdf(2, 3):  {round(chi2pdf(2, 3), 4)}")
print("normpdf(0):     {round(normpdf(0), 4)}")

walk = random_walk(20, drift = 0.1, volatility = 1.0, seed = 3)
print("random walk, last value: {round(walk[19], 3)}")
```

## Putting it together

Everything above shows one call at a time. Two things only make sense as a
sequence: reusing a scaler that was fitted somewhere else, and composing a
transform with an estimator.

**Fit the scaling on the training set, apply it unchanged to the test
set.** This is the whole reason `fit_scaler` exists as a separate step —
scaling the test set on its own statistics leaks it into the model.

```qu
split = train_test_split(X, y, test_size = 0.25, seed = 1)
scaler = fit_scaler(split.X_train, method = "standard")
print("training column means: {round(scaler.mean, 3)}")

Xtr = scaler.transform(split.X_train)
Xte = scaler.transform(split.X_test)
print("train columns after scaling: {round(mean(Xtr, axis = 0), 3)}")
print("test columns after scaling:  {round(mean(Xte, axis = 0), 3)}")
```

The training columns come out at zero because that is what they were
centred on. The test columns do not, and that is correct: they were scaled
by numbers the model was allowed to know.

**A pipeline does the same thing without you carrying the transform
around.** Every stage but the last is `(X) -> X`; the last is
`(X, y) -> Model`. Stages are named by string, the same convention `pmap`
uses.

```qu
standardize_cols(M) := (M - mean(M)) ./ std(M)
final(A, b) := ridge_model(A, b, 0.1)

pipe = pipeline("standardize_cols", "final")
fitted = pipe.fit(split.X_train, split.y_train)
print("held-out R2: {round(fitted.score(split.X_test, split.y_test), 4)}")
```

`.predict` and `.score` on the fitted pipeline replay `standardize_cols`
before delegating, so the test set goes through exactly the transform the
training set did.

## More functions

| Function | Signature | Description |
|---|---|---|
| `corrcoef` | `corrcoef(M)` | Called on an `N`-row, `D`-column matrix or table `M`, returns the full `(D, D)` matrix of pairwise Pearson correlations between its columns. For two length-`N` vectors `x`/`y`, use `corr(x, y)` instead, which returns their correlation as a single number. `corr_heatmap` draws the same `corrcoef` numbers as a plot. Returns a `(D, D)` `Mat`, symmetric with ones down the diagonal. |
| `mae`, `mse`, `rmse` | `rmse(actual, predicted)` | The regression-error trio, each taking a length-`N` vector of true values `actual` and a length-`N` vector of predictions `predicted`, and returning a single number: `mae` the mean absolute error, `mse` the mean squared error, `rmse` its square root. `.score(X, y)` on a model already gives R-squared; these three give the plain error numbers instead, in the target's own units for `mae` and `rmse` (`mse` is in squared units). Returns a scalar `Num`. |
| `precision`, `recall`, `f1` | `f1(actual, predicted)` | Classification metrics on a length-`N` vector of true labels `actual` and a length-`N` vector of predicted labels `predicted`, each returning a single number macro-averaged over every class present (the per-class score averaged with equal weight, regardless of how many examples that class has), so the same call works on binary and multi-class labels alike. Returns a scalar `Num` in `[0, 1]`. |
| `k_fold` | `k_fold(X, y, folds)` | Splits an `N`-row feature matrix `X` and a length-`N` target vector `y` into an integer number of `folds` cross-validation folds. Returns a length-`folds` list, each entry a Record with `X_train`/`X_test`/`y_train`/`y_test` for that fold; the loop over them (fitting a model per fold and averaging the score) is yours to write. |
| `cv_stability` | `cv_stability(X, y, [n_repeats=10], [cv_folds=5])` | Repeats cross-validation `n_repeats` times (integer, default 10) with `cv_folds` folds each (integer, default 5) on an `N`-row `X` and length-`N` `y`, reshuffling between repeats. Returns a length-`n_repeats` vector of the raw per-repeat scores (not just their average): `mean` and `std` of the result give the usual score-plus-variability summary, and a wide spread means the single number you were about to quote was luck. |
| `ablation_study` | `ablation_study(X, y, [cv_folds=3])` | Computes one importance score per feature column of an `N`-row, `D`-column `X` and length-`N` `y`, using `cv_folds` folds (integer, default 3): the full model's cross-validated score minus the score of the same model refit without that column. Returns a length-`D` vector of importance scores — a report on each feature's contribution, not a selection of which to keep. |
| `rfe` | `rfe(X, y, n_features)` | Recursive feature elimination on an `N`-row, `D`-column `X` and length-`N` `y`, down to an integer target `n_features`: repeatedly drops the weakest-scoring remaining column and refits. Returns a vector of the kept column indices, zero-based, length `n_features`. |
| `mutual_info_classif` | `mutual_info_classif(X, y, [k=3])` | k-nearest-neighbour mutual information between each column of an `N`-row, `D`-column `X` and a length-`N` discrete label vector `y`, using `k` neighbours (integer, default 3). Returns a length-`D` vector of non-negative scores, one per feature; catches a relationship that is real but not linear, which `corr`/`corrcoef` misses. |
| `astar_mrmr` | `astar_mrmr(X, y, n_features, ...)` | A guided (A*) search on an `N`-row, `D`-column `X` and length-`N` `y` for a feature subset of size `n_features` (integer) that is relevant to the target and not redundant with itself (minimum-redundancy-maximum-relevance). Returns a vector of the selected column indices, zero-based, length `n_features`, the same convention as `rfe`. |
| `permutation_importance` | `permutation_importance(model, X, y, [n_repeats=])` | Shuffles one column of an `N`-row, `D`-column `X` at a time and re-scores an already-fitted model Record `model` against the matching length-`N` target `y`, over `n_repeats` shuffles per column (integer). Returns a length-`D` vector of importance scores (drop in score caused by shuffling that column); works on any fitted model because it perturbs the input rather than reaching inside the model itself. |

The error and classification metrics, plus the model-selection helpers
above them, all line up against the regression and classification data
from earlier in this chapter:

```qu
print("corrcoef matrix:\n{round(corrcoef(X), 3)}")

m = ols_model(X, y)
yhat = m.predict(X)
print("mae: {round(mae(y, yhat), 4)}, mse: {round(mse(y, yhat), 4)}")

cm = knn_model(Xc, yc, 5, kind = "classification")
predc = cm.predict(Xc)
print("precision: {round(precision(yc, predc), 3)}")
print("recall:    {round(recall(yc, predc), 3)}")
print("f1:        {round(f1(yc, predc), 3)}")

folds = k_fold(X, y, 4)
print("k_fold: {len(folds)} folds, first fold trains on {rows(folds[0].X_train)} rows")

stab = cv_stability(Xc, yc, n_repeats = 5, cv_folds = 3)
print("cv_stability: {round(mean(stab), 3)} +/- {round(std(stab), 3)}")

ab = ablation_study(Xc, yc, cv_folds = 3)
print("ablation_study scores: {round(ab, 4)}")
print("rfe keeps:             {rfe(Xc, yc, 1)}")
print("mutual_info_classif:   {round(mutual_info_classif(Xc, yc, k = 3), 4)}")
print("astar_mrmr keeps:      {astar_mrmr(Xc, yc, 1)}")
print("permutation_importance: {round(permutation_importance(m, X, y, n_repeats = 5), 4)}")
```

`ablation_study`, `cv_stability` and `rfe` default to `kind =
"classification"`, which is why this block scores them against `Xc`/`yc`
rather than the regression `X`/`y` -- feeding a continuous target through
a classifier's cross-validation silently scores nonsense.

#### Overloads: `ablation_study`

#### Case: classification (the default)

With no `kind=` given, `ablation_study` cross-validates a classifier and
scores by accuracy -- correct only when `y` really is a set of discrete
labels.

```qu
Xac = randn(60, 2, seed = 91)
yac = zeros(60)
for i in 0 to 59
    if Xac[i, 0] + Xac[i, 1] > 0
        yac[i] = 1
    end if
end for
print("ablation_study, default kind (classification): {round(ablation_study(Xac, yac, cv_folds = 3), 4)}")
```

#### Case: regression

`kind = "regression"` switches the internal model and score to R². Skip
this on a continuous `y` and you hit the exact silent-nonsense trap the
default invites:

```qu
Xar = randn(60, 3, seed = 94)
betar = [2.0, -1.0, 0.5]
yar = Xar * betar + 0.05 * randn(60, seed = 95)
rk = "regression"
print("default kind on a continuous target: {round(ablation_study(Xar, yar, cv_folds = 3), 4)}")   # wrong! all zero
print("kind=regression on the same target:  {round(ablation_study(Xar, yar, cv_folds = 3, kind = rk), 4)}")
```

#### Overloads: `cv_stability`

#### Case: classification (the default)

Same default as `ablation_study`: with no `kind=`, `cv_stability` repeats
classifier cross-validation and reports accuracy scores.

```qu
Xcs = randn(60, 2, seed = 91)
ycs = zeros(60)
for i in 0 to 59
    if Xcs[i, 0] + Xcs[i, 1] > 0
        ycs[i] = 1
    end if
end for
print("cv_stability, default kind (classification): {round(cv_stability(Xcs, ycs, n_repeats = 3, cv_folds = 3), 4)}")
```

#### Case: regression

On a continuous `y`, the same default silently returns all zeros;
`kind = "regression"` recovers real R² scores instead:

```qu
Xrs = randn(60, 3, seed = 94)
betas = [2.0, -1.0, 0.5]
yrs = Xrs * betas + 0.05 * randn(60, seed = 95)
rk2 = "regression"
print("default kind on a continuous target: {round(cv_stability(Xrs, yrs, n_repeats = 3, cv_folds = 3), 4)}")   # wrong! all zero
print("kind=regression on the same target:  {round(cv_stability(Xrs, yrs, n_repeats = 3, cv_folds = 3, kind = rk2), 4)}")
```

### Saving a fitted model

`save_model`/`load_model` need the engine built with `--features
h5-models` (off by default, see the dependency note in `qu-interp`'s
`Cargo.toml`) and are not demonstrated by a runnable example in this
chapter for that reason — this build was compiled without the feature, and
calling either one on such a build fails with a message naming the missing
flag, the same pattern `llm_load`/`generate` use for `--features llm`
above. They are documented here so the signatures and behaviour are
accurate even without a runnable example.

| Function | Signature | Description |
|---|---|---|
| `save_model` | `save_model(m, path)` | Writes a fitted `kind="sequential"` model Record `m` (from `sequential`/`compile`/`quick_mlp`/`mlp_classifier`/etc.) to an HDF5 file at the string `path`, weights and all, in a Keras-compatible layout. Returns nothing. Only sequential models are supported — calling it on an `ols_model`/`kmeans_model`/etc. is rejected with a clear error. |
| `load_model` | `load_model(path)` | Reads a model previously written by `save_model` back from the HDF5 file at the string `path`. Returns a fitted `kind="sequential"` model Record with `.predict` ready to use immediately, so a fit does not have to be repeated. |
