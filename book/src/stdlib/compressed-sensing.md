# Compressed Sensing

Recovering a signal from far fewer measurements than Nyquist demands.

## The idea

You measure `y = A·x`, where `A` is `m × n` and `m` is much smaller than
`n`. That system is massively underdetermined — infinitely many `x` explain
any `y` — so on its own it says nothing at all.

Compressed sensing adds one assumption and gets a unique answer back: **`x`
is sparse**. At most `k` of its entries are non-zero, and `k` is small
compared with `m`.

The assumption is not a trick, and it is not rare. Almost everything a
laboratory measures is sparse in *some* basis: a spectrum is a handful of
tones among thousands of bins, an impedance sweep is a few relaxation
processes, a vibration record is a few modes, an image is sparse in
wavelets. Whenever the interesting part of a measurement is small compared
with the grid it is measured on, you can measure below Nyquist and lose
nothing.

The example below builds exactly that scene. `A` is a `60 × 200` sensing
matrix — 60 random measurements of 200 unknowns — and the true `x` is
3-sparse: three named entries, the rest exactly zero.
`cs_recover(A, y, method = "omp", sparsity = 3)` is told the target
sparsity up front and returns a record whose `.x` field is the
reconstruction; the printed line reports the reconstruction error
`‖r.x - x‖₂`, which comes out on the order of `1e-12` — recovered, to
floating-point precision, from well under a third of the samples a direct
measurement of `x` would need.

```qu
n = 200
m = 60
A = randn(m, n, seed = 7) / sqrt(m)

x = zeros(n)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75

y = A * x

r = cs_recover(A, y, method = "omp", sparsity = 3)
print("recovered from {m} of {n}: error {norm(r.x - x):.2e}")
```

## Is your sensing matrix any good?

Two conditions, in decreasing order of usefulness.

**Coherence** — `coherence(A)` — takes one argument, `A` (a `Mat`, `m × n`,
the sensing matrix), and returns a single `number`: the largest absolute
inner product between two different unit-normalised columns of `A`. Small
is good. Exact recovery is *guaranteed* whenever `k < (1 + 1/mu)/2`, where
`mu` is that coherence. `cs_guarantee(A)` takes the same single argument
`A` (`Mat`, `m × n`) and returns that bound directly, as a `number` giving
the largest sparsity `k` coherence alone can *prove* recoverable.

