//! Dense, column-major **complex** matrices — the complex counterpart of
//! [`crate::matrix::Matrix`].
//!
//! Mirrors the real matrix's API (constructors, broadcast, `matmul`,
//! transpose/conjugate-transpose, axis reductions) over [`Complex64`] entries,
//! so real and complex code share one mental model. Reuses
//! [`crate::matrix::ShapeError`] for shape diagnostics — the failure modes
//! (broadcast mismatch, bad matmul dims, ragged rows, out-of-bounds index) are
//! identical in shape to the real case.

use crate::matrix::ShapeError;
use crate::Complex64;
use std::fmt::{self, Display, Formatter};

/// A dense complex matrix stored column-major.
#[derive(Clone, Debug, PartialEq)]
pub struct CMatrix {
    rows: usize,
    cols: usize,
    data: Vec<Complex64>,
}

impl CMatrix {
    pub fn from_col_major(rows: usize, cols: usize, data: Vec<Complex64>) -> Self {
        assert_eq!(rows * cols, data.len(), "column-major length mismatch");
        CMatrix { rows, cols, data }
    }

    pub fn filled(rows: usize, cols: usize, value: Complex64) -> Self {
        CMatrix {
            rows,
            cols,
            data: vec![value; rows * cols],
        }
    }

    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self::filled(rows, cols, Complex64::new(0.0, 0.0))
    }

    /// Build from row-major nested rows, the shape a complex matrix literal
    /// `[1+2i, 3; 4, 5-1i]` produces. Rejects ragged rows.
    pub fn from_rows(rows: &[Vec<Complex64>]) -> Result<Self, ShapeError> {
        let nrows = rows.len();
        if nrows == 0 {
            return Ok(CMatrix::zeros(0, 0));
        }
        let ncols = rows[0].len();
        for r in rows {
            if r.len() != ncols {
                return Err(ShapeError::RaggedRows {
                    expected: ncols,
                    found: r.len(),
                });
            }
        }
        let mut data = vec![Complex64::new(0.0, 0.0); nrows * ncols];
        for (r, row) in rows.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                data[r + c * nrows] = v;
            }
        }
        Ok(CMatrix { rows: nrows, cols: ncols, data })
    }

    /// A real [`crate::matrix::Matrix`] lifted to complex (zero imaginary part).
    pub fn from_real(m: &crate::matrix::Matrix) -> Self {
        CMatrix {
            rows: m.rows(),
            cols: m.cols(),
            data: m.as_slice().iter().copied().map(Complex64::real).collect(),
        }
    }

    pub fn from_column(values: &[Complex64]) -> Self {
        CMatrix {
            rows: values.len(),
            cols: 1,
            data: values.to_vec(),
        }
    }

    pub fn from_row(values: &[Complex64]) -> Self {
        CMatrix {
            rows: 1,
            cols: values.len(),
            data: values.to_vec(),
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn shape(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn as_slice(&self) -> &[Complex64] {
        &self.data
    }
    pub fn into_vec(self) -> Vec<Complex64> {
        self.data
    }
    pub fn is_scalar(&self) -> bool {
        self.data.len() == 1
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Result<Complex64, ShapeError> {
        if row >= self.rows || col >= self.cols {
            return Err(ShapeError::Index { row, col, rows: self.rows, cols: self.cols });
        }
        Ok(self.data[row + col * self.rows])
    }

    #[inline]
    pub fn set(&mut self, row: usize, col: usize, value: Complex64) -> Result<(), ShapeError> {
        if row >= self.rows || col >= self.cols {
            return Err(ShapeError::Index { row, col, rows: self.rows, cols: self.cols });
        }
        self.data[row + col * self.rows] = value;
        Ok(())
    }

    pub fn row_vec(&self, r: usize) -> Result<Vec<Complex64>, ShapeError> {
        if r >= self.rows {
            return Err(ShapeError::Index { row: r, col: 0, rows: self.rows, cols: self.cols });
        }
        Ok((0..self.cols).map(|c| self.data[r + c * self.rows]).collect())
    }

    pub fn col_vec(&self, c: usize) -> Result<Vec<Complex64>, ShapeError> {
        if c >= self.cols {
            return Err(ShapeError::Index { row: 0, col: c, rows: self.rows, cols: self.cols });
        }
        Ok(self.data[c * self.rows..(c + 1) * self.rows].to_vec())
    }

    /// Plain transpose (no conjugation) — MATLAB `.'`.
    pub fn transpose(&self) -> CMatrix {
        let mut out = CMatrix::zeros(self.cols, self.rows);
        for c in 0..self.cols {
            for r in 0..self.rows {
                out.data[c + r * self.cols] = self.data[r + c * self.rows];
            }
        }
        out
    }

    /// Conjugate (Hermitian) transpose — MATLAB `'`.
    pub fn ctranspose(&self) -> CMatrix {
        let mut out = self.transpose();
        for v in out.data.iter_mut() {
            *v = v.conj();
        }
        out
    }

    pub fn map<F: Fn(Complex64) -> Complex64>(&self, f: F) -> CMatrix {
        CMatrix {
            rows: self.rows,
            cols: self.cols,
            data: self.data.iter().copied().map(f).collect(),
        }
    }

    pub fn reshape(&self, rows: usize, cols: usize) -> Result<CMatrix, ShapeError> {
        if rows * cols != self.data.len() {
            return Err(ShapeError::Reshape { from: self.data.len(), to: (rows, cols) });
        }
        Ok(CMatrix { rows, cols, data: self.data.clone() })
    }

    /// Broadcast-tile this matrix to an explicit target shape, used by the
    /// `value as matrix(r, c)` reshape contract (§8/§14), mirroring
    /// [`crate::matrix::Matrix::broadcast_to`].
    pub fn broadcast_to(&self, rows: usize, cols: usize) -> Result<CMatrix, ShapeError> {
        let template = CMatrix::zeros(rows, cols);
        self.broadcast(&template, |a, _| a)
    }

    /// Broadcast-combine two complex matrices with a binary op (same rule as
    /// the real [`crate::matrix::Matrix::broadcast`]).
    pub fn broadcast<F: Fn(Complex64, Complex64) -> Complex64>(
        &self,
        other: &CMatrix,
        f: F,
    ) -> Result<CMatrix, ShapeError> {
        let rows = broadcast_dim(self.rows, other.rows)
            .ok_or(ShapeError::Broadcast { left: self.shape(), right: other.shape() })?;
        let cols = broadcast_dim(self.cols, other.cols)
            .ok_or(ShapeError::Broadcast { left: self.shape(), right: other.shape() })?;
        let mut data = vec![Complex64::new(0.0, 0.0); rows * cols];
        for c in 0..cols {
            let lc = if self.cols == 1 { 0 } else { c };
            let rc = if other.cols == 1 { 0 } else { c };
            for r in 0..rows {
                let lr = if self.rows == 1 { 0 } else { r };
                let rr = if other.rows == 1 { 0 } else { r };
                let a = self.data[lr + lc * self.rows];
                let b = other.data[rr + rc * other.rows];
                data[r + c * rows] = f(a, b);
            }
        }
        Ok(CMatrix { rows, cols, data })
    }

    /// Ordinary complex matrix product `(m, k) * (k, n) -> (m, n)`.
    pub fn matmul(&self, other: &CMatrix) -> Result<CMatrix, ShapeError> {
        if self.cols != other.rows {
            return Err(ShapeError::MatMul { left: self.shape(), right: other.shape() });
        }
        let (m, k, n) = (self.rows, self.cols, other.cols);
        let mut out = CMatrix::zeros(m, n);
        for c in 0..n {
            for p in 0..k {
                let b = other.data[p + c * other.rows];
                for r in 0..m {
                    let a = self.data[r + p * self.rows];
                    out.data[r + c * m] = out.data[r + c * m].add(a.mul(b));
                }
            }
        }
        Ok(out)
    }

    pub fn sum_axis(&self, axis: usize) -> Result<CMatrix, ShapeError> {
        match axis {
            0 => {
                let mut out = vec![Complex64::new(0.0, 0.0); self.cols];
                for c in 0..self.cols {
                    let mut acc = Complex64::new(0.0, 0.0);
                    for r in 0..self.rows {
                        acc = acc.add(self.data[r + c * self.rows]);
                    }
                    out[c] = acc;
                }
                Ok(CMatrix::from_row(&out))
            }
            1 => {
                let mut out = vec![Complex64::new(0.0, 0.0); self.rows];
                for c in 0..self.cols {
                    for r in 0..self.rows {
                        out[r] = out[r].add(self.data[r + c * self.rows]);
                    }
                }
                Ok(CMatrix::from_column(&out))
            }
            other => Err(ShapeError::Axis(other)),
        }
    }
}

/// Errors from complex-matrix linear algebra (inversion).
#[derive(Clone, Debug, PartialEq)]
pub enum CLinalgError {
    NotSquare { rows: usize, cols: usize },
    Singular,
    Empty,
    /// `cholesky` on a matrix that is not positive-definite.
    NotPositiveDefinite,
    /// `eigh` on a matrix that is not Hermitian. Checked rather than
    /// assumed: a Hermitian solver handed a general matrix returns real
    /// numbers that mean nothing, which is worse than an error.
    NotHermitian,
}

impl Display for CLinalgError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSquare { rows, cols } => {
                write!(f, "matrix must be square to invert (got {rows}x{cols})")
            }
            Self::Singular => write!(f, "matrix is singular (no inverse exists)"),
            Self::Empty => write!(f, "complex linear algebra requires a non-empty matrix"),
            Self::NotPositiveDefinite => {
                write!(f, "matrix is not Hermitian positive-definite")
            }
            Self::NotHermitian => write!(f, "matrix is not Hermitian"),
        }
    }
}
impl std::error::Error for CLinalgError {}

