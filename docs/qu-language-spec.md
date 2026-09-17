# Qu Programming Language Specification

Version: 0.12.1 draft
Date: August 2026
Status: Specification first; engine later

> **What changed in 0.12.1 — circuit simulation syntax.** §52.3 adopts a
> SPICE-familiar `circuit name ... end circuit` block with explicit component and
> node columns, unit-checked values, ordinary asynchronous analysis calls, and an
> optional `spice.compile` step. ASCII schematics remain excellent comments, but
> not executable syntax: explicit nodes make polarity, probes, arbitrary topology,
> and generated netlists unambiguous.

> **What changed in 0.12 — testing scientific and AI algorithms.** §59 adds one
> testing model from scalar numerics to hardware-in-the-loop: familiar `test`,
> `bench`, and `testbench` blocks; tolerance-, shape-, unit-, spectrum-, and
> invariant-aware assertions; property tests and reference oracles for algorithms
> such as A*; capability-limited, deterministic sandboxes; replayable artifacts;
> and provider-neutral AI evaluation. AI services, including OpenAI integrations,
> remain packages rather than core syntax. The language records the exact model,
> grader, tool trace, seed, resource use, and latency needed to reproduce a run.

> **What changed in 0.11 — familiar-first syntax.** §58 defines a small canonical
> teaching surface and progressive disclosure: ordinary `=`, parenthesized calls,
> explicit `end if`/`end for` blocks, readable `to`/`step` ranges, and optional
> `as` contracts come first; dataflow, mapping notation, schedules, and backends
> come later. The data/model/stream pipe is now consistently `|>` everywhere;
> bare `|` is not a data pipe and remains available for domain meanings such as
> parallel circuit composition. General parenthesis-free calls are rejected;
> only a small closed set of declarative commands such as `grid on` remains.
> Documentation, formatting, and teaching prefer one canonical ASCII spelling
> even where the parser accepts compatibility aliases.

> **What changed in 0.9 — distributed execution.** §54 lets Qu reach other Qu
> instances running on the network: connect to a machine by IP address or hostname
> (`worker("192.168.0.20")`, `worker("gpu-box")`), group them into a `cluster`, and
> **allocate** work across them. The resource-aware `pool` (§47.3) extends across
> nodes with allocation policies (`round_robin`, `least_loaded`, `affinity`,
> `capacity`) and per-node capacity caps; a task runs where its resource need and
> the policy send it, or on an explicit node (`spawn fft(x) on worker("gpu-box")`).
> Two intents are supported — **accelerate** (offload transparently to a faster/GPU
> node as a remote backend) and **allocate** (`reserve 2 gpu on lab` for an
> experiment) — with data locality (`pin`, distributed arrays), fault tolerance
> (`on drop = reschedule`), and authenticated, sandboxed workers.

> **What changed in 0.8 — OS/comms, 3D, and simulation.** Three chapters extend Qu
> outward. §50 adds the operating-system and communication layer: binary/text file
> I/O, file watching with `tail`/`head`/`grep`, serial ports, TCP/UDP sockets, a
> POSIX `os` interface, and one reactive `on EVENT do … end` form (a flag, a changed
> value, a signal, a socket message, a file change, a key). §51 adds 3D: build a
> `mesh` from vertex/face arrays, texture it, and `view` it, with `scene3d`, cameras,
> and volumetric plotting reusing the animation subsystem. §52 adds simulation:
> ODE/DAE solvers, a `simulate` time-stepping block, and FEM/physics — with the
> larger domain simulators (`spice`, `fem`) and the impedance chapter (§48, now the
> `eis` library) delivered as **includable libraries**, keeping the core small
> (§52.4). Also in 0.8: **classes** (VB.NET-style) with properties,
> inheritance, and interfaces (§53); a reasoned **no-`goto`** policy with labeled
> `break`/`continue` (§53.6); **asynchronous training** and **fine-tuning /
> hyperparameter `tune`** (§45.5–§45.6); a **resource-aware scheduler** with
> per-device capacity and round-robin (`pool with cpu=3, gpu=1`, §47.3); and
> explicit **`compile`** and **`vectorize`** for peak speed (§41.3, §47.6).

> **What changed in 0.7 — impedance, native architectures, more inspirations, and
> ASCII-canonical notation.** §48 makes electrochemical impedance spectroscopy
> native: circuit elements (`R`, `C`, `L`, `Q`/CPE, `W`, `G`, `T`), building an
> equivalent circuit as a **network** (series `+` / parallel `|`, or a CDC string),
> **native ECM fitting** (complex NLLS with parameter errors), **parallel batch
> fitting** across spectra, Kramers-Kronig validation, DRT, and Nyquist/Bode plots.
> §45.4 adds **native architectures** as one-call models (`resnet`, `xception`,
> `mlp`, `cnn`, `unet`, `transformer`, `vae`, …) with data-parallel/distributed
> training (§45.5). §49 folds in further inspirations — Halide's algorithm/schedule
> separation, Futhark's stencils and nested parallelism, Chapel's distributed
> domains, and APL/J tacit composition. And §42.6 fixes the notation direction:
> **ASCII is canonical** (`Ohm`, `Hz`, `int`, `diff`, `tau`, `pi`, `u` for micro);
> Unicode is an accepted alias, never required.

Qu is pronounced "queue".

> **What changed in 0.10 — reliability, a 3D/audio/physics studio, and state
> estimation.** §55 adds a reliability layer for mission-critical continuous
> monitoring (structural health monitoring): real-time deadlines, watchdogs,
> supervisors, redundancy and voting, design-by-contract, alarms, and safe states.
> §56 turns Qu into a 3D studio: solid/CSG modeling, PBR materials and rendering,
> positional 3D audio (HRTF/ambisonics, built on the DSP core), and a real-time
> physics engine (rigid/soft bodies, cloth, SPH fluids). §57 adds state estimation
> — Kalman, EKF/UKF, and particle filters under one predict/update interface, all
> animatable. All reuse the array/signal/mesh/solver core.

> **What changed in 0.6 — the runtime and systems layer.** §47 adds everything an
> instrument actually needs beyond equations: real-time **timers** (`every 1 ms
> do … end`), **multithreading** (`spawn`/`await`, `parallel for`), **channels and
> an instruction queue**, first-class **collections** (`list`, `queue` FIFO,
> `stack`, `deque`, `ring` circular buffer), **memory-mapped and streaming fast
> I/O** (`mmap`, `stream`, out-of-core), explicit **`simd`** vectorization,
> **Processing-style `sketch`es** (`setup`/`draw`), and a declarative **GUI**
> (`window` with sliders, buttons, meters, live plots). All of it runs on one
> cooperative scheduler shared with the array dataflow planner, under the same
> `explain`-everything honesty contract.

> **What changed in 0.4 — notation over programming.** Version 0.4 makes the
> defining commitment explicit: Qu should read like a scientific worksheet, not
> like a program. One new chapter (§42) adds a mathematical/scientific notation
> surface — Unicode operators and Greek identifiers with exact ASCII equivalents,
> first-class physical units with dimensional analysis (`Fs = 5 kHz`), `where`
> clauses and piecewise `cases` definitions, and notation for sums, products,
> integrals, and derivatives (`∑ ∏ ∫ ∂ ∇`) that lowers to reductions, quadrature,
> and autodiff. Design pillar 8 (§3) is added for it. The influences broaden past
> the four languages to mathematical notation itself (LaTeX, Mathematica, APL/J)
> and to Julia and JAX, whose type-specialization, JIT, and autodiff models —
> proven in the `matty` interpreter that precedes Qu — inform the speed and
> differentiation chapters. The backend chapter (§20) gains a vendor-neutral
> Vulkan target and a benchmark-driven `backend auto`, and the standard library
> (§23) guarantees the decompositions and polynomial routines that real work
> needs.
>
> **0.4.1 — numerical calculus.** Adds §43: finite-difference differentiation
> (`diff`, with `forward`/`backward`/`central` schemes), numerical integration
> (`int`, `cumint`), and the `df/dx` notation that dispatches between *exact*
> autodiff (for functions/expressions, §38) and *finite differences* (for sampled
> data). To read as mathematics, `int` is reclaimed for integration; the integer
> type is spelled `integer`/`int64` (§8).
>
> **0.5 — heritage idioms and intuitive model building.** Two new chapters. §44
> imports the distinctive, still-useful idioms documented in Qu's ancestor
> languages — FORTRAN (masked `where`/`elsewhere`, `cshift`/`eoshift`/`pack`/
> `merge`/`spread`, `pure`/`elemental`), old BASIC (`select case`, `repeat`/
> `until`, `swap`, `restore`, `print using`), Octave (`+= ++` increment operators,
> `do…until`), MATLAB (`arrayfun`, `nargin`/`varargin`, anonymous `@`/arrow
> lambdas), and VB6 (`Optional`/default parameters, `with` member access) — each
> cited to its reference documentation. §45 adds intuitive advanced model
> building: pipe-style sequential and graph networks, **mechanistic model fitting**
> (write the equivalent-circuit equation, fit its parameters — direct to the
> impedance domain), and probabilistic `~` models with inference. Compound
> assignment is added to the core (§11). §46 grounds the design in the wishes
> practicing scientists repeatedly voice — one language (no two-language problem),
> open and free, keep-your-libraries interop, reproducibility and provenance, and
> deploy-anywhere including fixed-point on an MCU — turning each into a commitment.

> **What changed in 0.2.** Version 0.1 defined a signal-processing and numerical
> core. Version 0.2 widens the language from DSP into the full scientific data
> workflow — DSP → data science → machine learning → animated visualization —
> without disturbing the 0.1 core semantics. New material lives in four added
> chapters: dataframes and tables (§36), statistics and modeling (§37), machine
> learning with tensors and automatic differentiation (§38), and an animation and
> live-visualization subsystem (§39). The 0.1 sections are unchanged except for
> the reserved-keyword list (§6) and the open-question list (§35), which now
> reflect the wider scope.

> **What changed in 0.3 — the best-in-class commitments.** Version 0.3 adds the
> load-bearing decisions that make Qu *excellent*, not merely pleasant, at its
> three core jobs — signal processing, data science, and numerical computation.
> They live in one new chapter (§41) and are grounded in the reference workloads
> in `catalog/`, which are ports of real measurement projects. The four
> commitments: (1) `:=` is the canonical function-definition form (§41.1);
> (2) sampled-signal types that carry their own `Fs` and axis (§41.2);
> (3) multiple dispatch with compile-time type specialization, the model behind
> the "blazing fast" promise (§41.3); and (4) lazy columnar dataframes with a
> formula interface (§41.4). The guiding influences narrow to four —
> **Visual Basic, FORTRAN, MATLAB, and Python** — for readable control flow,
> array speed and layout, matrix/DSP ergonomics, and generic protocols
> respectively. A small format-spec addition (§7.C) closes a gap the reference
> scripts already relied on.

## 1. What Qu Is

Qu is a domain-specific language for signal processing, numerical computing, data
science, machine learning, visualization, and scientific reporting. It is designed
for scientists who want readable mathematical code, for data scientists who need
tables and models, for ML practitioners who need tensors and gradients, for
language developers who need precise semantics, and for backend engineers who need
a clean path to CUDA, OpenCL, SYCL, MKL, BLAS, FFT, and CPU reference execution.

Qu blends ideas from:

| Influence | What Qu takes |
|---|---|
| BASIC and Visual Basic | `to`, `step`, `then`, `end for`, direct readable control flow |
| MATLAB and Octave | matrix-first computation, plotting, DSP vocabulary |
| FORTRAN | column-major numerical layout, performance-friendly arrays |
| Scientific Python | broadcasting, `linspace`, `logspace`, packages, reproducible workflows |
| LINQ | future query syntax for streams, tables, records, and signal metadata |
| Mathematical notation (LaTeX, Mathematica, APL/J) | Unicode operators, Greek identifiers, `where`/piecewise definitions, `∑ ∏ ∫ ∂ ∇` (§42) |
| Julia and JAX (via the `matty` lineage) | multiple dispatch, type specialization, JIT, autodiff, benchmark-driven backend selection |
| Physics and engineering practice | physical units with dimensional analysis, `Fs = 5 kHz`, worksheet-style code |

The four languages that shape everyday syntax — Visual Basic, FORTRAN, MATLAB, and
Python — remain the readability, layout, ergonomics, and protocol references. But
Qu is not *only* those four: its guiding aim is to read like mathematics and
science rather than like programming, so mathematical notation itself is a
first-class influence (§42), and the numerical execution model draws on Julia and
JAX as proven in the `matty` interpreter that precedes this project.

The language should let users write:

```qu
x = 1, 2, ..., 50
y = sin(2*pi*x)
z = cos(2*pi*x)

plot(x, y, label="sin(x)")
plot(x, z, label="cos(x)")
legend
show plot
```

and should let an implementation infer shapes, memory placement, vectorization, fused kernels, and backend calls.

## 2. Reader Contract

This specification is written for three readers.

For scientists:

1. Code should look like the math.
2. Small scripts should run without declarations.
3. Errors should explain shape, unit, and backend problems in plain language.
4. Plot and report export should be standard.

For language developers:

1. Syntax and semantics must be deterministic.
2. Parser behavior must not depend on backend availability.
3. Type and shape contracts must be explicit enough for diagnostics.
4. Compatibility modes must not change the base language silently.

For backend engineers:

1. Array operations must expose enough information for fusion and scheduling.
2. Memory layout, device placement, precision, and reductions must be specified.
3. Fallbacks and host/device transfers must be visible through diagnostics.
4. A slow correct CPU reference backend is required before fast backends.

## 3. Design Pillars

1. Matrix-first: every numeric value is either a scalar or an N-dimensional homogeneous array. Scalars are rank-0 arrays.
2. Shape-aware: dimensions and orientation are part of the language contract.
3. Backend-neutral: source code is not CUDA, OpenCL, or MKL code, but can lower to them.
4. Report-ready: figures and data can export to PNG, SVG, PDF, DOCX, LaTeX, and CSV.
5. Friendly by default: scripts are dynamic unless the user adds type and shape contracts.
6. Precise when needed: functions, modules, types, shapes, devices, and execution policy can be declared.
7. Honest about performance: the implementation must expose inserted copies, fallbacks, precision changes, and nondeterministic reductions.
8. Notation over programming (§42): source should read like a scientific worksheet.
   Mathematical and scientific notation — Unicode operators, Greek identifiers,
   physical units, `where`/piecewise definitions, and `∑ ∏ ∫ ∂ ∇` — is a
   first-class surface, and every mathematical glyph has an exact plain-ASCII
   equivalent so the language is fully writable on any keyboard and diff-friendly.
9. Familiar before clever (§58): the introductory language is ordinary assignment,
   parenthesized calls, explicit blocks, arrays, and plots. Advanced notation and
   execution policy are progressively disclosed; documentation and diagnostics
   always prefer one canonical spelling.

## 4. Non-Goals for Version 0.1

1. Exact MATLAB compatibility.
2. Manual GPU thread/block programming in ordinary Qu code.
3. Replacing C, C++, Rust, or Fortran as systems languages.
4. Hiding numerical differences across devices.
5. Standardizing a full GUI toolkit beyond figures, plots, and report export.

## 5. Source Files and Style

Recommended extension: `.qu`

Encoding: UTF-8.

Statements are newline-oriented. A statement may continue inside parentheses, brackets, or braces, after a comma, or after a binary operator.

One trailing semicolon is accepted for pasted MATLAB/Octave-style code. It suppresses
interactive echo and otherwise has no semantic effect; canonical Qu examples omit it.

Line comment:

```qu
# this is a comment
```

Block comments are reserved for a future version. MATLAB-style `'` comments are not part of the base language because `'` is used for strings and transpose compatibility.

## 6. Lexical Elements
Identifiers:

```text
[A-Za-z_][A-Za-z0-9_]*
```

Identifiers are case-sensitive.

Qu follows a **keyword austerity rule** (§34.C.1): a word is *reserved* only if
it appears inside expressions or mid-statement (operators spelled as words,
control flow, definitions, literals) — the words no one expects to use as a
name. Everything else is a **contextual introducer**: recognized as syntax only
at a statement or block head, an ordinary identifier everywhere else. Type names
(`signal`, `frame`, `int`, ...) are contextual after `as`.

For comparison: MATLAB reserves 20 words, Python 35. Qu reserves 52 — and none
of them are domain vocabulary. A scientist may write `window = hann(64)`,
`fit = ml.linear.ols(X, y)`, `data = read_csv(...)`, `table = [1, 2]` without a
single collision.

Reserved keywords (structural, 52):

```text
and, as, await, break, case, cases, catch, class, const, constant, continue,
def, dim, dimension, each, elif, else, elsewhere, end, false, finally, for,
from, function, if, implicit, import, in, local, mod, module, none, not, on,
optional, or, otherwise, repeat, return, select, skip, step, sub, then, to,
true, try, until, using, where, while, with
```

Notes: `await` is reserved because it appears inside expressions (`X1 = await t1`).
`optional` appears inside parameter lists. `error` and `warn` are builtin
*functions*, not keywords (§18.B). `loop`, `pure`, `elemental`, and `async` are
statement-head attributes, hence contextual.

Contextual introducers — statement/block-head syntax, ordinary identifiers
elsewhere. If a binding shadows one, a later introducer use in the same scope is
a `Q1015` diagnostic asking for a rename, never a silent reinterpretation:

| Domain | Introducers |
|---|---|
| Data & frames (§18.A, §36) | `data`, `table`, `read`, `frame`, `signal`, `spectrum`, `collect`, `join`, `group`, `order`, `of`, `over`, `by` |
| Models & ML (§37–§38, §41, §45) | `fit`, `model`, `train`, `param`, `grad`, `layer`, `stage`, `pipeline`, `tune`, `finetune`, `method`, `compile`, `vectorize`, `parallel`, `spawn`, `async` |
| Animation & GUI (§39, §47.7–47.8) | `animate`, `at`, `ease`, `timeline`, `tween`, `scene`, `scene3d`, `sketch`, `window`, `control` |
| Runtime & systems (§47, §50–§54) | `every`, `after`, `run`, `stop`, `watch`, `flag`, `set`, `clear`, `mesh`, `view`, `simulate`, `swap`, `restore` |
| Objects (§53) | `new`, `me`, `shared`, `freeze`, `inherits`, `implements`, `interface`, `override`, `property`, `base` |
| Distributed (§54) | `node`, `reserve`, `release`, `pool`, `pin`, `worker`, `cluster`, `distributed` |
| Notation & libraries (§42, §48–§49) | `unit`, `circuit`, `schedule`, `stencil`, `compose` |
| Declarations & misc | `backend`, `device`, `export`, `render`, `input`, `let`, `namespace`, `type`, `assert`, `loop` |

*(Provenance: before 0.11.1 these words accumulated as per-version reserved
lists; §34.C.1 regrouped them. No grammar production changed — only the
reservation class. Collection constructors (`list`, `queue`, `stack`, `deque`,
`ring`, `set`, `dict`), GUI layout words (`row`, `col`, `grid`, `button`,
`meter`, `knob`), circuit-element constructors (`R`, `C`, `L`, `Q`, `W`, `G`,
`T`), EIS verbs (`nyquist`, `bode`, `drt`, `validate_kk`), stream verbs
(`tail`, `head`, `grep`), and solver/library names (`solve_ode`, `fem`,
`spice`, `eis`) remain contextual functions, as before. Every Unicode operator
introduced in §42 (`≤ ≥ ≠ · × √ ∑ ∏ ∫ ∂ ∇ ∈ → ↦ ∧ ∨ ¬ ± π τ ω …`) is an
operator or identifier token, not a keyword, and each has an exact ASCII
equivalent (§42.1).)*

Preprocessor directives (not tokens, but syntactically significant at start of line):

```text
#include, #define, #if, #end, #declare, #macro, #else
```

Contextual command words, allowed as identifiers outside command position:

```text
box, close, colorbar, figure, grid, legend, plot, show,
subtitle, title, xlabel, xlim, xscale, ylabel, ylim, yscale,
zlabel, zlim
```

The animation subsystem adds `camera`, `clear`, `pause`, `play`, `record`, and
`snapshot` (§39); like the other command words they are ordinary identifiers
outside command position. The paren-free command set is closed — no library may
add one (§34.C.7).

Numerical-calculus words (§43), usable in command position and as scheme/method
selectors: `diff`, `int`, `cumint`, `forward`, `backward`, `central`, `trapz`,
`simpson`. Outside command position they are ordinary identifiers.
## 7. Literals

Integers:

```qu
0
42
1_000_000
0xFF        # hex, 255 — 0x/0X prefix, case-insensitive digits
0xDEAD_BEEF # underscores allowed, same as decimal
```

Floating point:

```qu
0.1
1.
.5
5e3     # 5000
5e-3    # 0.005
```

Complex numbers:

```qu
1i
3 + 4i
complex(3, 4)
```

Strings:

```qu
name = "adc"
label = 'phase'
```

String interpolation:

```qu
f = 50
df = 0.5
k = 3
print("freqs: {f} -> df={df}")
print("Sine {k}")
```

Interpolation uses `{expr}` inside double-quoted strings. Single-quoted strings do not interpolate.

Raw strings, `r"..."`, are text exactly as written: no escape decoding, no
`{}` interpolation.

```qu
path  = r"C:\temp\new\data.csv"   # plain "..." gives C:<tab>emp<newline>ew\...
regex = r"\d+\.\d+"
tmpl  = r"block { x = 1; }"       # braces outside a $...$ span
```

The main use is a Windows path. In a plain string `"C:\temp\new"` is not a
near miss — `\t` and `\n` are a tab and a newline, so the value silently
stops resembling a path at all, on the platform Qu is most used on.

The body ends at the first `"`, so a raw string cannot contain a double
quote — but a trailing backslash is fine (`r"C:\dir\"`), where Rust and
Python both make that spelling a syntax error. Raw strings are
double-quoted only: `r'` is already the transpose of a variable named `r`.

LaTeX in figure labels generally does *not* need one — escapes are already
left alone inside a `$...$` span, and a `{` directly after `^`, `_` or a
macro name is treated as a LaTeX group rather than an interpolation, so
`"$10^{-3}$"`, `"$V_{pp}$"` and `"$\frac{a}{b}$"` all work as written while
`"$\alpha = {a}$"` still substitutes.

Format specifiers follow the expression after a colon, `{expr:spec}`, using a
printf/Python-style mini-grammar (`0.3f`, `4.0f`, `e`, `g`, `d`, `x`):

<!-- Grammar, not an example: the names stand for whatever the caller
     has, and `crest()` is illustrative rather than a builtin. -->

```qu,ignore
print("Fs={Fs:0.0f} Hz, N={N}, K={K} tones, df={df:0.4f} Hz")
title("multisine, cf={crest(s):0.3f}")
```

This is the `§7.C` format-spec form the reference scripts in `catalog/`
already use. A bare `{expr}` uses the default representation
for the value's type.

Built-in constants:

```text
pi, e, inf, nan, i
```

`i` is the imaginary unit unless shadowed by a local variable. Code that needs a loop variable named `i` may use `1i` or `complex(0, 1)` for clarity.

## 7.A. Preprocessor Directives

Four preprocessor directives are part of the base language:

<!-- Grammar, not an example: parse-time directives shown as forms. -->

```qu,ignore
#include "common.qu"          # file inclusion at parse time
#define DEBUG true             # compile-time flag
#if DEBUG                      # conditional compilation
    print("Debug mode")
#end

#declare PI = 3.141592653589793   # alias for 'constant' (POV-Ray style)
```

Preprocessor directives are evaluated at parse time. `#define` values are immutable and available only as boolean or string constants for `#if` conditions. `#if` supports boolean expressions using `and`, `or`, `not`. `#else` and `#elif` are NOT part of the base language (use nested `#if`/`#end`).

## 7.B. Code Macros

Parse-time code substitution templates:

<!-- Grammar, not an example: `mydata`/`Fs` stand for the caller's own. -->

```qu,ignore
#macro fft_plot(signal, sample_rate)
    x = fft(signal, size(signal))
    f = arange(0, size(signal)/2) * sample_rate / size(signal)
    mag = abs(x[0, size(signal)/2])
    plot(f, mag, label="FFT")
#end

# Usage — expands at parse time
fft_plot(mydata, Fs)
```

Restrictions: macros cannot be recursive. The macro body is not type-checked. Macro expansion happens before name resolution. Use `#macro` sparingly; prefer `function` when type safety is needed.

## 8. Type System

Qu is dynamically typed by default, with optional type and shape contracts.

Strict mode:

```qu
#top of file or module
implicit none

dimension x[1024] as float64    # explicit declaration (FORTRAN DIMENSION)

# Block-scoped local (POV-Ray #local)
# Inside a function or block:
local temp = compute_auxiliary()
```

When `implicit none` is declared:
1. All variables must be declared before use via `dimension` or must be `constant`.
2. Variables declared with `const` or `dimension` cannot be reassigned to a different type.
3. `local var` creates a block-scoped temporary that is destroyed at block exit.

Without `implicit none`, variables are duck-typed (Python/MATLAB model).

| Type | Alias | Meaning |
|---|---|---|
| `bool` | `logical` | Boolean |
| `int64` | `integer` | 64-bit signed integer |
| `uint64` | `uint` | 64-bit unsigned integer |
| `float64` | `float`, `double` | 64-bit IEEE float |
| — | — | (`int` is **not** a type alias in 0.4.1; it is reserved for integration, §43. Use `integer` or `int64`.) |
| `complex128` | `complex`, `cdouble` | two 64-bit floats |
| `string` | `str` | Unicode string |

Additional implementation types may include `int32`, `uint32`, `float32`, and `complex64`.

Container and structured types:

| Type | Meaning |
|---|---|
| `array` | N-dimensional homogeneous array |
| `vector` | 1-D or oriented 2-D numeric array |
| `matrix` | 2-D numeric array |
| `tensor` | N-D numeric array |
| `record` | named fields |
| `list` | heterogeneous ordered collection |
| `figure` | plot/visualization handle |

Type contracts:

```qu
Fs as float64 = 5e3
N as int64 = 1000
x as vector(1, N, float64)
M as matrix(K, N, complex128)
```

Post-assignment contracts:

```qu
f as vector(K, 1)
t as vector(1, N)
```

A post-assignment `as` contract checks type and shape. It may perform a reshape only if the contract uses `reshape` explicitly:

```qu
x = x as reshape(K, 1)
```

This rule protects scientists from accidental silent reshaping while still giving engineers an explicit operation to lower.

## 9. Shapes and Arrays

Shape notation:

```text
scalar              rank 0
vector(N)           one-dimensional vector with no orientation
vector(N, 1)        column vector
vector(1, N)        row vector
matrix(M, N)        two-dimensional matrix
tensor(D0, D1, ...) N-dimensional tensor
```

Array literals:

```qu
v = [1, 2, 3, 4]       # row vector
c = [1; 2; 3; 4]       # column vector
M = [1, 2; 3, 4]       # 2x2 matrix
```

Space-separated row literals are allowed in MATLAB compatibility mode only:

```qu
module old_code(compat="matlab")
    v = [1 2 3]
end module
```

Default memory layout for matrices is column-major. This matches FORTRAN, MATLAB, BLAS, and LAPACK.

Layout contract:

```qu
A as matrix(M, N, float64, layout=col_major)
B as matrix(M, N, float32, layout=row_major)
```

## 10. Size, Shape, and Length

These names are deliberately separate:

```qu
shape(x)       # tuple of dimensions, e.g. (K, N)
numel(x)       # total element count
length(x)      # largest dimension, MATLAB-style convenience
size(x, dim)   # length along one dimension
rank(x)        # number of dimensions
```

`size(x)` without a dimension is not part of the base language because it is ambiguous. Use `shape(x)` or `numel(x)`.

Predicates:

```qu
isempty(x)
isscalar(x)
isvector(x)
ismatrix(x)
istensor(x)
```

## 11. Assignment

Ordinary assignment:

```qu
x = 1
x = x + 1
```

Compound assignment (Octave heritage, §44.3):

```qu
x += 1        # x = x + 1
x -= dt       # x = x - dt
g *= 2        # x = x * 2
a ./= n       # elementwise in place, a = a ./ n
v .*= w       # elementwise in place, v = v .* w
M ^= 2        # in place, M = M ^ 2 (real matrix power on a matrix)
```

