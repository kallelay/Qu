# Sparse Matrices

*Added in v0.4.7.* `import sparse` gives compressed-sparse-row (CSR) matrices for systems where almost every entry is zero (finite-difference grids, graphs, circuit matrices). Always call the functions qualified (`sparse.mul(A, x)`): `size`, `get`, `add`, `diag`, `eye`, `scale`, `transpose`, `random` and `solve` are also core names or common variable names, so a bare use after `import sparse` is an ambiguity error.

A sparse matrix is an **immutable value**: every operation returns a new matrix and never changes its arguments. It is a `Model` of kind `"sparse"` with read-only fields `rows`, `cols`, `nnz`, `indptr`, `indices` and `data` (the CSR arrays).

**Indices are zero-based**, like the rest of base Qu: `sparse.get(S, 0, 0)` is the top-left entry.

**Limits.** Rows and columns each at most 10,000,000; at most 20,000,000 stored entries; `to_dense`/`from_dense` at most 25,000,000 elements. Larger requests are refused with an error naming the function and the sizes. Entries that are exactly zero (including exact cancellations in `add`/`sub`) are not stored. Non-finite values are refused by `from_triplets`.

## Constructing

| Function | Signature | Description |
|---|---|---|
| `sparse.from_triplets` | `sparse.from_triplets(i, j, v, m, n)` | Builds an `m` x `n` matrix from coordinate lists: row indices `i`, column indices `j` (zero-based non-negative integers) and values `v`, all the same length. Duplicate coordinates are **summed**. Out-of-range indices are an error naming the entry. Added in v0.4.7. |
| `sparse.from_dense` | `sparse.from_dense(A)` | Stores the nonzeros of a dense matrix `A` (a vector is taken as a column). Added in v0.4.7. |
| `sparse.eye` | `sparse.eye(n)` | The `n` x `n` identity. Added in v0.4.7. |
| `sparse.diag` | `sparse.diag(v)` | The square matrix with vector `v` on its diagonal. Added in v0.4.7. |
| `sparse.random` | `sparse.random(m, n, density, [seed=])` | A random `m` x `n` matrix with exactly `round(density*m*n)` stored entries at distinct positions, values uniform in (0, 1). `seed=` follows the `randn` convention: with it the result reproduces; without it the session stream (`seed(n)`) is used. Added in v0.4.7. |

## Inspecting

| Function | Signature | Description |
|---|---|---|
| `sparse.size` | `sparse.size(S)` | Returns the vector `[rows, cols]`. Added in v0.4.7. |
| `sparse.nnz` | `sparse.nnz(S)` | Number of stored entries. Added in v0.4.7. |
| `sparse.density` | `sparse.density(S)` | `nnz / (rows*cols)`, 0 for an empty matrix. Added in v0.4.7. |
| `sparse.to_dense` | `sparse.to_dense(S)` | The dense `Mat`. Refuses a matrix with more than 25,000,000 elements. Added in v0.4.7. |
| `sparse.get` | `sparse.get(S, i, j)` | The entry at row `i`, column `j` (zero-based; 0 when not stored). Binary search within the row, so it is cheap in a loop. Out of range is an error. Added in v0.4.7. |
| `sparse.triplets` | `sparse.triplets(S)` | A record with vectors `i`, `j`, `v` (zero-based), the inverse of `from_triplets`. Added in v0.4.7. |
| `sparse.transpose` | `sparse.transpose(S)` | The transpose, as a new sparse matrix. Added in v0.4.7. |

## Arithmetic

| Function | Signature | Description |
|---|---|---|
| `sparse.add` | `sparse.add(A, B)` | Sum of two sparse matrices of the same shape. A shape mismatch is an error giving both shapes. Added in v0.4.7. |
| `sparse.sub` | `sparse.sub(A, B)` | Difference, same rules as `add`. Added in v0.4.7. |
| `sparse.scale` | `sparse.scale(A, k)` | Every entry times the scalar `k`. Added in v0.4.7. |
| `sparse.hadamard` | `sparse.hadamard(A, B)` | Element-wise product of two same-shape sparse matrices. Added in v0.4.7. |
| `sparse.mul` | `sparse.mul(A, B)` | Matrix product. `B` sparse gives a sparse result; `B` a vector gives a vector; `B` a dense matrix gives a dense matrix. Inner dimensions must agree. Added in v0.4.7. |

