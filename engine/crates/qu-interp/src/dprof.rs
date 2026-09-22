//! Dispatch-phase profiler (measurement scaffolding for the
//! `claude/dispatch-cost-breakdown` lane). Not a product feature.
//!
//! WHY THIS EXISTS. board2's "interpreter dispatch is 48% of Qu's wall
//! time on `element_wise_ops`" is a RESIDUAL: qu.exe's chain time minus
//! `qu-core/examples/ew_profile.rs`'s kernel-only chain time. A residual
//! names no phase, so it cannot tell you whether an inline cache would
//! remove any of it. This module attributes that residual to phases the
//! interpreter actually has, measured from inside it.
//!
//! EXCLUSIVE TIME, NOT INCLUSIVE. Probed regions nest (`Expr::Binary`
//! inside `Expr::Binary`, `map1` inside `dispatch_builtin`). Each region
//! pushes a child-time accumulator; on exit it charges its own bucket
//! only `elapsed - children` and hands `elapsed` up to its parent. So
//! every bucket is disjoint and the buckets simply ADD. Whatever the
//! probes do not cover shows up as `UNATTRIBUTED` rather than being
//! quietly folded into a neighbour.
//!
//! LAP SCOPING. `tic()` resets the counters and `toc()` closes a lap; the
//! snapshot kept is the one belonging to the FASTEST lap so far. That
//! matches `bench_fair.qu`'s own best-of-N rule, which exists because
//! this machine is shared: a mean would blend a lap that lost the CPU to
//! another session's build into one that did not.
//!
//! COST WHEN OFF. `QU_DPROF` unset => `enabled()` is a cached bool and
//! `region()` returns `None` without reading the clock. Probes sit at
//! per-OPERATION seams (a handful per statement), never per element.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::Instant;

/// Bucket indices. Keep in sync with `PHASES`.
pub mod p {
    /// `eval_call`/`call_named` minus every probed child: name resolution,
    /// `resolve_method`, `reject_unknown_kwargs`, the `call_named` match.
    pub const CALL_OTHER: usize = 0;
    /// Evaluating and collecting the argument list (`eval_call_args`).
    pub const CALL_ARGS: usize = 1;
    /// `call_builtin` before `dispatch_builtin`: `open_module_name`,
    /// `warn_deprecated`, kwarg-key clone, ArgFrame/StyleFrame push.
    pub const CALL_PROLOGUE: usize = 2;
    /// `dispatch_builtin`'s own body (the ~700-arm `match f`) minus the
    /// numeric kernel it ends up calling.
    pub const CALL_DISPATCH: usize = 3;
    /// `call_builtin` after `dispatch_builtin`: unread-kwarg and
    /// extra-positional rejection, colour-error drain.
    pub const CALL_EPILOGUE: usize = 4;
    /// `Matrix::map` — the real elementwise kernel for `sin`/`cos`/`abs`.
    pub const MAP1_KERNEL: usize = 5;
    /// `eval`'s `Expr::Binary` arm minus every probed child.
    pub const BIN_OTHER: usize = 6;
    /// Evaluating the two operands, minus nested probed regions — i.e.
    /// variable lookup and `Value` clone for a bare name.
    pub const BIN_OPERANDS: usize = 7;
    /// `binop`'s type cascade (Unit/CMat/complex/matrix tests) minus
    /// `matrix_ew`.
    pub const BIN_BINOP: usize = 8;
    /// `matrix_ew` minus its four probed children.
    pub const EW_OTHER: usize = 9;
    /// `orient_vec_for_broadcast(lhs)` — `Value::into_matrix`, which
    /// DEEP-COPIES when the `Arc` is still held by a live variable.
    pub const EW_ORIENT_L: usize = 10;
    /// Same for the right operand.
    pub const EW_ORIENT_R: usize = 11;
    /// `Matrix::broadcast` — the real elementwise kernel for `.*`/`+`.
    pub const EW_KERNEL: usize = 12;
    /// `mat_value` (re-wrap the result in an `Arc`).
    pub const EW_WRAP: usize = 13;
    /// `var_set` at the end of an assignment — where the PREVIOUS binding
    /// is dropped (a full-size free for a big matrix).
    pub const ASSIGN_STORE: usize = 14;
}

pub const N: usize = 15;

pub const PHASES: [&str; N] = [
    "eval_call/call_named  (self)",
    "  argument eval+marshal",
    "  call_builtin prologue",
    "  dispatch_builtin match (self)",
    "  call_builtin epilogue (kwargs)",
    "  map1 kernel  Matrix::map",
    "Expr::Binary          (self)",
    "  operand eval (var lookup)",
    "  binop type cascade    (self)",
    "  matrix_ew             (self)",
    "    orient lhs  into_matrix",
    "    orient rhs  into_matrix",
    "    kernel  Matrix::broadcast",
    "    mat_value wrap",
    "assign var_set (drops old)",
];

#[derive(Clone, Copy)]
pub struct Snap {
    pub ns: [u64; N],
    pub cnt: [u64; N],
    pub wall: f64,
}

impl Snap {
    const fn zero() -> Self {
        Snap { ns: [0; N], cnt: [0; N], wall: 0.0 }
    }
}

struct State {
    cur: Snap,
    best: Option<Snap>,
    laps: usize,
    /// One entry per open region: nanoseconds already charged to probed
    /// children of that region.
    stack: Vec<u64>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State {
        cur: Snap::zero(),
        best: None,
        laps: 0,
        stack: Vec::new(),
    });
}

static ON: OnceLock<bool> = OnceLock::new();

#[inline]
pub fn enabled() -> bool {
    *ON.get_or_init(|| std::env::var_os("QU_DPROF").is_some())
}

