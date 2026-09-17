//! Special functions shared by distribution PDFs/CDFs (chi-square today;
//! any future Gamma-family distribution — Gamma, Beta, Student's t via a
//! ratio of Gammas — builds on the same two primitives).
//!
//! Both algorithms below are standard, textbook numerical methods (Lanczos
//! approximation for `ln Gamma`; series/continued-fraction evaluation of
//! the regularized incomplete gamma function, split at `x = a+1` where the
//! series stops converging quickly) — the same split used across open
//! numerical libraries, reimplemented here from the mathematical
//! description rather than copied from any one of them, to keep this
//! crate dependency-free.

/// `ln(Gamma(x))` via the Lanczos approximation, accurate to ~15
/// significant digits over the positive reals. Chi-square only ever calls
/// this with `x = k/2` for `k >= 1` (so `x >= 0.5`), but the reflection
/// formula for `x < 0.5` is included for robustness against future callers.
pub fn ln_gamma(x: f64) -> f64 {
    const COEFFICIENTS: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1259.139_216_722_402_8,
        771.323_428_777_653_13,
        -176.615_029_162_140_59,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_312e-7,
    ];
    if x < 0.5 {
        // Reflection formula: Gamma(x)*Gamma(1-x) = pi / sin(pi*x).
        (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let g = 7.0;
        let mut a = COEFFICIENTS[0];
        let t = x + g + 0.5;
        for (i, &c) in COEFFICIENTS.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

const MAX_ITER: usize = 200;
const EPS: f64 = 1e-14;
const FPMIN: f64 = 1e-300;

/// Regularized lower incomplete gamma `P(a, x) = gamma(a,x) / Gamma(a)`,
/// for `a > 0`, `x >= 0` — this is exactly a chi-square CDF once `a = k/2`,
/// `x` is halved. Dispatches to a power series (fast-converging for
/// `x < a+1`) or a continued fraction (`x >= a+1`, where the series would
/// converge too slowly to be practical) — the standard split.
pub fn regularized_lower_incomplete_gamma(a: f64, x: f64) -> f64 {
    if a <= 0.0 || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        gamma_series(a, x)
    } else {
        1.0 - gamma_continued_fraction(a, x)
    }
}

fn gamma_series(a: f64, x: f64) -> f64 {
    let gln = ln_gamma(a);
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut del = sum;
    for _ in 0..MAX_ITER {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            break;
        }
    }
    sum * (-x + a * x.ln() - gln).exp()
}

