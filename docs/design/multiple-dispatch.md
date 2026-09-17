# Multiple dispatch for Qu — design document

Status: **proposal for discussion, no code written**. This document is the
"dedicated design discussion" BACKLOG.md's verdict table asked for against
the row:

> User-extensible multiple dispatch (Julia-style: one `filter` name,
> behavior varies by argument type) — **ASK** — The single biggest
> architectural gap both write-ups flag. Qu today is one big builtin
> `match`, not an open, user-extensible dispatch table. Real and valuable,
> but changes the semantics of every function definition — needs a
> dedicated design discussion, not a queue slot. (`BACKLOG.md:1152`)

Grounded entirely in the current interpreter/parser as they exist today
(2026-08-24), not generic PL theory. Every claim below was verified by
reading `engine/crates/qu-syntax/src/lib.rs` and
`engine/crates/qu-interp/src/lib.rs` directly, not assumed.

## 0. What exists today (verified)

**Parser (`qu-syntax`)**

- Function parameters are `Vec<String>` — bare names, no type annotation
  syntax exists anywhere in the grammar. `param_list()` (line ~775) parses
  `expect_name()` in a loop; there is no `:` after a parameter name that
  the parser recognizes. This is a genuinely blank slate — no existing
  syntax to conflict with or half-support.
- Two function-declaration AST nodes, both single-body:
  - `Stmt::DefFn { name, params: Vec<String>, body: Expr }` — one-liner
    `name(params) := expr`.
  - `Stmt::Function { name, params: Vec<String>, body: Vec<Stmt> }` —
    block form `function name(params) … end function`.
  - There is no `Item`/module-level overload-set concept; a `Program` is
    just `Vec<Stmt>`, and a second `Function`/`DefFn` with the same name
    is only ever a *redefinition* at the interpreter level (see below),
    not an additional overload.
- `expr.method(args)` does **not** desugar in the parser at all. It parses
  as ordinary postfix chaining: `.` produces `Expr::Field { value, name }`
  (`postfix()`, line ~1279), and a following `(` wraps that in
  `Expr::Call { callee: Field{...}, args }` (line ~1267). The desugaring
  to "prepend receiver as first argument" happens entirely in the
  interpreter, at the call site, not in the AST shape.
- Binary operators (`+ - * / == < >` etc.) parse into a single
  `Expr::Binary { op: String, lhs, rhs }` node via the `bin()` helper
  (line ~1530) at the bottom of a conventional precedence-climbing ladder
  (`or_expr` → `and_expr` → `eq_expr` → `cmp_expr` → `add_expr` →
  `mul_expr` → `unary` → `power` → `postfix` → `primary`). `op` is a
  plain string (`"+"`, `"=="`, ...); there's no operator-overload table of
  any kind at parse time.

**Interpreter (`qu-interp`)**

- `Value` (line 51) is a 20-variant enum: `Num, Bool, Str, Vec(Arc<...>),
  Mat(Arc<Matrix>), Complex, CVec(Arc<...>), CMat(Arc<...>), Signal(Vec,
  f64), Mask, Table, Worker, Mutex, Semaphore, Model(Arc<ModelHandle>),
  List(Arc<...>), Record(Arc<...>), Image(Arc<...>), Timer, File, Nothing`.
  `Value::type_name(&self) -> &'static str` (line 361) is a plain
  exhaustive match returning a string tag per variant (`"number"`,
  `"vector"`, `"matrix"`, `"model"`, ...) — this is the entire "runtime
  type" concept that exists today. There is no separate `Type`/`TypeTag`
  value distinct from `Value` itself, no user-defined nominal types beyond
  the built-in `Record`/`Model` shapes, and no notion of a type hierarchy
  (nothing is "a subtype of" anything else).
- User functions live in exactly two flat tables on `Interp`
  (`struct Interp`, ~line 1075):
  ```rust
  funcs: HashMap<String, (Vec<String>, Expr)>,        // DefFn bodies
  block_funcs: HashMap<String, (Vec<String>, Vec<Stmt>)>, // Function bodies
  ```
  plus a soft-compile cache `compiled_funcs: HashMap<String, Option<CompiledFn>>`
  keyed by the same name, for an all-`f64`-argument fast path. **One name
  → one body, period.** `Stmt::DefFn`/`Stmt::Function` execution
  (`~line 2326`) is a plain `.insert(name.clone(), ...)` — defining
  `f(x)` twice silently overwrites the first definition, there is no
  arity- or type-based overload set, and no error is raised on
  redefinition.
