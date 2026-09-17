//! One-time, lazy, **process-global** CPU-vs-GPU speed probe (§ GPU offload
//! threshold, 2026-08-27 — replaces the old hand-picked `GPU_GRAM_THRESHOLD`
//! constant everywhere it was used).
//!
//! **Why this exists.** Every GPU-dispatch decision point in `lib.rs`
//! (`tensor_binop_forward`'s tracked matmul, `gram_matrix`, `svm_cross_term`)
//! used to compare `m*n*k` against a single hardcoded `1 << 27` guess. That
//! number came from ONE session's measurement on ONE machine — real GPU/CPU
//! relative speed varies hugely by hardware (a fast CPU + slow/integrated
//! GPU makes the guess wrong in one direction, a fast discrete GPU + modest
//! CPU makes it wrong in the other). This module replaces the guess with a
//! real measurement, taken once per process, on whatever machine is actually
//! running.
//!
//! **Design.**
//! - **Lazy**: nothing runs until [`probe`]'s first call — a `qu` process
//!   that never touches a GPU-eligible op (the overwhelming majority: this
//!   whole module only compiles under `--features gpu` to begin with) never
//!   pays the probe's cost. First call is typically the first tracked
//!   `param()`-matmul, `gram_matrix`/`svm_cross_term` invocation, or an
//!   explicit `gpu_matmul(...)` call.
//! - **Once per PROCESS, not per `Interp`**: `spawn(...)` (see `lib.rs`'s
//!   `"spawn"` builtin) creates a brand-new `Interp::new()` on its own OS
//!   thread for every worker — a per-`Interp` cache (which is what the old
//!   `gpu_context: Option<qu_gpu::GpuContext>` field on `Interp` was) would
//!   mean every spawned worker re-runs the ~600ms-plus adapter/device/
//!   shader-compile setup AND re-races CPU vs GPU at 4 sizes, wastefully,
//!   the first time it touches a GPU-eligible op. A `std::sync::OnceLock`
//!   at module scope runs the whole probe (including acquiring the actual
//!   `qu_gpu::GpuContext`) at most once for the lifetime of the process,
//!   and every `Interp`/thread that calls [`probe`] shares the SAME
//!   `Arc<qu_gpu::GpuContext>` afterward — cheap to read (one atomic-backed
//!   `OnceLock::get_or_init` check plus an `Arc` clone), safe to call from
//!   any thread (`wgpu::Device`/`Queue` are themselves `Send + Sync`, the
//!   same cross-thread-use guarantee any multi-threaded wgpu renderer
//!   already relies on).
//! - **Graceful no-GPU fallback**: if `qu_gpu::GpuContext::new_blocking()`
//!   itself fails (no adapter, driver issue, or the crate simply wasn't
//!   built with `--features gpu` — in which case this whole module isn't
//!   compiled in at all), [`probe`] caches `context: None` and
//!   `crossover: usize::MAX` (i.e. "never worth it") ONCE, the same as any
//!   other result — every caller's own `Option`-returning dispatch check
//!   (`try_gpu_tracked_matmul` returning `None`, `gram_matrix`/
//!   `svm_cross_term`'s own `if let Some(ctx) = ...`) already falls back to
//!   the plain CPU path on `None`, so this never crashes or hangs the
//!   interpreter, and never re-attempts the failed probe on a later call.
//!
//! **Benchmark.** [`run_probe_with`] races a real
//! `qu_core::matrix::Matrix::matmul` (CPU: SIMD GEMM + rayon, see
//! `qu-core`'s own `Cargo.toml` doc comment) against a real
//! `qu_gpu::GpuContext::matmul` (the SAME naive one-thread-per-output-
//! element compute shader every GPU dispatch site already uses) at four
//! square sizes (128/256/512/1024 -- straddling the old `1 << 27` (~512^3)
//! guess from both sides, so the probe can confirm or correct it either
//! way), timed with `Instant::now()` around EXACTLY the call each real
//! call site makes (`Matrix::matmul` / `GpuContext::matmul`, including the
//! GPU path's buffer upload + dispatch + submit + blocking map-readback --
//! the real, size-independent dispatch/transfer overhead a production call
//! also pays, not a warm-loop microbenchmark that would hide it). The
//! smallest size where GPU actually measured faster becomes the crossover,
//! floored at [`MIN_SANE_CROSSOVER`] so a single noisy small-size sample
//! can never send trivially small ops to the GPU. No ceiling is needed the
//! other direction: a larger crossover only means GPU offload triggers
//! less often, which is always safe (leaves performance on the table at
//! worst, never wrong), unlike a too-low floor which would actively slow
//! small ops down with pure dispatch overhead.
//!
//! **Testability** (`run_probe_with`, [`would_use_gpu`], [`record_dispatch`]/
//! [`dispatch_count`]) is deliberately split out from the cached
//! [`probe`] entry point so the fallback/routing LOGIC is unit-testable
//! without needing to fake absent hardware, and so real dispatches can be
//! counted from an integration test that DOES have hardware — see this
//! module's own `#[cfg(test)]` block and `lib.rs`'s
//! `tracked_matmul_routes_to_gpu_only_above_measured_crossover` test.

