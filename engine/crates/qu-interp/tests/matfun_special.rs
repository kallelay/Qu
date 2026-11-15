//! Matrix functions (`schur`, `sqrtm`, `logm`, `sylvester`, `lyap`) and special
//! functions (`erfinv`, `erfcinv`, `gammainc`, `betainc`, `beta`, `besselj`,
//! `bessely`, `besseli`, `besselk`, `expint`, `ellipke`, `legendre`, `zeta`),
//! through the language (v0.4.14).
//!
//! Expected values are independent of the code under test: identities that must
//! hold (`sqrtm(A)^2 = A`, `expm(logm(A)) = A`, `U T U' = A`, equation residuals,
//! Wronskians, recurrences, closed forms) and reference numbers computed with
//! mpmath at 40 digits.

use qu_core::matrix::Matrix;
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

fn mat(it: &Interp, n: &str) -> Matrix {
    match it.get(n) {
        Some(Value::Mat(m)) => m.as_ref().clone(),
        Some(Value::Num(x)) => Matrix::from_col_major(1, 1, vec![*x]),
        other => panic!("`{n}` is not a matrix: {other:?}"),
    }
}

fn rel(a: f64, b: f64) -> f64 {
    if a == b {
        0.0
    } else {
        (a - b).abs() / b.abs().max(1e-300)
    }
}

fn close_rel(name: &str, got: f64, want: f64, tol: f64) {
    assert!(rel(got, want) <= tol, "{name}: got {got:e}, want {want:e}, rel {:e}", rel(got, want));
}

fn maxdiff(a: &Matrix, b: &Matrix) -> f64 {
    assert_eq!(a.shape(), b.shape(), "shape");
    a.as_slice().iter().zip(b.as_slice()).fold(0.0, |s, (x, y)| s.max((x - y).abs()))
}

fn mm(a: &Matrix, b: &Matrix) -> Matrix {
    a.matmul(b).unwrap()
}

// ------------------------------------------------------------ matrix functions

#[test]
fn schur_reconstructs_and_is_quasi_triangular() {
    let it = run(
        "A = [4, 1, 2, 0; 3, 5, 1, 1; 0, 2, 6, 3; 1, 0, 1, 2]\n\
         s = schur(A)\nT = s.T\nU = s.U",
    );
    let (a, t, u) = (mat(&it, "A"), mat(&it, "T"), mat(&it, "U"));
    assert!(maxdiff(&mm(&mm(&u, &t), &u.transpose()), &a) < 1e-12);
    assert!(maxdiff(&mm(&u.transpose(), &u), &Matrix::eye(4)) < 1e-13);
    for j in 0..4 {
        for i in (j + 2)..4 {
            assert_eq!(t.get(i, j).unwrap(), 0.0, "T[{i},{j}] below the first subdiagonal");
        }
    }
}

#[test]
fn schur_of_a_rotation_has_a_two_by_two_block() {
    let it = run("A = [0, -1, 0; 1, 0, 0; 0, 0, 2]\ns = schur(A)\nT = s.T\nU = s.U");
    let (a, t, u) = (mat(&it, "A"), mat(&it, "T"), mat(&it, "U"));
    assert!(maxdiff(&mm(&mm(&u, &t), &u.transpose()), &a) < 1e-14);
    // the complex pair +-i cannot be triangularised over the reals
    let sub = t.get(1, 0).unwrap().abs().max(t.get(2, 1).unwrap().abs());
    assert!(sub > 0.5, "expected a 2x2 block, subdiagonal {sub}");
}

#[test]
fn sqrtm_squares_back_for_symmetric_and_general_matrices() {
    let it = run(
        "A = [4, 1; 1, 3]\nR = sqrtm(A)\nRR = R * R\n\
         G = [3, 1, -2; 0.5, 4, 1; 2, -1, 5]\nS = sqrtm(G)\nSS = S * S\n\
         Z = [2, -3; 3, 2]\nW = sqrtm(Z)\nWW = W * W",
    );
    assert!(maxdiff(&mat(&it, "RR"), &mat(&it, "A")) < 1e-13);
    assert!(maxdiff(&mat(&it, "SS"), &mat(&it, "G")) < 1e-12);
    // complex-pair eigenvalues 2 +- 3i: the root is still real
    assert!(maxdiff(&mat(&it, "WW"), &mat(&it, "Z")) < 1e-13);
}

