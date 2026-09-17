# Standard Library Reference

The chapters in this section are function-by-function reference tables,
organized by domain. Unlike the Getting Started books (which teach concepts
in order, with prose and worked examples), these chapters aim for
**completeness of the function list** over depth of prose per function: each
table is produced by surveying the interpreter's own builtin dispatch table
(the `fn call_builtin` match statement in
`engine/crates/qu-interp/src/lib.rs`), so it reflects what actually runs
today rather than what the specification merely describes. Worked examples
are included for the most illustrative functions in each chapter, not for
every single one.

## Calling convention

Every builtin is called as `name(positional, args, key=value, ...)`. Most
accept a scalar, a `Vec`, or a `Mat`/`Signal` interchangeably and apply
elementwise where that makes sense — this is noted per-function only when
the behavior is not the obvious elementwise case. A trailing `style=`
argument bag (`color=`, `marker=`, `label=`, `method=`, ...) is used
extensively by the plotting and modeling builtins; where a whole family of
functions shares the same named-argument convention, it is described once at
the top of that chapter rather than repeated in every row.

## Chapters

- [Core Math & Linear Algebra](core-math.md) — elementwise math, reductions,
  shape/construction, and matrix decompositions.
- [Signal Processing & Filters](signal-processing.md) — transforms, filter
  design and application, spectral analysis, signal generation, and the
  private rLKK impedance-spectroscopy method.
- [Statistics & Machine Learning](statistics-ml.md) — descriptive statistics,
  regression, clustering, the fitted-model protocol (`fit`/`predict`/
  `score`), estimation theory (Kalman/EKF/UKF/particle filters), feature
  scaling, and random distributions.
- [Plotting](plotting.md) — figure/axes control, chart types, annotations,
  and export.
- [Images & Computer Vision](images.md) — loading, transforms, filtering,
  segmentation, and region analysis.
- [Collections, Strings & Data Frames](collections-strings.md) — string
  manipulation, bitwise operations, array utilities, and tabular data.
- [File I/O](file-io.md) — text, binary, CSV, and memory-mapped file access.
- [Concurrency & Workers](concurrency.md) — parallel execution, mutexes,
  semaphores, channels, and HTTP.
- [REPL, Diagnostics & Units](repl-diagnostics.md) — output/diagnostics,
  timers, autodifferentiation, RNG control, and physical-unit literals.

## Function Memoization

`memoize function name(params) ... end function` is a modifier on the
block `function` declaration (not a builtin, and not available on the
one-line `name(params) := expr` form) that caches the function's return
value, keyed by its argument values, so a repeated call with the same
inputs skips recomputing the body entirely:

```qu
memoize function slow_square(x)
    print("computing...")
    return x * x
end function

a = slow_square(7)   # prints "computing...", returns 49
b = slow_square(7)   # no print — served straight from cache, still 49
```

**Cache key**: an argument is only usable as a cache key if it's a plain
number, string, or bool — the same three kinds `==` compares directly. A
call where every argument is one of those is cached; a call with any other
argument (a `Vec`, `Mat`, `Table`, `Model`, `List`, `Record`, or any live
handle) still runs and returns the correct result, it's simply **never
cached** for that call (a silent, documented bypass — not an error). Two
numeric arguments are compared by exact bit pattern, so `-0.0` and `0.0`
are different cache entries.

**Scope and lifetime**: the cache lives on the running interpreter, not a
global/static — it's cleared naturally when the script ends, and each
isolated `spawn`/`pmap`/`parallel for` worker gets its own independent
cache, the same isolation every other per-worker state already has.
Re-declaring a memoized function (a bugfix, or a REPL re-run) drops every
cached entry for that name, so a stale result from the old body can never
be served afterward.

**Verifying it's working**: since the whole point is that the body does
NOT run again, the direct way to confirm memoization is happening is a
counter the body itself increments (as in the example above, replace
`print` with `calls = calls + 1`) — call the function twice with identical
arguments and check the counter only advanced once.

## Lazy Variables

`lazy name = expr` (§ lazy variables, 2026-08-31) is a variable-binding form
distinct from `memoize` above (which caches a *function's* result across
repeated calls) — a **lazy variable** defers evaluating its right-hand side
entirely: the expression does not run at the `lazy name = expr` statement at
all, only on the variable's first actual *read* afterward, and the computed
value is then cached in place so a second read never re-runs it. An ordinary
`name = expr` is unchanged and still evaluates immediately, exactly as
before — `lazy` is an explicit opt-in, not a new default.

```qu
lazy x = expensive_computation()   # nothing runs yet
print("declared, nothing computed yet")
y = x   # FIRST read: runs expensive_computation() now, caches the result
z = x   # second read: reuses the cached value, no recomputation
```

**Verifying it's deferred**: the same technique `memoize` above uses — a
counter (or a `print`) the expression's own body advances — confirms the
work happens on first read, not at the `lazy` statement:

```qu
counter = 0
function tick()
    counter = counter + 1
    return 42
end function

lazy x = tick()
before = counter        # 0 — tick() has not run
y = x                   # forces tick() now
after_first = counter   # 1
z = x                   # cached — does not call tick() again
after_second = counter  # still 1
```

**Reassignment before the first read**: `lazy x = expr` followed by a plain
`x = other` *before* `x` is ever read discards the pending expression
without ever evaluating it — the same behavior an ordinary eager
reassignment already has (`x = 1` then `x = 2` never looks at, or forces
anything from, the `1` it replaces). A lazy binding that hasn't been read
yet is treated exactly like any other value about to be overwritten, not as
a special case that must run first.

## What's out of scope here

Circuit simulation and distributed execution are specified but not yet
implemented as builtins (see [Book 3](../getting-started/book3-specialized.md)
and the [Language Reference](../language-reference/spec.md) for the design);
they have no entries in these tables because there is nothing to call yet.
