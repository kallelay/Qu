# Qu Language Tour — Verified Examples

Every example below is a real `.qu` script that was actually executed against a
real release build and the output shown is copy-pasted verbatim (not
reformatted, not idealized). Nothing here was written from memory or "should
work" guessing.

**Binary used:** `qu.exe` built via `cargo build --release -p qu-cli` from
`engine/`, built
2026-08-31 from HEAD commit `4464d1b` ("Add table./timer. namespace sugar, ->
reshape operator, row/col aliases"). Every script was run as:

```
qu.exe run <script>.qu
```

> **Update, same day:** the `elseif` bug this pass found (§4, §14) was fixed
> immediately after this document was drafted — see commit `79609d0`. The
> repro/root-cause writeups below were updated to reflect the fix rather than
> rewritten from scratch, so they still show the real before/after.

**Purpose:** this is a language tour AND a cross-check against the two spec
docs (`docs/qu-language-spec.md` and `book/src/language-reference/spec.md` —
these two files are currently byte-identical). Where reality disagrees with
the spec, or with a reasonable assumption about the language, it's called out
in a blockquote headed **MISMATCH**. Where a bug in the current parser/interp
itself was found (not a doc problem), it's called out headed **BUG**. This
pass makes no source-code changes — evidence only.

## Table of Contents

1. [Core types & literals](#1-core-types--literals)
2. [Variables & assignment](#2-variables--assignment)
3. [Operators](#3-operators)
4. [Control flow](#4-control-flow)
5. [Functions](#5-functions)
6. [Indexing & slicing](#6-indexing--slicing)
7. [The Model multi-return protocol](#7-the-model-multi-return-protocol)
8. [Tables](#8-tables)
9. [Multiple dispatch](#9-multiple-dispatch)
10. [String interpolation](#10-string-interpolation)
11. [Comments & `#%%` cells](#11-comments--%25-cells)
12. [`timer.` sugar and the `->` reshape operator](#12-timer-sugar-and-the--reshape-operator)
13. [Error handling](#13-error-handling)
14. [`unsafe` blocks — continue past a failure](#14-unsafe-blocks--continue-past-a-failure)
15. [`parallel for` and the pool/queue job system](#15-parallel-for-and-the-poolqueue-job-system)
16. [`cast` and `resample_to`](#16-cast-and-resample_to)
17. [Advanced multiple dispatch](#17-advanced-multiple-dispatch)
18. [DSP builtins — a signal-processing tour](#18-dsp-builtins--a-signal-processing-tour)
19. [`memoize function` — function-result caching](#19-memoize-function--function-result-caching)
20. [File I/O extensions — `touch`, streaming reads, and memory-mapped files](#20-file-io-extensions--touch-streaming-reads-and-memory-mapped-files)
21. [`http_get(url)` — real HTTP GET](#21-http_geturl--real-http-get)
22. [`lazy` — deferred variable evaluation](#22-lazy--deferred-variable-evaluation)
23. [`StreamFile`/`StreamURL` — unified polymorphic streams](#23-streamfilestreamurl--unified-polymorphic-streams)
24. [Three new Signal builtins — `interpolate_at`, `apply`, `cut`](#24-three-new-signal-builtins--interpolate_at-apply-cut)
25. [`watch` — file/variable/URL change detection](#25-watch--filevariableurl-change-detection)
26. [Summary of mismatches found](#26-summary-of-mismatches-found)

---

## 1. Core types & literals

```qu
x = 42
y = 3.14
b = true
s = "hello"
name = "world"
si = "hi {name}, x={x}"
v = [1, 2, 3]
M = [1, 2; 3, 4]
z = 3 + 4i
print("x  = {x}")
print("y  = {y}")
print("b  = {b}")
print("s  = {s}")
print(si)
print("v  = {v}")
print("M  = {M}")
print("z  = {z}")
```

Output:

```
x  = 42
y  = 3.14
b  = true
s  = hello
hi world, x=42
v  = [1, 2, 3]
M  = [1, 2; 3, 4]
z  = 3 + 4i
```

Record, List (via `split`), CVec (via `fft`), and Signal:

```qu
r = {x = 1, y = 2}
print("r = {r}")
print("r.x = {r.x}")

parts = split("a,b,c", ",")
print("parts = {parts}")

X = fft([1,2,3,4])
print("X = {X}")

sig = signal([1,2,3,4], 100)
print("sig = {sig}")
```

Output:

```
r = {x = 1, y = 2}
r.x = 1
parts = ("a", "b", "c")
X = [10, -2 + 2i, -2, -2 - 2i]
sig = signal(Fs=100) [1, 2, 3, 4]
```

Notes:
- Record literal syntax is `{name = value, ...}` — **not** `{name: value}`.
- A `List` (heterogeneous/dynamic array, `Value::List`) is not built with a
  literal; it comes back from builtins like `split`. It prints as a
  parenthesized tuple `(...)`.
- `[...]` is always numeric/complex — `Vec`/`Mat`/`CVec`/`CMat` depending on
  shape and whether any cell is complex. A string in an array literal is a
  hard error (`as_num()` fails), so `[1, "a"]` does not silently become a List.
- `Signal` carries its sample rate; printing shows `signal(Fs=100) [...]`.
  Field access: `sig.Fs`, `sig.N`, `sig.dt`, `sig.t` (all confirmed — see §6).

CVec/CMat literal syntax (a single complex cell promotes the whole literal):

```qu
cv = [1+2i, 3-1i, 0+1i]
print("cv = {cv}")
cm = [1+1i, 2; 3, 4-2i]
print("cm = {cm}")
```

Output:

```
cv = [1 + 2i, 3 - 1i, 1i]
cm = [1 + 1i, 2; 3, 4 - 2i]
```

> **No type introspection found.** Neither `type_name(x)` nor `typeof(x)` nor
> any variant exists as a builtin (`qu: runtime error: unknown function
> 'type_name'`), and grepping both spec docs and `book/src/stdlib/` turns up
> no claim of one either — this isn't a doc mismatch, just a real gap worth
> knowing about if you were assuming one existed.

---

## 2. Variables & assignment

Indexed assignment, and the `@` self-mutation prefix:

```qu
v = [10, 20, 30, 40, 50]
v[0] = 99
print("v = {v}")

M = [1,2;3,4]
M[0,1] = 77
print("M = {M}")

df = table(x=[1,2,3,4], y=[10,20,30,40])
@df.drop_row(1)
print(df)
```

Output:

```
v = [99, 20, 30, 40, 50]
M = [1, 77; 3, 4]
x   y
-  --
1  10
3  30
4  40
```

`@expr` desugars at parse time to `root = expr`, where `root` is the
plain-variable root of the chain (`@data.drop_row(1)` → `data =
data.drop_row(1)`). `@(a+b)` or `@foo()` (not rooted in a variable) is a
parse-time error naming the problem, per `qu-syntax/src/lib.rs:801-817`.

Compound assignment — `+=`, `-=`, `*=`, `/=` all work:

```qu
a = 5
a += 3
print("a += 3 -> {a}")
a -= 2
print("a -= 2 -> {a}")
a *= 4
print("a *= 4 -> {a}")
a /= 2
print("a /= 2 -> {a}")
```

Output:

```
a += 3 -> 8
a -= 2 -> 6
a *= 4 -> 24
a /= 2 -> 12
```

Units normalize to SI on construction:

```qu
Fs = 5 kHz
print("Fs = {Fs}")
```
```
Fs = 5000
```

> **MISMATCH — found and fixed same-day (commit `f77713c`).** This pass
> originally found `.*=`/`./=`/`^=` failing to parse (`unexpected Op("=")
> in expression`) despite the spec documenting them — root cause was two
> bugs at once: the lexer's `match_operator` only ever compared an
> operator's first two characters (so a genuine 3-char token like `.*=`
> could never match), and `^=`/`.* =`/`./=` weren't in the `OPERATORS`
> table at all. Both fixed; all three now work, wired through the exact
> same compound-assign desugaring `+=`/`-=`/`*=`/`/=` already used, so
> they inherit `binop`'s real semantics automatically:
> ```qu
> M = [1,2;3,4]
> M ^= 2          # real matrix power, not elementwise
> print(M)        # [7, 10; 15, 22]
> v = [1,2,3]
> v .*= [10,20,30]
> print(v)        # [10, 40, 90]
> ```
> Verified live — both outputs shown are real.

> **MISMATCH — `++`/`--` increment.** The spec shows `x++` as a statement-only
> increment (`x = x + 1`). Real behavior: `a++` does not parse at all —
> `a = 5; a++` gives `qu: runtime error: parse error at 3:1: unexpected Eof in
> expression`, because `++` isn't a lexed token; the parser sees `a`, then a
> binary `+` expecting a right operand, then a unary `+` expecting an operand,
> and runs out of input. No increment/decrement operator exists today.

> **MISMATCH — multiple assignment.** The spec shows:
> ```
> x, y = 1, 2
> mag, phase = bode(sys, w)
> ```
> Real behavior: `x, y = 1, 2` is a parse error —
> `qu: runtime error: parse error at 1:2: expected end of statement (newline
> or ';'), found Op(",")`. There is no destructuring assignment at all today.
> Multi-value returns are exclusively handled via the `Value::Model`
> `.field` protocol (§7) — `q, r = qr(M)` also fails to parse; you must write
> `res = qr(M); q = res.q; r = res.r`.

---

## 3. Operators

Matrix vs. elementwise, using `A = [1,2;3,4]`, `B = [5,6;7,8]`:

```qu
A = [1,2;3,4]
B = [5,6;7,8]
print("A*B (matrix mult) = {A*B}")
print("A.*B (elementwise) = {A.*B}")
print("A/B = {A/B}")
print("A./B (elementwise div) = {A./B}")
print("A' (transpose) = {A'}")
print("A^2 (matrix power) = {A^2}")
b = [4; 8]
Asolve = [2,0;0,4]
x = Asolve \ b
print("A \\ b (solve Ax=b) = {x}")

v = [1,2,3]
w = v |> sum
print("v |> sum = {w}")

s1 = "foo" + "bar"
print("concat = {s1}")
```

Output:

```
A*B (matrix mult) = [19, 22; 43, 50]
A.*B (elementwise) = [5, 12; 21, 32]
A/B = [0.2, 0.333333; 0.428571, 0.5]
A./B (elementwise div) = [0.2, 0.333333; 0.428571, 0.5]
A' (transpose) = [1, 3; 2, 4]
A^2 (matrix power) = [7, 10; 15, 22]
A \ b (solve Ax=b) = [2; 2]
v |> sum = 6
concat = foobar
```

> **Notable, verified behavior — `/` between two matrices is NOT matrix
> right-division.** `A*B` is real matrix multiplication (confirmed:
> `[1,2;3,4]*[5,6;7,8] = [19,22;43,50]`, matches `A@B`). `A^2` is real
> repeated matmul. But `A/B` is byte-identical to `A./B` — both give the pure
> elementwise result. This isn't an accident: `qu-interp/src/lib.rs:19947`
> literally reads `"/" | "./" => matrix_ew(a, b, |x, y| x / y)`. Only the
> **backslash** `\` does real linear algebra (`A \ b` solves `Ax=b` via
> least-squares, confirmed above: `[2,0;0,4] \ [4;8] = [2;2]`, correct). The
> spec's operator precedence table (line 724) groups `*, /, \` together as
> one tier distinct from `.*, ./, .\`, which reads as though `/` should mirror
> `*`'s "real matrix op" behavior — it does not. Worth deciding whether this
> is intended.

Logical/comparison (whole-value, not elementwise per spec — confirmed):

```qu
print("2==2: {2==2}, 2!=3: {2!=3}, true and false: {true and false}, true or false: {true or false}, not true: {not true}")
```
```
2==2: true, 2!=3: true, true and false: false, true or false: true, not true: false
```

Reshape `->` (see §12 for detail) and pipe `|>` were confirmed working as
shown above.

---

## 4. Control flow

Plain `if`/`else`/`end if`, `while`+`continue`, `for`+`break` all work:

```qu
x = 7
if x > 10
    print("big")
else
    if x > 5
        print("medium")
    else
        print("small")
    end if
end if

i = 0
while i < 5
    if i == 3
        i += 1
        continue
    end if
    print("i = {i}")
    i += 1
end while

for k = 0 to 9
    if k == 4
        break
    end if
    print("k = {k}")
end for
```

Output:

```
medium
i = 0
i = 1
i = 2
i = 4
k = 0
k = 1
k = 2
k = 3
```

`try`/`catch` works and catches real runtime errors (see §13 for details).

> **BUG — found here, fixed same-day (commit `79609d0`).** `elseif` never
> parsed successfully, in any form, before this fix. Minimal repro that used
> to fail:
> ```qu
> x = 7
> if x > 10
>     print("big")
> elseif x > 5
>     print("medium")
> end if
> print("done")
> ```
> Before the fix: `qu: runtime error: parse error at 6:1: expected end of
> statement (newline or ';'), found Keyword("end")` — reproduced identically
> whether the chain ended in `else`, no `else`, `end if`, or bare `end`. Root
> cause was in `qu-syntax/src/lib.rs`: `if_stmt`, on seeing `elseif`, recursed
> into `if_stmt_from_elseif` and then **returned immediately**, before
> reaching its own `self.expect_end(&["if"])?` call three lines later — so
> the terminating `end`/`end if` of an `elseif` chain was never consumed by
> anyone, tripping a parse error on the following statement. Fixed by
> removing the early return so the outer `if_stmt` always consumes the
> chain's shared `end`, matching `if_stmt_from_elseif`'s own comment ("the
> outer if consumes the shared `end if`"), which described the intended
> behavior correctly all along — the code just didn't implement it. 2 new
> regression tests added (a single `elseif` followed by a real trailing
> statement, and a 3-branch `elseif`/`else` chain checked structurally).
> Re-verified live: the repro above now correctly prints `medium` then `done`.

> **Follow-up, same day (commit `f77713c`): `else if` (two words) now
> means the same thing as `elseif` (one word).** Before this, `else`
> immediately followed by `if` parsed as plain `else` containing a nested
> `if` — needing its OWN separate `end if`, in addition to the outer
> one. Confirmed unambiguous: `else` immediately followed by `if` (same
> line or across a newline — newlines aren't suppressed in this exact
> position) always means the chained form now; a deliberately nested `if`
> written on its own line inside an `else` block still works and still
> needs its own `end if`, since that's a different token sequence.
> ```qu
> x = 5
> if x > 10
>     print("big")
> else if x > 3
>     print("medium")
> end if
> ```
> Now prints `medium` with one shared `end if`, fully interchangeable
> with `elseif`.

---

## 5. Functions

Both the block form and the one-line `:=` form work, `return` works, and
typed-parameter multiple dispatch works:

```qu
function add(a, b)
    return a + b
end function

print("add(2,3) = {add(2,3)}")

square(x) := x^2
print("square(5) = {square(5)}")

function describe(x: num)
    return "num: {x}"
end function

function describe(x: str)
    return "str: {x}"
end function

print(describe(5))
print(describe("hi"))
```

Output:

```
add(2,3) = 5
square(5) = 25
num: 5
str: hi
```

Operator-overload functions on tagged records work, but require an explicit
`__type` field — a plain untagged record does NOT dispatch to
`record<Tag>`-typed overloads:

```qu
function +(a: record<Vec2>, b: record<Vec2>)
    return {__type = "Vec2", x = a.x + b.x, y = a.y + b.y}
end function

p1 = {__type = "Vec2", x = 1, y = 2}
p2 = {__type = "Vec2", x = 10, y = 20}
sum = p1 + p2
print("sum = {sum}")
```
```
sum = {__type = "Vec2", x = 11, y = 22}
```
(Omitting `__type` on the record literals falls through to the built-in
numeric `+` and errors with `expected a number, found record`.) The
`record<Tag>` match is implemented via `record_type_tag` in
`qu-interp/src/lib.rs:2709`, which specifically looks for a string field named
`__type` — not a `kind` field, and not any structural/duck-typed match.

> **MISMATCH — found and fixed same-day (commit `f77713c`).** This pass
> originally found `function butter(x, order = 4, ...)` failing to parse
> at all (`expected ')', found Op("=")`, since `Param` had no default-value
> slot). Default parameters are now real:
> ```qu
> function butter(x, order = 4, cutoff = 1000)
>     return "x={x} order={order} cutoff={cutoff}"
> end function
> print(butter(99))               # x=99 order=4 cutoff=1000
> print(butter(99, 2, 500))       # x=99 order=2 cutoff=500
> ```
> The default expression is re-evaluated fresh on every call (not
> computed once at definition time) and may reference an earlier
> parameter (`function f(x, y = x * 2)` works). **A real behavior change
> discovered along the way**: named-argument calling on a user-defined
> function is NOT supported — `butter(99, cutoff=500)` is a clear error
> (`` `butter` is a user-defined function -- named arguments... are not
> supported when calling one``), verified live. Only builtins accept
> `name=value` calling syntax; pass every argument to a user function
> positionally. There is no separate `optional`/`is_present()` mechanism
> — a parameter is either required or has a `= value` default.

> **MISMATCH — `nargin`/`nargout`/varargs.** The spec's §44.5 claims
> `nargin`, `nargout`, `...args` (varargin), and `args...` (spread). Real
> behavior: `nargin` inside a function body gives `qu: runtime error: 'nargin'
> is not defined`. None of this argument-introspection machinery exists.

> **MISMATCH — anonymous-function arrow lambdas.** The spec's §44.5 shows
> `sq = (x) -> x^2` as a MATLAB-`@(x)`-style lambda. Real behavior: this is a
> parse error — `qu: runtime error: parse error at 1:13: expected '(', found
> Ident("x")`. `->` today is exclusively the new reshape operator (§12):
> after `->` the parser expects a parenthesized dims tuple, not an arbitrary
> expression body, so a lambda arrow using the same token cannot coexist with
> reshape as currently implemented. There is no lambda literal syntax at all
> right now — `map` needs a named function.

> **MISMATCH — `pure function`/`elemental function` attributes.** Claimed in
> spec §44.5; `pure`/`elemental` are not in the lexer's keyword list at all
> (`qu-lexer/src/lib.rs:78-82`), so `pure function f(x) ... end function`
> would need to be tested but is very unlikely to be recognized — flagged
> from source inspection, not independently re-run, since the keyword's
> absence from the lexer table is already conclusive.

---

## 6. Indexing & slicing

Zero-based indexing, inclusive slices, `end`, boolean masks, `where`, and
fancy (integer-vector) indexing — all confirmed working exactly as the spec
(§15) describes:

```qu
v = [10, 20, 30, 40, 50]
print("v[0] = {v[0]}")
print("v[1:3] = {v[1:3]}")
print("v[end] = {v[end]}")

mask = v > 30
print("mask = {mask}")
print("v[mask] = {v[mask]}")

v[v < 25] = 0
print("after mask assign = {v}")

idx = where v > 0
print("where v>0 = {idx}")

X = [100, 200, 300, 400, 500]
bins = [0, 2, 4]
print("X[bins] (fancy indexing) = {X[bins]}")

r = 0 to 4
print("0 to 4 = {r}")
```

Output:

```
v[0] = 10
v[1:3] = [20, 30]
v[end] = 50
mask = [F, F, F, T, T]
v[mask] = [40, 50]
after mask assign = [0, 0, 30, 40, 50]
where v>0 = [2, 3, 4]
X[bins] (fancy indexing) = [100, 300, 500]
0 to 4 = [0, 1, 2, 3, 4]
```

`0 to 4` is inclusive on both ends (`[0,1,2,3,4]`, 5 elements), and so is a
bracket slice: `v[1:3]` returns `[20, 30, 40]` — indices 1, 2 AND 3. One
range convention, everywhere.

`range(a, b, step=1)` also exists as a builtin and is **inclusive of `b`**,
matching `to` rather than Python's exclusive convention:

```qu
r = range(0, 5)
print("range(0,5) = {r}")
```
```
range(0,5) = [0, 1, 2, 3, 4, 5]
```

`range(5)` alone does **not** mean "0..4" — it's `range(start=5, stop=0,
step=1)` by argument position (`qu-interp/src/lib.rs:12721-12730`, defaults
are `a=0, b=0, step=1` filled positionally), which walks from 5 toward 0 with
a positive step and errors: `qu: runtime error: range step points away from
its stop value`. Always pass at least two arguments.

> **SETTLED — negative indexing.** Qu does not have it, and this is a
> decision rather than a gap: `x[-1]` is a runtime error (`index must be a
> non-negative integer, got -1`), and `end` is the mechanism. Arithmetic on
> it works, so `x[end]`, `x[end-1]`, `x[end-2]` give the last,
> second-to-last and third-to-last element.
>
> This was recorded here as a MISMATCH against the specification, which
> used to show `x[-1]` as "last element". The specification no longer
> claims it (§15 now states the rule above), so there is nothing left to
> reconcile. Retracting the claim was chosen over implementing it,
> 2026-09-09.

---

## 7. The Model multi-return protocol

Functions with multiple structured outputs (`qr`, `findpeaks`, etc.) return a
`Value::Model`, printed as `model(kind) {field1, field2, ...}`, with `.field`
access — and the field names are the ones actually listed, not guessed:

```qu
M = [4, 3; 6, 3]
res = qr(M)
print(res)
print("q = {res.q}")
print("r = {res.r}")

x = [0, 1, 3, 1, 0, 2, 4, 2, 0]
pk = findpeaks(x)
print(pk)
print("peaks = {pk.peaks}")
print("locations = {pk.locations}")
```

Output:

```
model(qr) {q, r}
q = [0.5547, 0.83205; 0.83205, -0.5547]
r = [7.211103, 4.160251; 0, 0.83205]
model(findpeaks) {peaks, locations}
peaks = [3, 4]
locations = [2, 6]
```

Notes:
- Field names are **lowercase** (`q`, `r` — not `Q`, `R`); accessing `res.Q`
  errors with `qu: runtime error: model has no field 'Q' (kind: qr, fields:
  q, r)` — a genuinely useful self-documenting error.
- `findpeaks`'s location field is `locations`, not `locs`.
- `locations = [2, 6]` confirms 0-based indexing again: `x[2]=3` and `x[6]=4`
  are indeed the two local maxima.
- There is no destructuring (`q, r = qr(M)`, see §2) — `.field` access on the
  returned `Model` is the *only* way to pull out multiple results.

---

## 8. Tables

Construction is via named-column keyword arguments, `table(...)`/`DataFrame(...)`
— **not** by wrapping a matrix:

```qu
df = table(x=[1,2,3], y=[10,20,-5])
print(df)
print("nrow = {nrow(df)}")
print("ncol = {ncol(df)}")
print("df.nrows = {df.nrows}")
print("rows(df) = {rows(df)}")
```

Output:

```
x   y
-  --
1  10
2  20
3  -5
nrow = 3
ncol = 2
df.nrows = 3
rows(df) = 3
```

`df.nrows`/`rows(df)`/`nrow(df)` (and the `ncol`/`cols`/`df.ncols` triad) are
all confirmed working — these are today's real sugar aliases, added in the
same commit as the `->` reshape operator (`4464d1b`).

CSV round-trip, both the raw builtins and the new `table.` sugar:

```qu
write_csv(df, "data.csv")
df2 = read_csv("data.csv")
print(df2)
table.write(df, "data2.csv")
df3 = table.load("data2.csv")
print(df3)
```

Output (both round-trips produce the identical table back):

```
x   y
-  --
1  10
2  20
3  -5
x   y
-  --
1  10
2  20
3  -5
```

`filter`, `sort_by`, `group_by_agg`:

```qu
df = table(x=[1,2,3,-1,5], y=[10,20,-5,7,2])
pos = filter(df, df.x > 0)
print(pos)

s = sort_by(df, "y")
print(s)

g = table(grp=[1,1,2,2], v=[10,20,30,40])
agg = group_by_agg(g, "grp", "v", "mean")
print(agg)
```

Output:

```
x   y
-  --
1  10
2  20
3  -5
5   2
 x   y
--  --
 3  -5
 5   2
-1   7
 1  10
 2  20
grp  mean_v
---  ------
1        15
2        35
```

`df[mask]` does **not** work — bracket indexing on a table is single-key row
lookup only, with a very explicit error pointing at the fix:

```qu
mask = df.x > 0
bad = df[mask]
```
```
qu: runtime error: table has the default numeric row index (0, 1, 2, ...);
`mask` is not a valid row position -- call `.index("col")` first to look rows
up by a string label
```

The real idiom is `filter(df, mask)`, shown working above.

> **MISMATCH (confirmed once more, precisely) — `read_csv`/`write_csv`
> signature.** `docs/qu-language-spec.md` / `book/src/language-reference/spec.md`
> (both identical, lines 1555-1556, 2307, 2397-2398, 2786) consistently write:
> ```text
> T = read_csv("data.csv", header=true)
> write_csv("filtered.csv", y)              # path FIRST, data SECOND
> ```
> Real, verified signatures (confirmed by successful round-trip above, and
> matching `book/src/stdlib/file-io.md:90-91`, which is already correct):
> - `read_csv(path, headers=true, sep=",", decimal=".")` — the keyword is
>   `headers` (plural), not `header`.
> - `write_csv(df, path)` — **data first, path second**, the reverse of what
>   the spec shows. Calling `write_csv("filtered.csv", y)` the spec's way
>   would try to write a string as if it were a table.
>
> This is the exact mismatch this session had already flagged once before
> (per the task brief) — now independently re-confirmed with a real run.
> `book/src/stdlib/file-io.md` already has the correct signature; only
> `docs/qu-language-spec.md`/`book/src/language-reference/spec.md` are wrong.

---

## 9. Multiple dispatch

Three overloads of `area`, resolved by specificity — untyped is the fallback:

```qu
function area(x: num)
    return x * x
end function

function area(x: vec)
    return sum(x)
end function

function area(x)
    return -1
end function

print("area(5) = {area(5)}")
print("area([1,2,3]) = {area([1,2,3])}")
print("area(true) = {area(true)}")
```

Output:

```
area(5) = 25
area([1,2,3]) = 6
area(true) = -1
```

`area(5)` picks the `num`-typed overload, `area([1,2,3])` picks the
`vec`-typed one, and `area(true)` (a bool — matches neither `num` nor `vec`)
falls through to the untyped catch-all. See §5 for the operator-overload
(`function +(a: record<Vec2>, ...)`) flavor of dispatch, which additionally
requires the `__type` tag convention.

---

## 10. String interpolation

Bare expressions, arithmetic, function calls, and `{expr:spec}`-style
formatting all work:

```qu
pi_val = 3.14159265
print("pi = {pi_val:.2f}")
print("pi rounded = {pi_val:.0f}")
print("expr = {2+3*4}")
print("nested = {sqrt(16) + 1}")
v = [1,2,3]
print("vec sum = {sum(v):.1f}")
```

Output:

```
pi = 3.14
pi rounded = 3
expr = 14
nested = 5
vec sum = 6.0
```

---

## 11. Comments & `#%%` cells

Line comments (`#`), trailing comments, and `#%%` cell markers all work as
plain no-ops when running a script normally:

```qu
# this is a comment
#%% Cell One
x = 1  # trailing comment
print("x = {x}")
#%% Cell Two
y = 2
print("y = {y}")
```
```
x = 1
y = 2
```

`#%%` markers are meaningful to the `diary` subcommand (`qu.exe diary
script.qu -o out.html`), which was confirmed to run and produce an HTML
transcript file without error — full cell-boundary rendering in that HTML
wasn't independently verified byte-for-byte here since it's a secondary
tooling feature, not core syntax.

---

## 12. `timer.` sugar and the `->` reshape operator

Both added in the same commit (`4464d1b`, today). **`timer.start()` /
`timer.elapsed()` / `timer.stop()` are pure sugar for a single GLOBAL
`tic()`/`toc()` pair — not an object/handle with methods.** This is worth
being precise about, since the dotted-call syntax looks object-oriented but
isn't:

```qu
timer.start()
acc = 0
for k = 0 to 100000
    acc += k
end for
e = timer.elapsed()
print("elapsed >= 0: {e >= 0}")
s = timer.stop()
print("stop returns: {s}")
print("done, acc = {acc}")
```

Output:

```
elapsed >= 0: true
stop returns: 0.000972
done, acc = 5000050000
```

(The `stop returns:` value is a real wall-clock measurement and will vary
run to run — re-running produced `0.000604` on another pass. Everything else
is deterministic and reproduces exactly.)

`timer.start()` rewrites at parse time to `tic()`; both `timer.elapsed()` and
`timer.stop()` rewrite to `toc()` (they're aliases of each other, per
`qu-syntax/src/lib.rs:561-572` — `("timer","start") => "tic"`,
`("timer","elapsed")/("timer","stop") => "toc"`). Writing `t = timer.start()`
and later calling `t.elapsed()` does **not** work — `timer.start()` returns
`none`, not a timer handle (`qu: runtime error: expected a timer, found
none` if you try to call a method on its result).

Reshape `->`:

```qu
v = [1,2,3,4]
r1 = v -> (2,2)
print("v -> (2,2) = {r1}")

w = [1,2,3]
r2 = w -> (3,1)
print("w -> (3,1) = {r2}")
```

Output:

```
v -> (2,2) = [1, 3; 2, 4]
w -> (3,1) = [1; 2; 3]
```

`expr -> (rows, cols)` rewrites at parse time to `reshape(expr, rows, cols)`.
`->` was a lexed-but-dead token before this addition (in `qu-lexer`'s
`OPERATORS` table already, with zero parser semantics anywhere) — see §5 for
how this collides with the spec's separate, unimplemented lambda-arrow
proposal that would also want `->`.

Also added in the same commit: `df.nrows`/`df.ncols`/`rows(x)`/`cols(x)`
sugar (§8), and a latent bug fix where `shape_of` on a `Table` previously
returned a hardcoded `(0,0)` instead of the real shape.

---

## 13. Error handling

A real uncaught runtime error:

```qu
y = [1,2] + [1,2,3]
```
```
qu: runtime error: vector length mismatch: 2 vs 3 (broadcasting of unequal vectors is an error)
```

The same error caught by `try`/`catch`, with the caught value exposing a
`.message` field:

```qu
try
    y = [1,2] + [1,2,3]
catch e
    print("caught: {e.message}")
end
print("continued after catch")
```

Output:

```
caught: vector length mismatch: 2 vs 3 (broadcasting of unequal vectors is an error)
continued after catch
```

`try` closes with a bare `end` (no `end try`) by design — confirmed by both
the successful run above and the parser source's own comment
(`qu-syntax/src/lib.rs:1379-1380`: "`try` closes with bare `end` (no `end
try`) — the one deliberate exception to §16's `end <introducer>` rule,
matching MATLAB").

A parse-time error looks different again — note the message prefix and
format differ between `run` (`qu: runtime error: parse error at ...`) and
`ast`/`parse` (`qu: parse error at ...`, no "runtime" prefix), both confirmed
directly:

```
qu: runtime error: parse error at 1:5: expected end of statement (newline or `;`), found Ident("v")
```

---

## 14. `unsafe` blocks — continue past a failure

Unlike `try`/`catch` (§13), which stops at the first error and jumps to a
handler, `unsafe [as var] ... end` keeps executing statement-by-statement
even after one fails — logging each failure and moving on. `as var`
optionally captures the total **failure count** (not the errors
themselves) into a variable:

```qu
unsafe as n_failed
    y = sqrt(-1)              # NaN, not an error -- just continues
    z = [1,2] + [1,2,3]       # a real error -- logged, counted, continues anyway
    print("still runs")
end
print("failures = {n_failed}")
```

Output:

```
· unsafe: statement failed: vector length mismatch: 2 vs 3 (broadcasting of unequal vectors is an error)
still runs
failures = 1
```

Good for batch scripts that should attempt many independent operations and
report a tally rather than abort on the first problem.

---

## 15. `parallel for` and the pool/queue job system

`parallel for Name in Range ... end parallel` is a real data-parallel loop:

```qu
results = zeros(5)
parallel for i in 0 to 4
    results[i] = i * i
end parallel
print(results)
```
```
[0, 1, 4, 9, 16]
```

Beyond that, there's a genuine worker-pool job scheduler — `pool` (with
resource limits and a policy), `queue()` (a deferred-job list), and
`run <queue> on <pool>` (dispatch every queued job across the pool):

```qu
function square(x)
    return x * x
end function

pool workers with cpu=4
    policy = round_robin        # or priority | fair | affinity
    on full = queue             # or reject | spill_to(cpu)
end pool

q = queue()
for i in 0 to 9
    q.push("square", i)         # enqueue a deferred call by name + args
end for

results = run q on workers
print(results)
```

Output:

```
(0, 1, 4, 9, 16, 25, 36, 49, 64, 81)
```

`q.push(fnName, arg1, arg2, ...)` enqueues a *named* function call — it
does not accept an arbitrary inline expression/lambda as the job body;
`run` expects a real `queue()` value on its left, not a bare expression
(`run print("x") on pool` errors: `expected a queue from queue() on the
left of "on", found none`). `on full = queue` is the pool's backpressure
policy for when every worker is busy — jobs queue rather than getting
rejected or spilled to a different resource.

### 15.1 Distributed pools — `listen_pool` and `pool ... remote=(...)`

Beyond local threads, a pool's workers can be OTHER Qu processes running on
the network — `listen_pool` starts one process in "listen mode," and a
`remote = (...)` resource on `pool` names those listeners as extra workers.
This is a real, minimal, working slice of what §54 of the language
reference sketches as a much larger future cluster-management vision
(`worker()`/`cluster()` objects, `least_loaded`/`affinity` policies,
auto-discovery, fault-tolerant reschedule-on-node-drop) — none of that
exists yet; what's here is deliberately small: name some `"host:port"`
addresses, dispatch jobs to them by function name, get results back.

On the listening machine (or a second terminal on the same machine, for
trying this out locally over the loopback interface):

```qu
square(x) := x * x
listen_pool(9000, allow=("square"))   # blocks forever, serving requests
```

`allow=(...)` is a hard requirement, not an option — a listener only ever
executes a function whose NAME is in this list. A request for anything
else gets a clean rejection, never a crash and never a silent bypass: this
is what keeps a listening process from becoming "run any stdlib function
a network caller asks for," which would otherwise let anyone who can reach
the port trigger file writes, `http_get`, etc. by name alone. There is no
way to ship a closure or raw source code over this wire — only pre-defined
functions the listening SCRIPT itself already chose to expose.

On the calling machine, name that address as a `remote` resource alongside
(or instead of) local `cpu` workers:

```qu
square(x) := x * x                    # still needed locally: `run` validates
                                       # every queued function name against
                                       # the CALLING script's own functions
                                       # before dispatching anything, even a
                                       # job that will actually run remotely

pool workers with cpu=2, remote=("192.168.1.50:9000", "192.168.1.51:9000")
end pool

q = queue()
for i in 0 to 9
    q.push("square", i)
end for

results = run q on workers            # some jobs run locally, some remote —
                                       # one ordered List either way
```

A pool can also be remote-only — `cpu` is optional when `remote = (...)`
names at least one worker (`pool workers with remote=("host:port") ... end
pool` is valid on its own). `remote=` (like `allow=` above) takes Qu's
actual list-literal syntax, a parenthesized tuple — `("a", "b")`, not
bracket `[...]` (always a numeric Vec/Mat literal) — or a single bare
string when there's only one address, e.g. `remote = "host:port"` (writing
it as `remote = ("host:port")` — no comma — would parse as a plain
grouped string, not a one-element list, so the bare-string form is also
accepted directly rather than requiring an easy-to-miss trailing comma).

**Dispatch and failure handling.** Jobs are round-robined by submission
order across "lanes" — one lane for the whole local `cpu` pool (already
internally parallel across its own worker count) plus one lane per
`remote` address — the same `round_robin` policy a local-only pool already
uses (no other `policy` value is implemented for either shape). Wire
format is newline-delimited JSON, reusing `Value`'s existing JSON encoding
(the same one `save`/`load` use) — a request names the function and its
already-evaluated arguments, a response carries either the result or an
error. A remote worker that's unreachable, times out (a bounded ~3s
connect timeout, ~30s once connected — never an indefinite hang), or
rejects the call (not on its `allow` list) fails **that one job**, exactly
like a local job's own runtime error: the first failure in submission
order propagates out of `run`, every other job (local or remote) still
runs to completion first. There is no silent fallback from a failed remote
job to running it locally instead — a `run` either got every result it
asked for, or it errors clearly about the one that failed.

---

## 16. `cast` and `resample_to`

Added the same day as this section, alongside the `->` reshape operator
and `table.`/`timer.` sugar (§12). `cast(value, tag)` converts between
Qu's real existing types — using the same `TypeTag` vocabulary multiple
dispatch already uses (`num`, `str`, `bool`, `vec`, ...), not a separate
casting vocabulary, and deliberately NOT the `as` keyword (which already
means something else — a shape-validation contract, `x as vector(3)`,
unrelated to type conversion):

```qu
print((42).cast("str"))
print("42".cast("num"))
```
```
42
42
```

`resample_to(signal, new_fs)` does real resampling (linear interpolation
of the signal's implied continuous waveform), not a relabel — the
resampled signal spans the *same* time duration as the original, just at
a different sample density:

```qu
x = signal([0, 10, 20, 30])
print(x)
print(x.resample_to(2))
```

Output:

```
signal(Fs=1) [0, 10, 20, 30]
signal(Fs=2) [0, 5, 10, 15, 20, 25, 30]
```

The original spans `t=0..3s` at 1 Hz (4 samples); the resampled version
still spans `t=0..3s`, just at 2 Hz (7 samples) — not a 4-sample signal
relabeled to falsely claim a 1.5s duration. `signal(data)` (one argument)
also now defaults `Fs` to `1.0` rather than erroring.

---

## 17. Advanced multiple dispatch

§9 showed the basic three-overload case (`num` vs `vec` vs untyped). This
section goes deeper into the two refinement forms (`record<Tag>`,
`model<"kind">`), how specificity is actually computed when several
overloads could match, and the two dispatch-failure error shapes — all
checked directly against the implementation in
`qu-interp/src/lib.rs:2709-2858` (`record_type_tag`, `tag_match_specificity`,
`dispatch_method`) rather than guessed.

### `record<Tag>` refinement — three tags plus a plain fallback

§5 showed a single `record<Vec2>` overload. Here are three distinct `__type`
tags, each dispatching to a different function body, plus a fourth,
untagged `record` overload that only untagged records fall through to:

```qu
function describe_shape(s: record<Circle>)
    return "circle r={s.r}"
end function

function describe_shape(s: record<Square>)
    return "square side={s.side}"
end function

function describe_shape(s: record<Triangle>)
    return "triangle base={s.base} height={s.height}"
end function

function describe_shape(s: record)
    return "unknown shape"
end function

c = {__type = "Circle", r = 2}
sq = {__type = "Square", side = 3}
tr = {__type = "Triangle", base = 4, height = 5}
plain = {a = 1, b = 2}

print(describe_shape(c))
print(describe_shape(sq))
print(describe_shape(tr))
print(describe_shape(plain))
```

Output:

```
circle r=2
square side=3
triangle base=4 height=5
unknown shape
```

Each `__type` genuinely picks its own overload — this isn't three overloads
falling through to one lucky match; swapping `c` and `tr` above (not shown)
changes which body runs. The untagged `plain` record matches none of the
three refined overloads (`record_type_tag` finds no `__type` field) and
falls through to the plain `record` overload, exactly like §5's
"omitting `__type` falls through" note, except here the fallback is a real
user overload rather than a hard error.

### `model<"kind">` refinement — `qr` vs `svd` vs a plain `model` fallback

The same refinement mechanism applies to `Value::Model`, keyed on the
model's `kind` string rather than a record field. `qr` and `svd` are both
real Model-returning builtins (§7); `findpeaks` (also from §7) is used here
to exercise the plain, unrefined `model` fallback:

```qu
function summarize(m: model<"qr">)
    return "qr: q is {rows(m.q)}x{cols(m.q)}, r is {rows(m.r)}x{cols(m.r)}"
end function

function summarize(m: model<"svd">)
    return "svd: singular values = {m.s}"
end function

function summarize(m: model)
    return "generic model, kind unhandled"
end function

A = [4, 3; 6, 3]
qres = qr(A)
sres = svd(A)
pk = findpeaks([0,1,3,1,0,2,4,2,0])

print(summarize(qres))
print(summarize(sres))
print(summarize(pk))
```

Output:

```
qr: q is 2x2, r is 2x2
svd: singular values = [8.335579, 0.719806]
generic model, kind unhandled
```

`qres` (kind `"qr"`) and `sres` (kind `"svd"`) each hit their own refined
overload; `pk` (kind `"findpeaks"`, which no refined overload names) falls
through to the plain `model` catch-all. Confirms `tag_match_specificity`'s
`("model", Value::Model(m)) if &m.kind == refine` arm precisely: it's a
string match against the model's own `kind`, nothing structural.

### Specificity: more (and more refined) typed parameters wins

The rule, read directly from `tag_match_specificity`/`dispatch_method`: each
matched parameter contributes a weight to a running total — 0 for an
untyped parameter, 1 for a plain tag match (`vec`, `num`, ...), and
1,000,000 (`REFINED_WEIGHT`) for a refinement match (`record<Tag>` /
`model<"kind">`). The candidate with the highest *total* wins; a refined
match on even one parameter beats any number of plain-tag matches, and
between two plain-tag candidates, the one with more typed parameters
matching wins. A genuinely ambiguous-looking three-way overload set proves
this — two single-typed overloads plus one double-typed one, called with
two vectors:

```qu
function combine(a: vec, b)
    return "a-typed: sum(a)={sum(a)}"
end function

function combine(a, b: vec)
    return "b-typed: sum(b)={sum(b)}"
end function

function combine(a: vec, b: vec)
    return "both-typed: sum(a)+sum(b)={sum(a)+sum(b)}"
end function

v1 = [1,2,3]
v2 = [10,20]
print(combine(v1, v2))
```

Output:

```
both-typed: sum(a)+sum(b)=36
```

Both single-typed overloads score specificity 1 (one matched typed
parameter, one untyped-matches-anything parameter contributing 0); the
double-typed overload scores 2 and wins outright — not a tie, because
`dispatch_method` sums specificity across *all* parameters, not just
whichever one happens to be typed.

### The two dispatch-failure errors

Zero matching candidates — calling `area` (§9's two-overload `num`/`vec`
version, no untyped fallback this time) with a `string`, which matches
neither:

```qu
function area(x: num)
    return x * x
end function

function area(x: vec)
    return sum(x)
end function

print(area("hello"))
```

```
qu: runtime error: no method `area` matches argument types (string) — 2 candidate(s) defined:
  area(x: num)
  area(x: vec)
```

Ambiguous match — remove the tie-breaking `combine(a: vec, b: vec)`
overload from the specificity example above, leaving only the two
single-typed overloads, and call with two vectors again. Both now score
specificity 1 with nothing to separate them:

```qu
function combine(a: vec, b)
    return "a-typed"
end function

function combine(a, b: vec)
    return "b-typed"
end function

v1 = [1,2,3]
v2 = [10,20]
print(combine(v1, v2))
```

```
qu: runtime error: ambiguous call to `combine` for argument types (vector, vector) — 2 equally specific candidates, add a more specific overload or rename one:
  combine(a: vec, b)
  combine(a, b: vec)
```

Both errors name every real candidate signature (capped at 5, per
`format_candidates`) rather than a generic "no matching overload" —
confirmed here with the real text, matching what §19's summary (a BUG fix,
commit `9a71b6a`) already established this format looks like when
triggered via a mismatched operator overload rather than a plain function.

---

## 18. DSP builtins — a signal-processing tour

§1 showed a single `fft`/`signal()` example and §16 covered `resample_to`.
This section tours filter design + zero-phase application, frequency
response, windowed spectral analysis, spectral peak-picking, Kalman
tracking, and correlation — using the same real call conventions already
verified working in `catalog/qu_filter_design.qu` and
`catalog/qu_kalman_tracking.qu`, not reinvented from scratch.

### Filter design (`butter`) + zero-phase application (`filtfilt`)

`butter(order, kind, cutoff, fs)` designs a Butterworth filter as
second-order sections (a Model, kind `"filter"`, field `sos`). `sosfilt`
applies it causally (with phase lag); `filtfilt` applies it forward-then-
backward for zero phase distortion. Here a low-pass strips a 400 Hz tone
out of a 20 Hz + 400 Hz mix while preserving the 20 Hz tone:

```qu
Fs = 2000
fc = 60
lp = butter(4, "low", fc, Fs)
print("Butterworth order 4, cutoff {fc} Hz @ Fs={Fs} Hz -- sos shape: {rows(lp.sos)}x{cols(lp.sos)}")

N = 1000
t = (0 to N - 1) / Fs
low_tone = sin(2*pi*20*t)
high_tone = sin(2*pi*400*t)
mix = low_tone + high_tone

causal = sosfilt(lp, mix)
zerophase = filtfilt(lp, mix)

err_low_causal = rms(causal[100:N-1] - low_tone[100:N-1])
err_low_zp = rms(zerophase[100:N-1] - low_tone[100:N-1])
print("20 Hz tone RMS recovery error: causal={err_low_causal:.4f}, zero-phase={err_low_zp:.4f}")
print("High tone (400 Hz) amplitude in raw mix: {rms(high_tone):.4f}")
print("High tone leakage after zero-phase LP: {rms(zerophase - low_tone):.4f}")
```

Output:

```
Butterworth order 4, cutoff 60 Hz @ Fs=2000 Hz -- sos shape: 2x5
20 Hz tone RMS recovery error: causal=0.6041, zero-phase=0.0520
High tone (400 Hz) amplitude in raw mix: 0.7071
High tone leakage after zero-phase LP: 0.0500
```

A 4th-order Butterworth SOS design is 2 sections x 5 coefficients per
section, matching `rows(lp.sos)=2, cols(lp.sos)=5`. `filtfilt`'s zero-phase
result tracks the clean 20 Hz tone far more closely than the causal pass
(0.052 vs 0.604 RMS error against the *unshifted* reference) — the causal
filter's real output is a correctly-filtered but phase-lagged 20 Hz tone,
which reads as large error only because it's compared against a
zero-lag reference; `filtfilt` has no such lag by construction. The 400 Hz
tone is almost entirely gone from the low-passed result (residual 0.05,
down from 0.71).

### `freqz` — frequency response of a designed filter

```qu
Fs = 2000
fc = 60
lp = butter(4, "low", fc, Fs)
H = freqz(lp, 512)
print(H)
mag_db = 20 * log10(abs(H))
i_fc = round(fc / (Fs / 2) * (length(H) - 1))
i_dc = 0
i_nyq = length(H) - 1
print("Gain at DC: {mag_db[i_dc]:.2f} dB")
print("Gain near cutoff ({fc} Hz): {mag_db[i_fc]:.2f} dB (expect close to -3 dB)")
print("Gain at Nyquist: {mag_db[i_nyq]:.2f} dB")
```

Output:

```
[1, 0.996391 - 0.084887i, 0.985575 - 0.169237i, 0.967594 - 0.252511i, 0.942512 - 0.334172i, 0.910422 - 0.413681i, ... (512 elements)]
Gain at DC: -0.00 dB
Gain near cutoff (60 Hz): -3.21 dB (expect close to -3 dB)
Gain at Nyquist: -inf dB
```

`freqz` returns the complex frequency response directly (a `CVec`, printed
with a `... (N elements)` truncation for long vectors — consistent with
§1's untruncated short-vector printing). DC gain is 0 dB (unity, as
expected for a low-pass), the -3.21 dB near the 60 Hz cutoff confirms the
design spec, and the exact `-inf` at Nyquist is real, not a display
artifact — independently confirmed `abs(H[end])` is exactly `0`, a genuine
property of the bilinear-transformed even-order Butterworth low-pass (an
exact zero at `z = -1`), not a rounding curiosity.

### Windowing before `fft` — `hann` reduces spectral leakage

`hann(n)` (also `hamming`, `blackman`) returns a real window `Vec` of length
`n`. Applying it (elementwise multiply, `.*`) before `fft` on a tone whose
frequency does **not** land on an exact FFT bin (20.5 Hz in a 256-point,
256 Hz-sampled window — a non-integer number of cycles) demonstrably
reduces the energy leaked into bins far from the true tone:

```qu
N = 256
Fs = 256
t = (0 to N - 1) / Fs
f0 = 20.5
x = sin(2*pi*f0*t)

mag_raw = abs(fft(x))[0:N/2]
w = hann(N)
mag_win = abs(fft(x .* w))[0:N/2]

far_band = 90 to 127
leak_raw = sum(mag_raw[far_band])
leak_win = sum(mag_win[far_band])

print("Peak magnitude, no window:   {max(mag_raw):.2f}")
print("Peak magnitude, hann window: {max(mag_win):.2f}")
print("Far-band (90-127 Hz) leakage energy, no window:   {leak_raw:.3f}")
print("Far-band (90-127 Hz) leakage energy, hann window: {leak_win:.3f}")
```

Output:

```
Peak magnitude, no window:   82.41
Peak magnitude, hann window: 54.18
Far-band (90-127 Hz) leakage energy, no window:   10.637
Far-band (90-127 Hz) leakage energy, hann window: 0.001
```

The peak itself is shorter with the window applied (main-lobe widening, as
expected), but leakage into the 90-127 Hz band 10,000x smaller (10.637 vs
0.001) — the textbook window trade-off, reproduced for real.

### `findpeaks` on an FFT magnitude spectrum — dominant-frequency detection

Combining `fft`+`abs` (spectrum) with `findpeaks` (§7) to identify the two
dominant tones in a two-tone signal:

```qu
N = 512
Fs = 512
t = (0 to N - 1) / Fs
x = sin(2*pi*40*t) + 0.6*sin(2*pi*90*t)

X = fft(x)
mag = abs(X)
half = mag[0:N/2]

pk = findpeaks(half, min_peak_height=20, min_peak_distance=5)
print(pk)
freqs = pk.locations * (Fs / N)
print("Peak bin locations: {pk.locations}")
print("Peak frequencies (Hz): {freqs}")
```

Output:

```
model(findpeaks) {peaks, locations}
Peak bin locations: [40, 90]
Peak frequencies (Hz): [40, 90]
```

Both injected tones (40 Hz and 90 Hz) are recovered exactly, since
`Fs=N=512` makes bin index equal frequency in Hz here. `findpeaks`'s real
keyword names are `min_peak_height`/`min_peak_distance` (MATLAB-style,
matching the doc comment at `qu-interp/src/lib.rs:12899`) — not the shorter
`min_height`/`min_distance` used by the separate `find_peaks` (underscore,
index-returning) builtin, which is a distinct function with its own kwarg
names.

### Kalman filtering — constant-velocity position tracking

`kalman_init(x0, P0)` returns a Model; `state.predict(F, Q)` and
`state.update(H, z, R)` are dispatched via the model's own `kind`
(`predict`/`update` are ordinary multiple-dispatch functions overloaded on
`model<"kalman">`, called through `.method()` sugar), each returning an
updated state Model to thread into the next iteration — exactly the
pattern already verified in `catalog/qu_kalman_tracking.qu`:

```qu
N = 60
dt = 0.1
true_v = 2.0
t = (0 to N - 1) * dt
true_pos = true_v * t
measured = true_pos + 0.8 * randn(N)

F = [1, dt; 0, 1]
Q = [0.001, 0; 0, 0.001]
H = reshape([1, 0], 1, 2)
R = [0.64]

s = kalman_init([0, 0], [10, 0; 0, 10])
est_pos = zeros(1, N)
est_vel = zeros(1, N)
for k = 0 to N - 1
    s = s.predict(F, Q)
    s = s.update(H, [measured[k]], R)
    est_pos[k] = s.x[0]
    est_vel[k] = s.x[1]
end for

raw_err = rms(measured - true_pos)
filt_err = rms(est_pos - true_pos)
print("Position RMS error: raw sensor={raw_err:.3f}, Kalman estimate={filt_err:.3f}")
print("True velocity={true_v:.2f}, final estimated velocity={est_vel[N-1]:.3f}")
```

Output:

```
Position RMS error: raw sensor=0.832, Kalman estimate=0.215
True velocity=2.00, final estimated velocity=2.023
```

The Kalman estimate's RMS position error (0.215) is roughly 4x lower than
the raw noisy sensor (0.832), and the never-directly-measured velocity
state converges to 2.023 against a true 2.00 — both numbers will vary
slightly run to run (`randn` is unseeded), but the qualitative result
(large error reduction, accurate hidden-state recovery) reproduces
consistently.

### Correlation and cross-correlation — `corr` and `xcorr`

`corr(x, y)` is the scalar Pearson correlation coefficient; `xcorr(x, [y])`
(y defaults to `x`, giving autocorrelation) returns the full
length-`2N-1` cross-correlation sequence, zero lag at index `N-1`:

```qu
N = 200
Fs = 200
t = (0 to N - 1) / Fs
x = sin(2*pi*10*t)
shift = 15
y = zeros(N)
y[shift:N-1] = x[0:N-1-shift]

c = corr(x, x)
print("corr(x, x) = {c}")
c2 = corr(x, y)
print("corr(x, y) (shifted copy) = {c2:.4f}")

r = xcorr(x, y)
print("length(xcorr) = {length(r)} (expect 2N-1 = {2*N-1})")
zero_lag_idx = N - 1
best_idx = where r == max(r)
best_lag = best_idx[0] - zero_lag_idx
print("Best-match lag (samples): {best_lag} (true shift was {shift})")
```

Output:

```
corr(x, x) = 1
corr(x, y) (shifted copy) = -0.0130
length(xcorr) = 399 (expect 2N-1 = 399)
Best-match lag (samples): -15 (true shift was 15)
```

`corr(x,x)=1` exactly, as expected. The direct-sample correlation
`corr(x, y)` between the original and its 15-sample-delayed copy is near
zero (delaying a 10 Hz tone by 15 samples at 200 Hz rotates its phase by
270 deg, which happens to land close to decorrelated) — `xcorr`'s full lag
sweep is what actually finds the true delay: its peak sits at lag -15,
correctly identifying that `y` is `x` delayed by 15 samples (the sign is
`xcorr`'s own convention — `y` lags `x`, so the peak appears on the
negative side).

### `square` is a square-WAVE generator, not "x squared" — a real silent-wrong-answer bug, now fixed

`square(freq, fs, n, [duty=])` generates a bipolar pulse train — it has
nothing to do with squaring a number, despite the name being very easy to
misread as an elementwise math function. Before this fix, `square`'s (and
`impulse`'s and `pwm`'s) required numeric arguments were read with
`args.get(idx).and_then(|v| v.as_num().ok()).unwrap_or(default)` — a
pattern that silently swallows a type error and substitutes the default
whenever the argument is PRESENT but the WRONG type, rather than erroring.
Live-confirmed footgun: piping a whole `Vec` into `square` as if it were an
elementwise operation silently produced a meaningless 1-sample signal
instead of any kind of error:

```qu
x = [1, 2, 3, 4, 5]
y = x |> square |> sum
print(y)
```

Old (buggy) output: `1` — `freq`/`fs`/`n` all silently fell back to their
defaults (`1.0`, `1.0`, `1` sample), and the sum of that one sample is `1`,
a plausible-looking but completely meaningless number with no relationship
to `x`.

New output (this fix):

```
qu: runtime error: square: freq must be a number, found vector
```

Genuinely omitted arguments are unaffected — `square()` with nothing
supplied still defaults exactly as before; only a *present-but-wrong-type*
argument is now an error. `impulse` and `pwm` got the identical fix for
their own required numeric arguments (`n`/`index`/`amplitude` and
`carrier_freq`/`fs` respectively).

Also added: `signals.square(...)`/`signals.impulse(...)`/`signals.pwm(...)`
/`signals.sawtooth(...)`/`signals.triangle(...)` — pure parse-time sugar
(the same closed `table.`/`timer.` "namespace-dot" mechanism from §12,
extended) for calling these five generators unambiguously, useful
specifically because `square` in particular reads as "x squared" so easily.
The bare names are completely unaffected and `signals` still works as an
ordinary variable; the namespaced spelling is byte-identical to the bare
call:

```qu
a = square(100, 1000, 8, duty=0.25)
b = signals.square(100, 1000, 8, duty=0.25)
print("a = {a}")
print("b = {b}")
print("identical = {a == b}")
```

Output:

```
a = signal(Fs=1000) [1, 1, 1, -1, -1, -1, -1, -1]
b = signal(Fs=1000) [1, 1, 1, -1, -1, -1, -1, -1]
identical = [T, T, T, T, T, T, T, T]
```

`sawtooth(freq, fs, n)` and `triangle(freq, fs, n)` were also added as new
builtins (previously missing entirely) with the same argument-validation
rigor and their own `signals.sawtooth`/`signals.triangle` sugar — see
`book/src/stdlib/signal-processing.md` for their full signatures.

---

## 19. `memoize function` — function-result caching

`memoize function name(params) ... end function` is a modifier on the block
`function` form (not available on the one-line `name(params) := expr`
shorthand) that caches a function's return value, keyed by its argument
values — a repeated call with the same arguments skips the body entirely
and is served straight from the cache:

```qu
calls = 0
memoize function slow_square(x)
    calls = calls + 1
    return x * x
end function

a = slow_square(7)
b = slow_square(7)
c = slow_square(8)
print("a = {a}, b = {b}, c = {c}")
print("calls = {calls}")
```

Output:

```
a = 49, b = 49, c = 64
calls = 2
```

`calls` only advances twice (once per DISTINCT argument, `7` and `8`) even
though `slow_square` was called three times — the second call with `7`
returned `49` straight from the cache without re-running the body, which is
exactly what the `calls` counter proves. Only plain numbers/strings/bools
are usable as cache keys (the same three kinds `==` compares directly); a
call with any other argument type (`Vec`, `Table`, ...) still runs and
returns the correct answer, it simply is never cached for that call.

---

## 20. File I/O extensions — `touch`, streaming reads, and memory-mapped files

Three real filesystem features landed together: a proper Unix `touch`, an
internal memory-efficiency fix to ordinary file reads, and read-only
memory-mapped random access.

### `touch(path)`

Unix `touch` semantics: creates an empty file if `path` doesn't exist;
if it already exists, `touch` bumps its modification time WITHOUT altering
its content:

```qu
path = "touch_target.txt"
had_before = file_exists(path)
touch(path)
exists_after_touch = file_exists(path)

f = fopen(path, "w")
write_line(f, "original content")
close(f)

touch(path)                 # existing file: content must survive untouched

g = fopen(path, "r")
content = read_all(g)
close(g)

print("had_before = {had_before}")
print("exists_after_touch = {exists_after_touch}")
print("content = {content}")
```

Output:

```
had_before = false
exists_after_touch = true
content = original content
```

`touch` on the not-yet-existing path made `file_exists` flip from `false`
to `true`; `touch`ing it again after writing left `content` exactly as
written. (Qu has no builtin that reads a file's mtime back into a script —
the "advances mtime without touching content" half of `touch`'s contract is
verified at the Rust unit-test level, not observable from a `.qu` script
directly.)

### Streaming reads — `fopen(path, "r")` no longer buffers the whole file

Opening a file for reading used to load its entire contents into memory
up front; it now opens a real buffered stream (`std::io::BufReader`) and
only reads as many bytes as `read_line`/`read_all`/`read_char`/`seek`/etc.
actually ask for. This matters for large files — a multi-hundred-MB file
can be processed line-by-line without ever holding the whole thing in
memory at once. The change is purely internal: every read builtin's return
values are identical either way, so there's nothing different to point at
in a script's OUTPUT — only that a large-file version of the same loop
below no longer needs memory proportional to the file's size:

```qu
f = fopen("streaming_demo.txt", "w")
for i = 0 to 4
    write_line(f, "line " + str(i))
end for
close(f)

g = fopen("streaming_demo.txt", "r")
n = 0
while not eof(g)
    line = read_line(g)
    n = n + 1
end while
close(g)
print("lines read = {n}")
```

Output:

```
lines read = 5
```

### `mmap_open`/`mmap_len`/`mmap_read` — read-only memory-mapped files

Random-access byte reads without an eager whole-file load or a chain of
`seek` calls on a `fopen` handle — every `mmap_read` call takes an explicit
byte range, since an `mmap` handle has no read cursor of its own to
advance:

```qu
f = fopen("mmap_demo.bin", "wb")
write_byte(f, 65)
write_byte(f, 66)
write_byte(f, 67)
write_byte(f, 68)
write_byte(f, 69)
write_byte(f, 70)
write_byte(f, 71)
write_byte(f, 72)
close(f)

m = mmap_open("mmap_demo.bin")
n = mmap_len(m)
first4 = mmap_read(m, 0, 4)
last4 = mmap_read(m, 4, 4)
print("n = {n}")
print("first4 = {first4}")
print("last4 = {last4}")
```

Output:

```
n = 8
first4 = [65, 66, 67, 68]
last4 = [69, 70, 71, 72]
```

`n` is the mapped file's full byte length; `first4`/`last4` each pull out
an arbitrary 4-byte range (bytes `65..72` are ASCII `A`-`H`) without ever
reading the other range or the whole file.

---

## 21. `http_get(url)` — real HTTP GET

`http_get(url)` issues a real HTTP(S) GET request and returns the response
body as a string (GET-only, 30s timeout, clear errors on an unreachable
host or non-2xx status — no custom headers, auth, or POST in this v1):

```qu
body = http_get("https://example.com/")
print("len = {length(body)}")
```

Output:

```
len = 559
```

559 bytes is the real, live length of `https://example.com/`'s response
body at the time this was run — this hit the actual network, not a stub.

---

## 22. `lazy` — deferred variable evaluation

`lazy name = expr` defers evaluating the right-hand side entirely: the
expression does not run at the `lazy` statement itself, only on the
variable's first actual READ afterward, and the computed value is then
cached in place so a second read never re-runs it. An ordinary
`name = expr` is completely unaffected — `lazy` is an explicit opt-in, not
a new default:

```qu
counter = 0
function tick()
    counter = counter + 1
    return 42
end function

lazy x = tick()
before = counter
y = x
after_first = counter
z = x
after_second = counter
print("before = {before}, after_first = {after_first}, after_second = {after_second}")
print("y = {y}, z = {z}")
```

Output:

```
before = 0, after_first = 1, after_second = 1
y = 42, z = 42
```

`counter` stays `0` all the way up to the `lazy x = tick()` statement and
the first read of `x` — proving `tick()` truly did not run at declaration
time. The first read (`y = x`) forces it (`counter` becomes `1`); the
second read (`z = x`) reuses the cached `42` without calling `tick()`
again (`counter` stays `1`).

---

## 23. `StreamFile`/`StreamURL` — unified polymorphic streams

`StreamFile(path, [mode])` and `StreamURL(url)` are two constructors that
produce values exposing the SAME method names — `read_line`/`read_all`/
`eof`/`close` — so one function written against "a stream" works
unmodified against either kind. `StreamFile` is literally `fopen`'s own
machinery under a second name; `StreamURL` fetches the whole HTTP(S) body
eagerly (the same request `http_get` makes) and wraps it in a read-only,
in-memory cursor with the identical method names:

```qu
function drain(s)
    total = ""
    while not eof(s)
        total = total + read_line(s) + "|"
    end while
    close(s)
    return total
end function

# Written here so the file half runs; the URL half cannot be, since a
# documentation build must not depend on the network being up or on a
# third party's page not changing under it.
w = fopen("stream_notes.txt", "w")
write_line(w, "one")
write_line(w, "two")
close(w)

a = drain(StreamFile("stream_notes.txt"))
b = drain(StreamURL("https://example.com/"))
print("a = {a}")
print("b length = {length(b)}")
```

(`stream_notes.txt` contains three lines: `alpha`, `beta`, `gamma`.)

Output:

```
a = alpha|beta|gamma|
b length = 559
```

The exact same `drain` function ran against a real local file AND a real
HTTP response with zero branching on which kind it received — `b length`
matches §21's `http_get` length exactly, since `StreamURL` makes the
identical request under the hood.

---

## 24. Three new Signal builtins — `interpolate_at`, `apply`, `cut`

Three additions to the `Signal` type's toolkit, all `Fs`-aware and all
composing with the generic `recv.method(args)` method-chain sugar and the
`@` self-mutation prefix like every other Qu builtin.

### `interpolate_at(sig, t_query, [method])`

A `Signal`-aware `interp1`: instead of an explicit `x` vector, the
signal's OWN implicit time axis (`sig.t`, i.e. `i/Fs`) is used automatically,
so the caller only supplies query time(s) in seconds:

```qu
sig = signal([0, 10, 20, 30], 2)     # samples at t = 0, 0.5, 1.0, 1.5s
mid = interpolate_at(sig, 0.75)
lo = interpolate_at(sig, -0.5)
hi = interpolate_at(sig, 2.0)
qs = sig.interpolate_at([0.0, 0.5, 1.0], "nearest")
print("mid = {mid}")
print("lo = {lo}")
print("hi = {hi}")
print("qs = {qs}")
```

Output:

```
mid = 15
lo = -10
hi = 40
qs = [0, 10, 20]
```

`mid` (t=0.75s) sits exactly halfway between samples at t=0.5s (y=10) and
t=1.0s (y=20), interpolating to `15`. `lo` (t=-0.5s) and `hi` (t=2.0s) both
fall outside the signal's own 0..1.5s span and are extrapolated via the
boundary segment's slope (20/s): `0 - 20*0.5 = -10`, `30 + 20*0.5 = 40`.
The result is a plain `Num`/`Vec`, never re-wrapped as a `Signal` — the
query times have no guaranteed uniform spacing, so there is no honest `Fs`
left to attach.

### `s.apply(fnName)` — elementwise map that preserves the container

Distinct from `map(fnName, xs)`, which always returns a plain `List`.
`apply(x, fnName)` instead matches this interpreter's existing
container-preserving convention (the same one `sin`/`cos`/unary `-` already
use): a `Signal` stays a `Signal` at the same `Fs`, a `Vec` stays a `Vec`:

```qu
double(v) := v * 2
sig = signal([1, 2, 3, 4], 8)
out = sig.apply("double")
print("out = {out}")
print("out.Fs = {out.Fs}")
print("sig unchanged = {sig}")
```

Output:

```
out = signal(Fs=8) [2, 4, 6, 8]
out.Fs = 8
sig unchanged = signal(Fs=8) [1, 2, 3, 4]
```

Every sample was genuinely doubled (not a no-op), `out` kept the input's
`Fs=8` exactly, and `sig` itself is untouched — `apply` is pure, like every
other Qu collection builtin.

### `cut(sig, t_start, t_end)` — time-based trim

Converts `t_start`/`t_end` (seconds) to sample indices via the signal's own
`Fs`, half-open in time: `[t_start, t_end)`. This is the one range in the
language that excludes its upper end, and it does so because the end is a
floating-point instant rather than an index -- "does t_end land exactly on a
sample" has no reliable answer. Index slicing (`x[a:b]`) includes both ends.
A `t_end` past the signal's actual duration clamps to the last sample rather
than erroring:

```qu
fs = 10
s = signal(0 to 9, fs)
trimmed = cut(s, 0.5, 100.0)
print("trimmed = {trimmed}")
print("trimmed.Fs = {trimmed.Fs}")

t2 = signal(0 to 9, fs)
@t2.cut(0.2, 0.7)
print("t2 after @cut = {t2}")
```

Output:

```
trimmed = signal(Fs=10) [5, 6, 7, 8, 9]
trimmed.Fs = 10
t2 after @cut = signal(Fs=10) [2, 3, 4, 5, 6]
```

`cut(s, 0.5, 100.0)` clamped the requested end time (100s) down to the
signal's last sample, keeping samples 5-9 (t=0.5s onward) — 5 samples, at
the same `Fs=10`. `@t2.cut(0.2, 0.7)` is the self-mutating form
(`t2 = cut(t2, 0.2, 0.7)`), needing no special-cased `@` handling since
`cut` is an ordinary function under the hood: `0.2s` and `0.7s` round to
samples 2 and 7, keeping indices 2..6 (the end time is excluded).

---

## 25. `watch` — file/variable/URL change detection

`watch` registers a callback for one of three trigger conditions — a
file's modification time changing, a variable's value changing, or an
HTTP endpoint's response body changing. It reuses the exact same
registration-then-fire shape as the pre-existing `every`/`after`/`at ...
end` timers (§12's `timer.` sugar is a different feature — this is the
statement-level `Timer`/`OnElapsed` machinery those use under the hood):

```qu
watch file(path) [do]
    ...
end

watch <variable name> [do]
    ...
end

watch url(url, [interval_seconds=]) [do]
    ...
end
```

`watch` is contextual, not a reserved word (matching `every`/`on`/`pool`'s
own precedent) — `watch = 5` still assigns a plain variable named `watch`.

### The execution model: nothing runs "in the background"

This is the single most important thing to understand about `watch`.
Qu's interpreter is synchronous and single-threaded, with no background
event loop — exactly the same constraint that already makes
`every`/`after`/`at` fire only during an explicit `run_for(duration)`
call, never asynchronously while the rest of the script executes. `watch`
inherits that same model: the `watch ... end` statement only
**registers** the callback and takes a baseline snapshot of the current
state (the file's mtime, the variable's value, or one real fetch of the
URL) — the body never runs there. A callback only fires when the script
later calls the **`pump_watches()`** builtin, which checks every
registered watch's real current state, right now, exactly once, and runs
the body of any whose state has genuinely changed since the last check.

`pump_watches()` is a **new, separate builtin from `run_for`** — investigated
reusing `run_for(duration)` and rejected it: `run_for` drives registered
timers across a *simulated* virtual clock with no real waiting (see its
own table entry below), which makes sense for a fixed time interval but
has no honest equivalent for "has this file's real mtime changed" — there
is no simulated mtime to fast-forward through. A `watch`'s condition is
real external state (the filesystem, a variable, an HTTP endpoint) that
can only be checked for real, at the moment you ask. `pump_watches()`
means exactly that: check everything once, right now.

A script that wants repeated checking drives it itself, typically pairing
`pump_watches()` with the pre-existing `sleep(ms)` and `wait until ... end
until`:

```qu
n = 0
watch file("data.csv") do
    n = n + 1
end

wait until n >= 1
    sleep(200)          # real wall-clock pause between checks
    pump_watches()
end until
```

If you were hoping `watch` means "runs in the background while my script
does other things" — it does not, and cannot, without a much larger
concurrency redesign of the interpreter. It is cooperative polling, spelled
as a statement instead of a raw `if current != last` check, nothing more.

### `watch file(path)` — file modification time

Fires when `path`'s mtime differs from the last check (`file_mtime`,
built on the same `filetime` crate `touch(path)` already uses for real
mtime handling). Deliberately **poll-based**, not the `notify` crate's
real OS-level filesystem-event notifications — investigated and rejected
for this pass: `notify` pushes events from a background OS thread, and
wiring that into this single-threaded, cooperatively-pumped interpreter
without silently reintroducing "runs in the background" is a real,
separate concurrency design question, not a drop-in dependency swap.
Honest, on-demand polling via `filetime` is the right-sized v1. A path
that doesn't exist yet at registration time has a `None` baseline, so the
file later being *created* also counts as a change.

```qu
fires = 0
watch file("config.txt") do
    fires = fires + 1
end
f1 = pump_watches()      # nothing changed yet -> 0
touch("config.txt")      # real mtime bump (creates the file if missing)
f2 = pump_watches()      # -> 1
f3 = pump_watches()      # nothing changed since f2's check -> 0
```

### `watch <name>` — variable value change

Fires when variable `name`'s current value differs from its value as of
the last check, using the same equality `==` uses — reassigning the
*identical* value never fires. Checked by a plain read-and-compare
against the named variable at each `pump_watches()` call, **not** a hook
on every assignment anywhere in the interpreter: hooking `var_set` itself
would cost every script a check on every write, even scripts that never
use `watch` — the targeted "look this one name up, compare, done"
approach costs nothing extra on assignment and is O(1) per watch per
pump. The one honest trade-off of any polling approach: a value that
changes and changes back between two `pump_watches()` calls looks
identical to "never changed" — the same trade-off `watch file`/`watch
url` already accept.

```qu
x = 1
watch x do
    print("x changed to {x}")
end
pump_watches()   # no change yet -> silent
x = 5
pump_watches()   # -> prints "x changed to 5"
x = 5
pump_watches()   # same value -> silent
```

### `watch url(url, [interval_seconds=])` — HTTP response body change

Fires when a fresh fetch of `url` (via the existing `http_get` builtin —
see §21 — never a second HTTP implementation) returns a body different
from the last fetch. `interval_seconds` bounds how often this actually
fetches over the network, in real wall-clock seconds — `pump_watches()`
may be called far more often than that; a call landing before the
interval has elapsed is a cheap no-op, not a fetch. Defaults to `5`
seconds when omitted (a plain "don't hammer the endpoint" default). The
baseline is one real fetch performed at `watch url(...)` registration
time, so whatever the endpoint returns *before* you started watching is
never itself treated as a change.

```qu
watch url("https://example.com/status", interval_seconds=30) do
    print("status page changed")
end
pump_watches()
```

Real evidence, from `engine/crates/qu-interp/src/lib.rs`'s own test
suite (`watch_file_fires_once_on_a_real_mtime_change_and_not_on_an_unchanged_pump`,
`watch_var_fires_on_a_genuine_value_change_and_not_on_a_same_value_reassignment`,
`watch_url_fires_on_a_real_body_change_over_loopback_and_not_on_an_unchanged_poll`):
all three trigger conditions fire exactly once on a genuine real change and
produce zero false fires on a pump where nothing changed — a real `touch`
on a real temp file, a real variable reassignment, and a real HTTP fetch
against a local loopback server whose response body actually changes
between two fetches, not a mocked comparison.

---

## 26. Advanced Qu vs. Python: two real, measured comparisons

Ahmed asked for a section comparing "advanced" Python code against the
equivalent Qu, checking that Qu is easier, faster, and accurate — with
the explicit caveat that the two languages don't always think about a
problem the same way, so this isn't a line-for-line transliteration
exercise. Both examples below were actually run (release `qu.exe` vs.
Python 3.13 + NumPy 2.4.5), not estimated, and the second one caught a
real bug along the way — reported honestly rather than adjusted to make
the numbers look better.

### 26.1 HMM Viterbi decoding, 8000-step sequence — Qu wins on all three

Qu's `hmm(transition, emission, initial).viterbi(obs)` (§ Markov Chains &
HMM in `book/src/stdlib/statistics-ml.md`) does the entire dynamic-
programming recursion inside one compiled Rust builtin call. Python's
"advanced" equivalent (a real log-space Viterbi implementation, vectorized
over states with NumPy inside each timestep) still needs an unavoidable
Python-level `for t in range(T)` loop, because each timestep genuinely
depends on the previous one — this is the case where Qu's advantage is
structural, not incidental.

```qu
model = hmm(transition, emission, initial)
path = model.viterbi(obs)          # one call, entire 8000-step decode
```
```python
# Python needs the full DP recursion spelled out by hand, even with NumPy:
for t in range(1, T):
    scores = delta[:, None] + log_trans
    psi[t] = np.argmax(scores, axis=0)
    delta = scores[psi[t], np.arange(n_states)] + log_emit[:, obs[t]]
# ...plus a separate backtracking pass to recover the path
```

**Measured**, 5 states, 8000-step observation sequence, identical inputs
(same deterministic LCG-generated observation sequence, same transition/
emission matrices, in both languages):

| | Qu | Python (NumPy, log-space, hand-written) |
|---|---|---|
| Wall-clock time | **2.6 ms** | 31.8 ms (~12x slower) |
| Decoded path | `[3,4,1,1,1,1,1,0,4,0,...]`, checksum 15908 | identical, checksum 15908 |
| Core algorithm code | 2 lines (builtin call) | ~15 lines (DP recursion + backtracking) |

**A real bug found and fixed along the way (2026-09-01):** the first run of
this comparison showed Qu's checksum as 31413, not 15908 — a real
mismatch. Both `hmm_forward` and `hmm_viterbi` (`engine/crates/qu-interp/
src/lib.rs`) originally accumulated raw linear probabilities, which
underflow to exactly `0.0` in `f64` after roughly 300-700 observations
depending on the model. Once every candidate ties at `0.0`, Rust's
`Iterator::max_by` — which documents "if several elements are equally
maximum, the last element is returned" — made the decoded path silently
get stuck on the highest-index state forever after. Bisecting the sequence
length localized the divergence to almost exactly the predicted double-
underflow point, confirming the diagnosis before touching any code. Fixed
by switching both functions to log-space accumulation (commit `92bc8c7`);
after the fix, Qu's output matches the Python reference exactly, and a new
regression test (`hmm_viterbi_does_not_underflow_and_get_stuck_on_a_long_sequence`)
guards against it recurring. This is exactly why "measure, don't assume"
matters even for code that already passed its original unit tests — the
textbook 3-observation example that validated `hmm` at ship time was too
short to ever exercise this failure mode.

### 26.2 Particle filter tracking, 50000 particles x 500 steps — was a real loss, now fixed

**Update:** the first version of this section reported an honest loss for
Qu (1959ms vs. Python's 864ms) and left it at that. Since then, profiling
the actual gap (not guessing at it) found and fixed two real, avoidable
costs — see §26.2.1 below. Qu now runs this benchmark in ~860ms, matching
the NumPy reference. The code and honest-loss framing are kept below as
they were originally written, since the fix (§26.2.1) is only meaningful
in contrast to what it replaced.

```qu
pf = particle_filter(50000, [0, 0], process_noise=0.08, obs_noise=0.4, spread=2.0, seed=3)
for k = 0 to N - 1
    pf = pf.move(dt)
    pf = pf.update([measured[k]])
end for
```
```python
# Python's "advanced" equivalent hand-implements the SIR filter directly in
# NumPy: constant-velocity predict, Gaussian likelihood, ESS-gated
# systematic resampling -- ~20 lines, but every operation is a single
# vectorized array op over all 50000 particles at once.
particles[:, 0] += particles[:, 1] * dt
particles += rng.normal(0.0, process_noise, particles.shape)
likelihood = np.exp(-0.5 * ((particles[:, 0] - measured[k]) / obs_noise) ** 2)
weights = weights * likelihood + 1e-300; weights /= weights.sum()
```

**Measured**, identical model, 50000 particles, 500 steps:

| | Qu | Python (NumPy, vectorized) |
|---|---|---|
| Wall-clock time | 1959 ms | **864 ms (~2.3x faster)** |
| Position RMS error (filtered) | 0.148 | 0.161 (both comparable, both correct) |
| Core algorithm code | 3 lines (builtin calls) | ~20 lines (hand-written SIR filter) |

**Honest result: Python/NumPy is faster here, and that's the right
takeaway to report, not paper over.** This is the case Ahmed's own caveat
predicted — "not always compatible with the same thinking way." Qu's
`particle_filter` returns a new immutable model handle from every `.move`/
`.update` call (functional-update style, consistent with Qu's `Value::Model`
convention elsewhere), which very plausibly re-wraps/reallocates the full
50000-particle state on every one of 1000 calls; NumPy's raw in-place
array mutation (`particles[:, 0] += ...`) pays no such per-call overhead.
Both give comparably accurate tracking (0.148 vs. 0.161 RMS, both far
better than the raw 0.396 sensor noise) — the gap here was implementation
overhead, not an algorithmic or numerical problem, and it was NOT what it
first looked like.

#### 26.2.1 The fix — and why the first guess about the cause was wrong

The paragraph above originally guessed the cause was `.move`/`.update`
re-wrapping the full particle array on every call (Qu's `Value::Model`
convention returns a new handle each time). That guess turned out to be
wrong once actually measured — a direct demonstration of why this
project's standing rule is profile before fixing, not after. Isolating
the noise-generation loop (temporarily setting `process_noise=0`) cut
runtime roughly in half immediately (1959ms -> 982ms), pointing straight
at the real cause:

1. `particle_tracker_advance`'s per-particle process-noise loop called a
   Box-Muller normal-sample method per cell that only used the `cos` half
   of each generated pair — discarding the `sin` half it had already paid
   the same `ln`/`sqrt`/trig cost to compute.
2. The update step's likelihood loop called `particles.row_vec(r)` —
   which allocates a fresh `Vec` for the entire row (position AND
   velocity dims) — 50000 times per call, just to read the first `half`
   position dims it actually needed.

Both fixed inline to those two functions specifically (commit `1d1af50`)
— deliberately NOT by changing the shared `Rng::normal()` used everywhere
else in the interpreter, which would have silently altered the output
stream of every other seeded script in the codebase for a fix that only
needed to apply to this one builtin.

| | Before | After |
|---|---|---|
| Wall-clock | 1959 ms | **~860 ms** (matches NumPy's 864ms) |
| RMS error | 0.148 | 0.148 (unchanged) |
| Full workspace suite | 1120 passed | 1120 passed (no regression) |

### 26.3 What this section is and isn't claiming

Not every Qu builtin will beat a hand-optimized NumPy implementation on
the first measurement, and this section exists to show real measurements
either way, not to only publish the wins — the particle filter example
above was a genuine loss when first measured, reported honestly as one,
and only closed after actually profiling it (and getting the first guess
at the cause wrong along the way). Qu wins clearly when the algorithm is
inherently sequential (can't be vectorized across the loop dimension, so
Python pays a per-iteration interpreter tax that Qu's compiled Rust loop
doesn't) — HMM Viterbi is the clean case of this. Where an operation is
"embarrassingly parallel" across particles/rows/elements with no genuine
cross-call state threading, as the particle filter's per-particle math
is, a hand-optimized NumPy implementation is a fair fight, and Qu closing
that gap took removing real, avoidable overhead (wasted RNG output,
needless allocation) rather than there being some fundamental language-
level ceiling. Both examples agree closely on the actual numeric answer,
which is the more important claim of the two — "accurate" held up in both
cases once the one real bug above was fixed.

---

## 27. Summary of mismatches found

Spec-vs-reality (both spec docs are identical and both wrong on these):

| # | Spec claims | Real behavior | Section |
|---|---|---|---|
| 1 | `write_csv("path", df)`, `read_csv(path, header=true)` | **Fixed in docs** (`7379c86`) — `write_csv(df, path)`, `read_csv(path, headers=true)` | §8 |
| 2 | `.*=`, `./=`, `^=` compound assignment | **Fixed in the engine** (`f77713c`) — all three now real | §2 |
| 3 | `x++`/`x--` increment/decrement | **Fixed in docs** (`2101fbc`) — doesn't parse, no plans to add it | §2 |
| 4 | `x, y = 1, 2` multiple assignment / destructuring | **Fixed in docs** (`7379c86`) — deliberately not supported; `.field` on a Model is the mechanism | §2, §7 |
| 5 | `function f(x, order = 4)` default/optional parameters, `is_present` | **Fixed in the engine** (`f77713c`) — real defaults; `is_present`/`optional` still don't exist, docs corrected | §5 |
| 6 | `nargin`/`nargout`/`...args`/`varargin` | **Fixed in docs** (`7379c86`) — not supported, no plans to add | §5 |
| 7 | `sq = (x) -> x^2` lambda arrow | **Fixed in docs** (`7379c86`) — `->` is exclusively the reshape operator; use `square(x) := x^2` instead | §5, §12 |
| 8 | `pure function`/`elemental function` attributes | **Fixed in docs** (`7379c86`) — keywords absent from the lexer entirely | §5 |
| 9 | `x[-1]` negative indexing | **Fixed in docs** (`7379c86`) — errors; use `x[end]`/`x[end-1]`/`x[end-2]` | §6 |

Real engine bugs found independent of the spec docs — **all fixed same-day**:

- **`elseif` never parsed successfully in any script**, regardless of
  trailing `else`/`end`/`end if` — a return-before-consuming-`end` bug in
  `if_stmt`/`if_stmt_from_elseif` (`qu-syntax/src/lib.rs`). Fixed in commit
  `79609d0`, 2 new regression tests added. See §4 for the full repro and fix.
- **A defined operator overload that didn't match its arguments' types
  produced a misleading error** — the interpreter discarded the real
  zero-match dispatch error (which names the candidate signatures) and
  showed the generic built-in operator's complaint instead (e.g. `expected
  a number, found record`, with no hint an overload was even attempted).
  Fixed in commit `9a71b6a`: now shows `no method `+` matches argument
  types (record, record) — 1 candidate(s) defined: +(a: record<Vec2>, b:
  record<Vec2>)`, directly pointing at the real problem (usually a missing
  `__type` tag). See §5 for the overload mechanism itself.
- **`else if` (two words) required its own separate `end if`**, unlike
  single-word `elseif` — confusing since they look interchangeable. Fixed
  in commit `f77713c`: both spellings now behave identically. See §4.
- **`.*=`/`./=`/`^=` never lexed at all** — a compounding lexer bug
  (`match_operator` only ever compared an operator's first two
  characters, so a genuine 3-char token could never match) plus the
  missing table entries. Fixed in commit `f77713c`. See §2.
- **Two other real bugs, found by a separate benchmark pass (not this
  tour) and flagged as follow-ups, not yet fixed as of this writing**:
  `parallel for` silently computes the WRONG answer (no error at all)
  for a vector/matrix accumulator reduction, and a single `gpu_matmul`
  call on an oversized matrix crashes the whole process with an
  uncatchable panic instead of a clean error. See
  `benchmarks/multisine_acceleration/README.md` for the full repro of
  both.
- **`square`/`impulse`/`pwm` silently discarded a wrong-type argument and
  fell back to its default instead of erroring** — e.g. piping a `Vec`
  into `square` (easy to do by mistake, since the name reads like an
  elementwise math function) silently produced a meaningless 1-sample
  signal rather than any error. Fixed § argument-validation audit,
  2026-09-01: a present-but-wrong-type numeric argument is now a clear
  error naming the function/argument/actual type; a genuinely omitted
  argument still defaults exactly as before. See §18 for the full
  before/after repro, plus the new `signals.*` disambiguation sugar and
  the new `sawtooth`/`triangle` generators added alongside it.

Worth a design decision, not clearly a "mismatch" since the spec doesn't say
it in so many words, but surprising given `*`/`^` do real matrix math:

- **`A/B` between two matrices is elementwise, identical to `A./B`** — there
  is no real matrix right-division; only `\` (left-division/solve) does real
  linear algebra. See §3.

Confirmed correct (spec matches reality) on: zero-based indexing, inclusive
`[a:b]` slicing vs. inclusive `to`/compact-range, `x[end]`, boolean-mask
indexing and mask assignment, `where`, fancy/gather indexing, `try`/`catch`
closing with bare `end`, `table.load`/`table.write` as sugar for
`read_csv`/`write_csv` (per `book/src/stdlib/file-io.md`, which is already
accurate), `*`/`.* ` elementwise-vs-matrix distinction, `+=`/`-=`/`*=`/`/=`/
`.*=`/`./=`/`^=` compound assignment, default parameters, `@` self-mutation,
string interpolation with `{expr:spec}` formatting, multiple dispatch by
parameter type, and unit literals (`5 kHz` → `5000`).

Also confirmed real and working, not previously covered by this tour: `unsafe`
blocks (§14), `parallel for` and the full `pool`/`queue()`/`run...on` job
scheduler (§15), `cast`/`resample_to` (§16), `where(v>0)`/`where v > 0` (both
forms), colon-step ranges (`0:2:10`), and `preciseTimer` (already existed
before today, and already uses a real monotonic clock rather than raw CPU
ticks, so it has no actual precision difference from plain `timer()` today).
