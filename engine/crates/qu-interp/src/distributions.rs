//! § probability distributions: cdf / inverse cdf / pdf (2026-09-28).
//!
//! Qu had `chi2cdf`, `chi2pdf` and `normpdf` and nothing else: no normal
//! CDF, no Student t, no F, no inverse of any of them -- so a confidence
//! interval, a critical value or a p-value meant leaving the language. This
//! module is the family, under the names MATLAB and Octave use (`normcdf`/
//! `norminv`, `tcdf`/`tinv`, `chi2inv`, `fcdf`/`finv`, `gamcdf`/`gaminv`,
//! `betacdf`/`betainv`, `expcdf`/`expinv`), because those are the names a
//! reader of any statistics text already knows, and the parameterisations
//! they use: `gam*(x, a, b)` is shape `a` and SCALE `b` (mean `a*b`), and
//! `exp*(x, mu)` is the MEAN `mu`, not a rate.
//!
//! Every function broadcasts over a vector/matrix first argument like the
//! existing `chi2cdf`, and the parameters are scalars.
//!
//! **Numerics.** The normal CDF is `erfc` (libm), so the far tail keeps its
//! relative precision instead of rounding `1 - tiny` to 1. The t, F and
//! beta CDFs go through the regularized incomplete beta function (Lentz's
//! continued fraction, the standard construction), gamma and chi-square
//! through the regularized lower incomplete gamma already in `qu_core`.
//! Inverses: `norminv` is Acklam's rational approximation polished with
//! one Halley step (about 1e-15 relative); the rest invert their CDF by
//! Newton steps safeguarded by bisection inside a bracket that always
//! contains the answer, so a bad Newton step can slow convergence but never
//! walk off to a wrong root.
//!
//! **Domain.** A probability outside `[0, 1]` or a parameter outside its
//! range is an error naming it, not a NaN -- `tinv(1.2, 5)` is a mistake in
//! the caller's code and saying so is the useful answer. `p = 0` and
//! `p = 1` return the support's ends (`-inf`/`inf` for the normal and t,
//! `0`/`inf` for chi-square, F, gamma and exponential, `0`/`1` for beta).

use crate::{arg0, arg_get, e, map1, numeric, EvalError, Value, R};

pub const NAMES: &[&str] = &[
    "normcdf", "norminv", "tcdf", "tinv", "tpdf", "chi2inv", "fcdf", "finv", "fpdf", "gamcdf",
    "gaminv", "gampdf", "betacdf", "betainv", "betapdf", "expcdf", "expinv", "exppdf",
];

pub fn call(f: &str, args: &[Value]) -> R<Value> {
    if FAMILY_NAMES.contains(&f) {
        return family_call(f, args);
    }
    let x = arg0(args)?.clone();
    match f {
        "normcdf" | "norminv" => {
            let mu = param_or(args, 1, f, "mu", 0.0)?;
            let sigma = param_or(args, 2, f, "sigma", 1.0)?;
            positive(f, "sigma", sigma)?;
            if f == "normcdf" {
                map1(x, move |x| norm_cdf((x - mu) / sigma))
            } else {
                check_probs(f, &x)?;
                map1(x, move |p| mu + sigma * norm_inv(p))
            }
        }
        "tcdf" | "tinv" | "tpdf" => {
            let nu = param(args, 1, f, "nu")?;
            positive(f, "nu (degrees of freedom)", nu)?;
            match f {
                "tcdf" => map1(x, move |t| t_cdf(t, nu)),
                "tpdf" => map1(x, move |t| t_pdf(t, nu)),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| t_inv(p, nu))
                }
            }
        }
        "chi2inv" => {
            let k = param(args, 1, f, "k")?;
            positive(f, "k (degrees of freedom)", k)?;
            check_probs(f, &x)?;
            map1(x, move |p| gamma_inv(p, k / 2.0, 2.0))
        }
        "fcdf" | "finv" | "fpdf" => {
            let d1 = param(args, 1, f, "d1")?;
            let d2 = param(args, 2, f, "d2")?;
            positive(f, "d1 (numerator degrees of freedom)", d1)?;
            positive(f, "d2 (denominator degrees of freedom)", d2)?;
            match f {
                "fcdf" => map1(x, move |x| f_cdf(x, d1, d2)),
                "fpdf" => map1(x, move |x| f_pdf(x, d1, d2)),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| f_inv(p, d1, d2))
                }
            }
        }
        "gamcdf" | "gaminv" | "gampdf" => {
            let a = param(args, 1, f, "a (shape)")?;
            let b = param_or(args, 2, f, "b (scale)", 1.0)?;
            positive(f, "a (shape)", a)?;
            positive(f, "b (scale)", b)?;
            match f {
                "gamcdf" => map1(x, move |x| gamma_cdf(x, a, b)),
                "gampdf" => map1(x, move |x| gamma_pdf(x, a, b)),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| gamma_inv(p, a, b))
                }
            }
        }
        "betacdf" | "betainv" | "betapdf" => {
            let a = param(args, 1, f, "a")?;
            let b = param(args, 2, f, "b")?;
            positive(f, "a", a)?;
            positive(f, "b", b)?;
            match f {
                "betacdf" => map1(x, move |x| beta_cdf(x, a, b)),
                "betapdf" => map1(x, move |x| beta_pdf(x, a, b)),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| beta_inv(p, a, b))
                }
            }
        }
        "expcdf" | "expinv" | "exppdf" => {
            let mu = param_or(args, 1, f, "mu (mean)", 1.0)?;
            positive(f, "mu (mean)", mu)?;
            match f {
                "expcdf" => map1(x, move |x| if x <= 0.0 { 0.0 } else { -(-x / mu).exp_m1() }),
                "exppdf" => map1(x, move |x| if x < 0.0 { 0.0 } else { (-x / mu).exp() / mu }),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| if p >= 1.0 { f64::INFINITY } else { -mu * (-p).ln_1p() })
                }
            }
        }
        other => e(format!("distributions: unknown function `{other}`")),
    }
}

// ------------------------------------------------------------ arguments

fn param(args: &[Value], i: usize, f: &str, name: &str) -> R<f64> {
    match arg_get(args, i) {
        Some(v) => v.as_num().map_err(|m| EvalError { msg: format!("{f}: `{name}` {m}") }),
        None => e(format!("{f}: missing argument {} `{name}`", i + 1)),
    }
}

fn param_or(args: &[Value], i: usize, f: &str, name: &str, default: f64) -> R<f64> {
    match arg_get(args, i) {
        Some(_) => param(args, i, f, name),
        None => Ok(default),
    }
}

fn positive(f: &str, name: &str, v: f64) -> R<()> {
    if v > 0.0 && v.is_finite() {
        Ok(())
    } else {
        e(format!("{f}: {name} must be positive and finite, got {v}"))
    }
}

/// Every probability handed to an inverse must lie in `[0, 1]`; a value
/// outside is a bug in the caller's code, so it is named, not NaN'd.
fn check_probs(f: &str, x: &Value) -> R<()> {
    let bad = |p: f64| !(0.0..=1.0).contains(&p);
    let first_bad = match x {
        Value::Num(p) => bad(*p).then_some(*p),
        Value::Vec(v) => v.iter().copied().find(|&p| bad(p)),
        Value::Mat(m) => m.as_slice().iter().copied().find(|&p| bad(p)),
        _ => None,
    };
    match first_bad {
        Some(p) => e(format!("{f}: a probability must be between 0 and 1, got {p}")),
        None => Ok(()),
    }
}

// ------------------------------------------------------------ normal

pub(crate) fn norm_cdf(z: f64) -> f64 {
    0.5 * libm::erfc(-z / std::f64::consts::SQRT_2)
}

