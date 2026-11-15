//! Real-time pacing for `run_for(duration, pace=)`, v0.4.14.
//!
//! `run_for` advances a *virtual* timeline. With `pace=` each timer firing
//! is additionally held back until its wall-clock deadline
//! `start + virtual_time / pace`, and how late each one actually started is
//! recorded. Deadlines are anchored to the start of the run, so lateness does
//! not accumulate into drift: a late step is followed by a shorter wait.
//! Nothing is skipped to catch up; a run that cannot keep up simply reports
//! every step as late.
//!
//! This is soft real time on a desktop OS. The wait sleeps until about a
//! millisecond before the deadline and spins for the rest, which costs one
//! busy core for that last millisecond but is not at the mercy of the
//! scheduler's wake-up granularity.

use std::time::{Duration, Instant};

/// What `run_for_stats()` reports about the most recent `run_for`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunStats {
    pub paced: bool,
    /// The real-time factor asked for; 0 for an unpaced (virtual) run.
    pub pace: f64,
    pub steps: usize,
    /// Steps that started later than the tolerance after their deadline.
    pub overruns: usize,
    pub max_lateness_ms: f64,
    pub mean_lateness_ms: f64,
    /// Wall-clock length of the whole call.
    pub wall_ms: f64,
    /// Lateness of every step, in firing order (empty for an unpaced run).
    pub lateness_ms: Vec<f64>,
}

impl RunStats {
    pub(crate) fn unpaced(steps: usize, wall: Duration) -> RunStats {
        RunStats {
            paced: false,
            pace: 0.0,
            steps,
            overruns: 0,
            max_lateness_ms: 0.0,
            mean_lateness_ms: 0.0,
            wall_ms: wall.as_secs_f64() * 1e3,
            lateness_ms: Vec::new(),
        }
    }
}

/// How long before the deadline the sleep hands over to the spin.
const SPIN: Duration = Duration::from_micros(1000);

/// Blocks until `deadline`; returns immediately if it has passed.
fn wait_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let left = deadline - now;
        if left > SPIN + Duration::from_micros(500) {
            std::thread::sleep(left - SPIN);
        } else {
            std::hint::spin_loop();
        }
    }
}

pub(crate) struct Pacer {
    start: Instant,
    pace: f64,
    tolerance_s: f64,
    lateness_s: Vec<f64>,
}

impl Pacer {
    /// `pace`: virtual seconds per wall-clock second (1 = real time, 2 =
    /// twice as fast). `tolerance_s`: lateness above which a step counts as
    /// an overrun.
    pub(crate) fn new(pace: f64, tolerance_s: f64) -> Pacer {
        Pacer { start: Instant::now(), pace, tolerance_s, lateness_s: Vec::new() }
    }

    fn deadline(&self, virtual_t: f64) -> Instant {
        // A deadline past ~30 years is "never"; Duration cannot hold NaN/inf.
        self.start + Duration::from_secs_f64((virtual_t / self.pace).clamp(0.0, 1e9))
    }

    /// Holds a step scheduled at virtual time `virtual_t` back until it is
    /// due and records how late it starts. Returns that lateness in seconds.
    pub(crate) fn step(&mut self, virtual_t: f64) -> f64 {
        let deadline = self.deadline(virtual_t);
        wait_until(deadline);
        let late = Instant::now().saturating_duration_since(deadline).as_secs_f64();
        self.lateness_s.push(late);
        late
    }

    /// Waits out the rest of the run (so a paced `run_for(d)` occupies
    /// `d / pace` of wall clock even if its last timer fired earlier) and
    /// returns the statistics.
    pub(crate) fn finish(self, virtual_end: f64) -> RunStats {
        wait_until(self.deadline(virtual_end));
        summarize(&self.lateness_s, self.pace, self.tolerance_s, self.start.elapsed())
    }
}