use qu_core::matrix::Matrix;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// Square sizes (`m = n = k`) raced against each other to find the real
/// crossover — see this module's own doc comment for why these four.
const PROBE_SIZES: [usize; 4] = [128, 256, 512, 1024];

/// Hard floor on the measured crossover: even an arbitrarily fast GPU still
/// pays a real, size-independent buffer-upload/dispatch/submit/map-readback
/// round trip (this is exactly what dominates the probe's own smallest-size
/// sample) — below this many multiply-adds, offloading is never worth it
/// regardless of what a single noisy timing sample suggests. ~1M
/// multiply-adds (e.g. a 100x100x100 matmul) is comfortably beneath every
/// size this session's own ML benchmark suite produces on the CPU path by
/// default, so this floor costs nothing in practice — it only guards
/// against a spuriously-low measured crossover.
const MIN_SANE_CROSSOVER: usize = 1 << 20;

/// Counts how many times [`build_probe`]'s body has actually run — used
/// ONLY to verify "the expensive probe runs at most once per process" from
/// a test ([`probe_call_count`]); never consulted by the dispatch logic
/// itself.
static PROBE_RUNS: AtomicUsize = AtomicUsize::new(0);

// `record_dispatch`/`dispatch_count` are deliberately **thread-local**, not
// a shared process-global counter: `cargo test` runs different `#[test]`
// functions concurrently on their own OS threads within the SAME process,
// and the `gpu` feature build now routes noticeably MORE matmuls to real
// GPU dispatch than the old, much higher, hardcoded `GPU_GRAM_THRESHOLD`
// did (see `lib.rs`'s `tracked_matmul_routes_to_gpu_only_above_measured_
// crossover` — the actual measured crossover on the dev RTX 5080 came out
// well under the old `1 << 27` guess). A single shared atomic counter would
// make any test doing an exact before/after delta around "did MY operation
// dispatch" racy against unrelated tests dispatching concurrently on other
// threads — confirmed the hard way while writing this module (see IMPL.md's
// dated entry). Scoping the count per-thread instead means each test's own
// thread only ever observes ITS OWN dispatches, however many other tests
// are dispatching in parallel.
thread_local! {
    static LOCAL_DISPATCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static GPU_PROBE: OnceLock<GpuProbeResult> = OnceLock::new();

/// The measured result of racing CPU `Matrix::matmul` against
/// `qu_gpu::GpuContext::matmul`, once per process. See this module's own
/// doc comment for the full design.
pub struct GpuProbeResult {
    /// `Some` iff a real GPU adapter/device was successfully acquired.
    /// `Arc`-wrapped so every `Interp` (including short-lived ones spun up
    /// inside `spawn(...)` worker threads) shares the SAME device/pipeline
    /// objects instead of paying wgpu's adapter/device/shader-compile setup
    /// cost again.
    pub context: Option<Arc<qu_gpu::GpuContext>>,
    /// Human-readable reason `context` is `None`, when it is — surfaced by
    /// the explicit `gpu_matmul` builtin so a user still gets a clear error
    /// message (matching this builtin's pre-existing behavior) instead of a
    /// bare "no GPU" with no detail.
    pub init_error: Option<String>,
    /// The measured `m*n*k` crossover: at or above this many multiply-adds
    /// worth of work, GPU dispatch (buffer upload + compute + readback
    /// included) measured faster than `Matrix::matmul` on THIS machine.
    /// `usize::MAX` when no GPU is available, or GPU never won at any
    /// tested size.
    pub crossover: usize,
    /// `(m*n*k, cpu_seconds, gpu_seconds)` at every size actually raced —
    /// kept for introspection (`lib.rs`'s `"gpu_probe_info"` builtin,
    /// IMPL.md's dated entry, and this module's own tests), not consulted
    /// by the crossover decision again once computed.
    pub samples: Vec<(usize, f64, f64)>,
}

impl GpuProbeResult {
    /// The actual dispatch decision every call site should make: GPU is
    /// even a candidate at all (`context.is_some()`), AND `work` (each
    /// call site's own `m*n*k`, or `n*n*d` for `gram_matrix`) clears the
    /// measured crossover. Pure and side-effect-free — see
    /// [`would_use_gpu`] for the hardware-independent unit-tested version
    /// of this same rule.
    pub fn should_use_gpu(&self, work: usize) -> bool {
        would_use_gpu(self.context.is_some(), self.crossover, work)
    }
}

/// The dispatch rule itself, factored out as a free function over plain
/// `bool`/`usize` so it's unit-testable with synthetic values — no real
/// `qu_gpu::GpuContext` (and therefore no real GPU hardware) required to
/// confirm "above crossover with a GPU available routes to GPU, below it
/// or with none available never does."
pub fn would_use_gpu(gpu_available: bool, crossover: usize, work: usize) -> bool {
    gpu_available && work >= crossover
}

/// The process-global accessor — the ONLY entry point every real call site
/// in `lib.rs` should use. `OnceLock::get_or_init` guarantees [`build_probe`]
/// runs at most once per process, however many `Interp`s (including many
/// short-lived `spawn(...)` worker ones) call this.
pub fn probe() -> &'static GpuProbeResult {
    GPU_PROBE.get_or_init(build_probe)
}