#[test]
fn sqrtm_known_roots() {
    let it = run("R = sqrtm([4, 0; 0, 9])\nN = sqrtm([1, 1; 0, 1])\nO = sqrtm([0, 0; 0, 0])");
    let r = mat(&it, "R");
    assert!(maxdiff(&r, &Matrix::from_rows(&[vec![2.0, 0.0], vec![0.0, 3.0]]).unwrap()) < 1e-14);
    let n = mat(&it, "N");
    assert!(maxdiff(&n, &Matrix::from_rows(&[vec![1.0, 0.5], vec![0.0, 1.0]]).unwrap()) < 1e-14);
    assert!(mat(&it, "O").as_slice().iter().all(|x| *x == 0.0));
    // singular but diagonalisable: [1 1; 1 1] = 2 P has the root sqrt(2) P and stays real
    let it = run("R = sqrtm([1, 1; 1, 1])");
    assert!(matches!(it.get("R"), Some(Value::Mat(_))));
    assert!(maxdiff(&mat(&it, "R"), &Matrix::filled(2, 2, 0.5 * 2f64.sqrt())) < 1e-12);
}

#[test]
fn sqrtm_with_a_negative_eigenvalue_is_complex() {
    let it = run("R = sqrtm([-4, 0; 0, 1])\nRR = R * R\nre = real(RR)\nim = imag(RR)");
    assert!(matches!(it.get("R"), Some(Value::CMat(_))), "expected a complex matrix");
    let re = mat(&it, "re");
    let want = Matrix::from_rows(&[vec![-4.0, 0.0], vec![0.0, 1.0]]).unwrap();
    assert!(maxdiff(&re, &want) < 1e-14);
    assert!(mat(&it, "im").as_slice().iter().all(|x| x.abs() < 1e-14));
}

#[test]
fn sqrtm_of_a_nilpotent_matrix_is_an_error() {
    assert!(err("R = sqrtm([0, 1; 0, 0])").contains("no square root"));
}

#[test]
fn logm_inverts_expm_over_a_wide_spread_of_eigenvalues() {
    // eigenvalues from 1e-3 to 1e3: many square roots are needed before Pade applies
    let it = run(
        "A = [1000, 5, 1; 0, 1, 2; 0, 0, 0.001]\nL = logm(A)\nB = expm(L)\n\
         G = [2, 1, 0, -1; 0.5, 3, 1, 0; 0, 1, 2.5, 1; 1, 0, 0.3, 4]\nM = logm(G)\nH = expm(M)\n\
         Z = [1, -2; 2, 1]\nK = logm(Z)\nZb = expm(K)",
    );
    let (a, b) = (mat(&it, "A"), mat(&it, "B"));
    for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
        assert!((x - y).abs() <= 1e-9 * x.abs().max(1.0), "expm(logm(A)): {x} vs {y}");
    }
    assert!(maxdiff(&mat(&it, "G"), &mat(&it, "H")) < 1e-11);
    assert!(maxdiff(&mat(&it, "Z"), &mat(&it, "Zb")) < 1e-12);
}

#[test]
fn logm_known_values() {
    let it = run("L = logm([1, 1; 0, 1])\nI = logm([1, 0; 0, 1])\nE = logm([2.718281828459045, 0; 0, 1])");
    assert!(maxdiff(&mat(&it, "L"), &Matrix::from_rows(&[vec![0.0, 1.0], vec![0.0, 0.0]]).unwrap()) < 1e-13);
    assert!(mat(&it, "I").as_slice().iter().all(|x| x.abs() < 1e-15));
    assert!(maxdiff(&mat(&it, "E"), &Matrix::from_rows(&[vec![1.0, 0.0], vec![0.0, 0.0]]).unwrap()) < 1e-14);
}

#[test]
fn logm_of_a_singular_matrix_is_an_error_and_negative_gives_complex() {
    assert!(err("L = logm([1, 0; 0, 0])").contains("singular"));
    let it = run("L = logm([-1, 0; 0, 1])\nim = imag(L)");
    assert!(matches!(it.get("L"), Some(Value::CMat(_))));
    close_rel("imag log(-1)", mat(&it, "im").get(0, 0).unwrap(), std::f64::consts::PI, 1e-14);
}

#[test]
fn sylvester_has_a_zero_residual() {
    let it = run(
        "A = [4, 1, 0; 0.5, 5, 1; 0, 1, 6]\nB = [3, -1; 2, 4]\nC = [1, 2; 3, 4; 5, 6]\n\
         X = sylvester(A, B, C)\nR = A * X + X * B - C",
    );
    assert!(it.get("X").is_some());
    assert!(mat(&it, "R").as_slice().iter().all(|x| x.abs() < 1e-12), "residual");
    // 1x1: 2x + 3x = 10
    let it = run("x = sylvester(2, 3, 10)");
    close_rel("2x+3x=10", num(&it, "x"), 2.0, 1e-15);
}

