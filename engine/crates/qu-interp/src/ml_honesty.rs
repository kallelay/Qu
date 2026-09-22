//! § the ML honesty layer (2026-09-16).
//!
//! `docs/design/toolkit-ml.md` §2 and §4. The gap analysis found `fit`,
//! `predict` and `pipeline` present and the honesty layer wholly absent:
//! no held-out metrics with intervals, no leakage-aware splitting, no
//! uncertainty on a prediction, no way to check whether a stated interval
//! actually covers.
//!
//! Everything here is a **pure data function** over vectors of numbers. It
//! deliberately does not reach into `fit`/`predict`: an honesty layer that
//! only works with this engine's own estimators would be unusable for the
//! case that matters most here — a model fitted elsewhere, or a physical
//! model like an equivalent circuit, whose predictions arrive as a vector.
//!
//! ## The three ideas
//!
//! **Splitting is structural.** `kfold` accepts `group=`, which keeps every
//! row sharing a group id on the same side of every fold. Random splitting
//! of grouped data (repeated measurements of one cell, one patient, one
//! device) is the most common way a reported score is optimistic, and it is
//! invisible in the score itself. `split_time` splits in order instead of
//! at random, for series where a random fold trains on the future.
//!
//! **A metric without an interval is a number pretending to be a result.**
//! `bootstrap_ci` resamples a paired `(y, yhat)` set and reports the metric's
//! distribution, so "R² = 0.91" becomes "0.91, 95% CI 0.78–0.96 on n=40",
//! which reads very differently.
//!
//! **A prediction is a distribution.** `conformal` turns held-out absolute
//! residuals into a half-width with a distribution-free coverage guarantee,
//! and `coverage` measures what an interval actually achieved so the claim
//! can be checked rather than trusted.

use crate::{e, style_entry, style_num, style_str, to_vec, EvalError, Value, R};
use std::sync::Arc;

/// `seed` arrives as its own parameter, NOT through `style`. Every
/// `Arg::Named("seed", _)` is special-cased out in `eval_call_args` before
/// `style` is built, so `style_num(&style, "seed")` always returns `None`
/// -- a `seed=` that is accepted, runs clean, and does nothing. That is
/// exactly what this module did in its first draft: five different seeds
/// produced one identical fold. `lib.rs` records the same mistake living
/// on in `particle_filter_init`.
pub fn call(f: &str, args: &[Value], style: &[(String, Value)], seed: Option<u64>) -> R<Value> {
    match f {
        "kfold" => kfold(args, style, seed),
        "split_time" => split_time(args, style),
        "r2" => r2(args),
        "accuracy" => accuracy(args, style),
        "roc_auc" => roc_auc(args),
        "brier" => brier(args),
        "conformal" => conformal(args, style),
        "coverage" => coverage(args),
        "residual_acf" => residual_acf(args, style),
        "bootstrap_ci" => bootstrap_ci(args, style, seed),
        other => e(format!("ml_honesty: unknown function `{other}`")),
    }
}

// --------------------------------------------------------------- helpers

fn vec_arg(args: &[Value], i: usize, who: &str, what: &str) -> R<Vec<f64>> {
    match args.get(i) {
        Some(v) => to_vec(v).map_err(|_| EvalError {
            msg: format!("{who}: {what} must be a vector, got {}", v.type_name()),
        }),
        None => e(format!("{who}: missing {what}")),
    }
}

fn same_len(a: &[f64], b: &[f64], who: &str, an: &str, bn: &str) -> R<()> {
    if a.len() != b.len() {
        return e(format!(
            "{who}: {an} has {} element{} but {bn} has {} -- a metric over \
             mismatched vectors would silently score the wrong pairs",
            a.len(),
            if a.len() == 1 { "" } else { "s" },
            b.len()
        ));
    }
    if a.is_empty() {
        return e(format!("{who}: {an} is empty, so there is nothing to score"));
    }
    Ok(())
}

/// Deterministic PRNG so every split and every bootstrap is reproducible
/// from its `seed=`. A resampling result that cannot be reproduced is not
/// evidence, and the platform RNG would make one.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

fn seed_of(seed: Option<u64>) -> u64 {
    seed.unwrap_or(0)
}

