# Claude write-up: "Qu Language Specification" (a different, hypothetical Qu)

Saved verbatim, 2026-08-23, per Ahmed's request ("Food for thoughts Claude
1 ... to be saved as well and processed"). **Important framing**: despite
the shared name, this is a from-scratch design for a *statically typed,
ahead-of-time-compiled* language — dependent-ish shape types, a borrow
checker, effects, compiler-integrated autodiff, an MLIR/LLVM backend,
fixed-point embedded compilation, and so on. It describes a different
system from the actual Qu in this repository (a dynamically-typed,
tree-walking interpreter in the MATLAB/Julia/Python tradition). Treat it
as an aspirational reference/idea-mine, not a literal spec for this
codebase. See BACKLOG.md's "External language-design proposals (verdicts)"
section for the extracted, actionable items and an explicit list of what
does *not* fit this project's actual architecture.

---

# Qu Language Specification

Version 0.1 (design freeze candidate)

A statically typed, ahead of time compiled language for numerical computation, signal processing, and machine learning, intended to replace MATLAB and Python for the workloads those tools are actually used for.

---

## 0. Preface: what I tell myself before writing the spec

These are the commitments and the warnings. They constrain every decision that follows.

**Commitment 1. The differentiator is what the compiler knows, not what the syntax looks like.** MATLAB and Python are not painful because their syntax is bad. They are painful because the compiler knows nothing about sample rates, physical units, axis meanings, numerical conditioning, or deployment targets, so every one of those becomes a runtime surprise or a human convention. Qu earns its existence only if the compiler holds that knowledge. If a feature does not let the compiler reject a wrong program or generate better code, it belongs in a library, not in the language.

**Commitment 2. Interop on day one, or the language is dead on arrival.** Nobody abandons pandas, LAPACK, cuDNN, or forty years of Fortran because a new language is elegant. Qu must call C and Fortran with zero copy and zero wrapper code, and must embed in and be embedded by CPython. This is not an appendix, it is a top five requirement.

**Commitment 3. One canonical implementation of everything in the standard library.** The fragmentation of the Python numerical ecosystem (four array libraries, six plotting libraries, three autodiff systems that do not compose) is itself a usability cost. Qu ships one array type, one spectral estimator interface, one filter design interface, one autodiff mechanism, one plotting interface. Third parties extend, they do not replace.

**Commitment 4. The prototype and the deployed artifact are the same source.** The gap between a working script and a fixed point implementation on a microcontroller is where most measurement and instrumentation projects lose months. Qu closes it by making the embedded version a compilation target with a reported error bound, not a rewrite.

**Commitment 5. Determinism is a feature, not an accident.** Same source, same data, same result, on one thread or sixty four, on CPU or GPU, today or in five years. This forces decisions (no global RNG state, declared reduction order, no tracing garbage collector) that are unpleasant individually and correct collectively.

**Warning 1. Static shapes will hurt.** Every language that put array shapes into the type system has hit the same wall: real data has shapes that are not known until runtime, and ragged data has shapes that are not uniform at all. The spec must answer this explicitly (section 3.6) or it will be answered badly by users writing casts everywhere.

**Warning 2. Automatic device placement does not work.** Data movement dominates the cost, the compiler cannot know the cost model without profiling, and every system that promised `device="auto"` ended up requiring manual placement anyway. Qu makes placement explicit, cheap to write, and reports transfer cost. It does not pretend.

**Warning 3. Ownership is the largest usability risk.** Rejecting a tracing garbage collector is required for the real time and embedded targets, but affine types are the feature most likely to make an engineer give up in week one. The mitigation (value semantics, copy on write, inference so annotations are rare in numerical code) has to be in the design from the start, not bolted on.

**Warning 4. Scope creep kills domain languages.** Qu is not a systems language, not a web language, not a scripting language. Every request to make it general purpose is a request to make it worse at its job.

**What I am not allowed to defer:** indexing base, memory model, dispatch semantics, unit algebra including the logarithmic and affine cases, autodiff through mutation, reduction order, error handling strategy, and the ABI. Languages that deferred these shipped them by accident.

---

## 1. Charter

### 1.1 Scope

Qu targets programs whose subject matter is measurement, signals, numerical mathematics, statistical models, and machine learning models, from interactive exploration through to deployment on servers, accelerators, and embedded targets.

### 1.2 Non-goals

Qu is not intended for operating systems, device drivers, web services, general application programming, or as a shell scripting language. It has no ambition to be a good language for writing a compiler, including its own beyond the bootstrap.

### 1.3 Success criteria

1. A loop written naively over a typed array compiles to machine code equivalent to the C loop, with no vectorization by hand.

2. A dimensional, sample rate, or axis error is a compile error with a message naming the two conflicting quantities.

3. A model trained interactively can be compiled to a fixed point C99 implementation with a reported worst case deviation from the float reference, without editing the source.

4. Calling LAPACK, cuDNN, FFTW, or an arbitrary C function requires no wrapper code and no copy.

5. A numerical result is bit identical between a single threaded run and a parallel run by default.

6. A MATLAB or Python user writes useful code on day one, having learned units, named axes, and `let` versus `var`, and nothing else.

---

## 2. Lexical structure

### 2.1 Encoding and source form

Source files are UTF-8, extension `.qu`. Newlines are LF or CRLF and are significant only as statement terminators where a statement is otherwise complete (section 5.1). Identifiers are Unicode XID sequences, normalized to NFC. Greek letters and common mathematical symbols are permitted in identifiers, so `σ`, `Δt`, and `θ` are legal names. Confusable identifier pairs are a compile error, not a warning.

### 2.2 Comments

`#` to end of line. `#[` ... `]#` for nestable block comments. `##` at the start of a line is a documentation comment attached to the following item, and its content is Markdown with embedded runnable examples (section 19.4).

### 2.3 Keywords

```
let var fn return if else match while for in break continue
struct enum trait impl mod use pub type where as
true false and or not
async await task parallel on device
const comptime macro quote unquote
```

Attributes are not keywords, they are `@name(args)` and are extensible.

### 2.4 Literals

**Integers.** `42`, `0x2A`, `0b1010`, `0o52`, `1_000_000`. Default type `Int` (64 bit signed) unless inference determines otherwise.

**Reals.** `3.14`, `1e-9`, `2.5e3`, `1_234.5`. Default type `F64`. Suffix `f32`, `f16`, `bf16` forces width.

**Complex.** `3 + 4i`, `1i`, `2.5e-3i`. Type `C64` (pair of `F64`) by default.

**Units.** A numeric literal immediately followed by a unit identifier is a united quantity: `48kHz`, `2.5s`, `1.2mV`, `100Ω`, `9.81m/s^2`. There is no space and no operator. `48 kHz` with a space is also accepted and means the same thing, since `kHz` is not a valid expression on its own. Compound units use `*`, `/`, and `^` inside a unit position: `1.5kg*m/s^2`.

**Booleans.** `true`, `false`.

**Strings.** `"..."` with `\n`, `\t`, `\\`, `\"`, `\u{...}` escapes. Raw strings `r"..."`. Interpolation `"value is \(x)"` where the interpolated expression must implement `Display`. Multi-line strings use `"""`.

