# Linear Algebra panel (Qu Studio)

The **Linear Algebra** tab of Qu Studio is a front end to the matrix functions in
[Core Math & Linear Algebra](core-math.md) and [Sparse Matrices](sparse.md). You enter matrices, pick a
task, and the panel answers, says which method it chose and why, checks the answer, and shows the Qu
script that did it. Like the Filter Designer and the Loop tab, the panel never computes anything itself:
every number on screen comes out of one ordinary Qu script that you can copy, open in the editor, edit and
re-run.

## Walkthrough

1. **Load an example** (top of the left column), for instance *Solve: Hilbert 8x8 (ill-conditioned), true x = ones*.
   Or pick a task and type your own matrices.
2. **Enter the matrices.** Each matrix card has five sources: a spreadsheet-style *Grid* (up to 12 x 12:
   arrow keys and Enter move, and pasting a block from a spreadsheet fills it), a *Builder* (identity, zeros,
   ones, diagonal, seeded random, Hilbert, Vandermonde, Toeplitz, 1-D Poisson, or a sparse pattern of
   `row col value` lines), *Paste / CSV*, a *File* chosen from disk, or a *Variable* from the Code tab's last
   run. Pasted, file and variable data above 400 entries is written to a temporary file that the script
   reads with `read_array`; smaller data is written into the script as a literal.
3. **Read the summary under each matrix.** Size, symmetric, positive definite (a Cholesky attempt),
   triangular, banded (bandwidths), sparsity, rank, condition number and determinant are all measured by
   Qu. Above 600 x 600 the cubic-cost tests (rank, condition number, determinant, Cholesky) are skipped and
   "likely positive definite" is judged from a dominant positive diagonal.
4. **Read the answer.** Every result card names the Qu call that produced it. Check cards turn red when
   the identity they test fails.

## Tasks

| Task | What it does | Qu calls |
|---|---|---|
| Solve A x = b | The **advisor** looks at the measured structure and picks a method with a one-line reason; you can override it. Always shows the residual, the condition number and a plain-words warning above condition number 1e8, with a one-click offer of Tikhonov (lambda slider) or truncated SVD. | `chol`, `solve`, `qr`, `svd`, `lstsq`, `pinv`, `sparse.cg`, `sparse.gmres`, `sparse.solve` |
| Decompositions | LU, QR, Cholesky, SVD, eigen, Schur: the factors, the reconstruction identity and the orthogonality check, singular values as bars, eigenvalues in the complex plane. | `lu`, `qr`, `chol`, `svd`, `eig`, `schur` |
| Properties | Determinant, rank, trace, the 1, 2, infinity and Frobenius norms, condition number, null space, range, pseudo-inverse, inverse (with the reminder that `solve` is better). | `det`, `rank`, `trace`, `norm`, `cond`, `null`, `orth`, `pinv`, `inv` |
| Least squares | Normal equations, QR and SVD side by side, optionally weighted, with the error against a known answer, so the accuracy lost to squaring the condition number is visible. | `solve`, `qr`, `svd` |
| Matrix functions | `expm`, `sqrtm`, `logm`, integer powers, `lyap`, `sylvester`, each with its residual. | `expm`, `sqrtm`, `logm`, `lyap`, `sylvester` |
| Eigenproblems | All eigenvalues in the complex plane; for symmetric matrices the eigenvectors and their orthogonality. | `eig` |

### How the advisor chooses

| Structure of A | Method | Why |
|---|---|---|
| rank-deficient | `lstsq` | no unique solution: the minimum-norm least-squares one |
| more rows than columns | QR least squares (SVD when `cond` > 1e12) | avoids squaring the condition number |
| fewer rows than columns | `lstsq` | infinitely many solutions: the minimum-norm one |
| triangular | substitution | no factorisation needed |
| large (n >= 200), sparse (<= 10% filled), SPD | `sparse.cg` with a Jacobi preconditioner | symmetric positive definite: conjugate gradient applies |
| large, sparse, banded, not SPD | `sparse.solve` (sparse LU) | fill-in stays inside the band |
| large, sparse, other | `sparse.gmres` | general sparse system |
| symmetric positive definite | Cholesky | about twice as fast as LU |
| otherwise | LU with partial pivoting (`solve`) | the general-purpose method |

The decision is made in the panel from the flags that Qu measured; the generated script states the method
and the reason in a comment. Editing a setting rewrites the script; **Edit the code** detaches the panel
from its settings (it says so) so you can change the script by hand and press Run.

## A worked example

