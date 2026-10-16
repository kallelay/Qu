//! Shared kernels for the blocked dense decompositions (`dense_qr`,
//! `dense_eigsym`, `dense_svd`, `dense_eiggen`): a bounds-checked strided
//! GEMM that updates in place (so the trailing-matrix updates need no copies)
//! and a deferred, cache-blocked, row-parallel application of Givens rotation
//! sequences.
//!
//! Everything is column-major. All parallelism goes through the same rayon
//! feature flag as `Matrix::matmul`; without it every routine is serial and
//! produces bit-identical results (the parallel splits only ever divide
//! *independent* output columns / rows, never a reduction).

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Define a function whose body is compiled twice: once as written, once with
/// AVX2 + FMA enabled, and pick between them at run time. Every helper the
/// body calls in its hot loops must be `#[inline(always)]` to share the wider
/// instruction set.
///
/// The body is ordinary safe Rust -- no intrinsics, no raw pointers -- so the
/// only `unsafe` is the call into the `#[target_feature]` copy, made only after
/// `is_x86_feature_detected!` has confirmed the CPU supports it. Rust never
/// fuses `a * b + c` into an FMA on its own, so both copies perform exactly the
/// same floating-point operations in the same order and return bit-identical
/// results; the wider copy only issues them 4 lanes at a time. Parameters must
/// be plain identifiers (rebind with `let mut x = x;` inside if you need `mut`).
macro_rules! avx2_fn {
    ($(#[$meta:meta])* $vis:vis fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        $vis fn $name($($arg: $ty),*) $(-> $ret)? {
            #[inline(always)]
            fn generic($($arg: $ty),*) $(-> $ret)? $body
            #[cfg(target_arch = "x86_64")]
            {
                #[target_feature(enable = "avx2,fma")]
                unsafe fn fast($($arg: $ty),*) $(-> $ret)? {
                    generic($($arg),*)
                }
                if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
                    // SAFETY: the CPU features were just detected at run time.
                    return unsafe { fast($($arg),*) };
                }
            }
            generic($($arg),*)
        }
    };
}
pub(crate) use avx2_fn;

/// Number of worker threads the parallel kernels will use (1 without the
/// `parallel` feature).
#[inline]
pub fn threads() -> usize {
    #[cfg(feature = "parallel")]
    {
        rayon::current_num_threads().max(1)
    }
    #[cfg(not(feature = "parallel"))]
    {
        1
    }
}

/// `C = alpha * op(A) * op(B) + beta * C` for column-major operands with
/// explicit leading dimensions. `C` is `m x n`; `op(A)` is `m x k`; `op(B)`
/// is `k x n`. `a_t` / `b_t` say the operand is stored transposed (so `A` is
/// stored `k x m`, `B` stored `n x k`). Out-of-extent slices panic (checked,
/// not assumed). With `beta == 0` the previous contents of `C` are ignored.
#[allow(clippy::too_many_arguments)]
pub fn gemm(
    m: usize,
    n: usize,
    k: usize,
    alpha: f64,
    a: &[f64],
    a_ld: usize,
    a_t: bool,
    b: &[f64],
    b_ld: usize,
    b_t: bool,
    beta: f64,
    c: &mut [f64],
    c_ld: usize,
) {
    if m == 0 || n == 0 {
        return;
    }
    let need_c = (n - 1) * c_ld + m;
    assert!(c_ld >= m && c.len() >= need_c, "gemm: C extent");
    if k == 0 {
        if beta != 1.0 {
            for j in 0..n {
                for v in &mut c[j * c_ld..j * c_ld + m] {
                    *v = if beta == 0.0 { 0.0 } else { *v * beta };
                }
            }
        }
        return;
    }
    let need_a = if a_t { (m - 1) * a_ld + k } else { (k - 1) * a_ld + m };
    let need_b = if b_t { (k - 1) * b_ld + n } else { (n - 1) * b_ld + k };
    assert!(a.len() >= need_a && b.len() >= need_b, "gemm: operand extent");
    assert!(if a_t { a_ld >= k } else { a_ld >= m }, "gemm: A ld");
    assert!(if b_t { b_ld >= n } else { b_ld >= k }, "gemm: B ld");

    #[cfg(feature = "parallel")]
    {
        let nt = threads();
        let work = m.saturating_mul(n).saturating_mul(k);
        if nt > 1 && work >= (1 << 24) && n >= 2 * nt {
            // Split the output columns into contiguous chunks. Each chunk is
            // an independent sub-product, so the result does not depend on
            // the chunking.
            let per = n.div_ceil(nt * 2).max(8);
            let c = &mut c[..need_c];
            c.par_chunks_mut(c_ld * per).enumerate().for_each(|(ci, chunk)| {
                let j0 = ci * per;
                let nc = per.min(n - j0);
                let boff = if b_t { j0 } else { j0 * b_ld };
                gemm_serial(m, nc, k, alpha, a, a_ld, a_t, &b[boff..], b_ld, b_t, beta, chunk, c_ld);
            });
            return;
        }
    }
    gemm_serial(m, n, k, alpha, a, a_ld, a_t, b, b_ld, b_t, beta, c, c_ld);
}