**Characters.** `'a'`, type `Char`, a Unicode scalar value.

**Array literals.** `[1, 2, 3]` is a one dimensional array with an anonymous axis. `[freq: [1,2,3]]` names the axis. Two dimensional literals use `;` for the major axis separator: `[1, 2; 3, 4]`.

### 2.5 Operators and punctuation

```
+  -  *  /  %  ^        arithmetic (^ is exponentiation, not xor)
.+ .- .* ./ .% .^       elementwise, when the default is not elementwise
==  !=  <  <=  >  >=    comparison
&&  ||  !                logical (aliases: and, or, not)
&   |   ~   <<  >>       bitwise on integers
=   +=  -=  *=  /=       assignment (only to var bindings)
|>                       pipeline
->                       function return type, closure body
=>                       match arm
::                       type ascription
..                       inclusive range
:                        half-open range, axis label, type annotation
?                        error propagation
@                        attribute
&  &mut                  borrow, mutable borrow
_                        wildcard, unused binding
```

`*` on two matrices is matrix multiplication. `.*` is elementwise. This inverts NumPy and matches MATLAB, chosen because the domain writes more matrix products than elementwise products, and because the elementwise form is the one that should be visually marked.

### 2.6 Precedence

From tightest to loosest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | postfix `[]`, `.`, `()`, `?` | left |
| 2 | unary `-`, `!`, `not`, `&`, `&mut` | right |
| 3 | `^`, `.^` | right |
| 4 | `*`, `/`, `%`, `.*`, `./`, `.%` | left |
| 5 | `+`, `-`, `.+`, `.-` | left |
| 6 | `<<`, `>>` | left |
| 7 | `..`, `:` (range construction) | none |
| 8 | `&` | left |
| 9 | `~` | left |
| 10 | `\|` | left |
| 11 | `==`, `!=`, `<`, `<=`, `>`, `>=` | none (chaining is an error) |
| 12 | `&&`, `and` | left |
| 13 | `\|\|`, `or` | left |
| 14 | `::` | none |
| 15 | `\|>` | left |
| 16 | `=`, and other assignments | right |

Comparison chaining (`a < b < c`) is a compile error with a message suggesting `a < b && b < c`, because the mathematical reading and the C reading differ and silently choosing either is a defect.

---

## 3. Type system

### 3.1 Kinds

Qu has four kinds of compile time entity: types, values, shape terms, and dimension terms. Shape terms are natural number expressions (section 3.5). Dimension terms are rational exponent vectors over base units (section 3.3). Generic parameters may range over any of the four, which makes the type system lightly dependent without admitting arbitrary term level computation in types.

### 3.2 Primitive types

| Type | Meaning |
|---|---|
| `Bool` | boolean |
| `I8 I16 I32 I64` | signed integers, `Int` aliases `I64` |
| `U8 U16 U32 U64` | unsigned integers |
| `F16 BF16 F32 F64` | IEEE 754 binary formats plus bfloat16 |
| `C32 C64` | complex, pairs of `F32` and `F64` |
| `Q(i, f)` | fixed point, `i` integer bits and `f` fractional bits, signed |
| `Char` | Unicode scalar value |
| `Str` | immutable UTF-8 string slice |
| `Unit` | the empty tuple, written `()` |

`Q(1, 15)` is the common Q1.15 audio format. Fixed point arithmetic has explicitly specified rounding and saturation behavior (section 12.5), it is not an approximation of float behavior.

### 3.3 Units and dimensions

A numeric type may carry a dimension: `F64<Hz>`, `F32<V>`, `F64<V/A>`. The dimension is a compile time term, a vector of rational exponents over the seven SI base dimensions plus two Qu additions, `angle` and `count`. Angle is dimensioned to distinguish radians from cycles, which is the single most common source of factor of `2π` errors in DSP. Count is dimensioned to distinguish samples from seconds.

Dimensional rules:

- Addition, subtraction, and comparison require identical dimension.
- Multiplication and division add and subtract dimension vectors.
- `^` with a rational literal exponent multiplies the dimension vector. A non-literal exponent requires a dimensionless base.
- Transcendental functions (`exp`, `log`, `sin`, and so on) require a dimensionless argument, except that `sin`, `cos`, and `tan` accept `angle` and their inverses produce `angle`.
- Dimension is erased before code generation and has no runtime cost or representation.

**Units versus dimensions.** `Hz` and `1/s` are the same dimension. Units are a presentation and conversion layer over dimensions. Conversion between units of the same dimension is implicit and exact where the scale factor is exact, and is performed at compile time on literals.

**Affine units.** Temperature in `degC` and `degF`, and any unit with a nonzero offset, is a distinct type `Affine[degC]`, not a scaled linear unit. `Affine` values support subtraction (producing a linear difference in `K`) and addition of a linear value, but not addition of two affine values, not multiplication, and not division. This prevents the classic error of averaging Celsius by summing.

**Logarithmic quantities.** `dB` is not a unit, it is a representation of a dimensionless ratio, and `dBm`, `dBV`, and `dBFS` are representations of a referenced quantity. They have type `Log[Ratio]` and `Log[V]` respectively. Addition of two `Log` values corresponds to multiplication of the underlying quantities, and this is the defined behavior, so `3dB + 3dB == 6dB` and `db_to_linear(6dB) ≈ 3.98`. Mixing `Log` and linear values in arithmetic is a compile error. Conversion is explicit: `linear(x)`, `db(x)`, `db_power(x)`, `db_amplitude(x)`, where the last two make the factor of 10 versus 20 a naming decision rather than a silent convention.

**User defined units.**

```qu
unit lsb = 1 / 32768.0             # dimensionless scale
unit ppm = 1e-6
unit galvanostat_gain: V/A = 1000.0
```

### 3.4 Arrays

The array is the central type.

```qu
Array[T; axis1: n1, axis2: n2, ...]
```

with sugar `[T; freq: 61, time: 500]`. An array carries, at the type level, its element type, an ordered list of named axes with shape terms, and, per axis, an optional coordinate type. It carries, at runtime, a pointer, per axis strides, and per axis coordinate metadata where present.

**Axis names.** Axis names are part of the type. `[F64; time: 100, ch: 4]` and `[F64; ch: 4, time: 100]` are the same type up to axis permutation, and the compiler chooses the physical layout. This is the key decision: axis identity is semantic, not positional. Two arrays with the same axis names in different written order are compatible, and the compiler inserts no transpose unless the memory layout requires it.

An anonymous axis is written `_`. Arrays with anonymous axes behave like conventional positional arrays and interoperate with foreign code.

**Coordinates.** An axis may carry coordinates:

```qu
let x: [F64<V>; time: 4800 @ 48kHz]        # uniform coordinate from a rate
let X: [C64<V>; freq: 2401 @ 0Hz : 10Hz]   # uniform coordinate from origin and step
let z: [F64; cell: 12 @ labels]            # categorical coordinate
```

Coordinates enable physical slicing (section 4.4), make sample rate a property of the value rather than a loose variable, and let `fft` compute the output coordinate automatically. A rate coordinate is stored as an origin and a step, not as a materialized vector.