## Solving

| Function | Signature | Description |
|---|---|---|
| `sparse.solve` | `sparse.solve(A, b, [method="lu"], [tol=1e-10], [maxiter=], [precond="none"])` | Solves `A x = b` for square `A`. **`method="lu"`** (default): sparse LU, a left-looking Gilbert-Peierls factorisation with threshold partial pivoting (the diagonal is preferred while within 10x of the largest candidate). The fill-reducing column ordering is **reverse Cuthill-McKee on the pattern of A + A'**, not COLAMD or AMD: very good on banded and mesh-like matrices, possibly poor on irregular unsymmetric patterns. `b` may be a vector (returns a vector) or a dense matrix of right-hand sides (returns a matrix). A singular or numerically singular matrix (no pivot above 1e-13 times the largest entry) is an error. **`method="cg"`**: conjugate gradient for symmetric positive definite matrices; the matrix is checked for symmetry and refused otherwise. **`method="bicgstab"`**: BiCGSTAB for nonsymmetric matrices. The iterative methods return a record with `x`, `iterations`, `residual` (the true relative residual `norm(b - A x)/norm(b)`, recomputed at the end), `converged` (bool), `status` (text: `"converged"`, `"did not converge in N iterations"` or a breakdown description such as an indefinite matrix under `cg`) and `method`. They do **not** error on non-convergence: check `converged`. `tol` is the relative residual target, `maxiter` defaults to `max(1000, 10*n)`, and `precond="jacobi"` applies diagonal preconditioning (needs a nonzero diagonal, positive for `cg`). Eigenvalue solvers (`eigs`) are not provided. Added in v0.4.7. |
| `sparse.cg` <br> *Added in v0.4.14* | `sparse.cg(S, b, [x0], [tol=1e-10], [max_iter=], [precond="none"])` | Conjugate gradient for symmetric positive definite `S`. `S`: a square sparse matrix; `b`: a vector of the same length (one right-hand side); `x0` (optional vector, positional or `x0=`): the starting guess, default zeros; `tol` (default 1e-10): the target relative residual `norm(b - S x)/norm(b)`; `max_iter` (positive whole number): the cap on iterations, default `max(1000, 10*n)`; `precond` (`"none"` or `"jacobi"`): diagonal preconditioning, which needs a nonzero diagonal. Returns a record with `x`, `iterations`, `residual` (the **true** relative residual, recomputed at the end), `converged` (boolean) and `status` (text). Running out of iterations or breaking down is not an error: check `converged`. A start `x0` that already meets `tol` returns after 0 iterations; `b = 0` returns `x = 0`. Errors: a non-square matrix, a `b` or `x0` of the wrong length or with NaN or Inf, an unknown `precond`, a non-positive `tol` or `max_iter`, and a non-sparse first argument. The matrix is checked for symmetry and refused if it is not (the message points to `sparse.gmres`/`sparse.bicgstab`); a symmetric but indefinite matrix stops at the first non-positive curvature `p'Sp` with `converged = false` and a `status` saying why, never loops. |
| `sparse.gmres` <br> *Added in v0.4.14* | `sparse.gmres(S, b, [x0], [tol=1e-10], [restart=30], [max_iter=], [precond="none"])` | Restarted GMRES(`restart`) for general (nonsymmetric) `S`, with right preconditioning and a Givens-rotation least-squares update. `restart` (positive whole number, default 30, capped at the matrix size) is the Krylov dimension kept before restarting: a small value saves memory (`restart * n` numbers) and may need more iterations. `max_iter` counts inner iterations (matrix-vector products) over all restarts. `S`: a square sparse matrix; `b`: a vector of the same length (one right-hand side); `x0` (optional vector, positional or `x0=`): the starting guess, default zeros; `tol` (default 1e-10): the target relative residual `norm(b - S x)/norm(b)`; `max_iter` (positive whole number): the cap on iterations, default `max(1000, 10*n)`; `precond` (`"none"` or `"jacobi"`): diagonal preconditioning, which needs a nonzero diagonal. Returns a record with `x`, `iterations`, `residual` (the **true** relative residual, recomputed at the end), `converged` (boolean) and `status` (text). Running out of iterations or breaking down is not an error: check `converged`. A start `x0` that already meets `tol` returns after 0 iterations; `b = 0` returns `x = 0`. Errors: a non-square matrix, a `b` or `x0` of the wrong length or with NaN or Inf, an unknown `precond`, a non-positive `tol` or `max_iter`, and a non-sparse first argument.  |
| `sparse.bicgstab` <br> *Added in v0.4.14* | `sparse.bicgstab(S, b, [x0], [tol=1e-10], [max_iter=], [precond="none"])` | BiCGSTAB for general (nonsymmetric) `S`: short recurrences (constant memory) but an irregular residual history and possible breakdown (reported in `status`, not raised). `S`: a square sparse matrix; `b`: a vector of the same length (one right-hand side); `x0` (optional vector, positional or `x0=`): the starting guess, default zeros; `tol` (default 1e-10): the target relative residual `norm(b - S x)/norm(b)`; `max_iter` (positive whole number): the cap on iterations, default `max(1000, 10*n)`; `precond` (`"none"` or `"jacobi"`): diagonal preconditioning, which needs a nonzero diagonal. Returns a record with `x`, `iterations`, `residual` (the **true** relative residual, recomputed at the end), `converged` (boolean) and `status` (text). Running out of iterations or breaking down is not an error: check `converged`. A start `x0` that already meets `tol` returns after 0 iterations; `b = 0` returns `x = 0`. Errors: a non-square matrix, a `b` or `x0` of the wrong length or with NaN or Inf, an unknown `precond`, a non-positive `tol` or `max_iter`, and a non-sparse first argument. These are the same kernels as `sparse.solve(method="cg")` and `method="bicgstab"`, with the starting guess and the `max_iter` spelling added. |

