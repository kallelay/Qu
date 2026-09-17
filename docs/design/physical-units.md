# Physical units as a tracked runtime type — design document

Status: **design + phases 1-4 implemented** (phase 3, full SI exponent-vector
dimensional analysis, and phase 4, wiring `x in Y`, landed 2026-09-16 per
Ahmed's ruling that day — see the table below). This document is the "dedicated
design discussion" for the two related BACKLOG.md rows:

> Physical units as a *static type* catching `V + F` at compile time —
> **QUEUE** — Gap: today units scale to SI at eval time; they're not a
> distinct type the checker can reject a mismatch on. Real, valuable, not
> started. (BACKLOG.md, "original verdict table")

> Affine unit type distinguishing `degC`/`degF` from ordinary scaled units
> (no `degC + degC`, only `degC - degC -> K` and `degC + K -> degC`) —
> **QUEUE** — A real, specific bug class (silently averaging Celsius by
> summing) not previously called out. Smaller in scope than the general
> "units as a static type" row already queued, and could land as a runtime
> check even before static unit types exist. (BACKLOG.md:1920)

Grounded entirely in the current interpreter/parser/lexer as they exist
today (2026-08-27), verified by reading `engine/crates/qu-lexer/src/lib.rs`,
`engine/crates/qu-syntax/src/lib.rs`, and `engine/crates/qu-interp/src/lib.rs`
directly, not assumed — mirroring `docs/design/multiple-dispatch.md`'s own
process.

## 0. What exists today (verified)

**Lexer (`qu-lexer`)**

- `UNITS: &[&str]` (line 100) is a flat list of 38 recognized unit-suffix
  spellings: frequency (`Hz kHz MHz GHz`), time (`s ms us ns`), voltage
  (`V mV kV`), current (`A mA uA`), resistance (`Ohm mOhm kOhm MOhm`),
  capacitance (`F uF nF pF`), inductance (`H mH uH`), power (`W mW kW`),
  logarithmic/dimensionless-passthrough (`dB dBm rad cycles samples`),
  angle (`deg` — scaled by pi/180 to `rad`, the SI base already in this
  list), binary byte prefixes (`MiB GiB KiB` — scaled by 1024^n), one
  temperature unit (`degC`, `degF`), and
  bare SI magnitude prefixes with no base unit (`k M G m u p a`, e.g.
  `2k == 2000`). `is_unit(s)` (line 110) is a linear `contains` check
  against this fixed list — deliberately not "any identifier," specifically
  to avoid swallowing `2 x`-style adjacency (comment, line 98).
- A number token immediately followed by one of these becomes a single
  `Tok::UnitLit(f64, String)` at the lexer level, OR (depending on call
  site — see below) the parser recognizes `Tok::Int`/`Tok::Float` followed
  by a unit `Ident` and constructs the same shape itself. Either path lands
  on one AST node: `Expr::Unit(f64, String)` (`qu-syntax/src/lib.rs:325`).

**Parser (`qu-syntax`)**

- `primary()`'s `Tok::Int`/`Tok::Float` arms (`~line 2020`) call
  `unit_suffix()` (line 2131) right after consuming the numeric literal;
  if the very next token is a known unit identifier, it's consumed and
  wrapped into `Expr::Unit(n, u)`. **This only fires directly after a bare
  numeric literal** — `unit_suffix()` is not called anywhere else (verified
  by grep: its only call sites are the `Tok::Int`/`Tok::Float`/`Tok::UnitLit`
  arms of `primary()`). Concretely, `[20, 30] degC` (used in
  `catalog/qu_ml_report.qu:29`, see §5) does **not** attach `degC` to the
  array — the matrix literal branch (`Tok::Op("[")` → `matrix_literal()`)
  never calls `unit_suffix()` afterward, so that line is either a parse
  error or (if `degC` there parses as something else, e.g. a dangling
  statement) not actually exercising unit-array semantics. This is real,
  useful evidence for §5: the one non-trivial `degC`-on-a-collection
  example in the repo isn't even reachable through today's grammar, so
  there is no *working* prior art combining units with vectors to be
  compatible with.
- Separately, `unit_expr()` (line 1734, grammar `UnitExpr ::= Or ['in'
  UnitName]`) recognizes a trailing `in <UnitName>` on any expression
  (`x in Ohm`, used for e.g. `fit.a in mOhm` in `catalog/qu_ml_report.qu:73`)
  and produces `Expr::InUnit { value, unit }`.

**Interpreter (`qu-interp`)**

- `Expr::Unit(v, u) => Ok(Value::Num(apply_unit(*v, u)))` (line 4075) —
  the *entire* current semantics of a unit literal. `apply_unit` (line
  29153) is a flat `match u { "kHz" => 1e3, "mV" => 1e-3, ... , _ => 1.0
  /* dB, degC, cycles, samples: value passes through */ }` returning a
  scale factor, multiplied into `v` immediately, at eval time. The result
  is a bare `Value::Num` — **there is no `Value::Unit` variant today, no
  tracking of what unit a number "is" once created, and no distinction at
  all between `Value::Num(5.0)` written as `5` and one written as `5V`**
  once evaluation is done. This is BACKLOG.md's own "DONE" note for
  "physical units as literals," and its own "QUEUE" note for the type gap.
- `Expr::InUnit { value, .. } => self.eval(value), // already SI` (line
  4179) — **`x in Ohm` is a complete no-op today.** It does not rescale
  for display, does not check anything, and does not tag anything; it
  evaluates the inner expression and silently discards the unit name.
  This means the `{fit.a in mOhm}` interpolation in
  `catalog/qu_ml_report.qu:73` would today print the raw SI-ohm value
  (e.g. `0.012`), not `12`, contrary to what the surrounding comment
  ("§42.6 ASCII-canonical units") suggests was intended. `qu_ml_report.qu`
  is not referenced anywhere in `engine/crates/qu-interp/tests/acceptance.rs`
  (grep confirms zero hits) — it is an aspirational/example script, not a
  CI-gated test, so this inert `InUnit` behavior has had no chance to be
  noticed or relied upon by a passing test suite.
- `Value` (line 138) is now a much larger enum than `multiple-dispatch.md`'s
  own 2026-08-24 survey recorded (that doc counted 20 variants; today's
  enum has 29: the new ones are `EnumType`, `EnumVal`, `Channel`, `Tensor`,
  `TcpListener`, `TcpConn`, `Queue`, `Pool`, `Dict`). `Value::type_name()`
  (line 742) is a fully exhaustive match, no wildcard arm — every variant
  must have an explicit entry, which is exactly the mechanism that forces
  every other exhaustive match over `Value` (`truthy`, `as_num`,
  `display_value`, `binop`'s type-boundary checks, ...) to be revisited
  when a new variant is added. `value_unchanged` (line 29116, used only by
  `parallel for`'s merge-safety check) is the one exhaustive-looking match
  that already ends in a conservative wildcard (`_ => false`).
- Multiple dispatch phases 1-3 (commits `4eec135`, `92bc834`, `0ec878b`,
  landed 2026-08-27, same day as this design) added `is_operator_overload_candidate`
  (line 2524): a value is a "user-overload candidate" for `+ - * / == < >`
  iff it is **not** one of `Num, Bool, Str, Vec, Mat, Complex, CVec, CMat,
  Signal` — anything else (today: `Table, Worker, Mutex, Semaphore, Model,
  List, Record, EnumType, EnumVal, Image, Timer, File, Channel, Tensor,
  TcpListener, TcpConn, Queue, Pool, Dict`) routes through
  `binop_or_user_method`, which tries the hardcoded `binop`/`unop` cascade
  first and only on error falls back to a user-defined `methods[op]`
  overload. This is the exact hook §6 below reasons about for `Value::Unit`.
- `binop` (line 5533) is the same priority cascade `multiple-dispatch.md`
  already documented: string-concat, then complex+matrix, then flat
  complex, then real matrix, then `Signal`-aware, then a plain `match op`
  over `map2`/`compare`. A new value kind that needs its own arithmetic
  rules is added as an early `if` check in this same cascade, exactly the
  way `Signal` already is (line 5564).

**Prior design notes found.** Searched `BACKLOG.md`/`IMPL.md` for "unit",
"degC", "degF", "affine", "dimensional analysis": no prior design doc,
prototype, or abandoned attempt exists — this is a green field, same as
multiple dispatch was. `docs/design/multiple-dispatch.md` itself explicitly
flags this exact gap as out of its own scope (§8: *"Physical units: fully
orthogonal... If/when units become a real static type... a `unit<V>`-style
type tag could in principle join the `TypeTag` vocabulary... but that is
new work contingent on the units item landing first"*) — confirming this
document is the correct place to do that follow-on work, not a duplicate
of it.

## 1. What "static" honestly means here

Qu has **zero** compile-time/static-analysis pass of any kind today.
Confirmed two ways: (a) `multiple-dispatch.md`'s own survey states this
plainly and nothing has changed it since; (b) a repo-wide grep for
`cranelift` (the one JIT/AOT path ever mentioned in BACKLOG.md as a future
possibility) returns zero hits — there is no bytecode compiler, no type
checker, no separate "compile" phase at all. `qu-syntax` produces an AST;
`qu-interp` walks it directly. The one thing that resembles "compilation"
(`try_run_compiled`/`compiled_funcs`, multiple-dispatch.md's §7) is a
runtime fast-path cache for all-`f64`-argument functions, not a type-
checking pass — it never rejects a program, it only skips `Value` boxing
for a call shape it's already seen succeed.

Given that, "physical units as a static type" **cannot honestly mean**
"the interpreter refuses to run a program with a unit mismatch before
executing it," the way it would in Rust, Haskell, or F#'s units-of-measure
extension. What is actually achievable, and what this document commits
to, is the same sense multiple-dispatch.md used for its own "static
dispatch": **the type/dimension travels with the value and is checked at
the point of use** — a `Value::Unit(f64, UnitTag)` is a distinct runtime
type from `Value::Num`, `Value::type_name()` reports it as its own kind,
and an operation between incompatible units is rejected with a specific,
loud, "operator `+` … " runtime error **the first time the two values are
actually combined** — not silently producing a plausible-looking wrong
number, and not deferred to whatever much later point the corrupted value
happens to cause a visible symptom (e.g. `mean()` quietly averaging
Celsius readings today). This is "static" only in the sense that the type
is now a first-class, checked property of the value at every operation —
never in the sense of "caught before the program runs." Any future write-
up that calls this "compile-time" would be overselling it; this document
does not.

## 2. The specific `degC`/`degF` affine-unit bug class

Every unit `apply_unit` scales today (`Hz`, `V`, `A`, `Ohm`, `F`, `H`, `W`,
and their SI-prefixed forms) is a **linear/ratio scale**: converting to SI
base units is pure multiplication (`3 kHz = 3 * 1e3 = 3000 Hz`), and two
values in the same family combine sensibly under `+`/`-` regardless of
which prefixed spelling produced them (`3 kHz + 500 Hz` is a perfectly
meaningful `3500 Hz`). Celsius (and Fahrenheit, once it exists) is
categorically different: it is an **affine** scale, `K = C + 273.15`
(and `K = (F - 32) * 5/9 + 273.15`) — there is a nonzero offset, not just
a scale factor. This breaks two things a linear-unit design would
otherwise take for granted:

- **`degC + degC` is not "a wrong number," it is not a physically
  meaningful operation at all.** `10 degC + 20 degC` naively summed as
  plain numbers gives `30`, which happens to look like a plausible
  temperature — this is precisely what makes it dangerous. `apply_unit`'s
  own `_ => 1.0` catch-all (comment: `"dB, degC, cycles, samples: value
  passes through"`) means today `10 degC` and `20 degC` are already
  indistinguishable from the plain numbers `10.0` and `20.0` by the time
  they reach any `+`. There is no SI base unit being scaled to here in the
  first place (unlike `V`/`Hz`/etc.), so "just convert to SI and treat as
  a plain number" was never even attempted for this one unit — it silently
  degraded to "don't scale at all," which is coincidentally *why*
  `mean([10 degC, 20 degC])` (sum then divide) still gives the numerically
  correct answer (`15`) purely because averaging is invariant to any
  shared additive offset cancelling in the division — a lucky accident of
  the specific operation, not evidence the representation is sound. The
  moment a script writes `total = t1 + t2` (a "combine two readings" bug,
  not an averaging step) the result is a number with no physical meaning,
  and nothing about today's runtime hints that anything went wrong.
- **Multiplying/scaling a Celsius value is *also* physically invalid**,
  for the same affine reason: "twice as hot" is well-defined on Kelvin
  (`2 * 300 K = 600 K`) but not on Celsius (`2 * 27 degC != 2 * 300 K`
  converted back) — the offset does not scale. This is a second,
  independent consequence of "affine, not linear," not just an add/sub
  concern, and phase 1 (§8) rejects it too.
- **Comparisons and differences remain perfectly meaningful**: `degC -
  degC` is a temperature *difference*, and because the additive offset is
  the same on both sides, it cancels exactly — `(20 degC) - (10 degC) ==
  10 K`, correctly, regardless of what the offset actually is. Likewise
  `10 degC < 20 degC` is unambiguous. And a temperature can be validly
  *shifted* by a plain delta (`20 degC + 5` meaning "5 degrees warmer" is
  fine — the result is still a temperature, just moved along the same
  affine line), which is different from adding two temperatures together.

Phase 1 (§8) implements exactly this distinction: temperatures are **not**
freely `+`-combinable with each other or `*`/`/`-scalable at all, while
`-` between two temperatures and ordinary comparisons are allowed and
correctly cross-scale-aware (`10 degC` vs `50 degF`, once `degF` exists,
converts through Kelvin rather than comparing raw numbers).

## 3. Dimensional analysis: flat tags, not full SI vectors — and only one family for phase 1

Two designs were weighed for how "compatible" is decided:

- **Full SI dimensional analysis**: every unit is a 7-exponent vector over
  the SI base dimensions (length, mass, time, current, temperature,
  amount, luminosity); `V/A` divides exponent vectors to derive an
  Ohm-shaped result automatically, `Hz * s` cancels to a dimensionless
  exponent vector, etc. This is the theoretically "complete" answer, and
  is what a from-scratch units library (e.g. Rust's `uom` crate) does.
- **Flat named-tag system**: units are opaque string/enum tags (`"Hz"`,
  `"V"`, `"Ohm"`...) with a small, hand-coded compatibility table (same
  family = compatible; nothing else is), and no general multiply/divide
  derivation (`V / A` does not automatically become "Ohm" — it stays
  whatever the plain-number division already does today).

**Recommendation: flat tags, explicitly, and only build the one family
(`degC`/`degF`) phase 1 actually needs.** Reasons:

- The concrete, cited bug (§2) needs precisely zero cross-family
  derivation to fix — it needs "these two temperatures are not freely
  addable" and "these two temperatures compare correctly across scale."
  Building `V/A -> Ohm`-style derived-unit algebra is real, separable
  work that nothing in this document's stated bug class requires.
- Every existing linear unit (`V`, `A`, `Hz`, `Ohm`, `F`, `H`, `W`, and
  their prefixed forms) is used *pervasively* in real scripts as plain
  numbers immediately after the literal (§5) — `Fs = 5 kHz` then `Fs / 2`,
  `N * dt`, passed as plain `f64` into `design_lowpass(Fs=48 kHz, ...)`,
  etc. Routing all of those through a tracked `Value::Unit` in phase 1
  would touch the single highest-traffic literal spelling in the DSP/EIS
  half of this codebase (`BACKLOG.md`'s own signal-processing/EIS
  emphasis) for a bug class (`V + A`) that, while real, has no cited
  concrete incident the way the Celsius-averaging bug does.
- Building general dimensional algebra "a little bit" (e.g. only for `*`/
  `/`, not full exponent-vector inference for every builtin that touches
  a number) is exactly the "half-build general dimensional analysis"
  outcome the task explicitly warns against. Better to say plainly: **phase
  1 ships flat, hand-coded temperature-affine semantics only; a
  general flat-tag compatibility table for the linear EE units (`V`, `A`,
  `Hz`, `Ohm`, ...) is real, valuable, follow-on work (§8 phase 2), and
  full SI exponent-vector algebra (`V/A -> Ohm`) is bigger still and not
  scoped here at all.**

## 4. Runtime representation

```rust
/// A unit-tracked quantity: an affine or linear physical unit tag riding
/// along with a plain f64 magnitude, checked at the point of use rather
/// than silently collapsed to a bare `Value::Num` the way every OTHER
/// unit literal (`V`, `Hz`, `Ohm`, ...) still is (§5 — deliberately
/// unchanged). Phase 1 has exactly one tag family (temperature); a
/// `Family(&'static str)` variant for linear EE units is the natural
/// phase-2 extension (§8) but is not added here to avoid an unused,
/// half-built variant with no literal that ever constructs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnitTag {
    /// An affine temperature scale — NOT freely `+`-combinable with
    /// another temperature or `*`/`/`-scalable at all (§2). The f64
    /// magnitude is stored in this scale's own units (e.g. `Temp(Celsius)`
    /// stores raw Celsius, not Kelvin) so a plain `degC` value still
    /// prints and round-trips as the number the user wrote.
    Temp(TempScale),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempScale { Celsius, Fahrenheit }
```

and one new `Value` variant, `Value::Unit(f64, UnitTag)`, added next to
`Value::Num` (documented the same way `Value::Signal`'s own doc comment
explains its rationale, per this codebase's convention).

**Why store the magnitude in its own declared unit, not pre-converted to
Kelvin?** So a `degC` literal still displays and compares as the number
the user wrote (`display_value` prints `"10 degC"`, not a converted
Kelvin figure the user never typed) — conversion to Kelvin happens only
internally, at the moment two different temperature scales must actually
be compared (§2's `10 degC` vs `50 degF` case).

**Literal evaluation changes, narrowly.** `Expr::Unit(v, u)`'s evaluation
(line 4075) grows exactly one new branch:

```rust
Expr::Unit(v, u) => match u.as_str() {
    "degC" => Ok(Value::Unit(*v, UnitTag::Temp(TempScale::Celsius))),
    "degF" => Ok(Value::Unit(*v, UnitTag::Temp(TempScale::Fahrenheit))),
    _ => Ok(Value::Num(apply_unit(*v, u))),  // unchanged for every other unit
},
```

`degF` is also added to `qu-lexer`'s `UNITS` list (it is not lexable at
all today — §0) so `98.6 degF` becomes a real literal for the first time.

## 5. Interaction with existing code: strictly opt-in, narrowly scoped breaking change

**This is the one place this document must be maximally honest, per the
task's own instruction to verify rather than assume.** Grepped every
`.qu` file in the repo (excluding `.claude/worktrees/*`, other agents'
in-progress trees) for unit-suffix usage:

- `V`, `A`, `Hz`, `kHz`, `mOhm`, `Ohm`, `dB`, `dBm`, `ms`, `cycles`, etc.
  appear extensively and are used as plain numbers immediately afterward
  (`benchmarks/filter_design/apply_scale.qu`, `engine/examples/demo_signal.qu`,
  `catalog/demo_signal.qu`, `catalog/qu_algorithm_testbench.qu`,
  `catalog/qu_battery_multisine_rpg.qu`, and others across `catalog/`).
  **None of this changes** — §3/§4 deliberately
  leave every non-temperature unit's evaluation byte-for-byte identical
  to today (`apply_unit(*v, u)`, unchanged function, unchanged call).
- `degC` appears in exactly one place outside docs/spec files:
  `catalog/qu_ml_report.qu:29`, `temp in [20, 30] degC` — and per §0,
  this doesn't even parse as a unit-tagged array under today's (or phase
  1's) grammar; `unit_suffix()` only fires directly after a bare numeric
  literal, not after a matrix literal. `qu_ml_report.qu` is not part of
  `qu-interp/tests/acceptance.rs` (grep-confirmed zero hits) — it is an
  example/spec-illustration file, not CI-gated.
- `degF` appears nowhere in any `.qu` file in the repo (it isn't lexable
  today at all — §0).

**Decision: `degC`/`degF` literals changing from `Value::Num` to
`Value::Unit` is a deliberate, narrow, justified breaking change to
exactly two unit spellings, made safe by the fact that the repo's only
real (non-doc) `degC` usage doesn't exercise arithmetic on it and isn't
CI-gated, and `degF` has no existing usage at all to break.** Every other
of the 37 remaining unit spellings is completely unchanged — this is
**not** a breaking change to "unit literals" as a system, only to the two
specific spellings whose old behavior (`_ => 1.0`, i.e. "pass through
unchanged" per `apply_unit`'s own comment) was already the least
defensible part of the existing design (it wasn't even doing SI
normalization for these two, unlike every other unit). Recommendation
matches multiple-dispatch.md's own bias here: **prefer the safer, opt-in-
shaped path** — and the safest opt-in boundary available, given this
audit, is "only the two units nothing in the tested codebase relies on
today, not a general opt-in syntax that would need its own new grammar."
A hypothetical alternative (a brand-new syntax, e.g. `unit(10, "degC")`,
leaving bare `10 degC` untouched) was considered and rejected as
needless: it would leave the *actual bare literal* users write
(`10 degC`) exactly as buggy as today, protecting nobody who writes the
natural spelling, in exchange for a "zero blast radius" guarantee this
audit shows isn't needed for a change this narrow.

`Value::Unit` deliberately does **not** support `.as_num()` (i.e. the
existing exhaustive `as_num` match's wildcard arm, `_ => Err(...)`,
applies to it unchanged, needing no new arm) — passing a temperature into
any plain-number builtin (`sqrt`, `mean`, a vector literal `[10 degC, ...]`,
matrix construction, ...) is a loud "expected a number, found unit
quantity" error rather than a silent unwrap, consistent with this
codebase's stated bias for loud errors over convenient-but-silent
fallbacks (multiple-dispatch.md §3, citing `expect_stmt_end`'s own doc
comment). A script that genuinely wants the raw magnitude out of a
`Value::Unit` needs an explicit accessor — out of scope for phase 1 (§8),
since no cited use case needs it yet and it's a small, separable add.

## 6. Multiple dispatch interaction

`Value::Unit` is a new runtime type, so it needs a `type_name()` entry
(`"unit"`) — one line, matching the pattern every other variant already
follows. Two smaller, concrete interactions, both already resolved by
existing machinery with no new code:

- **Dispatch tag vocabulary** (multiple-dispatch.md §1): if/when a script
  wants `function f(x: unit) ... end`, that "just works" once `"unit"`
  exists as a `type_name()` string — multiple-dispatch's `TypeTag::Named`
  matching is already generic over whatever strings `type_name()` returns,
  no change needed there.
- **Operator-overload fallback gate** (multiple-dispatch.md §5,
  `is_operator_overload_candidate`, line 2524): `Value::Unit` is **added
  to the existing "safe" exclusion list** (alongside `Num`/`Vec`/`Mat`/
  `Complex`/`Signal`/...), i.e. it does **not** become an "overload
  candidate." Reasoning: phase 1's dimensional-compatibility checks are
  implemented directly inside `binop`/`compare` (§4), not via a user-space
  fallback — there is no `methods["+"]` overload to fall back to for
  `Value::Unit` in phase 1, so routing it through
  `binop_or_user_method`'s extra indirection would be pure overhead with
  no behavioral difference (the gate's whole purpose is only useful for
  types `binop` doesn't already understand by type — `Value::Unit` will).
  This is a one-line addition to an existing `!matches!(v, A | B | C |
  ...)` list, not new dispatch logic.

No other interaction: `Value::Unit` does not participate in the `Model`
protocol, record-tag refinement, or any of multiple dispatch's phase 2/3
machinery.

## 7. What this does NOT solve

- **Not compile-time verification** — see §1. Every check is a runtime
  check at the point two values are combined; a program that would fail
  is only caught when that operation actually executes, exactly like
  every other type error in this entirely-dynamic interpreter.
- **No general dimensional algebra** — see §3. `V / A` does not become an
  "Ohm"-tagged result; it stays exactly what plain-number division
  already does today (a bare `Value::Num`), because `V`/`A`/`Ohm`
  literals are entirely untouched by this phase. `Hz * s` does not cancel
  to a tracked dimensionless value for the same reason.
- **No tracking for any unit other than `degC`/`degF`** — `5V`, `3kHz`,
  `12 mOhm`, etc. keep producing a bare `Value::Num` exactly as before;
  `5V + 3A` still silently type-checks as `8` today, unchanged by this
  phase. This is the single largest thing explicitly deferred (§8, phase
  2) — flagged here rather than silently left implicit, since it is the
  literal example the original BACKLOG.md row (`V + F`) uses.
- **No user-defined units** (`unit lsb = 1/32768.0`) — a separate,
  already-`QUEUE`'d BACKLOG.md row, orthogonal to this one (it's about
  *naming* a scale factor, not about *tracking* a unit at runtime).
- **No display-side fix for `x in Ohm`** — §0 notes `Expr::InUnit` is
  currently an inert no-op; this document does not touch it. It would be
  natural follow-on work (once `Value::Unit` exists generally, `x in Y`
  could mean "convert/tag as `Y`" instead of doing nothing), but making
  `InUnit` do real work for the 37 non-temperature units is exactly the
  phase-2-sized "linear unit tracking" scope this document deliberately
  defers, so it's left alone rather than half-wired to only work for
  temperature.
- **No accessor to strip a `Value::Unit` back to a plain number** — see
  §5's closing note. A script that hits the new "expected a number, found
  unit quantity" error where it doesn't want the safety has no escape
  hatch yet; small, separable follow-on if it turns out to matter.

## 8. Recommended incremental build order

| Phase | Scope | Size |
|---|---|---|
| **1 (this document, implemented 2026-08-27)** | `Value::Unit(f64, UnitTag)` with exactly one tag family, `UnitTag::Temp(TempScale::{Celsius,Fahrenheit})`. `degF` added to `qu-lexer::UNITS`. `Expr::Unit` evaluation special-cases `"degC"`/`"degF"` to produce `Value::Unit` instead of `apply_unit`'s old pass-through; every other of the 37 unit spellings is byte-for-byte unchanged. `binop`/`compare` grow a `Value::Unit`-aware arm: `Temp + Temp` and `Temp * anything`/`Temp / anything` are hard runtime errors naming the bug class; `Temp - Temp` yields a plain `Value::Num` Kelvin delta; `Temp +/- plain Num` shifts the temperature (stays a `Value::Unit`, same scale); comparisons (`== != < > <= >=`) between two `Value::Unit(Temp)`s convert through Kelvin first (so `degC` vs `degF` compares correctly); comparisons/±between a `Value::Unit(Temp)` and a plain `Value::Num` treat the number as being in the temperature's own declared scale. `type_name()` → `"unit"`; `truthy`/`display_value` grow the one needed arm each; `as_num`'s existing wildcard already rejects it (no change needed); `is_operator_overload_candidate` gets `Value::Unit` added to its exclusion list (§6). | **Small–Medium** — one new enum, one new `Value` variant, exhaustive-match fallout across `type_name`/`truthy`/`display_value` (each a one-line addition, compiler-enforced so nothing is missed), plus the real work: the `binop`/`compare` arithmetic rules themselves and their tests. |
| **2 (implemented — see below)** | Extend `UnitTag` with `Family(&'static str)` for the 8 linear families already lexed (frequency `Hz`, time `s`, voltage `V`, current `A`, resistance `Ohm`, capacitance `F`, inductance `H`, power `W`) plus a small hand-coded compatibility table (same family ⇒ freely `+`/`-`/comparable; different family ⇒ hard error). Route the 34 remaining non-dimensionless, non-prefix unit literals through `Value::Unit` the same way `degC`/`degF` are routed in phase 1. **This is the phase that actually fixes the BACKLOG-cited `V + A` example** and is a materially bigger lift than phase 1: every real script currently doing plain arithmetic on a unit-derived number (§5's audit) would need to keep working, meaning `Value::Unit` needs to compose transparently with ordinary `f64` arithmetic wherever dimensional tracking isn't the point (division by a plain scalar, use inside `sqrt`/`mean`/etc.) — a much larger compatibility surface than temperature's deliberately narrow, mostly-`+`/`-`-only ruleset. Resolved by a deliberate divergence from `Temp`'s precedent: `as_num` (and `map1`, the elementwise-math helper) unwrap a `Family`-tagged quantity to its plain SI magnitude silently, same as today, while `Temp` stays rejected — so `design_lowpass(Fs=48 kHz, ...)`/`sqrt(...)`/etc. keep working, and the new loud errors are scoped to exactly the two things that were never meaningful before: cross-family `+`/`-` (`V + A`) and any `*`/`/` between two tagged quantities at all (`V * A` does not silently become a plausible-looking wrong number, nor does it derive `W` — that's phase 3). `*`/`/` by a plain scalar still rescales and keeps the tag, matching the §5 audit's dominant real usage (`Fs / 2`). | **Large** |
| **3 (implemented 2026-09-16)** | Full SI base-dimension exponent-vector algebra (`V/A -> Ohm`, `4 V / 2 A -> 2 Ohm`, `V * A -> W`) rather than phase 2's flat named-family table. Ahmed ruled explicitly, 2026-09-16, that `*`/`/` between two tagged quantities MAY derive a new dimension — superseding phase 2's blanket "no cross-unit derivation" error. `UnitTag::Family(&'static str)` became `UnitTag::Dim(Dim([i8; 7]), Option<&'static str>)`: the exponent vector decides compatibility/derivation, the optional field is only a preferred display spelling and does not participate in equality (`PartialEq` is hand-written, not derived, for exactly this reason). The exponents are **deliberately `i8`, not rational** — Ahmed was told this makes `V/sqrt(Hz)` (noise spectral density) unrepresentable, and chose integers anyway to ship; see `Dim`'s doc comment in `qu-interp/src/lib.rs` for the recorded deferral. A dimension with no `NAMED_DIMS` entry displays as a composition of base units (`Dim::compose`, e.g. `10 kg m^2 s^-3`) rather than an invented canonical name. | **Large** |
| **4 (implemented 2026-09-16)** | Wire `Expr::InUnit` (`x in Y`) into `Value::Unit` for real display/tagging semantics, replacing the previous silent no-op (`Expr::InUnit { value, .. } => self.eval(value)`, which discarded the unit and evaluated `value` alone — so `5 km in mm` printed `5000`, the SI metres, and a wrong-dimension conversion like `5 km in s` was silently accepted). `Interp::eval_in_unit` now checks the source and target `Dim`s match (erroring with both compared, via `Dim::compose`, when they don't), converts the SI magnitude to the target unit's own scale, and retags with the target as the new preferred spelling. Temperatures keep their own Kelvin-mediated path (`degC`/`degF`), unchanged in kind from phase 1. | **Small** |

Phase 1 alone already delivers the concrete, specifically-cited bug fix
(BACKLOG.md:1920's Celsius-averaging class) with a real runtime check that
fires the first time two temperatures are wrongly combined, at low risk
(narrowly audited, two unit spellings, one with zero prior repo usage),
and is the natural place to stop and reassess before committing to
phase 2's materially larger "track every EE unit" scope.