`+=`, `-=`, `*=`, `/=`, `^=`, and the elementwise `.*=`, `./=` all update a
binding in place, inheriting the exact same semantics as the corresponding
plain operator (`^=` on a matrix does real matrix power, matching `^`,
not elementwise). In-place update is a statement, not an expression, so it
cannot be nested inside another expression (this keeps evaluation order
unambiguous). **`++`/`--` are NOT supported** — `x++` is a parse error
(`unexpected Eof in expression`, since `++` isn't a lexed token at all);
there is no increment/decrement operator, statement-only or otherwise.

Multiple assignment / destructuring — **deliberately not supported.** `x, y
= 1, 2` is a parse error (`expected end of statement (newline or ';'),
found Op(",")`); `Stmt::Assign` only ever holds one name, and `Expr::Tuple`
is rejected as an assignment target on purpose. Qu's one mechanism for a
function to return multiple named results is the `Value::Model` protocol:
a function returns a `Model` with named fields, and the caller pulls them
out via `.field` access —

```qu
res = qr(M)
q = res.q
r = res.r
```

— never `q, r = qr(M)`. See §7 of `docs/qu-language-tour.md` for a fully
verified walkthrough (`qr`, `findpeaks`, and others all follow this same
pattern).

Read-only binding:

```qu
const Fs = 5e3                   # standard form
constant PI = 3.141592653589793  # FORTRAN PARAMETER / POV-Ray #declare style
```

`const` and `constant` are equivalent. Once bound, the variable cannot be reassigned. Attempting `Fs = 1000` after `const Fs = 5e3` produces a compile error.

`#declare` from POV-Ray is accepted as an alias for `constant`:

```qu
#declare PI = 3.141589265358979   # same as constant PI = ...
```

Deferred dataflow binding:

```qu
Mx := A * cos(2*pi*f*Mt + phi)
```

`:=` creates a deferred expression node. It tells the compiler and backend planner that this expression may be fused, scheduled, cached, or lowered as a unit. A deferred value becomes concrete when consumed by a strict operation such as `plot`, `print`, export, file output, external calls, or explicit `eval`.

Strict evaluation:

```qu
Mx = eval(Mx)
```

## 12. Ranges

Comma sequence:

```qu
x = 1, 2, ..., 50
```

Equivalent explicit form:

```qu
x = range(1, 50, step=1, closed=true)
```

VB-style range:

```qu
x = 1 to 20
x = 1 to 20 step pi / 4
```

Skip guard:

```qu
x = 1 to 20 step pi / 4 skip 0.1
```

`skip eps` removes generated values whose absolute value is less than or equal to `eps`. It is not an alias for `step`.

Explicit constructor form:

```qu
x = range(start=1, stop=20, step=pi/4, skip=0.1, closed=true)
```

Tutorials prefer the readable `to` / `step` form. The constructor is useful when
range parameters are computed or forwarded as named arguments (§58).

Range endpoints are inclusive when the endpoint is reached exactly within tolerance. Implementations must document the tolerance used for floating ranges and should warn when the final point is tolerance-dependent.

## 12.A. Compact Range Literal

A bare `start:step:stop` is a third spelling for the same inclusive range as `to`/`step` and comma-ellipsis, useful inline where a full `to` clause would be noisy:

```qu
plot(x := 1:0.1:3, y)
for k = 1:2:9
    print(k)
end for
```

It requires exactly two colons (three parts) — `1:3` alone is not a range literal, since a bare `:` has no other meaning outside `[...]`. It shares `to`'s inclusive, float-tolerant endpoint semantics (§12), and since §15 bracket slicing is now inclusive too, the three-part `start:step:stop` reading names the same span inside brackets and out. The remaining difference is what they produce, not which elements they name: the literal builds a value, the slice selects from one.

It is reachable everywhere a range already is — assignment right-hand sides, `for` loops, and call arguments — but, like `to`-ranges, not from inside an arbitrary parenthesized sub-expression; write `x = 1:0.1:3` and reference `x` rather than `(1:0.1:3)` inline. Inside string interpolation (`"{...}"`, §7.C) it composes correctly with the `{expr:spec}` format-spec syntax: a `{...}` body with exactly one bare `:` is a format spec as before, while one with two or more (as a `start:step:stop` literal produces) is treated as the whole expression instead.

## 13. Operators

Precedence, high to low:

| Level | Operators | Meaning |
|---|---|---|
| 1 | `()`, `[]`, `.` | call, index, field access |
| 2 | unary `+`, unary `-`, `not`, `~` | sign, logical not (`~` is an alias of `not`) |
| 3 | `^`, `**` | power |
| 4 | `*`, `/`, `\`, `.*`, `./`, `.\`, `mod` | multiply and divide |
| 5 | `+`, `-` | add, subtract |
| 6 | `<`, `<=`, `>`, `>=` | order comparison |
| 7 | `==`, `!=` | equality |
| 8 | `and` | logical and |
| 9 | `or` | logical or |
| 10 | `|>` | left-to-right data/model pipeline |
| 11 | `? :` | ternary conditional |
| 12 | `:=`, `=`, `+=`, `-=`, `*=`, `/=`, `.=` | assignment (compound forms read the current value, apply the op, then rebind — not an in-place mutation) |

`*` means matrix multiplication except when one side is scalar, where scalar expansion applies.

`and`/`or`/`not`/`~` are whole-value truthiness (a non-empty vector/matrix/
string/table is truthy), not elementwise, and **not short-circuiting** — both
operands are always evaluated, so `a and expensive()` always runs
`expensive()`.

Qu has no bitwise operator family (no `&`, `&&`, `|`, `||`, `<<`, `>>` —
writing `a & b` or `a | b` is a parse error that points here). Bitwise and
extra boolean logic are builtin functions instead, since `and`/`or`/`^`
already have established, different meanings (logical and/or, matrix
power) that `&`/`|`/`^` would collide with: `bitand(a,b)`, `bitor(a,b)`,
`bitxor(a,b)`, `bitcmp(a)`, `bitshift(a,n)` (positive `n` shifts left,
negative shifts right — operate on integers; `Value` has no separate int
type, so a non-integral argument is a clear error, not silent truncation),
plus the boolean connectives `xor(a,b)`, `nand(a,b)`, `nor(a,b)` (same
whole-value-truthiness, non-short-circuit semantics as `and`/`or`).
Integer literals also accept a `0x`/`0X` hex prefix (`0xFF == 255`), and
`hex2dec`/`dec2hex`/`bin2dec`/`dec2bin` convert to/from hex and binary
strings at runtime.

Elementwise multiplication:

```qu
y = a .* b
```

Elementwise division:

```qu
y = a ./ b
```

Power:

```qu
y = x^2
y = x**2
```

Implementations may accept both `^` and `**`; diagnostics should prefer `^` in examples.

## 13.A. Ternary Conditional

`cond ? then : else` is a C-style ternary expression: `cond` is evaluated first, and only the selected branch runs — the other branch is never evaluated, so it may contain code that would otherwise error (a zero divisor, an out-of-range index):

```qu
sign = x > 0 ? 1 : -1
y = x != 0 ? 10 / x : 0
```

It is right-associative, so a chain reads like a cascade of guards, each checked in order:

```qu
grade = score >= 90 ? "A" : score >= 80 ? "B" : score >= 70 ? "C" : "F"
```

Its precedence is lower than every operator above it and higher than assignment (§13), so `flag ? a : b` binds tighter than `x = ...` but looser than `+`/comparisons/`and`/`or`: `x = a > 0 ? a : -a` parses as `x = (a > 0) ? a : (-a)` with no parens needed. Nesting a ternary or a compact range literal (§12.A) inside a branch needs explicit parens to disambiguate its `:` from the enclosing ternary's own separator; unparenthesized, the parser reports a clear error rather than silently misreading it.

## 14. Broadcasting

Qu uses NumPy-style broadcasting with explicit orientation contracts.

Rules:

1. Dimensions are compared from the trailing dimension backward.
2. Equal dimensions are compatible.
3. Dimension `1` expands to match the other side.
4. Missing leading dimensions behave as `1`.
5. If vector orientation is ambiguous, the compiler must ask for a contract.

Example:

```qu
f as vector(K, 1)
t as vector(1, N)
phase = 2*pi*f*t
```

`phase` has shape `matrix(K, N)`.

Broadcast-to contract:

```qu
Mt := t as matrix(K, N)
```

This means `Mt` is a deferred broadcast view of `t` with shape `(K, N)`. The backend should not materialize `Mt` unless required.

## 15. Indexing and Slicing

Base Qu uses zero-based indexing.

```qu
x[0]
Mx[k, :]
Mx[:, j]
Mx[0:10, :]
Mx[0:2:10]
```

Slices are INCLUSIVE at both ends: `start:stop` selects `start` through
`stop`, so `v[0:4]` is five elements and `M[0:1, :]` is two rows. This is the
same rule as `to` and as the compact `start:step:stop` literal -- one range
convention across the language. There is no empty slice; `a:a` is one
element. Use `()` for an empty list.

> **Changed in 0.13.** Slices used to be half-open. `a:b` produced `[a..=b]`
> as a value and `[a..b)` as an index, so `k = 0:2` then `v[k]` gave three
> elements while `v[0:2]` gave two -- the same expression answering
> differently depending on whether it was stored first. Existing code that
> took the first `n` elements as `x[0:n]` must become `x[0:n - 1]`.

Slice form:

```text
start:stop
start:step:stop
:stop
start:
:
```

The bare (bracket-free) `start:step:stop` compact range literal (§12.A) reads the same three parts in the same order and with the same inclusive meaning.

**Negative indexes do NOT count from the end** — `x[-1]` is a runtime
error (`index must be a non-negative integer, got -1`), not "last
element." `end` is the real, existing mechanism, and arithmetic on it
works normally, so `end`, `end-1`, `end-2`, ... give the last, second-to-last,
third-to-last element and so on:

```qu
x[end]        # last element
x[end-1]      # second-to-last
x[end-2]      # third-to-last
x[0:end]
```

(Verified: on `[10,20,30,40,50]`, `x[end]` → `50`, `x[end-1]` → `40`,
`x[end-2]` → `30`.)

Integer-vector indexing (fancy indexing) gathers elements:

```qu
bins = round(f * N / Fs)    # integer vector
phi = angle(X[bins])        # gathers X at the bin positions
```

A logical comparison produces a boolean mask. It may be used directly for mutation,
or converted to stable zero-based indices with prefix `where`:

```qu
x = randn(3, 1)
x[x < 0] = 0                         # logical assignment / ReLU-style clamp

idx1 := where x > 3                  # deferred index vector
idx2  = where x > 2                  # eager snapshot of the indices
plot(x)
plot(x[idx1], "o", color="red")
plot(x[idx2], "x", color="blue")
show()
```

`where MASK` in expression position returns an integer vector in source order;
an empty match returns a valid zero-length vector. It is the readable prefix twin of
`find(MASK)`. This does not conflict with the statement-head `where ... end where`
block (§44.4): the expression form returns indices, while the block form applies
multiple statements under a mask. A deferred `:=` binding recomputes the mask when
forced; ordinary `=` captures the current index vector.

For visualization, a gathered vector carries its source-index provenance so
`plot(x[idx], "o")` places markers at the corresponding positions of a preceding
`plot(x)`. This provenance is plotting metadata, not another numeric dimension;
supplying an explicit x-axis (`plot(idx, x[idx], "o")`) always takes precedence.

Index, slice, and masked assignment (`Mx[k, j] = v`, `x[0:10] = v`,
`x[m] = v` with boolean `m`) write through to the underlying array and are the
only mutating index operations; gathered reads (`X[bins]`) are always copies.

One-based indexing is allowed only through explicit compatibility mode:

```qu
module old_code(index_base=1)
    y = x[1]
end module
```

## 15.A. Expression Functions

One-line functions are defined with `:=` — "is defined as" (§41.1). When the left
side is `name(args)` it is a function; when it is a bare `name` it is a deferred
value (§11). One operator, one mental model (FORTRAN `ELEMENTAL`, so it maps over
arrays with broadcasting):

```qu
magnitude(x) := sqrt(x.real^2 + x.imag^2)
normalize(x) := (x - mean(x)) / std(x)
f(x, y)      := x^2 + y^2
db(gain)     := 20 * log10(gain)
```

Constraints: the right side is a single expression — no assignments, control
flow, or multiple statements (use `function … end function`, §15.B/§17, for
those), and the one-line form is non-recursive. Expression functions are inlined
when possible; the backend may lift them to regular functions.

> The legacy spelling `def fn name(args) = expr` is retained as an accepted alias
> so existing scripts keep working, but `:=` is the canonical, taught form.
> Declaration is never required — `name(args) := expr` both introduces and defines
> the function in one line; the same is true of values, where `x = …` needs no
> prior declaration and `x as shape = …` declares and initializes together (§14).

## 15.B. Sub Procedures

A sub procedure performs side effects but returns no value (FORTRAN `SUBROUTINE`, BASIC `SUB`):

```qu
sub save_report(data, filename)
    print("Saving {filename}...")
    export data to filename
end sub

# Called without assignment:
save_report(mydata, "report.docx")
```

Difference from function:
| | `function` | `sub` |
|---|---|---|
| Return value | required (or explicit `return`) | not allowed |
| Call style | `y = f(x)` or bare `f(x)` | bare `save(x)` only |
| Keyword | `function ... end function` | `sub ... end sub` |

If you need both a return value and a side effect, use `function`. `sub` exists only for clarity: when the reader knows the call produces no value.

## 16. Control Flow

Every block introducer closes with `end <introducer>`: `end if`, `end for`,
`end while`, `end each`, `end with`. The single-line `if` form needs no
terminator; `try` is the one bare-`end` exception (§18.B).

If:

```qu
if Fs <= 0 then
    error("sampling frequency must be positive")
elif Fs < 2*fmax then
    warn("sampling frequency is below Nyquist")
else
    dt = 1 / Fs
end if
```

Single-line form:

```qu
if x > 0 then print("positive")
```

For:

```qu
for k = 0 to K - 1
    plot(t, Mx[k, :])
end for
```

Stepped for:

```qu
for k = 0 to K - 1 step 2
    print(k)
end for
```

`for NAME in EXPR` is the same statement with the collection spelling. Over a
numeric range, vector, or signal it is identical to `=` (`for k = 0 to 9` and
`for k in 0 to 9` are one loop). Over a **list** it binds each element
verbatim, whatever kind that element is:

```qu
for theme in ["ocean", "journal", "dark"]   # a heterogeneous list literal (§47.4)
    save(figure(), "trend_" + theme + ".svg", theme = theme)
end for

for path in list_dir("aging/")             # any list-returning call works
    process(path)
end for
```

Those are the only two kinds `for` iterates. A **table's rows** are reached with
the query form `from r in df ... select r` (§36.2), not by iterating the frame;
a **dict** through `keys(d)`/`values(d)` (§47.4), which are themselves lists and
so drop straight into this loop. Anything else is a `cannot iterate a <kind>`
error rather than a silent coercion.

While:

```qu
while err > tol
    err = step_solver()
end while
```

Data-parallel each:

```qu
each k in 0 to K - 1 on gpu
    Mx[k, :] = A[k] * cos(2*pi*f[k]*t + phi[k])
end each
```

`each` iterations must be independent unless a reduction clause is added in a future version.

## 17. Functions

### 17.0 Scope

A function body **reads** through to the enclosing scope and **writes**
locally. Assigning a name inside a function binds it in that call's own
frame, even when a variable of the same name exists at module scope:

```qu
n = 42

function helper(v)
    n = len(v)          # a local `n`, not the module's
    return n
end function

helper([1, 2, 3])       # returns 3
print(n)                # 42 -- unchanged
```

Reading is unrestricted, so a function may close over module-level state
without taking it as a parameter:

```qu
Fs = 5000

function tone(f)
    return sin(2 * pi * f * (0 to Fs - 1) / Fs)   # reads Fs
end function
```

To assign to a module-level name, declare it:

```qu
counter = 0

function bump()
    global counter
    counter = counter + 1
end function
```

`global` takes one or more names (`global a, b`) and applies for the rest
of the function body. At module scope it is a harmless no-op, so a helper
can be pasted into a script unchanged.

Prefer a `ref` parameter over `global` when a function exists to modify
something the caller owns: it names the thing at the call site, where the
reader is looking.

Basic function:

```qu
function normalize(x)
    m = mean(x)
    s = std(x)
    return (x - m) / s
end function
```

Typed function:

```qu
function tone(f as float64, Fs as float64, N as int64) -> vector(1, N, float64)
    dt = 1 / Fs
    t = 0 to (N - 1) * dt step dt
    return cos(2*pi*f*t)
end function
```

Multiple returns:

```qu
function minval, idx = argmin_pair(x)
    idx = argmin(x)
    minval = x[idx]
    return minval, idx
end function
```

Expression function:

```qu
function add(a, b) = a + b
```

Variadic function:

```qu
function mymax(...args)
    return max(args)
end function
```

### 17.1 The mapping form (removed)

0.8 adopted a third function-definition spelling here, the "mapping form"
(`g: x -> y ... end`). It was never implemented in the parser or interpreter
and, per Ahmed's direct ruling on 2026-09-17, is dropped as not needed -- see
the debate record at §34.C.2 for the history. Qu has two, and only two,
function forms:

| Form | Shape | Use |
|---|---|---|
| `f(x) := expr` (§41.1) | one line, one expression | elemental rules, math one-liners |
| `function ... end function` | full imperative, `return` | control flow, side effects, multiple statements |

`sub` (§15.B) remains for side-effect-only procedures. No further function syntax
will be added; the exploratory forms in Appendix X that overlap these are closed.

## 18.A. Embedded Data

Tabular data block (novel Qu feature, inspired by BASIC `DATA` + FORTRAN tabular input):

```qu
data table
    time    amp    noise
    0.0     1.0    0.001
    0.1     0.98   0.002
    0.2     0.95   0.003
end table
```

A `data table` block creates a `record` with one column-field per header. Each field is a numeric array. The table itself is accessible by the implicit name `__data__` or by explicit binding:

```qu
measurements = data table
    voltage   current
    1.0       0.5
    2.0       1.02
    3.0       1.48
end table

v = measurements.voltage
i = measurements.current
```

BASIC stream data (legacy alias):

```qu
data 1.0, 2.5, 3.3, 4.7, 5.1
read a, b, c        # a=1.0, b=2.5, c=3.3
```

`DATA` creates a read-only stream. `READ` consumes values left-to-right. Subsequent `READ` calls continue from the last position. Used rarely in new code; retained for BASIC familiarity and short test vectors.

## 18.B. Error Handling

Error handling uses `try / catch / else / finally / end` (Python model with MATLAB keywords):

```qu
try
    result = fft(x)
catch e
    print("FFT error at line {e.line}: {e.message}")
    result = zeros(size(x))       # fallback
else
    print("FFT succeeded")        # runs if no exception
finally
    cleanup_buffers()             # always runs
end
```

The catch variable `e` is a `record` with fields: `message`, `line`, `stack`, `type`.

Raising errors:

```qu
if Fs <= 0 then
    error("sampling frequency must be positive")
end if

if Fs < 2*fmax then
    warn("sampling frequency is below Nyquist")
end if
```

`error` raises an exception and stops. `warn` prints a diagnostic but continues execution.

BASIC `ON ERROR GOTO` is NOT part of the base language. Use `try / catch / end`.
(`try` is the one MATLAB-style exception to the `end <introducer>` terminator
rule of §16; all other blocks close with `end if`, `end for`, `end while`,
`end each`, `end with`, `end module`, `end model`, `end train`, ... .)

## 18.C. Declarative Plot Blocks

Complex figures use a declarative block (POV-Ray architecture):

```qu
plot {
    data x, y label="Signal" color="red" linewidth=2
    data x, z label="Noise" color="blue" linestyle="dashed"

    axes {
        xlabel="Time (s)"
        ylabel="Amplitude"
        xlim=0, 10
        grid=on
    }

    title="Frequency Spectrum"
    subtitle="Fs = {Fs:4.0f} Hz"
    legend loc="upper right"
    export="spectrum.png" dpi=300
    export="report.docx"
}
```

The imperative form remains as syntactic sugar:

```qu
plot(x, y, label="Signal", color="red")
plot(x, z, label="Noise", color="blue")
xlabel("Time (s)")
ylabel("Amplitude")
grid on
```

Internally, the imperative form builds the same scene graph as the declarative block. The declarative form is recommended for multi-component figures, exported reports, and published papers.

`render` command: explicitly renders all pending plots:

```qu
render plots              # render all open figures
render plot to "fig.png"  # render and save
```

## 18.D. Panels, Axes, Legends, and Annotations

The commands below are implemented today (`engine/crates/qu-interp/src/plotting.rs`), unlike the aspirational `plot { … }` block and `render` command above.

Every `plot`/`stem`/`scatter`/`bar`/`hist` call draws into the figure's *current panel*. `hold on` is the default: successive calls overlay on the same panel. `hold off` gives each call a fresh panel instead:

```qu
plot(x, sin(x))
plot(x, cos(x))     # hold on (default) — overlays with the line above

hold off
plot(x, sin(x))     # its own panel
plot(x, cos(x))     # a different panel
```

`next plot [vertical|horizontal]` starts a new panel regardless of `hold`, stacking it below (`vertical`, the default) or beside (`horizontal`) the previous one — a lightweight alternative to a full grid:

```qu
plot(t, x)
next plot
plot(f, abs(fft(x)))   # a second panel, stacked under the first
```

`panel(rows, cols, index)` is the MATLAB-`subplot`-style explicit grid selector (1-based, row-major); `subplot(...)` is a plain alias, same spelling either way:

```qu
panel(2, 2, 1)
plot(t, x)
panel(2, 2, 4)
plot(t, y)
```

`clear` resets figure/panel state: `clear figure` (everything), `clear panel` (the current one), `clear panel N` (by creation order), `clear panel rows, cols, index` (by grid coordinate, even before that cell has been visited).

Axes: `xlim(lo, hi)` / `ylim(lo, hi)` (call with no arguments to return to auto-scaling), `axis equal` (equal data units per pixel on both axes), `axis scale x log` / `axis scale y log` (and back to `linear`), `grid on|off`, `box on|off`. `xlabel(...)`, `ylabel(...)`, and `title(...)` label whichever panel is current.

Legend: `legend("sin(x)", "cos(x)")` labels the trailing series of the current panel, in call order, and turns the legend on. The bare command sets position and visibility: `legend top left`, `legend bottom right`, `legend top`/`bottom`/`left`/`right`, `legend best` (scores the four corners by how many plotted points fall in each quadrant and places the legend in the emptiest one), `legend off`. The legend box is translucent with rounded corners by default.

Shapes and annotations, all attaching to the current panel:

```qu
vline(3)                          # vertical reference line at x = 3
hline(0)                          # horizontal reference line at y = 0
rectangle(x0, y0, x1, y1)         # translucent highlighted box
xspan(2, 4)                       # translucent vertical band over x in [2, 4]
yspan(-1, 1)                      # translucent horizontal band over y in [-1, 1]
text(1, 1, "note")                # a plain label at (1, 1)
annotate(2, 2, "note")            # a label with a marker dot at (2, 2)
arrow(0, 0, 1, 1)                 # an arrow with no label
arrowtext(0, 0, 1, 1, "go")       # an arrow ending in a labeled point
point(3, 4)                       # a highlighted "radioactive" callout,
                                   # auto-labeled "(3, 4)" from its own coordinates
```

`color=`/`label=`/`marker=` are accepted as named arguments on `plot`/`stem`/`scatter`/`vline`/`hline`/`rectangle`/`xspan`/`yspan` (e.g. `plot(x, y, color="red", label="signal")`); `plot(x, y, "o")` also accepts a MATLAB-style positional marker string.

`savefig(path)` exports the figure, format chosen by extension. `.svg`, `.html` (an SVG embedded in a minimal page), and `.tikz`/`.tex` (LaTeX/TikZ source) are implemented; `.png` and `.pdf` are not yet (IMPL.md) — `savefig` raises a clear error naming the still-missing formats rather than silently producing nothing.

Not yet implemented: an inset zoom-rectangle panel linked to a source region, axis breaks, custom tick positions/formatting, and distinct rasterization for `bar`/`hist`/`histogram` (they currently render as a line, like `plot`) — all tracked in IMPL.md.

## 19. Query Expressions

LINQ-inspired syntax is reserved for records, tables, streams, and signal metadata.

Draft form:

```qu
peaks =
    from bin in spectrum
    where bin.mag > noise_floor
    select { freq=bin.freq, mag=bin.mag }
```

Queries are expressions. They must be optimizable and must not hide side-effecting loops.

## 20. Backend and Device Model

Backend declarations:

```qu
backend auto
backend cpu
backend cpu(mkl)
backend cuda
backend opencl
backend sycl
backend vulkan          # vendor-neutral GPU via a wgpu-style compute layer
```

`backend auto` is **benchmark-driven**, not a fixed preference: the implementation
periodically calibrates the available backends on representative workloads (matrix
multiply, FFT, elementwise, loop-heavy) and selects per operation by measured
speed, memory, and accuracy — the calibration approach proven in the `matty`
interpreter that precedes Qu. The reference implementations that inform lowering
are Julia (type specialization + LLVM JIT), JAX (XLA + autodiff), a native Rust
core (ndarray-linalg, rustfft) for CPU, and Vulkan via a wgpu-style layer for
vendor-neutral GPU (AMD, Intel, NVIDIA, Apple) without a CUDA toolkit dependency.

Device placement:

```qu
x on gpu = randn(1, N)
y on cpu = gather(x)
```

Execution policy:

```qu
with backend cuda, precision=float32, fastmath=true
    X = fft(x)
end with
```

Required backend roles:

| Backend | Role |
|---|---|
| `cpu` | portable reference execution |
| `cpu(mkl)` | accelerated BLAS, LAPACK, and FFT on CPU |
| `cuda` | NVIDIA GPU execution through CUDA, cuBLAS, and cuFFT |
| `opencl` | portable GPU and accelerator execution |
| `sycl` | optional oneAPI/SYCL-style portability target |

Implementations must provide:

```qu
explain expr
```

`explain` reports selected backend, fused operations, memory transfers, fallback calls, precision policy, and reduction policy.

## 21. Precision and Reproducibility

Default policy:

1. Integer literals are `int64`.
2. Floating literals are `float64`.
3. Complex literals are `complex128`.
4. Backends must not silently narrow precision.

Explicit precision:

```qu
with precision=float32
    y = fft(x)
end with
```

Reduction policy:

```qu
with reduction=deterministic
    s = sum(x)
end with

with reduction=fast
    s = sum(x)
end with
```

Default reduction policy is `deterministic` on **all** backends, including GPUs (§34.B.7). Fast nondeterministic GPU reductions are available only through an explicit `with reduction=fast` block. When the deterministic path is measurably slower on a device, the implementation must say so through a `Q2105` performance note rather than silently switching.

Random streams:

```qu
seed(1234)
phi = randn(K, 1)
```

Backends should document when random streams differ across devices.

## 22. Memory Model

Arrays may live on host memory, device memory, or managed memory. Placement is an implementation detail unless the user declares it.

Views are non-copying when possible:

```qu
row = Mx[k, :]
col = Mx[:, k]
```

The implementation must diagnose performance-relevant copies:

```text
Q2104 performance note:
  Mx[k, :] is a non-contiguous row view in column-major layout.
  CUDA backend materialized a contiguous temporary before fft.
```

## 23. Built-In Math and Array Functions

Math:

```text
sin, cos, tan, asin, acos, atan, atan2
exp, log, log10, log2, sqrt, cbrt
abs, angle, real, imag, conj, complex
round, floor, ceil, fix, sign
```

Array construction:

```text
zeros, ones, eye, rand, randn, randi
linspace, logspace, range
meshgrid, diag
```

Array manipulation:

```text
reshape, transpose, ctranspose, squeeze
flip, sort, unique, intersect, union, setdiff
concat, stack, pad
```

Reductions:

```text
sum, prod, mean, median, std, var
min, max, argmin, argmax
all, any, cumsum, cumprod, rms, diff
```

Reductions accept an `axis=` keyword on matrices (`axis=0` collapses rows to a
row of column results; `axis=1` collapses columns to a column of row results).
`min`/`max` are reductions with one argument and elementwise (with broadcasting)
with two — the latter is the clip idiom `min(max(x, lo), hi)`; `clip(x, lo, hi)`
is the named form.

Smooth-max / optimization surrogates:

```text
logsumexp(x)              # log sum exp, numerically stable
smoothmax(x, beta=1)      # (1/beta) * logsumexp(beta*x) -> max as beta -> inf
softmax(x)                # gradient of logsumexp; a probability vector
```

`logsumexp` is a smooth, convex upper bound on `max` (`max(x) <= logsumexp(x) <=
max(x) + log(n)`), and `smoothmax` sharpens toward the true maximum as `beta`
grows. Because the crest factor `max|x|/rms(x)` is non-differentiable at the
peak, `smoothmax(abs(x), beta)` gives a differentiable surrogate whose gradient
<!-- private:begin -->
is a `softmax`-weighted sum — the basis of gradient-based crest-factor reduction.
<!-- private:end -->

Linear algebra (required — these are load-bearing for real numerical work and are
guaranteed, not optional):

```text
matmul, dot, cross, kron, outer, trace, det, norm
inv, pinv, solve, lstsq, rank, cond
eig, svd, qr, lu, chol, schur, expm
```

Polynomials:

```text
polyfit, polyval, roots, poly, conv, deconv, polyder, polyint
```

These lists are the coverage floor established by the reference workloads and by
the gap analysis carried over from the `matty` interpreter, where missing
decompositions (`eig`, `svd`, `qr`, `chol`) and polynomial routines (`polyfit`,
`polyval`, `roots`) were the top blockers. A conforming Qu implementation provides
all of them on the CPU reference backend before claiming conformance.

## 24. DSP Standard Library

Namespaces:

```text
dsp.axis
dsp.fft
dsp.filter
dsp.window
dsp.spectrum
dsp.signal
dsp.random
dsp.io
```

Required DSP functions:

```text
fft, ifft, rfft, irfft, fftfreq, fftshift
power_spectrum, psd, spectrogram
stft, istft
filter, filtfilt, freqz, group_delay
conv, convolve, xcorr, correlate
resample, resample_poly, decimate, upsample, downsample
hilbert, envelope
moving_average, exponential_smooth, median_filter
butter, cheby1, cheby2, fir1
hann, hamming, hanning, blackman, blackmanharris
kaiser, rectwin, triang
tf, zpk, step_response, impulse_response, bode, nyquist, margin
```

`logspace(a, b, n)` returns `n` values from `10^a` to `10^b` inclusive. If `n == 1`, it returns a one-element vector containing `10^a`.

## 25. Statistics

Required statistics functions:

```text
mean, median, mode
std, var, skewness, kurtosis
percentile, quantile
cov, corrcoef
histogram, ecdf
fit
```

## 26. I/O and Reports

Data I/O:

```qu
x = load("signal.csv")
save("workspace.npz", x, Fs)
T = read_csv("data.csv", headers=true)
write_csv(y, "filtered.csv")
cfg = read_json("config.json")
write_json("result.json", result)
```

Required file families:

```text
csv, tsv, json, mat, npy, npz, wav
```

Optional file families:

```text
xlsx, hdf5, parquet
```

Report export:

```qu
export plot to "figure.png" width=1200 height=800 dpi=300
export plot to "figure.svg"
export plot to "figure.pdf"
export plot to "report.docx"
export plot to "figure.tex"
export plot to "data.csv"
export all to "report.docx"
```

DOCX export should include the figure and, when practical, the plotted data table. LaTeX export should produce either a standalone figure or embeddable figure code, depending on options.

## 27. Plotting

Figure management:

```qu
figure()
figure(tab=k)
figure(size=(900, 600))
figure(title="Spectrum")
close figure
close all
```

Plot types:

```text
plot, scatter, stem, bar, area, histogram
boxplot, heatmap, contour, contourf
surf, mesh, quiver, polar, pcolor
```

Style:

```qu
plot(x, y,
    label="sin(x)",
    color="red",
    linestyle="solid",
    linewidth=2,
    marker="o",
    markersize=5,
    alpha=0.5)
```

Axes and decoration:

```qu
xlabel("Time (s)")
ylabel("Amplitude")
title("Signal")
xlim(0, 1)
yscale("log")
grid on
box on
legend
tight_layout()
show plot
show()                              # familiar function alias
```

Subplots:

```qu
subplot(2, 2, 1)
plot(t, x)
subplot(2, 2, 2)
plot(f, mag)
```

Hold (overlay multiple datasets on same axes):

```qu
plot(x, y, label="Signal")
hold on
plot(x, y_filtered, label="Filtered")
hold off
legend
```

Render (POV-Ray inspired explicit render command):

```qu
render plots                  # render all pending figures
render plot to "fig.png"      # render current figure to file
render all                    # render and close all
```

## 28. Extension System

Qu supports three extension levels.

Qu modules:

```qu
module mydsp
    export wavelet_denoise

    function wavelet_denoise(x, threshold)
        return x
    end function
end module
```

Native plugins:

```qu
import mydsp
y = mydsp.wavelet_denoise(x, threshold=0.1)
```

Foreign functions:

```qu
using c_backend "libmydsp"
```

The FFI ABI is not specified in version 0.1. Implementations may provide experimental Python, C, C++, Fortran, or Rust extension mechanisms, but source-level Qu semantics must not depend on a specific host language.

## 29. REPL and CLI

Recommended CLI:

```text
qu
qu script.qu
qu script.qu --backend=cuda
qu script.qu --backend=cpu(mkl)
qu script.qu --headless
qu script.qu --explain
```

Recommended REPL commands:

```text
%run script.qu
%time expr
%explain expr
%who
%clear
%export path
```

These commands are REPL features, not language syntax.

## 30. Diagnostics

Diagnostics must include:

1. Source location.
2. Plain-language message.
3. Expected and actual types or shapes.
4. Backend name when backend behavior is involved.
5. Suggested fix when obvious.

Shape error example:

```text
Q1023 shape mismatch at synth.qu:22:8
  f has shape vector(K, 1)
  t has shape vector(N)
  The expression f * t needs a row-vector contract for t.
Try:
  t as vector(1, N)
```

Backend fallback example:

```text
Q2201 backend fallback at fft.qu:14:5
  CUDA backend has no implementation for wavelet_transform.
  Falling back to CPU for this operation and copying input from GPU to CPU.
```

### 30.1 Diagnostic identity and severity

Every diagnostic has a stable identifier of the form `QNNNN`. The identifier names
the condition, not the wording, so editors, CI systems, and search results remain
useful when the explanation improves. A diagnostic is one of:

| Severity | Meaning |
|---|---|
| `error` | Qu cannot preserve the program's declared meaning; execution stops. |
| `warning` | Execution can continue, but cost, precision, portability, or reproducibility may change. |
| `note` | Supporting context attached to an error or warning. |

Warnings may be promoted to errors by a project policy or CLI flag. Suppression is
by stable code and must be scoped to a line, block, module, or project; Qu does not
provide an unscoped “ignore all warnings” source directive.

Machine-readable diagnostics expose at least `code`, `severity`, `message`,
`source`, `span`, and any available `expected`, `actual`, `backend`, and `fix`
fields. Human-facing wording follows this order:

1. state what failed and where;
2. show the relevant declared and actual values;
3. explain why Qu cannot safely infer the programmer's intent;
4. offer a concrete correction only when it preserves meaning.

Unit error example:

```text
Q1304 incompatible units at filter.qu:9:21
  cutoff = 12 ms has dimension time
  butter(..., cutoff=) expects frequency.
Try:
  cutoff = 1 / (12 ms)
  or pass a value in Hz.
```

Backend warnings quantify hidden cost when it is knowable:

```text
Q2201 backend fallback at features.qu:31:7
  CUDA has no implementation for wavelet_transform(float64).
  This operation will run on CPU and copy 64 MiB from GPU to CPU.
Try:
  use precision=float32, select backend=cpu for this graph,
  or install a CUDA implementation.
```

The familiarity seam diagnostics of §34.C.3 (zero-based indexing beside
inclusive ranges) are required of every implementation:

```text
Q1031 index equals length at port.qu:17:9
  x[1000] on data of length 1000.
  Qu indexes from 0, so valid indexes are 0..999.
Try:
  x[999] for the last element, or the slice x[0:1000] for all of it.
```

```text
Q1015 contextual introducer shadowed at sweep.qu:8:1
  'fit' is bound as a variable in this scope (line 4), so the
  'fit model to data' statement here cannot be parsed safely.
Try:
  rename the binding (e.g. fit_result) or the statement takes
  the qualified form ml.fit(...).
```

`Q1032` (slice stop equals length — informational, reports the element count
when a shape contract follows) and `Q1033` (inclusive-range endpoint within
tolerance — names the tolerance) complete the set; their formats follow the
`Q1031` template.

Real-time diagnostics identify forbidden work before entering a deadline-bound
region whenever static analysis can prove it:

```text
Q5102 allocation in realtime block at monitor.qu:44:5
  concat(history, sample) may grow the heap.
  The declared deadline is 1 ms; allocation is forbidden in realtime.
Try:
  preallocate a ring buffer before entering the realtime block.
```

The diagnostic contract is part of language conformance. An implementation may add
more context, but it must not silently perform a shape guess, unit conversion,
precision downgrade, device transfer, or real-time allocation that the applicable
language policy requires it to report.

## 31. Reference Example: Multisine Synthesis

```qu
backend auto

fo = logspace(-2, 3, 64)
Fs = 5e3
N = 1000
df = Fs / N
dt = 1 / Fs

f = round(fo / df) * df
f = unique(f)
K = length(f)
print("freqs: {f} -> df={df}")

t = 0 to (N - 1) * dt step dt

A = ones(K, 1)
phi = randn(K, 1)

f as vector(K, 1)
A as vector(K, 1)
phi as vector(K, 1)
t as vector(1, N)

Mt := t as matrix(K, N)
Mx := A * cos(2*pi*f*Mt + phi)
x = sum(Mx, axis=0)

plot(t, x)
xlabel("Time in s")
ylabel("Amplitude")
grid on
box on
show plot

for k = 0 to K - 1
    figure(tab=k)
    plot(t, Mx[k, :])
    title("Sine {k}")
end for
```

## 32. Reference Example: FFT Report

```qu
Fs = 10000
N = 4096
dt = 1 / Fs
t = 0 to (N - 1) * dt step dt

f1 = 100
f2 = 500
x = sin(2*pi*f1*t) + 0.5*sin(2*pi*f2*t)

X = fft(x, N)
freq = 0 to (N/2 - 1) step 1
freq = freq * Fs / N
mag = abs(X[0:N/2])

plot(freq, mag, label="Magnitude")
xlabel("Frequency (Hz)")
ylabel("|X(f)|")
title("FFT Spectrum")
grid on
export plot to "fft_spectrum.png" dpi=300
export plot to "fft_report.docx"
```

## 33. Implementation Roadmap

Phase 1: specification and examples

1. Freeze core syntax.
2. Freeze scalar, array, range, indexing, function, module, and plot command semantics.
3. Write example programs and parser acceptance tests.

Phase 2: frontend

1. Lexer.
2. Parser.
3. Concrete syntax tree.
4. AST with source spans.
5. Name resolution.
6. Type and shape contracts.

Phase 3: reference runtime

1. CPU interpreter.
2. Dense arrays.
3. Standard math and array functions.
4. Plot and export stubs.
5. Deterministic tests.

Phase 4: optimization IR

1. Dataflow graph extraction for array expressions.
2. Broadcast fusion.
3. Range laziness.
4. Common subexpression elimination.
5. Memory-transfer planning.
6. `explain` output.

Phase 5: accelerated backends

1. CPU BLAS/MKL.
2. FFT library integration.
3. CUDA lowering.
4. OpenCL or SYCL lowering.
5. Backend calibration benchmarks.

## 34. Minimum Viable Language

The first useful implementation should support:

1. Numbers, strings, booleans.
2. Vectors and matrices.
3. Assignment and multiple assignment.
4. `as` type and shape contracts.
5. Ranges, `linspace`, `logspace`.
6. Indexing and slicing.
7. Elementwise math and matrix multiplication.
8. Broadcasting.
9. `sum`, `mean`, `round`, `unique`, `length`, `shape`, `numel`.
10. `if`, `for`, and `while`.
11. Functions.
12. Plot commands as runtime calls.
13. CPU reference execution.
14. `explain` for planned array expressions.

## 34. A. Language Influence Debates

**Process:** Five language personas (FORTRAN, BASIC, POV-Ray, MATLAB, Python) debated 10 open design questions. Each advocated their heritage. The synthesis below records the result.

### 34.A.1 Constants — `constant` keyword

```qu
constant PI = 3.141592653589793
constant Fs = 5000
```

**FORTRAN** argued `PARAMETER` is essential for correctness in numerical code. **POV-Ray** argued `#declare` separates metadata from execution. **Python** said `const` with runtime enforcement.
**Verdict:** `constant` keyword, enforced at runtime. `#declare` accepted as POV-Ray alias.

### 34.A.2 Strict Typing — `implicit none` optional

```qu
implicit none
constant Fs = 5000
dim x[1024] as float
```

**FORTRAN** said `IMPLICIT NONE` is non-negotiable. **Python** and **MATLAB** said dynamic is default. **POV-Ray** said `#local` for block scope.
**Verdict:** Duck-typed by default. `implicit none` is an optional script-level directive. `dim` for explicit declaration. `local var` for block-scoped temporaries.

### 34.A.3 Embedded Data — `data table` block

```qu
data table
    time    amp    noise
    0.0     1.0    0.001
    0.1     0.98   0.002
end table
```

**BASIC** defended `DATA`/`READ`. **FORTRAN** said tabular data is novel and useful. **Python** said dict literals work for small data, but a table block is genuinely valuable.
**Verdict:** `data table` block adopted. Columns accessible as named fields. BASIC `data`/`read` as legacy alias for stream data.

### 34.A.4 Error Handling — `try / catch / else / finally`

```qu
try
    result = fft(x)
catch e
    print("FFT error: {e.message}")
else
    print("FFT succeeded")
finally
    cleanup_buffers()
end
```

**Python** defended `try/except/finally`. **MATLAB** uses `try/catch/end`. **BASIC** defended `ON ERROR GOTO`.
**Verdict:** Python model with MATLAB keywords: `try / catch / else / finally / end`.

### 34.A.5 Declarative Plot Blocks

```qu
plot {
    data x, y label="Signal" color="red" linewidth=2
    data x, z label="Noise" color="blue" linestyle="dashed"
    axes { xlabel="Time (s)" ylabel="Amplitude" grid=on }
    title="Frequency Spectrum"
    legend loc="upper right"
    export="spectrum.png" dpi=300
}
```

**POV-Ray** won the architecture argument. Declarative is the primary form for complex figures. Imperative `plot(x, y)` remains as syntactic sugar.
**Verdict:** Both imperative and declarative supported. Declarative is the recommended form for multi-component figures, reports, and papers.

### 34.A.6 One-Line `def fn` Functions

```qu
def fn magnitude(x) as float = sqrt(x.real**2 + x.imag**2)
def fn normalize(x) = (x - mean(x)) / std(x)
```

**BASIC** defended `DEF FN` as essential for math scripts. **FORTRAN** said it's `ELEMENTAL` done right. **Python** said it's cleaner than `lambda`.
**Verdict:** Adopted. `def fn` is for single-expression functions. Full `function / end function` remains for multi-statement.

### 34.A.7 Light Preprocessor — `#include`, `#define`, `#if`

```qu
#include "common.qu"
#define DEBUG true
#if DEBUG
    print("Debug mode")
#end
```

**POV-Ray** defended `#` system as foundational. **FORTRAN** said `INCLUDE` and `PARAMETER` are enough. **MATLAB** said preprocessors add compile-time complexity.
**Verdict:** Four-directive minimum: `#include`, `#define`, `#if`, `#end`. No `#ifdef`, `#endif`, `#elif`, `#undef`. `#declare` alias for `constant`.

### 34.A.8 `sub` vs `function` — Distinction kept

```qu
# Returns a value
function normalize(x) as float[]
    return (x - mean(x)) / std(x)
end function

# Side-effect only
sub save_report(data, filename)
    print("Saving {filename}...")
    export data to filename
end sub
```

**FORTRAN** and **BASIC** both defended the distinction. **MATLAB** and **Python** merged them.
**Verdict:** `sub` for side-effect procedures (no return value). `function` for value-returning. `sub` called without assignment; `function` called with or without assignment.

### 34.A.9 Global State — `constant` + explicit context

**FORTRAN** defended `COMMON` blocks. **Python** said global state makes code untestable. **POV-Ray** said `#declare` is global by design. **MATLAB** said base workspace is global.
**Verdict:** `constant` replaces FORTRAN `COMMON` for shared read-only values. Module-level scope for variables (Python model). `global` keyword allowed but discouraged.

### 34.A.10 `#macro` Code Templates — Restricted adoption

```qu
#macro fft_plot(signal, sample_rate)
    x = fft(signal, size(signal))
    f = arange(0, size(signal)/2) * sample_rate / size(signal)
    mag = abs(x[0, size(signal)/2])
    plot(f, mag, label="FFT")
#end

fft_plot(mydata, Fs)
```

**POV-Ray** defended `#macro` as the mechanism for code reuse. **FORTRAN** warned about type-safety.
**Verdict:** Adopted with restrictions: parse-time substitution only, no recursion, no type-checking on macro body, documented limitation.

## 34.B. Second Debate Record (0.2.1, reaffirmed 0.10)

**Process:** The five personas debated the questions left open after draft 0.1 —
§35 items 1–6 plus four recorded on the coordination board (deterministic
reductions, plotting scope, `data table` column types, `#macro` defaults).
Verdicts are normative; the sections they amend are named. (Originally recorded
2026-08-19; lost in a concurrent rewrite and restored.)

### 34.B.1 Vector Orientation — orientation-free everywhere, contracts at boundaries

**MATLAB** wanted row/column to always matter. **FORTRAN** wanted every vector declared. **Python** noted NumPy's 1-D arrays cause the fewest surprises in elementwise math.
**Verdict:** Orientation-free `vector(N)` is a first-class shape for all elementwise arithmetic. At matrix boundaries (`*`, `\`, 2-D producers/consumers) orientation-free is a contract error (`Q1023`), never an implicit convention. Amends §14 rule 5.

### 34.B.2 MATLAB Compatibility Scope — per module, never per file

**MATLAB** argued per-file compat helps ported scripts. **FORTRAN** and **Python** argued a file must parse identically everywhere.
**Verdict:** `compat="matlab"` and `index_base=1` apply to `module` blocks only. A file is always base Qu outside such modules.

### 34.B.3 OpenCL Lowering Target — requirements fixed now, target chosen in Phase 5

**Verdict:** The spec fixes requirements, not the artifact: lowering must be deterministic, visible in `explain`, and must not change source semantics. The concrete choice (OpenCL C vs SPIR-V vs a portability layer) is delegated to roadmap Phase 5, with a recorded recommendation: start with a portability layer over OpenCL C; revisit SPIR-V when multiple vendors demand it.

### 34.B.4 Report Export Scope — split required/optional

**Verdict:** Required of every implementation: PNG, SVG, PDF figure export and CSV data export (the MVL set). Required of the full standard library: DOCX and LaTeX reports per §26. Optional packages: xlsx, hdf5, parquet families.

### 34.B.5 Units — ~~metadata~~ superseded by 0.4

**Original verdict** (0.2.1): units stay metadata. **Superseded:** 0.4 adopted first-class units with dimensional analysis (§42.2), opt-in per value — the openness the original verdict wanted is preserved because plain numbers stay unitless. Recorded for provenance.

### 34.B.6 Query Timing — superseded by 0.2 frames

**Verdict:** Queries are active for frames (§19, §36.2); over arrays of records and streams they remain reserved until the IR stabilizes.

### 34.B.7 Deterministic Reductions — default on all backends

**FORTRAN** insisted reproducibility is non-negotiable for tests. **MATLAB** wanted GPU speed by default. **Python** noted silent nondeterminism violates the honesty pillar (§3.7).
**Verdict:** `deterministic` is the default everywhere, GPU included. `with reduction=fast` is the explicit opt-out; slow deterministic paths must surface a `Q2105` performance note instead of switching silently. Amends §21.

### 34.B.8 Plotting — core namespace, required

**Verdict:** The `plot` namespace is required standard library (report-ready pillar, §3.4). Themes, styles, and exotic figure types are optional packages. §27 stands.

### 34.B.9 `data table` Column Types — adopted

Confirmed in 0.2: the `table` literal takes an optional type row (§36.1); untyped headers keep 0.1 inference behavior.

### 34.B.10 `#macro` Default Parameters — adopted, restricted

**Verdict:** `#macro name(a, b=1)` is legal; defaults substitute textually at expansion. No recursion and no body type-checking still hold. Amends §7.B.

## 34.C. Third Debate Record (0.11.1): Syntax and Ease

**Mandate:** argue syntax with one goal — *familiar and very intuitive*. The test
is not "can an expert write it" but "can a newcomer *predict* it" (§58.7). The
same five personas debated seven standing questions; verdicts below are normative
and the sections they amend are named. This record builds on §58 (familiar-first
surface) rather than replacing it.

### 34.C.1 How many reserved words? — Austerity: ~45 structural, the rest contextual

**MATLAB** opened bluntly: "I reserve twenty words. Twenty. My users write
`fit = fit(x,y)`, `data = load(...)`, `table = readtable(...)` every day, and
nothing breaks. Qu reserves **a hundred and twenty-five** — including `window`,
`signal`, `spectrum`, `data`, `table`, `fit`, `train`, `model`. A DSP engineer
porting a windowing script cannot name a variable `window`. That is the least
familiar thing in the entire language."

**Python** agreed: "Thirty-three keywords, and `dict`, `list`, `str` are ordinary
names. Reserving domain vocabulary is how a language tells scientists it was
designed by compiler writers."

**FORTRAN** defended the reservations: "A keyword that appears mid-expression
must be reserved, or parsing becomes guesswork and error messages rot."

**BASIC** noted the heritage sets are small: "BASIC survived on fifteen."
**POV-Ray** added that its directive words live only after `#`, never in the
program text.

**Verdict:** *Austerity rule adopted.* Two classes, no third:
1. **Reserved** (≈45 words): only words that appear **inside expressions or
   mid-statement** — control flow, operators spelled as words, definition and
   module words, literals. These are words no one expects to use as a name.
2. **Contextual introducers** (everything else): recognized as syntax **only at
   a statement/block head**; everywhere else they are ordinary identifiers. If a
   local binding shadows one, a later introducer use in the same scope is a
   `Q1015` diagnostic asking for a rename — never a silent reinterpretation.
Type names (`signal`, `frame`, `int`, ...) are contextual after `as`. The full
regrouped list is §6. Amends §6 and the grammar header.

### 34.C.2 The third function form `g: x -> y` — dropped, never implemented

**BASIC:** "Three spellings for one idea is two too many. A 14-year-old meets
`function`, finally gets it, and then a textbook shows `g: x -> y`."
**POV-Ray:** "It reads like the mathematics on the whiteboard; the worksheet
style is the point of Qu."
**MATLAB:** "One keyword, one form, twenty-five years of that lesson."
**Python:** "One obvious way—but we also keep comprehensions beside loops, and
nobody dies."

**Verdict (2026-09-17):** *Dropped.* The provisional verdict below shipped 0.8
documentation for `g: x -> y ... end` as valid Layer-3 syntax, but the form was
never actually built: neither `qu-syntax` nor `qu-interp` ever gained a parser
rule, statement variant, or arrow-token handling for it. Ahmed ruled directly
that the form is not needed and it is withdrawn; Qu keeps its two real function
forms, `f(x) := expr` (§41.1) and `function ... end function` (§17.1, §58.3). This
record is kept for history rather than deleted, per this document's own
convention of keeping debate records alongside their outcomes.

**Original provisional verdict (0.8, superseded above):** `g: x -> y ... end`
remains valid Layer-3 syntax (§58.2), but: (a) no tutorial, example, or standard
library may **require or emit** it; (b) it is absent from the teaching surface;
(c) at 1.0 it is either promoted with usage evidence or demoted to
formatter-displayed sugar over `function`. Resolved the standing question in
the 0.11 board note; amended §17.1 usage guidance (no grammar change) at the time.

### 34.C.3 Inclusive `to` ranges beside half-open slices — keep both, diagnose the seam (REVERSED — see §15)

> **Reversed in 0.13.** The verdict below was to keep both conventions and
> make the seam safe with diagnostics. The diagnostics were never built, and
> the seam turned out to be sharper than the debate allowed for: `a:b` was
> inclusive as a *value* and half-open as an *index*, so `k = 0:2` followed by
> `v[k]` gave three elements where `v[0:2]` gave two. That is not a seam
> between two conventions, it is one expression with two meanings. Slices are
> now inclusive, matching `to` and the compact literal. The Python argument
> below is still the strongest one against, and the price is real: `a[0:k]`
> and `a[k:N]` now overlap at `k`, and there is no empty slice. The record
> stays because the reasoning was sound on what it knew.

**FORTRAN:** "Inclusive bounds are sixty years of muscle memory: `DO 10 K=1,N`."
**Python:** "Half-open slices compose: `x[0:N]` has exactly `N` elements, and
`a[0:k] ++ a[k:N]` is `a`. That is why I win at boundaries."
**MATLAB:** "I do inclusive everything, and my users write `n+1` in every loop."
**BASIC:** "`FOR K = 1 TO 20` — inclusive, obviously; it names the last value."

**Verdict:** *Keep both — they do different jobs.* `start to stop` **enumerates**
(names its last value, loop-reading); `a:b` **bounds** (names edges,
partition-reading). The seam is made safe by three **required** diagnostics:
1. `Q1031` — index equals length: `x[N]` on length-`N` data asks
   "zero-based: did you mean `x[N-1]`, or the slice `x[0:N]`?"
2. `Q1032` — slice-stop equals length is reported with the element count in
   `explain` when the slice feeds a shape contract.
3. `Q1033` — inclusive-range endpoint within tolerance of `stop` warns and names
   the tolerance (§12 already requires this; the code makes it testable).
A porting checker (`qu check --from-matlab`) must flag `[...]`-with-`end`
arithmetic that implies one-based habits. Resolves the second standing question
in the 0.11 board note. Amends §30.

### 34.C.4 `:=` doing double duty — keep the unification, explain the reading

**Python:** "Two meanings for one operator is how you get Stack Overflow
questions." **POV-Ray:** "It is *one* idea — 'is defined as' — over values and
over functions; mathematics has used `:=` exactly this way for a century."
**FORTRAN:** "So long as the parser never guesses, I withdraw the objection."
**MATLAB:** "Anything but another symbol."

**Verdict:** *Keep.* `name := expr` (deferred value) and `head(args) := expr`
(elemental definition) share one mental model (§41.1). Two support rules: parse
errors involving `:=` must name which reading was attempted, and the formatter
canonicalizes spacing (`x := value`, `f(x) := body`) so the two never look
identical at a glance. No grammar change.

### 34.C.5 Block closure — `end <introducer>` forever; indentation never semantic

**BASIC/VB heritage** carried before the argument started. **Python** defended
significant indentation: "millions of users." **MATLAB:** "My users paste code
from emails and PDFs; whitespace-as-meaning is hostile to that world." **POV-Ray**
and **FORTRAN** both use explicit terminators.

**Verdict:** *Closed, not revisited.* `end if`, `end for`, `end function`, ...
with `try ... end` the single documented exception (§16, §18.B). Braces rejected
(wrong audience), indentation rejected (paste/generated-code hostility, §58.7
question 3). Future block forms **must** close with `end <introducer>`.

### 34.C.6 Indexing base — zero stays; the compat story is tooling, not syntax

Reopened one last time by **MATLAB**: "Every port starts with an off-by-one."
**FORTRAN** noted one-based arrays and zero-based *offsets* coexisted in his
world for decades without confusion — because the convention was *visible*.
**Python/BASIC** had no stake left after 34.C.3.

**Verdict:** *Final.* Zero-based in base Qu (slice harmony, §58.1); one-based
only inside `module ... (index_base=1)`. The familiarity burden moves to
**tooling**: the porting checker (34.C.3) plus `Q1031` carry it. This question
is removed from the open list permanently.

### 34.C.7 Parenthesis-free commands — the closed set is frozen

**Verdict:** *Affirm and freeze.* The declarative commands (`grid on`, `box on`,
`hold on/off`, `show plot`, `close figure/all`, `legend`, `clear`, and the
§39.4 animation verbs `render`/`play`/`pause`/`record`/`snapshot`) are the
complete set. Their operands are modes, not arguments (§58.4). No library may
add new paren-free call syntax; the grammar's `FigureCommand` and
`RenderStatement` productions are exhaustive.

---

## 35. Open Design Questions

Items 1–4 and 6 are resolved — see the second debate record (§34.B); item 5 was resolved by 0.4 (§42.2). The 0.11 familiarity questions (the dropped `g: x -> y` proposal, inclusive-`to` beside half-open slices, indexing base, keyword count, `:=` duality) are resolved by the third debate record (§34.C). The standing questions:

1. (0.2) Should the frame index be a distinguished column or an ordinary column with a role tag?
2. (0.2) Should `grad` default to reverse-mode always, or auto-select forward vs reverse from the shape of `wrt`?
3. (0.2) Should the animation frame clock be wall-time-locked in interactive mode, or always exact-frame with dropped frames on slow render?
4. (0.2) Should `train` and `scene` be core syntax or standard-library macros expressed with the §16 `each`/`for` and §18.C plot primitives?
5. The FFI ABI (§28) is unspecified; fixing it must precede native plugins.
6. The `each` reduction clause (§16) is deferred; a deterministic-by-default reduction design must accompany it.
7. Appendix X explorations not yet promoted (`@type` suffix, `Redim`/`keep`, `ez`/`ze` blocks, linespecs) await promotion debates. The mapping form `g: x -> y` (formerly Appendix X.4) was never implemented and is dropped as of 2026-09-17 (§34.C.2); it is not a standing question.
8. Keep `diff` as the derivative (0.10 §43) or the MATLAB differences meaning? Units: interval arithmetic or first-order uncertainty? (standing from the 0.6 board note)
9. Novice testing must validate the §34.C.3 seam diagnostics (`Q1031`–`Q1033`) and the §58 teaching order before 1.0 freezes the surface.


---

## 36. DataFrames and Tables

Version 0.1 has arrays, matrices, and the `data table` block for embedded numeric
data. Version 0.2 adds a first-class **frame** — a named, heterogeneous, columnar
table — for data-science workflows. A frame is columnar: each column is a
homogeneous Qu array, so every array and DSP operation applies to a column without
conversion.

### 36.1 The `frame` type

```qu
df = read_csv("quality_data.csv", headers=true)   # -> frame
shape(df)          # (rows, cols)
df.columns         # vector of column names (string)
df["voltage"]      # one column, as a numeric array
df.voltage         # same column, dotted access when the name is an identifier
```

A frame has an ordered set of named columns and an optional index column. Column
access returns a live view; assigning to it mutates the frame:

```qu
df["power"] = df.voltage .* df.current      # add a computed column
```

The `table` keyword builds an inline frame literal (a superset of the 0.1
`data table` block, now with per-column types):

```qu
readings = table
    t as float64   v as float64   ok as bool
    0.0            1.00           true
    0.1            0.98           true
    0.2            0.95           false
end table
```

If the type row is omitted, column types are inferred from the first data row,
matching the 0.1 `data table` behavior.

### 36.2 Selection and filtering

Row filtering uses the LINQ-style query syntax reserved in §19, now active for
frames:

```qu
good =
    from r in df
    where r.ok and r.v > 0.9
    select r
```

Column projection and boolean masks:

```qu
sub = df[["t", "v"]]              # projection to two columns -> frame
hot = df[df.v > 0.97]            # boolean row mask -> frame
```

Slicing follows array rules (zero-based, both ends included): `df[0:99]` is the
first 100 rows as a frame.

### 36.3 Group, aggregate, join

```qu
by_batch =
    df
    |> group by batch
    |> aggregate mean(v) as v_mean, std(v) as v_std, count() as n
```

The `|>` pipe is the single left-to-right dataflow pipe from §13 level 10, now
defined for frames. Bare `|` is not a data pipeline; it remains available for
domain operators such as parallel circuit composition (§48.3).
Each stage takes a frame and returns a frame.

Joins:

```qu
merged = join left=orders right=customers on order.cust_id == customer.id kind=inner
```

`kind` is one of `inner`, `left`, `right`, `outer`. Join is a pure expression and
must be optimizable; it may not hide a side-effecting loop.

### 36.4 Reshape and missing data

```qu
long = pivot_longer(df, keep="t", into=("channel", "value"))
wide = pivot_wider(long, names="channel", values="value")

df2 = dropna(df)                 # drop rows containing nan
df3 = fillna(df, 0.0)            # replace nan with a value
```

Missing numeric data is represented by `nan`; missing strings by the empty-optional
value `none`. Reductions over columns respect a `skipna` policy (default `true` for
frame aggregations, `false` for raw array reductions, so array math stays exact).

### 36.5 Frame I/O

Frames read and write every file family from §26. `read_csv`, `read_json`,
`read_parquet`, and `read_xlsx` return frames; `write_csv`, `write_json`,
`write_parquet`, `write_xlsx` accept them. A frame also exports directly to a
report:

```qu
export df to "summary.docx"       # rendered as a formatted table
export df to "summary.tex"        # LaTeX tabular
```

### 36.6 The fluent pipeline — from data to a plot in one line

Data work reads best as a left-to-right chain of verbs that ends in a picture. The
`|>` pipeline (§41.4, lazy) carries a frame through selection, sorting, and grouping
straight into a plotting verb, so exploration is a single readable expression:

```qu
df
|> where soc > 0.2 and ok
|> select voltage, current, power
|> sort by power desc
|> histogram(column=power, bins=40)

df |> group by batch |> aggregate mean(power) as p |> sort by p |> bar(x=batch, y=p)

df |> sort by t |> scatter(x=voltage, y=current, color=batch)
```

Verbs: `where` (filter, §36.2), `select` (project columns), `sort by COL [asc|desc]`,
`top n` / `head n`, `group by` / `aggregate` (§36.3), `join` (§36.3). Terminal
plotting verbs — `plot histogram`, `plot bar`, `plot scatter`, `plot line`,
`plot box`, `plot heatmap` — consume the frame and read column names for axes and
legends automatically. Because the pipeline is lazy, the optimizer fuses filter,
sort, and aggregate before the plot forces execution (§41.4); `sort by` on a
plotted column is elided when the plot does not need order. The whole chain stays
one expression, so a histogram of a filtered, sorted subset is a single line.

---

## 37. Statistics and Modeling

Version 0.1 lists reduction and statistics functions (§25). Version 0.2 adds a
`stats` namespace and a small, uniform **model** protocol so that classical
statistics and machine learning share one call shape.

### 37.1 Descriptive and inferential statistics

```qu
import stats

s = stats.describe(x)            # record: n, mean, std, min, q1, median, q3, max
r = stats.corr(a, b)             # Pearson correlation
c = stats.cov(A)                 # covariance matrix of columns

# Hypothesis tests return a record with .stat, .pvalue, .df
t = stats.ttest(a, b, kind="welch")
k = stats.ks_test(sample, "normal")
a = stats.anova(groups)
```

### 37.2 The model protocol

Every model — a line fit, a GLM, a clustering, or a neural network (§38) — obeys
the same three-verb protocol:

```qu
m = model.fit(X, y)              # returns a fitted model handle
yhat = m.predict(Xnew)           # apply the model
s = m.score(X, y)                # scalar goodness metric (R^2, accuracy, ...)
```

A fitted model is a `record`-like handle with read-only fields for its parameters
and diagnostics (`m.coef`, `m.intercept`, `m.residuals`, `m.loss_curve`, ...).

### 37.3 Classical models in the standard library

```qu
import ml.linear
import ml.cluster

fit = ml.linear.ols(X, y)                 # ordinary least squares
fit.coef                                   # coefficients (vector)
fit.r2                                      # coefficient of determination

ridge = ml.linear.ridge(X, y, alpha=0.1)
logit = ml.linear.logistic(X, y)

km = ml.cluster.kmeans(X, k=3, seed=1234)
labels = km.predict(X)
```

Because a fitted model exposes its parameters as ordinary Qu arrays, downstream DSP
and plotting apply directly: `plot(x, fit.predict(x))` overlays a regression line
on a scatter without special glue.

### 37.4 Pipelines

A `pipeline` composes transforms and a final estimator into one model that obeys
the protocol:

```qu
pipe = pipeline
    stage scale = ml.pre.standardize()
    stage reduce = ml.pre.pca(k=10)
    stage model = ml.linear.logistic()
end pipeline

fit = pipe.fit(Xtrain, ytrain)
acc = fit.score(Xtest, ytest)
```

`stage` names are addressable after fitting: `fit.reduce.components`.

---

## 38. Machine Learning: Tensors, Autodiff, and Layers

Version 0.2 makes Qu differentiable. The array model from §9 is unchanged; a
**tensor** is an array that additionally carries a gradient tape when it is created
with `param` or wrapped by `track`.

### 38.1 Parameters and automatic differentiation

```qu
param W = randn(10, 3)           # a trainable tensor, gradient-tracked
param b = zeros(1, 3)

# grad(f, wrt) returns the gradient of a scalar-valued expression
loss = mean((X * W + b - Y)^2)
gW, gb = grad(loss, wrt=(W, b))
```

`grad` performs reverse-mode automatic differentiation over the deferred dataflow
graph already defined for `:=` in §11. Because array expressions are recorded as a
graph, `grad` reuses the same fusion and backend lowering path; gradients run on
the selected backend (CPU, CUDA, OpenCL, SYCL) with the same precision policy
(§21). Forward-mode is available as `grad(f, wrt=x, mode="forward")` for
Jacobian-vector products.

`track expr` promotes an ordinary array into the tape for a local computation;
`stop_grad(expr)` detaches a subexpression so no gradient flows through it.

### 38.2 Optimizers and the training loop

```qu
opt = ml.optim.adam(params=(W, b), lr=1e-3)

for epoch = 0 to 200
    loss = mean((X * W + b - Y)^2)
    opt.step(loss)               # computes grads, updates params in place
    if epoch mod 20 == 0 then
        print("epoch {epoch}: loss={loss}")
end for
```

`opt.step(loss)` is sugar for `grad` followed by an in-place parameter update.
Optimizers include `sgd`, `momentum`, `rmsprop`, and `adam`. A `with reduction=...`
block (§21) still controls determinism of the gradient reductions.

### 38.3 Declarative training with `train`

The imperative loop above always works. For standard supervised training, the
`train` block is the recommended declarative form (mirroring the declarative plot
block of §18.C):

```qu
result = train model over dataset
    loss = cross_entropy
    optimizer = adam(lr=1e-3)
    epochs = 50
    batch = 64
    metric accuracy
    on epoch report loss, accuracy
end train

plot(result.loss_curve, label="train loss")
```

`train` returns a record with `.model` (the fitted handle), `.loss_curve`,
`.metrics`, and `.history`. The `model` obeys the §37.2 protocol, so
`result.model.predict(Xnew)` works.

### 38.4 Layers and models

Neural networks are built from `layer` definitions composed inside a `model` block.
A `layer` is a named, parameterized, differentiable function; a `model` is an
ordered composition.

```qu
model mlp(in_dim, hidden, out_dim)
    layer h1 = dense(in_dim, hidden), activation=relu
    layer h2 = dense(hidden, hidden), activation=relu
    layer out = dense(hidden, out_dim), activation=softmax
end model

net = mlp(784, 128, 10)
y = net(x)                        # forward pass; parameters are gradient-tracked
```

Built-in layers include `dense`, `conv1d`, `conv2d`, `lstm`, `gru`, `attention`,
`dropout`, `batchnorm`, `embedding`, and `pool`. Activations (`relu`, `gelu`,
`tanh`, `sigmoid`, `softmax`) are ordinary elementwise functions and may be used
outside layers. A `model` is callable, composes with `train` (§38.3), and obeys the
model protocol (§37.2). Saving and loading:

```qu
save_model(net, "mlp.qm")
net2 = load_model("mlp.qm")
```

### 38.5 Devices and precision for ML

ML inherits the full device and precision model of §20–§21. A whole training run
can be placed and mixed-precision-trained with the existing `with` block:

```qu
with backend cuda, precision=float32, reduction=fast
    result = train model over dataset
        loss = cross_entropy
        optimizer = adam(lr=1e-3)
        epochs = 50
    end train
end with
```

Nothing about ML introduces a second execution model: tensors are arrays, layers
are functions, gradients are dataflow, and backends are declarations.

---

## 39. Animation and Live Visualization

Version 0.1 defines static figures, plot commands, and a declarative plot block
(§18.C, §27). Version 0.2 adds **time**: a first-class animation subsystem for
producing motion — evolving signals, training curves that draw themselves,
parameter sweeps, rotating surfaces, and interactive scenes — and exporting them to
GIF, MP4, or an interactive HTML canvas.

### 39.1 The `animate` block

The core construct is an `animate` block. It declares a time range and a body that
is re-evaluated for each frame. The loop variable is the frame time.

```qu
animate t from 0 to 2 step 1/60      # 2 seconds at 60 fps
    x = sin(2*pi*(f*tt - t))          # a traveling wave; tt is the space axis
    plot(tt, x)
    ylim(-1, 1)
    title("t = {t:0.2f} s")
end animate
```

Each pass through the body renders one frame into the current figure. The range
uses the same `from ... to ... step` grammar as `for` (§16); `step 1/60` sets the
frame period. An optional `fps=` clause is equivalent:

```qu
animate t from 0 to 2 fps 60
    ...
end animate
```

### 39.2 Frames, timelines, and easing

For animation of discrete states rather than a continuous clock, `frame` iterates a
sequence and `tween` interpolates between keyframes:

```qu
timeline demo
    at 0.0   set angle = 0,   ease=linear
    at 1.0   set angle = 90,  ease=in_out
    at 2.5   set angle = 360, ease=out
end timeline

animate over demo
    surf(rotate(Z, timeline.angle))
end animate
```

A `timeline` is a named schedule of keyframes. `at TIME set VAR = VALUE` fixes a
control value at a time; `ease=` chooses the interpolation curve (`linear`, `in`,
`out`, `in_out`, `bounce`, `elastic`, or a user `def fn` mapping `[0,1] -> [0,1]`).
`animate over TIMELINE` drives the body from the timeline's total duration and
exposes each control as `timeline.NAME`.

`tween(a, b, u)` returns the eased interpolation between `a` and `b` for `u` in
`[0, 1]`; it works elementwise on arrays, so whole signals or surfaces can morph.

Keyframes may target a member path such as `car.rotation.y`, `camera.position`, or
`material.roughness`. A timeline compiles those declarations into typed track arrays
(`time`, `value`, `ease`) before playback. Missing channels retain their previous
value; duplicate times for the same channel are a `Q39xx` error rather than an
order-dependent overwrite. Transform interpolation uses linear translation/scale
and shortest-path quaternion rotation in production backends. The portable
reference representation may store Euler angles, but it must produce the same
sampled transforms at exact frame times.

### 39.3 Scenes and layers (declarative animation)

Following the POV-Ray-influenced declarative plot architecture (§18.C), complex
animations are described as a **scene** of layered, independently-animated objects:

```qu
scene spectrum_show
    layer wave
        data tt, sin(2*pi*f*tt)
        color = "blue"
        animate f from 1 to 20 fps 30
    end layer

    layer bars
        data freqs, magnitude
        kind = bar
        color = "orange"
    end layer

    camera view2d
    axes { xlabel="t", ylabel="amplitude", grid=on }
    duration = 4
    export = "spectrum.mp4" fps=30
end scene

render scene spectrum_show
```

A `scene` composes layers into one timeline; each `layer` may carry its own
`animate` clause or be static. `camera` selects `view2d`, `view3d`, or a scripted
camera path. The scene's `duration` and `export` behave like the plot block's
export options, extended with `fps`.

### 39.4 Playback, capture, and export

```qu
render animation                 # play in the interactive figure window
pause animation                  # controls available in REPL / GUI
play animation

record animation to "wave.gif" fps=30
record animation to "wave.mp4" fps=60 quality=high
snapshot to "frame_042.png" at t=0.7      # single frame at a time
```

Export targets: `gif`, `mp4`, `webm`, and `apng` for motion; `png`, `svg`, `pdf`
for a single snapshot; and an interactive `html` canvas that embeds the animation
with play controls. Headless execution (`qu script.qu --headless`) renders directly
to the file without opening a window, so animations can be produced on a server or
in a report pipeline. A report (`export all to "report.docx"`) embeds a static
snapshot of each animation plus a link to the motion file.

### 39.5 Interactive controls

An animation may bind live controls that a viewer manipulates. Controls are
declared, not wired by callbacks, keeping scripts declarative:

```qu
control f = slider(1, 50, default=5, label="frequency")
control show_env = toggle(default=true, label="envelope")

animate t from 0 to 4 fps 30
    x = sin(2*pi*f*tt) * exp(-0.5*tt)
    plot(tt, x)
    if show_env then plot(tt, exp(-0.5*tt), color="orange", linestyle="dashed")
end animate
```

When a control changes, the current frame re-renders with the new value. In a
headless render, controls take their `default`. Controls include `slider`,
`toggle`, `dropdown`, and `field`.

### 39.6 Determinism and performance

Animation obeys the honesty pillar (§3.7). The frame clock is exact and
backend-independent: frame `k` always corresponds to time `k * step`, regardless of
render speed, so a recorded file is reproducible. Per-frame array expressions use
the same deferred dataflow and fusion as ordinary code; the planner may cache
frame-invariant subexpressions across frames, and `explain` reports which
subexpressions were hoisted out of the frame loop.

---

## 40. Reference Example: Data Science to Animation

A single script that reads a table, fits a model, and animates the fit converging —
exercising §36, §37, §38, and §39 together.

```qu
backend auto
import ml.linear

df = read_csv("quality_data.csv", headers=true)
X = df[["voltage", "current"]]
y = df.power

param W = zeros(2, 1)
param b = zeros(1, 1)
opt = ml.optim.adam(params=(W, b), lr=1e-2)

losses = []
animate epoch from 0 to 120 fps 30
    yhat = X * W + b
    loss = mean((yhat - y)^2)
    opt.step(loss)
    losses = concat(losses, loss)

    subplot(1, 2, 1)
    scatter(y, yhat, label="pred vs true")
    title("epoch {epoch}: loss={loss:0.3f}")

    subplot(1, 2, 2)
    plot(losses, color="orange")
    title("training loss")
end animate

record animation to "fit_converging.mp4" fps=30
export df to "quality_summary.docx"
```

---

---

## 41. Design Commitments for Best-in-Class Signal Processing, Data Science, and Numerics

The chapters up to §40 make Qu *capable*. This chapter makes it *excellent*. These
are the four load-bearing decisions that separate a best-in-class language for
signals, data, and numerics from a merely pleasant one. Each is grounded in the
reference workloads in `catalog/`, which are ports of real projects and exercise
the language as it is actually used.

The guiding influences narrow to four, each contributing one strength:

| Influence | Contribution to the commitments |
|---|---|
| Visual Basic | Readable definition and control-flow syntax; `to`/`step`/`then` |
| FORTRAN | Array layout, `ELEMENTAL` functions, and a real speed discipline |
| MATLAB | Matrix-first ergonomics and a compact DSP vocabulary |
| Python | Duck-typed generic protocols and broadcasting — resolved at compile time |

### 41.1 `:=` is the canonical function-definition form (definition, kind 1)

**Decision.** `:=` means "is defined as." When its left side is `name(args)`, it
defines a one-line function; when the left side is a bare `name`, it binds a
deferred dataflow value (§11). One operator, one mental model: a named rule the
planner may inline, fuse, cache, or evaluate on demand.

```qu
zof(fr)  := R0 + R1 ./ (1 + 2i*pi*fr*tau1) + R2 ./ (1 + 2i*pi*fr*tau2)
crest(x) := max(abs(x)) / rms(x)
db(g)    := 20 * log10(g)

Mt := t as matrix(K, N)          # deferred value — same operator, same idea
S  := A * cos(2*pi*f*Mt + phi)
```

Both example scripts previously wrote these one-liners as `def fn zof(fr) = ...`
and `def fn crest(x) = ...`. Under 0.3 the canonical spelling is `zof(fr) := ...`;
`def fn` is retained as an accepted alias so existing scripts keep working.

**Semantics.**

1. A `:=` function body is a single expression — no assignments, no control flow
   (use `function ... end function` for those). It is pure and side-effect-free.
2. It is `ELEMENTAL` by default (FORTRAN influence): applying it to an array maps
   over elements with broadcasting, so `zof(f)` returns a vector when `f` is one.
3. The planner may inline it into a fused dataflow graph or lift it to a real
   function; either way the observable result is identical.
4. The one-line form is non-recursive. Recursion, multiple statements, or multiple
   named outputs require `function`. A third "mapping form" (`g: x -> y … end`)
   was documented as adopted in 0.8 but never implemented, and is dropped
   (§34.C.2). Only two function forms are accepted: this one and `function`.
5. `=` is eager assignment; `:=` is definition/deferral. This is the only
   distinction a reader must learn, and it matches mathematical usage.

### 41.2 Sampled-signal types — `Fs` and axis travel with the value

**Decision.** `signal` and `spectrum` are first-class types: an array plus a
**sampling contract** (sample rate `Fs`, sample count `N`, and an axis role — time
or frequency). The sampling contract is part of the value and propagates through
operations, exactly as shape does for arrays (§9).

```qu
x = signal(data, Fs=5e3)         # or:  x as signal(Fs=5e3)
x.Fs        # 5000
x.N         # length
x.dt        # 1/Fs
x.t         # the time axis, computed, not hand-built
```

**Why this is the decisive DSP lever.** The most common defect in MATLAB/NumPy DSP
code is losing track of `Fs`, `df`, and bin indexing. Both reference scripts carry
the same boilerplate on every run:

```qu
df   = fs / N
bins = round(f * N / fs)
freq = range(0, N/2 - 1) * df
```

With sampled-signal types this becomes structural, not manual. `fft` returns a
`spectrum` that already knows its frequency axis and bin mapping:

```qu
X = fft(x)                       # X is a spectrum
X.freq                           # frequency axis in Hz — no df, no range()
mag = X.mag[0 : X.nyquist]       # half spectrum by name
Z   = X.at(f)                    # values at physical frequencies f (bin snap)
plot(X)                          # x-axis auto-labeled in Hz
```

**Semantics.**

1. Operations that preserve rate (add, scale, filter) preserve the contract.
2. Operations that change rate (`resample`, `decimate`, `upsample`) update `Fs`
   and `N` and record the change; `explain` reports it.
3. Combining two signals of different `Fs` without an explicit `resample` is a
   diagnostic (a `Q`-series error), the temporal analogue of a shape mismatch.
4. A `spectrum` of a real signal carries Hermitian-symmetry metadata, so
   `ifft(X)` is real by construction and the half/full-spectrum assembly in
   the battery-impedance workload (`concat(Zh, conj(flip(...)))`) is available as a checked
   built-in rather than hand-rolled indexing.
5. Filter designers state cutoffs in Hz against the signal's own `Fs`
   (`butter(x, 4, cutoff=1e3)`), never in normalized radians the caller must
   convert.

A `signal` is still an array underneath, so every array operation, dispatch
method, and fusion rule in this specification applies to it unchanged.

### 41.3 Multiple dispatch and type specialization — the speed model

**Decision.** Functions dispatch on the types of *all* their arguments — element
type, shape class, precision, device, and the signal/spectrum roles above — and
the compiler **specializes** each call site to concrete types, generating
monomorphized kernels with no boxing. One generic `fft`, `conv`, `solve`,
`filter`, or `zof` serves real, complex, `float32`, `float64`, GPU, and `signal`
arguments; users extend a built-in by adding a method for their own type.

```qu
function conv(a as signal,   b as fir)        -> signal      # method 1
function conv(a as gpu.array, b as gpu.array) -> gpu.array   # method 2
method   fft(x as fixed)                                     # user-added overload

# The compiler picks the most specific method and specializes to the
# concrete argument types at the call site — no runtime type tests.
```

This is why `zof` in the battery-impedance workload already works on a scalar frequency, a
real vector, and the complex broadcasting in `Zref = zof(f)` without being
rewritten: it is one elemental generic, specialized per call.

**The performance contract (what "blazing fast" means, precisely).** An
implementation claiming Qu conformance must:

1. **Specialize** on argument type and shape — monomorphized, unboxed kernels.
2. **Fuse** `:=` dataflow graphs — no materialized temporaries for broadcast views
   like `Mt` or tone matrices like `S`; the multisine `sum(A * cos(...), axis=0)`
   lowers to a single fused reduction.
3. **Lower** dense linear algebra and transforms to BLAS, LAPACK, and vendor FFT
   libraries, honoring column-major layout (§9).
4. **Reduce deterministically** on the CPU reference backend by default (§21).
5. **Report honestly** via `explain`: the selected method, the specialization, the
   fused operations, any host/device transfer, and — critically — any case where
   specialization was impossible and a value had to be boxed or a fallback taken.
   Silent boxing or silent fallback is non-conformant.

Influence framing: FORTRAN's speed discipline meets Python's generic protocols,
with the dynamism resolved at compile time instead of at run time.

**Explicit compilation for faster runs.** Execution is JIT by default (specialize on
first call, cache the compiled kernel). For a guaranteed fast path, `compile` forces
ahead-of-time compilation of a function or block to a native, specialized kernel:

```qu
compile function hot_kernel(x as signal, w as vector) -> signal
    ...
end function

fast = compile(my_fn, for=(signal, vector))   # AOT-compile a specialization now
with compile                                    # compile everything in the block
    y = pipeline(x)
end with
```

`compile` removes first-call JIT latency and pins optimization level; it composes
with the standalone/MCU builds of §46.4 (`qu build` AOT-compiles whole programs) and
with explicit `simd` vectorization (§47.6). `explain` reports whether a call ran
JIT-compiled, AOT-compiled, or (a diagnostic) interpreted.

### 41.4 Lazy columnar dataframes — the data-science engine

**Decision.** A `frame` (§36) is **columnar**, and the `|>` pipeline is **lazy by
default**: a pipeline builds a query plan that the optimizer fuses, reorders, and
prunes before any data is touched. A strict consumer — `collect`, `plot`,
`export`, `print` — forces execution. This is the frame-level analogue of `:=`
laziness for arrays, so one execution story covers both.

```qu
summary =
    df
    |> where v > 0.9 and ok
    |> group by batch
    |> aggregate mean(power) as p_mean, std(power) as p_std, count() as n
    |> order by p_mean

result = collect(summary)        # plan optimized, then executed once
explain summary                  # show the query plan and fusions
```

Because a frame column *is* a Qu array, column expressions reuse the same
specialization and fusion as array math (§41.3) — the Monte-Carlo SNR sweep in
The battery-impedance workload expresses naturally as a fused pipeline rather than nested
materializing loops.

**Formula interface.** Statistical models accept an R-style formula over frame
columns, tying directly into the model protocol (§37.2):

```qu
fit = ols(power ~ voltage + current, data=df)
fit = logistic(pass ~ voltage + temp + voltage:temp, data=df)
```

**Missing data.** Nulls are typed — `nan` for numerics, `none` for strings — and
`where` uses three-valued logic so a null predicate excludes the row rather than
silently coercing. Aggregations take an explicit `skipna` (default `true` for
frame aggregations, `false` for raw array reductions so numerical results stay
exact), consistent with §36.4.

### 41.5 One core, four facets

These commitments are deliberately not four subsystems — they are four facets of a
single core: a **typed, shape-aware, sampling-aware, fusible array**.

- A `signal` is an array with a sampling contract.
- A `spectrum` is a signal on a frequency axis.
- A frame column is an array; a `frame` is a set of named columns.
- `:=` defines both fusible values and elemental functions.
- Dispatch and specialization apply uniformly to all of them.

The binding constraint (design pillar §3.7, honesty) is that none of this forks the
execution model: there is still one CPU-reference-first pipeline that lowers to
MKL, CUDA, OpenCL, or SYCL, with `explain` exposing every method choice, fusion,
transfer, and fallback. Best-in-class is not extra machinery bolted on; it is the
same small core, chosen well.

---

---

## 42. Mathematical and Scientific Notation

This chapter is Qu's defining commitment (design pillar §3.8): source should read
like a scientific worksheet, not like a program. It adds a notation surface on top
of the semantics already defined — nothing here changes evaluation, only how it is
written. The governing rule is **dual-surface**: every mathematical glyph has an
exact plain-ASCII equivalent, so the same program can be written on any keyboard,
stored as UTF-8, diffed line-by-line, and rendered with symbols by an editor. A
tool can round-trip between the two surfaces losslessly.

Qu is notation-forward but **not a symbolic algebra system**: it stays numeric-first
(§3.1). `∫`, `∂`, and `solve` are numerical operators (quadrature, autodiff,
root-finding), not a CAS. Symbolic manipulation, if it ever arrives, is a separate
optional layer.

### 42.1 Unicode operators and Greek identifiers

Greek letters and a fixed set of mathematical symbols are valid in source. Each has
a canonical ASCII spelling; the two are interchangeable and mean exactly the same
token.

| Math | ASCII | Meaning |
|---|---|---|
| `≤` `≥` `≠` | `<=` `>=` `!=` | comparisons |
| `·` `×` | `.*`/`*` | multiply (`·` elementwise-or-scalar, `×` cross/outer by type) |
| `÷` | `./` | elementwise divide |
| `√` | `sqrt` | square root (prefix: `√x`) |
| `^` `ⁿ` | `^` | power (`x²` ≡ `x^2`) |
| `∑` `∏` | `sum` `prod` | reduction / big-operator (§42.4) |
| `∫` `∂` `∇` | `integral` `deriv` `grad` | quadrature, derivative, gradient (§42.4) |
| `∈` `∉` | `in` `not in` | membership |
| `→` `↦` | `->` | maps-to (function/return arrow) |
| `∧` `∨` `¬` | `and` `or` `not` | logic |
| `±` | `plus_minus` | value with uncertainty (§42.2) |
| `∞` `π` `τ` `e` | `inf` `pi` `tau` `e` | constants |
| `αβγ…ω`, `ΔΣΩ…` | `alpha` `beta` … | Greek identifiers |
| `ℝ ℂ ℤ ℕ` | `real` `complex` `int` `nat` | number-set type tags |

```qu
ω = 2·π·f                       # ASCII:  w = 2*pi*f
Z ∈ ℂ                           # ASCII:  Z as complex
if 0 ≤ x < 1 then ...           # ASCII:  if 0 <= x and x < 1 then ...
r = √(a² + b²)                  # ASCII:  r = sqrt(a^2 + b^2)
```

Greek identifiers are ordinary names: `τ1`, `ω_c`, `Δf` bind and reference like any
identifier. Unicode subscripts (`ωₖ`, `xᵢ`) are accepted as identifier characters;
`A[k]` remains the indexing form (subscript-as-index is exploratory, Appendix X).

### 42.2 Physical units and dimensional analysis

A numeric literal may carry a physical unit. The value then participates in
**dimensional analysis**: units combine under arithmetic, incompatible units are a
compile error, and conversions are explicit or automatic within a dimension.

```qu
Fs = 5 kHz                      # frequency
R0 = 35 mΩ                      # resistance      (ASCII: 35 mOhm)
C  = 4.7 uF                     # capacitance
t  = 100 s
V  = 3.6 V
I  = V / R0                     # -> 102.857 A     (units derived)
τ  = R0 · C                     # -> 164.5 us      (dimension: time)
f_c = 1 / (2·π·τ)               # -> Hz, checked

x = Fs + 3 V                    # COMPILE ERROR: cannot add [Hz] and [V]
```

Semantics.

1. A bare number is **unitless** (dimension one); unit adoption is opt-in and never
   forced on existing code.
2. Units form the standard SI algebra: base dimensions (s, m, kg, A, K, mol, cd)
   plus derived units and SI prefixes (`p n u m k M G`). **The canonical spelling of
   every unit is ASCII** (§42.6): `Ohm`, `Hz`, `V`, `A`, `F`, `H`, `W`, `J`, with
   the micro prefix written `u` — so `35 mOhm`, `4.7 uF`, `164 us`. The Unicode
   forms `Ω` and `µ` are accepted aliases (`35 mΩ`, `4.7 µF`) but are never
   required, and the formatter emits the ASCII spelling.
3. Arithmetic propagates units; comparison and addition require equal dimensions;
   multiplication/division combine them; transcendental functions require a
   dimensionless argument (so `sin(2·π·f·t)` is checked: `f·t` must be
   dimensionless).
4. A shape/type contract may fix a unit: `Fs as float64 [Hz]`, `Z as vector(K,
   complex128) [Ohm]`.
5. `in` converts within a dimension: `t in ms`, `Z.re in mOhm`. Display uses the
   declared or a natural prefix.
6. Uncertainty rides along: `R = 35 mΩ ± 2 mΩ` is a value with an error bar;
   arithmetic propagates it (first-order), and `plot`/`errorbar` read it directly.
7. Units are **metadata that the compiler checks and then erases**: after checking,
   kernels run on raw numbers with no runtime cost (the honesty pillar still
   applies — `explain` shows the checked dimensions).

This is decisive for the reference domain: an impedance is `[Ohm]`, an excitation
is `[A]`, a measured voltage is `[V]`, and `Z = V ./ I` is dimensionally verified
rather than merely hoped correct.

### 42.3 Definitional forms: `where`, and piecewise `cases`

Mathematics defines things with qualifying clauses and by cases. Qu mirrors both.

**`where` clause** — local definitions attached to an expression or a `:=`
definition, exactly as a paper writes “… where fc = 1 kHz and n = 4”:

```qu
H(f) := 1 / sqrt(1 + (f/fc)^(2n))   where fc = 1 kHz, n = 4

y = m·x + b                         where m = Δy/Δx, b = y0 - m·x0
```

Names bound in a `where` are local to the definition and evaluated once; the clause
is pure. This keeps the headline formula uncluttered — the worksheet style.

**Piecewise `cases`** — a definition by conditions, laid out like a brace in math:

```qu
step(x) := cases
    0        if x < 0
    0.5      if x == 0
    1        otherwise
end cases

relu(x) := cases  x if x ≥ 0  else 0  end        # single-line form
```

`cases` evaluates conditions top to bottom and returns the first match; `otherwise`
(or `else`) is the default and is required unless the conditions are provably
exhaustive. `cases` is `ELEMENTAL` (§41.1), so `step(v)` maps over an array.

### 42.4 Sums, products, integrals, and derivatives

Big-operator and calculus notation are first-class and lower to existing
operations — reductions (§23), numerical quadrature, and autodiff (§38).

**Sum and product** with an index binding:

```qu
x = ∑ k in 0..K-1 : A[k]·cos(2·π·f[k]·t + φ[k])     # traveling multisine
p = ∏ i in 1..n : (1 - r[i])
```

ASCII: `x = sum(A[k]*cos(...) for k in 0..K-1)`. Both forms build the same fused
reduction (§41.3); the multisine in `catalog/qu_multisine.qu` is one `∑`.

**Integral** (numerical quadrature):

```qu
E = ∫ x(t)² dt  from 0 to T            # energy of a signal
q = ∫ f dμ      over region            # general measure form
```

ASCII: `E = integral(x(t)^2, t, 0, T)`. The method (`trapz`, `simpson`, adaptive)
is selectable; default is documented and deterministic.

**Derivative and gradient** (automatic differentiation, §38):

```qu
g  = ∂f/∂x  at x0                       # scalar derivative via autodiff
J  = ∂y/∂x                              # Jacobian when y, x are vectors
gr = ∇ loss                            # gradient of a scalar field
```

ASCII: `g = deriv(f, x) at x0`, `gr = grad(loss)`. These reuse the reverse/forward
autodiff of §38 — `∇` is literally `grad`, so notation and machinery are the same
thing.

### 42.5 Chained comparisons and set-builder

**Chained comparison** reads as in mathematics and evaluates each neighbor once:

```qu
if 0 ≤ i < N then ...                   # 0 <= i and i < N
valid = a < b ≤ c
```

**Set/array-builder** notation constructs a collection from a rule and a filter:

```qu
S = { f(k) for k in 1..K }
peaks = { (f[i], m[i]) for i in 0..N-1 where m[i] > floor }
evens = { 2k for k in 0..n }
```

The builder is an expression (no hidden side-effecting loop, §19) and composes with
the lazy frame pipeline (§41.4) and the `∑`/`∏` binders of §42.4.

### 42.6 ASCII is canonical; Unicode is an accepted alias

The notation surface is sugar with a hard contract, and the contract has a
**direction**: the plain-ASCII spelling is the *canonical, default* form, and the
Unicode glyph is an accepted alias for those who want it. This is the rule the
whole specification follows:

1. **ASCII is the default.** The keyboard-typable spelling is canonical: `Ohm`,
   `Hz`, `V`, `A`, `int`, `diff`, `sum`, `prod`, `grad`, `tau`, `pi`, `2*pi`,
   `sqrt`, `<=`, `>=`, `!=`, `u` (micro). Documentation, generated code, error
   messages, and the formatter all emit ASCII by default. No program ever *requires*
   a non-ASCII character.
2. **Unicode is allowed, never mandatory.** `Ω τ π · ∑ ∫ ∂ ∇ ≤ µ` and the Greek
   letters are accepted because a physicist may prefer to *read* them, but they are
   optional aliases — the editor offers them and a formatter converts either
   direction losslessly.
3. **Exact equivalence.** Each glyph denotes the same token as its ASCII spelling;
   a program is identical in meaning in either surface.
4. **Diff-friendly.** The mapping is one-to-one and line-preserving, so version
   control and review work on either surface; because ASCII is canonical, a diff of
   formatter output never churns on glyph choice.
5. **Checked, then erased.** Units and dimensions are verified at compile time and
   carry no runtime cost (§42.2); notation adds zero execution overhead.

So the canonical multisine is `x = sum(A[k]*cos(2*pi*f[k]*t + phi[k]) for k in
0..K-1)` and the canonical impedance is `Z = R0 + R1/(1 + 2i*pi*f*tau1)` — every
token typable on any keyboard. A reader who prefers `x = ∑ k : A[k]·cos(2·π·f[k]·t +
φ[k])` may write that, and it means exactly the same thing. Earlier chapters show
the Unicode surface to demonstrate it; the ASCII surface is what Qu defaults to.

---

## 43. Numerical Calculus: Differences, Derivatives, and Integrals

Qu has **two differentiation families and one integration family**, and keeping
them distinct is the whole point of this chapter.

| Need | Tool | Chapter |
|---|---|---|
| Derivative of a *function/expression* (exact) | autodiff — `∂`, `grad`, `∇` | §38, §42.4 |
| Derivative of *sampled data* (a signal/array) | finite differences — `diff` | §43.2 |
| Area / accumulation of *sampled data* or a function | quadrature — `int`, `cumint` | §43.4 |

Exact autodiff is for closed forms and learning (a loss, a transfer function you
wrote as `:=`). Finite differences are for measured data where no closed form
exists — the derivative of a battery's measured terminal voltage, `dV/dt`. Both
respect the sampling contract (§41.2) and units (§42.2): the derivative of a `[V]`
signal sampled in `[s]` is a `[V/s]` signal, and the integral of an `[A]` signal
over `[s]` is `[A·s] = [C]`.

### 43.1 Successive differences vs the numerical derivative

Two different operations that MATLAB unfortunately spells the same way; Qu keeps
them apart.

```qu
Δx = differences(x)      # raw successive differences x[k+1]-x[k]; length N-1
g  = diff(x, dx)         # numerical DERIVATIVE dx-scaled; same length, edge-aware
```

`differences` (ASCII alias for `Δ`) is the plain first-difference of §23. `diff`
is the estimated derivative `dx/dt`, scaled by the spacing and corrected at the
ends so the result keeps the signal's length and axis.

### 43.2 Finite-difference schemes: `forward`, `backward`, `central`

`diff` estimates the first derivative with a selectable scheme. `central` is the
default because it is second-order accurate.

| Scheme | Stencil | Accuracy | Notes |
|---|---|---|---|
| `forward` | `(f[k+1] − f[k]) / h` | O(h) | biased right; needs the next sample |
| `backward` | `(f[k] − f[k−1]) / h` | O(h) | biased left; causal — usable in real time |
| `central` | `(f[k+1] − f[k−1]) / (2h)` | O(h²) | default; symmetric, most accurate |

```qu
g = diff(y, dx)                       # central, first derivative
g = diff(y, dx, scheme=forward)       # keyword form
g = diff backward y by dx             # command form (scheme as modifier)

# on a signal, spacing comes from x.Fs and units propagate:
dvdt = diff(v)                        # v is a signal [V] -> dvdt is [V/s]
```

Details.

1. **Spacing.** On a `signal`, `h = dt` from `Fs`. Otherwise pass `dx` (uniform) or
   the coordinate vector `x` (nonuniform — Qu uses the correct unequal-spacing
   stencil rather than silently assuming uniform).
2. **Edges.** To preserve length, `central` falls back to one-sided `forward`
   (first sample) and `backward` (last sample). `edge="drop"` instead shortens the
   output; `edge="periodic"` wraps.
3. **Higher order and accuracy.** `diff(y, dx, order=2)` is the second derivative;
   `accuracy=4` selects a wider, higher-order stencil.
4. **`backward` is the causal choice** — it uses only past samples, so it is the
   scheme for online/streaming differentiation of a live signal.
5. `diff` is `ELEMENTAL`-friendly along the chosen axis; for a matrix, `axis=`
   selects the differentiation direction.

### 43.3 The `df/dx` notation and how dispatch chooses

The mathematical forms `df/dx`, `d²f/dx²`, and `∂f/∂x` are all accepted. Which
machinery runs is decided by **what `f` is** (§41.3 dispatch):

```qu
f(x) := sin(x)
g = df/dx           at x0        # f is a definition -> EXACT autodiff (§38)

v = signal(data, Fs=5e3)
a = dv/dt                        # v is sampled data -> finite difference (central)
```

Rule: the derivative of a *callable definition or expression graph* is exact
autodiff; the derivative of a *materialized array or signal* is a finite
difference (default `central`). `∂` and `∇` always mean autodiff. `explain dv/dt`
reports which family was chosen, the scheme, order, spacing source, and edge
handling. If a value is genuinely ambiguous, the compiler asks for `grad(...)` or
`diff(...)` explicitly rather than guessing.

### 43.4 Integration: `int` and `cumint`

`int` is reclaimed from the integer-type alias (§8) so integration reads as
mathematics. Two forms, mirroring differentiation:

```qu
S   = int(y, dx)                 # definite: total area (a scalar)
Y   = cumint(y, dx)              # cumulative: running integral (same length)

# function form with limits (adaptive quadrature):
E   = ∫ x(t)² dt from 0 to T     # ascii: E = int(x(t)^2, t, 0, T)
```

Methods: `trapz` (default for sampled data), `simpson`, `romberg`, and an adaptive
`quad` for callable integrands. Select with `method=`; the default is documented
and deterministic (§21). On a `signal`, spacing is `dt` and units integrate — this
is exactly the battery state-of-charge computation, now dimensionally checked:

```qu
# the battery-impedance workload wrote:  soc = cumsum(i_exc) * dt / 3600
# with units + calculus it becomes, checked as charge:
q   = cumint(i)                  # i is [A], dt from Fs  ->  q is [A·s] = [C]
soc = q in Ah                    # convert C -> Ah
```

`diff` and `int` are numerical inverses to the stated accuracy:
`int(diff(y, dx), dx) ≈ y − y[0]`.

### 43.5 Determinism and accuracy contract

Every scheme and method is documented and deterministic. `explain` on any `diff`
or `int` expression reports the scheme/method, the truncation-error order (e.g.
O(h²) for `central`, O(h²) for `trapz`), the spacing source (signal `dt`, scalar
`dx`, or coordinate vector), the edge handling, and the units of the result. A
nonuniform grid is never silently treated as uniform — passing a coordinate vector
is required, or the implementation raises a diagnostic.

---

## 44. Heritage Idioms from FORTRAN, BASIC, Octave, MATLAB, and VB6

Qu's ancestors documented idioms that are still the clearest way to express common
tasks. This chapter imports the distinctive ones — not for nostalgia, but because
they read well and a scientist coming from any of these languages should feel at
home. Each is attributed to its source; deliberate deviations are called out in
§44.10.

### 44.1 Provenance

| Feature | From | Qu form |
|---|---|---|
| Masked array assignment | FORTRAN `WHERE`/`ELSEWHERE` | `where … elsewhere … end where` (§44.4) |
| Array shifts / gather / spread | FORTRAN `CSHIFT`,`EOSHIFT`,`PACK`,`MERGE`,`SPREAD` | intrinsics (§44.7) |
| `pure` / `elemental` attributes | FORTRAN | function attributes (§44.5) |
| Multi-way branch | BASIC/VB `SELECT CASE`, MATLAB `switch` | `select … case … end select` (§44.2) |
| Post-condition loop | BASIC/Octave/VB `DO…LOOP UNTIL` | `repeat … until` (§44.2) |
| `swap`, `restore` statements | BASIC | `swap a, b`, `restore` (§44.6) |
| Formatted print | BASIC `PRINT USING` | `print using` (§44.9) |
| Increment operators | Octave `+=`, `++` | compound assignment (§11, §44.3) |
| `printf`/`sprintf`/`disp` | Octave/MATLAB | I/O (§44.9) |
| Default / optional parameters | VB6 `Optional`, Python defaults | `optional`, `p = default` (§44.5) |
| `nargin`/`nargout`/`varargin` | MATLAB | arg introspection (§44.5) |
| `arrayfun` and anonymous functions | MATLAB `@(x) …` | `map`, `(x) -> expr` (§44.5) |
| Member-access block | VB6 `With … End With` | `with expr … end with` (§44.6) |

### 44.2 Multi-way branch and post-condition loops

`select case` (BASIC/VB `SELECT CASE`, MATLAB `switch`) is the readable multi-way
branch. It matches values, ranges, and lists, with `otherwise` as the default:

```qu
select case backend_name
    case "cpu", "cpu(mkl)"     ; use_host()
    case "cuda", "opencl"      ; use_device()
    case 1 to 4                 ; use_tier(backend_name)
    otherwise                   ; error("unknown backend")
end select
```

`repeat … until` runs the body at least once, then tests (BASIC/VB `DO…LOOP UNTIL`,
Octave `do…until`). It complements the pre-condition `while` (§16):

```qu
repeat
    err = step_solver()
until err < tol
```

### 44.3 Compound assignment and increment

Octave's in-place operators are core Qu (defined in §11): `+= -= *= /= ^=`
and the elementwise `.*= ./=` are all real and working. `++`/`--` are NOT
supported — no increment/decrement operator exists.

### 44.4 Masked array assignment: `where` / `elsewhere`

FORTRAN's `WHERE` construct assigns to array elements selected by a mask. Qu adopts
it as the readable form of masked assignment (it lowers to the logical-indexing
assignment of §15):

```qu
where x < 0
    y = 0                      # only where the mask is true
elsewhere
    y = sqrt(x)
end where
```

The single-value selector `merge(mask, a, b)` (FORTRAN `MERGE`) is the expression
form: `y = merge(x ≥ 0, sqrt(x), 0)`. Direct masked assignment `y[x < 0] = 0`
(§15) remains valid; `where` is the block form for multi-statement masks.

### 44.5 Functions: defaults, optionals, arg counts, attributes, lambdas

**Default parameters** (Python-style defaults) — **real, implemented**:

```qu
function butter(x, order = 4, cutoff = 1 kHz, kind = "low")
    …
end function

butter(sig)                 # uses all defaults
butter(sig, 2, 500)         # positional overrides, left to right
```

A parameter with `= value` is optional; once one parameter has a default,
every parameter after it must too. The default expression is re-evaluated
fresh on every call (not computed once at definition time), and may refer
to an earlier parameter (`function f(x, y = x * 2)` works). Composes with
multiple dispatch — arity checking becomes a `[required, total]` range.

**Named arguments on a user-defined function are NOT supported** — only
builtins accept `name=value` calling syntax. `butter(sig, 2, cutoff = 500
Hz)` is a clear runtime error (`` `butter` is a user-defined function --
named arguments... are not supported when calling one``), not the
positional-plus-named mix this once implied; pass every argument to a
user function positionally. There is no separate `optional`
keyword or `is_present()` — a parameter is either required or has a
`= value` default, nothing in between.

**Argument introspection — not supported.** `nargin`/`nargout`,
`varargin`/`varargout`, and `...args` spread/rest parameters do not exist.
`nargin` inside a function body errors `'nargin' is not defined`. None of
this MATLAB-style argument-introspection machinery is planned; multiple
dispatch (§9 of `docs/qu-language-tour.md`) and the `Value::Model`
multi-field-return protocol together cover the use cases this was meant
to address.

**`pure`/`elemental` attributes — not supported.** Neither keyword exists
in the lexer at all; `pure function f(x) ... end function` is not
recognized syntax.

**Anonymous functions — not supported; use the real one-line form
instead.** `sq = (x) -> x^2` is a parse error: `->` is exclusively the
reshape operator (`expr -> (rows, cols)`, sugar for `reshape(expr, rows,
cols)` — §12 of `docs/qu-language-tour.md`), and after it the parser
expects a parenthesized dims tuple, not an arbitrary expression body —
the same token cannot mean both things. There is no lambda-literal syntax
today. The real, existing, verified equivalent for a short named function
is the one-line `:=` form:

```qu
square(x) := x^2
print(square(5))   # 25
```

This already works exactly like a lambda assigned to a name — it just
requires a name, there is no anonymous form. `map`/`reduce`/`filter` take
the function's name **as a string** (`map("square", xs)`, not
`map(square, xs)` — matches the same by-name convention the job-queue
system's `q.push("fnName", ...)` uses, §15 of `docs/qu-language-tour.md`),
and only a function YOU defined, not a builtin:

```qu
square(x) := x^2
print(map("square", [1,2,3,4]))   # (1, 4, 9, 16)
```

### 44.6 Utility statements: `swap`, `restore`, `with`

```qu
swap a, b                        # BASIC SWAP: exchange two bindings
restore                          # BASIC RESTORE: rewind the data/read cursor (§18.A)
restore label                    # rewind to a labeled data block

with measurement                 # VB6 With: member-access shorthand
    .voltage = .voltage - offset
    .flagged = .voltage > vmax
end with
```

`with expr … end with` opens a record so its fields are reachable with a leading
dot; it is distinct from `with backend …` (§20), disambiguated by whether the
operand is a policy or a value.

### 44.7 FORTRAN array intrinsics

Added to the standard library (§23), these are load-bearing for DSP and array code:

```text
cshift(a, n)        # circular shift  (FFT-shift, rotation buffers)
eoshift(a, n, fill) # end-off shift with a fill value
pack(a, mask)       # gather elements where mask is true -> vector
unpack(v, mask, f)  # scatter v back into a masked array
spread(a, dim, n)   # replicate along a new dimension (explicit broadcast)
merge(mask, t, f)   # elementwise select
count(mask)         # number of true elements
maxloc(a) minloc(a) # index of the max / min
```

`cshift` in particular gives `fftshift`/`circshift` for free and expresses the
circular convolution and rotation patterns common in signal processing.

### 44.8 String library (BASIC / VB)

Qu is numeric-first but ships the classic string toolkit so text handling in
scripts is not an afterthought (BASIC `LEFT$`/`MID$`/`INSTR`, VB `Trim`/`Replace`):

```text
len, left(s,n), right(s,n), mid(s,i,n), instr(s,sub)
upper(s), lower(s), trim(s), pad(s,n), replace(s,a,b)
split(s,sep), join(parts,sep), val(s), str(x), format(x,spec)
```

The BASIC `$`-suffixed names (`LEFT$`, `MID$`) are accepted as aliases in a MATLAB/
BASIC compatibility mode; the base names carry no sigil.

### 44.9 Formatted output

`print using` (BASIC `PRINT USING`) formats with a picture template; `printf`/
`sprintf`/`fprintf` (Octave/MATLAB/C) format with `%` specifiers; `disp` prints a
value plainly; `format` sets the default display precision:

```qu
print using "Z = ###.## ohm  at ##.# Hz"; abs(Z), f
s = sprintf("%.3f %+.2e", cf, err)
disp(summary)
format long                      # display precision: long | short | engineering
```

These interoperate with the `{expr:spec}` interpolation of §7.C, which remains the
recommended default; `print using`/`sprintf` exist for column-aligned reports and
for readers coming from BASIC and MATLAB.

### 44.10 Deliberate deviations

Three places where Qu keeps its own meaning rather than the ancestor's, to avoid
ambiguity:

1. `\` is **matrix left-division** (MATLAB/§13), not BASIC/VB integer division;
   integer division is `div` (or `//`).
2. `diff` is the **numerical derivative** (§43), not MATLAB's successive-difference;
   the successive difference is `differences` / `Δ`.
3. `int` is **integration** (§43), not an integer-type alias; the integer type is
   `integer`/`int64` (§8).

Each deviation is in service of reading as mathematics and science; the ancestor's
spelling is provided as an alias only where it cannot be mistaken.

---

## 45. Intuitive Model Building (Advanced ML)

Chapter §38 defined the ML primitives — tensors, `grad`, layers, `train`. This
chapter is about *building models intuitively*: composing architectures by
sketching them, fitting mechanistic equations to data, and expressing probabilistic
models the way they are written on paper. The goal is that a scientist describes
*what the model is*, and Qu handles the fitting machinery.

### 45.1 Sequential and graph models by sketch

A network is built by **chaining** stages with the pipe `|>`, so the code reads in
data-flow order (Keras/Flux heritage, made native):

```qu
net = input(shape=(28, 28, 1))
    |> conv2d(16, kernel=3, activation=relu)
    |> pool(2)
    |> conv2d(32, kernel=3, activation=relu)
    |> flatten()
    |> dense(128, activation=relu)
    |> dropout(0.3)
    |> dense(10, activation=softmax)
```

Branching and merging make it a graph, still readable top to bottom:

```qu
model resnet_block(x)
    h = x |> conv2d(64, 3, activation=relu) |> conv2d(64, 3)
    return relu(h + x)                       # residual/skip connection
end model
```

Any `model` is callable, gradient-tracked, obeys the fit/predict/score protocol
(§37.2), and drops into a `train` block (§38.3). Pretrained blocks compose the same
way: `backbone |> head`. Independent branches of a graph model — the two paths of an
inception/Xception block, the arms of a residual — are scheduled to run
**concurrently** on the shared scheduler (§47.9), so building a wide network is also
building parallel work:

```qu
model inception(x)                     # branches run in parallel, then concatenate
    a = x |> conv2d(64, 1)
    b = x |> conv2d(64, 1) |> conv2d(96, 3)
    c = x |> pool(3) |> conv2d(32, 1)
    return concat(a, b, c, axis=channels)
end model
```

### 45.2 Mechanistic model fitting — write the equation, fit the parameters

The most intuitive model for a physical scientist is often the physics itself. Qu
lets you write a **parametric model as an equation** and fit its parameters to data
by nonlinear least squares — grey-box modeling. This is directly the equivalent-
circuit fitting of impedance spectroscopy:

```qu
# a Randles / two-RC equivalent circuit, written as the equation it is
model Z(f; R0, R1, τ1, R2, τ2) :=
    R0 + R1 / (1 + 2i·π·f·τ1) + R2 / (1 + 2i·π·f·τ2)

fit ecm = Z to (f_meas, Z_meas)
    start  R0 = 30 mΩ, R1 = 10 mΩ, τ1 = 20 s, R2 = 8 mΩ, τ2 = 0.5 s
    bounds R0 in [0, 1] Ω, τ1 > 0, τ2 > 0
    weight = 1 / abs(Z_meas)        # proportional weighting
    loss   = complex_residual        # fits real+imag jointly
end fit

print("R0 = {ecm.R0 in mΩ} ± {ecm.R0.stderr in mΩ}")
plot(real(ecm.Z(f)), -imag(ecm.Z(f)))    # Nyquist of the fitted model
```

`fit MODEL to DATA` returns a fitted model that obeys the §37.2 protocol and
additionally exposes each parameter with its estimate, standard error, and
confidence interval, plus goodness-of-fit (`ecm.r2`, `ecm.chi2`, `ecm.residuals`).
Solvers include Levenberg–Marquardt (default), trust-region, and global (basin-
hopping) for stiff parameter landscapes; gradients come from autodiff (§38) through
the equation, so no hand-derivatives are needed. The same mechanism fits any
model written as an equation — reaction kinetics, transfer functions, decay laws.

### 45.3 Probabilistic models with `~`

For uncertainty-aware modeling, Qu expresses a generative model in the notation of
statistics — `~` reads "is distributed as" — and infers the posterior. This reuses
the formula `~` of §37 and the notation surface of §42:

```qu
model calibration
    # priors
    slope     ~ normal(1, 0.5)
    intercept ~ normal(0, 1)
    σ         ~ halfnormal(0.1)
    # likelihood
    y ~ normal(slope · x + intercept, σ)
end model

post = infer(calibration, data=(x, y), method="nuts")   # or "vi", "map"
print("slope = {mean(post.slope)} ± {std(post.slope)}")
plot(post.slope)                                          # posterior density
```

`infer` runs the chosen inference — MAP (point estimate), variational (`vi`, fast
approximate), or MCMC (`nuts`/`hmc`, asymptotically exact) — and returns a posterior
whose parameters are sampled arrays, so ordinary Qu statistics and plotting apply.
Built-in distributions cover the common families; Gaussian processes
(`gp(kernel=rbf)`) provide nonparametric regression with calibrated uncertainty.

### 45.4 Native architecture and estimator library

Common architectures are **native, one-call constructors** — you do not re-derive
ResNet or a CNN each time. Each is a `model` (callable, gradient-tracked, obeys the
fit/predict/score protocol of §37.2), parameterized and composable, and any can be
used whole or as a backbone under a new head.

```text
classic nets:  mlp(dims),  cnn(...),  rnn / lstm / gru(...)
vision:        resnet(depth=18|34|50|101),  xception(),  vgg(16|19),
               efficientnet(b0..b7),  mobilenet(),  unet(),  vit()
sequence:      transformer(layers, heads, d_model),  bert(),  gpt(),  tcn()
generative:    autoencoder(),  vae(latent),  gan(),  diffusion()
probabilistic: gp(kernel=rbf),  bayesian_nn()
classical:     ols, ridge, lasso, logistic, svm, random_forest,
               gradient_boost, xgboost, kmeans, dbscan, pca, umap
```

```qu
net = resnet(depth=50, classes=10)         # a full ResNet-50, one line
net = xception(classes=1000)               # Xception
feat = resnet(depth=34, pretrained=true) |> dense(1, activation=sigmoid)   # backbone + head
```

Layer primitives (for building your own, §45.1): `dense`, `conv1d/2d/3d`, `lstm`,
`gru`, `attention`, `transformer_block`, `embedding`, `batchnorm`, `layernorm`,
`dropout`, `residual`, `pool`, `upsample`, `separable_conv` (the Xception building
block). Swapping a random forest for a ResNet is a one-line change because both obey
the same protocol, and a `pipeline` (§37.4) composes any of them.

### 45.5 Intuitive training: callbacks, schedules, validation, AutoML

The `train` block (§38.3) takes declarative controls so the loop stays a
description of intent, not bookkeeping:

```qu
result = train net over dataset
    loss = cross_entropy
    optimizer = adam(lr = schedule.cosine(1e-3, epochs))
    epochs = 100, batch = 64
    validate on holdout split = 0.2
    metric accuracy, f1
    early_stop patience = 8 on val_loss
    checkpoint best on val_accuracy
end train
```

`cross_validate(model, data, folds=5)` reports mean±std across folds. For
hands-off model selection, `fit_best(data, task="classify")` searches estimators
and hyperparameters and returns the best fitted model with its leaderboard — an
AutoML entry point that still yields an ordinary, inspectable model.

**Parallel and distributed training.** Training scales with the same concurrency
constructs as the rest of the language (§47.2, §46.5). A `train` block runs
data-parallel across devices when asked, and independent trainings (a
hyperparameter sweep, a k-fold, an ensemble) run concurrently with `parallel for`:

```qu
result = train net over dataset
    loss = cross_entropy, optimizer = adam(1e-3), epochs = 100
    parallel across gpu(0..3)         # data-parallel over 4 GPUs, gradients all-reduced
end train

# independent trainings in parallel (sweep / ensemble)
models = parallel for lr in [1e-2, 1e-3, 1e-4]
    train mlp(64) over data with optimizer = adam(lr), epochs = 50
end parallel
ens = ensemble(models)                # combine the parallel-trained models
```

Cross-validation folds and `fit_best` trials likewise fan out across the thread/GPU
pool, and the model graph itself runs its independent branches (§45.1 residual and
multi-branch models) concurrently on one scheduler (§47.9).

**Asynchronous training.** A `train` is launched asynchronously with `spawn` (§47.2)
or an `async` clause; it returns a live handle you can monitor, checkpoint, or stop
while doing other work, and `await` when you need the result:

```qu
job = spawn train net over data with loss = cross_entropy, epochs = 100
on change(job.epoch) do plot(job.loss_curve) end   # live monitoring, non-blocking
if job.val_loss.plateaued then job.stop() end
result = await job                                   # collect when ready
```

### 45.6 Fine-tuning and hyperparameter search

**Fine-tuning** adapts a pretrained model to a new task — freeze the backbone, train
a new head, then optionally unfreeze with a smaller, discriminative learning rate:

```qu
base  = resnet(depth=50, pretrained=true)
model = base.backbone |> dense(n_classes, activation=softmax)   # new head

finetune model on data
    freeze base.backbone                 # phase 1: train only the head
    optimizer = adam(1e-3), epochs = 5
    then unfreeze base.backbone          # phase 2: unfreeze and adapt
    optimizer = adam(1e-5)               # smaller LR for the pretrained weights
    discriminative_lr = true             # deeper layers get even smaller LR
    epochs = 20
end finetune
```

**Hyperparameter search** tunes parameters over a search space with grid, random, or
Bayesian strategies, running trials in parallel (§45.5) and returning the best model
plus the full trial table:

```qu
best = tune model on data
    search lr in log_range(1e-5, 1e-2), dropout in [0.1, 0.3, 0.5],
           width in {64, 128, 256}
    strategy = bayesian, trials = 40, parallel = true
    objective = minimize val_loss
end tune

best.model ; best.params ; best.trials   # fitted best, its hyperparameters, leaderboard
```

Both `finetune` and `tune` return ordinary, inspectable models obeying the §37.2
protocol, and both are async-launchable with `spawn` for long runs.

### 45.7 Introspection and explainability

A fitted model explains itself through a uniform interface: `importance(model)`
(feature importances / permutation importance), `partial_dependence(model, feature)`,
and `explain(model, x)` (per-prediction attributions). Because parameters and
diagnostics are ordinary Qu arrays, any explanation is also just data to plot,
tabulate, or export to a report (§26).

---

## 46. Design Wishes: What the Community Asks For

The chapters so far were shaped by Qu's ancestor languages. This one is shaped by
what practicing scientists and engineers repeatedly say they *want* from a
scientific language — the desiderata and pain points documented across comparative
studies and community wishlists. Each wish below is turned into a concrete Qu
commitment, so the design answers real needs rather than taste.

### 46.1 The wishes, and Qu's response

| Community wish | Recurring pain | Qu's response |
|---|---|---|
| One language, prototype → production | the "two-language problem": rewrite Python/MATLAB in C for speed | one language; type specialization + JIT lowering (§41.3), no rewrite |
| Compiled speed with scripting ease | fast loops need vectorization gymnastics or a C rewrite | specialization, fusion, backend lowering (§41.3); loops are fast |
| Open and free | commercial licenses block collaboration and inspection | Qu is open by design; no license wall; the reference implementation is inspectable |
| Keep my existing libraries | wrapping C/Fortran or bridging to NumPy is tedious | first-class interop and FFI (§46.2) |
| Reproducible results | seeds, environments, and provenance are ad-hoc; papers don't reproduce | reproducibility & provenance built in (§46.3) |
| Deploy anywhere, including embedded | can't ship a MATLAB/Python model to an MCU | AOT build to standalone binary and to MCU with fixed-point (§46.4) |
| Interactive, literate workflow | notebooks, plots, and variable inspection are bolted on | REPL, notebooks, literate documents, `explain` (§46.5) |
| Extensible without ceremony | overloading and namespaces are awkward | multiple dispatch (§41.3) + modules/namespaces (§18) |
| Easy parallelism | threads and GPUs are hard to reach | `each … on gpu` (§16), backend policy (§20), `parallel` (§46.5) |

### 46.2 Interoperability: keep your ecosystem

A new language must not orphan the libraries people already depend on. Qu treats
interop as first-class, not an afterthought — answering the wish to “integrate
existing libraries without tedious wrapping” and to “embed code from other
languages directly.”

```qu
use python as py                 # bridge the Python ecosystem
np = py.import("numpy")
sp = py.import("scipy.signal")
b, a = sp.butter(4, 0.2)         # call SciPy; arrays convert zero-copy where possible

use c "libmydsp"                 # load a C shared library
y = libmydsp.process(x)          # call a C function with a declared signature

inline fortran                   # embed a kernel in another language (MATLAB-wishlist "code embedding")
    subroutine saxpy(n, a, x, y)
        ...
    end subroutine
end inline
```

Arrays cross the boundary by shape and dtype with zero copy when memory layouts
agree; a copy with a diagnostic otherwise (§22 honesty). Interop is the migration
path: a team adopts Qu incrementally while keeping NumPy, SciPy, and their own C and
Fortran.

### 46.3 Reproducibility and provenance

The most-voiced scientific wish is that results reproduce. Qu makes reproducibility
a language-level guarantee rather than a discipline the user must remember.

```qu
# project.qu — the manifest, with a lockfile pinning exact versions
project "battery-eis"
    qu = "0.5"
    packages = { dsp = "1.2.0", stats = "0.9.1" }
    seed = 2026                  # default RNG seed for the whole project
end project
```

1. **Deterministic randomness.** A project seed (or `seed(n)`, §21) makes every
   random stream reproducible; backends must document any device-dependent stream
   (§21) so a result is either bit-reproducible or flagged as not.
2. **Provenance stamping.** Every exported figure, report, or data file (§26)
   carries a provenance record: the Qu and package versions, the seed, the selected
   backend and precision, the source-file hash, and the checked units. `provenance
   of fig` returns it; a DOCX/PDF report embeds it.
3. **Locked environments.** The lockfile pins exact package versions so a
   collaborator reruns the identical computation; `qu run` refuses to proceed
   silently on a mismatch.

This directly answers the reproducibility pain point: a Qu result travels with
everything needed to regenerate it.

### 46.4 Deployment, including embedded

Scientists want to *ship* what they prototype — as a standalone tool, and, in the
signal-processing and TinyML world, onto a microcontroller. Qu compiles ahead of
time.

```qu
qu build model.qu --standalone            # a self-contained native binary
qu build filter.qu --target cortex-m4     # AOT to an MCU
    --fixed q(16, 12)                      # fixed-point codegen (Q4.12)
    --emit c                               # or portable C for a vendor toolchain
```

The MCU path reintroduces **fixed-point types** as a deployment concern: a model
developed in `float64` is retargeted to `fixed(w, f)` with the numerical error
reported (§21 honesty), so an equivalent-circuit estimator or a filter designed on
the desktop runs on the device that measures the battery. This closes the loop for
the embedded domain without a separate language.

### 46.5 Interactive and literate workflow

```text
qu                       # REPL with variable inspection, %time, %explain (§29)
qu notebook file.qunb    # cells, inline plots and animations, saved outputs
#! literate              # a literate .qu: prose + code + rendered figures -> report
```

`parallel` runs independent work across cores without the GPU ceremony of `each`:

```qu
results = parallel for snr in SNR_LEVELS
    monte_carlo(snr, runs = 30)
end parallel
```

Interactive inspection, notebooks, and literate documents make the exploratory loop
first-class, and a literate `.qu` exports straight to a report (§26) — the analysis
and its write-up are one artifact.

### 46.6 Principle

Distilled to one line: **Qu should be the only language a scientist needs for a
result — from the first interactive plot to a reproducible paper figure to a
fixed-point binary on the measuring device — and it should be open, fast, and honest
about what it did.** These wishes are commitments, tracked like the rest of the
specification.

---

## 47. Runtime and Systems: Timing, Concurrency, Fast I/O, Collections, SIMD, Sketches, and GUI

Real instruments do more than evaluate equations: they acquire on a clock, stream
through buffers, spread work across threads, and present controls. This chapter adds
the runtime and systems layer — kept intuitive and consistent with the rest of the
language, and running on **one cooperative scheduler** shared with the array
dataflow planner (§41.3), so `explain` reports scheduling, threads, and copies just
as it does for kernels (§47.9).

### 47.1 Timers and periodic execution

`every` runs a body on a fixed period; `after` runs it once, later. Both return a
timer handle that can be stopped. The period is drift-corrected, so `every 1 ms`
stays on a 1 ms grid rather than accumulating error.

```qu
acq = every 1 ms do
    rb.push(read_adc())          # sample on a real-time clock
end

blink = every 0.5 s do led = not led end
after 10 s do stop acq end       # one-shot: stop acquisition after 10 s
```

`every N do … end`, `after N do … end`, and `at t do … end` are the three forms;
`stop handle` cancels. Timers are scheduled cooperatively and are deterministic in
headless runs.

### 47.2 Concurrency and multithreading

`spawn` starts a task and returns a future; `await` collects it. `parallel for`
(§46.5) runs independent iterations across an auto-sized thread pool.

```qu
t1 = spawn fft(x1)
t2 = spawn fft(x2)
X1, X2 = await t1, await t2
results = await all(map(spawn_fft, batches))
```

Tasks share immutable arrays freely (no data race possible); mutable sharing goes
through channels (§47.3) or declared atomics. `async`/`await` mark suspendable work
(I/O, timers) so many operations overlap without threads.

### 47.3 Channels and the instruction queue

A `channel` connects producers and consumers; a `queue` of **deferred instructions**
(thunks) is executed by a worker pool — a job/instruction queue. This is the same
scheduler that fuses `:=` dataflow graphs (§11), exposed directly.

```qu
ch = channel(capacity = 1024)
spawn producer(ch)               # ch.send(x)
spawn consumer(ch)               # for x in ch: process(x)

jobs = queue()
jobs.push(() -> process(chunk_a))    # queue instructions (thunks)
jobs.push(() -> process(chunk_b))
run jobs on pool(8)                  # execute the instruction queue across 8 workers
```

`run … on pool(n)` executes queued instructions in order or by `priority=`; a
`pipeline` (§37.4) is the staged form for producer→transform→consumer flows.

**Resource-aware pools.** A pool declares the *resources* available and a dispatch
policy, so a queue of heterogeneous tasks is placed across devices with a capacity
cap per resource — e.g. three CPU workers and one GPU:

```qu
pool workers with cpu = 3, gpu = 1          # 3 CPU slots, 1 GPU slot
    policy = round_robin                     # round_robin | priority | fair | affinity
    on full = queue                          # queue | reject | spill_to(cpu)
end pool

run jobs on workers                          # tasks placed by resource + policy
job = submit workers: fft(x) on gpu          # request a specific resource
```

A task declares the resource it needs (`on gpu`, `on cpu`, or `any`); the scheduler
admits at most the declared capacity per resource and round-robins (or prioritizes)
the rest, spilling GPU overflow to CPU when `spill_to` is set. `workers.status`
reports queue depth and per-resource utilization, and `explain` shows the placement
(§47.9). This is the one scheduler (§47.9) given explicit resource limits.

### 47.4 Collections: list, queue, stack, deque, ring

First-class container types complement arrays (which stay numeric and homogeneous):

```text
list()        dynamic ordered sequence        push, pop, insert, len
queue()       FIFO                            enqueue, dequeue, peek
stack()       LIFO                            push, pop, peek
deque()       double-ended                    push_front/back, pop_front/back
ring(n)       fixed circular buffer           push (overwrites oldest), window
set()         unique membership               add, has, union, intersect
dict()        key -> value map                get, set, keys, values
```

A bracket literal is an **array** while every element is numeric, and a **list**
as soon as one is not: `[1, 2, 3]` is a vector, `["a", "b"]` and `["sine", 440,
true]` are lists. (`[true, false]` stays numeric — booleans are numbers here.)
Only the row form `[a, b]` and the column form `[a; b]` have a list meaning; a 2-D
grid of non-numeric cells is an error, since there is no mixed-matrix value. Nest
with `[[a, b], [c, d]]` — one row whose cells are themselves lists.

`ring` is the streaming-DSP workhorse — a fixed live window over an incoming signal:

```qu
rb = ring(1024)
every 1 ms do rb.push(read_adc()) end
X = fft(rb.window as signal(Fs = 1 kHz))    # spectrum of the latest 1024 samples
```

### 47.5 Fast and advanced I/O — memory-mapped, streaming, intuitive

Large measurement files are handled without loading them. **Memory-mapped** arrays
are zero-copy and out-of-core; writes go through to disk. Streaming iterators walk
files bigger than RAM in chunks.

```qu
big = mmap("scope_2022.bin", shape=(N,), dtype=int16)   # 40 GB file, nothing loaded
peak = 0
for chunk in stream(big, size = 1e6)                    # chunked, out-of-core
    peak = max(peak, max(abs(chunk)))
end for

x = open_signal("meas.wav")      # intuitive: type + Fs inferred (§41.2)
save("out.npz", results)         # format from extension; async under the hood
```

The high-level `load`/`save`/`open_signal` layer is intuitive; underneath, I/O is
zero-copy and asynchronous where the platform allows, and `explain` reports any copy
or format conversion (§22 honesty). Async I/O composes with `await` (§47.2).

### 47.6 SIMD and explicit vectorization

Automatic fusion and vectorization are the default (§41.3). `simd` gives explicit,
guaranteed control for hot kernels — a data-parallel loop with no cross-lane
dependencies, mapped to CPU SIMD (AVX/NEON) or GPU lanes.

```qu
simd for i in 0..N-1             # vectorized across lanes
    y[i] = a * x[i] + b
end

simd(lanes = 8, align = 32)      # optional width/alignment hints
    z = fma(a, x, b)
end simd
```

`simd` is a promise by the author that iterations are independent; the compiler
verifies and, if it cannot, raises a diagnostic rather than silently serializing.

Vectorization comes at three levels, from automatic to explicit: (1) **auto** — every
array/`:=` expression is vectorized and fused by default (§41.3), so `y = a*x + b`
already runs on SIMD lanes with no annotation; (2) **`vectorize`** — a hint on a
loop the compiler could not prove independent, asserting it is
(`vectorize for i in …`), equivalent to `simd` without width control; (3) **`simd`** —
full control of lane width and alignment. Together with `compile` (§41.3) and the
device backends (§20), this is how a hot numerical kernel reaches peak throughput.

### 47.7 Processing-style sketches

Borrowing the Processing creative-coding model, a `sketch` has `setup` (run once) and
`draw` (run every frame), plus input handlers — an intuitive way to build real-time
visualizations and instrument front-panels on top of the animation subsystem (§39).

```qu
sketch scope
    on setup
        size(900, 400)
        rb = ring(1024)
    end
    on draw                       # called every frame
        rb.push(read_adc())
        background("surface")
        plot(rb.window as signal(Fs = 1 kHz))
    end
    on key k
        if k == "space" then pause end
    end
end sketch

run sketch scope
```

A `sketch` is the imperative sibling of the declarative `scene`/`animate` (§39);
both drive the same renderer and export paths, and both run headless.

### 47.8 GUI and controls

A `window` lays out panels and controls declaratively, with event handlers inline.
Controls extend the `control` set of §39.5 (`slider`, `toggle`, `dropdown`, `field`)
with `button`, `knob`, `meter`, and live plot widgets.

```qu
window "EIS Analyzer"
    row
        slider fmin = (0.01, 1000, default = 1, label = "f min (Hz)")
        slider fmax = (1, 1e5, default = 1000, label = "f max (Hz)")
        button "Measure" on click do
            last = run_sweep(fmin, fmax)
        end
    end
    plot nyquist of last          # a plot is a live widget
    meter soc label = "SoC"
end window
```

Layout is `row`/`col`/`grid`; a changed control re-runs its bound expression, as in
§39.5. In a headless run the GUI degrades to each control's default, so the same
script produces a figure on a server and an interactive panel on a desktop.

### 47.9 One scheduler, one honesty contract

Timers, tasks, channels, the instruction queue, sketch draw loops, and the array
dataflow planner all run on a single cooperative scheduler — not several runtimes
bolted together. `explain` extends to the runtime: it reports which work ran on
which thread, timer periods and drift, channel back-pressure, memory-map residency,
and any copy inserted at an I/O or device boundary. The systems layer obeys the same
"honest about what it did" pillar (§3.7) as the numerical core.

---

---

## 48. Impedance Spectroscopy and Equivalent-Circuit Modeling

Electrochemical impedance spectroscopy is a flagship domain for Qu. An impedance is a
complex signal `Z(f)` in `Ohm` on a frequency axis — a `spectrum` of `complex` values
with units (§41.2, §42.2) — so the sampled-signal machinery, units, plotting, and
model fitting all apply directly. All examples use the canonical ASCII surface
(§42.6): `Ohm`, `mOhm`, `Hz`, `tau`, `pi`.

> **This is a standard library, not core grammar.** EIS ships as the bundled `eis`
> library — `import eis` (or `import eis.*` to use the verbs unqualified). `circuit`,
> `fit ... to`, `nyquist`, `drt`, and the element constructors are library functions
> built on the core (autodiff §38, complex arrays, `fit` §45.2), exactly as `dsp`,
> `stats`, and `ml` are (§28, §52.4). It is documented here as a full chapter because
> it is the reference domain, but nothing in it is baked into the language.

### 48.1 The impedance datum

```qu
data = read_eis("cell.dta")        # autodetects Gamry/Biologic/Zahner/ZView/CSV
data.f                             # frequency axis, Hz
data.Z                             # complex impedance, Ohm  (Hermitian metadata carried)
```

`read_eis` returns a record with `f` and complex `Z`; a frame column of impedance
carries its `Ohm` unit so `Z = V ./ I` is dimensionally checked (§42.2).

### 48.2 Circuit elements

The primitives are the standard circuit elements; each is a one-line parameterized
element with named parameters and units:

| Element | Symbol | Impedance | Parameters |
|---|---|---|---|
| Resistor | `R` | `Z = R` | `R` [Ohm] |
| Capacitor | `C` | `Z = 1/(j*w*C)` | `C` [F] |
| Inductor | `L` | `Z = j*w*L` | `L` [H] |
| Constant-phase | `Q` (CPE) | `Z = 1/(Q*(j*w)^n)` | `Q`, `n` in (0,1] |
| Warburg (semi-inf) | `W` | `Z = Aw/sqrt(j*w)` | `Aw` |
| Warburg finite | `Ws`/`Wo` | `Z = R*tanh|coth(sqrt(j*w*tau))/sqrt(...)` | `R`, `tau` |
| Gerischer | `G` | `Z = Z0/sqrt(k + j*w)` | `Z0`, `k` |
| Transmission line | `T` | porous-electrode line | element list |

where `w = 2*pi*f`.

### 48.3 Building the circuit as a network

A circuit is a **network you wire up**: elements compose in **series** with `+` and
in **parallel** with `|`. This is the same "build a network" idea as the ML model
builder (§45.1), applied to circuits:

```qu
# Randles cell:  series Rs, then Rct in parallel with the double-layer Cdl
ecm = R("Rs") + (R("Rct") | C("Cdl"))

# Randles with diffusion:  Warburg in series with Rct, that branch || Cdl
ecm = R("Rs") + ((R("Rct") + W("Zw")) | C("Cdl"))

# two depressed arcs (batteries): Rs then two R||Q sections
ecm = R("R0") + (R("R1") | Q("Q1")) + (R("R2") | Q("Q2"))
```

Equivalently, the standard **circuit-description code** (CDC) string — `-` is series,
`p(...)` is parallel (Boukamp notation, as used by ZView and impedance.py):

```qu
ecm = circuit("R0-p(R1,Q1)-p(R2,Q2)")      # same network as above
ecm.Z(f)                                    # evaluate over a frequency vector
ecm.params                                  # named parameters, with units and bounds
```

### 48.4 Native ECM fitting

`fit ecm to data` runs complex nonlinear least squares — real and imaginary parts
fit jointly — with gradients from autodiff through the circuit equation (§38), so no
hand-derivatives:

```qu
fit = fit ecm to data
    start  Rs = 30 mOhm, Rct = 15 mOhm, Cdl = 2 mF
    bounds Rs in [0, 1] Ohm, all tau > 0
    weight = modulus                 # unit | proportional(1/|Z|^2) | modulus | sigma
    method = levenberg_marquardt     # or trust_region, global(basinhopping)
end fit

fit.params        # each parameter: estimate, stderr, 95% CI, in its unit
fit.chi2          # goodness of fit; fit.residuals, fit.aic, fit.bic
print("Rct = {fit.Rct in mOhm} +/- {fit.Rct.stderr in mOhm}")
```

Weighting follows Boukamp's schemes (`unit`, `proportional`, `modulus`, or explicit
`sigma`); parameter uncertainties come from the covariance matrix, with a bootstrap
option for skewed landscapes.

### 48.5 Parallel batch fitting

Real studies fit *many* spectra — an aging series, a state-of-charge or temperature
sweep, a spatial map. These fit **in parallel** (§47.2), optionally warm-started
from the neighbor for speed and continuity:

```qu
spectra = read_eis_folder("aging/*.dta")    # list of datasets
fits = parallel for s in spectra
    fit ecm to s start from previous         # warm-start chain
end parallel

plot(soc, [f.Rct in mOhm for f in fits])     # track Rct across the sweep
```

### 48.6 Model-free validation and analysis

Before trusting a circuit, validate the data and see how many processes it contains.

**Kramers-Kronig** consistency (linear KK / lin-KK, Schoenleber et al.) checks
causality, linearity, and stationarity:

```qu
kk = validate_kk(data)              # lin-KK; residuals within +/-1% => data is valid
plot(kk.f, kk.residual_re, kk.residual_im)
```

**Distribution of relaxation times** (DRT) deconvolves the spectrum into a
continuous `g(tau)` by Tikhonov regularization, so the peaks reveal how many RC
processes are present — a model-free guide to choosing the circuit:

```qu
gamma = drt(data, method="tikhonov", lambda="lcurve")   # auto regularization (L-curve)
plot(gamma.tau, gamma.g)            # each peak ~ one relaxation process
```

### 48.7 Plotting

```qu
nyquist(data, fit.Z)                # -Im(Z) vs Re(Z), equal aspect; data + fitted curve
bode(data, fit.Z)                   # |Z| and phase vs log f
residuals(fit)                      # relative residual vs f
```

`nyquist` enforces an equal aspect ratio and `Ohm` axes; `bode` draws the two-panel
magnitude/phase view. Both overlay measured data and the fitted model.

### 48.8 Standard circuit library

Ready-made circuits: `randles()`, `randles_cpe()`, `rc_ladder(n)`,
`transmission_line()` (porous electrode / battery), `warburg_short()`,
`warburg_open()`. Each returns an `ecm` ready to `fit`.

### 48.9 Grounding in the reference workload

The two-RC battery model is exactly `R("R0") + (R("R1") | C("C1")) +
(R("R2") | C("C2"))`, and its estimation step is `fit ecm to (f_meas, Z_meas)`. With
units, the excitation is `[A]`, the response is `[V]`, and `Z = V ./ I` is verified
as `[Ohm]` — the whole EIS pipeline, from measured spectrum to validated,
uncertainty-quantified circuit parameters, is native.

> **References.** Circuit-description code: Boukamp, *Solid State Ionics* (1986) and
> the `impedance.py` conventions. lin-KK validation: Schoenleber, Klotz & Ivers-Tiffee,
> *Electrochim. Acta* (2014). DRT by Tikhonov regularization with L-curve selection:
> Wan et al. and Schlueter et al.

---

## 49. Further Inspirations: Scheduling, Stencils, Distribution, and Composition

Beyond Qu's named ancestors, four ideas from modern array and HPC languages earn a
place. Each is adopted as an affordance layered on the same core — none changes what
a computation *means*, only how it is expressed or executed — and each is attributed.

### 49.1 Algorithm / schedule separation (Halide)

Borrowing Halide's central idea, *what* is computed is written separately from *how*
it runs. A computation is an algorithm; a `schedule` block annotates it with tiling,
vectorization, parallelization, and fusion without touching the math:

```qu
y := blur(x)                        # the algorithm — WHAT (a pure := definition)

schedule y                          # the schedule — HOW (does not change the result)
    tile(64, 64)
    vectorize(8)
    parallel
    fuse with x
end schedule
```

This complements the automatic fusion of §41.3: the default needs no schedule, but a
hot kernel can be hand-tuned for a target without rewriting the equation, and
`explain` shows the realized schedule.

### 49.2 Stencils, nested parallelism, safe in-place (Futhark)

A `stencil` combinator expresses neighborhood computations — FIR filters, finite
differences (§43), convolution, PDE updates — as a pure, parallel map over a window
with declared boundary handling:

```qu
y = stencil(x, offsets=[-1, 0, 1], (a, b, c) -> (a + 2*b + c)/4, edge="reflect")
```

Following Futhark, nested `parallel` (a parallel map inside a parallel map) is
flattened by the compiler rather than forbidden, and in-place array updates that the
compiler proves disjoint are allowed inside a parallel region (`x[i] <- v`) — safe
mutation without losing parallelism.

### 49.3 Distributed domains (Chapel)

For data bigger than one machine, Chapel's global-view model: an array is placed on a
distributed **domain** across locales/nodes, and ordinary `parallel for` (or
`forall`) over that domain runs across the cluster with one global view of the data —
the same source scales from a laptop to a cluster.

```qu
D = distributed(shape=(N,), over=nodes)
big on D = load_chunks("scope_*.bin")
parallel for i in D
    y[i] = process(big[i])          # runs across the cluster; global-view indexing
end parallel
```

This is the out-of-core/HPC counterpart to the single-node memory map of §47.5.

### 49.4 Tacit composition (APL / J — "notation as a tool of thought")

In the spirit of Iverson's *Notation as a Tool of Thought*, Qu supports point-free
composition so a pipeline of transforms can be named without naming the argument:

```qu
clean  = compose(normalize, detrend, bandpass)   # compose = f . g . h
rms_db = compose(db, rms)                         # point-free
y = clean(x)

y = x |> bandpass |> detrend |> normalize         # or the explicit pipe (same result)
```

`compose` (aliases `.` in an ASCII-safe position and the Unicode `∘`) joins pure
functions; combined with the `map`/`reduce`/`scan` combinators (§44.5) it gives the
array-language conciseness that inspired the whole notation surface (§42), while the
pipe `|>` keeps the reading order explicit for those who prefer it.

### 49.5 One core, honest throughout

These are expressiveness and performance affordances on the same typed, fusible-array
core. A `schedule` changes speed, not results; a `stencil` is a pure map; a
distributed domain is a placement; `compose` is function algebra. `explain` reports
the realized schedule, the stencil's fusion, the distribution and any cross-node
transfer, and the composed call chain — the honesty pillar (§3.7) extends to every
one of them.

---

## 50. Operating System, Files, and Communication

Instruments live in the world: they read and write files, talk over serial ports and
sockets, and react to the operating system. Qu exposes all of it intuitively, on the
same cooperative scheduler and `async`/`await` model as the rest of the runtime
(§47), with everything ASCII-canonical.

### 50.1 File I/O — binary and text

The high-level `load`/`save`/`open_signal` (§47.5) stay the intuitive default; this
is the explicit layer beneath them.

```qu
with open("meas.bin", "rb") as f            # modes: r/w/a + b(inary)/t(ext)
    hdr = read_array(f, dtype=int16, n=256)  # typed binary read
    seek(f, 4096) ; tell(f)
end                                          # closed automatically

with open("log.txt", "w") as f
    write(f, "started\n")
    for line in lines(source): write(f, line)
end
```

Binary helpers: `read_bytes`/`write_bytes`, typed `read_array`/`write_array`, and
`pack`/`unpack` (struct-style records). Text: line iteration, encoding, `readline`.

### 50.2 Watching, tailing, and filtering text

File watching and the classic stream utilities are first-class and compose through
the `|>` pipe (§41.4):

```qu
on watch("data/*.csv") do change
    reload(change.path)                      # fires when a file changes
end

for line in tail("run.log", follow=true) |> grep("ERROR")  # left-to-right stream pipe
    alert(line)
end

first20 = head("big.csv", 20)                # head; tail("f", 20) for the end
```

`tail(path, follow=true)` streams new lines as they arrive; `grep(pattern, source)`
yields matches; `head`/`tail` bound the ends. They are stream sources, so they slot
into pipelines and `for` loops.

### 50.3 Serial ports

```qu
port = serial("COM3", baud=115200, parity="none", timeout=1 s)
write(port, "MEAS?\n")
for line in port                             # async line stream from the device
    sample = parse(line)
    rb.push(sample)
end
```

Serial is the instrument/MCU link — byte or line oriented, async, with timeouts.

### 50.4 Sockets

```qu
sock = tcp_connect("192.168.0.10", 5025)     # e.g. a SCPI instrument
send(sock, "*IDN?\n") ; id = recv_line(sock)

server = tcp_listen(9000)
on connect(server) do client
    for msg in client: handle(msg)
end

u = udp(bind=5000)                            # datagrams
```

TCP and UDP clients and servers, async, iterable; WebSocket is an optional add-on.

### 50.5 POSIX / OS interface

The `os` namespace exposes a portable subset with POSIX semantics named where they
matter:

```qu
os.env["QU_BACKEND"]                          # environment
os.args                                        # command-line arguments
res = os.run("ffmpeg", ["-i", "in.mp4", "out.gif"])   # spawn a process, capture stdout
os.glob("meas/*.dta") ; os.mkdir(p) ; os.exists(p)    # filesystem
on signal("SIGINT") do stop_all() end          # signals
```

Process spawning, pipes, signals, environment, working directory, and filesystem
operations; cross-platform, with the POSIX behavior documented where platforms differ.

### 50.6 Events: `on EVENT do … end`

Every asynchronous source — a flag, a changed variable, a timer, a signal, a socket
message, a file change, a GUI control, a key — is handled by one reactive form. This
unifies §39.5 controls, §47.1 timers, and the sources above.

```qu
ready = flag()                                # a settable condition
on ready do
    process()                                 # runs when `set ready` is called
end
set ready

on change(temperature) do t                   # fires when the value changes
    if t > 60 C then warn("overtemp") end
end

on key "space" do pause end
every 1 s do tick() end                        # timer form (§47.1)
```

`on EVENT do … end` registers a handler on the cooperative scheduler (§47.9);
handlers are ordinary code and may themselves `await`. `flag()` plus `set`/`clear`
gives a simple named condition to trigger and wait on.

---

## 51. 3D Graphics, Meshes, and Scenes

Plotting (§27) covers `surf`/`mesh` for height fields; this chapter is full 3D:
building a mesh from arrays, texturing it, and viewing it interactively, integrated
with the animation subsystem (§39) and sketches (§47.7).

### 51.1 Building a mesh from arrays

A mesh is just arrays: vertices (an `n x 3` array of positions) and faces (an
`m x 3` integer array of vertex indices), plus optional per-vertex normals, colors,
and texture coordinates.

```qu
V as matrix(n, 3)                              # vertex positions
F as matrix(m, 3, integer)                     # triangle indices
m = mesh(V, F)
m.uv = UV                                       # texture coordinates (n x 2)
m.texture = img                                 # an image array
view(m)                                         # open an interactive 3D viewer (orbit/zoom)
```

From a grid, a height field becomes a surface mesh directly:

```qu
X, Y = meshgrid(-3 to 3 step 0.1, -3 to 3 step 0.1)
Z = sin(sqrt(X^2 + Y^2))
m = surface_mesh(X, Y, Z, color=Z)              # color by height
view(m)
```

Procedural primitives — `box()`, `sphere()`, `cylinder()`, `torus()`,
`grid_mesh()` — return meshes to compose or transform (`translate`, `rotate`,
`scale`).

Premade meshes and imported meshes do not introduce a privileged object format:
`m.vertices`, `m.faces`, `m.normals`, `m.uv`, and `m.colors` remain ordinary typed
arrays. Editing those arrays marks derived normals, acceleration structures, and
collision artifacts dirty; `validate(m)` reports non-finite positions,
out-of-range indices, repeated-index triangles, inconsistent attribute lengths, and
non-manifold edges without silently repairing them.

### 51.2 Scenes, cameras, lights

```qu
scene3d lab
    add m
    add sphere(r=0.5) |> translate(2, 0, 0)
    light(pos=(5, 5, 5))
    camera(pos=(6, 4, 6), look_at=(0, 0, 0), up=z)
    animate a from 0 to 2*pi fps 30
        camera.orbit(a)                         # spin the camera
    end animate
    export = "mesh.mp4" fps=30
end scene3d
render scene3d lab
```

`scene3d` reuses the `animate`/`timeline` machinery (§39) and the same export paths,
adding `mp4`/`webm`/`gif` for motion and `png` for a snapshot.

### 51.3 3D plotting and fields

```text
surf, mesh, scatter3, line3, quiver3        # surfaces, points, vectors
isosurface(vol, level), volume(vol)         # volumetric data
streamlines(vx, vy, vz), slice(vol, plane)  # flow and cut planes
```

### 51.4 Import / export

Mesh and scene interchange: `read_mesh`/`write_mesh` for `obj`, `ply`, `stl`, and
`gltf`; a mesh or scene also renders inline in a notebook (§46.5) or a `window`
(§47.8).

---

## 52. Simulation and Physics

Qu ships numerical **solvers** and a **simulation** framework in the core standard
library, and larger domain simulators — SPICE, FEM, and the EIS library of §48 — as
**includable libraries** you `import` (§52.4). All of it composes with arrays,
units, plotting, and the runtime.

### 52.1 ODE / DAE solvers and time-stepping

```qu
# dy/dt = f(t, y);  a damped oscillator
sol = solve_ode((t, y) -> [y[1], -k*y[0] - c*y[1]], y0=[1, 0], tspan=(0, 20),
                method="rk45", events=[zero_cross(y[0])])
plot(sol.t, sol.y[0, :])
```

Explicit and stiff solvers (`rk45`, `dop853`, `bdf`, `radau`), root/event handling,
and DAEs. For discrete-time or agent systems, a `simulate` block steps state on a
clock (reusing §47.1 timing):

```qu
simulate world over 0 to 10 s step dt
    state x, v
    on step do
        v += (force(x) / m) * dt               # semi-implicit Euler
        x += v * dt
        record(x, v)
    end
end simulate
```

### 52.2 PDEs and FEM

Finite-element and finite-difference PDE solving builds on the mesh of §51. FEM is
the `fem` library:

```qu
import fem

mesh = fem.mesh_from(geometry, h=0.05)         # or a §51 mesh
u = fem.solve(
        fem.laplacian(u) == source,            # weak form / operator equation
        on = mesh,
        bc = fem.dirichlet(boundary, 0))
view(u on mesh)                                 # field over the mesh (§51 viewer)
```

Prebuilt problems (`fem.poisson`, `fem.heat`, `fem.elasticity`, `fem.helmholtz`) and
a general `assemble`/`solve` path; also particle/N-body and rigid/soft-body `physics`
worlds with bodies, forces, and constraints, stepped like §52.1. This is the
"FEM-alike physics simulation" layer, native to the array/solver core.

### 52.3 SPICE and EIS as includable libraries

The canonical authoring form keeps the column structure familiar to SPICE users
while adding Qu assignment, physical units, and explicit block closure:

```qu
import spice

# vin -- R0 -- out -- (R1 || C1) -- 0
circuit rc_filter
    Vsrc vin 0   = sine(offset=0 V, amplitude=1 V, frequency=50 Hz)
    R0   vin out = 100 Ohm
    R1   out 0   = 1 kOhm
    C1   out 0   = 1 uF
end circuit

# The simple path combines preparation and execution.
run = await spice.transient(rc_filter, stop=1 s, step=10 us)
plot(run.t, run.voltage("out"))

# Compile once only when the circuit will be reused or sent to another engine.
spice1 = spice.compile(rc_filter, engine=spice.ngspice)
run = await spice1.transient(stop=1 s, step=10 us)
plot(run.t, run.voltage(across="R1"))
```

Each device line is `component node... = value_or_model`. The component prefix
follows SPICE convention (`R`, `C`, `L`, `V`, `I`, `D`, `M`, `Q`, `X`, ...), and
diagnostics print the inferred device kind. Node `0` is ground. The right side is
an ordinary Qu expression, so units and source/model constructors are checked
before a simulator runs. Two-terminal devices name two nodes; controlled sources
and subcircuits may name more.

The diagram-like proposal `V--R0--R1||C1--GND` remains useful as a comment, not as
canonical executable syntax. It hides the junction after `R0`, component values,
polarity, and whether `V(R1)` means node voltage or voltage across the device; it
also cannot express arbitrary circuit graphs. Explicit node columns solve those
problems while remaining immediately recognizable to a SPICE user.

`spice.transient` returns an awaitable analysis, so `await` replaces a separate
`wait(runner.run())`. The long names `transient`, `ac_sweep`, `dc_sweep`, and
`noise` are canonical; SPICE aliases `tran`, `ac`, and `dc` are accepted. Probe
meaning is explicit: `run.voltage("out")` is node-to-ground voltage;
`run.voltage("out", "vin")` states polarity; and
`run.voltage(across="R1")` uses the component's declared node order.

`spice.compile` is a library call, not another language-level compilation
statement. It validates the circuit, resolves models, emits a content-addressed
netlist, and returns a reusable runner whose engine, model, and netlist hashes are
recorded in result provenance.

#### Browser testbench and online-SPICE bridge

The Qu frontend may host a visual circuit bench in the browser. Its default
numerical engine is a pinned ngspice WebAssembly build running locally in a worker,
not an unversioned public web service:

```qu
bench = spice.bench(rc_filter,
                    engine=spice.ngspice_wasm,
                    ui=spice.web)

run = await bench.transient(stop=1 s, step=10 us)
bench.show(run, probes=[voltage("out"), current("Vsrc")])

# Reference parity check used by CI and release qualification.
reference = await spice.ngspice_native.transient(rc_filter,
                                                  stop=1 s,
                                                  step=10 us)
assert_close(run.voltage("out"), reference.voltage("out"),
             rtol=1e-7, atol=1 uV)
```

The browser bench owns schematic editing, animated current/voltage overlays,
scopes, controls, probe placement, and result inspection. The engine owns netlist
validation, device models, convergence, and numeric output. They communicate only
through the typed circuit/netlist and analysis-result protocols, so the UI cannot
silently alter the circuit being tested.

Three engine modes implement the same protocol:

| Engine | Intended use | Trust/reproducibility rule |
|---|---|---|
| `spice.ngspice_wasm` | instant browser work, tutorials, ordinary tests | pinned WASM and model hashes; no network required |
| `spice.ngspice_native` | reference results, compact models, CI qualification | pinned native/shared-library build and sandbox |
| `spice.remote(worker)` | large sweeps and licensed/private model farms | authenticated worker, explicit capabilities, same artifact record |

CircuitJS1-compatible import/export is an optional visualization bridge:
`spice.circuitjs.export(rc_filter)` and `spice.circuitjs.import(file)`. CircuitJS1
is designed to run and embed in a browser and supports circuit loading through
URL parameters, making it useful for interactive previews:
<https://github.com/sharpie7/circuitjs1>. It is not the reference oracle unless a
test explicitly selects and pins that engine.

ngspice remains the reference because its maintained documentation exposes batch
and shared-library integration, including callbacks and vector access:
<https://ngspice.sourceforge.io/docs.html>. The ngspice project also lists a
browser/WebAssembly simulator that processes netlists locally, demonstrating that
an online-style bench need not upload circuits or results:
<https://ngspice.sourceforge.io/resources.html>.

Every browser run records the emitted netlist hash, engine name and build hash,
model hashes, options, seed, tolerances, browser/worker limits, and raw vectors.
CI compares representative browser-WASM results with native ngspice. Public hosted
benches may be opened for sharing or visual inspection, but network availability,
site updates, or account state can never decide whether an offline Qu test passes.

Raw SPICE text remains the interoperability escape hatch:

Circuit simulation is the `spice` library — a netlist simulator with the standard
analyses, whose results are ordinary Qu signals and spectra, so they interoperate
with DSP (§24) and the EIS library (§48):

```qu
import spice
ckt = spice.netlist("""
    R1 in out 1k
    C1 out 0 1u
    V1 in 0 AC 1
""")
H = await spice.ac_sweep(ckt, f=logspace(0, 6, 200))
tr = await spice.transient(ckt, stop=5 ms, step=1 us)

import eis                                       # the impedance chapter (§48)
compare(H, eis.fit(model, measured))            # SPICE model vs measured EIS fit
```

Transient, AC, DC, and noise analyses are provided; an AC sweep yields a model
impedance directly comparable to a measured spectrum. Their results are ordinary
Qu signals and spectra and therefore compose with DSP (§24), EIS (§48), and tests
(§59).

### 52.4 The library model — core vs. bundled vs. extra

Qu's **core** is the array/notation/runtime language. Everything domain-specific is a
library imported by name:

```text
bundled (ship with Qu):   dsp, stats, ml, eis, viz3d, io, os
extra (installable, §46):  spice, fem, physics, and third-party packages
```

`import name` (or `import name.*` for unqualified use) loads a library; the package
manager and lockfile (§46.3) pin versions of extra libraries for reproducibility.
This keeps the language small and the domains — impedance, SPICE, FEM — modular and
independently versioned, exactly as §28 (extension system) intends.

---

## 53. Objects, Classes, and Structured Control

Qu's core is arrays, functions, and multiple dispatch (§41.3), not objects. But
stateful components with behavior — an instrument driver, a filter that carries
state, a GUI widget, a simulation body — are clearest as **classes**, modeled on
VB.NET. Dispatch still applies to their methods, so classes coexist with the generic
functions of §41.3.

### 53.1 Defining a class

```qu
class Filter
    dim order as integer = 4              # field, with a default
    dim state as vector

    new(order)                             # constructor (VB.NET 'New')
        me.order = order                   # 'me' is the instance (VB.NET 'Me')
        me.state = zeros(order)
    end new

    function apply(x) -> y                  # method (named return value)
        y = process(x, me.state)
    end
end class

f = new Filter(order = 2)                   # instantiate
y = f.apply(sample)
```

### 53.2 Properties with `get`/`set`

```qu
class Channel
    dim _gain as float = 1.0

    property gain                           # computed / validated property
        get: return me._gain
        set(v): me._gain = clamp(v, 0, 10)  # validation runs on assignment
    end property
end class

c = new Channel() ; c.gain = 20             # clamped to 10 by the setter
```

### 53.3 Inheritance and polymorphism

```qu
class Butterworth inherits Filter
    dim cutoff as float [Hz]

    new(order, cutoff)
        base.new(order)                     # call the parent constructor
        me.cutoff = cutoff
    end new

    override function apply(x) -> y         # override a parent method
        y = butter_step(x, me.state, me.cutoff)
    end
end class
```

Single inheritance with `inherits`, `base.` for the superclass, and `override`.
For behavior shared across *unrelated* types, prefer multiple dispatch (§41.3) or an
`interface`; inheritance is for genuine is-a hierarchies.

### 53.4 Shared members and interfaces

```qu
class Counter
    shared total = 0                        # 'Shared' (static): one per class
    new() ; Counter.total += 1 ; end new
end class

interface Fittable                          # a contract; models (§37.2) implement it
    function fit(x, y)
    function predict(x)
end interface

class Ridge implements Fittable
    ...
end class
```

### 53.5 Value vs reference

Classes are **reference types** (like VB.NET classes). For plain value-type data,
use a `record` (§8) — it copies by value and has no methods. The guidance: numeric
work stays in arrays, records, and functions; reach for a class when a thing has
*mutable state plus behavior*.

### 53.6 Structured control and the question of `goto`

Qu has **no unstructured `goto`** in the base language (a non-goal since §4).
FORTRAN's `GO TO`, BASIC's `GOTO`, and `ON ERROR GOTO` are replaced by structured
constructs that a reader — and the fusion/scheduling analysis of §41.3 — can follow:

- **Labeled `break`/`continue`** for multi-level loop control:

```qu
for outer i in 0 .. N-1
    for j in 0 .. M-1
        if converged(i, j) then break outer end     # exit both loops
        if skip(i, j)      then continue outer end
    end for
end for
```

- `select case` (§44.2) for multi-way dispatch, `try/catch` (§18.B) for errors, and
  a **state machine** as `select case` on a `state` variable inside a `repeat`
  (§44.2) — the structured replacement for `GOTO`-driven state code.

A single forward-only `goto label` within one function exists **only** in the BASIC
compatibility mode (`module (compat="basic")`), never in the base language, and even
there diagnostics discourage it. The rationale is concrete: arbitrary `goto` defeats
the dataflow analysis that fusion, scheduling, and `explain` (§41.3, §47.9) depend
on, so the base language keeps control flow structured.

---

## 54. Distributed Execution: Networked Qu Instances and Allocation

A lab has more than one machine — a GPU box, several desktops, an acquisition PC.
Qu treats every reachable Qu instance as a **compute resource**: connect to it by IP
address or hostname, add its CPUs and GPUs to a pool, and **allocate** work across
the network. This is the cluster extension of the resource-aware scheduler (§47.3)
and the distributed domains of §49.3, and it composes with everything above —
`backend auto` (§20), `parallel for` (§46.5), and async tasks (§47.2).

### 54.1 Qu instances as resources

Each machine runs a Qu worker daemon; a controller connects to it:

```qu
# on each machine:   qu serve --port 7000 --token <key>

w1 = worker("192.168.0.20:7000")        # by IP address
w2 = worker("gpu-box")                    # by hostname
lab = cluster(["lab-pc-1", "lab-pc-2", "192.168.0.20"])   # a named group
lab.discover()                            # optional LAN auto-discovery
lab.resources                             # advertised CPUs, GPUs, RAM per node
```

A worker advertises its resources, so the pool knows each node's capacity. Local
resources are the implicit `local` node, so the same code runs on one machine or
many.

### 54.2 Allocation — placing work across machines

The `pool` of §47.3 extends across nodes: it lists the resources each machine
contributes and an **allocation policy** that decides where each task runs.

```qu
pool lab with
    node "lab-pc-1"     cpu = 8,  gpu = 1
    node "192.168.0.20" cpu = 16, gpu = 2
    node local          cpu = 4
    policy   = least_loaded          # round_robin | least_loaded | affinity | capacity
    on full  = queue                  # queue | reject | spill_to(local)
    on drop  = reschedule             # a node vanishing re-runs its tasks elsewhere
end pool

run jobs on lab                       # tasks placed by resource need + policy
```

Allocation policies:

| Policy | Places a task on… |
|---|---|
| `round_robin` | the next node in turn (even spread) |
| `least_loaded` | the node with the most free capacity now |
| `affinity` | the node that already holds the task's data (data-local) |
| `capacity` | any node with a free slot, never exceeding its declared caps |

A task declares the resource it needs (`on gpu`, `on cpu`, `any`); GPU tasks are
allocated only to GPU-bearing nodes, and per-node capacity is never exceeded — the
round-robin and cap semantics of §47.3, now across the network. Explicit placement
overrides the policy:

```qu
X = spawn fft(x) on worker("gpu-box")         # this task, this machine
Y = spawn train net on gpu of "192.168.0.20"  # a specific remote GPU
Z = parallel for s in spectra on lab          # data-parallel across the cluster
```

### 54.3 Data movement and locality

Arrays ship to the worker and results return; `explain` reports the bytes
transferred and where each operation ran (the honesty pillar, §22). Send-once,
reuse-many is explicit:

```qu
pin dataset on lab                    # cache on the workers; don't resend per task
X on lab = load("huge.bin")           # a distributed array, one chunk per node (§49.3)
Y = parallel for i in X on affinity   # compute where the data already lives — no reshuffle
```

`pin` keeps a value resident on the cluster; a distributed array places chunks per
node so `affinity` allocation keeps computation next to its data.

### 54.4 Two intents: accelerate, or allocate

The same cluster serves two distinct needs:

- **Accelerate** — transparently offload a heavy operation to a faster or GPU node.
  A remote device is just another backend target (§20): `with backend
  remote("gpu-box")` runs the block there, or `backend auto` may select a remote GPU
  when the local machine lacks one.
- **Allocate** — reserve dedicated capacity for a job or experiment so a long run is
  not contended: `reserve 2 gpu on lab for run` holds the resource for the duration,
  and releases it after. This is batch/experiment scheduling on top of the pool.

```qu
with backend remote("gpu-box"), precision = float32   # accelerate: offload the FFTs
    spectra = parallel for s in raw: process_fft(s) end
end with

reserve 2 gpu, 32 cpu on lab for sweep                 # allocate: hold capacity
result = tune model on data on sweep                    # a big search gets its own nodes
release sweep
```

### 54.5 Reliability and security

Distributed execution runs code across machines, so it is guarded:

1. **Authentication.** A worker accepts work only from controllers presenting its
   token/key; transport is encrypted. A worker never runs code from an
   unauthenticated source.
2. **Fault tolerance.** Heartbeats detect a dropped node; `on drop = reschedule`
   re-runs its tasks elsewhere (tasks should be idempotent or checkpointed, §46.3).
   Timeouts bound a stuck task.
3. **Isolation.** Each job runs in its own sandbox on the worker; resource caps are
   enforced so one controller cannot starve a shared machine.

### 54.6 Grounding in the lab

The EIS batch fit of §48.5, the Monte-Carlo SNR sweep of a battery-impedance workload, and a
model `tune` (§45.6) all become one-line cluster runs — fit hundreds of spectra
across the lab PCs, offload the FFT-heavy simulation to the GPU box while the
acquisition desktop drives the instrument, and reserve the GPU nodes for an
overnight hyperparameter search:

```qu
lab = cluster(["desk-1", "desk-2", "gpu-box"])
fits = parallel for s in read_eis_folder("aging/*.dta") on lab
    fit ecm to s start from previous
end parallel                          # hundreds of ECM fits, allocated across the lab
```

Distributed execution changes *where* work runs, never *what it computes*; a result
carries its provenance (§46.3), including which nodes produced it.

---

## 55. Reliability and Mission-Critical Monitoring

A structural-health monitor on a bridge must run for years, never miss an event, and
fail safe. Qu adds a reliability layer for long-running, mission-critical monitoring —
real-time deadlines, watchdogs, supervisors, redundancy, contracts, alarms, and safe
states — built on the runtime (§47), comms (§50), and reproducibility (§46.3). All of
it is deterministic and auditable, so a monitoring program can be trusted.

### 55.1 Real-time deadlines

A timer (§47.1) can carry a **deadline**; an overrun is an event, not a silent slip:

```qu
every 1 ms deadline 0.8 ms do
    rb.push(read_adc(sensors))
on overrun d
    log_jitter(d)                     # runs if the body missed its 0.8 ms budget
end

with realtime                          # no hidden allocation / GC pause in this block
    control_loop()
end
```

`realtime` pins memory (preallocated buffers, no allocation on the hot path) and uses
a deterministic scheduler with bounded jitter.

### 55.2 Watchdogs and heartbeats

```qu
wd = watchdog(timeout = 2 s) do enter safe_state end
every 100 ms do compute() ; wd.kick() end     # must kick within 2 s or recovery fires
heartbeat to supervisor every 1 s              # liveness signal to a supervisor / remote
```

### 55.3 Supervisors and fault handling

A supervisor restarts failed work (Erlang/OTP-style), so a transient fault does not
take the monitor down:

```qu
supervise acquisition with restart = on_failure, max_restarts = 5 in 60 s

on fault e do
    log(e) ; retry after 1 s          # or: enter safe_state
end
```

`safe_state` is a declared, tested fallback (e.g. hold last valid output, close
valves, raise a flag); `enter safe_state` is always reachable.

### 55.4 Redundancy and voting

```qu
redundant(3) sensor temp                 # triple-modular redundancy
t = vote(temp)                            # majority; disagreement -> on_disagree
on disagree(temp) do flag_sensor_fault() end
```

### 55.5 Design by contract

Preconditions, postconditions, and invariants are first-class; a violation is a
diagnostic (or, in a monitor, a transition to `safe_state`), never silent:

```qu
function integrate(x as signal) -> y
    require x.Fs > 0                      # precondition
    ...
    ensure isfinite(y)                    # postcondition
end function

monitor loop
    invariant buffer.len <= buffer.cap    # checked every iteration
    ...
end monitor
```

### 55.6 Alarms and thresholds

```qu
alarm high when strain > 400 ustrain severity = warning
    hysteresis = 20 ustrain, debounce = 3 s, latching = true
do
    notify(engineers) ; escalate_if(persists > 60 s)
end
```

Alarms have severity, hysteresis, debounce, latching, and escalation; they are
`on EVENT` handlers (§50.6) specialized for thresholds.

### 55.7 Data integrity

Streams are checked for gaps, timestamp monotonicity, saturation, stuck-at values,
and calibration validity; every logged record carries provenance (§46.3). Missing
samples are flagged, never silently interpolated.

### 55.8 The bridge-monitoring example

```qu
monitor bridge
    # fast acquisition into a ring buffer, on a hard 1 ms grid
    every 1 ms deadline 0.9 ms do rb.push(read_adc(accel)) end

    # once a second: modal analysis + anomaly alarm
    every 1 s do
        acc = rb.window as signal(Fs = 1 kHz)
        f0  = modal_frequency(psd(acc))               # first natural frequency
        alarm high when f0 < 2.3 Hz severity = critical do
            notify(engineers) ; log(acc, provenance = true)
        end
    end

    redundant(3) sensor accel                          # voted accelerometers
    watchdog(2 s) do enter safe_state end
    supervise acquisition with restart = on_failure
end monitor
```

A drop in the first natural frequency signals stiffness loss (damage); the monitor
runs for years, votes redundant sensors, restarts transient faults, and fails to a
safe state on power or comms loss — with a full, provenance-stamped audit trail.

### 55.9 Posture

Qu does not claim a safety certification, but it provides the properties that make one
achievable: deterministic real-time execution, no hidden allocation in `realtime`,
explicit and reachable failure modes, contracts, redundancy, and a complete audit log.
`explain` extends to reliability — it reports deadlines, jitter, restarts, and alarm
history.

---

## 56. 3D Modeling, Spatial Audio, and Real-Time Physics

Chapter §51 built and viewed meshes. This chapter turns Qu into an interactive
3D **studio**: solid modeling, positional 3D **audio** (built on the DSP core), and a
real-time **physics** engine — all reusing the array/signal/mesh/solver core, none of
it a separate engine bolted on. It is headless-renderable, so scenes export on a
server as well as animate on a desktop.

### 56.1 Solid modeling

```qu
# primitives + transforms + constructive solid geometry (CSG)
body = csg.difference(box(2, 1, 1), cylinder(r = 0.3, h = 2))   # a block with a bore
lug  = extrude(profile2d, height = 0.5) |> rotate(z, 30 deg)     # 2D profile -> solid
shell = revolve(section, axis = y)                               # lathe a profile
part = csg.union(body, lug) |> smooth(iterations = 2)
```

Primitives (`box`, `sphere`, `cylinder`, `cone`, `torus`, `plane`), transforms
(`translate`, `rotate`, `scale`), boolean CSG (`union`, `difference`, `intersect`),
sweeps (`extrude`, `revolve`, `loft`), and subdivision/smoothing. Meshes remain arrays
(§51.1), so procedural geometry is ordinary array code.

### 56.2 Materials, scenes, rendering

```qu
part.material = pbr(albedo = "steel", metallic = 0.9, roughness = 0.3)
scene3d shop
    add part ; add ground(plane())
    light directional(dir = (-1, -2, -1)) ; environment ibl("studio.hdr")
    camera(pos = (4, 3, 5), look_at = (0, 0, 0))
    render mode = pathtraced, samples = 256           # offline quality
    export = "part.png"                                # or mp4 for an animation (§39)
end scene3d
```

Physically-based materials, point/directional/area lights, image-based lighting,
real-time preview, and offline ray/path-traced render; import/export `obj`, `ply`,
`stl`, `gltf`, `usd`.

### 56.3 Spatial (3D) audio

Audio *is* a Qu signal (§41.2), so 3D sound reuses the whole DSP toolbox. Sources are
placed in space; a listener renders them binaurally (HRTF) or as ambisonics:

```qu
src = sound(oscillator(440 Hz)) at (2, 0, -1)          # a positioned source
eng = sound(sample("engine.wav")) at car.position       # moving source -> Doppler
listener at camera

mix = spatialize([src, eng], method = "hrtf",           # binaural render
                 reverb = space("hall.wav"),             # convolution reverb from an IR
                 doppler = true, occlusion = true)
play(mix) ; record_audio(mix, "scene.wav")
```

Positional attenuation, HRTF binaural, ambisonic B-format, convolution reverb from a
measured impulse response, Doppler, and occlusion. Synthesis (`oscillator`, `sample`,
a DSP graph) and any filter/effect from §24 apply, because it is all signals.

### 56.4 Real-time physics

Visible geometry and collision geometry are separate. `physics.compile_hull`
validates a mesh and produces a compact, immutable, cacheable collider artifact:

```qu
render_mesh = read_mesh("car.gltf")

# Fast broad phase; exact static track; convex/compound for dynamic bodies.
coarse = physics.compile_hull(render_mesh, kind="box")
track  = physics.compile_hull(track_mesh, kind="trimesh", static=true)
car    = physics.compile_hull(render_mesh, kind="convex", max_vertices=64,
                              tolerance=0.5 mm)

write_hull(car, "car.qhull")                 # optional build artifact
view(render_mesh, wire=car)                  # inspect render/collision mismatch
```

Required kinds are `box`, `sphere`, `convex`, `trimesh`, and `compound`. Box and
sphere are cheap broad-phase approximations. Convex is the default for one dynamic
rigid body. Triangle meshes preserve the indexed surface and are for static bodies;
a backend must reject a dynamic triangle mesh unless it explicitly supports one.
Compound decomposes a concave object into child convex hulls. A compiled hull stores
position/index arrays, bounds, mass-property inputs, source units, tolerance,
provider/version, and a deterministic source fingerprint. The fingerprint makes
unchanged hulls reusable across runs and testbenches. Degenerate/non-finite geometry,
invalid indices, zero volume where volume is required, or a failed tolerance are
diagnostics, never silent fallback to a box.

```qu
world = physics(gravity = (0, -9.81, 0) [m/s^2])
world.add(plane(), static = true, friction = 0.6)
ball_shape = physics.compile_hull(sphere(r = 0.2 m), kind="sphere")
ball = world.add(ball_shape, mass = 1 kg, restitution = 0.7) at (0, 5, 0)

joint = world.add(hinge(a, b, axis = z))               # constraints / joints
cloth = world.add(cloth(mesh, stiffness = 0.8))        # soft bodies, cloth
smoke = world.add(fluid(sph, particles = 20000))       # SPH fluids / particles

simulate world over 0 to 10 s step 1/240 s
    on step do render(world) end                        # or record to mp4
end simulate
hit = world.raycast(from = camera, dir = forward)       # queries
```

Rigid bodies with collisions, friction, and restitution; constraints and joints
(hinge, ball, fixed, spring); soft bodies, cloth, and SPH fluids; particles; ray
queries; and a `deterministic = true` option for reproducible runs. Stepping reuses
the ODE integrators of §52.1.

### 56.5 One scene, one core

Modeling, physics, audio, animation (§39), and rendering compose in a single
`scene3d`: a physically-simulated, spatially-audible, rendered and animated world that
also exports headless. And it bridges to the science core — engineering physics is FEM
(§52.2), game physics is this chapter; audio is DSP; meshes are arrays; rendering is
the 3D of §51 — so a digital twin can be both accurate and interactive.

---

## 57. State Estimation and Filtering

Estimating hidden state from noisy measurements — target tracking, sensor fusion,
navigation, SoC estimation — is core to signal processing and to the sensor work Qu
is built for. Qu ships the standard estimators under one **predict / update**
interface (mirroring fit/predict for models, §37.2), and every one is animatable
(§39) because its state and covariance are ordinary arrays.

### 57.1 The Kalman filter (linear)

```qu
kf = kalman(F = A, H = C, Q = q, R = r, x0 = x0, P0 = P0)   # state/measurement + noises
for z in measurements
    kf.predict()                       # time update:  x <- F x,  P <- F P F' + Q
    kf.update(z)                       # measurement update with gain K
    est = kf.x ; unc = kf.P            # filtered state and covariance
end
```

### 57.2 Nonlinear: EKF and UKF

```qu
ekf = ekf(f = motion, h = sensor, Q = q, R = r)   # Jacobians via autodiff (§38)
ukf = ukf(f = motion, h = sensor, Q = q, R = r)   # sigma-point (unscented) transform
```

Same `predict`/`update`; the EKF linearizes with automatic differentiation (no hand
Jacobians), the UKF uses sigma points for stronger nonlinearity.

### 57.3 The particle filter

```qu
pf = particle_filter(transition = f, likelihood = g, n = 1000)
pf.predict() ; pf.update(z) ; pf.resample()     # weighted particles
est = pf.estimate ; cloud = pf.particles         # non-Gaussian / multimodal
```

### 57.4 Others and smoothing

Complementary filter (IMU fusion), RTS smoother and `smooth` (offline, uses future
data), and a moving-horizon estimator — all under the same interface.

### 57.5 Animated tracking example

```qu
kf = kalman(F = cv_model(dt), H = pos_only, Q = q, R = r)
animate t from 0 to 10 fps 30
    z = noisy_position(true_path(t))     # a noisy measurement
    kf.predict() ; kf.update(z)
    scatter(z, color = "muted")          # measurements
    plot(kf.trace, color = "blue")       # KF estimate
    ellipse(kf.x, kf.P, conf = 0.95)     # 95% uncertainty ellipse
    title("t = {t:0.1f} s")
end animate
record animation to "tracking.mp4" fps = 30
```

A particle filter animates the same way, drawing its converging particle cloud
instead of a covariance ellipse — useful when the posterior is multimodal.

### 57.6 One interface

Every estimator is `predict`/`update`, its state and covariance are arrays, and it
composes with signals (§41.2), units (§42.2), and animation (§39). The same code that
tracks a target estimates a battery's state of charge from current and voltage.

---

## 58. Familiarity, Ease, and the Canonical Teaching Surface

Qu is not easy merely because individual examples are short. It is easy when a new
user can predict unfamiliar code from a small set of rules, when advanced features
do not change the meaning of beginner syntax, and when documentation shows one
normal way to write each idea.

### 58.1 The first-ten-minutes language

The introductory surface is intentionally conventional:

```qu
x = open_signal("bearing.wav")
y = bandpass(x, 500 Hz, 8 kHz)
X = fft(y)

plot(X)
title("Bearing spectrum")
grid on
show plot

if crest_factor(y) > 4 then
    warn("inspect bearing")
end if
```

A beginner needs only these rules:

| Idea | Canonical form | Familiarity reason |
|---|---|---|
| assignment | `x = expression` | Python, MATLAB, BASIC, and ordinary algebra |
| call | `f(x, option=value)` | visible argument boundary and familiar keyword arguments |
| indexing | `x[k]`, `M[:, k]` | Python/C-family bracket convention |
| block | `if … then` / `end if` | readable closure; indentation is not semantic |
| loop | `for k = 0 to N - 1` / `end for` | reads aloud; BASIC/VB/MATLAB heritage |
| range | `start to stop step amount` | names direction and step without positional guessing |
| indexing | `x[0]`; slice `x[0:N - 1]` | zero-based; slices include both ends, like every other range |
| array multiply | `A * B`; `a .* b` | MATLAB/engineering distinction: matrix versus elementwise |
| contract | `x as signal`, `t as vector(1, N)` | optional, English-like, attached to its value |
| pipeline | `x |> detrend |> fft` | one left-to-right flow spelling |

Small scripts require no declarations, types, backend choice, deferred binding,
class, schedule, or Unicode notation.

Ranges are inclusive everywhere -- `to`, the compact `a:s:b` literal, and
bracket slices alike -- because they name the last value rather than a
boundary. The language had the distinction once, with slices half-open, and it
cost more than it bought: the same `a:b` meant different things as a value and
as an index. Indexing is zero-based in base Qu;
MATLAB compatibility is a visible module policy, never an inference.

### 58.2 Progressive disclosure

Features are taught in three layers:

1. **Familiar scripting:** assignment, expressions, calls, arrays, control flow,
   functions, files, and plots.
2. **Scientific correctness:** sampled signals, physical units, shape contracts,
   frames, models, and `|>` pipelines.
3. **Expert control:** `:=` deferred graphs, dispatch, schedules, explicit
   devices, distribution, and real-time policy.

Layer 3 can optimize or constrain a Layer 1 program, but cannot silently change its
numerical meaning. A tutorial must not require Layer 3 syntax merely to load,
filter, transform, fit, plot, or export data.

### 58.3 Functions: familiar default, mathematical options

The introductory form is the explicit block:

```qu
function crest_factor(x)
    return max(abs(x)) / rms(x)
end function
```

After ordinary functions are understood, `crest_factor(x) := ...` introduces a
one-expression mathematical definition (§41.1). These are the only two function
forms; a third "mapping form" (`g: x -> y … end`) was documented at 0.8 but never
implemented, and is dropped (§34.C.2). Formatters never rewrite one function form
into another.

### 58.4 One call grammar

Ordinary functions always use parentheses: `plot(x, y)`, `text(x, y, label)`, and
`filter(x, cutoff=1 kHz)`. The proposed general command-call form `plot x, y` is
rejected (Appendix X.7): it creates a second grammar for calls and becomes harder to
scan when expressions or keyword arguments grow.

A small closed set of declarative commands remains because its operands are modes,
not arbitrary function arguments: `grid on`, `hold off`, `box on`, `show plot`.
New libraries cannot create new parenthesis-free call syntax.

### 58.5 One pipeline spelling

`|>` is the only data, model, geometry, or stream pipeline operator:

```qu
clean = raw |> detrend |> bandpass(500 Hz, 8 kHz)
summary = df |> where ok |> group by batch |> aggregate mean(power) as p
errors = tail("run.log", follow=true) |> grep("ERROR")
```

Bare `|` is not a pipeline. This avoids a visual collision with parallel circuit
composition (`R("Rct") | C("Cdl")`, §48.3) and leaves every left-to-right workflow
with the same reading direction.

### 58.6 Canonical spelling and accepted aliases

The parser may accept heritage or compatibility aliases, but tutorials, diagnostics,
the formatter, and generated code use one canonical spelling. Examples include
`const` rather than `constant`, `float64` rather than `double`, `^` rather than
`**`, ASCII `pi` rather than requiring `π`, and parenthesized calls rather than
command-call sugar. Compatibility aliases must never create different semantics.

### 58.7 The usability test

A proposed syntax feature is adopted only if it passes all four questions:

1. Can a Python, MATLAB, BASIC/VB, or Fortran user guess its role on first reading?
2. Does it remove recurring scientific bookkeeping or prevent a real class of error?
3. Can the parser and diagnostic explain it without depending on whitespace or
   backend state?
4. Does it preserve one canonical way to teach and format the idea?

If the answer is no, ordinary words and function calls win. Brevity is a benefit
only after predictability.

---

## 59. Algorithm Testing, Sandboxes, Testbenches, and AI Evaluation

Scientific software is not trustworthy because it ran once. It is trustworthy
when numerical error is bounded, invariants survive generated inputs, performance
is measured separately from correctness, and an exact failure can be replayed.
Qu therefore treats evaluation as part of the scientific workflow.

### 59.1 A deliberately small syntax surface

Qu adds only three contextual block words. They are recognized at statement start
and remain available as identifiers elsewhere:

```qu
test "FFT round trip"
    x = randn(4096, seed=42)
    assert_close(ifft(fft(x)), x, rtol=1e-12, atol=1e-14)
end test

test "filter stays finite" for all x in signals(length=2048, cases=200)
    y = lowpass(x, 2 kHz)
    assert all(isfinite(y))
end test

bench "one-million-point FFT" with warmup=5, samples=30
    fft(x)
end bench
```

`test` is the familiar xUnit idea with explicit `end test`. The optional
`for all name in generator` header makes it a property test; there is no separate
property-testing language. `bench` measures a body but does not decide numerical
correctness. `testbench` owns a rig or scenario and can target simulation,
software-in-the-loop, a recording, or live hardware.

Assertions are calls so libraries can extend them without growing the grammar.
The core `assert condition [, message]` statement is the sole exception.

| Assertion | Scientific intent |
|---|---|
| `assert_equal(a, b)` | exact values, labels, and discrete states |
| `assert_close(a, b, rtol=..., atol=...)` | floating/complex error with explicit tolerances |
| `assert_shape(x, expected)` | dimensions and orientation |
| `assert_units(x, expected)` | dimensional compatibility |
| `assert_allclose_ulps(a, b, max_ulps=...)` | representation-level regression |
| `assert_stable(system)` / `assert_causal(system)` | DSP/control invariants |
| `assert_band(response, pass=..., stop=...)` | ripple and attenuation masks |
| `assert_path(path, graph)` | endpoints, edges, and accumulated cost |

Tolerance is never guessed from the backend. Tests state it or select a named,
reviewable policy such as `float64_reference`; reports print the resolved limits.

### 59.2 Numerical and signal-processing tests

DSP users think in test vectors, frequency masks, SNR, phase, delay, stability,
and streaming boundaries. Qu keeps those nouns visible:

```qu
test "48 kHz low-pass meets its mask"
    h = design_lowpass(Fs=48 kHz, cutoff=5 kHz, taps=127)
    H = freqz(h, points=8192)

    assert_band(H, pass=band(0 Hz to 4.5 kHz, ripple=0.2 dB),
                   stop=band(6 kHz to 24 kHz, attenuation=70 dB))
    assert_close(group_delay(H, 1 kHz), 63 samples, atol=0.05 samples)
end test
```

Golden vectors carry units, shape, sample rate, endianness, checksum, and producer
version. Array failures report the worst coordinate, signed/absolute/relative
error, and a small neighborhood instead of dumping the entire array. Streaming
tests can vary chunk boundaries and compare chunked with batch execution.

### 59.3 Algorithm oracles: A* as the reference pattern

A fast algorithm should be checked against a simpler trusted oracle on small cases
and independent invariants on large cases. For A*, Dijkstra is the cost oracle
when edges are nonnegative:

```qu
test "A* agrees with Dijkstra" for all g in connected_graphs(nodes=4 to 80,
                                                               weights=0.1 to 20,
                                                               cases=500,
                                                               seed=20260821)
    start, goal = sample_pair(g)
    got = astar(g, start, goal, heuristic=euclidean)
    ref = dijkstra(g, start, goal)

    assert_path(got.path, g)
    assert_close(got.cost, ref.cost, rtol=1e-12)
    assert got.path[0] == start and got.path[end] == goal
end test
```

Generators save their seed and shrink a failure to the smallest known
counterexample. An admissible-heuristic suite also checks `h(goal) == 0`,
`h(n) <= true_cost(n, goal)`, unreachable goals, zero-weight edges, duplicate
frontier entries, and tie-breaking determinism. Benchmarks report expanded nodes,
peak frontier size, time, and allocation—not only wall-clock time.

### 59.4 Deterministic, capability-limited sandboxes

A sandbox is a value passed to the existing `with ... end with` form, not a second
control-flow grammar:

```qu
with sandbox(network=none, files=temp, process=none, devices=none,
             cpu=2, memory=512 MiB, timeout=2 s, seed=42,
             clock=frozen("2026-08-21T12:00:00Z"))
    result = candidate(input)
end with
```

The default is deny: no network, host files, subprocesses, environment secrets,
serial ports, or hardware. Tests grant capabilities explicitly. The runner can
virtualize time, randomness, temporary files, environment values, serial streams,
and sockets. CPU, memory, output, and wall-time limits are enforced by the worker
process, not trusted to candidate code. Secrets are referenced by name and never
embedded in source or recorded artifacts.

A completed run produces a content-addressed artifact containing source and lock
hashes, input or cassette hashes, seed, sandbox policy, backend, precision,
reduction policy, stdout/stderr, assertions, timings, and the smallest
counterexample. `qu replay <artifact>` reconstructs it elsewhere.

### 59.5 Testbenches from simulation to hardware

`testbench` groups a rig, fixtures, cases, capture, and cleanup while leaving those
pieces as ordinary values and calls:

```qu
testbench "ADC -> FIR -> detector" with mode=simulation
    rig = adc_rig(Fs=48 kHz, bits=16, seed=7)

    scenario("passband tone",
         stimulus=tone(1 kHz, amplitude=-12 dBFS, duration=200 ms),
         expect=expectation(gain=approx(-0.1 dB, atol=0.2 dB), alarm=false))

    scenario("stopband tone",
         stimulus=tone(12 kHz, amplitude=-6 dBFS, duration=200 ms),
         expect=expectation(attenuation=at_least(70 dB), alarm=false))
end testbench
```

The same testbench can select `simulation`, `software`, `recording`, or `hardware`.
Hardware mode requires explicit device capabilities, identifies firmware and
instrument calibration, records raw captures before derived results, and always
runs cleanup. Reports identify the executed mode; simulation success is never
displayed as hardware success.

### 59.6 ML and AI evaluation, including OpenAI providers

AI evaluation is a library protocol because providers and model catalogs evolve
faster than the language. Core Qu has no `openai` keyword and no hard-coded model:

```qu
import ai.eval
import ai.openai

agent = ai.openai.agent(model=env("OPENAI_MODEL"), tools=[lookup_manual])
suite = ai.eval.suite("maintenance answers",
    cases=load_cases("evals/maintenance.jsonl"),
    graders=[exact_fields, citation_check, rubric_model],
    sandbox=sandbox(network=allow("api.openai.com"), files=none))

report = suite.run(agent, repeats=5, seed=42)
assert report.pass_rate >= 0.95
assert report.p95_latency <= 3 s
```

Start with deterministic graders—exact fields, schemas, executable checks,
reference calculations, citations, and tool arguments—then add model graders for
qualities that cannot be stated mechanically. OpenAI's official grader interface
documents string-check, text-similarity, Python, score-model, label-model, and
multi-grader forms; a Qu adapter maps them to the same result protocol rather than
copying them into the grammar:
<https://developers.openai.com/api/reference/ruby/resources/graders/subresources/grader_models>.

Evaluation uses representative tasks and reports quality beside tokens, latency,
cost, tool calls, and retries. Resource savings count only while quality gates
still pass, following OpenAI's model guidance:
<https://developers.openai.com/api/docs/guides/latest-model>.

Because model output can vary, a run records the exact provider/model identifier,
parameters, grader versions, prompt/template hashes, tool definitions and trace,
token use, latency, and raw response ID. Repeated trials report confidence rather
than treating one sample as certainty. Unit tests use mocks or cassettes by
default; live evaluation is explicit, tagged, budget-limited, and optional.

### 59.7 Runner, reports, and separation of concerns

```text
qu test                         # correctness suites
qu test --tag dsp              # selected suites
qu test --sandbox strict       # stricter policy
qu bench --baseline main       # performance, not correctness
qu testbench --mode simulation # simulated rig
qu testbench --mode hardware   # explicit device grant required
qu replay sha256:...            # exact saved case
```

Results are structured records first and console text second. JSON and JUnit XML
export are required; HTML is recommended. Correctness, statistical confidence,
performance, cost, and hardware status occupy separate fields. A faster result
cannot turn a failed assertion green, and a model score cannot override a failed
deterministic safety check.

The design stays familiar because the control structure is ordinary, domain words
live in readable calls, and one test can grow from a scalar to a GPU array,
generated graph, agent trace, or instrument without changing what `test` means.

---

## Appendix X. Syntax Explorations (Non-Normative)

This appendix is the syntax decision record. It contains live proposals, adopted
ideas retained for provenance, and rejected experiments; every entry states its
status. Only an explicit **ADOPTED** entry has normative force, through the section
to which it was promoted. The sketch itself is not a second syntax surface:

```text
Redim X as Vector@double(K, 1) keep = {1, 2, ..., K}
f(x) := x^2
Y = f(X)

Z = f(X)

ez
    plot X, Y .- blue
    plot X, Z
    grid on
    text 0, max(Y)+0.1 ; "hi"
ze
```

### X.1 `@type` suffix — terse type tags

`Vector@double(K, 1)` reads as "a vector of doubles, shape (K,1)". It is a
one-token-shorter alternative to the 0.2 contract `vector(K, 1, float64)`. The `@`
binds a container to its element type, so `Matrix@complex128`, `Tensor@float32`
follow naturally, and it composes with `as`. **Tension:** two spellings for one
contract; `@` is otherwise unused, so there is room, but the base spec should pick
one canonical form and treat the other as sugar. **Lean:** attractive as pure sugar
over §8 contracts; decide after real code shows which reads better at call sites.

### X.2 `Redim` and `keep` — VB-flavored (re)allocation with initializer

`Redim X as ...` revives Visual Basic's `Redim`/`Redim Preserve`. The novel part is
`keep = {1, 2, ..., K}`: an initializer that seeds the freshly-shaped array, with
`{...}` as a set/list literal that admits the `1, 2, ..., K` comma-range from §12.
`keep` echoes `Redim Preserve` (retain contents across a reshape). **Tension:**
§8's post-assignment `as` contract deliberately refuses silent reshape; `Redim`
would be the *explicit* reshape verb the spec already gestures at with `x = x as
reshape(...)`. Two ways to reshape is one too many. **Lean:** if adopted, `Redim`
becomes *the* explicit reshape/reallocate statement and `reshape(...)` is its
functional form; `{...}` list literals are worth having regardless.

### X.3 `:=` as definition, not just deferral

`f(x) := x^2` uses `:=` to *define a function*. In 0.2, `:=` binds a deferred
dataflow **value** (§11) and `def fn f(x) = x^2` defines a one-line function. The
sketch unifies them: read `:=` as mathematics' "is defined as." A deferred value
and a lazy function are then the same idea — a named rule the planner may fuse,
inline, or evaluate on demand — and `f(x) := x^2` / `y := a*b` share one mental
model. **Tension:** overloading `:=` blurs "lazy value" and "function"; parsing
must distinguish `name(args) :=` (function) from `name :=` (value). **Lean:** the
unification is genuinely elegant and mathematically honest; strongest candidate in
this appendix for eventual promotion.
**Status: ADOPTED in 0.3 — promoted to §41.1 as the canonical function-definition
form ("kind 1"). `def fn` is retained as an accepted alias.**

### X.4 (removed) — named-output mapping form

This entry proposed `g: x -> y` with an indented body computing `y`, generalizing
to `h: (x, w) -> (loss, grad)` for multiple named outputs, and its status line
once read "ADOPTED in 0.8". It was never implemented in the parser or
interpreter, and Ahmed ruled on 2026-09-17 that it is not needed. Removed; see
the debate record at §34.C.2 for the full history.

### X.5 `ez` / `ze` figure blocks — word-pair delimiters

`ez ... ze` brackets a figure/plot block with a playful palindrome delimiter (BASIC
spirit: a low-ceremony "draw everything between here and there"). It overlaps the
declarative `plot { ... }` block (§18.C) and the `scene ... end scene` block (§39).
**Tension:** the core uses `end <kw>` consistently; introducing a `ez`/`ze` pair
breaks that regularity for cuteness. **Lean:** likely express the same intent as
`figure ... end figure`; retain `ez`/`ze` only if a friendly one-word figure opener
proves to matter for teaching/first-contact.

### X.6 `.-` linespec and `text ... ; ...` annotations

`plot X, Y .- blue` attaches a style with a MATLAB-linespec feel (`'b-'`), and
`text 0, max(Y)+0.1 ; "hi"` places an annotation with `;` separating position from
content. Both are terser than the keyword-argument forms `plot(X, Y,
color="blue")` and `text(x, y, "hi")`. **Tension:** `.-` collides with the
elementwise-operator family (`.*`, `./`, `.\`); a dot-operator that sometimes means
"styled by" and sometimes "elementwise" is a hazard. **Lean:** keep `text x, y ;
"..."` as a candidate command (clean, unambiguous); redesign the linespec to avoid
`.-` — perhaps `plot X, Y with blue` or a trailing style string `plot X, Y "b-"`.

### X.7 Command-call plotting without parentheses — rejected

Throughout the sketch, `plot X, Y` and `text ...` were proposed as *commands*, not
function calls. The familiarity review (§58) rejects a general second call grammar:
argument boundaries and keyword arguments are clearer in `plot(X, Y)` and
`text(x, y, "hi")`. Qu retains only a small closed set of declarative commands
whose grammar is not function-like, including `grid on`, `hold on`, and `show plot`.
This keeps BASIC/MATLAB convenience without making every contextual identifier
parse differently at the start of a statement.

---
