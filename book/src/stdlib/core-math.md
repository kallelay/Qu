# Core Math & Linear Algebra

The tables below are generated from the interpreter's own builtin dispatch
table (the `fn call_builtin` match statement in
`engine/crates/qu-interp/src/lib.rs`), not written speculatively ahead of the
implementation — every function listed here runs today. Functions covering
regression, clustering, and probability *models* (`polyfit`, `ridge`,
`kmeans`, `pca`, `normpdf`, `mvnpdf`, ...) are documented in
[Statistics & Machine Learning](statistics-ml.md) instead, to avoid splitting
that workflow across two chapters.

## Example data

Names used by the examples below, defined here so each one runs.

```qu
x = linspace(0, 1, 12)
a = randn(3, 4, seed = 6)
b = randn(3, 4, seed = 7)
v = [1, 2, 3, 4, 5, 6]
n = 4
r = 3
c = 4
df = table(price = [10.0, 12.5, 9.75, 14.0], volume = [100, 80, 140, 60])
```

## Elementwise Math

All of these accept a scalar, a `Vec`, or a `Mat`/`Signal` and apply
elementwise; a handful (`abs`, `angle`/`arg`/`phase`, `real`/`re`, `imag`/
`im`, `conj`) are complex-aware and branch on whether the input is a real or
complex value/array.

| Function | Signature | Description |
|---|---|---|
| `sin`, `cos`, `tan` | `sin(x)` | Standard trigonometric functions, computed elementwise. `x` is the input angle in radians — a scalar, a `Vec` of any length, or a `Mat`/`Signal` of any shape. Returns exactly the same type and shape as `x`, one trig value per element. |
| `asin`, `acos`, `atan` | `asin(x)` | Inverse trigonometric functions, single-argument only (there is no two-argument `atan2` in this table). `x` is a scalar, `Vec`, or `Mat`/`Signal`; `asin`/`acos` expect values in `[-1, 1]` and return radians in `[-π/2, π/2]`/`[0, π]` (an out-of-domain element gives `NaN`), while `atan` accepts any real value and returns radians in `(-π/2, π/2)`. Return shape always matches `x`. Returns exactly the same type and shape as `x`, one angle per element. |
| `sinh`, `cosh`, `tanh` | `sinh(x)` | Hyperbolic sine/cosine/tangent, elementwise. `x` is a scalar, `Vec`, or `Mat`/`Signal`; returns the same shape, one hyperbolic value per input element. Returns exactly the same type and shape as `x`. |
| `coth`, `sech`, `csch` | `coth(x)` | The other three hyperbolics: `coth(x) = cosh(x)/sinh(x)`, `sech(x) = 1/cosh(x)`, `csch(x) = 1/sinh(x)`. `x` is a real number or `Complex`. `coth`/`csch` have a pole at `x=0` (and, for a `Complex` argument, at every `x = i*k*pi`) — evaluating there is an **error**, not a silent `inf`/`NaN`. `sech` has no real pole (`cosh` is never zero). |
| `asinh`, `acosh`, `atanh` | `asinh(x)` | Inverse hyperbolics. `asinh` is defined and finite on the whole real line. `acosh` is only real for `x >= 1`; a real `x < 1` gives `NaN` (same convention as `sqrt` on a negative real — no silent promotion to a complex result), but an explicit `Complex` argument still evaluates via the complex branch regardless of value. `atanh` has a pole at `x = +-1` (real or complex) — an **error**, not a silent `inf`. All three accept `Complex` as well as real input. |
| `exp` | `exp(x)` | `e^x`, elementwise. `x` is a scalar, `Vec`, or `Mat`/`Signal`; returns the same shape with every element exponentiated. Returns exactly the same type and shape as `x`. |
| `exp2` | `exp2(x)` | `2^x`, elementwise, real only. `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`. |
| `expm1` | `expm1(x)` | `e^x - 1`, computed so it stays accurate for `x` near zero (`expm1(1e-15)` does not lose precision to the cancellation that `exp(x) - 1` would hit there). `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`. |
| `log`, `ln` | `ln(x)` | Natural logarithm, elementwise (`log` is an alias for `ln` — it is **not** base-10; see `log10` for that). `x` is a scalar, `Vec`, or `Mat`/`Signal`; a non-positive element gives `NaN`/`-inf` rather than an error. Returns the same shape as `x`. |
| `log1p` | `log1p(x)` | `ln(1 + x)`, computed so it stays accurate for `x` near zero (`log1p(1e-15)` does not underflow to exactly `0`, unlike naively computing `1.0 + x` first). `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`. |
| `log10`, `log2` | `log10(x)` | Base-10 / base-2 logarithm, elementwise, same domain behavior as `ln` on a non-positive element. `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`. |
| `sqrt` | `sqrt(x)` | Elementwise square root. `x` is a scalar, `Vec`, or `Mat`/`Signal`; a negative element gives `NaN` (there is no automatic promotion to a complex result). Returns the same shape as `x`. |
| `cbrt` | `cbrt(x)` | Real cube root, elementwise — unlike `x^(1/3)`, correctly handles a negative `x` (`cbrt(-8) = -2`, not `NaN`). `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`. |
| `gamma`, `lgamma` | `gamma(x)` | The gamma function (`gamma(n) = (n-1)!` for a positive integer `n`) and its natural log (`lgamma(x) = ln\|gamma(x)\|`). `x` is a real scalar, `Vec`, or `Mat`/`Signal`. Both have a pole at every non-positive integer (`0, -1, -2, ...`) — evaluating there is an **error**, not a silent `inf`/`NaN`. |
| `erf`, `erfc` | `erf(x)` | The error function and its complement (`erfc(x) = 1 - erf(x)`, computed directly rather than by subtraction so it stays accurate for large `x`, where `erf(x)` itself has already saturated to `1.0`). `x` is a real scalar, `Vec`, or `Mat`/`Signal`; defined everywhere, no pole. |
| `abs` | `abs(x)` | Absolute value on a real scalar/`Vec`/`Mat`; magnitude (`sqrt(re^2 + im^2)`) on a `Complex` scalar, `CVec`, or `CMat`. `x` is the value(s) to measure. Returns the same shape as `x` but always real-valued — a length-N `CVec` in gives a length-N real `Vec` out. |
| `angle`, `arg`, `phase` | `angle(x)` | Complex argument — the angle from the positive real axis, in radians (`atan2(im, re)`) — on a `Complex`/`CVec`/`CMat`; on a signed real scalar or real `Vec`/`Mat` it degrades to `0` for a non-negative value or `π` for a negative one. `x` is the value(s) to measure. Returns the same shape as `x` but always real-valued — a length-N `CVec` in gives a length-N real `Vec` out, and a `Complex` scalar gives a `Num`. |
| `real`, `re` | `re(x)` | Real component of a `Complex`/`CVec`/`CMat` (identity — `x` passed through unchanged — when `x` is already real). `x` is a scalar, `Vec`, or `Mat` (real or complex). Returns the same shape as `x` but always real-valued — a `Complex` gives a `Num`, a `CVec` a real `Vec`, a `CMat` a real `Mat`. |
| `imag`, `im` | `im(x)` | Imaginary component of a `Complex`/`CVec`/`CMat` (`0` elementwise when `x` is real). `x` is a scalar, `Vec`, or `Mat` (real or complex). Returns the same shape as `x` but always real-valued — a `Complex` gives a `Num`, a `CVec` a real `Vec`, a `CMat` a real `Mat`. |
| `conj` | `conj(x)` | Complex conjugate (negates the imaginary part); passes a real `x` through unchanged. `x` is a scalar, `Vec`, or `Mat` (real or complex). Returns exactly the same type and shape as `x` — a `Complex` stays a `Complex`, a real `Vec` stays a real `Vec`. |
| `floor`, `ceil`, `round` | `floor(x)` | Standard rounding functions, elementwise: `floor` toward negative infinity, `ceil` toward positive infinity, `round` to the nearest integer (half away from zero). `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`, rounded elementwise. |
| `sign` | `sign(x)` | The sign of each element, using Rust's `f64::signum` semantics: `-1` for a negative value (including `-0.0`), `+1` for a non-negative value (including `+0.0` — so `sign(0)` returns `1`, never `0`, a real edge case worth knowing about), and `NaN` for a `NaN` input. `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns exactly the same type and shape as `x`, one sign per element. |