/// Complex matrix inverse via Gauss–Jordan elimination with partial pivoting
/// on magnitude. A portable reference implementation (`O(n^3)`, no external
/// dependency) — adequate for the small phase/tone matrices this language
/// targets; a BLAS/LAPACK-backed complex solve is a later acceleration.
pub fn inverse(m: &CMatrix) -> Result<CMatrix, CLinalgError> {
    let (n, cols) = m.shape();
    if n != cols {
        return Err(CLinalgError::NotSquare { rows: n, cols });
    }
    if n == 0 {
        return Ok(CMatrix::zeros(0, 0));
    }
    // augmented [A | I], row-major scratch for readable pivoting
    let mut aug: Vec<Vec<Complex64>> = (0..n)
        .map(|r| {
            let mut row = m.row_vec(r).unwrap();
            for c in 0..n {
                row.push(if c == r { Complex64::new(1.0, 0.0) } else { Complex64::new(0.0, 0.0) });
            }
            row
        })
        .collect();

    for col in 0..n {
        // partial pivot: largest magnitude in this column, at or below `col`
        let pivot = (col..n)
            .max_by(|&a, &b| aug[a][col].magnitude().total_cmp(&aug[b][col].magnitude()))
            .unwrap();
        if aug[pivot][col].magnitude() < 1e-14 {
            return Err(CLinalgError::Singular);
        }
        aug.swap(col, pivot);

        let p = aug[col][col];
        for v in aug[col].iter_mut() {
            *v = v.div(p);
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let factor = aug[r][col];
            if factor.magnitude() == 0.0 {
                continue;
            }
            for c in 0..2 * n {
                let sub = factor.mul(aug[col][c]);
                aug[r][c] = aug[r][c].sub(sub);
            }
        }
    }

    let mut data = vec![Complex64::new(0.0, 0.0); n * n];
    for r in 0..n {
        for c in 0..n {
            data[r + c * n] = aug[r][n + c];
        }
    }
    Ok(CMatrix::from_col_major(n, n, data))
}

