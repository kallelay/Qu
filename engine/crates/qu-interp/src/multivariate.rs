//! § multivariate models: LDA, QDA, PLS regression, ICA (2026-09-28).
//!
//! Four constructors in the `*_model` family the language already has
//! (`pca_model`, `knn_model`, `logistic_model`, ...), so they work with the
//! verbs a user already knows:
//!
//! | constructor | `predict(m, X)` | `transform(m, X)` |
//! |---|---|---|
//! | `lda_model(X, y)` | class labels | the discriminant axes, `min(classes-1, features)` columns |
//! | `qda_model(X, y, [reg=])` | class labels | -- |
//! | `pls_model(X, Y, k)` | fitted `Y` (a vector when `Y` was one) | the `k` X-scores |
//! | `ica_model(X, k, [max_iter=], [tol=], [seed=])` | -- | the `k` recovered sources |
//!
//! plus `ica(X, k)`, the sources directly, as `pca(X, k)` sits beside
//! `pca_model`.
//!
//! **Numerics.** Everything reduces to symmetric eigenproblems of size
//! features x features (or k x k), which are small, so this module carries
//! its own cyclic Jacobi solver: unconditionally stable, accurate to
//! machine precision on symmetric input, and giving orthonormal vectors,
//! which is the property the whitening steps rely on. A covariance that is
//! singular (a constant feature, more features than rows in a class) is
//! handled explicitly: LDA uses the pseudo-inverse and says nothing,
//! because its decision rule is well defined on the span of the data; QDA
//! refuses with the `reg=` fix named, because its log-determinant is not.
//!
//! **Labels** are numbers (the `knn_model`/`tree_model` convention), kept
//! as given: `predict` returns the same values it was trained on, not
//! 0-based class indices.
//!
//! **ICA** is FastICA with the `logcosh` contrast (`g = tanh`) and
//! symmetric decorrelation. Its output is defined only up to the order,
//! sign and scale of the sources -- that is the model, not an
//! approximation -- so the sources are scaled to unit variance and the
//! order is whatever the iteration converged to. `seed=` fixes the
//! starting rotation; the default is fixed too, so a script's output does
//! not change from run to run.

use crate::{e, EvalError, ModelHandle, Value, R};
use qu_core::matrix::Matrix;
use std::sync::Arc;

type Rows = Vec<Vec<f64>>;

// ------------------------------------------------------------ helpers

pub(crate) fn rows_of(m: &Matrix) -> Rows {
    let (r, c) = m.shape();
    (0..r).map(|i| (0..c).map(|j| m.get(i, j).unwrap_or(0.0)).collect()).collect()
}

fn mat(rows: &Rows) -> R<Value> {
    if rows.is_empty() {
        return Ok(Value::Mat(Arc::new(Matrix::zeros(0, 0))));
    }
    Matrix::from_rows(rows)
        .map(|m| Value::Mat(Arc::new(m)))
        .map_err(|se| EvalError { msg: se.to_string() })
}

