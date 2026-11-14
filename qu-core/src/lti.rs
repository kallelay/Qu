//! Linear time-invariant systems, v0.4.13.
//!
//! A system is a state-space realization
//!
//! ```text
//! continuous (dt = 0):  x' = A x + B u        discrete (dt > 0):  x[k+1] = A x[k] + B u[k]
//!                       y  = C x + D u                            y[k]   = C x[k] + D u[k]
//! ```
//!
//! plus, when it was built from a transfer function or from zeros, poles and
//! a gain, that original description ([`Form`]). Keeping the original matters:
//! the poles of `1/(s^3+3s^2+2s)` are then the roots of the polynomial that
//! was typed, not the eigenvalues of a realization of it, and a frequency
//! response is the polynomial ratio evaluated directly.
//!
//! What is exact and what is not:
//!
//! * Time responses of a continuous system ([`lsim`], [`step`], [`impulse`])
//!   use the matrix exponential of an augmented matrix, so the samples are
//!   the exact solution for an input that is constant between samples
//!   ([`Hold::Zoh`]) or linear between samples ([`Hold::Foh`]), up to the
//!   rounding of [`expm`]. No ODE solver, no step-size tolerance.
//! * [`expm`] is the degree-13 Pade approximant with scaling and squaring
//!   (Higham 2005).
//! * [`ss_to_tf`] uses the Faddeev-LeVerrier recursion. It is exact for small
//!   integer matrices and loses accuracy as the order grows (a few digits
//!   per ten states); a high-order system should stay in state space.
//! * [`logm`] (used by zero-order-hold [`d2c`]) takes repeated matrix square
//!   roots by the Denman-Beavers iteration and checks its own answer by
//!   exponentiating it; a matrix with no real logarithm is reported, not
//!   approximated.
//! * Continuous gain and phase margins are found from the roots of real
//!   polynomials in `w` (no frequency grid); discrete ones from a dense scan
//!   of `(0, pi]` refined by bisection.

use crate::matrix::Matrix;
use crate::Complex64;
use std::f64::consts::PI;
use std::ops::{Index, IndexMut};

// ------------------------------------------------------------------ Dense

/// A small dense real matrix, row-major. `Matrix` (column-major, fallible
/// accessors) is the interchange type; this is the working one.
#[derive(Clone, Debug, PartialEq)]
pub struct Dense {
    pub r: usize,
    pub c: usize,
    pub d: Vec<f64>,
}

impl Index<(usize, usize)> for Dense {
    type Output = f64;
    fn index(&self, (i, j): (usize, usize)) -> &f64 {
        &self.d[i * self.c + j]
    }
}

impl IndexMut<(usize, usize)> for Dense {
    fn index_mut(&mut self, (i, j): (usize, usize)) -> &mut f64 {
        &mut self.d[i * self.c + j]
    }
}

impl Dense {
    pub fn zeros(r: usize, c: usize) -> Self {
        Dense { r, c, d: vec![0.0; r * c] }
    }

    pub fn eye(n: usize) -> Self {
        let mut m = Dense::zeros(n, n);
        for i in 0..n {
            m[(i, i)] = 1.0;
        }
        m
    }

    /// From rows; every row must have the same length.
    pub fn from_rows(rows: &[&[f64]]) -> Self {
        let c = rows.first().map_or(0, |r| r.len());
        let mut d = Vec::with_capacity(rows.len() * c);
        for row in rows {
            assert_eq!(row.len(), c, "Dense::from_rows: ragged rows");
            d.extend_from_slice(row);
        }
        Dense { r: rows.len(), c, d }
    }

    pub fn from_matrix(m: &Matrix) -> Self {
        let (r, c) = m.shape();
        let s = m.as_slice();
        let mut out = Dense::zeros(r, c);
        for i in 0..r {
            for j in 0..c {
                out[(i, j)] = s[i + j * r];
            }
        }
        out
    }

    pub fn to_matrix(&self) -> Matrix {
        let mut data = vec![0.0; self.r * self.c];
        for i in 0..self.r {
            for j in 0..self.c {
                data[i + j * self.r] = self[(i, j)];
            }
        }
        Matrix::from_col_major(self.r, self.c, data)
    }

    pub fn row(&self, i: usize) -> &[f64] {
        &self.d[i * self.c..(i + 1) * self.c]
    }

    pub fn col(&self, j: usize) -> Vec<f64> {
        (0..self.r).map(|i| self[(i, j)]).collect()
    }

    pub fn mul(&self, o: &Dense) -> Dense {
        assert_eq!(self.c, o.r, "Dense::mul: {}x{} times {}x{}", self.r, self.c, o.r, o.c);
        let mut out = Dense::zeros(self.r, o.c);
        for i in 0..self.r {
            for k in 0..self.c {
                let a = self[(i, k)];
                if a == 0.0 {
                    continue;
                }
                for j in 0..o.c {
                    out[(i, j)] += a * o[(k, j)];
                }
            }
        }
        out
    }

    pub fn add(&self, o: &Dense) -> Dense {
        assert_eq!((self.r, self.c), (o.r, o.c));
        Dense { r: self.r, c: self.c, d: self.d.iter().zip(&o.d).map(|(a, b)| a + b).collect() }
    }

    pub fn sub(&self, o: &Dense) -> Dense {
        assert_eq!((self.r, self.c), (o.r, o.c));
        Dense { r: self.r, c: self.c, d: self.d.iter().zip(&o.d).map(|(a, b)| a - b).collect() }
    }

    pub fn scale(&self, k: f64) -> Dense {
        Dense { r: self.r, c: self.c, d: self.d.iter().map(|a| a * k).collect() }
    }

    /// Largest absolute column sum.
    pub fn norm1(&self) -> f64 {
        (0..self.c).map(|j| (0..self.r).map(|i| self[(i, j)].abs()).sum::<f64>()).fold(0.0, f64::max)
    }