fn broadcast_dim(a: usize, b: usize) -> Option<usize> {
    if a == b {
        Some(a)
    } else if a == 1 {
        Some(b)
    } else if b == 1 {
        Some(a)
    } else {
        None
    }
}

impl Display for CMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for r in 0..self.rows {
            if r > 0 {
                write!(f, "; ")?;
            }
            for c in 0..self.cols {
                if c > 0 {
                    write!(f, ", ")?;
                }
                let v = self.data[r + c * self.rows];
                write!(f, "{}+{}i", v.re, v.im)?;
            }
        }
        write!(f, "]")
    }
}

// ── Complex dense linear algebra ────────────────────────────────────────
//
// Impedance work is complex-matrix work by construction: a transfer
// function, a design matrix of tones, a Kramers-Kronig kernel are all
// complex, and every one of them wants a decomposition at some point.
// Until these existed the only complex matrix operations Qu had were
// `+`, matmul, transpose and `inverse`, so a port that got as far as
// assembling its matrix then had nowhere to go.
//
// Built on nalgebra's `Complex<f64>` implementations rather than
// hand-rolled: LU, QR and Cholesky are already there and already tested,
// and a second implementation of a pivoting strategy is a second place
// for it to be subtly wrong.

fn to_na(m: &CMatrix) -> nalgebra::DMatrix<nalgebra::Complex<f64>> {
    nalgebra::DMatrix::from_iterator(
        m.rows(),
        m.cols(),
        m.as_slice().iter().map(|z| nalgebra::Complex::new(z.re, z.im)),
    )
}

