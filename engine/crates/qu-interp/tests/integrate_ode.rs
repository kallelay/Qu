//! Numerical integration and ODE builtins through the language:
//! `trapz`, `cumtrapz`, `simpson`, `quad`, `quad_info`, `ode45`, `ode23`,
//! `ode_stiff`, `rk4`. Every expected value is an analytic solution.

use qu_interp::{Interp, Value};

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
    nums(it, n)[0]
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.to_string(),
        other => panic!("`{n}` is {other:?}"),
    }
}

// ------------------------------------------------------------ sampled data

#[test]
fn trapz_unit_spacing_and_with_x() {
    let it = run("a = trapz([1, 2, 3, 4])\nb = trapz([0, 1, 3], [1, 2, 3])\nc = trapz([5])");
    assert_eq!(num(&it, "a"), 7.5);
    assert_eq!(num(&it, "b"), 1.5 + 2.0 * 2.5);
    assert_eq!(num(&it, "c"), 0.0);
}

#[test]
fn trapz_of_a_matrix_integrates_each_column_along_dim_1() {
    // rows are samples: column 0 = x, column 1 = 2x, over x = 0, 1, 2, 3
    let it = run("M = [0, 0; 1, 2; 2, 4; 3, 6]\nr = trapz(M)\nq = trapz([0, 1, 2, 3], M)");
    assert_eq!(nums(&it, "r"), vec![4.5, 9.0]);
    assert_eq!(nums(&it, "q"), vec![4.5, 9.0]);
}

#[test]
fn cumtrapz_matches_the_antiderivative_and_keeps_matrix_shape() {
    let it = run(
        "x = linspace(0, 2, 201)\nc = cumtrapz(x, x .* x)\nlast = c[200]\nmid = c[100]\n\
         M = [1, 2; 1, 2; 1, 2]\nm = cumtrapz(M)\nsz = size(m)",
    );
    assert!((num(&it, "last") - 8.0 / 3.0).abs() < 1e-3);
    assert!((num(&it, "mid") - 1.0 / 3.0).abs() < 1e-4);
    assert_eq!(nums(&it, "sz"), vec![3.0, 2.0]);
    assert_eq!(nums(&it, "m"), vec![0.0, 1.0, 2.0, 0.0, 2.0, 4.0]);
}

#[test]
fn simpson_is_exact_on_a_cubic_and_beats_trapz_on_sin() {
    let it = run(
        "x = linspace(0, 2, 9)\ny = x .^ 3\ns = simpson(x, y)\n\
         xs = linspace(0, pi, 21)\nts = trapz(xs, sin(xs))\nss = simpson(xs, sin(xs))\n\
         odd = simpson(linspace(0, 3, 8), linspace(0, 3, 8) .^ 2)",
    );
    assert!((num(&it, "s") - 4.0).abs() < 1e-13);
    let (et, es) = ((num(&it, "ts") - 2.0).abs(), (num(&it, "ss") - 2.0).abs());
    assert!(es < et / 100.0, "trapz err {et}, simpson err {es}");
    // 7 intervals: still exact for a quadratic
    assert!((num(&it, "odd") - 9.0).abs() < 1e-12);
}

#[test]
fn sampled_integrals_validate_their_arguments() {
    assert!(err("trapz([1, 2, 3], [1, 2])").contains("trapz: x has 3 elements but y has 2"));
    assert!(err("simpson([0, 1, 1], [1, 2, 3])").contains("simpson: repeated x"));
    assert!(err("cumtrapz()").contains("cumtrapz"));
}

// -------------------------------------------------------------------- quad

const FNS: &str = "
function recip_sqrt(x)
    return 1 / sqrt(x)
end function
function gauss(x)
    return exp(-x * x)
end function
function osc(x)
    return x * cos(20 * x)
end function
function bad(x)
    return 1 / x
end function
";