fn norm_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Acklam's inverse-normal approximation (relative error < 1.2e-9), then
/// one Halley step against `erfc`, which brings it to full precision.
pub(crate) fn norm_inv(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2,
        1.383577518672690e2, -3.066479806614716e1, 2.506628277459239,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2,
        6.680131188771972e1, -1.328068155288572e1,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838,
        -2.549732539343734, 4.374664141464968, 2.938163982698783,
    ];
    const D: [f64; 4] = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996, 3.754408661907416];
    let plow = 0.02425;
    let x = if p < plow {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - plow {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // Halley refinement on `cdf(x) - p`. For `x > 0` that is computed as
    // `(1 - p) - Q(x)` so the upper tail keeps its digits (`1 - p` is exact
    // for `p >= 0.5`, and `Q(x) = norm_cdf(-x)` never rounds to 1).
    let err = if x > 0.0 { (1.0 - p) - norm_cdf(-x) } else { norm_cdf(x) - p };
    let u = err / norm_pdf(x);
    x - u / (1.0 + x * u / 2.0)
}

// ------------------------------------------------------------ gamma family

fn ln_gamma(x: f64) -> f64 {
    numeric::special::ln_gamma(x)
}

fn gamma_cdf(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else if x.is_infinite() {
        1.0
    } else {
        numeric::special::regularized_lower_incomplete_gamma(a, x / b)
    }
}

fn gamma_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    if x == 0.0 {
        return if a < 1.0 { f64::INFINITY } else if a == 1.0 { 1.0 / b } else { 0.0 };
    }
    ((a - 1.0) * (x / b).ln() - x / b - ln_gamma(a) - b.ln()).exp()
}

fn gamma_inv(p: f64, a: f64, b: f64) -> f64 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    // Wilson-Hilferty in the body; in the lower tail, where it breaks down
    // for small shapes, the leading term of the series, P(a, x) ~
    // x^a / Gamma(a + 1), inverted exactly.
    let z = norm_inv(p);
    let t = 1.0 - 1.0 / (9.0 * a) + z / (3.0 * a.sqrt());
    let wh = a * t * t * t;
    let tail = ((p.ln() + ln_gamma(a + 1.0)) / a).exp();
    let guess = if wh > 0.0 && wh > tail { wh } else { tail };
    invert(p, guess * b, 0.0, f64::INFINITY, |x| gamma_cdf(x, a, b), |x| gamma_pdf(x, a, b))
}

// ------------------------------------------------------------ beta family

/// Regularized incomplete beta `I_x(a, b)`, by the continued fraction on
/// whichever side converges fast (`x < (a+1)/(a+b+2)`), the symmetry
/// `I_x(a,b) = 1 - I_{1-x}(b,a)` otherwise.
pub(crate) fn inc_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    if x < (a + 1.0) / (a + b + 2.0) {
        ln_front.exp() * beta_cf(x, a, b) / a
    } else {
        1.0 - ln_front.exp() * beta_cf(1.0 - x, b, a) / b
    }
}

/// Lentz's method for the incomplete-beta continued fraction.
fn beta_cf(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=10_000 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    h
}

fn beta_cdf(x: f64, a: f64, b: f64) -> f64 {
    inc_beta(x, a, b)
}

fn beta_pdf(x: f64, a: f64, b: f64) -> f64 {
    if !(0.0..=1.0).contains(&x) {
        return 0.0;
    }
    if x == 0.0 || x == 1.0 {
        let (edge, shape) = if x == 0.0 { (a, b) } else { (b, a) };
        // With the edge's own exponent 0, the density there is 1/B(1, other)
        // = the other shape parameter.
        return if edge < 1.0 {
            f64::INFINITY
        } else if edge == 1.0 {
            shape
        } else {
            0.0
        };
    }
    ((a - 1.0) * x.ln() + (b - 1.0) * (1.0 - x).ln() + ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b)).exp()
}

fn beta_inv(p: f64, a: f64, b: f64) -> f64 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    // Solve the upper half through the mirror distribution, so the answer's
    // distance from 1 is computed directly instead of as `1 - x` after `x`
    // has already rounded.
    if p > 0.5 {
        return 1.0 - beta_inv_lower(1.0 - p, b, a);
    }
    beta_inv_lower(p, a, b)
}

fn beta_inv_lower(p: f64, a: f64, b: f64) -> f64 {
    // Near 0, I_x(a, b) ~ x^a / (a B(a, b)): invert that for the start.
    let ln_beta = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
    let tail = ((p.ln() + a.ln() + ln_beta) / a).exp();
    let mean = a / (a + b);
    let guess = if tail < mean { tail } else { mean };
    invert(p, guess, 0.0, 1.0, |x| beta_cdf(x, a, b), |x| beta_pdf(x, a, b))
}

// ------------------------------------------------------------ Student t

pub(crate) fn t_cdf(t: f64, nu: f64) -> f64 {
    if t.is_infinite() {
        return if t > 0.0 { 1.0 } else { 0.0 };
    }
    let tail = 0.5 * inc_beta(nu / (nu + t * t), nu / 2.0, 0.5);
    if t > 0.0 {
        1.0 - tail
    } else {
        tail
    }
}

fn t_pdf(t: f64, nu: f64) -> f64 {
    (ln_gamma((nu + 1.0) / 2.0) - ln_gamma(nu / 2.0) - 0.5 * (nu * std::f64::consts::PI).ln()
        - (nu + 1.0) / 2.0 * (1.0 + t * t / nu).ln())
    .exp()
}

pub(crate) fn t_inv(p: f64, nu: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    if p == 0.5 {
        return 0.0;
    }
    // Symmetric: solve on the lower half, where the tail keeps its digits,
    // through the incomplete beta directly -- `x = I^-1_{2p}(nu/2, 1/2)`,
    // `t = -sqrt(nu (1-x) / x)`.
    let lower = p.min(1.0 - p);
    let x = beta_inv(2.0 * lower, nu / 2.0, 0.5);
    let t = -(nu * (1.0 - x) / x).sqrt();
    if p < 0.5 {
        t
    } else {
        -t
    }
}

// ------------------------------------------------------------ F

fn f_cdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return 1.0;
    }
    inc_beta(d1 * x / (d1 * x + d2), d1 / 2.0, d2 / 2.0)
}

fn f_pdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    if x == 0.0 {
        return if d1 < 2.0 { f64::INFINITY } else if d1 == 2.0 { 1.0 } else { 0.0 };
    }
    let ln = 0.5 * (d1 * (d1 * x).ln() + d2 * d2.ln() - (d1 + d2) * (d1 * x + d2).ln())
        - x.ln()
        - (ln_gamma(d1 / 2.0) + ln_gamma(d2 / 2.0) - ln_gamma((d1 + d2) / 2.0));
    ln.exp()
}

fn f_inv(p: f64, d1: f64, d2: f64) -> f64 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    // F = (d2/d1) * y / (1 - y) with y ~ Beta(d1/2, d2/2). In the upper
    // half solve for 1 - y ~ Beta(d2/2, d1/2) directly, so `1 - y` keeps
    // its digits when y is close to 1.
    if p > 0.5 {
        let one_minus_y = beta_inv(1.0 - p, d2 / 2.0, d1 / 2.0);
        return d2 * (1.0 - one_minus_y) / (d1 * one_minus_y);
    }
    let y = beta_inv(p, d1 / 2.0, d2 / 2.0);
    d2 * y / (d1 * (1.0 - y))
}

// ------------------------------------------------------------ inversion

