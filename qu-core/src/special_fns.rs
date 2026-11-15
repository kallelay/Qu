//! Special functions (added in v0.4.14): `erfinv`/`erfcinv`, the regularized
//! incomplete gamma and beta functions, `beta`, the Bessel functions
//! `J`, `Y`, `I`, `K` of real order, the exponential integral `E1`, the
//! complete elliptic integrals, associated Legendre functions and the
//! Riemann zeta function.
//!
//! Scalar `f64 -> f64` kernels only; broadcasting over `Vec`/`Mat` is the
//! interpreter glue's job. `erf`, `erfc`, `gamma` and `lgamma` come from
//! `libm`; everything else is here.
//!
//! Out-of-domain arguments return `NaN` (never panic), except where a
//! function returns a `Result` because the caller should say why
//! ([`legendre`], [`zeta`]).
//!
//! Methods (all standard; the file's tests compare each against values
//! computed independently with mpmath at 30 digits, and against identities):
//! * `erfinv`/`erfcinv`: a rational starting guess refined by Halley steps
//!   on `erf`/`erfc` themselves, so the result is as accurate as `libm`'s.
//! * `gammainc`: power series for `x < a + 1`, Lentz continued fraction
//!   beyond; the prefactor `x^a e^-x / Gamma(a)` is evaluated with Loader's
//!   saddle-point form (`stirlerr`, `bd0`) for `a >= 10`, which keeps full
//!   relative accuracy for large `a`.
//! * `betainc`: Lentz continued fraction with the symmetry switch, same
//!   Loader prefactor for `a, b >= 10`.
//! * Bessel: Steed's method (CF1 + CF2) with Temme's series for `x < 2`
//!   (Numerical Recipes' `bessjy`/`bessik` scheme, which handles any real
//!   order), Hankel's asymptotic series with a double-double phase for
//!   `x >= 25`, and reflection formulas for negative order.
//! * `expint`: series for `x <= 1`, continued fraction beyond.
//! * `ellipke`: arithmetic-geometric mean.
//! * `legendre`: the standard upward recurrence in degree.
//! * `zeta`: Euler-Maclaurin summation, the functional equation for `s < 0`.

use std::f64::consts::PI;

const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
const FPMIN: f64 = 1e-300;

// ------------------------------------------------------------ erf inverse

/// Inverse complementary error function: `erfc(erfcinv(x)) = x` for
/// `0 <= x <= 2`; `erfcinv(0) = +inf`, `erfcinv(2) = -inf`, `NaN` outside.
pub fn erfcinv(x: f64) -> f64 {
    if x.is_nan() || !(0.0..=2.0).contains(&x) {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::INFINITY;
    }
    if x == 2.0 {
        return f64::NEG_INFINITY;
    }
    if x > 1.0 {
        return -erfcinv(2.0 - x);
    }
    if x == 1.0 {
        return 0.0;
    }
    // starting guess (Numerical Recipes), good to ~1e-3 relative
    let t = (-2.0 * (x / 2.0).ln()).sqrt();
    let mut z = -std::f64::consts::FRAC_1_SQRT_2
        * ((2.30753 + t * 0.27061) / (1.0 + t * (0.99229 + t * 0.04481)) - t);
    // Halley on f(z) = erfc(z) - x
    for _ in 0..8 {
        let err = libm::erfc(z) - x;
        let g = 2.0 / PI.sqrt() * (-z * z).exp();
        let dz = err / (g - z * err);
        z += dz;
        if !dz.is_finite() || dz.abs() <= 1e-16 * z.abs() {
            break;
        }
    }
    z
}

/// Inverse error function: `erf(erfinv(y)) = y` on `[-1, 1]`;
/// `erfinv(+-1) = +-inf`, `NaN` outside.
pub fn erfinv(y: f64) -> f64 {
    if y.is_nan() || y.abs() > 1.0 {
        return f64::NAN;
    }
    if y == 1.0 {
        return f64::INFINITY;
    }
    if y == -1.0 {
        return f64::NEG_INFINITY;
    }
    if y == 0.0 {
        return y; // keeps the sign of zero
    }
    let a = y.abs();
    let z = if a > 0.5 {
        // 1 - a is exact for a in [0.5, 1]
        erfcinv(1.0 - a)
    } else {
        // start from the series y*sqrt(pi)/2 (1 + pi y^2 / 12 + ...) or the
        // erfc-based guess, then Halley on erf, which is relatively accurate
        // near 0 where erfc(z) = 1 - erf(z) is not
        let mut z = if a < 1e-3 {
            0.5 * PI.sqrt() * a * (1.0 + PI * a * a / 12.0)
        } else {
            erfcinv(1.0 - a)
        };
        for _ in 0..8 {
            let err = libm::erf(z) - a;
            let g = 2.0 / PI.sqrt() * (-z * z).exp();
            let dz = err / (g + z * err);
            z -= dz;
            if !dz.is_finite() || dz.abs() <= 1e-16 * z.abs() {
                break;
            }
        }
        z
    };
    z.copysign(y)
}

// ------------------------------------------------- Loader's saddle point

/// `ln Gamma(n+1) - ln(sqrt(2 pi n) (n/e)^n)`.
fn stirlerr(n: f64) -> f64 {
    if n > 15.0 {
        let n2 = n * n;
        let s = 1.0 / 12.0
            - (1.0 / 360.0 - (1.0 / 1260.0 - (1.0 / 1680.0 - (1.0 / 1188.0 - 691.0 / 360360.0 / n2) / n2) / n2) / n2) / n2;
        s / n
    } else {
        libm::lgamma(n + 1.0) - (n + 0.5) * n.ln() + n - 0.5 * (2.0 * PI).ln()
    }
}

/// `x ln(x/np) + np - x`, evaluated without cancellation when `x ~ np`.
fn bd0(x: f64, np: f64) -> f64 {
    if (x - np).abs() < 0.1 * (x + np) {
        let v = (x - np) / (x + np);
        let mut s = (x - np) * v;
        if s.abs() < f64::MIN_POSITIVE {
            return s;
        }
        let mut ej = 2.0 * x * v;
        let v2 = v * v;
        for j in 1..1000 {
            ej *= v2;
            let s1 = s + ej / (2 * j + 1) as f64;
            if s1 == s {
                return s1;
            }
            s = s1;
        }
        s
    } else {
        x * (x / np).ln() + np - x
    }
}

/// `x^a e^-x / Gamma(a)` for `a > 0`, `x > 0`.
fn gamma_prefactor(a: f64, x: f64) -> f64 {
    if a >= 10.0 {
        // a * Poisson density: exp(-stirlerr(a) - bd0(a, x)) / sqrt(2 pi a) * a
        (-stirlerr(a) - bd0(a, x)).exp() * (a / (2.0 * PI)).sqrt()
    } else {
        (a * x.ln() - x - libm::lgamma(a)).exp()
    }
}

// ----------------------------------------------------- incomplete gamma

/// Which tail of the regularized incomplete gamma function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    Lower,
    Upper,
}

fn gamma_series_p(a: f64, x: f64) -> f64 {
    // x < a + 1: P = prefactor * sum_{n>=0} x^n / (a (a+1) ... (a+n))
    let mut ap = a;
    let mut del = 1.0 / a;
    let mut sum = del;
    for _ in 0..20_000_000 {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * 1e-17 {
            return sum * gamma_prefactor(a, x);
        }
    }
    f64::NAN
}

fn gamma_cf_q(a: f64, x: f64) -> f64 {
    // x >= a + 1: modified Lentz
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..20_000_000u64 {
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
        if (del - 1.0).abs() < 1e-16 {
            return h * gamma_prefactor(a, x);
        }
    }
    f64::NAN
}

