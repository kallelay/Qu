//! Parameter uncertainty, resampling and solver choice for the
//! least-squares fitters (`curve_fit`, `least_squares`, and the reporting
//! half of `circuit_fit`).
//!
//! The numerics that do not need a distribution live in
//! `qu_core::optimize` (`fit_covariance`, `gauss_newton`,
//! `gradient_descent`); this module adds what does -- Student-t p-values and
//! confidence intervals through `distributions::{t_cdf, t_inv}` -- plus the
//! residual bootstrap and the `summary(model)` table.
//!
//! Everything here is linearized inference: `cov = s^2 (J^T J)^+` at the
//! solution, `t = estimate / stderr` against `n - p` degrees of freedom.
//! Exact for a model linear in its parameters with iid normal errors (it is
//! then the textbook OLS table); an approximation otherwise, and the reason
//! `bootstrap=` exists -- when the two disagree, the linearization is the
//! one to distrust.

use crate::distributions::{t_cdf, t_inv};
use crate::{numeric, EvalError, ModelHandle, Rng, Value, R};
use numeric::matrix::Matrix;
use std::fmt::Write as _;
use std::sync::Arc;

/// Which solver a fit runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Method {
    Lm,
    GaussNewton,
    GradientDescent,
}

pub(crate) const METHOD_NAMES: &str = "\"lm\", \"gauss_newton\", \"gradient_descent\"";

impl Method {
    pub(crate) fn parse(func: &str, s: Option<String>) -> R<Method> {
        match s.as_deref() {
            None | Some("lm") => Ok(Method::Lm),
            Some("gauss_newton") => Ok(Method::GaussNewton),
            Some("gradient_descent") => Ok(Method::GradientDescent),
            Some(other) => Err(EvalError {
                msg: format!("{func}: unknown method=\"{other}\" -- valid methods are {METHOD_NAMES}"),
            }),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Method::Lm => "lm",
            Method::GaussNewton => "gauss_newton",
            Method::GradientDescent => "gradient_descent",
        }
    }
}

/// Which Levenberg-Marquardt `method="lm"` means: `curve_fit` and
/// `least_squares` have always run different LM implementations (the second
/// supports bounds), and `"lm"` keeps each one exactly as it was.
#[derive(Clone, Copy)]
pub(crate) enum LmFlavour {
    CurveFit,
    LeastSquares,
}