#[test]
fn lyap_solves_the_lyapunov_equation_and_is_symmetric_psd() {
    let it = run(
        "A = [-2, 1, 0; 0.5, -3, 1; 0, 0.2, -1]\nQ = [2, 0.5, 0; 0.5, 1, 0; 0, 0, 3]\n\
         X = lyap(A, Q)\nR = A * X + X * A' + Q\nD = X - X'\nd = [X[0,0], X[1,1], X[2,2]]",
    );
    assert!(mat(&it, "R").as_slice().iter().all(|x| x.abs() < 1e-12), "residual");
    assert!(mat(&it, "D").as_slice().iter().all(|x| x.abs() < 1e-12), "symmetry");
    assert!(nums(&it, "d").iter().all(|x| *x > 0.0));
    // scalar: 2 a x + q = 0
    let it = run("x = lyap(-3, 6)");
    close_rel("lyap scalar", num(&it, "x"), 1.0, 1e-15);
}

#[test]
fn matrix_function_errors_are_clear() {
    assert!(err("x = schur([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(err("x = sqrtm([1, nan; 0, 1])").contains("NaN"));
    assert!(err("x = logm([1+2j, 0; 0, 1])").contains("real"));
    assert!(err("x = schur()").contains("missing argument"));
    assert!(err("x = sylvester([1], [1])").contains("missing argument"));
    assert!(err("x = sylvester([1, 0; 0, 1], [1], [1, 2, 3])").contains("2x1"));
    assert!(err("x = sylvester(1, -1, 1)").contains("eigenvalue"));
    assert!(err("x = lyap([0, 1; -1, 0], [1, 0; 0, 1])").contains("l1 + l2 = 0"));
    assert!(err("x = lyap([-1, 0; 0, -2], [1])").contains("Q must be 2x2"));
    // an unknown keyword or an extra argument is an error, not ignored
    assert!(!err("x = schur([1, 2; 3, 4], foo=1)").is_empty());
    assert!(!err("x = sqrtm([1, 2; 3, 4], 2)").is_empty());
}

// ------------------------------------------------------------ erfinv & friends

#[test]
fn erfinv_and_erfcinv_invert_erf_and_erfc() {
    let it = run(
        "x = [-2, -0.5, -1e-6, 0, 1e-9, 0.3, 1, 2.2]\ny = erf(x)\nback = erfinv(y)\n\
         t = [0.1, 1, 4, 10, 20]\nz = erfc(t)\nbackc = erfcinv(z)\n\
         e1 = erfinv(1)\ne2 = erfinv(-1)\ne3 = erfinv(2)\nc0 = erfcinv(0)\nc2 = erfcinv(2)\nc3 = erfcinv(-1)",
    );
    let (x, back) = (nums(&it, "x"), nums(&it, "back"));
    for (a, b) in x.iter().zip(&back) {
        assert!((a - b).abs() <= 1e-13 * a.abs().max(1e-3), "erfinv(erf({a})) = {b}");
    }
    let (t, bc) = (nums(&it, "t"), nums(&it, "backc"));
    for (a, b) in t.iter().zip(&bc) {
        close_rel("erfcinv(erfc(t))", *b, *a, 1e-13);
    }
    assert_eq!(num(&it, "e1"), f64::INFINITY);
    assert_eq!(num(&it, "e2"), f64::NEG_INFINITY);
    assert!(num(&it, "e3").is_nan() && num(&it, "c3").is_nan());
    assert_eq!(num(&it, "c0"), f64::INFINITY);
    assert_eq!(num(&it, "c2"), f64::NEG_INFINITY);
    // mpmath: erfinv(0.5) = 0.4769362762044698733...
    let it = run("v = erfinv(0.5)");
    close_rel("erfinv(.5)", num(&it, "v"), 0.476_936_276_204_469_9, 1e-15);
}

#[test]
fn erfinv_broadcasts_like_erf() {
    let it = run("M = erfinv([0, 0.5; -0.5, 0.9])\nr = size(M)");
    let m = mat(&it, "M");
    assert_eq!(m.shape(), (2, 2));
    close_rel("M[1,1]", m.get(1, 1).unwrap(), 1.163_087_153_676_674_3, 1e-14);
    assert!(err("x = erfinv(1+2j)").contains("real"));
    assert!(err("x = erfinv()").contains("at least one argument") || err("x = erfinv()").contains("missing"));
}

// ------------------------------------------------------------ incomplete gamma / beta

#[test]
fn gammainc_closed_forms_and_tails() {
    let it = run(
        "x = [1e-6, 0.1, 1, 3, 10, 40]\n\
         p1 = gammainc(x, 1)\nwant1 = -expm1(-x)\n\
         ph = gammainc(x, 0.5)\nwanth = erf(sqrt(x))\n\
         q1 = gammainc(x, 1, \"upper\")\nwantq1 = exp(-x)\n\
         s = gammainc(x, 2.5) + gammainc(x, 2.5, \"upper\")",
    );
    for (n, w) in [("p1", "want1"), ("ph", "wanth"), ("q1", "wantq1")] {
        let (g, e) = (nums(&it, n), nums(&it, w));
        for (a, b) in g.iter().zip(&e) {
            assert!(rel(*a, *b) < 1e-12, "{n}: {a} vs {b}");
        }
    }
    for v in nums(&it, "s") {
        assert!((v - 1.0).abs() < 1e-14);
    }
}

#[test]
fn gammainc_against_mpmath_including_large_shape() {
    let it = run(
        "a = gammainc(100, 100)\nb = gammainc(90, 100)\nc = gammainc(1000, 1000, \"upper\")\n\
         d = gammainc(30, 2.5, \"upper\")\ne = gammainc(0.5, 0.1)\nf = gammainc(1e5, 1e5)",
    );
    close_rel("P(100,100)", num(&it, "a"), 0.5132987982791487, 1e-12);
    close_rel("P(100,90)", num(&it, "b"), 0.15822098918643016, 1e-12);
    close_rel("Q(1000,1000)", num(&it, "c"), 0.4957947558197845, 1e-12);
    close_rel("Q(2.5,30)", num(&it, "d"), 1.215456977718304e-11, 1e-12);
    close_rel("P(0.1,0.5)", num(&it, "e"), 0.9414024458901336, 1e-12);
    close_rel("P(1e5,1e5)", num(&it, "f"), 0.5004205221103651, 1e-12);
}

#[test]
fn gammainc_edge_cases_and_errors() {
    let it = run("a = gammainc(0, 3)\nb = gammainc(inf, 3)\nc = gammainc(-1, 3)\nd = gammainc(1, -2)\ne = gammainc(2, 0)");
    assert_eq!(num(&it, "a"), 0.0);
    assert_eq!(num(&it, "b"), 1.0);
    assert!(num(&it, "c").is_nan() && num(&it, "d").is_nan());
    assert_eq!(num(&it, "e"), 1.0);
    assert!(err("x = gammainc(1, 2, \"middle\")").contains("\"lower\" or \"upper\""));
    assert!(err("x = gammainc(1, 2, 3)").contains("lower"));
    assert!(err("x = gammainc(1)").contains("missing argument"));
    assert!(err("x = gammainc([1, 2, 3], [1, 2])").contains("same shape"));
    assert!(err("x = gammainc(1+1j, 2)").contains("real"));
}

#[test]
fn betainc_identities_and_reference_values() {
    let it = run(
        "x = [0.05, 0.3, 0.5, 0.9]\nlin = betainc(x, 1, 1)\n\
         l = betainc(x, 2.5, 4)\nr = 1 - betainc(1 - x, 4, 2.5)\n\
         a = betainc(0.2, 0.5, 0.5)\nb = betainc(0.3, 30, 40)\nc = betainc(0.4, 1000, 1500)\n\
         d = betainc(0.001, 0.2, 5)",
    );
    let (x, lin) = (nums(&it, "x"), nums(&it, "lin"));
    for (p, q) in x.iter().zip(&lin) {
        close_rel("I_x(1,1)", *q, *p, 1e-14);
    }
    for (p, q) in nums(&it, "l").iter().zip(&nums(&it, "r")) {
        assert!((p - q).abs() < 1e-14, "symmetry {p} {q}");
    }
    close_rel("I(.2;.5,.5)", num(&it, "a"), 0.2951672353008666, 1e-13);
    close_rel("I(.3;30,40)", num(&it, "b"), 0.012126160623329713, 1e-13);
    close_rel("I(.4;1000,1500)", num(&it, "c"), 0.5010858488742157, 1e-12);
    close_rel("I(.001;.2,5)", num(&it, "d"), 0.3711099209733869, 1e-13);
}

#[test]
fn betainc_and_beta_domain_and_values() {
    let it = run(
        "a = betainc(0, 2, 3)\nb = betainc(1, 2, 3)\nc = betainc(1.5, 2, 3)\nd = betainc(0.5, -1, 3)\n\
         e = beta(2, 3)\nf = beta(0.5, 0.5)\ng = beta([1, 2, 3], 4)\nh = beta(100, 100)\ni = beta(2, 1e6)",
    );
    assert_eq!(num(&it, "a"), 0.0);
    assert_eq!(num(&it, "b"), 1.0);
    assert!(num(&it, "c").is_nan() && num(&it, "d").is_nan());
    close_rel("B(2,3)", num(&it, "e"), 1.0 / 12.0, 1e-15);
    close_rel("B(.5,.5)", num(&it, "f"), std::f64::consts::PI, 1e-15);
    let g = nums(&it, "g");
    close_rel("B(1,4)", g[0], 0.25, 1e-15);
    close_rel("B(2,4)", g[1], 1.0 / 20.0, 1e-15);
    close_rel("B(3,4)", g[2], 1.0 / 60.0, 1e-15);
    close_rel("B(100,100)", num(&it, "h"), 2.2087606931995024e-61, 1e-13);
    close_rel("B(2,1e6)", num(&it, "i"), 1.0 / (1e6 * (1e6 + 1.0)), 1e-13);
    assert!(err("x = betainc(0.5, 1)").contains("missing argument"));
    assert!(err("x = beta(1)").contains("missing argument"));
}

// ------------------------------------------------------------ Bessel

#[test]
fn bessel_published_values() {
    let it = run(
        "j0 = besselj(0, 1)\nj1 = besselj(1, 1)\ny0 = bessely(0, 1)\ni0 = besseli(0, 1)\nk0 = besselk(0, 1)\n\
         k1 = besselk(1, 1)\ny1 = bessely(1, 1)\ni1 = besseli(1, 1)",
    );
    close_rel("J0(1)", num(&it, "j0"), 0.7651976865579666, 1e-14);
    close_rel("J1(1)", num(&it, "j1"), 0.4400505857449335, 1e-14);
    close_rel("Y0(1)", num(&it, "y0"), 0.08825696421567697, 1e-14);
    close_rel("I0(1)", num(&it, "i0"), 1.2660658777520082, 1e-14);
    close_rel("K0(1)", num(&it, "k0"), 0.42102443824070834, 1e-14);
    close_rel("K1(1)", num(&it, "k1"), 0.6019072301972346, 1e-14);
    close_rel("Y1(1)", num(&it, "y1"), -0.7812128213002887, 1e-14);
    close_rel("I1(1)", num(&it, "i1"), 0.5651591039924851, 1e-14);
}

#[test]
fn bessel_against_mpmath_over_order_and_argument() {
    // (nu, x, J, Y, I, K) from mpmath, 40 digits
    let cases: &[(f64, f64, f64, f64, f64, f64)] = &[
        (0.0, 3.7, -0.39923020337119114, 0.1060743153203541, 8.738617524169397, 0.01563065992162666),
        (0.0, 100.0, 0.019985850304223122, -0.07724431336508315, 1.0737517071310738e+42, 4.656628229175902e-45),
        (0.0, 1000000.0, 0.00033104301373987376, -0.0007259685223351791, f64::INFINITY, 0.0),
        (1.0, 0.001, 0.0004999999375000026, -636.6221672311394, 0.0005000000625000026, 999.9962381560856),
        (5.0, 0.5, 8.053627241357474e-06, -7946.301478807473, 8.223171313109265e-06, 12097.979476096394),
        (5.0, 200.0, -0.055132678944014676, 0.012019640832200107, 1.915814101523687e+85, 1.3045247397975134e-88),
        (20.0, 20.0, 0.16474777377532654, -0.28548945860020347, 3188.750328853615, 5.543111636125816e-06),
        (50.0, 30.0, 2.0581656631564178e-08, -386759.32602734736, 0.00014590106916468948, 58.77068625800724),
        (0.3, 1.2, 0.7111886464520507, -0.08022932823167017, 1.2421327157959323, 0.32769323123536503),
        (2.5, 7.0, -0.2834366512016992, 0.12852374780895653, 104.61336757234871, 0.0006435411544813076),
        (12.7, 0.6, 7.942880407027284e-17, -315905105165984.4, 8.047928015397373e-17, 488646807001704.8),
        (7.5, 100000.0, -0.00252154491487329, 8.94929258534912e-05, f64::INFINITY, 0.0),
    ];
    for &(nu, x, j, y, i, k) in cases {
        let it = run(&format!(
            "nu = {nu:e}\nx = {x:e}\nj = besselj(nu, x)\ny = bessely(nu, x)\ni = besseli(nu, x)\nk = besselk(nu, x)"
        ));
        for (n, want) in [("j", j), ("y", y), ("i", i), ("k", k)] {
            let got = num(&it, n);
            if want.is_infinite() || want == 0.0 {
                assert_eq!(got, want, "{n}({nu},{x})");
            } else {
                assert!(rel(got, want) < 1e-11, "{n}_{nu}({x}): got {got:e}, want {want:e}");
            }
        }
    }
}

#[test]
fn bessel_half_integer_orders_have_closed_forms() {
    let it = run(
        "x = [0.1, 1, 2.5, 7, 30, 120]\n\
         j = besselj(0.5, x)\nwj = sqrt(2 ./ (pi * x)) .* sin(x)\n\
         y = bessely(0.5, x)\nwy = -sqrt(2 ./ (pi * x)) .* cos(x)\n\
         i = besseli(0.5, x)\nwi = sqrt(2 ./ (pi * x)) .* sinh(x)\n\
         k = besselk(0.5, x)\nwk = sqrt(pi ./ (2 * x)) .* exp(-x)",
    );
    let c = |n: &str| nums(&it, n);
    let x = c("x");
    for (n, w, tol_abs) in [("j", "wj", true), ("y", "wy", true), ("i", "wi", false), ("k", "wk", false)] {
        for ((g, e), xv) in c(n).iter().zip(c(w)).zip(&x) {
            if tol_abs {
                assert!((g - e).abs() < 3e-14 * (2.0 / (std::f64::consts::PI * xv)).sqrt(), "{n}({xv}): {g} vs {e}");
            } else {
                assert!(rel(*g, e) < 3e-14, "{n}({xv}): {g} vs {e}");
            }
        }
    }
}

#[test]
fn bessel_wronskian_and_recurrence_hold_for_vectors() {
    let it = run(
        "x = [0.3, 1.5, 4, 10, 24, 26, 90, 600]\nnu = 2.5\n\
         w = besselj(nu + 1, x) .* bessely(nu, x) - besselj(nu, x) .* bessely(nu + 1, x)\nwt = 2 ./ (pi * x)\n\
         wi = besseli(nu, x) .* besselk(nu + 1, x) + besseli(nu + 1, x) .* besselk(nu, x)\nwit = 1 ./ x\n\
         rec = besselj(nu - 1, x) + besselj(nu + 1, x)\nrhs = 2 * nu ./ x .* besselj(nu, x)",
    );
    for (a, b) in nums(&it, "w").iter().zip(&nums(&it, "wt")) {
        assert!(rel(*a, *b) < 1e-11, "Wronskian {a} vs {b}");
    }
    for (a, b) in nums(&it, "wi").iter().zip(&nums(&it, "wit")) {
        assert!(rel(*a, *b) < 1e-11, "I/K Wronskian {a} vs {b}");
    }
    for (a, b) in nums(&it, "rec").iter().zip(&nums(&it, "rhs")) {
        assert!((a - b).abs() < 1e-12, "recurrence {a} vs {b}");
    }
}

#[test]
fn bessel_extremes_and_special_arguments() {
    let it = run(
        "a = besselj(0, 0)\nb = besselj(3, 0)\nc = bessely(0, 0)\nd = besseli(0, 0)\ne = besselk(2, 0)\n\
         f = besselj(1, -2)\ng = besselj(0.5, -2)\nh = bessely(0, -1)\ni = besselj(0, inf)\nj = besselk(1, inf)\n\
         k = besselj(-2, 3)\nl = besselj(2, 3)\nm = besselj(-0.5, 2.3)\nn = sqrt(2 / (pi * 2.3)) * cos(2.3)\n\
         o = besselj(500, 1)\np = besselj(0, 1e12)\nq = besselj(nan, 1)\nr = besselk(0, 800)\ns = besseli(0, 800)",
    );
    assert_eq!(num(&it, "a"), 1.0);
    assert_eq!(num(&it, "b"), 0.0);
    assert_eq!(num(&it, "c"), f64::NEG_INFINITY);
    assert_eq!(num(&it, "d"), 1.0);
    assert_eq!(num(&it, "e"), f64::INFINITY);
    close_rel("J1(-2)", num(&it, "f"), -0.5767248077568734, 1e-14);
    assert!(num(&it, "g").is_nan() && num(&it, "h").is_nan());
    assert_eq!(num(&it, "i"), 0.0);
    assert_eq!(num(&it, "j"), 0.0);
    close_rel("J-2 = J2", num(&it, "k"), num(&it, "l"), 1e-15);
    close_rel("J-1/2", num(&it, "m"), num(&it, "n"), 1e-13);
    assert_eq!(num(&it, "o"), 0.0);
    assert!(num(&it, "p").abs() < 1e-5);
    assert!(num(&it, "q").is_nan());
    assert_eq!(num(&it, "r"), 0.0);
    assert_eq!(num(&it, "s"), f64::INFINITY);
}

#[test]
fn bessel_broadcast_shapes_and_errors() {
    let it = run(
        "a = besselj([0, 1, 2], 3)\nb = besselj(1, [1, 2; 3, 4])\nc = besselj([0, 1, 2], [3, 4, 5])\nd = besselj(0, 1)",
    );
    assert_eq!(nums(&it, "a").len(), 3);
    close_rel("J1(1)", nums(&it, "b")[0], 0.4400505857449335, 1e-14);
    assert_eq!(mat(&it, "b").shape(), (2, 2));
    assert_eq!(nums(&it, "c").len(), 3);
    assert!(err("x = besselj([0, 1], [1, 2, 3])").contains("same shape"));
    assert!(err("x = besselj(1)").contains("missing argument"));
    assert!(err("x = besselk(1+1j, 2)").contains("real"));
    assert!(err("x = bessely(\"a\", 2)").contains("number"));
}

// ------------------------------------------------------------ other special functions

#[test]
fn expint_values_and_domain() {
    let it = run(
        "a = expint(1)\nb = expint(0.5)\nc = expint([1e-5, 0.3, 1.5, 5, 20, 50, 300])\nd = expint(0)\ne = expint(-1)\nf = expint(inf)",
    );
    close_rel("E1(1)", num(&it, "a"), 0.21938393439552029, 1e-14);
    close_rel("E1(.5)", num(&it, "b"), 0.5597735947761608, 1e-14);
    let want = [10.935719800043696, 0.9056766516758468, 0.10001958240663265, 0.0011482955912753257, 9.835525290649882e-11, 3.783264029550459e-24, 1.71038427680451e-133];
    for (g, w) in nums(&it, "c").iter().zip(want) {
        close_rel("E1", *g, w, 1e-13);
    }
    assert_eq!(num(&it, "d"), f64::INFINITY);
    assert!(num(&it, "e").is_nan());
    assert_eq!(num(&it, "f"), 0.0);
}

#[test]
fn ellipke_returns_k_and_e() {
    let it = run(
        "r = ellipke(0)\nk0 = r.K\ne0 = r.E\nq = ellipke(0.5)\nk5 = q.K\ne5 = q.E\n\
         v = ellipke([-3, 0.2, 0.9])\nvk = v.K\nve = v.E\nu = ellipke(1)\nuk = u.K\nue = u.E\nw = ellipke(2)\nwk = w.K",
    );
    close_rel("K(0)", num(&it, "k0"), std::f64::consts::FRAC_PI_2, 1e-15);
    close_rel("E(0)", num(&it, "e0"), std::f64::consts::FRAC_PI_2, 1e-15);
    close_rel("K(.5)", num(&it, "k5"), 1.8540746773013719, 1e-15);
    close_rel("E(.5)", num(&it, "e5"), 1.3506438810476755, 2e-14);
    let (vk, ve) = (nums(&it, "vk"), nums(&it, "ve"));
    for (g, w) in vk.iter().zip([1.0782578237498217, 1.659623598610528, 2.5780921133481733]) {
        close_rel("K", *g, w, 1e-14);
    }
    for (g, w) in ve.iter().zip([2.422112055136919, 1.489035058095853, 1.1047747327040733]) {
        close_rel("E", *g, w, 1e-14);
    }
    assert_eq!(num(&it, "uk"), f64::INFINITY);
    assert_eq!(num(&it, "ue"), 1.0);
    assert!(num(&it, "wk").is_nan());
}

#[test]
fn legendre_matches_closed_forms_and_shapes() {
    let it = run(
        "p = legendre(2, 0.5)\nq = legendre(3, [0.2, 0.5, -0.7])\nz = legendre(0, 0.3)\nedge = legendre(4, 1)\nm1 = legendre(4, -1)",
    );
    let p = nums(&it, "p");
    let x = 0.5f64;
    let s = (1.0 - x * x).sqrt();
    assert_eq!(p.len(), 3);
    close_rel("P20", p[0], 0.5 * (3.0 * x * x - 1.0), 1e-15);
    close_rel("P21", p[1], -3.0 * x * s, 1e-15); // Condon-Shortley phase
    close_rel("P22", p[2], 3.0 * (1.0 - x * x), 1e-15);
    let q = mat(&it, "q");
    assert_eq!(q.shape(), (4, 3), "(n+1) x length(x)");
    for (c, xv) in [0.2f64, 0.5, -0.7].iter().enumerate() {
        let s = (1.0 - xv * xv).sqrt();
        close_rel("P30", q.get(0, c).unwrap(), 0.5 * (5.0 * xv.powi(3) - 3.0 * xv), 1e-14);
        close_rel("P31", q.get(1, c).unwrap(), -1.5 * (5.0 * xv * xv - 1.0) * s, 1e-14);
        close_rel("P32", q.get(2, c).unwrap(), 15.0 * xv * (1.0 - xv * xv), 1e-14);
        close_rel("P33", q.get(3, c).unwrap(), -15.0 * s.powi(3), 1e-14);
    }
    assert_eq!(nums(&it, "z"), vec![1.0]);
    let e = nums(&it, "edge");
    assert_eq!(e[0], 1.0);
    assert!(e[1..].iter().all(|v| *v == 0.0));
    assert_eq!(nums(&it, "m1")[0], 1.0);
    // large degree against mpmath: P_40^m(0.9)
    let it = run("v = legendre(40, 0.9)");
    let v = nums(&it, "v");
    close_rel("P40^0", v[0], 0.036987418421876216, 1e-12);
    close_rel("P40^1", v[1], 7.506933008812437, 1e-12);
    close_rel("P40^2", v[2], -91.65912002788629, 1e-12);
    close_rel("P40^16", v[16], 8.916098989362137e+24, 1e-12);
    close_rel("P40^20", v[20], 2.923831045243681e+30, 1e-12);
    close_rel("P40^40", v[40], 2.998850715296857e+44, 1e-12);
}

#[test]
fn legendre_errors() {
    assert!(err("x = legendre(2, 1.5)").contains("[-1, 1]"));
    assert!(err("x = legendre(-1, 0.5)").contains("non-negative"));
    assert!(err("x = legendre(1.5, 0.5)").contains("whole number"));
    assert!(err("x = legendre([1, 2], 0.5)").contains("single whole number"));
    assert!(err("x = legendre(2, [0, 0.5; 0.1, 0.2])").contains("not a matrix"));
    assert!(err("x = legendre(2)").contains("missing argument"));
}

#[test]
fn zeta_values_domain_and_pole() {
    let it = run(
        "a = zeta(2)\nb = zeta(4)\nc = zeta(3)\nd = zeta(0.5)\ne = zeta(0)\nf = zeta(-1)\ng = zeta(-2)\n\
         h = zeta([1.5, 2.5, 10, -0.5, -1.5, -7, -20.5, 30.5])\ni = zeta(inf)",
    );
    close_rel("zeta(2)", num(&it, "a"), std::f64::consts::PI.powi(2) / 6.0, 1e-15);
    close_rel("zeta(4)", num(&it, "b"), std::f64::consts::PI.powi(4) / 90.0, 1e-15);
    close_rel("zeta(3)", num(&it, "c"), 1.2020569031595942, 1e-15);
    close_rel("zeta(.5)", num(&it, "d"), -1.4603545088095868, 1e-14);
    close_rel("zeta(0)", num(&it, "e"), -0.5, 1e-15);
    close_rel("zeta(-1)", num(&it, "f"), -1.0 / 12.0, 1e-14);
    assert_eq!(num(&it, "g"), 0.0);
    let want = [2.612375348685488, 1.341487257250917, 1.000994575127818, -0.20788622497735457, -0.025485201889833036, 0.004166666666666667, -108.21747505877606, 1.0000000006585472];
    for (g, w) in nums(&it, "h").iter().zip(want) {
        close_rel("zeta", *g, w, 1e-13);
    }
    assert_eq!(num(&it, "i"), 1.0);
    assert!(err("x = zeta(1)").contains("pole"));
    assert!(err("x = zeta([2, 1, 3])").contains("pole"));
    assert!(err("x = zeta(2+1j)").contains("real"));
    assert!(err("x = zeta()").contains("missing argument"));
}

#[test]
fn every_new_name_is_a_listed_builtin() {
    for n in [
        "schur", "sqrtm", "logm", "lyap", "sylvester", "erfinv", "erfcinv", "gammainc", "betainc", "beta",
        "besselj", "bessely", "besseli", "besselk", "expint", "ellipke", "legendre", "zeta",
    ] {
        assert!(qu_interp::BUILTIN_NAMES.contains(&n), "{n} missing from BUILTIN_NAMES");
    }
}