#### Overloads: `abs`

#### Case: scalar

`x` a real number. Returns a `Num`, the standard real absolute value.

```qu
print(abs(-5))   # 5
```

#### Case: vector

`x` a `Vec`. Returns a `Vec` of the same length, `abs` applied elementwise.

```qu
print(abs([-2, -0.5, 0.5, 2]))   # [2, 0.5, 0.5, 2]
```

#### Case: complex

`x` a `Complex` value (the `i`/`j`-suffix literal). Returns a real `Num` —
the magnitude `sqrt(re^2 + im^2)`, not either component alone — the case
that most often surprises newcomers.

```qu
print(abs(3 + 4j))   # 5 — magnitude, not the real part
```

#### Overloads: `angle`

#### Case: real

`x` a signed real number (scalar, `Vec`, or `Mat`). Degrades to `0` for a
non-negative value or `π` for a negative one — there is no real angle to
compute, so this is a convention, not a measurement.

```qu
print("angle(5)={angle(5)}  angle(-5)={angle(-5)}")
```

#### Case: complex

`x` a `Complex` value. Returns the true angle from the positive real axis,
`atan2(im, re)`, in radians.

```qu
z = 3 + 4j
print(angle(z))   # 0.927295
```

#### Overloads: `re`

#### Case: real

`x` a real scalar, `Vec`, or `Mat`. Identity — passed through unchanged.

```qu
print(re(5))   # 5
```

#### Case: complex

`x` a `Complex` value. Returns just the real component, discarding the
imaginary part.

```qu
print(re(3 + 4j))   # 3
```

#### Overloads: `im`

#### Case: real

`x` a real scalar, `Vec`, or `Mat`. Always `0`, since a real value has no
imaginary part.

```qu
print(im(5))   # 0
```

#### Case: complex

`x` a `Complex` value. Returns just the imaginary component.

```qu
print(im(3 + 4j))   # 4
```

#### Overloads: `conj`

#### Case: real

`x` a real scalar, `Vec`, or `Mat`. Identity — passed through unchanged,
since conjugation is a no-op on a value with no imaginary part.

```qu
print(conj(5))   # 5
```

#### Case: complex

`x` a `Complex` value. Negates the imaginary part, leaving the real part
alone.

```qu
print(conj(3 + 4j))   # 3 - 4i
```

#### `sign`'s exact-zero case

`sign` has its own exact-zero edge case worth seeing separately: it
returns `+1`, never `0`, per Rust's `f64::signum`.

```qu
print(sign([-2, -0.5, 0.5, 2]))   # [-1, -1, 1, 1] — sign(0) would be +1, never 0
```

The remaining functions in this table — the rest of the trig and
inverse-trig family, the hyperbolic functions, the log family, the
complex-component accessors (`arg`/`phase`/`re`/`im`/`conj`), and `ceil` —
all follow the exact same elementwise pattern already shown above: one
function in, one value of the same shape out, with only the domain notes
already given in the table above to keep in mind. The example below runs
each of them once on a scalar (or, for the complex ones, a single complex
literal) so their actual return values are visible side by side.

```qu
x = 0.5
print("tan={tan(x)}  asin={asin(x)}  acos={acos(x)}  atan={atan(x)}")
print("sinh={sinh(x)}  cosh={cosh(x)}  tanh={tanh(x)}")
print("ln(e^2)={ln(exp(2))}  log(e^2)={log(exp(2))}  log2(8)={log2(8)}")

z = 3 + 4j
print("arg={arg(z)}  phase={phase(z)}  re={re(z)}  imag={imag(z)}  im={im(z)}  conj={conj(z)}")

print(ceil(2.1))   # 3
```

## Reductions & Descriptive Statistics

Basic order-statistic and spread reductions. `corr`, `cov`, `mode`,
`skewness`, and `kurtosis` are documented in
[Statistics & Machine Learning](statistics-ml.md). Several of the pure
reductions below (`sum`, `prod`, `mean`, `std`, `var`, `max`, `min`,
`argmax`, `argmin`, `median`, `quantile`) additionally accept an `axis=`
keyword when `x` is a `Mat`: `axis=0` collapses each column to one number
(a length-`cols` `Vec`), `axis=1` collapses each row (a length-`rows`
`Vec`), instead of flattening the whole matrix to a single scalar.

`mean`, `min`, `max`, `sum`, `std`, `var`, `median`, `quantile`, `argmin`,
and `argmax` also accept `on_invalid=` — R's `na.rm=` for a NaN or
Infinity in the input, extended to cover Inf too: `on_invalid="propagate"`
(the default, unchanged from before this keyword existed) lets a NaN/Inf
flow through untouched; `on_invalid="ignore"` drops every NaN/Inf element
before reducing (for `mean`/`std`/`var` this also shrinks the `n` the
average/variance divides by, since it is computed over the already-
filtered values; for `argmin`/`argmax` the returned index still refers to
the position in the ORIGINAL, unfiltered `x`); `on_invalid="error"` raises
instead of silently returning a NaN/Inf result, naming what was found and
at what index. On a `Mat` under `axis=`, `"ignore"` drops NaN/Inf
independently within each row/column, not across the whole matrix.