**Layout.** Default memory layout is column major, chosen so that BLAS, LAPACK, and Fortran interop require no copy. Layout is a type level attribute (`@layout(row_major)`, `@layout(strided)`, `@layout(blocked(64))`) that affects performance and interop but not semantics. The compiler may choose layout for values whose layout is not observed.

**Semantic aliases.** The standard library defines aliases, not new types:

```qu
type Signal[T, n, r]      = [T; time: n @ r]
type Spectrum[T, n, df]   = [T; freq: n @ 0Hz : df]
type Spectrogram[T, f, t] = [T; freq: f, time: t]
type Image[T, h, w, c]    = [T; y: h, x: w, ch: c]
```

This is a deliberate rejection of a design where `Signal` and `Spectrum` are distinct first class types. Distinct types buy elegant dispatch but produce a combinatorial explosion of binary operator definitions and break generic code. Named axes buy the same dispatch (`plot` dispatches on the presence of a `freq` axis) with none of the explosion.

### 3.5 Shape algebra

Shape terms are natural number expressions built from literals, generic parameters, and the operations `+`, `-`, `*`, `/` (exact division only), `min`, `max`, and `ceil_div`. Equality of shape terms is decided by normalization to a canonical polynomial form plus a decision procedure for linear integer arithmetic. Terms outside that fragment are compared syntactically and fall back to a runtime check at the nearest function boundary, with a warning under `@strict_shapes`.

This is deliberately a weak fragment. Full dependent types would make the compiler undecidable and the error messages unreadable. Linear arithmetic over naturals covers convolution output sizes, strided windows, FFT bin counts, and batch splitting, which is the overwhelming majority of real shape arithmetic.

```qu
fn stft[n, w, h](x: [F64; time: n], window: w, hop: h)
    -> [C64; freq: w/2 + 1, frame: (n - w) / h + 1]
```

The return shape is checked, not asserted.

### 3.6 Dynamic and jagged shapes

This is the hard case, and the answer is explicit rather than implicit.

**Existential shapes.** A value whose shape is not known until runtime has an existentially quantified shape, introduced at an input boundary and eliminated by an `unpack`:

```qu
let raw = load_csv("measurement.csv")        # [F64; ?, ?]
unpack raw as x: [F64; time: n, ch: c] {
    # inside this block, n and c are ordinary shape parameters
    let y = fft(x, over: time)               # freq: n/2 + 1, statically related to n
}
```

Outside an `unpack`, indexing an existentially shaped array is permitted but each index is a runtime bounds check. Inside, the checks are hoisted or eliminated. There is one dynamic check at the boundary and static reasoning after it. This is the whole mechanism, and it is the answer to Warning 1.

**Jagged data.** Variable length records are not shape polymorphism, they are a distinct type:

```qu
Jagged[T; outer: n, inner]      # n records, each with its own inner length
```

with an offsets vector, `lengths()`, iteration yielding `[T; inner: len_i]` under an implicit `unpack`, and explicit `pad_to(len)`, `pack()`, and `bucket_by_length()` operations. Ragged batches for sequence models therefore have a name and a set of operations, instead of being encoded as a padded dense array plus a mask by convention.

**Dynamic axis count.** Rank is always static. There is no dynamically ranked array. Code that needs to be rank generic is written with generic axis lists (`fn f[Ax...](x: [F64; Ax...])`). Rank polymorphism is a type level list, not a runtime integer.

### 3.7 Structs, enums, tuples

```qu
struct Biquad {
    b: [F64; tap: 3],
    a: [F64; tap: 3],
    rate: F64<Hz>,
}

enum FilterKind {
    Lowpass(F64<Hz>),
    Bandpass(F64<Hz>, F64<Hz>),
    Notch { center: F64<Hz>, q: F64 },
}

type Pair = (F64, F64)
```

Structs are values with no inheritance. Enums are tagged unions with exhaustive `match`. Field access is `.name`. Tuples are anonymous product types with `.0`, `.1` access.

### 3.8 Traits

```qu
trait Spectral {
    fn spectrum[n](self, over: Axis) -> [F64; freq: n/2 + 1]
    fn nyquist(self) -> F64<Hz>
}

impl Spectral for Signal[F64, n, r] {
    fn nyquist(self) -> F64<Hz> { r / 2 }
    ...
}
```

Traits provide interface abstraction and are resolved statically. There is no trait object by default; dynamic dispatch requires `dyn Trait` and an explicit box, which is rare in numerical code and should be visible where it occurs.

### 3.9 Generics and multiple dispatch

Functions may be overloaded on the types of all arguments, resolved by specificity, with ambiguity being a compile error rather than an arbitrary choice.

```qu
fn solve(a: Matrix[F64], b: Vector[F64]) -> Result[Vector[F64], LinAlgError]
fn solve(a: Symmetric[F64], b: Vector[F64]) -> ...
fn solve(a: PosDef[F64],    b: Vector[F64]) -> ...      # Cholesky
fn solve(a: Sparse[F64],    b: Vector[F64]) -> ...
fn solve(a: Tridiagonal[F64], b: Vector[F64]) -> ...     # Thomas
```

The user writes `solve(a, b)` and receives the appropriate algorithm. Structural properties (`Symmetric`, `PosDef`, `Banded`, `Sparse`) are wrapper types that can be asserted (`assume_posdef`, unchecked, `@unsafe`), checked (`as_posdef(a)?`, returning a `Result`), or inferred by the compiler from construction (the result of `a' * a` is known symmetric positive semidefinite).

**World sealing.** Method sets are sealed per compilation unit. Adding a method after compilation is permitted only in JIT mode, and invalidates dependent specializations explicitly rather than silently. This is the price of ahead of time compilation and predictable dispatch, and it is worth paying.

### 3.10 Conversion

There is no implicit numeric widening, with three exceptions: integer literals to any numeric type in which they are exactly representable, unit conversions of the same dimension, and `F32` to `F64` in a context that already requires `F64`. Every other conversion is explicit (`x as F32`, `narrow(x)?`, `round_to(x, Q(1,15))`). Silent precision loss is the single most common source of irreproducible numerical results and is not permitted.

---

## 4. Expressions

### 4.1 Broadcasting

Broadcasting is by axis name, not by trailing position.

```qu
let x: [F64; time: 100, ch: 4]
let g: [F64; ch: 4]
let y = x .* g              # g is broadcast over time, unambiguously
```

An axis present in one operand and absent in the other is broadcast. An axis present in both with different shape terms is a compile error unless one of them is 1. There is no alignment rule to memorize, no `keepdims`, and no `newaxis`. Anonymous axes broadcast positionally from the right, matching NumPy, for interop code only.

### 4.2 Reductions

```qu
mean(x, over: time)
sum(x, over: [time, ch])
max(x, over: freq)
argmax(x, over: freq)          # returns a coordinate, in Hz, not an index
```

The reduced axis is removed from the result type. `over:` is mandatory when the array has more than one axis, so a whole array reduction of a multi-axis array is written `mean(x, over: all)` and never happens by accident.

### 4.3 Integer indexing