fn from_na(m: &nalgebra::DMatrix<nalgebra::Complex<f64>>) -> CMatrix {
    CMatrix::from_col_major(
        m.nrows(),
        m.ncols(),
        m.as_slice().iter().map(|z| Complex64::new(z.re, z.im)).collect(),
    )
}

fn require_square(m: &CMatrix) -> Result<usize, CLinalgError> {
    let (r, c) = m.shape();
    if m.is_empty() {
        return Err(CLinalgError::Empty);
    }
    if r != c {
        return Err(CLinalgError::NotSquare { rows: r, cols: c });
    }
    Ok(r)
}

/// `det(A)` for a complex square matrix, via LU with partial pivoting.
pub fn det(m: &CMatrix) -> Result<Complex64, CLinalgError> {
    require_square(m)?;
    let d = to_na(m).determinant();
    Ok(Complex64::new(d.re, d.im))
}

/// Frobenius norm — the square root of the summed squared magnitudes,
/// which is what `numpy.linalg.norm` returns for a matrix by default.
pub fn norm(m: &CMatrix) -> f64 {
    m.as_slice()
        .iter()
        .map(|z| z.re * z.re + z.im * z.im)
        .sum::<f64>()
        .sqrt()
}

/// `A = L U`, with the row permutation already applied to `L` so that
/// `L * U == A` directly — the form a caller checking the factorization
/// expects, rather than one that only holds up to a permutation they were
/// never handed.
pub struct CLu {
    pub l: CMatrix,
    pub u: CMatrix,
}

pub fn lu(m: &CMatrix) -> Result<CLu, CLinalgError> {
    require_square(m)?;
    let (p, l, u) = to_na(m).lu().unpack();
    let mut lp = l;
    p.inv_permute_rows(&mut lp);
    Ok(CLu { l: from_na(&lp), u: from_na(&u) })
}

/// `A = Q R` with `Q` unitary, by Householder reflections.
pub struct CQr {
    pub q: CMatrix,
    pub r: CMatrix,
}

pub fn qr(m: &CMatrix) -> Result<CQr, CLinalgError> {
    if m.is_empty() {
        return Err(CLinalgError::Empty);
    }
    let (q, r) = to_na(m).qr().unpack();
    Ok(CQr { q: from_na(&q), r: from_na(&r) })
}

/// Cholesky `A = L L^H` for a Hermitian positive-definite `A`.
///
/// Errors rather than returning a NaN-filled factor when `A` is not
/// positive-definite: for a covariance or a Gram matrix that is a real
/// numerical result about the input, not something to paper over.
pub fn cholesky(m: &CMatrix) -> Result<CMatrix, CLinalgError> {
    require_square(m)?;
    to_na(m)
        .cholesky()
        .map(|c| from_na(&c.unpack()))
        .ok_or(CLinalgError::NotPositiveDefinite)
}

/// Singular values, largest first, with the 2-norm condition number and
/// the numerical rank.
///
/// The singular values of a complex matrix are real by construction,
/// which is exactly what makes them the right thing to measure
/// conditioning with.
pub struct CSvd {
    pub singular_values: Vec<f64>,
    pub condition: f64,
    pub rank: usize,
}

pub fn svd(m: &CMatrix) -> Result<CSvd, CLinalgError> {
    if m.is_empty() {
        return Err(CLinalgError::Empty);
    }
    let na = to_na(m);
    let (rows, cols) = (na.nrows(), na.ncols());
    let mut values: Vec<f64> = na.singular_values().iter().copied().collect();
    values.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let hi = values.first().copied().unwrap_or(0.0);
    let lo = values.last().copied().unwrap_or(0.0);
    // The same rank tolerance the real path uses: a singular value counts
    // once it clears eps * max(dim) * the largest one.
    let tol = f64::EPSILON * (rows.max(cols) as f64) * hi;
    let rank = values.iter().filter(|v| **v > tol).count();
    Ok(CSvd {
        singular_values: values,
        condition: if lo > 0.0 { hi / lo } else { f64::INFINITY },
        rank,
    })
}

