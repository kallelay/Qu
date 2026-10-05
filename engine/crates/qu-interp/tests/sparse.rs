//! End-to-end tests for `import sparse`: dispatch, argument reading and the
//! shape of every returned value, checked against dense arithmetic done in
//! plain Rust here (not by the code under test).

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

fn vec_of(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(xs)) => xs.as_ref().clone(),
        other => panic!("{name} is {other:?}, not a Vec"),
    }
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(x)) => *x,
        other => panic!("{name} is {other:?}, not a Num"),
    }
}

/// Dense matrix as row-major rows.
fn mat_of(it: &Interp, name: &str) -> Vec<Vec<f64>> {
    match it.get(name) {
        Some(Value::Mat(m)) => (0..m.rows()).map(|i| (0..m.cols()).map(|j| m.as_slice()[j * m.rows() + i]).collect()).collect(),
        other => panic!("{name} is {other:?}, not a Mat"),
    }
}

fn rec_field<'a>(it: &'a Interp, name: &str, key: &str) -> &'a Value {
    match it.get(name) {
        Some(Value::Record(fields)) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("{name} has no field {key}")),
        other => panic!("{name} is {other:?}, not a Record"),
    }
}

fn lit(xs: &[f64]) -> String {
    format!("[{}]", xs.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", "))
}

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*seed >> 33) as f64) / (1u64 << 31) as f64
}

/// A random m x n matrix with ~40% fill as (rows, triplets, dense row-major).
fn random_case(m: usize, n: usize, seed: &mut u64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<Vec<f64>>) {
    let mut d = vec![vec![0.0; n]; m];
    let (mut ri, mut ci, mut vv) = (vec![], vec![], vec![]);
    for i in 0..m {
        for j in 0..n {
            if lcg(seed) < 0.4 {
                let v = (lcg(seed) * 10.0).round() / 2.0 - 2.0;
                if v != 0.0 {
                    d[i][j] = v;
                    ri.push(i as f64);
                    ci.push(j as f64);
                    vv.push(v);
                }
            }
        }
    }
    (ri, ci, vv, d)
}

fn make(name: &str, ri: &[f64], ci: &[f64], vv: &[f64], m: usize, n: usize) -> String {
    format!("{name} = sparse.from_triplets({}, {}, {}, {m}, {n})\n", lit(ri), lit(ci), lit(vv))
}

fn assert_close(a: &[Vec<f64>], b: &[Vec<f64>], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: row count");
    for i in 0..a.len() {
        assert_eq!(a[i].len(), b[i].len(), "{what}: col count");
        for j in 0..a[i].len() {
            assert!((a[i][j] - b[i][j]).abs() < 1e-9, "{what}: [{i}][{j}] {} vs {}", a[i][j], b[i][j]);
        }
    }
}