| Function | Signature | Description |
|---|---|---|
| `sum` | `sum(x, [on_invalid=])` | Sum of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal` (real; additionally accepts a `CVec`/`CMat` and returns a `Complex` in that case). With no other argument, reduces to one scalar `Num` (or `Complex`). On a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead, returning a `Vec` of length `cols` (`axis=0`) or `rows` (`axis=1`). `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling, including for a `CVec`'s complex elements — a NaN/Inf in either the real or the imaginary part counts. Returns a scalar `Num` — a `Complex` where `sum` reduced a `CVec`/`CMat` — or that `Vec` under `axis=`. |
| `prod` | `prod(x)` | Product of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, real only (unlike `sum`, does not accept a `CVec`/`CMat`). With no other argument, reduces to one scalar `Num`. On a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead, returning a `Vec` of length `cols`/`rows`. `prod` does NOT accept `on_invalid=` — that keyword was added to `sum` and the other nine reductions listed in this section's intro, not to `prod`. Returns a scalar `Num`, or that `Vec` under `axis=`. |
| `mean` | `mean(x, [on_invalid=])` | Arithmetic mean of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`. With no other argument, flattens `x` to one scalar `Num` (errors on an empty vector). On a `Mat`, the optional `axis=` keyword (`0` or `1`) computes the mean along just that dimension, returning a length-`cols` or length-`rows` `Vec` — see `row_mean` below for a dedicated column-`Mat`-returning version of the row case. `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling; under `"ignore"` the divisor is the count of elements that survive filtering, not the original length. Returns a scalar `Num` with no `axis=`, or that `Vec` with one. |
| `max`, `min` | `max(x, [on_invalid=])` or `max(a, b)` | One argument reduces `x` (a scalar, `Vec`, or `Mat`/`Signal`) to its largest/smallest element, a scalar `Num`, and accepts `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro). Two arguments `a`, `b` (matching or broadcastable shapes) instead compare elementwise and keep the larger/smaller of each pair — the "clip" idiom, e.g. `max(x, 0)`; `on_invalid=` does not apply to this two-argument form. In the one-argument form on a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead, returning a length-`cols`/`rows` `Vec` rather than a single scalar. |
| `argmin`, `argmax` | `argmin(x, [on_invalid=])` | 0-based index of the first-occurring minimum/maximum element of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened). Returns a scalar `Num` index. `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling; under `"ignore"` the returned index still refers to a position in the original, unfiltered `x` (or, under `axis=`, the original row/column), so it can be used to index straight back into the caller's own data. On a `Mat`, the optional `axis=` keyword (`0` or `1`) instead returns the index of the extremum *within* each column or row — a `Vec` of length `cols`/`rows`, each entry an index into that column/row rather than into the flattened matrix. |
| `median` | `median(x, [on_invalid=])` | Middle order statistic of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened): the middle element for an odd-length input, the average of the two middle elements when `len(x)` is even. `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling. On a `Mat`, the optional `axis=` keyword (`0` or `1`) computes the median along just that dimension instead of flattening. Returns a scalar `Num` with no `axis=`, or a length-`cols`/`rows` `Vec` with one. |
| `quantile` | `quantile(x, q, [on_invalid=])` | Linear-interpolation quantile (NumPy's default `interpolation="linear"` method). `x` is a scalar, `Vec`, or `Mat`/`Signal` (flattened); `q` is a scalar rank in `[0, 1]` (`0.5` is the median), interpolated between the two nearest order statistics when it doesn't land exactly on one. `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling. On a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead of flattening. Returns a scalar `Num` with no `axis=`, or a length-`cols`/`rows` `Vec` with one. |
| `percentile` | `percentile(x, p)` | Same computation as `quantile`, with `p` on a `0..100` scale instead of `0..1` (`p=50` is the median). `x` is a scalar, `Vec`, or `Mat`/`Signal`; `p` must lie in `[0, 100]` or the call errors. Returns a scalar `Num`. |
| `iqr` | `iqr(x)` | Interquartile range: `percentile(x,75) - percentile(x,25)` — a spread measure robust to outliers, unlike `std`/`var`. `x` is a scalar, `Vec`, or `Mat`/`Signal`. Returns a scalar `Num`. |
| `argmedian` | `argmedian(x)` | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) of the element sitting at the median position. Because an even-length `x` has no single median element, this returns the lower of the two middle indices — a deliberate, documented tie-break (NumPy has no `argmedian`, precisely for this reason). Returns a scalar `Num` index (0-based). |
| `argquantile` | `argquantile(x, q)` | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) nearest the `q`-quantile rank, computed as `round(q*(n-1))` where `n = len(x)`; `q` is a scalar in `[0, 1]`. Unlike `quantile`, which may interpolate between two elements, this always lands on a real element of `x`. Returns a scalar `Num` index (0-based). |
| `argmean` | `argmean(x)` | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) of the element closest to `mean(x)` (ties broken by first occurrence) — not a true order-statistic index like the others above, since the mean generally isn't any actual data point. Returns a scalar `Num` index (0-based); errors on an empty `x`. |
| `clip`, `clamp` | `clip(x, lo, hi)` | Elementwise clamp of `x` (a scalar, `Vec`, or `Mat`/`Signal`) into `[lo, hi]`. `lo` and `hi` are scalars (or broadcastable arrays); both are optional, defaulting to `-inf`/`+inf` respectively, so e.g. `clip(x, 0)` floors at zero and leaves the top end open. Returns the same shape as `x`. |
| `std` | `std(x, [on_invalid=])` | Sample standard deviation (N-1, unbiased, denominator) of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`. With no other argument, flattens `x` to one scalar `Num`. On a `Mat`, the optional `axis=` keyword (`0` or `1`) computes the standard deviation down each column or across each row instead, returning a length-`cols`/`rows` `Vec`. `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling; under `"ignore"` the divisor is the count of elements that survive filtering. Returns a scalar `Num` with no `axis=`, or that `Vec` with one. |
| `var` | `var(x, [on_invalid=])` | Sample variance, same N-1 (unbiased) denominator as `std`, so `var(x) == std(x)^2` always holds. `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened. On a `Mat`, the optional `axis=` keyword (`0` or `1`) computes the variance down each column or across each row instead, returning a length-`cols`/`rows` `Vec` (same support as `std`). `on_invalid=` (`"propagate"`/`"ignore"`/`"error"`, see this section's intro) controls NaN/Inf handling. Returns a scalar `Num` with no `axis=`, or that `Vec` with one. |
| `rms` | `rms(x)` | Root-mean-square, `sqrt(mean(x.^2))`, of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, flattened. Returns a scalar `Num`. Errors on an empty vector. |
| `peak` | `peak(x)` | `max(abs(x))` over every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, flattened. Returns a scalar `Num`. Errors on an empty vector. |
| `crest_factor` | `crest_factor(x)` | `peak(x) / rms(x)`, a dimensionless ratio (a sine wave's is `sqrt(2)` ≈ 1.414) — the same quantity `sinad_estimate(bits, crest_factor)` takes as its second argument. `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened. Returns a scalar `Num`. Errors on an empty vector or one whose RMS is zero (silence has no crest factor). |
| `dbfs` | `dbfs(x, [full_scale=1.0])` | Peak level relative to full scale: `20*log10(peak(abs(x)) / full_scale)`. `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened; `full_scale` (optional number, default `1.0`) must be positive. An uncalibrated digital signal can answer in dBFS with no physical unit attached — anything past that (dB SPL, a sensitivity-based unit) needs a calibration this engine does not carry yet. Returns a scalar `Num`. Errors on an empty vector. |
| `norm` | `norm(x)` | Euclidean (L2) norm; for a matrix this is the Frobenius norm (every element treated as one long vector), not a matrix operator norm. `x` is a scalar, `Vec`, or `Mat` (real, or `CVec`/`CMat`, which norm by magnitude). Returns a scalar `Num`. |
| `logsumexp`, `lse` | `lse(x)` | Numerically stable `log(sum(exp(x)))`, via the usual max-shift so a large input can't overflow `exp`. `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened. Returns a scalar `Num` — `-inf` for an empty `x`. |
| `smoothmax` | `smoothmax(x, beta=1)` | Smooth (differentiable) approximation to `max(x)`: `(1/beta) * logsumexp(beta * x)`, which tracks the true maximum more tightly as `beta` grows (at the cost of a stiffer gradient). `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened; `beta` is an optional positional scalar defaulting to `1` (a non-positive or non-finite `beta` falls back to the plain hard `max`). Returns a scalar `Num`. |
| `softmax` | `softmax(x)` | Elementwise `exp(x_i) / sum(exp(x))` — the gradient of `logsumexp`, and a probability distribution that sums to 1. `x` is a scalar, `Vec`, or `Mat`/`Signal`, flattened. Returns a `Vec` the same length as `x` whatever `x`'s original shape was — a `Mat` in still gives a flat `Vec` out, and an empty `x` an empty `Vec`. |

#### Overloads: `sum`

#### Case: real

`x` a real scalar, `Vec`, or `Mat`/`Signal`. Returns a real scalar `Num`.

```qu
print(sum([1, 2, 3]))   # 6
```

#### Case: complex

`x` a `CVec` or `CMat`. Returns a `Complex` scalar instead of a `Num` —
the one reduction in this table that changes its own return *type* based
on its input, rather than just its shape.

```qu
print(sum([1 + 1j, 2 + 2j]))   # 3 + 3i
```

#### Overloads: `max`

| Call | What it returns |
|---|---|
| `max(x)` | Largest element of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened) — a single scalar `Num`. |
| `max(a, b)` | Elementwise max of `a` and `b` (matching or broadcastable shapes) — the "clip" idiom, e.g. `max(x, 0)`. Returns the broadcast shape, not a scalar. |

```qu
print(max([1, 5, 3]))      # 5 -- one-argument reduction
print(max([1, 5, 3], 4))   # [4, 5, 4] -- two-argument elementwise clip
```

#### Overloads: `min`

| Call | What it returns |
|---|---|
| `min(x)` | Smallest element of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened) — a single scalar `Num`. |
| `min(a, b)` | Elementwise min of `a` and `b` (matching or broadcastable shapes) — the same "clip" idiom as `max`, from the other side. |

```qu
print(min([1, 5, 3]))      # 1 -- one-argument reduction
print(min([1, 5, 3], 4))   # [1, 4, 3] -- two-argument elementwise clip
```

The next example runs the everyday descriptive-statistics toolkit —
`mean`, `std`, `iqr`, and `percentile` — over a batch of 1000
standard-normal draws from `randn`, the kind of quick numeric summary a
script reaches for before deciding whether a distribution looks the way
it should. `percentile(x, 95)` is included specifically to show the
0-100 scale, as opposed to `quantile`'s 0-1 scale for the same
computation.

```qu
samples = randn(1000, 1)
print("mean={mean(samples)}  std={std(samples)}  iqr={iqr(samples)}")
print("p95={percentile(samples, 95)}")
```

The second batch below covers the functions that don't fit neatly into
the first example: `prod` (multiplicative reduction), the whole `arg*`
index family (`argmedian`, `argquantile`, `argmean` — each returning a
*position* into the data rather than a value), `clamp`'s three-argument
clipping, and the `var`/`std` relationship (`var` is always `std`
squared, which the last line checks directly), plus `lse` as the
numerically stable form of `log(sum(exp(x)))`.

```qu
v = [5.0, 3.0, 8.0, 1.0, 9.0]
print("prod={prod(v)}")
print("argmedian={argmedian(v)}  argquantile(0.9)={argquantile(v, 0.9)}  argmean={argmean(v)}")
print("clamp={clamp([-5, 0, 5, 10], 0, 5)}")
print("var={var(v)}  std^2={std(v)^2}")
print("lse={lse(v)}")
```

## Shape, Construction & Introspection

| Function | Signature | Description |
|---|---|---|
| `rows`, `cols` | `rows(A)` | Row / column count of `A`. On a `Vec` (shape `(n, 1)`), a `Mat`/`Signal` (shape `(r, c)`), or a bare scalar (shape `(1, 1)`), returns the corresponding dimension as a scalar `Num`. On a `Table`, returns the same values as `nrow(df)`/`ncol(df)` — row count is the record count, column count the field count. Returns a scalar `Num`. |
| `size`, `shape` | `shape(A)` | The full shape of `A` as a 2-element `Vec`, `[rows, cols]`, using the same per-type shape rules as `rows`/`cols` above. `size` is the same builtin — MATLAB's name for it. (`sizeof` is a third name for it and is deprecated: it means a byte count everywhere else). **An `Image` is the one exception and reports `[width, height]`**, not rows-first — because an image's own constructors are width-first (`image_new(width, height)`, `resize(img, width, height)`, `crop(img, x0, y0, width, height)`), and answering rows-first made `image_new(640, 480)` report `[480, 640]`. A `Mat` of pixels is still a matrix and still `[rows, cols]`. Returns a 2-element `Vec`. |
| `length`, `numel` | `numel(x)` | Total element count of `x`: `1` for a scalar, its length for a `Vec`/`CVec`/`Signal`, `rows*cols` for a `Mat`, a Unicode-aware character count for a `Str`, and the element/node count for `List`/`Record`/`Dict`/`fifo`/`linked_list`/`graph`. Returns a scalar `Num`. |
| `zeros` | `zeros(n)` / `zeros(r, c)` | A vector or matrix of zeros. One numeric argument `n` gives a length-`n` `Vec`; two arguments `r`, `c` give an `r×c` `Mat` — even a degenerate two-argument shape like `zeros(n, 1)` stays a genuine oriented matrix, distinct from the plain `Vec` that `zeros(n)` returns. Overloaded by argument type: `zeros(filt)`, where `filt` is a `"filter"` model (from `butter`/`fir1`/etc.), instead returns that filter's zeros as a `CVec` (see [Signal Processing & Filters](signal-processing.md)). |
| `ones` | `ones(n)` / `ones(r, c)` | A vector or matrix of ones, with the same one-argument-vs-two-argument shape rules as `zeros` above. Returns a length-`n` `Vec` for `ones(n)`, or an `r×c` `Mat` for `ones(r, c)`. |
| `eye`, `identity` | `eye(n)` | The `n×n` identity matrix (ones on the diagonal, zeros elsewhere). `n` is a single non-negative integer. `identity` is an alias for `eye`. Returns an `n×n` `Mat`. |
| `reshape` | `reshape(x, r, c)` | Reinterprets a value as a matrix of a given shape. `x` is the value being reshaped, a scalar, `Vec` or `Mat`; `r` (number) is the row count of the result; `c` (number) is the column count, and `r * c` must equal the number of elements `x` holds. This is a genuine element relayout (column-major) when `x` is a scalar or `Vec`, or when `x` is already an `r×c` `Mat`; when `x` is a `Mat` with one dimension equal to `1` it is also a real reshape. **Reshaping a matrix that has more than one row *and* more than one column into a different two-dimensional shape does not relayout the data** — it falls through to a broadcast check instead and errors unless the two shapes are broadcast-compatible, a real, confirmed engine limitation rather than a documentation simplification. `expr -> (r, c)` is postfix-style sugar for exactly this call — see below. |
| `hstack`, `vstack` | `hstack(a, b, ...)` | Named equivalents of bracket-literal block concatenation (`[a, b]` horizontally / `[a; b]` vertically). Each of the 2+ required arguments is a scalar, `Vec`, or `Mat` (real or complex — any complex argument makes the whole result complex); `hstack` needs matching row counts, `vstack` needs matching column counts. Returns a `Mat` (or `CMat` if any input was complex). |
| `cat` | `cat(dim, a, b, ...)` | MATLAB-style concatenation: `dim` (a scalar, `1` or `2`) selects the axis, then the 2+ remaining arguments (same shape rules as `hstack`/`vstack`) are joined along it — `dim=1` vertical (same as `vstack`), `dim=2` horizontal (same as `hstack`). Returns a `Mat`. Taking the axis first means `cat` is the one array builtin you cannot write as `a.cat(b)` or `a |> cat(...)` — the receiver would land in the axis slot. Use `hstack`/`vstack` for those forms; they do the same job with the array first. |

#### Overloads: `zeros`

| Call | What it returns |
|---|---|
| `zeros(n)` | A length-`n` `Vec` of zeros. |
| `zeros(r, c)` | An `r×c` `Mat` of zeros — even `zeros(n, 1)` stays a genuine oriented matrix, distinct from `zeros(n)`. |
| `zeros(filt)` | `filt` a `"filter"` model (from `butter`/`fir1`/etc.), not a count at all — dispatch here is by argument *type*, not count. Returns that filter's zeros as a `CVec`. |

```qu
print(zeros(3))                              # [0, 0, 0]
print(zeros(2, 3))                           # [0, 0, 0; 0, 0, 0]
filt = butter(4, "low", 100, 1000)
print(zeros(filt))                           # [-1, -1, -1, -1] -- a CVec, not a fresh zero-fill
```

#### Overloads: `reshape`

| Call | What happens |
|---|---|
| `reshape(x, r, c)` where `x` is a scalar, `Vec`, or a `Mat` with one dimension equal to `1` | A genuine element relayout (column-major) into the new `r×c` shape. |
| `reshape(x, r, c)` where `x` is a `Mat` with more than one row **and** more than one column | **Not a relayout.** Falls through to a broadcast check instead and errors unless the two shapes are broadcast-compatible — a real, confirmed engine limitation, not a documentation simplification. |

```qu
v = [1, 2, 3, 4, 5, 6]
print(reshape(v, 2, 3))        # [1, 3, 5; 2, 4, 6] -- vector source, real relayout
```

`reshape(A, 3, 2)` on the general `2×3` matrix `A = [1, 2, 3; 4, 5, 6]`
from the earlier example, by contrast, does not relayout the data — it
hits the broadcast check and errors:

```
qu: runtime error: shapes (2x3) and (3x2) do not broadcast (each axis must match or be 1)
```

The example below builds a small matrix and reads its shape back with
`size`/`numel`, constructs identity matrices with both spellings
(`eye`/`identity`), and shows the two `cat` orientations side by side —
`dim=1` stacks the two vectors as new rows, `dim=2` lays them end to end
as one longer row — the same distinction `vstack`/`hstack` make under
more MATLAB-flavored names.

```qu
A = [1, 2, 3; 4, 5, 6]
print("size={size(A)}  numel={numel(A)}")
print(eye(3))
print(identity(2))
p = [1, 2]
q = [3, 4]
print(cat(1, p, q))   # vertical: stacks rows
print(cat(2, p, q))   # horizontal: concatenates
```

### `->` reshape operator

`expr -> (rows, cols)` is sugar for `reshape(expr, rows, cols)` — rewritten
to that exact call at parse time (no separate evaluation path, so results
are always identical to calling `reshape` directly). Scoped to the
2-argument form only (matching `reshape`'s own signature); there is no
1-argument `-> n` shorthand. Because `reshape` itself only truly relays
out data for a vector-like source (see the `reshape` row above), chaining
two reshapes only works when the intermediate shape keeps one dimension
equal to `1` — reshaping a genuinely two-dimensional intermediate result
into yet another two-dimensional shape hits the same broadcast-only
limitation and errors.

Precedence: `->` sits *above* `|>` (pipe) but *below* `as`/`??`/the ternary
`? :` — it takes the **entire** preceding pipe/arithmetic expression as its
value operand, not just the nearest operand:

```qu
v = [1, 2, 3, 4, 5, 6]
m = v -> (2, 3)                  # same as reshape(v, 2, 3)
print(m)
mp = df.price -> (n, 1)          # field access resolves first: (df.price) -> (n, 1)
print(mp)
s = a + b -> (r, c)              # reshapes the SUM: (a + b) -> (r, c)
print(s)
r2 = x |> normalize -> (r, c)    # reshapes the PIPED RESULT: (x |> normalize) -> (r, c)
print(r2)
chained = v -> (6, 1) -> (2, 3)  # left-associative: reshape twice, via an intermediate column shape
print(chained)
```

## Rearranging Arrays

| Function | Signature | Description |
|---|---|---|
| `val` | `val(x)` | Materializes a value; currently the identity on any `x` (`:=` is already eager pre-M4) but will force evaluation of a deferred binding once M4 lazy fusion lands. Returns `x` unchanged, same type and shape. |
| `transpose` | `transpose(A)` | Matrix/vector transpose: swaps rows and columns. `A` is a `Vec` (treated as a single row or column) or `Mat`/`CMat`; an `r×c` input returns a `c×r` output of the same element type. Returns a `Mat`, or a `CMat` for a complex input — including for a `Vec` argument, which comes back as an `n×1` column `Mat` rather than a `Vec`. |
| `ctranspose` | `ctranspose(A)` | Conjugate transpose (Hermitian): transpose plus elementwise complex conjugation. `A` is a `Vec`/`Mat`/`CMat`; an `r×c` input returns a `c×r` output (real inputs behave exactly like `transpose`, since conjugation is a no-op on real numbers). Returns a `Mat`, or a `CMat` for a complex input. |
| `flipud`, `fliplr`, `mirror`, `flip` | `flip(A, dim=1)` | Reverses row order (`flipud`, `dim=1`) or column order (`fliplr`/`mirror`, `dim=2`) of `A`, a `Str`, `Vec`, `Mat`, or `Image`. On a `Str` every spelling reverses the character order; on a plain `Vec` (which has no row/column orientation) both directions just reverse element order. `dim` (only meaningful for `flip`) is an optional keyword-or-positional scalar, `1` or `2`, defaulting to `1`. Returns the same type and shape as `A`. |
| `rot90` | `rot90(A, k=1)` | Rotates `A` (a `Mat`, or anything coercible to one) 90°×`k` counterclockwise; `k` is an optional scalar defaulting to `1`, may be negative for clockwise rotation, and is taken mod 4. An `r×c` input becomes `c×r` after an odd number of quarter-turns, or stays `r×c` after an even number. Returns a `Mat`. |

The example below runs the identity operation `val` on a plain number
(to show it really is just a pass-through today), transposes a small
matrix, and reverses a vector with `mirror` — the three "rearrange, don't
recompute" operations in this table that are easiest to check by eye.

```qu
A = [1, 2, 3; 4, 5, 6]
x = 5
print("val={val(x)}")
print(transpose(A))
print("mirror={mirror([1, 2, 3])}")
```

#### Overloads: `flip`

#### Case: Str

`A` a `Str`. Every spelling (`flip`, `flipud`, `fliplr`, `mirror`) reverses
character order the same way, regardless of `dim` — a string has no row/
column orientation to pick between.

```qu
print(flip("hello"))   # olleh
```

#### Case: Vec

`A` a plain `Vec`, which likewise has no row/column orientation. Both
directions just reverse element order, so `dim` makes no difference here
either.

```qu
print(flip([1, 2, 3]))   # [3, 2, 1]
```

#### Case: Mat

`A` an `r×c` `Mat` (or `Image`). Here `dim` genuinely matters: `1` reverses
row order (top-to-bottom), `2` reverses column order (left-to-right).

```qu
M = [1, 2; 3, 4]
print(flip(M, 1))   # [3, 4; 1, 2] -- rows reversed
```

## Selection

| Function | Signature | Description |
|---|---|---|
| `where` | `where(mask)` or `where(cond, a, b)` | One argument `mask` (a `Vec`/`Mat` of booleans, typically produced by a comparison like `x < 0`) returns a `Vec` of the 0-based indices where it is true. Three arguments — `cond` (a boolean mask), `a`, `b` (scalars or arrays broadcastable to `cond`'s shape) — instead select elementwise: `a`'s value where `cond` is true, `b`'s otherwise (NumPy's `where(cond, a, b)`), returning the same shape as `cond`. |
| `find` | `find(mask)` or `find(collection, fnName)` | One argument: identical to `where(mask)` — the indices where a boolean `mask` is true, as a `Vec`. Two arguments: `collection` (any indexable/iterable value) and `fnName` (a `Str` naming an already-defined one-argument predicate function); returns the first element of `collection` for which that predicate is truthy. |

#### Overloads: `where`

| Call | What it returns |
|---|---|
| `where(mask)` | Indices where the boolean `mask` (a `Vec`/`Mat`) is true, as a `Vec` — the 1-argument form. |
| `where(cond, a, b)` | Elementwise selection: `a` where `cond` is true, `b` otherwise (NumPy's `where(cond, a, b)`) — the 3-argument form, returning `cond`'s shape rather than a list of indices. |

```qu
mask = [true, false, true]
print(where(mask))          # [0, 2] -- indices, 1-argument form
print(where(mask, 1, 0))    # [1, 0, 1] -- elementwise pick, 3-argument form
```

#### Overloads: `find`

| Call | What it returns |
|---|---|
| `find(mask)` | Identical to `where(mask)` — the indices where a boolean `Vec`/`Mat` is true. |
| `find(collection, fnName)` | `collection` (any indexable/iterable value) and `fnName` (a `Str` naming an already-defined one-argument predicate) — returns the *first element* of `collection` for which that predicate is truthy, not an index. |

```qu
mask = [true, false, true]
print(find(mask))                 # [0, 2] -- same as where(mask)