/// Solve `cdf(x) = p` on `(lo, hi)` (`hi` may be infinite) by Newton steps
/// from `guess`, each kept inside a bracket that is tightened as it goes
/// and replaced by bisection whenever Newton would leave it.
fn invert(p: f64, guess: f64, lo: f64, hi: f64, cdf: impl Fn(f64) -> f64, pdf: impl Fn(f64) -> f64) -> f64 {
    let (mut lo, mut hi) = (lo, hi);
    // Make an infinite upper end finite: double until the CDF passes `p`.
    if hi.is_infinite() {
        let mut h = guess.max(1.0);
        while cdf(h) < p {
            lo = h;
            h *= 2.0;
            if !h.is_finite() {
                return f64::INFINITY;
            }
        }
        hi = h;
    }
    let mut x = guess.clamp(lo, hi);
    if !(x > lo && x < hi) {
        x = 0.5 * (lo + hi);
    }
    for _ in 0..2000 {
        let fx = cdf(x) - p;
        if fx == 0.0 {
            return x;
        }
        if fx < 0.0 {
            lo = x;
        } else {
            hi = x;
        }
        let d = pdf(x);
        let mut next = if d > 0.0 && d.is_finite() { x - fx / d } else { f64::NAN };
        if !(next > lo && next < hi) {
            // Bisect on a log scale when the bracket spans decades, so a
            // root at 1e-120 is minutes of halvings away, not thousands.
            next = if lo > 0.0 && hi / lo > 4.0 {
                (lo * hi).sqrt()
            } else if lo == 0.0 && hi > 1e-300 {
                hi * 1e-4
            } else {
                0.5 * (lo + hi)
            };
        }
        if (next - x).abs() <= 1e-15 * x.abs().max(1e-300) || hi - lo <= 1e-15 * hi.abs().max(1e-300) {
            return next;
        }
        x = next;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: f64, want: f64, rel: f64) {
        assert!(
            (got - want).abs() <= rel * want.abs().max(1e-300),
            "got {got}, want {want} (rel {:.2e})",
            (got - want).abs() / want.abs()
        );
    }

    /// Textbook critical values -- what a table in the back of a
    /// statistics book says, which is what a user will check against.
    #[test]
    fn critical_values_match_the_tables() {
        close(norm_cdf(1.96), 0.9750021048517795, 1e-14);
        close(norm_inv(0.975), 1.959963984540054, 1e-14);
        close(norm_inv(1e-10), -6.361340902404056, 1e-12);
        close(t_inv(0.975, 10.0), 2.2281388519649385, 1e-10);
        close(t_inv(0.975, 1.0), 12.706204736174698, 1e-10);
        close(t_cdf(2.0, 5.0), 0.9490302605850709, 1e-10);
        close(gamma_inv(0.95, 0.5, 2.0), 3.841458820694124, 1e-10); // chi2inv(0.95, 1)
        close(gamma_inv(0.95, 5.0, 2.0), 18.307038053275146, 1e-10); // chi2inv(0.95, 10)
        close(gamma_inv(0.05, 1.0, 2.0), 0.10258658877510106, 1e-10); // chi2inv(0.05, 2)
        close(f_inv(0.95, 5.0, 10.0), 3.325834530413011, 1e-9);
        close(f_inv(0.95, 1.0, 1.0), 161.44763879758855, 1e-9);
        close(beta_cdf(0.5, 2.0, 3.0), 11.0 / 16.0, 1e-14);
    }

    #[test]
    fn every_inverse_undoes_its_cdf() {
        let ps = [1e-12, 1e-6, 0.001, 0.05, 0.3, 0.5, 0.7, 0.95, 0.999, 1.0 - 1e-9];
        let mut bad = Vec::new();
        // `cdf(inv(p)) == p` to `rel`, OR `p` lies between the CDF at the
        // representable numbers either side of the answer -- when the
        // answer is 1 - 1e-30 no double can do better, and no
        // implementation can either.
        let mut chk = |what: String, x: f64, cdf: &dyn Fn(f64) -> f64, want: f64, rel: f64| {
            let got = cdf(x);
            let eps = 8.0 * f64::EPSILON;
            let (lo, hi) = (cdf(x - eps * x.abs().max(1e-300)), cdf(x + eps * x.abs().max(1e-300)));
            let bracketed = lo.min(hi) <= want && want <= lo.max(hi);
            if !((got - want).abs() <= rel * want.abs()) && !bracketed {
                bad.push(format!("{what}: x={x:e} got {got:e}, want {want:e}"));
            }
        };
        for &p in &ps {
            chk(format!("norm p={p}"), norm_inv(p), &norm_cdf, p, 1e-12);
            for &nu in &[0.5, 1.0, 3.0, 30.0, 1e4] {
                chk(format!("t p={p} nu={nu}"), t_inv(p, nu), &|t| t_cdf(t, nu), p, 1e-9);
            }
            for &a in &[0.1, 0.5, 1.0, 2.5, 50.0] {
                chk(format!("gamma p={p} a={a}"), gamma_inv(p, a, 3.0), &|x| gamma_cdf(x, a, 3.0), p, 1e-9);
                for &b in &[0.3, 1.0, 4.0] {
                    chk(format!("beta p={p} a={a} b={b}"), beta_inv(p, a, b), &|x| beta_cdf(x, a, b), p, 1e-9);
                }
            }
            for &(d1, d2) in &[(1.0, 1.0), (5.0, 10.0), (30.0, 2.0)] {
                chk(format!("F p={p} d1={d1} d2={d2}"), f_inv(p, d1, d2), &|x| f_cdf(x, d1, d2), p, 1e-9);
            }
        }
        assert!(bad.is_empty(), "{} round trips off:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn densities_integrate_to_their_cdf() {
        // Trapezoid from a to b of the pdf == cdf(b) - cdf(a).
        let check = |pdf: &dyn Fn(f64) -> f64, cdf: &dyn Fn(f64) -> f64, a: f64, b: f64| {
            let n = 20_000;
            let h = (b - a) / n as f64;
            let mut s = 0.5 * (pdf(a) + pdf(b));
            for i in 1..n {
                s += pdf(a + i as f64 * h);
            }
            close(s * h, cdf(b) - cdf(a), 1e-6);
        };
        check(&|t| t_pdf(t, 4.0), &|t| t_cdf(t, 4.0), -3.0, 2.0);
        check(&|x| f_pdf(x, 5.0, 7.0), &|x| f_cdf(x, 5.0, 7.0), 0.2, 4.0);
        check(&|x| gamma_pdf(x, 2.5, 1.5), &|x| gamma_cdf(x, 2.5, 1.5), 0.1, 9.0);
        check(&|x| beta_pdf(x, 2.0, 3.5), &|x| beta_cdf(x, 2.0, 3.5), 0.05, 0.95);
    }

    #[test]
    fn support_ends() {
        assert_eq!(norm_inv(0.0), f64::NEG_INFINITY);
        assert_eq!(t_inv(1.0, 3.0), f64::INFINITY);
        assert_eq!(gamma_inv(0.0, 2.0, 1.0), 0.0);
        assert_eq!(beta_inv(1.0, 2.0, 2.0), 1.0);
        assert_eq!(t_inv(0.5, 7.0), 0.0);
    }
}

// ================================================== the whole family
//
// § the rest of the distribution chart (2026-09-29): every family on the
// usual "relationships between distributions" chart -- uniform (continuous
// and discrete), Bernoulli, binomial, geometric, negative binomial,
// Poisson, hypergeometric, normal, log-normal, exponential, Weibull,
// gamma, beta, chi-square, Student t (and F) -- each with the same five
// MATLAB-named operations: `*pdf` (the pmf for a discrete family), `*cdf`,
// `*inv` (smallest support point whose CDF reaches `p` for a discrete
// family), `*rnd` (in `lib.rs`, next to the RNG), `*stat` (a result with
// `.mean`/`.var`), and `*fit` for the families with a standard maximum
// likelihood fit (a result with the named parameters, `.params`, `.n` and
// `.loglik`). Parameterisations are MATLAB's: `geo`/`nbin` count FAILURES
// before the (r-th) success, `wbl(a, b)` is scale `a` and shape `b`,
// `hyge(M, K, N)` draws `N` from `M` items of which `K` are successes,
// `logn(mu, sigma)` are the mean and deviation of `log(x)`.

/// pdf / cdf / inv / stat / fit names handled by [`family_call`].
pub const FAMILY_NAMES: &[&str] = &[
    "unifpdf", "unifcdf", "unifinv", "unidpdf", "unidcdf", "unidinv", "bernpdf", "berncdf", "berninv",
    "binopdf", "binocdf", "binoinv", "geopdf", "geocdf", "geoinv", "nbinpdf", "nbincdf", "nbininv",
    "poisspdf", "poisscdf", "poissinv", "hygepdf", "hygecdf", "hygeinv", "lognpdf", "logncdf", "logninv",
    "wblpdf", "wblcdf", "wblinv", "unifstat", "unidstat", "bernstat", "binostat", "geostat", "nbinstat",
    "poisstat", "hygestat", "normstat", "lognstat", "expstat", "wblstat", "gamstat", "betastat",
    "chi2stat", "tstat", "fstat", "normfit", "lognfit", "expfit", "gamfit", "wblfit", "betafit",
    "unifit", "poissfit", "binofit", "geofit", "nbinfit", "bernfit",
];

/// The samplers, dispatched from `lib.rs` (they need the interpreter's
/// RNG and `seed=`): parameters first, then optional `rows`, `cols`.
pub const RND_NAMES: &[&str] = &[
    "unifrnd", "unidrnd", "bernrnd", "binornd", "geornd", "nbinrnd", "poissrnd", "hygernd", "normrnd",
    "lognrnd", "exprnd", "wblrnd", "gamrnd", "betarnd", "chi2rnd", "trnd", "frnd",
];

#[derive(Clone, Copy, Debug)]
enum Fam {
    Unif(f64, f64),
    Unid(f64),
    Bern(f64),
    Bino(f64, f64),
    Geo(f64),
    Nbin(f64, f64),
    Poiss(f64),
    Hyge(f64, f64, f64),
    Norm(f64, f64),
    Logn(f64, f64),
    Exp(f64),
    Wbl(f64, f64),
    Gam(f64, f64),
    Beta(f64, f64),
    Chi2(f64),
    T(f64),
    F(f64, f64),
}

/// `binopdf` -> (`bino`, `pdf`); MATLAB's two irregular spellings too.
fn split_name(f: &str) -> (&str, &str) {
    match f {
        "poisstat" => ("poiss", "stat"),
        "unifit" => ("unif", "fit"),
        _ if f.ends_with("stat") => (&f[..f.len() - 4], "stat"),
        _ => (&f[..f.len() - 3], &f[f.len() - 3..]),
    }
}

fn is_int(v: f64) -> bool {
    v.is_finite() && v.fract() == 0.0
}

fn need_int(f: &str, name: &str, v: f64, min: f64) -> R<()> {
    if is_int(v) && v >= min {
        Ok(())
    } else {
        e(format!("{f}: {name} must be a whole number >= {min}, got {v}"))
    }
}

fn need_prob(f: &str, name: &str, v: f64, zero_ok: bool) -> R<()> {
    let ok = if zero_ok { (0.0..=1.0).contains(&v) } else { v > 0.0 && v <= 1.0 };
    if ok {
        Ok(())
    } else {
        e(format!("{f}: {name} must be in {}, got {v}", if zero_ok { "[0, 1]" } else { "(0, 1]" }))
    }
}

/// Reads and validates a family's parameters from `args[first..]`.
/// `defaults` allows MATLAB's optional trailing parameters (the samplers
/// pass `false`: there the size follows, so every parameter is explicit).
fn parse_fam(prefix: &str, f: &str, args: &[Value], first: usize, defaults: bool) -> R<Fam> {
    let get = |i: usize, name: &str, d: Option<f64>| -> R<f64> {
        match (arg_get(args, first + i), d) {
            (None, Some(d)) if defaults => Ok(d),
            _ => param(args, first + i, f, name),
        }
    };
    Ok(match prefix {
        "unif" => {
            let (a, b) = (get(0, "a", Some(0.0))?, get(1, "b", Some(1.0))?);
            if !(a < b && a.is_finite() && b.is_finite()) {
                return e(format!("{f}: needs finite a < b, got a = {a}, b = {b}"));
            }
            Fam::Unif(a, b)
        }
        "unid" => {
            let n = get(0, "N", None)?;
            need_int(f, "N", n, 1.0)?;
            Fam::Unid(n)
        }
        "bern" => {
            let p = get(0, "p", None)?;
            need_prob(f, "p", p, true)?;
            Fam::Bern(p)
        }
        "bino" => {
            let (n, p) = (get(0, "n", None)?, get(1, "p", None)?);
            need_int(f, "n (trials)", n, 0.0)?;
            need_prob(f, "p", p, true)?;
            Fam::Bino(n, p)
        }
        "geo" => {
            let p = get(0, "p", None)?;
            need_prob(f, "p", p, false)?;
            Fam::Geo(p)
        }
        "nbin" => {
            let (r, p) = (get(0, "r", None)?, get(1, "p", None)?);
            positive(f, "r (successes)", r)?;
            need_prob(f, "p", p, false)?;
            Fam::Nbin(r, p)
        }
        "poiss" => {
            let l = get(0, "lambda", None)?;
            if !(l >= 0.0 && l.is_finite()) {
                return e(format!("{f}: lambda must be finite and >= 0, got {l}"));
            }
            Fam::Poiss(l)
        }
        "hyge" => {
            let (m, k, n) = (get(0, "M", None)?, get(1, "K", None)?, get(2, "N", None)?);
            need_int(f, "M (population size)", m, 0.0)?;
            need_int(f, "K (successes in the population)", k, 0.0)?;
            need_int(f, "N (draws)", n, 0.0)?;
            if k > m || n > m {
                return e(format!("{f}: K and N cannot exceed M, got M = {m}, K = {k}, N = {n}"));
            }
            Fam::Hyge(m, k, n)
        }
        "norm" | "logn" => {
            let (mu, s) = (get(0, "mu", Some(0.0))?, get(1, "sigma", Some(1.0))?);
            positive(f, "sigma", s)?;
            if !mu.is_finite() {
                return e(format!("{f}: mu must be finite, got {mu}"));
            }
            if prefix == "norm" { Fam::Norm(mu, s) } else { Fam::Logn(mu, s) }
        }
        "exp" => {
            let mu = get(0, "mu (mean)", Some(1.0))?;
            positive(f, "mu (mean)", mu)?;
            Fam::Exp(mu)
        }
        "wbl" => {
            let (a, b) = (get(0, "a (scale)", Some(1.0))?, get(1, "b (shape)", Some(1.0))?);
            positive(f, "a (scale)", a)?;
            positive(f, "b (shape)", b)?;
            Fam::Wbl(a, b)
        }
        "gam" => {
            let (a, b) = (get(0, "a (shape)", None)?, get(1, "b (scale)", Some(1.0))?);
            positive(f, "a (shape)", a)?;
            positive(f, "b (scale)", b)?;
            Fam::Gam(a, b)
        }
        "beta" => {
            let (a, b) = (get(0, "a", None)?, get(1, "b", None)?);
            positive(f, "a", a)?;
            positive(f, "b", b)?;
            Fam::Beta(a, b)
        }
        "chi2" => {
            let k = get(0, "k (degrees of freedom)", None)?;
            positive(f, "k (degrees of freedom)", k)?;
            Fam::Chi2(k)
        }
        "t" => {
            let nu = get(0, "nu (degrees of freedom)", None)?;
            positive(f, "nu (degrees of freedom)", nu)?;
            Fam::T(nu)
        }
        "f" => {
            let (d1, d2) = (get(0, "d1", None)?, get(1, "d2", None)?);
            positive(f, "d1 (numerator degrees of freedom)", d1)?;
            positive(f, "d2 (denominator degrees of freedom)", d2)?;
            Fam::F(d1, d2)
        }
        other => return e(format!("distributions: unknown family `{other}`")),
    })
}

// Loader's saddle-point pmfs ("Fast and Accurate Computation of Binomial
// Probabilities", 2000 -- what R and SciPy use): the difference of large
// `lgamma` values cancels ~12 digits away at n ~ 1e3; `stirlerr` and `bd0`
// carry only the small remainders, so the pmfs stay at full precision.

/// `ln(n!) - ln(sqrt(2 pi n) (n/e)^n)`.
fn stirlerr(n: f64) -> f64 {
    const HALVES: [f64; 31] = [
        0.0,
        0.153_426_409_720_027_345_291_384_8,
        0.081_061_466_795_327_258_219_670_2,
        0.054_814_121_051_917_653_896_139_0,
        0.041_340_695_955_409_294_093_822_1,
        0.033_162_873_519_936_287_485_110_48,
        0.027_677_925_684_998_339_148_789_29,
        0.023_746_163_656_297_495_971_329_20,
        0.020_790_672_103_765_093_111_522_77,
        0.018_488_450_532_673_185_230_779_34,
        0.016_644_691_189_821_192_163_194_87,
        0.015_134_973_221_917_378_873_512_55,
        0.013_876_128_823_070_747_998_745_73,
        0.012_810_465_242_920_226_924_249_86,
        0.011_896_709_945_891_770_095_055_72,
        0.011_104_559_758_206_917_326_629_91,
        0.010_411_265_261_972_096_497_478_567,
        0.009_799_416_126_158_803_298_389_475,
        0.009_255_462_182_712_732_917_728_637,
        0.008_768_700_134_139_385_462_952_823,
        0.008_330_563_433_362_871_256_469_318,
        0.007_934_114_564_314_020_547_248_100,
        0.007_573_675_487_951_840_794_972_024,
        0.007_244_554_301_320_383_179_543_912,
        0.006_942_840_107_209_529_865_664_152,
        0.006_665_247_032_707_682_442_354_394,
        0.006_408_994_188_004_207_068_439_631,
        0.006_171_712_263_039_457_647_532_867,
        0.005_951_370_112_758_847_735_624_416,
        0.005_746_216_513_010_115_682_023_589,
        0.005_554_733_551_962_801_371_038_690,
    ];
    if n <= 15.0 {
        let nn = n + n;
        if nn == nn.floor() {
            return HALVES[nn as usize];
        }
        return ln_gamma(n + 1.0) - (n + 0.5) * n.ln() + n - 0.918_938_533_204_672_741_780_329_7;
    }
    let nn = n * n;
    let (s0, s1, s2, s3, s4) = (1.0 / 12.0, 1.0 / 360.0, 1.0 / 1260.0, 1.0 / 1680.0, 1.0 / 1188.0);
    if n > 500.0 {
        return (s0 - s1 / nn) / n;
    }
    if n > 80.0 {
        return (s0 - (s1 - s2 / nn) / nn) / n;
    }
    if n > 35.0 {
        return (s0 - (s1 - (s2 - s3 / nn) / nn) / nn) / n;
    }
    (s0 - (s1 - (s2 - (s3 - s4 / nn) / nn) / nn) / nn) / n
}

/// `x ln(x / np) + np - x`, by a series when `x` is near `np` (where the
/// direct form cancels).
fn bd0(x: f64, np: f64) -> f64 {
    if (x - np).abs() < 0.1 * (x + np) {
        let mut v = (x - np) / (x + np);
        let mut s = (x - np) * v;
        let mut ej = 2.0 * x * v;
        v *= v;
        for j in 1..1000 {
            ej *= v;
            let s1 = s + ej / (2 * j + 1) as f64;
            if s1 == s {
                return s1;
            }
            s = s1;
        }
        return s;
    }
    x * (x / np).ln() + np - x
}

/// Binomial pmf at `x` of `n` trials (`n` may be non-integer: the negative
/// binomial and hypergeometric pmfs are built from this).
fn dbinom_raw(x: f64, n: f64, p: f64, q: f64) -> f64 {
    if p == 0.0 {
        return (x == 0.0) as u8 as f64;
    }
    if q == 0.0 {
        return (x == n) as u8 as f64;
    }
    if x == 0.0 {
        if n == 0.0 {
            return 1.0;
        }
        let lc = if p < 0.1 { -bd0(n, n * q) - n * p } else { n * q.ln() };
        return lc.exp();
    }
    if x == n {
        let lc = if q < 0.1 { -bd0(n, n * p) - n * q } else { n * p.ln() };
        return lc.exp();
    }
    if x < 0.0 || x > n {
        return 0.0;
    }
    let lc = stirlerr(n) - stirlerr(x) - stirlerr(n - x) - bd0(x, n * p) - bd0(n - x, n * q);
    let lf = std::f64::consts::TAU.ln() + x.ln() + (-x / n).ln_1p();
    (lc - 0.5 * lf).exp()
}

/// `P(X <= k) = Q(k+1, lambda)`. The incomplete gamma's prefactor
/// `e^-l l^(k+1) / k!` is exactly `l * pmf(k)`, so it is taken from the
/// accurate pmf rather than `exp` of a difference of `lgamma`s; the series
/// (below the mean) or Lentz continued fraction (above) supplies the rest.
fn poiss_cdf(k: f64, l: f64) -> f64 {
    let (a, x) = (k + 1.0, l);
    let front = l * dpois_raw(k, l);
    if x < a + 1.0 {
        let (mut ap, mut del) = (a, 1.0 / a);
        let mut sum = del;
        for _ in 0..100_000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-17 {
                break;
            }
        }
        (1.0 - front * sum).max(0.0)
    } else {
        const TINY: f64 = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / TINY;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..100_000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < TINY {
                d = TINY;
            }
            c = b + an / c;
            if c.abs() < TINY {
                c = TINY;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-16 {
                break;
            }
        }
        front * h
    }
}