#[test]
fn quad_closed_forms() {
    let it = run(&format!(
        "{FNS}\na = quad(\"gauss\", -inf, inf)\nb = quad(\"gauss\", 0, inf)\nc = quad(\"osc\", 0, 3, 1e-11)\n\
         d = quad(\"recip_sqrt\", 0, 1, 1e-9)\ne1 = quad((x) := sin(x), 0, pi)\nf1 = quad((x) := 1 / (1 + x * x), -inf, inf)\n\
         g = quad(\"gauss\", 3, 0)"
    ));
    let pi = std::f64::consts::PI;
    assert!((num(&it, "a") - pi.sqrt()).abs() < 1e-9);
    assert!((num(&it, "b") - pi.sqrt() / 2.0).abs() < 1e-9);
    let want_c = (3.0 * 60.0f64.sin()) / 20.0 + (60.0f64.cos() - 1.0) / 400.0;
    assert!((num(&it, "c") - want_c).abs() < 1e-9);
    assert!((num(&it, "d") - 2.0).abs() < 1e-8, "endpoint singularity: {}", num(&it, "d"));
    assert!((num(&it, "f1") - pi).abs() < 1e-9);
    assert!((num(&it, "g") + 0.886226925452758 * 0.9999779095030014).abs() < 1e-9);
    assert!(num(&it, "g") < 0.0, "reversed limits give a negative value");
}

#[test]
fn quad_info_returns_the_error_estimate_and_effort() {
    let it = run(&format!(
        "{FNS}\nr = quad_info(\"gauss\", -3, 3)\nv = r.value\ne = r.error\nn = r.nfev\nk = r.intervals\ns = r.status"
    ));
    // erf(3) * sqrt(pi)
    assert!((num(&it, "v") - 1.7724538509055159 * 0.9999779095030014).abs() < 1e-10);
    assert!(num(&it, "e") >= 0.0 && num(&it, "e") < 1e-9);
    assert!(num(&it, "n") >= 15.0 && num(&it, "k") >= 1.0);
    assert_eq!(text(&it, "s"), "ok");
}

#[test]
fn quad_refuses_to_return_an_unconverged_value_but_quad_info_reports_it() {
    let m = err(&format!("{FNS}\nx = quad(\"recip_sqrt\", 0, 1, 1e-12, 3)"));
    assert!(m.contains("quad: did not reach tol") && m.contains("max_depth"), "{m}");
    let it = run(&format!("{FNS}\nr = quad_info(\"recip_sqrt\", 0, 1, 1e-12, 3)\ns = r.status\nv = r.value"));
    assert_eq!(text(&it, "s"), "max_depth");
    assert!((num(&it, "v") - 2.0).abs() < 0.1);
}

#[test]
fn quad_argument_errors_name_the_function() {
    assert!(err(&format!("{FNS}\nquad(\"gauss\", 0)")).contains("quad("));
    assert!(err("quad(\"nope\", 0, 1)").contains("quad: no user function named `nope`"));
    assert!(err(&format!("{FNS}\nquad(\"gauss\", 0, 1, -1)")).contains("quad: tol must be positive"));
    assert!(err(&format!("{FNS}\nquad(\"bad\", -1, 1)")).contains("finite"));
}

// --------------------------------------------------------------------- ODE

const ODES: &str = "
function decay(t, y)
    return -2 * y
end function
function sho(t, y)
    return [y[1], -y[0]]
end function
function stiff(t, y)
    return -10000 * (y - cos(t))
end function
function rober(t, y)
    return [-0.04 * y[0] + 10000 * y[1] * y[2], 0.04 * y[0] - 10000 * y[1] * y[2] - 30000000 * y[1] * y[1], 30000000 * y[1] * y[1]]
end function
function ground(t, y)
    return y[0]
end function
function fall(t, y)
    return [y[1], -9.81]
end function
function wrong(t, y)
    return [1, 2, 3]
end function
";

