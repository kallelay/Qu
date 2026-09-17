//! Job queue / worker pool (§47.3, 2026-08-26) — `queue()` + `.push` +
//! `pool ... end pool` / `pool(n)` + `run <queue> on <pool>`. Kept as its
//! own module (not inserted into `lib.rs`'s already-large builtin match)
//! per this pass's own stated goal of minimizing merge conflicts with
//! concurrent work on that file; `lib.rs` only gets a handful of small,
//! additive insertion points (the `Value::Queue`/`Value::Pool` variants, a
//! few exhaustive-match arms, `Stmt::Pool`/`Expr::Run` execution, and the
//! `"queue"`/`"push"`/`"pool"` builtin-call arms) that call straight into
//! this module.
//!
//! **Isolation model**: identical to `spawn`/`pmap`/`parallel for` (see
//! their own doc comments in `lib.rs`) — share-nothing, each job runs in a
//! freshly constructed `Interp` seeded with a SNAPSHOT of the parent's
//! `env`/`funcs`/`block_funcs` (never a live reference) and its own
//! freshly-drawn RNG seed (drawn from the parent's stream, in submission
//! order, before any thread starts — reproducible under `seed(n)`
//! regardless of scheduling).
//!
//! **Concurrency primitive — a deliberate, reasoned deviation worth
//! flagging.** This feature was scoped assuming `spawn`/`Worker`/
//! `parallel for` were backed by raw `std::thread::spawn`, one OS thread
//! per task. By the time this module was actually written, they had
//! already migrated (2026-08-24, see `WorkerHandle`'s doc comment in
//! `lib.rs`) onto `rayon::spawn`/`rayon`'s global pool instead, precisely
//! to stop a script that mixes `spawn`/`pmap` with heavy matrix math from
//! oversubscribing the machine with two independent pools competing for
//! cores. Reusing raw `std::thread` here would reintroduce exactly that
//! problem for any script mixing `run ... on pool(n)` with `pmap`/matrix
//! math — so this module dispatches each pool's own jobs through a
//! scoped, EXACTLY-`cpu`-sized `rayon::ThreadPool` (`ThreadPoolBuilder::
//! num_threads(cpu).build()` + `.install(...)`) instead of either raw
//! `std::thread` or the shared global `rayon` pool: same `rayon` primitive
//! already established elsewhere in this codebase, just scoped to the
//! caller-chosen worker count `cpu = N` actually requires (the shared
//! global pool has no per-call way to express "exactly N workers, no
//! more"). Results are collected/returned in submission order, never
//! completion order.
//!
//! **Phase A2 (multi-pool load-aware auto-routing), same date.** Phase A
//! above gave every pool its own scoped thread pool built FRESH, per
//! `run` call — fine when exactly one pool is ever in play, but useless
//! for "route each job to whichever of SEVERAL pools is least busy right
//! now": that needs occupancy that outlives a single `run` call and is
//! visible to every call that references the same pool concurrently. So
//! `Value::Pool` now wraps a live `PoolHandle` (a persistent, built-ONCE
//! `rayon::ThreadPool` plus an atomic "currently active" counter) instead
//! of a bare `PoolSpec` clone — see `PoolHandle`'s own doc comment for the
//! full design, and `Interp::pool_registry`/`eval_run` (`lib.rs`) for how
//! a job actually gets routed to one.

use crate::{e, EvalError, Interp, MethodEntry, Rng, Value, R};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

/// One deferred job pushed via `q.push(fnName, arg1, arg2, ..., on="any")`:
/// the name of an already-defined user function, its (already-evaluated,
/// at push time) arguments, and an optional resource tag — called once
/// `run` drains the queue. String-named, not a real closure: Qu's `->`
/// token is lexed (`qu-lexer`'s `OPERATORS` list) but never consumed
/// anywhere in `qu-syntax` or `qu-interp` (checked directly, not assumed)
/// — there is no evaluable anonymous-function/closure value in Qu today,
/// so this follows the exact same convention `spawn("fnName", args...)`
/// and `pmap(xs, "fnName")` already use, rather than the spec's own
/// `() -> process(chunk_a)` closure-literal spelling.
#[derive(Clone, Debug)]
pub struct Job {
    pub fn_name: String,
    pub args: Vec<Value>,
    /// `"cpu"`, `"gpu"`, or `"any"` (default) — which resource kind this
    /// job may run on (§47.3 Phase A2, 2026-08-26). Only consulted by
    /// AUTO-routed `run` (`run jobs on any`, see `PoolHandle::
    /// matches_resource`/`run_jobs_auto`); a `run jobs on myPool` that
    /// names one specific pool ignores it entirely — the caller already
    /// said exactly where the job runs, tag or not. `"cpu"` and `"any"`
    /// behave identically today (only cpu-resource pools can be
    /// constructed at all, see `PoolSpec::build`); `"gpu"` never matches
    /// any pool that can exist yet, which is the point — it's meant to
    /// error clearly, not silently fall back to cpu.
    pub on: String,
}

/// A validated worker-pool CONFIGURATION — either from a named `pool
/// <name> with cpu = N ... end pool` block, or the anonymous `pool(n)`
/// shorthand (§47.3). Validation happens once, at construction
/// (`build`/`build_anonymous`), never re-checked at `run` time. Immutable
/// data only — the live, shared, mutable-occupancy wrapper is
/// `PoolHandle` below; every `Value::Pool` in the interpreter is a
/// `PoolHandle`, never a bare `PoolSpec`.
#[derive(Clone, Debug)]
pub struct PoolSpec {
    pub cpu: usize,
    /// Always 0 — `build`/`build_anonymous` reject any nonzero `gpu`
    /// before a `PoolSpec` can exist at all (see `build`'s doc comment).
    /// Kept as a field (not just discarded) so a future `pool.status`-
    /// style introspection builtin can show it without re-plumbing.
    pub gpu: usize,
    /// `"host:port"` addresses of other Qu processes running
    /// `listen_pool(port, allow=(...))` (§ distributed job dispatch,
    /// 2026-08-31) — additional workers `run <queue> on <thisPool>` may
    /// dispatch jobs to, alongside (or instead of) this pool's own local
    /// `cpu` workers. Empty (the default) means purely local behavior,
    /// unchanged from before this feature existed. Never deduplicated or
    /// otherwise validated beyond "non-empty string" at construction time
    /// — an unreachable or malformed address is a per-JOB failure surfaced
    /// the moment `run` actually tries to use it (`remote_pool::
    /// send_job_remote`), not a pool-creation-time error, since a remote
    /// worker can legitimately come and go after the pool is built.
    pub remote: Vec<String>,
    pub policy: String,
    pub on_full: String,
}

