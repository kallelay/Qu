//! Computational-numerics builtins (v0.4.14), through the language: `roots`,
//! `poly`, `deconv`, `polyder`, `polyint`, `gcd`, `lcm`, `primes`, `isprime`,
//! `factor`, `nchoosek`, `factorial`, `toeplitz`, `hankel`, `vander`, `null`,
//! `orth` and `gradient`.
//!
//! Every expected value is known independently of the implementation (Vieta,
//! a textbook factorisation, a hand-built matrix, an identity that must hold),
//! never read back from the code under test.

use qu_core::matrix::Matrix;
use qu_core::Complex64;
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
        Some(Value::Num(x)) => vec![*x],
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    nums(it, n)[0]
}

fn cvec(it: &Interp, n: &str) -> Vec<Complex64> {
    match it.get(n) {
        Some(Value::CVec(v)) => v.to_vec(),
        other => panic!("`{n}` is not a complex vector: {other:?}"),
    }
}

fn mat(it: &Interp, n: &str) -> Matrix {
    match it.get(n) {
        Some(Value::Mat(m)) => m.as_ref().clone(),
        other => panic!("`{n}` is not a matrix: {other:?}"),
    }
}

fn close(a: &[f64], b: &[f64], tol: f64) {
    assert_eq!(a.len(), b.len(), "length: {a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= tol, "{a:?} vs {b:?}");
    }
}

fn rows(m: &Matrix) -> Vec<Vec<f64>> {
    (0..m.rows()).map(|i| m.row_vec(i).unwrap()).collect()
}

fn max_abs(m: &Matrix) -> f64 {
    m.as_slice().iter().fold(0.0f64, |a, b| a.max(b.abs()))
}

/// |p(z)| for a real coefficient vector at a complex point (Horner).
fn peval(p: &[f64], z: Complex64) -> f64 {
    let mut v = Complex64::real(0.0);
    for &c in p {
        v = v.mul(z).add(Complex64::real(c));
    }
    v.magnitude()
}

// ------------------------------------------------------------------ roots

#[test]
fn roots_of_a_cubic_with_integer_roots() {
    let it = run("r = sort(roots([1, -6, 11, -6]))");
    close(&nums(&it, "r"), &[1.0, 2.0, 3.0], 1e-10);
}

#[test]
fn roots_of_x_squared_plus_one_are_plus_minus_j() {
    let it = run("z = roots([1, 0, 1])");
    let z = cvec(&it, "z");
    assert_eq!(z.len(), 2);
    let mut ims: Vec<f64> = z.iter().map(|c| c.im).collect();
    ims.sort_by(|a, b| a.partial_cmp(b).unwrap());
    close(&ims, &[-1.0, 1.0], 1e-12);
    assert!(z.iter().all(|c| c.re.abs() < 1e-12));
}

#[test]
fn repeated_roots_are_found_to_the_accuracy_their_conditioning_allows() {
    // (x - 2)^2: an eps perturbation moves a double root by ~sqrt(eps)
    let it = run("r = roots([1, -4, 4])");
    for x in nums(&it, "r") {
        assert!((x - 2.0).abs() < 1e-6, "{x}");
    }
    let it = run("r = roots([1, -3, 3, -1])"); // (x - 1)^3
    let r = match it.get("r") {
        Some(Value::Vec(v)) => v.iter().map(|&x| Complex64::real(x)).collect::<Vec<_>>(),
        Some(Value::CVec(v)) => v.to_vec(),
        other => panic!("{other:?}"),
    };
    assert_eq!(r.len(), 3);
    for z in r {
        assert!(z.sub(Complex64::real(1.0)).magnitude() < 1e-4);
    }
}

#[test]
fn leading_zeros_are_dropped_and_trailing_zeros_are_exact_zero_roots() {
    // x^2 (x - 1)(x - 2) padded with two leading zeros
    let it = run("p = [0, 0, 1, -3, 2, 0, 0]\nr = sort(roots(p))\nzeros_found = sum(r == 0)");
    close(&nums(&it, "r"), &[0.0, 0.0, 1.0, 2.0], 1e-10);
    assert_eq!(num(&it, "zeros_found"), 2.0, "the zero roots must be exactly 0");
}

