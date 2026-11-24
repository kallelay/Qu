//! ML and data-preparation builtins (added in v0.4.15): `silhouette`,
//! `silhouette_score`, `learning_curve`, `one_hot`, `label_encode`,
//! `label_decode`, `fillna`, `impute` and `sortperm`.
//!
//! `learning_curve` needs the interpreter (it calls the user's `fit`), so
//! it is an `impl Interp` method defined here and called from its arm in
//! `lib.rs`; the rest are pure functions of their arguments (`call`).
//!
//! Reading arguments goes through `arg_get` and `style_entry`/`style_num`/
//! `style_str`, which record what was read: a surplus positional argument
//! or an unread `name=value` is rejected by the interpreter afterwards, so
//! none of these builtins can silently ignore a misspelt option.
//!
//! Conventions shared by the group:
//! - `NaN` is the missing-value marker (`fillna`, `impute`).
//! - Multi-output builtins return a record (`one_hot` gives
//!   `{matrix, classes}`, `label_encode` gives `{codes, classes}`), like
//!   `kfold`, `cross_val` and `meshgrid`.
//! - A "label" vector is a numeric `Vec` or a list of strings; classes are
//!   always listed in sorted order (numbers ascending, strings in byte order).

use super::{
    arg_all, arg_get, collections, cv_metric_arg, e, metric_label, select_rows_value, shuffled_row_order,
    style_entry, style_num, to_cow, value_row_count, EvalError, Interp, Rng, Value, R,
};
use qu_core::matrix::Matrix;
use std::cmp::Ordering;
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "fillna", "impute", "label_decode", "label_encode", "one_hot", "silhouette", "silhouette_score",
    "sortperm",
];

pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "silhouette" => Ok(Value::Vec(Arc::new(silhouette_values("silhouette", args)?))),
        "silhouette_score" => {
            let s = silhouette_values("silhouette_score", args)?;
            Ok(Value::Num(s.iter().sum::<f64>() / s.len() as f64))
        }
        "one_hot" => one_hot(args),
        "label_encode" => label_encode(args),
        "label_decode" => label_decode(args),
        "fillna" => fillna(args),
        "impute" => impute(args),
        "sortperm" => sortperm(args),
        other => e(format!("ml_prep: unknown function `{other}`")),
    }
}

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn reject_complex(f: &str, what: &str, v: &Value) -> R<()> {
    if matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_)) {
        return e(format!("{f}: {what} must be real (complex input is not supported)"));
    }
    Ok(())
}

// ---------------------------------------------------------------- labels

/// A label vector: numbers or strings.
enum Labels {
    Num(Vec<f64>),
    Str(Vec<String>),
}

impl Labels {
    fn len(&self) -> usize {
        match self {
            Labels::Num(v) => v.len(),
            Labels::Str(v) => v.len(),
        }
    }
}

fn labels_of(f: &str, what: &str, v: &Value) -> R<Labels> {
    if let Value::List(items) = v {
        if !items.is_empty() && items.iter().all(|x| matches!(x, Value::Str(_))) {
            let strs = items
                .iter()
                .map(|x| match x {
                    Value::Str(s) => s.clone(),
                    _ => unreachable!("checked above"),
                })
                .collect();
            return Ok(Labels::Str(strs));
        }
        if items.iter().any(|x| matches!(x, Value::Str(_))) {
            return e(format!("{f}: {what} mixes strings with other values; use all numbers or all strings"));
        }
    }
    reject_complex(f, what, v)?;
    let xs = to_cow(v).map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") })?;
    if xs.iter().any(|x| x.is_nan()) {
        return e(format!("{f}: {what} contains NaN, which cannot be a class"));
    }
    Ok(Labels::Num(xs.into_owned()))
}

