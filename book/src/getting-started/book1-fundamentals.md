# Book 1 — Qu Fundamentals

For people who already know MATLAB, NumPy, or both. Every snippet below is
runnable on the reference interpreter today, and its printed output is the
actual output — not a mockup.

## 1. Hello, Qu

```qu
print("Hello, Qu!")
```
```
Hello, Qu!
```

Save that as `hello.qu` and run it:

```bash
cargo run --manifest-path engine/Cargo.toml -p qu-cli -- run hello.qu
```

No `main` function, no imports, no semicolons required. A Qu script is a
sequence of statements, executed top to bottom.

## 2. Variables are implicitly typed by default

You never declare a type. Assign a value, and the runner infers what it is:

```qu
x = 5           # a number
y = 2.5         # also a number (Qu doesn't distinguish int/float at this level)
z = x + y
print("z = {z}")
```
```
z = 7.5
```

This is **`type implicit`** mode, and it is the default — nothing to opt into.
If you want the discipline of declaring a name's shape before using it (useful
in larger scripts, or if you're porting from a strongly-typed background),
turn on **`type explicit`** mode at the top of a script:

```qu
type explicit
x as vector(3)      # declare: x is a 3-element vector
x = [1, 2, 3]        # now assignment is allowed
print("x = {x}")
# y = 5              # would ERROR: `y` is not declared
```

Switch back with `type implicit` (or just don't write `type explicit` at all —
every example in the rest of this book uses the default implicit mode).

## 3. Numbers: plain, with units, and complex

Plain numbers work exactly as you'd expect. Qu also has first-class **unit
literals** — a number followed directly by a unit name normalizes to SI base
units automatically:

```qu
Fs = 5 kHz        # normalizes to 5000
dt = 50m          # bare SI-magnitude prefix: 50 * 1e-3 = 0.05 (no base unit)
print("Fs = {Fs}, dt = {dt}")
```
```
Fs = 5000, dt = 0.05
```

Recognized magnitude prefixes with no base unit: `k` (1e3), `M` (1e6), `G`
(1e9), `m` (1e-3), `u` (1e-6), `p` (1e-12), `a` (1e-18). Recognized physical
units include `Hz kHz MHz GHz`, `s ms us ns`, `V mV kV`, `A mA uA`, `Ohm mOhm
kOhm MOhm`, `F uF nF pF`, `H mH uH`, `W mW kW`, `dB dBm`, `degC`, `rad deg`.

Complex numbers use an imaginary-literal suffix — `i` (math convention) or `j`
(engineering/Python convention) both work and mean the same thing:

```qu
z = 3 + 4j
print("|z| = {abs(z)}")
print("angle = {angle(z):.4f}")
```
```
|z| = 5
angle = 0.9273
```

`abs`, `angle`/`arg`/`phase`, `real`, `imag`, and `conj` are all complex-aware;
called on a real number they just do the obvious thing (`abs` is absolute
value, `angle` is 0 or π, `real` is identity, `imag` is 0).

**A deliberate divergence from Python:** a complex result stays complex even
when its imaginary part is exactly zero (`z * conj(z)` is `Complex`, not
demoted to a plain number) — use `real(...)` to pull out the number. This
matches how NumPy's complex dtype behaves, not how Python's `complex` silently
compares equal to `int`.

## 4. Vectors and ranges