The example below takes the same `60 × 200` Gaussian matrix used above and
prints both numbers on one line, so the coherence and the sparsity it
proves recoverable can be read off together.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
print("coherence {coherence(A):.3f}, provable sparsity {cs_guarantee(A)}")
```

That guarantee is real and badly pessimistic. The matrix above proves `k=1`
and recovers `k=5` without difficulty. **Treat a pass as proof and a
failure as no information.** It is the only certificate that is computable
at all, which is why it is here.

**The restricted isometry property** is the condition the strong theorems
are stated in: `A` satisfies RIP of order `k` with constant `d` when
`(1-d)‖x‖² ≤ ‖Ax‖² ≤ (1+d)‖x‖²` for every `k`-sparse `x`. Verifying it for
a given matrix is NP-hard, so nothing here computes it. You get it from
*construction* instead: a random Gaussian or Bernoulli matrix satisfies RIP
of order `k` with high probability once `m ≳ k·log(n/k)`. That is where the
working rule of thumb comes from:

> **`m ≈ 4k`** measurements for `k` non-zeros, and check by trying.

Randomness here is not laziness. No deterministic construction is known
that does better for general use, and `randn(m, n, seed=)` with a fixed
seed is reproducible, which is what actually matters.

## The methods

Finding the sparsest `x` is combinatorial — an `l0` problem, NP-hard — so
every method below is a tractable stand-in. They divide into two families
that fail differently, which is the reason to have both.

All of them are reached through one call:

```
cs_recover(A, y, method = , sparsity = , lambda = , iters = , tol = , debias = , step = , rho = )
```

One dispatcher rather than eight builtins, because the arguments are the
same for all of them and the method is a keyword — so comparing methods
means changing one word, not one call. Its parameters:

| Parameter | Type / shape | Meaning |
|---|---|---|
| `A` | `Mat`, `m × n` | The sensing matrix: `m` measurements of `n` unknowns, with `m` usually far smaller than `n`. |
| `y` | `Vec`, length `m` | The measurements, `y = A·x`. |
| `method` | `string`, default `"omp"` | Which solver to run: greedy (`"omp"`, `"cosamp"`, `"sp"`/`"subspace_pursuit"`, `"iht"`, `"niht"`) or convex (`"fista"`, `"ista"`, `"bp"`/`"basis_pursuit"`). |
| `sparsity` | `number` (an integer count `k`), no default | The target number of non-zeros, `1 ≤ k ≤ n`. Required by every greedy method — omitting it is an error; the convex methods infer the support instead and silently ignore `sparsity=` if it is passed anyway. |
| `lambda` | `number`, default `1e-3` | The `l1` penalty weight, used only by `"ista"`/`"fista"`. |
| `iters` | `number` (an integer count), default `200` | The iteration cap. `"ista"`, `"fista"`, and `"bp"` silently raise this to at least `1000`, and `"niht"` to at least `500`, regardless of a smaller value passed here. |
| `tol` | `number`, default `1e-9` | The residual norm at which the solver declares convergence and stops early. `"bp"` never accepts a value tighter than `1e-8`. |
| `debias` | `bool`, default `false` | When `true`, re-fits the recovered support by plain least squares after the chosen method finishes, removing the `l1` shrinkage bias (see below). |
| `step` | `number`, default `0.5` | The gradient step size, used only by `"iht"`. Must stay under `1/‖A‖₂²` or the iteration diverges; `"niht"` computes its own step every iteration and ignores this parameter. |
| `rho` | `number`, default `1.0` | The ADMM penalty parameter, used only by `"bp"`. |

`cs_recover` always returns a `record`; see "What comes back" below for its
fields.

### Greedy

Build the support one decision at a time. Fast, need `k` up front, and
brittle: a column chosen wrongly early is a wrong answer.

| `method=` | What it does | When |
|---|---|---|
| `"omp"` | Orthogonal matching pursuit, using `sparsity=k` and `tol=`. Add the column most correlated with the residual, re-solve least squares on the whole support, repeat until `k` columns are chosen or the residual falls below `tol`. | The baseline. Hard to beat when `A` is well conditioned and `k` is genuinely small. |
| `"cosamp"` | Compressive sampling matching pursuit, using `sparsity=k`, `iters=`, `tol=`. Takes the `2k` best candidates at once, solves least squares on the union with the current support, prunes back to the best `k`, and repeats up to `iters` times or until `tol` is met. | When OMP stalls. The prune lets it *undo* a bad early choice, which OMP structurally cannot. |
| `"sp"` / `"subspace_pursuit"` | Subspace pursuit, using the same `sparsity=k`, `iters=`, `tol=` as CoSaMP: the same structure with a `k`-sized (rather than `2k`-sized) working set each round. | Cheaper than CoSaMP, comparable robustness. |
| `"iht"` | Iterative hard thresholding, using `sparsity=k`, `step=` (default `0.5`), `iters=`, `tol=`. Gradient step, keep the `k` largest entries, repeat. No least-squares solve at all. | A problem too large to factorise. Needs `step=` below `1/‖A‖₂²` or it diverges. |
| `"niht"` | Normalised IHT, same `sparsity=k`, `iters=`, `tol=` as `"iht"`, but the step is recomputed from the gradient every iteration, so `step=` is ignored, and at least `500` iterations always run. | Almost always in preference to `iht`. It does not diverge. |

### Convex

Replace the count of non-zeros with the `l1` norm, its tightest convex
relaxation, and solve that exactly. Slower, and they do **not** need to be
told `k`.

| `method=` | What it does | When |
|---|---|---|
| `"fista"` | Proximal gradient with Nesterov momentum, using `lambda=` (default `1e-3`), `iters=` (floored at `1000`), `tol=`, on `½‖Ax-y‖² + λ‖x‖₁` — the LASSO / basis-pursuit-denoising problem. | The default convex choice. Noisy measurements, or `k` only estimated. |
| `"ista"` | The same objective, and the same `lambda=`/`iters=`/`tol=`, without the momentum term. | Only to see the difference: same cost per iteration, `O(1/t)` convergence against FISTA's `O(1/t²)`. |
| `"bp"` / `"basis_pursuit"` | Basis pursuit by ADMM, using `rho=` (default `1.0`), `iters=` (floored at `1000`), `tol=` (floored at `1e-8`): `min ‖x‖₁ subject to Ax = y`. | Clean measurements, when you want the constraint satisfied rather than penalised. |

The two families are not just different code paths — they behave visibly
differently on the exact same problem, which the tables above describe but
do not show: same sensing matrix, same signal, same measurements, and one
call finishes in three iterations at machine precision while the other
takes hundreds and settles for a residual bias. Neither number is a
defect; it is what "combinatorial search over a known support size" versus
"convex relaxation that infers the support" costs and buys.

#### Overloads: `cs_recover`

#### Case: Greedy (`omp`)

Told the true sparsity `k = 3` up front, OMP picks one column per
iteration and stops the moment it has three — `.iterations` comes out
equal to `k`, and the least-squares re-solve on that exact support drives
the error to floating-point zero.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "omp", sparsity = 3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Convex (`fista`)

The same `A`, `x`, and `y`, handed to FISTA instead — no `sparsity=` at
all, the support falls out of the `l1` penalty — takes hundreds of
proximal-gradient steps rather than three, and still settles for an error
around `1e-3` instead of machine precision, the shrinkage bias the next
section is about.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "fista", lambda = 1e-3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Greedy (`cosamp`)

Same problem, `2k`-at-a-time candidate selection with pruning — converges
in even fewer iterations than plain OMP here because the prune corrects
for any early misstep, though OMP already had no misstep to correct on
this well-conditioned a matrix.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "cosamp", sparsity = 3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Greedy (`sp`)

Subspace pursuit: the same structure as CoSaMP with a `k`-sized rather
than `2k`-sized working set each round — cheaper per iteration, same
machine-precision result on this problem.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "sp", sparsity = 3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Greedy (`iht`)

No least-squares solve at all — just a gradient step and a hard threshold
to the `k` largest entries, repeated. Needs many more iterations than the
solve-based greedy methods above to reach the same precision, the real
cost of avoiding the solve.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "iht", sparsity = 3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Greedy (`niht`)

The normalised variant of `iht`: the step size is recomputed from the
gradient every iteration instead of held fixed, so it needs roughly a
third as many iterations here and, unlike plain `iht`, cannot diverge
from a poorly chosen fixed step.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "niht", sparsity = 3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Convex (`ista`)

The same LASSO objective as `fista`, without the Nesterov momentum term.
Both hit the iteration cap on this problem, but where `fista` still lands
at a `1e-3`-scale error, plain `ista`'s slower `O(1/t)` convergence
leaves it far short — the practical reason FISTA, not ISTA, is the
default convex choice.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "ista", lambda = 1e-3)
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

#### Case: Convex (`bp`)

Basis pursuit by ADMM: solves `min ‖x‖₁ subject to Ax = y` exactly rather
than a penalized version of it — no `lambda=` to choose at all — landing
between the greedy methods' machine precision and FISTA/ISTA's shrinkage
bias, at a cost of roughly a hundred iterations on this problem.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
x[91] = 0.75
y = A * x

r = cs_recover(A, y, method = "bp")
print("{r.iterations} iterations, converged {r.converged}, error {norm(r.x - x):.2e}")
```