## Differential operators (method of lines)

| Function | Signature | Description |
|---|---|---|
| `sparse.diffmat` <br> *Added in v0.4.14* | `sparse.diffmat(n, [h=1], [order=1], [bc="none"])` | The central-difference first (`order=1`, stencil `(u[i+1] - u[i-1])/(2h)`) or second (`order=2`, `(u[i+1] - 2u[i] + u[i-1])/h^2`) derivative on `n` equally spaced points `h` apart, as an `n` x `n` sparse matrix (apply it with `sparse.mul(D, u)`). `n`: a positive whole number; `h` (positive number): the spacing; `order`: 1 or 2; `bc`: how the two ends are treated. `"none"` (default): one-sided second-order stencils at the first and last row (`(-3, 4, -1)/(2h)` and `(2, -5, 4, -1)/h^2`), so every row is a derivative and the whole matrix is second-order accurate; needs `n >= 3` (`order=1`) or `n >= 4` (`order=2`). `"periodic"`: the grid wraps around (`u[-1]` is `u[n-1]`), for a periodic function sampled at `x = 0, h, ..., (n-1)h`; needs `n >= 3`. `"dirichlet"`: the `n` points are the interior unknowns and the value just outside each end is zero, so the end rows drop that neighbour (`order=2` gives the familiar tridiagonal `1, -2, 1` over `h^2`); needs `n >= 2`. The second derivative is **negative** semidefinite (`u'' = D u`); `sparse.laplacian` is its negative. Because it is sparse, it feeds `sparse.solve` and, for a time-dependent PDE, the right-hand side of `ode_stiff`/`ode45`: `u_t = u_xx` is `ode_stiff((t, u) := sparse.mul(D, u), tspan, u0)`. Errors: a non-positive or non-finite `h`, `order` other than 1 or 2, an unknown `bc` (the message lists the three), and too few points for the chosen `bc`. |
| `sparse.laplacian` <br> *Added in v0.4.14* | `sparse.laplacian(shape, [h=1], [bc="dirichlet"])` | The discrete negative Laplacian (`-u_xx`, or `-(u_xx + u_yy)` in 2-D) with the 3-point (5-point) stencil, as a sparse matrix; it is symmetric and positive (semi)definite, so `sparse.cg` applies. `shape`: `n` for a 1-D grid or `[nx, ny]` for a 2-D one; `h`: the spacing, a number or (2-D) a vector `[hx, hy]`; `bc`: `"dirichlet"` (default; the grid holds the interior unknowns, zero beyond the ends; the 1-D eigenvalues are `(2 - 2cos(k*pi/(n+1)))/h^2`, `k = 1..n`), `"neumann"` (zero flux at the ends of a cell-centred grid: the end rows are `[1, -1]/h^2`; singular, with the constants as null space; eigenvalues `(2 - 2cos(k*pi/n))/h^2`, `k = 0..n-1`) or `"periodic"` (wraps around; eigenvalues `(2 - 2cos(2*pi*k/n))/h^2`). **Node ordering in 2-D:** the unknown for grid node `(i, j)` (`i` along x, `0 <= i < nx`; `j` along y, `0 <= j < ny`) is number `i + nx*j`: x runs fastest, so an `nx` x `ny` Qu matrix (stored column-major) flattens to the unknown vector in the same order; the matrix is `kron(I_ny, Lx) + kron(Ly, I_nx)` and has `nx*ny` rows. Poisson's equation `-lap(u) = f` with `u = 0` on the boundary of the unit square is `u = sparse.solve(sparse.laplacian([n, n], 1/(n+1)), f)` for the `n` x `n` interior points. Errors: a `shape` that is not `n` or `[nx, ny]` (3-D is not supported) or is larger than the sparse limit, a non-positive `h`, an `h` vector of the wrong length, an unknown `bc`, and too few points per direction (1 for Dirichlet, 2 for Neumann, 3 for periodic). |