fn level_of(style: &[(String, Value)], who: &str) -> R<f64> {
    let lv = style_num(style, "level").unwrap_or(0.95);
    if !(lv > 0.0 && lv < 1.0) {
        return e(format!(
            "{who}: `level={lv}` must be strictly between 0 and 1 (0.95 means a 95% interval)"
        ));
    }
    Ok(lv)
}

fn idx_vec(ix: &[usize]) -> Value {
    Value::Vec(ix.iter().map(|i| *i as f64).collect::<Vec<f64>>().into())
}

/// Type-7 (linear interpolation) quantile, the convention R and numpy use
/// by default -- named because quantile conventions differ at small n and
/// an unstated one is a number nobody can reproduce.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let h = (sorted.len() as f64 - 1.0) * q;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

// -------------------------------------------------------------- splits

/// `kfold(n, k, [seed=], [group=])` -> a list of records with `train` and
/// `test` index vectors.
///
/// With `group=`, rows sharing a group id always land in the same fold.
/// That is the difference between an honest score and an optimistic one for
/// repeated measurements, and nothing in the score itself reveals which you
/// got.
fn kfold(args: &[Value], style: &[(String, Value)], seed: Option<u64>) -> R<Value> {
    let n = match args.first() {
        Some(Value::Num(x)) => *x as usize,
        Some(v) => to_vec(v)
            .map_err(|_| EvalError {
                msg: "kfold: first argument must be a row count or a vector".to_string(),
            })?
            .len(),
        None => return e("kfold: missing the row count".to_string()),
    };
    let k = match args.get(1) {
        Some(Value::Num(x)) => *x as usize,
        _ => 5usize,
    };
    if k < 2 {
        return e(format!("kfold: k={k} must be at least 2"));
    }
    if n < k {
        return e(format!(
            "kfold: {n} rows cannot be split into {k} folds -- a fold with no test \
             rows scores nothing while still reporting a number"
        ));
    }
    // `style_entry`, not a bare `style.iter()`: keyword arguments are
    // validated, and a key read without going through it is reported back
    // to the user as an unknown argument even though the builtin used it.
    let groups = match style_entry(style, "group") {
        None => None,
        Some((_, v)) => {
            let g = to_vec(v).map_err(|_| EvalError {
                msg: "kfold: `group=` must be a vector of group ids, one per row".to_string(),
            })?;
            if g.len() != n {
                return e(format!(
                    "kfold: `group=` has {} entries but there are {n} rows",
                    g.len()
                ));
            }
            Some(g)
        }
    };

    let mut rng = Rng::new(seed_of(seed));
    // Assign fold membership to whole groups when grouping, else to rows.
    let mut units: Vec<f64> = match &groups {
        None => (0..n).map(|i| i as f64).collect(),
        Some(g) => {
            let mut u = g.clone();
            u.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            u.dedup();
            u
        }
    };
    if units.len() < k {
        return e(format!(
            "kfold: {} distinct group{} cannot fill {k} folds -- with `group=` the \
             folds are made of whole groups, not rows",
            units.len(),
            if units.len() == 1 { "" } else { "s" }
        ));
    }
    // Fisher-Yates over the units.
    for i in (1..units.len()).rev() {
        let j = rng.below(i + 1);
        units.swap(i, j);
    }

    let mut folds = Vec::with_capacity(k);
    for f in 0..k {
        let members: Vec<f64> = units
            .iter()
            .enumerate()
            .filter(|(i, _)| i % k == f)
            .map(|(_, u)| *u)
            .collect();
        let mut train = Vec::new();
        let mut test = Vec::new();
        for row in 0..n {
            let unit = match &groups {
                None => row as f64,
                Some(g) => g[row],
            };
            if members.iter().any(|m| *m == unit) {
                test.push(row);
            } else {
                train.push(row);
            }
        }
        folds.push(Value::Record(Arc::new(vec![
            ("fold".to_string(), Value::Num(f as f64)),
            ("train".to_string(), idx_vec(&train)),
            ("test".to_string(), idx_vec(&test)),
        ])));
    }
    Ok(Value::List(Arc::new(folds)))
}

