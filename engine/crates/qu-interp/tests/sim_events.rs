//! Simulation-event and SIL building blocks through the language (v0.4.14):
//!
//! 1. ODE events at SciPy `solve_ivp` parity: a list of event functions,
//!    `direction`, `terminal`, per-event `t_events` / `y_events`.
//! 2. `trace_diff`, `trace_save`, `trace_load` for back-to-back comparison.
//! 3. `run_for(duration, pace=)` against the wall clock, `run_for_stats()`.
//!
//! Every expected number is analytic (or a wall-clock bound), and every test
//! reads the value back out of the interpreter, so a builtin that returns
//! nothing or the wrong shape fails here rather than passing quietly.

use qu_interp::{Interp, Value};
use std::time::Instant;

const PI: f64 = std::f64::consts::PI;

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

fn nums(it: &Interp, n: &str) -> Vec<f64> {
    match it.get(n) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        Some(Value::Num(x)) => vec![*x],
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    match it.get(n) {
        Some(Value::Num(x)) => *x,
        other => panic!("`{n}` is {other:?}, expected a number"),
    }
}

fn flag(it: &Interp, n: &str) -> bool {
    match it.get(n) {
        Some(Value::Bool(b)) => *b,
        other => panic!("`{n}` is {other:?}, expected true/false"),
    }
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.to_string(),
        other => panic!("`{n}` is {other:?}, expected text"),
    }
}

fn shape(it: &Interp, n: &str) -> (usize, usize) {
    match it.get(n) {
        Some(Value::Mat(m)) => (m.rows(), m.cols()),
        other => panic!("`{n}` is {other:?}, expected a matrix"),
    }
}

fn assert_close(got: &[f64], want: &[f64], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: got {got:?}, want {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < tol, "{what}: got {got:?}, want {want:?}");
    }
}

// ================================================================ 1. events

const MODELS: &str = "
function fall(t, y)
    return [y[1], -9.81]
end function
function ground(t, y)
    return y[0]
end function
function sho(t, y)
    return [y[1], -y[0]]
end function
function pos(t, y)
    return y[0]
end function
function vel(t, y)
    return y[1]
end function
function never(t, y)
    return y[0] + 5
end function
function cosine(t, y)
    return cos(t)
end function
function value(t, y)
    return y
end function
";

#[test]
fn a_dropped_object_hits_the_ground_at_sqrt_2h_over_g() {
    let it = run(&format!(
        "{MODELS}\nr = ode45(\"fall\", [0, 10], [20, 0], events=\"ground\", terminal=true, rtol=1e-9, atol=1e-12)\n\
         te = r.t_events\nye = r.y_events\nT = r.t\nY = r.y\ns = r.status"
    ));
    let want = (2.0f64 * 20.0 / 9.81).sqrt();
    let te = nums(&it, "te");
    assert_eq!(te.len(), 1);
    assert!((te[0] - want).abs() < 1e-8, "{} vs {want}", te[0]);
    assert_eq!(text(&it, "s"), "event");
    // the integration stopped AT the event: last time and last state are it
    let t = nums(&it, "T");
    assert_eq!(*t.last().unwrap(), te[0]);
    assert!(t.len() >= 2);
    let ye = nums(&it, "ye");
    assert_eq!(shape(&it, "ye"), (1, 2));
    assert!(ye[0].abs() < 1e-8, "height at impact {}", ye[0]);
    assert!((ye[1] + 9.81 * want).abs() < 1e-7, "speed at impact {}", ye[1]);
    let y = nums(&it, "Y");
    assert_eq!(y[t.len() - 1], ye[0]);
    assert_eq!(y[2 * t.len() - 1], ye[1]);
}

#[test]
fn the_same_fall_is_located_by_every_adaptive_solver() {
    let want = (2.0f64 * 20.0 / 9.81).sqrt();
    for solver in ["ode45", "ode23", "ode_stiff"] {
        let it = run(&format!(
            "{MODELS}\nr = {solver}(\"fall\", [0, 10], [20, 0], events=\"ground\", terminal=true, rtol=1e-8, atol=1e-11)\n\
             te = r.t_events\ns = r.status"
        ));
        assert_close(&nums(&it, "te"), &[want], 1e-8, solver);
        assert_eq!(text(&it, "s"), "event", "{solver}");
    }
}

