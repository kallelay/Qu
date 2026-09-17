//! Dense, column-major real matrices — the M2 N-D value backing the Qu runtime.
//!
//! Storage is **column-major** (`data[row + col * rows]`) to match the language
//! specification and the FFI expectations of BLAS/LAPACK providers that arrive
//! in later milestones. Every operation here is portable and dependency-free so
//! native and `wasm32` builds share one definition; accelerated providers must
//! reproduce these reference results.
//!
//! Broadcasting follows the spec's rule (§14): two shapes combine when, for each
//! axis, the extents are equal or one of them is `1`. A `1` extent is stretched
//! (its stride is treated as zero). Scalars are the `(1, 1)` case; a Qu vector of
//! length `k` is treated by the interpreter as the column matrix `(k, 1)`.

use std::fmt::{self, Display, Formatter};

/// Errors from shape-checked matrix algebra. Distinct from the numeric-range
/// errors in the crate root so callers can render precise diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// Elementwise/broadcast combination of incompatible shapes.
    Broadcast {
        left: (usize, usize),
        right: (usize, usize),
    },
    /// `matmul` inner-dimension mismatch: `(m, k) * (p, n)` with `k != p`.
    MatMul {
        left: (usize, usize),
        right: (usize, usize),
    },
    /// A matrix literal whose rows do not all share one width.
    RaggedRows { expected: usize, found: usize },
    /// An index that fell outside the matrix bounds.
    Index {
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
    },
    /// A reshape whose element count does not match the source.
    Reshape {
        from: usize,
        to: (usize, usize),
    },
    /// An axis argument other than 0 (columns) or 1 (rows).
    Axis(usize),
}

impl Display for ShapeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Broadcast { left, right } => write!(
                f,
                "shapes ({}x{}) and ({}x{}) do not broadcast (each axis must match or be 1)",
                left.0, left.1, right.0, right.1
            ),
            Self::MatMul { left, right } => write!(
                f,
                "matrix multiply ({}x{}) * ({}x{}) needs the inner dimensions to agree",
                left.0, left.1, right.0, right.1
            ),
            Self::RaggedRows { expected, found } => write!(
                f,
                "matrix rows must have equal width: expected {expected}, found a row of {found}"
            ),
            Self::Index { row, col, rows, cols } => write!(
                f,
                "index [{row}, {col}] is out of bounds for a {rows}x{cols} matrix"
            ),
            Self::Reshape { from, to } => write!(
                f,
                "cannot reshape {from} elements into {}x{} ({} elements)",
                to.0, to.1, to.0 * to.1
            ),
            Self::Axis(a) => write!(f, "axis must be 0 (down columns) or 1 (across rows), got {a}"),
        }
    }
}

impl std::error::Error for ShapeError {}

