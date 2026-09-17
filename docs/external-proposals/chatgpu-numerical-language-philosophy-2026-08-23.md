# External proposal: "numerical computing as the semantic center" (2026-08-23)

Source: pasted directly by Ahmed in chat (referred to as "ChatGPU wishes" —
presumably ChatGPT-authored), saved here verbatim so it survives context
compaction. See `BACKLOG.md`'s "External language-design proposals
(verdicts)" section for how this compares against what Qu already does —
this file is the raw source material, not the current authority. Two
earlier external write-ups got the same treatment; this is the third.

---

Yes. If the goal is a **new language specifically for signal processing, numerical computation, data science, ML, and electrical engineering**, I would *not* make it a general-purpose language that happens to have numerical libraries.

I would make **numerical computing the semantic center of the language**.

My overall design would be:

> **Julia's numerical philosophy + MATLAB's mathematical ergonomics + Python's readability + FORTRAN's numerical seriousness + a small amount of BASIC's immediacy — but without inheriting the weaknesses of any of them.**

The important part is **not** to literally combine their syntax. That tends to produce an incoherent language. Instead, take the *best design principles* from each.

---

# 1. What each language gets right

| Language    | What I would steal                                                            | What I would avoid                                                                               |
| ----------- | ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| **BASIC**   | Immediate readability, low ceremony, approachable syntax                      | Weak typing, historical syntax, poor abstraction                                                 |
| **FORTRAN** | Numerical performance, arrays, HPC, predictable compilation                   | Legacy syntax, verbosity, less elegant metaprogramming                                           |
| **Python**  | Readability, ecosystem, simple control flow, interactive use                  | Runtime overhead, numerical semantics delegated entirely to libraries                            |
| **Julia**   | Multiple dispatch, JIT, generic numerical programming, composability          | Some complexity around metaprogramming/types, occasionally surprising semantics                  |
| **MATLAB**  | Matrix notation, plotting, interactive workflow, signal-processing ergonomics | Language/library inconsistencies, historically awkward object/type system, proprietary ecosystem |

If I had to choose a **starting point**, I would choose **Julia**.

But I would then ask:

> "What would Julia look like if it had been designed from day one for engineers rather than being a general scientific programming language?"

That is the interesting language.

---

# 2. The fundamental philosophy

I would establish perhaps ten principles.

### Principle 1 — Mathematics should look like mathematics

This:

```text
y = A * x + b
```

should mean exactly what a mathematician/engineer expects.

And:

```text
y = A @ x
```

shouldn't be necessary just because `*` is ambiguous between matrix and elementwise multiplication.

I'd actually consider making the distinction:

```text
A * B       # matrix / linear-algebra multiplication
A .* B      # elementwise multiplication
```

like MATLAB/Julia.

That's extremely useful.

---

### Principle 2 — Arrays are fundamental language objects

Not an afterthought.

An array should be as fundamental as an integer.

```text
x = [1, 2, 3, 4, 5]
```

and multidimensional arrays:

```text
A = [
    1 2 3
    4 5 6
]
```

Then:

```text
A'
```

transpose,

```text
A \ b
```

solve the linear system,

```text
inv(A)
```

inverse when actually requested,

```text
eig(A)
```

eigenvalues.

This is one area where MATLAB's philosophy was absolutely correct.

---

# 3. I would seriously consider 1-based indexing

This is controversial.

Python/C programmers will hate it.

Mathematicians and engineers will often love it.

I'd probably choose:

```text
x[1]
x[2]
x[3]
```

because mathematical vectors naturally start at 1, and engineering notation constantly uses `x[n]`, `X[k]`, `A_{ij}`.

It also makes things such as:

```text
x[1:N]
```

very natural.

However, I would make **zero-based indexing available without making it the default**.

More importantly, indexing should be **generic over ranges**, rather than hardcoding an integer-array worldview.

For example:

```text
x[0:9]
```

