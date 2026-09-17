//! Advanced dense linear algebra with explicit numerical diagnostics.
//!
//! These functions are the portable reference semantics for `pinv` and least
//! squares. Provider backends (BLAS/LAPACK/GPU) must satisfy the same evals.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use nalgebra::DMatrix;

use crate::matrix::{Matrix, ShapeError};

#[derive(Clone, Debug, PartialEq)]
pub enum LinalgError {
    EmptyMatrix,
    NonFiniteInput,
    InvalidTolerance(f64),
    Decomposition(&'static str),
    Shape(ShapeError),
    /// A decomposition that requires a square input (`cholesky`) got a
    /// non-square one.
    NotSquare { rows: usize, cols: usize },
}

impl Display for LinalgError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMatrix => write!(f, "linear algebra requires a non-empty matrix"),
            Self::NonFiniteInput => write!(f, "linear algebra input contains NaN or infinity"),
            Self::InvalidTolerance(value) => write!(
                f,
                "SVD tolerance must be finite and non-negative, got {value}"
            ),
            Self::Decomposition(part) => write!(f, "{part} decomposition failed"),
            Self::Shape(error) => Display::fmt(error, f),
            Self::NotSquare { rows, cols } => {
                write!(f, "expected a square matrix, got {rows}x{cols}")
            }
        }
    }
}

impl Error for LinalgError {}