#[test]
fn arithmetic_matches_dense_on_random_matrices() {
    let mut seed = 42u64;
    for &(m, k, n) in &[(1usize, 1usize, 1usize), (3, 4, 2), (7, 5, 6), (12, 12, 12)] {
        let (ai, aj, av, ad) = random_case(m, k, &mut seed);
        let (bi, bj, bv, bd) = random_case(k, n, &mut seed);
        let (ci, cj, cv, cd) = random_case(m, k, &mut seed); // same shape as A
        let x: Vec<f64> = (0..k).map(|i| lcg(&mut seed) * 4.0 - 2.0).collect();
        let mut src = String::from("import sparse\n");
        src += &make("A", &ai, &aj, &av, m, k);
        src += &make("B", &bi, &bj, &bv, k, n);
        src += &make("C", &ci, &cj, &cv, m, k);
        src += &format!("x = {}\n", lit(&x));
        src += "AB = sparse.to_dense(sparse.mul(A, B))\n";
        src += "ApC = sparse.to_dense(sparse.add(A, C))\n";
        src += "AmC = sparse.to_dense(sparse.sub(A, C))\n";
        src += "AhC = sparse.to_dense(sparse.hadamard(A, C))\n";
        src += "At = sparse.to_dense(sparse.transpose(A))\n";
        src += "A3 = sparse.to_dense(sparse.scale(A, 3))\n";
        src += "Ax = sparse.mul(A, x)\n";
        src += "AD = sparse.to_dense(A)\n";
        src += "AX = sparse.mul(A, sparse.to_dense(B))\n";
        let it = run(&src);
        let mut want_ab = vec![vec![0.0; n]; m];
        for i in 0..m {
            for j in 0..n {
                want_ab[i][j] = (0..k).map(|t| ad[i][t] * bd[t][j]).sum();
            }
        }
        assert_close(&mat_of(&it, "AB"), &want_ab, "A*B");
        assert_close(&mat_of(&it, "AX"), &want_ab, "A*dense(B)");
        assert_close(&mat_of(&it, "AD"), &ad, "to_dense");
        let combine = |f: &dyn Fn(f64, f64) -> f64| -> Vec<Vec<f64>> { (0..m).map(|i| (0..k).map(|j| f(ad[i][j], cd[i][j])).collect()).collect() };
        assert_close(&mat_of(&it, "ApC"), &combine(&|a, c| a + c), "add");
        assert_close(&mat_of(&it, "AmC"), &combine(&|a, c| a - c), "sub");
        assert_close(&mat_of(&it, "AhC"), &combine(&|a, c| a * c), "hadamard");
        assert_close(&mat_of(&it, "A3"), &combine(&|a, _| 3.0 * a), "scale");
        let at: Vec<Vec<f64>> = (0..k).map(|j| (0..m).map(|i| ad[i][j]).collect()).collect();
        assert_close(&mat_of(&it, "At"), &at, "transpose");
        let ax = vec_of(&it, "Ax");
        for i in 0..m {
            let want: f64 = (0..k).map(|t| ad[i][t] * x[t]).sum();
            assert!((ax[i] - want).abs() < 1e-9, "A*x row {i}");
        }
    }
}

#[test]
fn lu_solve_matches_the_known_solution_on_random_systems() {
    let mut seed = 99u64;
    for n in [1usize, 2, 6, 25] {
        let (mut ri, mut ci, mut vv, mut d) = random_case(n, n, &mut seed);
        for i in 0..n {
            // make it comfortably nonsingular
            ri.push(i as f64);
            ci.push(i as f64);
            vv.push(30.0);
            d[i][i] += 30.0;
        }
        let xs: Vec<f64> = (0..n).map(|i| i as f64 - 3.0).collect();
        let b: Vec<f64> = (0..n).map(|i| (0..n).map(|j| d[i][j] * xs[j]).sum()).collect();
        let src = format!("import sparse\n{}b = {}\nx = sparse.solve(A, b)\n", make("A", &ri, &ci, &vv, n, n), lit(&b));
        let it = run(&src);
        let x = vec_of(&it, "x");
        for i in 0..n {
            assert!((x[i] - xs[i]).abs() < 1e-8, "n={n} x[{i}] = {} want {}", x[i], xs[i]);
        }
    }
}

fn poisson_src(n: usize) -> String {
    let (mut ri, mut ci, mut vv) = (vec![], vec![], vec![]);
    for i in 0..n {
        ri.push(i as f64);
        ci.push(i as f64);
        vv.push(2.0);
        if i + 1 < n {
            ri.push(i as f64);
            ci.push((i + 1) as f64);
            vv.push(-1.0);
            ri.push((i + 1) as f64);
            ci.push(i as f64);
            vv.push(-1.0);
        }
    }
    // exact solution x_i = sin(i/50); b = A x computed here in Rust
    let xs: Vec<f64> = (0..n).map(|i| (i as f64 / 50.0).sin()).collect();
    let b: Vec<f64> = (0..n)
        .map(|i| {
            let mut s = 2.0 * xs[i];
            if i > 0 {
                s -= xs[i - 1];
            }
            if i + 1 < n {
                s -= xs[i + 1];
            }
            s
        })
        .collect();
    format!("import sparse\n{}b = {}\nxe = {}\n", make("A", &ri, &ci, &vv, n, n), lit(&b), lit(&xs))
}