could mean ten elements if the array itself is zero-indexed.

The array should know its index domain.

That gives you something much more powerful:

```text
x[-10:10]
```

is a perfectly reasonable signal representation.

---

# 4. Ranges should be first-class

This is one of the things MATLAB and Julia got very right.

I would absolutely keep:

```text
1:10
0:0.1:10
```

and perhaps:

```text
0..10 step 0.1
```

if you want a more explicit syntax.

For numerical work:

```text
t = 0:1/fs:1
```

is vastly nicer than:

```text
t = linspace(0, 1, fs + 1)
```

for the common engineering case.

---

# 5. Broadcasting should be built into the language

Consider:

```text
y = sin(x)
```

If `x` is an array, I want that to work naturally. I don't want users constantly thinking `numpy.sin(x)`, `sin.(x)`, or `array_map(sin, x)`.

For this particular language, **scalar operations should naturally lift over arrays**.

So:

```text
y = sin(x)
z = exp(-x^2)
```

should work whether `x` is a scalar, vector, matrix, GPU array, distributed array, etc., provided the operation is defined.

This would be one of the biggest departures from traditional languages.

---

# 6. Multiple dispatch is extremely valuable

This is one of the strongest reasons I'd borrow heavily from Julia.

Suppose:

```text
filter(x, h)
```

could operate on ordinary vectors, complex vectors, fixed-point vectors, GPU arrays, streaming signals, symbolic signals, distributed signals. The algorithm shouldn't necessarily have to be rewritten. Types can determine implementation.

For example:

```text
fft(x)
```

could dispatch to CPU FFT, GPU FFT, distributed FFT, fixed-point FFT, symbolic FFT depending on the arguments.

That's much more powerful than a language where everything ultimately becomes "call library X."

---

# 7. But I would make the type system easier than Julia's

This is important. An engineer shouldn't need to become a programming-language theorist to understand `Vector{Float64}` or parametric types.

I'd provide simple declarations:

```text
x: Float64
x: Vector[Float64]
A: Matrix[Float64]
```

Potentially:

```text
x: Vector[Float64, 100]
A: Matrix[Float64, 3, 3]
```

for statically sized vectors/matrices. The compiler could then exploit the information.

---

# 8. Physical units should be part of the language

This is where I'd go **beyond MATLAB, Python, Julia and FORTRAN**.

For electrical engineering, this is enormously useful.

```text
R = 10 kΩ
C = 100 nF
L = 10 mH
V = 5 V
I = V / R
```

gives `I = 0.5 mA`. The compiler/runtime should understand dimensions. `V = I * R` is valid; `V = R + C` is rejected. This catches enormous numbers of engineering mistakes.

---

# 9. Complex numbers should be first-class

Not `Complex(x, y)` everywhere. Simply:

```text
z = 3 + 4im
```

Then `abs(z)`, `angle(z)`, `real(z)`, `imag(z)`, `conj(z)`. Electrical engineering notation should feel natural: `Z = R + im * X` or `Z = R + jX` with `j` optionally available as an engineering alias for `im`.

---

# 10. Signals deserve a native abstraction

Instead of treating every signal as merely `Vector[Float64]`, I would have `signal x(t)` or `signal x[n]`, with attachable metadata:

```text
x = signal(
    data,
    rate = 48 kHz,
    domain = time
)
```

Now the language knows `x` is a sampled signal:

```text
y = lowpass(x, cutoff = 5 kHz)
```

rather than making engineers manually manage sampling frequency, normalized frequency, vector lengths, timestamps, channel dimensions, units.

---

# 11. Sampling should be explicit

The language should distinguish continuous, discrete, and sampled signals:

```text
x = sine(freq = 1 kHz)
y = sample(x, rate = 48 kHz)
plot(y)
```

The compiler could know `y.rate == 48 kHz` and warn about nonsensical operations.

---

# 12. DSP syntax could be beautiful