A **range** is `start to stop [step delta]`, inclusive at both ends (unlike
Python's `range`, which excludes the stop):

```qu
v = 1 to 5
print("v = {v}")
w = 0 to 10 step 2
print("w = {w}")
```
```
v = [1, 2, 3, 4, 5]
w = [0, 2, 4, 6, 8, 10]
```

If you'd rather spell out the first two elements and the endpoint (MATLAB-
adjacent, but with an explicit ellipsis so it's never ambiguous), Qu supports
the comma-ellipsis form too — the step is inferred from the gap between the
first two elements:

```qu
c = 1, 2, ..., 10
print("c = {c}")
```
```
c = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
```

A vector literal is just square brackets with commas:

```qu
a = [1, 2, 3]
b = [10, 20, 30]
```

**Indexing is zero-based**, like Python and unlike MATLAB:

```qu
print(a[0])   # 1, not a[1]
```

## 5. Elementwise arithmetic

Ordinary `+`, `-`, `*`, `/` on two 1-D vectors of the same length are
elementwise (this is the *orientation-free* vector shape — see §6 for when
`*` switches to matrix multiplication):

```qu
print("a+b = {a+b}")
print("a.*b = {a .* b}")
print("a.^2 = {a .^ 2}")
```
```
a+b = [11, 22, 33]
a.*b = [10, 40, 90]
a.^2 = [1, 4, 9]
```

`.*`, `./`, `.^` are the *explicitly* elementwise spellings — always
elementwise, never matrix multiplication, matmul-solve, or matrix power, even
on a genuine 2-D matrix. Bare `*` and `^` change meaning once you're working
with an oriented 2-D matrix (next section) — this is the single most important
thing to internalize coming from NumPy, where `*` is always elementwise and
`@` is matmul.

## 6. Matrices: literals, indexing, orientation

A matrix literal uses `;` to separate rows:

```qu
M = [1, 2; 3, 4]
print("M = {M}")
print("M[0,1] = {M[0,1]}")   # 2 — zero-based, row then column
print("M[1,:] = {M[1,:]}")   # a whole row: [3, 4]
```
```
M = [1, 2; 3, 4]
M[0,1] = 2
M[1,:] = [3, 4]
```

`:` alone selects a whole dimension, exactly like MATLAB and NumPy. Slices
include **both** ends, like every other range in the language: `M[0:1, :]` is
rows 0 and 1, and `v[0:4]` is five elements. If you come from Python, this is
the one place to unlearn a habit — `v[0:n]` gives `n + 1` elements here, and
the first `n` are `v[0:n - 1]`.

**At a matrix boundary, `*` is matrix multiplication** (MATLAB's convention,
not NumPy's):

```qu
print("M*M = {M*M}")
```
```
M*M = [7, 10; 15, 22]
```

**Transpose** has three equivalent spellings in each of two families — the
conjugating (Hermitian) ones and the plain ones. For real values they behave
identically; for complex values they differ exactly as in MATLAB:

| Spelling | Conjugates? |
|---|---|
| `A'` | yes |
| `A.H` | yes |
| `ctranspose(A)` | yes |
| `A.'` | no |
| `A.T` | no |
| `transpose(A)` | no |

A 1-D vector gains an explicit orientation the moment you transpose it:

```qu
col = [1, 2, 3].T     # a genuine (3,1) column matrix now, not a bare vector
```

There is deliberately **no** `A^T`/`A^H` spelling, even though you'll see it
in MATLAB. The reason is a real footgun: `^` is the power operator, so `x^T`
is ambiguous between "transpose `x`" and "raise `x` to the power of a
variable named `T`" — and a first implementation of this sugar silently
picked the transpose reading, discarding the variable with no warning:

```qu
T = 3
x = 2
y = x^T
print("y = {y}")   # 8, not 2 — `T` is an ordinary variable here
```
```
y = 8
```

`.T`/`.H` avoid the collision entirely (`.` never means "raise to a power"),
so they're the only field-style spellings Qu supports.

**Matrix power and inverse** live at the same `^` boundary:

```qu
print("M^-1 = {M^-1}")
```
```
M^-1 = [-2, 1; 1.5, -0.5]
```

`M^-1` is the inverse (square + full-rank checked — a clear error otherwise,
never a silent `Inf`/`NaN`), `M^n` for a positive integer `n` is repeated
`matmul`, `M^0` is the identity. There's also a plain `inv(M)` function and,
for non-square or rank-deficient matrices, `pinv(M)` (the Moore-Penrose
pseudoinverse).

**Flipping, mirroring, rotating**:

```qu
v = [1, 2, 3, 4]
print("flip(v) = {flip(v)}")            # [4, 3, 2, 1]

A = [1, 2; 3, 4]
print("fliplr(A) = {fliplr(A)}")        # reverses column order: [2, 1; 4, 3]
print("flipud(A) = {flipud(A)}")        # reverses row order:    [3, 4; 1, 2]
print("rot90(A) = {rot90(A)}")          # 90 deg counterclockwise: [2, 4; 1, 3]
print("rot90(A, -1) = {rot90(A, -1)}")  # negative k rotates clockwise instead
```

`mirror(x)` is an alias for `fliplr(x)` (the more common colloquial name for
the same operation). `flip(A, dim)` picks between the two on a matrix —
`dim=1` (the default) is `flipud`, `dim=2` is `fliplr`; on a plain vector
`dim` has no effect since there's only one axis to reverse either way.
`rot90(A, k)` takes `k` mod 4, so `k=2` is a 180-degree flip and `k=4` is the
identity.

### Orientation-free vectors vs. explicit shape

A bare `Vec` like `a = [1, 2, 3]` has **no** orientation — it's neither a row
nor a column, and elementwise ops don't care. It only gains an orientation
when it meets a matrix boundary, and the rule there is: **default to a
column**. This is why `M * a` treats `a` as a `(3,1)` column automatically,
but two orientation-free vectors multiplied together at a `*` boundary can
give a shape-mismatch error if that default guess doesn't fit your intent —
fix it with an explicit contract:

```qu
u = [1, 2, 3]
v = [10, 20]
u as vector(3, 1)     # column
v as vector(1, 2)     # row
outer = u * v          # (3,1) * (1,2) -> (3,2) outer product
```

This orientation contract can also declare **and** initialize in one line:

```qu
y as vector(2, 2) = [1, 2, 3, 4]   # reshaped column-major into a 2x2
```

## 7. Logical indexing (no `find`/boolean-mask ceremony needed)

Comparisons on a vector produce a **mask**, and masks index directly — this
is NumPy's boolean-indexing idiom, built into the language rather than bolted
on as a library feature:

```qu
x = -3 to 3
x[x < 0] = 0
print("relu = {x}")

idx := where x > 1
print("idx = {idx}")
```
```
relu = [0, 0, 0, 0, 1, 2, 3]
idx = [5, 6]
```

`where(mask)` returns the integer positions where the mask is true —
equivalent to NumPy's `np.where(cond)[0]` or MATLAB's `find(cond)`.

## 8. Control flow

`for`/`while`/`if` all close with an explicit `end` — no significant
whitespace (unlike Python), no curly braces (unlike C-family languages):

```qu
acc = 0
for k = 1 to 10
    acc += k
end for
print("sum 1..10 = {acc}")

n = 5
if n mod 2 == 0 then
    print("even")
else
    print("odd")
end if
```
```
sum 1..10 = 55
odd
```

`mod` is the keyword for remainder (not `%`). `+=`, `-=`, `*=`, `/=` all work
as compound assignment, including on indexed/masked targets (`x[mask] += 1`).

## 9. Functions: three forms, declaration always optional

**One-line functions** use `:=` — "is defined as." They take a single
expression body, are non-recursive, and are *elemental*: call one on a vector
and it maps over every element automatically (FORTRAN's `ELEMENTAL`, minus the
ceremony):

```qu
square(x) := x^2
print("square(5) = {square(5)}")
print("square(v) = {square(v)}")
```
```
square(5) = 25
square(v) = [1, 4, 9, 16, 25]
```

**Multi-statement functions** (control flow, recursion, early `return`) use
`function … end function`:

```qu
function fact(n)
    if n < 2 then
        return 1
    end if
    return n * fact(n - 1)
end function
print("fact(5) = {fact(5)}")
```
```
fact(5) = 120
```

There is no `def`/`function` *keyword requirement* to introduce a name before
using it, and no forward-declaration step — a function is simply a statement
that can appear anywhere a statement can, and it's visible from that point
on, same as an ordinary variable assignment.

## 10. Strings and interpolation

Double-quoted strings interpolate `{expr}` directly, with an optional
`:spec` — `.Nf` fixed-point, `.Ne` scientific, `d` integer:

```qu
name = "Qu"
print("Hello, {name}!")
print("pi = {pi:.4f}")
```
```
Hello, Qu!
pi = 3.1416
```

`pi`, `tau` (2π), `e`, `inf`, and `nan` are built-in constants — no `import
math` required.

## 11. Reductions and a first taste of DSP

`sum`, `mean`, `std`, `rms`, `max`, `min`, `prod` all work on vectors, and
accept an `axis=` keyword on matrices (`axis=0` collapses rows, `axis=1`
collapses columns — same convention as NumPy):

```qu
print("mean = {mean(v)}, std = {std(v):.4f}, rms = {rms(v):.4f}")
G = [1, 2, 3; 4, 5, 6]
print("colsum = {sum(G, axis=0)}")
print("rowsum = {sum(G, axis=1)}")
```
```
mean = 3, std = 1.5811, rms = 3.3166
colsum = [5, 7, 9]
rowsum = [6, 15]
```

`min`/`max` with **two** arguments are elementwise (the clip idiom), not a
reduction:

```qu
clipped = min(max(x, -1), 1)   # same as clip(x, -1, 1)
```

`argmin`/`argmax` give you the **index** of the extremum instead of its
value — the index-returning sibling of `min`/`max`, matching `where`'s own
existing "return indices" convention:

```qu
y = [5, 2, 9, 1, 1, 9]
print("argmin(y) = {argmin(y)}, argmax(y) = {argmax(y)}")   # 3, 2 (first tie wins)
```

`median`/`quantile` round out the order statistics `sum`/`mean`/`std`
don't cover; `argmedian`/`argquantile` are their index-returning versions
(needed because, unlike `min`/`max`, a "value" like the median or a
quantile doesn't always land on any single array element — see each
function's own note on how the tied/interpolated case is resolved):

```qu
q1 = quantile(y, 0.25)
q3 = quantile(y, 0.75)
print("median = {median(y)}, IQR = {q3 - q1}")
```

There's no `argmean` in NumPy or MATLAB — the mean generally isn't any
actual data point — but Qu's own `argmean(x)` is still a well-defined,
useful question: *which* sample sits closest to the mean.

`sort(x, [descending])` and its index-returning sibling `argsort(x,
[descending])` both default to ascending, matching `sort_by(table, col,
[descending])`'s own positional-boolean convention rather than a
`desc=`/`order=` keyword:

```qu
y = [5, 2, 9, 1, 1, 9]
print("sort ascending  = {sort(y)}")
print("sort descending = {sort(y, true)}")
print("argsort(y) = {argsort(y)}")   # the permutation that would sort y
```
```
sort ascending  = [1, 1, 2, 5, 9, 9]
sort descending = [9, 9, 5, 2, 1, 1]
argsort(y) = [3, 4, 1, 0, 2, 5]
```

## 12. Save and load your workspace

`save`/`save_all`/`load` persist workspace variables to a plain JSON file —
human-readable, diffable, and portable across platforms (no pickle-style
binary format tied to a specific Qu build):

```qu
x = 1 to 5
A = [1, 2; 3, 4]
m = ols_model([1;2;3;4], [2;4;6;8])   # a fitted model round-trips too
save_all("session.json")              # every variable currently in scope

# ...later, in a fresh session...
load("session.json")                  # restores x, A, m into the workspace
p = predict(m, [5])                   # -> [10]
```

Use `save(path, "name1", "name2", ...)` instead of `save_all` to persist only
specific variables:

```qu
save("results.json", "x", "A")
```

Every `Value` Qu can hold round-trips — numbers, vectors, matrices, complex
numbers and matrices, signals (their sample rate `Fs` survives too), tables,
and fitted models. `NaN`/`Infinity`/`-Infinity` round-trip as real IEEE
floats, not `null` — a plain `nan != nan` check after a `load` still comes
back `true`, exactly as it would for a value that was never saved at all.

`Worker`/`Mutex`/`Semaphore` handles are live OS resources, not data, so
`save`/`save_all` refuse to serialize them with a clear error rather than
silently producing a file that can never be loaded back into a working
state.

## 13. Plotting

```qu
plot(v, v .^ 2)
xlabel("x")
ylabel("x^2")
grid on
savefig("squares.pdf")
```

`savefig` writes PDF with the fonts embedded, SVG, or TikZ for LaTeX,
chosen by the extension. A script that never calls it still runs — the
figure is built and reported, which is what you want in a test or on a
machine with no display — so the same script works in CI and on your desk.

The [plotting reference](../stdlib/plotting.md) has the whole surface.
`show plot` renders in the REPL and in Qu Studio.

`polarplot(theta, r)` converts to Cartesian (`x = r*cos(theta)`,
`y = r*sin(theta)`) and draws a correctly shaped polar curve — but on
ordinary axes, with no circular gridlines or angular tick labels:

```qu
idx = 0 to 100
theta = idx ./ 100 .* (2*pi)
r = ones(101)
polarplot(theta, r)   # a unit circle, drawn as an (x, y) curve
```

`axis origin` moves the axis spines themselves to cross through data
`(0, 0)` — clamped to the nearest edge if `0` falls outside the visible
range — instead of framing the panel in the default box (`axis edge`):

```qu
xs = -5 to 5
plot(xs, xs.^2 - 10)
axis origin   # spines cross at (0,0) instead of boxing the panel
```

## 14. Splitting a program across files

Once a program is more than one screen, or once two programs want the same
helper, put the helper in its own file and `import` it.

```qu
# geometry.qu
PI_ISH = 3.14159

function area(r)
    return PI_ISH * r * r
end function
```

```qu,ignore
# main.qu
import "geometry.qu"
print(area(2))            # 12.56636
```

The path is relative **to the file doing the importing**, not to wherever
you happened to run the program from. A module in `tools/lib/` can name its
neighbour as `"text.qu"`, and a tool in `tools/` can say
`import "lib/text.qu"` and still work when you run it from somewhere else.
Data paths are the opposite and deliberately so: `read_csv("data.csv")`
names a file relative to where *you* are standing, because that is a file
you chose at the moment you ran the thing. An import is a fixed
relationship between two source files.

A file runs **once per path**, however many times it is imported. That is
what makes two files that import each other terminate rather than spin, and
it is why a module's top level should *define* things rather than *do*
them — the second import will not re-run it.

### `as` names the namespace

```qu,ignore
import "geometry.qu" as g
print(g.area(2))          # 12.56636
print(area(2))            # 12.56636 -- the bare name too
print(PI_ISH)             # 3.14159
```

An import **opens the file's names and binds a namespace**, the way
`Imports System.Text` in VB gives you `StringBuilder` bare and
`System.Text.StringBuilder` qualified. Both spellings work; the qualified
one is there for when the bare one is not clear enough, not as a
replacement for it.

For a file, `as` is what supplies the namespace — without it there is no
name to qualify with, because a path has no obvious one (`"lib/text.qu"`
could be `text`, or `lib`, or neither). A native module already has a name,
so it binds one whether you alias it or not.

### Two mechanisms, one keyword

A quoted path imports a **Qu file**. A bare word imports a **native
module** compiled into the engine:

```qu,ignore
import codec
info = flac_info(bytes)          # opened by the import
info = codec.flac_info(bytes)    # or qualified
```

They follow the same rule now and differ in three details:

| | `import "file.qu"` | `import name` |
|---|---|---|
| What it is | another Qu source file | a module built into the engine |
| Namespace | only if you write `as` | always, under its own name |
| Name collision | shadows, like any definition | **refused as ambiguous** |
| Availability | any file you can read | only if the build included it |

### When a name is claimed twice

`import xlsx` opens `read` and `write`, and both are already builtins. Qu
refuses rather than choosing:

```
`read` is ambiguous: it is a builtin, and `import xlsx` opened a `read`
too. Qualify the one you mean -- `xlsx.read(...)` for the module's, or
drop the import to get the builtin back
```

The alternative is a program whose meaning changes because of a line at the
top of the file, silently. Qualifying always works, so there is always an
answer.

A **file** import behaves differently here, and deliberately: a file that
defines `sqrt` shadows the builtin, because running the file is what an
import does and defining a function named `sqrt` shadows it whether an
import brought it or not. A native module defines nothing — it opens a
namespace — so a collision there has two equally good readings and nothing
to choose between them.

A native module the build does not have says so plainly, rather than
failing later at the first call:

```
import xlsx: this build does not include the `xlsx` module
             -- rebuild with `--features xlsx`
```

and a bare word that is not a module at all points you at the other form:

```
import nope: no such module. This build knows: xlsx, codec.
             To import a Qu file instead, quote the path: `import "nope.qu"`
```

### What this is for

The site generators under `tools/` are the honest example. Nine of them had
grown five copies of an HTML-escaping function, three of a backtick
stripper, and two of a syntax highlighter — and the copies had begun to
drift. One generator ends up reading finished HTML back off disk purely
because it could not call the highlighter that produced it.

`tools/lib/text.qu` is now the shared copy, and `tools/gen_builtin_index.qu`
imports it. That is a small thing, but it is the difference between a
language you can build a library in and one where every program starts from
the builtins.

## 15. Cheat sheet: coming from MATLAB

| MATLAB | Qu | Note |
|---|---|---|
| `A(1,2)` | `A[0,1]` | zero-based |
| `A(2,:)` | `A[1,:]` | |
| `A'` | `A'` | same — conjugate transpose |
| `A.'` | `A.'` | same — plain transpose |
| `A*B` | `A*B` | same — matrix multiply |
| `A.*B` | `A.*B` | same — elementwise |
| `A^-1` | `A^-1` | same |
| `inv(A)` | `inv(A)` | same |
| `1:5` | `1 to 5` | inclusive both ends, like MATLAB |
| `for k = 1:10 ... end` | `for k = 1 to 10 ... end for` | |
| `function y = f(x) ... end` | `function f(x) ... return y ... end function` | return is explicit |
| `x = 5;` (semicolon suppresses echo) | `x = 5` (no echo by default) | no semicolon needed |
| `%` comment | `#` comment | |

## 16. Cheat sheet: coming from NumPy/Python

| NumPy/Python | Qu | Note |
|---|---|---|
| `a[0]` | `a[0]` | same — zero-based |
| `a * b` (elementwise) | `a .* b` at a matrix boundary; plain `*` for 1-D vectors | **`*` is matmul once 2-D**, unlike NumPy |
| `a @ b` (matmul) | `a * b` (once oriented) | |
| `np.arange(0, 10, 2)` | `0 to 10 step 2` | inclusive of the stop, unlike `arange` |
| `a[a < 0] = 0` | `a[a < 0] = 0` | identical idiom |
| `np.where(cond)[0]` | `where cond` or `where(cond)` | |
| `def f(x): return x**2` | `f(x) := x^2` | one-liner; use `function` for a body |
| `f"{x:.2f}"` | `"{x:.2f}"` | same format-spec mini-language |
| `3+4j` | `3+4j` (or `3+4i`) | both suffixes accepted |
| `a.T` | `a.T` (or `a'`, `a.'`) | not `a^T` — see §6's footgun note |
| assignment aliases (`b = a` shares the array) | assignment copies | Qu vectors/matrices have **value semantics** — no accidental aliasing bugs from `b = a; a[:] = ...` |

## 17. What's different, and why it might surprise you

- **`*` is matrix multiplication at a 2-D boundary**, not elementwise. This is
  MATLAB's convention, chosen deliberately (§13 of the spec) because Qu treats
  linear algebra as load-bearing, not an afterthought. Use `.*` when you mean
  elementwise.
- **Indexing is zero-based**, not one-based — MATLAB users, take note.
- **Assignment has value semantics.** `b = a` does not alias `a`; mutating one
  never mutates the other. This is closer to how you'd *want* NumPy to behave
  than how it actually behaves (NumPy slices are views by default).
- **A bare 1-D vector has no orientation** until it meets a matrix boundary,
  where it defaults to a column. If your outer product or matmul isn't doing
  what you expect, add an explicit `as vector(r, c)` contract.
- **Complex values don't silently demote to real** even when their imaginary
  part is exactly zero — call `real(...)` explicitly.

## What's next

Move on to **[Book 2 — Numerics, DSP, and Linear Algebra](book2-numerics-dsp.md)**
for FFT, spectra, matrix decompositions, and the crest-factor / smooth-max
tools this language was built around.