/// `split_time(n, horizon=)` -> `{train, test}` split in ORDER, not at
/// random: the last `horizon` rows are the test set. For a series, a random
/// split trains on the future and reports a score no deployment can reach.
fn split_time(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let n = match args.first() {
        Some(Value::Num(x)) => *x as usize,
        Some(v) => to_vec(v)
            .map_err(|_| EvalError {
                msg: "split_time: first argument must be a row count or a vector".to_string(),
            })?
            .len(),
        None => return e("split_time: missing the row count".to_string()),
    };
    let h = match args.get(1) {
        Some(Value::Num(x)) => *x as usize,
        _ => match style_num(style, "horizon") {
            Some(x) => x as usize,
            None => return e("split_time: needs a horizon (rows held out at the end)".to_string()),
        },
    };
    if h == 0 || h >= n {
        return e(format!(
            "split_time: horizon {h} must be between 1 and {} for {n} rows",
            n.saturating_sub(1)
        ));
    }
    let train: Vec<usize> = (0..n - h).collect();
    let test: Vec<usize> = (n - h..n).collect();
    Ok(Value::Record(Arc::new(vec![
        ("train".to_string(), idx_vec(&train)),
        ("test".to_string(), idx_vec(&test)),
    ])))
}

// ------------------------------------------------------------- metrics

fn r2(args: &[Value]) -> R<Value> {
    let y = vec_arg(args, 0, "r2", "the observed values")?;
    let yhat = vec_arg(args, 1, "r2", "the predicted values")?;
    same_len(&y, &yhat, "r2", "the observed values", "the predicted values")?;
    let mean = y.iter().sum::<f64>() / y.len() as f64;
    let ss_tot: f64 = y.iter().map(|v| (v - mean).powi(2)).sum();
    let ss_res: f64 = y.iter().zip(&yhat).map(|(a, b)| (a - b).powi(2)).sum();
    if ss_tot == 0.0 {
        return e(
            "r2: the observed values are all identical, so there is no variance to \
             explain and R² is undefined -- reporting 0 or 1 here would both be claims \
             the data cannot support"
                .to_string(),
        );
    }
    Ok(Value::Num(1.0 - ss_res / ss_tot))
}

/// Classification accuracy. `threshold=` (default 0.5) applies when the
/// predictions are scores rather than labels.
fn accuracy(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let y = vec_arg(args, 0, "accuracy", "the true labels")?;
    let p = vec_arg(args, 1, "accuracy", "the predictions")?;
    same_len(&y, &p, "accuracy", "the true labels", "the predictions")?;
    let t = style_num(style, "threshold").unwrap_or(0.5);
    let hits = y
        .iter()
        .zip(&p)
        .filter(|(a, b)| {
            let pred = if **b == 0.0 || **b == 1.0 { **b } else { (**b >= t) as i32 as f64 };
            (**a - pred).abs() < f64::EPSILON
        })
        .count();
    Ok(Value::Num(hits as f64 / y.len() as f64))
}

/// Area under the ROC curve, computed by the rank (Mann-Whitney) identity
/// with ties averaged -- exact, and not a trapezoid over a sampled curve.
fn roc_auc(args: &[Value]) -> R<Value> {
    let y = vec_arg(args, 0, "roc_auc", "the true labels")?;
    let s = vec_arg(args, 1, "roc_auc", "the scores")?;
    same_len(&y, &s, "roc_auc", "the true labels", "the scores")?;
    for (i, v) in y.iter().enumerate() {
        if *v != 0.0 && *v != 1.0 {
            return e(format!(
                "roc_auc: label {i} is `{v}` -- labels must be 0 or 1, and silently \
                 treating anything non-zero as positive would score a different problem"
            ));
        }
    }
    let npos = y.iter().filter(|v| **v == 1.0).count();
    let nneg = y.len() - npos;
    if npos == 0 || nneg == 0 {
        return e(format!(
            "roc_auc: the labels contain {npos} positive and {nneg} negative cases -- \
             AUC is undefined without both"
        ));
    }
    let mut order: Vec<usize> = (0..s.len()).collect();
    order.sort_by(|a, b| s[*a].partial_cmp(&s[*b]).unwrap_or(std::cmp::Ordering::Equal));
    // Average ranks within ties.
    let mut ranks = vec![0f64; s.len()];
    let mut i = 0usize;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && s[order[j + 1]] == s[order[i]] {
            j += 1;
        }
        let avg = ((i + j) as f64) / 2.0 + 1.0;
        for o in &order[i..=j] {
            ranks[*o] = avg;
        }
        i = j + 1;
    }
    let sum_pos: f64 = (0..y.len()).filter(|i| y[*i] == 1.0).map(|i| ranks[i]).sum();
    let auc = (sum_pos - (npos * (npos + 1)) as f64 / 2.0) / (npos * nneg) as f64;
    Ok(Value::Num(auc))
}

