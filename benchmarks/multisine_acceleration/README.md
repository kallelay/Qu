# Multisine synthesis: every Qu acceleration method, head to head

One workload, five ways to compute it, all in `bench.qu`, all cross-checked
against each other before any timing number is trusted:

```
x(t) = sum_{k=1..51} A_k * sin(2*pi*f_k*t + phi_k)
Fs = 5000 Hz, duration = 300 s  ->  N = 1,500,000 samples, K = 51 tones
```

51 tones log-spaced 5 Hz-2000 Hz, amplitude budget `1/sqrt(51)` per tone
(unit-ish RMS), phases drawn from `seed(2026)` so every method starts from
the exact same tone table. `catalog/qu_multisine_acceleration.qu` is the
same workload as a standalone, well-commented catalog example covering just
method 1.

Run:

```
qu run benchmarks/multisine_acceleration/bench.qu
```

Method 5 (`gpu_matmul`) needs a separate `qu.exe` built with `--features
gpu`; the default build still runs methods 1-4 and reports method 5 as
unavailable (see "GPU" below for exactly what "unavailable" means here).

## The five methods

| # | Method | Shape |
|---|---|---|
| 1 | Baseline | `for k in 0..50: x += A[k]*sin(2*pi*f[k]*t+phi[k])` -- a scalar loop over the 51 tones, each iteration vectorized over the N-length time axis. Qu has no reasonable scalar-loop-over-N alternative worth demonstrating, so this vectorized-per-tone loop IS the right "unaccelerated" baseline. |
| 2 | Full vectorization | Zero loops over tones or time: build `T_col` (N,1), `F_row`/`Phi_row` (1,K) via `reshape`, outer-product them into a (N,K) phase matrix (`T_col * F_row`, a real matmul), `sin(...)` it into a basis matrix, then collapse with **one more matmul** against the (K,1) amplitude vector (`Basis * A_col`) instead of `sum(..., axis=)` -- chosen specifically so the exact same (N,K) basis matrix can be reused unchanged by method 5. |
| 3 | `parallel for` | Disjoint per-tone **column** writes into a shared (N,K) matrix (`Contrib[:, k] = ...`), summed with `sum(Contrib, axis=1)` after the parallel region closes. See "`parallel for` and reductions" below for why this is the *safe* pattern, not just *a* pattern. |
| 4 | pool + queue + workers | 51 tones split into 8 chunks; each chunk is a queued call to a named `multisine_chunk(k_start, k_end)` function returning that chunk's own N-length partial sum; `run jobs on pool(8)` dispatches all 8, partials summed afterward. |
| 5 | GPU | Method 2's own (N,K) basis matrix, collapsed via `gpu_matmul(basis_chunk, A_col)` instead of a CPU matmul -- see "GPU" below for why this is chunked rather than one call. |

## Correctness cross-check (the point of this benchmark, not an afterthought)

Every method's output is diffed against method 1's directly, `max(abs(x1 -
xN))`, printed by the script itself:

| vs. baseline | max abs diff | verdict |
|---|---:|---|
| `[2]` full vectorization | `3.441e-10` | floating-point noise (f64 matmul reordering) |
| `[3]` parallel for | `0.000e0` | bit-identical |
| `[4]` pool/queue | `2.665e-15` | floating-point noise |
| `[5]` gpu_matmul | `1.179e-6` | consistent with `gpu_matmul`'s documented **f32** precision (see its own doc comment in `lib.rs`), not a bug |

All five compute the numerically identical signal. Only after confirming
this are the timing numbers below worth reading.

## Results (this machine, AMD Ryzen 9700X / 16 logical cores, RTX 5080 -- 4 single-trial runs, not statistically rigorous, same convention as every other benchmark in this directory)

| Method | Time | Speedup vs. baseline | Correctness |
|---|---:|---:|---|
| `[1]` baseline loop | 0.70-0.87 s | 1.00x | reference |
| `[2]` full vectorization | 0.38-0.48 s | **1.6-1.9x faster** | matches to 1e-10 |
| `[3]` parallel for | 22.0-24.6 s | **0.03-0.04x (~30x SLOWER)** | matches exactly |
| `[4]` pool + queue + workers | 0.23-0.40 s | **1.8-3.8x faster** | matches to 1e-15 |
| `[5]` gpu_matmul (chunked) | 2.17-2.50 s | **0.32-0.35x (~3x SLOWER)** | matches to 1e-6 (f32) |

**Winner: method 4 (pool/queue/workers)**, narrowly ahead of method 2 (full
vectorization) across the 4 runs -- both solidly ahead of the unaccelerated
baseline, both well within normal single-trial noise of each other (their
ranges overlap). `parallel for` and GPU are the two genuine surprises, both
*slower* than doing nothing extra at all -- neither is a demo artifact,
both have a specific, identified root cause below.

