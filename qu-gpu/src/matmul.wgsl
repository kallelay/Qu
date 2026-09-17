struct Params {
    m: u32,
    k: u32,
    n: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

// Naive O(m*k*n) matmul — one thread per output element. Row-major:
// a is (m, k), b is (k, n), c is (m, n). No tiling/shared-memory blocking;
// this is a first real GPU kernel to prove the wiring, not a tuned one.
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let row = id.y;
    let col = id.x;
    if row >= params.m || col >= params.n {
        return;
    }
    var sum = 0.0;
    var i = 0u;
    loop {
        if i >= params.k {
            break;
        }
        sum += a[row * params.k + i] * b[i * params.n + col];
        i += 1u;
    }
    c[row * params.n + col] = sum;
}