/// Brier score -- mean squared error of a probability. The calibration
/// companion to accuracy: a model can be accurate and badly calibrated.
fn brier(args: &[Value]) -> R<Value> {
    let y = vec_arg(args, 0, "brier", "the true labels")?;
    let p = vec_arg(args, 1, "brier", "the predicted probabilities")?;
    same_len(&y, &p, "brier", "the true labels", "the probabilities")?;
    for (i, v) in p.iter().enumerate() {
        if !(0.0..=1.0).contains(v) {
            return e(format!(
                "brier: probability {i} is `{v}`, outside 0..1 -- a score is not a probability"
            ));
        }
    }
    let s: f64 = y.iter().zip(&p).map(|(a, b)| (a - b).powi(2)).sum();
    Ok(Value::Num(s / y.len() as f64))
}

// --------------------------------------------------------- uncertainty

/// `conformal(residuals, [level=])` -> the half-width of a split-conformal
/// prediction interval.
///
/// Given absolute residuals from data the model did NOT train on, the
/// interval `prediction ± halfwidth` covers a new observation with at least
/// `level` probability, with no assumption about the error distribution.
/// The finite-sample correction `ceil((n+1)*level)/n` is what makes that a
/// guarantee rather than an approximation, and it is why this returns an
/// error for samples too small to support the level asked for.
fn conformal(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    // Kept signed and in order, because the exchangeability check below
    // needs the sequence: taking absolute values first destroys exactly the
    // structure that reveals a misspecified model.
    let signed: Vec<f64> = match args.get(1) {
        Some(_) => {
            let y = vec_arg(args, 0, "conformal", "the observed values")?;
            let yhat = vec_arg(args, 1, "conformal", "the predicted values")?;
            same_len(&y, &yhat, "conformal", "the observed values", "the predicted values")?;
            y.iter().zip(&yhat).map(|(a, b)| a - b).collect()
        }
        None => vec_arg(args, 0, "conformal", "the residuals")?,
    };
    let mut r = match args.get(1) {
        // Two vectors: treat as (y, yhat) and take residuals here, so the
        // caller cannot accidentally hand in signed residuals.
        Some(_) => {
            let y = vec_arg(args, 0, "conformal", "the observed values")?;
            let yhat = vec_arg(args, 1, "conformal", "the predicted values")?;
            same_len(&y, &yhat, "conformal", "the observed values", "the predicted values")?;
            y.iter().zip(&yhat).map(|(a, b)| (a - b).abs()).collect::<Vec<f64>>()
        }
        None => vec_arg(args, 0, "conformal", "the residuals")?
            .iter()
            .map(|x| x.abs())
            .collect(),
    };
    if r.is_empty() {
        return e("conformal: no residuals, so no interval can be justified".to_string());
    }

    // Conformal coverage rests on exchangeability. Residuals in a fixed
    // order (a frequency sweep, a time series) violate it exactly when the
    // MODEL is wrong -- and then the interval narrows, because correlated
    // residuals under-represent the spread of a fresh draw. So the band is
    // most confident precisely when it is least justified, which is the
    // failure this whole module exists to refuse.
    //
    // The 0.5 threshold is measured, not guessed. The ECM lane ran it over
    // an 18-class circuit library: 47 correctly-specified fits that reached
    // the noise floor, and 16 deliberately misspecified ones (a simpler
    // circuit fitted to a richer circuit's spectrum).
    //
    //                      good fits            wrong fits
    //     lag-1 mean        -0.034               +0.870
    //     lag-1 median      -0.042               +0.981
    //     lag-1 range       -0.259 .. +0.404     +0.034 .. +0.993
    //
    //     threshold 0.3 -> fires on 2.1% of good, catches 93.8% of wrong
    //     threshold 0.5 -> fires on 0.0% of good, catches 87.5% of wrong
    //
    // 0.5 sits in the gap between the worst good fit (+0.404) and the
    // median wrong one (+0.981). 0.3 buys six points of detection and costs
    // a false refusal in every fifty good fits, which is worse than it
    // sounds: a guard that fires on correct work trains people to pass
    // `assume_exchangeable` reflexively, and then the override stops
    // meaning anything.
    //
    // KNOWN BLIND SPOT, and it is the case a user is most likely to be in.
    // The weakest wrong fit measured +0.034 -- invisible to any threshold.
    // It was a NESTED pair: the wrong circuit was a special case of the
    // right one, so it absorbed most of the structure and left residuals
    // that look clean. This guard catches STRUCTURALLY wrong models well
    // and NESTED wrong models poorly, and moving the threshold cannot fix
    // that. Passing this check is evidence that the residuals are
    // exchangeable; it is not evidence that the model is right.
    let lag1 = acf_at(&signed, 1);
    let assumed = crate::style_entry(style, "assume_exchangeable")
        .map(|(_, v)| crate::truthy(v))
        .unwrap_or(false);
    if !assumed && lag1.abs() > 0.5 {
        return e(format!(
            "conformal: the residuals have lag-1 autocorrelation {lag1:.3}, so they are \
             not exchangeable and the interval would be narrower than the truth -- \
             correlated residuals under-represent a fresh draw, so the band gets MORE \
             confident as the model gets more wrong. This usually means the model is \
             misspecified rather than that the data is awkward. Fit the right model, or \
             pass `assume_exchangeable=true` to state that you have checked it yourself. \
             Note the converse does NOT hold: passing this check says the residuals are \
             exchangeable, not that the model is right -- a nested wrong model can absorb \
             the structure and leave clean-looking residuals."
        ));
    }

    let level = level_of(style, "conformal")?;
    let n = r.len();
    let k = ((n as f64 + 1.0) * level).ceil();
    if k > n as f64 {
        return e(format!(
            "conformal: {n} residuals cannot support a {:.0}% interval -- the \
             finite-sample rank ceil((n+1)*level) = {k} exceeds n. Use at least {} \
             calibration points, or a lower level. Returning the maximum residual \
             here would quietly report a weaker guarantee than the one asked for.",
            level * 100.0,
            (level / (1.0 - level)).ceil() as usize
        ));
    }
    r.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Value::Num(r[k as usize - 1]))
}


