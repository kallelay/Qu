//! Parameter uncertainty on least-squares fits (`curve_fit`,
//! `least_squares`): stderr / t / p / CI / bootstrap / `summary`, and the
//! `method=` solvers, through the language.

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

const LINE: &str = "
function line(x, p)
    return p[0] + p[1] * x
end function
x = [1, 2, 3, 4, 5, 6, 7, 8]
y = [2.1, 3.9, 6.2, 7.8, 10.1, 12.2, 13.8, 16.1]
";

/// A straight line fit by `curve_fit` IS ordinary least squares, so every
/// statistic must equal the textbook closed form -- computed here in Rust,
/// independently of the fitting code.
#[test]
fn a_line_reproduces_the_ols_table_exactly() {
    let it = run(&format!("{LINE}\nfit = curve_fit(\"line\", x, y, [0, 1])\nse = fit.stderr\nci = fit.ci\npv = fit.p_values\ndof = fit.dof\nr2 = fit.r_squared"));
    let x: Vec<f64> = (1..=8).map(|v| v as f64).collect();
    let y = [2.1, 3.9, 6.2, 7.8, 10.1, 12.2, 13.8, 16.1];
    let n = 8.0;
    let xb = x.iter().sum::<f64>() / n;
    let yb = y.iter().sum::<f64>() / n;
    let sxx: f64 = x.iter().map(|v| (v - xb).powi(2)).sum();
    let b1 = x.iter().zip(&y).map(|(a, b)| (a - xb) * (b - yb)).sum::<f64>() / sxx;
    let b0 = yb - b1 * xb;
    let rss: f64 = x.iter().zip(&y).map(|(a, b)| (b - b0 - b1 * a).powi(2)).sum();
    let s2 = rss / (n - 2.0);
    let se = [(s2 * (1.0 / n + xb * xb / sxx)).sqrt(), (s2 / sxx).sqrt()];
    let got = nums(&it, "se");
    for j in 0..2 {
        assert!((got[j] - se[j]).abs() < 1e-8 * se[j], "stderr[{j}] {} vs {}", got[j], se[j]);
    }
    // t(0.975, 6) = 2.446911851144969
    let tq = 2.446911851144969;
    let ci = nums(&it, "ci");
    assert!((ci[0] - (b0 - tq * se[0])).abs() < 1e-7 && (ci[3] - (b1 + tq * se[1])).abs() < 1e-7, "{ci:?}");
    assert_eq!(num(&it, "dof"), 6.0);
    let pv = nums(&it, "pv");
    assert!(pv[0] > 0.8 && pv[0] < 0.81, "intercept is not significant: {pv:?}");
    assert!(pv[1] < 1e-8, "slope is: {pv:?}");
    let tss: f64 = y.iter().map(|v| (v - yb).powi(2)).sum();
    assert!((num(&it, "r2") - (1.0 - rss / tss)).abs() < 1e-12);
}

#[test]
fn the_three_methods_agree_and_are_named() {
    let it = run(&format!(
        "{LINE}\na = curve_fit(\"line\", x, y, [0, 1])\nb = curve_fit(\"line\", x, y, [0, 1], method=\"gauss_newton\")\nc = curve_fit(\"line\", x, y, [0, 1], method=\"gradient_descent\", max_iter=50000)\npa = a.params\npb = b.params\npc = c.params\nm = b.method"
    ));
    let (a, b, c) = (nums(&it, "pa"), nums(&it, "pb"), nums(&it, "pc"));
    for j in 0..2 {
        assert!((a[j] - b[j]).abs() < 1e-8, "gauss_newton {b:?} vs lm {a:?}");
        assert!((a[j] - c[j]).abs() < 1e-4, "gradient_descent {c:?} vs lm {a:?}");
    }
    assert!(matches!(it.get("m"), Some(Value::Str(s)) if s == "gauss_newton"));
    assert!(err(&format!("{LINE}\nf = curve_fit(\"line\", x, y, [0, 1], method=\"newton\")")).contains("gauss_newton"));
}

#[test]
fn bootstrap_is_reproducible_and_sane() {
    let src = format!("{LINE}\nb = curve_fit(\"line\", x, y, [0, 1], bootstrap=200, seed=7)\nbs = b.bootstrap_stderr\nse = b.stderr\nbci = b.bootstrap_ci");
    let (i1, i2) = (run(&src), run(&src));
    assert_eq!(nums(&i1, "bs"), nums(&i2, "bs"), "same seed, same resamples");
    let (bs, se) = (nums(&i1, "bs"), nums(&i1, "se"));
    for j in 0..2 {
        assert!(bs[j] > se[j] / 3.0 && bs[j] < se[j] * 3.0, "bootstrap {bs:?} vs linearized {se:?}");
    }
    assert_eq!(nums(&i1, "bci").len(), 4);
    assert!(err(&format!("{LINE}\nb = curve_fit(\"line\", x, y, [0, 1], bootstrap=2.5)")).contains("whole number"));
}

#[test]
fn no_degrees_of_freedom_means_nan_and_a_note_not_a_crash() {
    let it = run("function line(x, p)\n  return p[0] + p[1] * x\nend function\nf = curve_fit(\"line\", [1, 2], [3, 5], [0, 1])\nse = f.stderr\nnote = f.note");
    assert!(nums(&it, "se").iter().all(|v| v.is_nan()), "{:?}", nums(&it, "se"));
    assert!(matches!(it.get("note"), Some(Value::Str(s)) if s.contains("dof")), "{:?}", it.get("note"));
}

#[test]
fn an_unidentifiable_parameter_is_flagged() {
    // p[0] and p[1] only ever appear as their sum.
    let it = run("function m(x, p)\n  return p[0] + p[1] + p[2] * x\nend function\nx = [1, 2, 3, 4, 5]\ny = [3, 5.1, 6.9, 9.2, 11]\nf = curve_fit(\"m\", x, y, [1, 1, 1])\nnote = f.note");
    assert!(matches!(it.get("note"), Some(Value::Str(s)) if s.contains("identifiable")), "{:?}", it.get("note"));
}

#[test]
fn least_squares_flags_a_parameter_on_its_bound() {
    let it = run(
        "function r(p)\n  return [p[0] - 5, p[1] - 1, p[0] + p[1] - 6.2]\nend function\nf = least_squares(\"r\", [1, 0.5], upper=[3, 10])\nab = f.at_bound\nse = f.stderr",
    );
    let ab = match it.get("ab") {
        Some(Value::Mask(m)) => m.clone(),
        Some(Value::Vec(v)) => v.iter().map(|x| *x != 0.0).collect(),
        Some(Value::List(l)) => l.iter().map(|v| matches!(v, Value::Bool(true))).collect(),
        other => panic!("at_bound is {other:?}"),
    };
    assert_eq!(ab, vec![true, false], "p[0] is pinned at upper=3");
}

#[test]
fn summary_prints_the_table_and_refuses_other_models() {
    let it = run(&format!("{LINE}\ns = summary(curve_fit(\"line\", x, y, [0, 1]))"));
    let s = match it.get("s") {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("{other:?}"),
    };
    assert!(s.contains("estimate") && s.contains("P>|t|") && s.contains("dof = 6"), "{s}");
    assert!(err("m = kmeans_model([1, 2, 10, 11] as matrix(4, 1), 2)\nsummary(m)").contains("summary"));
}