#[test]
fn a_list_of_events_reports_times_and_states_per_event() {
    // y = cos t, y' = -sin t on [0, 7]: position zeros at pi/2, 3pi/2;
    // velocity zeros at pi, 2pi; `never` has none.
    let it = run(&format!(
        "{MODELS}\nr = ode45(\"sho\", [0, 7], [1, 0], events=(\"pos\", \"vel\", \"never\"), rtol=1e-10, atol=1e-13)\n\
         k = len(r.t_events)\ntp = r.t_events[0]\ntv = r.t_events[1]\ntn = r.t_events[2]\n\
         yp = r.y_events[0]\nyv = r.y_events[1]\nyn = r.y_events[2]\n\
         order = r.event_order\nterm = r.terminal_event\ns = r.status\nT = r.t"
    ));
    assert_eq!(num(&it, "k"), 3.0);
    assert_close(&nums(&it, "tp"), &[PI / 2.0, 1.5 * PI], 1e-8, "position zeros");
    assert_close(&nums(&it, "tv"), &[PI, 2.0 * PI], 1e-8, "velocity zeros");
    assert!(nums(&it, "tn").is_empty());
    // y_events: one row per occurrence, one column per state
    assert_eq!(shape(&it, "yp"), (2, 2));
    assert_eq!(shape(&it, "yv"), (2, 2));
    assert_eq!(shape(&it, "yn"), (0, 2));
    // column-major: [y(pi/2), y(3pi/2), v(pi/2), v(3pi/2)] = [0, 0, -1, 1]
    assert_close(&nums(&it, "yp"), &[0.0, 0.0, -1.0, 1.0], 1e-8, "state at position zeros");
    assert_close(&nums(&it, "yv"), &[-1.0, 1.0, 0.0, 0.0], 1e-8, "state at velocity zeros");
    assert_eq!(nums(&it, "order"), vec![0.0, 1.0, 0.0, 1.0]);
    assert_eq!(num(&it, "term"), -1.0);
    assert_eq!(text(&it, "s"), "ok");
    assert_eq!(*nums(&it, "T").last().unwrap(), 7.0);
}

#[test]
fn direction_filters_a_sine_to_its_upward_or_downward_crossings() {
    // y = sin t from t = 0.001: falls through 0 at pi and 3pi, rises at 2pi
    let go = |direction: &str| {
        let it = run(&format!(
            "{MODELS}\nr = ode45(\"cosine\", [0.001, 10], sin(0.001), events=\"value\"{direction}, rtol=1e-10, atol=1e-13)\nte = r.t_events"
        ));
        nums(&it, "te")
    };
    assert_close(&go(""), &[PI, 2.0 * PI, 3.0 * PI], 1e-8, "no direction");
    assert_close(&go(", direction=0"), &[PI, 2.0 * PI, 3.0 * PI], 1e-8, "direction=0");
    assert_close(&go(", direction=1"), &[2.0 * PI], 1e-8, "upward only");
    assert_close(&go(", direction=-1"), &[PI, 3.0 * PI], 1e-8, "downward only");
}

#[test]
fn direction_and_terminal_can_be_given_per_event() {
    // pos falling only, vel terminal: stops at the first velocity zero (pi),
    // having seen the falling position zero at pi/2
    let it = run(&format!(
        "{MODELS}\nr = ode45(\"sho\", [0, 50], [1, 0], events=(\"pos\", \"vel\"), direction=[-1, 0], terminal=[false, true], rtol=1e-10, atol=1e-13)\n\
         tp = r.t_events[0]\ntv = r.t_events[1]\nterm = r.terminal_event\ns = r.status\nT = r.t"
    ));
    assert_close(&nums(&it, "tp"), &[PI / 2.0], 1e-8, "pos");
    assert_close(&nums(&it, "tv"), &[PI], 1e-8, "vel");
    assert_eq!(num(&it, "term"), 1.0);
    assert_eq!(text(&it, "s"), "event");
    assert!((nums(&it, "T").last().unwrap() - PI).abs() < 1e-8);
}