## `parallel for` and reductions: investigated, not assumed

The task-level question this benchmark had to answer first: does `parallel
for` safely support a REDUCTION (`x += ...` from every iteration into one
shared accumulator), or only disjoint-index writes (`results[i] = ...`)?
Checked directly against the real interpreter before writing method 3, and
the answer is more interesting than "reductions are rejected":

```qu
total = 0
parallel for i in 0 to 4
    total = total + i
end parallel
```
This **does** get rejected -- `parallel for: iteration N tried to
reassign scalar 'total' ...` -- confirmed by an existing test,
`parallel_for_rejects_a_whole_value_reassignment_of_a_preexisting_scalar`
(`engine/crates/qu-interp/src/lib.rs`).

But the same pattern on a **vector** accumulator does not error at all --
it silently computes the wrong answer:

```qu
x = zeros(5)
parallel for k in 0 to 2
    x = x + [1,2,3][k]
end parallel
# expected, if this were a real reduction: [6,6,6,6,6]
# actual, every run: [3,3,3,3,3]
```

Root cause, read directly from `merge_parallel_iteration`
(`engine/crates/qu-interp/src/lib.rs`): the post-loop merge for a
`Value::Vec`/`Value::Mat` diffs each iteration's whole final value against
the pre-loop value element-by-element and copies over whatever changed --
built for the disjoint-index-write shape (`results[i] = ...`, where only
index `i` actually differs from the pre-loop value each iteration), not a
real reduction, where *every* element differs on *every* iteration, so the
"merge" just keeps whichever iteration happened to be folded in last. This
is a real, confirmed gap between the scalar path (explicitly detected and
rejected) and the vector/matrix path (silently wrong) -- flagged here
rather than fixed, since `lib.rs` is a large, actively-shared file and this
benchmark's job is to measure, not patch the interpreter.

Method 3 above therefore uses the confirmed-safe pattern instead: each of
the 51 iterations writes to its **own column** of a shared `(N, K)` matrix
(`Contrib[:, k] = ...`), which never touches another iteration's elements,
followed by an ordinary sequential `sum(Contrib, axis=1)` once the parallel
region closes.