impl From<ShapeError> for LinalgError {
    fn from(value: ShapeError) -> Self {
        Self::Shape(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PseudoInverse {
    pub matrix: Matrix,
    pub rank: usize,
    pub tolerance: f64,
    pub condition_number: f64,
    pub singular_values: Vec<f64>,
}

/// Moore-Penrose pseudoinverse from an SVD. `relative_tolerance=None` uses
/// `eps * max(m,n)`, matching standard numerical-library practice.
pub fn pseudo_inverse(
    input: &Matrix,
    relative_tolerance: Option<f64>,
) -> Result<PseudoInverse, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let relative =
        relative_tolerance.unwrap_or(f64::EPSILON * input.rows().max(input.cols()) as f64);
    if !relative.is_finite() || relative < 0.0 {
        return Err(LinalgError::InvalidTolerance(relative));
    }

    let source = DMatrix::from_column_slice(input.rows(), input.cols(), input.as_slice());
    let svd = source.svd(true, true);
    let u = svd.u.ok_or(LinalgError::Decomposition("U"))?;
    let v_t = svd.v_t.ok_or(LinalgError::Decomposition("V transpose"))?;
    let singular_values: Vec<f64> = svd.singular_values.iter().copied().collect();
    let largest = singular_values.first().copied().unwrap_or(0.0);
    let tolerance = relative * largest;
    let rank = singular_values
        .iter()
        .filter(|&&value| value > tolerance)
        .count();
    let smallest_retained = singular_values
        .iter()
        .copied()
        .filter(|&value| value > tolerance)
        .fold(f64::INFINITY, f64::min);
    let condition_number = if rank == 0 {
        f64::INFINITY
    } else {
        largest / smallest_retained
    };

    // nalgebra returns the thin factors U(m,k), S(k), Vt(k,n), k=min(m,n).
    // Therefore A+ = V(n,k) * S+(k,k) * Ut(k,m).
    let thin = singular_values.len();
    let mut sigma_inverse = DMatrix::zeros(thin, thin);
    for (index, &singular) in singular_values.iter().enumerate() {
        if singular > tolerance {
            sigma_inverse[(index, index)] = 1.0 / singular;
        }
    }
    let inverse = v_t.transpose() * sigma_inverse * u.transpose();
    Ok(PseudoInverse {
        matrix: Matrix::from_col_major(input.cols(), input.rows(), inverse.as_slice().to_vec()),
        rank,
        tolerance,
        condition_number,
        singular_values,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct SvdResult {
    /// `(m, k)`, `k = min(m,n)` — the thin left singular vectors.
    pub u: Matrix,
    /// Descending, length `k`.
    pub singular_values: Vec<f64>,
    /// `(k, n)` — the thin right singular vectors, transposed (rows are
    /// principal directions in `n`-dimensional column space, matching
    /// scikit-learn's own `components_` convention for downstream PCA use).
    pub v_t: Matrix,
}

/// The thin SVD `A = U * diag(s) * V^T` — the same decomposition
/// [`pseudo_inverse`] already computes internally, exposed directly for
/// callers that need the actual factors (e.g. PCA's principal directions),
/// not just the pseudoinverse built from them.
pub fn svd(input: &Matrix) -> Result<SvdResult, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let source = DMatrix::from_column_slice(input.rows(), input.cols(), input.as_slice());
    let result = source.svd(true, true);
    let u = result.u.ok_or(LinalgError::Decomposition("U"))?;
    let v_t = result.v_t.ok_or(LinalgError::Decomposition("V transpose"))?;
    let singular_values: Vec<f64> = result.singular_values.iter().copied().collect();
    let (ur, uc) = u.shape();
    let (vr, vc) = v_t.shape();
    Ok(SvdResult {
        u: Matrix::from_col_major(ur, uc, u.as_slice().to_vec()),
        singular_values,
        v_t: Matrix::from_col_major(vr, vc, v_t.as_slice().to_vec()),
    })
}

/// Fast path for [`least_squares`]: a direct LU solve for a square `A`,
/// `O(n^3/3)` with a small constant versus the SVD pseudoinverse's
/// `O(n^3)` with a much larger one (multiple bidiagonalization + iterative
/// QR sweeps per factor) -- the same LU-based dispatch
/// `numpy.linalg.solve`/MATLAB's `\` already use for this, exact case.
///
/// Returns `Ok(None)` (not an error) rather than falling back itself,
/// whenever a direct solve isn't the right tool, so the caller can retry
/// with the general-purpose SVD pseudoinverse:
/// - `A` isn't square (rectangular least-squares needs SVD's minimum-norm
///   solution, not a direct solve).
/// - `observations`'s row count doesn't match `A`'s (an error either way;
///   deferring to [`pseudo_inverse`]/`matmul` keeps one error message).
/// - LU factorization (partial pivoting) still finds a zero pivot, i.e.
///   `A` is genuinely singular -- nalgebra's `LU::solve` reports this by
///   returning `None` itself, the same failure-detection [`lu`] already
///   relies on rather than an explicit condition-number estimate.
fn try_lu_solve(coefficients: &Matrix, observations: &Matrix) -> Option<Matrix> {
    let (rows, cols) = coefficients.shape();
    if rows != cols || rows == 0 || observations.rows() != rows {
        return None;
    }
    let source = DMatrix::from_column_slice(rows, cols, coefficients.as_slice());
    let rhs = DMatrix::from_column_slice(
        observations.rows(),
        observations.cols(),
        observations.as_slice(),
    );
    let solution = source.lu().solve(&rhs)?;
    let (sol_rows, sol_cols) = solution.shape();
    Some(Matrix::from_col_major(
        sol_rows,
        sol_cols,
        solution.as_slice().to_vec(),
    ))
}

/// Minimum-norm least-squares solution `x = pinv(A) * b`.
///
/// For a square, finite, non-empty `A` solved with the default tolerance,
/// tries the much cheaper [`try_lu_solve`] direct solve first (this covers
/// the `\` operator and the overwhelmingly common "plain square system"
/// case); falls back to the full SVD-based [`pseudo_inverse`] whenever
/// that fast path declines (non-square `A`, or a singular/zero-pivot `A`)
/// or whenever a caller supplies an explicit `relative_tolerance` (a
/// signal they want SVD's tolerance-based rank handling, e.g. treating a
/// technically-invertible but severely ill-conditioned `A` as reduced
/// rank -- `eis.rs`'s regularized DRT fit is exactly this case).
pub fn least_squares(
    coefficients: &Matrix,
    observations: &Matrix,
    relative_tolerance: Option<f64>,
) -> Result<Matrix, LinalgError> {
    if relative_tolerance.is_none()
        && !coefficients.is_empty()
        && coefficients.as_slice().iter().all(|value| value.is_finite())
    {
        if let Some(solution) = try_lu_solve(coefficients, observations) {
            return Ok(solution);
        }
    }
    let inverse = pseudo_inverse(coefficients, relative_tolerance)?;
    Ok(inverse.matrix.matmul(observations)?)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CholeskyResult {
    /// Lower-triangular `L` such that `A = L * L^T` — the same `(n, n)`
    /// shape as the input, upper triangle all zeros.
    pub l: Matrix,
}

/// Cholesky decomposition `A = L * L^T` of a symmetric positive-definite
/// `A` (a covariance matrix is the common case this crate cares about —
/// `mvnpdf`/Kalman-filter covariance updates/sampling correlated Gaussians
/// all build on this). Only `A`'s lower triangle is read, the standard
/// convention, so a caller who only filled in one triangle of a symmetric
/// matrix still gets the right answer. Returns
/// [`LinalgError::Decomposition`] if `A` isn't positive-definite —
/// nalgebra's own check — which is the right signal for a covariance-like
/// matrix with a real numerical problem, not something to silently paper
/// over with e.g. a clamped square root.
pub fn cholesky(input: &Matrix) -> Result<CholeskyResult, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let (rows, cols) = input.shape();
    if rows != cols {
        return Err(LinalgError::NotSquare { rows, cols });
    }
    let source = DMatrix::from_column_slice(rows, cols, input.as_slice());
    let decomposition = source
        .cholesky()
        .ok_or(LinalgError::Decomposition("Cholesky"))?;
    let l = decomposition.l();
    Ok(CholeskyResult {
        l: Matrix::from_col_major(rows, cols, l.as_slice().to_vec()),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct QrResult {
    /// `(m, min(m,n))` factor with orthonormal columns — nalgebra's thin
    /// ("economy") QR, matching MATLAB's `qr(A, 0)`/NumPy's default
    /// `mode='reduced'` rather than the square, zero-padded full form.
    pub q: Matrix,
    /// `(min(m,n), n)` upper-triangular factor, `A = Q * R`.
    pub r: Matrix,
}

/// QR decomposition `A = Q * R` via Householder reflections (nalgebra's
/// own implementation) — always succeeds for a finite input, no
/// positive-definiteness or symmetry requirement unlike [`cholesky`].
pub fn qr(input: &Matrix) -> Result<QrResult, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let (rows, cols) = input.shape();
    let source = DMatrix::from_column_slice(rows, cols, input.as_slice());
    let decomposition = source.qr();
    let q = decomposition.q();
    let r = decomposition.r();
    let (qr_rows, qr_cols) = q.shape();
    let (rr_rows, rr_cols) = r.shape();
    Ok(QrResult {
        q: Matrix::from_col_major(qr_rows, qr_cols, q.as_slice().to_vec()),
        r: Matrix::from_col_major(rr_rows, rr_cols, r.as_slice().to_vec()),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct LuResult {
    /// `(m, min(m,n))` unit-lower-triangular factor.
    pub l: Matrix,
    /// `(min(m,n), n)` upper-triangular factor.
    pub u: Matrix,
    /// `(m, m)` row-permutation matrix such that `P * A = L * U`.
    pub p: Matrix,
}

/// LU decomposition with partial pivoting, `P * A = L * U` — always
/// succeeds for a finite input (pivoting handles the reordering that
/// would otherwise fail on a zero pivot); a genuinely singular `A`
/// surfaces as a zero row in `U`; callers wanting an explicit singularity
/// check should use [`pseudo_inverse`]'s reported rank instead.
pub fn lu(input: &Matrix) -> Result<LuResult, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let (rows, cols) = input.shape();
    let source = DMatrix::from_column_slice(rows, cols, input.as_slice());
    let decomposition = source.lu();
    let l = decomposition.l();
    let u = decomposition.u();
    let mut p = DMatrix::<f64>::identity(rows, rows);
    decomposition.p().permute_rows(&mut p);
    let (l_rows, l_cols) = l.shape();
    let (u_rows, u_cols) = u.shape();
    Ok(LuResult {
        l: Matrix::from_col_major(l_rows, l_cols, l.as_slice().to_vec()),
        u: Matrix::from_col_major(u_rows, u_cols, u.as_slice().to_vec()),
        p: Matrix::from_col_major(rows, rows, p.as_slice().to_vec()),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct EigResult {
    /// Real part of each eigenvalue.
    pub values_re: Vec<f64>,
    /// Imaginary part of each eigenvalue — all zero for a symmetric input,
    /// since a real symmetric matrix's eigenvalues are always real.
    pub values_im: Vec<f64>,
    /// Eigenvectors as matrix columns, only for a symmetric input (nalgebra
    /// only computes real eigenvectors from `symmetric_eigen`; a general
    /// matrix's eigenvectors would themselves be complex-valued, which
    /// isn't computed here — `None` in that case, not an approximation).
    pub vectors: Option<Matrix>,
}

/// Eigenvalues (and, for a symmetric input, eigenvectors) of a square
/// matrix. Dispatches on symmetry (checked directly, within a relative
/// tolerance, not assumed): a symmetric `A` uses nalgebra's
/// `symmetric_eigen` (real eigenvalues, real orthonormal eigenvectors, the
/// numerically preferred algorithm when it applies); a general `A` uses
/// the Schur decomposition's eigenvalues, which may be complex, and
/// reports eigenvalues only.
pub fn eig(input: &Matrix) -> Result<EigResult, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let (rows, cols) = input.shape();
    if rows != cols {
        return Err(LinalgError::NotSquare { rows, cols });
    }
    let source = DMatrix::from_column_slice(rows, cols, input.as_slice());
    let tol = 1e-9;
    let is_symmetric = (0..rows).all(|i| {
        (0..i).all(|j| {
            let a = source[(i, j)];
            let b = source[(j, i)];
            (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs()))
        })
    });
    if is_symmetric {
        let decomposition = source.symmetric_eigen();
        let values_re = decomposition.eigenvalues.iter().copied().collect();
        let vectors = decomposition.eigenvectors;
        Ok(EigResult {
            values_re,
            values_im: vec![0.0; rows],
            vectors: Some(Matrix::from_col_major(rows, cols, vectors.as_slice().to_vec())),
        })
    } else {
        let schur = source.schur();
        let eigenvalues = schur.complex_eigenvalues();
        Ok(EigResult {
            values_re: eigenvalues.iter().map(|c| c.re).collect(),
            values_im: eigenvalues.iter().map(|c| c.im).collect(),
            vectors: None,
        })
    }
}

/// Determinant via LU decomposition (nalgebra's own implementation).
pub fn det(input: &Matrix) -> Result<f64, LinalgError> {
    if input.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if input.as_slice().iter().any(|value| !value.is_finite()) {
        return Err(LinalgError::NonFiniteInput);
    }
    let (rows, cols) = input.shape();
    if rows != cols {
        return Err(LinalgError::NotSquare { rows, cols });
    }
    let source = DMatrix::from_column_slice(rows, cols, input.as_slice());
    Ok(source.determinant())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_deficient_matrix_reports_rank() {
        let matrix = Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![2.0, 4.0, 6.0]]).unwrap();
        let inverse = pseudo_inverse(&matrix, None).unwrap();
        assert_eq!(inverse.rank, 1);
        assert_eq!(inverse.matrix.shape(), (3, 2));
    }

    #[test]
    fn tolerance_can_remove_a_tiny_singular_direction() {
        let matrix = Matrix::from_rows(&[vec![1.0, 0.0], vec![0.0, 1.0e-12]]).unwrap();
        assert_eq!(pseudo_inverse(&matrix, Some(1.0e-10)).unwrap().rank, 1);
        assert_eq!(pseudo_inverse(&matrix, Some(1.0e-14)).unwrap().rank, 2);
    }

    #[test]
    fn svd_reconstructs_the_original_matrix() {
        let matrix = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]).unwrap();
        let result = svd(&matrix).unwrap();
        assert_eq!(result.u.shape(), (3, 2));
        assert_eq!(result.v_t.shape(), (2, 2));
        assert_eq!(result.singular_values.len(), 2);
        // descending order.
        assert!(result.singular_values[0] >= result.singular_values[1]);
        // A = U * diag(s) * V^T, reconstructed within floating-point tolerance.
        let mut sigma = Matrix::zeros(2, 2);
        sigma.set(0, 0, result.singular_values[0]).unwrap();
        sigma.set(1, 1, result.singular_values[1]).unwrap();
        let reconstructed = result.u.matmul(&sigma).unwrap().matmul(&result.v_t).unwrap();
        for (a, b) in matrix.as_slice().iter().zip(reconstructed.as_slice()) {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }

    #[test]
    fn svd_rejects_an_empty_matrix() {
        assert_eq!(svd(&Matrix::zeros(0, 0)), Err(LinalgError::EmptyMatrix));
    }

    #[test]
    fn cholesky_reconstructs_a_known_positive_definite_matrix() {
        // A textbook example: A = [[4, 12, -16], [12, 37, -43], [-16, -43, 98]]
        // has the exact factor L = [[2,0,0],[6,1,0],[-8,5,3]].
        let a = Matrix::from_rows(&[
            vec![4.0, 12.0, -16.0],
            vec![12.0, 37.0, -43.0],
            vec![-16.0, -43.0, 98.0],
        ])
        .unwrap();
        let result = cholesky(&a).unwrap();
        let expected = [2.0, 6.0, -8.0, 0.0, 1.0, 5.0, 0.0, 0.0, 3.0]; // column-major
        for (got, want) in result.l.as_slice().iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        }
    }

    #[test]
    fn cholesky_reconstruction_l_times_lt_recovers_the_input() {
        let a = Matrix::from_rows(&[vec![25.0, 15.0, -5.0], vec![15.0, 18.0, 0.0], vec![-5.0, 0.0, 11.0]])
            .unwrap();
        let result = cholesky(&a).unwrap();
        let reconstructed = result.l.matmul(&result.l.transpose()).unwrap();
        for (got, want) in reconstructed.as_slice().iter().zip(a.as_slice()) {
            assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        }
    }

    #[test]
    fn cholesky_rejects_a_non_positive_definite_matrix() {
        // A symmetric but indefinite matrix (eigenvalues of opposite sign).
        let a = Matrix::from_rows(&[vec![1.0, 2.0], vec![2.0, 1.0]]).unwrap();
        assert_eq!(cholesky(&a), Err(LinalgError::Decomposition("Cholesky")));
    }

    fn assert_matrix_close(a: &Matrix, b: &Matrix, tol: f64) {
        assert_eq!(a.shape(), b.shape(), "{:?} vs {:?}", a.shape(), b.shape());
        for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
            assert!((x - y).abs() < tol, "{x} vs {y}");
        }
    }

    #[test]
    fn qr_reconstructs_a_non_square_matrix() {
        let a = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]).unwrap();
        let result = qr(&a).unwrap();
        assert_eq!(result.q.shape(), (3, 2)); // thin QR: (m, min(m,n))
        assert_eq!(result.r.shape(), (2, 2));
        let reconstructed = result.q.matmul(&result.r).unwrap();
        assert_matrix_close(&reconstructed, &a, 1e-9);
    }

    #[test]
    fn qr_produces_an_orthogonal_q() {
        let a = Matrix::from_rows(&[vec![1.0, -1.0], vec![2.0, 3.0]]).unwrap();
        let result = qr(&a).unwrap();
        let qtq = result.q.transpose().matmul(&result.q).unwrap();
        assert_matrix_close(&qtq, &Matrix::eye(2), 1e-9);
    }

    #[test]
    fn lu_satisfies_p_times_a_equals_l_times_u() {
        let a = Matrix::from_rows(&[vec![2.0, 1.0, 1.0], vec![4.0, 3.0, 3.0], vec![8.0, 7.0, 9.0]]).unwrap();
        let result = lu(&a).unwrap();
        let pa = result.p.matmul(&a).unwrap();
        let lu_product = result.l.matmul(&result.u).unwrap();
        assert_matrix_close(&pa, &lu_product, 1e-9);
    }

    #[test]
    fn eig_of_a_symmetric_matrix_matches_known_eigenvalues() {
        // [[2,0],[0,3]] has eigenvalues exactly 2 and 3, eigenvectors the
        // standard basis (up to sign) -- a textbook check, not just
        // internal self-consistency.
        let a = Matrix::from_rows(&[vec![2.0, 0.0], vec![0.0, 3.0]]).unwrap();
        let result = eig(&a).unwrap();
        let mut values = result.values_re.clone();
        values.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert!((values[0] - 2.0).abs() < 1e-9);
        assert!((values[1] - 3.0).abs() < 1e-9);
        assert!(result.values_im.iter().all(|&im| im == 0.0));
        assert!(result.vectors.is_some());
    }

    #[test]
    fn eig_reconstructs_a_symmetric_matrix_via_a_equals_v_lambda_vt() {
        let a = Matrix::from_rows(&[vec![4.0, 1.0], vec![1.0, 3.0]]).unwrap();
        let result = eig(&a).unwrap();
        let v = result.vectors.unwrap();
        let mut lambda = Matrix::zeros(2, 2);
        lambda.set(0, 0, result.values_re[0]).unwrap();
        lambda.set(1, 1, result.values_re[1]).unwrap();
        let reconstructed = v.matmul(&lambda).unwrap().matmul(&v.transpose()).unwrap();
        assert_matrix_close(&reconstructed, &a, 1e-9);
    }

    #[test]
    fn eig_of_a_non_symmetric_matrix_reports_eigenvalues_only() {
        // A 2x2 rotation-like matrix with genuinely complex eigenvalues:
        // [[0,-1],[1,0]] has eigenvalues +-i.
        let a = Matrix::from_rows(&[vec![0.0, -1.0], vec![1.0, 0.0]]).unwrap();
        let result = eig(&a).unwrap();
        assert!(result.vectors.is_none());
        for &re in &result.values_re {
            assert!(re.abs() < 1e-9, "expected zero real part, got {re}");
        }
        let mut ims: Vec<f64> = result.values_im.iter().map(|v| v.abs()).collect();
        ims.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((ims[0] - 1.0).abs() < 1e-9);
        assert!((ims[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn det_matches_a_hand_computed_value() {
        // det([[1,2],[3,4]]) = 1*4 - 2*3 = -2, by hand.
        let a = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert!((det(&a).unwrap() - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn det_of_the_identity_is_one() {
        assert!((det(&Matrix::eye(4)).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn det_rejects_a_non_square_matrix() {
        let a = Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(det(&a), Err(LinalgError::NotSquare { rows: 2, cols: 3 }));
    }

    #[test]
    fn cholesky_rejects_a_non_square_matrix() {
        let a = Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(cholesky(&a), Err(LinalgError::NotSquare { rows: 2, cols: 3 }));
    }

    #[test]
    fn cholesky_rejects_an_empty_matrix() {
        assert_eq!(cholesky(&Matrix::zeros(0, 0)), Err(LinalgError::EmptyMatrix));
    }

    #[test]
    fn least_squares_lu_fast_path_matches_svd_path_for_a_square_system() {
        // A well-conditioned 4x4 system with a known exact solution
        // (x = [1,2,3,4]), so both the new LU fast path and the old
        // SVD-based pseudoinverse path can be checked directly against
        // ground truth, and against each other.
        let a = Matrix::from_rows(&[
            vec![4.0, 1.0, 0.0, 0.0],
            vec![1.0, 5.0, 2.0, 0.0],
            vec![0.0, 2.0, 6.0, 1.0],
            vec![0.0, 0.0, 1.0, 7.0],
        ])
        .unwrap();
        let x_expected = [1.0, 2.0, 3.0, 4.0];
        let b_data: Vec<f64> = (0..4)
            .map(|r| (0..4).map(|c| a.get(r, c).unwrap() * x_expected[c]).sum())
            .collect();
        let b = Matrix::from_column(&b_data);

        // The fast path (default `None` tolerance).
        let via_lu = least_squares(&a, &b, None).unwrap();
        // Force the old SVD path directly via `pseudo_inverse`, bypassing
        // `least_squares`'s new dispatch entirely, as a snapshot of the
        // pre-fix behavior.
        let via_svd = pseudo_inverse(&a, None).unwrap().matrix.matmul(&b).unwrap();

        assert_eq!(via_lu.shape(), (4, 1));
        for (lu_val, svd_val) in via_lu.as_slice().iter().zip(via_svd.as_slice()) {
            assert!((lu_val - svd_val).abs() < 1e-9, "{lu_val} vs {svd_val}");
        }
        for (got, want) in via_lu.as_slice().iter().zip(x_expected.iter()) {
            assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        }
    }

    #[test]
    fn least_squares_falls_back_to_svd_for_a_singular_square_system() {
        // Row 2 is exactly twice row 1 -- singular, LU must decline (a
        // zero pivot after partial pivoting) and least_squares must still
        // return a finite, sane answer via the SVD fallback rather than
        // panicking or returning garbage.
        let a = Matrix::from_rows(&[
            vec![1.0, 2.0, 3.0],
            vec![2.0, 4.0, 6.0],
            vec![1.0, 0.0, 1.0],
        ])
        .unwrap();
        let b = Matrix::from_column(&[6.0, 12.0, 2.0]);

        assert!(try_lu_solve(&a, &b).is_none(), "expected LU to decline a singular system");

        let solution = least_squares(&a, &b, None).unwrap();
        assert_eq!(solution.shape(), (3, 1));
        for value in solution.as_slice() {
            assert!(value.is_finite(), "expected a finite fallback solution, got {value}");
        }
    }

    #[test]
    fn least_squares_non_square_system_is_unaffected_by_the_lu_fast_path() {
        // A rectangular (overdetermined) system: try_lu_solve must decline
        // immediately (not square), so least_squares takes exactly the
        // same SVD path it always has.
        let a = Matrix::from_rows(&[
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, 1.0],
        ])
        .unwrap();
        let b = Matrix::from_column(&[1.0, 2.0, 3.5]);

        assert!(try_lu_solve(&a, &b).is_none(), "LU must decline a non-square A");

        let via_least_squares = least_squares(&a, &b, None).unwrap();
        let via_svd = pseudo_inverse(&a, None).unwrap().matrix.matmul(&b).unwrap();
        assert_eq!(via_least_squares, via_svd);
    }

    #[test]
    fn least_squares_with_explicit_tolerance_still_uses_the_svd_path() {
        // A square, well-conditioned system, but with an explicit
        // relative_tolerance -- the fast path must not engage (a caller
        // asking for tolerance-based rank handling gets the real thing).
        let a = Matrix::from_rows(&[vec![2.0, 0.0], vec![0.0, 3.0]]).unwrap();
        let b = Matrix::from_column(&[4.0, 9.0]);
        let via_least_squares = least_squares(&a, &b, Some(1e-10)).unwrap();
        let via_svd = pseudo_inverse(&a, Some(1e-10)).unwrap().matrix.matmul(&b).unwrap();
        assert_eq!(via_least_squares, via_svd);
    }
}

/// Non-negative least squares: the `x >= 0` minimizer of `||A x - b||`.
///
/// Lawson-Hanson active set, the reference algorithm (their 1974 NNLS,
/// chapter 23), which is what `scipy.optimize.nnls` implements and what
/// this is checked against.
///
/// Why a constrained solve rather than clipping an unconstrained one at
/// zero: in a distribution-of-relaxation-times fit the unknowns are
/// resistances, and a negative resistance is not a slightly wrong answer
/// but a physically impossible one. An unconstrained fit on an
/// ill-conditioned kernel returns them freely -- a fitted distribution can
/// swing tens of milliohms either side of zero -- and clipping afterwards is not
/// the same estimate: the constraint has to be inside the optimization,
/// because excluding a variable changes what the others should be.
///
/// The active set `P` holds the indices currently allowed to be positive.
/// Each outer step admits the index with the largest positive gradient,
/// solves the unconstrained problem on `P` alone, and -- if that solution
/// went negative anywhere -- walks partway toward it and drops whatever
/// hit zero. Finite: the objective strictly decreases each outer step, so
/// no set of active indices can repeat.
pub fn nnls(
    coefficients: &Matrix,
    observations: &[f64],
    max_iterations: Option<usize>,
) -> Result<NnlsResult, LinalgError> {
    let (m, n) = coefficients.shape();
    if coefficients.is_empty() || observations.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if observations.len() != m {
        return Err(LinalgError::Shape(ShapeError::MatMul {
            left: (m, n),
            right: (observations.len(), 1),
        }));
    }
    if !coefficients.as_slice().iter().all(|v| v.is_finite())
        || !observations.iter().all(|v| v.is_finite())
    {
        return Err(LinalgError::NonFiniteInput);
    }

    let a = DMatrix::from_iterator(m, n, coefficients.as_slice().iter().copied());
    let b = DMatrix::from_column_slice(m, 1, observations);
    let mut x = DMatrix::<f64>::zeros(n, 1);
    let mut active = vec![false; n]; // true = in P, free to be positive

    // Scaled off the data, so the test means the same thing whatever the
    // units are -- an absolute epsilon would stop early on milliohms and
    // never stop on ohms.
    let scale = a.norm().max(b.norm()).max(1.0);
    let tol = 10.0 * f64::EPSILON * scale * (m.max(n) as f64);
    let max_outer = max_iterations.unwrap_or(3 * n);

    for _ in 0..max_outer {
        let w = a.transpose() * (&b - &a * &x); // negative gradient
        // The most promising index not already free. Stop when no
        // excluded variable would improve the fit by moving off zero --
        // that is the KKT condition for this problem.
        let mut best = None;
        let mut best_w = tol;
        for j in 0..n {
            if !active[j] && w[(j, 0)] > best_w {
                best_w = w[(j, 0)];
                best = Some(j);
            }
        }
        let Some(j_in) = best else { break };
        active[j_in] = true;

        // Inner loop: solve on P, and if that leaves any active variable
        // negative, move as far as feasible and release what hit zero.
        for _ in 0..(3 * n) {
            let idx: Vec<usize> = (0..n).filter(|&j| active[j]).collect();
            if idx.is_empty() {
                break;
            }
            let ap = DMatrix::from_fn(m, idx.len(), |r, c| a[(r, idx[c])]);
            // Least squares on the free set. SVD rather than normal
            // equations: the sub-problem is routinely rank-deficient
            // right after an index is admitted.
            let Some(zp) = ap.clone().svd(true, true).solve(&b, 1e-12).ok() else {
                active[j_in] = false;
                break;
            };
            if idx.iter().enumerate().all(|(c, _)| zp[(c, 0)] > tol) {
                for (c, &j) in idx.iter().enumerate() {
                    x[(j, 0)] = zp[(c, 0)];
                }
                break;
            }
            // How far toward `z` we can go before an active variable
            // would cross zero.
            let mut alpha = f64::INFINITY;
            for (c, &j) in idx.iter().enumerate() {
                if zp[(c, 0)] <= tol {
                    let denom = x[(j, 0)] - zp[(c, 0)];
                    if denom > 0.0 {
                        alpha = alpha.min(x[(j, 0)] / denom);
                    }
                }
            }
            if !alpha.is_finite() {
                alpha = 0.0;
            }
            for (c, &j) in idx.iter().enumerate() {
                x[(j, 0)] += alpha * (zp[(c, 0)] - x[(j, 0)]);
            }
            for &j in &idx {
                if x[(j, 0)] <= tol {
                    x[(j, 0)] = 0.0;
                    active[j] = false;
                }
            }
        }
    }

    let residual = (&b - &a * &x).norm();
    Ok(NnlsResult {
        x: Matrix::from_col_major(n, 1, x.as_slice().to_vec()),
        residual,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct NnlsResult {
    /// The non-negative solution, as an `n x 1` column.
    pub x: Matrix,
    /// `||A x - b||`, the 2-norm of the residual at the solution.
    pub residual: f64,
}

/// Nonlinear least squares with optional box bounds.
///
/// Named apart from the linear `least_squares` above, which solves
/// `A x = b`: these are different problems with the same English name.
///
/// Levenberg-Marquardt on a residual function the caller supplies, which
/// is the contract `scipy.optimize.least_squares` has and `curve_fit`
/// does not: a residual vector rather than a model evaluated against
/// data, so anything expressible as "make these numbers small" fits --
/// a circuit fit with a weighting, a Kramers-Kronig penalty, a
/// regularization term appended to the residual.
///
/// The Jacobian is finite-differenced with a step scaled to each
/// parameter, because the parameters in an impedance fit differ by many
/// orders of magnitude (an ohm and a farad in the same vector) and one
/// absolute step cannot serve both.
///
/// Bounds are enforced by PROJECTION: each trial point is clamped into
/// the box before it is evaluated. Not as sharp as a trust-region
/// reflective method on a solution that sits exactly on a bound, but it
/// never leaves the box, which is the property callers actually depend on
/// -- a negative capacitance evaluated once can be enough to put NaN
/// through the whole residual.
pub fn nonlinear_least_squares<F>(
    residual: F,
    start: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    max_iterations: usize,
    tolerance: f64,
) -> Result<LeastSquaresResult, LinalgError>
where
    F: Fn(&[f64]) -> Vec<f64>,
{
    let n = start.len();
    if n == 0 {
        return Err(LinalgError::EmptyMatrix);
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(LinalgError::InvalidTolerance(tolerance));
    }
    let clamp = |p: &mut Vec<f64>| {
        for j in 0..n {
            if let Some(lo) = lower {
                if j < lo.len() && p[j] < lo[j] {
                    p[j] = lo[j];
                }
            }
            if let Some(hi) = upper {
                if j < hi.len() && p[j] > hi[j] {
                    p[j] = hi[j];
                }
            }
        }
    };

    let mut p: Vec<f64> = start.to_vec();
    clamp(&mut p);
    let mut r = residual(&p);
    let m = r.len();
    if m == 0 {
        return Err(LinalgError::EmptyMatrix);
    }
    let cost = |r: &[f64]| -> f64 { r.iter().map(|v| v * v).sum::<f64>() };
    let mut c = cost(&r);
    let mut lambda = 1e-3;
    let mut iterations = 0usize;
    let mut converged = false;

    for _ in 0..max_iterations {
        iterations += 1;
        // Forward differences, step scaled to the parameter's own size.
        let mut j = DMatrix::<f64>::zeros(m, n);
        for k in 0..n {
            let h = (1e-7 * p[k].abs()).max(1e-10);
            let mut q = p.clone();
            q[k] += h;
            clamp(&mut q);
            let step = q[k] - p[k];
            if step == 0.0 {
                continue; // pinned against a bound; column stays zero
            }
            let rq = residual(&q);
            if rq.len() != m {
                return Err(LinalgError::Decomposition("residual length changed"));
            }
            for i in 0..m {
                j[(i, k)] = (rq[i] - r[i]) / step;
            }
        }

        let jt = j.transpose();
        let jtj = &jt * &j;
        let jtr = &jt * DMatrix::from_column_slice(m, 1, &r);
        let mut improved = false;
        // Up to ten damping increases before giving up on this iteration:
        // that is 10^10 on lambda, far past the point where the step is
        // gradient descent with a tiny step and nothing more will come.
        for _ in 0..10 {
            let mut aug = jtj.clone();
            for k in 0..n {
                aug[(k, k)] += lambda * (1.0 + jtj[(k, k)]);
            }
            let Ok(delta) = aug.clone().svd(true, true).solve(&jtr, 1e-14) else {
                lambda *= 10.0;
                continue;
            };
            let mut trial = p.clone();
            for k in 0..n {
                trial[k] -= delta[(k, 0)];
            }
            clamp(&mut trial);
            let rt = residual(&trial);
            let ct = cost(&rt);
            if ct.is_finite() && ct < c {
                let rel = (c - ct) / c.max(f64::MIN_POSITIVE);
                p = trial;
                r = rt;
                c = ct;
                lambda = (lambda * 0.3).max(1e-12);
                improved = true;
                if rel < tolerance {
                    converged = true;
                }
                break;
            }
            lambda *= 10.0;
        }
        if !improved || converged {
            converged = true;
            break;
        }
    }

    Ok(LeastSquaresResult {
        parameters: p,
        residual: r,
        cost: c,
        iterations,
        converged,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeastSquaresResult {
    pub parameters: Vec<f64>,
    pub residual: Vec<f64>,
    /// Sum of squared residuals -- `scipy` reports half this as `cost`.
    pub cost: f64,
    pub iterations: usize,
    pub converged: bool,
}