#[test]
fn degree_one_degree_zero_and_the_zero_polynomial() {
    let it = run("a = roots([2, -6])\nb = roots([5])\nc = roots([0, 0, 5])\nd = roots([0, 0])\nnb = length(b)\nnc = length(c)\nnd = length(d)");
    close(&nums(&it, "a"), &[3.0], 1e-15);
    assert_eq!(num(&it, "nb"), 0.0);
    assert_eq!(num(&it, "nc"), 0.0);
    assert_eq!(num(&it, "nd"), 0.0);
    let it = run("a = roots([0, 1, 0])");
    close(&nums(&it, "a"), &[0.0], 0.0);
}

#[test]
fn even_polynomials_that_are_hard_for_eigenvalue_solvers() {
    // x^4 + 1 (the known open `eig` issue concerns some even polynomials) and
    // x^8 + 1, x^6 - 1, x^4 + 3x^2 + 2: every root must satisfy the polynomial.
    for p in [
        vec![1.0, 0.0, 0.0, 0.0, 1.0],
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0],
        vec![1.0, 0.0, 3.0, 0.0, 2.0],
        vec![1.0, 0.0, -5.0, 0.0, 4.0],
    ] {
        let lit = p.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
        let it = run(&format!("z = roots([{lit}])"));
        let z: Vec<Complex64> = match it.get("z") {
            Some(Value::CVec(v)) => v.to_vec(),
            Some(Value::Vec(v)) => v.iter().map(|&x| Complex64::real(x)).collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(z.len(), p.len() - 1, "{p:?}");
        for r in z {
            assert!(peval(&p, r) < 1e-9, "p = {p:?}, root {r:?}, |p(r)| = {}", peval(&p, r));
        }
    }
    // x^4 + 1: all four roots on the unit circle at 45 degrees
    let it = run("z = roots([1, 0, 0, 0, 1])");
    for c in cvec(&it, "z") {
        assert!((c.magnitude() - 1.0).abs() < 1e-12);
        assert!((c.re.abs() - 0.5f64.sqrt()).abs() < 1e-12);
    }
}

#[test]
fn roots_errors() {
    assert!(err("roots([])").contains("empty"), "{}", err("roots([])"));
    assert!(err("roots()").contains("roots"));
    assert!(err("roots([1, 2], [3, 4])").contains("roots"));
    assert!(err("roots([1, 2], tol=1)").contains("tol"));
    assert!(err("roots(\"abc\")").contains("roots"));
    assert!(err("roots([1, nan, 3])").contains("finite"));
    assert!(err("roots([1+2j, 3])").contains("real"));
}

// --------------------------------------------------------------- poly etc.

#[test]
fn poly_from_roots_matches_vieta() {
    let it = run("p = poly([1, 2, 3])");
    close(&nums(&it, "p"), &[1.0, -6.0, 11.0, -6.0], 1e-12);
    let it = run("p = poly([2])");
    close(&nums(&it, "p"), &[1.0, -2.0], 0.0);
}

#[test]
fn poly_of_conjugate_pairs_is_real() {
    let it = run("p = poly([1j, -1j])");
    close(&nums(&it, "p"), &[1.0, 0.0, 1.0], 1e-12);
    // (x - 2)(x^2 + 2x + 5): roots 2, -1 +- 2j
    let it = run("p = poly([2, -1+2j, -1-2j])");
    close(&nums(&it, "p"), &[1.0, -0.0, 1.0, -10.0], 1e-12);
}

#[test]
fn poly_of_an_unpaired_complex_root_stays_complex() {
    let it = run("p = poly([1j])");
    let p = cvec(&it, "p");
    assert_eq!(p.len(), 2);
    assert!((p[1].im + 1.0).abs() < 1e-15 && p[1].re.abs() < 1e-15);
}

#[test]
fn poly_roots_round_trip() {
    let it = run("p0 = [2, -3, 0, 5, 1, -7]\np1 = poly(roots(p0)) .* 2");
    close(&nums(&it, "p1"), &nums(&it, "p0"), 1e-9);
    let it = run("p0 = [1, -10, 35, -50, 24]\np1 = poly(roots(p0))");
    close(&nums(&it, "p1"), &nums(&it, "p0"), 1e-8);
}