```text
fs = 48 kHz
x = microphone(rate = fs)
b, a = butterworth(
    order = 4,
    lowpass = 5 kHz
)
y = filter(b, a, x)
plot(y)
```

Or:

```text
spectrum = fft(x)
plot(spectrum.frequency, abs(spectrum))
```

That's much more domain-oriented than the equivalent MATLAB idiom of hand-building the frequency axis.

---

# 13. Matrix syntax should be extremely good

MATLAB-style matrix literals:

```text
A = [
    1 2 3
    4 5 6
    7 8 9
]
x = [1, 2, 3]
```

Plus `A[:, 2]`, `A[2, :]`, `A[1:5, 2:7]` — the good idea Python/NumPy and MATLAB converge on.

---

# 14. Linear algebra deserves operators

Keep MATLAB's `x = A \ b` (solve `Ax=b`) rather than `x = inverse(A) * b`, and `x = b / A` for the right-side solve. Not just sugar — it lets the implementation choose LU/QR/Cholesky/sparse/iterative/GPU without the user picking the algorithm unnecessarily.

---

# 15. Functions should be simple

One-line: `square(x) = x^2`. Multiline, Python/Julia readability without Python's whitespace parsing — indentation stylistic, `end` closes blocks:

```text
fn normalize(x):
    μ = mean(x)
    σ = std(x)
    return (x - μ) / σ
end
```

---

# 16. I would probably use `fn`, not `def`

`def` is historically Python; `function` is MATLAB/Fortran-ish and verbose; `fn` is concise. Minor choice — semantics matter far more.

---

# 17. Control flow should be boring

```text
if x > 0:
    y = log(x)
else:
    y = NaN
end

for i in 1:N:
    y[i] = x[i]^2
end

while error > tolerance:
    ...
end
```

No exotic syntax. Scientists and engineers want the *problems* to be intellectually interesting, not the syntax.

---

# 18. But vectorization should not be required for performance

The bad historical MATLAB philosophy: "if you write a loop, you're doing it wrong." The language should let you write:

```text
for i in 1:N:
    y[i] = x[i]^2 + 2x[i]
end
```

and compile it efficiently, without forcing cryptic vectorized rewrites purely for speed. Julia's philosophy here is excellent.

---

# 19. `for` loops should compile like FORTRAN

Ideally such a loop compiles to SIMD; the language should recognize loop independence, vectorization opportunities, memory access patterns, parallelization, GPU suitability. **Write clear code first. Optimize second.**

---

# 20. Automatic differentiation should be built in

```text
fn loss(w):
    return mean((model(x, w) - y)^2)
end
g = gradient(loss, w)
H = hessian(loss, w)
```

The runtime should understand differentiation natively, not merely provide it as an external package.

---

# 21. Symbolic computation could be integrated too

```text
x = symbol("x")
f = sin(x)^2
differentiate(f, x)
```

producing `2sin(x)cos(x)` — useful for control systems, circuit equations, optimization, analytical DSP, codegen. Not mandatory; just another type of computation.

---

# 22. Electrical engineering should have native concepts

```text
R1 = 1 kΩ
R2 = 2 kΩ
C1 = 100 nF
Vcc = 5 V

circuit amplifier:
    Vcc -> R1 -> out
    out -> R2 -> ground
    out -> C1 -> ground
end
```

Or more conventionally, a component-list form with `resistor`/`capacitor`/`voltage` primitives, then `simulate(circuit, time = 0:1 ns:10 μs)`.

---

# 23. Differential equations should be first-class

```text
eq:
    dx/dt = A*x + B*u
    y = C*x + D*u
end
solution = solve(eq, t = 0:0.001:10)
```

Or a simpler `ode: ... end` form with an initial condition. More expressive than manually building arrays and solver callbacks.

---

# 24. Data science syntax should be first-class too

```text
data = read("measurements.csv")
data
|> filter(.temperature > 20 °C)
|> groupby(.device)
|> mean(.voltage)
|> sort(.voltage)
```