is_even(n) := n mod 2 == 0
xs = [1, 3, 4, 5, 6]
print(find(xs, "is_even"))        # 4 -- first element, not an index
```

The example below builds a small vector, zeroes out its negative entries
with a boolean-mask assignment, then uses `where` to find which
(now non-negative) entries exceed a threshold and pulls just those values
back out with `x[idx]` — the round trip of "get the indices, then use
them to index back into the data" that `where` exists for.

```qu
x = [-2, 0.5, 4, -1, 5, 2]
x[x < 0] = 0
print(x)              # [0, 0.5, 4, 0, 5, 2]
idx := where x > 3
print(idx)             # [2, 4] -- 0-based positions of the entries above 3
print(x[idx])          # [4, 5] -- the entries themselves
plot(x[idx], "x", color="red")
```

`find`'s two-argument form searches by a named predicate instead of a
mask — useful when the condition isn't a simple elementwise comparison
but a small function of its own. The example below defines `is_even` as
an ordinary one-argument Qu function, then hands its *name* (as a string)
to `find` to get back the first element of `xs` that satisfies it.

```qu
is_even(n) := n mod 2 == 0
xs = [1, 3, 4, 5, 6]
print(find(xs, "is_even"))   # 4 -- first element for which is_even is true
```

## Matrix Algebra & Decompositions

Decompositions that return more than one related result (`svd`, `qr`, `lu`,
`eig`) come back as a `Value::Model` handle with named fields — the same
convention the fitted-model protocol uses (see
[Statistics & Machine Learning](statistics-ml.md)) — rather than a tuple,
since Qu has no multi-return values.

| Function | Signature | Description |
|---|---|---|
| `dot` | `dot(a, b)` | Dot product of two equal-length vectors `a`, `b` (each a `Vec`; a length mismatch errors). Returns a scalar `Num`, `sum(a_i * b_i)`. |
| `add`, `subtract`, `mul`, `div`, `pow` | `add(a, b, ...)` | The operators as named functions, so an operation can be passed where a name is what you can pass. Variadic and folded left: `add(a, b, c)` is `a + b + c`. Each routes through the very same code the operator does, so broadcasting, units, complex, matrices and every error message are whatever `a + b` already gives — there is no second set of arithmetic rules that could drift from the first. `mul`/`div` follow `*` and `/`, which on two **matrices** mean matrix multiply and solve. **`subtract`, not `sub`** — `sub` is a keyword (it opens a subroutine), so a builtin of that name could never be called. Returns whatever the matching operator returns for those arguments: a `Num` from two scalars, a `Vec` from a `Vec` and a scalar, a `Mat` from two matrices, a `Complex`/`CVec`/`CMat` as soon as either side is complex. |
| `elemul`, `elediv`, `elepow` | `elemul(a, b, ...)` | The `.*`, `./` and `.^` family: **always** elementwise, whatever the shapes. The distinction from `mul`/`div`/`pow` only bites on matrices, which is exactly why both families have names. Returns the broadcast shape of the arguments — a `Num` from two scalars, otherwise a `Vec` or `Mat`, or the complex counterpart of either as soon as one side is complex. |
| `range` | `range(a, b, [step])` | Inclusive numeric range as a `Vec`, from `a` to `b` (both scalars) in increments of `step` (an optional **positional** scalar, default `1` — `step=` as a keyword is rejected, unlike most optional Qu arguments) — distinct from the `to`/`a:step:b` loop-range syntax. Returns a `Vec` whose length depends on `a`, `b`, and `step`. |
| `matmul`, `mtimes` | `matmul(A, B)` | Matrix multiplication of `A` (`r×k`) and `B` (`k×c`), both `Mat` (inner dimensions must agree). Returns an `r×c` `Mat`. `mtimes` is the exact same builtin under a second, MATLAB-flavored name. |
| `gpu_matmul` | `gpu_matmul(A, B)` | WebGPU-accelerated matmul via `qu-gpu`, same `A`/`B` shape requirements and `r×c` `Mat` return as `matmul`; only present in builds compiled with `--features gpu`. **f32 precision** (~7 significant digits), not f64 — a real, documented tradeoff, not a bug. Returns an `m×n` `Mat` from an `m×k` `A` and a `k×n` `B`. |
| `pinv` | `pinv(A, tol=)` | Moore-Penrose pseudo-inverse (SVD-based) of `A`, an `r×c` `Mat` or `CMat`. `tol` is an optional scalar relative tolerance for treating a singular value as zero; it defaults to `f64::EPSILON * max(rows, cols)` (real input) — the same tolerance `rank` reports. Returns a `c×r` `Mat`/`CMat`. |
| `inv` | `inv(A)` | True inverse of `A` (a square `Mat` or `CMat`); requires full rank, errors otherwise. Returns an `n×n` matrix of the same real/complex kind as `A`. |
| `chol` | `chol(A)` | Lower-triangular Cholesky factor `L` of `A = L·Lᵀ`, where `A` is a symmetric positive-definite `n×n` `Mat`; errors clearly if `A` isn't positive-definite. Returns an `n×n` `Mat`. |
| `det` | `det(A)` | Determinant of a square `Mat` or `CMat`, via LU decomposition. Returns a scalar `Num` (or `Complex` for a `CMat`). |
| `rank` | `rank(A, tol=)` | Numerical rank of `A` (an `r×c` `Mat` or `CMat`) — the count of singular values clearing the tolerance. `tol` is an optional scalar, same default and convention as `pinv`'s. Returns a scalar `Num` (integer-valued). |
| `svd` | `svd(A)` | Thin SVD of `A` (`r×c` `Mat` or `CMat`), `A = U·diag(s)·Vᵀ`. Returns a `Model` (kind `"svd"`) with fields: `u` (`r×k` `Mat`), `s` (length-`k` `Vec` of singular values, `k = min(r,c)`), `vt` (`k×c` `Mat`) for a real `A`; a `CMat` input instead reports `values` (singular values), `condition`, and `rank` only. |
| `qr` | `qr(A)` | Thin QR decomposition of `A` (`r×c` `Mat`), `A = Q·R`. Returns a `Model` (kind `"qr"`) with fields `q` (`r×k` `Mat`, orthonormal columns) and `r` (`k×c` `Mat`, upper triangular), `k = min(r,c)`. |
| `lu` | `lu(A)` | LU decomposition with partial pivoting of a square `n×n` `Mat` `A`, `P·A = L·U`. Returns a `Model` (kind `"lu"`) with fields `l` (`n×n` lower-triangular `Mat`, unit diagonal), `u` (`n×n` upper-triangular `Mat`), and `p` (`n×n` permutation `Mat`). |
| `eig` | `eig(A)` | Eigenvalues (and, for symmetric `A`, real eigenvectors) of a square `n×n` `Mat` `A`. Returns a `Model` (kind `"eig"`) with fields `values` (a length-`n` `Vec` if all eigenvalues are real, otherwise a length-`n` `CVec`) and `vectors` (an `n×n` `Mat` of eigenvectors as columns when `A` is symmetric, otherwise `Nothing`). |

Unlike `pinv`/`inv`/`det`/`svd`, `matmul` does not accept a `CMat` — calling
it on a complex matrix errors with "cannot use complex matrix as a matrix"
rather than producing a complex product; there is no complex overload to
reach for here.

One convention worth knowing before the example: a bracket literal like
`[1, 1, 0, 0]` is a **column** vector (`4×1`) in Qu, the opposite of
MATLAB's row-vector default for the same syntax — reshape with `-> (1, n)`
to get a row. The example below builds the row/column pair, multiplies
them one way to get the `1×1` inner product (the dot product), then the
other way to get the `4×4` outer product — the same progression MATLAB's
own `mtimes` documentation uses, and the same two numbers it gets — before
moving on to two same-shape square matrices multiplied via both names
`matmul`/`mtimes` are known by.

```qu
row = [1, 1, 0, 0] -> (1, 4)
col = [1; 2; 3; 4]
print(matmul(row, col))    # 3 — inner product (dot product) of the two
print(matmul(col, row))    # 4x4 outer product: [1,1,0,0; 2,2,0,0; 3,3,0,0; 4,4,0,0]