/// Absurd-guard for `cpu = N` (this feature's own "capped sanely against
/// `available_parallelism()`, with a clear error or clamp if N is absurd"
/// scope note): 0 workers can never make progress, and >10,000 is never a
/// real hardware request — almost certainly a typo or a unit mistake.
/// Anything in between is accepted AS THE CALLER WROTE IT, even past the
/// machine's own `available_parallelism()` — deliberately not silently
/// clamped down to the hardware thread count, since oversubscribing on
/// purpose (testing pool mechanics, or mostly-I/O-bound jobs) is a
/// legitimate, explicit request; only the two clearly-nonsensical ends
/// are rejected.
const MAX_SANE_CPU: usize = 10_000;

impl PoolSpec {
    /// Validates a named `pool <name> with cpu = <cpu>[, gpu = <gpu>]
    /// ... end pool` block's already-evaluated resource/config values.
    /// `policy` must be `"round_robin"` (the only value v1 actually
    /// implements — `priority`/`fair`/`affinity` parse fine but hit this
    /// error) and `gpu` must be exactly 0 (parses fine, errors here) —
    /// both are clear runtime errors at pool-CREATION time, not silently
    /// downgraded to round_robin / silently ignored, per this feature's
    /// scope decision (`BACKLOG.md`'s "Sixth proposal" section,
    /// "Scoped, 2026-08-26").
    pub fn build(cpu: f64, gpu: Option<f64>, policy: &str, on_full: &str, remote: Vec<String>) -> R<PoolSpec> {
        // `cpu = 0` is only legal when at least one `remote` worker fills
        // in for local capacity (§ distributed job dispatch, 2026-08-31,
        // "a pool with ONLY remote workers" — verification point (a));
        // `validate_cpu` still rejects a bare `cpu = 0` with no remote
        // workers at all, exactly as before this feature existed.
        let cpu = validate_cpu(cpu, !remote.is_empty())?;
        for addr in &remote {
            if addr.trim().is_empty() {
                return e("pool: remote addresses must be non-empty \"host:port\" strings");
            }
        }
        let gpu = gpu.unwrap_or(0.0);
        if gpu < 0.0 || gpu.fract() != 0.0 {
            return e("pool: gpu must be a non-negative integer");
        }
        let gpu = gpu as usize;
        if gpu != 0 {
            return e(
                "pool: gpu resource slots are not implemented yet (gpu_matmul is a single \
                 feature-gated builtin, not a general device target) — use cpu only",
            );
        }
        if policy != "round_robin" {
            return e(format!(
                "pool: policy '{policy}' is not implemented yet, only round_robin — see BACKLOG.md"
            ));
        }
        // `on full = reject` / `spill_to(...)` parse (§47.3 grammar) but
        // have nothing implemented behind them — `run` only ever behaves
        // as `on full = queue` (unbounded, every job eventually runs)
        // today, so silently accepting another value would silently
        // misbehave rather than error. Same "parse it, error clearly if
        // unsupported" treatment as `policy` above.
        if on_full != "queue" {
            return e(format!(
                "pool: on full = {on_full} is not implemented yet, only `on full = queue` — see BACKLOG.md"
            ));
        }
        Ok(PoolSpec {
            cpu,
            gpu,
            remote,
            policy: policy.to_string(),
            on_full: on_full.to_string(),
        })
    }

    /// The anonymous `pool(n)` shorthand (§47.3): `n` cpu workers, fixed
    /// `round_robin` policy, `on full = queue`, no remote workers (the
    /// anonymous shorthand has no syntax to name any) — no resource/policy
    /// options to parse, so there's nothing to validate beyond `cpu`
    /// itself.
    pub fn build_anonymous(n: f64) -> R<PoolSpec> {
        let cpu = validate_cpu(n, false)?;
        Ok(PoolSpec {
            cpu,
            gpu: 0,
            remote: Vec::new(),
            policy: "round_robin".to_string(),
            on_full: "queue".to_string(),
        })
    }
}

/// `has_remote_fallback` relaxes the "cpu must be at least 1" floor to
/// "cpu must be at least 0" — a pool that names at least one `remote`
/// worker doesn't need any local capacity at all (verification point (a):
/// a pool with ONLY remote workers). Every other caller (the anonymous
/// `pool(n)` shorthand, which has no `remote` syntax) still passes `false`
/// and keeps the original "0 workers can never make progress" error.
fn validate_cpu(n: f64, has_remote_fallback: bool) -> R<usize> {
    if n.is_nan() || n < 0.0 || n.fract() != 0.0 {
        return e("pool: cpu must be a non-negative integer");
    }
    let n = n as usize;
    if n == 0 && !has_remote_fallback {
        return e("pool: cpu must be at least 1 (0 workers can never make progress) — or add at least one `remote` worker");
    }
    if n > MAX_SANE_CPU {
        return e(format!(
            "pool: cpu = {n} is unreasonably large (max {MAX_SANE_CPU}) — likely a mistake"
        ));
    }
    Ok(n)
}

/// A LIVE, shared worker-pool handle (§47.3 Phase A2, 2026-08-26) — every
/// `Value::Pool` is `Arc<PoolHandle>`, the same "opaque, cheaply-cloned,
/// shared mutable state behind an `Arc`" shape `Value::Worker`/
/// `Value::File`/`Value::Mutex` already use (see their own doc comments).
/// Two things Phase A's plain `PoolSpec` couldn't offer, both required for
/// least-busy auto-routing to mean anything real:
///
/// 1. **A persistent thread pool**, built ONCE here (`from_spec`), not
///    freshly per `run` call. Occupancy has to persist between `run`
///    calls (a pool sits idle between two separate `run`s, then gets used
///    again) and be visible DURING a call too (two concurrent `run`s that
///    both reference this same pool must see each other's load) — neither
///    is possible if the underlying `rayon::ThreadPool` gets torn down and
///    rebuilt every time.
/// 2. **A live occupancy counter** (`active`, `AtomicUsize`) — `cpu -
///    active.load()` is a pool's free capacity RIGHT NOW, read by
///    `run_jobs_auto`'s least-busy selection. Incremented the moment a
///    job is dispatched onto this pool (by whichever path — explicit
///    `run jobs on myPool` or auto-routed `run jobs on any`), decremented
///    the moment it finishes; never held across the job's own execution
///    in a way that could deadlock (see `run_jobs_auto`'s doc comment).
/// Liveness of ONE remote worker address, scoped to one `run`.
///
/// A remote lane holds exactly one job at a time -- `send_job_remote`
/// opens one connection per job, and a `listen_pool` server serializes
/// concurrent connects through its own single-threaded accept loop
/// anyway -- so "busy" is a flag, not a count.
///
/// `unusable` is the anti-hang half. It is set ONLY when a CONNECT to the
/// worker fails, never when a job that reached the worker returns an
/// error: a script that throws on a healthy machine must not evict that
/// machine. It is reset at the start of every `run` (see
/// `reset_lane_liveness`), because a worker that was down five minutes
/// ago should be re-probed, not written off for the life of the process.
#[derive(Debug, Default)]
struct RemoteLane {
    busy: std::sync::atomic::AtomicBool,
    unusable: std::sync::atomic::AtomicBool,
}