/// A dense real matrix stored column-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Matrix {
    /// Build directly from column-major storage. Panics if the length is wrong;
    /// this is an internal invariant, not a user-facing error.
    pub fn from_col_major(rows: usize, cols: usize, data: Vec<f64>) -> Self {
        assert_eq!(rows * cols, data.len(), "column-major length mismatch");
        Matrix { rows, cols, data }
    }

    /// Build from column-major storage, returning an error (rather than
    /// panicking) when the length does not match `rows * cols`.
    pub fn from_col_major_checked(
        rows: usize,
        cols: usize,
        data: Vec<f64>,
    ) -> Result<Self, ShapeError> {
        if rows * cols != data.len() {
            return Err(ShapeError::Reshape {
                from: data.len(),
                to: (rows, cols),
            });
        }
        Ok(Matrix { rows, cols, data })
    }

    /// A `rows x cols` matrix filled with a constant.
    pub fn filled(rows: usize, cols: usize, value: f64) -> Self {
        Matrix {
            rows,
            cols,
            data: vec![value; rows * cols],
        }
    }

    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self::filled(rows, cols, 0.0)
    }

    pub fn ones(rows: usize, cols: usize) -> Self {
        Self::filled(rows, cols, 1.0)
    }

    /// The `n x n` identity.
    pub fn eye(n: usize) -> Self {
        let mut m = Self::zeros(n, n);
        for i in 0..n {
            m.data[i + i * n] = 1.0;
        }
        m
    }

    /// Build from row-major nested rows (`[[1,2],[3,4]]`), the shape a matrix
    /// literal `[1, 2; 3, 4]` produces. Rejects ragged rows.
    pub fn from_rows(rows: &[Vec<f64>]) -> Result<Self, ShapeError> {
        let nrows = rows.len();
        if nrows == 0 {
            return Ok(Matrix::zeros(0, 0));
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
        let mut data = vec![0.0; nrows * ncols];
        for (r, row) in rows.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                data[r + c * nrows] = v;
            }
        }
        Ok(Matrix { rows: nrows, cols: ncols, data })
    }

    /// Interpret a flat vector as a column matrix `(k, 1)`.
    pub fn from_column(values: &[f64]) -> Self {
        Matrix {
            rows: values.len(),
            cols: 1,
            data: values.to_vec(),
        }
    }

    /// Interpret a flat vector as a row matrix `(1, k)`.
    pub fn from_row(values: &[f64]) -> Self {
        Matrix {
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
    /// Raw column-major buffer (for FFI / providers).
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }
    pub fn into_vec(self) -> Vec<f64> {
        self.data
    }

    /// True when the matrix has a single element (a promoted scalar).
    pub fn is_scalar(&self) -> bool {
        self.data.len() == 1
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Result<f64, ShapeError> {
        if row >= self.rows || col >= self.cols {
            return Err(ShapeError::Index {
                row,
                col,
                rows: self.rows,
                cols: self.cols,
            });
        }
        Ok(self.data[row + col * self.rows])
    }

    #[inline]
    pub fn set(&mut self, row: usize, col: usize, value: f64) -> Result<(), ShapeError> {
        if row >= self.rows || col >= self.cols {
            return Err(ShapeError::Index {
                row,
                col,
                rows: self.rows,
                cols: self.cols,
            });
        }
        self.data[row + col * self.rows] = value;
        Ok(())
    }

    /// Extract row `r` as a flat vector of length `cols`.
    pub fn row_vec(&self, r: usize) -> Result<Vec<f64>, ShapeError> {
        if r >= self.rows {
            return Err(ShapeError::Index {
                row: r,
                col: 0,
                rows: self.rows,
                cols: self.cols,
            });
        }
        Ok((0..self.cols).map(|c| self.data[r + c * self.rows]).collect())
    }

    /// Extract column `c` as a flat vector of length `rows`.
    pub fn col_vec(&self, c: usize) -> Result<Vec<f64>, ShapeError> {
        if c >= self.cols {
            return Err(ShapeError::Index {
                row: 0,
                col: c,
                rows: self.rows,
                cols: self.cols,
            });
        }
        Ok(self.data[c * self.rows..(c + 1) * self.rows].to_vec())
    }

    /// Transpose (conjugate transpose is identical for real matrices).
    pub fn transpose(&self) -> Matrix {
        let mut out = Matrix::zeros(self.cols, self.rows);
        for c in 0..self.cols {
            for r in 0..self.rows {
                out.data[c + r * self.cols] = self.data[r + c * self.rows];
            }
        }
        out
    }

    /// Below this many total elements, rayon's thread-pool dispatch overhead
    /// outweighs the gain — same reasoning as [`Self::PARALLEL_MATMUL_THRESHOLD`],
    /// but **not the same value** on purpose (an earlier pass copy-pasted
    /// matmul's `1<<16` here, which turned out wrong): matmul's threshold
    /// suits a rare, huge, one-shot call, but elementwise ops are typically
    /// called *repeatedly* in a tight loop (an iterative optimizer's
    /// per-iteration objective/gradient, not a one-off bulk transform), and
    /// repeated rayon dispatch has real per-call overhead that doesn't
    /// amortize until arrays are much bigger than matmul's crossover.
    ///
    /// Measured directly (2026-08-23): an iterative-optimizer benchmark
    /// regressed from 76s to 320-390s after the first elementwise-
    /// parallelization pass — its `synth`/`grad` hot loop repeatedly
    /// transforms 31×25000 = 775,000-element matrices. At that size, 400
    /// repeated calls averaged ~80-90ms/call *parallel*, against a measured
    /// ~4.8ms/call *serial* baseline at 62,000 elements (~61ms/call
    /// extrapolated linearly to 775,000) — parallel dispatch was actively
    /// *losing* to serial at the old threshold under this call pattern, not
    /// just failing to help. Raised to where a single call plainly wins
    /// (measured ~46ms parallel vs ~79ms serial-extrapolated at 1,000,000
    /// elements), with headroom above the 775,000-element case that
    /// exposed this so a repeated-call workload at that exact size doesn't
    /// re-trip it.
    #[cfg(feature = "parallel")]
    const PARALLEL_ELEMENTWISE_THRESHOLD: usize = 1 << 20;

    /// Apply a scalar function elementwise. Every output element is
    /// independent (unlike `matmul`'s column accumulation), so above
    /// [`Self::PARALLEL_ELEMENTWISE_THRESHOLD`] this splits across a rayon
    /// thread pool with a plain `par_iter`/`map`/`collect` — no chunking
    /// logic needed. `sin`/`cos`/`abs`/... on a large `Value::Mat` route
    /// through here (`qu-interp`'s `map1`), so this is the actual bottleneck
    /// behind `element_wise_ops`'s benchmark gap (`benchmarks/README.md`).
    pub fn map<F: Fn(f64) -> f64 + Sync + Send>(&self, f: F) -> Matrix {
        #[cfg(feature = "parallel")]
        {
            if self.data.len() >= Self::PARALLEL_ELEMENTWISE_THRESHOLD {
                use rayon::prelude::*;
                return Matrix {
                    rows: self.rows,
                    cols: self.cols,
                    data: self.data.par_iter().copied().map(f).collect(),
                };
            }
        }
        Matrix {
            rows: self.rows,
            cols: self.cols,
            data: self.data.iter().copied().map(f).collect(),
        }
    }

    /// Reshape preserving column-major element order; total count must match.
    pub fn reshape(&self, rows: usize, cols: usize) -> Result<Matrix, ShapeError> {
        if rows * cols != self.data.len() {
            return Err(ShapeError::Reshape {
                from: self.data.len(),
                to: (rows, cols),
            });
        }
        Ok(Matrix {
            rows,
            cols,
            data: self.data.clone(),
        })
    }

    /// Broadcast-combine two matrices with a binary op (§14 rule). Each
    /// output column is independent (same column-major-contiguity fact
    /// `matmul` exploits), so above
    /// [`Self::PARALLEL_ELEMENTWISE_THRESHOLD`] columns compute on a rayon
    /// thread pool instead of serially — `.* + - / mod` on two large
    /// `Value::Mat`s (`qu-interp`'s `map2`/`matrix_binop`) route through
    /// here.
    pub fn broadcast<F: Fn(f64, f64) -> f64 + Sync + Send>(
        &self,
        other: &Matrix,
        f: F,
    ) -> Result<Matrix, ShapeError> {
        // Same-shape fast path (§ broadcast per-element branching,
        // 2026-09-10). The overwhelmingly common case -- two equal-shaped
        // operands, no broadcasting happening at all -- still paid four
        // per-element branches (`self.cols==1`, `other.cols==1`,
        // `self.rows==1`, `other.rows==1`) and a row/column index
        // recomputation on every single element below, purely to support
        // the broadcasting the general path exists for. `map`'s own flat-
        // iterator kernel has none of that.
        //
        // Measured, not assumed: a single SERIAL thread doing a flat zip
        // over the same 4M-element data beat this function's PARALLEL
        // (16-thread) path outright -- which rules out memory bandwidth as
        // the limiter, since a bandwidth-bound workload cannot lose to
        // fewer threads doing the same total work. The branching itself
        // was the cost, and this removes it for the shape that never
        // needed it, while leaving the general broadcasting path below
        // completely unchanged for the case that does.
        //
        // Mathematically identical to the general path, not an
        // approximation: when `self.rows==other.rows` and
        // `self.cols==other.cols`, the general path's `lr`/`rr`/`lc`/`rc`
        // always equal `r`/`r`/`c`/`c` -- this is that same computation
        // with the now-constant branches removed, verified against it
        // directly as a test rather than assumed from the algebra alone.
        if self.rows == other.rows && self.cols == other.cols {
            #[cfg(feature = "parallel")]
            {
                if self.data.len() >= Self::PARALLEL_ELEMENTWISE_THRESHOLD {
                    use rayon::prelude::*;
                    let data: Vec<f64> = self
                        .data
                        .par_iter()
                        .zip(other.data.par_iter())
                        .map(|(&a, &b)| f(a, b))
                        .collect();
                    return Ok(Matrix { rows: self.rows, cols: self.cols, data });
                }
            }
            let data: Vec<f64> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(&a, &b)| f(a, b))
                .collect();
            return Ok(Matrix { rows: self.rows, cols: self.cols, data });
        }
        let rows = broadcast_dim(self.rows, other.rows).ok_or(ShapeError::Broadcast {
            left: self.shape(),
            right: other.shape(),
        })?;
        let cols = broadcast_dim(self.cols, other.cols).ok_or(ShapeError::Broadcast {
            left: self.shape(),
            right: other.shape(),
        })?;
        let mut data = vec![0.0; rows * cols];
        let compute_column = |c: usize, col: &mut [f64]| {
            let lc = if self.cols == 1 { 0 } else { c };
            let rc = if other.cols == 1 { 0 } else { c };
            for r in 0..rows {
                let lr = if self.rows == 1 { 0 } else { r };
                let rr = if other.rows == 1 { 0 } else { r };
                let a = self.data[lr + lc * self.rows];
                let b = other.data[rr + rc * other.rows];
                col[r] = f(a, b);
            }
        };
        #[cfg(feature = "parallel")]
        {
            if rows.saturating_mul(cols) >= Self::PARALLEL_ELEMENTWISE_THRESHOLD {
                use rayon::prelude::*;
                data.par_chunks_mut(rows.max(1)).enumerate().for_each(|(c, col)| compute_column(c, col));
                return Ok(Matrix { rows, cols, data });
            }
        }
        for (c, col) in data.chunks_mut(rows.max(1)).enumerate() {
            compute_column(c, col);
        }
        Ok(Matrix { rows, cols, data })
    }

    /// Below this many total output-times-contraction elements (`m*n*k`),
    /// rayon's thread-pool dispatch overhead outweighs the gain, so small
    /// matrices — the overwhelming common case in real Qu scripts — stay
    /// serial. Chosen generously (not tuned to a specific machine): the goal
    /// is skipping parallelism only where it obviously can't pay off yet.
    #[cfg(feature = "parallel")]
    const PARALLEL_MATMUL_THRESHOLD: usize = 1 << 16;

    /// The same idea as [`Self::PARALLEL_MATMUL_THRESHOLD`], but **not the
    /// same value on purpose** — measured directly (2026-08-26,
    /// `matmul_size_sweep_naive_vs_fast`, Ryzen 9700X, release,
    /// `fast-matmul,parallel`) that reusing the naive path's threshold here
    /// actively regresses `matmul_fast`: a single `matrixmultiply::dgemm`
    /// call is so much faster per FLOP than the naive scalar loop that
    /// rayon's dispatch overhead doesn't pay off until a much larger
    /// aggregate size.
    ///
    /// | shape (m=n=k)  | m*n*k       | single-thread `matmul_fast` | parallel-chunked `matmul_fast` |
    /// |----------------|------------:|----------------------------:|--------------------------------:|
    /// | 64             |     262,144 |                      17.8 us|            87.6 us (**0.20x**) |
    /// | 128            |   2,097,152 |                      81.0 us|           124-140 us (**~0.6x**)|
    /// | 256            |  16,777,216 |                     650.7 us|          240.1 us (**2.7x**)   |
    /// | 600            | 216,000,000 |                    5823.4 us|         1592.5 us (**3.7x**)   |
    ///
    /// The crossover sits between 128^3 and 256^3; `1 << 23`
    /// (8,388,608 — roughly 200^3) is chosen inside that gap, on the
    /// conservative side (closer to the losing 128^3 case) so a borderline
    /// shape defaults to the single-thread path, which is already a solid
    /// win over naive on its own (see `matmul`'s own doc comment).
    #[cfg(all(feature = "fast-matmul", feature = "parallel"))]
    const PARALLEL_FAST_MATMUL_THRESHOLD: usize = 1 << 23;

    /// A "thin" shape — many rows, few output columns, e.g. a
    /// `(2400,32)@(32,4)` ML output layer — can clear
    /// [`Self::PARALLEL_MATMUL_THRESHOLD`]'s *aggregate* `m*n*k` check while
    /// still only handing rayon `n=4` column tasks: too few to keep a
    /// many-core machine busy, and (measured below) actively worse than
    /// staying serial. `matmul` requires the column count `n` to reach the
    /// live thread count (`rayon::current_num_threads()`, read at call time
    /// so this adapts to `RAYON_NUM_THREADS`/the actual machine rather than
    /// a hardcoded guess) before taking the parallel path at all.
    ///
    /// **Two other fixes were tried first and both measured worse.** Both
    /// attempts and the final fix were checked on the exact three per-epoch
    /// shapes from `benchmarks/mlp/bench.qu`
    /// (`(2400,20)@(20,64)`/`(2400,64)@(64,32)`/`(2400,32)@(32,4)`),
    /// isolated (no `param`/`grad` tracking), Ryzen 9700X (16 logical
    /// cores), release build, 3000 repeated calls per shape, interleaved
    /// `RAYON_NUM_THREADS=1` vs default runs:
    ///
    /// | shape (m,k,n)         | serial      | plain column-parallel | nested row+col split |
    /// |------------------------|------------:|-----------------------:|----------------------:|
    /// | (2400,20,64)           |    ~775 us  |      **~395 us** (2.0x)|            not needed |
    /// | (2400,64,32)           |   ~1110 us  |      **~500 us** (2.2x)|            not needed |
    /// | (2400,32,4)  ("thin")  |     ~98 us  |        ~153 us (0.64x) |          ~140-176 us  |
    ///
    /// The `n=64`/`n=32` shapes were already comfortably above the machine's
    /// 16-thread count and genuinely benefit (2-2.2x, matching this crate's
    /// other measured multi-core wins). The `n=4` shape loses under *any*
    /// rayon dispatch: the first attempt guessed the fix was too little work
    /// *per task* (`m*k = 76,800` multiply-adds) and tried splitting each of
    /// the 4 columns into row-blocks too (more, smaller tasks) — measured
    /// *worse* than plain column-only parallel, not better, because it only
    /// added more dispatches without adding real parallel work. The second
    /// attempt gated on `m*k` directly — but that shape's `m*k = 76,800` is
    /// *larger* than `(2400,20,64)`'s `m*k = 48,000`, which parallelizes
    /// fine, so a size-based gate either let the losing case through or
    /// blocked the winning ones too depending on where it was set; measured
    /// disabling parallelism for all three shapes at a threshold high enough
    /// to exclude the `n=4` case. The actual dividing line, per the table
    /// above, is `n` versus the live thread count, not per-task size —
    /// column-only parallelism structurally cannot use more threads than
    /// there are columns, so below that there is no argument to try harder.
    ///
    /// Ordinary matrix product `(m, k) * (k, n) -> (m, n)`.
    ///
    /// Each output column is independent (column-major storage means column
    /// `c` is the contiguous slice `data[c*m .. (c+1)*m]`), so with the
    /// `parallel` feature on, matrices clearing both
    /// [`Self::PARALLEL_MATMUL_THRESHOLD`] and the `n >= current_num_threads`
    /// check above compute their columns on a rayon thread pool instead of
    /// serially — same arithmetic, same result, no unsafe code (rayon's
    /// `par_chunks_mut` gives each worker a disjoint, non-overlapping
    /// column).
    ///
    /// Skips a `b == 0.0` term outright (common for the identity/permutation-
    /// heavy matrices this crate builds constantly, e.g. `eye(n)`) *only*
    /// once `self` is confirmed to hold no `NaN`/`Infinity` anywhere — for a
    /// genuinely finite `self`, `a * 0.0 == 0.0` for every `a`, so skipping
    /// changes nothing. Skipping unconditionally used to silently corrupt
    /// exactly the case where that's not true: `NaN * 0.0` and
    /// `Infinity * 0.0` are both `NaN`, not `0.0`, so a matmul term that
    /// should have poisoned the whole dot product with `NaN` instead
    /// vanished, giving a finite-looking wrong answer instead of the
    /// correct `NaN`.
    pub fn matmul(&self, other: &Matrix) -> Result<Matrix, ShapeError> {
        if self.cols != other.rows {
            return Err(ShapeError::MatMul {
                left: self.shape(),
                right: other.shape(),
            });
        }
        let (m, k, n) = (self.rows, self.cols, other.cols);

        #[cfg(feature = "fast-matmul")]
        {
            return Ok(self.matmul_fast(other, m, k, n));
        }

        #[cfg(not(feature = "fast-matmul"))]
        {
            self.matmul_naive(other, m, k, n)
        }
    }

    /// The original portable triple loop, kept as the reference
    /// implementation and as the actual path taken whenever `fast-matmul`
    /// is not compiled in (minimal/WASM builds, or anyone who hasn't opted
    /// in). See [`Self::matmul`]'s doc comment on the crate for the full
    /// rationale; this is unchanged from before `fast-matmul` existed.
    #[allow(dead_code)]
    fn matmul_naive(&self, other: &Matrix, m: usize, k: usize, n: usize) -> Result<Matrix, ShapeError> {
        let mut out = Matrix::zeros(m, n);
        let self_all_finite = self.data.iter().all(|v| v.is_finite());
        let compute_column = |c: usize, col: &mut [f64]| {
            for p in 0..k {
                let b = other.data[p + c * other.rows];
                if b == 0.0 && self_all_finite {
                    continue;
                }
                let a_col = &self.data[p * self.rows..p * self.rows + m];
                for r in 0..m {
                    col[r] += a_col[r] * b;
                }
            }
        };
        #[cfg(feature = "parallel")]
        {
            if m.saturating_mul(n).saturating_mul(k) >= Self::PARALLEL_MATMUL_THRESHOLD
                && n >= rayon::current_num_threads()
            {
                use rayon::prelude::*;
                out.data.par_chunks_mut(m).enumerate().for_each(|(c, col)| compute_column(c, col));
                return Ok(out);
            }
        }
        for (c, col) in out.data.chunks_mut(m).enumerate() {
            compute_column(c, col);
        }
        Ok(out)
    }

    /// SIMD-dispatched GEMM via the `matrixmultiply` crate, compiled in only
    /// under the `fast-matmul` feature. `Matrix` is already column-major
    /// (`data[row + col * rows]`, see the module doc comment), which is
    /// exactly what `matrixmultiply::dgemm`'s `(row_stride, col_stride)`
    /// pair expresses directly -- `rs=1, cs=<row count>` for every one of
    /// `self`, `other`, and the output, no transposition or repacking
    /// needed, which is the single easiest place a silent transpose/stride
    /// bug could otherwise creep in.
    ///
    /// `alpha=1.0, beta=0.0`: compute `self * other` from scratch and
    /// overwrite `out` (not accumulate into it), matching `matmul`'s
    /// existing contract. Real float multiply-adds propagate `NaN`/
    /// `Infinity` correctly on their own, so (unlike [`Self::matmul_naive`])
    /// this path needs no separate `self_all_finite` special case for the
    /// `b == 0.0` skip -- there is no such skip here at all.
    ///
    /// When `parallel` is also enabled and the shape clears the same
    /// [`Self::PARALLEL_MATMUL_THRESHOLD`]/thread-count gate `matmul_naive`
    /// uses, this splits `other`'s columns into `current_num_threads()`
    /// contiguous chunks and runs one `dgemm` call per chunk on a rayon
    /// worker -- coarser-grained than `matmul_naive`'s one-task-per-column
    /// split (a handful of large `dgemm` calls, not `n` small ones), which
    /// matters more here since each task's per-call overhead is now the
    /// crate's own setup cost, not a handful of scalar multiply-adds.
    ///
    /// See `IMPL.md`'s dated entry for the full measured comparison table
    /// (naive scalar vs this path, serial and parallel, across the exact
    /// `benchmarks/mlp/` shapes and a size sweep from small to large).
    #[cfg(feature = "fast-matmul")]
    fn matmul_fast(&self, other: &Matrix, m: usize, k: usize, n: usize) -> Matrix {
        let mut out = Matrix::zeros(m, n);
        if m == 0 || k == 0 || n == 0 {
            return out;
        }

        #[cfg(feature = "parallel")]
        {
            if m.saturating_mul(n).saturating_mul(k) >= Self::PARALLEL_FAST_MATMUL_THRESHOLD
                && n >= rayon::current_num_threads()
            {
                use rayon::prelude::*;
                let threads = rayon::current_num_threads().max(1);
                let chunk_cols = n.div_ceil(threads);
                let chunk_len = m * chunk_cols;
                out.data
                    .par_chunks_mut(chunk_len)
                    .enumerate()
                    .for_each(|(chunk_idx, chunk)| {
                        let col_start = chunk_idx * chunk_cols;
                        let ncols = chunk.len() / m;
                        // SAFETY: `self.data` is a valid, immutably-borrowed
                        // `m*k`-element column-major buffer (rs=1, cs=m).
                        // `other`'s submatrix starting at column `col_start`
                        // for `ncols` columns is in-bounds because
                        // `col_start + ncols <= n` (rayon's chunking can't
                        // overrun `out.data`'s own `m*n` length, and
                        // `chunk_len = m*chunk_cols` keeps `other`'s slice
                        // the same column count as `out`'s chunk). `chunk`
                        // is `rayon::par_chunks_mut`'s own disjoint,
                        // non-overlapping mutable slice, so no two workers
                        // ever write the same output element.
                        unsafe {
                            matrixmultiply::dgemm(
                                m,
                                k,
                                ncols,
                                1.0,
                                self.data.as_ptr(),
                                1,
                                m as isize,
                                other.data[col_start * k..].as_ptr(),
                                1,
                                k as isize,
                                0.0,
                                chunk.as_mut_ptr(),
                                1,
                                m as isize,
                            );
                        }
                    });
                return out;
            }
        }

        // SAFETY: `self.data` (`m*k` elements, rs=1/cs=m), `other.data`
        // (`k*n` elements, rs=1/cs=k), and `out.data` (`m*n` elements,
        // rs=1/cs=m) are each a single contiguous column-major buffer of
        // exactly the length `dgemm` will read/write for these strides and
        // dimensions -- no aliasing between `out` (freshly allocated here)
        // and either input.
        unsafe {
            matrixmultiply::dgemm(
                m,
                k,
                n,
                1.0,
                self.data.as_ptr(),
                1,
                m as isize,
                other.data.as_ptr(),
                1,
                k as isize,
                0.0,
                out.data.as_mut_ptr(),
                1,
                m as isize,
            );
        }
        out
    }

    /// Sum along an axis: `axis = 0` collapses rows → `(1, cols)`;
    /// `axis = 1` collapses columns → `(rows, 1)`.
    pub fn sum_axis(&self, axis: usize) -> Result<Matrix, ShapeError> {
        match axis {
            0 => {
                let mut out = vec![0.0; self.cols];
                for c in 0..self.cols {
                    let mut acc = 0.0;
                    for r in 0..self.rows {
                        acc += self.data[r + c * self.rows];
                    }
                    out[c] = acc;
                }
                Ok(Matrix::from_row(&out))
            }
            1 => {
                let mut out = vec![0.0; self.rows];
                for c in 0..self.cols {
                    for r in 0..self.rows {
                        out[r] += self.data[r + c * self.rows];
                    }
                }
                Ok(Matrix::from_column(&out))
            }
            other => Err(ShapeError::Axis(other)),
        }
    }

    /// Mean along an axis (see [`sum_axis`](Self::sum_axis)).
    pub fn mean_axis(&self, axis: usize) -> Result<Matrix, ShapeError> {
        let n = match axis {
            0 => self.rows,
            1 => self.cols,
            other => return Err(ShapeError::Axis(other)),
        }
        .max(1) as f64;
        Ok(self.sum_axis(axis)?.map(|x| x / n))
    }

    /// Broadcast-tile this matrix to an explicit target shape, used by the
    /// `value as matrix(r, c)` reshape contract (§8/§14).
    pub fn broadcast_to(&self, rows: usize, cols: usize) -> Result<Matrix, ShapeError> {
        // A length-matching flat reshape wins when the element count already
        // matches and neither source extent is a broadcastable 1.
        let template = Matrix::zeros(rows, cols);
        self.broadcast(&template, |a, _| a)
    }
}