A = [1, 2; 3, 4]
B = [5, 6; 7, 8]
print(matmul(A, B))
print(mtimes(A, B))   # matmul and mtimes are the same builtin under two names
```

`gpu_matmul(A, B)` has the same signature and result as `matmul`, but only
runs in a build compiled with `--features gpu` (and dispatches to a real
GPU device at runtime); a normal build errors clearly if called.

The next example demonstrates two of the heavier decompositions:
`chol`, which factors a symmetric positive-definite matrix into a
lower-triangular square root (useful for generating correlated random
samples or solving normal equations efficiently), and `svd`, whose
singular values summarize how well- or ill-conditioned a matrix is.
Multiplying the Cholesky factor by its own conjugate transpose should
reproduce the original matrix exactly, which is the cheapest possible
sanity check on the factorization.

```qu
A = [4, 1; 1, 3]
L = chol(A)
print(L * L.ctranspose())   # reproduces A

r = svd(A)
print(r.s)   # singular values
```

### Seeing what a matrix does

`cond(A)` is a number, and a number is hard to have intuition about. It is
the ratio of the longest to the shortest axis of the ellipse `A` turns the
unit circle into, so it can just be drawn:

```qu
A = [4, 1, 1, 3] as matrix(2, 2)
print("cond(A) = {round(cond(A), 3)}")

