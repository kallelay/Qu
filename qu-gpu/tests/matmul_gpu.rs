use qu_gpu::{GpuContext, GpuError};

fn cpu_matmul(a: &[f32], m: usize, k: usize, b: &[f32], n: usize) -> Vec<f32> {
    let mut c = vec![0.0f32; m * n];
    for row in 0..m {
        for col in 0..n {
            let mut sum = 0.0f32;
            for i in 0..k {
                sum += a[row * k + i] * b[i * n + col];
            }
            c[row * n + col] = sum;
        }
    }
    c
}

#[test]
fn gpu_matmul_matches_cpu_reference() {
    let gpu = match GpuContext::new_blocking() {
        Ok(gpu) => gpu,
        Err(GpuError::NoAdapter) => {
            if std::env::var_os("QU_REQUIRE_GPU").is_some() {
                panic!("QU_REQUIRE_GPU is set but no compatible WebGPU adapter was found");
            }
            eprintln!("GPU SKIP: no compatible WebGPU adapter");
            return;
        }
        Err(error) => panic!("GPU initialization failed: {error}"),
    };

    // A non-square, non-power-of-two shape to catch off-by-one boundary bugs
    // in the workgroup dispatch (m/n aren't multiples of the 16x16 tile).
    let (m, k, n) = (37, 23, 41);
    let a: Vec<f32> = (0..m * k).map(|i| (i % 7) as f32 - 3.0).collect();
    let b: Vec<f32> = (0..k * n).map(|i| (i % 5) as f32 - 2.0).collect();

    let expected = cpu_matmul(&a, m, k, &b, n);
    let actual = gpu.matmul(&a, m, k, &b, n).unwrap();

    assert_eq!(actual.len(), expected.len());
    let max_error = actual
        .iter()
        .zip(&expected)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(max_error < 1e-2, "GPU/CPU matmul mismatch: max error {max_error}");
}

#[test]
fn gpu_matmul_rejects_incompatible_shapes() {
    let gpu = match GpuContext::new_blocking() {
        Ok(gpu) => gpu,
        Err(GpuError::NoAdapter) => {
            eprintln!("GPU SKIP: no compatible WebGPU adapter");
            return;
        }
        Err(error) => panic!("GPU initialization failed: {error}"),
    };
    let a = vec![1.0f32; 6]; // 6 elements, but m=2,k=4 needs 8
    let b = vec![1.0f32; 12]; // k=4, n=3 needs 12 — this one's fine
    assert!(gpu.matmul(&a, 2, 4, &b, 3).is_err());
}

#[test]
fn gpu_matmul_gives_a_clean_error_instead_of_crashing_on_a_tall_skinny_shape() {
    // Regression test for a REAL crash found while adding `qu-interp`'s
    // particle-filter GPU predict step: `matmul`'s dispatch is
    // `(n.div_ceil(16), m.div_ceil(16), 1)` — a "tall and skinny" matmul
    // (millions of rows, a handful of columns, exactly the shape a
    // particle filter's `(n_particles, state_dim) x (state_dim, state_dim)`
    // motion update produces for a large particle count) drives
    // `m.div_ceil(16)` past wgpu's `max_compute_workgroups_per_dimension`
    // limit (65535) long before either buffer involved gets anywhere near
    // `max_storage_buffer_binding_size`. Before `check_workgroup_count`
    // existed, this hit wgpu's uncaptured-error callback and panicked the
    // whole process (the same failure MODE `BufferTooLarge`/
    // `check_buffer_size` already guards against, just a different limit)
    // instead of returning a catchable `Err`.
    let gpu = match GpuContext::new_blocking() {
        Ok(gpu) => gpu,
        Err(GpuError::NoAdapter) => {
            eprintln!("GPU SKIP: no compatible WebGPU adapter");
            return;
        }
        Err(error) => panic!("GPU initialization failed: {error}"),
    };
    let (m, k, n) = (2_000_000, 4, 4); // m.div_ceil(16) = 125_000 > 65_535
    let a = vec![0.0f32; m * k];
    let b = vec![0.0f32; k * n];
    let err = gpu.matmul(&a, m, k, &b, n).expect_err("a dispatch this tall must be rejected cleanly, not crash the process");
    assert!(matches!(err, GpuError::DispatchTooLarge { .. }), "expected DispatchTooLarge, got: {err}");
}