Zero based. `x[time: 0]` is the first element. `x[time: 0:10]` is a half-open range of ten elements. `x[time: 0..9]` is an inclusive range of the same ten elements. `end` denotes the last valid index, so `x[time: end]` and `x[time: end-3 .. end]`.

The two range forms with distinct spellings resolve the endless off by one argument by making the intent visible at the use site rather than being a global convention that half the users remember incorrectly.

One based indexing is available under `@one_based` at module scope for MATLAB migration, and is a lexical transformation on integer index expressions only. It is documented as a migration aid, not a supported style, and the formatter flags it.

### 4.4 Coordinate indexing

When an axis carries coordinates, it may be indexed by coordinate value:

```qu
x[time: 0.5s .. 1.5s]
X[freq: 1kHz .. 10kHz]
z[cell: "A3"]
```

Coordinate ranges are inclusive of both endpoints, because that is what an engineer means by "from 1 kHz to 10 kHz". A coordinate index that falls between samples selects the nearest, and `x[time: 0.5s, interp: linear]` interpolates. Out of range coordinates are a runtime error, or a compile error where the coordinate and the shape are both static.

### 4.5 Pipeline

`a |> f(b, c)` is `f(a, b, c)`. `a |> f(x, _, y)` places `a` at the underscore. Pipelines are the idiomatic way to write processing chains and compile identically to nested calls.

```qu
let clean = raw
    |> detrend()
    |> bandpass(20Hz, 2kHz)
    |> notch(50Hz, q: 30)
    |> resample(to: 16kHz)
```

### 4.6 Comprehensions

```qu
let y = [time: t in 0:n => sin(2*pi * 1kHz * t / rate)]
let m = [row: i in 0:n, col: j in 0:n => if i == j { 1.0 } else { 0.0 }]
```

Comprehensions produce arrays with named axes and are lowered to loops with no intermediate allocation.

### 4.7 Closures and function values

```qu
let f = |x: F64| -> F64 { x^2 + 1 }
let g = |x| x^2 + 1                # types inferred at the use site
```

Closures capture by value unless the capture is explicitly borrowed. A closure that captures nothing is a plain function pointer.

---

## 5. Statements and control flow

### 5.1 Statements

Statements are terminated by a newline where the statement is syntactically complete, or by `;`. A line ending inside unclosed brackets, or with a trailing binary operator or `|>`, continues. There is no line continuation character.

### 5.2 Bindings

```qu
let x = 3.0                # immutable
var acc = 0.0              # mutable
let y: [F64; time: 100]    # declared, must be initialized before use
```

Shadowing in an inner scope is permitted. Shadowing in the same scope is an error.

### 5.3 Conditionals

`if` is an expression. All branches must have the same type, or the value is discarded.

```qu
let gain = if snr > 20dB { 1.0 } else { 0.5 }
```

### 5.4 Match

```qu
match kind {
    Lowpass(fc)          => design_lp(fc),
    Bandpass(lo, hi)     => design_bp(lo, hi),
    Notch { center, q }  => design_notch(center, q),
}
```

Exhaustiveness is checked. A non-exhaustive match without a `_` arm is a compile error.

### 5.5 Loops

```qu
for i in 0:n { ... }
for (t, v) in enumerate(x, over: time) { ... }
while cond { ... }
loop { ... break }
```

Loops over an array axis yield elements with the remaining axes intact, so iterating a `[F64; time: n, ch: 4]` over `time` yields `[F64; ch: 4]` values.

Under `@realtime` and `@target(embedded)`, every loop must have a statically provable bound (section 14).

---

## 6. Functions

### 6.1 Declaration

```qu
fn welch[n, w](
    x: [F64; time: n @ rate],
    window: Window = hann(w),
    overlap: F64 = 0.5,
) -> [F64<V^2/Hz>; freq: w/2 + 1 @ 0Hz : rate/w] {
    ...
}
```

Parameters are positional, then keyword with defaults. A parameter may be marked keyword only with a leading `*`. Generic shape and unit parameters are inferred from arguments and rarely written at the call site.

### 6.2 Purity and effects

Function types carry an effect row (section 8). A function with no effects is pure, is a candidate for compile time evaluation, common subexpression elimination, and reordering, and is automatically differentiable.

### 6.3 Returning multiple values

Tuples, with destructuring:

```qu
let (peaks, props) = find_peaks(x, prominence: 0.1)
```

Named struct returns are preferred for more than two values.

---

## 7. Memory model

### 7.1 Value semantics

Arrays are values. Assignment is a logical copy. The implementation uses copy on write with reference counting, and the compiler elides the copy whenever it can prove the source is dead or not observed, which in straight-line numerical code is nearly always.

```qu
let a = zeros[F64; time: 1_000_000]()
let b = a                # no copy, refcount
var c = a                # no copy yet
c[time: 0] = 1.0         # copy happens here, and only here
```

### 7.2 Mutation and borrowing

Mutation requires a `var` binding or a `&mut` borrow. The borrow rules are the standard affine discipline: any number of shared borrows or exactly one mutable borrow, not both. Lifetimes are inferred and are almost never written, because numerical code overwhelmingly passes owned values or shared borrows into pure functions.

```qu
fn normalize_inplace(x: &mut [F64; time: n]) { ... }
```

Two functions may not hold overlapping mutable views of the same array, which makes aliasing information available to the optimizer for free and eliminates the class of aliasing bugs that plague in-place BLAS calls.

### 7.3 No tracing garbage collector

Memory is managed by ownership plus reference counting for shared values. There is no tracing collector, therefore no unpredictable pause, therefore real time and embedded targets are expressible in the same language. This is the single decision that most constrains the rest of the design and it is not negotiable given Commitment 4.

### 7.4 Arenas

```qu
@arena(size: 4MB)
fn process(frame: &[F64; time: 512]) -> Detection {
    # all allocation in this scope comes from the arena
    # and is released at scope exit, with no per-object cost
}
```

Arenas make allocation cost statically bounded, which is what `@realtime` requires.

---

## 8. Effects and capabilities

Every function has an inferred effect set. The tracked effects are:

| Effect | Meaning |
|---|---|
| `alloc` | may allocate heap memory |
| `io` | may perform input or output |
| `rand` | consumes randomness |
| `nondet` | result may vary between identical runs |
| `unbounded` | may not terminate, or has no static loop bound |
| `unsafe` | contains unchecked operations |
| `device(d)` | executes on device `d` |

Effects propagate through calls and are part of the function type. Attributes constrain them:

```qu
@pure                 # no effects at all
@realtime             # no alloc, no io, no unbounded
@deterministic        # no nondet; rand is permitted if explicitly seeded
@target(cortex_m7)    # implies @realtime and more (section 14)
```

An effect violation is a compile error naming the offending call chain. This is the mechanism that makes the embedded promise checkable rather than aspirational.

---

## 9. Error handling

There are no exceptions.

**Expected failure** is `Result[T, E]` with `?` propagation:

```qu
fn fit(z: Spectrum[C64, n, df]) -> Result[CircuitParams, FitError] {
    let a = build_jacobian(z)?
    let x = solve(a, residual)?
    Ok(x)
}
```

