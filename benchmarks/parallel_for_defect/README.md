# `parallel for` clones its target array per iteration

Found 2026-09-11 while writing a `catalog/` demo for `with reduce(+: x)`.
The demo was not shipped: a demo whose measured result is a crash or a 0.01x
speedup advertises a defect rather than a feature.

## 1. Disjoint index writes abort the process

`oom_repro.qu`. Not a Qu runtime error — a raw Rust allocation abort:

```
out = zeros(400000)
parallel for i in 0 to 49999
    out[i] = sqrt(i + 1)
end parallel
```
```
memory allocation of 3200000 bytes failed
```

**The array is copied per iteration.** Iterations held fixed at 50,000,
growing only the array:

| array | elements | iterations | result |
|---|---|---|---|
| 400 KB | 50,000 | 50,000 | ok |
| 3.2 MB | 400,000 | 50,000 | **OOM abort** |
| 16 MB | 2,000,000 | 50,000 | **OOM abort** |

Memory scaling with array size at *constant* iteration count is only
possible if each iteration copies the whole array. 50,000 × 3.2 MB ≈ 160 GB,
which is the abort. Holding the array small and sweeping iterations instead,
it survives 50,000 and dies by 100,000.

This is the pattern the language recommends. The unsafe-reassignment error
tells you to use disjoint `x[i] = ...` writes rather than a reduction, and
that is the form that crashes.

## 2. `with reduce` is correct but 77x slower than serial

`reduce_is_slow.qu`. 1,000,000 iterations of `sqrt(i) * sin(i)`:

```
serial   : -1031.7532   in 0.0401 s
parallel : -1031.7532   in 3.0701 s
speedup  : 0.01x
```

The answer is right to the last digit — the reduction itself is sound. But
40 ns/iteration serial against 3070 ns/iteration parallel is not thread
overhead, it is a different execution path: the serial loop gets the
register fast path and the parallel body evidently does not. Probably the
same per-iteration environment cost as (1).

## Also worth knowing

`reduce` requires `with`. `parallel for i in 1 to n reduce(+: t)` is a parse
error, because `n reduce` lexes as a number with a unit attached (like
`100 Hz`) before it can lex as a clause. The parser says so in a comment;
the error message does not.