pub struct PoolHandle {
    pub spec: PoolSpec,
    active: AtomicUsize,
    thread_pool: rayon::ThreadPool,
    /// One entry per `spec.remote` address, in the same order.
    remote_lanes: Vec<RemoteLane>,
    /// Assigned when a NAMED pool is inserted into `Interp::pool_registry`
    /// (`lib.rs`'s `Stmt::Pool` execution), via a monotonic
    /// `Interp::next_pool_id` counter — breaks ties deterministically when
    /// two registered pools have equal free capacity (earliest-registered
    /// wins, matching this feature's own "ties broken by registration
    /// order for determinism" scope note). `usize::MAX` for a pool that
    /// was never registered (the anonymous `pool(n)` shorthand) — never
    /// compared against a real registry entry, since only registered
    /// pools are ever candidates for auto-routing in the first place.
    pub registration_order: usize,
}

impl std::fmt::Debug for PoolHandle {
    // Hand-written rather than derived: printing `active`'s CURRENT count
    // (not just the field's existence) is more useful for debugging than
    // rayon's own `ThreadPool` Debug output, which this skips entirely.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolHandle")
            .field("spec", &self.spec)
            .field("active", &self.active.load(Ordering::SeqCst))
            .field("registration_order", &self.registration_order)
            .finish()
    }
}

impl PoolHandle {
    /// Named `pool <name> with cpu = N ... end pool`. Validates via
    /// `PoolSpec::build` (unchanged from Phase A), then eagerly builds
    /// this pool's own persistent `cpu`-sized `rayon::ThreadPool` right
    /// away — occupancy tracking needs a real, running pool to exist from
    /// the moment the `pool ... end pool` block finishes, since another
    /// concurrent `run` could reference it (by name, or via auto-routing
    /// once registered) immediately after.
    pub fn build(cpu: f64, gpu: Option<f64>, policy: &str, on_full: &str, remote: Vec<String>) -> R<PoolHandle> {
        Self::from_spec(PoolSpec::build(cpu, gpu, policy, on_full, remote)?)
    }

    /// The anonymous `pool(n)` shorthand — same eager construction, just
    /// never registered (so `registration_order` stays `usize::MAX` and
    /// it's never a candidate for auto-routing; it only ever runs the one
    /// `run jobs on pool(n)` call that built it).
    pub fn build_anonymous(n: f64) -> R<PoolHandle> {
        Self::from_spec(PoolSpec::build_anonymous(n)?)
    }

    fn from_spec(spec: PoolSpec) -> R<PoolHandle> {
        // `rayon::ThreadPoolBuilder::num_threads(0)` does NOT mean "zero
        // threads" — rayon's own documented behavior is "use the default
        // number of threads as if you hadn't called this at all," which
        // would silently hand a `cpu = 0, remote = (...)` pool (a real,
        // explicitly-supported shape — verification point (a): remote-only
        // workers) a large, unwanted local thread pool. `.max(1)` sidesteps
        // that entirely by always building at least a 1-thread pool, but
        // `spec.cpu` itself (0 in this case) is what every capacity check
        // (`free`, `has_local` in `run_jobs_on_pool`) actually reads — so
        // this harmless idle thread is simply never dispatched to.
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(spec.cpu.max(1))
            .build()
            .map_err(|err| EvalError {
                msg: format!(
                    "pool: failed to create a {}-worker thread pool: {err}",
                    spec.cpu
                ),
            })?;
        let remote_lanes = (0..spec.remote.len()).map(|_| RemoteLane::default()).collect();
        Ok(PoolHandle {
            spec,
            active: AtomicUsize::new(0),
            thread_pool,
            remote_lanes,
            registration_order: usize::MAX,
        })
    }

    /// How many jobs are dispatched on this pool RIGHT NOW — read-only
    /// introspection (used by `display_value`'s `pool(...)` formatting);
    /// never used for selection itself (`claim_least_busy` reads the raw
    /// atomic directly, right before trying to claim a slot, to avoid a
    /// stale-then-claim race).
    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Free capacity right now: `cpu - active`. A plain (non-atomic-CAS)
    /// read — callers that need to ACT on this value atomically (i.e.
    /// `claim_least_busy`) re-check via `compare_exchange` before trusting
    /// it, since it can go stale the instant another thread claims or
    /// releases a slot.
    fn free(&self) -> usize {
        self.spec.cpu.saturating_sub(self.active_count())
    }

    /// Whether this pool can ever satisfy resource tag `on`. Only
    /// `cpu`-resource pools can be constructed at all today (`gpu` is
    /// hard-rejected at `PoolSpec::build` time, not just unimplemented) —
    /// so in practice this only ever returns `true` for `"cpu"`/`"any"`
    /// jobs; a `"gpu"`-tagged job matches NOTHING yet, which is exactly
    /// what lets `run_jobs_auto` raise a clear "no gpu pool registered"
    /// error instead of silently running it on a cpu pool.
    /// Whether ANY lane this pool actually has can serve resource tag
    /// `on`. Capacity-aware on purpose: a pool with no remote addresses
    /// cannot serve `on="remote"` however willing it is.
    fn matches_resource(&self, on: &str) -> bool {
        (self.spec.cpu > 0 && tag_admits_local_cpu(on))
            || (!self.spec.remote.is_empty() && tag_admits_remote(on))
    }

    /// Whether `run jobs on any` can actually DISPATCH to this pool for
    /// resource tag `on`. Deliberately stricter than `matches_resource`:
    /// the auto path runs every job through the chosen pool's own local
    /// `rayon` pool (`thread_pool.install`) and has no remote lane, so a
    /// pool whose only capacity is remote workers (`cpu = 0, remote =
    /// (...)`, which `PoolSpec::build` deliberately allows -- see
    /// `pool_spec_build_allows_cpu_zero_when_at_least_one_remote_worker_
    /// is_named`) can never run anything here, however long you wait.
    ///
    /// Keeping the two predicates apart is the whole point.
    /// `matches_resource` answers "could this pool EVER satisfy the tag",
    /// which is what the `on="gpu"` fast-fail needs. This answers "can
    /// THIS path put a job on it", which is what capacity selection
    /// needs. Conflating them is exactly what let a legal script hang:
    /// the tag check said yes, capacity was permanently zero, and
    /// `claim_least_busy` waited for a slot that could never appear.
    /// Kept as a distinct name from `matches_resource` even though the
    /// two now coincide, because they answer different questions and the
    /// day they diverge again is the day something hangs: this one is
    /// what a DISPATCH PATH asks before committing a job to a wait.
    /// `matches_resource` became capacity-aware when the taxonomy landed,
    /// which is what collapsed them; the guarantee against the original
    /// hang no longer rests on this predicate at all but on
    /// `try_claim_lane` returning `NoLane::Never` when nothing is
    /// claimable and nothing is running.
    fn auto_dispatchable(&self, on: &str) -> bool {
        self.matches_resource(on)
    }