pub(crate) struct Solved {
    pub params: Vec<f64>,
    pub cost: f64,
    pub iterations: usize,
    pub converged: bool,
    /// The residual at `params`, when the solver hands it back.
    pub residual: Option<Vec<f64>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn solve(
    flavour: LmFlavour,
    method: Method,
    resid: &mut dyn FnMut(&[f64]) -> Vec<f64>,
    x0: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    max_iter: usize,
    tol: f64,
) -> R<Solved> {
    let err = |msg: String| EvalError { msg };
    let from_lm = |r: numeric::optimize::LmResult| Solved {
        params: r.params,
        cost: r.cost,
        iterations: r.iterations,
        converged: r.converged,
        residual: None,
    };
    match method {
        Method::Lm => match flavour {
            LmFlavour::CurveFit => numeric::optimize::levenberg_marquardt(resid, x0, max_iter, tol)
                .map(from_lm)
                .map_err(|e| err(e.to_string())),
            LmFlavour::LeastSquares => {
                let cell = std::cell::RefCell::new(resid);
                numeric::linalg::nonlinear_least_squares(|p| (*cell.borrow_mut())(p), x0, lower, upper, max_iter, tol)
                    .map(|r| Solved {
                        params: r.parameters,
                        cost: r.cost,
                        iterations: r.iterations,
                        converged: r.converged,
                        residual: Some(r.residual),
                    })
                    .map_err(|e| err(e.to_string()))
            }
        },
        Method::GaussNewton => numeric::optimize::gauss_newton(resid, x0, lower, upper, max_iter, tol)
            .map(from_lm)
            .map_err(|e| err(e.to_string())),
        Method::GradientDescent => numeric::optimize::gradient_descent(resid, x0, lower, upper, max_iter, tol)
            .map(from_lm)
            .map_err(|e| err(e.to_string())),
    }
}

fn vec_value(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

fn mat(rows: usize, cols: usize, col_major: Vec<f64>) -> Value {
    Value::Mat(Arc::new(Matrix::from_col_major(rows, cols, col_major)))
}

pub(crate) fn param_label(names: Option<&[String]>, j: usize) -> String {
    names.and_then(|n| n.get(j).cloned()).unwrap_or_else(|| format!("p[{j}]"))
}

/// `level=`, validated: a confidence level strictly between 0 and 1.
pub(crate) fn check_level(func: &str, level: f64) -> R<f64> {
    if level.is_finite() && level > 0.0 && level < 1.0 {
        Ok(level)
    } else {
        Err(EvalError { msg: format!("{func}: level= must be strictly between 0 and 1 (e.g. 0.95), got {level}") })
    }
}

/// Student-t columns from estimates and standard errors: `t`, two-sided
/// `p_values`, and the `(p, 2)` interval `estimate -/+ t_{(1+level)/2, dof} * stderr`.
/// All `NaN` when `dof <= 0`.
pub(crate) fn t_columns(params: &[f64], stderr: &[f64], dof: i64, level: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let p = params.len();
    if dof <= 0 {
        return (vec![f64::NAN; p], vec![f64::NAN; p], vec![f64::NAN; 2 * p]);
    }
    let nu = dof as f64;
    let tq = t_inv(0.5 + level / 2.0, nu);
    let t: Vec<f64> = params
        .iter()
        .zip(stderr)
        .map(|(b, s)| if s.is_infinite() { 0.0 } else { b / s })
        .collect();
    let pv: Vec<f64> = t.iter().map(|t| if t.is_nan() { f64::NAN } else { (2.0 * t_cdf(-t.abs(), nu)).min(1.0) }).collect();
    let mut ci = vec![0.0; 2 * p];
    for j in 0..p {
        ci[j] = params[j] - tq * stderr[j];
        ci[p + j] = params[j] + tq * stderr[j];
    }
    (t, pv, ci)
}

/// What [`uncertainty`] computed, beyond the model fields themselves.
pub(crate) struct Uncertainty {
    pub fields: Vec<(String, Value)>,
    pub residual: Vec<f64>,
    pub rss: f64,
}

/// The linearized-uncertainty fields every least-squares fit reports.
///
/// Never fails the fit: if the covariance cannot be computed (a Jacobian
/// that is NaN next to the solution, say) the statistics are `NaN` and
/// `note` says why.
pub(crate) fn uncertainty(
    resid: &mut dyn FnMut(&[f64]) -> Vec<f64>,
    params: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    level: f64,
    names: Option<&[String]>,
) -> Uncertainty {
    let p = params.len();
    let label = |j: usize| param_label(names, j);
    let mut notes: Vec<String> = Vec::new();
    let cov = numeric::optimize::fit_covariance(resid, params, lower, upper);
    let (residual, rss, dof, stderr, covm, corr, rank, at_bound) = match cov {
        Ok(c) => {
            if c.dof <= 0 {
                notes.push(format!(
                    "no uncertainty: {} residual(s) for {} parameter(s) leaves dof = {} <= 0, so the residual variance \
                     cannot be estimated -- stderr/cov/t/p_values/ci are NaN",
                    c.n, c.p, c.dof
                ));
            } else if c.rank < p {
                let bad: Vec<String> = (0..p).filter(|&j| !c.identifiable[j]).map(label).collect();
                notes.push(format!(
                    "J is rank-deficient (rank {} of {}): {} not identifiable from this data -- stderr = inf and \
                     cov rows/columns NaN for {}; the covariance of the rest uses the pseudo-inverse (J^T J)^+",
                    c.rank,
                    p,
                    if bad.len() == 1 { format!("{} is", bad[0]) } else { format!("{} are", bad.join(", ")) },
                    if bad.len() == 1 { "it" } else { "them" },
                ));
            }
            let pinned: Vec<String> = (0..p).filter(|&j| c.at_bound[j]).map(label).collect();
            if !pinned.is_empty() {
                notes.push(format!(
                    "{} on an active lower=/upper= bound: the linearized stderr/t/p_values/ci assume an interior \
                     optimum and do not hold there (see at_bound)",
                    if pinned.len() == 1 { format!("{} sits", pinned[0]) } else { format!("{} sit", pinned.join(", ")) }
                ));
            }
            (
                c.residual,
                c.rss,
                c.dof,
                c.stderr,
                c.cov.as_slice().to_vec(),
                c.correlation.as_slice().to_vec(),
                c.rank as f64,
                c.at_bound,
            )
        }
        Err(e) => {
            notes.push(format!("no uncertainty: {e}"));
            let r = resid(params);
            let rss: f64 = r.iter().map(|v| v * v).sum();
            let dof = r.len() as i64 - p as i64;
            (r, rss, dof, vec![f64::NAN; p], vec![f64::NAN; p * p], vec![f64::NAN; p * p], f64::NAN, vec![false; p])
        }
    };
    let n = residual.len();
    let (t, pv, ci) = t_columns(params, &stderr, dof, level);
    let sigma = if dof > 0 { (rss / dof as f64).sqrt() } else { f64::NAN };
    let fields = vec![
        ("level".to_string(), Value::Num(level)),
        ("nobs".to_string(), Value::Num(n as f64)),
        ("dof".to_string(), Value::Num(dof as f64)),
        ("rmse".to_string(), Value::Num((rss / n.max(1) as f64).sqrt())),
        ("sigma".to_string(), Value::Num(sigma)),
        ("stderr".to_string(), vec_value(stderr)),
        ("t".to_string(), vec_value(t)),
        ("p_values".to_string(), vec_value(pv)),
        ("ci".to_string(), mat(p, 2, ci)),
        ("cov".to_string(), mat(p, p, covm)),
        ("correlation".to_string(), mat(p, p, corr)),
        ("rank".to_string(), Value::Num(rank)),
        ("at_bound".to_string(), Value::List(Arc::new(at_bound.into_iter().map(Value::Bool).collect()))),
        ("note".to_string(), Value::Str(notes.join("; "))),
    ];
    Uncertainty { fields, residual, rss }
}

/// Linear-interpolation quantile (type 7, numpy's default) of sorted data.
fn quantile_sorted(s: &[f64], q: f64) -> f64 {
    if s.is_empty() {
        return f64::NAN;
    }
    let h = (s.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(s.len() - 1);
    s[lo] + (h - lo as f64) * (s[hi] - s[lo])
}

/// Residual bootstrap.
///
/// The fitted residuals are centred and inflated by `sqrt(n / dof)` (the
/// "modified residuals" of Davison & Hinkley, 1997, sec. 6.2 -- raw
/// residuals are systematically smaller than the errors they estimate,
/// by exactly that factor on average), resampled with replacement, and
/// added back to the fitted values; the model is refitted from the
/// solution each time. In residual terms, with `r = f(p) - y`, a
/// replicate's residual function is `r(p) - r_hat + e*`.
///
/// `refit(shift)` fits `r(p) + shift` and returns the parameters, `None`
/// when that replicate's fit failed numerically (it is dropped and not
/// counted), or an error when the user's function itself failed (which
/// fails the call).
pub(crate) fn bootstrap(
    func: &str,
    n_boot: usize,
    seed: u64,
    level: f64,
    r_hat: &[f64],
    p: usize,
    refit: &mut dyn FnMut(&[f64]) -> R<Option<Vec<f64>>>,
) -> R<Vec<(String, Value)>> {
    let n = r_hat.len();
    if n <= p {
        return Err(EvalError {
            msg: format!("{func}: bootstrap= needs more residuals than parameters ({n} residual(s), {p} parameter(s))"),
        });
    }
    let mean = r_hat.iter().sum::<f64>() / n as f64;
    let inflate = (n as f64 / (n - p) as f64).sqrt();
    let modified: Vec<f64> = r_hat.iter().map(|r| (r - mean) * inflate).collect();
    let mut rng = Rng::new(seed);
    let mut samples: Vec<Vec<f64>> = Vec::with_capacity(n_boot);
    let mut shift = vec![0.0; n];
    for _ in 0..n_boot {
        for i in 0..n {
            let k = rng.randi(0, n as i64 - 1) as usize;
            shift[i] = modified[k] - r_hat[i];
        }
        if let Some(params) = refit(&shift)? {
            if params.len() == p && params.iter().all(|v| v.is_finite()) {
                samples.push(params);
            }
        }
    }
    let b = samples.len();
    let (lo_q, hi_q) = (0.5 - level / 2.0, 0.5 + level / 2.0);
    let mut se = vec![f64::NAN; p];
    let mut ci = vec![f64::NAN; 2 * p];
    for j in 0..p {
        let mut col: Vec<f64> = samples.iter().map(|s| s[j]).collect();
        if b >= 2 {
            let m = col.iter().sum::<f64>() / b as f64;
            se[j] = (col.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (b - 1) as f64).sqrt();
        }
        col.sort_by(|a, b| a.total_cmp(b));
        ci[j] = quantile_sorted(&col, lo_q);
        ci[p + j] = quantile_sorted(&col, hi_q);
    }
    let mut draws = vec![0.0; b * p];
    for (i, s) in samples.iter().enumerate() {
        for j in 0..p {
            draws[j * b + i] = s[j];
        }
    }
    Ok(vec![
        ("bootstrap_n".to_string(), Value::Num(b as f64)),
        ("bootstrap_stderr".to_string(), vec_value(se)),
        ("bootstrap_ci".to_string(), mat(p, 2, ci)),
        ("bootstrap_params".to_string(), mat(b, p, draws)),
    ])
}

// ---------------------------------------------------------------- summary

/// A number for a fixed-width table cell: fixed-point in the ordinary
/// range, scientific outside it, always `width` characters.
fn cell(v: f64, width: usize) -> String {
    let s = if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "inf".to_string() } else { "-inf".to_string() }
    } else if v == 0.0 {
        "0".to_string()
    } else if v.abs() >= 1e-3 && v.abs() < 1e5 {
        let digits = 5 - (v.abs().log10().floor() as i32 + 1).clamp(0, 5);
        format!("{:.*}", digits.max(0) as usize, v)
    } else {
        format!("{v:.3e}")
    };
    format!("{s:>width$}")
}

fn p_cell(v: f64, width: usize) -> String {
    if v.is_finite() && v < 1e-4 {
        format!("{:>width$}", "<1e-4")
    } else if v.is_finite() {
        format!("{:>width$}", format!("{v:.4}"))
    } else {
        cell(v, width)
    }
}

fn nums(m: &ModelHandle, name: &str) -> Option<Vec<f64>> {
    match m.field(name)? {
        Value::Vec(v) => Some(v.to_vec()),
        Value::Mat(v) => Some(v.as_slice().to_vec()),
        Value::Num(x) => Some(vec![*x]),
        _ => None,
    }
}

fn num(m: &ModelHandle, name: &str) -> Option<f64> {
    match m.field(name)? {
        Value::Num(x) => Some(*x),
        _ => None,
    }
}

/// `summary(model)` for a least-squares fit: a statsmodels-style parameter
/// table (estimate, stderr, t, P>|t|, confidence interval), the fit's
/// headline numbers, bootstrap columns when the fit ran one, and `note`.
pub(crate) fn summary_text(m: &ModelHandle) -> R<String> {
    let params = nums(m, "params").ok_or_else(|| EvalError { msg: format!("summary: the {} model has no params", m.kind) })?;
    let p = params.len();
    let names: Option<Vec<String>> = match m.field("param_names") {
        Some(Value::List(l)) => Some(l.iter().map(crate::display_value).collect()),
        _ => None,
    };
    let get = |name: &str| nums(m, name).filter(|v| v.len() >= p).unwrap_or_else(|| vec![f64::NAN; p]);
    let stderr = get("stderr");
    let t = get("t");
    let pv = get("p_values");
    let ci = nums(m, "ci").filter(|v| v.len() == 2 * p).unwrap_or_else(|| vec![f64::NAN; 2 * p]);
    let level = num(m, "level").unwrap_or(0.95);
    let boot_se = nums(m, "bootstrap_stderr").filter(|v| v.len() == p);
    let boot_ci = nums(m, "bootstrap_ci").filter(|v| v.len() == 2 * p);

    let mut s = String::new();
    let method = match m.field("method") {
        Some(Value::Str(x)) => format!(", method = {x}"),
        _ => String::new(),
    };
    let converged = match m.field("converged") {
        Some(Value::Bool(b)) => format!(", converged = {b}"),
        _ => String::new(),
    };
    let _ = writeln!(s, "{} fit{method}{converged}", m.kind);
    let mut head: Vec<String> = Vec::new();
    for (label, key) in [("nobs", "nobs"), ("params", ""), ("dof", "dof"), ("rmse", "rmse"), ("sigma", "sigma"), ("R^2", "r_squared"), ("chi2_red", "chi2_red"), ("iterations", "iterations")] {
        if key.is_empty() {
            head.push(format!("params = {p}"));
        } else if let Some(v) = num(m, key) {
            // Counts are counts: `nobs = 8`, not `8.0000`.
            let shown = if matches!(key, "nobs" | "dof" | "iterations") && v.fract() == 0.0 && v.abs() < 1e15 {
                format!("{}", v as i64)
            } else {
                cell(v, 0)
            };
            head.push(format!("{label} = {shown}"));
        }
    }
    let _ = writeln!(s, "{}", head.join("   "));
    let wn = (0..p).map(|j| param_label(names.as_deref(), j).len()).max().unwrap_or(4).max(9);
    let lo_pct = format!("[{}", trim_pct(0.5 - level / 2.0));
    let hi_pct = format!("{}]", trim_pct(0.5 + level / 2.0));
    let _ = write!(s, "{:<wn$} {:>12} {:>12} {:>9} {:>8} {:>12} {:>12}", "", "estimate", "stderr", "t", "P>|t|", lo_pct, hi_pct);
    if boot_se.is_some() {
        let _ = write!(s, " {:>12}", "boot_se");
    }
    if boot_ci.is_some() {
        let q_lo = trim_pct(0.5 - level / 2.0);
        let q_hi = trim_pct(0.5 + level / 2.0);
        let _ = write!(s, " {:>12} {:>12}", format!("boot {q_lo}"), format!("boot {q_hi}"));
    }
    s.push('\n');
    for j in 0..p {
        let _ = write!(
            s,
            "{:<wn$} {} {} {} {} {} {}",
            param_label(names.as_deref(), j),
            cell(params[j], 12),
            cell(stderr[j], 12),
            cell(t[j], 9),
            p_cell(pv[j], 8),
            cell(ci[j], 12),
            cell(ci[p + j], 12),
        );
        if let Some(b) = &boot_se {
            let _ = write!(s, " {}", cell(b[j], 12));
        }
        if let Some(b) = &boot_ci {
            let _ = write!(s, " {} {}", cell(b[j], 12), cell(b[p + j], 12));
        }
        s.push('\n');
    }
    if let Some(Value::Str(note)) = m.field("note") {
        if !note.is_empty() {
            let _ = writeln!(s, "note: {note}");
        }
    }
    Ok(s.trim_end().to_string())
}

fn trim_pct(q: f64) -> String {
    let s = format!("{q:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}