### The shrinkage, and `debias=true`

The `l1` penalty does two jobs with one term: it finds the support, and it
pulls every coefficient toward zero. The first is what you wanted; the
second is a systematic bias, visible as a few parts in ten thousand.

Once the support is known that pull has no purpose. `debias = true` re-fits
those columns by plain least squares and discards the shrunk values.

The example below recovers the same 2-sparse setup twice with
`method = "fista"`: once as `raw`, straight off the solver, and once as
`fixed`, with `debias = true` added. Both find the same support, but `raw.x` still
carries the `l1` shrinkage on its two non-zero coefficients, while
`fixed.x` has had that support re-fit by ordinary least squares. The two
printed lines report the worst-case coefficient error, `max(abs(...))`,
for each version; the debiased one should read several orders of magnitude
smaller.

```qu
A = randn(60, 200, seed = 7) / sqrt(60)
x = zeros(200)
x[7] = 1.5
x[33] = -2.0
y = A * x

raw = cs_recover(A, y, method = "fista", lambda = 1e-3)
fixed = cs_recover(A, y, method = "fista", lambda = 1e-3, debias = true)
print("fista alone    {max(abs(raw.x - x)):.2e}")
print("fista debiased {max(abs(fixed.x - x)):.2e}")
```

Standard practice, and cheap. The one reason not to: if the support is
wrong, debiasing commits to the error instead of leaving it visibly shrunk.