/// An open probed region. Charges its bucket on drop, so an early `?`
/// return cannot desynchronise the stack.
pub struct Region {
    idx: usize,
    t: Instant,
}

impl Drop for Region {
    fn drop(&mut self) {
        let total = self.t.elapsed().as_nanos() as u64;
        let idx = self.idx;
        ST.with(|s| {
            let mut s = s.borrow_mut();
            let child = s.stack.pop().unwrap_or(0);
            if let Some(parent) = s.stack.last_mut() {
                *parent += total;
            }
            s.cur.ns[idx] += total.saturating_sub(child);
            s.cur.cnt[idx] += 1;
        });
    }
}

#[inline]
pub fn region(idx: usize) -> Option<Region> {
    if !enabled() {
        return None;
    }
    ST.with(|s| s.borrow_mut().stack.push(0));
    let t = Instant::now();
    // Inside the clock deliberately: this is the "can this probe detect a
    // change at all?" lever, and it has to land in the bucket it names.
    inject(idx);
    Some(Region { idx, t })
}

/// Deliberate cost injection, used ONLY to prove a probe can see a change
/// (`QU_DPROF_INJECT=<bucket>:<microseconds>`). A probe that cannot be
/// made to move is not a probe, so this is part of the instrument, not a
/// debugging leftover.
pub fn inject(idx: usize) {
    if !enabled() {
        return;
    }
    static SPEC: OnceLock<Option<(usize, u64)>> = OnceLock::new();
    let spec = SPEC.get_or_init(|| {
        let raw = std::env::var("QU_DPROF_INJECT").ok()?;
        let (a, b) = raw.split_once(':')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    });
    if let Some((want, us)) = *spec {
        if want == idx {
            let until = Instant::now() + std::time::Duration::from_micros(us);
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        }
    }
}

/// `tic()` — begin a fresh lap.
pub fn lap_start() {
    if !enabled() {
        return;
    }
    // The counters are zeroed, but the region STACK is deliberately left
    // alone: `tic()` is itself reached through `eval_call`/
    // `dispatch_builtin`, so regions are open right now and clearing the
    // stack would leave their `Drop`s popping from nothing and charging
    // their parents garbage. The only cost of leaving it is that tic/toc's
    // own microseconds land in the lap they open, which is what they are.
    ST.with(|s| s.borrow_mut().cur = Snap::zero());
}

/// `toc()` — close the lap; keep it if it is the fastest seen.
pub fn lap_end(wall: f64) {
    if !enabled() {
        return;
    }
    ST.with(|s| {
        let mut s = s.borrow_mut();
        s.laps += 1;
        s.cur.wall = wall;
        let keep = s.best.map_or(true, |b| wall < b.wall);
        if keep {
            s.best = Some(s.cur);
        }
    });
}

pub fn report() -> String {
    if !enabled() {
        return String::new();
    }
    let (best, laps) = ST.with(|s| {
        let s = s.borrow();
        (s.best, s.laps)
    });
    let Some(b) = best else {
        return "[dprof] no tic()/toc() lap recorded -- nothing to report\n".to_string();
    };
    let wall = (b.wall * 1e9).max(1.0);
    let mut out = String::new();
    out.push_str(&format!(
        "\n[dprof] fastest of {laps} lap(s); wall = {:.5} s   (all buckets are EXCLUSIVE and add up)\n",
        b.wall
    ));
    out.push_str(&format!("{:<34}{:>11}{:>8}{:>9}\n", "phase (exclusive)", "ms", "calls", "% wall"));
    let mut sum = 0u64;
    for i in 0..N {
        sum += b.ns[i];
        out.push_str(&format!(
            "{:<34}{:>11.3}{:>8}{:>8.1}%\n",
            PHASES[i],
            b.ns[i] as f64 / 1e6,
            b.cnt[i],
            100.0 * b.ns[i] as f64 / wall
        ));
    }
    out.push_str(&format!(
        "{:<34}{:>11.3}{:>8}{:>8.1}%\n",
        "UNATTRIBUTED (alloc/drop/other)",
        (b.wall * 1e9 - sum as f64) / 1e6,
        "",
        100.0 * (b.wall * 1e9 - sum as f64) / wall
    ));

    let kernel = b.ns[p::EW_KERNEL] + b.ns[p::MAP1_KERNEL];
    let marshal = b.ns[p::EW_ORIENT_L] + b.ns[p::EW_ORIENT_R];
    // Everything a perfect inline cache / call-site specialisation could
    // plausibly remove: it caches "which callee, which arity, which
    // validation" — not the numerics and not the data movement.
    let cacheable = b.ns[p::CALL_OTHER]
        + b.ns[p::CALL_PROLOGUE]
        + b.ns[p::CALL_DISPATCH]
        + b.ns[p::CALL_EPILOGUE]
        + b.ns[p::BIN_OTHER]
        + b.ns[p::BIN_BINOP];
    out.push_str("\nderived (as % of the fastest lap's wall):\n");
    for (label, v) in [
        ("REAL KERNEL  broadcast + map", kernel),
        ("ARG MARSHALLING  into_matrix copies", marshal),
        ("OPERAND/ARG EVAL  var lookup", b.ns[p::BIN_OPERANDS] + b.ns[p::CALL_ARGS]),
        ("CACHEABLE  name res + validation + type cascade", cacheable),
        ("RESULT WRAP + ASSIGN STORE", b.ns[p::EW_WRAP] + b.ns[p::ASSIGN_STORE] + b.ns[p::EW_OTHER]),
    ] {
        out.push_str(&format!(
            "  {:<48}{:>10.3} ms{:>8.1}%\n",
            label,
            v as f64 / 1e6,
            100.0 * v as f64 / wall
        ));
    }
    out
}
