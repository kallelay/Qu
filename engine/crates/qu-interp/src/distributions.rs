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