/// Maps each label to the index of its class in sorted class order.
/// Returns `(codes, classes)` with `classes` as a ready-made `Value`
/// (a `Vec` for numbers, a list of strings for strings).
fn encode(labels: &Labels) -> (Vec<usize>, Value) {
    match labels {
        Labels::Num(v) => {
            let mut classes: Vec<f64> = v.clone();
            // -0.0 and 0.0 are the same class
            classes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            classes.dedup_by(|a, b| *a == *b);
            let codes = v
                .iter()
                .map(|x| classes.partition_point(|c| c < x))
                .collect();
            (codes, Value::Vec(Arc::new(classes)))
        }
        Labels::Str(v) => {
            let mut classes: Vec<String> = v.clone();
            classes.sort();
            classes.dedup();
            let codes = v.iter().map(|x| classes.binary_search(x).unwrap_or(0)).collect();
            (codes, Value::List(Arc::new(classes.into_iter().map(Value::Str).collect())))
        }
    }
}

fn n_classes(classes: &Value) -> usize {
    match classes {
        Value::Vec(v) => v.len(),
        Value::List(l) => l.len(),
        _ => 0,
    }
}

fn record(fields: Vec<(&str, Value)>) -> Value {
    Value::Record(Arc::new(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

fn one_hot(args: &[Value]) -> R<Value> {
    let v = need("one_hot", "one_hot(labels)", args, 0)?;
    let labels = labels_of("one_hot", "labels", v)?;
    if labels.len() == 0 {
        return e("one_hot: labels is empty");
    }
    let (codes, classes) = encode(&labels);
    let (n, k) = (codes.len(), n_classes(&classes));
    let mut data = vec![0.0; n * k];
    for (i, &c) in codes.iter().enumerate() {
        data[c * n + i] = 1.0; // column-major: column c, row i
    }
    Ok(record(vec![
        ("matrix", Value::Mat(Arc::new(Matrix::from_col_major(n, k, data)))),
        ("classes", classes),
    ]))
}

fn label_encode(args: &[Value]) -> R<Value> {
    let v = need("label_encode", "label_encode(labels)", args, 0)?;
    let labels = labels_of("label_encode", "labels", v)?;
    if labels.len() == 0 {
        return e("label_encode: labels is empty");
    }
    let (codes, classes) = encode(&labels);
    Ok(record(vec![
        ("codes", Value::Vec(Arc::new(codes.into_iter().map(|c| c as f64).collect()))),
        ("classes", classes),
    ]))
}

fn label_decode(args: &[Value]) -> R<Value> {
    let sig = "label_decode(codes, classes)";
    let cv = need("label_decode", sig, args, 0)?;
    let classes = need("label_decode", sig, args, 1)?;
    reject_complex("label_decode", "codes", cv)?;
    let codes = to_cow(cv).map_err(|m| EvalError { msg: format!("label_decode: codes: {m}") })?;
    let k = match classes {
        Value::List(l) => l.len(),
        other => to_cow(other)
            .map_err(|m| EvalError { msg: format!("label_decode: classes: {m}") })?
            .len(),
    };
    let mut idx = Vec::with_capacity(codes.len());
    for (i, &c) in codes.iter().enumerate() {
        if !(c.is_finite() && c == c.trunc()) {
            return e(format!("label_decode: codes[{i}] = {c} is not a whole number"));
        }
        if c < 0.0 || c >= k as f64 {
            return e(format!(
                "label_decode: codes[{i}] = {c} is outside 0..{} (there are {k} classes)",
                k as i64 - 1
            ));
        }
        idx.push(c as usize);
    }
    match classes {
        Value::List(l) => Ok(Value::List(Arc::new(idx.iter().map(|&i| l[i].clone()).collect()))),
        other => {
            let cls = to_cow(other).map_err(|m| EvalError { msg: format!("label_decode: classes: {m}") })?;
            Ok(Value::Vec(Arc::new(idx.iter().map(|&i| cls[i]).collect())))
        }
    }
}

// ------------------------------------------------------------ silhouette

fn silhouette_values(f: &str, args: &[Value]) -> R<Vec<f64>> {
    let sig = format!("{f}(X, labels)");
    let xv = need(f, &sig, args, 0)?;
    let lv = need(f, &sig, args, 1)?;
    reject_complex(f, "X", xv)?;
    let x = xv.to_matrix().map_err(|m| EvalError { msg: format!("{f}: X: {m}") })?;
    let (n, d) = (x.rows(), x.cols());
    let data = x.as_slice();
    if data.iter().any(|v| v.is_nan()) {
        return e(format!("{f}: X contains NaN; impute or drop those rows first (see fillna and impute)"));
    }
    let labels = labels_of(f, "labels", lv)?;
    if labels.len() != n {
        return e(format!("{f}: X has {n} rows but labels has {} elements", labels.len()));
    }
    let (codes, classes) = encode(&labels);
    let k = n_classes(&classes);
    if k < 2 {
        return e(format!("{f}: needs at least 2 clusters, but labels holds only {k}"));
    }
    if k == n {
        return e(format!(
            "{f}: every point is in its own cluster ({n} clusters for {n} points), so the silhouette is undefined"
        ));
    }
    let mut size = vec![0usize; k];
    for &c in &codes {
        size[c] += 1;
    }
    let mut out = vec![0.0; n];
    let mut sums = vec![0.0; k];
    for i in 0..n {
        sums.iter_mut().for_each(|s| *s = 0.0);
        for j in 0..n {
            if j == i {
                continue;
            }
            let mut sq = 0.0;
            for c in 0..d {
                let diff = data[c * n + i] - data[c * n + j];
                sq += diff * diff;
            }
            sums[codes[j]] += sq.sqrt();
        }
        let own = codes[i];
        if size[own] == 1 {
            continue; // a singleton scores 0 (scikit-learn's convention)
        }
        let a = sums[own] / (size[own] - 1) as f64;
        let b = (0..k)
            .filter(|&c| c != own)
            .map(|c| sums[c] / size[c] as f64)
            .fold(f64::INFINITY, f64::min);
        let m = a.max(b);
        out[i] = if m > 0.0 { (b - a) / m } else { 0.0 };
    }
    Ok(out)
}

// ------------------------------------------------------ fillna / impute

/// The columns of a real vector or matrix (a vector is one column), with
/// the shape needed to put it back together.
struct Cols {
    cols: Vec<Vec<f64>>,
    is_vec: bool,
    signal: Option<(f64, Arc<super::SigMeta>)>,
}

fn cols_of(f: &str, v: &Value) -> R<Cols> {
    reject_complex(f, "x", v)?;
    match v {
        Value::Mat(m) => {
            let (r, c) = (m.rows(), m.cols());
            let cols = (0..c).map(|j| m.as_slice()[j * r..(j + 1) * r].to_vec()).collect();
            Ok(Cols { cols, is_vec: false, signal: None })
        }
        Value::Signal(xs, fs, meta) => Ok(Cols {
            cols: vec![xs.to_vec()],
            is_vec: true,
            signal: Some((*fs, meta.clone())),
        }),
        other => {
            let xs = to_cow(other).map_err(|m| EvalError { msg: format!("{f}: x: {m}") })?;
            Ok(Cols { cols: vec![xs.into_owned()], is_vec: true, signal: None })
        }
    }
}

impl Cols {
    fn into_value(self) -> Value {
        if self.is_vec {
            let xs = Arc::new(self.cols.into_iter().next().unwrap_or_default());
            match self.signal {
                Some((fs, meta)) => Value::Signal(xs, fs, meta),
                None => Value::Vec(xs),
            }
        } else {
            let r = self.cols.first().map_or(0, |c| c.len());
            let c = self.cols.len();
            let data: Vec<f64> = self.cols.into_iter().flatten().collect();
            Value::Mat(Arc::new(Matrix::from_col_major(r, c, data)))
        }
    }
}

fn fillna(args: &[Value]) -> R<Value> {
    let sig = "fillna(x, value)";
    let xv = need("fillna", sig, args, 0)?;
    let val = need("fillna", sig, args, 1)?;
    let mut cols = cols_of("fillna", xv)?;
    reject_complex("fillna", "value", val)?;
    let per_col: Vec<f64> = match val {
        Value::Num(v) => vec![*v; cols.cols.len()],
        other => {
            let vs = to_cow(other).map_err(|m| EvalError { msg: format!("fillna: value: {m}") })?;
            if cols.is_vec {
                return e(format!(
                    "fillna: x is a vector, so value must be a single number (got {} values)",
                    vs.len()
                ));
            }
            if vs.len() != cols.cols.len() {
                return e(format!(
                    "fillna: x has {} columns but value has {} entries (give one number, or one per column)",
                    cols.cols.len(),
                    vs.len()
                ));
            }
            vs.into_owned()
        }
    };
    for (col, fill) in cols.cols.iter_mut().zip(per_col) {
        for x in col.iter_mut() {
            if x.is_nan() {
                *x = fill;
            }
        }
    }
    Ok(cols.into_value())
}

fn median_sorted(s: &[f64]) -> f64 {
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// Most frequent value; ties go to the smallest value. `s` is sorted and
/// non-empty.
fn mode_sorted(s: &[f64]) -> f64 {
    let (mut best, mut best_n) = (s[0], 0usize);
    let mut i = 0;
    while i < s.len() {
        let mut j = i;
        while j < s.len() && s[j] == s[i] {
            j += 1;
        }
        if j - i > best_n {
            best = s[i];
            best_n = j - i;
        }
        i = j;
    }
    best
}

fn impute(args: &[Value]) -> R<Value> {
    let sig = "impute(x, strategy)";
    let xv = need("impute", sig, args, 0)?;
    let sv = need("impute", sig, args, 1)?;
    let strategy = match sv {
        Value::Str(s) => s.as_str(),
        other => {
            return e(format!(
                "impute: strategy must be a string (\"mean\", \"median\", \"most_frequent\" or \"zero\"), found {}",
                other.type_name()
            ))
        }
    };
    if !["mean", "median", "most_frequent", "zero"].contains(&strategy) {
        return e(format!(
            "impute: unknown strategy \"{strategy}\" -- use \"mean\", \"median\", \"most_frequent\" or \"zero\""
        ));
    }
    let mut cols = cols_of("impute", xv)?;
    let many = !cols.is_vec;
    for (j, col) in cols.cols.iter_mut().enumerate() {
        if !col.iter().any(|x| x.is_nan()) {
            continue;
        }
        let mut seen: Vec<f64> = col.iter().copied().filter(|x| !x.is_nan()).collect();
        let fill = if strategy == "zero" {
            0.0
        } else if seen.is_empty() {
            let where_ = if many { format!("column {} of x", j + 1) } else { "x".to_string() };
            return e(format!(
                "impute: {where_} has no observed values (all NaN), so its {strategy} is undefined; \
                 drop it or use fillna with a value of your own"
            ));
        } else if strategy == "mean" {
            seen.iter().sum::<f64>() / seen.len() as f64
        } else {
            seen.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            if strategy == "median" {
                median_sorted(&seen)
            } else {
                mode_sorted(&seen)
            }
        };
        for x in col.iter_mut() {
            if x.is_nan() {
                *x = fill;
            }
        }
    }
    Ok(cols.into_value())
}

// -------------------------------------------------------------- sortperm

/// NaN sorts as the largest value; -0.0 and 0.0 are equal.
fn cmp_num(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

fn sortperm(args: &[Value]) -> R<Value> {
    let sig = "sortperm(x, [desc])";
    let xv = need("sortperm", sig, args, 0)?;
    let desc = match arg_get(args, 1) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return e(format!("sortperm: desc must be true or false, found {}", other.type_name()))
        }
    };
    let mut idx: Vec<usize>;
    match xv {
        Value::List(items) if !items.is_empty() && items.iter().all(|x| matches!(x, Value::Str(_))) => {
            let strs: Vec<&str> = items
                .iter()
                .map(|x| match x {
                    Value::Str(s) => s.as_str(),
                    _ => unreachable!("checked above"),
                })
                .collect();
            idx = (0..strs.len()).collect();
            if desc {
                idx.sort_by(|&a, &b| strs[b].cmp(strs[a]));
            } else {
                idx.sort_by(|&a, &b| strs[a].cmp(strs[b]));
            }
        }
        other => {
            reject_complex("sortperm", "x", other)?;
            if let Value::Mat(m) = other {
                if m.rows() > 1 && m.cols() > 1 {
                    return e("sortperm: x is a matrix; pass a vector (or a row or column of it)");
                }
            }
            let xs = to_cow(other).map_err(|m| EvalError { msg: format!("sortperm: x: {m}") })?;
            idx = (0..xs.len()).collect();
            // sort_by is stable, so equal elements keep their original order
            // in both directions
            if desc {
                idx.sort_by(|&a, &b| cmp_num(xs[b], xs[a]));
            } else {
                idx.sort_by(|&a, &b| cmp_num(xs[a], xs[b]));
            }
        }
    }
    Ok(Value::Vec(Arc::new(idx.into_iter().map(|i| i as f64).collect())))
}

// -------------------------------------------------------- learning_curve

impl Interp {
    /// `learning_curve(fit, X, y, [extra...], [sizes=], [folds=5],
    /// [shuffle=true], [seed=], [metric="auto"])`.
    ///
    /// Same argument shape as `cross_val`. Rows are dealt into `folds`
    /// folds exactly as `cross_val` does (same seed gives the same folds);
    /// for each fold the model is fitted on the first `s` rows of the other
    /// folds (taken in the shuffled order, so each size's training set
    /// contains the smaller one) for every `s` in `sizes`, and scored on
    /// those same `s` rows (train score) and on the held-out fold
    /// (validation score).
    pub(crate) fn learning_curve(
        &mut self,
        args: &[Value],
        style: &[(String, Value)],
        seed: Option<u64>,
    ) -> R<Value> {
        const SIG: &str = "learning_curve(fit, X, y, [extra...], [sizes=], [folds=5], [metric=])";
        let all = arg_all(args).to_vec();
        if all.len() < 3 {
            return e(format!("{SIG} needs a fit function, X and y"));
        }
        let fit = collections::callable_arg(self, &all, 0, "learning_curve", "swept")?;
        let y = super::to_vec(&all[2]).map_err(|m| EvalError { msg: format!("learning_curve: y: {m}") })?;
        let n = value_row_count(&all[1])?;
        if y.len() != n {
            return e(format!("learning_curve: X has {n} rows but y has {} elements", y.len()));
        }
        let k = match style_entry(style, "folds") {
            None => 5,
            Some((_, v)) => match v.as_num() {
                Ok(x) if x.is_finite() && x == x.trunc() && x >= 0.0 => x as usize,
                _ => return e("learning_curve: folds must be a whole number"),
            },
        };
        if k < 2 || k > n {
            return e(format!("learning_curve: folds={k} must be between 2 and the number of rows ({n})"));
        }
        let shuffle = style_num(style, "shuffle").map(|v| v != 0.0).unwrap_or(true);
        let metric = cv_metric_arg(style, "learning_curve")?;

        // fold assignment: the same deal as `cross_val`'s, but keeping the order
        let order: Vec<usize> = if shuffle {
            let mut local;
            let rng: &mut Rng = match seed {
                Some(s) => {
                    local = Rng::new(s);
                    &mut local
                }
                None => &mut self.rng,
            };
            shuffled_row_order(rng, n)
        } else {
            (0..n).collect()
        };
        let (base, rem) = (n / k, n % k);
        let mut fold_of = vec![0usize; n];
        let mut pos = 0;
        for fold in 0..k {
            let len = base + usize::from(fold < rem);
            for &row in &order[pos..pos + len] {
                fold_of[row] = fold;
            }
            pos += len;
        }
        // the smallest training set any fold has: sizes are measured against it
        // so every fold uses the same row counts
        let m = n - (base + usize::from(rem > 0));

        let sizes = learning_sizes(style, m)?;
        let s_count = sizes.len();
        let mut train = vec![0.0; s_count * k];
        let mut val = vec![0.0; s_count * k];
        for fold in 0..k {
            let train_rows: Vec<usize> = order.iter().copied().filter(|&r| fold_of[r] != fold).collect();
            let test_idx: Vec<usize> = (0..n).filter(|&r| fold_of[r] == fold).collect();
            let x_test = select_rows_value(&all[1], &test_idx)?;
            let y_test = Value::Vec(Arc::new(test_idx.iter().map(|&r| y[r]).collect()));
            for (si, &s) in sizes.iter().enumerate() {
                let sub = &train_rows[..s];
                let x_sub = select_rows_value(&all[1], sub)?;
                let y_sub = Value::Vec(Arc::new(sub.iter().map(|&r| y[r]).collect()));
                let mut argv = vec![x_sub.clone(), y_sub.clone()];
                argv.extend(all[3..].iter().cloned());
                let model = self.apply(&fit, argv, Vec::new())?;
                let tr = self.lc_score(&model, x_sub, y_sub, &metric, fold, "training")?;
                let va = self.lc_score(&model, x_test.clone(), y_test.clone(), &metric, fold, "validation")?;
                train[fold * s_count + si] = tr;
                val[fold * s_count + si] = va;
            }
        }
        let summarize = |scores: &[f64]| -> (Vec<f64>, Vec<f64>) {
            let mut means = Vec::with_capacity(s_count);
            let mut stds = Vec::with_capacity(s_count);
            for si in 0..s_count {
                let row: Vec<f64> = (0..k).map(|f| scores[f * s_count + si]).collect();
                let (mu, sd) = super::ml_eval::mean_std(&row);
                means.push(mu);
                stds.push(sd);
            }
            (means, stds)
        };
        let (tm, ts) = summarize(&train);
        let (vm, vs) = summarize(&val);
        let vecv = |v: Vec<f64>| Value::Vec(Arc::new(v));
        Ok(record(vec![
            ("sizes", vecv(sizes.iter().map(|&s| s as f64).collect())),
            ("train_mean", vecv(tm)),
            ("train_std", vecv(ts)),
            ("val_mean", vecv(vm)),
            ("val_std", vecv(vs)),
            ("train_scores", Value::Mat(Arc::new(Matrix::from_col_major(s_count, k, train)))),
            ("val_scores", Value::Mat(Arc::new(Matrix::from_col_major(s_count, k, val)))),
            ("folds", Value::Num(k as f64)),
            ("metric", Value::Str(metric_label(&metric))),
        ]))
    }

    fn lc_score(&mut self, model: &Value, x: Value, y: Value, metric: &str, fold: usize, which: &str) -> R<f64> {
        let value = if metric == "auto" {
            self.apply("score", vec![model.clone(), x, y], Vec::new())?
        } else {
            let predicted = self.apply("predict", vec![model.clone(), x], Vec::new())?;
            self.apply(metric, vec![y, predicted], Vec::new())?
        };
        value.as_num().map_err(|msg| EvalError {
            msg: format!("learning_curve: the {which} score of fold {} is not a number ({msg})", fold + 1),
        })
    }
}

/// Resolves `sizes=` to whole row counts against `m`, the smallest training
/// set. Every entry at most 1 means fractions of `m`; otherwise every entry
/// must be a whole number of rows no larger than `m`.
fn learning_sizes(style: &[(String, Value)], m: usize) -> R<Vec<usize>> {
    let raw: Vec<f64> = match style_entry(style, "sizes") {
        None => vec![0.2, 0.4, 0.6, 0.8, 1.0],
        Some((_, v)) => to_cow(v)
            .map_err(|msg| EvalError { msg: format!("learning_curve: sizes: {msg}") })?
            .into_owned(),
    };
    if raw.is_empty() {
        return e("learning_curve: sizes is empty");
    }
    if let Some(bad) = raw.iter().find(|x| !x.is_finite() || **x <= 0.0) {
        return e(format!("learning_curve: sizes must be positive numbers, found {bad}"));
    }
    if raw.iter().all(|&x| x <= 1.0) {
        return Ok(raw.iter().map(|&f| ((f * m as f64).round() as usize).clamp(1, m)).collect());
    }
    let mut out = Vec::with_capacity(raw.len());
    for &x in &raw {
        if x != x.trunc() {
            return e(format!(
                "learning_curve: sizes mixes fractions and row counts (found {x}); give either all fractions \
                 in (0, 1] or all whole numbers of rows"
            ));
        }
        if x > m as f64 {
            return e(format!(
                "learning_curve: a training size of {x} rows is larger than the {m} rows the smallest \
                 training fold has"
            ));
        }
        out.push(x as usize);
    }
    Ok(out)
}