angles = linspace(0, 2 * pi, 200)
unit_x = cos(angles)
unit_y = sin(angles)
out_x = zeros(200)
out_y = zeros(200)
for i in 0 to 199
    v = A * [unit_x[i], unit_y[i]]
    out_x[i] = v[0]
    out_y[i] = v[1]
end for

plot(unit_x, unit_y, color = "#94a3b8", label = "unit circle")
plot(out_x, out_y, color = "#4169e1", width = 2, label = "A applied to it")
legend()
title("what a 2x2 matrix does to a circle")
```

A well-conditioned matrix gives a nearly round ellipse. As the condition
number grows the ellipse flattens, and a solve loses digits along the short
axis — which is what "ill-conditioned" means, drawn.

#### Overloads: `svd`

#### Case: real

`A` a real `r×c` `Mat`. Returns a `Model` with fields `u` (`r×k` `Mat`),
`s` (length-`k` `Vec` of singular values), and `vt` (`k×c` `Mat`), where
`k = min(r,c)` — the full factorization, ready to reassemble `A`.

```qu
A = [4, 1; 1, 3]
r = svd(A)
print(r.s)   # [4.618034, 2.381966]
```

#### Case: complex

`A` a `CMat`. Returns a *differently-shaped* `Model` — only `values`
(the singular values), `condition`, and `rank`; there is no `u`/`vt`
factor pair to reassemble `A` from in this branch.

```qu
C = [1 + 1j, 2; 3, 4 - 1j]
rc = svd(C)
print("values={rc.values}  condition={round(rc.condition, 3)}  rank={rc.rank}")
```

## More functions

| Function | Signature | Description |
|---|---|---|
| `diag` | `diag(M)` or `diag(v)` | Overloaded on argument type: `diag(M)`, where `M` is an `r×c` `Mat`, returns its main diagonal as a length-`min(r,c)` `Vec`; `diag(v)`, where `v` is a length-`n` `Vec` (or scalar/other coercible value), returns an `n×n` `Mat` with `v`'s entries on the diagonal and zeros elsewhere. Returns a `Vec` when handed a `Mat`, and a `Mat` when handed a `Vec` — the argument's type decides, not a keyword. |
| `trace` | `trace(M)` | The sum of the diagonal of `M`, a square (or rectangular — only `min(rows,cols)` diagonal entries are summed) `Mat`. Returns a scalar `Num`, equal to the sum of the eigenvalues — the cheap way to sanity-check an eigendecomposition. |
| `solve` | `solve(A, b)` | Solves `A·x = b` for `x` by LU with partial pivoting (or, for a `CMat`/`CVec` `A`/`b`, by complex SVD least squares). `A` is an `n×n` `Mat`/`CMat`, `b` is a length-`n` `Vec`/`CVec` or an `n×k` `Mat`/`CMat` of multiple right-hand sides. Returns a `Mat`/`CMat` the same shape as `b`. Prefer it to `inv(A) * b`: it is faster and numerically better behaved. |
| `cond` | `cond(A)` | The 2-norm condition number of `A` (an `r×c` `Mat` or `CMat`) — the ratio of its largest to smallest singular value. Returns a scalar `Num`, `+inf` for a singular matrix rather than an error (conditioning is routinely measured on matrices that turn out to be singular, and infinity *is* the answer there). Roughly how many digits a solve can lose, so `1e12` on a double means about four digits survive. |
| `nnls` | `nnls(A, b, max_iter=)` | Non-negative least squares by Lawson-Hanson, for when a negative coefficient would be physically meaningless — a concentration, a mass, a relaxation strength. `A` is an `m×n` `Mat`, `b` a length-`m` `Vec`; `max_iter` is an optional scalar iteration cap (unset by default, meaning the solver's own internal limit applies). Returns a `Record` with fields `x` (length-`n` `Vec`, every entry `>= 0`) and `residual` (a scalar `Num`, `‖A·x - b‖`). |
| `least_squares` | `least_squares("f", x0, lower=, upper=, max_iter=200, tol=1e-10)` | Nonlinear least squares on a residual function you named, via Levenberg-Marquardt. `"f"` is a `Str` naming an already-defined function that takes a length-`p` parameter `Vec` and returns a residual `Vec`; `x0` is the length-`p` initial guess `Vec`. `lower=`/`upper=` are optional length-`p` `Vec`s of box bounds (enforced by clamping every trial point before it is evaluated, rather than by penalty); `max_iter=` (default `200`) caps the iteration count, `tol=` (default `1e-10`) sets the convergence tolerance. Returns a `Model` (kind `"least_squares"`) with fields `params` (length-`p` `Vec`, the fitted parameters), `residual` (`Vec`, the residual at the solution), `cost` (scalar `Num`), `iterations` (scalar `Num`), and `converged` (`Bool`). |
| `meshgrid` | `meshgrid(x, y)` | The two coordinate matrices a surface or contour needs, from two axis vectors `x` (length `nx`) and `y` (length `ny`). Returns a `Record` with fields `x` and `y`, each an `ny×nx` `Mat`: `.x` repeats the `x` vector down every row, `.y` repeats the `y` vector across every column, so `(.x[j,i], .y[j,i])` is the grid point for `x[i]`, `y[j]`. |
| `ones_like`, `zeros_like` | `zeros_like(x)` | A vector or matrix of the same shape as `x` (a `Vec` or `Mat`), filled with one or zero respectively — the shape is read directly from `x`, so it cannot fall out of step with it. Returns the same type and shape as `x`. |
| `row_mean`, `row_sum` | `row_mean(M)` | Collapses each row of `M` (an `r×c` `Mat`, or anything coercible to one) to one number, returning an `r×1` column `Mat` (not a plain `Vec`). `mean(M, axis = 0)` collapses columns instead and returns a plain `Vec` — the axes are 0 and 1, not 1 and 2, and note the different return container (`Mat` here vs. `Vec` for the `axis=` form). Returns an `r×1` column `Mat`, one entry per row of `M`. |
| `mm`, `cm`, `inch`, `pt` | `figure_size(mm(180), mm(120))` | Length units as functions, converting a scalar `v` (millimetres/centimetres/inches/points as named) to the figure's own coordinate space — a tenth of a millimetre, so `mm(1)` is `10`, `cm(1)` is `100`, `inch(1)` is `254`, and `pt(1)` is `≈3.528` (`25.4/72` mm per point). Returns a scalar `Num`. There is no `180mm` literal — a bare number is always taken as figure units directly. |
| `is_empty`, `is_full` | `is_empty(q)` | Whether the fixed-capacity ring buffer `q` (a `Fifo` handle created by `fifo(capacity)`) currently holds nothing, or has no room left for another `push` without evicting the oldest element. `q` must be a `Fifo`. Returns a `Bool`. |

The example below runs the small-matrix toolkit that doesn't fit neatly
into the decomposition table above: `diag`/`trace`/`cond` read three
different summary numbers off the same 2×2 matrix, and `nnls` fits a
non-negative least-squares solution to a tiny system, showing both the
fitted `x` (each entry constrained `>= 0`) and the leftover `residual`.

```qu
A = [4.0, 1.0; 1.0, 3.0]
print("diag={diag(A)}  trace={trace(A)}  cond={cond(A)}")