/// `ln Gamma(1 + a)` for `0 <= a < 1`, accurate for tiny `a` where `1 + a`
/// loses the low bits of `a` (series `-gamma a + sum (-1)^k zeta(k) a^k / k`).
fn lgamma_1p(a: f64) -> f64 {
    if a >= 0.1 {
        return libm::lgamma(1.0 + a);
    }
    let mut sum = -EULER_GAMMA * a;
    let mut pw = -a; // (-a)^k, k = 1
    for k in 2..30 {
        pw *= -a;
        let t = zeta_em(k as f64) * pw / k as f64;
        sum += t;
        if t.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    sum
}

/// `Q(a, x)` for `a < 1`, `x < 1.5` without the cancellation of `1 - P`
/// (DiDonato and Morris): `Q = (1 - t) - t a S`, `t = x^a / Gamma(a+1)`.
fn gamma_q_small(a: f64, x: f64) -> f64 {
    let u = a * x.ln() - lgamma_1p(a);
    let t = u.exp();
    let mut term = 1.0;
    let mut s = 0.0;
    for n in 1..200 {
        term *= -x / n as f64;
        let v = term / (a + n as f64);
        s += v;
        if v.abs() < 1e-18 * s.abs().max(1e-300) {
            break;
        }
    }
    -u.exp_m1() - t * a * s
}

/// Regularized incomplete gamma function: `P(a, x)` (`Lower`) or
/// `Q(a, x) = 1 - P(a, x)` (`Upper`), `a >= 0`, `x >= 0`.
pub fn gammainc(x: f64, a: f64, tail: Tail) -> f64 {
    if x.is_nan() || a.is_nan() || x < 0.0 || a < 0.0 {
        return f64::NAN;
    }
    let (lo, up) = (tail == Tail::Lower, tail == Tail::Upper);
    let pick = |p: f64, q: f64| if lo { p } else { q };
    let _ = up;
    if a == 0.0 {
        return pick(1.0, 0.0);
    }
    if x == 0.0 {
        return pick(0.0, 1.0);
    }
    if x.is_infinite() {
        if a.is_infinite() {
            return f64::NAN;
        }
        return pick(1.0, 0.0);
    }
    if a.is_infinite() {
        return pick(0.0, 1.0);
    }
    if x < a + 1.0 {
        if !lo && a < 1.0 && x < 1.5 {
            return gamma_q_small(a, x);
        }
        let p = gamma_series_p(a, x);
        pick(p, 1.0 - p)
    } else {
        let q = gamma_cf_q(a, x);
        pick(1.0 - q, q)
    }
}

// ------------------------------------------------------- beta functions

/// `ln B(a, b)` for `a, b > 0` without the cancellation of
/// `lgamma(a) + lgamma(b) - lgamma(a + b)` when the arguments are large
/// (Stirling's series for every argument that is at least 10).
fn lbeta(a: f64, b: f64) -> f64 {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    if b < 10.0 {
        return libm::lgamma(a) + libm::lgamma(b) - libm::lgamma(a + b);
    }
    let n = a + b;
    if a >= 10.0 {
        0.5 * (2.0 * PI).ln() + a * (a / n).ln() + b * (b / n).ln()
            - 0.5 * (a.ln() + b.ln() - n.ln())
            + stirlerr(a)
            + stirlerr(b)
            - stirlerr(n)
    } else {
        // lgamma(a) + [ln Gamma(b) - ln Gamma(a + b)]
        libm::lgamma(a) - (b - 0.5) * (a / b).ln_1p() - a * n.ln() + a + stirlerr(b) - stirlerr(n)
    }
}

/// The beta function `B(a, b)` for `a, b > 0` (`NaN` otherwise).
pub fn beta(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() || a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if a.is_infinite() || b.is_infinite() {
        return 0.0;
    }
    if a + b <= 170.0 {
        libm::tgamma(a) / libm::tgamma(a + b) * libm::tgamma(b)
    } else {
        lbeta(a, b).exp()
    }
}

/// `x^a (1-x)^b / B(a, b)`.
fn beta_front(x: f64, q: f64, a: f64, b: f64) -> f64 {
    if a >= 10.0 && b >= 10.0 {
        let n = a + b;
        let dbin = (stirlerr(n) - stirlerr(a) - stirlerr(b) - bd0(a, n * x) - bd0(b, n * q)).exp()
            * (n / (2.0 * PI * a * b)).sqrt();
        dbin * a * b / n
    } else {
        (a * x.ln() + b * (-x).ln_1p() - lbeta(a, b)).exp()
    }
}

fn beta_cf(x: f64, a: f64, b: f64) -> f64 {
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..5_000_000u64 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            return h;
        }
    }
    f64::NAN
}

/// Regularized incomplete beta function `I_x(a, b)` for `0 <= x <= 1`,
/// `a, b > 0` (`NaN` otherwise).
pub fn betainc(x: f64, a: f64, b: f64) -> f64 {
    if x.is_nan() || a.is_nan() || b.is_nan() || !(0.0..=1.0).contains(&x) || a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    if a.is_infinite() || b.is_infinite() {
        // limits: a -> inf pushes the mass to x = 1, b -> inf to x = 0
        return match (a.is_infinite(), b.is_infinite()) {
            (true, true) => f64::NAN,
            (true, false) => 0.0,
            _ => 1.0,
        };
    }
    let q = 1.0 - x;
    let front = beta_front(x, q, a, b);
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(x, a, b) / a
    } else {
        1.0 - front * beta_cf(q, b, a) / b
    }
}

// -------------------------------------------------------- Bessel kernels

const BESSEL_EPS: f64 = 4.0e-16;
const BESSEL_MAXIT: usize = 3_000_000;
/// Largest order accepted (the order recurrences are linear in it).
pub const BESSEL_MAX_ORDER: f64 = 1.0e6;

/// Coefficients `c_k` of `1/Gamma(z) = sum_{k>=1} c_k z^k` (Abramowitz and
/// Stegun 6.1.34).
const RGAMMA_C: [f64; 26] = [
    1.0,
    0.5772156649015329,
    -0.6558780715202539,
    -0.04200263503409524,
    0.16653861138229148,
    -0.04219773455554433,
    -0.009621971527876973,
    0.0072189432466631,
    -0.0011651675918590652,
    -0.00021524167411495098,
    0.0001280502823881162,
    -2.013485478078824e-05,
    -1.2504934821426706e-06,
    1.133027231981696e-06,
    -2.056338416977607e-07,
    6.116095104481416e-09,
    5.002007644469223e-09,
    -1.18127457048702e-09,
    1.0434267116911005e-10,
    7.782263439905071e-12,
    -3.696805618642206e-12,
    5.100370287454476e-13,
    -2.0583260535665066e-14,
    -5.348122539423018e-15,
    1.2267786282382608e-15,
    -1.1812593016974588e-16,
];

/// `(gam1, gam2, 1/Gamma(1+mu), 1/Gamma(1-mu))` for `|mu| <= 1/2`, with
/// `gam1 = (1/Gamma(1-mu) - 1/Gamma(1+mu)) / (2 mu)` and
/// `gam2 = (1/Gamma(1-mu) + 1/Gamma(1+mu)) / 2` (Temme's coefficients).
fn temme_gammas(mu: f64) -> (f64, f64, f64, f64) {
    let m2 = mu * mu;
    // 1/Gamma(1+z) = sum_{k>=0} c_{k+1} z^k  (RGAMMA_C[k])
    let (mut even, mut odd) = (0.0, 0.0); // sums over even / odd powers, without mu
    let mut pw = 1.0;
    // even part: c_1 + c_3 m2 + c_5 m2^2 ...   odd part: c_2 + c_4 m2 + ...
    for j in 0..13 {
        even += RGAMMA_C[2 * j] * pw;
        odd += RGAMMA_C[2 * j + 1] * pw;
        pw *= m2;
    }
    let gam2 = even;
    let gam1 = -odd;
    let gampl = even + mu * odd;
    let gammi = even - mu * odd;
    (gam1, gam2, gampl, gammi)
}

/// Double-double reduction: `x - k*pi` reduced modulo `2 pi` to `[-pi, pi]`.
fn phase_reduce(x: f64, k: f64) -> f64 {
    const PI_HI: f64 = 3.141_592_653_589_793;
    const PI_LO: f64 = 1.224_646_799_147_353_2e-16;
    const TWO_PI_HI: f64 = 6.283_185_307_179_586;
    const TWO_PI_LO: f64 = 2.449_293_598_294_706_4e-16;
    let p = k * PI_HI;
    let e = k.mul_add(PI_HI, -p); // exact error of the product
    // two-sum of x - p
    let s = x - p;
    let bb = s - x;
    let err = (x - (s - bb)) + (-p - bb);
    let lo = err - e - k * PI_LO;
    let n = (s / TWO_PI_HI).round();
    // s - n*2pi, exact leading part via fma
    let r = (-n).mul_add(TWO_PI_HI, s) - n * TWO_PI_LO + lo;
    r
}

/// Hankel's asymptotic sums `(P, Q)` for `J`/`Y` (`alternate = true`) or the
/// non-alternating-sign pair used for `K`/`I` (`alternate = false`, only the
/// `P` slot is meaningful: `sum a_k / x^k`, and `Q` the alternating one).
/// `None` if the series does not reach `1e-17` before its terms grow.
fn hankel_sums(nu: f64, x: f64) -> Option<(f64, f64, f64, f64)> {
    // returns (P, Q, S_plus, S_alt): P,Q for J/Y; S_plus = sum a_k/x^k, S_alt = sum (-1)^k a_k/x^k
    let mu4 = 4.0 * nu * nu;
    let (mut p, mut q) = (1.0, 0.0);
    let (mut s_plus, mut s_alt) = (1.0, 1.0);
    let mut term = 1.0f64;
    for k in 1..400usize {
        let kf = k as f64;
        let odd = (2.0 * kf - 1.0).powi(2);
        let next = term * (mu4 - odd) / (kf * 8.0 * x);
        if next.abs() > term.abs() && next.abs() > 1e-17 {
            return None;
        }
        term = next;
        match k % 4 {
            0 => p += term,
            1 => q += term,
            2 => p -= term,
            _ => q -= term,
        }
        s_plus += term;
        s_alt += if k % 2 == 0 { term } else { -term };
        if term.abs() < 1e-17 {
            return Some((p, q, s_plus, s_alt));
        }
        if term == 0.0 {
            return Some((p, q, s_plus, s_alt));
        }
    }
    None
}

/// `(J_nu(x), Y_nu(x))` for `nu >= 0`, `x > 0` finite.
fn hankel_jy(nu: f64, x: f64) -> Option<(f64, f64)> {
    let (p, q, _, _) = hankel_sums(nu, x)?;
    let r = phase_reduce(x, 0.5 * nu + 0.25);
    let (s, c) = r.sin_cos();
    let amp = (2.0 / (PI * x)).sqrt();
    Some((amp * (p * c - q * s), amp * (p * s + q * c)))
}