#[allow(clippy::too_many_arguments)]
pub fn gemm_serial(
    m: usize,
    n: usize,
    k: usize,
    alpha: f64,
    a: &[f64],
    a_ld: usize,
    a_t: bool,
    b: &[f64],
    b_ld: usize,
    b_t: bool,
    beta: f64,
    c: &mut [f64],
    c_ld: usize,
) {
    if m == 0 || n == 0 || k == 0 {
        // Degenerate: defer to the checked entry point's handling.
        return gemm_degenerate(m, n, k, beta, c, c_ld);
    }
    let need_c = (n - 1) * c_ld + m;
    let need_a = if a_t { (m - 1) * a_ld + k } else { (k - 1) * a_ld + m };
    let need_b = if b_t { (k - 1) * b_ld + n } else { (n - 1) * b_ld + k };
    assert!(c.len() >= need_c && a.len() >= need_a && b.len() >= need_b, "gemm: extent");
    let (rsa, csa) = if a_t { (a_ld as isize, 1isize) } else { (1isize, a_ld as isize) };
    let (rsb, csb) = if b_t { (b_ld as isize, 1isize) } else { (1isize, b_ld as isize) };
    #[cfg(feature = "fast-matmul")]
    {
        // SAFETY: the three extents were asserted above to cover every index
        // `dgemm` touches for these dimensions and strides (A: m x k, B: k x
        // n, C: m x n, C strictly disjoint from A and B because it is a
        // `&mut` borrow distinct from the shared ones).
        unsafe {
            matrixmultiply::dgemm(
                m,
                k,
                n,
                alpha,
                a.as_ptr(),
                rsa,
                csa,
                b.as_ptr(),
                rsb,
                csb,
                beta,
                c.as_mut_ptr(),
                1,
                c_ld as isize,
            );
        }
    }
    #[cfg(not(feature = "fast-matmul"))]
    {
        let (rsa, csa, rsb, csb) = (rsa as usize, csa as usize, rsb as usize, csb as usize);
        for j in 0..n {
            let col = &mut c[j * c_ld..j * c_ld + m];
            if beta == 0.0 {
                col.fill(0.0);
            } else if beta != 1.0 {
                for v in col.iter_mut() {
                    *v *= beta;
                }
            }
            for p in 0..k {
                let bv = alpha * b[p * rsb + j * csb];
                if bv == 0.0 {
                    continue;
                }
                for (i, v) in col.iter_mut().enumerate() {
                    *v += a[i * rsa + p * csa] * bv;
                }
            }
        }
    }
}

fn gemm_degenerate(m: usize, n: usize, k: usize, beta: f64, c: &mut [f64], c_ld: usize) {
    if m == 0 || n == 0 || k != 0 || beta == 1.0 {
        return;
    }
    for j in 0..n {
        for v in &mut c[j * c_ld..j * c_ld + m] {
            *v = if beta == 0.0 { 0.0 } else { *v * beta };
        }
    }
}

// ---------------------------------------------------------------------------
// Deferred Givens rotation sequences
// ---------------------------------------------------------------------------