b = [1.0, 2.0]
r = nnls(A, b)
print("nnls x={r.x}  residual={r.residual}")
```

This next example covers three unrelated but frequently-needed shape
utilities together: `meshgrid` builds the pair of coordinate matrices a
contour or surface plot needs from two plain axis vectors, `ones_like`/
`zeros_like` stamp out a filler array that automatically matches another
array's shape, and `row_mean`/`row_sum` collapse a matrix down to one
number per row.

```qu
g = meshgrid([1, 2, 3], [1, 2])
print(g.x)   # x-coordinate at every grid point
print(g.y)   # y-coordinate at every grid point

M = [1, 2, 3; 4, 5, 6]
print(ones_like(M))
print(zeros_like(M))
print("row_mean={row_mean(M)}  row_sum={row_sum(M)}")
```

Finally, the length-unit helpers and the `fifo` occupancy checks — two
unrelated conveniences that happen to round out this chapter's "more
functions" table. `mm`/`cm`/`inch`/`pt` convert a physical length into
the figure's internal coordinate units (used anywhere a plotting call
wants an exact size, like `legend(fontsize = pt(7))`); `is_empty`/
`is_full` let a script check a `fifo` ring buffer's occupancy before
deciding whether to `push` into it.

```qu
print("mm(10)={mm(10)}  cm(1)={cm(1)}  inch(1)={inch(1)}")   # e.g. figure_size(mm(180), mm(120))

f = fifo(2)
print("is_empty={is_empty(f)}")
push(f, 1)
push(f, 2)
print("is_full={is_full(f)}")   # at capacity -- the next push would evict the oldest
```

#### Overloads: `diag`

#### Case: matrix

`M` an `r×c` `Mat`. Reads the main diagonal off `M` and returns it as a
length-`min(r,c)` `Vec` — extraction.

```qu
M = [1, 2; 3, 4]
print(diag(M))   # [1, 4]
```

#### Case: vector

`v` a length-`n` `Vec` (or scalar/other coercible value). Builds a new
`n×n` `Mat` with `v`'s entries on the diagonal and zeros elsewhere —
construction, the opposite direction from the matrix case.

```qu
print(diag([1, 2, 3]))   # [1, 0, 0; 0, 2, 0; 0, 0, 3]
```
