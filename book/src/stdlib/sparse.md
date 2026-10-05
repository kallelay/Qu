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