fn dpois_raw(x: f64, l: f64) -> f64 {
    if l == 0.0 {
        return (x == 0.0) as u8 as f64;
    }
    if x == 0.0 {
        return (-l).exp();
    }
    (-stirlerr(x) - bd0(x, l)).exp() / (std::f64::consts::TAU * x).sqrt()
}

/// `(lo, hi)` support of a discrete family; `None` for a continuous one.
fn support(fam: Fam) -> Option<(f64, f64)> {
    Some(match fam {
        Fam::Unid(n) => (1.0, n),
        Fam::Bern(_) => (0.0, 1.0),
        Fam::Bino(n, _) => (0.0, n),
        Fam::Geo(_) | Fam::Nbin(..) => (0.0, f64::INFINITY),
        Fam::Poiss(l) => (0.0, if l == 0.0 { 0.0 } else { f64::INFINITY }),
        Fam::Hyge(m, k, n) => ((n + k - m).max(0.0), k.min(n)),
        _ => return None,
    })
}

/// Probability mass at an integer `k` inside the support.
fn pmf(fam: Fam, k: f64) -> f64 {
    match fam {
        Fam::Unid(n) => 1.0 / n,
        Fam::Bern(p) => if k == 1.0 { p } else { 1.0 - p },
        Fam::Bino(n, p) => dbinom_raw(k, n, p, 1.0 - p),
        Fam::Geo(p) => {
            if p == 1.0 {
                return (k == 0.0) as u8 as f64;
            }
            (p.ln() + k * (-p).ln_1p()).exp()
        }
        Fam::Nbin(r, p) => {
            if p == 1.0 {
                return (k == 0.0) as u8 as f64;
            }
            // R's dnbinom: r/(r+k) times the binomial pmf of r in r+k.
            r / (r + k) * dbinom_raw(r, k + r, p, 1.0 - p)
        }
        Fam::Poiss(l) => dpois_raw(k, l),
        Fam::Hyge(m, kk, n) => {
            // R's dhyper: three binomial pmfs at p = N/M.
            let (p, q) = (n / m, (m - n) / m);
            dbinom_raw(k, kk, p, q) * dbinom_raw(n - k, m - kk, p, q) / dbinom_raw(n, m, p, q)
        }
        _ => f64::NAN,
    }
}