### What comes back

`cs_recover` returns a `record`, and the diagnostics are not decoration:

| Field | Type / shape | Meaning |
|---|---|---|
| `.x` | `Vec`, length `n` | The recovered signal. |
| `.support` | `Vec` of indices, length ≤ `n` | Indices where `.x` is non-zero, ascending. |
| `.residual` | `number` | `‖A·x - y‖₂` at the answer. |
| `.iterations` | `number` (an integer count) | How many the solver actually ran. |
| `.converged` | `bool` | Whether the tolerance was met, or the iteration cap hit. |
| `.method` | `string` | The `method=` value that was used, echoed back exactly as passed (or `"omp"` if `method=` was omitted) — useful when a script loops over several methods and prints results together. |

A method that ran out of iterations and one that converged return the same
shape of answer, and are told apart only by `.converged`. A script that
ignores it will eventually publish a result that never converged.

## The other kind of sparsity

Everything above assumes few non-zero *samples*. **Total variation**
assumes few *changes* instead — a staircase, a segmented profile, a step
response — by penalising `‖∇x‖₁` rather than `‖x‖₁`.

`tv_denoise(y, lambda)` takes the noisy signal `y` (a `Vec`, length `n`)
and the penalty weight `lambda` (a `number`, default `1.0` if the second
argument is omitted — larger means smoother), and returns a `Vec` of
length `n`: the denoised signal.

The example below builds a clean step (`clean`), corrupts it with Gaussian
noise, and denoises it with `tv_denoise(noisy, 0.3)`. `sum(abs(diff(...)))`
is a cheap proxy for how much "changiness" is left in a signal — the total
variation of the noisy trace versus the denoised one — printed before and
after so the two can be compared directly; the denoised number should be
far smaller, showing the flat regions were smoothed while the one real
jump at the step survived.

```qu
clean = zeros(200)
for i in 60 to 129
    clean[i] = 1.0
end for
noisy = clean + 0.15 * randn(200, seed = 3)

tv = tv_denoise(noisy, 0.3)
print("total variation {sum(abs(diff(noisy))):.1f} -> {sum(abs(diff(tv))):.1f}")
```

This is what recovers an edge that a smoothing filter would round off. On
the signal above, against a low-pass filter tuned to remove a comparable
amount of noise: TV reaches half the RMS error, and its 10–90% rise is 5
samples against the filter's 7. Neither reaches the true 1, and TV is
nonlinear with a penalty to choose where a filter has a cutoff — but it
never assumed the signal was smooth, only that it changes rarely.

`tv_denoise` uses Condat's direct algorithm: one pass, `O(n)`, exact rather
than iterative.

## Choosing

| | |
|---|---|
| Clean, `k` known, `A` well conditioned | `"omp"` |
| Noisy, or `k` only estimated | `"fista"`, with `debias = true` |
| OMP finding the wrong support | `"cosamp"` |
| Too large to factorise | `"niht"` |
| Piecewise-constant signal | `tv_denoise` |

`catalog/qu_compressed_sensing.qu` runs all six on the same data and prints
the comparison.

### The claim, drawn

The whole subject rests on one surprising fact: a signal with few non-zeros
can be recovered from far fewer measurements than it has unknowns. Here are
200 unknowns, 8 of them non-zero, from 60 measurements.

```qu
n = 200
k = 8
m = 60
truth = zeros(n)
for j in (11, 27, 48, 76, 103, 140, 168, 191)
    truth[j] = 1
end for

A = randn(m, n, seed = 12) / sqrt(m)
y = A * truth
r = cs_recover(A, y, method = "omp", sparsity = k)

print("{m} measurements, {n} unknowns, {k} non-zeros")
print("support found:  {r.support}")
print("recovery error: {round(rmse(truth, r.x), 8)}")

plot(0 to n - 1, truth, color = "#0f172a", width = 2, label = "truth")
scatter(0 to n - 1, r.x, color = "#e11d48", label = "recovered")
legend()
title("8 spikes from 60 measurements")
```

The error is zero — not small, zero to floating precision — because OMP
identified the exact support and then solved an ordinary least-squares
problem on those eight columns. That only happens while the number of
measurements is comfortably above the sparsity; push `m` down towards `k`
and it stops working, abruptly rather than gracefully.