/// Autocorrelation at one lag, on the residual sequence as given. Used by
/// `conformal` to detect the model error that would make its interval
/// dishonest, and exposed as `residual_acf` so the same number can be
/// reported next to any interval.
fn acf_at(x: &[f64], lag: usize) -> f64 {
    if x.len() <= lag + 1 {
        return 0.0;
    }
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let denom: f64 = x.iter().map(|v| (v - mean).powi(2)).sum();
    if denom == 0.0 {
        return 0.0;
    }
    let num: f64 = (lag..x.len()).map(|i| (x[i] - mean) * (x[i - lag] - mean)).sum();
    num / denom
}

/// `residual_acf(residuals, [lag=1])` -- or `(y, yhat, [lag=])`.
///
/// Report it next to any interval you quote. A lag-1 much above ~0.5 means
/// the residuals carry structure the model did not, and every uncertainty
/// statement computed from them is optimistic.
fn residual_acf(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let r: Vec<f64> = match args.get(1) {
        Some(Value::Num(_)) | None => vec_arg(args, 0, "residual_acf", "the residuals")?,
        Some(_) => {
            let y = vec_arg(args, 0, "residual_acf", "the observed values")?;
            let yhat = vec_arg(args, 1, "residual_acf", "the predicted values")?;
            same_len(&y, &yhat, "residual_acf", "the observed values", "the predicted values")?;
            y.iter().zip(&yhat).map(|(a, b)| a - b).collect()
        }
    };
    let lag = match style_num(style, "lag") {
        None => 1usize,
        Some(l) => {
            if l < 1.0 || l.fract() != 0.0 {
                return e(format!("residual_acf: `lag={l}` must be a whole number >= 1"));
            }
            l as usize
        }
    };
    if r.len() <= lag + 1 {
        return e(format!(
            "residual_acf: {} residuals cannot support lag {lag}",
            r.len()
        ));
    }
    Ok(Value::Num(acf_at(&r, lag)))
}