#[test]
fn poisson_1d_n2000_by_lu_and_cg() {
    let n = 2000;
    let mut src = poisson_src(n);
    src += "xl = sparse.solve(A, b)\n";
    src += "r = sparse.solve(A, b, method=\"cg\", tol=1e-12, maxiter=20000)\n";
    src += "rj = sparse.solve(A, b, method=\"cg\", precond=\"jacobi\", tol=1e-12, maxiter=20000)\n";
    src += "rb = sparse.solve(A, b, method=\"bicgstab\", tol=1e-12, maxiter=20000)\n";
    let it = run(&src);
    let xe = vec_of(&it, "xe");
    let xl = vec_of(&it, "xl");
    let max_err = |x: &[f64]| x.iter().zip(&xe).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    assert!(max_err(&xl) < 1e-9, "lu error {}", max_err(&xl));
    for name in ["r", "rj", "rb"] {
        let x = match rec_field(&it, name, "x") {
            Value::Vec(v) => v.as_ref().clone(),
            _ => panic!("x is not a Vec"),
        };
        assert!(matches!(rec_field(&it, name, "converged"), Value::Bool(true)), "{name} did not converge: {:?}", rec_field(&it, name, "status"));
        assert!(max_err(&x) < 1e-6, "{name} error {}", max_err(&x));
        match rec_field(&it, name, "residual") {
            Value::Num(r) => assert!(*r < 1e-10, "{name} residual {r}"),
            _ => panic!("residual"),
        }
        match rec_field(&it, name, "iterations") {
            Value::Num(k) => assert!(*k > 0.0 && *k <= 20000.0),
            _ => panic!("iterations"),
        }
    }
}

#[test]
fn singular_matrix_is_a_clear_error() {
    let m = err("import sparse\nA = sparse.from_triplets([0, 0, 1, 1], [0, 1, 0, 1], [1, 2, 2, 4], 2, 2)\nx = sparse.solve(A, [1, 1])");
    assert!(m.contains("sparse.solve") && m.contains("singular"), "{m}");
    // a structurally empty column
    let m = err("import sparse\nA = sparse.from_triplets([0, 1], [0, 0], [1, 1], 2, 2)\nx = sparse.solve(A, [1, 1])");
    assert!(m.contains("singular"), "{m}");
}

#[test]
fn cg_on_an_indefinite_system_reports_non_convergence() {
    let it = run("import sparse\nA = sparse.diag([1, -1])\nr = sparse.solve(A, [0, 1], method=\"cg\")");
    assert!(matches!(rec_field(&it, "r", "converged"), Value::Bool(false)));
    match rec_field(&it, "r", "status") {
        Value::Str(s) => assert!(s.contains("not symmetric positive definite"), "{s}"),
        _ => panic!("status"),
    }
    // maxiter exhausted is also a status, not an error
    let it = run(&(poisson_src(300) + "r = sparse.solve(A, b, method=\"cg\", maxiter=3)\n"));
    assert!(matches!(rec_field(&it, "r", "converged"), Value::Bool(false)));
    match rec_field(&it, "r", "status") {
        Value::Str(s) => assert!(s.contains("did not converge"), "{s}"),
        _ => panic!("status"),
    }
}

#[test]
fn cg_refuses_a_nonsymmetric_matrix_and_bicgstab_solves_it() {
    let m = err("import sparse\nA = sparse.from_triplets([0, 0, 1], [0, 1, 1], [2, 1, 3], 2, 2)\nr = sparse.solve(A, [1, 1], method=\"cg\")");
    assert!(m.contains("symmetric"), "{m}");
    let it = run("import sparse\nA = sparse.from_triplets([0, 0, 1], [0, 1, 1], [2, 1, 3], 2, 2)\nr = sparse.solve(A, [3, 3], method=\"bicgstab\")\nx = r.x");
    let x = vec_of(&it, "x");
    assert!((x[0] - 1.0).abs() < 1e-8 && (x[1] - 1.0).abs() < 1e-8, "{x:?}");
}