fn fam_pdf(fam: Fam, x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if let Some((lo, hi)) = support(fam) {
        return if is_int(x) && x >= lo && x <= hi { pmf(fam, x) } else { 0.0 };
    }
    match fam {
        Fam::Unif(a, b) => if x >= a && x <= b { 1.0 / (b - a) } else { 0.0 },
        Fam::Norm(mu, s) => norm_pdf((x - mu) / s) / s,
        Fam::Logn(mu, s) => {
            if x <= 0.0 {
                0.0
            } else {
                norm_pdf((x.ln() - mu) / s) / (s * x)
            }
        }
        Fam::Exp(mu) => if x < 0.0 { 0.0 } else { (-x / mu).exp() / mu },
        Fam::Wbl(a, b) => {
            if x < 0.0 {
                0.0
            } else if x == 0.0 {
                if b < 1.0 { f64::INFINITY } else if b == 1.0 { 1.0 / a } else { 0.0 }
            } else {
                let z = x / a;
                (b / a) * ((b - 1.0) * z.ln() - z.powf(b)).exp()
            }
        }
        Fam::Gam(a, b) => gamma_pdf(x, a, b),
        Fam::Beta(a, b) => beta_pdf(x, a, b),
        Fam::Chi2(k) => gamma_pdf(x, k / 2.0, 2.0),
        Fam::T(nu) => t_pdf(x, nu),
        Fam::F(d1, d2) => f_pdf(x, d1, d2),
        _ => f64::NAN,
    }
}