    fn max_abs(&self) -> f64 {
        self.d.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    fn all_finite(&self) -> bool {
        self.d.iter().all(|v| v.is_finite())
    }

    /// Copy `src` into this matrix with its top-left corner at `(r0, c0)`.
    pub fn put(&mut self, r0: usize, c0: usize, src: &Dense) {
        for i in 0..src.r {
            for j in 0..src.c {
                self[(r0 + i, c0 + j)] = src[(i, j)];
            }
        }
    }

    /// The `r x c` block whose top-left corner is `(r0, c0)`.
    pub fn block(&self, r0: usize, c0: usize, r: usize, c: usize) -> Dense {
        let mut out = Dense::zeros(r, c);
        for i in 0..r {
            for j in 0..c {
                out[(i, j)] = self[(r0 + i, c0 + j)];
            }
        }
        out
    }

    /// `[[a, b], [c, d]]`.
    fn blocks(a: &Dense, b: &Dense, c: &Dense, d: &Dense) -> Dense {
        let mut out = Dense::zeros(a.r + c.r, a.c + b.c);
        out.put(0, 0, a);
        out.put(0, a.c, b);
        out.put(a.r, 0, c);
        out.put(a.r, a.c, d);
        out
    }

    fn vstack(a: &Dense, b: &Dense) -> Dense {
        let mut out = Dense::zeros(a.r + b.r, a.c.max(b.c));
        out.put(0, 0, a);
        out.put(a.r, 0, b);
        out
    }

    fn hstack(a: &Dense, b: &Dense) -> Dense {
        let mut out = Dense::zeros(a.r.max(b.r), a.c + b.c);
        out.put(0, 0, a);
        out.put(0, a.c, b);
        out
    }
}

/// Solve `A X = B` by LU with partial pivoting. `None` when `A` is singular
/// to working precision (a pivot below `n * eps * max|A|`).
pub fn solve(a: &Dense, b: &Dense) -> Option<Dense> {
    let n = a.r;
    assert_eq!(a.c, n, "solve: A must be square");
    assert_eq!(b.r, n, "solve: row mismatch");
    let mut lu = a.clone();
    let mut x = b.clone();
    let tiny = (n.max(1) as f64) * f64::EPSILON * a.max_abs();
    for k in 0..n {
        let mut piv = k;
        for i in k + 1..n {
            if lu[(i, k)].abs() > lu[(piv, k)].abs() {
                piv = i;
            }
        }
        let p = lu[(piv, k)];
        if !(p.abs() > tiny) {
            return None;
        }
        if piv != k {
            for j in 0..n {
                lu.d.swap(k * n + j, piv * n + j);
            }
            for j in 0..x.c {
                x.d.swap(k * x.c + j, piv * x.c + j);
            }
        }
        for i in k + 1..n {
            let f = lu[(i, k)] / p;
            if f == 0.0 {
                continue;
            }
            for j in k + 1..n {
                lu[(i, j)] -= f * lu[(k, j)];
            }
            for j in 0..x.c {
                x[(i, j)] -= f * x[(k, j)];
            }
        }
    }
    for k in (0..n).rev() {
        for j in 0..x.c {
            let mut s = x[(k, j)];
            for q in k + 1..n {
                s -= lu[(k, q)] * x[(q, j)];
            }
            x[(k, j)] = s / lu[(k, k)];
        }
    }
    Some(x)
}

/// `X A = B`, i.e. `B A^-1`.
fn solve_right(a: &Dense, b: &Dense) -> Option<Dense> {
    solve(&transpose(a), &transpose(b)).map(|x| transpose(&x))
}

fn transpose(a: &Dense) -> Dense {
    let mut out = Dense::zeros(a.c, a.r);
    for i in 0..a.r {
        for j in 0..a.c {
            out[(j, i)] = a[(i, j)];
        }
    }
    out
}

/// Complex `A X = B` (row-major `n x n` and `n x m`), partial pivoting.
fn csolve(mut a: Vec<Complex64>, n: usize, mut b: Vec<Complex64>, m: usize) -> Option<Vec<Complex64>> {
    let scale = a.iter().fold(0.0f64, |s, v| s.max(v.magnitude()));
    let tiny = (n.max(1) as f64) * f64::EPSILON * scale;
    for k in 0..n {
        let mut piv = k;
        for i in k + 1..n {
            if a[i * n + k].magnitude() > a[piv * n + k].magnitude() {
                piv = i;
            }
        }
        let p = a[piv * n + k];
        if !(p.magnitude() > tiny) {
            return None;
        }
        if piv != k {
            for j in 0..n {
                a.swap(k * n + j, piv * n + j);
            }
            for j in 0..m {
                b.swap(k * m + j, piv * m + j);
            }
        }
        for i in k + 1..n {
            let f = a[i * n + k].div(p);
            for j in k + 1..n {
                a[i * n + j] = a[i * n + j].sub(f.mul(a[k * n + j]));
            }
            for j in 0..m {
                b[i * m + j] = b[i * m + j].sub(f.mul(b[k * m + j]));
            }
        }
    }
    for k in (0..n).rev() {
        for j in 0..m {
            let mut s = b[k * m + j];
            for q in k + 1..n {
                s = s.sub(a[k * n + q].mul(b[q * m + j]));
            }
            b[k * m + j] = s.div(a[k * n + k]);
        }
    }
    Some(b)
}

// ------------------------------------------------------- matrix functions

/// Matrix exponential: degree-13 Pade approximant with scaling and squaring
/// (Higham, "The scaling and squaring method for the matrix exponential
/// revisited", 2005).
pub fn expm(a: &Dense) -> Result<Dense, String> {
    if a.r != a.c {
        return Err(format!("expm: the matrix must be square, got {}x{}", a.r, a.c));
    }
    if !a.all_finite() {
        return Err("expm: the matrix has a NaN or infinite entry".into());
    }
    let n = a.r;
    if n == 0 {
        return Ok(a.clone());
    }
    const B: [f64; 14] = [
        64764752532480000.0,
        32382376266240000.0,
        7771770303897600.0,
        1187353796428800.0,
        129060195264000.0,
        10559470521600.0,
        670442572800.0,
        33522128640.0,
        1323241920.0,
        40840800.0,
        960960.0,
        16380.0,
        182.0,
        1.0,
    ];
    const THETA13: f64 = 5.371920351148152;
    let norm = a.norm1();
    let s = if norm > THETA13 { (norm / THETA13).log2().ceil().max(0.0) as i32 } else { 0 };
    let a = a.scale(0.5f64.powi(s));
    let id = Dense::eye(n);
    let a2 = a.mul(&a);
    let a4 = a2.mul(&a2);
    let a6 = a4.mul(&a2);
    let lin = |c6: f64, c4: f64, c2: f64, c0: f64| a6.scale(c6).add(&a4.scale(c4)).add(&a2.scale(c2)).add(&id.scale(c0));
    let u = a.mul(&a6.mul(&lin(B[13], B[11], B[9], 0.0)).add(&lin(B[7], B[5], B[3], B[1])));
    let v = a6.mul(&lin(B[12], B[10], B[8], 0.0)).add(&lin(B[6], B[4], B[2], B[0]));
    let mut r = solve(&v.sub(&u), &v.add(&u)).ok_or_else(|| "expm: the Pade denominator is singular".to_string())?;
    for _ in 0..s {
        r = r.mul(&r);
    }
    if !r.all_finite() {
        return Err("expm: the result overflows".into());
    }
    Ok(r)
}

/// Principal square root by the Denman-Beavers iteration.
fn sqrtm(m: &Dense) -> Option<Dense> {
    let n = m.r;
    let id = Dense::eye(n);
    let mut y = m.clone();
    let mut z = id.clone();
    for _ in 0..100 {
        let yi = solve(&y, &id)?;
        let zi = solve(&z, &id)?;
        let yn = y.add(&zi).scale(0.5);
        let zn = z.add(&yi).scale(0.5);
        let change = yn.sub(&y).norm1();
        y = yn;
        z = zn;
        if !y.all_finite() {
            return None;
        }
        if change <= 4.0 * f64::EPSILON * y.norm1() {
            return Some(y);
        }
    }
    None
}

/// Principal real matrix logarithm, by inverse scaling and squaring: square
/// roots until the matrix is near the identity, then
/// `log(X) = 2 atanh((X - I)(X + I)^-1)` as a series. The answer is checked
/// by exponentiating it, so a matrix without a real logarithm (an eigenvalue
/// on the closed negative real axis) is an error.
pub fn logm(m: &Dense) -> Result<Dense, String> {
    if m.r != m.c {
        return Err(format!("logm: the matrix must be square, got {}x{}", m.r, m.c));
    }
    let n = m.r;
    if n == 0 {
        return Ok(m.clone());
    }
    let fail = || "the matrix has no real logarithm (an eigenvalue is zero or on the negative real axis)".to_string();
    let id = Dense::eye(n);
    let mut x = m.clone();
    let mut k = 0;
    while x.sub(&id).norm1() > 0.25 {
        if k >= 60 {
            return Err(fail());
        }
        x = sqrtm(&x).ok_or_else(fail)?;
        k += 1;
    }
    let z = solve_right(&x.add(&id), &x.sub(&id)).ok_or_else(fail)?;
    let z2 = z.mul(&z);
    let mut term = z.clone();
    let mut sum = z.clone();
    for j in 1..200 {
        term = term.mul(&z2);
        sum = sum.add(&term.scale(1.0 / (2 * j + 1) as f64));
        if term.norm1() <= f64::EPSILON * sum.norm1() {
            break;
        }
    }
    let l = sum.scale(2.0 * 2f64.powi(k));
    let back = expm(&l).map_err(|_| fail())?;
    if !(back.sub(m).norm1() <= 1e-8 * m.norm1().max(1.0)) {
        return Err(fail());
    }
    Ok(l)
}

// ------------------------------------------------------------ polynomials

/// Drop leading exact zeros (coefficients are highest power first); an
/// all-zero polynomial becomes `[0]`.
fn strip_leading(c: &[f64]) -> Vec<f64> {
    let first = c.iter().position(|v| *v != 0.0).unwrap_or(c.len());
    if first == c.len() {
        vec![0.0]
    } else {
        c[first..].to_vec()
    }
}

/// Drop leading coefficients that are rounding noise next to the largest.
fn trim_leading(c: &[f64], rel: f64) -> Vec<f64> {
    let big = c.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let first = c.iter().position(|v| v.abs() > rel * big).unwrap_or(c.len());
    if first == c.len() {
        vec![0.0]
    } else {
        c[first..].to_vec()
    }
}

/// Horner evaluation at a complex point, coefficients highest power first.
pub fn polyval(c: &[f64], s: Complex64) -> Complex64 {
    c.iter().fold(Complex64::real(0.0), |acc, &k| acc.mul(s).add(Complex64::real(k)))
}

fn polymul(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

/// `a + b`, aligned at the constant term.
fn polyadd(a: &[f64], b: &[f64]) -> Vec<f64> {
    let n = a.len().max(b.len());
    let mut out = vec![0.0; n];
    for (i, v) in a.iter().enumerate() {
        out[n - a.len() + i] += v;
    }
    for (i, v) in b.iter().enumerate() {
        out[n - b.len() + i] += v;
    }
    out
}

/// All roots of a real polynomial (highest power first). Roots at the origin
/// are split off exactly; the rest are companion-matrix eigenvalues, or, for
/// the companion matrices the QR iteration does not converge on (polynomials
/// in `x^2` are the usual case), Durand-Kerner iterates.
pub fn poly_roots(c: &[f64]) -> Result<Vec<Complex64>, String> {
    let c = strip_leading(c);
    let mut end = c.len();
    while end > 1 && c[end - 1] == 0.0 {
        end -= 1;
    }
    let mut roots = vec![Complex64::real(0.0); c.len() - end];
    if end > 1 {
        match crate::filter::polynomial_roots(&c[..end]) {
            Ok(r) => roots.extend(r),
            Err(_) => roots.extend(durand_kerner(&c[..end])?),
        }
    }
    Ok(roots)
}

/// Simultaneous root iteration (Durand-Kerner / Weierstrass).
fn durand_kerner(c: &[f64]) -> Result<Vec<Complex64>, String> {
    let n = c.len() - 1;
    let monic: Vec<f64> = c.iter().map(|v| v / c[0]).collect();
    let radius = 1.0 + monic[1..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let seed = Complex64::new(0.4, 0.9);
    let mut z = Vec::with_capacity(n);
    let mut cur = Complex64::real(radius.min(2.0));
    for _ in 0..n {
        z.push(cur);
        cur = cur.mul(seed);
    }
    for _ in 0..5000 {
        let mut change = 0.0f64;
        for k in 0..n {
            let mut denom = Complex64::real(1.0);
            for j in 0..n {
                if j != k {
                    denom = denom.mul(z[k].sub(z[j]));
                }
            }
            if denom.magnitude() == 0.0 {
                continue;
            }
            let delta = polyval(&monic, z[k]).div(denom);
            z[k] = z[k].sub(delta);
            change = change.max(delta.magnitude());
        }
        if !z.iter().all(|v| v.re.is_finite() && v.im.is_finite()) {
            break;
        }
        if change <= 4.0 * f64::EPSILON * radius {
            return Ok(z);
        }
    }
    Err("could not find the roots of the polynomial".into())
}

/// The monic real polynomial with the given roots. The roots must be real or
/// come in conjugate pairs.
pub fn poly_from_roots(roots: &[Complex64], what: &str) -> Result<Vec<f64>, String> {
    let mut c = vec![Complex64::real(1.0)];
    for r in roots {
        if !(r.re.is_finite() && r.im.is_finite()) {
            return Err(format!("{what} must be finite"));
        }
        let mut next = vec![Complex64::real(0.0); c.len() + 1];
        for (i, v) in c.iter().enumerate() {
            next[i] = next[i].add(*v);
            next[i + 1] = next[i + 1].sub(v.mul(*r));
        }
        c = next;
    }
    let big = c.iter().fold(0.0f64, |m, v| m.max(v.magnitude()));
    if c.iter().any(|v| v.im.abs() > 1e-9 * big) {
        return Err(format!("complex {what} must come in conjugate pairs (the polynomial would not be real)"));
    }
    Ok(c.iter().map(|v| v.re).collect())
}

// ------------------------------------------------------------ state space

/// A state-space realization. `dt == 0` is continuous time, `dt > 0` a
/// discrete system with that sample time.
#[derive(Clone, Debug, PartialEq)]
pub struct Ss {
    pub a: Dense,
    pub b: Dense,
    pub c: Dense,
    pub d: Dense,
    pub dt: f64,
}

fn check_dt(dt: f64) -> Result<(), String> {
    if dt.is_finite() && dt >= 0.0 {
        Ok(())
    } else {
        Err(format!("the sample time dt must be 0 (continuous) or a positive number, got {dt}"))
    }
}

impl Ss {
    pub fn new(a: Dense, b: Dense, c: Dense, d: Dense, dt: f64) -> Result<Ss, String> {
        if a.r != a.c {
            return Err(format!("A must be square, got {}x{}", a.r, a.c));
        }
        let n = a.r;
        if b.r != n {
            return Err(format!("B must have {n} row(s), one per state, got {}x{}", b.r, b.c));
        }
        if c.c != n {
            return Err(format!("C must have {n} column(s), one per state, got {}x{}", c.r, c.c));
        }
        if d.r != c.r || d.c != b.c {
            return Err(format!(
                "D must be {}x{} (outputs x inputs, from C's rows and B's columns), got {}x{}",
                c.r, b.c, d.r, d.c
            ));
        }
        if b.c == 0 || c.r == 0 {
            return Err("a system needs at least one input and one output".into());
        }
        if !(a.all_finite() && b.all_finite() && c.all_finite() && d.all_finite()) {
            return Err("A, B, C and D must not contain NaN or infinite entries".into());
        }
        check_dt(dt)?;
        Ok(Ss { a, b, c, d, dt })
    }

    /// A static gain (no states).
    pub fn gain(d: Dense, dt: f64) -> Ss {
        Ss { a: Dense::zeros(0, 0), b: Dense::zeros(0, d.c), c: Dense::zeros(d.r, 0), d, dt }
    }

    pub fn n(&self) -> usize {
        self.a.r
    }
    pub fn inputs(&self) -> usize {
        self.b.c
    }
    pub fn outputs(&self) -> usize {
        self.c.r
    }
    pub fn is_discrete(&self) -> bool {
        self.dt > 0.0
    }
    pub fn is_siso(&self) -> bool {
        self.inputs() == 1 && self.outputs() == 1
    }

    /// Eigenvalues of `A`.
    pub fn poles(&self) -> Result<Vec<Complex64>, String> {
        if self.n() == 0 {
            return Ok(Vec::new());
        }
        let r = crate::linalg::eig(&self.a.to_matrix()).map_err(|err| err.to_string())?;
        Ok(r.values_re.iter().zip(&r.values_im).map(|(&re, &im)| Complex64::new(re, im)).collect())
    }

    /// `D + C (sI - A)^-1 B` at one complex point, row-major `outputs x
    /// inputs`; `None` when `s` is a pole.
    pub fn eval(&self, s: Complex64) -> Option<Vec<Complex64>> {
        let (n, m, p) = (self.n(), self.inputs(), self.outputs());
        let mut out: Vec<Complex64> = self.d.d.iter().map(|&v| Complex64::real(v)).collect();
        if n == 0 {
            return Some(out);
        }
        let mut lhs = vec![Complex64::real(0.0); n * n];
        for i in 0..n {
            for j in 0..n {
                lhs[i * n + j] = Complex64::real(-self.a[(i, j)]);
            }
            lhs[i * n + i] = lhs[i * n + i].add(s);
        }
        let rhs: Vec<Complex64> = self.b.d.iter().map(|&v| Complex64::real(v)).collect();
        let x = csolve(lhs, n, rhs, m)?;
        for i in 0..p {
            for j in 0..m {
                let mut acc = out[i * m + j];
                for q in 0..n {
                    acc = acc.add(x[q * m + j].scale(self.c[(i, q)]));
                }
                out[i * m + j] = acc;
            }
        }
        Some(out)
    }
}

/// Controllable canonical realization of `num(s)/den(s)` (coefficients
/// highest power first). The numerator degree must not exceed the
/// denominator's.
pub fn tf_to_ss(num: &[f64], den: &[f64], dt: f64) -> Result<Ss, String> {
    check_dt(dt)?;
    if num.is_empty() || den.is_empty() {
        return Err("the numerator and the denominator each need at least one coefficient".into());
    }
    if num.iter().chain(den).any(|v| !v.is_finite()) {
        return Err("the coefficients must not contain NaN or infinite values".into());
    }
    let den = strip_leading(den);
    if den == [0.0] {
        return Err("the denominator is zero".into());
    }
    let num = strip_leading(num);
    if num.len() > den.len() {
        return Err(format!(
            "improper transfer function: the numerator has degree {} and the denominator degree {}. \
             A state-space model needs numerator degree <= denominator degree",
            num.len() - 1,
            den.len() - 1
        ));
    }
    let n = den.len() - 1;
    let a0 = den[0];
    let a: Vec<f64> = den.iter().map(|v| v / a0).collect();
    let mut b = vec![0.0; n + 1];
    for (i, v) in num.iter().enumerate() {
        b[n + 1 - num.len() + i] = v / a0;
    }
    let mut am = Dense::zeros(n, n);
    let mut bm = Dense::zeros(n, 1);
    let mut cm = Dense::zeros(1, n);
    for i in 0..n {
        if i + 1 < n {
            am[(i, i + 1)] = 1.0;
        }
        am[(n - 1, i)] = -a[n - i];
        cm[(0, i)] = b[n - i] - a[n - i] * b[0];
    }
    if n > 0 {
        bm[(n - 1, 0)] = 1.0;
    }
    Ok(Ss { a: am, b: bm, c: cm, d: Dense::from_rows(&[&[b[0]]]), dt })
}

/// Transfer function of a single-input single-output realization, by the
/// Faddeev-LeVerrier recursion. The denominator is monic, of degree `n`;
/// leading numerator coefficients that are rounding noise are dropped.
pub fn ss_to_tf(ss: &Ss) -> Result<(Vec<f64>, Vec<f64>), String> {
    if !ss.is_siso() {
        return Err(format!(
            "a transfer function needs one input and one output; this system has {} input(s) and {} output(s)",
            ss.inputs(),
            ss.outputs()
        ));
    }
    let n = ss.n();
    let d = ss.d[(0, 0)];
    let mut den = vec![1.0];
    let mut num = vec![d];
    let mut m = Dense::eye(n);
    for k in 1..=n {
        let cmb = ss.c.mul(&m).mul(&ss.b)[(0, 0)];
        let am = ss.a.mul(&m);
        let ck = -(0..n).map(|i| am[(i, i)]).sum::<f64>() / k as f64;
        den.push(ck);
        num.push(d * ck + cmb);
        m = am;
        for i in 0..n {
            m[(i, i)] += ck;
        }
    }
    let num = trim_leading(&num, 64.0 * f64::EPSILON * n.max(1) as f64);
    Ok((num, den))
}

// ------------------------------------------------------------ Lti = Ss + form

/// How a system was described when it was built.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Ss,
    /// `num(s)/den(s)`, highest power first.
    Tf { num: Vec<f64>, den: Vec<f64> },
    /// `k * prod(s - z_i) / prod(s - p_i)`.
    Zpk { z: Vec<Complex64>, p: Vec<Complex64>, k: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lti {
    pub ss: Ss,
    pub form: Form,
}

impl Lti {
    pub fn from_ss(ss: Ss) -> Lti {
        Lti { ss, form: Form::Ss }
    }

    pub fn from_tf(num: &[f64], den: &[f64], dt: f64) -> Result<Lti, String> {
        let ss = tf_to_ss(num, den, dt)?;
        Ok(Lti { ss, form: Form::Tf { num: strip_leading(num), den: strip_leading(den) } })
    }

    pub fn from_zpk(z: &[Complex64], p: &[Complex64], k: f64, dt: f64) -> Result<Lti, String> {
        if !k.is_finite() {
            return Err("the gain k must be a finite number".into());
        }
        if z.len() > p.len() {
            return Err(format!(
                "improper transfer function: {} zero(s) but only {} pole(s). \
                 A state-space model needs no more zeros than poles",
                z.len(),
                p.len()
            ));
        }
        let num: Vec<f64> = poly_from_roots(z, "zeros")?.iter().map(|v| v * k).collect();
        let den = poly_from_roots(p, "poles")?;
        let ss = tf_to_ss(&num, &den, dt)?;
        Ok(Lti { ss, form: Form::Zpk { z: z.to_vec(), p: p.to_vec(), k } })
    }

    /// Numerator and denominator (single-input single-output only).
    pub fn tf(&self) -> Result<(Vec<f64>, Vec<f64>), String> {
        match &self.form {
            Form::Tf { num, den } => Ok((num.clone(), den.clone())),
            Form::Zpk { z, p, k } => {
                Ok((poly_from_roots(z, "zeros")?.iter().map(|v| v * k).collect(), poly_from_roots(p, "poles")?))
            }
            Form::Ss => ss_to_tf(&self.ss),
        }
    }

    /// Zeros, poles and gain (single-input single-output only).
    pub fn zpk(&self) -> Result<(Vec<Complex64>, Vec<Complex64>, f64), String> {
        if let Form::Zpk { z, p, k } = &self.form {
            return Ok((z.clone(), p.clone(), *k));
        }
        let (num, den) = self.tf()?;
        let (num, den) = (strip_leading(&num), strip_leading(&den));
        let p = match &self.form {
            Form::Ss => self.ss.poles()?,
            _ => poly_roots(&den)?,
        };
        Ok((poly_roots(&num)?, p, num[0] / den[0]))
    }

    pub fn poles(&self) -> Result<Vec<Complex64>, String> {
        match &self.form {
            Form::Zpk { p, .. } => Ok(p.clone()),
            Form::Tf { den, .. } => poly_roots(den),
            Form::Ss => self.ss.poles(),
        }
    }

    /// Zeros of the transfer function (single-input single-output only).
    pub fn zeros(&self) -> Result<Vec<Complex64>, String> {
        if !self.ss.is_siso() {
            return Err(format!(
                "zeros are computed from the transfer function, which needs one input and one output; \
                 this system has {} input(s) and {} output(s)",
                self.ss.inputs(),
                self.ss.outputs()
            ));
        }
        match &self.form {
            Form::Zpk { z, .. } => Ok(z.clone()),
            _ => {
                let (num, _) = self.tf()?;
                if num.iter().all(|v| *v == 0.0) {
                    return Ok(Vec::new());
                }
                poly_roots(&num)
            }
        }
    }

    pub fn is_stable(&self) -> Result<bool, String> {
        let discrete = self.ss.is_discrete();
        Ok(self.poles()?.iter().all(|p| if discrete { p.magnitude() < 1.0 } else { p.re < 0.0 }))
    }

    /// The single-input single-output transfer function at one complex
    /// point. At a pole the value is infinite.
    pub fn at(&self, s: Complex64) -> Result<Complex64, String> {
        if !self.ss.is_siso() {
            return Err(format!(
                "a frequency response needs one input and one output; this system has {} input(s) and {} output(s)",
                self.ss.inputs(),
                self.ss.outputs()
            ));
        }
        let ratio = |n: Complex64, d: Complex64| {
            if d.magnitude() == 0.0 {
                if n.magnitude() == 0.0 {
                    Complex64::new(f64::NAN, f64::NAN)
                } else {
                    Complex64::new(f64::INFINITY, 0.0)
                }
            } else {
                n.div(d)
            }
        };
        Ok(match &self.form {
            Form::Tf { num, den } => ratio(polyval(num, s), polyval(den, s)),
            Form::Zpk { z, p, k } => {
                let n = z.iter().fold(Complex64::real(*k), |acc, r| acc.mul(s.sub(*r)));
                let d = p.iter().fold(Complex64::real(1.0), |acc, r| acc.mul(s.sub(*r)));
                ratio(n, d)
            }
            Form::Ss => match self.ss.eval(s) {
                Some(v) => v[0],
                None => Complex64::new(f64::INFINITY, 0.0),
            },
        })
    }

    /// The point on the stability boundary for frequency `w`: `jw`
    /// (continuous, rad/s) or `e^{jw}` (discrete, rad/sample).
    pub fn boundary(&self, w: f64) -> Complex64 {
        if self.ss.is_discrete() {
            Complex64::from_polar(1.0, w)
        } else {
            Complex64::new(0.0, w)
        }
    }

    /// Frequency response at each `w` (rad/s continuous, rad/sample discrete).
    pub fn freqresp(&self, w: &[f64]) -> Result<Vec<Complex64>, String> {
        w.iter().map(|&wk| self.at(self.boundary(wk))).collect()
    }

    /// Steady-state gain: `H(0)` continuous, `H(1)` discrete; `outputs x
    /// inputs`. A single-input single-output system with a pole there has
    /// gain `+-inf` (`NaN` if a zero cancels it).
    pub fn dcgain(&self) -> Result<Dense, String> {
        let ss = &self.ss;
        let n = ss.n();
        let lhs = if ss.is_discrete() { Dense::eye(n).sub(&ss.a) } else { ss.a.scale(-1.0) };
        if let Some(x) = solve(&lhs, &ss.b) {
            return Ok(ss.d.add(&ss.c.mul(&x)));
        }
        if !ss.is_siso() {
            return Err(format!(
                "the system has a pole at {}, so its steady-state gain is infinite",
                if ss.is_discrete() { "z = 1" } else { "s = 0" }
            ));
        }
        let s0 = Complex64::real(if ss.is_discrete() { 1.0 } else { 0.0 });
        let (num, den) = self.tf()?;
        let nv = polyval(&num, s0).re;
        let scale = num.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let _ = den;
        let g = if nv.abs() <= 1e-12 * scale { f64::NAN } else { f64::INFINITY.copysign(nv) };
        Ok(Dense::from_rows(&[&[g]]))
    }
}

// ------------------------------------------------------------- simulation

/// What the input does between samples of a continuous simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// Constant between samples (zero-order hold).
    Zoh,
    /// Linear between samples (first-order hold).
    Foh,
}

/// A time response: `t`, outputs `y` (`len(t) x outputs`) and states `x`
/// (`len(t) x states`).
#[derive(Clone, Debug)]
pub struct Sim {
    pub t: Vec<f64>,
    pub y: Dense,
    pub x: Dense,
}

/// One step of length `h`: `x1 = ad x0 + b0 u0 + b1 u1`.
struct StepMats {
    ad: Dense,
    b0: Dense,
    b1: Dense,
}

fn step_mats(ss: &Ss, h: f64, hold: Hold) -> Result<StepMats, String> {
    let (n, m) = (ss.n(), ss.inputs());
    match hold {
        Hold::Zoh => {
            // exp([[A, B], [0, 0]] h) = [[Ad, Bd], [0, I]]
            let mut aug = Dense::zeros(n + m, n + m);
            aug.put(0, 0, &ss.a.scale(h));
            aug.put(0, n, &ss.b.scale(h));
            let e = expm(&aug)?;
            Ok(StepMats { ad: e.block(0, 0, n, n), b0: e.block(0, n, n, m), b1: Dense::zeros(n, m) })
        }
        Hold::Foh => {
            // exp([[A h, B h, 0], [0, 0, I], [0, 0, 0]]) = [[Ad, G1, G2], ...]
            // with x1 = Ad x0 + (G1 - G2) u0 + G2 u1.
            let mut aug = Dense::zeros(n + 2 * m, n + 2 * m);
            aug.put(0, 0, &ss.a.scale(h));
            aug.put(0, n, &ss.b.scale(h));
            aug.put(n, n + m, &Dense::eye(m));
            let e = expm(&aug)?;
            let g1 = e.block(0, n, n, m);
            let g2 = e.block(0, n + m, n, m);
            Ok(StepMats { ad: e.block(0, 0, n, n), b0: g1.sub(&g2), b1: g2 })
        }
    }
}

fn check_times(ss: &Ss, t: &[f64]) -> Result<(), String> {
    if t.is_empty() {
        return Err("the time vector is empty".into());
    }
    if t.iter().any(|v| !v.is_finite()) {
        return Err("the time vector has a NaN or infinite entry".into());
    }
    for k in 1..t.len() {
        let h = t[k] - t[k - 1];
        if !(h > 0.0) {
            return Err(format!("the time vector must be strictly increasing (t[{}] = {}, t[{k}] = {})", k - 1, t[k - 1], t[k]));
        }
        if ss.is_discrete() && (h - ss.dt).abs() > 1e-6 * ss.dt {
            return Err(format!(
                "a discrete system is simulated at its own sample time dt = {}: the time vector must step by dt, \
                 but t[{k}] - t[{}] = {h}",
                ss.dt,
                k - 1
            ));
        }
    }
    Ok(())
}

/// Response to an input `u` (`len(t) x inputs`) sampled at `t`, from initial
/// state `x0` (zero when `None`).
///
/// Continuous: exact for an input that is constant ([`Hold::Zoh`]) or linear
/// ([`Hold::Foh`]) between samples; `t` may be unevenly spaced. Discrete: the
/// difference equation, `t` must step by `dt` and `hold` is not used.
pub fn lsim(ss: &Ss, u: &Dense, t: &[f64], x0: Option<&[f64]>, hold: Hold) -> Result<Sim, String> {
    let (n, m, p) = (ss.n(), ss.inputs(), ss.outputs());
    check_times(ss, t)?;
    let len = t.len();
    if u.r != len || u.c != m {
        return Err(format!(
            "the input must have one row per time sample and one column per input ({len}x{m}), got {}x{}",
            u.r, u.c
        ));
    }
    if !u.all_finite() {
        return Err("the input has a NaN or infinite entry".into());
    }
    let mut x = match x0 {
        Some(v) => {
            if v.len() != n {
                return Err(format!("the initial state x0 must have {n} element(s), one per state, got {}", v.len()));
            }
            if v.iter().any(|q| !q.is_finite()) {
                return Err("the initial state x0 has a NaN or infinite entry".into());
            }
            v.to_vec()
        }
        None => vec![0.0; n],
    };
    let mut xs = Dense::zeros(len, n);
    let mut ys = Dense::zeros(len, p);
    let discrete = ss.is_discrete();
    let mut mats: Option<(f64, StepMats)> = None;
    for k in 0..len {
        xs.d[k * n..(k + 1) * n].copy_from_slice(&x);
        let uk = u.row(k);
        for i in 0..p {
            let mut acc = 0.0;
            for j in 0..n {
                acc += ss.c[(i, j)] * x[j];
            }
            for j in 0..m {
                acc += ss.d[(i, j)] * uk[j];
            }
            ys[(k, i)] = acc;
        }
        if k + 1 == len {
            break;
        }
        let mut next = vec![0.0; n];
        if discrete {
            for i in 0..n {
                let mut acc = 0.0;
                for j in 0..n {
                    acc += ss.a[(i, j)] * x[j];
                }
                for j in 0..m {
                    acc += ss.b[(i, j)] * uk[j];
                }
                next[i] = acc;
            }
        } else {
            let h = t[k + 1] - t[k];
            // Step sizes that differ only by the rounding of the time
            // stamps themselves share one set of matrices.
            let same = |h0: f64| (h - h0).abs() <= 8.0 * f64::EPSILON * t[k + 1].abs().max(t[k].abs());
            if !mats.as_ref().is_some_and(|(h0, _)| same(*h0)) {
                mats = Some((h, step_mats(ss, h, hold)?));
            }
            let sm = &mats.as_ref().expect("just set").1;
            let u1 = u.row(k + 1);
            for i in 0..n {
                let mut acc = 0.0;
                for j in 0..n {
                    acc += sm.ad[(i, j)] * x[j];
                }
                for j in 0..m {
                    acc += sm.b0[(i, j)] * uk[j] + sm.b1[(i, j)] * u1[j];
                }
                next[i] = acc;
            }
        }
        if next.iter().any(|v| !v.is_finite()) {
            return Err(format!("the response overflows at t = {}", t[k + 1]));
        }
        x = next;
    }
    Ok(Sim { t: t.to_vec(), y: ys, x: xs })
}

/// The default time grid for a response that runs to `t_end`: `points`
/// evenly spaced samples for a continuous system, every multiple of `dt` for
/// a discrete one.
pub fn time_grid(ss: &Ss, t_end: f64, points: usize) -> Result<Vec<f64>, String> {
    if !(t_end.is_finite() && t_end > 0.0) {
        return Err(format!("the end time must be a positive number, got {t_end}"));
    }
    if ss.is_discrete() {
        let steps = (t_end / ss.dt + 1e-9).floor();
        if steps > 1e7 {
            return Err(format!("{steps} samples to reach t = {t_end} at dt = {} is too many", ss.dt));
        }
        Ok((0..=steps as usize).map(|k| k as f64 * ss.dt).collect())
    } else {
        if points < 2 {
            return Err("a time grid needs at least 2 points".into());
        }
        Ok((0..points).map(|k| t_end * k as f64 / (points - 1) as f64).collect())
    }
}

fn check_input_index(ss: &Ss, input: usize) -> Result<(), String> {
    if input >= ss.inputs() {
        return Err(format!("input index {input} is out of range: the system has {} input(s), numbered from 0", ss.inputs()));
    }
    Ok(())
}

/// Step and impulse responses are defined from `t = 0`; a time vector that
/// starts later is simulated from 0 and the extra first sample dropped.
fn from_zero(t: &[f64]) -> Result<(Vec<f64>, bool), String> {
    match t.first() {
        None => Err("the time vector is empty".into()),
        Some(&t0) if t0 < 0.0 => Err(format!("the response starts at t = 0, but the time vector starts at {t0}")),
        Some(&t0) if t0 > 0.0 => {
            let mut full = vec![0.0];
            full.extend_from_slice(t);
            Ok((full, true))
        }
        _ => Ok((t.to_vec(), false)),
    }
}

fn drop_first(sim: Sim, drop: bool) -> Sim {
    if !drop {
        return sim;
    }
    let len = sim.t.len() - 1;
    Sim { t: sim.t[1..].to_vec(), y: sim.y.block(1, 0, len, sim.y.c), x: sim.x.block(1, 0, len, sim.x.c) }
}

/// Unit step on one input (the others stay zero), from rest.
pub fn step(ss: &Ss, t: &[f64], input: usize) -> Result<Sim, String> {
    check_input_index(ss, input)?;
    let (full, dropped) = from_zero(t)?;
    if ss.is_discrete() && dropped {
        return Err("a discrete response starts at t = 0: the time vector must start at 0".into());
    }
    let mut u = Dense::zeros(full.len(), ss.inputs());
    for k in 0..full.len() {
        u[(k, input)] = 1.0;
    }
    Ok(drop_first(lsim(ss, &u, &full, None, Hold::Zoh)?, dropped))
}

/// Impulse on one input, from rest. Continuous: a unit-area Dirac impulse,
/// i.e. the free response from `x(0+) = B[:, input]` (a direct term `D`
/// would add an impulse at `t = 0` to the output; it is not representable in
/// samples and is left out). Discrete: the unit sample `u[0] = 1`.
pub fn impulse(ss: &Ss, t: &[f64], input: usize) -> Result<Sim, String> {
    check_input_index(ss, input)?;
    let (full, dropped) = from_zero(t)?;
    let mut u = Dense::zeros(full.len(), ss.inputs());
    if ss.is_discrete() {
        if dropped {
            return Err("a discrete response starts at t = 0: the time vector must start at 0".into());
        }
        u[(0, input)] = 1.0;
        return lsim(ss, &u, &full, None, Hold::Zoh);
    }
    let x0 = ss.b.col(input);
    Ok(drop_first(lsim(ss, &u, &full, Some(&x0), Hold::Zoh)?, dropped))
}

// ------------------------------------------------------------ connections

/// The common sample time of two systems. A static gain takes the other
/// system's.
fn common_dt(a: &Ss, b: &Ss) -> Result<f64, String> {
    if a.n() == 0 && a.dt == 0.0 {
        return Ok(b.dt);
    }
    if b.n() == 0 && b.dt == 0.0 {
        return Ok(a.dt);
    }
    if a.is_discrete() != b.is_discrete() {
        return Err("cannot connect a continuous system with a discrete one (convert one with lti_c2d or lti_d2c first)".into());
    }
    if (a.dt - b.dt).abs() > 1e-12 * a.dt.abs().max(b.dt.abs()) {
        return Err(format!("the two systems have different sample times ({} and {})", a.dt, b.dt));
    }
    Ok(a.dt)
}

/// `u -> first -> second -> y` (the transfer function `second * first`).
pub fn series(first: &Ss, second: &Ss) -> Result<Ss, String> {
    let dt = common_dt(first, second)?;
    if first.outputs() != second.inputs() {
        return Err(format!(
            "the first system has {} output(s) but the second has {} input(s)",
            first.outputs(),
            second.inputs()
        ));
    }
    let (n1, n2) = (first.n(), second.n());
    let a = Dense::blocks(&first.a, &Dense::zeros(n1, n2), &second.b.mul(&first.c), &second.a);
    let b = Dense::vstack(&first.b, &second.b.mul(&first.d));
    let c = Dense::hstack(&second.d.mul(&first.c), &second.c);
    let d = second.d.mul(&first.d);
    Ss::new(a, b, c, d, dt)
}

/// Both systems driven by the same input, outputs added.
pub fn parallel(x: &Ss, y: &Ss) -> Result<Ss, String> {
    let dt = common_dt(x, y)?;
    if x.inputs() != y.inputs() || x.outputs() != y.outputs() {
        return Err(format!(
            "both systems need the same number of inputs and outputs, got {} in / {} out and {} in / {} out",
            x.inputs(),
            x.outputs(),
            y.inputs(),
            y.outputs()
        ));
    }
    let (n1, n2) = (x.n(), y.n());
    let a = Dense::blocks(&x.a, &Dense::zeros(n1, n2), &Dense::zeros(n2, n1), &y.a);
    Ss::new(a, Dense::vstack(&x.b, &y.b), Dense::hstack(&x.c, &y.c), x.d.add(&y.d), dt)
}

/// Closed loop of `g` in the forward path and `h` in the return path:
/// `y = g (r + sign * h y)`, i.e. `g / (1 - sign * g h)`; `sign = -1` is
/// negative feedback, `g / (1 + g h)`.
pub fn feedback(g: &Ss, h: &Ss, sign: f64) -> Result<Ss, String> {
    if !(sign == 1.0 || sign == -1.0) {
        return Err(format!("sign must be -1 (negative feedback) or +1 (positive feedback), got {sign}"));
    }
    let dt = common_dt(g, h)?;
    if g.outputs() != h.inputs() || h.outputs() != g.inputs() {
        return Err(format!(
            "the return path must take the forward path's {} output(s) and give its {} input(s); it has {} input(s) and {} output(s)",
            g.outputs(),
            g.inputs(),
            h.inputs(),
            h.outputs()
        ));
    }
    let p = g.outputs();
    // (I - sign D1 D2) y = C1 x1 + sign D1 C2 x2 + D1 r
    let loop_d = Dense::eye(p).sub(&g.d.mul(&h.d).scale(sign));
    let algebraic = || "the loop is algebraic: 1 - sign * D_forward * D_return is singular, so the closed loop is not defined".to_string();
    let e_c1 = solve(&loop_d, &g.c).ok_or_else(algebraic)?;
    let e_d1 = solve(&loop_d, &g.d).ok_or_else(algebraic)?;
    let e_d1c2 = e_d1.mul(&h.c);
    let b1d2 = g.b.mul(&h.d).scale(sign);
    let a11 = g.a.add(&b1d2.mul(&e_c1));
    let a12 = g.b.mul(&h.c).scale(sign).add(&b1d2.mul(&e_d1c2).scale(sign));
    let a21 = h.b.mul(&e_c1);
    let a22 = h.a.add(&h.b.mul(&e_d1c2).scale(sign));
    let a = Dense::blocks(&a11, &a12, &a21, &a22);
    let b = Dense::vstack(&g.b.add(&b1d2.mul(&e_d1)), &h.b.mul(&e_d1));
    let c = Dense::hstack(&e_c1, &e_d1c2.scale(sign));
    Ss::new(a, b, c, e_d1, dt)
}

// ---------------------------------------------------------- discretization

/// How a continuous system is turned into a discrete one (and back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Zero-order hold on the input: exact at the samples for a
    /// piecewise-constant input.
    Zoh,
    /// Bilinear (trapezoidal) transform, `s = (2/dt)(z-1)/(z+1)`.
    Tustin,
    /// Forward Euler, `s = (z-1)/dt`.
    Euler,
    /// Backward Euler, `s = (z-1)/(dt z)`.
    Backward,
}

/// The generalized bilinear transform with weight `alpha` on the new sample
/// (0 forward Euler, 1/2 Tustin, 1 backward Euler).
fn gbt(ss: &Ss, h: f64, alpha: f64) -> Result<Ss, String> {
    let n = ss.n();
    let ima = Dense::eye(n).sub(&ss.a.scale(alpha * h));
    let singular = || "I - alpha*dt*A is singular: the system has a pole exactly where this method maps to infinity; pick another dt or method".to_string();
    let ad = solve(&ima, &Dense::eye(n).add(&ss.a.scale((1.0 - alpha) * h))).ok_or_else(singular)?;
    let bd = solve(&ima, &ss.b.scale(h)).ok_or_else(singular)?;
    let cd = solve_right(&ima, &ss.c).ok_or_else(singular)?;
    let dd = ss.d.add(&ss.c.mul(&bd).scale(alpha));
    Ss::new(ad, bd, cd, dd, h)
}

pub fn c2d(ss: &Ss, h: f64, method: Method) -> Result<Ss, String> {
    if ss.is_discrete() {
        return Err(format!("the system is already discrete (dt = {})", ss.dt));
    }
    if !(h.is_finite() && h > 0.0) {
        return Err(format!("the sample time dt must be a positive number, got {h}"));
    }
    match method {
        Method::Zoh => {
            let sm = step_mats(ss, h, Hold::Zoh)?;
            Ss::new(sm.ad, sm.b0, ss.c.clone(), ss.d.clone(), h)
        }
        Method::Tustin => gbt(ss, h, 0.5),
        Method::Euler => gbt(ss, h, 0.0),
        Method::Backward => gbt(ss, h, 1.0),
    }
}

/// The inverse of [`c2d`] for zero-order hold and Tustin.
pub fn d2c(ss: &Ss, method: Method) -> Result<Ss, String> {
    if !ss.is_discrete() {
        return Err("the system is already continuous".into());
    }
    let (n, m, h) = (ss.n(), ss.inputs(), ss.dt);
    match method {
        Method::Zoh => {
            // log([[Ad, Bd], [0, I]]) = [[A, B], [0, 0]] dt
            let mut aug = Dense::zeros(n + m, n + m);
            aug.put(0, 0, &ss.a);
            aug.put(0, n, &ss.b);
            aug.put(n, n, &Dense::eye(m));
            let l = logm(&aug).map_err(|why| format!("no zero-order-hold continuous equivalent: {why}"))?;
            Ss::new(l.block(0, 0, n, n).scale(1.0 / h), l.block(0, n, n, m).scale(1.0 / h), ss.c.clone(), ss.d.clone(), 0.0)
        }
        Method::Tustin => {
            let apl = ss.a.add(&Dense::eye(n));
            let singular = || "the system has a pole at z = -1, which the Tustin transform maps to infinity".to_string();
            let a = solve(&apl, &ss.a.sub(&Dense::eye(n))).ok_or_else(singular)?.scale(2.0 / h);
            let b = solve(&apl, &ss.b).ok_or_else(singular)?.scale(2.0 / h);
            let c = solve_right(&apl, &ss.c).ok_or_else(singular)?.scale(2.0);
            // Dd = D + C Bd / 2 with Bd = (dt/2)(Ad + I) B, so C Bd / 2 = (dt/4) C (Ad + I) B
            let d = ss.d.sub(&c.mul(&apl).mul(&b).scale(h / 4.0));
            Ss::new(a, b, c, d, 0.0)
        }
        Method::Euler | Method::Backward => Err("only \"zoh\" and \"tustin\" can be inverted".into()),
    }
}

// -------------------------------------------------------- frequency domain

/// A Bode response: magnitude in dB and unwrapped phase in degrees.
#[derive(Clone, Debug)]
pub struct Bode {
    pub w: Vec<f64>,
    pub mag_db: Vec<f64>,
    pub phase_deg: Vec<f64>,
}

/// Log-spaced frequencies covering every pole and zero with a decade to
/// spare on each side (continuous), or up to the Nyquist frequency `pi`
/// rad/sample (discrete).
pub fn default_frequencies(sys: &Lti) -> Result<Vec<f64>, String> {
    let (z, p, _) = sys.zpk()?;
    let discrete = sys.ss.is_discrete();
    let corners: Vec<f64> = z
        .iter()
        .chain(&p)
        .map(|r| if discrete { r.ln().magnitude() } else { r.magnitude() })
        .filter(|v| v.is_finite() && *v > 1e-12)
        .collect();
    let lo = corners.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = corners.iter().copied().fold(0.0, f64::max);
    let (lo_dec, hi_dec) = if discrete {
        let top = PI.log10();
        let lo_dec = if lo.is_finite() { (lo.min(PI / 10.0).log10().floor() - 1.0).max(-8.0) } else { top - 3.0 };
        (lo_dec, top)
    } else if lo.is_finite() {
        (lo.log10().floor() - 1.0, hi.log10().ceil() + 1.0)
    } else {
        (-1.0, 1.0)
    };
    let points = (((hi_dec - lo_dec) * 50.0).ceil() as usize).max(200) + 1;
    Ok((0..points).map(|k| 10f64.powf(lo_dec + (hi_dec - lo_dec) * k as f64 / (points - 1) as f64)).collect())
}

/// Magnitude (dB) and phase (degrees) at each `w`. The phase is unwrapped
/// along `w`, and its branch is chosen so the first point agrees with the sum
/// of the angles each zero and pole contributes (so `1/s^3` reads -270
/// degrees, not +90).
pub fn bode(sys: &Lti, w: &[f64]) -> Result<Bode, String> {
    let h = sys.freqresp(w)?;
    let mag_db: Vec<f64> = h.iter().map(|v| 20.0 * v.magnitude().log10()).collect();
    let mut phase: Vec<f64> = h.iter().map(|v| v.arg()).collect();
    let mut offset = 0.0;
    for k in 1..phase.len() {
        let raw = h[k].arg();
        let prev = h[k - 1].arg();
        if raw.is_finite() && prev.is_finite() {
            let jump = raw - prev;
            if jump > PI {
                offset -= 2.0 * PI;
            } else if jump < -PI {
                offset += 2.0 * PI;
            }
        }
        phase[k] = raw + offset;
    }
    if let (Some(&w0), Some(&p0)) = (w.first(), phase.first()) {
        if p0.is_finite() {
            let (z, p, k) = sys.zpk()?;
            let s0 = sys.boundary(w0);
            let natural = if k < 0.0 { PI } else { 0.0 } + z.iter().map(|r| s0.sub(*r).arg()).sum::<f64>()
                - p.iter().map(|r| s0.sub(*r).arg()).sum::<f64>();
            if natural.is_finite() {
                let shift = 2.0 * PI * ((natural - p0) / (2.0 * PI)).round();
                for v in &mut phase {
                    *v += shift;
                }
            }
        }
    }
    Ok(Bode { w: w.to_vec(), mag_db, phase_deg: phase.iter().map(|v| v.to_degrees()).collect() })
}

/// Stability margins of a loop transfer function `L` closed with unity
/// negative feedback.
#[derive(Clone, Debug)]
pub struct Margins {
    /// Gain margin as a factor: the loop gain can be multiplied by this
    /// before the closed loop reaches the stability boundary. `inf` when
    /// the phase never crosses -180 degrees.
    pub gm: f64,
    /// Phase margin in degrees. `inf` when the gain never crosses 1.
    pub pm: f64,
    /// Frequency of the gain margin (phase = -180 degrees); `NaN` if none.
    pub w_gm: f64,
    /// Frequency of the phase margin (|L| = 1); `NaN` if none.
    pub w_pm: f64,
    /// Whether `L/(1+L)` is stable.
    pub stable: bool,
}

/// Real and imaginary parts of `c(jw)` as polynomials in `w`, ascending.
fn on_imaginary_axis(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let deg = c.len() - 1;
    let mut re = vec![0.0; deg + 1];
    let mut im = vec![0.0; deg + 1];
    for (i, &v) in c.iter().enumerate() {
        let d = deg - i;
        match d % 4 {
            0 => re[d] = v,
            1 => im[d] = v,
            2 => re[d] = -v,
            _ => im[d] = -v,
        }
    }
    (re, im)
}

/// Positive real roots `w` of a real polynomial given in ascending powers
/// that has only even powers (`odd = false`) or only odd powers (`odd =
/// true`): solved as a polynomial in `w^2` of half the degree.
fn positive_real_roots(ascending: &[f64], odd: bool) -> Result<Vec<f64>, String> {
    let half: Vec<f64> = ascending.iter().skip(odd as usize).step_by(2).copied().collect();
    let mut desc: Vec<f64> = half.iter().rev().copied().collect();
    desc = trim_leading(&desc, 1e-13);
    if desc.len() < 2 {
        return Ok(Vec::new());
    }
    let mut out: Vec<f64> = poly_roots(&desc)?
        .into_iter()
        .filter(|r| r.re > 0.0 && r.im.abs() <= 1e-6 * r.re)
        .map(|r| r.re.sqrt())
        .collect();
    out.sort_by(|a, b| a.total_cmp(b));
    Ok(out)
}

fn wrap_deg(d: f64) -> f64 {
    let mut v = d % 360.0;
    if v > 180.0 {
        v -= 360.0;
    }
    if v <= -180.0 {
        v += 360.0;
    }
    v
}

fn bisect(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    let mut fa = f(a);
    for _ in 0..200 {
        let mid = 0.5 * (a + b);
        if mid == a || mid == b {
            break;
        }
        let fm = f(mid);
        if (fm > 0.0) == (fa > 0.0) {
            a = mid;
            fa = fm;
        } else {
            b = mid;
        }
    }
    0.5 * (a + b)
}

/// Gain and phase margins of the loop `L` (single-input single-output).
/// When the phase crosses -180 degrees, or the gain crosses 1, more than
/// once, the crossing with the smallest margin (closest to 0 dB / 0
/// degrees) is reported.
pub fn margins(sys: &Lti) -> Result<Margins, String> {
    let (num, den) = sys.tf()?;
    let discrete = sys.ss.is_discrete();
    let l = |w: f64| sys.at(sys.boundary(w)).unwrap_or(Complex64::new(f64::NAN, f64::NAN));
    let (phase_cross, gain_cross): (Vec<f64>, Vec<f64>) = if discrete {
        let mut grid: Vec<f64> = (1..=4000).map(|k| PI * k as f64 / 4000.0).collect();
        grid.extend((0..2000).map(|k| PI * 10f64.powf(-8.0 + 8.0 * k as f64 / 2000.0)));
        grid.sort_by(|a, b| a.total_cmp(b));
        grid.dedup();
        let vals: Vec<Complex64> = grid.iter().map(|&w| l(w)).collect();
        let (mut pc, mut gc) = (Vec::new(), Vec::new());
        for k in 1..grid.len() {
            let (a, b) = (vals[k - 1], vals[k]);
            if !(a.re.is_finite() && a.im.is_finite() && b.re.is_finite() && b.im.is_finite()) {
                continue;
            }
            if a.re < 0.0 && b.re < 0.0 && (a.im > 0.0) != (b.im > 0.0) && a.im != 0.0 {
                let w = bisect(&|w| l(w).im, grid[k - 1], grid[k]);
                let v = l(w);
                // a sign change through a pole is not a crossing
                if v.im.abs() <= 1e-6 * v.magnitude() {
                    pc.push(w);
                }
            }
            let (ma, mb) = (a.magnitude() - 1.0, b.magnitude() - 1.0);
            if (ma > 0.0) != (mb > 0.0) {
                gc.push(bisect(&|w| l(w).magnitude() - 1.0, grid[k - 1], grid[k]));
            }
        }
        // At the Nyquist frequency a real system's response is real.
        let at_pi = l(PI);
        if at_pi.re.is_finite() && at_pi.re < 0.0 && at_pi.im.abs() <= 1e-9 * at_pi.magnitude() && pc.last().map_or(true, |w| PI - w > 1e-9) {
            pc.push(PI);
        }
        (pc, gc)
    } else {
        let (nr, ni) = on_imaginary_axis(&strip_leading(&num));
        let (dr, di) = on_imaginary_axis(&strip_leading(&den));
        let asc_mul = |a: &[f64], b: &[f64]| polymul(a, b);
        let asc_add = |a: &[f64], b: &[f64], sb: f64| {
            let mut out = vec![0.0; a.len().max(b.len())];
            for (i, v) in a.iter().enumerate() {
                out[i] += v;
            }
            for (i, v) in b.iter().enumerate() {
                out[i] += sb * v;
            }
            out
        };
        // L = N conj(D) / |D|^2: phase crossings where Im(N conj D) = 0 and
        // Re < 0; gain crossings where |N|^2 = |D|^2.
        let im = asc_add(&asc_mul(&ni, &dr), &asc_mul(&nr, &di), -1.0);
        let re = asc_add(&asc_mul(&nr, &dr), &asc_mul(&ni, &di), 1.0);
        let re_at = |w: f64| re.iter().rev().fold(0.0, |acc, c| acc * w + c);
        let pc = positive_real_roots(&im, true)?.into_iter().filter(|&w| re_at(w) < 0.0).collect();
        let n2 = asc_add(&asc_mul(&nr, &nr), &asc_mul(&ni, &ni), 1.0);
        let d2 = asc_add(&asc_mul(&dr, &dr), &asc_mul(&di, &di), 1.0);
        let gc = positive_real_roots(&asc_add(&n2, &d2, -1.0), false)?;
        (pc, gc)
    };
    let (mut gm, mut w_gm) = (f64::INFINITY, f64::NAN);
    for w in phase_cross {
        let mag = l(w).magnitude();
        if !(mag.is_finite() && mag > 0.0) {
            continue;
        }
        let g = 1.0 / mag;
        if w_gm.is_nan() || g.log10().abs() < gm.log10().abs() {
            gm = g;
            w_gm = w;
        }
    }
    let (mut pm, mut w_pm) = (f64::INFINITY, f64::NAN);
    for w in gain_cross {
        let v = l(w);
        if !(v.re.is_finite() && v.im.is_finite()) {
            continue;
        }
        let m = wrap_deg(180.0 + v.arg().to_degrees());
        if w_pm.is_nan() || m.abs() < pm.abs() {
            pm = m;
            w_pm = w;
        }
    }
    let closed = poly_roots(&polyadd(&num, &den))?;
    let stable = closed.iter().all(|p| if discrete { p.magnitude() < 1.0 } else { p.re < 0.0 });
    Ok(Margins { gm, pm, w_gm, w_pm, stable })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b} (tol {tol})");
    }

