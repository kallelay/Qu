//! Polynomial helpers on coefficient vectors, highest power first (the same
//! convention as `polyfit`/`polyval`/`conv`): [`roots`], [`poly_from_roots`],
//! [`deconv`], [`polyder`], [`polyint`].
//!
//! `roots` is the eigenvalues of the companion matrix, the textbook method
//! (and MATLAB's). Eigenvalue iterations can fail to converge on some
//! companion matrices (a known open issue in `linalg::eig` for certain even
//! polynomials), and a result can also come back with a poor residual, so
//! every eigenvalue answer is CHECKED against the polynomial itself
//! (backward error) and, if it fails the check or the solver errors, the
//! roots are recomputed with the Aberth-Ehrlich simultaneous iteration and
//! polished with Newton steps. Nothing is returned unverified.
//!
//! Errors are plain messages (`Result<_, String>`).

use crate::linalg;
use crate::matrix::Matrix;
use crate::Complex64;

/// Strip leading (highest-power) zeros. Returns the remaining slice, which is
/// empty for the zero polynomial.
fn strip_leading_zeros(p: &[f64]) -> &[f64] {
    let first = p.iter().position(|&c| c != 0.0).unwrap_or(p.len());
    &p[first..]
}

fn check_finite(p: &[f64], who: &str) -> Result<(), String> {
    if p.iter().any(|c| !c.is_finite()) {
        return Err(format!("{who}: coefficients must be finite numbers"));
    }
    Ok(())
}

/// Which method produced a set of roots (exposed for the tests, which must
/// exercise the fallback directly rather than hope an input triggers it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootsMethod {
    Companion,
    Aberth,
}

/// Roots of `p` (highest power first). Leading zeros are dropped; trailing
/// zeros become exact zero roots (appended last). A constant (or the zero
/// polynomial) has no roots and gives an empty vector.
pub fn roots(p: &[f64]) -> Result<Vec<Complex64>, String> {
    roots_with_method(p).map(|(r, _)| r)
}

/// As [`roots`], also reporting which method produced the answer.
pub fn roots_with_method(p: &[f64]) -> Result<(Vec<Complex64>, RootsMethod), String> {
    if p.is_empty() {
        return Err("roots: the coefficient vector is empty".to_string());
    }
    check_finite(p, "roots")?;
    let p = strip_leading_zeros(p);
    if p.len() <= 1 {
        return Ok((Vec::new(), RootsMethod::Companion));
    }
    let trailing = p.iter().rev().take_while(|&&c| c == 0.0).count();
    let core = &p[..p.len() - trailing];
    let mut out: Vec<Complex64>;
    let mut method = RootsMethod::Companion;
    if core.len() == 1 {
        out = Vec::new();
    } else if core.len() == 2 {
        out = vec![Complex64::real(-core[1] / core[0])];
    } else {
        out = match companion_roots(core) {
            Some(r) if residuals_ok(core, &r) => r,
            _ => {
                method = RootsMethod::Aberth;
                let r = aberth_roots(core);
                if !residuals_ok(core, &r) {
                    return Err(format!(
                        "roots: could not find the roots of this degree-{} polynomial to a verified accuracy",
                        core.len() - 1
                    ));
                }
                r
            }
        };
    }
    out.extend(std::iter::repeat(Complex64::real(0.0)).take(trailing));
    Ok((out, method))
}

/// Eigenvalues of the companion matrix of `core` (leading coeff non-zero,
/// degree >= 2). `None` if the eigen-solver fails or returns non-finite values.
fn companion_roots(core: &[f64]) -> Option<Vec<Complex64>> {
    let n = core.len() - 1;
    // first row: -a_i / a_0 ; sub-diagonal ones. Column-major layout.
    let mut d = vec![0.0; n * n];
    for j in 0..n {
        d[j * n] = -core[j + 1] / core[0];
    }
    for i in 1..n {
        d[(i - 1) * n + i] = 1.0;
    }
    let m = Matrix::from_col_major(n, n, d);
    let res = linalg::eig(&m).ok()?;
    if res.values_re.len() != n
        || res.values_re.iter().chain(res.values_im.iter()).any(|v| !v.is_finite())
    {
        return None;
    }
    Some(
        res.values_re
            .iter()
            .zip(res.values_im.iter())
            .map(|(&re, &im)| Complex64::new(re, im))
            .collect(),
    )
}

/// Horner evaluation with a running bound: returns `(p(z), sum |a_i| |z|^i)`.
fn horner_with_scale(p: &[f64], z: Complex64) -> (Complex64, f64) {
    let mut v = Complex64::real(p[0]);
    let mut s = p[0].abs();
    let az = z.magnitude();
    for &c in &p[1..] {
        v = v.mul(z).add(Complex64::real(c));
        s = s * az + c.abs();
    }
    (v, s)
}