    /// Clears per-run lane liveness. Called at the start of each `run` so
    /// a worker that was unreachable during an earlier run gets re-probed
    /// rather than being written off for the life of the interpreter.
    fn reset_lane_liveness(&self) {
        for lane in &self.remote_lanes {
            lane.unusable.store(false, Ordering::SeqCst);
        }
    }
}

/// Builds one fresh, isolated `Interp` for a single job (the same
/// share-nothing snapshot scheme `spawn`/`pmap`/`parallel for` already
/// use — see this module's own top doc comment), runs it, and converts a
/// worker-thread panic into a normal `Err` instead of poisoning anything
/// (there's no `Mutex` here to poison, but a bare panic inside a `rayon`
/// job would otherwise abort the whole process). Shared by both dispatch
/// paths below (`run_jobs_on_pool` and `run_jobs_auto`) so the actual
/// job-execution semantics can't drift between them.
fn run_one_job(
    job: &Job,
    env: &HashMap<String, Value>,
    methods: &HashMap<String, Vec<MethodEntry>>,
    seed: u64,
) -> R<Value> {
    let mut worker = Interp::new();
    worker.env = env.clone();
    worker.methods = methods.clone();
    worker.rng = Rng::new(seed);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker.apply(&job.fn_name, job.args.clone(), Vec::new())
    }));
    outcome.unwrap_or_else(|_| e("run: a pool worker thread panicked"))
}

/// Runs every job in `jobs` (in submission order) on ONE specific,
/// already-chosen pool (`run jobs on myPool` / `run jobs on pool(n)` —
/// unchanged Phase A entry point, still exactly as fast/simple as before:
/// no least-busy selection to do when the caller already named the pool)
/// and returns their results, ALSO in submission order (§47.3's own
/// reproducibility requirement — matches `parallel for`'s existing
/// order-stability discipline, not a new one; the fastest job can be
/// submitted last and its result still lands at the END of the returned
/// list, not the front). `env`/`methods` are the calling `Interp`'s
/// already-taken snapshot (`Interp::visible_env_snapshot`/
/// `self.methods.clone()` — built by the caller); `seeds` is one fresh
/// RNG seed per job, drawn by the caller
/// from ITS OWN stream up front, in submission order (so the whole run
/// stays reproducible under `seed(n)` regardless of how the pool happens
/// to schedule jobs across threads) — same discipline
/// `exec_parallel_for`/`pmap` already use.
///
/// Dispatches onto `pool`'s own PERSISTENT thread pool (Phase A2 — was a
/// freshly-built-then-discarded one per call in Phase A) and bumps its
/// `active` counter around each job, so a concurrent `run jobs on any`
/// elsewhere that also considers this same named pool sees accurate
/// occupancy — real shared state, not per-call bookkeeping that only this
/// call can see.
///
/// A job's error is NEVER silently swallowed: the first one, in
/// SUBMISSION order (not whichever happens to finish first), propagates
/// out of `run` — mirroring `join(w)`'s own "re-raise the real error"
/// behavior for a single worker. Jobs after the first error still run to
/// completion (nothing cancels an in-flight `rayon` task once started),
/// but their results are discarded once the first error is found.
/// Since a `PoolSpec` guarantees `cpu > 0 || !remote.is_empty()` (see
/// `PoolSpec::build`), a pool that reaches here always has at least one
/// place to put a job.
pub(crate) fn run_jobs_on_pool(
    jobs: Vec<Job>,
    pool: &PoolHandle,
    env: HashMap<String, Value>,
    methods: HashMap<String, Vec<MethodEntry>>,
    seeds: Vec<u64>,
) -> R<Vec<Value>> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    debug_assert_eq!(jobs.len(), seeds.len());

    if pool.spec.remote.is_empty() {
        // Unchanged local-only path (pre-2026-08-31): every job onto this
        // pool's own persistent rayon thread pool, `active` bumped around
        // each job for `run jobs on any`'s cross-pool occupancy tracking.
        let results: Vec<R<Value>> = pool.thread_pool.install(|| {
            jobs.par_iter()
                .zip(seeds.par_iter())
                .map(|(job, &seed)| -> R<Value> {
                    pool.active.fetch_add(1, Ordering::SeqCst);
                    let out = run_one_job(job, &env, &methods, seed);
                    pool.active.fetch_sub(1, Ordering::SeqCst);
                    out
                })
                .collect()
        });
        let mut out = Vec::with_capacity(results.len());
        for r in results {
            out.push(r?);
        }
        return Ok(out);
    }

    // §  distributed job dispatch, 2026-08-31: `pool` has `remote`
    // addresses configured (alongside, or — verification point (a) —
    // instead of, local `cpu` workers). Each job's INDEX is round-robined
    // across "lanes": one lane for the whole local cpu pool (only if
    // `cpu > 0`; `rayon` balances the N cpu workers internally, so the
    // local pool is one lane, not `cpu` separate lanes) plus one lane per
    // configured remote address — deterministic by submission order, the
    // same `round_robin` policy this pool already validated at creation
    // (`PoolSpec::build` rejects every other `policy` value, so this is
    // the only one that can reach here). A remote lane's jobs are sent
    // SEQUENTIALLY to that one address, one TCP connection per job
    // (`remote_pool::send_job_remote`) — a `listen_pool` server naturally
    // serializes concurrent connection attempts via its own single-
    // threaded accept loop, so nothing more elaborate is needed to keep
    // one remote worker from being asked to run two jobs at once. Every
    // lane (the local pool's `install` call, and each remote address's
    // sequential loop) runs concurrently with every other lane via
    // `std::thread::scope`.
    //
    // Failure handling (decided here, matching the LOCAL pool's own
    // per-job failure semantics above exactly, not inventing new ones): an
    // unreachable/timed-out/rejecting remote worker is just THIS JOB's
    // `Err` — never a silent fallback to local execution. The first error
    // in SUBMISSION order (not completion order, not which lane it came
    // from) propagates out of `run`; every other job (local or remote)
    // still runs to completion first (nothing here cancels an in-flight
    // job once dispatched), its result simply discarded once the first
    // error is found — identical to the local-only path above.
    let has_local = pool.spec.cpu > 0;
    let remote = &pool.spec.remote;
    let num_lanes = (has_local as usize) + remote.len();
    debug_assert!(
        num_lanes > 0,
        "PoolSpec::build guarantees cpu>0 or a non-empty remote list"
    );

    let mut local_indices: Vec<usize> = Vec::new();
    let mut remote_indices: Vec<Vec<usize>> = vec![Vec::new(); remote.len()];
    for i in 0..jobs.len() {
        let lane = i % num_lanes;
        if has_local && lane == 0 {
            local_indices.push(i);
        } else {
            let r = if has_local { lane - 1 } else { lane };
            remote_indices[r].push(i);
        }
    }

    let slots: Vec<StdMutex<Option<R<Value>>>> = (0..jobs.len()).map(|_| StdMutex::new(None)).collect();
    std::thread::scope(|scope| {
        if has_local && !local_indices.is_empty() {
            let jobs = &jobs;
            let seeds = &seeds;
            let env = &env;
            let methods = &methods;
            let slots = &slots;
            let local_indices = &local_indices;
            scope.spawn(move || {
                // `active` counts OCCUPIED SLOTS, so it is bumped around
                // each job individually -- exactly as the local-only path
                // and `run_jobs_auto` do it. This lane used to add
                // `local_indices.len()` once, up front, which made the
                // same counter mean "jobs QUEUED to this lane" here and
                // "jobs running"/"slots held" everywhere else. Since
                // `free()` is `cpu - active` and `run_jobs_auto` reads it
                // to place work, a cpu=1 pool handed 10 local jobs
                // reported active=10 and looked permanently full to a
                // concurrent `on any` for as long as it took to drain --
                // one number with three meanings, read across paths that
                // each assumed their own.
                let results: Vec<(usize, R<Value>)> = pool.thread_pool.install(|| {
                    local_indices
                        .par_iter()
                        .map(|&i| {
                            pool.active.fetch_add(1, Ordering::SeqCst);
                            let out = run_one_job(&jobs[i], env, methods, seeds[i]);
                            pool.active.fetch_sub(1, Ordering::SeqCst);
                            (i, out)
                        })
                        .collect()
                });
                for (i, r) in results {
                    *slots[i].lock().unwrap() = Some(r);
                }
            });
        }
        for (r_idx, idxs) in remote_indices.into_iter().enumerate() {
            if idxs.is_empty() {
                continue;
            }
            let addr = remote[r_idx].clone();
            let jobs = &jobs;
            let slots = &slots;
            scope.spawn(move || {
                for i in idxs {
                    let outcome = crate::remote_pool::send_job_remote(&addr, &jobs[i]);
                    *slots[i].lock().unwrap() = Some(outcome);
                }
            });
        }
    });

    let mut out = Vec::with_capacity(slots.len());
    for slot in slots {
        out.push(
            slot.into_inner()
                .unwrap()
                .expect("every slot is filled by its own scoped thread before the scope exits"),
        );
    }
    let mut final_out = Vec::with_capacity(out.len());
    for r in out {
        final_out.push(r?);
    }
    Ok(final_out)
}

