//! The `lti_*` builtins (linear time-invariant system objects, v0.4.13),
//! through the language. The numerics are unit-tested in `qu_core::lti` and
//! the argument handling in `lti_ops.rs`; these run Qu source and check the
//! numbers a script would see against closed forms: a first-order lag's step
//! response, a second-order overshoot, an integrator's ramp, a discrete
//! accumulator, zero-order-hold and Tustin pole mappings, closed-loop poles
//! against the roots of `1 + L`, and the margins of a textbook loop.

use qu_interp::{Interp, Value};
use std::f64::consts::PI;

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

/// A real variable as a flat vector (a matrix reads column-major).
fn nums(it: &Interp, n: &str) -> Vec<f64> {
    match it.get(n) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Num(x)) => vec![*x],
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    let v = nums(it, n);
    assert_eq!(v.len(), 1, "`{n}` is not a scalar: {v:?}");
    v[0]
}

fn shape(it: &Interp, n: &str) -> (usize, usize) {
    match it.get(n) {
        Some(Value::Mat(m)) => m.shape(),
        other => panic!("`{n}` is not a matrix: {other:?}"),
    }
}

/// A complex variable as `(re, im)` pairs, sorted.
fn cplx(it: &Interp, n: &str) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = match it.get(n) {
        Some(Value::CVec(v)) => v.iter().map(|c| (c.re, c.im)).collect(),
        Some(Value::Complex(c)) => vec![(c.re, c.im)],
        other => panic!("`{n}` is {other:?}"),
    };
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    out
}

fn flag(it: &Interp, n: &str) -> bool {
    match it.get(n) {
        Some(Value::Bool(b)) => *b,
        other => panic!("`{n}` is {other:?}"),
    }
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.to_string(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} vs {b} (tol {tol})");
}

fn close_all(a: &[f64], b: &[f64], tol: f64) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= tol, "{a:?} vs {b:?}");
    }
}

fn close_c(a: &[(f64, f64)], b: &[(f64, f64)], tol: f64) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x.0 - y.0).hypot(x.1 - y.1) <= tol, "{a:?} vs {b:?}");
    }
}

// ------------------------------------------------------------ constructors