/// Backward-error test: every root must satisfy `|p(r)| <= tol * sum|a_i||r|^i`,
/// the standard measure under which a multiple root still passes. Also
/// requires that all roots are finite.
fn residuals_ok(p: &[f64], r: &[Complex64]) -> bool {
    let n = p.len() - 1;
    if r.len() != n {
        return false;
    }
    r.iter().all(|&z| {
        if !(z.re.is_finite() && z.im.is_finite()) {
            return false;
        }
        let (v, s) = horner_with_scale(p, z);
        v.magnitude() <= 1e-8 * s.max(f64::MIN_POSITIVE)
    })
}

/// Aberth-Ehrlich simultaneous root iteration for a real polynomial with a
/// non-zero leading and trailing coefficient, polished by Newton steps.
fn aberth_roots(p: &[f64]) -> Vec<Complex64> {
    let n = p.len() - 1;
    // Monic copy, scaled so the Cauchy bound is of order one.
    let lead = p[0];
    let a: Vec<f64> = p.iter().map(|c| c / lead).collect();
    let bound = 1.0 + a[1..].iter().fold(0.0f64, |m, c| m.max(c.abs()));
    // Starting points on a circle of radius from the geometric-mean root
    // magnitude, offset by an irrational angle so conjugate symmetry is not
    // reproduced exactly (that symmetry is what stalls naive starts).
    let r0 = a[n].abs().powf(1.0 / n as f64).clamp(1e-3 * bound, bound);
    let mut z: Vec<Complex64> = (0..n)
        .map(|k| {
            Complex64::from_polar(
                r0,
                2.0 * std::f64::consts::PI * k as f64 / n as f64 + 0.4,
            )
        })
        .collect();
    let der: Vec<f64> = (0..n).map(|i| a[i] * (n - i) as f64).collect();
    for _ in 0..1000 {
        let mut worst = 0.0f64;
        for k in 0..n {
            let (pv, _) = horner_with_scale(&a, z[k]);
            if pv.magnitude() == 0.0 {
                continue;
            }
            let (dv, _) = horner_with_scale(&der, z[k]);
            let ratio = pv.div(dv);
            let mut sum = Complex64::real(0.0);
            for j in 0..n {
                if j != k {
                    let d = z[k].sub(z[j]);
                    if d.magnitude() > 0.0 {
                        sum = sum.add(Complex64::real(1.0).div(d));
                    }
                }
            }
            let denom = Complex64::real(1.0).sub(ratio.mul(sum));
            let step = if denom.magnitude() > 0.0 { ratio.div(denom) } else { ratio };
            if !(step.re.is_finite() && step.im.is_finite()) {
                continue;
            }
            z[k] = z[k].sub(step);
            worst = worst.max(step.magnitude() / z[k].magnitude().max(1e-300));
        }
        if worst < 1e-15 {
            break;
        }
    }
    // Newton polish against the ORIGINAL coefficients; keep a step only if it
    // does not increase the backward error (so a cluster is never made worse).
    let dp: Vec<f64> = (0..n).map(|i| p[i] * (n - i) as f64).collect();
    for zk in z.iter_mut() {
        for _ in 0..3 {
            let (v, s) = horner_with_scale(p, *zk);
            let (d, _) = horner_with_scale(&dp, *zk);
            if d.magnitude() == 0.0 {
                break;
            }
            let cand = zk.sub(v.div(d));
            let (vc, sc) = horner_with_scale(p, cand);
            if vc.magnitude() / sc.max(f64::MIN_POSITIVE) < v.magnitude() / s.max(f64::MIN_POSITIVE) {
                *zk = cand;
            } else {
                break;
            }
        }
        // Tiny imaginary parts of a real root are noise from the iteration.
        if zk.im != 0.0 {
            let (v, s) = horner_with_scale(p, Complex64::real(zk.re));
            if v.magnitude() <= 1e-12 * s && zk.im.abs() <= 1e-9 * zk.re.abs().max(1e-300) {
                zk.im = 0.0;
            }
        }
    }
    z
}

/// Coefficients (highest power first, leading `1`) of the monic polynomial
/// with the given roots, as complex numbers. `[]` gives `[1]`.
pub fn poly_complex(r: &[Complex64]) -> Vec<Complex64> {
    let mut c = vec![Complex64::real(1.0)];
    for &root in r {
        let mut next = vec![Complex64::real(0.0); c.len() + 1];
        for (i, &ci) in c.iter().enumerate() {
            next[i] = next[i].add(ci);
            next[i + 1] = next[i + 1].sub(ci.mul(root));
        }
        c = next;
    }
    c
}