/// Two extents broadcast when equal, or when either is `1`.
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

impl Display for Matrix {
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
                write!(f, "{}", self.data[r + c * self.rows])?;
            }
        }
        write!(f, "]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rows_is_column_major() {
        let m = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert_eq!(m.shape(), (2, 2));
        // column-major: [1,3,2,4]
        assert_eq!(m.as_slice(), &[1.0, 3.0, 2.0, 4.0]);
        assert_eq!(m.get(0, 1).unwrap(), 2.0);
        assert_eq!(m.get(1, 0).unwrap(), 3.0);
    }

    #[test]
    fn ragged_rows_are_rejected() {
        let err = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0]]).unwrap_err();
        assert!(matches!(err, ShapeError::RaggedRows { .. }));
    }

    #[test]
    fn eye_and_transpose() {
        let i = Matrix::eye(3);
        assert_eq!(i.get(1, 1).unwrap(), 1.0);
        assert_eq!(i.get(0, 1).unwrap(), 0.0);
        let m = Matrix::from_rows(&[vec![1.0, 2.0, 3.0]]).unwrap(); // 1x3
        let t = m.transpose();
        assert_eq!(t.shape(), (3, 1));
        assert_eq!(t.get(2, 0).unwrap(), 3.0);
    }

    #[test]
    fn matmul_reference() {
        let a = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        let b = Matrix::from_rows(&[vec![5.0, 6.0], vec![7.0, 8.0]]).unwrap();
        let c = a.matmul(&b).unwrap();
        // [[19,22],[43,50]]
        assert_eq!(c.get(0, 0).unwrap(), 19.0);
        assert_eq!(c.get(0, 1).unwrap(), 22.0);
        assert_eq!(c.get(1, 0).unwrap(), 43.0);
        assert_eq!(c.get(1, 1).unwrap(), 50.0);
    }

    #[test]
    fn matmul_does_not_silently_drop_a_nan_times_zero_term() {
        // matmul's `b == 0.0` fast-skip is only valid when `a * 0.0 == 0.0`
        // for every `a` in the contracted column -- true for any finite `a`,
        // but not for NaN/Infinity (`NaN * 0.0 == NaN`, not `0.0`). Before
        // this test, the skip fired unconditionally: row 0 of `a` is
        // `[NaN, 1.0]`, column 0 of `b` is `[0.0, 1.0]`, so the correct dot
        // product `NaN*0.0 + 1.0*1.0 == NaN` was silently computed as the
        // finite-looking `1.0` instead, because the `NaN*0.0` term was
        // skipped rather than added.
        let a = Matrix::from_rows(&[vec![f64::NAN, 1.0], vec![2.0, 3.0]]).unwrap();
        let b = Matrix::from_rows(&[vec![0.0, 1.0], vec![1.0, 0.0]]).unwrap();
        let c = a.matmul(&b).unwrap();
        assert!(c.get(0, 0).unwrap().is_nan(), "got {} instead of NaN", c.get(0, 0).unwrap());
        assert!(c.get(0, 1).unwrap().is_nan(), "got {} instead of NaN", c.get(0, 1).unwrap());
        // row 1 (`[2.0, 3.0]`, no NaN) must still take the fast path and be
        // arithmetically exact: [2,3].[0,1]=3, [2,3].[1,0]=2.
        assert_eq!(c.get(1, 0).unwrap(), 3.0);
        assert_eq!(c.get(1, 1).unwrap(), 2.0);
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn large_matmul_takes_the_parallel_path_and_still_matches_a_naive_reference() {
        // Big enough to clear PARALLEL_MATMUL_THRESHOLD (m*n*k = 600^3 =
        // 216,000,000 >= 1<<16) AND the `n >= current_num_threads` column
        // check `matmul` also requires (600 columns comfortably clears any
        // realistic thread count) via a square shape -- matching the real
        // 600x600 measurement already recorded in BACKLOG.md, so this test
        // keeps genuinely exercising `par_chunks_mut`, not just the serial
        // fallback every other matmul test hits.
        let n = 600;
        let a = Matrix::from_col_major(n, n, (0..n * n).map(|i| (i % 7) as f64 - 3.0).collect());
        let b = Matrix::from_col_major(n, n, (0..n * n).map(|i| (i % 5) as f64 - 2.0).collect());
        let got = a.matmul(&b).unwrap();

        // naive reference, independent of matmul's own column-chunking logic
        let mut want = vec![0.0; n * n];
        for r in 0..n {
            for c in 0..n {
                let mut acc = 0.0;
                for p in 0..n {
                    acc += a.get(r, p).unwrap() * b.get(p, c).unwrap();
                }
                want[r + c * n] = acc;
            }
        }
        for (i, &w) in want.iter().enumerate() {
            let (r, c) = (i % n, i / n);
            assert!((got.get(r, c).unwrap() - w).abs() < 1e-9, "mismatch at ({r},{c})");
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn thin_matmul_falls_back_to_serial_and_still_matches_a_naive_reference() {
        // The real MLP benchmark's own last layer
        // (`(2400,64)@(64,4)`, `benchmarks/mlp/bench.qu`'s Z3 = A2*W3+b3):
        // m*n*k = 2400*4*64 = 614400 clears PARALLEL_MATMUL_THRESHOLD, but
        // n=4 columns is (on any machine with more than 4 threads) below the
        // `n >= rayon::current_num_threads()` gate documented on `matmul`
        // (see the measurement table in its doc comment: this exact shape
        // measured ~1.56x *slower* under column-only rayon dispatch than
        // plain serial) -- so this takes the serial fallback despite the
        // large aggregate. Correctness is the point of this test, checked
        // against an independent naive triple loop; which internal path ran
        // is not directly observable from here, but the fallback is
        // exercised by construction on any machine with more than 4 threads.
        let (m, k, n) = (2400, 64, 4);
        let a = Matrix::from_col_major(m, k, (0..m * k).map(|i| ((i % 11) as f64 - 5.0) * 0.1).collect());
        let b = Matrix::from_col_major(k, n, (0..k * n).map(|i| ((i % 7) as f64 - 3.0) * 0.1).collect());
        let got = a.matmul(&b).unwrap();

        for c in 0..n {
            for r in 0..m {
                let mut acc = 0.0;
                for p in 0..k {
                    acc += a.get(r, p).unwrap() * b.get(p, c).unwrap();
                }
                assert!((got.get(r, c).unwrap() - acc).abs() < 1e-9, "mismatch at ({r},{c})");
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn enough_columns_matmul_takes_the_parallel_path_and_still_matches_a_naive_reference() {
        // The opposite corner from the test above: `n` derived from the
        // machine's own live thread count (not a hardcoded guess) so this
        // keeps genuinely exercising the `n >= current_num_threads` parallel
        // path documented on `matmul` no matter how many cores run it --
        // e.g. on the 16-thread machine this was measured on, `n=64` (bench
        // shape `(2400,20)@(20,64)`) measured ~2x faster than serial. `k` is
        // kept small like the real ML-layer shapes that motivated this gate,
        // `m` large enough to clear PARALLEL_MATMUL_THRESHOLD comfortably.
        // Checked against an independent naive triple loop.
        let n = rayon::current_num_threads().max(1);
        let (m, k) = (2400, 20);
        let a = Matrix::from_col_major(m, k, (0..m * k).map(|i| (i % 13) as f64 - 6.0).collect());
        let b = Matrix::from_col_major(k, n, (0..k * n).map(|i| (i % 9) as f64 - 4.0).collect());
        let got = a.matmul(&b).unwrap();

        for c in 0..n {
            for r in 0..m {
                let mut acc = 0.0;
                for p in 0..k {
                    acc += a.get(r, p).unwrap() * b.get(p, c).unwrap();
                }
                assert!((got.get(r, c).unwrap() - acc).abs() < 1e-6, "mismatch at ({r},{c})");
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn large_map_takes_the_parallel_path_and_still_matches_a_serial_reference() {
        // Computed from the threshold itself (not a hardcoded guess) so
        // this keeps genuinely exercising the `par_iter` path — not just
        // the serial fallback every other `map` test hits — no matter how
        // `PARALLEL_ELEMENTWISE_THRESHOLD` gets retuned later.
        let n = (Matrix::PARALLEL_ELEMENTWISE_THRESHOLD as f64).sqrt().ceil() as usize + 1;
        let m = Matrix::from_col_major(n, n, (0..n * n).map(|i| (i as f64) * 0.001 - 40.0).collect());
        let got = m.map(|x| x.sin() * x.cos() + x.abs());
        for (i, &x) in m.as_slice().iter().enumerate() {
            let want = x.sin() * x.cos() + x.abs();
            assert!((got.as_slice()[i] - want).abs() < 1e-12, "mismatch at flat index {i}");
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn large_broadcast_takes_the_parallel_path_and_still_matches_a_serial_reference() {
        let n = (Matrix::PARALLEL_ELEMENTWISE_THRESHOLD as f64).sqrt().ceil() as usize + 1;
        let a = Matrix::from_col_major(n, n, (0..n * n).map(|i| (i % 11) as f64 - 5.0).collect());
        let b = Matrix::from_col_major(n, n, (0..n * n).map(|i| (i % 13) as f64 - 6.0).collect());
        let got = a.broadcast(&b, |x, y| x * y + y.sin()).unwrap();
        for r in 0..n {
            for c in 0..n {
                let want = a.get(r, c).unwrap() * b.get(r, c).unwrap() + b.get(r, c).unwrap().sin();
                assert!((got.get(r, c).unwrap() - want).abs() < 1e-12, "mismatch at ({r},{c})");
            }
        }
    }

    #[test]
    fn matmul_shape_mismatch_errors() {
        let a = Matrix::zeros(2, 3);
        let b = Matrix::zeros(2, 2);
        assert!(matches!(a.matmul(&b), Err(ShapeError::MatMul { .. })));
    }

    /// Deterministic, not-all-zero/not-all-integer test data -- avoids the
    /// naive path's `b == 0.0` fast-skip trivially matching by never
    /// exercising the accumulation it skips.
    #[cfg(feature = "fast-matmul")]
    fn fast_matmul_test_matrix(rows: usize, cols: usize, seed: usize) -> Matrix {
        Matrix::from_col_major(
            rows,
            cols,
            (0..rows * cols)
                .map(|i| ((i + seed) % 17) as f64 * 0.31 - 2.5)
                .collect(),
        )
    }

    /// `matmul_fast` (the `matrixmultiply`-crate-backed GEMM path) must
    /// agree with `matmul_naive` (the portable triple loop) to tight float
    /// tolerance on every shape a real workload exercises: the exact
    /// `benchmarks/mlp/bench.qu` layer shapes, plus non-square and
    /// non-power-of-2 shapes those don't cover.
    #[cfg(feature = "fast-matmul")]
    #[test]
    fn fast_matmul_matches_naive_reference_on_mlp_and_other_shapes() {
        let shapes: &[(usize, usize, usize)] = &[
            // benchmarks/mlp/bench.qu's exact per-epoch layer shapes.
            (2400, 20, 64),
            (2400, 64, 32),
            (2400, 32, 4),
            // Non-square, non-power-of-2, and edge-ish shapes.
            (1, 1, 1),
            (1, 7, 1),
            (7, 1, 1),
            (3, 5, 2),
            (17, 13, 11),
            (100, 3, 100),
            (128, 129, 130),
            (200, 200, 200),
        ];
        for &(m, k, n) in shapes {
            let a = fast_matmul_test_matrix(m, k, 1);
            let b = fast_matmul_test_matrix(k, n, 5);
            let want = a.matmul_naive(&b, m, k, n).unwrap();
            let got = a.matmul_fast(&b, m, k, n);
            assert_eq!(got.shape(), want.shape(), "shape mismatch for ({m},{k},{n})");
            for c in 0..n {
                for r in 0..m {
                    let g = got.get(r, c).unwrap();
                    let w = want.get(r, c).unwrap();
                    assert!(
                        (g - w).abs() < 1e-9,
                        "mismatch at ({r},{c}) for shape ({m},{k},{n}): fast={g}, naive={w}"
                    );
                }
            }
        }
    }

    /// Same comparison via the public `matmul` entry point (not the two
    /// internal helpers directly), so this also exercises whichever path
    /// (`fast-matmul`'s parallel-chunked-gemm branch included, for large
    /// enough shapes) `matmul` itself actually dispatches to.
    #[cfg(feature = "fast-matmul")]
    #[test]
    fn matmul_public_entry_point_matches_naive_reference() {
        let shapes: &[(usize, usize, usize)] = &[(2400, 20, 64), (2400, 64, 32), (2400, 32, 4), (600, 600, 600)];
        for &(m, k, n) in shapes {
            let a = fast_matmul_test_matrix(m, k, 2);
            let b = fast_matmul_test_matrix(k, n, 9);
            let want = a.matmul_naive(&b, m, k, n).unwrap();
            let got = a.matmul(&b).unwrap();
            for c in 0..n {
                for r in 0..m {
                    let g = got.get(r, c).unwrap();
                    let w = want.get(r, c).unwrap();
                    assert!(
                        (g - w).abs() < 1e-9,
                        "mismatch at ({r},{c}) for shape ({m},{k},{n}): got={g}, want={w}"
                    );
                }
            }
        }
    }

    /// Honest size-sweep timing: `matmul_naive` (the pre-existing scalar
    /// triple loop, rayon-parallel where its own gates allow when
    /// `parallel` is also on) vs `matmul_fast` (this session's
    /// `matrixmultiply`-crate GEMM path) vs the public `matmul` dispatch
    /// (whichever of the two it actually picks), across small-to-large
    /// shapes plus the exact `benchmarks/mlp/bench.qu` layer shapes. Not a
    /// pass/fail assertion (crate-call overhead losing on tiny matrices is
    /// an expected, honestly-reported possibility, not a bug) — `#[ignore]`d
    /// so the normal suite stays fast; run explicitly:
    /// `cargo test -p qu-core --release --features fast-matmul,parallel
    /// matmul_size_sweep -- --ignored --nocapture`.
    #[cfg(feature = "fast-matmul")]
    #[test]
    #[ignore]
    fn matmul_size_sweep_naive_vs_fast() {
        use std::time::Instant;

        // (m, k, n, reps) — small shapes get more reps so the timer has
        // enough total wall-clock to be meaningful.
        let cases: &[(usize, usize, usize, u32)] = &[
            (4, 4, 4, 200_000),
            (8, 8, 8, 100_000),
            (16, 16, 16, 50_000),
            (32, 32, 32, 20_000),
            (64, 64, 64, 5_000),
            (128, 128, 128, 1_000),
            (256, 256, 256, 200),
            (600, 600, 600, 20),
            (1024, 1024, 1024, 5),
            // benchmarks/mlp/bench.qu's exact per-epoch layer shapes.
            (2400, 20, 64, 300),
            (2400, 64, 32, 300),
            (2400, 32, 4, 300),
        ];

        eprintln!("\nshape (m,k,n)        reps   naive       fast        dispatched  fast-vs-naive");
        for &(m, k, n, reps) in cases {
            let a = fast_matmul_test_matrix(m, k, 3);
            let b = fast_matmul_test_matrix(k, n, 11);

            // Warm up (thread pool spin-up, page faults) before timing.
            let _ = a.matmul_naive(&b, m, k, n).unwrap();
            let _ = a.matmul_fast(&b, m, k, n);
            let _ = a.matmul(&b).unwrap();

            let start = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(a.matmul_naive(&b, m, k, n).unwrap());
            }
            let naive_time = start.elapsed().as_secs_f64() / reps as f64;

            let start = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(a.matmul_fast(&b, m, k, n));
            }
            let fast_time = start.elapsed().as_secs_f64() / reps as f64;

            let start = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(a.matmul(&b).unwrap());
            }
            let dispatched_time = start.elapsed().as_secs_f64() / reps as f64;

            eprintln!(
                "({m:>5},{k:>4},{n:>4}) {reps:>7}  {:>9.3} us  {:>9.3} us  {:>9.3} us  {:>6.2}x",
                naive_time * 1e6,
                fast_time * 1e6,
                dispatched_time * 1e6,
                naive_time / fast_time,
            );
        }
    }

    #[test]
    fn broadcast_column_and_row() {
        // (2,1) column against (2,3) matrix stretches across columns.
        let col = Matrix::from_column(&[10.0, 20.0]);
        let m = Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let r = col.broadcast(&m, |a, b| a + b).unwrap();
        assert_eq!(r.shape(), (2, 3));
        assert_eq!(r.get(0, 2).unwrap(), 13.0);
        assert_eq!(r.get(1, 0).unwrap(), 24.0);
    }

    #[test]
    fn broadcast_incompatible_errors() {
        let a = Matrix::zeros(2, 3);
        let b = Matrix::zeros(3, 2);
        assert!(matches!(a.broadcast(&b, |x, _| x), Err(ShapeError::Broadcast { .. })));
    }

    /// The same-shape fast path (§ broadcast per-element branching,
    /// 2026-09-10) against a hand-derived reference, on a RECTANGULAR
    /// (non-square) shape -- `large_broadcast_...` above already covers
    /// the square case at parallel size, and `broadcast_column_and_row`
    /// above proves the shape guard does not wrongly intercept genuine
    /// broadcasting. This is the third leg: same-shape, but not square,
    /// so a bug that only shows up when rows != cols (an accidental
    /// row/col transposition in the fast path, say) would not hide behind
    /// symmetry.
    #[test]
    fn broadcast_same_shape_fast_path_matches_reference_on_a_rectangular_shape() {
        let (r, c) = (7, 13);
        let a = Matrix::from_col_major(r, c, (0..r * c).map(|i| (i as f64) * 0.37 - 3.0).collect());
        let b = Matrix::from_col_major(r, c, (0..r * c).map(|i| ((i as f64) * 0.19).cos()).collect());
        let got = a.broadcast(&b, |x, y| x * y - y).unwrap();
        assert_eq!(got.shape(), (r, c));
        for row in 0..r {
            for col in 0..c {
                let want = a.get(row, col).unwrap() * b.get(row, col).unwrap() - b.get(row, col).unwrap();
                assert!((got.get(row, col).unwrap() - want).abs() < 1e-12, "mismatch at ({row},{col})");
            }
        }
    }

    #[test]
    fn sum_and_mean_axes() {
        let m = Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let s0 = m.sum_axis(0).unwrap(); // (1,3) -> column sums
        assert_eq!(s0.shape(), (1, 3));
        assert_eq!(s0.as_slice(), &[5.0, 7.0, 9.0]);
        let s1 = m.sum_axis(1).unwrap(); // (2,1) -> row sums
        assert_eq!(s1.shape(), (2, 1));
        assert_eq!(s1.as_slice(), &[6.0, 15.0]);
        let mean0 = m.mean_axis(0).unwrap();
        assert_eq!(mean0.as_slice(), &[2.5, 3.5, 4.5]);
    }

    #[test]
    fn reshape_preserves_column_major_order() {
        let m = Matrix::from_column(&[1.0, 2.0, 3.0, 4.0]); // (4,1)
        let r = m.reshape(2, 2).unwrap();
        assert_eq!(r.as_slice(), &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(r.get(0, 1).unwrap(), 3.0);
        assert!(matches!(m.reshape(3, 2), Err(ShapeError::Reshape { .. })));
    }

    #[test]
    fn broadcast_to_tiles_a_row() {
        let row = Matrix::from_row(&[1.0, 2.0, 3.0]); // (1,3)
        let tiled = row.broadcast_to(2, 3).unwrap();
        assert_eq!(tiled.shape(), (2, 3));
        assert_eq!(tiled.row_vec(0).unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(tiled.row_vec(1).unwrap(), vec![1.0, 2.0, 3.0]);
    }
}