fn bessel_jy(nu: f64, x: f64) -> Option<(f64, f64)> {
    if x >= 25.0 {
        if let Some(v) = hankel_jy(nu, x) {
            return Some(v);
        }
        // Oscillatory region (x >= nu) with a large order: the Hankel series is
        // useless at order nu but fine at orders mu and mu + 1 (mu < 1), and
        // the forward recurrence in the order is stable for both J and Y while
        // x >= nu, so start there and climb.
        if nu <= x {
            let n = nu.floor() as usize;
            let mu = nu - n as f64;
            if let (Some((mut jm, mut ym)), Some((mut j, mut y))) = (hankel_jy(mu, x), hankel_jy(mu + 1.0, x)) {
                for k in 1..n {
                    let c = 2.0 * (mu + k as f64) / x;
                    let (jn, yn) = (c * j - jm, c * y - ym);
                    jm = j;
                    ym = y;
                    j = jn;
                    y = yn;
                }
                return Some(if n == 0 { hankel_jy(mu, x)? } else { (j, y) });
            }
        }
    }
    let xmin = 2.0;
    let nl = if x < xmin { (nu + 0.5) as usize } else { ((nu - x + 1.5) as i64).max(0) as usize };
    let xmu = nu - nl as f64;
    let xmu2 = xmu * xmu;
    let xi = 1.0 / x;
    let xi2 = 2.0 * xi;
    let w = xi2 / PI;
    // CF1 by modified Lentz
    let mut isign = 1.0f64;
    let mut h = (nu * xi).max(FPMIN);
    let mut b = xi2 * nu;
    let mut d = 0.0;
    let mut c = h;
    let mut converged = false;
    for _ in 0..BESSEL_MAXIT {
        b += xi2;
        d = b - d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b - 1.0 / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = c * d;
        h *= del;
        if d < 0.0 {
            isign = -isign;
        }
        if (del - 1.0).abs() < BESSEL_EPS {
            converged = true;
            break;
        }
    }
    if !converged {
        return None;
    }
    let mut rjl = isign;
    let mut rjpl = h * rjl;
    let mut rjl1 = rjl;
    let mut rjp1 = rjpl;
    let mut fact = nu * xi;
    for _ in 0..nl {
        let rjtemp = fact * rjl + rjpl;
        fact -= xi;
        rjpl = fact * rjtemp - rjl;
        rjl = rjtemp;
        if rjl.abs() > 1e200 {
            rjl *= 1e-200;
            rjpl *= 1e-200;
            rjl1 *= 1e-200;
            rjp1 *= 1e-200;
        }
    }
    let _ = rjp1;
    if rjl == 0.0 {
        rjl = BESSEL_EPS;
    }
    let f = rjpl / rjl;
    let (rjmu, mut rymu, mut ry1);
    if x < xmin {
        let x2 = 0.5 * x;
        let pimu = PI * xmu;
        let fct = if pimu.abs() < BESSEL_EPS { 1.0 } else { pimu / pimu.sin() };
        let dd = -x2.ln();
        let e = xmu * dd;
        let fact2 = if e.abs() < BESSEL_EPS { 1.0 } else { e.sinh() / e };
        let (gam1, gam2, gampl, gammi) = temme_gammas(xmu);
        let mut ff = 2.0 / PI * fct * (gam1 * e.cosh() + gam2 * fact2 * dd);
        let e = e.exp();
        let mut p = e / (gampl * PI);
        let mut q = 1.0 / (e * PI * gammi);
        let pimu2 = 0.5 * pimu;
        let fact3 = if pimu2.abs() < BESSEL_EPS { 1.0 } else { pimu2.sin() / pimu2 };
        let r = PI * pimu2 * fact3 * fact3;
        let mut cc = 1.0;
        let dq = -x2 * x2;
        let mut sum = ff + r * q;
        let mut sum1 = p;
        let mut ok = false;
        for i in 1..10_000usize {
            let fi = i as f64;
            ff = (fi * ff + p + q) / (fi * fi - xmu2);
            cc *= dq / fi;
            p /= fi - xmu;
            q /= fi + xmu;
            let del = cc * (ff + r * q);
            sum += del;
            let del1 = cc * p - fi * del;
            sum1 += del1;
            if del.abs() < (1.0 + sum.abs()) * BESSEL_EPS {
                ok = true;
                break;
            }
        }
        if !ok {
            return None;
        }
        rymu = -sum;
        ry1 = -sum1 * xi2;
        let rymup = xmu * xi * rymu - ry1;
        rjmu = w / (rymup - f * rymu);
    } else {
        let mut a = 0.25 - xmu2;
        let mut p = -0.5 * xi;
        let mut q = 1.0;
        let br = 2.0 * x;
        let mut bi = 2.0;
        let mut fct = a * xi / (p * p + q * q);
        let mut cr = br + q * fct;
        let mut ci = bi + p * fct;
        let mut den = br * br + bi * bi;
        let mut dr = br / den;
        let mut di = -bi / den;
        let mut dlr = cr * dr - ci * di;
        let mut dli = cr * di + ci * dr;
        let mut temp = p * dlr - q * dli;
        q = p * dli + q * dlr;
        p = temp;
        let mut ok = false;
        for i in 2..BESSEL_MAXIT {
            a += 2.0 * (i as f64 - 1.0);
            bi += 2.0;
            dr = a * dr + br;
            di = a * di + bi;
            if dr.abs() + di.abs() < FPMIN {
                dr = FPMIN;
            }
            fct = a / (cr * cr + ci * ci);
            cr = br + cr * fct;
            ci = bi - ci * fct;
            if cr.abs() + ci.abs() < FPMIN {
                cr = FPMIN;
            }
            den = dr * dr + di * di;
            dr /= den;
            di /= -den;
            dlr = cr * dr - ci * di;
            dli = cr * di + ci * dr;
            temp = p * dlr - q * dli;
            q = p * dli + q * dlr;
            p = temp;
            if (dlr - 1.0).abs() + dli.abs() < BESSEL_EPS {
                ok = true;
                break;
            }
        }
        if !ok {
            return None;
        }
        let gam = (p - f) / q;
        let r = (w / ((p - f) * gam + q)).sqrt();
        rjmu = if rjl < 0.0 { -r } else { r };
        rymu = rjmu * gam;
        let rymup = rymu * (p + q / gam);
        ry1 = xmu * xi * rymu - rymup;
    }
    let fct = rjmu / rjl;
    let rj = rjl1 * fct;
    for i in 1..=nl {
        let rytemp = (xmu + i as f64) * xi2 * ry1 - rymu;
        rymu = ry1;
        ry1 = rytemp;
    }
    Some((rj, rymu))
}

/// `(I_nu(x) e^-x, K_nu(x) e^x)` for `nu >= 0`, `x > 0` finite.
fn bessel_ik_scaled(nu: f64, x: f64) -> Option<(f64, f64)> {
    if x >= 25.0 {
        if let Some((_, _, s_plus, s_alt)) = hankel_sums(nu, x) {
            let ke = (PI / (2.0 * x)).sqrt() * s_plus;
            let ie = s_alt / (2.0 * PI * x).sqrt();
            return Some((ie, ke));
        }
    }
    let nl = (nu + 0.5) as usize;
    let xmu = nu - nl as f64;
    let xmu2 = xmu * xmu;
    let xi = 1.0 / x;
    let xi2 = 2.0 * xi;
    let mut h = (nu * xi).max(FPMIN);
    let mut b = xi2 * nu;
    let mut d = 0.0;
    let mut c = h;
    let mut converged = false;
    for _ in 0..BESSEL_MAXIT {
        b += xi2;
        d = 1.0 / (b + d);
        c = b + 1.0 / c;
        let del = c * d;
        h *= del;
        if (del - 1.0).abs() < BESSEL_EPS {
            converged = true;
            break;
        }
    }
    if !converged {
        return None;
    }
    let mut ril = 1.0f64;
    let mut ripl = h * ril;
    let mut ril1 = ril;
    let mut fact = nu * xi;
    for _ in 0..nl {
        let ritemp = fact * ril + ripl;
        fact -= xi;
        ripl = fact * ritemp + ril;
        ril = ritemp;
        if ril.abs() > 1e200 {
            ril *= 1e-200;
            ripl *= 1e-200;
            ril1 *= 1e-200;
        }
    }
    let f = ripl / ril;
    let (mut rkmu, mut rk1);
    if x < 2.0 {
        let x2 = 0.5 * x;
        let pimu = PI * xmu;
        let fct = if pimu.abs() < BESSEL_EPS { 1.0 } else { pimu / pimu.sin() };
        let dd = -x2.ln();
        let e = xmu * dd;
        let fact2 = if e.abs() < BESSEL_EPS { 1.0 } else { e.sinh() / e };
        let (gam1, gam2, gampl, gammi) = temme_gammas(xmu);
        let mut ff = fct * (gam1 * e.cosh() + gam2 * fact2 * dd);
        let mut sum = ff;
        let e = e.exp();
        let mut p = 0.5 * e / gampl;
        let mut q = 0.5 / (e * gammi);
        let mut cc = 1.0;
        let dq = x2 * x2;
        let mut sum1 = p;
        let mut ok = false;
        for i in 1..10_000usize {
            let fi = i as f64;
            ff = (fi * ff + p + q) / (fi * fi - xmu2);
            cc *= dq / fi;
            p /= fi - xmu;
            q /= fi + xmu;
            let del = cc * ff;
            sum += del;
            let del1 = cc * (p - fi * ff);
            sum1 += del1;
            if del.abs() < sum.abs() * BESSEL_EPS {
                ok = true;
                break;
            }
        }
        if !ok {
            return None;
        }
        let ex = x.exp();
        rkmu = sum * ex;
        rk1 = sum1 * xi2 * ex;
    } else {
        let mut b = 2.0 * (1.0 + x);
        let mut d = 1.0 / b;
        let mut h = d;
        let mut delh = d;
        let mut q1 = 0.0;
        let mut q2 = 1.0;
        let a1 = 0.25 - xmu2;
        let mut q = a1;
        let mut c = a1;
        let mut a = -a1;
        let mut s = 1.0 + q * delh;
        let mut ok = false;
        for i in 2..BESSEL_MAXIT {
            a -= 2.0 * (i as f64 - 1.0);
            c = -a * c / i as f64;
            let qnew = (q1 - b * q2) / a;
            q1 = q2;
            q2 = qnew;
            q += c * qnew;
            b += 2.0;
            d = 1.0 / (b + a * d);
            delh = (b * d - 1.0) * delh;
            h += delh;
            let dels = q * delh;
            s += dels;
            if (dels / s).abs() < BESSEL_EPS {
                ok = true;
                break;
            }
        }
        if !ok {
            return None;
        }
        h *= a1;
        rkmu = (PI / (2.0 * x)).sqrt() / s;
        rk1 = rkmu * (xmu + x + 0.5 - h) * xi;
    }
    let rkmup = xmu * xi * rkmu - rk1;
    let rimu = xi / (f * rkmu - rkmup);
    let ie = (rimu * ril1) / ril;
    for i in 1..=nl {
        let rktemp = (xmu + i as f64) * xi2 * rk1 + rkmu;
        rkmu = rk1;
        rk1 = rktemp;
    }
    Some((ie, rkmu))
}