/// Is the multiset of roots closed under conjugation (to a tolerance relative
/// to each root's size)? If so, the product polynomial is real.
pub fn is_conjugate_closed(r: &[Complex64]) -> bool {
    let mut used = vec![false; r.len()];
    for i in 0..r.len() {
        if used[i] {
            continue;
        }
        if r[i].im == 0.0 {
            used[i] = true;
            continue;
        }
        let target = r[i].conj();
        let tol = 1e-8 * (1.0 + r[i].magnitude());
        let mut found = None;
        for j in (i + 1)..r.len() {
            if !used[j] && r[j].sub(target).magnitude() <= tol {
                found = Some(j);
                break;
            }
        }
        match found {
            Some(j) => {
                used[i] = true;
                used[j] = true;
            }
            None => return false,
        }
    }
    true
}

/// `deconv(b, a)`: polynomial long division `b = conv(a, q) + r`. `r` has the
/// same length as `b`; `q` has `len(b) - len(a) + 1` coefficients (a single
/// `0` when `a` is longer than `b`, in which case `r = b`). Leading zeros of
/// the divisor are ignored; a zero divisor is an error.
pub fn deconv(b: &[f64], a: &[f64]) -> Result<(Vec<f64>, Vec<f64>), String> {
    if b.is_empty() || a.is_empty() {
        return Err("deconv: both polynomials must be non-empty".to_string());
    }
    check_finite(b, "deconv")?;
    check_finite(a, "deconv")?;
    let a = strip_leading_zeros(a);
    if a.is_empty() {
        return Err("deconv: the divisor is the zero polynomial".to_string());
    }
    if b.len() < a.len() {
        return Ok((vec![0.0], b.to_vec()));
    }
    let nq = b.len() - a.len() + 1;
    let mut r = b.to_vec();
    let mut q = vec![0.0; nq];
    for i in 0..nq {
        let c = r[i] / a[0];
        q[i] = c;
        for (j, &aj) in a.iter().enumerate() {
            r[i + j] -= c * aj;
        }
        r[i] = 0.0;
    }
    Ok((q, r))
}

/// Derivative of a polynomial; leading zeros of the result are stripped, a
/// constant gives `[0]`.
pub fn polyder(p: &[f64]) -> Result<Vec<f64>, String> {
    if p.is_empty() {
        return Err("polyder: the coefficient vector is empty".to_string());
    }
    check_finite(p, "polyder")?;
    let p = strip_leading_zeros(p);
    if p.len() <= 1 {
        return Ok(vec![0.0]);
    }
    let n = p.len() - 1;
    Ok((0..n).map(|i| p[i] * (n - i) as f64).collect())
}