fn vecv(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

fn field_rows(m: &ModelHandle, name: &str) -> R<Rows> {
    match m.field(name) {
        Some(Value::Mat(x)) => Ok(rows_of(x)),
        _ => e(format!("{} model is missing its `{name}` field", m.kind)),
    }
}

fn field_vec(m: &ModelHandle, name: &str) -> R<Vec<f64>> {
    match m.field(name) {
        Some(Value::Vec(v)) => Ok(v.to_vec()),
        Some(Value::Num(n)) => Ok(vec![*n]),
        _ => e(format!("{} model is missing its `{name}` field", m.kind)),
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn mean_rows(x: &Rows) -> Vec<f64> {
    let d = x.first().map_or(0, |r| r.len());
    let mut m = vec![0.0; d];
    for r in x {
        for (a, v) in m.iter_mut().zip(r) {
            *a += v;
        }
    }
    let n = x.len().max(1) as f64;
    m.iter_mut().for_each(|a| *a /= n);
    m
}

fn transpose(a: &Rows) -> Rows {
    let (r, c) = (a.len(), a.first().map_or(0, |x| x.len()));
    (0..c).map(|j| (0..r).map(|i| a[i][j]).collect()).collect()
}

fn matmul(a: &Rows, b: &Rows) -> Rows {
    let bt = transpose(b);
    a.iter().map(|r| bt.iter().map(|c| dot(r, c)).collect()).collect()
}

fn check_features(kind: &str, x: &Rows, d: usize) -> R<()> {
    let got = x.first().map_or(d, |r| r.len());
    if got != d {
        return e(format!(
            "{kind}: model was fit on {d} feature(s), input has {got} -- a single new point needs \
             an explicit orientation, e.g. `[a, b] as matrix(1, 2)`, since a bare vector \
             defaults to a column"
        ));
    }
    Ok(())
}

/// Symmetric eigendecomposition by cyclic Jacobi rotations. Returns the
/// eigenvalues in DESCENDING order and the matching unit eigenvectors
/// (`vecs[j]` belongs to `vals[j]`).
pub(crate) fn sym_eig(a: &Rows) -> (Vec<f64>, Rows) {
    let n = a.len();
    let mut a = a.clone();
    let mut v: Rows = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for _sweep in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        let scale: f64 = (0..n).map(|i| a[i][i] * a[i][i]).sum::<f64>().max(1e-300);
        if off <= 1e-30 * scale || off == 0.0 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| a[j][j].partial_cmp(&a[i][i]).unwrap_or(std::cmp::Ordering::Equal));
    let vals = order.iter().map(|&i| a[i][i]).collect();
    let vecs = order.iter().map(|&i| v.iter().map(|row| row[i]).collect()).collect();
    (vals, vecs)
}

/// `f(A)` for symmetric `A`, applying `f` to the eigenvalues; eigenvalues
/// at or below `tol * max` map to 0 (the pseudo-inverse convention).
fn sym_fn(a: &Rows, f: impl Fn(f64) -> f64) -> Rows {
    let n = a.len();
    let (vals, vecs) = sym_eig(a);
    let top = vals.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let tol = top * 1e-12 * n.max(1) as f64;
    let mut out = vec![vec![0.0; n]; n];
    for (lam, u) in vals.iter().zip(&vecs) {
        if *lam <= tol {
            continue;
        }
        let g = f(*lam);
        for i in 0..n {
            for j in 0..n {
                out[i][j] += g * u[i] * u[j];
            }
        }
    }
    out
}

/// Gauss-Jordan inverse with partial pivoting, for the small k x k systems.
fn inverse(a: &Rows) -> R<Rows> {
    let n = a.len();
    let mut m: Rows = a
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            row
        })
        .collect();
    for col in 0..n {
        let piv = (col..n)
            .max_by(|&i, &j| m[i][col].abs().partial_cmp(&m[j][col].abs()).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        if m[piv][col].abs() < 1e-14 {
            return e("singular matrix");
        }
        m.swap(col, piv);
        let p = m[col][col];
        m[col].iter_mut().for_each(|v| *v /= p);
        for r in 0..n {
            if r != col {
                let f = m[r][col];
                if f != 0.0 {
                    for c in 0..2 * n {
                        m[r][c] -= f * m[col][c];
                    }
                }
            }
        }
    }
    Ok(m.into_iter().map(|r| r[n..].to_vec()).collect())
}

/// Distinct labels in ascending order, and each row's class index.
fn classes(kind: &str, y: &[f64], n: usize) -> R<(Vec<f64>, Vec<usize>)> {
    if y.len() != n {
        return e(format!("{kind}: X has {n} rows but y has {} labels", y.len()));
    }
    if let Some(bad) = y.iter().position(|v| !v.is_finite()) {
        return e(format!("{kind}: label {} at index {bad} is not a finite number", y[bad]));
    }
    let mut cls: Vec<f64> = y.to_vec();
    cls.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cls.dedup();
    if cls.len() < 2 {
        return e(format!("{kind}: needs at least 2 classes in y, found {}", cls.len()));
    }
    let idx = y.iter().map(|v| cls.iter().position(|c| c == v).unwrap()).collect();
    Ok((cls, idx))
}

fn check_x(kind: &str, x: &Rows) -> R<usize> {
    if x.is_empty() || x[0].is_empty() {
        return e(format!("{kind}: X is empty"));
    }
    if x.iter().flatten().any(|v| !v.is_finite()) {
        return e(format!("{kind}: X contains a NaN or infinite value"));
    }
    Ok(x[0].len())
}

// ------------------------------------------------------------ LDA

pub fn lda_fit(x: &Matrix, y: &[f64]) -> R<Value> {
    let x = rows_of(x);
    let d = check_x("lda_model", &x)?;
    let n = x.len();
    let (cls, idx) = classes("lda_model", y, n)?;
    let k = cls.len();
    if n <= k {
        return e(format!("lda_model: needs more rows ({n}) than classes ({k})"));
    }
    let mut means = vec![vec![0.0; d]; k];
    let mut counts = vec![0usize; k];
    for (r, &c) in x.iter().zip(&idx) {
        counts[c] += 1;
        for (m, v) in means[c].iter_mut().zip(r) {
            *m += v;
        }
    }
    for (m, &cnt) in means.iter_mut().zip(&counts) {
        m.iter_mut().for_each(|v| *v /= cnt as f64);
    }
    let overall = mean_rows(&x);
    // Pooled within-class covariance (unbiased, n - k).
    let mut sw = vec![vec![0.0; d]; d];
    for (r, &c) in x.iter().zip(&idx) {
        let dv: Vec<f64> = r.iter().zip(&means[c]).map(|(a, b)| a - b).collect();
        for i in 0..d {
            for j in 0..d {
                sw[i][j] += dv[i] * dv[j];
            }
        }
    }
    sw.iter_mut().flatten().for_each(|v| *v /= (n - k) as f64);
    let mut sb = vec![vec![0.0; d]; d];
    for (m, &cnt) in means.iter().zip(&counts) {
        let dv: Vec<f64> = m.iter().zip(&overall).map(|(a, b)| a - b).collect();
        for i in 0..d {
            for j in 0..d {
                sb[i][j] += cnt as f64 * dv[i] * dv[j] / n as f64;
            }
        }
    }
    let sw_inv = sym_fn(&sw, |l| 1.0 / l);
    let sw_isqrt = sym_fn(&sw, |l| 1.0 / l.sqrt());
    // Discriminant axes: eigenvectors of Sw^-1/2 Sb Sw^-1/2, mapped back.
    let (vals, vecs) = sym_eig(&matmul(&matmul(&sw_isqrt, &sb), &sw_isqrt));
    let top = vals.first().copied().unwrap_or(0.0).max(0.0);
    let m_axes = vals.iter().take((k - 1).min(d)).filter(|&&l| l > top * 1e-12).count().max(1);
    let scalings: Rows = (0..d)
        .map(|i| (0..m_axes).map(|a| dot(&sw_isqrt[i], &vecs[a])).collect())
        .collect();
    let total: f64 = vals.iter().take(m_axes).map(|v| v.max(0.0)).sum();
    let ratio: Vec<f64> =
        vals.iter().take(m_axes).map(|v| if total > 0.0 { v.max(0.0) / total } else { 0.0 }).collect();
    let priors: Vec<f64> = counts.iter().map(|&c| c as f64 / n as f64).collect();
    Ok(Value::Model(Arc::new(ModelHandle::new(
        "lda",
        vec![
            ("classes".to_string(), vecv(cls)),
            ("priors".to_string(), vecv(priors)),
            ("means".to_string(), mat(&means)?),
            ("mean".to_string(), vecv(overall)),
            ("covariance".to_string(), mat(&sw)?),
            ("precision".to_string(), mat(&sw_inv)?),
            ("scalings".to_string(), mat(&scalings)?),
            ("explained_variance_ratio".to_string(), vecv(ratio)),
        ],
    ))))
}

fn lda_scores(m: &ModelHandle, x: &Rows) -> R<Rows> {
    let means = field_rows(m, "means")?;
    let prec = field_rows(m, "precision")?;
    let priors = field_vec(m, "priors")?;
    check_features("predict", x, prec.len())?;
    let coef: Rows = means.iter().map(|mu| prec.iter().map(|row| dot(row, mu)).collect()).collect();
    let bias: Vec<f64> = means
        .iter()
        .zip(&coef)
        .zip(&priors)
        .map(|((mu, w), p)| -0.5 * dot(mu, w) + p.ln())
        .collect();
    Ok(x.iter().map(|r| coef.iter().zip(&bias).map(|(w, b)| dot(r, w) + b).collect()).collect())
}

// ------------------------------------------------------------ QDA

pub fn qda_fit(x: &Matrix, y: &[f64], reg: f64) -> R<Value> {
    if !(0.0..=1.0).contains(&reg) {
        return e(format!("qda_model: reg={reg} must be between 0 and 1"));
    }
    let x = rows_of(x);
    let d = check_x("qda_model", &x)?;
    let n = x.len();
    let (cls, idx) = classes("qda_model", y, n)?;
    let k = cls.len();
    let mut fields = vec![("classes".to_string(), vecv(cls.clone()))];
    let mut priors = Vec::with_capacity(k);
    let mut means = Vec::with_capacity(k);
    let mut logdets = Vec::with_capacity(k);
    for c in 0..k {
        let rows: Rows = x.iter().zip(&idx).filter(|(_, &i)| i == c).map(|(r, _)| r.clone()).collect();
        let nc = rows.len();
        if nc < 2 {
            return e(format!("qda_model: class {} has {nc} row(s); each class needs at least 2", cls[c]));
        }
        let mu = mean_rows(&rows);
        let mut s = vec![vec![0.0; d]; d];
        for r in &rows {
            for i in 0..d {
                for j in 0..d {
                    s[i][j] += (r[i] - mu[i]) * (r[j] - mu[j]) / (nc - 1) as f64;
                }
            }
        }
        // `reg` shrinks each eigenvalue towards 1: (1 - reg) * lambda + reg,
        // the same regularization scikit-learn's `reg_param` applies.
        let (vals, vecs) = sym_eig(&s);
        let vals: Vec<f64> = vals.iter().map(|l| (1.0 - reg) * l.max(0.0) + reg).collect();
        let top = vals.first().copied().unwrap_or(0.0);
        if vals.iter().any(|&l| l <= top * 1e-12) {
            return e(format!(
                "qda_model: the covariance of class {} is singular (a constant feature, or \
                 {nc} rows for {d} features) -- pass reg= (e.g. reg=0.1) to regularize it",
                cls[c]
            ));
        }
        let mut prec = vec![vec![0.0; d]; d];
        for (l, u) in vals.iter().zip(&vecs) {
            for i in 0..d {
                for j in 0..d {
                    prec[i][j] += u[i] * u[j] / l;
                }
            }
        }
        logdets.push(vals.iter().map(|l| l.ln()).sum::<f64>());
        priors.push(nc as f64 / n as f64);
        fields.push((format!("precision_{c}"), mat(&prec)?));
        means.push(mu);
    }
    fields.push(("priors".to_string(), vecv(priors)));
    fields.push(("means".to_string(), mat(&means)?));
    fields.push(("log_determinants".to_string(), vecv(logdets)));
    fields.push(("reg".to_string(), Value::Num(reg)));
    Ok(Value::Model(Arc::new(ModelHandle::new("qda", fields))))
}

fn qda_scores(m: &ModelHandle, x: &Rows) -> R<Rows> {
    let means = field_rows(m, "means")?;
    let priors = field_vec(m, "priors")?;
    let logdets = field_vec(m, "log_determinants")?;
    check_features("predict", x, means[0].len())?;
    let precs: Vec<Rows> = (0..means.len()).map(|c| field_rows(m, &format!("precision_{c}"))).collect::<R<_>>()?;
    Ok(x.iter()
        .map(|r| {
            (0..means.len())
                .map(|c| {
                    let dv: Vec<f64> = r.iter().zip(&means[c]).map(|(a, b)| a - b).collect();
                    let q: f64 = precs[c].iter().zip(&dv).map(|(row, di)| di * dot(row, &dv)).sum();
                    -0.5 * logdets[c] - 0.5 * q + priors[c].ln()
                })
                .collect()
        })
        .collect())
}

fn argmax_labels(m: &ModelHandle, scores: &Rows) -> R<Value> {
    let cls = field_vec(m, "classes")?;
    Ok(vecv(
        scores
            .iter()
            .map(|s| {
                let best = (0..s.len()).max_by(|&i, &j| s[i].partial_cmp(&s[j]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);
                cls[best]
            })
            .collect(),
    ))
}

// ------------------------------------------------------------ PLS

/// PLS2 by NIPALS on centered (unscaled) data -- scale the columns first
/// (`scaler_model`) if they are in different units, as with PCA.
pub fn pls_fit(x: &Matrix, y: &Value, k: usize) -> R<Value> {
    let x = rows_of(x);
    let d = check_x("pls_model", &x)?;
    let n = x.len();
    let (y_rows, y_is_vec): (Rows, bool) = match y {
        Value::Mat(m) => (rows_of(m), false),
        other => (crate::to_vec(other)?.into_iter().map(|v| vec![v]).collect(), true),
    };
    if y_rows.len() != n {
        return e(format!("pls_model: X has {n} rows but Y has {}", y_rows.len()));
    }
    let q = y_rows[0].len();
    let max_k = d.min(n - 1).max(1);
    if k == 0 || k > max_k {
        return e(format!("pls_model: k={k} components must be between 1 and {max_k} (min(features, rows - 1))"));
    }
    let xm = mean_rows(&x);
    let ym = mean_rows(&y_rows);
    let mut xr: Rows = x.iter().map(|r| r.iter().zip(&xm).map(|(a, b)| a - b).collect()).collect();
    let mut yr: Rows = y_rows.iter().map(|r| r.iter().zip(&ym).map(|(a, b)| a - b).collect()).collect();
    let (mut w_cols, mut p_cols, mut c_cols) = (Vec::new(), Vec::new(), Vec::new());
    for comp in 0..k {
        // Start from the Y column with the most remaining variance.
        let start = (0..q)
            .max_by(|&a, &b| {
                let va: f64 = yr.iter().map(|r| r[a] * r[a]).sum();
                let vb: f64 = yr.iter().map(|r| r[b] * r[b]).sum();
                va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        let mut u: Vec<f64> = yr.iter().map(|r| r[start]).collect();
        let (mut w, mut t, mut c) = (vec![0.0; d], vec![0.0; n], vec![0.0; q]);
        for _ in 0..500 {
            w = (0..d).map(|j| xr.iter().zip(&u).map(|(r, ui)| r[j] * ui).sum()).collect();
            let wn = dot(&w, &w).sqrt();
            if wn == 0.0 {
                return e(format!(
                    "pls_model: X has no variance left to explain after {comp} component(s) -- use k={comp}"
                ));
            }
            w.iter_mut().for_each(|v| *v /= wn);
            t = xr.iter().map(|r| dot(r, &w)).collect();
            let tt = dot(&t, &t);
            c = (0..q).map(|j| yr.iter().zip(&t).map(|(r, ti)| r[j] * ti).sum::<f64>() / tt).collect();
            let cc = dot(&c, &c).max(1e-300);
            let u_new: Vec<f64> = yr.iter().map(|r| dot(r, &c) / cc).collect();
            let diff: f64 = u_new.iter().zip(&u).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
            let norm = dot(&u_new, &u_new).sqrt().max(1e-300);
            u = u_new;
            if q == 1 || diff / norm < 1e-12 {
                break;
            }
        }
        let tt = dot(&t, &t);
        let p: Vec<f64> = (0..d).map(|j| xr.iter().zip(&t).map(|(r, ti)| r[j] * ti).sum::<f64>() / tt).collect();
        for (r, ti) in xr.iter_mut().zip(&t) {
            for j in 0..d {
                r[j] -= ti * p[j];
            }
        }
        for (r, ti) in yr.iter_mut().zip(&t) {
            for j in 0..q {
                r[j] -= ti * c[j];
            }
        }
        w_cols.push(w);
        p_cols.push(p);
        c_cols.push(c);
    }
    let w = transpose(&w_cols); // d x k
    let p = transpose(&p_cols); // d x k
    let c = transpose(&c_cols); // q x k
    // Rotation R = W (P^T W)^-1 maps centered X straight to scores, and
    // the regression coefficients are B = R C^T.
    let ptw = matmul(&transpose(&p), &w);
    let rot = matmul(&w, &inverse(&ptw).map_err(|_| EvalError {
        msg: "pls_model: the component loadings are degenerate -- try a smaller k".into(),
    })?);
    let coef = matmul(&rot, &transpose(&c)); // d x q
    Ok(Value::Model(Arc::new(ModelHandle::new(
        "pls",
        vec![
            ("coef".to_string(), mat(&coef)?),
            ("x_mean".to_string(), vecv(xm)),
            ("y_mean".to_string(), vecv(ym)),
            ("x_weights".to_string(), mat(&w)?),
            ("x_loadings".to_string(), mat(&p)?),
            ("y_loadings".to_string(), mat(&c)?),
            ("x_rotations".to_string(), mat(&rot)?),
            ("y_is_vector".to_string(), Value::Bool(y_is_vec)),
        ],
    ))))
}

fn centered(m: &ModelHandle, x: &Rows, mean_field: &str) -> R<Rows> {
    let mu = field_vec(m, mean_field)?;
    check_features("predict", x, mu.len())?;
    Ok(x.iter().map(|r| r.iter().zip(&mu).map(|(a, b)| a - b).collect()).collect())
}

// ------------------------------------------------------------ ICA

pub fn ica_fit(x: &Matrix, k: usize, max_iter: usize, tol: f64, seed: u64) -> R<Value> {
    let x = rows_of(x);
    let d = check_x("ica_model", &x)?;
    let n = x.len();
    if k == 0 || k > d {
        return e(format!("ica_model: k={k} sources must be between 1 and the {d} feature(s)"));
    }
    if n < 2 {
        return e("ica_model: needs at least 2 rows");
    }
    let mu = mean_rows(&x);
    let xc: Rows = x.iter().map(|r| r.iter().zip(&mu).map(|(a, b)| a - b).collect()).collect();
    let mut cov = vec![vec![0.0; d]; d];
    for r in &xc {
        for i in 0..d {
            for j in 0..d {
                cov[i][j] += r[i] * r[j] / n as f64;
            }
        }
    }
    let (vals, vecs) = sym_eig(&cov);
    if vals[k - 1] <= vals[0].max(0.0) * 1e-12 {
        return e(format!(
            "ica_model: the data spans fewer than {k} independent directions -- use a smaller k"
        ));
    }
    // Whitening K (k x d): rows are eigenvectors scaled by 1/sqrt(lambda).
    let kw: Rows = (0..k).map(|a| vecs[a].iter().map(|v| v / vals[a].sqrt()).collect()).collect();
    let z: Rows = kw.iter().map(|row| xc.iter().map(|r| dot(row, r)).collect()).collect(); // k x n

    // Fixed, seeded starting rotation (splitmix64 -> uniform in [-1, 1)).
    let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut v = state;
        v = (v ^ (v >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        v = (v ^ (v >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        v ^= v >> 31;
        (v >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    let mut w: Rows = (0..k).map(|_| (0..k).map(|_| next()).collect()).collect();
    let decorrelate = |w: &Rows| -> Rows {
        let wwt = matmul(w, &transpose(w));
        matmul(&sym_fn(&wwt, |l| 1.0 / l.sqrt()), w)
    };
    w = decorrelate(&w);
    let mut converged = false;
    let mut iters = 0;
    for it in 0..max_iter {
        iters = it + 1;
        let wz = matmul(&w, &z); // k x n
        let mut w_new: Rows = Vec::with_capacity(k);
        for (i, row) in wz.iter().enumerate() {
            let g: Vec<f64> = row.iter().map(|v| v.tanh()).collect();
            let gp_mean: f64 = g.iter().map(|t| 1.0 - t * t).sum::<f64>() / n as f64;
            w_new.push((0..k).map(|j| dot(&g, &z[j]) / n as f64 - gp_mean * w[i][j]).collect());
        }
        let w_new = decorrelate(&w_new);
        let change = (0..k).map(|i| (dot(&w_new[i], &w[i]).abs() - 1.0).abs()).fold(0.0, f64::max);
        w = w_new;
        if change < tol {
            converged = true;
            break;
        }
    }
    if !converged {
        return e(format!(
            "ica_model: FastICA did not converge in {max_iter} iterations (tol={tol}) -- raise \
             max_iter=, loosen tol=, or try another seed=; the data may also have fewer than {k} \
             non-Gaussian sources"
        ));
    }
    let unmixing = matmul(&w, &kw); // k x d
    // Mixing = E_k diag(sqrt(lambda)) W^T (d x k), the exact inverse on
    // the whitened subspace because W is orthogonal.
    let ek_scaled: Rows = (0..d).map(|i| (0..k).map(|a| vecs[a][i] * vals[a].sqrt()).collect()).collect();
    let mixing = matmul(&ek_scaled, &transpose(&w));
    Ok(Value::Model(Arc::new(ModelHandle::new(
        "ica",
        vec![
            ("unmixing".to_string(), mat(&unmixing)?),
            ("mixing".to_string(), mat(&mixing)?),
            ("mean".to_string(), vecv(mu)),
            ("n_iter".to_string(), Value::Num(iters as f64)),
        ],
    ))))
}

// ------------------------------------------------------------ predict / transform

/// `predict(m, X)` for the kinds this module owns; `None` for any other.
pub fn predict(m: &ModelHandle, xnew: &Value) -> Option<R<Value>> {
    let run = || -> R<Value> {
        let x = rows_of(&xnew.to_matrix().map_err(|msg| EvalError { msg })?);
        match m.kind.as_str() {
            "lda" => argmax_labels(m, &lda_scores(m, &x)?),
            "qda" => argmax_labels(m, &qda_scores(m, &x)?),
            "pls" => {
                let xc = centered(m, &x, "x_mean")?;
                let coef = field_rows(m, "coef")?;
                let ym = field_vec(m, "y_mean")?;
                let yhat: Rows = matmul(&xc, &coef)
                    .into_iter()
                    .map(|r| r.iter().zip(&ym).map(|(a, b)| a + b).collect())
                    .collect();
                if matches!(m.field("y_is_vector"), Some(Value::Bool(true))) {
                    Ok(vecv(yhat.into_iter().map(|r| r[0]).collect()))
                } else {
                    mat(&yhat)
                }
            }
            "ica" => e("predict: an ica model has nothing to predict -- use transform(m, X) for the sources"),
            other => e(format!("predict: unexpected model kind `{other}`")),
        }
    };
    matches!(m.kind.as_str(), "lda" | "qda" | "pls" | "ica").then(run)
}

/// `transform(m, X)` for the kinds this module owns; `None` for any other.
pub fn transform(m: &ModelHandle, xnew: &Value) -> Option<R<Value>> {
    let run = || -> R<Value> {
        let x = rows_of(&xnew.to_matrix().map_err(|msg| EvalError { msg })?);
        match m.kind.as_str() {
            "lda" => mat(&matmul(&centered(m, &x, "mean")?, &field_rows(m, "scalings")?)),
            "pls" => mat(&matmul(&centered(m, &x, "x_mean")?, &field_rows(m, "x_rotations")?)),
            "ica" => mat(&matmul(&centered(m, &x, "mean")?, &transpose(&field_rows(m, "unmixing")?))),
            "qda" => e("transform: a qda model has no projection -- its classes are quadratic regions, not axes"),
            other => e(format!("transform: unexpected model kind `{other}`")),
        }
    };
    matches!(m.kind.as_str(), "lda" | "qda" | "pls" | "ica").then(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(rows: &[&[f64]]) -> Matrix {
        Matrix::from_rows(&rows.iter().map(|r| r.to_vec()).collect::<Vec<_>>()).unwrap()
    }

    fn model(v: R<Value>) -> Arc<ModelHandle> {
        match v.unwrap() {
            Value::Model(m) => m,
            _ => panic!("expected a model"),
        }
    }

    fn vals(v: Value) -> Vec<f64> {
        match v {
            Value::Vec(v) => v.to_vec(),
            Value::Mat(m) => m.as_slice().to_vec(),
            _ => panic!("expected numbers"),
        }
    }

    #[test]
    fn jacobi_eigen_reconstructs_a_symmetric_matrix() {
        let a = vec![vec![4.0, 1.0, 2.0], vec![1.0, 3.0, 0.5], vec![2.0, 0.5, 5.0]];
        let (vals, vecs) = sym_eig(&a);
        assert!(vals.windows(2).all(|w| w[0] >= w[1]));
        for i in 0..3 {
            for j in 0..3 {
                let r: f64 = (0..3).map(|k| vals[k] * vecs[k][i] * vecs[k][j]).sum();
                assert!((r - a[i][j]).abs() < 1e-12);
            }
        }
    }

    fn two_clouds() -> (Matrix, Vec<f64>) {
        // Two classes separated along x, with correlated within-class spread.
        let mut rows = Vec::new();
        let mut y = Vec::new();
        for i in 0..20 {
            let t = i as f64 / 19.0 - 0.5;
            rows.push(vec![t + 0.1 * (i % 3) as f64, 2.0 * t]);
            y.push(3.0);
            rows.push(vec![4.0 + t - 0.1 * (i % 2) as f64, 2.0 * t + 0.3]);
            y.push(7.0);
        }
        (Matrix::from_rows(&rows).unwrap(), y)
    }

    #[test]
    fn lda_and_qda_separate_two_clouds_and_keep_the_labels() {
        let (x, y) = two_clouds();
        let lda = model(lda_fit(&x, &y));
        let qda = model(qda_fit(&x, &y, 0.0));
        let xv = Value::Mat(Arc::new(x.clone()));
        assert_eq!(vals(predict(&lda, &xv).unwrap().unwrap()), y);
        assert_eq!(vals(predict(&qda, &xv).unwrap().unwrap()), y);
        let probe = Value::Mat(Arc::new(m(&[&[0.0, 0.0], &[4.2, 0.1]])));
        assert_eq!(vals(predict(&lda, &probe).unwrap().unwrap()), vec![3.0, 7.0]);
        // Two classes -> one discriminant axis.
        let proj = transform(&lda, &xv).unwrap().unwrap();
        assert!(matches!(&proj, Value::Mat(p) if p.shape() == (40, 1)));
    }

    #[test]
    fn qda_refuses_a_singular_class_and_names_reg() {
        let x = m(&[&[0.0, 1.0], &[1.0, 1.0], &[2.0, 1.0], &[5.0, 3.0], &[6.0, 4.0], &[7.0, 6.0]]);
        let err = qda_fit(&x, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 0.0).unwrap_err();
        assert!(err.msg.contains("reg="), "{}", err.msg);
        assert!(qda_fit(&x, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 0.1).is_ok());
    }

    #[test]
    fn pls_with_all_components_matches_least_squares() {
        // y = 2 x1 - 3 x2 + 1 exactly: full-rank PLS is OLS, so it recovers it.
        let x = m(&[&[1.0, 2.0], &[2.0, 1.0], &[3.0, 5.0], &[4.0, 3.0], &[5.0, 7.0], &[0.5, 0.2]]);
        let y: Vec<f64> = rows_of(&x).iter().map(|r| 2.0 * r[0] - 3.0 * r[1] + 1.0).collect();
        let pls = model(pls_fit(&x, &vecv(y.clone()), 2));
        let got = vals(predict(&pls, &Value::Mat(Arc::new(x.clone()))).unwrap().unwrap());
        for (g, w) in got.iter().zip(&y) {
            assert!((g - w).abs() < 1e-9, "{got:?} vs {y:?}");
        }
        let coef = vals(pls.field("coef").unwrap().clone());
        assert!((coef[0] - 2.0).abs() < 1e-9 && (coef[1] + 3.0).abs() < 1e-9, "{coef:?}");
    }

    #[test]
    fn ica_unmixes_two_mixed_sources() {
        // A square wave and a sawtooth, mixed by a known matrix.
        let n = 2000;
        let s: Rows = (0..n)
            .map(|i| {
                let t = i as f64 * 0.01;
                vec![if (t * 3.0).sin() >= 0.0 { 1.0 } else { -1.0 }, (t * 1.3) % 2.0 - 1.0]
            })
            .collect();
        let x: Rows = s.iter().map(|r| vec![r[0] + 0.6 * r[1], 0.4 * r[0] + r[1]]).collect();
        let ica = model(ica_fit(&Matrix::from_rows(&x).unwrap(), 2, 500, 1e-8, 0));
        let got = match transform(&ica, &mat(&x).unwrap()).unwrap().unwrap() {
            Value::Mat(m) => rows_of(&m),
            _ => unreachable!(),
        };
        // Each true source must match one recovered source up to sign and
        // scale: |correlation| close to 1.
        let corr = |a: &[f64], b: &[f64]| {
            let (ma, mb) = (a.iter().sum::<f64>() / n as f64, b.iter().sum::<f64>() / n as f64);
            let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
            let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
            let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
            (cov / (va * vb).sqrt()).abs()
        };
        for src in 0..2 {
            let truth: Vec<f64> = s.iter().map(|r| r[src]).collect();
            let best = (0..2)
                .map(|j| corr(&truth, &got.iter().map(|r| r[j]).collect::<Vec<_>>()))
                .fold(0.0, f64::max);
            assert!(best > 0.99, "source {src} recovered with |corr| {best}");
        }
    }
}