/// A rotation of the column pair `(a, b)` (`a < b`, usually `b = a + 1`) of a
/// matrix: `x_a' = c x_a + s x_b`, `x_b' = -s x_a + c x_b`, applied down every row.
#[derive(Clone, Copy, Debug)]
pub struct Rot {
    pub a: u32,
    pub b: u32,
    pub c: f64,
    pub s: f64,
}

/// A matrix kept in row blocks so rotation sequences can be applied to each
/// block independently (cache-resident, one block per task). Every row sees
/// exactly the same operations in the same order as a plain sequential
/// application, so the result is bit-identical to it.
pub struct RotTarget {
    rows: usize,
    cols: usize,
    blocks: Vec<Vec<f64>>,
    queue: Vec<Rot>,
    /// A batch being applied by a background task: the blocks come back
    /// through this channel when it is done. At most one is in flight, and
    /// the next batch only starts after it has finished, so batches apply in
    /// the order they were queued.
    #[cfg(feature = "parallel")]
    pending: Option<std::sync::mpsc::Receiver<Vec<Vec<f64>>>>,
}

/// Rotations are flushed to the blocks in batches of this many. Smaller
/// batches overlap more of the (scalar) generation of rotations with their
/// (parallel) application.
const FLUSH: usize = 1 << 16;

/// Apply `rots` to every block (rows are independent, so blocks run in parallel).
fn apply_all(blocks: &mut [Vec<f64>], cols: usize, rots: &[Rot], rows: usize) {
    let apply = |blk: &mut Vec<f64>| {
        let h = blk.len() / cols.max(1);
        apply_rots_block(blk, h, rots);
    };
    #[cfg(feature = "parallel")]
    {
        if rows * rots.len() >= 1 << 22 && blocks.len() > 1 {
            blocks.par_iter_mut().for_each(apply);
            return;
        }
    }
    let _ = rows;
    blocks.iter_mut().for_each(apply);
}

impl RotTarget {
    /// Take ownership of a column-major `rows x cols` matrix.
    pub fn new(data: &[f64], rows: usize, cols: usize) -> Self {
        assert_eq!(data.len(), rows * cols);
        let nt = threads();
        // Keep a block's working set (rb * cols doubles) around 256 KiB and
        // leave at least a few blocks per thread when the matrix is big
        // enough to be worth splitting.
        let mut rb = (32768 / cols.max(1)).clamp(16, 128);
        if nt > 1 {
            rb = rb.min(rows.div_ceil(nt * 2).max(8));
        }
        let mut rb = rb.min(rows.max(1));
        if rb >= CHAIN_W {
            rb -= rb % CHAIN_W; // whole chain-kernel strips
        }
        let mut blocks = Vec::with_capacity(rows.div_ceil(rb.max(1)));
        let mut r0 = 0;
        while r0 < rows {
            let h = rb.min(rows - r0);
            let mut blk = vec![0.0; h * cols];
            for j in 0..cols {
                blk[j * h..(j + 1) * h].copy_from_slice(&data[r0 + j * rows..r0 + j * rows + h]);
            }
            blocks.push(blk);
            r0 += h;
        }
        Self {
            rows,
            cols,
            blocks,
            queue: Vec::new(),
            #[cfg(feature = "parallel")]
            pending: None,
        }
    }

    /// Queue `a' = c a + s b`, `b' = -s a + c b` on columns `(idx, idx+1)`.
    #[inline]
    pub fn push(&mut self, idx: usize, c: f64, s: f64) {
        self.push_pair(idx, idx + 1, c, s);
    }

    /// Queue the same rotation on an arbitrary column pair `a < b`.
    #[inline]
    pub fn push_pair(&mut self, a: usize, b: usize, c: f64, s: f64) {
        debug_assert!(a < b && b < self.cols);
        self.queue.push(Rot { a: a as u32, b: b as u32, c, s });
        if self.queue.len() >= FLUSH {
            self.flush(true);
        }
    }