/// Antiderivative with constant of integration `k`: length `len(p) + 1`.
pub fn polyint(p: &[f64], k: f64) -> Result<Vec<f64>, String> {
    if p.is_empty() {
        return Err("polyint: the coefficient vector is empty".to_string());
    }
    check_finite(p, "polyint")?;
    if !k.is_finite() {
        return Err("polyint: the constant of integration must be finite".to_string());
    }
    let n = p.len();
    let mut out: Vec<f64> = (0..n).map(|i| p[i] / (n - i) as f64).collect();
    out.push(k);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_re(r: &[Complex64]) -> Vec<f64> {
        let mut v: Vec<f64> = r.iter().map(|z| z.re).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    }

    #[test]
    fn cubic_with_integer_roots() {
        let r = roots(&[1.0, -6.0, 11.0, -6.0]).unwrap();
        let s = sorted_re(&r);
        for (got, want) in s.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-10, "{s:?}");
        }
        assert!(r.iter().all(|z| z.im.abs() < 1e-10));
    }

    #[test]
    fn leading_and_trailing_zeros() {
        // x^2 (x - 1)(x - 2), with two leading zeros
        let r = roots(&[0.0, 0.0, 1.0, -3.0, 2.0, 0.0, 0.0]).unwrap();
        assert_eq!(r.len(), 4);
        let s = sorted_re(&r);
        for (got, want) in s.iter().zip([0.0, 0.0, 1.0, 2.0]) {
            assert!((got - want).abs() < 1e-10, "{s:?}");
        }
        // the zero roots are exact
        assert_eq!(r.iter().filter(|z| z.re == 0.0 && z.im == 0.0).count(), 2);
    }

    #[test]
    fn degrees_one_and_zero() {
        let r = roots(&[2.0, -6.0]).unwrap();
        assert_eq!(r.len(), 1);
        assert!((r[0].re - 3.0).abs() < 1e-15);
        assert!(roots(&[5.0]).unwrap().is_empty());
        assert!(roots(&[0.0, 0.0, 5.0]).unwrap().is_empty());
        assert!(roots(&[0.0, 0.0]).unwrap().is_empty());
        assert!(roots(&[]).is_err());
        assert!(roots(&[1.0, f64::NAN]).is_err());
    }

    #[test]
    fn repeated_roots_within_expected_sensitivity() {
        // (x - 1)^3: perturbation of size eps moves roots by ~eps^(1/3)
        let r = roots(&[1.0, -3.0, 3.0, -1.0]).unwrap();
        assert_eq!(r.len(), 3);
        for z in &r {
            assert!(z.sub(Complex64::real(1.0)).magnitude() < 1e-4, "{r:?}");
        }
    }

    #[test]
    fn even_polynomials_x4_plus_1() {
        let (r, how) = roots_with_method(&[1.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
        eprintln!("x^4+1 solved by {how:?}");
        assert_eq!(r.len(), 4);
        for z in &r {
            assert!((z.magnitude() - 1.0).abs() < 1e-10, "{r:?}");
            assert!((z.re.abs() - 0.5f64.sqrt()).abs() < 1e-10);
        }
    }

    #[test]
    fn aberth_fallback_is_correct_on_its_own() {
        for p in [
            vec![1.0, 0.0, 0.0, 0.0, 1.0],
            vec![1.0, -6.0, 11.0, -6.0],
            vec![1.0, 0.0, 1.0],
            vec![2.0, -3.0, 0.0, 5.0, 1.0, -7.0],
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0],
        ] {
            let r = aberth_roots(&p);
            assert!(residuals_ok(&p, &r), "aberth failed for {p:?}: {r:?}");
        }
        let r = aberth_roots(&[1.0, -6.0, 11.0, -6.0]);
        let s = sorted_re(&r);
        for (got, want) in s.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-10, "{s:?}");
        }
    }

    #[test]
    fn even_polynomial_sweep_never_returns_unverified_roots() {
        // x^(2m) + c for several m, c and a few mixed even polynomials.
        let mut cases: Vec<Vec<f64>> = Vec::new();
        for m in 1..=8usize {
            for c in [1.0, -1.0, 4.0, 0.01, 100.0] {
                let mut p = vec![0.0; 2 * m + 1];
                p[0] = 1.0;
                p[2 * m] = c;
                cases.push(p);
            }
        }
        cases.push(vec![1.0, 0.0, 3.0, 0.0, 2.0]);
        cases.push(vec![1.0, 0.0, -5.0, 0.0, 4.0]);
        cases.push(vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
        for p in cases {
            let (r, how) = roots_with_method(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
            if how == RootsMethod::Aberth {
                eprintln!("FALLBACK used (companion eig failed or inaccurate) for {p:?}");
            }
            assert!(residuals_ok(&p, &r), "{p:?}");
        }
    }

    #[test]
    fn poly_round_trip() {
        let p = [1.0, -6.0, 11.0, -6.0];
        let r = roots(&p).unwrap();
        assert!(is_conjugate_closed(&r));
        let c = poly_complex(&r);
        for (got, want) in c.iter().zip(p) {
            assert!((got.re - want).abs() < 1e-9 && got.im.abs() < 1e-9);
        }
        assert!(is_conjugate_closed(&[Complex64::new(0.0, 1.0), Complex64::new(0.0, -1.0)]));
        assert!(!is_conjugate_closed(&[Complex64::new(0.0, 1.0)]));
        assert!(!is_conjugate_closed(&[Complex64::new(0.0, 1.0), Complex64::new(0.0, 1.0)]));
    }

    #[test]
    fn deconv_known_division() {
        // (x^3 + 2x^2 + 3x + 4) / (x + 1) = x^2 + x + 2, remainder 2
        let (q, r) = deconv(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0]).unwrap();
        assert_eq!(q, vec![1.0, 1.0, 2.0]);
        assert_eq!(r, vec![0.0, 0.0, 0.0, 2.0]);
        // divisor longer than dividend
        let (q, r) = deconv(&[1.0, 2.0], &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(q, vec![0.0]);
        assert_eq!(r, vec![1.0, 2.0]);
        // leading zeros on the divisor
        let (q, _) = deconv(&[1.0, 2.0, 3.0, 4.0], &[0.0, 1.0, 1.0]).unwrap();
        assert_eq!(q, vec![1.0, 1.0, 2.0]);
        assert!(deconv(&[1.0], &[0.0, 0.0]).is_err());
    }

    #[test]
    fn derivative_and_integral() {
        assert_eq!(polyder(&[3.0, 2.0, 1.0]).unwrap(), vec![6.0, 2.0]);
        assert_eq!(polyder(&[0.0, 0.0, 4.0, 1.0]).unwrap(), vec![4.0]);
        assert_eq!(polyder(&[7.0]).unwrap(), vec![0.0]);
        assert_eq!(polyint(&[6.0, 2.0], 1.0).unwrap(), vec![3.0, 2.0, 1.0]);
        assert_eq!(polyint(&[3.0], 0.0).unwrap(), vec![3.0, 0.0]);
    }
}