/// Atomically claims one worker slot on whichever pool in `pools` (a) can
/// satisfy resource tag `on` and (b) currently has the most free capacity
/// — ties broken by earliest `registration_order`, per this feature's own
/// determinism requirement — blocking (via a short sleep, not a busy-spin)
/// until a slot is actually available if every matching pool is full
/// right now. Never holds any lock while waiting: this only ever touches
/// plain atomics (`AtomicUsize::load`/`compare_exchange`), so a thread
/// parked here can't block anyone else's progress, and a slot always
/// eventually frees (every dispatched job runs to completion and
/// decrements `active` — see `run_jobs_auto`).
///
/// That last part only holds while at least one candidate pool has
/// non-zero LOCAL capacity, which is why candidates are filtered by
/// `auto_dispatchable` rather than `matches_resource`, and why
/// `run_jobs_auto` refuses the whole batch up front when nothing is
/// dispatchable. Before that guard existed this comment was simply
/// wrong: a legal `cpu = 0, remote = (...)` pool made this loop wait
/// forever for capacity that could never appear -- no output, no error,
/// no timeout (verified 2026-09-09: `timeout 25` returned 124 on such a
/// script, while the same script with `run jobs on p` errored cleanly).
///
/// Selection re-reads free capacity from scratch on every loop iteration
/// (never trusts a snapshot across the `compare_exchange` below): if the
/// chosen pool's slot gets claimed by a different thread in the gap
/// between "read free capacity" and "try to claim," the `compare_exchange`
/// simply fails and this loops around to re-select rather than assuming
/// success.
/// One place a job can actually run: a pool's local worker set, or one
/// of its remote worker addresses. This is the unit `run jobs on any`
/// schedules over now -- it used to schedule over POOLS, which is why it
/// could never reach a remote worker however many were configured.
/// Ahmed's resource taxonomy, in one place (2026-09-09). Two axes were
/// tangled before this: WHAT device (cpu | gpu) and WHERE it runs (host |
/// remote). The tags name points in that space:
///
///   cpu     local CPU only
///   gpu     local GPU only
///   host    local, either device -- opts INTO the GPU trade
///   remote  other machines only
///   any     DEFAULT -- "anywhere my numbers stay the same" = cpu + remote
///   auto    "anywhere at all" = any + gpu, an explicit opt-in
///
/// The line between `any` and `auto` is the only one that matters, and it
/// is about NUMBERS, not speed. `any` may move work anywhere numerically
/// equivalent -- another core, another machine, same answer. `auto` may
/// also use a device whose floating point differs, and whose admission
/// depends on the job's data size, so the same script can take a
/// different path at a different input size. That is a trade a researcher
/// must opt into by name, which is why it is not the default; and it is
/// what lets "use every resource automatically" be both true and honest,
/// since `any` already spans every core and every remote machine.
fn tag_admits_local_cpu(on: &str) -> bool {
    matches!(on, "cpu" | "host" | "any" | "auto")
}

fn tag_admits_remote(on: &str) -> bool {
    matches!(on, "remote" | "any" | "auto")
}

/// No pool can own a GPU lane yet, so this admits nothing in practice --
/// it exists so the taxonomy is complete in one place rather than half
/// here and half in a future commit, and so `on="gpu"` keeps failing with
/// "no gpu pool registered" instead of silently matching a CPU lane.
#[allow(dead_code)]
fn tag_admits_gpu(on: &str) -> bool {
    matches!(on, "gpu" | "host" | "auto")
}