- Call resolution (`call_named`, line 3826, and `apply_seeded`, line
  3696) is name-then-arity only: **user functions unconditionally shadow
  builtins by name** (checked first, no fallback to the builtin if the
  user's arity doesn't match), and arity is checked exactly
  (`call_block_fn`/`call_user`, ~line 3784/3808: `params.len() !=
  argv.len()` is a hard error, "function expects N argument(s), got M").
  There is no argument-type-based selection among *user* functions,
  because there is only ever one body per name to select from.
- `recv.method(args)` desugaring is exactly two lines, in `eval_call`
  (line 2991):
  ```rust
  if let Expr::Field { value, name } = callee {
      let recv = self.eval(value)?;
      let (mut argv, seed, axis, style) = self.eval_call_args(args)?;
      argv.insert(0, recv);
      return self.call_named(name, argv, seed, axis, style);
  }
  ```
  i.e. `recv.method(args)` is *literally* `method(recv, args...)` — single
  dispatch on the receiver only, folded into the exact same name/arity
  resolution as an ordinary call. `.predict(...)`/`.fit(...)` are not
  special-cased anywhere; they are ordinary builtins named `"predict"`/
  `"fit"` that receive a `Value::Model` as their first argument and
  internally `match` on `ModelHandle.kind` (a plain `String` tag,
  `model.rs:28`) to decide which fitted-model algorithm to run.
- Builtin argument-type dispatch is hand-written, per builtin, as a Rust
  `match` on `args.first()` / `arg0(&args)`. Confirmed concretely:
  - `"zeros"` (line 4931): `match args.first() { Some(Value::Model(m)) if
    m.kind == "filter" => ..., _ => Ok(make_filled(&args, 0.0)) }` — IMPL.md
    documents this explicitly as a resolved naming collision: *"Resolved
    by overloading the existing `"zeros"` match arm on argument type (a
    `Value::Model` with `kind == "filter"` routes to filter zeros;
    anything else falls through to the original `make_filled(&args,
    0.0)`) rather than inventing a second name"* (IMPL.md:2468-2477).
  - `"remove"` (line 4205): `match arg0(&args)? { Value::Timer(_) => ...,
    Value::Vec(xs) => ..., Value::List(items) => ..., other => e(...) }`.
  - `"insert"`/`"join"` follow the identical per-builtin single-`match`
    pattern.
  - The binary-operator dispatcher `binop` (line 3660) is the same idea at
    larger scale: an explicit priority cascade — string concat, then
    complex+matrix, then flat complex, then (unseen below the excerpt but
    following the same shape) real matrix, then plain elementwise — all
    hardcoded as sequential `if`/`match` guards inside one function, one
    time, for every operator name.
  - Net effect: **every one of these dispatch points is closed.** A Qu
    script cannot add a new arm to `zeros`'s match, cannot teach `+` a new
    case for a user `Record`, and cannot add a same-named alternate body
    for a `function` it defines — only editing this Rust file can.
