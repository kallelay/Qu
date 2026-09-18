//! `qu run --max-time`/`--max-memory`/`--profile`'s shared resource-
//! monitoring backstop.
//!
//! # Design: an external watchdog thread, not a cooperative in-interpreter
//! check
//!
//! `qu-interp`'s tree-walking evaluator runs every Qu-level `for`/`while`
//! loop and function call in-process on one thread — a cooperative
//! "check the clock every N interpreter steps" hook inside that loop would
//! catch a runaway *Qu* script, but it can't catch a single slow *native*
//! builtin call (a huge `gpu_matmul`, a pathological regex, a giant
//! `fft`, ...) that never returns to the interpreter's own dispatch loop
//! at all while it runs. The only mechanism that reliably bounds BOTH
//! cases is the one this module uses: a separate thread that polls
//! wall-clock time and process RSS on a fixed interval and, on a breach,
//! calls `std::process::exit` directly — genuine OS-level process
//! termination, which tears down every thread in the process (including a
//! hung native loop with zero cooperation from it) rather than trying to
//! signal/interrupt the stuck thread from outside (not reliably possible
//! for a plain OS thread in safe Rust). This was chosen over adding
//! `Instant::now()` checks inside `qu-interp`'s loop/statement dispatch
//! for two reasons: it actually covers the native-builtin case the
//! cooperative approach cannot, and the watchdog thread itself (this
//! module) stays entirely inside `qu-cli` — no changes needed to
//! `qu-interp/src/lib.rs`'s heavily-trafficked, actively-edited
//! loop/statement dispatch. (The plain RSS *reader* below is shared with
//! `qu-interp`'s `profile_start`/`profile_end` builtins — see its own doc
//! comment — but that's a leaf function with no interpreter-loop
//! involvement, not the watchdog mechanism this note is about.)
//!
//! # Honest limitations
//!
//! - **Time precision**: bounded by `POLL_INTERVAL` below (25ms) — a run
//!   can overshoot `--max-time` by up to one poll interval before the
//!   watchdog notices and kills it. The reported "ran X.Xs" is measured at
//!   the moment the watchdog thread observes the breach, not the exact
//!   instant it happened.
//! - **Memory precision**: same polling granularity applies to
//!   `--max-memory`, AND a single large allocation that jumps RSS from
//!   comfortably-under-limit to far-over-limit between two polls will be
//!   caught late, with the reported "used ~NMB" reflecting whatever RSS
//!   happened to be at the next poll (which can be considerably more than
//!   the limit if the allocation itself was huge). This is a coarse,
//!   sampling-based cap, not a hard allocator-level ceiling.
//! - **Output loss on kill**: `qu-interp::Interp` buffers all `print`/
//!   `disp`/... output in memory (`it.out`) and `qu-cli` only writes it to
//!   stdout after `it.run()` returns. A watchdog-triggered
//!   `std::process::exit` therefore discards whatever output the killed
//!   script had produced so far — expected behavior for a hard kill (the
//!   alternative, silently letting a runaway script keep going, is
//!   strictly worse), but worth stating plainly rather than leaving
//!   readers to assume partial output survives.
//! - **RSS availability**: `current_rss_bytes` (re-exported from
//!   `qu_interp::resource` — see its doc comment for the Windows/Linux
//!   FFI) returns `None` on any other target; `--max-memory` on such a
//!   target is silently unenforceable (never trips) and `--profile`'s
//!   peak-RSS line reports "unavailable" rather than a fabricated number.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Current process resident-set size, in bytes. The platform FFI used to
/// live here directly; it now lives in `qu_interp::resource` (shared with
/// the `profile_start`/`profile_end` builtins) and this is just a
/// re-export, so both call sites stay in sync automatically instead of
/// maintaining two copies of the same `psapi.dll`/`/proc/self/status`
/// code.
pub use qu_interp::resource::current_rss_bytes;

fn flush_stderr() {
    let _ = std::io::stderr().flush();
}

/// Renders a `--max-time` limit for the kill message: `30` -> `"30s"`,
/// `1.5` -> `"1.5s"` — whatever the user actually typed, not a re-rounded
/// value.
fn fmt_limit_secs(secs: f64) -> String {
    if secs.fract() == 0.0 {
        format!("{}s", secs as i64)
    } else {
        format!("{secs}s")
    }
}

/// Background time/memory watchdog for one `qu run` invocation — see this
/// module's doc comment for why this is an external polling thread rather
/// than a cooperative in-interpreter check. Constructed with `start`
/// before the script actually runs, and `stop_and_join`ed once it
/// finishes normally; a limit breach in between calls `std::process::exit`
/// directly from the background thread and never returns.
pub struct ResourceMonitor {
    inner: Option<MonitorInner>,
}

struct MonitorInner {
    stop: Arc<AtomicBool>,
    peak_rss: Arc<AtomicU64>,
    rss_seen: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

impl ResourceMonitor {
    /// `max_time_secs`/`max_memory_mb` are `--max-time`/`--max-memory`, if
    /// given. `track_peak` additionally samples peak RSS for `--profile`'s
    /// report even when neither limit is set. If all three are absent/off,
    /// no thread is spawned at all — plain `qu run` (the overwhelming
    /// majority of invocations) pays exactly zero cost for this feature.
    pub fn start(max_time_secs: Option<f64>, max_memory_mb: Option<u64>, track_peak: bool) -> Self {
        if max_time_secs.is_none() && max_memory_mb.is_none() && !track_peak {
            return ResourceMonitor { inner: None };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let peak_rss = Arc::new(AtomicU64::new(0));
        let rss_seen = Arc::new(AtomicBool::new(false));
        let (stop2, peak2, seen2) = (stop.clone(), peak_rss.clone(), rss_seen.clone());

        let handle = std::thread::spawn(move || {
            let start = Instant::now();
            loop {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
                if let Some(rss) = current_rss_bytes() {
                    seen2.store(true, Ordering::Relaxed);
                    peak2.fetch_max(rss, Ordering::Relaxed);
                    if let Some(max_mb) = max_memory_mb {
                        let used_mb = rss / (1024 * 1024);
                        if used_mb >= max_mb {
                            eprintln!(
                                "qu: killed: exceeded --max-memory {max_mb}MB (used ~{used_mb}MB)"
                            );
                            flush_stderr();
                            std::process::exit(137);
                        }
                    }
                }
                if let Some(max_secs) = max_time_secs {
                    let elapsed = start.elapsed().as_secs_f64();
                    if elapsed >= max_secs {
                        eprintln!(
                            "qu: killed: exceeded --max-time {} (ran {elapsed:.1}s)",
                            fmt_limit_secs(max_secs)
                        );
                        flush_stderr();
                        std::process::exit(124);
                    }
                }
            }
        });

        ResourceMonitor {
            inner: Some(MonitorInner { stop, peak_rss, rss_seen, handle }),
        }
    }

    /// Stops sampling and returns the peak RSS observed in bytes, or
    /// `None` if this monitor never sampled anything (either it was never
    /// started because no limit/profiling was requested, or
    /// `current_rss_bytes` is unsupported on this platform).
    pub fn stop_and_join(self) -> Option<u64> {
        let inner = self.inner?;
        inner.stop.store(true, Ordering::Relaxed);
        let _ = inner.handle.join();
        if inner.rss_seen.load(Ordering::Relaxed) {
            Some(inner.peak_rss.load(Ordering::Relaxed))
        } else {
            None
        }
    }
}
