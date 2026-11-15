//! Solver builtins (v0.4.14), through the language: `fsolve`, `linprog`,
//! `quadprog`, `minimize(method="nelder_mead")` and the iterative sparse
//! solvers `sparse.cg`, `sparse.gmres`, `sparse.bicgstab`.
//!
//! Expected values come from the problem itself (a residual, a KKT
//! condition, a closed form, a brute-force vertex enumeration done here),
//! never from reading the answer back out of the code under test.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    match it.run(src) {
        Ok(()) => panic!("expected an error, but this ran clean:\n{src}"),
        Err(e) => e.to_string(),
    }
}

fn nums(it: &Interp, n: &str) -> Vec<f64> {
    match it.get(n) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Num(x)) => vec![*x],
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    nums(it, n)[0]
}

fn flag(it: &Interp, n: &str) -> bool {
    match it.get(n) {
        Some(Value::Bool(b)) => *b,
        other => panic!("`{n}` is {other:?}, not a bool"),
    }
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{n}` is {other:?}, not a string"),
    }
}

fn lit(v: &[f64]) -> String {
    format!("[{}]", v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", "))
}

const FNS: &str = "
function circle(v)
    return [v[0]^2 + v[1]^2 - 4, v[0] - v[1]]
end function
function circle_jac(v)
    return [2*v[0], 2*v[1]; 1, -1]
end function
function rosen_sys(v)
    return [10*(v[1] - v[0]^2), 1 - v[0]]
end function
function powell(v)
    return [10000*v[0]*v[1] - 1, exp(-v[0]) + exp(-v[1]) - 1.0001]
end function
function smooth3(v)
    return [v[0]^2 + v[1]^2 + v[2]^2 - 14, v[0]*v[1] - 2, exp(v[0] - 1) + v[1]*v[2] - 7]
end function
function no_root(v)
    return [v[0]^2 + 1]
end function
function boom(v)
    error(\"user function exploded\")
end function
function wrong_len(v)
    if v[0] == 1 then
        return [1, 2]
    end if
    return [1, 2, 3]
end function
function bad_jac(v)
    return [1, 2, 3; 4, 5, 6]
end function
function over(v)
    return [v[0] - 1, v[0] - 2, v[0] - 6]
end function
function plane(v)
    return [v[0] + v[1] + v[2] - 3]
end function
function bowl(p)
    return (p[0] - 3)^2 + (p[1] + 2)^2
end function
function rosen(p)
    return (1 - p[0])^2 + 100*(p[1] - p[0]^2)^2
end function
function absnorm(p)
    return abs(p[0] - 1) + abs(p[1] + 2)
end function
function objboom(p)
    error(\"objective exploded\")
end function
";

// ---------------------------------------------------------------- fsolve

#[test]
fn fsolve_circle_and_line() {
    let it = run(&format!("{FNS}\nr = fsolve(circle, [1, 0.5])\nx = r.x\nfv = r.fval\nn = r.norm\nc = r.converged\nit = r.iterations"));
    assert!(flag(&it, "c"));
    let s = 2.0f64.sqrt();
    let x = nums(&it, "x");
    assert!((x[0] - s).abs() < 1e-8 && (x[1] - s).abs() < 1e-8, "{x:?}");
    // residual checked against the equations, not against r.fval
    assert!((x[0] * x[0] + x[1] * x[1] - 4.0).abs() < 1e-9 && (x[0] - x[1]).abs() < 1e-9);
    assert_eq!(nums(&it, "fv").len(), 2);
    assert!(num(&it, "n") < 1e-9);
    assert!(num(&it, "it") >= 1.0);
}

#[test]
fn fsolve_with_an_analytic_jacobian_reaches_the_same_root() {
    let it = run(&format!("{FNS}\nr = fsolve(circle, [1, 0.5], jac=circle_jac)\nx = r.x\nc = r.converged"));
    assert!(flag(&it, "c"));
    let s = 2.0f64.sqrt();
    let x = nums(&it, "x");
    assert!((x[0] - s).abs() < 1e-9 && (x[1] - s).abs() < 1e-9);
}

#[test]
fn fsolve_rosenbrock_powell_and_a_three_by_three() {
    let it = run(&format!(
        "{FNS}\na = fsolve(rosen_sys, [-1.2, 1])\nax = a.x\nac = a.converged\n\
         p = fsolve(powell, [0, 1], max_iter=1000)\npx = p.x\npc = p.converged\n\
         q = fsolve(smooth3, [0.8, 1.5, 2.5])\nqx = q.x\nqc = q.converged"
    ));
    assert!(flag(&it, "ac") && flag(&it, "pc") && flag(&it, "qc"));
    let ax = nums(&it, "ax");
    assert!((ax[0] - 1.0).abs() < 1e-8 && (ax[1] - 1.0).abs() < 1e-8);
    let px = nums(&it, "px");
    assert!((1e4 * px[0] * px[1] - 1.0).abs() < 1e-8, "{px:?}");
    assert!(((-px[0]).exp() + (-px[1]).exp() - 1.0001).abs() < 1e-8);
    let qx = nums(&it, "qx");
    for (a, b) in qx.iter().zip([1.0, 2.0, 3.0]) {
        assert!((a - b).abs() < 1e-7, "{qx:?}");
    }
}

#[test]
fn fsolve_reports_failure_instead_of_raising() {
    let it = run(&format!(
        "{FNS}\nr = fsolve(no_root, [2])\nc = r.converged\nn = r.norm\n\
         s = fsolve(rosen_sys, [-1.2, 1], max_iter=2)\nsc = s.converged\nsi = s.iterations"
    ));
    assert!(!flag(&it, "c"));
    assert!(num(&it, "n") >= 0.99, "the residual of x^2+1 can never go below 1");
    assert!(!flag(&it, "sc"));
    assert!(num(&it, "si") <= 2.0);
}

#[test]
fn fsolve_under_and_over_determined_are_least_squares() {
    let it = run(&format!(
        "{FNS}\nu = fsolve(plane, [0, 0, 0])\nux = u.x\nuc = u.converged\n\
         o = fsolve(over, [0])\nox = o.x\noc = o.converged"
    ));
    assert!(flag(&it, "uc") && flag(&it, "oc"));
    assert!((nums(&it, "ux").iter().sum::<f64>() - 3.0).abs() < 1e-9);
    assert!((num(&it, "ox") - 3.0).abs() < 1e-7, "least squares of x=1,2,6 is the mean 3");
}

#[test]
fn fsolve_errors_are_clear() {
    assert!(err(&format!("{FNS}\nr = fsolve(\"nope\", [1])")).contains("no user function named `nope`"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [1, nan])")).contains("finite"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [])")).contains("x0"));
    assert!(err(&format!("{FNS}\nr = fsolve(boom, [1])")).contains("user function exploded"));
    assert!(err(&format!("{FNS}\nr = fsolve(wrong_len, [1])")).contains("returned 3 values"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [1, 0.5], jac=bad_jac)")).contains("2x2"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [1, 0.5], jac=\"nope\")")).contains("jac="));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [1, 0.5], max_iter=0)")).contains("max_iter"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle, [1, 0.5], tolerance=1e-6)")).to_lowercase().contains("tolerance"));
    assert!(err(&format!("{FNS}\nr = fsolve(circle)")).contains("2 arguments"));
}

// -------------------------------------------------------------- linprog

#[test]
fn linprog_textbook_production_problem() {
    let it = run(
        "r = linprog([-3, -5], [1, 0; 0, 2; 3, 2], [4, 12, 18], lb=0)\nx = r.x\nf = r.fval\ns = r.status\nk = r.iterations",
    );
    assert_eq!(text(&it, "s"), "optimal");
    let x = nums(&it, "x");
    assert!((x[0] - 2.0).abs() < 1e-9 && (x[1] - 6.0).abs() < 1e-9, "{x:?}");
    assert!((num(&it, "f") + 36.0).abs() < 1e-9);
    assert!(num(&it, "k") >= 1.0);
}

#[test]
fn linprog_beale_degenerate_problem_terminates() {
    let it = run(
        "r = linprog([-0.75, 20, -0.5, 6], [0.25, -8, -1, 9; 0.5, -12, -0.5, 3; 0, 0, 1, 0], [0, 0, 1], lb=0)\n\
         s = r.status\nf = r.fval\nx = r.x",
    );
    assert_eq!(text(&it, "s"), "optimal");
    assert!((num(&it, "f") + 1.25).abs() < 1e-9);
    let x = nums(&it, "x");
    assert!((x[0] - 1.0).abs() < 1e-9 && (x[2] - 1.0).abs() < 1e-9 && x[1].abs() < 1e-9 && x[3].abs() < 1e-9);
}

#[test]
fn linprog_infeasible_and_unbounded() {
    let it = run(
        "a = linprog([1, 1], [1, 1; -1, -1], [1, -3], lb=0)\nqa = a.status\nax = a.x\n\
         b = linprog([-1, -1], [1, -1], [1], lb=0)\nbs = b.status\nbf = b.fval",
    );
    assert_eq!(text(&it, "qa"), "infeasible");
    assert!(nums(&it, "ax").iter().all(|v| v.is_nan()));
    assert_eq!(text(&it, "bs"), "unbounded");
    assert_eq!(num(&it, "bf"), f64::NEG_INFINITY);
}

#[test]
fn linprog_equality_constraints_and_bounds() {
    // min x + 2y + 3z, x + y + z = 10, x - y = 2, x,y,z >= 0: (6, 4, 0), 14
    let it = run(
        "r = linprog([1, 2, 3], [], [], Aeq=[1, 1, 1; 1, -1, 0], beq=[10, 2], lb=0)\nx = r.x\nf = r.fval\ns = r.status\n\
         p = linprog([-1, -1], [], [], lb=[-2, -inf], ub=[3, 4])\npx = p.x",
    );
    assert_eq!(text(&it, "s"), "optimal");
    let x = nums(&it, "x");
    assert!((x[0] - 6.0).abs() < 1e-9 && (x[1] - 4.0).abs() < 1e-9 && x[2].abs() < 1e-9);
    assert!((num(&it, "f") - 14.0).abs() < 1e-9);
    let px = nums(&it, "px");
    assert!((px[0] - 3.0).abs() < 1e-12 && (px[1] - 4.0).abs() < 1e-12);
}

#[test]
fn linprog_default_lower_bound_is_minus_infinity() {
    // x >= -5 only through A; with no lb the variable is free and reaches -5.
    let it = run("r = linprog([1], [-1], [5])\nx = r.x\ns = r.status");
    assert_eq!(text(&it, "s"), "optimal");
    assert!((num(&it, "x") + 5.0).abs() < 1e-9);
    // the same call with lb=0 is a different problem
    let it = run("r = linprog([1], [-1], [5], lb=0)\nx = r.x");
    assert!(num(&it, "x").abs() < 1e-12);
}

#[test]
fn linprog_accepts_matlab_positional_order() {
    let it = run("r = linprog([1, 2, 3], [], [], [1, 1, 1; 1, -1, 0], [10, 2], 0)\nf = r.fval");
    assert!((num(&it, "f") - 14.0).abs() < 1e-9);
}

/// All vertices of {A x <= b, 0 <= x <= 10} in 2-D by intersecting every
/// pair of constraint lines, then the best objective: the independent oracle.
fn brute_2d(c: [f64; 2], a: &[[f64; 2]], b: &[f64]) -> Option<f64> {
    let mut rows: Vec<([f64; 2], f64)> = a.iter().copied().zip(b.iter().copied()).collect();
    rows.push(([-1.0, 0.0], 0.0));
    rows.push(([0.0, -1.0], 0.0));
    rows.push(([1.0, 0.0], 10.0));
    rows.push(([0.0, 1.0], 10.0));
    let mut best: Option<f64> = None;
    for i in 0..rows.len() {
        for j in i + 1..rows.len() {
            let (p, q) = (rows[i], rows[j]);
            let det = p.0[0] * q.0[1] - p.0[1] * q.0[0];
            if det.abs() < 1e-12 {
                continue;
            }
            let x = (p.1 * q.0[1] - p.0[1] * q.1) / det;
            let y = (p.0[0] * q.1 - p.1 * q.0[0]) / det;
            if rows.iter().all(|(r, rb)| r[0] * x + r[1] * y <= rb + 1e-9) {
                let v = c[0] * x + c[1] * y;
                best = Some(best.map_or(v, |b0: f64| b0.min(v)));
            }
        }
    }
    best
}

#[test]
fn linprog_random_problems_match_vertex_enumeration() {
    let mut seed = 2024u64;
    let mut rnd = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (1u64 << 31) as f64
    };
    let (mut feas, mut infeas) = (0, 0);
    for case in 0..40 {
        let m = 2 + case % 3;
        let c = [(rnd() * 10.0).round() - 5.0, (rnd() * 10.0).round() - 5.0];
        let a: Vec<[f64; 2]> = (0..m).map(|_| [(rnd() * 8.0).round() - 3.0, (rnd() * 8.0).round() - 3.0]).collect();
        let b: Vec<f64> = (0..m).map(|_| (rnd() * 30.0).round() - 4.0).collect();
        let rows: Vec<String> = a.iter().map(|r| format!("{:?}, {:?}", r[0], r[1])).collect();
        let src = format!(
            "r = linprog({}, [{}], {}, lb=0, ub=10)\nf = r.fval\ns = r.status",
            lit(&c),
            rows.join("; "),
            lit(&b)
        );
        let it = run(&src);
        match brute_2d(c, &a, &b) {
            None => {
                infeas += 1;
                assert_eq!(text(&it, "s"), "infeasible", "case {case}: {src}");
            }
            Some(v) => {
                feas += 1;
                assert_eq!(text(&it, "s"), "optimal", "case {case}: {src}");
                assert!((num(&it, "f") - v).abs() < 1e-7 * (1.0 + v.abs()), "case {case}: {} vs {v}\n{src}", num(&it, "f"));
            }
        }
    }
    assert!(feas > 5 && infeas > 2, "feasible={feas} infeasible={infeas}");
}

#[test]
fn linprog_errors_are_clear() {
    assert!(err("r = linprog()").contains("needs at least c"));
    assert!(err("r = linprog([1, 2], [1, 1, 1], [1])").contains("one column per variable") || err("r = linprog([1, 2], [1, 1, 1], [1])").contains("entries"));
    assert!(err("r = linprog([1, 2], [1, 1; 2, 2], [1])").contains("rows"));
    assert!(err("r = linprog([1, nan], [1, 1], [1])").contains("c must be finite"));
    assert!(err("r = linprog([1, 2], [1, nan], [1])").contains("finite"));
    assert!(err("r = linprog([1, 2], [1, 1], [nan])").contains("finite"));
    assert!(err("r = linprog([1, 2], [1, 1], [1], lb=[0, 0, 0])").contains("lb"));
    assert!(err("r = linprog([1, 2], [1, 1])").contains("without b"));
    assert!(err("r = linprog([1, 2], [], [], Aeq=[1, 1])").contains("without beq"));
    assert!(err("r = linprog([1, 2], [], [], [1, 1], [1], lb=0, ub=1, Aeq=[1, 1])").contains("both positionally"));
    assert!(err("r = linprog([1, 2], [1, 1], [1], bogus=3)").contains("bogus"));
    assert!(err("r = linprog([1, 2], [1, 1], [1], lb=nan)").contains("NaN"));
}

#[test]
fn linprog_iteration_limit_is_a_status_not_a_hang() {
    let it = run("r = linprog([-3, -5], [1, 0; 0, 2; 3, 2], [4, 12, 18], lb=0, max_iter=1)\ns = r.status");
    assert_eq!(text(&it, "s"), "iteration_limit");
}

// ------------------------------------------------------------- quadprog

#[test]
fn quadprog_unconstrained_matches_the_closed_form() {
    let it = run("r = quadprog([4, 1; 1, 3], [1, 2])\nx = r.x\nf = r.fval\ns = r.status");
    assert_eq!(text(&it, "s"), "optimal");
    let x = nums(&it, "x");
    assert!((x[0] + 1.0 / 11.0).abs() < 1e-12 && (x[1] + 7.0 / 11.0).abs() < 1e-12, "{x:?}");
    let want = 0.5 * (x[0] * (4.0 * x[0] + x[1]) + x[1] * (x[0] + 3.0 * x[1])) + x[0] + 2.0 * x[1];
    assert!((num(&it, "f") - want).abs() < 1e-12);
}

#[test]
fn quadprog_box_constrained_and_classic_inequality_example() {
    let it = run(
        "b = quadprog([2, 0; 0, 2], [-6, 2], lb=0, ub=2)\nbx = b.x\n\
         n = quadprog([2, 0; 0, 2], [-2, -5], [-1, 2; 1, 2; 1, -2], [2, 6, 2], lb=0)\nnx = n.x",
    );
    let bx = nums(&it, "bx");
    assert!((bx[0] - 2.0).abs() < 1e-9 && bx[1].abs() < 1e-9, "{bx:?}");
    // Nocedal & Wright, example 16.4
    let nx = nums(&it, "nx");
    assert!((nx[0] - 1.4).abs() < 1e-8 && (nx[1] - 1.7).abs() < 1e-8, "{nx:?}");
}

#[test]
fn quadprog_portfolio_has_inverse_variance_weights() {
    // min x'Sx with S = diag(1, 4, 9), sum x = 1, x >= 0
    let it = run("r = quadprog([2, 0, 0; 0, 8, 0; 0, 0, 18], [0, 0, 0], Aeq=[1, 1, 1], beq=[1], lb=0)\nx = r.x\ns = r.status");
    assert_eq!(text(&it, "s"), "optimal");
    let tot = 1.0 + 0.25 + 1.0 / 9.0;
    for (xi, w) in nums(&it, "x").iter().zip([1.0, 0.25, 1.0 / 9.0]) {
        assert!((xi - w / tot).abs() < 1e-9);
    }
}

#[test]
fn quadprog_equality_constrained_satisfies_the_kkt_system() {
    let it = run(
        "H = [2, 0.5, 0; 0.5, 3, 0.2; 0, 0.2, 1]\nf = [1, -2, 0.5]\nAeq = [1, 1, 1; 1, -1, 0]\n\
         r = quadprog(H, f, [], [], Aeq, [1, 0.25])\nx = r.x",
    );
    let x = nums(&it, "x");
    assert!((x[0] + x[1] + x[2] - 1.0).abs() < 1e-10 && (x[0] - x[1] - 0.25).abs() < 1e-10);
    // H x + f = -Aeq' lambda: the gradient is orthogonal to the null space of
    // Aeq, whose single direction here is (1, 1, -2) (orthogonal to both rows).
    let hx = [2.0 * x[0] + 0.5 * x[1], 0.5 * x[0] + 3.0 * x[1] + 0.2 * x[2], 0.2 * x[1] + x[2]];
    let g = [hx[0] + 1.0, hx[1] - 2.0, hx[2] + 0.5];
    assert!((g[0] + g[1] - 2.0 * g[2]).abs() < 1e-9, "gradient not in the row space: {g:?}");
}

#[test]
fn quadprog_infeasible_and_unbounded_are_statuses() {
    let it = run(
        "a = quadprog([2, 0; 0, 2], [0, 0], [1, 1], [-1], lb=0)\nqa = a.status\n\
         u = quadprog([1, 0; 0, 0], [0, -1], lb=[-inf, 0])\nus = u.status\n\
         c = quadprog([1, 0; 0, 0], [0, -1], lb=[-inf, 0], ub=[inf, 5])\ncs = c.status\ncx = c.x",
    );
    assert_eq!(text(&it, "qa"), "infeasible");
    assert_eq!(text(&it, "us"), "unbounded");
    assert_eq!(text(&it, "cs"), "optimal");
    assert!((nums(&it, "cx")[1] - 5.0).abs() < 1e-8);
}

#[test]
fn quadprog_errors_are_clear() {
    assert!(err("r = quadprog([1, 0; 0, -1], [0, 0])").contains("not positive semidefinite"));
    assert!(err("r = quadprog([1, 2; 0, 1], [0, 0])").contains("symmetric"));
    assert!(err("r = quadprog([1, 0; 0, 1], [0, 0, 0])").contains("H has 2 columns but there are 3"));
    assert!(err("r = quadprog([1, 0; 0, nan], [0, 0])").contains("finite"));
    assert!(err("r = quadprog([1, 0; 0, 1], [0, nan])").contains("finite"));
    assert!(err("r = quadprog([1, 0; 0, 1], [0, 0], [1, 1, 1], [1])").contains("A"));
    assert!(err("r = quadprog([1, 0; 0, 1], [0, 0], [1, 1], [1, 2])").contains("rows"));
    assert!(err("r = quadprog([1, 0; 0, 1])").contains("H and f"));
    assert!(err("r = quadprog([1, 0; 0, 1], [0, 0], zzz=1)").contains("zzz"));
}

// ------------------------------------------------- minimize / nelder_mead

#[test]
fn minimize_nelder_mead_finds_rosenbrock_and_non_smooth_minima() {
    let it = run(&format!(
        "{FNS}\nr = minimize(rosen, [-1.2, 1], method=\"nelder_mead\", max_iter=1000, tol=1e-10)\nx = r.params\nv = r.value\nc = r.converged\n\
         a = minimize(absnorm, [4, 3], method=\"nelder_mead\", max_iter=3000, tol=1e-9)\nax = a.params\n\
         b = minimize(bowl, [0, 0], method=\"nelder_mead\")\nbx = b.params\nbc = b.converged"
    ));
    assert!(flag(&it, "c"));
    let x = nums(&it, "x");
    assert!((x[0] - 1.0).abs() < 1e-4 && (x[1] - 1.0).abs() < 1e-4, "{x:?}");
    assert!(num(&it, "v") < 1e-8);
    let ax = nums(&it, "ax");
    assert!((ax[0] - 1.0).abs() < 1e-5 && (ax[1] + 2.0).abs() < 1e-5, "{ax:?}");
    let bx = nums(&it, "bx");
    assert!((bx[0] - 3.0).abs() < 1e-4 && (bx[1] + 2.0).abs() < 1e-4);
    assert!(flag(&it, "bc"));
}

#[test]
fn minimize_default_and_explicit_bfgs_are_identical() {
    let it = run(&format!(
        "{FNS}\na = minimize(bowl, [0, 0])\nap = a.params\nav = a.value\n\
         b = minimize(bowl, [0, 0], method=\"bfgs\")\nbp = b.params\nbv = b.value"
    ));
    assert_eq!(nums(&it, "ap"), nums(&it, "bp"));
    assert_eq!(num(&it, "av").to_bits(), num(&it, "bv").to_bits());
    let p = nums(&it, "ap");
    assert!((p[0] - 3.0).abs() < 1e-3 && (p[1] + 2.0).abs() < 1e-3);
}

#[test]
fn minimize_nelder_mead_is_bounded_and_errors_are_clear() {
    let it = run(&format!("{FNS}\nr = minimize(rosen, [-1.2, 1], method=\"nelder_mead\", max_iter=5)\nc = r.converged"));
    assert!(!flag(&it, "c"));
    assert!(err(&format!("{FNS}\nr = minimize(bowl, [0, 0], method=\"simplex\")")).contains("unknown method"));
    assert!(err(&format!("{FNS}\nr = minimize(bowl, [0, 0], method=\"nelder_mead\", grad=\"bowl\")")).contains("grad="));
    assert!(err(&format!("{FNS}\nr = minimize(objboom, [0, 0], method=\"nelder_mead\")")).contains("objective exploded"));
    assert!(err(&format!("{FNS}\nr = minimize(bowl, [nan, 0], method=\"nelder_mead\")")).contains("finite"));
    assert!(err(&format!("{FNS}\nr = minimize(bowl, [0, 0], method=\"nelder_mead\", max_iter=0)")).contains("max_iter"));
    assert!(err(&format!("{FNS}\nr = minimize(bowl, [0, 0], method=\"nelder_mead\", bogus=1)")).contains("bogus"));
}

// ---------------------------------------------------------- sparse solvers

/// Triplet literal source for `A = sparse.from_triplets(...)`.
fn sparse_src(name: &str, n: usize, t: &[(usize, usize, f64)]) -> String {
    let i: Vec<f64> = t.iter().map(|x| x.0 as f64).collect();
    let j: Vec<f64> = t.iter().map(|x| x.1 as f64).collect();
    let v: Vec<f64> = t.iter().map(|x| x.2).collect();
    format!("{name} = sparse.from_triplets({}, {}, {}, {n}, {n})\n", lit(&i), lit(&j), lit(&v))
}

fn poisson_1d(n: usize) -> Vec<(usize, usize, f64)> {
    let mut t = vec![];
    for i in 0..n {
        t.push((i, i, 2.0));
        if i > 0 {
            t.push((i, i - 1, -1.0));
        }
        if i + 1 < n {
            t.push((i, i + 1, -1.0));
        }
    }
    t
}

fn poisson_2d(m: usize) -> Vec<(usize, usize, f64)> {
    let mut t = vec![];
    for r in 0..m {
        for c in 0..m {
            let k = r * m + c;
            t.push((k, k, 4.0));
            if r > 0 {
                t.push((k, k - m, -1.0));
            }
            if r + 1 < m {
                t.push((k, k + m, -1.0));
            }
            if c > 0 {
                t.push((k, k - 1, -1.0));
            }
            if c + 1 < m {
                t.push((k, k + 1, -1.0));
            }
        }
    }
    t
}

fn matvec(t: &[(usize, usize, f64)], x: &[f64]) -> Vec<f64> {
    let mut y = vec![0.0; x.len()];
    for &(i, j, v) in t {
        y[i] += v * x[j];
    }
    y
}

fn rel_resid(t: &[(usize, usize, f64)], x: &[f64], b: &[f64]) -> f64 {
    let ax = matvec(t, x);
    let r: f64 = ax.iter().zip(b).map(|(p, q)| (p - q) * (p - q)).sum::<f64>().sqrt();
    r / b.iter().map(|v| v * v).sum::<f64>().sqrt()
}

fn solve_with(name: &str, t: &[(usize, usize, f64)], b: &[f64], opts: &str) -> Interp {
    let n = b.len();
    run(&format!(
        "import sparse\n{}b = {}\nr = sparse.{name}(A, b{opts})\nx = r.x\nit = r.iterations\nres = r.residual\nc = r.converged",
        sparse_src("A", n, t),
        lit(b)
    ))
}

#[test]
fn sparse_cg_solves_1d_and_2d_poisson_to_the_known_solution() {
    // 1-D: manufactured solution
    let n = 60;
    let t = poisson_1d(n);
    let xs: Vec<f64> = (0..n).map(|i| ((i as f64) * 0.2).sin() + 1.0).collect();
    let b = matvec(&t, &xs);
    let it = solve_with("cg", &t, &b, "");
    assert!(flag(&it, "c"));
    let x = nums(&it, "x");
    for (p, q) in x.iter().zip(&xs) {
        assert!((p - q).abs() < 1e-8, "1-D Poisson solution off");
    }
    assert!(num(&it, "res") < 1e-9);
    assert!((num(&it, "res") - rel_resid(&t, &x, &b)).abs() < 1e-12, "reported residual is the true one");
    assert!(num(&it, "it") <= n as f64 + 1.0, "CG finishes in at most n steps in exact arithmetic");
    // against sparse.solve (LU)
    let lu = run(&format!("import sparse\n{}b = {}\nx = sparse.solve(A, b)", sparse_src("A", n, &t), lit(&b)));
    for (p, q) in x.iter().zip(nums(&lu, "x")) {
        assert!((p - q).abs() < 1e-8);
    }
    // 2-D: 12 x 12 grid
    let m = 12;
    let t2 = poisson_2d(m);
    let xs2: Vec<f64> = (0..m * m).map(|k| 1.0 + ((k % 7) as f64) * 0.1 - ((k % 5) as f64) * 0.05).collect();
    let b2 = matvec(&t2, &xs2);
    let it2 = solve_with("cg", &t2, &b2, "");
    assert!(flag(&it2, "c"));
    for (p, q) in nums(&it2, "x").iter().zip(&xs2) {
        assert!((p - q).abs() < 1e-8, "2-D Poisson solution off");
    }
}

#[test]
fn sparse_gmres_and_bicgstab_solve_a_convection_diffusion_matrix() {
    // tridiagonal (-1 - p, 2, -1 + p): nonsymmetric, diagonally dominant for |p| < 1
    let n = 80;
    let p = 0.6;
    let mut t = vec![];
    for i in 0..n {
        t.push((i, i, 2.0));
        if i > 0 {
            t.push((i, i - 1, -1.0 - p));
        }
        if i + 1 < n {
            t.push((i, i + 1, -1.0 + p));
        }
    }
    let xs: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64 * 0.3).cos()).collect();
    let b = matvec(&t, &xs);
    for name in ["gmres", "bicgstab"] {
        let it = solve_with(name, &t, &b, "");
        assert!(flag(&it, "c"), "{name} did not converge");
        for (a, q) in nums(&it, "x").iter().zip(&xs) {
            assert!((a - q).abs() < 1e-7, "{name} solution off");
        }
        assert!(num(&it, "res") < 1e-9);
    }
    // restarted GMRES with a small restart still converges, taking more steps
    let full = solve_with("gmres", &t, &b, ", restart=80");
    let small = solve_with("gmres", &t, &b, ", restart=5, max_iter=5000");
    assert!(flag(&full, "c") && flag(&small, "c"));
    assert!(num(&small, "it") >= num(&full, "it"));
    for (a, q) in nums(&small, "x").iter().zip(&xs) {
        assert!((a - q).abs() < 1e-6);
    }
    // gmres also solves the SPD Poisson system
    let tp = poisson_1d(40);
    let xp: Vec<f64> = (0..40).map(|i| (i as f64).sqrt()).collect();
    let bp = matvec(&tp, &xp);
    let g = solve_with("gmres", &tp, &bp, "");
    assert!(flag(&g, "c"));
    for (a, q) in nums(&g, "x").iter().zip(&xp) {
        assert!((a - q).abs() < 1e-7);
    }
}

#[test]
fn sparse_jacobi_preconditioning_cuts_the_iteration_count() {
    // A = S T S with T = tridiag(-1, 4, -1) and a wildly varying diagonal
    // scale S: SPD, and exactly the case a Jacobi preconditioner undoes.
    let n = 60;
    let s: Vec<f64> = (0..n).map(|i| 10f64.powf(1.5 * (1.7 * i as f64 + 0.3).sin())).collect();
    let mut t = vec![];
    for i in 0..n {
        t.push((i, i, 4.0 * s[i] * s[i]));
        if i > 0 {
            t.push((i, i - 1, -s[i] * s[i - 1]));
        }
        if i + 1 < n {
            t.push((i, i + 1, -s[i] * s[i + 1]));
        }
    }
    let xs: Vec<f64> = (0..n).map(|i| 1.0 + (i % 3) as f64).collect();
    let b = matvec(&t, &xs);
    let plain = solve_with("cg", &t, &b, ", max_iter=5000");
    let jac = solve_with("cg", &t, &b, ", max_iter=5000, precond=\"jacobi\"");
    assert!(flag(&jac, "c"));
    // the matrix has condition number ~1e6, so judge the answer by its
    // residual (computed here) and a correspondingly loose solution error
    let xj = nums(&jac, "x");
    assert!(rel_resid(&t, &xj, &b) < 1e-9, "preconditioned residual");
    for (a, q) in xj.iter().zip(&xs) {
        assert!((a - q).abs() < 1e-3 * q.abs().max(1.0), "preconditioned solution off");
    }
    assert!(
        num(&jac, "it") * 2.0 < num(&plain, "it"),
        "jacobi {} vs plain {} iterations",
        num(&jac, "it"),
        num(&plain, "it")
    );
    // the same holds for the nonsymmetric solvers on a scaled nonsymmetric matrix
    for name in ["gmres", "bicgstab"] {
        let p = solve_with(name, &t, &b, ", max_iter=5000");
        let j = solve_with(name, &t, &b, ", max_iter=5000, precond=\"jacobi\"");
        assert!(flag(&j, "c"), "{name} jacobi");
        assert!(num(&j, "it") < num(&p, "it"), "{name}: jacobi {} vs plain {}", num(&j, "it"), num(&p, "it"));
    }
}

#[test]
fn sparse_cg_on_a_non_spd_matrix_stops_and_says_so() {
    // symmetric indefinite: p'Ap hits zero or goes negative; must not hang
    let t = vec![(0, 0, 1.0), (1, 1, -1.0), (2, 2, 2.0), (3, 3, -2.0)];
    let it = solve_with("cg", &t, &[1.0, 1.0, 1.0, 1.0], ", max_iter=50");
    assert!(!flag(&it, "c"));
    assert!(num(&it, "it") <= 50.0);
    // nonsymmetric input is refused with a pointer to gmres/bicgstab
    let e = err(&format!("import sparse\n{}b = [1, 1]\nr = sparse.cg(A, b)", sparse_src("A", 2, &[(0, 0, 1.0), (0, 1, 2.0), (1, 1, 1.0)])));
    assert!(e.contains("symmetric") && e.contains("gmres"), "{e}");
}

#[test]
fn sparse_iterative_solvers_use_x0_and_respect_max_iter() {
    let n = 30;
    let t = poisson_1d(n);
    let xs: Vec<f64> = (0..n).map(|i| 1.0 + i as f64).collect();
    let b = matvec(&t, &xs);
    // starting at the answer converges in zero iterations
    for name in ["cg", "gmres", "bicgstab"] {
        let it = run(&format!(
            "import sparse\n{}b = {}\nx0 = {}\nr = sparse.{name}(A, b, x0)\nit = r.iterations\nc = r.converged",
            sparse_src("A", n, &t),
            lit(&b),
            lit(&xs)
        ));
        assert!(flag(&it, "c") && num(&it, "it") == 0.0, "{name}");
    }
    // a starting guess near the answer helps vs zero
    let near: Vec<f64> = xs.iter().map(|v| v + 1e-3).collect();
    let cold = solve_with("cg", &t, &b, "");
    let warm = run(&format!(
        "import sparse\n{}b = {}\nr = sparse.cg(A, b, x0={}, tol=1e-8)\nit = r.iterations\nc = r.converged",
        sparse_src("A", n, &t),
        lit(&b),
        lit(&near)
    ));
    assert!(flag(&warm, "c") && num(&warm, "it") <= num(&cold, "it"));
    // max_iter caps the work and converged says false
    for name in ["cg", "gmres", "bicgstab"] {
        let it = solve_with(name, &t, &b, ", max_iter=3");
        assert!(!flag(&it, "c"), "{name}");
        assert!(num(&it, "it") <= 3.0, "{name}: {}", num(&it, "it"));
        assert!(num(&it, "res") > 1e-10);
    }
}

#[test]
fn sparse_iterative_solver_errors_are_clear() {
    let base = format!("import sparse\n{}", sparse_src("A", 3, &poisson_1d(3)));
    assert!(err(&format!("{base}r = sparse.cg(A, [1, 2])")).contains("length 2"));
    assert!(err(&format!("{base}r = sparse.gmres(A, [1, nan, 3])")).contains("non-finite"));
    assert!(err(&format!("{base}r = sparse.bicgstab(A, [1, 2, 3], [1, 2])")).contains("x0"));
    assert!(err(&format!("{base}r = sparse.cg(A, [1, 2, 3], precond=\"ilu\")")).contains("precond"));
    assert!(err(&format!("{base}r = sparse.gmres(A, [1, 2, 3], restart=0)")).contains("restart"));
    assert!(err(&format!("{base}r = sparse.cg(A, [1, 2, 3], max_iter=0)")).contains("max_iter"));
    assert!(err(&format!("{base}r = sparse.cg(A, [1, 2, 3], tol=-1)")).contains("tol"));
    assert!(err(&format!("{base}r = sparse.cg([1, 2], [1, 2, 3])")).contains("sparse matrix"));
    assert!(err(&format!("{base}r = sparse.cg(A, [1, 2, 3], bogus=1)")).contains("bogus"));
    let rect = "import sparse\nA = sparse.from_triplets([0, 1], [0, 1], [1, 1], 2, 3)\nr = sparse.cg(A, [1, 2])";
    assert!(err(rect).contains("square"));
    // zero right-hand side: the solution is zero, no work
    let z = run(&format!("{base}r = sparse.cg(A, [0, 0, 0])\nx = r.x\nc = r.converged"));
    assert!(flag(&z, "c") && nums(&z, "x").iter().all(|v| *v == 0.0));
}