#[test]
fn poly_errors() {
    assert!(err("poly()").contains("poly"));
    assert!(err("poly([1, 2], [3])").contains("poly"));
    assert!(err("poly(\"x\")").contains("poly"));
}

#[test]
fn deconv_returns_quotient_and_remainder() {
    // x^3 + 2x^2 + 3x + 4 = (x + 1)(x^2 + x + 2) + 2
    let it = run("d = deconv([1, 2, 3, 4], [1, 1])\nq = d.q\nr = d.r");
    close(&nums(&it, "q"), &[1.0, 1.0, 2.0], 1e-15);
    close(&nums(&it, "r"), &[0.0, 0.0, 0.0, 2.0], 1e-15);
}

#[test]
fn deconv_then_conv_recovers_the_dividend() {
    let it = run(
        "b = [3, -2, 7, 1, 0, 5]\na = [2, 1, -1]\nd = deconv(b, a)\nback = conv(a, d.q) + d.r",
    );
    close(&nums(&it, "back"), &nums(&it, "b"), 1e-12);
    // exact division leaves a zero remainder
    let it = run("d = deconv(conv([1, 2, 3], [1, -1]), [1, -1])\nq = d.q\nr = d.r");
    close(&nums(&it, "q"), &[1.0, 2.0, 3.0], 1e-14);
    assert!(nums(&it, "r").iter().all(|x| x.abs() < 1e-14));
}

#[test]
fn deconv_with_a_longer_divisor_and_errors() {
    let it = run("d = deconv([1, 2], [1, 2, 3])\nq = d.q\nr = d.r");
    close(&nums(&it, "q"), &[0.0], 0.0);
    close(&nums(&it, "r"), &[1.0, 2.0], 0.0);
    assert!(err("deconv([1, 2, 3], [0, 0])").contains("zero"));
    assert!(err("deconv([1, 2, 3])").contains("deconv"));
    assert!(err("deconv([], [1])").contains("empty"));
}

#[test]
fn polyder_and_polyint() {
    let it = run("d = polyder([3, 2, 1])\nd0 = polyder([7])\ni = polyint([6, 2], 1)\ni0 = polyint([3])");
    close(&nums(&it, "d"), &[6.0, 2.0], 0.0);
    close(&nums(&it, "d0"), &[0.0], 0.0);
    close(&nums(&it, "i"), &[3.0, 2.0, 1.0], 0.0);
    close(&nums(&it, "i0"), &[3.0, 0.0], 0.0);
    // integrating then differentiating is the identity
    let it = run("p = [4, -3, 2, 7]\nback = polyder(polyint(p))");
    close(&nums(&it, "back"), &nums(&it, "p"), 1e-14);
    assert!(err("polyder([])").contains("empty"));
}

// ------------------------------------------------------------ number theory

#[test]
fn gcd_and_lcm_known_values() {
    let it = run("a = gcd(48, 18)\nb = lcm(4, 6)\nc = gcd(-48, 18)\nd = gcd(0, 0)\ne1 = lcm(0, 5)\nf1 = gcd(0, 7)");
    assert_eq!(num(&it, "a"), 6.0);
    assert_eq!(num(&it, "b"), 12.0);
    assert_eq!(num(&it, "c"), 6.0);
    assert_eq!(num(&it, "d"), 0.0);
    assert_eq!(num(&it, "e1"), 0.0);
    assert_eq!(num(&it, "f1"), 7.0);
}

#[test]
fn gcd_and_lcm_are_elementwise_on_vectors() {
    let it = run("g = gcd([12, 18, 7], [8, 27, 14])\nl = lcm([12, 18, 7], 4)\ng2 = gcd(30, [12, 18, 7])");
    close(&nums(&it, "g"), &[4.0, 9.0, 7.0], 0.0);
    close(&nums(&it, "l"), &[12.0, 36.0, 28.0], 0.0);
    close(&nums(&it, "g2"), &[6.0, 6.0, 1.0], 0.0);
}