/// `coverage(y, lo, hi)` -> the fraction of observations that actually fell
/// inside their stated interval. The check on the claim: a 95% interval
/// covering 71% of held-out points is a result, and without this it is
/// invisible.
fn coverage(args: &[Value]) -> R<Value> {
    let y = vec_arg(args, 0, "coverage", "the observed values")?;
    let lo = vec_arg(args, 1, "coverage", "the lower bounds")?;
    let hi = vec_arg(args, 2, "coverage", "the upper bounds")?;
    same_len(&y, &lo, "coverage", "the observed values", "the lower bounds")?;
    same_len(&y, &hi, "coverage", "the observed values", "the upper bounds")?;
    for i in 0..y.len() {
        if lo[i] > hi[i] {
            return e(format!(
                "coverage: interval {i} has lower bound {} above upper bound {} -- \
                 swapped bounds would report 0% coverage and look like a bad model",
                lo[i], hi[i]
            ));
        }
    }
    let inside = (0..y.len()).filter(|i| y[*i] >= lo[*i] && y[*i] <= hi[*i]).count();
    Ok(Value::Num(inside as f64 / y.len() as f64))
}

/// `bootstrap_ci(y, yhat, metric="r2", [level=], [n=], [seed=])` ->
/// `{value, lo, hi, n}`.
///
/// Resamples the paired observations to get the metric's sampling
/// distribution. This exists because a bare score invites a comparison the
/// sample cannot support: "0.91 vs 0.88" stops being interesting when both
/// intervals run from 0.7 to 0.97.
fn bootstrap_ci(args: &[Value], style: &[(String, Value)], seed: Option<u64>) -> R<Value> {
    let y = vec_arg(args, 0, "bootstrap_ci", "the observed values")?;
    let yhat = vec_arg(args, 1, "bootstrap_ci", "the predicted values")?;
    same_len(&y, &yhat, "bootstrap_ci", "the observed values", "the predicted values")?;
    let metric = style_str(style, "metric").unwrap_or_else(|| "r2".to_string());
    let level = level_of(style, "bootstrap_ci")?;
    let reps = style_num(style, "n").unwrap_or(2000.0) as usize;
    if reps < 100 {
        return e(format!(
            "bootstrap_ci: n={reps} resamples is too few for a stable interval -- use at least 100"
        ));
    }
    let score = |ys: &[f64], ps: &[f64]| -> Option<f64> {
        match metric.as_str() {
            "r2" => {
                let mean = ys.iter().sum::<f64>() / ys.len() as f64;
                let tot: f64 = ys.iter().map(|v| (v - mean).powi(2)).sum();
                if tot == 0.0 {
                    return None;
                }
                let res: f64 = ys.iter().zip(ps).map(|(a, b)| (a - b).powi(2)).sum();
                Some(1.0 - res / tot)
            }
            "rmse" => {
                let res: f64 = ys.iter().zip(ps).map(|(a, b)| (a - b).powi(2)).sum();
                Some((res / ys.len() as f64).sqrt())
            }
            "mae" => {
                let res: f64 = ys.iter().zip(ps).map(|(a, b)| (a - b).abs()).sum();
                Some(res / ys.len() as f64)
            }
            _ => None,
        }
    };
    if !matches!(metric.as_str(), "r2" | "rmse" | "mae") {
        return e(format!(
            "bootstrap_ci: `metric=\"{metric}\"` is not supported -- use \"r2\", \"rmse\" or \"mae\""
        ));
    }
    let point = score(&y, &yhat).ok_or_else(|| EvalError {
        msg: "bootstrap_ci: the observed values are all identical, so r2 is undefined".to_string(),
    })?;

    let mut rng = Rng::new(seed_of(seed));
    let n = y.len();
    let mut vals = Vec::with_capacity(reps);
    let mut skipped = 0usize;
    for _ in 0..reps {
        let mut ys = Vec::with_capacity(n);
        let mut ps = Vec::with_capacity(n);
        for _ in 0..n {
            let i = rng.below(n);
            ys.push(y[i]);
            ps.push(yhat[i]);
        }
        match score(&ys, &ps) {
            Some(v) => vals.push(v),
            None => skipped += 1,
        }
    }
    if vals.len() < reps / 2 {
        return e(format!(
            "bootstrap_ci: {skipped} of {reps} resamples were degenerate (no variance \
             in the observed values) -- the sample is too small or too flat for this metric"
        ));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha = (1.0 - level) / 2.0;
    Ok(Value::Record(Arc::new(vec![
        ("value".to_string(), Value::Num(point)),
        ("lo".to_string(), Value::Num(quantile(&vals, alpha))),
        ("hi".to_string(), Value::Num(quantile(&vals, 1.0 - alpha))),
        ("level".to_string(), Value::Num(level)),
        ("n".to_string(), Value::Num(n as f64)),
    ])))
}