fn gamma_continued_fraction(a: f64, x: f64) -> f64 {
    let gln = ln_gamma(a);
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=MAX_ITER {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b + an / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    (-x + a * x.ln() - gln).exp() * h
}

/// Zeroth-order modified Bessel function of the first kind, `I0(x)`, via
/// its defining power series `I0(x) = sum_{k=0}^inf (x/2)^(2k) / (k!)^2`.
/// Only ever called by the `kaiser` window (`beta` typically single digits,
/// `x = beta * sqrt(1 - ...)` in `[0, beta]`), so a plain series (no
/// asymptotic large-`x` branch) is accurate and converges in well under 100
/// terms across that whole practical range. Terms are accumulated
/// iteratively as `term *= (x/2)^2 / k^2` rather than recomputed from
/// scratch each time (avoids repeated `powi`/factorial overflow for larger
/// `k`), and the series stops once a term's contribution drops below the
/// running sum scaled by `f64::EPSILON` — it can no longer change the
/// result at `f64` precision.
pub fn bessel_i0(x: f64) -> f64 {
    let half_x_sq = (x / 2.0) * (x / 2.0);
    let mut term = 1.0;
    let mut sum = term;
    let mut k = 1.0;
    loop {
        term *= half_x_sq / (k * k);
        sum += term;
        if term.abs() < sum.abs() * f64::EPSILON {
            break;
        }
        k += 1.0;
        if k > 1000.0 {
            break; // safety net; never reached for the beta ranges kaiser uses
        }
    }
    sum
}

/// `d/dx ln(Gamma(x))` (the digamma/psi function), for `x > 0` — the
/// standard recurrence-then-asymptotic-series method: `psi(x) = psi(x+1) -
/// 1/x` shifts `x` up past 6 (where the asymptotic series below reaches
/// full `f64` precision) before applying the series itself. Added
/// 2026-08-25 for the k-nearest-neighbor mutual information estimator
/// (Kraskov/Ross — see `mutual_info_classif` in qu-interp) A*-mRMR needs
/// as its relevance criterion; every call there is a digamma of a
/// positive integer-valued count, so unlike `ln_gamma` above this has no
/// reflection formula for `x <= 0` — not a real caller here.
pub fn digamma(mut x: f64) -> f64 {
    let mut result = 0.0;
    while x < 6.0 {
        result -= 1.0 / x;
        x += 1.0;
    }
    let inv = 1.0 / x;
    let inv2 = inv * inv;
    result + x.ln() - 0.5 * inv
        - inv2 * (1.0 / 12.0 - inv2 * (1.0 / 120.0 - inv2 * (1.0 / 252.0 - inv2 / 240.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digamma_matches_known_closed_form_values() {
        // psi(1) = -gamma (Euler-Mascheroni constant); psi(1/2) = -gamma -
        // 2*ln(2); psi(n+1) = -gamma + sum_{k=1}^{n} 1/k (the harmonic
        // recurrence) -- three independent closed forms, not just the same
        // series checked against itself.
        const EULER_MASCHERONI: f64 = 0.577_215_664_901_532_9;
        assert!((digamma(1.0) - (-EULER_MASCHERONI)).abs() < 1e-9);
        assert!((digamma(0.5) - (-EULER_MASCHERONI - 2.0 * 2.0f64.ln())).abs() < 1e-9);
        let harmonic_5 = 1.0 + 0.5 + 1.0 / 3.0 + 0.25 + 0.2;
        assert!((digamma(6.0) - (-EULER_MASCHERONI + harmonic_5)).abs() < 1e-9);
    }

    #[test]
    fn ln_gamma_matches_known_factorial_values() {
        // Gamma(n) = (n-1)! for positive integers.
        assert!(ln_gamma(1.0).abs() < 1e-9); // Gamma(1) = 1, ln(1) = 0
        assert!((ln_gamma(5.0).exp() - 24.0).abs() < 1e-6); // Gamma(5) = 4! = 24
        assert!((ln_gamma(6.0).exp() - 120.0).abs() < 1e-5); // Gamma(6) = 5! = 120
    }

    #[test]
    fn ln_gamma_matches_the_known_half_integer_value() {
        // Gamma(1/2) = sqrt(pi).
        assert!((ln_gamma(0.5).exp() - std::f64::consts::PI.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn regularized_lower_incomplete_gamma_is_the_exponential_cdf_when_a_is_one() {
        // P(1, x) = 1 - exp(-x) exactly (the a=1 special case is the
        // exponential distribution's own CDF) -- a clean, independent check
        // that isn't just re-deriving the same series.
        for &x in &[0.1, 0.5, 1.0, 2.0, 5.0, 10.0] {
            let got = regularized_lower_incomplete_gamma(1.0, x);
            let want = 1.0 - (-x).exp();
            assert!((got - want).abs() < 1e-9, "x={x}: {got} vs {want}");
        }
    }

    #[test]
    fn regularized_lower_incomplete_gamma_reaches_one_in_the_tail() {
        assert!((regularized_lower_incomplete_gamma(3.0, 100.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn regularized_lower_incomplete_gamma_is_zero_at_the_origin() {
        assert_eq!(regularized_lower_incomplete_gamma(2.5, 0.0), 0.0);
    }

    #[test]
    fn bessel_i0_is_one_at_the_origin() {
        // I0(0) = 1 exactly: every term past k=0 in the series has a
        // (x/2)^(2k) factor that vanishes at x=0.
        assert_eq!(bessel_i0(0.0), 1.0);
    }

    #[test]
    fn bessel_i0_matches_known_reference_values() {
        // Standard tabulated values (e.g. Abramowitz & Stegun Table 9.8).
        assert!((bessel_i0(1.0) - 1.266_065_88).abs() < 1e-7);
        assert!((bessel_i0(2.0) - 2.279_585_30).abs() < 1e-7);
        assert!((bessel_i0(5.0) - 27.239_871_82).abs() < 1e-6);
    }

    #[test]
    fn bessel_i0_is_even() {
        // I0(x) = I0(-x): the series only involves even powers of x.
        for &x in &[0.3, 1.7, 4.2] {
            assert!((bessel_i0(x) - bessel_i0(-x)).abs() < 1e-12);
        }
    }
}