**Programming errors** (index out of bounds, unwrapping an `Err`, arithmetic overflow in checked mode) panic. A panic unwinds, runs destructors, and terminates the task. Under `@realtime` and `@target(embedded)`, panics are configured to a fixed handler and unwinding is disabled.

**Numerical failure is expected failure.** A singular matrix, a non-convergent solver, a filter design that cannot meet a specification, all return `Result`. They do not return `NaN` and hope. `NaN` and `Inf` remain IEEE values and are produced only by the arithmetic that IEEE defines to produce them.

---

## 10. Modules, packages, editions

### 10.1 Modules

```qu
mod dsp {
    pub fn welch(...) -> ... { ... }
    fn helper(...) { ... }        # private by default
}

use qu.dsp.{welch, stft}
use qu.linalg as la
```

Module paths mirror directory structure. There is no circular import.

### 10.2 Packages

A package has a `qu.toml` manifest with a name, a version, dependencies with semantic version constraints, and a target list. The resolver produces a lock file containing exact versions and content hashes of every transitive dependency, including native artifacts. A build with a lock file is bit reproducible given the same toolchain version.

Binary artifacts for native dependencies (BLAS, FFTW, CUDA runtime) are distributed prebuilt per target triple, with a source fallback. The package manager is part of the language distribution, not a third party project, and there is exactly one of it.

### 10.3 Editions

Breaking language changes are gated by an edition declared in the manifest. A package of one edition may depend on a package of another. Editions are declared at most every three years, and the compiler supports every edition indefinitely.

---

## 11. Automatic differentiation

### 11.1 Position in the stack

Autodiff is a compiler transformation on the typed intermediate representation, applied before optimization and after type checking. It is not a library and does not require a special array type. Any function whose effect set excludes `io` and `nondet`, and whose operations all have registered rules, is differentiable.

### 11.2 Interface

```qu
let g  = grad(loss, wrt: theta)              # reverse mode, scalar output
let j  = jacobian(model, wrt: params)        # forward or reverse, chosen by shape
let h  = hessian(f, wrt: x)                  # forward over reverse
let dv = jvp(f, x, v)                        # forward mode directional
let vp = vjp(f, x, w)                        # reverse mode, explicit
```

Mode selection for `jacobian` is automatic and is a shape comparison, forward when inputs are fewer than outputs, reverse otherwise, overridable.

### 11.3 Units and differentiation

The derivative of a `F64<V>` with respect to a `F64<s>` has type `F64<V/s>`. Dimensional correctness of gradients is therefore checked, which catches an entire class of loss function and parameterization errors.

### 11.4 Control flow and mutation

Branches and loops are differentiated by recording the taken path. Loops with statically known bounds are unrolled or use a fixed size tape. Loops with dynamic bounds allocate a tape, which makes them `alloc` effecting and therefore illegal under `@realtime`, correctly.

Mutation is supported. An in-place write generates the corresponding scatter-accumulate in the reverse pass. Aliasing is already forbidden by the borrow rules (section 7.2), which is what makes this sound, and is a direct benefit of rejecting a garbage collector.

### 11.5 Custom rules

```qu
@custom_vjp(bessel_j0)
fn bessel_j0_vjp(x: F64, w: F64) -> F64 { -w * bessel_j1(x) }

@nodiff
fn quantize(x: F64) -> Q(1,15) { ... }

fn straight_through(x: F64) -> Q(1,15) {
    detach(quantize(x)) + x - detach(x)
}
```

`detach` stops gradient flow. `@nodiff` marks a function as non-differentiable, so calling it in a differentiated context is a compile error rather than a silent zero gradient, which is the failure mode that costs the most debugging time in existing frameworks.

### 11.6 Differentiating the standard library

Every numerical operation in the standard library has registered rules, including the linear algebra factorizations, the ODE solvers (via the adjoint method), the FFT, and the filter application. This means that fitting an equivalent circuit model, training a network, and performing a sensitivity analysis on a filter design all use the same mechanism.

---

## 12. Numerical semantics

### 12.1 Floating point

IEEE 754 semantics are preserved exactly by default. The compiler performs no reassociation, no distribution, no fused multiply add contraction, and no reciprocal substitution unless permitted. `@fastmath(reassoc, contract, no_nan, no_inf)` enables specific relaxations, individually, at the smallest granularity, and is never a whole program flag.

### 12.2 Reduction order

Reductions have a specified order: pairwise tree reduction with a fixed block size determined by the shape, not by the thread count. A parallel reduction produces bit identical results to a serial one. Naive left to right accumulation is available as `sum(x, order: sequential)` and Kahan compensated summation as `sum(x, order: compensated)`.

### 12.3 Randomness

There is no global random state. Random number generators are counter based (Philox style), explicitly constructed and explicitly passed or split:

```qu
let rng = Rng(seed: 42)
let (rng1, rng2) = split(rng)
let noise = randn(rng1, [F64; time: 1000])
```

A parallel program produces the same result as a serial one because each task receives a split stream, not a share of a global stream. Reproducibility is structural, not a discipline.

### 12.4 Conditioning

Operations with a known conditioning hazard report it. `inv(a)` exists but produces a warning that names `solve` as the intended alternative in a linear system context. Factorizations return a reciprocal condition estimate, and `solve` returns an `Err(IllConditioned { rcond })` when the estimate crosses a threshold, which is settable and non-silent.

### 12.5 Fixed point

`Q(i, f)` arithmetic has explicit rounding (`round_nearest_even` by default, `truncate` available) and explicit overflow behavior (`saturate` by default, `wrap` available, `checked` under debug). Multiplication of `Q(a,b)` by `Q(c,d)` produces `Q(a+c, b+d)` and a requantization is an explicit operation, so precision loss in a fixed point pipeline is always visible in the source.

---

## 13. Concurrency, parallelism, devices

### 13.1 Tasks

Structured concurrency. Tasks are spawned in a scope and joined at scope exit. No detached tasks, no orphaned work.

```qu
task scope {
    let a = spawn { expensive_1() }
    let b = spawn { expensive_2() }
    combine(a.await, b.await)
}
```

### 13.2 Data parallelism

```qu
@parallel
for cell in cells { process(cell) }

let y = map(x, over: batch, |sample| model(sample))
```

Data races are prevented by the borrow rules, not by convention. A parallel loop that mutates shared state without an atomic or a reduction is a compile error.

### 13.3 Devices

Placement is explicit and cheap. There is no automatic placement, for the reason given in Warning 3.

```qu
let xd = x on gpu0            # explicit transfer, a value of type Device[gpu0, ...]
on gpu0 {
    let y = model(xd)         # everything in this scope executes on the device
}
let result = y on cpu
```

A device typed value used in a host expression is a compile error naming the required transfer. The compiler reports transfer volume and estimated cost per scope with `qu explain --transfers`, so the user optimizes placement with data rather than by guessing.

Device kinds in the initial specification: `cpu`, `cuda`, `rocm`, `metal`, `vulkan` (via SPIR-V). A kernel is ordinary Qu code, not a separate sublanguage, subject to the effect restrictions of the target.