#[test]
fn lti_ss_builds_a_model_with_matrix_fields() {
    let it = run(r#"
sys = lti_ss([0, 1; -2, -3], [0; 1], [1, 0], 0)
form = sys.form
A = sys.A
B = sys.B
C = sys.C
D = sys.D
dt = sys.dt
kind = "{sys}"
d = lti_ss(0.5, 1, 1, 0, 0.25)
ddt = d.dt
dA = d.A
k = lti_ss(0.5, 1, 1, 0, dt=0.1)
kdt = k.dt
"#);
    assert_eq!(text(&it, "form"), "ss");
    assert_eq!(nums(&it, "A"), vec![0.0, -2.0, 1.0, -3.0]); // column-major
    assert_eq!((shape(&it, "A"), shape(&it, "B"), shape(&it, "C"), shape(&it, "D")), ((2, 2), (2, 1), (1, 2), (1, 1)));
    assert_eq!(nums(&it, "B"), vec![0.0, 1.0]);
    assert_eq!(nums(&it, "C"), vec![1.0, 0.0]);
    assert_eq!(num(&it, "dt"), 0.0);
    assert!(text(&it, "kind").starts_with("model(lti)"), "{}", text(&it, "kind"));
    assert_eq!(num(&it, "ddt"), 0.25);
    assert_eq!(shape(&it, "dA"), (1, 1));
    assert_eq!(num(&it, "kdt"), 0.1);
}

#[test]
fn lti_ss_reports_which_matrix_does_not_fit() {
    assert!(err("s = lti_ss([1, 2, 3; 4, 5, 6], [1; 2], [1, 0], 0)").contains("A must be square, got 2x3"));
    assert!(err("s = lti_ss([0, 1; -2, -3], [1; 2; 3], [1, 0], 0)").contains("B must have 2 row(s)"));
    assert!(err("s = lti_ss([0, 1; -2, -3], [0; 1], [1, 0, 0], 0)").contains("C must have 2 column(s)"));
    assert!(err("s = lti_ss([0, 1; -2, -3], [0; 1], [1, 0], [1, 2])").contains("D must be 1x1"));
    assert!(err("s = lti_ss([0, 1; -2, -3], [0; 1], [1, 0])").contains("missing argument 4"));
    assert!(err("s = lti_ss([0, 1; -2, -3], [0; 1], [1, 0], 0, -0.1)").contains("dt must be 0 (continuous) or a positive number"));
    assert!(err("s = lti_ss(\"A\", 1, 1, 0)").contains("A must be a number, a vector or a matrix"));
}

#[test]
fn lti_tf_is_controllable_canonical_and_rejects_improper() {
    let it = run(r#"
g = lti_tf([2, 3], [1, 4, 5])
form = g.form
num = g.num
den = g.den
A = g.A
B = g.B
C = g.C
D = g.D
h = lti_tf([4, 2, 6], [2, 6, 4])
hD = h.D
d = lti_tf([1], [1, -0.5], 0.1)
ddt = d.dt
"#);
    assert_eq!(text(&it, "form"), "tf");
    assert_eq!(nums(&it, "num"), vec![2.0, 3.0]);
    assert_eq!(nums(&it, "den"), vec![1.0, 4.0, 5.0]);
    assert_eq!(nums(&it, "A"), vec![0.0, -5.0, 1.0, -4.0]);
    assert_eq!(nums(&it, "B"), vec![0.0, 1.0]);
    assert_eq!(nums(&it, "C"), vec![3.0, 2.0]);
    assert_eq!(nums(&it, "D"), vec![0.0]);
    assert_eq!(nums(&it, "hD"), vec![2.0]);
    assert_eq!(num(&it, "ddt"), 0.1);
    let msg = err("g = lti_tf([1, 0, 0], [1, 1])");
    assert!(msg.contains("lti_tf: improper transfer function") && msg.contains("degree 2") && msg.contains("degree 1"), "{msg}");
    assert!(err("g = lti_tf([1], [0, 0])").contains("denominator is zero"));
    assert!(err("g = lti_tf([1])").contains("missing argument 2"));
    assert!(err("g = lti_tf([1], \"s\")").contains("den must be a numeric vector"));
}

#[test]
fn lti_zpk_builds_from_real_and_complex_roots() {
    let it = run(r#"
z = lti_zpk([-3], [-1 + 2i, -1 - 2i], 5)
form = z.form
p = z.p
k = z.k
t = lti_to_tf(z)
num = t.num
den = t.den
poles = lti_poles(lti_to_ss(z))
nozero = lti_zpk([], [-1, -2], 4)
gain = lti_dcgain(nozero)
"#);
    assert_eq!(text(&it, "form"), "zpk");
    assert_eq!(cplx(&it, "p"), vec![(-1.0, -2.0), (-1.0, 2.0)]);
    assert_eq!(num(&it, "k"), 5.0);
    assert_eq!(nums(&it, "num"), vec![5.0, 15.0]);
    assert_eq!(nums(&it, "den"), vec![1.0, 2.0, 5.0]);
    close_c(&cplx(&it, "poles"), &[(-1.0, -2.0), (-1.0, 2.0)], 1e-12);
    close(num(&it, "gain"), 2.0, 1e-15);
    assert!(err("z = lti_zpk([], [-1 + 2i], 1)").contains("conjugate pairs"));
    assert!(err("z = lti_zpk([1, 2], [-1], 1)").contains("improper"));
    assert!(err("z = lti_zpk([], [-1], [1, 2])").contains("k must be a number"));
}

// ------------------------------------------------- conversions and analysis

#[test]
fn conversions_round_trip_and_refuse_mimo() {
    let it = run(r#"
z = lti_zpk([-3], [-1, -2], 2)
t = lti_to_tf(z)
tform = t.form
num = t.num
den = t.den
s = lti_to_ss(t)
sform = s.form
back = lti_to_tf(s)
num2 = back.num
den2 = back.den
zz = lti_to_zpk(s)
zform = zz.form
zs = zz.z
poles = zz.p
gain = zz.k
"#);
    assert_eq!((text(&it, "tform"), text(&it, "sform"), text(&it, "zform")), ("tf".into(), "ss".into(), "zpk".into()));
    assert_eq!(nums(&it, "num"), vec![2.0, 6.0]);
    assert_eq!(nums(&it, "den"), vec![1.0, 3.0, 2.0]);
    assert_eq!(nums(&it, "num2"), vec![2.0, 6.0]);
    assert_eq!(nums(&it, "den2"), vec![1.0, 3.0, 2.0]);
    close_c(&cplx(&it, "zs"), &[(-3.0, 0.0)], 1e-12);
    close_c(&cplx(&it, "poles"), &[(-2.0, 0.0), (-1.0, 0.0)], 1e-12);
    close(num(&it, "gain"), 2.0, 1e-14);
    let mimo = "m = lti_ss(-1, [1, 2], [3; 4], 0)\n";
    assert!(err(&format!("{mimo}t = lti_to_tf(m)")).contains("this system has 2 input(s) and 2 output(s)"));
    assert!(err(&format!("{mimo}t = lti_to_zpk(m)")).contains("2 input(s) and 2 output(s)"));
    assert!(err(&format!("{mimo}t = lti_zeros(m)")).contains("one input and one output"));
    assert!(err("t = lti_to_tf(3)").contains("must be an lti system"));
    assert!(err("t = lti_poles(pid_init(1, 0, 0, 0.1))").contains("found a `pid` model"));
}

#[test]
fn poles_zeros_dcgain_stability_and_frequency_response() {
    let it = run(r#"
g = lti_tf([1, 5], [1, 3, 2])
p = lti_poles(g)
z = lti_zeros(g)
k = lti_dcgain(g)
ok = lti_is_stable(g)
h = lti_freqresp(g, [0, 1])
h1 = lti_freqresp(g, 1)
ps = lti_poles(lti_ss([0, 1; -2, -3], [0; 1], [5, 1], 0))
unstable = lti_is_stable(lti_tf([1], [1, -1]))
marginal = lti_is_stable(lti_tf([1], [1, 0]))
d_in = lti_is_stable(lti_tf([1], [1, 0.9], 1))
d_out = lti_is_stable(lti_tf([1], [1, 1.1], 1))
kd = lti_dcgain(lti_tf([1, 0], [1, -0.5], 0.1))
kint = lti_dcgain(lti_tf([1], [1, 0]))
km = lti_dcgain(lti_ss(-1, [1, 2], [3; 4], 0))
hd = lti_freqresp(lti_tf([1], [1, -0.5], 0.01), pi)
nz = len(lti_zeros(lti_tf([4], [1, 1])))
"#);
    close_c(&cplx(&it, "p"), &[(-2.0, 0.0), (-1.0, 0.0)], 1e-12);
    close_c(&cplx(&it, "ps"), &[(-2.0, 0.0), (-1.0, 0.0)], 1e-12);
    close_c(&cplx(&it, "z"), &[(-5.0, 0.0)], 1e-12);
    close(num(&it, "k"), 2.5, 1e-14);
    assert!(flag(&it, "ok") && !flag(&it, "unstable") && !flag(&it, "marginal"));
    assert!(flag(&it, "d_in") && !flag(&it, "d_out"));
    // H(0) = 5/2, H(j) = (5 + j)/(1 + 3j) = 0.8 - 1.4j
    close_c(&cplx(&it, "h"), &[(0.8, -1.4), (2.5, 0.0)], 1e-14);
    close_c(&cplx(&it, "h1"), &[(0.8, -1.4)], 1e-14);
    close(num(&it, "kd"), 2.0, 1e-15);
    assert_eq!(num(&it, "kint"), f64::INFINITY);
    assert_eq!(nums(&it, "km"), vec![3.0, 4.0, 6.0, 8.0]); // [3, 6; 4, 8] column-major
    // discrete, w in rad/sample: at w = pi, 1/(z - 0.5) = 1/(-1.5)
    close_c(&cplx(&it, "hd"), &[(-1.0 / 1.5, 0.0)], 1e-15);
    assert_eq!(num(&it, "nz"), 0.0);
    assert!(err("h = lti_freqresp(lti_ss(-1, [1, 2], [3; 4], 0), [1])").contains("one input and one output"));
}

// --------------------------------------------------------------- simulation

#[test]
fn first_order_lag_step_matches_one_minus_exp() {
    let it = run(r#"
tau = 0.7
g = lti_tf([1], [tau, 1])
r = lti_step(g, 5)
t = r.t
y = r.y
x = r.x
worst = max(abs(y - (1 - exp(-t / tau))))
r2 = lti_step(g, [0, 0.01, 0.5, 0.6, 3])
y2 = r2.y
r3 = lti_step(g, 5, n=11)
t3 = r3.t
"#);
    let (t, y) = (nums(&it, "t"), nums(&it, "y"));
    assert_eq!(t.len(), 501);
    assert_eq!((t[0], t[500]), (0.0, 5.0));
    for (tk, yk) in t.iter().zip(&y) {
        close(*yk, 1.0 - (-tk / 0.7).exp(), 2e-15);
    }
    assert!(num(&it, "worst") <= 2e-15, "worst error {}", num(&it, "worst"));
    assert_eq!(shape(&it, "x"), (501, 1));
    // uneven sample times are just as exact
    let want: Vec<f64> = [0.0f64, 0.01, 0.5, 0.6, 3.0].iter().map(|t| 1.0 - (-t / 0.7).exp()).collect();
    close_all(&nums(&it, "y2"), &want, 2e-15);
    assert_eq!(nums(&it, "t3").len(), 11);
}

#[test]
fn second_order_underdamped_overshoot() {
    let it = run(r#"
zeta = 0.3
wn = 2
g = lti_tf([wn^2], [1, 2*zeta*wn, wn^2])
wd = wn * sqrt(1 - zeta^2)
r = lti_step(g, [0, pi / wd])
at_peak = r.y[1] - 1
dense = lti_step(g, 10, n=20001)
overshoot = max(dense.y) - 1
last_y = dense.y[20000]
"#);
    let want = (-PI * 0.3 / (1.0f64 - 0.09).sqrt()).exp();
    assert!(want > 0.37 && want < 0.38);
    close(num(&it, "at_peak"), want, 1e-14);
    close(num(&it, "overshoot"), want, 1e-6);
    close(num(&it, "last_y"), 1.0, 5e-3);
}

#[test]
fn integrator_ramp_and_lsim_holds() {
    let it = run(r#"
g = lti_tf([1], [1, 0])
r = lti_step(g, 4, n=9)
ramp = r.y
t = r.t
foh = lti_lsim(g, t, t).y
zoh = lti_lsim(g, t, t, method="zoh").y
from3 = lti_lsim(g, zeros(9), t, [3]).y
from4 = lti_lsim(g, zeros(9), t, x0=4).y
lag = lti_tf([1], [0.5, 1])
lr = lti_lsim(lag, t, t).y
"#);
    let t: Vec<f64> = (0..9).map(|k| 0.5 * k as f64).collect();
    assert_eq!(nums(&it, "t"), t);
    close_all(&nums(&it, "ramp"), &t, 1e-15);
    // a ramp into an integrator: t^2/2 exactly under first-order hold,
    // the left Riemann sum under zero-order hold
    let half_sq: Vec<f64> = t.iter().map(|v| v * v / 2.0).collect();
    close_all(&nums(&it, "foh"), &half_sq, 1e-14);
    let left: Vec<f64> = (0..9).map(|k| t[..k].iter().map(|v| v * 0.5).sum()).collect();
    close_all(&nums(&it, "zoh"), &left, 1e-14);
    assert_eq!(nums(&it, "from3"), vec![3.0; 9]);
    assert_eq!(nums(&it, "from4"), vec![4.0; 9]);
    // a ramp into a lag: t - tau (1 - e^{-t/tau})
    let want: Vec<f64> = t.iter().map(|v| v - 0.5 * (1.0 - (-v / 0.5).exp())).collect();
    close_all(&nums(&it, "lr"), &want, 1e-14);
    let setup = "g = lti_tf([1], [1, 0])\n";
    assert!(err(&format!("{setup}r = lti_lsim(g, [1, 2], [0, 1, 2])")).contains("one row per time sample"));
    assert!(err(&format!("{setup}r = lti_lsim(g, [1, 2, 3], [0, 2, 1])")).contains("strictly increasing"));
    assert!(err(&format!("{setup}r = lti_lsim(g, [1, 2], [0, 1], [1, 2])")).contains("x0 must have 1 element"));
    assert!(err(&format!("{setup}r = lti_lsim(g, [1, 2], [0, 1], method=\"rk4\")")).contains("unknown method"));
    assert!(err(&format!("{setup}r = lti_step(g, -1)")).contains("end time must be a positive number"));
}

#[test]
fn discrete_accumulator_runs_the_difference_equation() {
    let it = run(r#"
acc = lti_tf([1], [1, -1], 0.5)
r = lti_step(acc, 3)
t = r.t
y = r.y
s = lti_lsim(acc, [3, -1, 4, 1], [0, 0.5, 1, 1.5]).y
imp = lti_impulse(lti_tf([1], [1, -0.5], 1), 3).y
"#);
    assert_eq!(nums(&it, "t"), vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0]);
    assert_eq!(nums(&it, "y"), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(nums(&it, "s"), vec![0.0, 3.0, 2.0, 6.0]);
    assert_eq!(nums(&it, "imp"), vec![0.0, 1.0, 0.5, 0.25]);
    let setup = "acc = lti_tf([1], [1, -1], 0.5)\n";
    assert!(err(&format!("{setup}r = lti_lsim(acc, [1, 2, 3], [0, 1, 2])")).contains("must step by dt"));
    assert!(err(&format!("{setup}r = lti_step(acc, 3, n=10)")).contains("does not apply to a discrete system"));
}

#[test]
fn impulse_of_a_lag_and_multi_input_step() {
    let it = run(r#"
tau = 0.25
r = lti_impulse(lti_tf([1], [tau, 1]), 2, n=21)
worst = max(abs(r.y - exp(-r.t / tau) / tau))
count = len(r.y)
m = lti_ss([-1, 0; 0, -2], [1, 0; 0, 1], [1, 0; 0, 1], 0)
a = lti_step(m, [0, 40])
b = lti_step(m, [0, 40], input=1)
ya = a.y
yb = b.y
"#);
    assert_eq!(num(&it, "count"), 21.0);
    assert!(num(&it, "worst") <= 1e-13, "{}", num(&it, "worst"));
    // two outputs: y is len(t) x 2; stepping input 0 moves output 0 to 1,
    // stepping input 1 moves output 1 to 1/2
    assert_eq!(shape(&it, "ya"), (2, 2));
    close_all(&nums(&it, "ya"), &[0.0, 1.0, 0.0, 0.0], 1e-12);
    close_all(&nums(&it, "yb"), &[0.0, 0.0, 0.0, 0.5], 1e-12);
    assert!(err("m = lti_ss(-1, [1, 2], 1, [0, 0])\nr = lti_step(m, 1, input=2)").contains("input index 2 is out of range"));
}

// -------------------------------------------------------------- connections

#[test]
fn series_parallel_and_feedback_match_polynomial_algebra() {
    let it = run(r#"
g1 = lti_tf([1], [1, 1])
g2 = lti_tf([2], [1, 3])
s = lti_to_tf(lti_series(g1, g2))
s_num = s.num
s_den = s.den
p = lti_to_tf(lti_parallel(g1, g2))
p_num = p.num
p_den = p.den
k = lti_to_tf(lti_series(5, g1))
k_num = k.num
u = lti_to_tf(lti_feedback(g1))
u_den = u.den
f = lti_to_tf(lti_feedback(g1, g2))
f_num = f.num
f_den = f.den
pos = lti_to_tf(lti_feedback(g1, g2, sign=1))
pos_den = pos.den
sform = lti_series(g1, g2).form
"#);
    // 2/((s+1)(s+3)); 1/(s+1) + 2/(s+3) = (3s+5)/(s^2+4s+3)
    assert_eq!((nums(&it, "s_num"), nums(&it, "s_den")), (vec![2.0], vec![1.0, 4.0, 3.0]));
    assert_eq!((nums(&it, "p_num"), nums(&it, "p_den")), (vec![3.0, 5.0], vec![1.0, 4.0, 3.0]));
    assert_eq!(nums(&it, "k_num"), vec![5.0]);
    // 1/(s+1) with unity feedback: 1/(s+2)
    assert_eq!(nums(&it, "u_den"), vec![1.0, 2.0]);
    // G/(1+GH) = (s+3)/(s^2+4s+5); positive feedback (s+3)/(s^2+4s+1)
    assert_eq!((nums(&it, "f_num"), nums(&it, "f_den")), (vec![1.0, 3.0], vec![1.0, 4.0, 5.0]));
    assert_eq!(nums(&it, "pos_den"), vec![1.0, 4.0, 1.0]);
    assert_eq!(text(&it, "sform"), "ss");
    let setup = "g = lti_tf([1], [1, 1])\nd = lti_tf([1], [1, -0.5], 0.1)\n";
    assert!(err(&format!("{setup}s = lti_series(g, d)")).contains("cannot connect a continuous system with a discrete one"));
    assert!(err(&format!("{setup}s = lti_parallel(d, lti_tf([1], [1, -0.5], 0.2))")).contains("different sample times"));
    assert!(err(&format!("{setup}s = lti_feedback(g, sign=3)")).contains("sign must be -1"));
    assert!(err(&format!("{setup}s = lti_feedback(1, 1, sign=1)")).contains("algebraic"));
    assert!(err(&format!("{setup}s = lti_series(g, \"x\")")).contains("the second argument must be an lti system"));
}

#[test]
fn closed_loop_poles_are_the_roots_of_one_plus_l() {
    // L = 10/(s(s+1)(s+2)): 1 + L = 0  <=>  s^3 + 3s^2 + 2s + 10 = 0
    let it = run(r#"
L = lti_tf([10], [1, 3, 2, 0])
cl = lti_feedback(L)
p = lti_poles(cl)
den = lti_to_tf(cl).den
ok = lti_is_stable(cl)
"#);
    let p = cplx(&it, "p");
    assert_eq!(p.len(), 3);
    for &(re, im) in &p {
        // evaluate s^3 + 3 s^2 + 2 s + 10 at s = re + j im by Horner
        let (mut ar, mut ai) = (1.0, 0.0);
        for c in [3.0, 2.0, 10.0] {
            let (nr, ni) = (ar * re - ai * im + c, ar * im + ai * re);
            ar = nr;
            ai = ni;
        }
        assert!(ar.hypot(ai) < 1e-11, "{re}+{im}j is not a root of 1 + L (residual {})", ar.hypot(ai));
    }
    // one real root and a conjugate pair in the right half plane: gain 10 > margin 6
    assert!(p[0].0 < -3.0 && p[0].1 == 0.0);
    assert!(p[1].0 > 0.0 && (p[1].0 - p[2].0).abs() < 1e-12 && (p[1].1 + p[2].1).abs() < 1e-12 && p[2].1 > 1.0);
    assert_eq!(nums(&it, "den"), vec![1.0, 3.0, 2.0, 10.0]);
    assert!(!flag(&it, "ok"));
}

// ----------------------------------------------------------- discretization

#[test]
fn c2d_zoh_of_a_lag_has_its_pole_at_exp_minus_dt_over_tau() {
    let it = run(r#"
tau = 0.4
dt = 0.1
d = lti_c2d(lti_tf([1], [tau, 1]), dt)
A = d.A
B = d.B
ddt = d.dt
pole = lti_poles(d)
gain = lti_dcgain(d)
y = lti_step(d, 1).y
t = lti_step(d, 1).t
worst = max(abs(y - (1 - exp(-t / tau))))
"#);
    let pole = (-0.1f64 / 0.4).exp();
    close(nums(&it, "A")[0], pole, 2e-16);
    close(cplx(&it, "pole")[0].0, pole, 2e-16);
    close(num(&it, "ddt"), 0.1, 0.0);
    close(num(&it, "gain"), 1.0, 1e-15);
    // the sampled step response is the continuous one at the samples
    assert_eq!(nums(&it, "y").len(), 11);
    assert!(num(&it, "worst") <= 1e-14, "{}", num(&it, "worst"));
}

#[test]
fn tustin_maps_the_left_half_plane_into_the_unit_disc() {
    let h = 0.3;
    for (re, im) in [(-0.01, 0.0), (-1.0, 5.0), (-40.0, 0.0), (-3.0, 100.0), (-1000.0, 0.0)] {
        let poles = if im == 0.0 { format!("[{re}]") } else { format!("[{re} + {im}i, {re} - {im}i]") };
        let it = run(&format!(
            "c = lti_zpk([], {poles}, 1)\nd = lti_c2d(c, {h}, \"tustin\")\np = lti_poles(d)\nok = lti_is_stable(d)\neu = lti_poles(lti_c2d(c, {h}, method=\"euler\"))\n"
        ));
        let p = cplx(&it, "p");
        assert_eq!(p.len(), if im == 0.0 { 1 } else { 2 });
        for &(zr, zi) in &p {
            assert!(zr.hypot(zi) < 1.0, "s = {re}+{im}j mapped to |z| = {}", zr.hypot(zi));
            // z = (1 + s h/2)/(1 - s h/2), with s the pole of matching sign
            let (sr, si) = (re * h / 2.0, im * h / 2.0 * zi.signum());
            let den = (1.0 - sr) * (1.0 - sr) + si * si;
            let want = (((1.0 + sr) * (1.0 - sr) - si * si) / den, (si * (1.0 - sr) + si * (1.0 + sr)) / den);
            assert!((zr - want.0).hypot(zi - want.1) < 1e-9, "{zr}+{zi}j vs {want:?}");
        }
        assert!(flag(&it, "ok"));
        // forward Euler: z = 1 + s h, which leaves the disc for the fast poles
        let e = cplx(&it, "eu");
        close(e[0].0, 1.0 + re * h, 1e-9 * (1.0 + (re * h).abs()));
        if re <= -40.0 {
            assert!(e[0].0.hypot(e[0].1) > 1.0);
        }
    }
    // the right half plane maps outside, the imaginary axis onto the circle
    let it = run("u = lti_poles(lti_c2d(lti_tf([1], [1, -2]), 0.3, \"tustin\"))\no = lti_poles(lti_c2d(lti_tf([1], [1, 0, 4]), 0.3, \"tustin\"))\n");
    assert!(cplx(&it, "u")[0].0 > 1.0);
    for (zr, zi) in cplx(&it, "o") {
        close(zr.hypot(zi), 1.0, 1e-14);
    }
    assert!(err("d = lti_c2d(lti_tf([1], [1, 1]), 0.1, \"magic\")").contains("unknown method"));
    assert!(err("d = lti_c2d(lti_tf([1], [1, 1]), 0)").contains("dt must be a positive number"));
    assert!(err("d = lti_c2d(lti_tf([1], [1, -0.5], 0.1), 0.1)").contains("already discrete"));
}

#[test]
fn d2c_inverts_c2d() {
    let it = run(r#"
g = lti_tf([1, 3], [1, 2, 5])
z = lti_to_tf(lti_d2c(lti_c2d(g, 0.05)))
z_num = z.num
z_den = z.den
t = lti_to_tf(lti_d2c(lti_c2d(g, 0.05, "tustin"), "tustin"))
t_num = t.num
t_den = t.den
back_dt = lti_d2c(lti_c2d(g, 0.05)).dt
"#);
    for name in ["z", "t"] {
        close_all(&nums(&it, &format!("{name}_num")), &[1.0, 3.0], 1e-10);
        close_all(&nums(&it, &format!("{name}_den")), &[1.0, 2.0, 5.0], 1e-10);
    }
    assert_eq!(num(&it, "back_dt"), 0.0);
    assert!(err("c = lti_d2c(lti_tf([1], [1, 0.5], 0.1))").contains("no real logarithm"));
    assert!(err("c = lti_d2c(lti_tf([1], [1, 1], 0.1), \"tustin\")").contains("z = -1"));
    assert!(err("c = lti_d2c(lti_tf([1], [1, 0.5], 0.1), \"euler\")").contains("only \"zoh\" and \"tustin\""));
    assert!(err("c = lti_d2c(lti_tf([1], [1, 1]))").contains("already continuous"));
}

// --------------------------------------------------------- bode and margins

#[test]
fn bode_returns_db_and_unwrapped_degrees() {
    let it = run(r#"
L = lti_tf([1], [1, 3, 2, 0])
one = lti_bode(L, [1])
mag1 = one.mag_db
ph1 = one.phase_deg
r = lti_bode(L)
w = r.w
mag = r.mag_db
ph = r.phase_deg
triple = lti_bode(lti_tf([1], [1, 0, 0, 0]), [0.5, 2]).phase_deg
dw = lti_bode(lti_tf([1], [1, -0.5], 1)).w
"#);
    // |L(j)| = 1/sqrt(10); phase = -90 - 45 - atan(1/2)
    close(num(&it, "mag1"), -10.0, 1e-12);
    close(num(&it, "ph1"), -135.0 - 0.5f64.atan().to_degrees(), 1e-12);
    let (w, mag, ph) = (nums(&it, "w"), nums(&it, "mag"), nums(&it, "ph"));
    assert!(w.len() >= 200 && mag.len() == w.len() && ph.len() == w.len());
    assert!(w[0] <= 0.1 && *w.last().unwrap() >= 20.0);
    for k in 0..w.len() {
        let wk = w[k];
        close(ph[k], -90.0 - wk.atan().to_degrees() - (wk / 2.0).atan().to_degrees(), 1e-9);
        close(mag[k], -20.0 * (wk * (wk * wk + 1.0).sqrt() * (wk * wk + 4.0).sqrt()).log10(), 1e-9);
    }
    // past -180 the phase keeps falling instead of wrapping to +180
    assert!(*ph.last().unwrap() < -260.0);
    assert_eq!(nums(&it, "triple").len(), 2);
    close_all(&nums(&it, "triple"), &[-270.0, -270.0], 1e-12);
    close(*nums(&it, "dw").last().unwrap(), PI, 1e-12);
    assert!(err("b = lti_bode(lti_ss(-1, [1, 2], [3; 4], 0))").contains("one input and one output"));
}

#[test]
fn margins_of_the_textbook_loop() {
    // L(s) = 1/(s(s+1)(s+2)): gain margin 6 (15.56 dB) at w = sqrt(2)
    let it = run(r#"
L = lti_tf([1], [1, 3, 2, 0])
m = lti_margin(L)
gm = m.gm
gm_db = m.gm_db
w_gm = m.w_gm
pm = m.pm
w_pm = m.w_pm
stable = m.stable
z = lti_margin(lti_zpk([], [0, -1, -2], 1))
z_gm = z.gm
s = lti_margin(lti_to_ss(L))
s_gm = s.gm
s_w = s.w_gm
hot = lti_margin(lti_series(7, L))
hot_gm = hot.gm
hot_stable = hot.stable
lag = lti_margin(lti_tf([0.5], [1, 1]))
lag_gm = lag.gm
lag_pm = lag.pm
lag_w = lag.w_gm
"#);
    close(num(&it, "gm"), 6.0, 1e-12);
    close(num(&it, "gm_db"), 15.56, 0.005);
    close(num(&it, "gm_db"), 20.0 * 6f64.log10(), 1e-11);
    close(num(&it, "w_gm"), 2f64.sqrt(), 1e-13);
    // at the gain crossover |L| = 1: w^2 (w^2+1)(w^2+4) = 1, and the margin is 180 + phase
    let w = num(&it, "w_pm");
    close(w * w * (w * w + 1.0) * (w * w + 4.0), 1.0, 1e-12);
    close(num(&it, "pm"), 90.0 - w.atan().to_degrees() - (w / 2.0).atan().to_degrees(), 1e-10);
    close(num(&it, "pm"), 53.4, 0.05);
    assert!(flag(&it, "stable"));
    close(num(&it, "z_gm"), 6.0, 1e-9);
    close(num(&it, "s_gm"), 6.0, 1e-9);
    close(num(&it, "s_w"), 2f64.sqrt(), 1e-9);
    // seven times the gain is past the margin
    close(num(&it, "hot_gm"), 6.0 / 7.0, 1e-9);
    assert!(!flag(&it, "hot_stable"));
    // a lag with gain 0.5 has neither crossing
    assert!(num(&it, "lag_gm").is_infinite() && num(&it, "lag_pm").is_infinite() && num(&it, "lag_w").is_nan());
    assert!(err("m = lti_margin(lti_ss(-1, [1, 2], [3; 4], 0))").contains("one input and one output"));
}

#[test]
fn discrete_margins() {
    // L(z) = 0.4/(z(z-1)): phase -180 at w = pi/3 where |L| = 0.4
    let it = run("m = lti_margin(lti_tf([0.4], [1, -1, 0], 1))\ngm = m.gm\nw = m.w_gm\nwp = m.w_pm\nok = m.stable\n");
    close(num(&it, "gm"), 2.5, 1e-9);
    close(num(&it, "w"), PI / 3.0, 1e-9);
    close(num(&it, "wp"), 2.0 * 0.2f64.asin(), 1e-9);
    assert!(flag(&it, "ok"));
}

// ------------------------------------------------------------ filter models

#[test]
fn a_filter_model_is_a_discrete_system() {
    let it = run(r#"
f = butter(2, "low", 100, 1000)
p = lti_poles(f)
refp = zpk(f).p
dc = lti_dcgain(f)
ok = lti_is_stable(f)
dt = lti_to_ss(f).dt
h = abs(lti_freqresp(f, 2 * pi * 100 / 1000))
fir = lti_dcgain(fir1(10, 0.2))
"#);
    close_c(&cplx(&it, "p"), &cplx(&it, "refp"), 1e-12);
    assert_eq!(cplx(&it, "p").len(), 2);
    close(num(&it, "dc"), 1.0, 1e-12);
    assert!(flag(&it, "ok"));
    close(num(&it, "dt"), 0.001, 1e-18);
    // -3 dB at the cutoff, with the frequency in rad/sample
    close(num(&it, "h"), 0.5f64.sqrt(), 1e-12);
    close(num(&it, "fir"), 1.0, 1e-12);
}

// ------------------------------------------------------------ housekeeping

#[test]
fn unknown_keywords_and_extra_arguments_are_refused() {
    let g = "g = lti_tf([1], [1, 1])\n";
    assert!(err(&format!("{g}r = lti_step(g, 1, points=10)")).contains("points"));
    assert!(err(&format!("{g}r = lti_poles(g, 1)")).contains("lti_poles"));
}

#[test]
fn help_describes_every_lti_builtin() {
    let names = [
        "lti_bode", "lti_c2d", "lti_d2c", "lti_dcgain", "lti_feedback", "lti_freqresp", "lti_impulse", "lti_is_stable", "lti_lsim",
        "lti_margin", "lti_parallel", "lti_poles", "lti_series", "lti_ss", "lti_step", "lti_tf", "lti_to_ss", "lti_to_tf",
        "lti_to_zpk", "lti_zeros", "lti_zpk",
    ];
    for name in names {
        let doc = qu_interp::builtin_docs::BUILTIN_DOCS
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no documentation row for `{name}`"));
        assert!(doc.signature.starts_with(&format!("{name}(")), "{name}: {}", doc.signature);
        assert!(doc.summary.starts_with("Returns"), "{name}: {}", doc.summary);
        assert_eq!(doc.chapter, "signal-processing");
    }
}