#[test]
fn ode45_scalar_decay_and_record_shape() {
    let it = run(&format!(
        "{ODES}\nr = ode45(\"decay\", [0, 2], 1.5, rtol=1e-9, atol=1e-12)\nT = r.t\nY = r.y\nst = r.status\nsteps = r.steps\nnf = r.nfev\n\
         nt = length(T)\nsz = size(Y)"
    ));
    let t = nums(&it, "T");
    let y = nums(&it, "Y");
    assert_eq!(t[0], 0.0);
    assert_eq!(*t.last().unwrap(), 2.0);
    for (ti, yi) in t.iter().zip(&y) {
        assert!((yi - 1.5 * (-2.0 * ti).exp()).abs() < 1e-7, "t={ti}");
    }
    assert_eq!(nums(&it, "sz"), vec![t.len() as f64, 1.0]);
    assert_eq!(text(&it, "st"), "ok");
    assert_eq!(num(&it, "steps") + 1.0, t.len() as f64);
    assert!(num(&it, "nf") > num(&it, "steps"));
}

#[test]
fn ode45_harmonic_oscillator_ten_periods_with_t_eval() {
    let it = run(&format!(
        "{ODES}\nte = linspace(0, 62.83185307179586, 201)\nr = ode45(\"sho\", [0, 62.83185307179586], [1, 0], 1e-9, 1e-12, t_eval=te)\n\
         Y = r.y\nT = r.t\nsz = size(Y)"
    ));
    assert_eq!(nums(&it, "sz"), vec![201.0, 2.0]);
    let y = nums(&it, "Y"); // column-major: [x..., v...]
    let t = nums(&it, "T");
    let mut worst_energy = 0.0f64;
    for i in 0..201 {
        let (x, v) = (y[i], y[201 + i]);
        worst_energy = worst_energy.max((0.5 * (x * x + v * v) - 0.5).abs());
        assert!((x - t[i].cos()).abs() < 1e-5 && (v + t[i].sin()).abs() < 1e-5, "i={i}");
    }
    assert!(worst_energy < 1e-6, "energy drift {worst_energy}");
}

#[test]
fn ode_accepts_a_lambda_and_a_longer_tspan_as_output_times() {
    let it = run("r = ode45((t, y) := -y, [0, 0.5, 1, 2], 1)\nT = r.t\nY = r.y");
    assert_eq!(nums(&it, "T"), vec![0.0, 0.5, 1.0, 2.0]);
    let y = nums(&it, "Y");
    for (i, t) in [0.0f64, 0.5, 1.0, 2.0].iter().enumerate() {
        assert!((y[i] - (-t).exp()).abs() < 1e-5);
    }
}

#[test]
fn ode23_and_rk4_against_the_exponential() {
    let it = run(&format!(
        "{ODES}\na = ode23(\"decay\", [0, 1], 1, 1e-8, 1e-11)\nya = a.y\nb = rk4(\"decay\", [0, 1], 1, 0.01)\nyb = b.y\nsb = b.steps\ntb = b.t"
    ));
    let want = (-2.0f64).exp();
    let ya = nums(&it, "ya");
    assert!((ya.last().unwrap() - want).abs() < 1e-6);
    let yb = nums(&it, "yb");
    assert!((yb.last().unwrap() - want).abs() < 1e-9);
    assert_eq!(num(&it, "sb"), 100.0);
    assert_eq!(*nums(&it, "tb").last().unwrap(), 1.0);
}

#[test]
fn ode_stiff_solves_a_stiff_problem_in_far_fewer_evaluations_than_ode45() {
    let it = run(&format!(
        "{ODES}\ns = ode_stiff(\"stiff\", [0, 10], 0)\nys = s.y\nns = s.nfev\ne = ode45(\"stiff\", [0, 10], 0)\nne = e.nfev\nye = e.y"
    ));
    let t: f64 = 10.0;
    let want = (1e8 * t.cos() + 1e4 * t.sin()) / (1e8 + 1.0);
    assert!((nums(&it, "ys").last().unwrap() - want).abs() < 1e-4);
    assert!((nums(&it, "ye").last().unwrap() - want).abs() < 1e-4);
    assert!(num(&it, "ns") * 5.0 < num(&it, "ne"), "stiff {} vs explicit {}", num(&it, "ns"), num(&it, "ne"));
}