Heat equation by the method of lines, then Poisson's equation on the unit
square (the printed errors are the measured ones, dominated by the O(h^2)
space discretisation):

```qu
import sparse

# u_t = u_xx on (0, 1), u = 0 at both ends, u(x, 0) = sin(pi x); exact: exp(-pi^2 t) sin(pi x)
n = 49                                   # interior grid points
h = 1 / (n + 1)
x = linspace(h, 1 - h, n)
D2 = sparse.diffmat(n, h, order=2, bc="dirichlet")
sol = ode_stiff((t, u) := sparse.mul(D2, u), [0, 0.1], sin(pi * x), rtol=1e-9, atol=1e-12)
u_end = sol.y[length(sol.t) - 1, 0:(n - 1)]
print("max error at t = 0.1: {max(abs(u_end - exp(-pi^2 * 0.1) * sin(pi * x)))}")   # 0.000121

# -lap(u) = 2 pi^2 sin(pi x) sin(pi y) on the unit square, u = 0 on the edge
m = 31
hm = 1 / (m + 1)
g = linspace(hm, 1 - hm, m)
xs = zeros(m * m)
ys = zeros(m * m)
for j = 0 to m - 1
    for i = 0 to m - 1
        xs[i + m * j] = g[i]             # unknown (i, j) is number i + m*j
        ys[i + m * j] = g[j]
    end for
end for
L = sparse.laplacian([m, m], hm)
v = sparse.solve(L, 2 * pi^2 * sin(pi * xs) * sin(pi * ys))
print("Poisson max error: {max(abs(v - sin(pi * xs) * sin(pi * ys)))}")           # 0.000804
```