#[test]
fn duplicates_are_summed_and_empty_rows_and_columns_work() {
    let it = run(
        "import sparse\n\
         A = sparse.from_triplets([0, 0, 3], [1, 1, 3], [1.5, 2.5, 7], 5, 5)\n\
         n = sparse.nnz(A)\n\
         a01 = sparse.get(A, 0, 1)\n\
         a22 = sparse.get(A, 2, 2)\n\
         D = sparse.to_dense(A)\n\
         y = sparse.mul(A, [1, 1, 1, 1, 1])\n\
         T = sparse.triplets(A)\n\
         sz = sparse.size(A)\n\
         dn = sparse.density(A)\n",
    );
    assert_eq!(num_of(&it, "n"), 2.0);
    assert_eq!(num_of(&it, "a01"), 4.0);
    assert_eq!(num_of(&it, "a22"), 0.0);
    assert_eq!(vec_of(&it, "y"), vec![4.0, 0.0, 0.0, 7.0, 0.0]);
    assert_eq!(vec_of(&it, "sz"), vec![5.0, 5.0]);
    assert!((num_of(&it, "dn") - 2.0 / 25.0).abs() < 1e-15);
    assert_eq!(mat_of(&it, "D")[3][3], 7.0);
    match rec_field(&it, "T", "v") {
        Value::Vec(v) => assert_eq!(v.as_ref(), &vec![4.0, 7.0]),
        _ => panic!(),
    }
    // exact cancellation is not stored
    let it = run("import sparse\nA = sparse.from_triplets([0, 0], [0, 0], [1, -1], 2, 2)\nn = sparse.nnz(A)");
    assert_eq!(num_of(&it, "n"), 0.0);
    // an all-empty matrix still multiplies and transposes
    let it = run("import sparse\nA = sparse.from_triplets([], [], [], 3, 2)\ny = sparse.mul(A, [1, 2])\nT = sparse.to_dense(sparse.transpose(A))");
    assert_eq!(vec_of(&it, "y"), vec![0.0, 0.0, 0.0]);
    assert_eq!(mat_of(&it, "T").len(), 2);
}

#[test]
fn constructors_eye_diag_dense_and_random() {
    let it = run(
        "import sparse\n\
         I = sparse.to_dense(sparse.eye(3))\n\
         D = sparse.to_dense(sparse.diag([1, 0, 5]))\n\
         F = sparse.from_dense(sparse.to_dense(sparse.diag([2, 3])))\n\
         nf = sparse.nnz(F)\n\
         R1 = sparse.random(30, 40, 0.1, seed=5)\n\
         R2 = sparse.random(30, 40, 0.1, seed=5)\n\
         R3 = sparse.random(30, 40, 0.1, seed=6)\n\
         d1 = sparse.to_dense(R1)\n\
         d2 = sparse.to_dense(R2)\n\
         d3 = sparse.to_dense(R3)\n\
         nr = sparse.nnz(R1)\n\
         big = sparse.random(100000, 100000, 0.000001, seed=1)\n\
         nb = sparse.nnz(big)\n\
         full = sparse.nnz(sparse.random(6, 7, 1.0, seed=2))\n\
         nothing_stored = sparse.nnz(sparse.random(6, 7, 0.0, seed=2))\n",
    );
    assert_eq!(mat_of(&it, "I")[1], vec![0.0, 1.0, 0.0]);
    assert_eq!(mat_of(&it, "D")[2][2], 5.0);
    assert_eq!(num_of(&it, "nf"), 2.0);
    assert_eq!(num_of(&it, "nr"), 120.0);
    assert_eq!(mat_of(&it, "d1"), mat_of(&it, "d2"), "same seed must reproduce");
    assert_ne!(mat_of(&it, "d1"), mat_of(&it, "d3"), "different seeds must differ");
    assert_eq!(num_of(&it, "nb"), 10000.0);
    assert_eq!(num_of(&it, "full"), 42.0);
    assert_eq!(num_of(&it, "nothing_stored"), 0.0);
}

