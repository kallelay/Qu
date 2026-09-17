# Performance & Optimization

Every number on this page was measured just now, on this machine (16 logical
cores, no GPU feature compiled in), against the actual `qu.exe` this build
produces — not assumed, not carried over from a different language's
intuition. Re-run any of it yourself; the scripts are short enough to retype.
Numbers will differ on your hardware, but the *shape* of each result — which
technique wins and by roughly how much — is the part worth trusting.

## The one-line version

| Do this | Not this | Why |
|---|---|---|
| `for i = 1 to n` | `while i <= n ... i = i + 1` | `for` has a fast register path; `while` currently does not (~30x) |
| `xs .* xs` | `for i ...: ys[i] = xs[i]*xs[i]` | vectorized ops skip per-element interpreter dispatch (~250x+) |
| Pass big `Vec`/`Mat` args freely | Hand-roll a "pass by reference" workaround | Qu is copy-on-write; nothing is deep-copied until it is actually mutated |
| `parallel for` over independent iterations | a serial `for` over the same work | real multi-threading when each iteration is heavy enough (~7x on 16 cores) |
| Native operator (`+`, `.*`, ...) | `x.apply(lambda)` / `apply(x, lambda)` for the same elementwise op | dot-call and function-call cost the same (~0), but *any* lambda-per-element call is ~420x slower than the native vectorized operator |
| Don't bother manually unrolling | manually unrolling a `for` loop body | measured no benefit — the fast path's loop overhead is already negligible |
| Know your data's sparsity pattern | assume `Mat` scales like the real world's `A` | Qu has **no sparse matrix type today** — every `Mat` is dense, always |

## Loop constructs: `for` vs `while` vs vectorized

Summing the integers 1..500,000 three ways:

```qu
N = 500000

t = timer()
start(t)
s1 = 0.0
for i = 1 to N
    s1 = s1 + i
end for
print("for            : {elapsed(t)} s")

t2 = timer()
start(t2)
s2 = 0.0
i = 1
while i <= N
    s2 = s2 + i
    i = i + 1
end while
print("while          : {elapsed(t2)} s")

t3 = timer()
start(t3)
s3 = sum(1 to N)
print("vectorized sum : {elapsed(t3)} s")
```

```
for            : 0.0047-0.0077 s
while          : 0.163-0.198 s
vectorized sum : 0.0018-0.0025 s
```