fn is_int(nu: f64) -> bool {
    nu.fract() == 0.0
}

fn parity_sign(nu: f64) -> f64 {
    if nu.abs() % 2.0 == 1.0 {
        -1.0
    } else {
        1.0
    }
}

fn nu_ok(nu: f64) -> bool {
    nu.is_finite() && nu.abs() <= BESSEL_MAX_ORDER
}

/// Bessel function of the first kind `J_nu(x)`, any real order with
/// `|nu| <= 1e6`; for `x < 0` only integer order is real (otherwise `NaN`).
pub fn bessel_j(nu: f64, x: f64) -> f64 {
    if x.is_nan() || !nu_ok(nu) {
        return f64::NAN;
    }
    if x < 0.0 {
        return if is_int(nu) { parity_sign(nu) * bessel_j(nu, -x) } else { f64::NAN };
    }
    if nu < 0.0 {
        if is_int(nu) {
            return parity_sign(nu) * bessel_j(-nu, x);
        }
        if x == 0.0 {
            return libm::tgamma(1.0 - nu).signum() * f64::INFINITY;
        }
        if x.is_infinite() {
            return 0.0;
        }
        let n = -nu;
        return match bessel_jy(n, x) {
            Some((j, y)) => (n * PI).cos() * j - (n * PI).sin() * y,
            None => f64::NAN,
        };
    }
    if x == 0.0 {
        return if nu == 0.0 { 1.0 } else { 0.0 };
    }
    if x.is_infinite() {
        return 0.0;
    }
    bessel_jy(nu, x).map_or(f64::NAN, |v| v.0)
}

/// Bessel function of the second kind `Y_nu(x)` for `x > 0` (`x = 0` gives
/// `-inf`, `x < 0` is `NaN`: the value is complex there).
pub fn bessel_y(nu: f64, x: f64) -> f64 {
    if x.is_nan() || !nu_ok(nu) || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return 0.0;
    }
    if nu < 0.0 {
        if is_int(nu) {
            return parity_sign(nu) * bessel_y(-nu, x);
        }
        let n = -nu;
        return match bessel_jy(n, x) {
            Some((j, y)) => (n * PI).sin() * j + (n * PI).cos() * y,
            None => f64::NAN,
        };
    }
    bessel_jy(nu, x).map_or(f64::NAN, |v| v.1)
}

/// Modified Bessel function of the first kind `I_nu(x)`; for `x < 0` only
/// integer order is real (otherwise `NaN`).
pub fn bessel_i(nu: f64, x: f64) -> f64 {
    if x.is_nan() || !nu_ok(nu) {
        return f64::NAN;
    }
    if x < 0.0 {
        return if is_int(nu) { parity_sign(nu) * bessel_i(nu, -x) } else { f64::NAN };
    }
    if nu < 0.0 && is_int(nu) {
        return bessel_i(-nu, x);
    }
    if x == 0.0 {
        return if nu == 0.0 {
            1.0
        } else if nu > 0.0 {
            0.0
        } else {
            f64::INFINITY * libm::tgamma(1.0 - nu).signum()
        };
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    if nu < 0.0 {
        let n = -nu;
        return match bessel_ik_scaled(n, x) {
            Some((ie, ke)) => {
                // I_{-n} = I_n + (2/pi) sin(n pi) K_n, in units of e^x
                (ie + 2.0 / PI * (n * PI).sin() * ke * (-2.0 * x).exp()) * x.exp()
            }
            None => f64::NAN,
        };
    }
    bessel_ik_scaled(nu, x).map_or(f64::NAN, |v| v.0 * x.exp())
}

/// Modified Bessel function of the second kind `K_nu(x)` for `x > 0`
/// (`x = 0` gives `+inf`, `x < 0` is `NaN`).
pub fn bessel_k(nu: f64, x: f64) -> f64 {
    if x.is_nan() || !nu_ok(nu) || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::INFINITY;
    }
    if x.is_infinite() {
        return 0.0;
    }
    let nu = nu.abs();
    bessel_ik_scaled(nu, x).map_or(f64::NAN, |v| v.1 * (-x).exp())
}

// ------------------------------------------------- exponential integral

/// The exponential integral `E1(x) = integral_x^inf e^-t / t dt` for
/// `x > 0` (`+inf` at 0, `NaN` for `x < 0`, where it is complex).
pub fn expint(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::INFINITY;
    }
    if x.is_infinite() {
        return 0.0;
    }
    if x <= 1.0 {
        let mut sum = 0.0;
        let mut term = 1.0;
        for k in 1..100 {
            term *= -x / k as f64;
            let t = -term / k as f64;
            sum += t;
            if t.abs() < 1e-18 * sum.abs().max(1e-300) {
                break;
            }
        }
        -EULER_GAMMA - x.ln() + sum
    } else {
        let mut b = x + 1.0;
        let mut c = 1.0 / FPMIN;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..100_000u64 {
            let an = -((i * i) as f64);
            b += 2.0;
            d = 1.0 / (an * d + b);
            c = b + an / c;
            let del = c * d;
            h *= del;
            if (del - 1.0).abs() < 1e-16 {
                break;
            }
        }
        h * (-x).exp()
    }
}

// ----------------------------------------------------- elliptic integrals

/// Complete elliptic integrals `(K(m), E(m))` of the first and second kind
/// with parameter `m = k^2`, for `m <= 1` (`K(1) = inf`, `E(1) = 1`;
/// `NaN` for `m > 1`).
pub fn ellipke(m: f64) -> (f64, f64) {
    if m.is_nan() || m > 1.0 || m == f64::NEG_INFINITY {
        return (f64::NAN, f64::NAN);
    }
    if m == 1.0 {
        return (f64::INFINITY, 1.0);
    }
    let mut a = 1.0;
    let mut b = (1.0 - m).sqrt();
    let mut c2 = m; // c_n^2
    let mut sum = 0.5 * m; // sum 2^(n-1) c_n^2, n = 0
    let mut pow = 0.5;
    for _ in 0..60 {
        let a1 = 0.5 * (a + b);
        let c1 = 0.5 * (a - b);
        b = (a * b).sqrt();
        a = a1;
        pow *= 2.0;
        c2 = c1 * c1;
        sum += pow * c2;
        if c1.abs() <= 1e-17 * a {
            break;
        }
    }
    let _ = c2;
    let k = PI / (2.0 * a);
    (k, k * (1.0 - sum))
}

// --------------------------------------------------------------- Legendre

/// Associated Legendre functions `P_n^m(x)` for `m = 0..=n` (unnormalised,
/// Condon-Shortley phase `(-1)^m` included, as in MATLAB), for `|x| <= 1`.
pub fn legendre(n: usize, x: f64) -> Result<Vec<f64>, String> {
    if x.is_nan() {
        return Ok(vec![f64::NAN; n + 1]);
    }
    if x.abs() > 1.0 {
        return Err(format!("legendre: x must lie in [-1, 1], got {x}"));
    }
    let s = ((1.0 - x) * (1.0 + x)).sqrt();
    let mut out = vec![0.0; n + 1];
    // P_m^m = (-1)^m (2m-1)!! s^m
    let mut pmm = 1.0f64;
    for m in 0..=n {
        if m > 0 {
            pmm *= -((2 * m - 1) as f64) * s;
        }
        // upward in degree from l = m to n
        let mut p_prev = pmm; // P_m^m
        if n == m {
            out[m] = p_prev;
            continue;
        }
        let mut p_cur = x * (2 * m + 1) as f64 * pmm; // P_{m+1}^m
        for l in (m + 2)..=n {
            let lf = l as f64;
            let mf = m as f64;
            let next = (x * (2.0 * lf - 1.0) * p_cur - (lf + mf - 1.0) * p_prev) / (lf - mf);
            p_prev = p_cur;
            p_cur = next;
        }
        out[m] = if n == m + 1 { p_cur } else { p_cur };
    }
    Ok(out)
}

// ------------------------------------------------------------------- zeta

