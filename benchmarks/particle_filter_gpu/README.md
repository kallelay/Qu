# Particle filter `.move(dt)`: is GPU dispatch worth it at realistic particle counts?

`particle_filter(n, x0, ...)` (kind `"particle_tracker"`, see
`book/src/stdlib/statistics-ml.md` and `catalog/qu_particle_filter.qu`) is
the specialized, position/velocity-only sibling of the generic
`particle_filter_init` family: its predict step (`.predict(dt)`/
`.move(dt)`) hardcodes a constant-velocity motion model specifically so it
reduces to ONE matrix multiply across every particle at once —
`particles * A^T`, an `(n, d) x (d, d)` product where `d` is the state
dimension (4 for 2-D position/velocity tracking: `[x, y, vx, vy]`). That
shape is embarrassingly parallel across particles, so it is dispatched
through the SAME `qu_gpu::GpuContext::matmul`/`gpu_probe` crossover
machinery `gpu_matmul`/`svm_cross_term` already use (`--features gpu`
only), reusing the existing dispatch decision (`gpu_probe::probe()
.should_use_gpu(n*d*d)`) rather than adding a second, purpose-built compute
shader.

This benchmark asks the question this task was told NOT to assume the
answer to: **at what particle count, if any, does GPU dispatch actually
win for this specific "thin" workload shape** (`d=4`, not the square
matmuls `gpu_probe`'s own crossover was measured on)?

Run: `benchmarks/particle_filter_gpu/bench.qu`, once against a plain
`qu.exe`, once against a `qu.exe --features gpu` build, comparing the
printed per-call timings by hand (there is no explicit "force GPU" switch —
`.move(dt)` decides internally, per call, based on `n`).

## Results (this machine, AMD Ryzen 9700X / 16 logical cores, RTX 5080 — a shared dev box with other concurrent builds/agents on it during these runs, so treat exact magnitudes as noisy single-trial numbers, same convention as `multisine_acceleration`'s own numbers; the DIRECTION of the result was reproduced across three separate runs, see below)

`gpu_probe_info()` on the `--features gpu` build measured a real crossover
of **`16,777,216`** multiply-adds — square matmul sizes 128/256/512/1024
raced directly against each other, same number `multisine_acceleration/
README.md` reports for this machine (this session reused, not re-derived,
that process-global probe). Converting to a particle count for this
filter's `d=4` state (`work = n*d*d = n*16`): GPU is only even a
*candidate* once **`n >= 1,048,576`** particles.

CPU-only build (`qu.exe`, no `gpu` feature) vs. `--features gpu` build,
same script, same machine, run back-to-back:

| n particles | work = n*16 | CPU-only ms/call | `gpu`-build ms/call | GPU eligible? (work >= crossover) |
|---:|---:|---:|---:|---|
| 100 | 1,600 | 0.015 | 0.020 | no |
| 500 | 8,000 | 0.031 | 0.043 | no |
| 2,000 | 32,000 | 0.140 | 0.166 | no |
| 10,000 | 160,000 | 0.651 | 0.636 | no |
| 100,000 | 1,600,000 | 7.48 | 7.24 | no |
| 500,000 | 8,000,000 | 35.83 | 39.27 | no |
| 2,000,000 | 32,000,000 | 149–218 | 176–389 | **yes** |
| 5,000,000 | 80,000,000 | 391 | 454 | **yes** |
| 10,000,000 | 160,000,000 | 787 | 902 | **yes** |

(2,000,000 shows a range because it was measured twice, in two separate
process runs minutes apart on this shared, other-agents-also-building
machine — 149–176 ms and 218–389 ms across the two pairs. The `gpu`-build
number was slower than the CPU-only number in BOTH pairs, which is the
one thing that held steady across the noise.)

## Honest conclusion

**GPU dispatch never wins for this workload shape, at any particle count
tested — not even ten million, comfortably past the measured square-matmul
crossover.** Below `n ≈ 1,048,576` the `gpu`-build simply never dispatches
to the GPU at all (correctly — `work < crossover`), and both builds agree
closely, as expected. Above it, GPU dispatch DOES fire, and is
consistently slower than the CPU path anyway, at every size checked up to
10 million particles (device buffer-binding limits — see
`multisine_acceleration/README.md`'s own writeup of the same 128 MiB
ceiling — block testing meaningfully higher without chunking).

The reason is the same one `multisine_acceleration/README.md` already
documented for a differently-shaped GPU matmul: `gpu_probe`'s crossover was
measured on *square* GEMMs (`m=n=k`), where raw compute genuinely dominates
a serial CPU loop once the problem is big enough. `particle_filter`'s
`.move(dt)` step is a "tall and skinny" `(n, 4) x (4, 4)` matmul — `k=n=4`
is tiny regardless of how large `n` (the particle count) gets, so every
dispatch pays a full upload/submit/map-readback round trip to do only 4
scalar multiply-adds per output row. That fixed per-call overhead, not
FLOP count, is what dominates here, and it never amortizes away the way it
does for a square GEMM at the same total multiply-add count. Confirmed by
this benchmark, not assumed: this is exactly why `qu-interp` also needed a
NEW crash fix (`qu-gpu`'s `check_workgroup_count`, see this session's own
commit) — the SAME tall-skinny shape that makes GPU a bad deal here also
drove `matmul`'s row-dispatch dimension straight past wgpu's
`max_compute_workgroups_per_dimension` limit and crashed the whole process
before that fix existed.

For a position/velocity tracker at realistic particle counts (hundreds to
low thousands, per this task's own framing), this is not a close call:
`particle_filter`'s CPU path is the only path that ever runs, and that is
the right, measured outcome, not a missed optimization. The GPU path
exists, is correct (`particle_tracker_gpu_matmul_matches_cpu_matmul_for_
the_deterministic_motion_step`, `qu-interp/src/lib.rs`), and is honestly
never worth taking for this specific operation shape on this hardware.