#[test]
fn an_event_record_carries_its_own_direction_and_terminal_count() {
    // pos rising only (3pi/2, 7pi/2 ...); vel stops the run at its 2nd zero (2pi)
    let it = run(&format!(
        "{MODELS}\nr = ode45(\"sho\", [0, 50], [1, 0], events=({{f = \"pos\", direction = 1}}, {{f = \"vel\", terminal = 2}}), rtol=1e-10, atol=1e-13)\n\
         tp = r.t_events[0]\ntv = r.t_events[1]\nterm = r.terminal_event\nT = r.t"
    ));
    assert_close(&nums(&it, "tp"), &[1.5 * PI], 1e-8, "pos rising");
    assert_close(&nums(&it, "tv"), &[PI, 2.0 * PI], 1e-8, "vel");
    assert_eq!(num(&it, "term"), 1.0);
    assert!((nums(&it, "T").last().unwrap() - 2.0 * PI).abs() < 1e-8);
}

#[test]
fn terminal_n_stops_at_the_nth_occurrence_of_a_single_event() {
    let it = run(&format!(
        "{MODELS}\nr = ode45(\"sho\", [0, 50], [1, 0], events=\"pos\", terminal=3, rtol=1e-10, atol=1e-13)\nte = r.t_events\ns = r.status"
    ));
    assert_close(&nums(&it, "te"), &[0.5 * PI, 1.5 * PI, 2.5 * PI], 1e-8, "three zeros");
    assert_eq!(text(&it, "s"), "event");
}

#[test]
fn a_lambda_works_as_an_event_function_alone_and_in_a_list() {
    let it = run(&format!(
        "{MODELS}\ng = (t, y) := y[0]\n\
         a = ode45(\"fall\", [0, 10], [20, 0], events=g, terminal=true, rtol=1e-9, atol=1e-12)\nta = a.t_events\n\
         b = ode45(\"sho\", [0, 4], [1, 0], events=(g, (t, y) := y[1]), rtol=1e-10, atol=1e-13)\n\
         tb0 = b.t_events[0]\ntb1 = b.t_events[1]"
    ));
    assert_close(&nums(&it, "ta"), &[(2.0f64 * 20.0 / 9.81).sqrt()], 1e-8, "lambda");
    assert_close(&nums(&it, "tb0"), &[PI / 2.0], 1e-8, "lambda in a list");
    assert_close(&nums(&it, "tb1"), &[PI], 1e-8, "inline lambda in a list");
}

#[test]
fn a_bouncing_ball_bounces_at_the_analytic_times() {
    // Dropped from h = 5 with restitution e = 0.8. First impact at
    // t1 = sqrt(2h/g); flight k then lasts 2 e^k t1. Each bounce restarts the
    // solver from the event state; direction=-1 means leaving the ground is
    // not itself an event.
    let it = run(&format!(
        "{MODELS}\nt0 = 0\ny0 = [5, 0]\nbounces = zeros(6)\nspeeds = zeros(6)\n\
         for k = 0 to 5\n\
             r = ode45(\"fall\", [t0, 100], y0, events=\"ground\", direction=-1, terminal=true, rtol=1e-9, atol=1e-12)\n\
             t0 = r.t_events[0]\n\
             ye = r.y_events\n\
             bounces[k] = t0\n\
             speeds[k] = ye[0, 1]\n\
             y0 = [0, -0.8 * ye[0, 1]]\n\
         end for"
    ));
    let (g, e) = (9.81f64, 0.8f64);
    let t1 = (2.0 * 5.0 / g).sqrt();
    let mut want_t = Vec::new();
    let mut want_v = Vec::new();
    let mut t = t1;
    for k in 0..6 {
        want_t.push(t);
        want_v.push(-g * t1 * e.powi(k));
        t += 2.0 * e.powi(k + 1) * t1;
    }
    assert_close(&nums(&it, "bounces"), &want_t, 1e-8, "bounce times");
    assert_close(&nums(&it, "speeds"), &want_v, 1e-7, "impact speeds");
}