/// The statistics of a paced run from its per-step lateness.
pub(crate) fn summarize(lateness_s: &[f64], pace: f64, tolerance_s: f64, wall: Duration) -> RunStats {
    let steps = lateness_s.len();
    let max = lateness_s.iter().cloned().fold(0.0f64, f64::max);
    let mean = if steps == 0 { 0.0 } else { lateness_s.iter().sum::<f64>() / steps as f64 };
    RunStats {
        paced: true,
        pace,
        steps,
        overruns: lateness_s.iter().filter(|&&l| l > tolerance_s).count(),
        max_lateness_ms: max * 1e3,
        mean_lateness_ms: mean * 1e3,
        wall_ms: wall.as_secs_f64() * 1e3,
        lateness_ms: lateness_s.iter().map(|l| l * 1e3).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_overruns_above_the_tolerance_only() {
        let s = summarize(&[0.0, 0.0005, 0.002, 0.010], 1.0, 0.001, Duration::from_millis(40));
        assert_eq!((s.steps, s.overruns), (4, 2));
        assert!((s.max_lateness_ms - 10.0).abs() < 1e-9);
        assert!((s.mean_lateness_ms - 3.125).abs() < 1e-9);
        assert!((s.wall_ms - 40.0).abs() < 1e-9 && s.paced && s.pace == 1.0);
        assert_eq!(s.lateness_ms.len(), 4);
        // exactly at the tolerance is not an overrun
        assert_eq!(summarize(&[0.001], 1.0, 0.001, Duration::ZERO).overruns, 0);
        // no steps: all zero, nothing divided by zero
        let s = summarize(&[], 2.0, 0.001, Duration::ZERO);
        assert_eq!((s.steps, s.overruns, s.max_lateness_ms, s.mean_lateness_ms), (0, 0, 0.0, 0.0));
    }

    #[test]
    fn paced_steps_do_not_start_before_their_deadline() {
        // 5 steps of 20 ms at pace 1: step k may not start before 20 k ms
        let mut p = Pacer::new(1.0, 0.005);
        let t0 = Instant::now();
        for k in 1..=5 {
            let late = p.step(0.020 * k as f64);
            assert!(late >= 0.0);
            assert!(t0.elapsed() >= Duration::from_micros(20_000 * k - 300), "step {k} started early: {:?}", t0.elapsed());
        }
        let s = p.finish(0.100);
        assert_eq!(s.steps, 5);
        assert!(s.wall_ms >= 99.0, "{}", s.wall_ms);
    }

    #[test]
    fn pace_scales_the_wall_clock() {
        // 100 ms of virtual time at pace 4 is 25 ms of wall clock
        let mut p = Pacer::new(4.0, 0.005);
        p.step(0.050);
        let s = p.finish(0.100);
        assert!(s.wall_ms >= 24.5 && s.wall_ms < 100.0, "{}", s.wall_ms);
    }

    #[test]
    fn a_step_whose_deadline_has_passed_is_late_and_does_not_wait() {
        let mut p = Pacer::new(1.0, 0.001);
        std::thread::sleep(Duration::from_millis(30));
        let before = Instant::now();
        let late = p.step(0.010); // was due 20 ms ago
        assert!(before.elapsed() < Duration::from_millis(25), "it waited: {:?}", before.elapsed());
        assert!(late >= 0.019, "{late}");
        let s = p.finish(0.010);
        assert_eq!(s.overruns, 1);
        assert!(s.max_lateness_ms >= 19.0);
    }

    #[test]
    fn lateness_does_not_accumulate_into_drift() {
        // One slow step (150 ms of work in a 100 ms period) makes the next
        // step 50 ms late. The step after that is due at 400 ms from the
        // START: with drift it would start at 250 + 200 = 450 ms instead.
        // (The 40 ms between the two is this test's margin for a busy machine.)
        let mut p = Pacer::new(1.0, 0.002);
        let t0 = Instant::now();
        p.step(0.100);
        std::thread::sleep(Duration::from_millis(150));
        let late2 = p.step(0.200);
        assert!(late2 >= 0.049, "{late2}");
        p.step(0.400);
        let at3 = t0.elapsed();
        assert!(at3 >= Duration::from_micros(399_700), "started early: {at3:?}");
        assert!(at3 < Duration::from_millis(440), "deadlines drifted: step 3 started at {at3:?}");
        let s = p.finish(0.400);
        assert_eq!((s.steps, s.overruns >= 1), (3, true));
    }
}
