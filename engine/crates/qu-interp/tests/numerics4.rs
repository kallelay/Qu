//! Round-4 numerics (v0.4.14), through the language: `bvp`/`bvp_eval`,
//! `sparse.diffmat`, `sparse.laplacian`, `chebdiff`, `integral2` and
//! `fminbnd`.
//!
//! Every expectation is independent of the code under test: a closed-form
//! solution, a convergence-order slope measured here, a spectrum known from
//! theory, a residual, or a reference computed in this file by a different
//! method (a fine finite-difference Newton solve for the Bratu problem).

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
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    nums(it, n)[0]
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{n}` is {other:?}, not a string"),
    }
}

fn flag(it: &Interp, n: &str) -> bool {
    match it.get(n) {
        Some(Value::Bool(b)) => *b,
        other => panic!("`{n}` is {other:?}, not a bool"),
    }
}

/// (rows, cols, row-major copy) of a matrix value.
fn mat(it: &Interp, n: &str) -> (usize, usize, Vec<Vec<f64>>) {
    match it.get(n) {
        Some(Value::Mat(m)) => {
            let rows = (0..m.rows()).map(|i| (0..m.cols()).map(|j| m.get(i, j).unwrap()).collect()).collect();
            (m.rows(), m.cols(), rows)
        }
        other => panic!("`{n}` is {other:?}, not a matrix"),
    }
}

fn lit(v: &[f64]) -> String {
    format!("[{}]", v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", "))
}

fn maxerr(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

// ---------------------------------------------------------------- bvp

const SINE: &str = "
function f(x, y)
    return [y[1], -y[0]]
end function
function bc(ya, yb)
    return [ya[0], yb[0] - 1]
end function
";

#[test]
fn bvp_sine_matches_the_closed_form() {
    let it = run(&format!(
        "{SINE}
x = linspace(0, pi/2, 11)
sol = bvp(f, bc, x, zeros(2, 11), tol=1e-9)
st = sol.status
xq = linspace(0, pi/2, 301)
yq = bvp_eval(sol, xq)
"
    ));
    assert_eq!(text(&it, "st"), "converged");
    let xq = nums(&it, "xq");
    let (rows, cols, yq) = mat(&it, "yq");
    assert_eq!((rows, cols), (2, 301));
    let e0 = (0..cols).map(|j| (yq[0][j] - xq[j].sin()).abs()).fold(0.0, f64::max);
    let e1 = (0..cols).map(|j| (yq[1][j] - xq[j].cos()).abs()).fold(0.0, f64::max);
    eprintln!("bvp sine: max err y {e0:e}, y' {e1:e}");
    assert!(e0 < 2e-8, "{e0}");
    assert!(e1 < 2e-7, "{e1}");
}

#[test]
fn bvp_default_tolerance_is_honoured_and_reported() {
    // the reported residual is below tol, and the true error is of that order
    let it = run(&format!(
        "{SINE}
sol = bvp(f, bc, linspace(0, pi/2, 11), zeros(2, 11))
st = sol.status
res = sol.max_residual
nodes = sol.nodes
xs = sol.x
ys = sol.y
"
    ));
    assert_eq!(text(&it, "st"), "converged");
    assert!(num(&it, "res") < 1e-6);
    let xs = nums(&it, "xs");
    assert_eq!(xs.len() as f64, num(&it, "nodes"));
    let (_, _, ys) = mat(&it, "ys");
    let e = xs.iter().enumerate().map(|(j, x)| (ys[0][j] - x.sin()).abs()).fold(0.0, f64::max);
    eprintln!("bvp default tol: {} nodes, max err {e:e}", xs.len());
    assert!(e < 1e-5, "{e}");
}

#[test]
fn bvp_cubic_is_reproduced_exactly() {
    // y'' = 6x, y(0) = 0, y(1) = 1  ->  y = x^3
    let it = run("
function f(x, y)
    return [y[1], 6*x]
end function
function bc(ya, yb)
    return [ya[0], yb[0] - 1]
end function
sol = bvp(f, bc, linspace(0, 1, 6), zeros(2, 6), tol=1e-10)
st = sol.status
xq = linspace(0, 1, 101)
yq = bvp_eval(sol, xq)
");
    assert_eq!(text(&it, "st"), "converged");
    let xq = nums(&it, "xq");
    let (_, cols, yq) = mat(&it, "yq");
    let e = (0..cols).map(|j| (yq[0][j] - xq[j].powi(3)).abs()).fold(0.0, f64::max);
    eprintln!("bvp cubic: max err {e:e}");
    assert!(e < 1e-10, "{e}");
}

/// Reference lower-branch Bratu solution `u'' + exp(u) = 0`, `u(0)=u(1)=0`
/// by Newton on the second-order finite differences, with Richardson
/// extrapolation over two grids: independent of the collocation code.
fn bratu_reference(xq: f64) -> f64 {
    fn solve(m: usize) -> Vec<f64> {
        // interior unknowns u_1..u_{m-1}, h = 1/m
        let h = 1.0 / m as f64;
        let n = m - 1;
        let mut u = vec![0.0; n];
        for _ in 0..50 {
            // residual F_i = (u_{i-1} - 2u_i + u_{i+1})/h^2 + exp(u_i)
            let get = |u: &Vec<f64>, i: isize| if i < 0 || i as usize >= n { 0.0 } else { u[i as usize] };
            let f: Vec<f64> = (0..n as isize).map(|i| (get(&u, i - 1) - 2.0 * get(&u, i) + get(&u, i + 1)) / (h * h) + get(&u, i).exp()).collect();
            // tridiagonal Jacobian
            let mut dl = vec![1.0 / (h * h); n];
            let mut dd: Vec<f64> = (0..n).map(|i| -2.0 / (h * h) + u[i].exp()).collect();
            let du = vec![1.0 / (h * h); n];
            let mut rhs: Vec<f64> = f.iter().map(|v| -v).collect();
            // Thomas
            for i in 1..n {
                let w = dl[i] / dd[i - 1];
                dd[i] -= w * du[i - 1];
                rhs[i] -= w * rhs[i - 1];
            }
            dl.clear();
            let mut step = vec![0.0; n];
            step[n - 1] = rhs[n - 1] / dd[n - 1];
            for i in (0..n - 1).rev() {
                step[i] = (rhs[i] - du[i] * step[i + 1]) / dd[i];
            }
            let mut mx: f64 = 0.0;
            for i in 0..n {
                u[i] += step[i];
                mx = mx.max(step[i].abs());
            }
            if mx < 1e-14 {
                break;
            }
        }
        u
    }
    let at = |m: usize| {
        let u = solve(m);
        // xq is a grid point of both grids when it is a multiple of 1/m
        let k = (xq * m as f64).round() as usize;
        u[k - 1]
    };
    let (a, b) = (at(2000), at(4000));
    b + (b - a) / 3.0
}

#[test]
fn bvp_bratu_lower_branch() {
    let it = run("
function f(x, y)
    return [y[1], -exp(y[0])]
end function
function bc(ya, yb)
    return [ya[0], yb[0]]
end function
sol = bvp(f, bc, linspace(0, 1, 11), zeros(2, 11), tol=1e-9)
st = sol.status
u = bvp_eval(sol, 0.5)
");
    assert_eq!(text(&it, "st"), "converged");
    let u = nums(&it, "u");
    let want = bratu_reference(0.5);
    eprintln!("bratu u(0.5): bvp {:.10} reference {want:.10}", u[0]);
    assert!((want - 0.1405).abs() < 1e-3, "reference itself is off: {want}");
    assert!((u[0] - want).abs() < 1e-6, "{} vs {want}", u[0]);
}

#[test]
fn bvp_boundary_layer_against_the_analytic_solution() {
    // eps y'' + y' = 0, y(0) = 0, y(1) = 1, eps = 0.02
    let it = run("
function f(x, y)
    return [y[1], -y[1] / 0.02]
end function
function bc(ya, yb)
    return [ya[0], yb[0] - 1]
end function
sol = bvp(f, bc, linspace(0, 1, 11), zeros(2, 11), tol=1e-7)
st = sol.status
nodes = sol.nodes
xq = linspace(0, 1, 501)
yq = bvp_eval(sol, xq)
");
    assert_eq!(text(&it, "st"), "converged");
    let xq = nums(&it, "xq");
    let (_, cols, yq) = mat(&it, "yq");
    let eps = 0.02f64;
    let exact = |x: f64| (1.0 - (-x / eps).exp()) / (1.0 - (-1.0 / eps).exp());
    let e = (0..cols).map(|j| (yq[0][j] - exact(xq[j])).abs()).fold(0.0, f64::max);
    eprintln!("bvp boundary layer: {} nodes, max err {e:e}", num(&it, "nodes"));
    assert!(e < 5e-6, "{e}");
    assert!(num(&it, "nodes") > 11.0, "the mesh must have been refined");
}

#[test]
fn bvp_accepts_a_guess_function_and_a_scalar_problem() {
    // y' = y with y(0) = 1: y(1) = e. n = 1, so f and bc see plain numbers.
    let it = run("
function f(x, y)
    return y
end function
function bc(ya, yb)
    return ya - 1
end function
sol = bvp(f, bc, linspace(0, 1, 6), (x) := 1 + x, tol=1e-9)
st = sol.status
y1 = bvp_eval(sol, 1)
");
    assert_eq!(text(&it, "st"), "converged");
    assert!((num(&it, "y1") - std::f64::consts::E).abs() < 1e-8);
    // a vector-valued guess function for a 2-unknown problem
    let it = run(&format!(
        "{SINE}
sol = bvp(f, bc, linspace(0, pi/2, 11), (x) := [x, 1], tol=1e-8)
st = sol.status
y = bvp_eval(sol, pi/4)
"
    ));
    assert_eq!(text(&it, "st"), "converged");
    let y = nums(&it, "y");
    assert!((y[0] - (std::f64::consts::FRAC_PI_4).sin()).abs() < 1e-7);
}

#[test]
fn bvp_reports_singular_and_unsolvable_problems_honestly() {
    // y' = 0 with y(0) = y(1): every constant is a solution -> singular
    let it = run("
function f(x, y)
    return 0
end function
function bc(ya, yb)
    return ya - yb
end function
sol = bvp(f, bc, linspace(0, 1, 6), linspace(0, 1, 6))
st = sol.status
");
    assert_eq!(text(&it, "st"), "singular");
    // y'' + y = 0, y(0) = 0, y(pi) = 1 has no solution (resonance)
    let it = run("
function f(x, y)
    return [y[1], -y[0]]
end function
function bc(ya, yb)
    return [ya[0], yb[0] - 1]
end function
sol = bvp(f, bc, linspace(0, pi, 11), zeros(2, 11), max_nodes=400)
st = sol.status
");
    assert_eq!(text(&it, "st"), "singular");
}

#[test]
fn bvp_max_nodes_is_a_status_not_a_crash() {
    let it = run("
function f(x, y)
    return [y[1], -y[1] / 0.002]
end function
function bc(ya, yb)
    return [ya[0], yb[0] - 1]
end function
sol = bvp(f, bc, linspace(0, 1, 11), zeros(2, 11), tol=1e-8, max_nodes=15)
st = sol.status
n = sol.nodes
");
    assert_eq!(text(&it, "st"), "max_nodes");
    assert!(num(&it, "n") <= 15.0);
}

#[test]
fn bvp_input_errors() {
    let m = err(&format!("{SINE}\nsol = bvp(f, bc, [0, 1, 0.5], zeros(2, 3))"));
    assert!(m.contains("strictly increasing"), "{m}");
    let m = err(&format!("{SINE}\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(2, 4))"));
    assert!(m.contains("one column per node"), "{m}");
    let m = err(&format!("{SINE}\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(5, 2))"));
    assert!(m.contains("transposed"), "{m}");
    let m = err("function f(x, y)\n  return [y[1], -y[0]]\nend function\nfunction bc(ya, yb)\n  return [ya[0]]\nend function\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(2, 5))");
    assert!(m.contains("one residual per component"), "{m}");
    let m = err("function bc(ya, yb)\n  return [ya[0], yb[0]]\nend function\nsol = bvp(nope, bc, linspace(0, 1, 5), zeros(2, 5))");
    assert!(m.contains("nope"), "{m}");
    let m = err("function f(x, y)\n  error(\"boom\")\nend function\nfunction bc(ya, yb)\n  return [ya[0], yb[0]]\nend function\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(2, 5))");
    assert!(m.contains("boom"), "{m}");
    let m = err(&format!("{SINE}\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(2, 5), tolerance=1e-3)"));
    assert!(m.contains("tolerance"), "{m}");
    let m = err(&format!("{SINE}\nsol = bvp(f, bc, linspace(0, 1, 5), zeros(2, 5))\nz = bvp_eval(sol, 2)"));
    assert!(m.contains("outside"), "{m}");
}

// --------------------------------------------------- diffmat / laplacian

fn slope(e1: f64, e2: f64) -> f64 {
    (e1 / e2).log2()
}

#[test]
fn diffmat_converges_at_second_order() {
    // errors at n and 2n against the exact derivative of sin; the slope of
    // log(error) against log(h) is measured here
    let src = "
import sparse
function dmax(n, order, bc)
    if bc == \"periodic\"
        h = 2*pi/n
        x = linspace(0, 2*pi - h, n)
    else
        h = 2*pi/(n - 1)
        x = linspace(0, 2*pi, n)
    end if
    D = sparse.diffmat(n, h, order=order, bc=bc)
    d = sparse.mul(D, sin(x + 0.5))
    if order == 1
        return max(abs(d - cos(x + 0.5)))
    end if
    return max(abs(d + sin(x + 0.5)))
end function
a1 = dmax(40, 1, \"periodic\")
a2 = dmax(80, 1, \"periodic\")
b1 = dmax(40, 2, \"periodic\")
b2 = dmax(80, 2, \"periodic\")
c1 = dmax(40, 1, \"none\")
c2 = dmax(80, 1, \"none\")
d1 = dmax(40, 2, \"none\")
d2 = dmax(80, 2, \"none\")
";
    let it = run(src);
    for (a, b, what) in [("a1", "a2", "order 1 periodic"), ("b1", "b2", "order 2 periodic"), ("c1", "c2", "order 1 one-sided ends"), ("d1", "d2", "order 2 one-sided ends")] {
        let s = slope(num(&it, a), num(&it, b));
        eprintln!("diffmat {what}: errors {:e} -> {:e}, slope {s:.3}", num(&it, a), num(&it, b));
        assert!((s - 2.0).abs() < 0.3, "{what}: slope {s}");
    }
    // and the absolute accuracy is the O(h^2) one, not a lucky zero
    assert!(num(&it, "a2") > 1e-5 && num(&it, "a2") < 1e-2);
}

#[test]
fn diffmat_dirichlet_is_the_textbook_tridiagonal() {
    let it = run("
import sparse
D = sparse.to_dense(sparse.diffmat(4, 0.5, order=2, bc=\"dirichlet\"))
G = sparse.to_dense(sparse.diffmat(4, 0.5, bc=\"dirichlet\"))
");
    let (_, _, d) = mat(&it, "D");
    for i in 0..4usize {
        for j in 0..4usize {
            let want = if i == j {
                -8.0
            } else if i.abs_diff(j) == 1 {
                4.0
            } else {
                0.0
            };
            assert!((d[i][j] - want).abs() < 1e-12, "D[{i}][{j}] = {}", d[i][j]);
        }
    }
    let (_, _, g) = mat(&it, "G");
    for i in 0..4usize {
        for j in 0..4usize {
            let want = if j == i + 1 {
                1.0
            } else if i == j + 1 {
                -1.0
            } else {
                0.0
            };
            assert!((g[i][j] - want).abs() < 1e-12, "G[{i}][{j}] = {}", g[i][j]);
        }
    }
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn laplacian_1d_spectra_match_theory() {
    let n = 12usize;
    let h = 0.3f64;
    let it = run(&format!(
        "
import sparse
ed = sort(eig(sparse.to_dense(sparse.laplacian({n}, {h}))).values)
en = sort(eig(sparse.to_dense(sparse.laplacian({n}, {h}, bc=\"neumann\"))).values)
ep = sort(eig(sparse.to_dense(sparse.laplacian({n}, {h}, bc=\"periodic\"))).values)
"
    ));
    let pi = std::f64::consts::PI;
    let dir = sorted((1..=n).map(|k| (2.0 - 2.0 * (k as f64 * pi / (n as f64 + 1.0)).cos()) / (h * h)).collect());
    let neu = sorted((0..n).map(|k| (2.0 - 2.0 * (k as f64 * pi / n as f64).cos()) / (h * h)).collect());
    let per = sorted((0..n).map(|k| (2.0 - 2.0 * (2.0 * pi * k as f64 / n as f64).cos()) / (h * h)).collect());
    let (ed, en, ep) = (nums(&it, "ed"), nums(&it, "en"), nums(&it, "ep"));
    eprintln!("laplacian eig errors: dirichlet {:e} neumann {:e} periodic {:e}", maxerr(&ed, &dir), maxerr(&en, &neu), maxerr(&ep, &per));
    assert!(maxerr(&ed, &dir) < 1e-9);
    assert!(maxerr(&en, &neu) < 1e-9);
    assert!(maxerr(&ep, &per) < 1e-9);
}

#[test]
fn laplacian_2d_ordering_and_spectrum() {
    // nx = 3, ny = 2, unknown (i, j) is number i + nx*j (x fastest)
    let (nx, ny, hx, hy) = (3usize, 2usize, 0.5f64, 0.25f64);
    let u: Vec<f64> = (0..nx * ny).map(|k| (k * k) as f64 + 1.0).collect();
    let it = run(&format!(
        "
import sparse
L = sparse.laplacian([{nx}, {ny}], [{hx}, {hy}])
Lu = sparse.mul(L, {})
ev = sort(eig(sparse.to_dense(L)).values)
",
        lit(&u)
    ));
    let at = |i: isize, j: isize| {
        if i < 0 || j < 0 || i >= nx as isize || j >= ny as isize {
            0.0
        } else {
            u[i as usize + nx * j as usize]
        }
    };
    let mut want = vec![];
    for j in 0..ny as isize {
        for i in 0..nx as isize {
            want.push(
                (2.0 * at(i, j) - at(i - 1, j) - at(i + 1, j)) / (hx * hx) + (2.0 * at(i, j) - at(i, j - 1) - at(i, j + 1)) / (hy * hy),
            );
        }
    }
    assert!(maxerr(&nums(&it, "Lu"), &want) < 1e-10, "{:?} vs {want:?}", nums(&it, "Lu"));
    // eigenvalues are sums of the 1-D ones
    let pi = std::f64::consts::PI;
    let l1 = |n: usize, h: f64, k: usize| (2.0 - 2.0 * (k as f64 * pi / (n as f64 + 1.0)).cos()) / (h * h);
    let mut ev = vec![];
    for a in 1..=nx {
        for b in 1..=ny {
            ev.push(l1(nx, hx, a) + l1(ny, hy, b));
        }
    }
    assert!(maxerr(&nums(&it, "ev"), &sorted(ev)) < 1e-9);
}

#[test]
fn poisson_2d_converges_at_second_order() {
    // -lap u = 2 pi^2 sin(pi x) sin(pi y) on the unit square, u = 0 on the edge
    let pi = std::f64::consts::PI;
    let solve = |n: usize| -> f64 {
        let h = 1.0 / (n as f64 + 1.0);
        let mut b = vec![0.0; n * n];
        let mut exact = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, y) = ((i + 1) as f64 * h, (j + 1) as f64 * h);
                exact[i + n * j] = (pi * x).sin() * (pi * y).sin();
                b[i + n * j] = 2.0 * pi * pi * exact[i + n * j];
            }
        }
        let it = run(&format!(
            "import sparse\nL = sparse.laplacian([{n}, {n}], {h:?})\nu = sparse.solve(L, {})\n",
            lit(&b)
        ));
        maxerr(&nums(&it, "u"), &exact)
    };
    let (e1, e2) = (solve(15), solve(31));
    let s = slope(e1, e2) / ((32.0f64 / 16.0).log2());
    eprintln!("poisson 2-D: max err {e1:e} (n=15), {e2:e} (n=31), slope {s:.3}");
    assert!(e1 < 2e-2 && e2 < 6e-3, "{e1} {e2}");
    assert!((s - 2.0).abs() < 0.15, "slope {s}");
}

#[test]
fn heat_equation_by_the_method_of_lines() {
    // u_t = u_xx on (0, 1), u = 0 at both ends, u(x, 0) = sin(pi x):
    // u = exp(-pi^2 t) sin(pi x)
    let n = 49usize;
    let it = run(&format!(
        "
import sparse
n = {n}
h = 1 / (n + 1)
x = linspace(h, 1 - h, n)
D2 = sparse.diffmat(n, h, order=2, bc=\"dirichlet\")
sol = ode_stiff((t, u) := sparse.mul(D2, u), [0, 0.1], sin(pi * x), rtol=1e-9, atol=1e-12)
st = sol.status
x_out = x
Y = sol.y
"
    ));
    assert_eq!(text(&it, "st"), "ok");
    let (rows, cols, y) = mat(&it, "Y");
    assert_eq!(cols, n);
    let last = &y[rows - 1];
    let pi = std::f64::consts::PI;
    let h = 1.0 / (n as f64 + 1.0);
    let xs: Vec<f64> = (1..=n).map(|i| i as f64 * h).collect();
    let t = 0.1;
    let analytic: Vec<f64> = xs.iter().map(|x| (-pi * pi * t).exp() * (pi * x).sin()).collect();
    // sin(pi x_i) is an exact eigenvector of the discrete operator
    let lam = -(2.0 - 2.0 * (pi * h).cos()) / (h * h);
    let discrete: Vec<f64> = xs.iter().map(|x| (lam * t).exp() * (pi * x).sin()).collect();
    let e_int = maxerr(last, &discrete);
    let e_pde = maxerr(last, &analytic);
    eprintln!("heat equation: error vs semi-discrete exact {e_int:e}, vs PDE solution {e_pde:e}");
    assert!(e_int < 1e-6, "{e_int}");
    assert!(e_pde < 5e-4, "{e_pde}");
}

#[test]
fn sparse_operator_errors() {
    let m = err("import sparse\nD = sparse.diffmat(2, 1, bc=\"periodic\")");
    assert!(m.contains("at least 3"), "{m}");
    let m = err("import sparse\nD = sparse.diffmat(10, 1, order=3)");
    assert!(m.contains("order must be 1 or 2"), "{m}");
    let m = err("import sparse\nD = sparse.diffmat(10, 0)");
    assert!(m.contains("positive"), "{m}");
    let m = err("import sparse\nD = sparse.diffmat(10, 1, bc=\"neumann\")");
    assert!(m.contains("unknown bc"), "{m}");
    let m = err("import sparse\nL = sparse.laplacian([3, 3, 3])");
    assert!(m.contains("shape"), "{m}");
    let m = err("import sparse\nL = sparse.laplacian(5, bc=\"none\")");
    assert!(m.contains("unknown bc"), "{m}");
    let m = err("import sparse\nL = sparse.laplacian([4, 4], [1, 2, 3])");
    assert!(m.contains("entries"), "{m}");
}

// ---------------------------------------------------------------- chebdiff

#[test]
fn chebdiff_is_spectrally_accurate() {
    let it = run("
c = chebdiff(16)
d = c.D * exp(c.x)
ex = exp(c.x)
d2 = c.D * (c.D * sin(c.x))
sx = sin(c.x)
x8 = chebdiff(8).x
d8 = chebdiff(8).D * exp(x8)
e8 = exp(x8)
x2 = chebdiff(2).x
");
    let e16 = maxerr(&nums(&it, "d"), &nums(&it, "ex"));
    let e8 = maxerr(&nums(&it, "d8"), &nums(&it, "e8"));
    eprintln!("chebdiff exp(x): n=8 error {e8:e}, n=16 error {e16:e}");
    assert!(e16 < 1e-9, "{e16}");
    assert!(e16 < e8 * 1e-4, "no spectral decay: {e8} -> {e16}");
    // second derivative of sin is -sin
    let neg_sin: Vec<f64> = nums(&it, "sx").iter().map(|v| -v).collect();
    assert!(maxerr(&nums(&it, "d2"), &neg_sin) < 1e-8);
    // nodes: cos(j pi/(n-1)), 1 down to -1
    assert_eq!(nums(&it, "x2"), vec![1.0, -1.0]);
}

#[test]
fn chebdiff_nodes_and_errors() {
    let it = run("c = chebdiff(5)\nx = c.x");
    let pi = std::f64::consts::PI;
    let want: Vec<f64> = (0..5).map(|j| (j as f64 * pi / 4.0).cos()).collect();
    assert!(maxerr(&nums(&it, "x"), &want) < 1e-15);
    assert!(err("c = chebdiff(1)").contains("from 2 to"));
    assert!(err("c = chebdiff(2.5)").contains("whole number"));
    assert!(err("c = chebdiff()").contains("number of points"));
}

// ---------------------------------------------------------------- integral2

#[test]
fn integral2_known_integrals() {
    let it = run("
a = integral2((x, y) := x * y, 0, 1, 0, 1)
b = integral2((x, y) := exp(-x^2 - y^2), -inf, inf, -inf, inf)
c = integral2((x, y) := 1, -1, 1, (x) := -sqrt(1 - x^2), (x) := sqrt(1 - x^2))
d = integral2((x, y) := x * y, 0, 1, 0, (x) := x)
d_rev = integral2((x, y) := x * y, 1, 0, 0, (x) := x)
s = integral2((x, y) := 1 / sqrt(x + y), 0, 1, 0, 1)
t = integral2((x, y) := 1 / sqrt(x * y), 0, 1, 0, 1, tol=1e-6)
g = integral2((x, y) := exp(-x^2 - y^2), 0, 5, 0, 5, tol=1e-10)
");
    let pi = std::f64::consts::PI;
    let ea = (num(&it, "a") - 0.25).abs();
    let eb = (num(&it, "b") - pi).abs();
    let ec = (num(&it, "c") - pi).abs();
    let ed = (num(&it, "d") - 0.125).abs();
    let es = (num(&it, "s") - 4.0 / 3.0 * (2.0 * 2f64.sqrt() - 2.0)).abs();
    let et = (num(&it, "t") - 4.0).abs();
    // exp(-x^2) integrates to sqrt(pi)/2 * erf(5) on [0, 5]: erf(5) = 1 - 1.5375e-12
    let eg = (num(&it, "g") - pi / 4.0).abs();
    eprintln!("integral2 errors: xy {ea:e}, plane {eb:e}, disc {ec:e}, triangle {ed:e}, (x+y)^-1/2 {es:e}, 1/sqrt(xy) {et:e}, box {eg:e}");
    assert!(ea < 1e-10);
    assert!(eb < 1e-7);
    assert!(ec < 1e-7);
    assert!(ed < 1e-9);
    assert!((num(&it, "d_rev") + 0.125).abs() < 1e-9, "reversed x limits negate");
    assert!(es < 1e-7);
    assert!(et < 1e-4);
    assert!(eg < 1e-9);
}

#[test]
fn integral2_evaluation_budget_is_an_error_not_a_hang() {
    let m = err("v = integral2((x, y) := x * y, 0, 1, 0, 1, max_evals=100)");
    assert!(m.contains("max_evals=100"), "{m}");
    // a wildly oscillating integrand cannot reach tol: bounded by the cap
    let m = err("v = integral2((x, y) := sin(1e7 / (x * y + 1e-9)), 0, 1, 0, 1, tol=1e-12, max_evals=20000)");
    assert!(m.contains("max_evals=20000") || m.contains("did not reach"), "{m}");
}

#[test]
fn integral2_input_errors() {
    let m = err("v = integral2((x, y) := x * y, 0, 1, 0)");
    assert!(m.contains("at least 5"), "{m}");
    let m = err("v = integral2((x, y) := x * y, 0, 1, \"a\", 1)");
    assert!(m.contains("a"), "{m}");
    let m = err("v = integral2((x, y) := x * y, 0, 1, 0, 1, tolerance=1e-3)");
    assert!(m.contains("tolerance"), "{m}");
    let m = err("v = integral2((x, y) := x * y, 0, 1, 0, 1, tol=-1)");
    assert!(m.contains("tol must be a positive number"), "{m}");
    let m = err("function f(x, y)\n  error(\"boom\")\nend function\nv = integral2(f, 0, 1, 0, 1)");
    assert!(m.contains("boom"), "{m}");
    let m = err("v = integral2((x, y) := 1 / (x - 0.5), 0, 1, 0, 1)");
    assert!(m.contains("integral2") || m.contains("finite"), "{m}");
}

// ---------------------------------------------------------------- fminbnd

#[test]
fn fminbnd_finds_known_minima() {
    let it = run("
a = fminbnd((x) := (x - 2)^2, 0, 5)
b = fminbnd((x) := cos(x), 3, 4)
c = fminbnd((x) := x, 1, 3)
d = fminbnd((x) := (x - 2)^2, 0, 5, tol=1e-12, max_iter=200)
pa = a.params
va = a.value
ca = a.converged
pb = b.params
pc = c.params
pd = d.params
cc = c.converged
na = a.iterations
");
    let pi = std::f64::consts::PI;
    eprintln!(
        "fminbnd: (x-2)^2 -> {:.3e} off, cos on [3,4] -> {:.3e} off, boundary x -> {:.3e} above 1",
        (num(&it, "pa") - 2.0).abs(),
        (num(&it, "pb") - pi).abs(),
        num(&it, "pc") - 1.0
    );
    assert!((num(&it, "pa") - 2.0).abs() < 1e-7);
    assert!(num(&it, "va") < 1e-13);
    assert!(flag(&it, "ca"));
    assert!((num(&it, "pb") - pi).abs() < 1e-6);
    assert!(num(&it, "pc") >= 1.0 && num(&it, "pc") < 1.0 + 1e-6, "{}", num(&it, "pc"));
    assert!(flag(&it, "cc"));
    assert!((num(&it, "pd") - 2.0).abs() < 1e-7);
    assert!(num(&it, "na") < 60.0);
}

#[test]
fn fminbnd_reports_non_convergence_and_errors() {
    let it = run("r = fminbnd((x) := (x - 2)^2, 0, 5, max_iter=3)\nc = r.converged");
    assert!(!flag(&it, "c"));
    let m = err("r = fminbnd((x) := x^2, 5, 0)");
    assert!(m.contains("a must be less than b"), "{m}");
    let m = err("r = fminbnd((x) := x^2, 1, 1)");
    assert!(m.contains("a must be less than b"), "{m}");
    let m = err("r = fminbnd((x) := inf, 0, 2)");
    assert!(m.contains("must be finite"), "{m}");
    let m = err("function f(x)\n  error(\"boom\")\nend function\nr = fminbnd(f, 0, 1)");
    assert!(m.contains("boom"), "{m}");
    let m = err("r = fminbnd((x) := x^2, 0, 1, tolerance=1e-3)");
    assert!(m.contains("tolerance"), "{m}");
    let m = err("r = fminbnd((x) := x^2, -inf, 1)");
    assert!(m.contains("finite"), "{m}");
}