---

## 14. Real time and embedded subset

`@target(t)` selects a compilation target and implies a language subset. For a microcontroller target:

- No heap allocation. Every array has a statically known shape or comes from an arena.
- Every loop has a statically provable bound.
- No unwinding. Panics go to a fixed handler.
- No dynamic dispatch.
- Recursion permitted only where the depth is statically bounded.

The compiler reports, per entry point, worst case stack usage, static memory footprint, and, where a cycle model for the target is available, a worst case cycle bound. These are not estimates produced by a separate tool, they are compiler outputs, and a build that cannot produce them fails rather than warning.

### 14.1 Fixed point compilation

```qu
@target(cortex_m7, fixed: Q(1,15))
fn detect(x: [F64<V>; time: 256 @ 100kHz]) -> F64 {
    goertzel(x, at: 10kHz).magnitude
}
```

The compiler compiles the function twice, once in the declared float reference and once in the requested fixed point format, and reports the worst case and RMS deviation between them over a supplied or generated input set. The user sets a tolerance, and exceeding it fails the build. This is what makes Commitment 4 checkable.

### 14.2 Real time deadlines

```qu
@realtime(deadline: 10ms, block: 512 @ 48kHz)
pipeline audio {
    input()
    |> highpass(20Hz)
    |> bandpass(100Hz, 8kHz)
    |> classifier()
    |> output()
}
```

The compiler reports estimated worst case execution time per block against the deadline, and identifies the dominant contribution when the deadline is not met. Where the timing model for the target is unavailable, it says so instead of guessing.

---

## 15. Metaprogramming

### 15.1 Compile time evaluation

`comptime` marks an expression or a function evaluated during compilation. A `comptime` function has the full language available minus `io` and `nondet`. Filter coefficient design, window generation, and lookup table construction therefore happen at compile time with no special mechanism.

```qu
const COEFFS = comptime butter(4, 100Hz, 2kHz, rate: 48kHz).sos
```

### 15.2 Macros

Hygienic macros operating on the typed abstract syntax tree, not on tokens:

```qu
macro layers(body: Block) -> Expr { ... }
```

Macros are used for the model definition sugar (section 18.7) and for domain notation. Token level macros and textual substitution do not exist.

---

## 16. Foreign interfaces

### 16.1 C

```qu
extern "C" {
    fn dgemm_(transa: &Char, ..., a: &mut [F64; _, _], ...) -> ()
}
```

Zero overhead, no wrapper generation, no marshalling. Qu arrays with a contiguous or strided layout pass as a pointer plus strides. The C header can be imported directly (`use c_header("fftw3.h")`) and the compiler generates the declarations.

### 16.2 Fortran

Native, because the default layout is column major and the calling convention is supported directly. LAPACK is callable without a shim, which is the practical reason for the layout choice.

### 16.3 Python

Bidirectional and zero copy through the buffer protocol and DLPack.

```qu
use python.numpy as np
let a = np.load("data.npy") as [F64; time: ?, ch: ?]
```

and from Python:

```python
import qu
mod = qu.load("processing.qu")
y = mod.process(x)          # x is a numpy array, no copy
```

The Python bridge is a first class supported component with the same stability guarantees as the rest of the language, because Commitment 2 says nobody migrates all at once.

### 16.4 MATLAB

`.mat` file reading and writing in the standard library. A `qu.compat.matlab` module providing one based wrappers and MATLAB named functions, explicitly documented as a migration aid. A source translator (`qu translate file.m`) that produces Qu source with a report of the constructs it could not translate faithfully, listing them rather than guessing.

---

## 17. Compilation model

### 17.1 Pipeline

Source, then parse, then macro expansion, then type and unit and shape inference, then effect inference, then autodiff transformation, then MLIR dialect lowering (`qu.array`, `qu.signal`, `qu.linalg`, `qu.nn`), then target lowering, then LLVM or SPIR-V or C99 emission.

MLIR is chosen rather than a bespoke compiler because the domain specific optimizations (loop fusion, tiling, layout selection, kernel fusion) are exactly what MLIR dialects exist to express, and because writing a competitive optimizing backend from scratch is a decade of work that produces nothing the domain user can see.

### 17.2 Modes

**JIT** for the REPL and notebooks, with incremental recompilation and a compile latency budget of well under a second for a typical cell, because the interactive loop is the thing MATLAB gets right and every compiled alternative has historically gotten wrong.

**AOT** for everything else, with whole program optimization, link time optimization, and optional profile guided optimization.

### 17.3 Targets

`x86_64`, `aarch64`, `riscv64`, `cortex-m` family, `cuda`, `rocm`, `metal`, `spirv`, `wasm`, and `c99` as a portable source output for targets with a vendor toolchain but no LLVM backend.

### 17.4 Explain

`qu explain` is a first class compiler output, not a debugging tool. For a pipeline or a function it reports the resolved algorithm choices, the filter realizations, the array layouts chosen, the fusion decisions, the allocation sites, the parameter counts, the estimated latency per stage, and the device transfers. This makes the compiler's knowledge inspectable, which matters for teaching, for review, and for trust.

---

## 18. Standard library

One canonical implementation of each of the following, all sharing the array type, the unit system, and the autodiff mechanism.

**18.1 core.** Primitives, `Result`, `Option`, iterators, strings, formatting, time, filesystem, and the array type with construction, reshaping, concatenation, sorting, searching, and set operations.

**18.2 units.** SI base and derived units, common engineering units, affine and logarithmic quantities, conversion, and parsing.

**18.3 linalg.** Dense and sparse. `solve` with property based dispatch, `lu`, `qr`, `chol`, `ldl`, `svd`, `eig`, `schur`, `hessenberg`, `pinv`, `lstsq`, `cond`, `rank`, `null`, `expm`, `logm`, `sqrtm`, iterative solvers (`cg`, `minres`, `gmres`, `bicgstab`, `lsqr`) with preconditioners, and Krylov eigensolvers.

**18.4 fft.** `fft`, `ifft`, `rfft`, `irfft`, `fft2`, `fftn`, `dct`, `dst`, `czt`, `goertzel`, `next_fast_len`, with automatic coordinate propagation so the output carries its frequency axis.

**18.5 dsp.** Filter design (`butter`, `cheby1`, `cheby2`, `ellip`, `bessel`, `firwin`, `firls`, `remez`, `iirnotch`, `iirpeak`), filter application (`filt`, `filtfilt`, `sosfilt`, `lfilter` with explicit state for streaming), analysis (`freqz`, `group_delay`, `zplane`, `impz`, `stepz`), spectral estimation (`welch`, `periodogram`, `multitaper`, `csd`, `coherence`, `tfestimate`, `stft`, `istft`, `spectrogram`, `cwt`), resampling (`resample`, `decimate`, `interpolate`, `upfirdn`, `polyphase`), windows, and the analysis utilities (`hilbert`, `envelope`, `xcorr`, `find_peaks`, `detrend`, `savgol`, `medfilt`, `rms`, `thd`, `snr`, `sinad`, `bandpower`). Multisine and pseudorandom excitation design (`multisine`, `crest_optimize`, `dibs`, `mls`, `chirp`) is in the core, because designing an excitation is not a niche operation in a language for measurement.