fn fam_cdf(fam: Fam, x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if let Some((lo, hi)) = support(fam) {
        let k = x.floor();
        if k < lo {
            return 0.0;
        }
        if k >= hi {
            return 1.0;
        }
        return match fam {
            Fam::Unid(n) => k / n,
            Fam::Bern(p) => 1.0 - p,
            // I_q(n-k, k+1), with the continued fraction's prefactor
            // written as the (accurate) pmf: p * pmf(k), or q * pmf(k+1)
            // for the upper tail on the other side.
            Fam::Bino(n, p) => {
                let q = 1.0 - p;
                let (a, b) = (n - k, k + 1.0);
                if q < (a + 1.0) / (a + b + 2.0) {
                    p * pmf(fam, k) * beta_cf(q, a, b)
                } else {
                    1.0 - q * pmf(fam, k + 1.0) * beta_cf(p, b, a)
                }
            }
            Fam::Geo(p) => -((k + 1.0) * (-p).ln_1p()).exp_m1(),
            // I_p(r, k+1), prefactor from the pmf as for the binomial.
            Fam::Nbin(r, p) => {
                if p >= 1.0 {
                    return 1.0;
                }
                let q = 1.0 - p;
                if p < (r + 1.0) / (r + k + 3.0) {
                    (r + k) * q * pmf(fam, k) * beta_cf(p, r, k + 1.0) / r
                } else {
                    1.0 - pmf(fam, k + 1.0) * beta_cf(q, k + 1.0, r)
                }
            }
            Fam::Poiss(l) => poiss_cdf(k, l),
            Fam::Hyge(..) => {
                let mut c = 0.0;
                let mut j = lo;
                while j <= k {
                    c += pmf(fam, j);
                    j += 1.0;
                }
                c.min(1.0)
            }
            _ => f64::NAN,
        };
    }
    match fam {
        Fam::Unif(a, b) => ((x - a) / (b - a)).clamp(0.0, 1.0),
        Fam::Norm(mu, s) => norm_cdf((x - mu) / s),
        Fam::Logn(mu, s) => if x <= 0.0 { 0.0 } else { norm_cdf((x.ln() - mu) / s) },
        Fam::Exp(mu) => if x <= 0.0 { 0.0 } else { -(-x / mu).exp_m1() },
        Fam::Wbl(a, b) => if x <= 0.0 { 0.0 } else { -(-(x / a).powf(b)).exp_m1() },
        Fam::Gam(a, b) => gamma_cdf(x, a, b),
        Fam::Beta(a, b) => beta_cdf(x, a, b),
        Fam::Chi2(k) => gamma_cdf(x, k / 2.0, 2.0),
        Fam::T(nu) => t_cdf(x, nu),
        Fam::F(d1, d2) => f_cdf(x, d1, d2),
        _ => f64::NAN,
    }
}

/// Inverse CDF. For a discrete family: the smallest support point whose
/// CDF reaches `p` (MATLAB's definition, so `binoinv(binocdf(k, ...), ...)`
/// is `k`), by doubling to a bracket and bisecting over the integers.
fn fam_inv(fam: Fam, p: f64) -> f64 {
    if p.is_nan() {
        return f64::NAN;
    }
    if let Some((lo, hi)) = support(fam) {
        if p >= 1.0 {
            return hi;
        }
        if fam_cdf(fam, lo) >= p {
            return lo;
        }
        let mut l = lo;
        let mut h = if hi.is_finite() { hi } else { lo + 1.0 };
        while fam_cdf(fam, h) < p {
            l = h;
            h = lo + 2.0 * (h - lo + 1.0);
            if h > 9.007_199_254_740_992e15 {
                return f64::INFINITY;
            }
        }
        while h - l > 1.0 {
            let m = ((l + h) / 2.0).floor();
            if fam_cdf(fam, m) >= p {
                h = m;
            } else {
                l = m;
            }
        }
        return h;
    }
    match fam {
        Fam::Unif(a, b) => a + p * (b - a),
        Fam::Norm(mu, s) => mu + s * norm_inv(p),
        Fam::Logn(mu, s) => (mu + s * norm_inv(p)).exp(),
        Fam::Exp(mu) => if p >= 1.0 { f64::INFINITY } else { -mu * (-p).ln_1p() },
        Fam::Wbl(a, b) => if p >= 1.0 { f64::INFINITY } else { a * (-(-p).ln_1p()).powf(1.0 / b) },
        Fam::Gam(a, b) => gamma_inv(p, a, b),
        Fam::Beta(a, b) => beta_inv(p, a, b),
        Fam::Chi2(k) => gamma_inv(p, k / 2.0, 2.0),
        Fam::T(nu) => t_inv(p, nu),
        Fam::F(d1, d2) => f_inv(p, d1, d2),
        _ => f64::NAN,
    }
}

/// `(mean, variance)`; NaN where a moment does not exist (t with nu <= 1),
/// inf where it diverges (t variance for 1 < nu <= 2).
fn fam_stat(fam: Fam) -> (f64, f64) {
    let g = |x: f64| ln_gamma(x).exp();
    match fam {
        Fam::Unif(a, b) => ((a + b) / 2.0, (b - a).powi(2) / 12.0),
        Fam::Unid(n) => ((n + 1.0) / 2.0, (n * n - 1.0) / 12.0),
        Fam::Bern(p) => (p, p * (1.0 - p)),
        Fam::Bino(n, p) => (n * p, n * p * (1.0 - p)),
        Fam::Geo(p) => ((1.0 - p) / p, (1.0 - p) / (p * p)),
        Fam::Nbin(r, p) => (r * (1.0 - p) / p, r * (1.0 - p) / (p * p)),
        Fam::Poiss(l) => (l, l),
        Fam::Hyge(m, k, n) => {
            let mean = n * k / m.max(1.0);
            let var = if m > 1.0 { n * (k / m) * ((m - k) / m) * ((m - n) / (m - 1.0)) } else { 0.0 };
            (if m == 0.0 { 0.0 } else { mean }, var)
        }
        Fam::Norm(mu, s) => (mu, s * s),
        Fam::Logn(mu, s) => ((mu + s * s / 2.0).exp(), (s * s).exp_m1() * (2.0 * mu + s * s).exp()),
        Fam::Exp(mu) => (mu, mu * mu),
        Fam::Wbl(a, b) => {
            let g1 = g(1.0 + 1.0 / b);
            (a * g1, a * a * (g(1.0 + 2.0 / b) - g1 * g1))
        }
        Fam::Gam(a, b) => (a * b, a * b * b),
        Fam::Beta(a, b) => (a / (a + b), a * b / ((a + b).powi(2) * (a + b + 1.0))),
        Fam::Chi2(k) => (k, 2.0 * k),
        Fam::T(nu) => (
            if nu > 1.0 { 0.0 } else { f64::NAN },
            if nu > 2.0 { nu / (nu - 2.0) } else if nu > 1.0 { f64::INFINITY } else { f64::NAN },
        ),
        Fam::F(d1, d2) => (
            if d2 > 2.0 { d2 / (d2 - 2.0) } else { f64::NAN },
            if d2 > 4.0 {
                2.0 * d2 * d2 * (d1 + d2 - 2.0) / (d1 * (d2 - 2.0).powi(2) * (d2 - 4.0))
            } else if d2 > 2.0 {
                f64::INFINITY
            } else {
                f64::NAN
            },
        ),
    }
}

fn model(kind: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Model(std::sync::Arc::new(crate::ModelHandle::new(
        kind,
        fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    )))
}

/// pdf / cdf / inv / stat / fit for every family in [`FAMILY_NAMES`].
pub fn family_call(f: &str, args: &[Value]) -> R<Value> {
    let (prefix, op) = split_name(f);
    match op {
        "pdf" | "cdf" | "inv" => {
            let x = arg0(args)?.clone();
            let fam = parse_fam(prefix, f, args, 1, true)?;
            match op {
                "pdf" => map1(x, move |x| fam_pdf(fam, x)),
                "cdf" => map1(x, move |x| fam_cdf(fam, x)),
                _ => {
                    check_probs(f, &x)?;
                    map1(x, move |p| fam_inv(fam, p))
                }
            }
        }
        "stat" => {
            let fam = parse_fam(prefix, f, args, 0, true)?;
            let (m, v) = fam_stat(fam);
            Ok(model(f, vec![("mean", Value::Num(m)), ("var", Value::Num(v))]))
        }
        "fit" => fit(prefix, f, args),
        _ => e(format!("distributions: unknown function `{f}`")),
    }
}

// ------------------------------------------------------------ sampling

/// Draws from `f`'s family. `u` yields uniforms in `[0, 1)`; everything
/// else is built from it, so `seed=`/`seed(n)` govern these exactly as they
/// do `rand`. Returns the draws and the requested `(rows, cols)`.
pub fn rnd(f: &str, args: &[Value], u: &mut dyn FnMut() -> f64) -> R<(Vec<f64>, usize, usize)> {
    let prefix = &f[..f.len() - 3];
    let fam = parse_fam(prefix, f, args, 0, false)?;
    let np = match fam {
        Fam::Unid(_) | Fam::Bern(_) | Fam::Geo(_) | Fam::Poiss(_) | Fam::Exp(_) | Fam::Chi2(_) | Fam::T(_) => 1,
        Fam::Hyge(..) => 3,
        _ => 2,
    };
    let dim = |i: usize, name: &str| -> R<usize> {
        match arg_get(args, np + i) {
            None => Ok(1),
            Some(v) => {
                let d = v.as_num().map_err(|m| EvalError { msg: format!("{f}: `{name}` {m}") })?;
                if is_int(d) && d >= 0.0 {
                    Ok(d as usize)
                } else {
                    e(format!("{f}: {name} must be a whole number >= 0, got {d}"))
                }
            }
        }
    };
    let (rows, cols) = (dim(0, "rows")?, dim(1, "cols")?);
    let n = rows.max(1) * cols.max(1);
    let chop = Chop::new(fam, n);
    let out = (0..n).map(|_| draw(fam, &chop, u)).collect();
    Ok((out, rows, cols))
}

