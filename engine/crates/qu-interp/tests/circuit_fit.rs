//! § circuit fitting and model selection (2026-09-24).
//!
//! The test this file exists for is the FIRST one: data generated from a
//! circuit whose parameters are written down here must come back out of
//! `circuit_fit` as those parameters. Everything else -- the protocol, the
//! error bars, the model selection -- is only worth testing if that holds.
//!
//! The forward model is `circuit_impedance`, which is checked against an
//! independent Python implementation across 40 topologies in
//! `papers/ecm-pf`. So "generate, fit, compare" here is not circular in the
//! usual way: the thing under test is the inverse map, and the forward map
//! it is inverted against has an outside witness.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

fn str_of(it: &Interp, name: &str) -> String {
    match it.get(name) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{name}` is {other:?}, expected a string"),
    }
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    format!("{}", it.run(src).unwrap_err())
}

/// Relative error, as a fraction.
fn rel(got: f64, want: f64) -> f64 {
    (got / want - 1.0).abs()
}

// --------------------------------------------------- ground truth

/// A Randles cell WITH diffusion: `Rs - (Rct - W | Cdl)`, four parameters
/// spanning seven decades. Clean data, and a fit started from nothing but
/// the spectrum's own shape.
#[test]
fn recovers_a_randles_cell_with_diffusion_from_clean_data() {
    let it = run(
        r#"
f = logspace(-1, 5, 61)
z = circuit_impedance("R-p(R-W,C)", f, [10, 100, 50, 1e-5])
m = circuit_fit(f, z, "R-p(R-W,C)")
rs = m.R1
rct = m.R2
aw = m.W1
cdl = m.C1
chi2 = m.chi2
spec = m.spec
"#,
    );
    assert_eq!(str_of(&it, "spec"), "R-p(R-W,C)");
    for (name, want) in [
        ("rs", 10.0),
        ("rct", 100.0),
        ("aw", 50.0),
        ("cdl", 1e-5),
    ] {
        let got = num_of(&it, name);
        assert!(
            rel(got, want) < 1e-4,
            "{name}: recovered {got}, truth {want} ({:.3}% off)",
            rel(got, want) * 100.0
        );
    }
    assert!(num_of(&it, "chi2") < 1e-12, "chi2 = {}", num_of(&it, "chi2"));
}

/// The same recovery with ~1% noise on both components, which is what a
/// real instrument delivers. The noise is a fixed deterministic pattern,
/// not `randn`: this test asserts numbers, so it cannot depend on a
/// generator whose stream is not guaranteed to be identical everywhere.
#[test]
fn recovers_a_randles_cell_from_noisy_data() {
    let it = run(
        r#"
n = 61
f = logspace(-1, 5, n)
z = circuit_impedance("R-p(R-W,C)", f, [10, 100, 50, 1e-5])
u1 = sin((1 to n) .* 12.9898)
u2 = cos((1 to n) .* 7.233)
zn = z .* (1 + 0.01 .* u1) + 1i .* (0.01 .* u2 .* abs(z))
m = circuit_fit(f, zn, "R-p(R-W,C)")
rs = m.R1
rct = m.R2
aw = m.W1
cdl = m.C1
chi2_red = m.chi2_red
se_rs = m.stderr[0]
"#,
    );
    for (name, want, tol) in [
        ("rs", 10.0, 0.05),
        ("rct", 100.0, 0.05),
        ("aw", 50.0, 0.10),
        ("cdl", 1e-5, 0.05),
    ] {
        let got = num_of(&it, name);
        assert!(
            rel(got, want) < tol,
            "{name}: recovered {got}, truth {want} ({:.2}% off, budget {:.0}%)",
            rel(got, want) * 100.0,
            tol * 100.0
        );
    }
    // With modulus weighting and ~1% noise, the reduced chi-squared is a
    // squared relative error: ~1e-4, not ~1.
    let c = num_of(&it, "chi2_red");
    assert!(
        c > 1e-6 && c < 1e-2,
        "chi2_red = {c}, which is not the ~1e-4 that 1% noise implies"
    );
    // A well-determined parameter gets a finite, small error bar.
    let se = num_of(&it, "se_rs");
    assert!(
        se.is_finite() && se > 0.0 && se < 1.0,
        "the series resistance is the best-determined parameter here; stderr = {se}"
    );
}

/// The exponent role: a CPE's `n` is not fitted in log space, and it is the
/// parameter a fit is most likely to park on a bound.
#[test]
fn recovers_a_cpe_exponent() {
    let it = run(
        r#"
f = logspace(-1, 5, 61)
z = circuit_impedance("R-p(R,Q)", f, [5, 50, 2e-5, 0.82])
m = circuit_fit(f, z, "R-p(R,Q)")
rs = m.R1
rct = m.R2
q = m.Q1_q
nq = m.Q1_n
"#,
    );
    for (name, want) in [("rs", 5.0), ("rct", 50.0), ("q", 2e-5), ("nq", 0.82)] {
        let got = num_of(&it, name);
        assert!(
            rel(got, want) < 1e-3,
            "{name}: recovered {got}, truth {want}"
        );
    }
}

/// A circuit VALUE carries its own numbers, and when one is passed they are
/// the starting point -- the spelling that lets a caller hand over a guess
/// without a separate `initial_guess=`.
#[test]
fn a_circuit_value_supplies_both_topology_and_starting_point() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
guess = circuit("R-p(R,C)", [1, 1000, 1e-4])
m = circuit_fit(f, z, guess, n_starts = 1)
rs = m.R1
rct = m.R2
cdl = m.C1
"#,
    );
    for (name, want) in [("rs", 10.0), ("rct", 100.0), ("cdl", 1e-5)] {
        assert!(
            rel(num_of(&it, name), want) < 1e-4,
            "{name}: recovered {}, truth {want}",
            num_of(&it, name)
        );
    }
}

// --------------------------------------------------- the protocol

#[test]
fn predict_evaluates_the_fitted_circuit_on_a_new_axis() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = circuit_fit(f, z, "R-p(R,C)")
# A DIFFERENT axis, half of it outside the fitted range.
g = logspace(-2, 6, 17)
zp = m.predict(g)
truth = circuit_impedance("R-p(R,C)", g, [10, 100, 1e-5])
worst = max(abs(zp - truth) ./ abs(truth))
n_out = len(zp)
"#,
    );
    assert_eq!(num_of(&it, "n_out"), 17.0);
    assert!(
        num_of(&it, "worst") < 1e-6,
        "predict disagrees with the forward model by {}",
        num_of(&it, "worst")
    );
}

/// `score` is reduced chi-squared, so LOWER is better -- the opposite of the
/// regression models' R^2 from the same method name. The seam is asserted
/// here so it cannot be quietly "fixed" into an R^2 later.
#[test]
fn score_is_a_reduced_chi_squared_where_lower_is_better() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
right = circuit_fit(f, z, "R-p(R,C)")
wrong = circuit_fit(f, z, "R-C")
s_right = right.score(f, z)
s_wrong = wrong.score(f, z)
"#,
    );
    let (good, bad) = (num_of(&it, "s_right"), num_of(&it, "s_wrong"));
    assert!(good < 1e-12, "a correct fit's score should be ~0, got {good}");
    assert!(
        bad > good * 1e3 + 1e-6,
        "the wrong topology should score WORSE (higher): {bad} vs {good}"
    );
}

#[test]
fn every_fitted_parameter_is_also_a_named_field() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,Q)", f, [5, 50, 2e-5, 0.82])
m = circuit_fit(f, z, "R-p(R,Q)")
by_field = [m.R1, m.R2, m.Q1_q, m.Q1_n]
gap = max(abs(by_field - m.params))
n_names = len(m.param_names)
"#,
    );
    assert_eq!(num_of(&it, "gap"), 0.0);
    assert_eq!(num_of(&it, "n_names"), 4.0);
}

// --------------------------------------------------- refusals

/// Frequencies first, impedance second. Handed over the other way round --
/// which is `rlkk_extrapolate`'s order, documented two sections earlier in
/// the same chapter -- this must refuse, not fit the frequency axis.
#[test]
fn the_reversed_argument_order_is_refused_not_fitted() {
    let msg = err(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = circuit_fit(z, f, "R-p(R,C)")
"#,
    );
    assert!(
        msg.contains("FREQUENCIES come first"),
        "got: {msg}"
    );
}

#[test]
fn an_initial_guess_of_the_wrong_length_names_the_parameters() {
    let msg = err(
        r#"
f = logspace(-1, 5, 21)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = circuit_fit(f, z, "R-p(R,C)", initial_guess = [1, 2])
"#,
    );
    assert!(msg.contains("R1, R2, C1"), "got: {msg}");
}

#[test]
fn an_unknown_weighting_is_refused() {
    let msg = err(
        r#"
f = logspace(-1, 5, 21)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = circuit_fit(f, z, "R-p(R,C)", weight = "proportional")
"#,
    );
    assert!(msg.contains("weight="), "got: {msg}");
}

// --------------------------------------------------- model selection

/// The point of ranking by an information criterion rather than by fit
/// quality: the candidate ladder is NESTED, so `R-p(R,Q)` can always reach
/// `R-p(R,C)`'s residual (set `n = 1`) and the two-arc candidates can always
/// reach the one-arc ones. Ranked by chi-squared, the largest candidate
/// wins every time, whatever generated the data.
#[test]
fn sysid_picks_the_generating_topology_not_the_largest_one() {
    let it = run(
        r#"
n = 61
f = logspace(-1, 5, n)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
u1 = sin((1 to n) .* 12.9898)
u2 = cos((1 to n) .* 7.233)
zn = z .* (1 + 0.005 .* u1) + 1i .* (0.005 .* u2 .* abs(z))
m = sysid(f, zn)
spec = m.spec
k = m.nparam
n_cand = len(m.candidates)
rs = m.R1
rct = m.R2
cdl = m.C1
"#,
    );
    assert_eq!(
        str_of(&it, "spec"),
        "R-p(R,C)",
        "selected the wrong topology"
    );
    assert_eq!(num_of(&it, "k"), 3.0);
    assert!(num_of(&it, "n_cand") >= 5.0, "every candidate should be reported");
    for (name, want) in [("rs", 10.0), ("rct", 100.0), ("cdl", 1e-5)] {
        assert!(
            rel(num_of(&it, name), want) < 0.05,
            "{name}: recovered {}, truth {want}",
            num_of(&it, name)
        );
    }
}

/// Chi-squared alone would have chosen differently, which is the whole
/// argument for the criterion. Asserted directly: the best-chi2 candidate
/// must have at least as many parameters as the AIC-chosen one, and on this
/// data strictly more.
#[test]
fn the_best_chi_squared_candidate_is_bigger_than_the_chosen_one() {
    let it = run(
        r#"
n = 61
f = logspace(-1, 5, n)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
u1 = sin((1 to n) .* 12.9898)
u2 = cos((1 to n) .* 7.233)
zn = z .* (1 + 0.005 .* u1) + 1i .* (0.005 .* u2 .* abs(z))
m = sysid(f, zn)
i_chi2 = argmin(m.candidate_chi2)
i_aic = argmin(m.candidate_aic)
k_chi2 = m.candidate_nparam[i_chi2]
k_aic = m.candidate_nparam[i_aic]
"#,
    );
    let (k_chi2, k_aic) = (num_of(&it, "k_chi2"), num_of(&it, "k_aic"));
    assert!(
        k_chi2 > k_aic,
        "on this data the lowest chi-squared should belong to a LARGER circuit \
         than AIC selects ({k_chi2} vs {k_aic}) -- if they agree, this test is \
         no longer demonstrating anything"
    );
}

#[test]
fn sysid_with_a_named_topology_is_circuit_fit() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = sysid(f, z, circuit_topology = "R-p(R,C)")
spec = m.spec
rct = m.R2
n_cand = len(m.candidates)
"#,
    );
    assert_eq!(str_of(&it, "spec"), "R-p(R,C)");
    assert_eq!(num_of(&it, "n_cand"), 1.0);
    assert!(rel(num_of(&it, "rct"), 100.0) < 1e-4);
}

#[test]
fn sysid_refuses_an_unparseable_topology() {
    let msg = err(
        r#"
f = logspace(-1, 5, 21)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
m = sysid(f, z, circuit_topology = "R-p(R,Zzz)")
"#,
    );
    assert!(msg.contains("unknown element"), "got: {msg}");
}

/// Same seed, same fit. The multi-start must not make a result depend on
/// anything but its inputs.
#[test]
fn a_fit_is_reproducible() {
    let it = run(
        r#"
f = logspace(-1, 5, 41)
z = circuit_impedance("R-p(R,C)", f, [10, 100, 1e-5])
a = circuit_fit(f, z, "R-p(R,C)", seed = 7)
b = circuit_fit(f, z, "R-p(R,C)", seed = 7)
gap = max(abs(a.params - b.params))
"#,
    );
    assert_eq!(num_of(&it, "gap"), 0.0);
}