enum Lane<'a> {
    Local(&'a Arc<PoolHandle>),
    Remote(&'a Arc<PoolHandle>, usize),
}

/// Why a claim attempt came back empty-handed -- the distinction that
/// keeps `claim_lane` from reintroducing the hang fixed in 1bbcf362.
enum NoLane {
    /// Nothing free right now, but at least one lane is BUSY, so capacity
    /// is genuinely coming. Waiting is legitimate.
    Retry,
    /// Nothing free and nothing running: no capacity will ever appear.
    /// Waiting here would be the old infinite sleep.
    Never,
}

/// Runs one job on an already-claimed lane. The single place a job meets
/// a worker, whichever path selected the lane -- the same discipline
/// `run_one_job` applies one level down, applied here where lane
/// selection had been allowed to drift between the two callers.
fn run_on_lane(
    lane: &Lane<'_>,
    job: &Job,
    env: &HashMap<String, Value>,
    methods: &HashMap<String, Vec<MethodEntry>>,
    seed: u64,
) -> R<Value> {
    match lane {
        Lane::Local(pool) => pool.thread_pool.install(|| run_one_job(job, env, methods, seed)),
        Lane::Remote(pool, idx) => {
            let outcome = crate::remote_pool::send_job_remote(&pool.spec.remote[*idx], job);
            // Only a CONNECT failure retires the lane. A job that reached
            // the worker and came back with an error -- a throw, a
            // rejected function name -- says nothing about the machine's
            // health, and evicting it for that would take a working
            // worker out of the pool because someone's script was wrong.
            if let Err(err) = &outcome {
                if err.msg.contains("is unreachable") {
                    pool.remote_lanes[*idx].unusable.store(true, Ordering::SeqCst);
                }
            }
            outcome
        }
    }
}

/// Releases a claimed lane. Mirrors the claim in `claim_lane` exactly;
/// every claim has exactly one release.
fn release_lane(lane: &Lane<'_>) {
    match lane {
        Lane::Local(pool) => {
            pool.active.fetch_sub(1, Ordering::SeqCst);
        }
        Lane::Remote(pool, idx) => pool.remote_lanes[*idx].busy.store(false, Ordering::SeqCst),
    }
}

/// One attempt to claim the best available lane across `pools`.
///
/// Scoring keeps the old preference visible: a local lane scores the
/// pool's free capacity, a remote lane scores 1, so big local pools are
/// used before remote workers and remote capacity is picked up only once
/// local is saturated. Ties break by `registration_order` then lane
/// index, preserving the determinism note on the original selector.
fn try_claim_lane<'a>(pools: &'a [Arc<PoolHandle>], on: &str) -> Result<Lane<'a>, NoLane> {
    let mut best: Option<(usize, usize, usize, Lane<'a>)> = None; // score, order, idx, lane
    let mut any_busy = false;
    for pool in pools.iter().filter(|p| p.matches_resource(on)) {
        let free = pool.free();
        if pool.active_count() > 0 {
            any_busy = true;
        }
        // Lane kind is checked per LANE, not per pool. A pool can serve
        // the tag through one kind of lane and not the other: an
        // `on="remote"` job on a cpu+remote pool must take the network,
        // not the local workers sitting right there.
        if free > 0 && tag_admits_local_cpu(on) {
            let cand = (free, pool.registration_order, 0usize, Lane::Local(pool));
            if best.as_ref().map_or(true, |b| (cand.0, usize::MAX - cand.1) > (b.0, usize::MAX - b.1)) {
                best = Some(cand);
            }
        }
        for (i, lane) in pool.remote_lanes.iter().enumerate() {
            if !tag_admits_remote(on) {
                break;
            }
            if lane.unusable.load(Ordering::SeqCst) {
                continue;
            }
            if lane.busy.load(Ordering::SeqCst) {
                any_busy = true;
                continue;
            }
            let cand = (1usize, pool.registration_order, i, Lane::Remote(pool, i));
            if best.as_ref().map_or(true, |b| (cand.0, usize::MAX - cand.1) > (b.0, usize::MAX - b.1)) {
                best = Some(cand);
            }
        }
    }
    let Some((_, _, _, lane)) = best else {
        return Err(if any_busy { NoLane::Retry } else { NoLane::Never });
    };
    // Commit atomically; losing the race just means re-selecting.
    match &lane {
        Lane::Local(pool) => {
            let cur = pool.active.load(Ordering::SeqCst);
            if cur < pool.spec.cpu
                && pool
                    .active
                    .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                return Ok(lane);
            }
        }
        Lane::Remote(pool, idx) => {
            if pool.remote_lanes[*idx]
                .busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(lane);
            }
        }
    }
    Err(NoLane::Retry)
}

/// Blocks until a lane is claimable, or returns `None` the moment it can
/// prove none ever will be. That proof is the whole point: a wait is only
/// legitimate while some lane is BUSY, because a busy lane frees. All
/// lanes unusable and none busy is the condition that used to sleep
/// forever.
fn claim_lane<'a>(pools: &'a [Arc<PoolHandle>], on: &str) -> Option<Lane<'a>> {
    loop {
        match try_claim_lane(pools, on) {
            Ok(lane) => return Some(lane),
            Err(NoLane::Never) => return None,
            Err(NoLane::Retry) => std::thread::sleep(std::time::Duration::from_micros(200)),
        }
    }
}

#[allow(dead_code)]
fn claim_least_busy<'a>(pools: &'a [Arc<PoolHandle>], on: &str) -> &'a Arc<PoolHandle> {
    loop {
        let mut best: Option<&Arc<PoolHandle>> = None;
        let mut best_free = 0usize;
        for p in pools.iter().filter(|p| p.auto_dispatchable(on)) {
            let free = p.free();
            if free == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some(b) => {
                    free > best_free
                        || (free == best_free && p.registration_order < b.registration_order)
                }
            };
            if better {
                best = Some(p);
                best_free = free;
            }
        }
        if let Some(chosen) = best {
            let cur = chosen.active.load(Ordering::SeqCst);
            if cur < chosen.spec.cpu
                && chosen
                    .active
                    .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                return chosen;
            }
            // Lost the race for that slot (or it was already gone by the
            // time we tried) — re-select from scratch immediately, no
            // sleep needed since capacity clearly still exists somewhere.
            continue;
        }
        // Every matching pool is fully occupied right now. Not a busy
        // spin: a short sleep between checks, since a job finishing (and
        // freeing a slot) takes far longer than this interval in any
        // realistic script.
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
}