#[test]
fn gcd_lcm_errors() {
    assert!(err("gcd(2.5, 3)").contains("integer"));
    assert!(err("lcm(4, 1.5)").contains("integer"));
    assert!(err("gcd([1, 2, 3], [1, 2])").contains("same length"));
    assert!(err("gcd(1)").contains("gcd"));
    assert!(err("gcd(1, 2, 3)").contains("gcd"));
    assert!(err("lcm(9007199254740991, 9007199254740990)").contains("2^53"));
    assert!(err("gcd(nan, 1)").contains("integer"));
}

#[test]
fn primes_up_to_n() {
    let it = run("p = primes(30)\nn100 = length(primes(100))\nn1e6 = length(primes(1000000))\ne1 = length(primes(1))\ne2 = length(primes(-5))\np2 = primes(2)");
    close(&nums(&it, "p"), &[2.0, 3.0, 5.0, 7.0, 11.0, 13.0, 17.0, 19.0, 23.0, 29.0], 0.0);
    assert_eq!(num(&it, "n100"), 25.0);
    assert_eq!(num(&it, "n1e6"), 78498.0);
    assert_eq!(num(&it, "e1"), 0.0);
    assert_eq!(num(&it, "e2"), 0.0);
    close(&nums(&it, "p2"), &[2.0], 0.0);
    assert!(err("primes(10.5)").contains("integer"));
    assert!(err("primes(1000000000)").contains("limit"));
}

#[test]
fn isprime_scalar_and_vector() {
    let it = run("a = isprime(97)\nb = isprime(1)\nc = isprime(0)\nd = isprime(-7)\nm = isprime([2, 3, 4, 5, 6, 7, 8, 9, 10, 11])\nbig = isprime(9007199254740881)\ncomp = isprime(9007199254740991)");
    assert!(matches!(it.get("a"), Some(Value::Bool(true))));
    assert!(matches!(it.get("b"), Some(Value::Bool(false))));
    assert!(matches!(it.get("c"), Some(Value::Bool(false))));
    assert!(matches!(it.get("d"), Some(Value::Bool(false))));
    match it.get("m") {
        Some(Value::Mask(m)) => assert_eq!(
            m.to_vec(),
            vec![true, true, false, true, false, true, false, false, false, true]
        ),
        other => panic!("{other:?}"),
    }
    assert!(matches!(it.get("big"), Some(Value::Bool(true))), "2^53 - 111 is prime");
    assert!(matches!(it.get("comp"), Some(Value::Bool(false))), "2^53 - 1 is composite");
    assert!(err("isprime(2.5)").contains("integer"));
}

#[test]
fn factor_returns_prime_factors_with_multiplicity() {
    let it = run("f = factor(360)\nf1 = factor(1)\nf2 = factor(97)\nbig = factor(9007199254740991)\nprod_ok = prod(factor(600851475143))");
    close(&nums(&it, "f"), &[2.0, 2.0, 2.0, 3.0, 3.0, 5.0], 0.0);
    close(&nums(&it, "f1"), &[1.0], 0.0);
    close(&nums(&it, "f2"), &[97.0], 0.0);
    close(&nums(&it, "big"), &[6361.0, 69431.0, 20394401.0], 0.0);
    assert_eq!(num(&it, "prod_ok"), 600851475143.0);
    assert!(err("factor(0)").contains("positive"));
    assert!(err("factor(-12)").contains("positive"));
    assert!(err("factor(12.5)").contains("integer"));
    assert!(err("factor([6, 10])").contains("single integer"));
}

#[test]
fn nchoosek_is_exact_or_an_error() {
    let it = run("a = nchoosek(52, 5)\nb = nchoosek(5, 0)\nc = nchoosek(5, 5)\nd = nchoosek(50, 25)\ne1 = nchoosek(10, 3)");
    assert_eq!(num(&it, "a"), 2598960.0);
    assert_eq!(num(&it, "b"), 1.0);
    assert_eq!(num(&it, "c"), 1.0);
    assert_eq!(num(&it, "d"), 126410606437752.0);
    assert_eq!(num(&it, "e1"), 120.0);
    assert!(err("nchoosek(60, 30)").contains("2^53"));
    assert!(err("nchoosek(3, 4)").contains("larger"));
    assert!(err("nchoosek(-3, 1)").contains("non-negative"));
    assert!(err("nchoosek(5, 1.5)").contains("integer"));
    assert!(err("nchoosek(5)").contains("nchoosek"));
}