    /// Wait for the batch in flight (if any) and take the blocks back.
    fn settle(&mut self) {
        #[cfg(feature = "parallel")]
        if let Some(rx) = self.pending.take() {
            self.blocks = rx.recv().expect("rotation batch task failed");
        }
    }

    /// Apply the queued batch. With `background` (and the `parallel`
    /// feature, a pool of more than one thread, and a batch worth the
    /// hand-off) the batch is applied by a pool task while the caller goes on
    /// generating the next one.
    fn flush(&mut self, background: bool) {
        if self.queue.is_empty() {
            return;
        }
        self.settle();
        let rots = std::mem::take(&mut self.queue);
        let mut blocks = std::mem::take(&mut self.blocks);
        let cols = self.cols;
        let rows = self.rows;
        #[cfg(feature = "parallel")]
        {
            if background && threads() > 1 && rows * rots.len() >= 1 << 22 {
                let (tx, rx) = std::sync::mpsc::channel();
                rayon::spawn(move || {
                    apply_all(&mut blocks, cols, &rots, rows);
                    let _ = tx.send(blocks);
                });
                self.pending = Some(rx);
                self.queue = Vec::with_capacity(FLUSH);
                return;
            }
        }
        let _ = background;
        apply_all(&mut blocks, cols, &rots, rows);
        self.blocks = blocks;
        self.queue = rots;
        self.queue.clear();
    }

    /// Apply everything queued and return the column-major matrix.
    pub fn finish(mut self) -> Vec<f64> {
        self.flush(false);
        self.settle();
        let mut out = vec![0.0; self.rows * self.cols];
        let mut r0 = 0;
        for blk in &self.blocks {
            let h = blk.len() / self.cols.max(1);
            for j in 0..self.cols {
                out[r0 + j * self.rows..r0 + j * self.rows + h].copy_from_slice(&blk[j * h..(j + 1) * h]);
            }
            r0 += h;
        }
        out
    }

    /// Negate column `j` (used to fix the sign of a singular vector). Applied
    /// after everything queued so far.
    pub fn negate_column(&mut self, j: usize) {
        self.flush(false);
        self.settle();
        for blk in &mut self.blocks {
            let h = blk.len() / self.cols.max(1);
            for v in &mut blk[j * h..(j + 1) * h] {
                *v = -*v;
            }
        }
    }
}

fn apply_rots_block(blk: &mut [f64], h: usize, rots: &[Rot]) {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the avx2 target feature was just detected at run time;
            // the function body is the same safe slice code as the generic
            // version (no raw pointers, no intrinsics), only compiled with
            // 256-bit vectors enabled. Rust never fuses a*b+c into an FMA on
            // its own, so the arithmetic (and every rounding) is identical.
            unsafe { apply_rots_block_avx2(blk, h, rots) };
            return;
        }
    }
    apply_rots_block_generic(blk, h, rots);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn apply_rots_block_avx2(blk: &mut [f64], h: usize, rots: &[Rot]) {
    apply_rots_block_generic(blk, h, rots);
}

/// Rows processed together by the chain kernel (4 AVX2 vectors).
const CHAIN_W: usize = 16;

/// One rotation on rows `r0..r1` of the block.
#[inline(always)]
fn apply_one_rot(blk: &mut [f64], h: usize, r: &Rot, r0: usize, r1: usize) {
    let (i, j) = (r.a as usize, r.b as usize);
    let (left, right) = blk.split_at_mut(j * h);
    let x = &mut left[i * h + r0..i * h + r1];
    let y = &mut right[r0..r1];
    let (c, s) = (r.c, r.s);
    for (a, b) in x.iter_mut().zip(y.iter_mut()) {
        let (av, bv) = (*a, *b);
        *a = av * c + s * bv;
        *b = bv * c - s * av;
    }
}