/// `run <queue> on any` (§47.3 Phase A2, 2026-08-26): auto-routes each job
/// (in submission order) onto whichever REGISTERED pool matching its
/// resource tag has the most free capacity **at the moment that specific
/// job is ready to dispatch** — not once for the whole batch. `pools` is
/// a snapshot of `Interp::pool_registry`'s values taken when `run` was
/// evaluated (a `Vec` of cloned `Arc`s, not a live borrow of the
/// registry): a pool registered by a DIFFERENT statement while this call
/// is still draining simply isn't a candidate for it — "every pool that
/// exists right now," not "every pool that will ever exist."
///
/// Dispatch shape: one plain OS thread per job (`std::thread::scope`),
/// each of which blocks on `claim_least_busy` until it owns a slot on
/// some matching pool, then runs the job INSIDE that pool's own
/// persistent `rayon::ThreadPool` (`.install(...)`, which blocks this
/// orchestrating thread until the job finishes — the real CPU work still
/// only ever runs on that pool's own `cpu`-sized worker set, never on the
/// orchestrating thread itself). This is deliberately simple over
/// "clever": the orchestrating threads spend almost all their time
/// blocked (either waiting for capacity or waiting on `install`), so one
/// OS thread per job costs little, and it sidesteps any question of
/// nesting one `rayon` pool's dispatch inside another's worker thread.
///
/// Same reproducibility discipline as `run_jobs_on_pool`: `seeds` drawn
/// by the caller up front in submission order; results collected into a
/// slot per job index and re-assembled in submission order at the end,
/// never completion order; the first error in submission order (not
/// completion order) propagates.
///
/// Fails FAST — before dispatching a single job — if any job's resource
/// tag can never be satisfied by any registered pool (e.g. `on="gpu"`
/// with zero gpu pools registered, which is every script today): letting
/// some jobs run while one is silently guaranteed to hang forever
/// waiting for a slot that will never exist would be far worse than
/// refusing the whole batch up front.
pub(crate) fn run_jobs_auto(
    jobs: Vec<Job>,
    pools: Vec<Arc<PoolHandle>>,
    env: HashMap<String, Value>,
    methods: HashMap<String, Vec<MethodEntry>>,
    seeds: Vec<u64>,
) -> R<Vec<Value>> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    debug_assert_eq!(jobs.len(), seeds.len());
    for pool in &pools {
        pool.reset_lane_liveness();
    }
    for job in &jobs {
        if !pools.iter().any(|p| p.matches_resource(&job.on)) {
            return e(format!(
                "run: job requires on=\"{0}\" but no {0}-resource pool is registered \
                 ({0} pools aren't implemented yet, see BACKLOG.md)",
                job.on
            ));
        }
        // A pool can match the tag and still be impossible for THIS path
        // to use. `cpu = 0, remote = (...)` is a legal pool whose only
        // capacity is remote workers, and `on any` has no remote lane
        // yet, so there is no amount of waiting that makes it runnable.
        // Refuse up front -- the same discipline as the `on="gpu"` check
        // above, and for the same reason: a job that is guaranteed never
        // to start must not be dispatched into a wait.
        if !pools.iter().any(|p| p.auto_dispatchable(&job.on)) {
            return e(format!(
                "run: `on any` found no pool with local workers for on=\"{0}\" -- every \
                 matching pool has cpu = 0. `on any` dispatches to local workers only and \
                 cannot use remote workers yet: either give a pool some local `cpu`, or \
                 dispatch to it explicitly with `run <queue> on <pool>`, which does use \
                 remote workers",
                job.on
            ));
        }
    }
    let slots: Vec<StdMutex<Option<R<Value>>>> = (0..jobs.len()).map(|_| StdMutex::new(None)).collect();
    std::thread::scope(|scope| {
        for (i, job) in jobs.iter().enumerate() {
            let seed = seeds[i];
            let pools = &pools;
            let env = &env;
            let methods = &methods;
            let slot = &slots[i];
            scope.spawn(move || {
                let outcome = match claim_lane(pools, &job.on) {
                    Some(lane) => {
                        let out = run_on_lane(&lane, job, env, methods, seed);
                        release_lane(&lane);
                        out
                    }
                    // Provably no capacity left -- every remote worker
                    // this job could have used proved unreachable and no
                    // lane is still running. Erroring is the point:
                    // before the lane work this waited forever.
                    None => e(format!(
                        "run: no usable worker left for on=\"{0}\" -- every matching pool's \
                         remote workers proved unreachable and no local capacity remains",
                        job.on
                    )),
                };
                *slot.lock().unwrap() = Some(outcome);
            });
        }
    });
    let mut out = Vec::with_capacity(slots.len());
    for slot in slots {
        out.push(
            slot.into_inner()
                .unwrap()
                .expect("every slot is filled by its own scoped thread before the scope exits"),
        );
    }
    let mut final_out = Vec::with_capacity(out.len());
    for r in out {
        final_out.push(r?);
    }
    Ok(final_out)
}

#[cfg(test)]
mod tests {
    // ---- `PoolSpec::build` validation for `remote` (§ distributed job
    // dispatch, 2026-08-31) — end-to-end coverage (real TCP loopback, real
    // `pool ... remote=(...)` scripts) lives in `qu-interp/src/lib.rs`'s
    // own test module, right alongside the rest of the `run jobs on pool`
    // tests; these are just the pure validation-logic unit tests that
    // belong next to `PoolSpec::build` itself.

    #[test]
    fn pool_spec_build_rejects_cpu_zero_without_any_remote_worker() {
        let err = PoolSpec::build(0.0, None, "round_robin", "queue", Vec::new()).unwrap_err();
        assert!(err.msg.contains("cpu"), "got: {}", err.msg);
    }

    #[test]
    fn pool_spec_build_allows_cpu_zero_when_at_least_one_remote_worker_is_named() {
        // Verification point (a): a pool with ONLY remote workers, no
        // local cpu at all, must be a valid, buildable configuration.
        let spec = PoolSpec::build(0.0, None, "round_robin", "queue", vec!["127.0.0.1:9".to_string()])
            .expect("cpu=0 with a remote worker should be accepted");
        assert_eq!(spec.cpu, 0);
        assert_eq!(spec.remote, vec!["127.0.0.1:9".to_string()]);
    }

    #[test]
    fn pool_spec_build_rejects_a_blank_remote_address() {
        let err = PoolSpec::build(1.0, None, "round_robin", "queue", vec!["   ".to_string()]).unwrap_err();
        assert!(err.msg.contains("remote"), "got: {}", err.msg);
    }

    #[test]
    fn pool_handle_from_spec_never_asks_rayon_for_a_zero_thread_pool() {
        // Regression guard: `rayon::ThreadPoolBuilder::num_threads(0)`
        // means "use the default thread count," NOT "zero threads" — a
        // naive `cpu=0, remote=(...)` pool would otherwise silently get a
        // large, unwanted local thread pool. `PoolHandle::build` must
        // still succeed (and `run_jobs_on_pool` must still never dispatch
        // to it — covered by the `lib.rs` end-to-end tests), not panic or
        // hang while constructing the (unused) underlying `rayon` pool.
        let handle = PoolHandle::build(0.0, None, "round_robin", "queue", vec!["127.0.0.1:9".to_string()])
            .expect("cpu=0 with a remote worker should build fine");
        assert_eq!(handle.spec.cpu, 0);
        assert_eq!(handle.free(), 0, "a cpu=0 pool must report zero LOCAL free capacity");
    }