#[test]
fn event_misuse_is_reported_not_ignored() {
    let call = |tail: &str| err(&format!("{MODELS}\nr = ode45(\"sho\", [0, 5], [1, 0]{tail})"));
    assert!(call(", events=\"pos\", direction=2").contains("direction must be -1"));
    assert!(call(", direction=1").contains("without events="));
    assert!(call(", terminal=true").contains("without events="));
    let m = call(", events=(\"pos\", \"vel\"), direction=[1, 0, -1]");
    assert!(m.contains("3 value(s)") && m.contains("2 function(s)"), "{m}");
    assert!(call(", events=(\"pos\", \"nope\")").contains("`nope`"));
    assert!(call(", events=({f = \"pos\", dir = 1},)").contains("`dir`"));
    assert!(call(", events=\"pos\", terminal=1.5").contains("whole number"));
    assert!(call(", events=3").contains("must be a function"));
    // rk4 has no events at all
    let m = err(&format!("{MODELS}\nr = rk4(\"sho\", [0, 5], [1, 0], 0.1, events=\"pos\")"));
    assert!(m.contains("events"), "{m}");
}

// ================================================================ 2. traces

#[test]
fn trace_diff_of_matching_vectors_is_ok_and_reports_zero() {
    let it = run("a = [1, 2, 3, 4]\nd = trace_diff(a, a)\nok = d.ok\nma = d.max_abs\nmr = d.max_rel\nwi = d.worst_index\nfb = d.first_bad_index\nn = d.n\nnb = d.n_bad");
    assert!(flag(&it, "ok"));
    assert_eq!((num(&it, "ma"), num(&it, "mr")), (0.0, 0.0));
    assert_eq!((num(&it, "wi"), num(&it, "fb")), (-1.0, -1.0));
    assert_eq!((num(&it, "n"), num(&it, "nb")), (4.0, 0.0));
}

#[test]
fn trace_diff_finds_the_worst_and_the_first_bad_sample() {
    let it = run(
        "a = [1, 2, 3, 4, 1000]\nb = [1, 2.000001, 3, 4.001, 1000.0005]\nd = trace_diff(a, b)\n\
         ok = d.ok\nma = d.max_abs\nmr = d.max_rel\nwi = d.worst_index\nfb = d.first_bad_index\nn = d.n\nnb = d.n_bad\n\
         loose = trace_diff(a, b, rtol=1e-3).ok\npos = trace_diff(a, b, 1e-3, 0).ok\ntight = trace_diff(a, b, rtol=0, atol=1e-4).n_bad",
    );
    assert!(!flag(&it, "ok"));
    assert!((num(&it, "ma") - 1e-3).abs() < 1e-12);
    assert!((num(&it, "mr") - 1e-3 / 4.001).abs() < 1e-12);
    assert_eq!((num(&it, "wi"), num(&it, "fb")), (3.0, 1.0));
    assert_eq!((num(&it, "n"), num(&it, "nb")), (5.0, 3.0));
    // tolerances: by keyword and by position
    assert!(flag(&it, "loose"));
    assert!(flag(&it, "pos"));
    assert_eq!(num(&it, "tight"), 2.0); // 1e-6 passes atol 1e-4; 1e-3 and 5e-4 do not
}

#[test]
fn trace_diff_names_both_lengths_on_a_mismatch() {
    let m = err("d = trace_diff([1, 2, 3], [1, 2, 3, 4])");
    assert!(m.contains("3 samples") && m.contains("b has 4"), "{m}");
    let m = err("d = trace_diff([1, 2; 3, 4; 5, 6], [1, 2, 3; 4, 5, 6; 7, 8, 9])");
    assert!(m.contains("3x2") && m.contains("3x3"), "{m}");
    let m = err("d = trace_diff({t = [0, 1], u = [1, 2]}, {t = [0, 1], u = [1, 2, 3]})");
    assert!(m.contains("`u`") && m.contains("2 samples") && m.contains("3 in b"), "{m}");
    let m = err("d = trace_diff({t = [0, 1]}, {t = [0, 1], u = [1, 2]})");
    assert!(m.contains("only in b: `u`"), "{m}");
    assert!(err("d = trace_diff([1, 2], {t = [1, 2]})").contains("compare two"));
    assert!(err("d = trace_diff([1], [1], rtol=-1)").contains("rtol"));
}