/// How many times the real probe body has executed so far — always `0` or
/// `1` in a correctly-behaving process; see this module's own
/// `probe_runs_at_most_once_per_process` test.
pub fn probe_call_count() -> usize {
    PROBE_RUNS.load(Ordering::SeqCst)
}

/// Call right after a real `GpuContext::matmul` dispatch succeeds, from
/// every call site that routes through [`GpuProbeResult::should_use_gpu`]
/// — a test-only observation hook (see [`dispatch_count`]), never read by
/// production dispatch logic. Thread-local (see [`LOCAL_DISPATCHES`]'s own
/// comment) — safe to call from production code paths run inside a
/// `spawn(...)` worker thread too, since it never contends with, or is
/// contended by, any other thread's count.
pub fn record_dispatch() {
    LOCAL_DISPATCHES.with(|c| c.set(c.get() + 1));
}

/// How many real GPU dispatches [`record_dispatch`] has observed so far
/// **on the calling thread** — read by tests as a before/after delta
/// around one operation on that same thread, never by production code.
pub fn dispatch_count() -> usize {
    LOCAL_DISPATCHES.with(|c| c.get())
}

fn build_probe() -> GpuProbeResult {
    PROBE_RUNS.fetch_add(1, Ordering::SeqCst);
    match qu_gpu::GpuContext::new_blocking() {
        Ok(ctx) => run_probe_with(Some(ctx)),
        Err(err) => GpuProbeResult {
            context: None,
            init_error: Some(err.to_string()),
            crossover: usize::MAX,
            samples: Vec::new(),
        },
    }
}