/// Riemann zeta function for real `s != 1`; errors at the pole `s = 1`.
/// `zeta(+inf) = 1`; `-inf` and `NaN` give `NaN`.
pub fn zeta(s: f64) -> Result<f64, String> {
    if s.is_nan() {
        return Ok(f64::NAN);
    }
    if s == 1.0 {
        return Err("zeta: pole at s = 1 (the series diverges)".to_string());
    }
    if s == f64::INFINITY {
        return Ok(1.0);
    }
    if s == f64::NEG_INFINITY {
        return Ok(f64::NAN);
    }
    if s < 0.0 {
        if s.fract() == 0.0 && (s as i64) % 2 == 0 {
            return Ok(0.0); // trivial zeros
        }
        // functional equation: zeta(s) = 2^s pi^(s-1) sin(pi s/2) Gamma(1-s) zeta(1-s)
        let z = zeta_em(1.0 - s);
        // sin(pi s / 2) with s reduced mod 4 for accuracy
        let r = s.rem_euclid(4.0);
        let sn = (PI * r / 2.0).sin();
        return Ok(2f64.powf(s) * PI.powf(s - 1.0) * sn * libm::tgamma(1.0 - s) * z);
    }
    Ok(zeta_em(s))
}

/// Euler-Maclaurin for `s >= 0`, `s != 1`.
fn zeta_em(s: f64) -> f64 {
    const N: usize = 20;
    // B_{2j} / (2j)!
    const B: [f64; 10] = [
        1.0 / 12.0,
        -1.0 / 720.0,
        1.0 / 30240.0,
        -1.0 / 1209600.0,
        1.0 / 47900160.0,
        -691.0 / 1307674368000.0,
        1.0 / 74724249600.0,
        -3617.0 / 10670622842880000.0,
        43867.0 / 5109094217170944000.0,
        -174611.0 / 802857662698291200000.0,
    ];
    let nf = N as f64;
    let mut sum = 0.0;
    // small terms first
    for k in (1..N).rev() {
        sum += (k as f64).powf(-s);
    }
    sum += nf.powf(1.0 - s) / (s - 1.0) + 0.5 * nf.powf(-s);
    // tail corrections
    let mut rising = s; // s (s+1) ... (s + 2j - 2)
    let mut npow = nf.powf(-s - 1.0); // N^(-s-2j+1) for j = 1
    for (j, b) in B.iter().enumerate() {
        let jj = (j + 1) as f64;
        sum += b * rising * npow;
        rising *= (s + 2.0 * jj - 1.0) * (s + 2.0 * jj);
        npow /= nf * nf;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(a: f64, b: f64) -> f64 {
        if a == b {
            0.0
        } else {
            (a - b).abs() / b.abs().max(1e-300)
        }
    }

    fn assert_rel(name: &str, got: f64, want: f64, tol: f64) {
        assert!(rel(got, want) <= tol, "{name}: got {got:e}, want {want:e}, rel err {:e}", rel(got, want));
    }

    #[test]
    fn rgamma_coefficients_match_gamma() {
        // the A&S series must reproduce 1/Gamma(1+z) for |z| <= 0.5
        for &z in &[-0.5, -0.3, -0.1, 0.0, 0.05, 0.25, 0.5] {
            let (_, _, gp, gm) = temme_gammas(z);
            assert!((gp - 1.0 / libm::tgamma(1.0 + z)).abs() < 4e-15, "1/G(1+{z})");
            assert!((gm - 1.0 / libm::tgamma(1.0 - z)).abs() < 4e-15, "1/G(1-{z})");
        }
    }

    #[test]
    fn erfinv_inverts_erf_and_edges() {
        for &x in &[-2.5, -1.0, -0.3, -1e-8, 1e-12, 1e-4, 0.1, 0.5, 0.9, 2.0, 2.5] {
            let y = libm::erf(x);
            if y.abs() == 1.0 {
                continue;
            }
            assert_rel("erfinv", erfinv(y), x, 1e-13);
        }
        assert_eq!(erfinv(0.0), 0.0);
        assert_eq!(erfinv(1.0), f64::INFINITY);
        assert_eq!(erfinv(-1.0), f64::NEG_INFINITY);
        assert!(erfinv(1.0000001).is_nan() && erfinv(f64::NAN).is_nan());
        // published value: erfinv(0.5) = 0.4769362762044699
        assert_rel("erfinv(.5)", erfinv(0.5), 0.476_936_276_204_469_9, 1e-15);
    }

    #[test]
    fn erfcinv_inverts_erfc_into_the_far_tail() {
        for &x in &[0.0, 0.5, 1.0, 3.0, 5.0, 8.0, 15.0, 25.0] {
            let y = libm::erfc(x);
            assert_rel("erfcinv", erfcinv(y), x, 2e-13);
        }
        assert_eq!(erfcinv(0.0), f64::INFINITY);
        assert_eq!(erfcinv(2.0), f64::NEG_INFINITY);
        assert_eq!(erfcinv(1.0), 0.0);
        assert!(erfcinv(-0.1).is_nan() && erfcinv(2.1).is_nan());
        assert_rel("erfcinv(1.5)", erfcinv(1.5), -erfcinv(0.5), 1e-15);
    }

    #[test]
    fn gammainc_closed_forms() {
        for &x in &[1e-8, 0.01, 0.3, 1.0, 2.5, 10.0, 50.0] {
            // P(1, x) = 1 - e^-x, Q(1, x) = e^-x
            assert_rel("P(1,x)", gammainc(x, 1.0, Tail::Lower), -(-x).exp_m1(), 1e-14);
            assert_rel("Q(1,x)", gammainc(x, 1.0, Tail::Upper), (-x).exp(), 1e-14);
            // P(1/2, x) = erf(sqrt x), Q = erfc(sqrt x)
            assert_rel("P(.5,x)", gammainc(x, 0.5, Tail::Lower), libm::erf(x.sqrt()), 1e-14);
            assert_rel("Q(.5,x)", gammainc(x, 0.5, Tail::Upper), libm::erfc(x.sqrt()), 1e-13);
        }
    }

    #[test]
    fn gammainc_poisson_sums() {
        // Q(n, x) = e^-x sum_{k<n} x^k / k!  (independent of the series/CF code)
        for &(n, x) in &[(3usize, 2.0f64), (10, 4.0), (10, 25.0), (50, 40.0), (50, 60.0), (100, 90.0), (100, 120.0), (200, 200.0)] {
            let mut term = (-x).exp();
            let mut s = 0.0;
            // work in logs for the big cases to avoid underflow of e^-x
            let lt0 = -x;
            let mut lterm = lt0;
            for k in 0..n {
                if k > 0 {
                    lterm += x.ln() - (k as f64).ln();
                }
                s += lterm.exp();
            }
            let _ = &mut term;
            assert_rel(&format!("Q({n},{x})"), gammainc(x, n as f64, Tail::Upper), s, 1e-12);
            assert!((gammainc(x, n as f64, Tail::Lower) + gammainc(x, n as f64, Tail::Upper) - 1.0).abs() < 1e-14);
        }
    }

    #[test]
    fn gammainc_edges() {
        assert_eq!(gammainc(0.0, 2.0, Tail::Lower), 0.0);
        assert_eq!(gammainc(0.0, 2.0, Tail::Upper), 1.0);
        assert_eq!(gammainc(f64::INFINITY, 2.0, Tail::Lower), 1.0);
        assert_eq!(gammainc(3.0, 0.0, Tail::Lower), 1.0);
        assert!(gammainc(-1.0, 2.0, Tail::Lower).is_nan());
        assert!(gammainc(1.0, -2.0, Tail::Lower).is_nan());
        assert!(gammainc(f64::NAN, 2.0, Tail::Lower).is_nan());
        // far tails keep relative accuracy: Q(2, 40) = 41 e^-40
        assert_rel("Q(2,40)", gammainc(40.0, 2.0, Tail::Upper), 41.0 * (-40.0f64).exp(), 1e-13);
        // tiny a: Q(a, x) ~ a E1(x) for a -> 0
        let q = gammainc(0.5, 1e-9, Tail::Upper);
        assert_rel("Q(1e-9,.5)", q, 5.597_735_950_695_406e-10, 1e-14);
    }

    #[test]
    fn betainc_identities() {
        // I_x(1,1) = x
        for &x in &[1e-10, 0.1, 0.5, 0.77, 0.999] {
            assert_rel("I(1,1)", betainc(x, 1.0, 1.0), x, 1e-14);
        }
        // symmetry I_x(a,b) = 1 - I_{1-x}(b,a)
        for &(x, a, b) in &[(0.3, 2.0, 5.0), (0.8, 0.5, 3.5), (0.5, 20.0, 30.0), (0.35, 200.0, 300.0), (0.01, 0.1, 40.0)] {
            let l = betainc(x, a, b);
            let r = 1.0 - betainc(1.0 - x, b, a);
            assert!((l - r).abs() < 1e-13, "symmetry {x} {a} {b}: {l} {r}");
        }
        // I_x(a,1) = x^a, I_x(1,b) = 1 - (1-x)^b
        assert_rel("I(a,1)", betainc(0.4, 3.5, 1.0), 0.4f64.powf(3.5), 1e-14);
        assert_rel("I(1,b)", betainc(0.4, 1.0, 3.5), 1.0 - 0.6f64.powf(3.5), 1e-14);
        // I_x(2,2) = 3x^2 - 2x^3
        assert_rel("I(2,2)", betainc(0.3, 2.0, 2.0), 3.0 * 0.09 - 2.0 * 0.027, 1e-14);
        assert_eq!(betainc(0.0, 2.0, 3.0), 0.0);
        assert_eq!(betainc(1.0, 2.0, 3.0), 1.0);
        assert!(betainc(-0.1, 2.0, 3.0).is_nan() && betainc(1.1, 2.0, 3.0).is_nan());
        assert!(betainc(0.5, 0.0, 3.0).is_nan() && betainc(0.5, 2.0, -3.0).is_nan());
        // Student-t: I_{n/(n+t^2)}(n/2, 1/2) / 2 = P(T > t); n = 1: Cauchy 1/2 - atan(t)/pi
        let t = 2.0f64;
        assert_rel("cauchy", 0.5 * betainc(1.0 / (1.0 + t * t), 0.5, 0.5), 0.5 - t.atan() / PI, 1e-13);
    }

    #[test]
    fn beta_values() {
        assert_rel("B(2,3)", beta(2.0, 3.0), 1.0 / 12.0, 1e-15);
        assert_rel("B(.5,.5)", beta(0.5, 0.5), PI, 1e-15);
        assert_rel("B(1,n)", beta(1.0, 7.0), 1.0 / 7.0, 1e-15);
                // B(2, b) = 1 / (b (b + 1)), through the large-argument branch
        assert_rel("B(2,1e6)", beta(2.0, 1e6), 1.0 / (1e6 * (1e6 + 1.0)), 1e-13);
        assert_rel("B(3,5e4)", beta(3.0, 5e4), 2.0 / (5e4 * (5e4 + 1.0) * (5e4 + 2.0)), 1e-13);
        // I_x(2, b) = 1 - (1-x)^b (1 + b x)
        let (x, b) = (1e-5f64, 1e6f64);
        assert_rel("I(2,1e6)", betainc(x, 2.0, b), 1.0 - (b * (-x).ln_1p()).exp() * (1.0 + b * x), 1e-13);
        assert!(beta(-1.0, 2.0).is_nan() && beta(0.0, 2.0).is_nan());
        assert_eq!(beta(1.0, f64::INFINITY), 0.0);
    }

    #[test]
    fn bessel_published_values() {
        assert_rel("J0(1)", bessel_j(0.0, 1.0), 0.765_197_686_557_966_6, 1e-14);
        assert_rel("J1(1)", bessel_j(1.0, 1.0), 0.440_050_585_744_933_5, 1e-14);
        assert_rel("Y0(1)", bessel_y(0.0, 1.0), 0.088_256_964_215_676_97, 1e-14);
        assert_rel("Y1(1)", bessel_y(1.0, 1.0), -0.781_212_821_300_288_7, 1e-14);
        assert_rel("I0(1)", bessel_i(0.0, 1.0), 1.266_065_877_752_008_2, 1e-14);
        assert_rel("I1(1)", bessel_i(1.0, 1.0), 0.565_159_103_992_485_1, 1e-14);
        assert_rel("K0(1)", bessel_k(0.0, 1.0), 0.421_024_438_240_708_34, 1e-14);
        assert_rel("K1(1)", bessel_k(1.0, 1.0), 0.601_907_230_197_234_6, 1e-14);
        assert_rel("J0(10)", bessel_j(0.0, 10.0), -0.245_935_764_451_348_3, 1e-13);
        assert_rel("I0(10)", bessel_i(0.0, 10.0), 2815.716_628_466_254, 1e-14);
        assert_rel("K0(10)", bessel_k(0.0, 10.0), 1.778_006_231_616_765e-5, 1e-13);
    }

    #[test]
    fn bessel_half_integer_closed_forms() {
        for &x in &[0.01, 0.5, 1.7, 3.0, 12.0, 24.9, 25.1, 80.0, 400.0] {
            let c = (2.0 / (PI * x)).sqrt();
            assert!((bessel_j(0.5, x) - c * x.sin()).abs() < 2e-14 * c, "J.5({x})");
            assert!((bessel_y(0.5, x) + c * x.cos()).abs() < 2e-14 * c, "Y.5({x})");
            assert!((bessel_j(1.5, x) - c * (x.sin() / x - x.cos())).abs() < 2e-14 * c.max(c / x), "J1.5({x})");
            assert_rel("I.5", bessel_i(0.5, x), c * x.sinh(), 2e-14);
            assert_rel("K.5", bessel_k(0.5, x), (PI / (2.0 * x)).sqrt() * (-x).exp(), 2e-14);
            assert_rel("K1.5", bessel_k(1.5, x), (PI / (2.0 * x)).sqrt() * (-x).exp() * (1.0 + 1.0 / x), 2e-14);
        }
    }

    #[test]
    fn bessel_wronskians_and_recurrences() {
        for &nu in &[0.0, 0.3, 1.0, 2.5, 7.0, 20.0, 60.7] {
            for &x in &[0.05, 0.9, 2.0, 5.0, 24.0, 26.0, 150.0, 5000.0, 1.0e6] {
                let w = bessel_j(nu + 1.0, x) * bessel_y(nu, x) - bessel_j(nu, x) * bessel_y(nu + 1.0, x);
                if w.is_finite() {
                    assert_rel(&format!("Wronskian nu={nu} x={x}"), w, 2.0 / (PI * x), 5e-12);
                }
                let wk = bessel_i(nu, x) * bessel_k(nu + 1.0, x) + bessel_i(nu + 1.0, x) * bessel_k(nu, x);
                if wk.is_finite() && wk != 0.0 && x < 700.0 {
                    assert_rel(&format!("IK Wronskian nu={nu} x={x}"), wk, 1.0 / x, 5e-12);
                }
                // J_{nu-1} + J_{nu+1} = 2 nu/x J_nu
                if nu >= 1.0 {
                    let l = bessel_j(nu - 1.0, x) + bessel_j(nu + 1.0, x);
                    let r = 2.0 * nu / x * bessel_j(nu, x);
                    let scale = bessel_j(nu - 1.0, x).abs().max(bessel_j(nu + 1.0, x).abs()).max(1e-300);
                    assert!((l - r).abs() <= 5e-13 * scale, "J recurrence nu={nu} x={x}: {l} {r}");
                }
            }
        }
    }

    #[test]
    fn bessel_edges() {
        assert_eq!(bessel_j(0.0, 0.0), 1.0);
        assert_eq!(bessel_j(2.0, 0.0), 0.0);
        assert_eq!(bessel_y(0.0, 0.0), f64::NEG_INFINITY);
        assert_eq!(bessel_i(0.0, 0.0), 1.0);
        assert_eq!(bessel_i(1.5, 0.0), 0.0);
        assert_eq!(bessel_k(0.0, 0.0), f64::INFINITY);
        // negative argument: parity for integer order, NaN for fractional or Y/K
        assert_rel("J1(-1)", bessel_j(1.0, -1.0), -bessel_j(1.0, 1.0), 1e-15);
        assert!(bessel_j(0.5, -1.0).is_nan() && bessel_y(0.0, -1.0).is_nan() && bessel_k(0.0, -1.0).is_nan());
        assert!(bessel_j(f64::NAN, 1.0).is_nan() && bessel_j(1.0, f64::NAN).is_nan());
        assert_eq!(bessel_j(0.0, f64::INFINITY), 0.0);
        assert_eq!(bessel_k(1.0, f64::INFINITY), 0.0);
        assert_eq!(bessel_i(1.0, f64::INFINITY), f64::INFINITY);
        // tiny x: J_nu ~ (x/2)^nu / Gamma(nu+1); Y_0 ~ (2/pi)(ln(x/2) + gamma)
        assert_rel("J2(1e-8)", bessel_j(2.0, 1e-8), 1.25e-17, 1e-7);
        assert_rel("Y0(1e-10)", bessel_y(0.0, 1e-10), 2.0 / PI * ((0.5e-10f64).ln() + EULER_GAMMA), 1e-9);
        // huge x stays finite and tiny; huge order underflows to zero, not NaN
        assert!(bessel_j(0.0, 1e12).abs() < 1e-5);
        assert_eq!(bessel_j(500.0, 1.0), 0.0);
        assert!(bessel_i(0.0, 800.0).is_infinite());
        assert_eq!(bessel_k(0.0, 800.0), 0.0);
        // negative order: J_{-n} = (-1)^n J_n; J_{-1/2}(x) = sqrt(2/(pi x)) cos x
        assert_rel("J-2", bessel_j(-2.0, 3.0), bessel_j(2.0, 3.0), 1e-15);
        let x = 2.3;
        assert_rel("J-.5", bessel_j(-0.5, x), (2.0 / (PI * x)).sqrt() * x.cos(), 1e-13);
        assert_rel("I-.5", bessel_i(-0.5, x), (2.0 / (PI * x)).sqrt() * x.cosh(), 1e-13);
        assert_rel("K-1.5", bessel_k(-1.5, x), bessel_k(1.5, x), 1e-15);
    }

    #[test]
    fn hankel_and_cf_agree_at_the_switch() {
        // (the CF branch is reached below x = 25)
        // evaluate just above and below x = 25 against each other through the
        // Wronskian-free route: both must continue the same smooth function.
        for &nu in &[0.0, 0.5, 1.0, 3.3] {
            let (jh, yh) = {
                let (p, q, _, _) = hankel_sums(nu, 25.0).unwrap();
                let r = phase_reduce(25.0, 0.5 * nu + 0.25);
                let (s, c) = r.sin_cos();
                let amp = (2.0 / (PI * 25.0)).sqrt();
                (amp * (p * c - q * s), amp * (p * s + q * c))
            };
            // CF path: x just under 25 would use CF; call the CF code by x = 24.999999999
            let eps = 1e-9;
            let (jc, yc) = bessel_jy(nu, 25.0 - eps).unwrap();
            // derivative bound: |J'| <= 1 so the shift moves values by at most ~eps
            assert!((jh - jc).abs() < 3.0 * eps + 1e-13, "J nu={nu}: {jh} {jc}");
            assert!((yh - yc).abs() < 3.0 * eps + 1e-13, "Y nu={nu}: {yh} {yc}");
        }
    }

    #[test]
    fn expint_values() {
        // E1(1) = 0.21938393439552029, E1(0.5) = 0.5597735947761608, E1(2) = 0.04890051070806112
        assert_rel("E1(1)", expint(1.0), 0.219_383_934_395_520_29, 1e-14);
        assert_rel("E1(.5)", expint(0.5), 0.559_773_594_776_160_8, 1e-14);
        assert_rel("E1(2)", expint(2.0), 0.048_900_510_708_061_12, 1e-14);
        assert_rel("E1(10)", expint(10.0), 4.156_968_929_685_324e-6, 1e-13);
        assert_eq!(expint(0.0), f64::INFINITY);
        assert!(expint(-1.0).is_nan());
        assert_eq!(expint(f64::INFINITY), 0.0);
        // E1(x) ~ -gamma - ln x for small x
        assert_rel("E1(1e-10)", expint(1e-10), -EULER_GAMMA - (1e-10f64).ln() + 1e-10, 1e-14);
        // derivative: E1'(x) = -e^-x / x, central difference
        let (x, h) = (3.0, 1e-5);
        let d = (expint(x + h) - expint(x - h)) / (2.0 * h);
        assert!((d + (-x).exp() / x).abs() < 1e-9);
    }

    #[test]
    fn ellipke_values() {
        let (k, e) = ellipke(0.0);
        assert_rel("K(0)", k, PI / 2.0, 1e-15);
        assert_rel("E(0)", e, PI / 2.0, 1e-15);
        let (k, e) = ellipke(0.5);
        assert_rel("K(.5)", k, 1.854_074_677_301_371_9, 1e-15);
        assert_rel("E(.5)", e, 1.350_643_881_047_675_5, 2e-14);
        let (k, e) = ellipke(1.0);
        assert!(k.is_infinite());
        assert_eq!(e, 1.0);
        assert!(ellipke(1.5).0.is_nan());
        // Legendre's relation: E K' + E' K - K K' = pi/2, with K' = K(1-m)
        for &m in &[0.1, 0.3, 0.7, 0.95] {
            let (k, e) = ellipke(m);
            let (kp, ep) = ellipke(1.0 - m);
            assert!((e * kp + ep * k - k * kp - PI / 2.0).abs() < 1e-14, "Legendre m={m}");
        }
        // negative parameter: K(-1) = 1.3110287771461 (= K(1/2)/sqrt 2)
        assert_rel("K(-1)", ellipke(-1.0).0, 1.854_074_677_301_371_9 / 2f64.sqrt(), 1e-15);
        // near 1: K ~ ln(4/sqrt(1-m))
        let m = 1.0 - 1e-12;
        assert_rel("K(1-1e-12)", ellipke(m).0, (4.0 / (1.0 - m).sqrt()).ln(), 1e-10);
    }

    #[test]
    fn legendre_closed_forms() {
        let x = 0.5f64;
        let s = (1.0 - x * x).sqrt();
        let p = legendre(2, x).unwrap();
        assert_rel("P20", p[0], 0.5 * (3.0 * x * x - 1.0), 1e-15);
        assert_rel("P21", p[1], -3.0 * x * s, 1e-15);
        assert_rel("P22", p[2], 3.0 * (1.0 - x * x), 1e-15);
        let p = legendre(3, x).unwrap();
        assert_rel("P30", p[0], 0.5 * (5.0 * x.powi(3) - 3.0 * x), 1e-15);
        assert_rel("P31", p[1], -1.5 * (5.0 * x * x - 1.0) * s, 1e-14);
        assert_rel("P32", p[2], 15.0 * x * (1.0 - x * x), 1e-14);
        assert_rel("P33", p[3], -15.0 * s.powi(3), 1e-14);
        assert_eq!(legendre(0, 0.3).unwrap(), vec![1.0]);
        // endpoints: P_n(1) = 1, P_n^m(1) = 0 for m > 0
        let p = legendre(5, 1.0).unwrap();
        assert_eq!(p[0], 1.0);
        assert!(p[1..].iter().all(|v| *v == 0.0));
        let p = legendre(4, -1.0).unwrap();
        assert_eq!(p[0], 1.0); // (-1)^4
        assert!(legendre(2, 1.5).is_err());
        assert!(legendre(2, f64::NAN).unwrap()[0].is_nan());
    }

    #[test]
    fn zeta_values() {
        assert_rel("zeta(2)", zeta(2.0).unwrap(), PI * PI / 6.0, 1e-15);
        assert_rel("zeta(4)", zeta(4.0).unwrap(), PI.powi(4) / 90.0, 1e-15);
        assert_rel("zeta(6)", zeta(6.0).unwrap(), PI.powi(6) / 945.0, 1e-15);
        assert_rel("zeta(3)", zeta(3.0).unwrap(), 1.202_056_903_159_594_3, 1e-15);
        assert_rel("zeta(.5)", zeta(0.5).unwrap(), -1.460_354_508_809_586_8, 1e-14);
        assert_rel("zeta(0)", zeta(0.0).unwrap(), -0.5, 1e-15);
        assert_rel("zeta(-1)", zeta(-1.0).unwrap(), -1.0 / 12.0, 1e-14);
        assert_rel("zeta(-3)", zeta(-3.0).unwrap(), 1.0 / 120.0, 1e-14);
        assert_eq!(zeta(-2.0).unwrap(), 0.0);
        assert_eq!(zeta(-10.0).unwrap(), 0.0);
        assert_rel("zeta(1.0001)", zeta(1.0001).unwrap(), 1.0 / 0.0001 + EULER_GAMMA, 2e-4);
        assert_rel("zeta(50)", zeta(50.0).unwrap(), 1.0 + 2f64.powf(-50.0), 1e-15);
        assert_eq!(zeta(f64::INFINITY).unwrap(), 1.0);
        assert!(zeta(1.0).is_err());
        assert!(zeta(f64::NAN).unwrap().is_nan());
    }

    // ---- reference values computed with mpmath (40 digits), not with this code ----
    const JYIK: &[(f64, f64, f64, f64, f64, f64)] = &[
        (0.0, 0.1, 0.99750156206604, -1.5342386513503667, 1.0025015629340956, 2.4270690247020164),
        (0.0, 3.7, -0.39923020337119114, 0.1060743153203541, 8.738617524169397, 0.01563065992162666),
        (0.0, 30.0, -0.08636798358104021, -0.11729573168666403, 781672297823.9775, 2.1324774964630563e-14),
        (0.0, 100.0, 0.019985850304223122, -0.07724431336508315, 1.0737517071310738e+42, 4.656628229175902e-45),
        (0.0, 1000000.0, 0.00033104301373987376, -0.0007259685223351791, f64::INFINITY, 0.0),
        (1.0, 0.001, 0.0004999999375000026, -636.6221672311394, 0.0005000000625000026, 999.9962381560856),
        (1.0, 2.0, 0.5767248077568734, -0.10703243154093754, 1.590636854637329, 0.13986588181652243),
        (2.0, 50.5, -0.0978194114079707, 0.05519934870663757, 4.6222146134088326e+20, 2.1404705859735832e-23),
        (5.0, 0.5, 8.053627241357474e-06, -7946.301478807473, 8.223171313109265e-06, 12097.979476096394),
        (5.0, 5.0, 0.26114054612017007, -0.4536948224911019, 2.1579745473225467, 0.03270627371203186),
        (5.0, 20.0, 0.15116976798239498, -0.10003576788953243, 23018392.21341367, 1.0538660139974233e-09),
        (5.0, 200.0, -0.055132678944014676, 0.012019640832200107, 1.915814101523687e+85, 1.3045247397975134e-88),
        (20.0, 3.0, 1.2275946737992987e-15, -13113540041757.447, 1.5209660019426694e-15, 16254643952204.365),
        (20.0, 20.0, 0.16474777377532654, -0.28548945860020347, 3188.750328853615, 5.543111636125816e-06),
        (20.0, 100.0, 0.062217458498338755, 0.051247973076188426, 1.4483461256427172e+41, 3.38520541489017e-44),
        (50.0, 30.0, 2.0581656631564178e-08, -386759.32602734736, 0.00014590106916468948, 58.77068625800724),
        (100.0, 50.0, 1.1159273690838094e-21, -3.2938001882026665e+18, 2.7278879470966917e-16, 16394035276269.252),
        (100.0, 1000.0, 0.011676135007802554, -0.022438688257723275, f64::INFINITY, 0.0),
        (0.3, 1.2, 0.7111886464520507, -0.08022932823167017, 1.2421327157959323, 0.32769323123536503),
        (2.5, 7.0, -0.2834366512016992, 0.12852374780895653, 104.61336757234871, 0.0006435411544813076),
        (12.7, 0.6, 7.942880407027284e-17, -315905105165984.4, 8.047928015397373e-17, 488646807001704.8),
        (7.5, 100000.0, -0.00252154491487329, 8.94929258534912e-05, f64::INFINITY, 0.0),
    ];
    const GAMMAINC: &[(f64, f64, f64, f64)] = &[
        (0.5, 0.1, 0.9414024458901336, 0.05859755410986648),
        (3.0, 0.001, 0.9999869268601987, 1.3073139801213742e-05),
        (1.0, 3.5, 0.040159631269898445, 0.9598403687301016),
        (10.0, 3.5, 0.9944303169270544, 0.005569683072945571),
        (100.0, 100.0, 0.5132987982791487, 0.48670120172085135),
        (90.0, 100.0, 0.15822098918643016, 0.8417790108135699),
        (100.0, 90.0, 0.8536538253012672, 0.1463461746987328),
        (1000.0, 1000.0, 0.5042052441802155, 0.4957947558197845),
        (100000.0, 100000.0, 0.5004205221103651, 0.4995794778896348),
        (30.0, 2.5, 0.9999999999878454, 1.215456977718304e-11),
        (0.01, 20.0, 4.071357979530877e-59, 1.0),
        (5.0, 0.5, 0.9984345977419975, 0.0015654022580025497),
    ];
    const BETAINC: &[(f64, f64, f64, f64)] = &[
        (0.2, 0.5, 0.5, 0.2951672353008666),
        (0.5, 2.0, 3.0, 0.6875),
        (0.9, 5.0, 0.7, 0.43587540861971585),
        (0.3, 30.0, 40.0, 0.012126160623329713),
        (0.5, 100.0, 100.0, 0.5),
        (0.4, 1000.0, 1500.0, 0.5010858488742157),
        (0.001, 0.2, 5.0, 0.3711099209733869),
        (0.97, 3.0, 70.0, 1.0),
    ];
    const BETA: &[(f64, f64, f64)] = &[
        (100.0, 100.0, 2.2087606931995024e-61),
        (0.001, 0.002, 1499.995075998483),
        (1e-05, 3.0, 99998.5000174998),
        (2.5, 170.0, 3.4893341834754894e-06),
        (200.0, 300.0, 1.6485491608664747e-147),
        (0.5, 1000.0, 0.056056918840616005),
    ];
    const ZETA: &[(f64, f64)] = &[
        (1.5, 2.612375348685488),
        (2.5, 1.341487257250917),
        (10.0, 1.000994575127818),
        (-0.5, -0.20788622497735457),
        (-1.5, -0.025485201889833036),
        (-7.0, 0.004166666666666667),
        (-20.5, -108.21747505877606),
        (-100.3, -4.713075229471476e+77),
        (1.01, 100.57794333849678),
        (0.1, -0.6030375198562418),
        (0.9, -9.430114019402255),
        (30.5, 1.0000000006585472),
        (3.0, 1.2020569031595942),
    ];
    const EXPINT: &[(f64, f64)] = &[
        (1e-05, 10.935719800043696),
        (0.3, 0.9056766516758468),
        (1.0, 0.21938393439552029),
        (1.5, 0.10001958240663265),
        (5.0, 0.0011482955912753257),
        (20.0, 9.835525290649882e-11),
        (50.0, 3.783264029550459e-24),
        (300.0, 1.71038427680451e-133),
    ];
    const ERFINV: &[(f64, f64)] = &[
        (1e-300, 8.86226925452758e-301),
        (1e-10, 8.862269254527581e-11),
        (0.1, 0.08885599049425769),
        (0.4, 0.37080715859355795),
        (0.6, 0.5951160814499948),
        (0.9, 1.1630871536766743),
        (0.99, 1.8213863677184494),
        (0.999999999, 4.320005388105362),
    ];
    const ELLIPKE: &[(f64, f64, f64)] = &[
        (-100.0, 0.368219248609141, 10.209260919814572),
        (-3.0, 1.0782578237498217, 2.422112055136919),
        (0.2, 1.659623598610528, 1.489035058095853),
        (0.9, 2.5780921133481733, 1.1047747327040733),
        (0.999999, 8.294051463601063, 1.0000038970261722),
        (0.5, 1.8540746773013719, 1.3506438810476755),
    ];

    const LEG6_03: &[f64] = &[0.1291811875, -1.928625388040708, -4.212560624999999, 82.4441716339795, -3.912772500000029, -2463.481068824283, 7833.370545];
    const LEG40_09: &[f64] = &[0.036987418421876216, 7.506933008812437, -91.65912002788629, -11539.346674098184, 292725.6171493195, 13950834.301198097, -762264179.8203347, -3574300433.522749, 1321418254738.7363, -37992504080461.52, -659979154188036.6, 8.614210968423317e+16, -2.903178245544926e+18, 1.396120843206408e+19, 3.558833030898796e+21, -2.2610141935357307e+23, 8.916098989362137e+24, -2.7256011006223928e+26, 6.936827350726132e+27, -1.5202403049464424e+29, 2.923831045243681e+30, -4.992794576166815e+31, 7.626350128414116e+32, -1.0469226466728427e+34, 1.2951927842738616e+35, -1.4458260584669795e+36, 1.4562811249091155e+37, -1.3219120012433126e+38, 1.0788792656008211e+39, -7.888965841765928e+39, 5.143117565428989e+40, -2.9702159137976538e+41, 1.5067811953335646e+42, -6.641293482763761e+42, 2.5068409534826586e+43, -7.947556901319012e+43, 2.0597168299117774e+44, -4.193216118520371e+44, 6.292391957246805e+44, -6.191851838562789e+44, 2.998850715296857e+44];

    #[test]
    fn bessel_against_mpmath() {
        let mut worst: f64 = 0.0;
        for &(nu, x, j, y, i, k) in JYIK {
            for (name, got, want) in [
                ("J", bessel_j(nu, x), j),
                ("Y", bessel_y(nu, x), y),
                ("I", bessel_i(nu, x), i),
                ("K", bessel_k(nu, x), k),
            ] {
                if want.is_infinite() || want == 0.0 {
                    assert_eq!(got, want, "{name}({nu},{x})");
                    continue;
                }
                // accuracy is relative to the size of the function's envelope, which
                // is what matters near a zero of J/Y
                let r = rel(got, want);
                worst = worst.max(r);
                if r > 1e-13 {
                    eprintln!("  {name}_{nu}({x}): rel {r:e}");
                }
                assert!(r < 1e-11, "{name}_{nu}({x}): got {got:e}, want {want:e}, rel {r:e}");
            }
        }
        eprintln!("worst relative error vs mpmath (Bessel table): {worst:e}");
    }

    #[test]
    fn gammainc_betainc_beta_against_mpmath() {
        let mut worst: f64 = 0.0;
        for &(x, a, p, q) in GAMMAINC {
            let (gp, gq) = (gammainc(x, a, Tail::Lower), gammainc(x, a, Tail::Upper));
            worst = worst.max(rel(gp, p)).max(rel(gq, q));
            if rel(gp, p).max(rel(gq, q)) > 1e-13 {
                eprintln!("  gammainc({x},{a}): {:e} {:e}", rel(gp, p), rel(gq, q));
            }
            assert!(rel(gp, p) < 1e-12, "P({a},{x}): {gp:e} vs {p:e}");
            assert!(rel(gq, q) < 1e-12, "Q({a},{x}): {gq:e} vs {q:e}");
        }
        for &(x, a, b, v) in BETAINC {
            let g = betainc(x, a, b);
            worst = worst.max(rel(g, v));
            if rel(g, v) > 1e-13 {
                eprintln!("  betainc({x},{a},{b}): {:e}", rel(g, v));
            }
            assert!(rel(g, v) < 1e-12, "I_{x}({a},{b}): {g:e} vs {v:e}");
        }
        for &(a, b, v) in BETA {
            let g = beta(a, b);
            worst = worst.max(rel(g, v));
            assert!(rel(g, v) < 1e-12, "B({a},{b}): {g:e} vs {v:e}");
        }
        eprintln!("worst relative error vs mpmath (gammainc/betainc/beta): {worst:e}");
    }

    #[test]
    fn zeta_expint_erfinv_ellipke_against_mpmath() {
        let (mut wz, mut we, mut wi, mut wk) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for &(s, v) in ZETA {
            wz = wz.max(rel(zeta(s).unwrap(), v));
        }
        for &(x, v) in EXPINT {
            we = we.max(rel(expint(x), v));
        }
        for &(y, v) in ERFINV {
            wi = wi.max(rel(erfinv(y), v));
        }
        for &(m, k, e) in ELLIPKE {
            let (gk, ge) = ellipke(m);
            wk = wk.max(rel(gk, k)).max(rel(ge, e));
        }
        eprintln!("worst rel vs mpmath: zeta {wz:e} expint {we:e} erfinv {wi:e} ellipke {wk:e}");
        assert!(wz < 1e-13 && we < 1e-13 && wi < 1e-14 && wk < 1e-14);
    }

    #[test]
    fn legendre_against_mpmath() {
        let mut worst: f64 = 0.0;
        for (n, x, want) in [(6usize, 0.3, LEG6_03), (40, 0.9, LEG40_09)] {
            let got = legendre(n, x).unwrap();
            for (m, (g, w)) in got.iter().zip(want).enumerate() {
                worst = worst.max(rel(*g, *w));
                assert!(rel(*g, *w) < 1e-12, "P_{n}^{m}({x}): {g:e} vs {w:e}");
            }
        }
        eprintln!("worst rel vs mpmath: legendre {worst:e}");
    }
}