    fn sorted(mut v: Vec<Complex64>) -> Vec<Complex64> {
        v.sort_by(|a, b| a.re.total_cmp(&b.re).then(a.im.total_cmp(&b.im)));
        v
    }

    fn close_roots(got: Vec<Complex64>, want: &[(f64, f64)], tol: f64) {
        let got = sorted(got);
        let want = sorted(want.iter().map(|&(re, im)| Complex64::new(re, im)).collect());
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!(g.sub(*w).magnitude() <= tol, "{got:?} vs {want:?}");
        }
    }

    fn tf(num: &[f64], den: &[f64]) -> Lti {
        Lti::from_tf(num, den, 0.0).unwrap()
    }

    fn grid(t_end: f64, n: usize) -> Vec<f64> {
        (0..n).map(|k| t_end * k as f64 / (n - 1) as f64).collect()
    }

    // ---- matrix functions

    #[test]
    fn expm_matches_closed_forms() {
        // rotation generator: exp([[0, w], [-w, 0]] t) is a rotation
        let e = expm(&Dense::from_rows(&[&[0.0, 2.0], &[-2.0, 0.0]])).unwrap();
        close(e[(0, 0)], 2f64.cos(), 1e-14);
        close(e[(0, 1)], 2f64.sin(), 1e-14);
        close(e[(1, 0)], -(2f64.sin()), 1e-14);
        // nilpotent: exact
        let e = expm(&Dense::from_rows(&[&[0.0, 3.0], &[0.0, 0.0]])).unwrap();
        assert_eq!(e.d, vec![1.0, 3.0, 0.0, 1.0]);
        // large norm exercises the squaring phase
        let e = expm(&Dense::from_rows(&[&[-50.0, 0.0], &[0.0, 3.0]])).unwrap();
        close(e[(0, 0)] / (-50f64).exp(), 1.0, 1e-12);
        close(e[(1, 1)] / 3f64.exp(), 1.0, 1e-13);
        // defective (Jordan block): exp([[a, 1], [0, a]]) = e^a [[1, 1], [0, 1]]
        let e = expm(&Dense::from_rows(&[&[-0.5, 1.0], &[0.0, -0.5]])).unwrap();
        close(e[(0, 1)], (-0.5f64).exp(), 1e-15);
        assert!(expm(&Dense::zeros(2, 3)).is_err());
        assert!(expm(&Dense::from_rows(&[&[f64::NAN]])).is_err());
    }

    #[test]
    fn logm_inverts_expm_and_refuses_a_negative_eigenvalue() {
        let a = Dense::from_rows(&[&[-1.0, 2.0, 0.0], &[0.5, -3.0, 1.0], &[0.0, 0.2, -0.4]]);
        let back = logm(&expm(&a).unwrap()).unwrap();
        assert!(back.sub(&a).norm1() < 1e-12, "{back:?}");
        assert!(logm(&Dense::from_rows(&[&[-1.0]])).is_err());
        assert!(logm(&Dense::from_rows(&[&[0.0]])).is_err());
    }

    #[test]
    fn solve_reports_a_singular_matrix() {
        assert!(solve(&Dense::from_rows(&[&[1.0, 2.0], &[2.0, 4.0]]), &Dense::eye(2)).is_none());
        let x = solve(&Dense::from_rows(&[&[0.0, 2.0], &[1.0, 1.0]]), &Dense::from_rows(&[&[4.0], &[3.0]])).unwrap();
        assert_eq!(x.d, vec![1.0, 2.0]);
    }

    #[test]
    fn poly_roots_handles_polynomials_in_x_squared() {
        // the companion matrix of an even polynomial defeats the QR iteration
        for c in [vec![-1.0, 0.0, -5.0, 0.0, -4.0, 0.0, 49.0], vec![1.0, 0.0, 4.0], vec![1.0, 0.0, 0.0, 0.0, -16.0], vec![2.0, -6.0, 4.0, 0.0, 0.0]] {
            let roots = poly_roots(&c).unwrap();
            assert_eq!(roots.len(), c.len() - 1);
            let scale: f64 = c.iter().map(|v| v.abs()).sum();
            for r in &roots {
                let size = r.magnitude().max(1.0).powi(c.len() as i32 - 1);
                assert!(polyval(&c, *r).magnitude() <= 1e-10 * scale * size, "{r:?} is not a root of {c:?}");
            }
        }
        assert!(durand_kerner(&[1.0, 0.0, -5.0, 0.0, 4.0]).unwrap().iter().all(|r| r.im.abs() < 1e-9));
        close_roots(durand_kerner(&[1.0, 0.0, -5.0, 0.0, 4.0]).unwrap(), &[(-2.0, 0.0), (-1.0, 0.0), (1.0, 0.0), (2.0, 0.0)], 1e-12);
    }

    // ---- constructors and conversions

    #[test]
    fn ss_new_validates_every_dimension() {
        let a = Dense::eye(2);
        let ok = Ss::new(a.clone(), Dense::zeros(2, 1), Dense::zeros(1, 2), Dense::zeros(1, 1), 0.0);
        assert!(ok.is_ok());
        let bad = |r: Result<Ss, String>, word: &str| {
            let msg = r.expect_err("should be refused");
            assert!(msg.contains(word), "{msg}");
        };
        bad(Ss::new(Dense::zeros(2, 3), Dense::zeros(2, 1), Dense::zeros(1, 2), Dense::zeros(1, 1), 0.0), "A must be square");
        bad(Ss::new(a.clone(), Dense::zeros(3, 1), Dense::zeros(1, 2), Dense::zeros(1, 1), 0.0), "B must have 2 row");
        bad(Ss::new(a.clone(), Dense::zeros(2, 1), Dense::zeros(1, 3), Dense::zeros(1, 1), 0.0), "C must have 2 column");
        bad(Ss::new(a.clone(), Dense::zeros(2, 1), Dense::zeros(1, 2), Dense::zeros(2, 1), 0.0), "D must be 1x1");
        bad(Ss::new(a.clone(), Dense::zeros(2, 1), Dense::zeros(1, 2), Dense::zeros(1, 1), -1.0), "dt");
        bad(Ss::new(a, Dense::zeros(2, 1), Dense::zeros(1, 2), Dense::from_rows(&[&[f64::NAN]]), 0.0), "NaN");
    }

    #[test]
    fn tf_to_ss_is_controllable_canonical_and_round_trips() {
        // (2 s + 3) / (s^2 + 4 s + 5)
        let ss = tf_to_ss(&[2.0, 3.0], &[1.0, 4.0, 5.0], 0.0).unwrap();
        assert_eq!(ss.a.d, vec![0.0, 1.0, -5.0, -4.0]);
        assert_eq!(ss.b.d, vec![0.0, 1.0]);
        assert_eq!(ss.c.d, vec![3.0, 2.0]);
        assert_eq!(ss.d.d, vec![0.0]);
        let (num, den) = ss_to_tf(&ss).unwrap();
        assert_eq!(num, vec![2.0, 3.0]);
        assert_eq!(den, vec![1.0, 4.0, 5.0]);
        // biproper with a non-monic denominator: (4 s^2 + 2 s + 6) / (2 s^2 + 6 s + 4)
        let ss = tf_to_ss(&[4.0, 2.0, 6.0], &[2.0, 6.0, 4.0], 0.0).unwrap();
        assert_eq!(ss.d.d, vec![2.0]);
        let (num, den) = ss_to_tf(&ss).unwrap();
        assert_eq!(num, vec![2.0, 1.0, 3.0]);
        assert_eq!(den, vec![1.0, 3.0, 2.0]);
        // a static gain has no states
        let ss = tf_to_ss(&[6.0], &[2.0], 0.0).unwrap();
        assert_eq!((ss.n(), ss.d.d.clone()), (0, vec![3.0]));
        assert_eq!(ss_to_tf(&ss).unwrap(), (vec![3.0], vec![1.0]));
    }

    #[test]
    fn tf_to_ss_refuses_improper_and_degenerate_input() {
        let msg = tf_to_ss(&[1.0, 0.0, 0.0], &[1.0, 1.0], 0.0).unwrap_err();
        assert!(msg.contains("improper") && msg.contains("degree 2") && msg.contains("degree 1"), "{msg}");
        // leading zeros do not make a proper function improper
        assert!(tf_to_ss(&[0.0, 0.0, 1.0], &[1.0, 1.0], 0.0).is_ok());
        assert!(tf_to_ss(&[1.0], &[0.0, 0.0], 0.0).unwrap_err().contains("denominator is zero"));
        assert!(tf_to_ss(&[], &[1.0], 0.0).is_err());
        assert!(tf_to_ss(&[1.0], &[1.0, 1.0], f64::NAN).is_err());
    }

    #[test]
    fn ss_to_tf_refuses_mimo() {
        let ss = Ss::new(Dense::eye(2), Dense::eye(2), Dense::eye(2), Dense::zeros(2, 2), 0.0).unwrap();
        assert!(ss_to_tf(&ss).unwrap_err().contains("2 input(s) and 2 output(s)"));
        assert!(Lti::from_ss(ss).zeros().is_err());
    }

    #[test]
    fn zpk_round_trips_and_validates() {
        let j = |re: f64, im: f64| Complex64::new(re, im);
        let sys = Lti::from_zpk(&[j(-3.0, 0.0)], &[j(-1.0, 2.0), j(-1.0, -2.0)], 5.0, 0.0).unwrap();
        let (num, den) = sys.tf().unwrap();
        assert_eq!(num, vec![5.0, 15.0]);
        assert_eq!(den, vec![1.0, 2.0, 5.0]);
        close_roots(sys.ss.poles().unwrap(), &[(-1.0, 2.0), (-1.0, -2.0)], 1e-12);
        // from a transfer function back to zeros, poles and gain
        let (z, p, k) = tf(&[2.0, 6.0], &[1.0, 3.0, 2.0]).zpk().unwrap();
        close_roots(z, &[(-3.0, 0.0)], 1e-12);
        close_roots(p, &[(-1.0, 0.0), (-2.0, 0.0)], 1e-12);
        close(k, 2.0, 0.0);
        assert!(Lti::from_zpk(&[], &[j(-1.0, 2.0)], 1.0, 0.0).unwrap_err().contains("conjugate pairs"));
        assert!(Lti::from_zpk(&[j(1.0, 0.0), j(2.0, 0.0)], &[j(-1.0, 0.0)], 1.0, 0.0).unwrap_err().contains("improper"));
        assert!(Lti::from_zpk(&[], &[j(-1.0, 0.0)], f64::INFINITY, 0.0).is_err());
    }

    // ---- analysis

    #[test]
    fn poles_zeros_and_stability() {
        let sys = tf(&[1.0, 5.0], &[1.0, 3.0, 2.0]);
        close_roots(sys.poles().unwrap(), &[(-1.0, 0.0), (-2.0, 0.0)], 1e-12);
        close_roots(sys.zeros().unwrap(), &[(-5.0, 0.0)], 1e-12);
        assert!(sys.is_stable().unwrap());
        // the same through the realization
        let ss = Lti::from_ss(sys.ss.clone());
        close_roots(ss.poles().unwrap(), &[(-1.0, 0.0), (-2.0, 0.0)], 1e-12);
        close_roots(ss.zeros().unwrap(), &[(-5.0, 0.0)], 1e-10);
        // an integrator is not asymptotically stable; neither is an unstable pole
        assert!(!tf(&[1.0], &[1.0, 0.0]).is_stable().unwrap());
        assert!(!tf(&[1.0], &[1.0, -1.0]).is_stable().unwrap());
        // discrete: the unit circle decides
        assert!(Lti::from_tf(&[1.0], &[1.0, -0.5], 1.0).unwrap().is_stable().unwrap());
        assert!(!Lti::from_tf(&[1.0], &[1.0, -1.5], 1.0).unwrap().is_stable().unwrap());
        // z = -1.5 is in the left half plane but outside the circle
        assert!(!Lti::from_tf(&[1.0], &[1.0, 1.5], 1.0).unwrap().is_stable().unwrap());
        // no zeros
        assert!(tf(&[4.0], &[1.0, 1.0]).zeros().unwrap().is_empty());
    }

    #[test]
    fn dcgain_continuous_discrete_and_at_a_pole() {
        close(tf(&[3.0], &[1.0, 2.0]).dcgain().unwrap()[(0, 0)], 1.5, 1e-15);
        // discrete: H(1) = 1 / (1 - 0.5)
        close(Lti::from_tf(&[1.0, 0.0], &[1.0, -0.5], 0.1).unwrap().dcgain().unwrap()[(0, 0)], 2.0, 1e-15);
        assert_eq!(tf(&[1.0], &[1.0, 0.0]).dcgain().unwrap()[(0, 0)], f64::INFINITY);
        assert_eq!(tf(&[-1.0], &[1.0, 0.0]).dcgain().unwrap()[(0, 0)], f64::NEG_INFINITY);
        assert!(tf(&[1.0, 0.0], &[1.0, 0.0]).dcgain().unwrap()[(0, 0)].is_nan());
        // two outputs
        let ss = Ss::new(
            Dense::from_rows(&[&[-1.0, 0.0], &[0.0, -2.0]]),
            Dense::from_rows(&[&[1.0], &[1.0]]),
            Dense::eye(2),
            Dense::zeros(2, 1),
            0.0,
        )
        .unwrap();
        assert_eq!(Lti::from_ss(ss).dcgain().unwrap().d, vec![1.0, 0.5]);
    }

    #[test]
    fn freqresp_agrees_across_forms() {
        let w = [0.0, 0.3, 1.0, 7.0];
        let a = tf(&[1.0, 3.0], &[1.0, 2.0, 5.0]);
        let b = Lti::from_ss(a.ss.clone());
        let c = Lti::from_zpk(&[Complex64::real(-3.0)], &[Complex64::new(-1.0, 2.0), Complex64::new(-1.0, -2.0)], 1.0, 0.0).unwrap();
        let (ha, hb, hc) = (a.freqresp(&w).unwrap(), b.freqresp(&w).unwrap(), c.freqresp(&w).unwrap());
        for k in 0..w.len() {
            // by hand: (jw + 3) / ((jw)^2 + 2 jw + 5)
            let s = Complex64::new(0.0, w[k]);
            let want = s.add(Complex64::real(3.0)).div(s.mul(s).add(s.scale(2.0)).add(Complex64::real(5.0)));
            for h in [ha[k], hb[k], hc[k]] {
                assert!(h.sub(want).magnitude() < 1e-14, "{h:?} vs {want:?}");
            }
        }
        // discrete: z = e^{jw}, w in rad/sample; at w = pi, 1/(z - 0.5) = 1/(-1.5)
        let d = Lti::from_tf(&[1.0], &[1.0, -0.5], 0.01).unwrap();
        let h = d.freqresp(&[PI]).unwrap()[0];
        close(h.re, -1.0 / 1.5, 1e-15);
        close(h.im, 0.0, 1e-15);
        // on a pole the response is infinite, in every form
        assert_eq!(tf(&[1.0], &[1.0, 0.0]).freqresp(&[0.0]).unwrap()[0].re, f64::INFINITY);
        assert_eq!(Lti::from_ss(tf(&[1.0], &[1.0, 0.0]).ss).freqresp(&[0.0]).unwrap()[0].re, f64::INFINITY);
    }

    // ---- simulation

    #[test]
    fn first_order_lag_step_is_exact() {
        let tau = 0.7;
        let sys = tf(&[1.0], &[tau, 1.0]);
        let t = grid(5.0, 41);
        let r = step(&sys.ss, &t, 0).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], 1.0 - (-tk / tau).exp(), 2e-15);
        }
        // uneven time stamps are exact too
        let t = [0.0, 0.01, 0.5, 0.6, 3.0];
        let r = step(&sys.ss, &t, 0).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], 1.0 - (-tk / tau).exp(), 2e-15);
        }
        // a time vector that starts after 0 still means "step at t = 0"
        let r = step(&sys.ss, &[1.0, 2.0], 0).unwrap();
        assert_eq!(r.t, vec![1.0, 2.0]);
        close(r.y[(0, 0)], 1.0 - (-1.0 / tau).exp(), 2e-15);
        close(r.y[(1, 0)], 1.0 - (-2.0 / tau).exp(), 2e-15);
    }

    #[test]
    fn underdamped_second_order_step_overshoot() {
        let (zeta, wn) = (0.3f64, 2.0f64);
        let sys = tf(&[wn * wn], &[1.0, 2.0 * zeta * wn, wn * wn]);
        let wd = wn * (1.0 - zeta * zeta).sqrt();
        // sample exactly at the peak time pi/wd
        let t = [0.0, PI / wd];
        let r = step(&sys.ss, &t, 0).unwrap();
        close(r.y[(1, 0)] - 1.0, (-PI * zeta / (1.0 - zeta * zeta).sqrt()).exp(), 1e-14);
        // and the whole trajectory against the closed form
        let t = grid(8.0, 400);
        let r = step(&sys.ss, &t, 0).unwrap();
        let phi = zeta.acos();
        let mut peak = 0.0f64;
        for (k, &tk) in t.iter().enumerate() {
            let want = 1.0 - (-zeta * wn * tk).exp() * (wd * tk + phi).sin() / (1.0 - zeta * zeta).sqrt();
            close(r.y[(k, 0)], want, 1e-13);
            peak = peak.max(r.y[(k, 0)]);
        }
        assert!(peak > 1.3 && peak < 1.3724, "peak {peak}");
    }

    #[test]
    fn integrator_ramp_and_first_order_hold() {
        let sys = tf(&[1.0], &[1.0, 0.0]);
        let t = grid(4.0, 9);
        // step in -> ramp out
        let r = step(&sys.ss, &t, 0).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], tk, 1e-15);
        }
        // ramp in: first-order hold is exact (t^2/2), zero-order hold is the left Riemann sum
        let u = Dense { r: t.len(), c: 1, d: t.clone() };
        let foh = lsim(&sys.ss, &u, &t, None, Hold::Foh).unwrap();
        let zoh = lsim(&sys.ss, &u, &t, None, Hold::Zoh).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(foh.y[(k, 0)], tk * tk / 2.0, 1e-14);
            let left: f64 = t[..k].iter().map(|v| v * 0.5).sum();
            close(zoh.y[(k, 0)], left, 1e-14);
        }
    }

    #[test]
    fn lsim_sine_into_a_lag_matches_the_analytic_response_for_foh_limit() {
        // u = t on a lag: y = t - tau (1 - e^{-t/tau}); linear input, so foh is exact
        let tau = 0.5;
        let sys = tf(&[1.0], &[tau, 1.0]);
        let t = grid(3.0, 7);
        let u = Dense { r: t.len(), c: 1, d: t.clone() };
        let r = lsim(&sys.ss, &u, &t, None, Hold::Foh).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], tk - tau * (1.0 - (-tk / tau).exp()), 1e-14);
        }
        // initial state: free decay
        let free = lsim(&sys.ss, &Dense::zeros(t.len(), 1), &t, Some(&[2.0]), Hold::Foh).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(free.x[(k, 0)], 2.0 * (-tk / tau).exp(), 1e-14);
        }
    }

    #[test]
    fn impulse_continuous_and_discrete() {
        let tau = 0.25;
        let sys = tf(&[1.0], &[tau, 1.0]);
        let t = grid(2.0, 21);
        let r = impulse(&sys.ss, &t, 0).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], (-tk / tau).exp() / tau, 1e-13);
        }
        // discrete: 1/(z - 0.5) -> 0, 1, 0.5, 0.25, ...
        let d = Lti::from_tf(&[1.0], &[1.0, -0.5], 1.0).unwrap();
        let r = impulse(&d.ss, &[0.0, 1.0, 2.0, 3.0], 0).unwrap();
        assert_eq!(r.y.d, vec![0.0, 1.0, 0.5, 0.25]);
    }

    #[test]
    fn discrete_accumulator_and_time_checks() {
        // y[k] = y[k-1] + u[k-1]: z^-1 / (1 - z^-1) = 1 / (z - 1)
        let acc = Lti::from_tf(&[1.0], &[1.0, -1.0], 0.5).unwrap();
        let t = time_grid(&acc.ss, 3.0, 0).unwrap();
        assert_eq!(t, vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0]);
        let r = step(&acc.ss, &t, 0).unwrap();
        assert_eq!(r.y.d, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        // running sum of an arbitrary input
        let u = Dense { r: 4, c: 1, d: vec![3.0, -1.0, 4.0, 1.0] };
        let r = lsim(&acc.ss, &u, &[0.0, 0.5, 1.0, 1.5], None, Hold::Foh).unwrap();
        assert_eq!(r.y.d, vec![0.0, 3.0, 2.0, 6.0]);
        // the time vector must step by dt
        let msg = lsim(&acc.ss, &u, &[0.0, 1.0, 2.0, 3.0], None, Hold::Zoh).unwrap_err();
        assert!(msg.contains("dt = 0.5"), "{msg}");
        // shape and ordering errors
        let c = tf(&[1.0], &[1.0, 1.0]);
        assert!(lsim(&c.ss, &u, &[0.0, 1.0, 1.0, 2.0], None, Hold::Zoh).unwrap_err().contains("strictly increasing"));
        assert!(lsim(&c.ss, &u, &[0.0, 1.0, 2.0], None, Hold::Zoh).unwrap_err().contains("one row per time sample"));
        assert!(lsim(&c.ss, &u, &[0.0, 1.0, 2.0, 3.0], Some(&[0.0, 0.0]), Hold::Zoh).unwrap_err().contains("x0"));
        assert!(step(&c.ss, &[-1.0, 0.0], 0).is_err());
        assert!(step(&c.ss, &[0.0, 1.0], 1).unwrap_err().contains("input index 1"));
        assert!(time_grid(&c.ss, -1.0, 100).is_err());
    }

    // ---- connections

    #[test]
    fn series_and_parallel_match_polynomial_algebra() {
        let g1 = tf(&[1.0], &[1.0, 1.0]);
        let g2 = tf(&[2.0], &[1.0, 3.0]);
        // 2 / ((s+1)(s+3))
        let (num, den) = ss_to_tf(&series(&g1.ss, &g2.ss).unwrap()).unwrap();
        assert_eq!((num, den), (vec![2.0], vec![1.0, 4.0, 3.0]));
        // 1/(s+1) + 2/(s+3) = (3 s + 5) / (s^2 + 4 s + 3)
        let (num, den) = ss_to_tf(&parallel(&g1.ss, &g2.ss).unwrap()).unwrap();
        assert_eq!((num, den), (vec![3.0, 5.0], vec![1.0, 4.0, 3.0]));
        // order matters for shapes, not for a SISO product
        let w = [0.5, 2.0];
        let ab = Lti::from_ss(series(&g1.ss, &g2.ss).unwrap()).freqresp(&w).unwrap();
        let ba = Lti::from_ss(series(&g2.ss, &g1.ss).unwrap()).freqresp(&w).unwrap();
        for k in 0..2 {
            assert!(ab[k].sub(ba[k]).magnitude() < 1e-15);
        }
        // a static gain joins either kind of system
        let d = Lti::from_tf(&[1.0], &[1.0, -0.5], 0.1).unwrap();
        let k = Lti::from_tf(&[4.0], &[1.0], 0.0).unwrap();
        let kd = series(&k.ss, &d.ss).unwrap();
        assert_eq!(kd.dt, 0.1);
        assert_eq!(ss_to_tf(&kd).unwrap().0, vec![4.0]);
        // mixing continuous and discrete is refused, as are mismatched sample times
        assert!(series(&g1.ss, &d.ss).unwrap_err().contains("continuous system with a discrete"));
        let d2 = Lti::from_tf(&[1.0], &[1.0, -0.5], 0.2).unwrap();
        assert!(parallel(&d.ss, &d2.ss).unwrap_err().contains("different sample times"));
    }

    #[test]
    fn feedback_poles_are_the_roots_of_one_plus_l() {
        // L = 10 / (s (s+1)(s+2)): 1 + L = 0 <=> s^3 + 3 s^2 + 2 s + 10 = 0
        let l = tf(&[10.0], &[1.0, 3.0, 2.0, 0.0]);
        let unity = Ss::gain(Dense::from_rows(&[&[1.0]]), 0.0);
        let cl = feedback(&l.ss, &unity, -1.0).unwrap();
        let want = poly_roots(&[1.0, 3.0, 2.0, 10.0]).unwrap();
        let want: Vec<(f64, f64)> = want.iter().map(|r| (r.re, r.im)).collect();
        close_roots(cl.poles().unwrap(), &want, 1e-12);
        let (num, den) = ss_to_tf(&cl).unwrap();
        assert_eq!((num, den), (vec![10.0], vec![1.0, 3.0, 2.0, 10.0]));
        // dynamics in the return path: G = 1/(s+1), H = 2/(s+3) -> G/(1+GH) = (s+3)/(s^2+4s+5)
        let g = tf(&[1.0], &[1.0, 1.0]);
        let h = tf(&[2.0], &[1.0, 3.0]);
        let (num, den) = ss_to_tf(&feedback(&g.ss, &h.ss, -1.0).unwrap()).unwrap();
        assert_eq!((num, den), (vec![1.0, 3.0], vec![1.0, 4.0, 5.0]));
        // positive feedback: G/(1-GH) = (s+3)/(s^2+4s+1)
        let (num, den) = ss_to_tf(&feedback(&g.ss, &h.ss, 1.0).unwrap()).unwrap();
        assert_eq!((num, den), (vec![1.0, 3.0], vec![1.0, 4.0, 1.0]));
        // direct terms on both sides: G = (s+2)/(s+1), H = 3 -> (s+2)/(4s+7)
        let g = tf(&[1.0, 2.0], &[1.0, 1.0]);
        let h3 = Ss::gain(Dense::from_rows(&[&[3.0]]), 0.0);
        let cl = Lti::from_ss(feedback(&g.ss, &h3, -1.0).unwrap());
        let got = cl.freqresp(&[0.7]).unwrap()[0];
        let s = Complex64::new(0.0, 0.7);
        let want = s.add(Complex64::real(2.0)).div(s.scale(4.0).add(Complex64::real(7.0)));
        assert!(got.sub(want).magnitude() < 1e-15);
        // an algebraic loop is refused: G = 1 (static), H = 1, positive feedback
        assert!(feedback(&unity, &unity, 1.0).unwrap_err().contains("algebraic"));
        assert!(feedback(&g.ss, &h3, 0.5).unwrap_err().contains("sign"));
    }

    // ---- discretization

    #[test]
    fn c2d_zoh_of_a_lag_is_exp_minus_dt_over_tau() {
        let (tau, h) = (0.4, 0.1);
        let d = c2d(&tf(&[1.0], &[tau, 1.0]).ss, h, Method::Zoh).unwrap();
        let pole = (-h / tau as f64).exp();
        close(d.a[(0, 0)], pole, 2e-16);
        // H(z) = (1 - p) / (z - p)
        let (num, den) = ss_to_tf(&d).unwrap();
        close(num[0], 1.0 - pole, 1e-15);
        close(den[1], -pole, 2e-16);
        assert_eq!(d.dt, h);
        // the discrete step response equals the continuous one at the samples
        let t = time_grid(&d, 1.0, 0).unwrap();
        let r = step(&d, &t, 0).unwrap();
        for (k, &tk) in t.iter().enumerate() {
            close(r.y[(k, 0)], 1.0 - (-tk / tau).exp(), 1e-14);
        }
    }

    #[test]
    fn tustin_maps_the_left_half_plane_into_the_unit_disc() {
        let h = 0.3;
        // stable poles of every flavour, including fast ones forward Euler would throw outside
        for (re, im) in [(-0.01, 0.0), (-1.0, 5.0), (-40.0, 0.0), (-3.0, 100.0), (-1000.0, 0.0)] {
            let sys = Lti::from_zpk(
                &[],
                &if im == 0.0 { vec![Complex64::real(re)] } else { vec![Complex64::new(re, im), Complex64::new(re, -im)] },
                1.0,
                0.0,
            )
            .unwrap();
            let d = c2d(&sys.ss, h, Method::Tustin).unwrap();
            for p in d.poles().unwrap() {
                assert!(p.magnitude() < 1.0, "s = {re}+{im}j -> |z| = {}", p.magnitude());
                // z = (1 + s h/2) / (1 - s h/2)
                let s = Complex64::new(re, im.abs() * p.im.signum());
                let want = Complex64::real(1.0).add(s.scale(h / 2.0)).div(Complex64::real(1.0).sub(s.scale(h / 2.0)));
                assert!(p.sub(want).magnitude() < 1e-9, "{p:?} vs {want:?}");
            }
        }
        // the imaginary axis lands on the circle, the right half plane outside
        let osc = c2d(&tf(&[1.0], &[1.0, 0.0, 4.0]).ss, h, Method::Tustin).unwrap();
        for p in osc.poles().unwrap() {
            close(p.magnitude(), 1.0, 1e-14);
        }
        let unstable = c2d(&tf(&[1.0], &[1.0, -2.0]).ss, h, Method::Tustin).unwrap();
        assert!(unstable.poles().unwrap()[0].magnitude() > 1.0);
        // Tustin keeps the DC gain and matches the response at the prewarped frequency
        let sys = tf(&[3.0], &[1.0, 2.0]);
        let d = Lti::from_ss(c2d(&sys.ss, h, Method::Tustin).unwrap());
        close(d.dcgain().unwrap()[(0, 0)], 1.5, 1e-15);
        let wd = 0.9; // rad/sample
        let wc = 2.0 / h * (wd / 2.0f64).tan();
        let (hd, hc) = (d.freqresp(&[wd]).unwrap()[0], sys.freqresp(&[wc]).unwrap()[0]);
        assert!(hd.sub(hc).magnitude() < 1e-14);
    }

    #[test]
    fn euler_forward_and_backward() {
        let sys = tf(&[1.0], &[1.0, 2.0]);
        let h = 0.1;
        let f = c2d(&sys.ss, h, Method::Euler).unwrap();
        close(f.a[(0, 0)], 1.0 - 2.0 * h, 1e-16);
        close(f.b[(0, 0)], h, 1e-16);
        let b = c2d(&sys.ss, h, Method::Backward).unwrap();
        close(b.a[(0, 0)], 1.0 / (1.0 + 2.0 * h), 1e-16);
        // forward Euler loses stability once dt > 2/|pole|
        assert!(c2d(&sys.ss, 1.5, Method::Euler).unwrap().poles().unwrap()[0].magnitude() > 1.0);
        assert!(c2d(&f, h, Method::Zoh).is_err());
        assert!(c2d(&sys.ss, 0.0, Method::Zoh).is_err());
    }

    #[test]
    fn d2c_inverts_c2d() {
        let sys = tf(&[1.0, 3.0], &[1.0, 2.0, 5.0]);
        for method in [Method::Zoh, Method::Tustin] {
            let back = d2c(&c2d(&sys.ss, 0.05, method).unwrap(), method).unwrap();
            assert_eq!(back.dt, 0.0);
            for (g, w) in back.a.d.iter().zip(&sys.ss.a.d).chain(back.b.d.iter().zip(&sys.ss.b.d)) {
                close(*g, *w, 1e-10);
            }
            let (num, den) = ss_to_tf(&back).unwrap();
            for (g, w) in num.iter().zip(&[1.0, 3.0]).chain(den.iter().zip(&[1.0, 2.0, 5.0])) {
                close(*g, *w, 1e-10);
            }
        }
        // a pole at z = -0.5 has no real zero-order-hold preimage
        let neg = Lti::from_tf(&[1.0], &[1.0, 0.5], 0.1).unwrap();
        assert!(d2c(&neg.ss, Method::Zoh).unwrap_err().contains("no real logarithm"));
        assert!(d2c(&neg.ss, Method::Euler).is_err());
        assert!(d2c(&sys.ss, Method::Zoh).unwrap_err().contains("already continuous"));
        let at_minus_one = Lti::from_tf(&[1.0], &[1.0, 1.0], 0.1).unwrap();
        assert!(d2c(&at_minus_one.ss, Method::Tustin).unwrap_err().contains("z = -1"));
    }

    // ---- bode and margins

    #[test]
    fn bode_of_a_lag_and_phase_branches() {
        let sys = tf(&[1.0], &[1.0, 1.0]);
        let b = bode(&sys, &[0.01, 1.0, 100.0]).unwrap();
        close(b.mag_db[1], -10.0 * 2f64.log10(), 1e-13);
        close(b.phase_deg[1], -45.0, 1e-13);
        close(b.phase_deg[2], -(100f64.atan().to_degrees()), 1e-12);
        // three integrators: -270 degrees, not the principal value +90
        let b = bode(&tf(&[1.0], &[1.0, 0.0, 0.0, 0.0]), &[0.5, 2.0]).unwrap();
        close(b.phase_deg[0], -270.0, 1e-12);
        close(b.phase_deg[1], -270.0, 1e-12);
        close(b.mag_db[1], -60.0 * 2f64.log10(), 1e-12);
        // negative gain: 180
        close(bode(&tf(&[-2.0], &[1.0]), &[1.0]).unwrap().phase_deg[0], 180.0, 1e-12);
        // the phase of 1/(s(s+1)(s+2)) runs from -90 to -270 without a jump
        let l = tf(&[1.0], &[1.0, 3.0, 2.0, 0.0]);
        let w = default_frequencies(&l).unwrap();
        assert!(w[0] <= 0.1 && *w.last().unwrap() >= 20.0 && w.len() >= 200, "{} .. {}", w[0], w.last().unwrap());
        let b = bode(&l, &w).unwrap();
        assert!(b.phase_deg[0] < -90.0 && *b.phase_deg.last().unwrap() > -270.0);
        assert!(*b.phase_deg.last().unwrap() < -260.0, "the phase must pass -180 without wrapping");
        for k in 0..w.len() {
            close(b.phase_deg[k], -90.0 - w[k].atan().to_degrees() - (w[k] / 2.0).atan().to_degrees(), 1e-10);
        }
        // the same curve through the realization
        let bs = bode(&Lti::from_ss(l.ss.clone()), &w).unwrap();
        for k in 0..w.len() {
            close(bs.phase_deg[k], b.phase_deg[k], 1e-6);
            close(bs.mag_db[k], b.mag_db[k], 1e-8);
        }
        // discrete default grid ends at Nyquist
        let d = Lti::from_tf(&[1.0], &[1.0, -0.5], 1.0).unwrap();
        let w = default_frequencies(&d).unwrap();
        close(*w.last().unwrap(), PI, 1e-12);
    }

    #[test]
    fn margins_of_the_textbook_loop() {
        // L = 1 / (s (s+1)(s+2)): gain margin 6 at w = sqrt(2)
        let l = tf(&[1.0], &[1.0, 3.0, 2.0, 0.0]);
        let m = margins(&l).unwrap();
        close(m.gm, 6.0, 1e-12);
        close(20.0 * m.gm.log10(), 15.563, 1e-3);
        close(m.w_gm, 2f64.sqrt(), 1e-13);
        // gain crossover: w^2 (w^2+1)(w^2+4) = 1
        let w = m.w_pm;
        close(w * w * (w * w + 1.0) * (w * w + 4.0), 1.0, 1e-12);
        close(m.pm, 180.0 - 90.0 - w.atan().to_degrees() - (w / 2.0).atan().to_degrees(), 1e-10);
        close(m.pm, 53.4, 0.1);
        assert!(m.stable);
        // the same loop as a realization and from zeros/poles/gain
        for other in [
            Lti::from_ss(l.ss.clone()),
            Lti::from_zpk(&[], &[Complex64::real(0.0), Complex64::real(-1.0), Complex64::real(-2.0)], 1.0, 0.0).unwrap(),
        ] {
            let m2 = margins(&other).unwrap();
            close(m2.gm, 6.0, 1e-9);
            close(m2.w_gm, 2f64.sqrt(), 1e-9);
            close(m2.pm, m.pm, 1e-8);
        }
        // seven times the gain is past the margin: unstable, gm < 1
        let m = margins(&tf(&[7.0], &[1.0, 3.0, 2.0, 0.0])).unwrap();
        close(m.gm, 6.0 / 7.0, 1e-12);
        assert!(!m.stable && m.pm < 0.0);
        // a lag never reaches -180 and (gain 0.5) never reaches 0 dB
        let m = margins(&tf(&[0.5], &[1.0, 1.0])).unwrap();
        assert!(m.gm.is_infinite() && m.w_gm.is_nan() && m.pm.is_infinite() && m.w_pm.is_nan() && m.stable);
        // an integrator: phase margin 90 at w = 1
        let m = margins(&tf(&[1.0], &[1.0, 0.0])).unwrap();
        close(m.pm, 90.0, 1e-12);
        close(m.w_pm, 1.0, 1e-14);
        assert!(m.gm.is_infinite());
    }

    #[test]
    fn margins_of_a_discrete_loop() {
        // L(z) = k / (z (z - 1)): at w, |L| and phase are closed-form.
        // Phase = -w - (pi/2 + w/2) = -pi  =>  w = pi/3, where |L| = k / (2 sin(w/2)) = k.
        let k = 0.4;
        let l = Lti::from_tf(&[k], &[1.0, -1.0, 0.0], 1.0).unwrap();
        let m = margins(&l).unwrap();
        close(m.w_gm, PI / 3.0, 1e-9);
        close(m.gm, 1.0 / k, 1e-9);
        // gain crossover: 2 sin(w/2) = k
        let w = 2.0 * (k / 2.0f64).asin();
        close(m.w_pm, w, 1e-9);
        close(m.pm, 180.0 - (w + PI / 2.0 + w / 2.0).to_degrees(), 1e-7);
        assert!(m.stable);
        assert!(!margins(&Lti::from_tf(&[1.2], &[1.0, -1.0, 0.0], 1.0).unwrap()).unwrap().stable);
    }
}