#[test]
fn trace_diff_compares_matrices_column_by_column() {
    // rows are samples; the error sits in row 2 of column 1
    let it = run(
        "a = [1, 10; 2, 20; 3, 30]\nb = [1, 10; 2, 20; 3, 30.5]\nd = trace_diff(a, b)\n\
         ok = d.ok\nma = d.max_abs\nwi = d.worst_index\nwc = d.worst_col\nfb = d.first_bad_index\nfc = d.first_bad_col\nn = d.n\n\
         same = trace_diff(a, a)\nsok = same.ok\nswc = same.worst_col",
    );
    assert!(!flag(&it, "ok"));
    assert_eq!(num(&it, "ma"), 0.5);
    assert_eq!((num(&it, "wi"), num(&it, "wc")), (2.0, 1.0));
    assert_eq!((num(&it, "fb"), num(&it, "fc")), (2.0, 1.0));
    assert_eq!(num(&it, "n"), 6.0);
    assert!(flag(&it, "sok"));
    assert_eq!(num(&it, "swc"), -1.0);
}

#[test]
fn trace_diff_compares_records_by_signal_name() {
    // field order differs on purpose: signals are matched by name
    let it = run(
        "ref = {t = [0, 1, 2], u = [5, 6, 7], y = [1, 1, 1]}\ndut = {y = [1, 1.25, 1], t = [0, 1, 2], u = [5, 6, 7.125]}\n\
         d = trace_diff(ref, dut)\nok = d.ok\nma = d.max_abs\nws = d.worst_signal\nwi = d.worst_index\nfs = d.first_bad_signal\nfb = d.first_bad_index\nnb = d.n_bad\n\
         good = trace_diff(ref, ref)\ngok = good.ok\ngws = good.worst_signal",
    );
    assert!(!flag(&it, "ok"));
    assert_eq!(num(&it, "ma"), 0.25);
    assert_eq!((text(&it, "ws"), num(&it, "wi")), ("y".to_string(), 1.0));
    assert_eq!((text(&it, "fs"), num(&it, "fb")), ("y".to_string(), 1.0));
    assert_eq!(num(&it, "nb"), 2.0);
    assert!(flag(&it, "gok"));
    assert_eq!(text(&it, "gws"), "");
}