`while` is **25-40x slower** than the equivalent `for` here, for identical
work — a real, currently-open interpreter gap (tracked as F-07 in this
project's own bug log). The precise mechanism (confirmed by reading
`qu-interp`'s actual fast-path compiler, not guessed): **`while` *does*
have the same kind of register fast path `for` does** (`try_fast_while_
plan`), but that compiler only harvests loop-body *assignment targets* into
its register table — it never scans the loop *condition* for the names it
reads. `while i <= N ...` reads `N`, which the body never reassigns, so `N`
is absent from the register table, the condition can't be compiled to fast
registers, and the whole loop silently falls back to the slow, fully
tree-walked path — for every single iteration, not just the first. (`wait
until` has a second, independent reason: its desugared condition is wrapped
in a `not(...)`, which the fast-path condition compiler rejects outright,
regardless of what's inside it.)

That mechanism gives a concrete, usable-today workaround, confirmed by
measurement — comparing against a literal/counter instead of an external
bound:

```qu
i = N
while i > 0        # no external name in the condition
    s1 = s1 + i
    i = i - 1
end while
```

```
while i > 0 (no external name in cond) : 0.011-0.013 s
while i <= N (N is external)           : 0.16-0.31 s
```

Rewriting the condition to avoid naming an outer variable recovers most of
the gap (still ~2x behind `for`'s fast path, but ~20x+ faster than the
naive version) — **because it engages the SAME fast-path mechanism `for`
uses**, not a different one. Until the condition-scanning gap is closed in
the interpreter, treat this as the practical rule: **prefer `for` whenever
the loop has a known bound; if you must write `while`, keep the condition
free of outer-scope names** (compare against a literal or a variable the
loop body itself reassigns every iteration).

The vectorized `sum(...)` beats even the fast `for` path by another 2-3x,
because it never enters the tree-walking evaluator per element at all — see
the next section for why that gap gets dramatically wider for anything more
than accumulation.

`wait until <cond> ... end until` is Qu's "do-until" — and it is not a
separate construct performance-wise at all: the parser desugars it directly
into the identical `Stmt::While` AST node with the condition negated
(`qu-syntax`'s `wait_until_stmt`, confirmed by reading it). Whatever makes
`while` slow above, `wait until` inherits exactly, for the same reason.

## Vectorization: avoid the per-element loop entirely

Squaring every element of a 500,000-length vector, indexed loop vs vectorized:

```qu
N = 500000
xs = 1 to N

t = timer(); start(t)
ys = zeros(N)
for i = 0 to N - 1
    ys[i] = xs[i] * xs[i]
end for
print("manual indexed loop : {elapsed(t)} s")

t2 = timer(); start(t2)
zs = xs .* xs
print("vectorized .*       : {elapsed(t2)} s")
```

```
manual indexed loop : 0.199-0.200 s
vectorized .*        : 0.0007-0.0009 s
```

**~275x.** This is a bigger gap than the `for`/`while` one above because two
costs stack: every iteration now pays for two index reads and an index
write (each a bounds-checked, interpreter-dispatched operation), not just a
register accumulate. `xs .* xs` compiles down to one call into `qu-core`'s
native elementwise kernel (SIMD-friendly, rayon-parallel above a measured
size threshold — the same kernel `+`, `-`, `.^`, and every other elementwise
operator use), which never re-enters the tree-walking evaluator per element.

**Rule of thumb: if you're writing `for i ...: result[i] = f(x[i])`, there is
almost always a vectorized spelling, and it will not be a close contest.**
`map`, elementwise operators (`+ - * / .* ./ .^`), and the builtin math
functions (`sin`, `sqrt`, `abs`, ...) all take whole `Vec`/`Mat` arguments
and vectorize automatically — reach for those before reaching for an
explicit loop with an index.

## Method call (`x.f(y)`) vs function call (`f(x,y)`) vs native operator

`x.method(...)` is pure syntax sugar for `method(x, ...)` in Qu — no
separate dispatch mechanism, so it costs nothing extra. What *does* cost
something is going through any user-defined function/lambda at all instead
of a native vectorized operator, even for trivial per-element work:

```qu
N = 500000
xs = 1 to N
inc = (v) := v + 1.0

t = timer(); start(t)
r1 = xs + 1.0
print("operator   x + 1           : {elapsed(t)} s")

t2 = timer(); start(t2)
r2 = xs.apply(inc)
print("method     x.apply(lambda) : {elapsed(t2)} s")

t3 = timer(); start(t3)
r3 = apply(xs, inc)
print("function   apply(x, lambda): {elapsed(t3)} s")
```

```
operator   x + 1           : 0.0008 s
method     x.apply(lambda) : 0.336-0.341 s
function   apply(x, lambda): 0.348-0.366 s
```

The method and function forms are statistically identical (confirming
dot-call sugar has zero overhead) — but both are **~420x slower** than the
operator, because `apply` calls back into the tree-walking evaluator once
per element (500,000 individual lambda invocations) instead of running the
native SIMD/rayon kernel `+` uses. This generalizes: any elementwise-style
helper written as "call a lambda per element" pays the interpreter-dispatch
cost per call no matter how it's spelled (`x.f(y)`, `f(x,y)`, a plain
`for` loop calling a function) — the only way out is a native vectorized
operator/builtin that never re-enters the evaluator per element. Named
arithmetic wrappers like `add`/`mul` (routing straight into the same
`binop` the operators use, not through `apply`) are the right way to get a
function-style spelling without this cost — but confirm that before relying
on it for any specific builtin: the *name* doesn't tell you which path it
takes, only reading (or timing) it does.

## Loop unrolling: measured, doesn't help

Manually unrolling a `for` loop body (processing 4 elements per iteration
instead of 1, same total number of additions either way):

```qu
N = 400000

t = timer(); start(t)
s1 = 0.0
for i = 1 to N
    s1 = s1 + i
end for
print("plain for (400000 iterations)      : {elapsed(t)} s")

t2 = timer(); start(t2)
s2 = 0.0
for i = 1 to N step 4
    s2 = s2 + i
    s2 = s2 + (i + 1)
    s2 = s2 + (i + 2)
    s2 = s2 + (i + 3)
end for
print("unrolled 4x (100000 iterations)    : {elapsed(t2)} s")
```

```
plain for (400000 iterations)   : 0.0040-0.0041 s
unrolled 4x (100000 iterations) : 0.0039-0.0041 s
```

Statistically indistinguishable. In a compiled language, unrolling can pay
off by amortizing branch/loop-control overhead across more work per check.
In Qu's tree-walking evaluator, per-*statement* execution cost dominates and
is roughly constant regardless of how the same total number of statements is
grouped into iterations — so the `for` fast path's own loop-control overhead
is already small enough that unrolling has nothing left to amortize.
**Don't bother**; it only adds source complexity for a real language design
that doesn't reward it (unlike the C/Fortran intuition many bring to this).

## Reference variables: Qu doesn't have them, and that's fine

Every Qu value is copy-on-write, always. There is no way to alias a
variable, and no way for a function to mutate its caller's argument:

```qu
x = [1.0, 2.0, 3.0]
y = x
y[0] = 99.0
print(x)   # [1, 2, 3]  -- untouched
print(y)   # [99, 2, 3]

function bump(v)
    v[0] = 999.0
    return v
end function

a = [1.0, 2.0]
b = bump(a)
print(a)   # [1, 2]    -- the caller's copy never changes
print(b)   # [999, 2]
```

If you're coming from C++/Fortran, there is no `&`-style "pass by reference
to avoid a copy" idiom to reach for here, and you don't need one: passing a
large `Vec`/`Mat` into a function does **not** deep-copy it. Every value is
`Arc`-backed under the hood, so passing it around is a cheap pointer-and-
refcount bump; an actual copy of the underlying buffer only happens at the
moment something tries to *mutate* a value that's still shared (copy-on-
write, the same technique Rust's own `Arc::make_mut`, and Clojure's
persistent collections, use). Write Qu the way you'd write it if copies
were free — because for anything that doesn't actually get written to,
they are.

## Compound assignment (`+=`, `-=`, ...): sugar today, not a guaranteed in-place win

The spec is explicit that `+= -= *= /= ^=` are *semantically* pure rebinding
— "read the current value, apply the op, then rebind — not an in-place
mutation" (language spec §12) — precisely so the copy-on-write guarantees
above never have an exception. That leaves room for the *engine* to
implement a compound assignment as a genuine in-place buffer write whenever
it can prove the buffer is uniquely owned (no other alias could observe the
difference) — free performance, zero semantic risk. Measured on this build,
accumulating into a 500,000-length vector 20 times:

```qu
acc = zeros(500000)
delta = ones(500000)

t = timer(); start(t)
for k = 1 to 20
    acc += delta
end for
print("+=            : {elapsed(t)} s")

acc2 = zeros(500000)
t2 = timer(); start(t2)
for k = 1 to 20
    acc2 = acc2 + delta
end for
print("acc = acc + d : {elapsed(t2)} s")
```

```
+=            : 0.0106-0.0117 s
acc = acc + d : 0.0106-0.0106 s
```

No measurable difference today — on this build, `+=` on a vector is plain
sugar for the rebinding form, not yet routed through an in-place write.
(A real Arc-unique-ownership in-place fast path for exactly this case has
been measured elsewhere in this project at ~1.38x and is in flight on an
active branch, not yet merged — worth re-measuring once it lands, but don't
budget on the speedup existing yet.)

## `parallel for`: real speedup, real restrictions

Running 200 independent CPU-heavy inner loops (20,000 `sin` calls each),
serial vs `parallel for`:

```qu
function heavy(seedval)
    acc = 0.0
    for j = 1 to 20000
        acc = acc + sin(seedval + j)
    end for
    return acc
end function

tic()
total1 = 0.0
for i = 0 to 199
    total1 = total1 + heavy(i)
end for
print("serial for   : {toc()} s")

tic()
results = zeros(200)
parallel for i in 0 to 199
    results[i] = heavy(i)
end parallel
print("parallel for : {toc()} s")
```

```
serial for   : 1.36-1.53 s
parallel for : 0.20-0.23 s     (6.6-7.6x, 16 logical cores)
```

A real win, once each iteration does enough work to be worth scheduling
onto the worker pool — `parallel for` shares the same pool `spawn`/`pmap`
use, so it doesn't pay thread-spawn cost per call. It won't help (and
shouldn't be reached for) on cheap per-iteration bodies where scheduling
overhead would dominate the actual work — that's a job for the vectorized
operators above instead.

**A restriction worth knowing before you hit it as a confusing error:**
each `parallel for` iteration runs against its own isolated copy of the
enclosing scope, merged back afterward only for plain `number`/`Vec`/`Mat`
reassignments. A `Timer`/`Model`/other handle-typed variable merely being
*visible* in that enclosing scope alongside the loop can trip a "can't be
safely merged" error even if the loop body never touches it — this cost me
a debugging detour while measuring the above. Workaround: keep handle-typed
values (timers, models, mutexes, files, ...) out of the scope wrapping a
`parallel for`, or use `tic()`/`toc()` (a global slot, not a value) instead
of a `timer()` object when timing code that contains one.

## The concurrency toolkit: what else there is besides `parallel for`

`parallel for` is the easy case (independent iterations, merge back into
`number`/`Vec`/`Mat` results). Qu's full concurrency surface is bigger, and
each piece is the right tool for a different shape of problem:

| Tool | Shape it fits |
|---|---|
| `parallel for` | Independent iterations over a range, results merge into plain values |
| `pmap(xs, "fn")` | Independent per-element work over an existing `List`/`Vec`, not a range |
| `spawn("fn", ...)` / `join`/`worker_done` | A handful of independent, possibly long-running, possibly differently-shaped jobs — fire-and-forget with a handle to collect later |
| `mutex`/`mutex_add`/`mutex_update` | Shared mutable state across workers (a running counter, a shared log) |
| `semaphore` | Bounding concurrency (at most N workers touching some limited resource at once) |
| `channel` | Message-passing between workers, rather than shared state |
| `queue`/`pool ... with cpu=N`/`run ... on` | A persistent, resource-limited pool with named/tagged jobs, rather than a one-off `spawn` |
| `listen_pool` + remote `pool ... remote=` | Distributing work across processes/machines, not just threads |

All of the thread-based ones (`spawn`, `pmap`, `parallel for`, named/
anonymous `pool`) share the same underlying `rayon` global thread pool and
the same **share-nothing isolation model**: each worker gets its own
snapshot of the calling script's globals/functions and a freshly-drawn RNG
stream, never the live shared environment — nothing a worker does can race
with the caller or another worker by accident. That's also why they're
"jobs," not "closures with side effects": a worker can't mutate the
caller's variables directly, only through an explicit shared handle
(`Mutex`/`Channel`/`Semaphore`) or its own return value.

Measured: `pmap` on the same 200-job, CPU-heavy workload as the `parallel
for` benchmark above (an existing `Vec` of inputs, not a range, so `pmap`
is the natural fit rather than `parallel for`):

```qu
xs = 0 to 199   # an existing Vec, not something you'd write a range-for over
tic()
r1 = map(heavy, xs)
print("serial map : {toc()} s")
tic()
r2 = pmap(xs, "heavy")
print("pmap       : {toc()} s")
```

```
serial map : 3.25-3.29 s
pmap       : 0.62-1.09 s     (~3-5x, noisier than parallel for's ~7x)
```

A real speedup, but less clean and more variable than `parallel for`'s —
`pmap` pays a fresh env/function-table snapshot cost per element rather
than per loop, which shows up more on 200 medium-sized jobs than it would
on fewer, heavier ones. If you can express the same work as a range,
`parallel for` is the more predictable choice; reach for `pmap` when the
work is naturally "already have a `List`, want it transformed," and for
`spawn`/`pool` when the jobs are few, heavy, and differently-shaped rather
than many small identical ones.

## Matmul: naive vs blocked vs parallel

Already measured and verified previously in this project (see `IMPL.md`'s
M6 acceleration entries) across a size sweep, naive vs a cache-blocked
single-thread kernel vs the same kernel parallelized:

| shape (m,k,n) | naive | blocked, 1 thread | blocked + parallel |
|---:|---:|---:|---:|
| 16×16×16 | ~1.5-1.7 us | ~0.35 us (4.7x) | ~0.30 us (4.1-4.3x) |
| 64×64×64 | ~40-71 us | ~17.8 us (4.0x) | ~16-17 us (2.5-2.7x) |
| 256×256×256 | ~0.8-4.0 ms | ~0.65 ms (6.1x) | ~0.68 ms (1.4-1.8x) |
| 600×600×600 | ~6.7-43.7 ms | ~5.8 ms (7.5x) | ~1.6-2.4 ms (3.7-4.9x) |
| 1024×1024×1024 | ~30-196 ms | ~26.1 ms (7.5x) | ~6.2-6.6 ms (4.8x) |

Two things worth noticing: cache-blocking wins at *every* size tested
(including tiny ones), while parallelism only pays for itself once there's
enough work per thread to outrun scheduling overhead — at 16×16×16 the
single-threaded blocked kernel actually beats the parallel one. `*`/`matmul`
picks the right strategy for you; this table is here so "why isn't my tiny
matmul faster with more cores" has an answer.

## GPU vs CPU: the transfer-time curse, made concrete

`gpu_matmul(A, B)` exists (needs a build with `--features gpu`) and gives
the exact same `A@B` as `*`, computed on the GPU — at **f32 precision**
(~7 significant digits), not `*`'s full f64. That precision drop is real
and permanent for that call, which is exactly why `*`/`matmul` never
auto-dispatches to the GPU regardless of size: silently trading precision
for speed on the language's default operator would be wrong for a
reproducibility-focused tool, so GPU use stays opt-in and explicit.

The "curse of transfer time" is not a metaphor — every GPU dispatch pays a
fixed cost (buffer upload, kernel dispatch, result readback) before a
single FLOP happens, and for small matrices that fixed cost dwarfs the
compute. Measured previously in this project on one machine (RTX 5080,
via `gpu_probe_info()`, real GPU dispatches — see `IMPL.md`'s GPU-probe
entry):

| work (`m*n*k`) | CPU | GPU | winner |
|---:|---:|---:|---|
| 2,097,152 (128³) | 0.124 ms | 3.005 ms | **CPU** — GPU loses to its own overhead |
| 16,777,216 (256³) | 0.603 ms | 0.390 ms | **GPU** ← measured crossover |
| 134,217,728 (512³) | 1.192 ms | 0.721 ms | GPU |
| 1,073,741,824 (1024³) | 5.122 ms | 4.995 ms | GPU |

At 128³ the GPU is **24x slower** than the CPU on the exact same math,
purely from dispatch/transfer overhead — the small-matrix case every naive
"just use the GPU" instinct gets burned by. The crossover point is not a
portable constant: it depends on the actual GPU/CPU/driver on the machine
running the script, which is why Qu doesn't hardcode one. `gpu_probe_info()`
measures it once per process, lazily, on whatever hardware is actually
running, and every internal size-gated GPU dispatch (`gram_matrix`,
`svm_cross_term`, t-SNE's Barnes-Hut path) reads that measured value rather
than a guess. Call it yourself before assuming GPU acceleration will help:

```qu
info = gpu_probe_info()
if info.available
    print("GPU crossover on this machine: {info.crossover} (m*n*k)")
else
    print("No GPU available in this build/on this machine")
end if
```

**Rule of thumb: GPU pays off only when a single call's `m*n*k` clears the
*measured* crossover on the machine actually running it — never assume it,
measure it, and expect it to be much higher than intuition suggests for
small-to-medium problems.**

## Sparse vs dense matrices: not implemented yet

Direct answer: **Qu has no sparse matrix type today.** Every `Mat` is a
dense, row-major buffer of `f64`, full stop — confirmed by searching the
entire engine and `qu-core` for any `SparseMat`/CSR/CSC storage type; there
is none. `compressed-sensing.md`'s `cs_recover`/`omp`/`fista`/etc. work with
sparse *signals* (vectors that are mostly zero, recovered from few
measurements) via a dense `Mat` sensing matrix `A` — a genuinely different
thing from a sparse matrix *storage format*, and no substitute for one.

Why this matters for performance: a dense `n×n` matrix costs `O(n²)` memory
and `O(n²)`-to-`O(n³)` compute regardless of how many entries are actually
zero. A finite-element stiffness matrix, a graph Laplacian, or a large
recommender-system interaction matrix is routinely >99% zero in real use —
on a dense `Mat`, Qu pays full price for all of it: real memory that isn't
there, and real FLOPs multiplying zero by numbers for no result. There is
currently no way in Qu to avoid that cost — not a slow path, an *absent*
one.

If workloads like that matter to you, this is a real, scoped engine
feature (CSR/CSC storage, sparse matmul/solve kernels, sparse-aware
`+`/`*`/`\`), not a small documentation gap — worth a explicit decision on
whether and when to build it, rather than assuming it exists because
"sparse" already appears elsewhere in the language (compressed sensing).
Flagging it here rather than quietly implying otherwise.