*Solve: Hilbert 8x8*, with b built as A times a vector of ones so the exact answer is known. The advisor
finds a symmetric positive definite matrix and writes this script (run with `qu` 0.4.13; the last three
`print` lines are added here to show the numbers):

```qu
# Qu Studio / Linear Algebra: solve A x = b
# method: Cholesky (symmetric positive definite) -- chosen by the advisor: A is symmetric positive definite (a Cholesky factorisation succeeded): Cholesky is about twice as fast as LU and needs no pivoting.
la_i = linspace(1, 8, 8)
A = 1 ./ (outer(la_i, ones(8)) + outer(ones(8), la_i) - 1)   # Hilbert: 1/(i+j-1)
b = [2.7178571428571425, 1.828968253968254, 1.428968253968254, 1.1865440115440116, 1.0198773448773448, 0.896800421800422, 0.8015623265623266, 0.7253718503718505] as matrix(8, 1)
n = cols(A)
L = chol(A)            # A = L L'
y = solve(L, b)         # forward substitution
x = solve(L', y)        # back substitution
# checks: how well does x solve the system, and how trustworthy is the matrix?
relres = norm(A * x - b) / max(norm(b), 1e-300)
gradres = norm(A' * (A * x - b)) / max(norm(A) * norm(b), 1e-300)   # zero exactly when x is a least-squares solution
nx = norm(x)
condA = cond(A)
print("relative residual = {relres}")
print("condition number  = {condA}")
print("error in x        = {norm(x - ones(8, 1))}")
```

```text
relative residual = 2.677392e-17
condition number  = 15257575773.976641
error in x        = 9.894696e-7
```

The residual looks perfect while about a third of the digits are gone. That is exactly what the panel's
warning ("A is ill-conditioned ... a solve loses about 10 of the 16 digits") is for. The *Least squares*
task on the same matrix shows the three methods side by side:

```qu
# Qu Studio / Linear Algebra: least squares, three ways
# the same problem solved by the normal equations, by QR and by the SVD
la_i = linspace(1, 8, 8)
A = 1 ./ (outer(la_i, ones(8)) + outer(ones(8), la_i) - 1)   # Hilbert: 1/(i+j-1)
n = cols(A)
x_true = ones(n, 1)
b = A * x_true        # exact data: the true answer is known
Aw = A
bw = b
cond_A = cond(Aw)
cond_normal = cond(Aw' * Aw)    # the normal equations square the condition number
# 1. normal equations
ne_failed = 0
x_ne = zeros(n, 1)
try
  x_ne = solve(Aw' * Aw, Aw' * bw)
catch e
  ne_failed = 1
  print("@@ne_error " + e.message)
end
# 2. QR
f = qr(Aw)
x_qr = solve(f.r, f.q' * bw)
# 3. SVD
g = svd(Aw)
tol = max(rows(Aw), cols(Aw)) * 2.220446049250313e-16 * g.s[0]
sinv = where(g.s > tol, 1 ./ g.s, 0)
x_svd = g.vt' * diag(sinv) * (g.u' * bw)
x_ref = x_true
res_ne = norm(Aw * x_ne - bw)
res_qr = norm(Aw * x_qr - bw)
res_svd = norm(Aw * x_svd - bw)
nref = max(norm(x_ref), 1e-300)
err_ne = norm(x_ne - x_ref) / nref
err_qr = norm(x_qr - x_ref) / nref
err_svd = norm(x_svd - x_ref) / nref
print("cond(A)          = {cond_A}")
print("normal equations: error {err_ne}")
print("QR              : error {err_qr}")
print("SVD             : error {err_svd}")
```

```text
cond(A)          = 15257575773.976641
normal equations: error 12.584113
QR              : error 4.850573e-7
SVD             : error 1.228675e-6
```

The normal equations (`solve(A'A, A'b)`) square the condition number to about 1e20, beyond what a double
can resolve, and return an answer that is wrong by a factor of twelve; QR and the SVD keep six digits.

## Notes and limits

- Qu has no induced-norm builtin: the 1-norm, infinity-norm and 2-norm are computed in the script from
  column sums, row sums and `svd(A).s[0]`. `norm(A)` alone is the Frobenius norm.
- `eig` returns eigenvectors only for symmetric matrices, so the panel shows them only then.
- `sqrtm` and `logm` of a matrix with a negative real eigenvalue are complex; the panel shows them as
  `a + bj`, and skips the `expm` round-trip check for `logm` there, because `expm` does not take a
  complex matrix.
- After `import sparse` a bare `solve`, `size`, `diag` or `eye` is ambiguous, so the sparse methods' scripts
  use only qualified names.