/// The actual benchmark, split out from [`build_probe`] so it's testable
/// without real hardware: passing `None` exercises exactly the "no GPU /
/// failed probe" fallback path deterministically, no absent-hardware test
/// environment required. Passing `Some(ctx)` runs the real race.
pub(crate) fn run_probe_with(ctx: Option<qu_gpu::GpuContext>) -> GpuProbeResult {
    let Some(ctx) = ctx else {
        return GpuProbeResult { context: None, init_error: None, crossover: usize::MAX, samples: Vec::new() };
    };

    let mut samples = Vec::with_capacity(PROBE_SIZES.len());
    for &n in &PROBE_SIZES {
        let a = bench_matrix(n, n, 0.0);
        let b = bench_matrix(n, n, 100.0);
        let work = n.saturating_mul(n).saturating_mul(n);

        let t0 = Instant::now();
        let cpu_ok = a.matmul(&b).is_ok();
        let cpu_s = t0.elapsed().as_secs_f64();
        if !cpu_ok {
            continue;
        }

        let a_f32 = to_row_major_f32(&a);
        let b_f32 = to_row_major_f32(&b);
        let t1 = Instant::now();
        let gpu_ok = ctx.matmul(&a_f32, n, n, &b_f32, n).is_ok();
        let gpu_s = t1.elapsed().as_secs_f64();
        if !gpu_ok {
            continue;
        }

        samples.push((work, cpu_s, gpu_s));
    }

    let crossover = samples
        .iter()
        .find(|&&(_, cpu_s, gpu_s)| gpu_s < cpu_s)
        .map(|&(work, _, _)| work.max(MIN_SANE_CROSSOVER))
        .unwrap_or(usize::MAX);

    GpuProbeResult { context: Some(Arc::new(ctx)), init_error: None, crossover, samples }
}

/// A deterministic (seeded, no RNG dependency) `n x n` matrix for
/// benchmarking — same sine-based fill `lib.rs`'s own
/// `tensor_matmul_gpu_forward_matches_cpu_forward_and_gradients` test uses,
/// picked purely so CPU and GPU race on realistic non-degenerate data (not
/// all-zeros, which some GEMM fast paths special-case) rather than for any
/// statistical property.
fn bench_matrix(rows: usize, cols: usize, seed: f64) -> Matrix {
    let data: Vec<f64> = (0..rows * cols)
        .map(|i| ((i as f64) * 0.618_034 + seed).sin() * 0.5)
        .collect();
    Matrix::from_col_major(rows, cols, data)
}

fn to_row_major_f32(mat: &Matrix) -> Vec<f32> {
    let (r, c) = mat.shape();
    (0..r)
        .flat_map(|row| (0..c).map(move |col| (row, col)))
        .map(|(row, col)| mat.get(row, col).unwrap_or(0.0) as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn would_use_gpu_routes_above_and_below_crossover_correctly() {
        assert!(!would_use_gpu(true, 1000, 999), "just below crossover must stay on CPU");
        assert!(would_use_gpu(true, 1000, 1000), "exactly at crossover must route to GPU");
        assert!(would_use_gpu(true, 1000, 1_000_000), "well above crossover must route to GPU");
        assert!(!would_use_gpu(false, 1000, 1_000_000), "no GPU available must never route to GPU regardless of size");
    }

    #[test]
    fn should_use_gpu_matches_the_free_function_rule() {
        let unavailable = GpuProbeResult { context: None, init_error: None, crossover: 1000, samples: Vec::new() };
        assert!(!unavailable.should_use_gpu(usize::MAX), "no context means never use GPU no matter the size");
    }

    #[test]
    fn run_probe_with_none_context_falls_back_cleanly() {
        // Exercises the "GPU init failed / not available" path
        // deterministically, without needing an environment that actually
        // lacks a GPU — this is exactly what `build_probe` produces when
        // `qu_gpu::GpuContext::new_blocking()` returns `Err`.
        let result = run_probe_with(None);
        assert!(result.context.is_none());
        assert_eq!(result.crossover, usize::MAX);
        assert!(result.samples.is_empty());
        assert!(!result.should_use_gpu(usize::MAX), "a failed probe must never route to GPU, even for huge work");
    }

    #[test]
    fn probe_runs_at_most_once_per_process() {
        // `GPU_PROBE` is a single process-wide `OnceLock` shared by every
        // test in this binary, so `probe_call_count()` may already be 1
        // by the time this test runs (if another test called `probe()`
        // first) — the invariant under test is "never re-runs," not
        // "starts at zero," so this only checks the count is stable
        // across two calls, regardless of what it started at.
        let _ = probe(); // ensure it has run at least once
        let after_first = probe_call_count();
        assert!(after_first >= 1);
        let _ = probe();
        let after_second = probe_call_count();
        assert_eq!(after_first, after_second, "probe() must not re-run its expensive body on a second call");
    }

    #[test]
    fn dispatch_counter_only_moves_when_recorded() {
        let before = dispatch_count();
        // no dispatch recorded here
        assert_eq!(dispatch_count(), before);
        record_dispatch();
        assert_eq!(dispatch_count(), before + 1);
    }
}
