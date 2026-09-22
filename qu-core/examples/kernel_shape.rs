//! Does the INSTANTIATION of the elementwise kernel matter?
//!
//! `qu-core/examples/ew_profile.rs` calls `Matrix::map(f64::sin)` and
//! `Matrix::broadcast(|x, y| x * y)` — both generic parameters are
//! zero-sized fn ITEMS / closures, so each monomorphises and the per-
//! element call inlines. The interpreter does NOT do that: `dispatch_
//! builtin`'s `e1` closure is typed `fn(f64) -> f64`, and `matrix_ew`
//! takes `f: fn(f64, f64) -> f64`. Both are function POINTERS, so
//! `map`/`broadcast` monomorphise ONCE for "call through a pointer" and
//! every element pays an indirect call that cannot inline or vectorise.
//!
//! That means "ew_profile.rs measures the same kernel the interpreter
//! runs" is not quite true — same source function, different generated
//! code. This example measures how much that difference is worth, with
//! the two shapes INTERLEAVED (this machine is shared; a block of A
//! followed by a block of B measures the machine's mood as much as the
//! code).
//!
//! Run from `qu-core/`:
//!   cargo run --release -j 1 --features "parallel,fast-matmul" --example kernel_shape
use qu_core::matrix::Matrix;
use std::time::Instant;

fn best<T, F: FnMut() -> T>(reps: usize, mut f: F) -> f64 {
    let mut b = f64::MAX;
    for _ in 0..reps {
        let t = Instant::now();
        let out = f();
        std::hint::black_box(&out);
        let dt = t.elapsed().as_secs_f64();
        if dt < b {
            b = dt;
        }
    }
    b
}

fn main() {
    let n = 3000;
    let a = Matrix::filled(n, n, 0.7);
    let b = Matrix::filled(n, n, 1.3);
    // warm the rayon pool and the allocator, same as the interpreter's
    // own warm-up run does before the timed laps.
    std::hint::black_box(a.map(f64::sin));
    std::hint::black_box(a.broadcast(&b, |x, y| x * y).unwrap());

    // `black_box` so the optimiser cannot constant-propagate the pointer
    // back into the call site and devirtualise it. In the interpreter the
    // pointer genuinely comes out of a runtime `match f { "sin" => ... }`,
    // so a devirtualised example would flatter the fn-pointer shape and
    // report a difference that the real engine does not enjoy.
    let sin_ptr: fn(f64) -> f64 = std::hint::black_box(f64::sin as fn(f64) -> f64);
    let mul_ptr: fn(f64, f64) -> f64 =
        std::hint::black_box((|x: f64, y: f64| x * y) as fn(f64, f64) -> f64);

    let reps = 7;
    // Interleaved: one pass of each shape per round, `reps` rounds.
    let mut map_item = f64::MAX;
    let mut map_ptr = f64::MAX;
    let mut bc_closure = f64::MAX;
    let mut bc_ptr = f64::MAX;
    for _ in 0..reps {
        map_item = map_item.min(best(1, || a.map(f64::sin)));
        map_ptr = map_ptr.min(best(1, || a.map(sin_ptr)));
        bc_closure = bc_closure.min(best(1, || a.broadcast(&b, |x, y| x * y).unwrap()));
        bc_ptr = bc_ptr.min(best(1, || a.broadcast(&b, mul_ptr).unwrap()));
    }

    // And the copy the interpreter makes that no kernel measurement sees:
    // `Value::into_matrix` on a `Value::Mat` whose `Arc` is still held by
    // a live variable falls back to `(*arc).clone()`.
    let clone_cost = best(reps, || a.clone());

    println!("map(f64::sin)   monomorphised item : {map_item:.5} s");
    println!("map(sin_ptr)    fn pointer         : {map_ptr:.5} s  ({:.2}x)", map_ptr / map_item);
    println!("broadcast(closure)                 : {bc_closure:.5} s");
    println!("broadcast(fn ptr)                  : {bc_ptr:.5} s  ({:.2}x)", bc_ptr / bc_closure);
    println!(
        "Matrix::clone  ({} MB)             : {clone_cost:.5} s",
        n * n * 8 / (1024 * 1024)
    );
}