**18.6 stats.** Distributions with `pdf`, `cdf`, `quantile`, `rand`. Descriptive statistics, hypothesis tests, regression, resampling, and uncertainty propagation (section 18.11).

**18.7 nn.** Layers as ordinary functions with parameters, not framework objects.

```qu
model classifier(x: Spectrogram[F32, f, t]) -> [F32; class: 4] {
    x |> conv2d(out: 32, kernel: 3) |> relu() |> maxpool(2)
      |> conv2d(out: 64, kernel: 3) |> relu() |> maxpool(2)
      |> flatten() |> dense(128) |> relu() |> dense(4) |> softmax()
}

let trained = fit(classifier, data: train,
                  loss: cross_entropy, opt: adam(1e-3),
                  epochs: 50, on: gpu0)?
```

`model` is sugar for a struct of parameters plus a function, generated by a macro, with the parameters visible and manipulable as ordinary values. There is no hidden state, no global graph, and no session.

**18.8 ml.** Classical methods: linear and generalized linear models, regularized regression, SVM, kernel methods, trees, forests, gradient boosting, Gaussian processes, clustering, mixture models, dimensionality reduction, feature selection (filter, wrapper, and embedded, including mRMR and relief), cross validation with grouping and time series awareness, pipelines that keep preprocessing inside the fold, and calibration and metrics.

**18.9 opt.** Unconstrained and constrained optimization, least squares, global methods, and derivative free methods, all consuming gradients from section 11 automatically when available.

**18.10 ode.** Initial value problems with stiff and non-stiff solvers, boundary value problems, differential algebraic equations, and adjoint sensitivity, differentiable end to end.

**18.11 uncert.** Optional uncertainty attached to a quantity, propagated through arithmetic and through the standard library by first order propagation with a Monte Carlo fallback. An impedance computed from a measured voltage and a measured current arrives with its own uncertainty without the user writing a propagation formula.

**18.12 io.** CSV, HDF5, Parquet, Arrow, NetCDF, WAV, TDMS, MAT, NPY, JSON, TOML. A loaded measurement carries its sample rate, units, and channel names as array metadata where the format records them.

**18.13 plot.** One interface, multiple backends (interactive, vector, raster). Dispatches on array structure, so an array with a `freq` axis plots on a log frequency axis with the correct label by default, and a `Spectrogram` plots as an image with time and frequency axes and a color bar.

**18.14 sym.** Symbolic expressions, simplification, symbolic differentiation and integration, equation solving, series, transforms (`laplace`, `ztrans`, `fourier`), and compilation of a symbolic expression to a Qu function. Symbolic and numeric coexist in one language, so an equivalent circuit model can be written symbolically, differentiated symbolically, compiled, and fitted numerically without leaving the file.

---

## 19. Tooling

**19.1 `qu` driver.** One binary. `qu run`, `qu build`, `qu test`, `qu repl`, `qu fmt`, `qu doc`, `qu explain`, `qu bench`, `qu translate`, `qu add`, `qu lock`.

**19.2 Formatter.** One canonical style, no configuration. Ends the formatting argument permanently.

**19.3 REPL and notebooks.** The REPL is the compiler in JIT mode with the same semantics as a compiled program. There is no separate interactive dialect. Notebook cells are compilation units with incremental recompilation, and a notebook file is plain Qu source with cell markers, so it diffs and reviews as source.

**19.4 Documentation and testing.** Examples in documentation comments are compiled and run as tests. Property based testing and numerical tolerance assertions (`assert_close(a, b, rtol: 1e-9)`) are in the core test framework.

**19.5 Experiment reproducibility.** The compiler emits, per build, a manifest of the toolchain version, dependency lock hashes, target, and any `@fastmath` relaxations in effect. Experiment tracking beyond that (metric logging, run comparison) is a library, not a language feature, because tooling conventions change on a two year cycle and language semantics should not.

---

## 20. Migration

### 20.1 From MATLAB

| MATLAB | Qu |
|---|---|
| `x(1)` | `x[0]` or `x[time: 0]` |
| `size(x, 1)` | `shape(x, time)` |
| `A\b` | `solve(a, b)?` |
| `A'` | `adjoint(a)`, `transpose(a)` for the non-conjugating form |
| `fft(x)` | `fft(x, over: time)` |
| `butter(4, fc/(fs/2))` | `butter(4, fc, rate: fs)` |
| `filtfilt(b, a, x)` | `x \|> filtfilt(f)` |
| `[b, a] = butter(...)` | `let f = butter(...)`, a `Filter` value |
| `end` | `end` |
| `1:n` | `1..n` or `0:n` |

The semantic differences that cannot be papered over: zero based indexing, no automatic array growth on out of range assignment, no implicit numeric conversion, functions are not files, and there is no global workspace. The translator reports each occurrence rather than emitting a guess.

### 20.2 From Python

| Python | Qu |
|---|---|
| `np.array([...])` | `[...]` |
| `x.mean(axis=0)` | `mean(x, over: time)` |
| `x[:, None]` | not needed, broadcasting is by name |
| `x.reshape(...)` | `reshape(x, [freq: f, frame: t])`, checked |
| `scipy.signal.welch(x, fs)` | `welch(x)`, the rate is in the value |
| `torch.tensor(x).cuda()` | `x on gpu0` |
| `with torch.no_grad()` | `detach(...)` or a non-differentiated call |
| `try/except` | `Result` and `?` |

The semantic differences: static types, no duck typing, no runtime attribute addition, no exceptions, and shapes checked before the run rather than during it.

---

## 21. Conformance and stability

A conforming implementation must implement sections 2 through 17 in full. Section 18 (standard library) has a required core (`core`, `units`, `linalg`, `fft`, `dsp`, `stats`, `io`) and an optional remainder, since a microcontroller target has no use for a plotting backend.

The language is versioned by edition (section 10.3). Within an edition, no source that compiles will stop compiling, and no numerical result will change, except where a defect is fixed and the fix is documented with an opt-out for one edition.

---

## 22. Core grammar

Abbreviated EBNF for the expression and declaration core. Whitespace and comments are omitted.