#[test]
fn values_are_immutable_and_fields_are_readable() {
    let it = run(
        "import sparse\n\
         A = sparse.eye(3)\n\
         B = sparse.scale(A, 5)\n\
         a = sparse.get(A, 1, 1)\n\
         b = sparse.get(B, 1, 1)\n\
         r = A.rows\n\
         k = A.nnz\n",
    );
    assert_eq!(num_of(&it, "a"), 1.0);
    assert_eq!(num_of(&it, "b"), 5.0);
    assert_eq!(num_of(&it, "r"), 3.0);
    assert_eq!(num_of(&it, "k"), 3.0);
}

#[test]
fn bad_arguments_name_the_function_and_the_shapes() {
    let m = err("import sparse\nA = sparse.eye(3)\nB = sparse.eye(4)\nC = sparse.add(A, B)");
    assert!(m.contains("sparse.add") && m.contains("3x3") && m.contains("4x4"), "{m}");
    let m = err("import sparse\nA = sparse.from_triplets([0], [0], [1], 3, 2)\nC = sparse.mul(A, sparse.eye(3))");
    assert!(m.contains("sparse.mul") && m.contains("3x2") && m.contains("3x3"), "{m}");
    let m = err("import sparse\ny = sparse.mul(sparse.eye(3), [1, 2])");
    assert!(m.contains("sparse.mul") && m.contains("length 2"), "{m}");
    let m = err("import sparse\nA = sparse.from_triplets([3], [0], [1], 3, 3)");
    assert!(m.contains("sparse.from_triplets") && m.contains("outside"), "{m}");
    let m = err("import sparse\nA = sparse.from_triplets([-1], [0], [1], 3, 3)");
    assert!(m.contains("sparse.from_triplets"), "{m}");
    let m = err("import sparse\nA = sparse.from_triplets([0, 1], [0], [1], 3, 3)");
    assert!(m.contains("same length"), "{m}");
    let m = err("import sparse\nx = sparse.get(sparse.eye(2), 2, 0)");
    assert!(m.contains("sparse.get") && m.contains("2x2"), "{m}");
    let m = err("import sparse\nx = sparse.solve(sparse.from_triplets([0], [0], [1], 2, 3), [1, 2])");
    assert!(m.contains("square") && m.contains("2x3"), "{m}");
    let m = err("import sparse\nx = sparse.solve(sparse.eye(2), [1, 2, 3])");
    assert!(m.contains("length 3"), "{m}");
    let m = err("import sparse\nx = sparse.solve(sparse.eye(2), [1, 2], method=\"gmres\")");
    assert!(m.contains("unknown method"), "{m}");
    let m = err("import sparse\nx = sparse.nnz([1, 2])");
    assert!(m.contains("sparse matrix"), "{m}");
    let m = err("import sparse\nx = sparse.random(3, 3, 1.5)");
    assert!(m.contains("density"), "{m}");
}

#[test]
fn size_limits_refuse_before_allocating() {
    let m = err("import sparse\nA = sparse.eye(100000000)");
    assert!(m.contains("sparse.eye") && m.contains("limit"), "{m}");
    let m = err("import sparse\nA = sparse.from_triplets([], [], [], 100000000, 10)");
    assert!(m.contains("limit"), "{m}");
    let m = err("import sparse\nA = sparse.random(10000000, 10000000, 0.5)");
    assert!(m.contains("limit"), "{m}");
    let m = err("import sparse\nD = sparse.to_dense(sparse.eye(9000000))");
    assert!(m.contains("to_dense") && m.contains("limited"), "{m}");
}

#[test]
fn multiple_right_hand_sides_and_the_jacobi_diagonal_check() {
    let it = run("import sparse\nA = sparse.diag([2, 4])\nX = sparse.solve(A, [2, 4; 8, 12])");
    assert_eq!(mat_of(&it, "X"), vec![vec![1.0, 2.0], vec![2.0, 3.0]]);
    let m = err("import sparse\nA = sparse.from_triplets([0, 1], [1, 0], [1, 1], 2, 2)\nr = sparse.solve(A, [1, 1], method=\"bicgstab\", precond=\"jacobi\")");
    assert!(m.contains("Jacobi"), "{m}");
}