/// Apply a run of rotations on consecutive adjacent column pairs
/// `(a0, a0+1), (a0+1, a0+2), ...` to rows `r0..r0+CHAIN_W`, keeping the
/// column that feeds the next rotation in registers (one load and one store
/// per rotation instead of two and two). Same arithmetic, same order, per row.
#[inline(always)]
fn apply_chain(blk: &mut [f64], h: usize, run: &[Rot], r0: usize) {
    let a0 = run[0].a as usize;
    let mut carry = [0.0f64; CHAIN_W];
    carry.copy_from_slice(&blk[a0 * h + r0..a0 * h + r0 + CHAIN_W]);
    for (q, r) in run.iter().enumerate() {
        let (c, s) = (r.c, r.s);
        let nx_off = (a0 + q + 1) * h + r0;
        let mut nx = [0.0f64; CHAIN_W];
        nx.copy_from_slice(&blk[nx_off..nx_off + CHAIN_W]);
        let mut out = [0.0f64; CHAIN_W];
        for l in 0..CHAIN_W {
            out[l] = carry[l] * c + s * nx[l];
            carry[l] = nx[l] * c - s * carry[l];
        }
        let cur = (a0 + q) * h + r0;
        blk[cur..cur + CHAIN_W].copy_from_slice(&out);
    }
    let last = (a0 + run.len()) * h + r0;
    blk[last..last + CHAIN_W].copy_from_slice(&carry);
}

#[inline(always)]
fn apply_rots_block_generic(blk: &mut [f64], h: usize, rots: &[Rot]) {
    let full = h - h % CHAIN_W; // rows handled by the chain kernel
    let mut i = 0;
    while i < rots.len() {
        // Length of the run of adjacent-pair rotations starting at i.
        let mut len = 1;
        if full > 0 && rots[i].b == rots[i].a + 1 {
            while i + len < rots.len() && rots[i + len].a == rots[i + len - 1].a + 1 && rots[i + len].b == rots[i + len].a + 1 {
                len += 1;
            }
        }
        let run = &rots[i..i + len];
        if full > 0 && len >= 2 {
            let mut r0 = 0;
            while r0 < full {
                apply_chain(blk, h, run, r0);
                r0 += CHAIN_W;
            }
            if full < h {
                for r in run {
                    apply_one_rot(blk, h, r, full, h);
                }
            }
        } else {
            for r in run {
                apply_one_rot(blk, h, r, 0, h);
            }
        }
        i += len;
    }
}

// ---------------------------------------------------------------------------
// Column-parallel helpers
// ---------------------------------------------------------------------------

/// Call `f(col_index, column)` for the columns `c0..c1` of the column-major
/// array `a` (leading dimension `ld`, so each `column` is a `&mut [f64]` of
/// length `ld`). Columns are independent, so running them in parallel gives
/// the same result as running them in order.
pub fn for_each_col_mut<F>(a: &mut [f64], ld: usize, c0: usize, c1: usize, parallel: bool, f: F)
where
    F: Fn(usize, &mut [f64]) + Sync + Send,
{
    let region = &mut a[c0 * ld..c1 * ld];
    #[cfg(feature = "parallel")]
    {
        if parallel && threads() > 1 && c1 - c0 >= 8 {
            let per = (c1 - c0).div_ceil(threads() * 4).max(2);
            region.par_chunks_mut(ld * per).enumerate().for_each(|(ci, chunk)| {
                for (q, col) in chunk.chunks_mut(ld).enumerate() {
                    f(c0 + ci * per + q, col);
                }
            });
            return;
        }
    }
    let _ = parallel;
    for (q, col) in region.chunks_mut(ld).enumerate() {
        f(c0 + q, col);
    }
}

/// Split the `nc` columns (leading dimension `ld`, `rows` rows each, starting
/// at the beginning of `c`) into contiguous groups and call
/// `f(first_col, ncols, group_slice)` for each; groups run in parallel when
/// `parallel`. The group count is `~2 x threads` (at least `min_cols` wide)
/// so the amount of dispatching stays small.
pub fn for_each_col_group<F>(c: &mut [f64], ld: usize, rows: usize, nc: usize, min_cols: usize, parallel: bool, f: F)
where
    F: Fn(usize, usize, &mut [f64]) + Sync + Send,
{
    if nc == 0 {
        return;
    }
    let need = (nc - 1) * ld + rows;
    let c = &mut c[..need];
    #[cfg(feature = "parallel")]
    {
        let nt = threads();
        if parallel && nt > 1 && nc >= 2 * min_cols {
            let per = nc.div_ceil(nt * 2).max(min_cols);
            c.par_chunks_mut(ld * per).enumerate().for_each(|(ci, chunk)| {
                let j0 = ci * per;
                f(j0, per.min(nc - j0), chunk);
            });
            return;
        }
    }
    let _ = (parallel, min_cols);
    f(0, nc, c);
}