```ebnf
program     = { item } ;
item        = mod_decl | use_decl | fn_decl | struct_decl | enum_decl
            | trait_decl | impl_decl | type_alias | const_decl | unit_decl ;
fn_decl     = { attribute } , "fn" , ident , [ generics ] ,
              "(" , [ params ] , ")" , [ "->" , type ] , [ where_cl ] , block ;
generics    = "[" , gparam , { "," , gparam } , "]" ;
gparam      = ident , [ ":" , kind ] ;
kind        = "Type" | "Shape" | "Dim" | "Value" ;
params      = param , { "," , param } , [ "," ] ;
param       = [ "*" ] , ident , ":" , type , [ "=" , expr ] ;
type        = prim_type | array_type | path_type | tuple_type
            | ref_type | fn_type | device_type ;
array_type  = "[" , type , ";" , axis_list , "]" ;
axis_list   = axis , { "," , axis } ;
axis        = ( ident | "_" ) , ":" , shape_term , [ "@" , coord ] ;
shape_term  = shape_add ;
shape_add   = shape_mul , { ( "+" | "-" ) , shape_mul } ;
shape_mul   = shape_atom , { ( "*" | "/" ) , shape_atom } ;
shape_atom  = int_lit | ident | "?" | "(" , shape_term , ")" ;
coord       = expr | expr , ":" , expr | "labels" ;
prim_type   = ( "F64" | "F32" | "I64" | ... ) , [ "<" , unit_expr , ">" ] ;
unit_expr   = unit_mul , { "/" , unit_mul } ;
unit_mul    = unit_atom , { "*" , unit_atom } ;
unit_atom   = ident , [ "^" , signed_int ] ;
ref_type    = "&" , [ "mut" ] , type ;
device_type = "Device" , "[" , ident , "," , type , "]" ;
stmt        = let_stmt | var_stmt | expr_stmt | for_stmt | while_stmt
            | loop_stmt | return_stmt | break_stmt | continue_stmt ;
let_stmt    = "let" , pattern , [ ":" , type ] , "=" , expr ;
var_stmt    = "var" , pattern , [ ":" , type ] , "=" , expr ;
expr        = assign_expr ;
assign_expr = pipe_expr , [ assign_op , assign_expr ] ;
pipe_expr   = or_expr , { "|>" , call_tail } ;
or_expr     = and_expr , { ( "||" | "or" ) , and_expr } ;
and_expr    = cmp_expr , { ( "&&" | "and" ) , cmp_expr } ;
cmp_expr    = range_expr , [ cmp_op , range_expr ] ;
range_expr  = add_expr , [ ( ".." | ":" ) , add_expr ] ;
add_expr    = mul_expr , { ( "+" | "-" | ".+" | ".-" ) , mul_expr } ;
mul_expr    = pow_expr , { ( "*" | "/" | "%" | ".*" | "./" ) , pow_expr } ;
pow_expr    = unary_expr , [ ( "^" | ".^" ) , pow_expr ] ;
unary_expr  = { "-" | "!" | "not" | "&" | "&mut" } , postfix_expr ;
postfix_expr= primary , { call_tail | index_tail | field_tail | "?" } ;
call_tail   = "(" , [ args ] , ")" ;
index_tail  = "[" , index_list , "]" ;
index_list  = index_item , { "," , index_item } ;
index_item  = [ ident , ":" ] , ( expr | range_expr | ":" ) , { "," , ident , ":" , expr } ;
field_tail  = "." , ( ident | int_lit ) ;
primary     = literal | ident | array_lit | comprehension | tuple
            | if_expr | match_expr | block | closure | "comptime" , expr ;
comprehension = "[" , comp_axis , { "," , comp_axis } , "=>" , expr , "]" ;
comp_axis   = ident , ":" , ident , "in" , range_expr ;
```

---

## 23. Open problems and what I would cut

Stated plainly, because a specification that hides its weak points produces a language that fails at them.

**23.1 Shape inference and error message quality.** The linear arithmetic fragment (section 3.5) is decidable, but the error messages it produces when a shape term fails to unify are the make or break usability question, and no existing system has solved this well. This needs a dedicated diagnostic engine that reports the two conflicting terms and the chain of operations that produced them, not a solver trace.

**23.2 Ownership ergonomics for scientific users.** Warning 3 stands. The mitigations are believed sufficient for numerical code but are unproven for the messy data wrangling that surrounds it. If user testing shows this is the barrier, the fallback is a reference counted mode with a tracing collector for the non embedded targets, which costs the uniform deployment story. This is the single largest design risk in the specification.

**23.3 Compile latency in the REPL.** The interactive loop is what MATLAB gets right and what every compiled competitor has gotten wrong. A sub second budget for a typical cell with full type, shape, and unit inference plus MLIR lowering is aggressive and is not yet demonstrated.

**23.4 Autodiff through foreign code.** A call into LAPACK or cuDNN has a mathematical derivative but no source to transform. Rules are registered by hand for the standard library, which is finite and known. There is no general answer for arbitrary user FFI, and the honest behavior is to make it a compile error rather than a zero.

**23.5 Sparse arrays in the named axis model.** Sparsity interacts awkwardly with named axis broadcasting, and the specification currently treats sparse matrices as a separate type family with a narrower operation set rather than as a layout of the general array. This is a compromise, not a solution.

**23.6 The units and foreign code boundary.** Units are erased before code generation, which is correct for performance, but it means a foreign function cannot enforce them and a value crossing the FFI boundary loses its dimension. The declaration site annotation is the only protection.

**23.7 What I would cut if the schedule demanded it.** In order: the symbolic module (18.14), the uncertainty module (18.11), the GPU targets beyond CUDA, the notebook format, the MATLAB source translator, and `dyn Trait`. In no case would I cut units, named axes, autodiff, or the embedded target, because those four are the entire reason for the language to exist.

---

## 24. Worked example

A complete measurement to model program, to show the pieces together.

```qu
use qu.{dsp, ml, io, plot}

@deterministic
fn main() -> Result[(), Error] {
    # Load. Rate and units come from the file.
    let raw = io.load_tdms("bearing_run_03.tdms")?

    unpack raw as x: Signal[F64<V>, n, 48kHz] {
        # DSP chain. The compiler knows the rate, so no normalized frequencies.
        let clean = x
            |> detrend(order: 1)
            |> bandpass(500Hz, 20kHz, order: 4)
            |> normalize()

        # Time frequency representation. The freq axis carries Hz.
        let s = clean |> stft(window: 25ms, hop: 10ms) |> log_power()

        # Feature selection on the classical path.
        let feats = features(s, [rms, kurtosis, crest, spectral_centroid])
        let sel = ml.mrmr(feats, labels, k: 12)

        # A small network on the same representation.
        model detector(z: Spectrogram[F32, f, t]) -> [F32; class: 4] {
            z |> conv2d(out: 32, kernel: 3) |> relu() |> maxpool(2)
              |> conv2d(out: 64, kernel: 3) |> relu() |> global_pool()
              |> dense(4) |> softmax()
        }

        let (tr, te) = ml.split(s, labels, by: group, frac: 0.8, rng: Rng(seed: 7))
        let trained = ml.fit(detector, tr,
                             loss: cross_entropy,
                             opt: adam(1e-3),
                             epochs: 40,
                             on: gpu0)?

        plot(ml.confusion(trained, te))
        plot(clean |> welch())

        # Deploy the same model, unchanged, to the target.
        @target(cortex_m7, fixed: Q(1,15), tolerance: 1e-3)
        export fn infer(frame: [F64<V>; time: 512 @ 48kHz]) -> [F32; class: 4] {
            frame |> bandpass(500Hz, 20kHz) |> stft_frame() |> trained()
        }
    }

    Ok(())
}
```

The compiler rejects this program if the bandpass exceeds Nyquist, if the STFT frame count does not match the model input, if the fixed point deviation exceeds the tolerance, if the embedded entry point allocates, or if a unit is inconsistent anywhere in the chain. That set of rejections, not the syntax, is the argument for the language.