    use super::*;

    /// Builds an anonymous (never-registered) `PoolHandle` with a
    /// specific `registration_order` set by hand — real registration
    /// normally happens in `Interp`'s `Stmt::Pool` execution, but these
    /// tests exercise `claim_least_busy` directly, without going through
    /// a whole `Interp`/script.
    fn handle(cpu: f64, registration_order: usize) -> Arc<PoolHandle> {
        let mut h = PoolHandle::build_anonymous(cpu).unwrap();
        h.registration_order = registration_order;
        Arc::new(h)
    }

    #[test]
    fn claim_least_busy_prefers_the_pool_with_more_free_capacity() {
        // `small` (cpu=1) registered FIRST, `big` (cpu=4) registered
        // SECOND — deliberately the opposite of registration order, so a
        // bug that just picks "the earliest-registered pool with any
        // room" (ignoring how much free capacity it actually has) would
        // wrongly favor `small` here instead.
        let small = handle(1.0, 0);
        let big = handle(4.0, 1);
        let pools = vec![small.clone(), big.clone()];

        // Rounds 1-3: `big`'s free capacity (4, then 3, then 2) always
        // strictly beats `small`'s constant free=1, so `big` is chosen
        // every time despite being registered SECOND — proves selection
        // is driven by actual free capacity, not registration order.
        for expected_big_active in 1..=3 {
            let chosen = claim_least_busy(&pools, "any");
            assert!(
                Arc::ptr_eq(chosen, &big),
                "expected the bigger, roomier pool to be chosen, got cpu={}",
                chosen.spec.cpu
            );
            assert_eq!(big.active_count(), expected_big_active);
            assert_eq!(small.active_count(), 0, "the smaller pool should stay untouched while `big` still has more room");
        }
        // Round 4: `big`'s free capacity has now dropped to 4-3=1, tying
        // `small`'s constant free=1 — the tie is broken by registration
        // order, and `small` was registered FIRST, so it wins this claim
        // (not `big` again, even though `big` is still the larger pool
        // overall).
        let chosen = claim_least_busy(&pools, "any");
        assert!(
            Arc::ptr_eq(chosen, &small),
            "expected the earlier-registered pool to win an exact free-capacity tie, got cpu={}",
            chosen.spec.cpu
        );
        assert_eq!(small.active_count(), 1);
        assert_eq!(big.active_count(), 3);
    }

    #[test]
    fn claim_least_busy_breaks_equal_capacity_ties_by_registration_order() {
        let first = handle(2.0, 0);
        let second = handle(2.0, 1);
        let pools = vec![first.clone(), second.clone()];

        // Free capacity is identical (2 vs 2) on the very first claim —
        // the earlier-registered pool (`first`) must win the tie.
        let chosen = claim_least_busy(&pools, "any");
        assert!(Arc::ptr_eq(chosen, &first));

        // Now `first` has free=1, `second` still has free=2 — `second`
        // is genuinely less busy, so it must win outright (not a tie).
        let chosen = claim_least_busy(&pools, "any");
        assert!(Arc::ptr_eq(chosen, &second));

        // Both pools are now at free=1 each — a tie again, `first` wins
        // again by registration order.
        let chosen = claim_least_busy(&pools, "any");
        assert!(Arc::ptr_eq(chosen, &first));
    }

    #[test]
    fn auto_dispatchable_accepts_a_remote_only_pool_but_never_the_wrong_tag() {
        // This test asserted the OPPOSITE one commit ago, and the change
        // is deliberate rather than convenient: `on any` reaching remote
        // workers was the chartered work, so a cpu=0 + remote pool being
        // undispatchable was the defect being removed, not a contract to
        // preserve. Renamed because a test called `..._rejects_...` that
        // asserts acceptance misleads the next reader with authority.
        let remote_only = Arc::new(
            PoolHandle::build(0.0, None, "round_robin", "queue", vec!["127.0.0.1:9".to_string()])
                .expect("cpu=0 with a remote worker is a legal pool"),
        );
        assert!(
            remote_only.auto_dispatchable("any"),
            "a remote-only pool IS dispatchable now -- its remote address is a lane"
        );
        // What the predicate still exists to express: the resource tag.
        // Keeping it separate from `matches_resource` is the point --
        // collapsing "could this pool ever satisfy the tag" into "can
        // this path put a job on it" is what made a legal script hang.
        assert!(!remote_only.auto_dispatchable("gpu"), "still never gpu");
        let local = handle(2.0, 0);
        assert!(local.auto_dispatchable("any"));
        assert!(local.auto_dispatchable("cpu"));
        assert!(!local.auto_dispatchable("gpu"), "still never gpu");
    }

    #[test]
    fn the_tag_table_says_what_the_taxonomy_says() {
        // The whole taxonomy as a table, so a change to it has to change
        // this test on purpose rather than by accident. The only line
        // that carries real weight is the last pair: `any` excludes gpu
        // and `auto` includes it, which is the difference between "move
        // my work anywhere numerically equivalent" and "I accept a device
        // that may change my last decimal place".
        for (tag, local, remote, gpu) in [
            ("cpu", true, false, false),
            ("gpu", false, false, true),
            ("host", true, false, true),
            ("remote", false, true, false),
            ("any", true, true, false),
            ("auto", true, true, true),
        ] {
            assert_eq!(tag_admits_local_cpu(tag), local, "local cpu for on={tag}");
            assert_eq!(tag_admits_remote(tag), remote, "remote for on={tag}");
            assert_eq!(tag_admits_gpu(tag), gpu, "gpu for on={tag}");
        }
    }

    #[test]
    fn a_pool_cannot_serve_a_tag_it_has_no_lane_for() {
        // Capacity-aware, not just tag-aware: willingness is not capacity.
        let local_only = handle(2.0, 0);
        assert!(local_only.matches_resource("cpu"));
        assert!(local_only.matches_resource("any"));
        assert!(
            !local_only.matches_resource("remote"),
            "a pool with no remote addresses cannot serve on=remote"
        );
        let remote_only = Arc::new(
            PoolHandle::build(0.0, None, "round_robin", "queue", vec!["127.0.0.1:9".to_string()])
                .expect("cpu=0 with a remote worker is a legal pool"),
        );
        assert!(remote_only.matches_resource("remote"));
        assert!(remote_only.matches_resource("any"));
        assert!(
            !remote_only.matches_resource("cpu"),
            "on=cpu means LOCAL cpu -- a remote-only pool must not serve it"
        );
    }

    #[test]
    fn matches_resource_only_matches_cpu_and_any_never_gpu() {
        let h = handle(1.0, 0);
        assert!(h.matches_resource("cpu"));
        assert!(h.matches_resource("any"));
        assert!(!h.matches_resource("gpu"));
    }
}