#[test]
fn ode_stiff_robertson_matches_the_published_values() {
    let it = run(&format!("{ODES}\nr = ode_stiff(\"rober\", [0, 40], [1, 0, 0], 1e-7, 1e-11)\nY = r.y\nn = length(r.t)"));
    let y = nums(&it, "Y");
    let n = num(&it, "n") as usize;
    // Hairer & Wanner, y(40)
    assert!((y[n - 1] - 0.7158270687).abs() < 1e-4);
    assert!((y[2 * n - 1] - 9.185534764e-6).abs() < 5e-8);
    assert!((y[3 * n - 1] - 0.2841637457).abs() < 1e-4);
}

#[test]
fn a_terminal_event_stops_at_the_zero_crossing() {
    // ball thrown up at 10 m/s from 1 mm: lands at (v + sqrt(v^2 + 2 g h)) / g
    let it = run(&format!(
        "{ODES}\nr = ode45(\"fall\", [0, 10], [0.001, 10], events=\"ground\", terminal=true, rtol=1e-10, atol=1e-12)\n\
         te = r.t_events\nT = r.t\ns = r.status\nye = r.y_events"
    ));
    let want = (10.0 + (100.0f64 + 2.0 * 9.81 * 1e-3).sqrt()) / 9.81;
    assert!((num(&it, "te") - want).abs() < 1e-8);
    assert_eq!(text(&it, "s"), "event");
    assert_eq!(*nums(&it, "T").last().unwrap(), num(&it, "te"));
    assert!(nums(&it, "ye")[0].abs() < 1e-8);
}

#[test]
fn a_non_terminal_event_records_every_crossing_and_continues() {
    let it = run(&format!(
        "{ODES}\nr = ode45(\"sho\", [0, 10], [1, 0], events=\"ground\")\nte = r.t_events\ns = r.status\nT = r.t"
    ));
    let te = nums(&it, "te");
    assert_eq!(text(&it, "s"), "ok");
    assert_eq!(te.len(), 3); // cos t = 0 at pi/2, 3pi/2, 5pi/2
    for (k, t) in te.iter().enumerate() {
        assert!((t - (k as f64 + 0.5) * std::f64::consts::PI).abs() < 1e-6);
    }
    assert_eq!(*nums(&it, "T").last().unwrap(), 10.0);
}

#[test]
fn ode_errors_name_the_function_and_the_problem() {
    assert!(err("ode45(\"nope\", [0, 1], 1)").contains("ode45: no user function named `nope`"));
    assert!(err(&format!("{ODES}\node45(\"decay\", [1, 1], 1)")).contains("nothing to integrate"));
    assert!(err(&format!("{ODES}\node45(\"decay\", [0], 1)")).contains("ode45: tspan"));
    assert!(err(&format!("{ODES}\node23(\"decay\", [0, 1], 1, rtol=-1)")).contains("ode23: rtol must be positive"));
    assert!(err(&format!("{ODES}\node45(\"wrong\", [0, 1], [1, 2])")).contains("returned 3 value(s) but y has 2"));
    assert!(err(&format!("{ODES}\node45(\"decay\", [0, 1], 1, t_eval=[0, 2])")).contains("outside the span"));
    assert!(err(&format!("{ODES}\nrk4(\"decay\", [0, 1], 1)")).contains("rk4(f, tspan, y0, h)"));
    assert!(err(&format!("{ODES}\nrk4(\"decay\", [0, 1], 1, 0)")).contains("rk4: h must be positive"));
    assert!(err(&format!("{ODES}\node_stiff(\"decay\", [0, 1], [1, nan])")).contains("ode_stiff: y0 must be"));
    // rk4 has no error control, so an adaptive-only keyword is an error, not silence
    assert!(err(&format!("{ODES}\nrk4(\"decay\", [0, 1], 1, 0.1, rtol=1e-3)")).contains("rtol"));
}