#[test]
fn factorial_values_and_limits() {
    let it = run("a = factorial(0)\nb = factorial(5)\nc = factorial(18)\nv = factorial([1, 2, 3, 4, 5])");
    assert_eq!(num(&it, "a"), 1.0);
    assert_eq!(num(&it, "b"), 120.0);
    assert_eq!(num(&it, "c"), 6402373705728000.0);
    close(&nums(&it, "v"), &[1.0, 2.0, 6.0, 24.0, 120.0], 0.0);
    assert!(err("factorial(171)").contains("overflow"));
    assert!(err("factorial(-1)").contains("non-negative"));
    assert!(err("factorial(2.5)").contains("integer"));
}

// ------------------------------------------------- matrix builders & friends

#[test]
fn toeplitz_symmetric_and_rectangular() {
    let it = run("T = toeplitz([1, 2, 3])\nR = toeplitz([1, 2, 3], [1, 4, 5, 6])");
    assert_eq!(
        rows(&mat(&it, "T")),
        vec![vec![1.0, 2.0, 3.0], vec![2.0, 1.0, 2.0], vec![3.0, 2.0, 1.0]]
    );
    assert_eq!(
        rows(&mat(&it, "R")),
        vec![vec![1.0, 4.0, 5.0, 6.0], vec![2.0, 1.0, 4.0, 5.0], vec![3.0, 2.0, 1.0, 4.0]]
    );
    assert!(err("toeplitz()").contains("toeplitz"));
    assert!(err("toeplitz([])").contains("non-empty"));
}

#[test]
fn hankel_with_and_without_a_last_row() {
    let it = run("H = hankel([1, 2, 3])\nG = hankel([1, 2, 3], [3, 7, 8, 9])");
    assert_eq!(
        rows(&mat(&it, "H")),
        vec![vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 0.0], vec![3.0, 0.0, 0.0]]
    );
    assert_eq!(
        rows(&mat(&it, "G")),
        vec![vec![1.0, 2.0, 3.0, 7.0], vec![2.0, 3.0, 7.0, 8.0], vec![3.0, 7.0, 8.0, 9.0]]
    );
}

#[test]
fn vander_decreasing_powers() {
    let it = run("V = vander([1, 2, 3])\nW = vander([0, 2], 4)");
    assert_eq!(
        rows(&mat(&it, "V")),
        vec![vec![1.0, 1.0, 1.0], vec![4.0, 2.0, 1.0], vec![9.0, 3.0, 1.0]]
    );
    assert_eq!(
        rows(&mat(&it, "W")),
        vec![vec![0.0, 0.0, 0.0, 1.0], vec![8.0, 4.0, 2.0, 1.0]]
    );
    // polynomial evaluation by matrix-vector product agrees with polyval
    let it = run("x = [1, 2, 3, 4]\nc = [2, -1, 3]\nlhs = vander(x, 3) * c\nrhs = polyval(c, x)");
    close(&nums(&it, "lhs"), &nums(&it, "rhs"), 1e-12);
    assert!(err("vander([1, 2], 0)").contains("at least 1"));
}

#[test]
fn null_space_of_a_rank_one_matrix() {
    let it = run("A = [1, 2; 2, 4; 3, 6]\nZ = null(A)\nAZ = A * Z\nG = transpose(Z) * Z");
    let z = mat(&it, "Z");
    assert_eq!(z.shape(), (2, 1));
    assert!(max_abs(&mat(&it, "AZ")) < 1e-12);
    // 1x1 Gram matrix collapses to a scalar: the column has unit norm
    assert!((num(&it, "G") - 1.0).abs() < 1e-12);
}