fn std_normal(u: &mut dyn FnMut() -> f64) -> f64 {
    // Box-Muller on (0, 1]: `1 - u` is never 0, so no clamp truncates the
    // tail.
    let (u1, u2) = (1.0 - u(), u());
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// Marsaglia-Tsang; shape < 1 through `G(a+1) * U^(1/a)`.
fn std_gamma(a: f64, u: &mut dyn FnMut() -> f64) -> f64 {
    if a < 1.0 {
        return std_gamma(a + 1.0, u) * (1.0 - u()).powf(1.0 / a);
    }
    let d = a - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = std_normal(u);
        let v = 1.0 + c * x;
        if v <= 0.0 {
            continue;
        }
        let v = v * v * v;
        let w = 1.0 - u();
        if w < 1.0 - 0.0331 * x.powi(4) || w.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

/// Inversion for the bounded-ratio discrete families, walking from the
/// mode with the pmf recurrence: exact up to rounding, and O(standard
/// deviation) steps per draw instead of O(n) or O(lambda).
struct Chop {
    m: f64,
    fm: f64,
    cm: f64,
    lo: f64,
    hi: f64,
    /// For many draws from a wide distribution: the CDF over mean +- 12 sd,
    /// built once, so a draw is a binary search instead of a walk of about
    /// one standard deviation. `(first support point, cdf below it, cdf)`.
    table: Option<(f64, f64, Vec<f64>)>,
}

impl Chop {
    fn new(fam: Fam, draws: usize) -> Chop {
        let (lo, hi) = support(fam).unwrap_or((0.0, 0.0));
        let m = match fam {
            Fam::Bino(n, p) => ((n + 1.0) * p).floor().min(n),
            Fam::Poiss(l) => l.floor(),
            Fam::Nbin(r, p) => if r > 1.0 { ((r - 1.0) * (1.0 - p) / p).floor() } else { 0.0 },
            Fam::Hyge(m, k, n) => ((n + 1.0) * (k + 1.0) / (m + 2.0)).floor(),
            _ => lo,
        }
        .clamp(lo, hi.max(lo));
        let (fm, cm) = if support(fam).is_some() { (pmf(fam, m), fam_cdf(fam, m)) } else { (0.0, 0.0) };
        let mut chop = Chop { m, fm, cm, lo, hi, table: None };
        let walks = matches!(fam, Fam::Bino(..) | Fam::Poiss(_) | Fam::Nbin(..) | Fam::Hyge(..));
        let (mean, var) = fam_stat(fam);
        let sd = var.sqrt();
        if walks && draws >= 64 && sd > 16.0 && sd < 150_000.0 && fm > 0.0 {
            let t_lo = (mean - 12.0 * sd).floor().max(lo);
            let t_hi = (mean + 12.0 * sd).ceil().min(hi);
            // pmf outward from the mode by the recurrence (each step is
            // exact up to one rounding), then one prefix sum.
            let len = (t_hi - t_lo) as usize + 1;
            let mut f = vec![0.0; len];
            let mi = (m - t_lo) as usize;
            f[mi] = fm;
            for i in mi + 1..len {
                f[i] = f[i - 1] * ratio(fam, t_lo + (i - 1) as f64);
            }
            for i in (0..mi).rev() {
                f[i] = f[i + 1] / ratio(fam, t_lo + i as f64);
            }
            let below = if t_lo > lo { fam_cdf(fam, t_lo - 1.0) } else { 0.0 };
            let mut acc = below;
            for v in f.iter_mut() {
                acc += *v;
                *v = acc;
            }
            chop.table = Some((t_lo, below, f));
        }
        chop
    }
}

/// `pmf(k + 1) / pmf(k)`.
fn ratio(fam: Fam, k: f64) -> f64 {
    match fam {
        Fam::Bino(n, p) => (n - k) / (k + 1.0) * p / (1.0 - p),
        Fam::Poiss(l) => l / (k + 1.0),
        Fam::Nbin(r, p) => (k + r) / (k + 1.0) * (1.0 - p),
        Fam::Hyge(m, kk, n) => (kk - k) * (n - k) / ((k + 1.0) * (m - kk - n + k + 1.0)),
        _ => 0.0,
    }
}

fn draw(fam: Fam, chop: &Chop, u: &mut dyn FnMut() -> f64) -> f64 {
    match fam {
        Fam::Unif(a, b) => a + (b - a) * u(),
        Fam::Unid(n) => ((u() * n).floor() + 1.0).min(n),
        Fam::Bern(p) => (u() < p) as u8 as f64,
        Fam::Geo(p) => {
            if p == 1.0 {
                0.0
            } else {
                ((1.0 - u()).ln() / (-p).ln_1p()).floor()
            }
        }
        Fam::Bino(n, p) if p == 0.0 || p == 1.0 => if p == 0.0 { 0.0 } else { n },
        Fam::Poiss(l) if l == 0.0 => 0.0,
        Fam::Nbin(_, p) if p == 1.0 => 0.0,
        Fam::Bino(..) | Fam::Poiss(_) | Fam::Nbin(..) | Fam::Hyge(..) => {
            let target = u();
            if let Some((t_lo, below, cum)) = &chop.table {
                if target > *below && target <= cum[cum.len() - 1] {
                    let i = cum.partition_point(|&c| c < target);
                    return t_lo + i as f64;
                }
            }
            let (mut k, mut c, mut f) = (chop.m, chop.cm, chop.fm);
            if target <= c {
                // Walk down while the CDF one step lower still covers it.
                while k > chop.lo && c - f >= target {
                    c -= f;
                    k -= 1.0;
                    f /= ratio(fam, k);
                }
            } else {
                while k < chop.hi && c < target {
                    f *= ratio(fam, k);
                    k += 1.0;
                    c += f;
                    if f == 0.0 {
                        break;
                    }
                }
            }
            k
        }
        Fam::Norm(mu, s) => mu + s * std_normal(u),
        Fam::Logn(mu, s) => (mu + s * std_normal(u)).exp(),
        Fam::Exp(mu) => -mu * (1.0 - u()).ln(),
        Fam::Wbl(a, b) => a * (-(1.0 - u()).ln()).powf(1.0 / b),
        Fam::Gam(a, b) => b * std_gamma(a, u),
        Fam::Beta(a, b) => {
            let (x, y) = (std_gamma(a, u), std_gamma(b, u));
            if x + y > 0.0 { x / (x + y) } else { (u() < a / (a + b)) as u8 as f64 }
        }
        Fam::Chi2(k) => 2.0 * std_gamma(k / 2.0, u),
        Fam::T(nu) => std_normal(u) / (2.0 * std_gamma(nu / 2.0, u) / nu).sqrt(),
        Fam::F(d1, d2) => (2.0 * std_gamma(d1 / 2.0, u) / d1) / (2.0 * std_gamma(d2 / 2.0, u) / d2),
    }
}

// ------------------------------------------------------------ fitting

fn trigamma(mut x: f64) -> f64 {
    let mut acc = 0.0;
    while x < 6.0 {
        acc += 1.0 / (x * x);
        x += 1.0;
    }
    let x2 = 1.0 / (x * x);
    acc + 1.0 / x + x2 / 2.0 + (1.0 / x) * x2 * (1.0 / 6.0 - x2 * (1.0 / 30.0 - x2 * (1.0 / 42.0 - x2 / 30.0)))
}

fn digamma(x: f64) -> f64 {
    numeric::special::digamma(x)
}

/// Bisection on `log(x)` for a sign change of `g` inside `[lo, hi]`,
/// widening the bracket by decades until it holds one.
fn log_bisect(g: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> Option<f64> {
    let (mut glo, mut ghi) = (g(lo), g(hi));
    let mut widen = 0;
    while glo.signum() == ghi.signum() {
        if widen > 60 {
            return None;
        }
        lo /= 10.0;
        hi *= 10.0;
        glo = g(lo);
        ghi = g(hi);
        widen += 1;
    }
    for _ in 0..300 {
        let mid = (lo * hi).sqrt();
        let gm = g(mid);
        if gm == 0.0 {
            return Some(mid);
        }
        if gm.signum() == glo.signum() {
            lo = mid;
            glo = gm;
        } else {
            hi = mid;
        }
        if hi / lo - 1.0 < 1e-15 {
            break;
        }
    }
    Some((lo * hi).sqrt())
}

fn fit(prefix: &str, f: &str, args: &[Value]) -> R<Value> {
    let x = crate::to_vec(arg0(args)?).map_err(|m| EvalError { msg: format!("{f}: {m}") })?;
    if x.is_empty() {
        return e(format!("{f}: needs at least one data value"));
    }
    if let Some(i) = x.iter().position(|v| !v.is_finite()) {
        return e(format!("{f}: data value {i} is {} -- remove NaN/inf values first", x[i]));
    }
    let n = x.len() as f64;
    let mean = crate::kmean(&x);
    let all_equal = x.iter().all(|&v| v == x[0]);
    let check = |ok: fn(f64) -> bool, what: &str| -> R<()> {
        match x.iter().position(|&v| !ok(v)) {
            Some(i) => e(format!("{f}: every value must be {what}; value {i} is {}", x[i])),
            None => Ok(()),
        }
    };
    let degenerate = || e::<Value>(format!("{f}: all {} values are equal, so the fit has no finite solution", x.len()));
    let (fam, names): (Fam, Vec<(&str, f64)>) = match prefix {
        "norm" | "logn" => {
            if prefix == "logn" {
                check(|v| v > 0.0, "positive")?;
            }
            if x.len() < 2 {
                return e(format!("{f}: needs at least two values for sigma"));
            }
            let y: Vec<f64> = if prefix == "logn" { x.iter().map(|v| v.ln()).collect() } else { x.clone() };
            let (mu, s) = (crate::kmean(&y), crate::variance(&y).sqrt());
            if !(s > 0.0) {
                return degenerate();
            }
            (if prefix == "logn" { Fam::Logn(mu, s) } else { Fam::Norm(mu, s) }, vec![("mu", mu), ("sigma", s)])
        }
        "exp" => {
            check(|v| v >= 0.0, "non-negative")?;
            if !(mean > 0.0) {
                return degenerate();
            }
            (Fam::Exp(mean), vec![("mu", mean)])
        }
        "poiss" => {
            check(|v| is_int(v) && v >= 0.0, "a non-negative whole number")?;
            (Fam::Poiss(mean), vec![("lambda", mean)])
        }
        "bern" => {
            check(|v| v == 0.0 || v == 1.0, "0 or 1")?;
            (Fam::Bern(mean), vec![("p", mean)])
        }
        "geo" => {
            check(|v| is_int(v) && v >= 0.0, "a non-negative whole number (failures before the first success)")?;
            let p = 1.0 / (1.0 + mean);
            (Fam::Geo(p), vec![("p", p)])
        }
        "bino" => {
            let trials = param(args, 1, f, "n (trials)")?;
            need_int(f, "n (trials)", trials, 1.0)?;
            if let Some(i) = x.iter().position(|&v| !(is_int(v) && v >= 0.0 && v <= trials)) {
                return e(format!("{f}: every value must be a whole number of successes in 0..{trials}; value {i} is {}", x[i]));
            }
            let p = mean / trials;
            (Fam::Bino(trials, p), vec![("p", p)])
        }
        "unif" => {
            let (a, b) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            if a == b {
                return degenerate();
            }
            (Fam::Unif(a, b), vec![("a", a), ("b", b)])
        }
        "gam" => {
            check(|v| v > 0.0, "positive")?;
            if all_equal {
                return degenerate();
            }
            // Solve ln a - digamma(a) = ln(mean) - mean(ln x) by Newton,
            // from the Choi-Wette closed-form start.
            let s = mean.ln() - crate::kmean(&x.iter().map(|v| v.ln()).collect::<Vec<_>>());
            let mut a = (3.0 - s + ((s - 3.0).powi(2) + 24.0 * s).sqrt()) / (12.0 * s);
            for _ in 0..100 {
                let g = a.ln() - digamma(a) - s;
                let step = g / (1.0 / a - trigamma(a));
                let next = if a - step > 0.0 { a - step } else { a / 2.0 };
                if (next - a).abs() <= 1e-14 * a {
                    a = next;
                    break;
                }
                a = next;
            }
            (Fam::Gam(a, mean / a), vec![("a", a), ("b", mean / a)])
        }
        "wbl" => {
            check(|v| v > 0.0, "positive")?;
            if all_equal {
                return degenerate();
            }
            // The shape equation is scale-free, so solve it on x / max(x)
            // (no overflow of x^b for large data or shapes).
            let top = x.iter().copied().fold(0.0f64, f64::max);
            let y: Vec<f64> = x.iter().map(|v| v / top).collect();
            let ly: Vec<f64> = y.iter().map(|v| v.ln()).collect();
            let mean_ly = crate::kmean(&ly);
            let h = |b: f64| {
                let (mut s0, mut s1) = (0.0, 0.0);
                for (yi, li) in y.iter().zip(&ly) {
                    let w = yi.powf(b);
                    s0 += w;
                    s1 += w * li;
                }
                s1 / s0 - 1.0 / b - mean_ly
            };
            let b = log_bisect(h, 0.1, 10.0).ok_or_else(|| EvalError { msg: format!("{f}: the shape equation has no root") })?;
            let a = top * (y.iter().map(|v| v.powf(b)).sum::<f64>() / n).powf(1.0 / b);
            (Fam::Wbl(a, b), vec![("a", a), ("b", b)])
        }
        "beta" => {
            check(|v| v > 0.0 && v < 1.0, "strictly between 0 and 1")?;
            if all_equal {
                return degenerate();
            }
            let s1 = crate::kmean(&x.iter().map(|v| v.ln()).collect::<Vec<_>>());
            let s2 = crate::kmean(&x.iter().map(|v| (-v).ln_1p()).collect::<Vec<_>>());
            let v = crate::variance(&x);
            let common = mean * (1.0 - mean) / v - 1.0;
            let (mut a, mut b) = if common > 0.0 { (mean * common, (1.0 - mean) * common) } else { (1.0, 1.0) };
            for _ in 0..200 {
                let dab = digamma(a + b);
                let (f1, f2) = (digamma(a) - dab - s1, digamma(b) - dab - s2);
                let tab = trigamma(a + b);
                let (j11, j12, j22) = (trigamma(a) - tab, -tab, trigamma(b) - tab);
                let det = j11 * j22 - j12 * j12;
                let (mut da, mut db) = ((j22 * f1 - j12 * f2) / det, (j11 * f2 - j12 * f1) / det);
                while a - da <= 0.0 || b - db <= 0.0 {
                    da /= 2.0;
                    db /= 2.0;
                }
                a -= da;
                b -= db;
                if da.abs() <= 1e-14 * a && db.abs() <= 1e-14 * b {
                    break;
                }
            }
            (Fam::Beta(a, b), vec![("a", a), ("b", b)])
        }
        "nbin" => {
            check(|v| is_int(v) && v >= 0.0, "a non-negative whole number (failures)")?;
            let var_mle = crate::variance(&x) * (n - 1.0) / n;
            if !(x.len() >= 2 && var_mle > mean && mean > 0.0) {
                return e(format!(
                    "{f}: the data are not overdispersed (variance {var_mle} <= mean {mean}), so the \
                     negative binomial has no finite fit -- a Poisson (`poissfit`) is the model for them"
                ));
            }
            let g = |r: f64| {
                let s: f64 = x.iter().map(|&xi| digamma(xi + r) - digamma(r)).sum();
                s + n * (r / (r + mean)).ln()
            };
            let r0 = mean * mean / (var_mle - mean);
            let r = log_bisect(g, r0 / 10.0, r0 * 10.0)
                .ok_or_else(|| EvalError { msg: format!("{f}: the likelihood equation has no root") })?;
            let p = r / (r + mean);
            (Fam::Nbin(r, p), vec![("r", r), ("p", p)])
        }
        other => return e(format!("distributions: no fit for `{other}`")),
    };
    let loglik = x.iter().map(|&v| fam_pdf(fam, v).ln()).sum::<f64>();
    let mut fields: Vec<(&str, Value)> = names.iter().map(|(k, v)| (*k, Value::Num(*v))).collect();
    fields.push(("params", Value::Vec(std::sync::Arc::new(names.iter().map(|(_, v)| *v).collect()))));
    fields.push(("n", Value::Num(n)));
    fields.push(("loglik", Value::Num(loglik)));
    Ok(model(f, fields))
}