/// Fill `out[i]` with `f(i)` for every `i`, in parallel above `parallel`
/// being true (independent outputs: same result either way).
pub fn fill_with<F>(out: &mut [f64], parallel: bool, f: F)
where
    F: Fn(usize) -> f64 + Sync + Send,
{
    #[cfg(feature = "parallel")]
    {
        if parallel && threads() > 1 && out.len() >= 64 {
            let per = out.len().div_ceil(threads() * 4).max(16);
            out.par_chunks_mut(per).enumerate().for_each(|(ci, chunk)| {
                for (q, o) in chunk.iter_mut().enumerate() {
                    *o = f(ci * per + q);
                }
            });
            return;
        }
    }
    let _ = parallel;
    for (i, o) in out.iter_mut().enumerate() {
        *o = f(i);
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Transpose a column-major `r x c` matrix into a column-major `c x r` one.
pub fn transpose(a: &[f64], r: usize, c: usize) -> Vec<f64> {
    let mut out = vec![0.0; r * c];
    const B: usize = 32;
    let mut j0 = 0;
    while j0 < c {
        let j1 = (j0 + B).min(c);
        let mut i0 = 0;
        while i0 < r {
            let i1 = (i0 + B).min(r);
            for j in j0..j1 {
                for i in i0..i1 {
                    out[j + i * c] = a[i + j * r];
                }
            }
            i0 = i1;
        }
        j0 = j1;
    }
    out
}

/// Euclidean norm of a slice (plain sum of squares, like the reference
/// implementation this replaces).
#[inline(always)]
pub fn norm2(x: &[f64]) -> f64 {
    let mut s = [0.0f64; 4];
    let mut chunks = x.chunks_exact(4);
    for c in &mut chunks {
        for l in 0..4 {
            s[l] += c[l] * c[l];
        }
    }
    let mut t = (s[0] + s[1]) + (s[2] + s[3]);
    for v in chunks.remainder() {
        t += v * v;
    }
    t.sqrt()
}

/// Dot product with four accumulators (vectorizes).
#[inline(always)]
pub fn dot(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    let (x, y) = (&x[..n], &y[..n]);
    let mut s = [0.0f64; 4];
    let mut xc = x.chunks_exact(4);
    let mut yc = y.chunks_exact(4);
    for (a, b) in (&mut xc).zip(&mut yc) {
        for l in 0..4 {
            s[l] += a[l] * b[l];
        }
    }
    let mut t = (s[0] + s[1]) + (s[2] + s[3]);
    for (a, b) in xc.remainder().iter().zip(yc.remainder()) {
        t += a * b;
    }
    t
}

/// `y += alpha * x`.
#[inline(always)]
pub fn axpy(alpha: f64, x: &[f64], y: &mut [f64]) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += alpha * xi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(m: usize, n: usize, k: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
        let mut c = vec![0.0; m * n];
        for j in 0..n {
            for p in 0..k {
                for i in 0..m {
                    c[i + j * m] += a[i + p * m] * b[p + j * k];
                }
            }
        }
        c
    }

    fn pseudo(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect()
    }

    #[test]
    fn gemm_matches_naive_with_strides_and_transposes() {
        for &(m, n, k) in &[(1, 1, 1), (5, 7, 3), (40, 130, 17), (200, 300, 64)] {
            let a = pseudo(m * k, 1);
            let b = pseudo(k * n, 2);
            let want = naive(m, n, k, &a, &b);
            let mut c = vec![0.0; m * n];
            gemm(m, n, k, 1.0, &a, m, false, &b, k, false, 0.0, &mut c, m);
            for (x, y) in c.iter().zip(&want) {
                assert!((x - y).abs() < 1e-12);
            }
            // transposed operands: store A^T and B^T explicitly
            let at = transpose(&a, m, k);
            let bt = transpose(&b, k, n);
            let mut c2 = vec![0.5; m * n];
            gemm(m, n, k, -1.0, &at, k, true, &bt, n, true, 1.0, &mut c2, m);
            for (x, y) in c2.iter().zip(&want) {
                assert!((x - (0.5 - y)).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn gemm_into_a_sub_block_leaves_the_rest_alone() {
        let (ld, m, n, k) = (10, 4, 3, 5);
        let a = pseudo(m * k, 3);
        let b = pseudo(k * n, 4);
        let mut big = vec![7.0; ld * 6];
        // sub-block at rows 2.., columns 1..
        let off = 2 + ld;
        gemm(m, n, k, 1.0, &a, m, false, &b, k, false, 0.0, &mut big[off..], ld);
        let want = naive(m, n, k, &a, &b);
        for j in 0..6 {
            for i in 0..ld {
                let v = big[i + j * ld];
                if (2..2 + m).contains(&i) && (1..1 + n).contains(&j) {
                    assert!((v - want[(i - 2) + (j - 1) * m]).abs() < 1e-12);
                } else {
                    assert_eq!(v, 7.0);
                }
            }
        }
    }

    #[test]
    fn mixed_chains_and_arbitrary_pairs_equal_sequential_application() {
        for &(rows, cols) in &[(150usize, 40usize), (16, 9), (17, 9), (48, 25), (7, 12), (300, 31)] {
            let z0 = pseudo(rows * cols, 21);
            let mut want = z0.clone();
            let mut t = RotTarget::new(&z0, rows, cols);
            let ang = pseudo(900, 22);
            let pick = pseudo(900, 23);
            let mut q = 0;
            while q < ang.len() {
                // a chain of random length, or a single arbitrary pair
                let chain = ((pick[q].abs() * 40.0) as usize) % 12;
                let (a, len) = if chain >= 2 {
                    let a = (pick[(q + 1) % 900].abs() * 100.0) as usize % (cols - 1);
                    (a, chain.min(cols - 1 - a))
                } else {
                    (0, 0)
                };
                let mut items: Vec<(usize, usize, f64)> = Vec::new();
                if len >= 2 {
                    for k in 0..len {
                        items.push((a + k, a + k + 1, ang[(q + k) % 900]));
                    }
                } else {
                    let i = (pick[(q + 2) % 900].abs() * 97.0) as usize % (cols - 1);
                    let j = i + 1 + (pick[(q + 3) % 900].abs() * 53.0) as usize % (cols - 1 - i);
                    items.push((i, j, ang[q]));
                }
                q += items.len().max(1);
                for (i, j, th) in items {
                    let (c, s) = ((th * 3.0).cos(), (th * 3.0).sin());
                    if j == i + 1 {
                        t.push(i, c, s);
                    } else {
                        t.push_pair(i, j, c, s);
                    }
                    for r in 0..rows {
                        let (x, y) = (want[r + i * rows], want[r + j * rows]);
                        want[r + i * rows] = x * c + s * y;
                        want[r + j * rows] = y * c - s * x;
                    }
                }
            }
            assert_eq!(t.finish(), want, "{rows}x{cols}");
        }
    }

    #[test]
    fn rotation_queue_equals_sequential_application() {
        let (rows, cols) = (150, 40);
        let z0 = pseudo(rows * cols, 9);
        let mut want = z0.clone();
        let mut t = RotTarget::new(&z0, rows, cols);
        let cs = pseudo(500, 10);
        for (q, v) in cs.iter().enumerate() {
            let idx = q % (cols - 1);
            let th = v * 3.0;
            let (c, s) = (th.cos(), th.sin());
            t.push(idx, c, s);
            for r in 0..rows {
                let (a, b) = (want[r + idx * rows], want[r + (idx + 1) * rows]);
                want[r + idx * rows] = a * c + s * b;
                want[r + (idx + 1) * rows] = b * c - s * a;
            }
        }
        let got = t.finish();
        assert_eq!(got, want);
    }
}