#[test]
fn null_space_of_a_wide_matrix_has_orthonormal_columns() {
    // 2 x 4, rank 2: null space is 2-dimensional (m < n exercises the
    // right-singular-vector padding)
    let it = run("A = [1, 2, 3, 4; 2, 4, 6, 8.5]\nZ = null(A)\nAZ = A * Z\nG = transpose(Z) * Z");
    let z = mat(&it, "Z");
    assert_eq!(z.shape(), (4, 2));
    assert!(max_abs(&mat(&it, "AZ")) < 1e-12);
    let g = mat(&it, "G");
    assert!((g.get(0, 0).unwrap() - 1.0).abs() < 1e-12);
    assert!((g.get(1, 1).unwrap() - 1.0).abs() < 1e-12);
    assert!(g.get(0, 1).unwrap().abs() < 1e-12);
}

#[test]
fn null_space_of_a_full_rank_matrix_is_empty_and_orth_is_everything() {
    let it = run("A = [2, 0; 0, 3]\nZ = null(A)\nO = orth(A)\nnz = size(Z)\nno = size(O)");
    assert_eq!(nums(&it, "nz"), vec![2.0, 0.0]);
    assert_eq!(nums(&it, "no"), vec![2.0, 2.0]);
}

#[test]
fn orth_spans_the_range() {
    // columns of A are multiples of [1, 2, 3]
    let it = run("A = [1, 2; 2, 4; 3, 6]\nO = orth(A)\nsz = size(O)\nP = O * transpose(O) * A\nres = max(abs(P - A))");
    assert_eq!(nums(&it, "sz"), vec![3.0, 1.0]);
    assert!(num(&it, "res") < 1e-12, "projecting A onto orth(A) must give A back");
    // orthonormal columns for a rank-2 3x3
    let it = run("A = [1, 0, 1; 0, 1, 1; 1, 1, 2]\nO = orth(A)\nsz = size(O)\nG = transpose(O) * O");
    assert_eq!(nums(&it, "sz"), vec![3.0, 2.0]);
    let g = mat(&it, "G");
    for i in 0..2 {
        for j in 0..2 {
            let want = if i == j { 1.0 } else { 0.0 };
            assert!((g.get(i, j).unwrap() - want).abs() < 1e-12);
        }
    }
    // and null() of the same matrix is the dependency [1, 1, -1]/sqrt(3) up to sign
    let it = run("A = [1, 0, 1; 0, 1, 1; 1, 1, 2]\nZ = null(A)\nAZ = A * Z");
    assert_eq!(mat(&it, "Z").shape(), (3, 1));
    assert!(max_abs(&mat(&it, "AZ")) < 1e-12);
}

#[test]
fn gradient_of_x_squared_is_two_x_in_the_interior() {
    let it = run(
        "x = linspace(0, 1, 11)\ng = gradient(x .* x, 0.1)\ng2 = gradient(x .* x, x)\nexact = 2 .* x",
    );
    let g = nums(&it, "g");
    let ex = nums(&it, "exact");
    for i in 1..10 {
        assert!((g[i] - ex[i]).abs() < 1e-12, "i = {i}");
    }
    // one-sided ends: (0.01 - 0)/0.1 and (1 - 0.81)/0.1
    assert!((g[0] - 0.1).abs() < 1e-12);
    assert!((g[10] - 1.9).abs() < 1e-12);
    // coordinate-vector form agrees with the scalar-spacing form
    close(&nums(&it, "g2"), &g, 1e-9);
}

#[test]
fn gradient_default_spacing_and_nonuniform_coordinates() {
    let it = run("g = gradient([1, 4, 9, 16, 25])");
    close(&nums(&it, "g"), &[3.0, 4.0, 6.0, 8.0, 9.0], 1e-14);
    let it = run("x = [0, 0.5, 2, 2.25, 5]\ng = gradient(3 .* x + 1, x)");
    close(&nums(&it, "g"), &[3.0; 5], 1e-12);
    assert!(err("gradient([1])").contains("two samples"));
    assert!(err("gradient([1, 2, 3], 0)").contains("non-zero"));
    assert!(err("gradient([1, 2, 3], [1, 2])").contains("coordinate"));
    assert!(err("gradient([1, 2; 3, 4])").contains("vectors"));
    assert!(err("gradient()").contains("gradient"));
}