**That safe pattern is also why method 3 is ~30x SLOWER than the
baseline it's supposed to accelerate.** `Contrib` is a single shared
`(1,500,000 x 51)` matrix (~612 MB of `f64`). Every one of the 51
iterations' `Contrib[:, k] = ...` writes goes through the same
Arc-copy-on-write path the rest of the interpreter uses for cheap value
sharing -- but with up to several iterations' worker environments all
holding a live reference to the *same* pre-loop `Contrib` Arc at once,
each write can force its own full ~612 MB deep clone of the matrix rather
than mutating in place. Fanning 51 tones out across 16 cores wins nothing
if each of those 51 writes is paying for a fresh 612 MB copy first --
consistent with the ~22-25 s measured, and with `parallel_for_scaling.qu`
(this directory's sibling benchmark) showing a real, uncontested 6x
speedup on a workload with no large shared mutable state to copy. This
looks like the disjoint-write-into-shared-large-matrix pattern being a
genuine perf trap in the current COW implementation, not a mistake in
this script -- flagged, not fixed, same reasoning as above.

## Pool/queue/workers: the actual winner (or tied for it)

`multisine_chunk(k_start, k_end)` closes over the top-level `f`/`A`/`phi`/`t`
arrays (visible to every queued job via `visible_env_snapshot`, confirmed
by reading `eval_run`'s own doc comment) and returns one N-length partial
sum per chunk -- 8 chunks (6-7 tones each), `run jobs on pool(8)` on this
16-logical-core machine. No shared mutable state during the parallel
region at all (every job's return value is independent, combined only
*after* `run` finishes), which is exactly why this sidesteps method 3's
COW trap entirely and comes out fastest (or tied-fastest) of the four
CPU-side methods.

## GPU: available, correct, and honestly slower here

This machine has a real GPU (RTX 5080, confirmed via `nvidia-smi`), and
`gpu_matmul` genuinely works -- a small-scale correctness probe
(`gpu_matmul([1,2;3,4],[1,0;0,1])`-scale, then a 4x2 case) matches the CPU
result exactly before this benchmark trusted it at all.

**A literal single `gpu_matmul(Basis2, A_col)` call -- what the task
first described -- does not work at this problem's size, and fails in a
way worth flagging on its own.** This build's wgpu device limits, measured
directly by triggering the failure rather than guessed:

| Limit | Value |
|---|---:|
| max buffer size | 268,435,456 bytes (256 MiB) |
| `max_*_buffer_binding_size` | 134,217,728 bytes (128 MiB) -- the binding one bites first |

The basis matrix at N=1,500,000, K=51 needs a `1,500,000 * 51 * 4 =
306,000,000`-byte buffer (`gpu_matmul` computes in **f32**, per its own
doc comment) -- over both limits. wgpu reports the overage through an
internal validation callback that this build turns into a **hard Rust
panic that kills the whole process**, not a `Value`-level `EvalError` --
so Qu's own `try`/`catch` never gets a chance to run. Confirmed directly:

```
wgpu error: Validation Error
Caused by:
    In Device::create_bind_group
      note: label = `Qu matmul bind group`
    Buffer binding 0 range 153000000 exceeds `max_*_buffer_binding_size` limit 134217728
```

A real gap: `gpu_matmul` has no size precheck against the device's own
limits before dispatching, and the failure mode when it's exceeded is an
uncatchable crash rather than a clean error a script could `try`/`catch`
around. Not fixed here (`qu-gpu`/the relevant `lib.rs` GPU dispatch path
are out of this benchmark's scope) -- worked around the only way that
doesn't crash the process: `bench.qu` splits the basis matrix into
row-chunks sized to fit under the measured binding limit (3 chunks for
this N, K), issuing one `gpu_matmul` call per chunk into the same output
vector. Still the same builtin, same GPU, same computation -- just 3
dispatches instead of the 1 originally scoped, and documented as such
directly in the script.

**With that workaround, GPU is real but ~3x SLOWER than the CPU
baseline here (2.17-2.50 s vs. 0.70-0.87 s)**, not a fluke: this
machine's own `gpu_probe_info()` measured CPU-vs-GPU crossover is
`16,777,216` (2^24) multiply-adds on **square** matmuls (its four probe
sizes are all `m=n=k`). This workload's `m*n*k = 1,500,000 * 1 * 51 =
76,500,000` is comfortably *above* that crossover, which would suggest
GPU should win -- and doesn't. The mismatch is the matrix *shape*, not
the total work: `gpu_probe`'s crossover was measured on dense square
GEMMs, where the GPU's massively parallel compute genuinely dominates a
serial CPU loop at that arithmetic intensity. This workload is a "skinny"
matrix-vector-shaped multiply (`(N,51) x (51,1) -> (N,1)`, one scalar
output per row) chunked three ways specifically because of a buffer-size
ceiling that has nothing to do with FLOPs -- three full upload/dispatch/
readback round trips over a combined ~918 MB of f32 basis data dominate
wall-clock completely, an overhead a square in-cache-friendly GEMM at the
same raw multiply-add count never pays. Exactly the "data-transfer
overhead can dominate at problem sizes that are large by typical-script
standards but still modest for a discrete GPU" case flagged as a
possibility going in -- confirmed, not assumed, and reported as a real
loss for GPU on this specific workload shape rather than smoothed over.

## What forced a deviation from the letter of the task, and why

- **`t = 0 to (N-1)*dt step dt` does not work at N=1,500,000.** Plain
  `to`/`step` ranges cap out at `qu-core::DEFAULT_ELEMENT_LIMIT =
  1,000,000` elements and error (`range would create 1500000 values;
  limit is 1000000`) above it. `linspace(0, (N-1)*dt, N)` has no such cap
  and produces the identical values (checked directly against a small-N
  `to`/`step` range before relying on it) -- used throughout instead.
- **Method 2/5's basis matrix is built via matmul-collapse
  (`Basis * A_col`), not `sum(Mx, axis=0)`** -- the task allowed either
  ("a matrix-vector product against the amplitude vector, or a `sum`
  along one axis if that's supported"). The matmul form was chosen
  specifically so method 5 could reuse the *exact same* basis matrix
  object with no reshaping in between, keeping "methods 2 and 5 compute
  the same thing, one more step accelerated" as literal as possible.
- **Method 5 is 3 chunked `gpu_matmul` calls, not 1** -- forced by the
  device buffer-binding limit above; the single-call attempt was tried
  first, confirmed to crash the whole process rather than return an
  error, and abandoned in favor of the chunked form for exactly that
  reason (full detail in "GPU" above).
- **Method 3's disjoint-column-write pattern is real and correct, but is
  the slowest method measured here, not the fastest** -- an honest result
  the task explicitly asked for ("be honest if GPU turns out to be slower
  ... rather than assuming GPU always wins" applies just as much to
  `parallel for` turning out slower here too). The task anticipated
  needing this exact fallback pattern if reduction wasn't safe; what
  wasn't anticipated going in was that the safe pattern would also be by
  far the slowest one, for reasons specific to this workload's one large
  shared output matrix (see above).