/// Eigenvalues and eigenvectors of a HERMITIAN matrix, ascending.
///
/// Hermitian only, and deliberately so. The general complex eigenproblem
/// needs a full Schur decomposition and is a different piece of work;
/// Hermitian covers what this field asks for — a Gram matrix `A^H A`, a
/// covariance, an admittance matrix — and for those the eigenvalues are
/// real, which is the reason they are worth reporting at all.
pub struct CEigh {
    pub values: Vec<f64>,
    pub vectors: CMatrix,
}

pub fn eigh(m: &CMatrix) -> Result<CEigh, CLinalgError> {
    let n = require_square(m)?;
    // Checked, not assumed: a Hermitian solver handed a general matrix
    // returns real numbers that mean nothing, which is worse than an
    // error.
    for i in 0..n {
        for j in 0..n {
            let a = m.get(i, j).map_err(|_| CLinalgError::Empty)?;
            let b = m.get(j, i).map_err(|_| CLinalgError::Empty)?;
            if (a.re - b.re).abs() > 1e-9 * (1.0 + a.re.abs())
                || (a.im + b.im).abs() > 1e-9 * (1.0 + a.im.abs())
            {
                return Err(CLinalgError::NotHermitian);
            }
        }
    }
    let e = to_na(m).symmetric_eigen();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        e.eigenvalues[a]
            .partial_cmp(&e.eigenvalues[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let values: Vec<f64> = order.iter().map(|&i| e.eigenvalues[i]).collect();
    let mut data = Vec::with_capacity(n * n);
    for &c in &order {
        for r in 0..n {
            let z = e.eigenvectors[(r, c)];
            data.push(Complex64::new(z.re, z.im));
        }
    }
    Ok(CEigh { values, vectors: CMatrix::from_col_major(n, n, data) })
}

/// Moore-Penrose pseudo-inverse, via SVD at the same rank tolerance.
pub fn pseudo_inverse(m: &CMatrix) -> Result<CMatrix, CLinalgError> {
    if m.is_empty() {
        return Err(CLinalgError::Empty);
    }
    let na = to_na(m);
    let hi = na.singular_values().iter().copied().fold(0.0f64, f64::max);
    let tol = f64::EPSILON * (na.nrows().max(na.ncols()) as f64) * hi;
    na.svd(true, true)
        .pseudo_inverse(tol)
        .map(|p| from_na(&p))
        .map_err(|_| CLinalgError::Singular)
}

/// Least-squares solution of `A x = b` for complex `A`.
///
/// The operation known-support recovery is built on: a complex design
/// matrix of tones, a complex measurement vector, and a solution that is
/// the tone amplitudes. Square systems go through the same path — SVD
/// rather than a direct solve — because a tone matrix on a short record
/// is routinely rank-deficient, and a silent wrong answer there is worse
/// than a slower right one.
pub fn lstsq(a: &CMatrix, b: &CMatrix) -> Result<CMatrix, CLinalgError> {
    if a.is_empty() || b.is_empty() {
        return Err(CLinalgError::Empty);
    }
    if a.rows() != b.rows() {
        return Err(CLinalgError::NotSquare { rows: a.rows(), cols: b.rows() });
    }
    let p = pseudo_inverse(a)?;
    let (pn, bn) = (to_na(&p), to_na(b));
    if pn.ncols() != bn.nrows() {
        return Err(CLinalgError::NotSquare { rows: pn.ncols(), cols: bn.nrows() });
    }
    Ok(from_na(&(pn * bn)))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> Complex64 {
        Complex64::new(re, im)
    }

    #[test]
    fn from_rows_is_column_major() {
        let m = CMatrix::from_rows(&[vec![c(1.0, 1.0), c(2.0, 0.0)], vec![c(3.0, 0.0), c(4.0, -1.0)]])
            .unwrap();
        assert_eq!(m.shape(), (2, 2));
        assert_eq!(m.get(0, 1).unwrap(), c(2.0, 0.0));
        assert_eq!(m.get(1, 1).unwrap(), c(4.0, -1.0));
    }

    #[test]
    fn transpose_vs_conjugate_transpose() {
        let m = CMatrix::from_row(&[c(1.0, 2.0), c(3.0, -4.0)]); // (1,2)
        let t = m.transpose();
        assert_eq!(t.shape(), (2, 1));
        assert_eq!(t.get(0, 0).unwrap(), c(1.0, 2.0)); // no conjugation
        let ct = m.ctranspose();
        assert_eq!(ct.get(0, 0).unwrap(), c(1.0, -2.0)); // conjugated
        assert_eq!(ct.get(1, 0).unwrap(), c(3.0, 4.0));
    }

    #[test]
    fn matmul_reference_against_hand_computation() {
        // [1+i, 2] * [3; 4-i]  = (1+i)*3 + 2*(4-i) = (3+3i) + (8-2i) = 11 + i
        let a = CMatrix::from_row(&[c(1.0, 1.0), c(2.0, 0.0)]);
        let b = CMatrix::from_column(&[c(3.0, 0.0), c(4.0, -1.0)]);
        let out = a.matmul(&b).unwrap();
        assert_eq!(out.shape(), (1, 1));
        assert_eq!(out.get(0, 0).unwrap(), c(11.0, 1.0));
    }

    #[test]
    fn outer_product_via_broadcast_and_matmul() {
        // column (2,1) * row (1,2) -> (2,2) outer product
        let col = CMatrix::from_column(&[c(1.0, 1.0), c(2.0, 0.0)]);
        let row = CMatrix::from_row(&[c(1.0, 0.0), c(0.0, 1.0)]);
        let out = col.matmul(&row).unwrap();
        assert_eq!(out.shape(), (2, 2));
        // (1+i)*1 = 1+i ; (1+i)*i = -1+i
        assert_eq!(out.get(0, 0).unwrap(), c(1.0, 1.0));
        assert_eq!(out.get(0, 1).unwrap(), c(-1.0, 1.0));
    }

    #[test]
    fn broadcast_elementwise_add() {
        let a = CMatrix::from_row(&[c(1.0, 0.0), c(2.0, 0.0)]);
        let b = CMatrix::filled(1, 1, c(10.0, 0.0));
        let out = a.broadcast(&b, |x, y| x.add(y)).unwrap();
        assert_eq!(out.get(0, 0).unwrap(), c(11.0, 0.0));
        assert_eq!(out.get(0, 1).unwrap(), c(12.0, 0.0));
    }

    #[test]
    fn from_real_lifts_zero_imaginary() {
        let real = crate::matrix::Matrix::from_rows(&[vec![1.0, 2.0]]).unwrap();
        let cm = CMatrix::from_real(&real);
        assert_eq!(cm.get(0, 1).unwrap(), c(2.0, 0.0));
    }

    #[test]
    fn inverse_round_trips_to_identity() {
        // a well-conditioned complex 2x2
        let m = CMatrix::from_rows(&[vec![c(2.0, 1.0), c(0.0, 1.0)], vec![c(1.0, 0.0), c(1.0, -1.0)]])
            .unwrap();
        let inv = inverse(&m).unwrap();
        let id = m.matmul(&inv).unwrap();
        for r in 0..2 {
            for col in 0..2 {
                let v = id.get(r, col).unwrap();
                let expected = if r == col { 1.0 } else { 0.0 };
                assert!((v.re - expected).abs() < 1e-9, "re mismatch at {r},{col}: {v:?}");
                assert!(v.im.abs() < 1e-9, "im mismatch at {r},{col}: {v:?}");
            }
        }
    }

    #[test]
    fn inverse_rejects_non_square_and_singular() {
        let rect = CMatrix::from_row(&[c(1.0, 0.0), c(2.0, 0.0)]);
        assert!(matches!(inverse(&rect), Err(CLinalgError::NotSquare { .. })));
        let singular = CMatrix::from_rows(&[vec![c(1.0, 0.0), c(2.0, 0.0)], vec![c(2.0, 0.0), c(4.0, 0.0)]])
            .unwrap();
        assert!(matches!(inverse(&singular), Err(CLinalgError::Singular)));
    }

    #[test]
    fn sum_axis_matches_real_case_shape() {
        let m = CMatrix::from_rows(&[vec![c(1.0, 1.0), c(2.0, 0.0)], vec![c(3.0, 0.0), c(4.0, 1.0)]])
            .unwrap();
        let s0 = m.sum_axis(0).unwrap();
        assert_eq!(s0.shape(), (1, 2));
        assert_eq!(s0.get(0, 0).unwrap(), c(4.0, 1.0));
        let s1 = m.sum_axis(1).unwrap();
        assert_eq!(s1.shape(), (2, 1));
        assert_eq!(s1.get(0, 0).unwrap(), c(3.0, 1.0));
    }
}