A pipe operator (`|>`, as in Julia) makes data-processing workflows readable: `data |> clean |> normalize |> train`.

---

# 25. Tables should be native

```text
df.temperature
df.voltage
df.current
df[df.temperature > 50 °C]
```

rather than a huge dataframe API. Distinguish `Matrix` (homogeneous numerical storage) from `Table` (heterogeneous named data) — matters for both performance and clarity.

---

# 26. Missing values should not be confused with NaN

Distinguish `NaN` (numerical result undefined), `missing` (data wasn't recorded), and `Inf` (numerical infinity) — particularly important for data science.

---

# 27. Parallelism should be straightforward

```text
parallel for i in 1:N:
    y[i] = expensive(x[i])
end

gpu:
    y = model(x)
end
```

But ideally the `gpu:` block doesn't even need to exist — if `x` lives on a GPU (`x = GPUArray(...)`), the same code (`y = model(x)`) just runs there. The algorithm should be independent of execution hardware whenever possible (the Julia idea again).

---

# 28. GPU programming should not require CUDA knowledge for normal users

Layered access: high level (`y = model(x)`), intermediate (`parallel for`), low level (`kernel ...`) — accessibility and power both available.

---

# 29. Static compilation should be possible

Unlike Python, a `program.sim` should be able to produce a standalone executable (`sim compile amplifier.sim`) — important for embedded systems, instrumentation, lab equipment, real-time DSP, production ML, simulation software.

---

# 30. But the REPL should be excellent

Start the language, type `x = 2 + 3`, then `x^2`, then `plot(sin(0:0.01:10))`, and see something immediately. No project setup, no boilerplate, no compiler config, no imports for basic mathematics.

---

# 31. The standard library should be enormous in the numerical domain

- **Mathematics**: linear algebra, statistics, probability, optimization, numerical integration, interpolation, differential equations, root finding, special functions.
- **DSP**: FFT/IFFT, FIR, IIR, convolution, correlation, window functions, filter design, resampling, spectral analysis, STFT, wavelets.
- **Control**: transfer functions, state-space systems, PID, stability, Bode plots, Nyquist plots, pole-zero analysis.
- **ML**: tensors, autodiff, optimizers, neural networks, probabilistic models, GPU execution.
- **EE**: units, circuit equations, SPICE-like simulation, AC/transient analysis, impedance/admittance, noise analysis.

---

# 32. Plotting must be part of the culture

```text
plot(x, y)
plot(t, x, xlabel = "Time", ylabel = "Voltage", title = "Transient response")
```

For engineers: `bode(system)`, `nyquist(system)`, `polezero(system)`, `spectrum(signal)`, `histogram(data)`, `scatter(x, y)` — the language should know a Bode plot is not merely "a line plot with logarithmic axes."

---

# 33. An important difference from MATLAB: separate language and environment

MATLAB blurs programming language, IDE, plotting environment, numerical library, and commercial ecosystem. Separate them — the language should work in a terminal, IDE, Jupyter-like notebook, VS Code, embedded environment, CI server, HPC cluster, without depending on a particular GUI.

---

# 34. An important difference from Python: don't make the ecosystem carry the language

Python says "the language is general-purpose; NumPy/SciPy/PyTorch/Pandas provide the numerical world." This language should say "numerical computation is part of what the language *is*" — stronger semantic integration.

---

# 35. An important difference from FORTRAN: don't make performance visible everywhere

Take FORTRAN's philosophy (numerical code can be extremely fast without abandoning high-level mathematical concepts) without its syntax. `A: Matrix[Float64, 1000, 1000]` plus an ordinary `for` loop, and let the compiler do the work.

---

# 36. What I'd take from BASIC

BASIC's genius: you can type something and immediately understand what's happening. `R = 10 kΩ; V = 5 V; I = V / R; print(I)` should need no imports for basic operations. The first five minutes should feel like a calculator; the next five years should reveal a serious compiler.

---

# 37. A possible overall syntax

```text
module DSP
const π = 3.141592653589793
fn lowpass(x, cutoff, fs):
    normalized = cutoff / (fs / 2)
    b, a = butterworth(
        order = 4,
        cutoff = normalized
    )
    return filter(b, a, x)
end
end
```

```text
fs = 48 kHz
t  = 0:1/fs:1 s
x = sin(2π * 1 kHz * t)
x += 0.2 * sin(2π * 12 kHz * t)
y = lowpass(x, 5 kHz, fs)
plot(t, x)
plot(t, y)
```

Almost self-documenting.

---

# 38. Machine learning

```text
model = Sequential(
    Dense(128, activation = relu),
    Dense(64, activation = relu),
    Dense(10, activation = softmax)
)
loss(model, x, y)

optimizer = Adam(rate = 1e-3)
for epoch in 1:100:
    loss_value, gradients = gradient(model, x, y)
    optimizer.update(model, gradients)
end
```

Underneath: SIMD, multithreading, GPU, automatic differentiation, mixed precision, distributed computation.

---

# 39. One thing I would NOT do: make everything an operator

Avoid operator soup (`A ⊗ B ⊕ C ≫ D ⊙ E`). Keep a small core: `+ - * / ^`, comparisons, `&& || !`, `=`, `.* ./ .^`, plus a few domain operators (`|`, `|>`, `\`), maybe `≈` for approximate equality. Don't invent operators merely because Unicode permits it.

---

# 40. Unicode should be supported, but ASCII must always work

`μ = mean(x)`, `σ = std(x)`, `ω = 2πf`, `θ = angle(z)` should work, but so should `mu`/`sigma`/`omega`/`theta` spellings equally well. Unicode should be an enhancement, never a requirement.

---

# 41. A particularly powerful idea: dimensions and shapes as types

```text
A: Matrix[Float64, 3, 4]
B: Matrix[Float64, 4, 7]
C = A * B          # known at compile time to be Matrix[Float64, 3, 7]
A * A              # rejected at compile time
```

Likewise physical dimensions: `voltage: Vector[Volt, N]`, `resistance: Ohm`. Incredibly valuable in engineering.

---

# 42. I would make numerical precision explicit

`Float16/32/64/128`, `Int8/16/32/64`, `Complex64/128`, plus `BigFloat`/`Rational`/`Decimal` where useful. `x: Float32` should be a first-class declaration, not dependent on an external library — for scientific work, numerical precision is a **semantic property**, not just a storage detail.

---

# 43. Reproducibility needs to be a language-level concern

`seed = 12345` for deterministic pseudorandom behavior, plus an execution mode like `reproducible: ... end` that constrains nondeterministic reductions and numerical parallelism where possible. Scientific software needs to answer "can I run this experiment again six months from now and get equivalent results?"

---

# 44. Error handling should be explicit

Don't use exceptions for everything — numerical algorithms have legitimate failure states. `solve(A, b)` could return a structured result (`result.value`, `.converged`, `.iterations`, `.residual`); `fit(...)` could return `model`, `parameters`, `confidence`, `diagnostics` — more useful scientifically than throwing a generic `RuntimeError`.

---

# 45. The language should understand lazy data

For large signals, `x = read_signal("gigabyte.dat")` shouldn't necessarily load everything into RAM. Distinct abstractions — `Stream`, `Signal`, `Array`, `Matrix`, `Tensor`, `Table`, `Dataset` — so `y = lowpass(x)` can operate in streaming mode. Relevant to SDR, audio, instrumentation, sensor systems, telemetry, large datasets.

---

# 46. The syntax I'd ultimately choose

```text
# assignment
x = 10
# typed declaration
x: Float64 = 10
# function
fn f(x):
    return x^2 + 1
end
# loop
for i in 1:100:
    y[i] = f(x[i])
end
# condition
if x > 0:
    y = sqrt(x)
else:
    y = NaN
end
# matrix
A = [
    1 2 3
    4 5 6
]
# linear algebra
x = A \ b
# broadcasting
y = sin(x)
# elementwise explicit operations
z = x .* y
# pipeline
result = data
    |> clean
    |> normalize
    |> analyze
```

Intentionally **boring**. That's a compliment.

---

# 47. The "personality" of the language

> **It should read like Python, calculate like MATLAB, compile like FORTRAN, dispatch like Julia, and behave like an engineering calculator.**

> **A physicist should understand it without being a programmer, while a compiler engineer should be able to make it run like C/Fortran.**

---

# 48. My ranking of influences

- **Julia — 30%**: multiple dispatch, generic programming, JIT compilation, numerical abstractions, performance model, composability.
- **MATLAB — 25%**: matrix notation, ranges, numerical workflow, plotting, engineering conventions, signal-processing ergonomics.
- **Python — 20%**: readability, syntax, ecosystem philosophy, interactive programming, data manipulation.
- **FORTRAN — 20%** (mostly under the hood): compiler optimization, numerical reliability, HPC, memory layout, predictable performance, array-oriented compilation.
- **BASIC — 5%**: immediacy, approachability, "calculator that grew into a programming language."

Not a literal grammar combination.

---

# 49. What I think the killer feature would be

Not trying to beat Python at being Python — building something Python cannot express naturally: **the language understands numerical objects.**

```text
fs = 48 kHz
x: Signal[Float32]
y: Signal[Float32]
x = microphone(rate = fs)
y = lowpass(x, cutoff = 5 kHz)
spectrum = fft(y)
plot(spectrum.frequency, abs(spectrum))
```

The compiler/runtime knows `x` is a sampled signal, `48 kHz` is a sampling rate, `5 kHz` is a frequency, `Float32` is a precision, `fft` is a complex-valued transform — and can give meaningful diagnostics:

```text
warning:
    cutoff = 30 kHz exceeds Nyquist frequency
    Nyquist = 24 kHz
```

Far more interesting than creating another Python-like language.

---

# 50. The specification I'd aim for

```text
                    ┌─────────────────────┐
                    │   Domain Languages  │
                    │ EE / DSP / ML / DA  │
                    ├─────────────────────┤
                    │ Numerical Libraries │
                    │ LA / ODE / FFT / ML │
                    ├─────────────────────┤
                    │ Array / Tensor Core │
                    ├─────────────────────┤
                    │ Type / Unit System  │
                    ├─────────────────────┤
                    │ Language Semantics  │
                    ├─────────────────────┤
                    │ Optimizing Compiler │
                    ├─────────────────────┤
                    │ Native Runtime      │
                    └─────────────────────┘
```

Arrays, numerical types, dimensions, units, complex numbers, and mathematical operations belong near the language core; DSP, ML, circuit simulation, etc. are sophisticated standard libraries/domain layers on top.

---

# My strongest recommendation

Don't start by designing syntax. Start with a document answering:

1. What is an array? 2. What is a scalar? 3. What is a tensor? 4. What is a signal? 5. What is a unit? 6. What is a dimension? 7. What is a matrix multiplication? 8. What is broadcasting? 9. What is numerical promotion? 10. What does `Float64` actually guarantee? 11. How are NaN/Inf/missing represented? 12. How does generic code specialize? 13. How does code get compiled? 14. How does the compiler exploit SIMD? 15. How does GPU execution work? 16. How does automatic differentiation work? 17. How are shapes checked? 18. How are physical units checked? 19. How does parallelism work? 20. How are numerical algorithms made reproducible?

**Then design the syntax that makes those semantics pleasant to use** — the reverse of how many hobby languages are designed (attractive syntax first, semantics that don't scale discovered later).

The central identity: **"A compiled mathematical language for expressing physical and numerical computation directly."** Not "a better Python," and not "modern MATLAB."