/// A scratch file under the system temp directory, removed on drop.
struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("qu_sim_events_{}_{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        Scratch(p)
    }
    fn qu(&self) -> String {
        self.0.to_string_lossy().replace('\\', "/")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

const AWKWARD: &str = "t = [0, 0.1, 0.2, 0.30000000000000004, 1e-320, 1.7976931348623157e308, 123456789.12345679]\n\
                       u = [1/3, -2/3, 0.1 + 0.2, -1e-17, 5e-324, 2.2250738585072014e-308, 9007199254740992]\n";

#[test]
fn a_saved_trace_loads_back_bit_for_bit_in_both_formats() {
    for ext in ["csv", "npz"] {
        let f = Scratch::new(&format!("roundtrip.{ext}"));
        let it = run(&format!(
            "{AWKWARD}tr = {{t = t, u = u}}\ntrace_save(\"{p}\", tr)\nback = trace_load(\"{p}\")\n\
             bt = back.t\nbu = back.u\nnames = keys(back)\nfirst = names[0]\nk = len(names)\n\
             d = trace_diff(tr, back, rtol=0, atol=0)\nok = d.ok\nma = d.max_abs",
            p = f.qu()
        ));
        let bits = |n: &str| nums(&it, n).iter().map(|x| x.to_bits()).collect::<Vec<u64>>();
        assert_eq!(bits("bt"), bits("t"), "{ext}: t");
        assert_eq!(bits("bu"), bits("u"), "{ext}: u");
        assert_eq!(nums(&it, "t").len(), 7);
        assert_eq!(nums(&it, "u")[4], 5e-324, "{ext}: the subnormal must not have been flushed to 0 before saving");
        assert_eq!((num(&it, "k"), text(&it, "first")), (2.0, "t".to_string()), "{ext}: signal order");
        assert!(flag(&it, "ok"), "{ext}");
        assert_eq!(num(&it, "ma"), 0.0, "{ext}");
        assert!(std::fs::metadata(&f.0).unwrap().len() > 0);
    }
}

#[test]
fn saving_is_deterministic_and_a_table_saves_like_a_record() {
    for ext in ["csv", "npz"] {
        let (a, b, c) = (Scratch::new(&format!("det_a.{ext}")), Scratch::new(&format!("det_b.{ext}")), Scratch::new(&format!("det_c.{ext}")));
        let it = run(&format!(
            "{AWKWARD}trace_save(\"{a}\", {{t = t, u = u}})\ntrace_save(\"{b}\", {{t = t, u = u}})\n\
             trace_save(\"{c}\", table(t = t, u = u))\nback = trace_load(\"{c}\")\nbu = back.u",
            a = a.qu(),
            b = b.qu(),
            c = c.qu()
        ));
        let (fa, fb, fc) = (std::fs::read(&a.0).unwrap(), std::fs::read(&b.0).unwrap(), std::fs::read(&c.0).unwrap());
        assert!(!fa.is_empty());
        assert_eq!(fa, fb, "{ext}: two saves of the same trace differ");
        assert_eq!(fa, fc, "{ext}: a table and a record with the same signals differ");
        let bits = |n: &str| nums(&it, n).iter().map(|x| x.to_bits()).collect::<Vec<u64>>();
        assert_eq!(bits("bu"), bits("u"), "{ext}");
    }
}

#[test]
fn a_csv_trace_is_plain_csv_with_a_header_row() {
    let f = Scratch::new("plain.csv");
    run(&format!("trace_save(\"{}\", {{t = [0, 0.5], u = [1.25, -3]}})", f.qu()));
    let text = std::fs::read_to_string(&f.0).unwrap();
    assert_eq!(text.replace("\r\n", "\n"), "t,u\n0,1.25\n0.5,-3\n");
}

#[test]
fn trace_save_and_load_refuse_what_is_not_a_trace() {
    let f = Scratch::new("bad.csv");
    let p = f.qu();
    let m = err(&format!("trace_save(\"{p}\", {{t = [0, 1, 2], u = [1, 2]}})"));
    assert!(m.contains("`u` has 2 samples") && m.contains("`t` has 3"), "{m}");
    assert!(err(&format!("trace_save(\"{p}\", {{t = [0, 1], name = \"run 7\"}})")).contains("`name`"));
    assert!(err(&format!("trace_save(\"{p}\", [1, 2, 3])")).contains("record of vectors or a table"));
    assert!(err("trace_save(\"trace.txt\", {t = [0, 1]})").contains(".csv"));
    assert!(err("x = trace_load(\"definitely_not_here_4711.csv\")").contains("could not read"));
    // nothing was written by the refused calls
    assert!(!f.0.exists());
    // a CSV with a text column is a table, not a trace
    std::fs::write(&f.0, "t,label\n0,start\n1,stop\n").unwrap();
    assert!(err(&format!("x = trace_load(\"{p}\")")).contains("`label` is not numeric"));
}

#[test]
fn trace_save_is_a_file_writer_for_the_sandbox() {
    let f = Scratch::new("sandbox.csv");
    let mut it = Interp::new();
    it.set_sandboxed(true);
    let e = it.run(&format!("trace_save(\"{}\", {{t = [0, 1]}})", f.qu())).expect_err("sandbox must refuse").to_string();
    assert!(e.contains("sandbox"), "{e}");
    assert!(!f.0.exists());
}

#[test]
fn record_replay_and_compare_an_ode_run_back_to_back() {
    // the workflow the three builtins exist for: record a reference run,
    // replay it from disk, compare a second run against it
    let f = Scratch::new("reference.npz");
    let it = run(&format!(
        "{MODELS}\nte = linspace(0, 5, 51)\nr = ode45(\"sho\", [0, 5], [1, 0], t_eval=te, rtol=1e-10, atol=1e-12)\n\
         trace_save(\"{p}\", {{t = r.t, y = r.y[:, 0], v = r.y[:, 1]}})\n\
         ref = trace_load(\"{p}\")\n\
         again = ode45(\"sho\", [0, 5], [1, 0], t_eval=te, rtol=1e-10, atol=1e-12)\n\
         same = trace_diff(ref, {{t = again.t, y = again.y[:, 0], v = again.y[:, 1]}}, rtol=0, atol=0)\nsok = same.ok\n\
         coarse = ode23(\"sho\", [0, 5], [1, 0], t_eval=te, rtol=1e-4, atol=1e-6)\n\
         d = trace_diff(ref, {{t = coarse.t, y = coarse.y[:, 0], v = coarse.y[:, 1]}}, rtol=1e-9, atol=1e-9)\n\
         dok = d.ok\nma = d.max_abs\nws = d.worst_signal\n\
         exact = trace_diff(ref.y, cos(te), rtol=0, atol=1e-8).ok",
        p = f.qu()
    ));
    assert!(flag(&it, "sok"), "an identical rerun must match the recording exactly");
    assert!(!flag(&it, "dok"), "a loose-tolerance solver must not pass a 1e-9 comparison");
    let ma = num(&it, "ma");
    assert!(ma > 1e-7 && ma < 1e-2, "max_abs {ma}");
    assert_ne!(text(&it, "ws"), "t");
    assert!(flag(&it, "exact"));
}

// ================================================================ 3. pacing

const TICKER: &str = "count = 0\nevery 50 ms do\n    count = count + 1\nend\n";

#[test]
fn run_for_without_pace_is_still_virtual_and_unchanged() {
    let start = Instant::now();
    let it = run(&format!(
        "{TICKER}fired = run_for(5)\ns = run_for_stats()\nsteps = s.steps\nov = s.overruns\npaced = s.paced\nmx = s.max_lateness_ms\nmean = s.mean_lateness_ms\nlate = s.lateness_ms"
    ));
    // 5 virtual seconds of a 50 ms timer: 100 firings, no real waiting
    assert!(start.elapsed().as_millis() < 2000, "a virtual run took {:?}", start.elapsed());
    assert_eq!(num(&it, "fired"), 100.0);
    assert_eq!(num(&it, "count"), 100.0);
    assert_eq!(it.out, "· run_for(5s): 100 timer firing(s) across 1 registered timer(s)\n");
    assert!(!flag(&it, "paced"));
    assert_eq!((num(&it, "steps"), num(&it, "ov")), (100.0, 0.0));
    assert_eq!((num(&it, "mx"), num(&it, "mean")), (0.0, 0.0));
    assert!(nums(&it, "late").is_empty());
}

#[test]
fn a_paced_run_of_ten_50_ms_steps_takes_real_time_and_reports_no_overrun() {
    // Lateness tolerance for this test: 40 ms, deliberately loose because the
    // suite runs on shared machines; the point is that a step is not late by
    // most of a period, and that the run really occupied the wall clock.
    // Everything except the lateness bound must hold on every attempt; the
    // lateness bound gets three attempts, because a machine that is busy
    // compiling can starve any thread for longer than that once.
    let mut lateness = Vec::new();
    for _attempt in 0..3 {
        let start = Instant::now();
        let it = run(&format!(
            "{TICKER}fired = run_for(500 ms, pace=1, tolerance=40 ms)\ns = run_for_stats()\n\
             steps = s.steps\nov = s.overruns\npaced = s.paced\npace = s.pace\nmx = s.max_lateness_ms\nmean = s.mean_lateness_ms\nwall = s.wall_ms\nlate = s.lateness_ms"
        ));
        let wall = start.elapsed().as_secs_f64() * 1e3;
        assert!(wall >= 450.0, "10 steps of 50 ms took only {wall} ms");
        assert!(wall < 10_000.0, "{wall} ms");
        assert_eq!(num(&it, "fired"), 10.0);
        assert_eq!(num(&it, "count"), 10.0);
        assert!(flag(&it, "paced"));
        assert_eq!(num(&it, "pace"), 1.0);
        assert_eq!(num(&it, "steps"), 10.0);
        let (mx, mean) = (num(&it, "mx"), num(&it, "mean"));
        assert!(mean >= 0.0 && mean <= mx);
        let late = nums(&it, "late");
        assert_eq!(late.len(), 10);
        assert!(late.iter().all(|&l| l >= 0.0 && l <= mx));
        assert_eq!(late.iter().cloned().fold(0.0, f64::max), mx);
        // overruns is exactly "how many steps were later than the tolerance"
        assert_eq!(num(&it, "ov"), late.iter().filter(|&&l| l > 40.0).count() as f64);
        let reported = num(&it, "wall");
        assert!(reported >= 499.0 && reported <= wall + 1.0, "reported {reported} ms, measured {wall} ms");
        // the printed line and the return value are the unpaced ones
        assert_eq!(it.out, "· run_for(0.5s): 10 timer firing(s) across 1 registered timer(s)\n");
        lateness.push(mx);
        if mx <= 40.0 {
            assert_eq!(num(&it, "ov"), 0.0);
            return;
        }
    }
    panic!("max lateness exceeded the 40 ms tolerance on all three attempts: {lateness:?} ms");
}

#[test]
fn pace_is_a_real_time_factor() {
    // 1.02 virtual seconds at pace 10 are 102 ms of wall clock. (Not 1.0: the
    // timeline adds 50 ms twenty times, which lands a hair above 1.0, so an
    // unpaced run_for(1) fires 19 times too. That is the timeline's existing
    // behaviour, not something pacing changes.)
    let start = Instant::now();
    let it = run(&format!("{TICKER}fired = run_for(1.02, pace=10, tolerance=40 ms)\nwall = run_for_stats().wall_ms\nsteps = run_for_stats().steps"));
    let wall = start.elapsed().as_secs_f64() * 1e3;
    assert!(wall >= 101.0 && wall < 900.0, "{wall} ms");
    assert_eq!(num(&it, "fired"), 20.0);
    assert_eq!(num(&it, "steps"), 20.0);
    assert!(num(&it, "wall") >= 101.9);
}

#[test]
fn a_step_that_takes_longer_than_its_period_is_counted_as_an_overrun() {
    // 10 ms period, 30 ms of work per step: every step after the first is late
    let it = run(
        "every 10 ms do\n    sleep(30)\nend\nfired = run_for(50 ms, pace=1)\ns = run_for_stats()\n\
         ov = s.overruns\nsteps = s.steps\nmx = s.max_lateness_ms\nmean = s.mean_lateness_ms",
    );
    assert_eq!(num(&it, "fired"), 5.0);
    assert_eq!(num(&it, "steps"), 5.0);
    assert!(num(&it, "ov") >= 4.0, "overruns {}", num(&it, "ov"));
    // step 5 is due at 50 ms but the four before it took at least 120 ms
    assert!(num(&it, "mx") >= 70.0, "max lateness {}", num(&it, "mx"));
    assert!(num(&it, "mean") > 20.0);
}

#[test]
fn pacing_misuse_is_reported() {
    assert!(err(&format!("{TICKER}run_for(1, pace=0)")).contains("pace must be a positive number"));
    assert!(err(&format!("{TICKER}run_for(1, pace=-2)")).contains("pace must be a positive number"));
    assert!(err(&format!("{TICKER}run_for(1, pace=\"fast\")")).contains("pace must be a number"));
    assert!(err(&format!("{TICKER}run_for(1, tolerance=5 ms)")).contains("only applies to a paced run"));
    assert!(err("s = run_for_stats()").contains("no run_for"));
}
