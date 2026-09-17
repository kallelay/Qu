# Qu numerical core

`qu-core` is the portable reference layer for Qu numerical semantics. It is the
authority shared by native builds and WebAssembly; accelerated backends may replace
an algorithm, but they must preserve this crate's observable behavior and pass its
acceptance tests.

Implemented in vertical slice 1:

- inclusive `start to stop step delta` ranges with direction and allocation checks;
- `mean` and RMS reference reductions;
- an iterative radix-2 real-input FFT and one-sided peak-bin query;
- explicit errors for empty reductions, invalid ranges, and unsupported FFT sizes;
- a compact capability string for runners.

Implemented in the geometry/animation slice:

- validated indexed `Mesh3` vertex and triangle arrays;
- premade box, plane, tetrahedron, and tessellated sphere meshes;
- deterministic `box` and exact static `trimesh` collision-hull compilation;
- stable hull fingerprints for asset/build caches;
- exact-clock transform keyframes with clamp, repeat, and ping-pong playback;
- flat mesh/hull arrays exposed through the WebAssembly boundary.

The selection slice adds stable zero-based `where` indices, logical-mask fill,
gathering, indexed assignment with shape/bounds diagnostics, and matching WASM
functions. These are the reference semantics behind `x[x < 0] = 0` and
`x[where x > limit]`.

The FFT is intentionally dependency-free and deterministic. It establishes
semantics and makes native/WASM bring-up testable; production sizes can later route
to RustFFT, FFTW, MKL, Accelerate, cuFFT, or rocFFT through provider packages.

## Verify

```text
cargo test -p qu-core
cargo test -p qu-wasm
cargo check -p qu-wasm --target wasm32-unknown-unknown
node --test tests/qu-worker.test.js
```

When the repository lives in a synchronized folder and Windows locks Cargo's
temporary archives, set `CARGO_TARGET_DIR` to a nonsynchronized local directory.

## Next boundary

The next milestone moves the frozen grammar, typed AST, and reference interpreter
into Rust. `qu-wasm` then exports program execution through the existing browser
Worker protocol. The Studio UI and its versioned `ready/run/result/failure`
contract do not change when the bootstrap evaluator is removed; the `result`
payload continues to carry diagnostics, plots, logs, bindings, and timing.