- Performance context that any dispatch design must respect:
  `Value::Vec`/`Mat`/`CVec`/`CMat`/`List`/`Record`/`Image` are all
  `Arc`-wrapped specifically so a *read* (including "read to pattern-match
  its type for dispatch") is an O(1) refcount bump, not a deep clone (see
  `Value`'s own doc comment, line 55 onward) — this was a real, measured
  optimization (`private/README.md`), not speculative. `qu_core::matrix`
  is rayon-parallelized (`qu-core/src/matrix.rs`, `qu-core/Cargo.toml`).
  There is also a soft-compile fast path (`try_run_compiled`, line 3742)
  that bypasses `Value` boxing entirely for functions whose every call
  argument is a plain `Value::Num` — any dispatch mechanism sits in front
  of, and must not defeat, that fast path for the common numeric case.

**Prior design notes found** (searched `IMPL.md`/`BACKLOG.md` for
"multiple dispatch", "generic function", "method table", "type
annotation" — no other hits beyond what's already quoted above and in
BACKLOG.md's verdict-table row). No prior competing design exists to
reconcile with; this is a green field.

## 1. Surface syntax

Minimal, additive extension of the *existing* `function` syntax — nothing
invented that ignores what's already there. Since `param_list()` currently
requires a bare identifier and nothing else, the natural, smallest change
is an **optional** `: TypeName` suffix per parameter, where `TypeName` is
one of the strings `Value::type_name()` already returns (`number`, `bool`,
`string`, `vector`, `matrix`, `complex`, `complex vector`, `complex
matrix`, `signal`, `mask`, `table`, `model`, `list`, `record`, `image`,
`timer`, `file`, `worker`, `mutex`, `semaphore`, `none`) plus a
user-`Record`/`Model` refinement (§1.1):

```
function filter(x: signal, f: model)
    ...
end function

function filter(x: vector, f: vector)
    ...
end function

function filter(x)         -- untyped: matches ANY argument type (§5)
    ...
end function
```

Multi-word tags like `"complex vector"`/`"complex matrix"` need a spelling
that survives the identifier lexer; `qu-lexer` doesn't currently emit
these as single tokens. Proposal: keep the type annotation as a single
identifier and rename the annotation vocabulary to lexer-friendly single
words distinct from (but obviously mapping to) `type_name()`'s display
strings: `num`, `bool`, `str`, `vec`, `mat`, `complex`, `cvec`, `cmat`,
`signal`, `mask`, `table`, `model`, `list`, `record`, `image`, `timer`,
`file`, `worker`, `mutex`, `semaphore`, `none`, `any` (explicit synonym
for "untyped"). This is a one-line addition of reserved-ish contextual
words, resolved the same way `type`/`every`/`parallel`/`on` are already
contextual (checked by exact identifier text at a specific grammar
position, not reserved globally) — an existing variable named `vec` still
works everywhere else.

One-liner form gets the same suffix:

```
area(x: vec) := sum(x) * dx
area(x: mat) := sum(sum(x)) * dx * dy
```

**§1.1 Record/Model refinement.** A parameter can additionally name a
user-defined record "shape" or model "kind" for narrower matching than
the bare `record`/`model` tag, using the field the interpreter already
carries for exactly this purpose:

```
function area(shape: record<Circle>)   ... end function
function area(shape: record<Square>)   ... end function
function predict(f: model<"filter">, x) ... end function
```

`record<Tag>` requires records to optionally carry a constructor tag (a
new, additive field on `Value::Record` or a conventional first field
`__type = "Circle"` set by a record-constructing function — a follow-on
decision, not phase 1; see §8 build order). `model<"kind">` needs no new
storage at all — `ModelHandle.kind: String` already exists exactly for
this. Both are strictly optional refinements layered on top of the plain
`record`/`model` tag; phase 1 (§8) does not require them.

**Grammar delta**, concretely, in `qu-syntax`:

```rust
// Stmt::Function / Stmt::DefFn's params field changes shape:
pub struct Param {
    pub name: String,
    pub ty: Option<TypeTag>,   // None == untyped, matches anything
}
pub enum TypeTag {
    Named(String),            // "vec", "mat", "model", ...
    Model(String),             // model<"filter">
    Record(String),            // record<Circle>
}
```
and `param_list()` grows an optional `self.eat_op(":")` + tag-parse per
parameter — a small, local, backward-compatible change (see §5): every
existing call to `param_list()` that never sees a `:` gets `ty: None` for
every parameter, identical behavior to today.

## 2. Method table & storage

Replace the two flat `HashMap<String, (Vec<String>, Body)>` tables with
one overload-aware table per function kind (or unify both into one table
carrying an enum of body kinds — either works; sketched here keeping the
existing `DefFn`/`Function` split since nothing forces unifying them):

```rust
struct MethodEntry {
    params: Vec<Param>,        // Param{name, ty} from §1
    body: MethodBody,          // Expr (DefFn) or Vec<Stmt> (Function)
    defined_at: u32,           // insertion order — see §3's tie-break
}
enum MethodBody { Expr(Expr), Block(Vec<Stmt>) }

// on Interp:
methods: HashMap<String, Vec<MethodEntry>>,   // replaces `funcs` + `block_funcs`
compiled_funcs: HashMap<(String, usize /* overload index or arity-key */), Option<CompiledFn>>,
```

`methods.get(name)` returns the whole overload set; dispatch (§3) picks
one `MethodEntry` from the `Vec` by argument types at call time. Defining
`function f(...)`/`f(...) := ...` **appends** a `MethodEntry` to
`methods[name]` instead of `.insert()`-overwriting, *unless* an existing
entry has an identical `(arity, param types)` signature, in which case it
replaces that one entry in place (so re-running a script/REPL cell that
redefines the same overload behaves like today's "redefinition wins,"
while defining a genuinely different overload adds rather than clobbers).
This one rule is what makes redefinition-in-a-REPL and
add-a-new-overload both work with the same syntax and no new keyword.

## 3. Dispatch algorithm

**Matching.** For a call `f(a1, ..., an)`, a candidate `MethodEntry` in
`methods["f"]` matches iff its arity equals `n` and, for every parameter
`i`, either `param[i].ty` is `None` (untyped — matches any argument) or
`param[i].ty` matches `ai`'s runtime type per the rule below.

**Specificity ordering**, most to least specific:

1. A `record<Tag>` or `model<"kind">` refinement match (exact tag/kind
   string equality) — most specific, since it can only ever match a
   narrower set of values than the bare `record`/`model` tag.
2. A plain named tag match (`vec`, `mat`, `model`, `record`, `num`, ...) —
   matches `Value::type_name()`'s category exactly.
3. `None` (untyped parameter) — matches anything; least specific.

For multi-argument dispatch, total specificity of a matching entry is the
tuple `(count of level-1 matches, count of level-2 matches, count of
level-3/untyped matches)` compared lexicographically **favoring fewer
untyped parameters and more refined ones** — i.e. an entry that pins down
more parameters more precisely wins. This mirrors Julia's rule ("a method
is more specific if its signature is a subtype of the other's, argument by
argument") without needing Qu to grow a real subtype lattice: Qu only has
three specificity *levels*, not an open hierarchy, so a simple per-level
count is sufficient and avoids building a general partial order this
language doesn't otherwise need.

Numeric literals get no special-cased extra specificity tier: `num` is
`num`, full stop. (Julia's Int/Float64 subtype lattice doesn't apply — Qu
is `f64`-only, per BACKLOG.md's own fixed-point/precision rows still being
QUEUE, not shipped.)

**Zero matches**: a hard runtime error, not a silent fallback to some
default overload — mirrors today's exact-arity error shape
(`call_block_fn`, line 3785) so the failure mode is familiar:

```
no method `f` matches argument types (vector, model<"filter">) — 2 candidate(s) defined:
  f(x: vec, y: mat)
  f(x, y: num)
```
(candidate list capped at, say, 5 entries with a "+N more" tail for very
large overload sets — a small, self-contained formatting detail, not a
design fork.)

**Multiple equally-specific matches: ambiguity error, not first-defined-
wins.** Justification: first-defined-wins is exactly the trap Julia
itself explicitly avoids (Julia raises `MethodError: f(...) is
ambiguous`), and for Qu specifically it would make behavior depend on
*file load order* / *statement order* — the same class of "silent,
order-dependent surprise" this codebase has repeatedly special-cased
*against* elsewhere (e.g. the `zeros(filt)` collision was caught as a
compiler warning and deliberately resolved rather than left to silently
shadow, IMPL.md:2468). An explicit ambiguity error is more code (a tie
check after finding the best score) but is consistent with the existing
project's stated bias for loud, actionable errors over convenient-but-
silent fallback (see also `expect_stmt_end`'s doc comment on why silent
fall-through was rejected for statement parsing, qu-syntax:400-421):

```
ambiguous call to `f` for argument types (vector, vector) — 2 equally
specific candidates, add a more specific overload or rename one:
  f(x: vec, y)
  f(x, y: vec)
```

**Untyped-only overload sets are unaffected**: if `methods["f"]` has
exactly one entry and it's untyped in every parameter (today's ordinary
`function f(x, y) ... end`), dispatch trivially always picks it — no
behavior change, no error path ever exercised. This is the backward-
compatibility argument made concrete (§5).

## 4. Interaction with the existing single-dispatch Model protocol

**Coexist, don't subsume — at least not in phase 1.** The `recv.method(args)`
sugar (`eval_call`, line 3003) already desugars to `method(recv, args...)`
purely syntactically, before any dispatch decision is made — it is
*orthogonal* to how `method` itself picks a body. Concretely:

- `.predict(...)`/`.fit(...)`/`.score(...)` call sites need **zero
  changes** — `state.predict(x)` still becomes `predict(state, x)`
  exactly as today, whether or not `predict` ever grows type-annotated
  overloads.
- If, separately, someone later adds `function predict(f: model<"filter">,
  x) ... end` and `function predict(f: model<"kmeans">, x) ... end` as
  *user-space* overloads, those compose with the existing sugar for free:
  `filt.predict(x)` → `predict(filt, x)` → dispatch picks the
  `model<"filter">` overload. This is a strict enhancement path for the
  model protocol (a user could eventually reimplement today's
  hardcoded-in-Rust `kind`-string `match` as Qu-level overloads), not a
  breaking one — but doing so is out of scope for phase 1, which ships
  dispatch as a feature usable by *new* user code, and leaves builtins'
  internal `match`-on-`kind` exactly as they are (§8, phase 3+ if ever).
- No change to `ModelHandle` or `Value::Model` is needed for phase 1 at
  all — the `model<"kind">` refinement (§1.1) reuses the existing `kind:
  String` field read-only.

## 5. Interaction with built-in operators

**Recommendation: keep `+ - * / == < >` etc. hardcoded in `binop`/`unop`
for phase 1 and every phase actually committed to here; treat "user-
overloadable operators" as a distinct, later, opt-in extension, not part
of this proposal's core scope.**

Tradeoffs considered:

- **Make operators ordinary multi-methods** (`+` becomes sugar for a call
  to a dispatchable function named e.g. `__add__`, exactly like Python
  `__add__`/Julia `Base.+`): lets a script overload `*` for its own
  `record<Complex3>`-style type. Real value, but:
  - `binop` (line 3660) is not just a `match` — it's a *priority cascade*
    (string-concat check, then complex+matrix combined path, then flat
    complex, then real matrix, then elementwise) intentionally ordered so
    e.g. a `Complex` meeting a `Mat` takes the combined-kernel path rather
    than either single-type path alone. Turning this into ordinary
    specificity-ranked multi-dispatch risks silently reordering which
    kernel wins for a type combination no test currently exercises,
    exactly the "backward-compat" risk this document is asked to be
    explicit about (§6) — the existing cascade encodes real, non-obvious
    domain knowledge (complex-matrix-first) that a generic specificity
    rule isn't guaranteed to reproduce correctly on day one.
  - It sits on the hottest path in the interpreter — every `+`/`*` in
    every loop body. Even a cheap per-call dispatch (§7) adds a
    branch/hash-lookup to code that today is a direct, monomorphic Rust
    match compiled to a jump table. The measured, real perf investment
    already made here (`Value::Vec`/`Mat` Arc-wrapping, `try_run_compiled`
    bypassing `Value` entirely for all-`Num` calls) is specifically about
    keeping numeric hot loops cheap; operator dispatch is the single place
    most likely to touch literally every hot loop at once.
- **Keep hardcoded, but add ONE user-extension point**: if no built-in
  `binop` arm matches (both operands are `Record`/some other value the
  cascade doesn't already special-case), fall through to looking up a
  user-defined method named after the operator (e.g. `+` → `methods["+"]`)
  using the exact same dispatch machinery as ordinary functions. This is
  the recommended middle path: it changes nothing about `binop`'s
  existing behavior for any type combination it already handles (pure
  addition at the bottom of the existing cascade, after every current
  arm), and gives scripts exactly the thing the original external
  write-ups actually asked for — "a user-defined `Record`-based type
  overloading `*`" — without touching the hot numeric path's existing
  code at all (the fallback only triggers once `binop` has already failed
  to find a hardcoded arm, i.e. only for genuinely new operand type
  combinations, never for `Num`/`Vec`/`Mat`/`Complex`/`Signal`). Sized as
  its own phase (§8, phase 3), not phase 1.

## 6. Migration path / backward compatibility

**Strictly additive and opt-in**, verified against the actual current
data flow, not just asserted:

- Every existing `function f(x, y) ... end` / `f(x, y) := ...` in every
  `.qu` script and every test parses with `ty: None` on every parameter
  (§1's grammar delta only *adds* an optional `: Tag` the parser doesn't
  currently accept at all — nothing existing used a `:` there, so there's
  no reparse ambiguity to resolve).
- An all-untyped overload set of size 1 (i.e. every function as it exists
  today) dispatches trivially to that one entry every time (§3's last
  paragraph) — this is not a special case bolted on for compatibility,
  it's what the general algorithm already does when there's only one
  candidate and it matches everything.
- Redefinition semantics are preserved for the common case: defining the
  *same* signature twice (including twice with zero type annotations,
  today's normal `function f(x) ... end` followed by a REPL re-edit of
  `f`) still replaces in place, matching current `.insert()`-overwrite
  behavior exactly (§2's "identical signature replaces" rule) — a script
  that redefines a function to fix a bug still just works, no ambiguity
  error, no accumulating stale overloads.
- `apply_seeded`'s existing "user functions win over builtins by name"
  rule (line 3696) is unchanged: an untyped user `function zeros(x) ...
  end` still fully shadows the builtin `zeros`, exactly as today — this
  document does not change that priority rule, only what happens once
  it's decided a *user* table lookup applies (now possibly an overload
  set instead of always exactly one body).
- The one behavior change existing code could observe: a script that
  currently defines `f(x)` and *separately* `f(x, y)` today gets two
  independent single-arity entries in the same flat `HashMap` only
  because they're keyed differently already (`funcs`/`block_funcs` are
  keyed by name alone, not name+arity — re-reading `Stmt::DefFn`'s
  execution at line 2326, a second `f(x, y)` `.insert()`s onto the *same*
  key `"f"` as first `f(x)`, so **today, defining two different arities of
  the same name is ALREADY a silent overwrite, not two coexisting
  functions** — arity-based "overloading" doesn't actually work today,
  contrary to what one might assume from `call_block_fn`'s arity check
  existing at all). This proposal is a strict improvement here: those two
  definitions become two distinct arity-1/arity-2 overloads that coexist,
  which is more useful and strictly closer to what a user would expect,
  never less. No script can be relying on today's silent-overwrite-by-
  arity behavior in a way that this change breaks, since today's behavior
  for that exact case (define same name, different arity, twice) is "the
  second one wins outright and the first is unreachable" — the *new*
  behavior (both callable, dispatched by argument count) is a superset,
  not a change to any currently-reachable code path.

## 7. Performance

- **Per-call-site cost, non-overloaded case (the overwhelming majority of
  existing code, per §6):** a `methods.get(name)` returning a
  `Vec<MethodEntry>` of length 1, whose single entry is all-`None` typed
  → the dispatch "check" is `entries.len() == 1 && entries[0].all_untyped()`,
  a boolean computed once and cacheable per-name exactly the way
  `compiled_funcs` already caches "is this function eligible for the
  fast numeric path" per name (line 3742). Reuses that exact caching
  idiom: a `dispatch_cache: HashMap<String, DispatchPlan>` where
  `DispatchPlan::Trivial(usize)` (index into the `Vec`, always that one)
  short-circuits straight past any type-matching work, computed once per
  name and invalidated on redefinition exactly like `compiled_funcs.remove(name)`
  already is (line 2332/2338).
- **Per-call-site cost, genuinely overloaded case:** a linear scan of the
  (typically small — realistically 2-6 entries) overload `Vec`, computing
  each candidate's specificity tuple from `Value::type_name()` string
  comparisons (already O(1) per argument — `type_name()` is a plain
  match, no allocation) and picking the max. This is `O(overloads ×
  arity)` string-tag comparisons per call — negligible next to any
  numeric work the function body itself does, and nowhere near the
  measured Arc-clone/deep-copy costs the existing `Value::Vec`/`Mat`
  Arc-wrapping optimization was built to fix. No new allocation is
  required if the specificity comparison works on `&'static str` tags
  (which `type_name()` already returns) rather than building any
  intermediate `Vec`/`String`.
- **Does this regress the optimized numeric hot path?** No, provided the
  `DispatchPlan::Trivial` fast-out above sits *before* `try_run_compiled`
  is even consulted — for the common single-overload, all-`Num`-argument
  case, the sequence is unchanged from today: name lookup → (new, O(1))
  triviality check → straight into the existing `try_run_compiled`/
  `call_user`/`call_block_fn` path exactly as now. The only genuinely new
  per-call cost anywhere is for functions that are *actually* overloaded,
  which by definition did not exist as callable Qu code before this
  feature (§6) — there is no existing benchmark or hot loop this could
  regress, because no such multi-overload function exists in the codebase
  today to slow down.
- **Caching per call site** (as opposed to per function name) is not
  needed for phase 1: Qu has no inline caching / call-site IDs today (no
  bytecode, tree-walking `Expr` nodes are the "call site," and they don't
  currently carry any mutable cache slot), and per-*name* caching already
  captures the useful case (a given function's overload count/shape
  rarely changes within one hot loop). Revisit only if profiling ever
  shows the per-name `HashMap` lookup itself is hot — not indicated by
  anything found in this codebase today.

## 8. What this does NOT solve

- **Not a static type system.** Every dispatch decision described here
  happens at call time, against runtime `Value` variants — there is no
  new compile-time type-checking pass, and a program that would fail
  every overload's match is only caught when that call actually executes
  (consistent with Qu's existing entirely-dynamic design — no type errors
  are caught earlier anywhere else in the interpreter either). This does
  not touch the separately-QUEUE'd "physical units as a static type" or
  "static, shape-checked parametric types" rows (BACKLOG.md:1139,1176) at
  all — those are about compile-time verification; this is purely a
  runtime call-routing mechanism.
- **Physical units**: fully orthogonal. Today a unit literal (`5V`) is
  just a `Value::Num` after `apply_unit` scales it to SI at eval time
  (BACKLOG.md:1138's "DONE" note) — there is no `Value::Unit` variant for
  a dispatch rule to key on. If/when units become a real static type
  (BACKLOG.md:1139, still QUEUE), a `unit<V>`-style type tag could in
  principle join the `TypeTag` vocabulary in §1 the same way `model<kind>`
  does today — but that is new work contingent on the units item landing
  first, not something this proposal does or assumes.
- **Named/labeled axis indexing** (BACKLOG.md:1151, also ASK): unrelated
  surface area (`X[f = 1kHz]` indexing syntax, not function dispatch) —
  no interaction either direction.
- **Does not give user code control over which *builtin* runs** for
  `zeros`/`remove`/`join`/etc. — those stay exactly as hardcoded Rust
  `match` arms unless/until a specific decision is made to migrate a
  given builtin onto the new `methods` table (a mechanical, one-at-a-time
  migration, not automatic — see §4's explicit note that `predict`'s
  `kind`-string match is left untouched by phase 1).
- **Does not add an ability to declare return types** or do any
  return-type-based dispatch (Qu's call sites don't know the desired
  result type ahead of a call the way e.g. `Default::default()` in a
  statically-typed language can be resolved by inference) — dispatch here
  is purely on argument types, matching what both external write-ups
  actually asked for (Julia-style *argument* multiple dispatch).
- **Does not itself make operators overloadable** — see §5; that is a
  distinct, explicitly deferred sub-feature (phase 3 in §9), not included
  in "multiple dispatch" by default.

## 9. Recommended incremental build order

| Phase | Scope | Size |
|---|---|---|
| **1** | Parser: optional `: Tag` per parameter (§1, plain named tags only — no `record<Tag>`/`model<"kind">` refinement yet). Interpreter: replace `funcs`/`block_funcs` with the overload-set `methods` table (§2); dispatch by arity + plain-tag specificity (§3, levels 2/3 only); zero-match and ambiguity errors; `DispatchPlan::Trivial` fast path (§7) so today's all-untyped code has no measurable behavior or performance change. No operator changes, no Model-protocol changes. | **Medium** — touches core call resolution (`call_named`/`apply_seeded`/`Stmt::DefFn`/`Stmt::Function` execution) and the parser's `param_list`, but each piece is small and the existing test suite (`qu-syntax`'s `#[cfg(test)]` module, `qu-interp`'s function-call tests) is the direct regression check: every existing test must keep passing unchanged, since every existing function is an all-untyped, single-entry overload set. |
| **2** | **DONE (2026-08-27, same day as phase 1)** — `record<Tag>` and `model<"kind">` refinements (§1.1). `model<"kind">` reuses `ModelHandle.kind` exactly as sketched, no new storage. `record<Tag>` picked the `__type` field convention over a dedicated `Value::Record` tag field: an ordinary `__type = "Tag"` entry in the existing field vec, set by a plain record-returning "constructor" function (`Circle(r) := {__type = "Circle", radius = r}`) — no grammar change, no `Value::Record` shape change, so none of the ~40 existing call sites that pattern-match `Value::Record(fields)` as a plain field vec needed to change. Specificity ordering extended: a refinement match outranks a bare-tag match (implemented as a weighted score, `+1` plain / `+1_000_000` refined, equivalent to the 3-level tuple ordering below since all compared candidates share one call's arity). See `IMPL.md`'s "Multiple dispatch, phase 2" entry for the full implementation and verification. | **Small–Medium**, as estimated. |
| **3** | **DONE (2026-08-27, same day as phases 1-2)** — User-overloadable operators via the single fallback hook described in §5. Landed as a call-site gate in `eval`'s `Expr::Binary`/`Expr::Unary` arms rather than inside `binop`/`unop` themselves: `binop`/`unop` are completely UNMODIFIED (still `&self`, no `methods`/`dispatch_cache` access needed), and the fallback (`binop_or_user_method`/`unop_or_user_method`) is only even consulted when `is_operator_overload_candidate` says at least one operand is a type the existing cascade doesn't cover by type at all (everything except `num`/`bool`/`str`/`vec`/`mat`/`complex`/`cvec`/`cmat`/`signal`) — for every combination of those "safe" types, `eval` calls `binop`/`unop` directly, byte-for-byte the same code path as before this phase, even in a script that also defines an operator overload elsewhere. When the gate does fire, it tries `binop`/`unop` first (so an existing arm's own error, e.g. a vector-length mismatch, is never second-guessed) and only on error looks up `methods[op]` via the exact same `resolve_method`/`dispatch_method` machinery phases 1-2 already built, falling back to the original error unchanged when no override matches. Grammar: `qu-syntax`'s `function_stmt` grew `expect_fn_name` (tries `expect_name` first, else accepts one of a new `OVERLOADABLE_OPERATORS` list as the function name), so `function +(a: record, b: record) ... end function` parses. Verified end to end via `qu.exe run` (a tagged `Vec2`-style record overloading `+`/unary and binary `-`, alongside untouched `1+2`/`[1,2]+[3,4]`/matrix and complex arithmetic/string concat in the same script) and a broad zero-regression test matrix in `qu-interp/tests/acceptance.rs`. Full workspace suite green, no regressions. See `IMPL.md`'s dated "Multiple dispatch, phase 3" entry for the full implementation. | **Medium**, as estimated — the hook itself was small; auditing `binop`'s cascade to place the gate correctly (by TYPE, not by whether a call happens to error) was the real work, exactly as predicted. |
| **4 (optional, not recommended without a separate ask)** | Migrate specific existing builtins (`zeros`, `predict`, `remove`, `join`, ...) from hardcoded Rust `match` arms onto the `methods` table so *scripts* can add new arms (e.g. a user adding a new model kind's `predict` overload without touching Rust). Each migration is its own small, isolated PR-sized change per builtin — deliberately not bundled, since each one is a real behavior-preservation risk (§6's same "existing cascade encodes real ordering" caution as §5) that deserves its own verification pass rather than a batch rewrite. | **Small per builtin, but many builtins** — no fixed total size; scope one builtin at a time if ever pursued. |

Phase 1 alone already delivers the concrete, most-requested capability
("one `filter` name, behavior varies by argument type" — BACKLOG.md's own
phrasing of the ask) for **new user-defined functions**, with zero risk to
any existing script, and is the natural place to stop and reassess before
committing to phases 2-4.
