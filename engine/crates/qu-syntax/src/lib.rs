//! Qu parser — a hand-written recursive-descent / precedence-climbing parser
//! for the subset of `docs/qu-grammar.ebnf` that the M1 milestone targets.
//!
//! Coverage (this milestone):
//!   * statements: `backend`, assignment (`=`, `+=`, `-=`, `*=`, `/=`, `.=`),
//!     deferred bind (`:=`), contract (`x as vector(K,1)`), `for … end for`,
//!     `while … end while`, `if … then … [else] … end if`, `const`, bare
//!     figure/command verbs (`grid on`, `show plot`), and expression
//!     statements (`plot(t, x)`);
//!   * the full expression precedence ladder of the grammar: `as` contracts,
//!     `->` reshape sugar (`expr -> (r,c)`, rewritten to `reshape(expr,r,c)`
//!     at parse time), `|>`, `in`, `or`, `and`, `== !=`, `< <= > >=`,
//!     `+ - |`, `* / \ .* ./ .\ mod`, unary `+ - not ~`, power `^ **`
//!     (right-assoc), postfix `() [] . '`, and primaries (literals, unit
//!     literals, names, matrices `[1,2;3,4]`, records `{a=1}`, ranges
//!     `a to b step c`);
//!   * a small closed set of `namespace.method(...)` call aliases
//!     (`table.load`/`table.write`/`timer.start`/`timer.elapsed`/
//!     `timer.stop`/`signals.square`/`signals.impulse`/`signals.pwm`/
//!     `signals.sawtooth`/`signals.triangle`), rewritten straight to the
//!     real builtin's `Call` AST — see `namespace_method_alias`/
//!     `Parser::postfix_primary`.
//!
//! Deliberately out of scope for M1 (tracked in IMPL.md): `test`/`testbench`/
//! `sandbox`/`fit`/`train`/`model`/`scene` blocks and the `from … select`
//! query form. The parser reports the first structural error it hits with a
//! line:col span rather than panicking.

use qu_lexer::{lex, Span, Tok, Token};
use std::fmt;

// ------------------------------------------------------------------ AST

#[derive(Clone, Debug)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

/// A function parameter, optionally annotated with a type tag for dispatch
/// (§ multiple dispatch phase 1, 2026-08-27 —
/// `docs/design/multiple-dispatch.md` §1/§2). `ty: None` is untyped and
/// matches any argument at dispatch time; this is what every parameter
/// parsed before this feature existed, and still is, unless a `: Tag`
/// suffix is written explicitly.
///
/// `default` (default-value parameters, 2026-08-31): `Some(expr)` when
/// the parameter was written `name = expr` (or `name: Tag = expr`),
/// making it optional at the call site — omitting the argument
/// re-evaluates `expr` fresh at call time (never memoized once at
/// definition time) in the callee's own new frame, with every
/// already-bound earlier parameter visible, so a later default may
/// reference an earlier parameter by name (`function f(x, y = x * 2)`).
/// `param_list` enforces that once one parameter has a default, every
/// parameter after it does too — see its own doc comment. No `PartialEq`
/// derive: `Expr` itself doesn't implement it, and nothing needs whole-
/// `Param` equality (`qu-interp`'s overload-signature check compares
/// `.ty` fields directly, ignoring `default`, which is correct: a
/// default value is not part of a dispatch signature).
/// Reference parameters (`function f(@x)` / `sub f(@x)`) — SCOPED OUT,
/// design only (§ ref variables, 2026-09-01). Ahmed's own request, verbatim:
/// "Same should go for `function f(@x)` or `sub f(@x)` ref variables" — a
/// parameter marked this way would let the function mutate the CALLER's
/// variable directly, the parameter analog of `ref y = x` above (which DID
/// ship — see `Stmt::RefAssign`).
///
/// Why this didn't ship alongside it: `ref y = x`'s aliasing mechanism
/// (`qu-interp`'s `AliasGroup`) links two NAMED SLOTS by construction — it
/// needs to know, at the moment the alias is created, exactly which
/// variable (by name and scope) it's linking to. A call argument doesn't
/// carry that information by the time a function body could use it: every
/// call site evaluates its arguments down to plain `Value`s FIRST
/// (`Interp::eval_call_args` returns `Vec<Value>`, and `dispatch_method`/
/// `call_user`/`call_block_fn`/the multiple-dispatch overload selection
/// and memoization key (`MemoKey`) all key off that already-evaluated
/// `Vec<Value>`) — by the time a parameter could be bound, "which variable
/// did argument 2 come from" has already been thrown away, on purpose,
/// for every call in the interpreter uniformly.
///
/// Making `@x` work would mean threading the ORIGINAL ARGUMENT EXPRESSION
/// (not just its evaluated value) through that entire path — arity
/// checking, default-value evaluation order, overload dispatch, and the
/// `memoize function` cache key would all need a rule for what an aliasing
/// argument means to each of them (does a `ref` parameter opt a call out
/// of memoization entirely? does overload resolution match on it?) — a
/// materially larger, separate design than a local `ref y = x` binding,
/// touching machinery that shipped very recently (multiple dispatch,
/// function memoization) and deserves its own careful pass rather than a
/// rushed addition here. Per the task's own instruction to prioritize a
/// cleanly correct subset over a half-working whole, this is logged as
/// explicit follow-up work, not attempted in this change.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeTag>,
    pub default: Option<Expr>,
}

/// What a function's body is: one expression, or a block of statements.
///
/// Named functions have had both forms since `name(x) := expr` and
/// `function name(x) ... end function`; a lambda is the same two shapes
/// without the name, so it carries the same distinction rather than
/// inventing a third.
#[derive(Clone, Debug)]
pub enum FnBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

/// A parameter's type-tag annotation (`x: vec`). `name` is the plain
/// named-tag form checked against [`TYPE_TAGS`]; `refine` is the phase 2
/// (2026-08-27, design doc §1.1) optional refinement carried by
/// `model<"kind">` (`refine = Some("kind")`, a string-literal payload) and
/// `record<Tag>` (`refine = Some("Tag")`, an identifier payload). Every
/// other tag word always has `refine: None`. `PartialEq`/`Hash` compare
/// both fields, so `model` and `model<"filter">` are distinct signatures
/// for `upsert_method`'s "identical signature replaces" rule (§2) — this
/// is what lets a script define both a bare-`model` overload and a
/// `model<"filter">` overload of the same name side by side.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeTag {
    pub name: String,
    pub refine: Option<String>,
}

impl TypeTag {
    /// A plain, unrefined tag — what every `TypeTag` was in phase 1.
    pub fn plain(name: impl Into<String>) -> Self {
        TypeTag { name: name.into(), refine: None }
    }
}

/// The closed, lexer-friendly type-tag vocabulary a parameter annotation
/// may use (§1 of the design doc — chosen specifically because each is a
/// single identifier token, unlike `Value::type_name()`'s own display
/// strings such as `"complex vector"`). `"any"` is an explicit synonym for
/// untyped (parses to `Param.ty: None`, not `Some(TypeTag("any"))`) rather
/// than a real tag in this list.
pub const TYPE_TAGS: &[&str] = &[
    "num", "bool", "str", "vec", "mat", "complex", "cvec", "cmat", "signal",
    "mask", "table", "model", "list", "record", "image", "timer", "file",
    "worker", "mutex", "semaphore", "none",
];

/// Operator spellings allowed as a `function <op>(...) ... end function`
/// name (§ multiple dispatch phase 3, `docs/design/multiple-dispatch.md`
/// §5) — exactly the set `qu-interp`'s `binop`/`compare` dispatch on for a
/// two-operand call, plus the unary `-` (a one-parameter overload of the
/// same name). Deliberately excludes pure punctuation/assignment tokens
/// (`=`, `:=`, `+=`, `(`, `,`, ...) that could never sensibly name a
/// function, and the keyword-spelled operators (`mod`, `and`, `or`, `not`)
/// since those lex as `Tok::Keyword`, not `Tok::Op`, and `expect_fn_name`
/// only special-cases `Tok::Op` — extending to keyword-spelled operators
/// is out of phase-3's scope (no design-doc example asks for it).
pub const OVERLOADABLE_OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "==", "!=", "<", ">", "<=", ">=",
    ".*", "./", ".\\", "\\", "^", "**", ".^",
];

/// What an `import` names: a native module compiled into the engine, or a
/// path to another Qu source file.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportSource {
    /// `import xlsx` -- a bare word.
    Native(String),
    /// `import "helpers.qu"` -- a quoted path, resolved relative to the
    /// importing file.
    File(String),
}

/// One `reduce(<op>: <name>)` entry on a `parallel for`.
///
/// WHY EXPLICIT RATHER THAN INFERRED. `total = total + f(i)` has a
/// recognisable shape -- the merge already detects it, in order to refuse
/// it -- so inferring `+` was possible. But the mechanism that makes `+`
/// work (start each iteration from the operator's identity and fold the
/// results) computes a WRONG ANSWER for the identical shape written with
/// `*`, silently, because the identity differs. Naming the operator
/// removes the guess: one that is not supported refuses loudly instead of
/// being given a plausible interpretation. That silent-wrong-answer class
/// is what `a116c451` existed to close, and inference would have reopened
/// it one operator to the left.
#[derive(Debug, Clone, PartialEq)]
pub struct Reduction {
    pub op: ReduceOp,
    pub var: String,
}

/// The folds a `parallel for` can combine. Each has an identity, and that
/// is the whole mechanism: an iteration's private copy of the variable
/// STARTS at the identity, so whatever it ends with is exactly that
/// iteration's own contribution, and the loop combines contributions in
/// INDEX ORDER -- the same sequence of operations the serial loop performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOp {
    Add,
    Mul,
    Min,
    Max,
}

impl ReduceOp {
    /// The spelling accepted in source. Also used to build error messages,
    /// so the accepted set and the advertised set cannot drift apart.
    pub fn from_symbol(s: &str) -> Option<Self> {
        match s {
            "+" => Some(Self::Add),
            "*" => Some(Self::Mul),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Mul => "*",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    /// What an iteration's private copy starts from. Combining any value
    /// with the identity must be a no-op -- that is what makes "start at
    /// the identity, fold in index order" reproduce the serial loop
    /// exactly rather than approximately.
    pub fn identity(self) -> f64 {
        match self {
            Self::Add => 0.0,
            Self::Mul => 1.0,
            Self::Min => f64::INFINITY,
            Self::Max => f64::NEG_INFINITY,
        }
    }

    pub fn apply(self, acc: f64, x: f64) -> f64 {
        match self {
            Self::Add => acc + x,
            Self::Mul => acc * x,
            // `f64::min`/`max` return the non-NaN operand when one is NaN,
            // which is what a reduction wants: a single NaN iteration
            // should not erase every other result.
            Self::Min => acc.min(x),
            Self::Max => acc.max(x),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Backend(String),
    /// `m means meter` / `m means milli` -- says which reading of an
    /// ambiguous unit this script intends (§ length units, 2026-09-09).
    ///
    /// `m` has meant the SI MILLI prefix since before Qu had lengths, so
    /// `5 m` is 0.005 and changing that silently would rewrite the meaning
    /// of every script that used it. But a language for measurement whose
    /// `m` is not the metre is its own trap. So neither reading is
    /// imposed: the default stays milli, using `m` undeclared warns once
    /// and says how to settle it, and this statement settles it.
    ///
    /// `means` is NOT a reserved word -- it is recognised only in the
    /// shape `<name> means <name>` at the start of a statement, which is
    /// not otherwise grammatical, so `means = 5` still works.
    UnitMeans { unit: String, meaning: String },
    /// `ignore warning m` -- silence one named warning for this run.
    ///
    /// The escape hatch for someone who has read a warning, decided it
    /// does not apply, and does not want it again. Keyed by name, so it
    /// silences exactly one thing rather than turning off diagnostics.
    IgnoreWarning(String),
    /// `import xlsx` / `import "helpers.qu"` / `import "helpers.qu" as h`.
    ///
    /// `import` has been a reserved word in `qu-lexer::KEYWORDS` since long
    /// before this, with no grammar and no implementation behind it -- the
    /// name was claimed and unused, so defining it here breaks nothing.
    ///
    /// Two things arrive through the same door because they are the same
    /// idea from the script's side: a namespace of functions it did not
    /// write. A bare NAME is a native module, compiled in behind a feature
    /// flag; a STRING is a path to another `.qu` file. Both bind one name,
    /// and neither adds anything to the global builtin table -- which is
    /// the entire point, since that table is already 725 names in one flat
    /// namespace inside a 57,000-line file.
    ///
    /// `alias` is the `as` name. Without one a native module binds its own
    /// name (`import xlsx` gives `xlsx`), and a file merges its functions
    /// into the importing scope unqualified.
    Import { source: ImportSource, alias: Option<String> },
    /// `global x, y` — names this function assigns straight through to the
    /// module scope instead of binding locally.
    ///
    /// Needed because assignment inside a function binds LOCALLY (the
    /// Python model the spec calls for). Before this existed, a function
    /// assigning any name that already existed at module scope wrote to
    /// it, so calling a function could silently rewrite the caller's
    /// variables -- and the caller had no way to say it did not want that.
    /// Now the default is safe and the sharing is opt-in and visible.
    Global(Vec<String>),
    /// `type implicit` (default) / `type explicit` — typing discipline (§8).
    /// Implicit lets the runner infer types; explicit requires a declared
    /// type/shape (`as …`) before a new binding.
    TypeMode { explicit: bool },
    /// `name <op>= rhs`  (op is "" for plain `=`)
    ///
    /// Also the desugar target of the `@expr` prefix-statement operator
    /// (§ self-assign chain, 2026-08-26): `@data.drop("target")` parses
    /// `data.drop("target")` as an ordinary expression, walks down to its
    /// root variable (`data`, via `root_ident` below), and produces exactly
    /// `Assign { name: "data", op: "", rhs: <that expression>, in_place: true }`
    /// — the same node an explicit `data = data.drop("target")` would
    /// produce, EXCEPT for `in_place`. See the `@`-handling block in
    /// `statement()`.
    ///
    /// `in_place` (§ in-place mutation for `@expr.method()`, 2026-09-01):
    /// `true` only when this `Assign` was produced by the `@expr` prefix
    /// operator, `false` for every other route to `Assign` (plain `name =
    /// rhs`, `+=`/`-=`/`.=`/etc., `enum`/`table`/... contextual-keyword
    /// fallthrough assigns). Until this field existed, `@` was PURE parse-
    /// time sugar — verified directly (see the `self_assign_desugars_to_
    /// plain_assign_one_level` test and the git history around
    /// `self_assign_root_identifier_survives_deeper_chains`): the interpreter
    /// could never tell an `@`-triggered self-assign apart from an ordinary
    /// `data = data.drop(...)` typed out by hand, so `@` carried no actual
    /// runtime information despite its name implying in-place semantics.
    /// `qu-interp`'s `Stmt::Assign` exec arm reads this flag to attempt a
    /// real in-place mutation (via `Arc::make_mut`, mutating the existing
    /// `Vec`/`Matrix` when nothing else aliases it) for a short allow-list of
    /// hot builtins (`sort`, `particle_tracker`'s `.move`/`.update`), falling
    /// back to the ordinary allocate-fresh evaluation for every other rhs
    /// shape and whenever `in_place` is `false` — a strict superset of the
    /// old behavior, never a semantic change to what `@`/`=` compute.
    Assign { name: String, op: String, rhs: Expr, in_place: bool },
    /// `handle.field = value` — set a property on something already drawn.
    ///
    /// Qu had no field assignment at all: `a.b = v` did not parse. It is
    /// added for plot artists, where the alternative is restating a colour
    /// at two call sites and hoping they stay equal.
    FieldAssign { name: String, field: String, rhs: Expr },
    /// `name[indices] <op>= rhs` — indexed / logical-mask assignment
    IndexAssign {
        name: String,
        indices: Vec<Idx>,
        op: String,
        rhs: Expr,
    },
    /// `name := rhs`
    Deferred { name: String, rhs: Expr },
    /// `lazy name = expr` (§ lazy variables, 2026-08-31 — Ahmed asked for a
    /// lazy variable distinct from the already-existing function
    /// `memoize`: the RHS expression is not evaluated at this statement at
    /// all, only on the FIRST actual read of `name` afterward, and the
    /// computed value is then cached in place so a second read never
    /// re-runs it. NOT the same feature as `name := rhs` (`Stmt::Deferred`
    /// above) — that form is reserved for a *future* operator-fusing
    /// planner and is explicitly documented as "eager in M2" at its own
    /// execution site (`qu-interp`), so hijacking it here would misrepresent
    /// what it does today and collide with that milestone's own plans.
    /// `lazy` is contextual (matching `parallel`/`pool`/`memoize`'s own
    /// precedent elsewhere in this file), not reserved — recognized only in
    /// the exact shape `lazy <name> = <expr>`, so a plain variable/function
    /// named `lazy` still works everywhere else (`lazy = 5`, `lazy()`,
    /// `lazy.field`, ...). See `qu-interp`'s `Value::Lazy` doc comment for
    /// the runtime representation and the forcing/caching mechanics, and
    /// its `Stmt::LazyAssign` execution arm for the plain-reassignment
    /// interaction (`lazy x = expr` then `x = other` before `x` is ever
    /// read: the pending thunk is discarded, never forced — see that arm's
    /// doc comment for why).
    LazyAssign { name: String, rhs: Expr },
    /// `ref name = rhs` (§ ref variables, 2026-09-01 — Ahmed's explicit
    /// request for real reference-variable syntax: after `ref y = x`,
    /// mutating OR reassigning EITHER `y` or `x` is visible through both,
    /// unlike plain `y = x` which is proven NOT to alias — see
    /// `qu-interp`'s `at_sort_on_an_aliased_vec_leaves_the_alias_unmodified`
    /// and the `Stmt::Assign.in_place` doc comment above for that
    /// guarantee, which `ref` is a deliberate, explicit, opt-in exception
    /// to and must never weaken for the non-`ref` case.
    ///
    /// Scoped to `rhs` being a plain variable name (`ref y = x`) for this
    /// milestone — `ref y = table.column` / `ref y = arr[3]` (aliasing a
    /// field/index rather than a whole variable) is NOT supported, since
    /// there is no existing "shared cell" representation for a single
    /// element of a table/vector to hand out, unlike a whole variable
    /// slot. `rhs` is still parsed as a full expression here (matching
    /// `lazy`'s own `assign_rhs` upstream) rather than restricted to a bare
    /// identifier at the grammar level, precisely so a non-identifier rhs
    /// (`ref y = f(x)`, `ref y = x + 1`) produces one clear, specific
    /// runtime error at `qu-interp`'s `Stmt::RefAssign` exec arm instead of
    /// a confusing parse failure at the call site — see that arm's doc
    /// comment for the exact error text.
    ///
    /// `ref` is contextual (same precedent as `lazy` immediately above),
    /// not reserved: recognized only in the exact shape `ref <name> =
    /// <expr>`, so a plain variable/function named `ref` still works
    /// everywhere else (`ref = 5`, `ref()`, `ref.field`, ...).
    RefAssign { name: String, rhs: Expr },
    /// `name as contract`
    Contract { name: String, contract: Expr },
    /// `name as contract = rhs` — declare shape/orientation and initialize in
    /// one statement (declaration is optional; this is the combined form).
    Declare {
        name: String,
        contract: Expr,
        rhs: Expr,
    },
    /// `const name = rhs`
    Const { name: String, rhs: Expr },
    /// `unit name = rhs` — a user-defined unit scale factor (§ user-defined
    /// units). Deliberately its own `Stmt` variant rather than reusing
    /// `Const`: its runtime effect binds `name` to the evaluated `rhs` for
    /// ordinary arithmetic use (`unit ppm = 1e-6` then `5 * ppm`), same as
    /// `const` would, but ALSO registers `name` in the interpreter's
    /// custom-unit table (`register_custom_unit`), which
    /// `Interp::unit_scale` consults as a fallback whenever a built-in
    /// spelling doesn't match. That fallback is what makes the bare
    /// numeral-adjacency literal (`5 ppm`, no `*`) work too, exactly like
    /// a built-in unit literal (`5 kHz`) does: the lexer/parser already
    /// accept `<number> <identifier>` for ANY trailing identifier, not
    /// just the statically-known spellings in `qu_lexer::UNITS` (see
    /// `Parser::unit_suffix`'s own doc comment for why that grammar was
    /// safely widened, and `apply_unit`'s doc comment in `qu-interp` for
    /// why an unknown one is a runtime error naming the unit rather than a
    /// parse failure) — this statement is what turns a previously-unknown
    /// identifier into a resolvable one, by giving that runtime lookup a
    /// second table to check.
    UnitDecl { name: String, rhs: Expr },
    For {
        var: String,
        range: Expr,
        body: Vec<Stmt>,
        /// `for … else … end for`: run only when the loop finished on its
        /// own, NOT when a `break` left it early.
        ///
        /// The search-loop idiom. Without it, "did the loop find one?"
        /// has to be carried in a flag set beside the `break` and tested
        /// after the loop -- two statements far apart that must agree,
        /// and the bug is that they stop agreeing.
        else_: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        else_: Vec<Stmt>,
    },
    /// `select case <subject> / case <v>[, <v>...] / case else / end select`
    ///
    /// The multi-way branch, which had to be written as an `if`/`elseif`
    /// chain that repeated the subject on every rung -- and a repeated
    /// subject is a place for one rung to end up testing something
    /// slightly different from its neighbours.
    ///
    /// NO FALL-THROUGH, deliberately. The first arm whose value matches
    /// runs, and then the statement is over. C-style fall-through is a
    /// silent-wrongness generator of exactly the kind this language is
    /// organised against: the failure is a MISSING token, so the code that
    /// is wrong looks like the code that is right.
    ///
    /// The subject is evaluated ONCE, so a `select case` over a function
    /// call calls it once however many arms it has.
    Select {
        subject: Expr,
        /// One entry per `case`; several values on one `case` match if any
        /// of them does.
        arms: Vec<(Vec<Expr>, Vec<Stmt>)>,
        /// `case else`. Empty when there is none.
        else_: Vec<Stmt>,
    },
    /// A loop whose body runs BEFORE its condition is tested, so it always
    /// runs at least once:
    ///
    ///   `do ... loop while <cond>` / `do ... loop until <cond>`
    ///   `repeat ... until <cond>`
    ///
    /// `do`, `until` and `repeat` have been reserved words since the
    /// lexer was written and did nothing. Without this the only way to
    /// run a body once before testing was to duplicate it above a
    /// `while`, or write `while true` with a `break` -- both of which
    /// state the loop's real condition somewhere other than where the
    /// loop says its condition is.
    DoLoop {
        body: Vec<Stmt>,
        cond: Expr,
        /// `until` inverts the test. Stored rather than folded into a
        /// `not` so an error message can name the form the author wrote.
        until: bool,
    },
    /// `try ... [catch [name] ...] end` — the one MATLAB-style exception to
    /// the `end <introducer>` rule (§16): closes with bare `end`, not
    /// `end try`. `catch_var`, if given, binds the caught error's message
    /// (a string) for the handler body.
    Try {
        body: Vec<Stmt>,
        catch_var: Option<String>,
        handler: Vec<Stmt>,
        /// `try … else`: runs when the body raised NOTHING.
        ///
        /// Specified in §34.A.4 and never implemented. It is the narrowing
        /// that makes a `try` honest: code that belongs after the risky
        /// call, but must not itself be guarded by the `catch`, otherwise
        /// a failure in the follow-up is reported as a failure of the
        /// thing being tried.
        else_: Vec<Stmt>,
        /// Whether a `catch` clause was written at all.
        ///
        /// An empty handler and no handler are different things and the
        /// old shape could not tell them apart. It matters for
        /// `try … finally` with no `catch`: that must run the cleanup and
        /// then let the error KEEP GOING, which is what the construct is
        /// for. (A bare `try … end` with no catch swallows, as it always
        /// has -- that is existing behaviour, not something `finally`
        /// should quietly change.)
        has_catch: bool,
        /// `finally`: runs on the way out however the block was left --
        /// normally, by a caught error, by an error that is still
        /// propagating, or by a `return`/`break` jumping out of it.
        ///
        /// The point is the LAST of those. Cleanup written after the
        /// `end` is skipped by exactly the paths that most need it, and
        /// that is where leaked file handles and unclosed ports come
        /// from.
        finally: Vec<Stmt>,
    },
    /// `unsafe [as name] ... end` — unlike `try`/`catch` (catch once, jump to
    /// a handler), each statement here runs independently: a failing one is
    /// reported and execution moves to the *next* statement in the block,
    /// not out of it. `report_var`, if given, is bound to the failure count
    /// once the block finishes (there's no vector-of-strings value yet to
    /// collect the actual messages into).
    Unsafe {
        body: Vec<Stmt>,
        report_var: Option<String>,
    },
    /// `parallel for Name in (RangeExpression|Expression) ... end parallel`
    /// (grammar §46.5) — each iteration runs in an ISOLATED environment (a
    /// snapshot of `env`/`funcs`/`block_funcs`, plus a freshly-drawn RNG
    /// seed — same reasoning as `spawn`/`Worker`: sharing state or an RNG
    /// stream across iterations would be silently wrong, not an error).
    /// Results merge back only for index-assignments into arrays that
    /// existed before the loop (`results[i] = ...`); a plain reassignment
    /// of any other pre-existing variable is a clear runtime error, not a
    /// silently dropped value. `parallel` is contextual (only recognized
    /// immediately before `for`), not reserved — a plain variable named
    /// `parallel` still assigns normally.
    ParallelFor {
        var: String,
        range: Expr,
        body: Vec<Stmt>,
        /// `with reduce(+: total, max: best)` -- variables the loop FOLDS
        /// rather than writes disjointly. Empty for a loop without the
        /// clause, which behaves exactly as it did before this existed.
        reductions: Vec<Reduction>,
    },
    /// `every|after|at <seconds> [do] ... end` (grammar §50
    /// `TimerStatement`) — registers a callback fired later by the
    /// `run_for(duration)` builtin. The interval is in **seconds**
    /// (matching the spec's own `every 1 s do tick() end` example and Qu's
    /// unit-literal convention where a bare time number is seconds —
    /// `apply_unit`'s `"s" => 1.0`, `"ms" => 1e-3` — so `every 500 ms do` and
    /// `every 0.5 do` mean the same interval). `every` repeats every
    /// `interval` (virtual) seconds; `after` fires once at `interval`; `at`
    /// is accepted as a spec-documented alias of `after` (there's no
    /// absolute wall clock in `run_for`'s simulated timeline for `at` to
    /// mean anything different yet). `every`/`after`/`at` are contextual,
    /// not reserved keywords — matching `type implicit`/`explicit`'s
    /// pattern — so a variable named e.g. `every` still assigns normally;
    /// only `<word> <expr> ...` (not `<word> = ...`) is the timer form.
    Timer {
        kind: String,
        interval: Expr,
        body: Vec<Stmt>,
    },
    /// `on elapsed(timer) do ... end` / `on elapsedOnce(timer) do ... end`
    /// (2026-08-24) — like `Timer` above, but the interval comes from a
    /// `Timer`/`preciseTimer` *object*'s own configured interval instead
    /// of an inline expression, so the same running/paused/stopped state a
    /// script controls via `start`/`pause`/`restart`/`stop`/`remove`
    /// governs whether this fires (checked live by `run_for` on each
    /// scheduled tick, not just at registration). `elapsed` repeats
    /// (`once=false`, mirrors `every`); `elapsedOnce` fires once
    /// (`once=true`, mirrors `after`/`at`). Contextual, not reserved: `on`
    /// is a plain identifier everywhere else (`on = 5` still assigns a
    /// variable named `on`) — only the exact shape `on elapsed(` / `on
    /// elapsedOnce(` triggers this form.
    OnElapsed {
        once: bool,
        timer: Expr,
        body: Vec<Stmt>,
    },
    /// `watch file(path) [do] ... end` / `watch url(url, [interval_seconds=])
    /// [do] ... end` / `watch <name> [do] ... end` (§ watch, 2026-09-01) —
    /// three trigger conditions checked by the new `pump_watches()` builtin
    /// (a fourth, serial-data arrival, is explicitly deferred until the
    /// concurrent serial-port work lands). Registration happens here, at
    /// the `watch` statement itself (which also snapshots the CURRENT
    /// state — the file's mtime, the variable's value, or a fresh fetch of
    /// the URL — as the baseline to compare against); the body never runs
    /// here.
    ///
    /// Same cooperative execution model as `Timer`/`OnElapsed` above (see
    /// their doc comments, and `pump_watches`'s own in `qu-interp` for the
    /// full reasoning): Qu's interpreter is single-threaded and has no
    /// background event loop, so a `watch` callback only ever runs when the
    /// script explicitly calls `pump_watches()` — never "in the background"
    /// while other code executes, even though "watch" reads as if it might.
    /// `pump_watches()` was chosen over reusing `run_for(duration)`
    /// specifically because it doesn't fit that virtual-time model: `run_for`
    /// drives registered timers across a SIMULATED clock with no real
    /// waiting, whereas a `watch`'s condition (a file's real mtime, a
    /// variable's real current value, an HTTP endpoint's real response) is
    /// external, real state that can only be checked for real, right now —
    /// there is no "simulated" file mtime to fast-forward through.
    ///
    /// `watch` is contextual (matching `every`/`on`/`pool`'s own
    /// precedent), not reserved: a plain variable/function named `watch`
    /// still parses normally (`watch = 5`, `watch(x)`) — only the three
    /// exact shapes above trigger this form (see `statement()`'s own
    /// handling).
    Watch {
        kind: WatchKind,
        body: Vec<Stmt>,
    },
    /// `pool <name> with cpu = n_cores() ... end pool` (§47.3 job-queue/
    /// worker-pool feature, 2026-08-26) — declares a named worker pool
    /// used later by `run <queue> on <name>`. `gpu` is parsed even though
    /// v1 only supports `cpu` workers — a nonzero `gpu` is a clear runtime
    /// error at pool-creation time, not a parse error (the grammar accepts
    /// it per spec, only the executor doesn't implement it yet). `policy`
    /// defaults to `"round_robin"` (the only value v1 actually implements
    /// — `priority`/`fair`/`affinity` parse but error clearly at
    /// pool-creation time) and `on_full` defaults to `"queue"`
    /// (`"reject"`/`"spill_to(<target>)"` parse the same way, same "parse
    /// it, error clearly if unsupported" treatment). `pool` is contextual
    /// (only recognized as this block form immediately before `<name>
    /// with`), not reserved — a plain variable named `pool`, or the
    /// anonymous-pool shorthand call `pool(n)` (parsed as an ordinary
    /// `Expr::Call` — no new grammar needed for that form), still work.
    /// `remote = ("host:port", ...)` (§ distributed job dispatch,
    /// 2026-08-31) — an optional additional resource alongside `cpu`/`gpu`:
    /// addresses of other Qu processes running `listen_pool(port,
    /// allow=(...))`, included as extra workers `run <queue> on
    /// <thisPool>` may dispatch jobs to. A parenthesized tuple (Qu's actual
    /// list-literal syntax, per `Expr::Tuple` — NOT bracket `[...]`, which
    /// is always a numeric Vec/Mat literal and can't hold strings; see
    /// `drop_accepts_a_list_of_column_names`'s test comment in
    /// `qu-interp/src/lib.rs` for the same convention already established
    /// elsewhere), evaluated (each element must be a string) at
    /// pool-creation time in `qu-interp`'s `Stmt::Pool` execution, same as
    /// `cpu`/`gpu`. `None` (the field is entirely absent from the `with`
    /// clause) means "no remote workers" — unchanged, purely local
    /// behavior from before this feature existed.
    Pool {
        name: String,
        cpu: Expr,
        gpu: Option<Expr>,
        remote: Option<Expr>,
        policy: String,
        on_full: String,
    },
    /// bare command verb with word/args, e.g. `grid on`, `show plot`
    Command { verb: String, words: Vec<String> },
    /// one-line function definition: `crest(x) := max(abs(x)) / rms(x)`
    /// (canonical, §41.1). The legacy `def fn crest(x) = …` spelling parses to
    /// the same node. `params` gained an optional per-parameter `: Tag`
    /// type annotation (§ multiple dispatch phase 1, 2026-08-27,
    /// `docs/design/multiple-dispatch.md` §1) — `Param{name, ty}` replaces
    /// the old bare `Vec<String>`; `ty: None` (every existing script, since
    /// nothing before this could write a `:` here) means untyped, matching
    /// any argument at dispatch time (see qu-interp's `methods` table).
    DefFn {
        name: String,
        params: Vec<Param>,
        body: Expr,
    },
    /// multi-statement function: `function name(params) … end function`
    /// — same `Param`-annotated params as `DefFn` above.
    ///
    /// `memoize` (§ function memoization, 2026-08-31): `true` when the
    /// declaration was written `memoize function name(params) ... end
    /// function` — a contextual modifier (like `parallel for`), not a
    /// reserved word, recognized only in that exact position (see
    /// `statement()`'s own handling just before `function`). `false` for
    /// every function declared without it, i.e. every function that
    /// existed before this feature. Only the block form carries this
    /// field — the one-line `DefFn` (`:=`) form was not asked to support
    /// memoization, and adding it there would need its own grammar
    /// decision this pass doesn't make.
    Function {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
        memoize: bool,
    },
    /// `return expr` inside a `function` body
    Return(Expr),
    /// `break` (§ break/continue/redo, scoped 2026-08-26) — exits the
    /// innermost enclosing `for`/`while` immediately. A clear runtime error
    /// ("break outside of a loop") if reached with no enclosing loop, never
    /// silently ignored — see `Interp::exec`'s `Stmt::Break` arm. Contextual
    /// (try-then-rewind style, matching `pool`/`run`/`wait`): only this
    /// exact shape — the bare word immediately followed by end-of-statement
    /// — is the control-flow form, so a plain variable/function named
    /// `break` (`break = 5`, `y = break`, `break()`) still works everywhere
    /// else. Rejected from the "fast for"/"fast while" register optimizer
    /// (`collect_fast_loop_locals`/`compile_fast_loop_body`'s catch-all
    /// `_ => return None/false` arms already reject any `Stmt` variant they
    /// don't explicitly know, so this needed no changes there) — a loop
    /// using `break` always falls back to the ordinary tree-walking
    /// interpreter, same treatment `Stmt::Return` already gets.
    /// `break` leaves one loop; `break N` leaves N of them.
    ///
    /// Without a count, escaping a nested loop needs a flag set in the
    /// inner loop and tested in the outer -- two statements far apart
    /// that must agree, which is exactly the arrangement that stops
    /// agreeing when someone edits one of them.
    Break(usize),
    /// `continue` — skips the rest of the current iteration's body and
    /// advances normally (`for`: next element; `while`: re-checks the
    /// condition). Same contextual-keyword and fast-path-rejection
    /// treatment as `Break` above.
    Continue,
    /// `redo` — Perl's exact `redo`: retries the current iteration from the
    /// top, WITHOUT advancing the `for` iterator or re-checking the `while`
    /// condition. Same contextual-keyword and fast-path-rejection treatment
    /// as `Break` above.
    Redo,
    Expr(Expr),
    /// `enum Name variant1[, variant2, ...] ... end [enum]` (2026-08-24) —
    /// a small, purely symbolic enum: a named type plus an ordered list of
    /// variant names, no per-variant payload and no compile-time
    /// exhaustiveness checking (Qu has no static type system yet — a real
    /// Rust/Swift-style tagged union is a deliberately separate,
    /// not-yet-approved conversation). One comma-separated group of
    /// variant names per line (or all on one line) — mirrors `function`'s
    /// block-with-`end` shape, not new grammar shape. A specific variant
    /// is reached by ordinary dot-access on the type (`Season.Spring`,
    /// `Expr::Field` — no new access syntax), validated at evaluation time
    /// against the declared list.
    EnumDef {
        name: String,
        variants: Vec<String>,
    },
    /// A synthetic marker, not real source syntax — `program()`/`block()`
    /// emit one immediately before every real statement they parse, so the
    /// interpreter can cheaply track "what line is currently executing"
    /// (`Interp::current_line`, read by `try/catch`'s exception record's
    /// `e.line` — spec §18.B) without threading a `Span`/line field
    /// through every existing `Stmt` variant and every place that builds
    /// or matches one (a much larger, more invasive change). Two call
    /// sites specifically must NOT treat this as a disqualifying "unknown
    /// statement" and fall back to slow-path interpretation for every
    /// function ever written: `collect_fn_locals`/`compile_fast_stmts`
    /// (the M4 Phase 0 soft-compile whitelist) skip it as a transparent
    /// no-op instead.
    SourceLine(u32),
}

/// The three `watch` trigger conditions (see `Stmt::Watch`'s own doc
/// comment). Each carries only the AST pieces needed at parse time — the
/// interpreter resolves these into its own runtime state (a `WatchDef` in
/// `qu-interp`) once at registration.
#[derive(Clone, Debug)]
pub enum WatchKind {
    /// `watch file(path) do ... end`
    File(Expr),
    /// `watch <name> do ... end`
    Var(String),
    /// `watch url(url, [interval_seconds]) do ... end` — the second `Expr`
    /// is the poll interval in REAL (wall-clock) seconds, from either a
    /// positional second argument or an `interval_seconds=` kwarg; `None`
    /// when omitted, in which case `qu-interp` applies its own default.
    Url(Expr, Option<Expr>),
}

#[derive(Clone, Debug)]
pub enum Expr {
    /// An anonymous function. Three spellings, one meaning:
    ///
    /// ```text
    /// x => x^2                      one parameter, no parentheses
    /// (x, y) := x + y               any number of parameters
    /// function(x, y) ... end function   a block body
    /// ```
    ///
    /// The last two are the NAMED forms with the name left out --
    /// `f(x) := expr` and `function f(x) ... end function` have been the
    /// two ways to define a function since long before this -- so a lambda
    /// needed no new token, and the terse and block versions stay in step
    /// with their named counterparts rather than drifting into a third
    /// syntax.
    ///
    /// `=>` came from the `journal-figures` branch, which needed to write
    /// a function where it was passed and reached the same conclusion by
    /// the same route: `->` is the obvious spelling and is already taken,
    /// `expr -> (r, c)` being reshape. It is kept because for one argument
    /// it reads better than any parenthesised form, and it parses to this
    /// same variant -- a spelling, not a second feature with its own
    /// scoping rules.
    Lambda { params: Vec<Param>, body: FnBody },
    Int(i64),
    Float(f64),
    Imag(f64),
    Str(String),
    /// An `r"..."` literal: finished text. Unlike `Str`, the interpreter
    /// runs neither escape decoding nor `{}` interpolation over it.
    RawStr(String),
    Unit(f64, String),
    Bool(bool),
    None,
    Name(String),
    Unary {
        op: String,
        rhs: Box<Expr>,
    },
    Binary {
        op: String,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `a to b [step c]`
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        step: Option<Box<Expr>>,
    },
    /// `x in Ohm`
    InUnit {
        value: Box<Expr>,
        unit: String,
    },
    /// `expr as contract`
    As {
        value: Box<Expr>,
        contract: Box<Expr>,
    },
    /// `f |> stage`
    Pipe {
        value: Box<Expr>,
        stage: Box<Expr>,
    },
    /// `cond ? then : else` — right-associative (`a ? b : c ? d : e` is
    /// `a ? b : (c ? d : e)`). Nesting a compact range literal or another
    /// ternary inside a branch needs parens: the branches parse at
    /// `as_expr` precedence, one level below the range-literal/`to` layer.
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    /// `a ?? b` — `a` unless it's `none`, else `b`. Short-circuits: `b` is
    /// only evaluated when `a` actually is `none`.
    Coalesce {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    Index {
        value: Box<Expr>,
        indices: Vec<Idx>,
    },
    Field {
        value: Box<Expr>,
        name: String,
    },
    /// postfix transpose. `conjugate` is true for `'` (conjugate/Hermitian
    /// transpose) and false for `.'`, `.T`, `transpose(...)` (plain
    /// transpose, no conjugation) — the MATLAB/NumPy distinction. There is
    /// deliberately no `^T`/`^H` spelling: it collides with "raise to the
    /// power of a variable named `T`/`H`" and silently prefers transpose.
    Transpose { value: Box<Expr>, conjugate: bool },
    /// `[1, 2; 3, 4]`
    Matrix(Vec<Vec<Expr>>),
    /// `{a = 1, b = 2}`
    Record(Vec<(String, Expr)>),
    /// parenthesized tuple `(a, b)`
    Tuple(Vec<Expr>),
    /// `run <queueExpr> on <poolExpr>` (grammar §47.3) — executes every
    /// deferred job on a `Value::Queue` (built via `queue()` + `.push`)
    /// across the given pool (a named `pool ... end pool` variable, the
    /// anonymous shorthand `pool(n)`, or the sentinel `any` for
    /// load-aware auto-routing across every registered pool — see
    /// `qu-interp`'s `eval_run`), blocking until every job is drained,
    /// and evaluates to a `Value::List` of each job's return value **in
    /// submission order** (not completion order — matters for
    /// reproducibility, same bar as `parallel for`'s own order-stability
    /// discipline). Deliberately an EXPRESSION, not just a statement —
    /// `results = run jobs on pool(8)` must work, not only a bare
    /// `run jobs on pool(8)` line — so it's parsed at the top of the
    /// expression precedence chain (`Parser::expression`), not in
    /// `statement()`'s keyword dispatch like `parallel for`/`every` are.
    /// `run` is contextual (try-then-rewind in the parser), not reserved:
    /// a plain variable/function named `run` still works.
    Run {
        queue: Box<Expr>,
        pool: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub enum Arg {
    Pos(Expr),
    Named(String, Expr),
}

#[derive(Clone, Debug)]
pub enum Idx {
    /// a single expression index
    Expr(Expr),
    /// `lo:step:hi` (MATLAB order, matching spec §15 and the bare
    /// `start:step:stop` range literal — any part optional); `:` alone is
    /// `Slice(None,None,None)`.
    Slice {
        lo: Option<Expr>,
        hi: Option<Expr>,
        step: Option<Expr>,
    },
}

// ------------------------------------------------------------------ errors

#[derive(Clone, Debug)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "parse error at {}:{}: {}",
            self.span.line, self.span.col, self.msg
        )
    }
}
impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

// ------------------------------------------------------------------ parser

pub struct Parser {
    toks: Vec<Token>,
    i: usize,
}

/// Parse a whole program from source text.
pub fn parse(src: &str) -> PResult<Program> {
    let toks = lex(src);
    let mut p = Parser { toks, i: 0 };
    p.program()
}

/// Parse a single expression (used for `{...}` string interpolation). Ranges
/// are permitted so `{0 to n}` works.
///
/// Requires the *entire* input to be consumed by one expression — not just
/// a leading prefix of it. Without this, `range_expr()` simply stops at the
/// first token it can't fold into the expression (e.g. the invalid `%` in
/// `n % 2`) and returns successfully with whatever partial expression it
/// managed to build (here, just `n`), silently discarding the rest. That
/// turned a typo inside `{...}` interpolation into a silent wrong-answer
/// instead of the loud parse error the exact same text gets everywhere else
/// in the language (e.g. `x = n % 2` correctly reports `Unknown('%')`). See
/// the interpolation bug write-up in IMPL.md for the full repro.
pub fn parse_expr(src: &str) -> PResult<Expr> {
    let toks = lex(src);
    let mut p = Parser { toks, i: 0 };
    p.skip_terms();
    let e = p.range_expr()?;
    p.skip_terms();
    if !matches!(p.peek(), Tok::Eof) {
        return p.err(format!(
            "unexpected {:?} after expression (interpolated `{{...}}` must contain exactly \
             one complete expression)",
            p.peek()
        ));
    }
    Ok(e)
}

/// Command verbs recognized in bare (paren-less) statement form.
/// The bare-word command verbs, public so `help`/`apropos` answer for them
/// from THIS list rather than a copy (§ help for commands, 2026-09-09).
/// A second list would drift the first time a verb was added, and the
/// symptom is a command that works and cannot be looked up.
pub const COMMAND_VERBS: &[&str] = &[
    "grid", "box", "hold", "show", "close", "legend", "clear", "axis", "colorbar", "next",
    "pan", "zoom", "yyaxis", "xxaxis", "split",
];

/// `print`/`disp`/`writeline`/`echo`/`printtex` also accept the bare
/// (no-parens) form, but unlike `COMMAND_VERBS` their argument is a full
/// expression — a function call, a variable, arithmetic, a string literal,
/// anything — not a sequence of literal words. See the bare-print check in
/// `statement()`.
const PRINT_VERBS: &[&str] = &["print", "disp", "writeline", "echo", "printtex"];

/// Root-identifier extraction for the `@expr` prefix-statement operator (see
/// the `@`-handling block in `statement()` and `Stmt::Assign`'s doc comment).
///
/// Walks down the leftmost "base" of a postfix chain — `Field`'s `value`,
/// `Index`'s `value`, `Transpose`'s `value`, and (conditionally, see below)
/// `Call`'s `callee` — until it bottoms out at a bare `Expr::Name`, and
/// returns that name. Returns `None` when the chain doesn't bottom out in a
/// plain variable at all, e.g. `a + b` (a `Binary`, no base to descend
/// into) or a literal.
///
/// `Call` is only chain-preserving when its callee is *itself* a
/// field/index/call/transpose — i.e. a method call on something, like
/// `data.drop(x)` (callee `Field{value: Name("data"), ..}`). When a call's
/// callee is directly a bare `Name`, that's an ordinary function invocation
/// with no receiver (`foo(x)`, callee `Name("foo")`) — `foo` there names
/// the function being called, not a variable holding a base value being
/// chained from, so it does NOT count as a root and the walk fails. This is
/// what makes `@foo()` a rejected input (per the feature's error-handling
/// contract) while `@data.drop("target")` and deeper chains like
/// `@data.drop("target").corrmat()` are accepted.
fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Name(n) => Some(n.as_str()),
        Expr::Field { value, .. } => root_ident(value),
        Expr::Index { value, .. } => root_ident(value),
        Expr::Transpose { value, .. } => root_ident(value),
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::Name(_) => None,
            other => root_ident(other),
        },
        _ => None,
    }
}

/// The closed set of `namespace.method` sugar pairs recognized by
/// `Parser::postfix_primary` (§ ergonomic API layer, 2026-08-31): maps
/// `(namespace, method)` to the real builtin function name to call instead.
/// Not a general/extensible namespacing mechanism — adding a new alias
/// means adding a match arm here, deliberately, not wiring up a new
/// concept. `timer.elapsed()` and `timer.stop()` both alias `toc()`: `toc()`
/// already reads the elapsed time WITHOUT resetting the stored `tic()`
/// start (see `Interp::eval_call`'s `"toc"` arm), so both are already,
/// genuinely, non-destructive peeks — no new interpreter state was needed
/// to give `elapsed()` correct non-resetting semantics.
///
/// `signals.square`/`signals.impulse`/`signals.pwm`/`signals.sawtooth`/
/// `signals.triangle` (§ argument-validation audit, 2026-09-01) are the odd
/// ones out in this set: unlike `table.load`/`timer.start`, the bare name
/// on the right isn't a *different* spelling of the underlying builtin —
/// it's the exact same name (`square` aliases `square`). The point isn't a
/// nicer name, it's disambiguation: `square` reads as "x squared" to a new
/// reader just as easily as "square wave", so `signals.square(...)` gives
/// an unambiguous way to spell the call when that confusion matters, with
/// zero effect on the bare name (still real, still callable, still the
/// exact same function either way).
fn namespace_method_alias(namespace: &str, method: &str) -> Option<&'static str> {
    match (namespace, method) {
        ("table", "load") => Some("read_csv"),
        ("table", "write") => Some("write_csv"),
        ("timer", "start") => Some("tic"),
        ("timer", "elapsed") => Some("toc"),
        ("timer", "stop") => Some("toc"),
        ("signals", "square") => Some("square"),
        ("signals", "impulse") => Some("impulse"),
        ("signals", "pwm") => Some("pwm"),
        ("signals", "sawtooth") => Some("sawtooth"),
        ("signals", "triangle") => Some("triangle"),
        _ => None,
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn peek_at(&self, k: usize) -> &Tok {
        self.toks
            .get(self.i + k)
            .map(|t| &t.tok)
            .unwrap_or(&Tok::Eof)
    }
    fn span(&self) -> Span {
        self.toks[self.i].span
    }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.i].tok.clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn at_op(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Keyword(k) if *k == kw)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        if self.at_op(op) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(ParseError {
            msg: msg.into(),
            span: self.span(),
        })
    }
    fn expect_op(&mut self, op: &str) -> PResult<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            self.err(format!("expected `{}`, found {:?}", op, self.peek()))
        }
    }
    /// skip statement separators (newlines and `;`)
    fn skip_terms(&mut self) {
        while matches!(self.peek(), Tok::Newline) || self.at_op(";") {
            self.bump();
        }
    }

    /// Requires that a just-parsed statement actually ends here — a
    /// newline, `;`, EOF, or (inside a block) one of its closing keywords —
    /// rather than silently letting `program`/`block` parse straight into
    /// the next statement's tokens. Without this, `print "hi"` (an
    /// expression statement `print` immediately followed by, on the same
    /// line, the unrelated expression statement `"hi"`) parsed as *two*
    /// statements with no error, and only failed later — confusingly, at
    /// runtime — when `print` was evaluated as a bare, undefined variable.
    fn expect_stmt_end(&self, stops: &[&str]) -> PResult<()> {
        if matches!(self.peek(), Tok::Newline | Tok::Eof) || self.at_op(";") {
            return Ok(());
        }
        if let Tok::Keyword(k) = self.peek() {
            if stops.contains(k) {
                return Ok(());
            }
        }
        self.err(format!(
            "expected end of statement (newline or `;`), found {:?}",
            self.peek()
        ))
    }

    fn program(&mut self) -> PResult<Program> {
        let mut stmts = Vec::new();
        self.skip_terms();
        while !matches!(self.peek(), Tok::Eof) {
            stmts.push(Stmt::SourceLine(self.span().line));
            stmts.push(self.statement()?);
            self.expect_stmt_end(&[])?;
            self.skip_terms();
        }
        Ok(Program { stmts })
    }

    /// Parse a block body until any of the given block-closing keywords, which
    /// is left unconsumed.
    fn block(&mut self, stops: &[&str]) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        self.skip_terms();
        loop {
            if matches!(self.peek(), Tok::Eof) {
                break;
            }
            if let Tok::Keyword(k) = self.peek() {
                if stops.contains(k) {
                    break;
                }
            }
            // A stop word may also be an ordinary IDENTIFIER. `case` and
            // `loop` are not keywords on purpose -- `select` is a builtin
            // name and reserving these words would break any script using
            // them -- so a block that must end at `case` has to recognise
            // it here. No existing caller is affected: every stop word
            // used before this was a keyword, and a keyword can never
            // arrive as an `Ident`.
            if let Tok::Ident(w) = self.peek() {
                if stops.contains(&w.as_str()) {
                    break;
                }
            }
            out.push(Stmt::SourceLine(self.span().line));
            out.push(self.statement()?);
            self.expect_stmt_end(stops)?;
            self.skip_terms();
        }
        Ok(out)
    }

    fn statement(&mut self) -> PResult<Stmt> {
        // `<name> means <name>` -- contextual, not a keyword. Checked
        // before anything else consumes the leading identifier, and only
        // when the shape matches exactly, so `means` stays usable as an
        // ordinary variable name.
        if let (Tok::Ident(unit), Tok::Ident(word)) = (self.peek().clone(), self.peek_at(1).clone())
        {
            if word == "means" {
                if let Tok::Ident(meaning) = self.peek_at(2).clone() {
                    self.bump();
                    self.bump();
                    self.bump();
                    return Ok(Stmt::UnitMeans { unit, meaning });
                }
            }
        }
        // `ignore warning <name>` -- same contextual treatment.
        if let (Tok::Ident(a), Tok::Ident(b)) = (self.peek().clone(), self.peek_at(1).clone()) {
            if a == "ignore" && b == "warning" {
                if let Tok::Ident(name) = self.peek_at(2).clone() {
                    self.bump();
                    self.bump();
                    self.bump();
                    return Ok(Stmt::IgnoreWarning(name));
                }
            }
        }
        // keyword-led statements
        if self.at_kw("backend") {
            self.bump();
            let name = self.ident_or_kw_word("backend name")?;
            return Ok(Stmt::Backend(name));
        }
        if self.at_kw("import") {
            self.bump();
            let source = match self.peek().clone() {
                Tok::Str(path) => {
                    self.bump();
                    ImportSource::File(path)
                }
                _ => ImportSource::Native(self.ident_or_kw_word("module name")?),
            };
            let alias = if self.eat_kw("as") {
                Some(self.expect_name()?)
            } else {
                None
            };
            return Ok(Stmt::Import { source, alias });
        }
        if self.at_kw("global") {
            self.bump();
            let mut names = vec![self.expect_name()?];
            while self.eat_op(",") {
                names.push(self.expect_name()?);
            }
            return Ok(Stmt::Global(names));
        }
        // `type implicit` / `type explicit` — `type` is contextual (not reserved)
        if matches!(self.peek(), Tok::Ident(w) if w == "type")
            && matches!(self.peek_at(1), Tok::Ident(m) if m == "implicit" || m == "explicit")
        {
            self.bump(); // type
            let m = self.expect_name()?;
            return Ok(Stmt::TypeMode { explicit: m == "explicit" });
        }
        if self.at_kw("const") || self.at_kw("constant") {
            self.bump();
            let name = self.expect_name()?;
            // optional `as contract` ignored for now
            if self.eat_kw("as") {
                let _ = self.postfix()?;
            }
            self.expect_op("=")?;
            let rhs = self.assign_rhs()?;
            return Ok(Stmt::Const { name, rhs });
        }
        // `unit <name> = <expr>` — user-defined unit declaration. `unit` is
        // contextual, exactly like `type implicit`/`type explicit` above: a
        // plain identifier everywhere else (`unit = 5`, `x.unit`, `unit()`
        // all keep their current meaning unchanged), and only becomes this
        // statement when followed by another bare identifier and then `=`
        // — the same three-token lookahead the `type` case already uses to
        // stay contextual rather than reserving a new keyword.
        if matches!(self.peek(), Tok::Ident(w) if w == "unit")
            && matches!(self.peek_at(1), Tok::Ident(_))
            && matches!(self.peek_at(2), Tok::Op("="))
        {
            self.bump(); // unit
            let name = self.expect_name()?;
            self.expect_op("=")?;
            let rhs = self.assign_rhs()?;
            return Ok(Stmt::UnitDecl { name, rhs });
        }
        if self.at_kw("for") {
            return self.for_stmt();
        }
        if self.at_kw("while") {
            return self.while_stmt();
        }
        if self.at_kw("if") {
            return self.if_stmt();
        }
        // `select case <subject>` — contextual, like `parallel for` above:
        // `select` is an ordinary identifier (there is a `select` builtin),
        // so it only introduces this statement when `case` follows it.
        // Anything else named `select` keeps working.
        if matches!(self.peek(), Tok::Ident(w) if w == "select")
            && matches!(self.peek_at(1), Tok::Ident(c) if c == "case")
        {
            return self.select_stmt();
        }
        // `do ... loop while|until <cond>` and `repeat ... until <cond>`.
        if self.at_kw("do") || self.at_kw("repeat") {
            return self.do_loop_stmt();
        }
        if self.at_kw("try") {
            return self.try_stmt();
        }
        if self.at_kw("unsafe") {
            return self.unsafe_stmt();
        }
        // `memoize function name(params) ... end function` (§ function
        // memoization, 2026-08-31) — `memoize` is contextual, recognized
        // only directly before the `function` keyword (same "try-then-
        // commit" pattern `parallel for` and `type implicit/explicit` above
        // already use), so a plain variable/function named `memoize` still
        // works everywhere else (`memoize = 5`, `memoize()`, ...).
        if matches!(self.peek(), Tok::Ident(w) if w == "memoize") && matches!(self.peek_at(1), Tok::Keyword("function")) {
            self.bump(); // memoize
            let mut stmt = self.function_stmt()?;
            if let Stmt::Function { memoize, .. } = &mut stmt {
                *memoize = true;
            }
            return Ok(stmt);
        }
        if self.at_kw("function") {
            return self.function_stmt();
        }
        if self.at_kw("return") {
            self.bump();
            // `return` with no expression yields nothing
            if matches!(self.peek(), Tok::Newline | Tok::Eof) || self.at_op(";") {
                return Ok(Stmt::Return(Expr::None));
            }
            let e = self.range_expr()?;
            return Ok(Stmt::Return(e));
        }

        // `break` / `continue` / `redo` (§ break/continue/redo, scoped
        // 2026-08-26) — see each `Stmt` variant's own doc comment for
        // semantics. None of the three is a reserved word (`qu-lexer`'s
        // `KEYWORDS` list is untouched); like `pool`/`run`/`wait`, they're
        // contextual, recognized only in this exact shape: the bare word
        // with nothing else on the statement — immediately followed by a
        // newline, `;`, EOF (the same "bare word, no expression" lookahead
        // `return`'s own no-value case uses just above), or one of the
        // block-closing keywords (`end`/`else`/`elseif`/`until`/`catch`) so
        // the equally common single-line form (`if x > 5 then break end
        // if`) is recognized too, not just one-per-line style. Any other
        // following token — `=`, `(`, a binary operator, `.`, `[`, ... —
        // means the word is being used as an ordinary variable/function/
        // call instead, and falls through untouched to the identifier-led
        // statement handling below (so `break = 5`, `y = break + 1`,
        // `continue()`, `redo.field` all still parse as plain expressions/
        // assignments involving a variable of that name).
        if let Tok::Ident(w) = self.peek().clone() {
            let stmt_ends_here = matches!(self.peek_at(1), Tok::Newline | Tok::Eof)
                || matches!(self.peek_at(1), Tok::Op(";"))
                || matches!(self.peek_at(1), Tok::Keyword("end" | "else" | "elseif" | "until" | "catch"));
            // `break N` -- a count, so the statement does NOT end here.
            if w == "break" && matches!(self.peek_at(1), Tok::Int(_)) {
                self.bump(); // break
                let n = match self.peek() {
                    Tok::Int(n) => *n,
                    _ => unreachable!(),
                };
                self.bump();
                if n < 1 {
                    return self.err("`break N` needs N >= 1 -- `break 1` is the plain `break`");
                }
                return Ok(Stmt::Break(n as usize));
            }
            if matches!(w.as_str(), "break" | "continue" | "redo") && stmt_ends_here {
                self.bump();
                return Ok(match w.as_str() {
                    "break" => Stmt::Break(1),
                    "continue" => Stmt::Continue,
                    "redo" => Stmt::Redo,
                    _ => unreachable!(),
                });
            }
        }

        // `@expr` — prefix statement operator (§ self-assign chain,
        // 2026-08-26): generalizes the existing `+=`/`-=`/`*=`/`/=`/`.=`
        // compound-assignment operators from binary ops to arbitrary
        // method/field/index chains. `@data.drop("target")` means
        // `data = data.drop("target")`; `@data.drop("target").corrmat()`
        // means `data = data.drop("target").corrmat()`. `@` prefixes a bare
        // EXPRESSION statement only — it is not itself an assignment
        // operator, so `@x = x + 1` is not this form: `@x` parses fine
        // (rooted in `x`) but the trailing `= x + 1` then has no statement
        // to attach to and errors normally, same as any other stray `=`
        // after a complete statement.
        //
        // Parses the expression after `@` with the ordinary expression
        // parser (no second grammar), then desugars directly to
        // `Stmt::Assign` by finding the root variable the chain is rooted
        // in via `root_ident` — see its doc comment for the exact walk and
        // why a bare call like `foo()` doesn't count as rooted. A chain
        // that isn't rooted in a plain variable (`@(a + b)`, `@foo()`) is a
        // clear parse error naming the problem, never a silent no-op.
        if self.at_op("@") {
            self.bump(); // @
            let e = self.expression()?;
            return match root_ident(&e) {
                Some(name) => {
                    let name = name.to_string();
                    Ok(Stmt::Assign { name, op: String::new(), rhs: e, in_place: true })
                }
                None => self.err(format!(
                    "`@`: expression `{:?}` is not rooted in a plain variable — \
                     `@` needs a variable/field/method/index chain like \
                     `@data.drop(\"x\")`, not a bare function call or a \
                     computed expression",
                    e
                )),
            };
        }

        // `def fn name(params) = expr`  (legacy alias for `name(params) := expr`)
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "def" && matches!(self.peek_at(1), Tok::Ident(f) if f == "fn") {
                return self.def_fn();
            }
        }

        // `name(params) := expr` — canonical one-line function definition (§41.1)
        if let Tok::Ident(name) = self.peek().clone() {
            if matches!(self.peek_at(1), Tok::Op("(")) {
                if let Some(after) = self.paren_close_offset(1) {
                    if matches!(self.peek_at(after + 1), Tok::Op(":=")) {
                        return self.fn_def_short(name);
                    }
                }
            }
        }

        // `parallel for <name> in ... end parallel` — contextual (only
        // recognized as this form immediately before `for`), not reserved.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "parallel" && matches!(self.peek_at(1), Tok::Keyword("for")) {
                return self.parallel_for_stmt();
            }
        }

        // `every|after|at <ms> [do] ... end` — contextual, not reserved (see
        // `Stmt::Timer`'s doc comment): only the timer form when NOT
        // immediately followed by an assignment operator, so `every = 5`
        // still assigns a plain variable named `every`.
        if let Tok::Ident(name) = self.peek().clone() {
            if matches!(name.as_str(), "every" | "after" | "at")
                && !matches!(self.peek_at(1), Tok::Op("=" | ":=" | "+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^="))
            {
                return self.timer_stmt(&name);
            }
        }

        // `on elapsed(timer) do ... end` / `on elapsedOnce(timer) do ... end`
        // — contextual (see `Stmt::OnElapsed`'s doc comment): only this
        // exact three-token shape (`on`, then literally `elapsed` or
        // `elapsedOnce`, then `(`) triggers it, so `on = 5`, `on(x)`, or a
        // variable/function named `on` used any other way still parses
        // normally.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "on" {
                if let Tok::Ident(ev) = self.peek_at(1).clone() {
                    if matches!(ev.as_str(), "elapsed" | "elapsedOnce") && matches!(self.peek_at(2), Tok::Op("(")) {
                        return self.on_elapsed_stmt(ev == "elapsedOnce");
                    }
                }
            }
        }

        // `watch file(path) [do] ... end` / `watch url(url, [interval_seconds=])
        // [do] ... end` / `watch <name> [do] ... end` (§ watch, 2026-09-01) —
        // contextual (see `Stmt::Watch`'s doc comment): only these exact
        // shapes trigger it, so a plain variable/function named `watch` still
        // parses normally (`watch = 5`, `watch(x)`).
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "watch" {
                if let Tok::Ident(k) = self.peek_at(1).clone() {
                    if k == "file" && matches!(self.peek_at(2), Tok::Op("(")) {
                        return self.watch_file_stmt();
                    }
                    if k == "url" && matches!(self.peek_at(2), Tok::Op("(")) {
                        return self.watch_url_stmt();
                    }
                    // `watch <name> [do] ... end` — bare variable-value watch.
                    // Only when what follows the name actually starts a block
                    // (`do`/newline/`;`/EOF), same "not an assignment or call"
                    // guard `every`/`after`/`at` use above, so `watch x = 5`,
                    // `watch x(1,2)`, etc. still parse as ordinary statements
                    // involving a variable/function named `watch`.
                    if matches!(self.peek_at(2), Tok::Keyword("do") | Tok::Newline | Tok::Eof | Tok::Op(";")) {
                        return self.watch_var_stmt();
                    }
                }
            }
        }

        // `pool <name> with cpu = ... end pool` (§47.3) — contextual (see
        // `Stmt::Pool`'s doc comment), not reserved: only this exact
        // three-token shape (`pool`, then an identifier, then the `with`
        // keyword) triggers it, so a plain variable named `pool`, or the
        // anonymous shorthand call `pool(n)`, still parse normally.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "pool"
                && matches!(self.peek_at(1), Tok::Ident(_))
                && matches!(self.peek_at(2), Tok::Keyword("with"))
            {
                return self.pool_stmt();
            }
        }

        // `enum Name variant1[, variant2, ...] ... end [enum]` — contextual
        // (see `Stmt::EnumDef`'s doc comment), not reserved: only when an
        // identifier immediately follows (the type name), so `enum = 5` /
        // `enum(x)` still work with `enum` as a plain variable/call name.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "enum" && matches!(self.peek_at(1), Tok::Ident(_)) {
                return self.enum_stmt();
            }
        }

        // `wait until COND [do] ... end [until]` (scoped 2026-08-26) —
        // inverted-condition sugar for `while not (COND) ... end while`,
        // the same relationship Ruby's `until` has to `while`. Desugars
        // straight to `Stmt::While` at parse time rather than introducing a
        // new AST node or execution path: there is no `Stmt::WaitUntil`
        // anywhere in the interpreter, so it automatically gets every
        // `while` behavior for free (the "fast while" register optimizer,
        // and — once `break`/`continue`/`redo` land for `while`, see
        // BACKLOG.md — those too, with zero additional work here).
        // `wait` is contextual (try-then-rewind style, matching `pool`/
        // `run`): only recognized immediately before the literal keyword
        // `until`, so a plain variable/function named `wait` still parses
        // normally (`wait = 5`, `wait(ms)`, ...). `until` itself is already
        // a reserved word in `qu-lexer` (unused until now) so it needs no
        // contextual handling of its own.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "wait" && matches!(self.peek_at(1), Tok::Keyword("until")) {
                return self.wait_until_stmt();
            }
        }

        // `lazy name = expr` (§ lazy variables, 2026-08-31) — contextual
        // (see `Stmt::LazyAssign`'s doc comment), not reserved: only this
        // exact three-token shape (`lazy`, then an identifier, then `=`)
        // triggers it, so a plain variable/function named `lazy` still
        // parses normally (`lazy = 5`, `lazy()`, ...). Deliberately does
        // NOT accept `+=`/`-=`/etc. or `as contract` after the name — lazy
        // declares a fresh deferred binding, not a compound update, so
        // `lazy x += 1` falls through to the ordinary identifier-led
        // handling below and parses `lazy` itself as a plain variable name
        // instead (consistent with how `every`/`after`/`at` only claim the
        // timer form when a bare `=` doesn't immediately follow).
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "lazy" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Op("=")) {
                self.bump(); // lazy
                let name = self.expect_name()?;
                self.expect_op("=")?;
                let rhs = self.assign_rhs()?;
                return Ok(Stmt::LazyAssign { name, rhs });
            }
        }

        // `ref name = expr` (§ ref variables, 2026-09-01) — contextual, same
        // three-token lookahead shape as `lazy` immediately above, so a
        // plain variable/function named `ref` still parses normally
        // (`ref = 5`, `ref()`, ...). See `Stmt::RefAssign`'s doc comment for
        // why `rhs` is parsed as a full expression rather than restricted to
        // a bare identifier here.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "ref" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Op("=")) {
                self.bump(); // ref
                let name = self.expect_name()?;
                self.expect_op("=")?;
                let rhs = self.assign_rhs()?;
                return Ok(Stmt::RefAssign { name, rhs });
            }
        }

        // identifier-led statements
        if let Tok::Ident(name) = self.peek().clone() {
            // `print <expr>` (no parens) — sugar for `print(<expr>)`. Only
            // when something actually follows on the same line and it isn't
            // `(` (the ordinary parenthesized call, left to the expression-
            // statement fallback below) or an assignment operator (so
            // `print = 5` still assigns a variable literally named `print`,
            // consistent with how `every`/`after`/`at` are guarded above).
            if PRINT_VERBS.contains(&name.as_str())
                && !matches!(
                    self.peek_at(1),
                    Tok::Op("(" | "=" | ":=" | "+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^=" | ";")
                        | Tok::Newline
                        | Tok::Eof
                )
            {
                self.bump(); // verb
                let arg = self.range_expr()?;
                return Ok(Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::Name(name)),
                    args: vec![Arg::Pos(arg)],
                }));
            }
            // command form: `Ident word ...` where the next token starts a bare
            // word (ident/keyword) rather than an operator/paren/assignment --
            // OR the verb is alone on its line (immediately followed by a
            // statement terminator). Before this fix, `next_is_bare_word()`
            // alone meant a COMMAND_VERB with NO trailing word (bare `legend`,
            // `colorbar`, `box`, `close`, `next`, ...) could never reach
            // `command_stmt` at all -- it fell through to plain identifier-
            // expression parsing and failed at eval time as "not defined",
            // even though every one of these verbs' interpreter-side handlers
            // already has a real, intentional zero-word case (e.g. `legend`'s
            // `w.is_empty()` arm just turns the legend on). Found live via
            // QuStudio's own bundled "Window Functions" DSP template, which
            // uses bare `legend` (2026-09-02).
            if COMMAND_VERBS.contains(&name.as_str())
                && (self.next_is_bare_word() || matches!(self.peek_at(1), Tok::Newline | Tok::Eof | Tok::Op(";")))
            {
                return self.command_stmt(name);
            }
            // indexed / logical-mask assignment: `x[...] = v`, `x[mask] op= v`
            if matches!(self.peek_at(1), Tok::Op("[")) {
                if let Some(op) = self.index_assign_op() {
                    self.bump(); // name
                    let indices = self.index_list()?;
                    self.bump(); // the assign operator token
                    let rhs = self.assign_rhs()?;
                    return Ok(Stmt::IndexAssign { name, indices, op, rhs });
                }
            }
            // `name.field = value`. Guarded on the `=` three tokens along,
            // so `obj.method(...)` -- which has `(` there -- is untouched.
            if matches!(self.peek_at(1), Tok::Op("."))
                && matches!(self.peek_at(3), Tok::Op("="))
            {
                if let Tok::Ident(field) = self.peek_at(2).clone() {
                    self.bump(); // name
                    self.bump(); // .
                    self.bump(); // field
                    self.bump(); // =
                    let rhs = self.assign_rhs()?;
                    return Ok(Stmt::FieldAssign { name, field, rhs });
                }
            }
            match self.peek_at(1) {
                Tok::Op("=") => {
                    self.bump(); // name
                    self.bump(); // =
                    let rhs = self.assign_rhs()?;
                    return Ok(Stmt::Assign {
                        name,
                        op: String::new(),
                        rhs,
                        in_place: false,
                    });
                }
                Tok::Op(":=") => {
                    self.bump();
                    self.bump();
                    let rhs = self.assign_rhs()?;
                    return Ok(Stmt::Deferred { name, rhs });
                }
                Tok::Op(o @ ("+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^=")) => {
                    let op = o.trim_end_matches('=').to_string();
                    self.bump();
                    self.bump();
                    let rhs = self.assign_rhs()?;
                    return Ok(Stmt::Assign { name, op, rhs, in_place: false });
                }
                Tok::Keyword("as") => {
                    self.bump(); // name
                    self.bump(); // as
                    let contract = self.postfix()?;
                    // `name as contract = rhs` declares and initializes together.
                    if self.eat_op("=") {
                        let rhs = self.assign_rhs()?;
                        return Ok(Stmt::Declare { name, contract, rhs });
                    }
                    return Ok(Stmt::Contract { name, contract });
                }
                _ => { /* fall through to expression statement */ }
            }
        }

        // expression statement
        let e = self.expression()?;
        Ok(Stmt::Expr(e))
    }

    /// Lookahead: with `Ident` at cursor and `[` next, scan to the matching
    /// `]` and, if an assignment operator follows, return its compound op
    /// (`""` for plain `=`). Otherwise `None` (it's an indexing expression).
    fn index_assign_op(&self) -> Option<String> {
        let mut k = 1; // the `[`
        let mut depth = 0i32;
        loop {
            match self.peek_at(k) {
                Tok::Op("[") => depth += 1,
                Tok::Op("]") => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Eof => return None,
                _ => {}
            }
            k += 1;
        }
        match self.peek_at(k + 1) {
            Tok::Op("=") => Some(String::new()),
            Tok::Op(o @ ("+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^=")) => {
                Some(o.trim_end_matches('=').to_string())
            }
            _ => None,
        }
    }

    /// True if the current token can begin an expression (used by the
    /// paren-less `where` prefix).
    fn starts_expr(&self) -> bool {
        match self.peek() {
            Tok::Int(_) | Tok::Float(_) | Tok::Imag(_) | Tok::Str(_) | Tok::RawStr(_) | Tok::UnitLit(_, _)
            | Tok::Ident(_) => true,
            Tok::Keyword(k) => matches!(*k, "true" | "false" | "none" | "not"),
            Tok::Op(o) => matches!(*o, "(" | "[" | "{" | "-" | "+" | "~"),
            _ => false,
        }
    }

    fn next_is_bare_word(&self) -> bool {
        matches!(self.peek_at(1), Tok::Ident(_) | Tok::Keyword(_))
            && !matches!(self.peek_at(1), Tok::Keyword("as"))
    }

    fn command_stmt(&mut self, verb: String) -> PResult<Stmt> {
        self.bump(); // verb
        let mut words = Vec::new();
        loop {
            match self.peek().clone() {
                Tok::Ident(w) => {
                    self.bump();
                    words.push(w);
                }
                Tok::Keyword(k) => {
                    self.bump();
                    words.push(k.to_string());
                }
                Tok::Str(s) => {
                    self.bump();
                    words.push(s);
                }
                // numeric/index words for commands like `clear panel 1, 2, 2`
                // or `axis scale 2 log` — kept as their literal text; the
                // executor re-parses whichever ones it expects to be numbers.
                Tok::Int(n) => {
                    self.bump();
                    words.push(n.to_string());
                }
                Tok::Float(f) => {
                    self.bump();
                    words.push(f.to_string());
                }
                Tok::Op(",") => {
                    self.bump(); // separator between index words, not itself a word
                }
                _ => break,
            }
        }
        Ok(Stmt::Command { verb, words })
    }

    fn def_fn(&mut self) -> PResult<Stmt> {
        self.bump(); // def
        self.bump(); // fn
        let name = self.expect_name()?;
        // reuses the same `: Tag`-aware parameter list `fn_def_short`/
        // `function_stmt` use, rather than a second copy of the loop, so
        // this legacy spelling gets the type-annotation grammar for free.
        let params = self.param_list()?;
        self.expect_op("=")?;
        let body = self.expression()?;
        Ok(Stmt::DefFn { name, params, body })
    }

    /// Offset (relative to the cursor) of the `)` matching the `(` at
    /// `self.peek_at(open)`. Returns `None` if unbalanced. Used to look past a
    /// parameter list when deciding if `name(...)` begins a function definition.
    fn paren_close_offset(&self, open: usize) -> Option<usize> {
        let mut k = open;
        let mut depth = 0i32;
        loop {
            match self.peek_at(k) {
                Tok::Op("(") => depth += 1,
                Tok::Op(")") => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                Tok::Eof => return None,
                _ => {}
            }
            k += 1;
        }
    }

    /// Parse a parenthesized parameter list, cursor sitting on `(`. Each
    /// parameter is a bare name, optionally followed by `: Tag` (§ multiple
    /// dispatch phase 1, 2026-08-27) — since no existing script could ever
    /// write a `:` here (this is genuinely new grammar, not a reinterpreted
    /// existing form), every parameter with no suffix parses to `ty: None`
    /// exactly as `param_list` behaved before this feature existed.
    ///
    /// Default values (2026-08-31): after the optional `: Tag`, an
    /// optional `= expr` (parsed at `range_expr` level, same as a call's
    /// `name = expr` keyword argument, so `order = 0 to 4`-style defaults
    /// are allowed too) makes the parameter optional. Once one parameter
    /// in the list has a default, every parameter after it must have one
    /// too — enforced here, not left as an interpreter-time surprise —
    /// matching how positional-then-keyword ordering works in essentially
    /// every language with default parameters: a required parameter after
    /// an optional one would create an unfillable positional gap. A
    /// parameter with no suffix at all is completely unaffected
    /// (`ty: None, default: None`), so this is a purely additive grammar
    /// extension exactly like the `: Tag` addition above it.
    fn param_list(&mut self) -> PResult<Vec<Param>> {
        self.expect_op("(")?;
        let mut params = Vec::new();
        let mut seen_default: Option<String> = None;
        if !self.at_op(")") {
            loop {
                let name = self.expect_name()?;
                let ty = if self.eat_op(":") {
                    self.param_type_tag()?
                } else {
                    None
                };
                let default = if self.eat_op("=") {
                    Some(self.range_expr()?)
                } else {
                    None
                };
                match (&seen_default, &default) {
                    (Some(earlier), None) => {
                        return self.err(format!(
                            "parameter `{name}` has no default value, but earlier parameter \
                             `{earlier}` does -- once one parameter has a default, every \
                             parameter after it must have one too"
                        ));
                    }
                    (None, Some(_)) => seen_default = Some(name.clone()),
                    _ => {}
                }
                params.push(Param { name, ty, default });
                if self.eat_op(",") {
                    continue;
                } else {
                    break;
                }
            }
        }
        self.expect_op(")")?;
        Ok(params)
    }

    /// Parses the `Tag` in a parameter's `: Tag` suffix, cursor sitting
    /// right after the `:`. `Tag` is a single identifier from the closed
    /// [`TYPE_TAGS`] vocabulary (§1 of the design doc), contextual exactly
    /// like `type`/`every`/`parallel`/`on` elsewhere in this parser — it's
    /// recognized only in this one grammar position, so a variable named
    /// `vec`/`model`/etc. is completely unaffected everywhere else. `none`
    /// is the one vocabulary word that's ALSO a globally-reserved keyword
    /// (`qu-lexer::KEYWORDS`), so it arrives as `Tok::Keyword("none")`
    /// rather than `Tok::Ident`; every other tag word is a plain
    /// identifier. `any` is an explicit synonym for untyped and returns
    /// `None` rather than a real `TypeTag`.
    ///
    /// Phase 2 (2026-08-27, design doc §1.1): after a bare `model` or
    /// `record` tag, an optional `<...>` refinement narrows the match —
    /// `model<"filter">` (a string literal, since `model`'s existing
    /// runtime tag is `ModelHandle.kind: String`) or `record<Circle>` (a
    /// bare identifier — see the `__type` field convention documented on
    /// `record_type_tag` in qu-interp). `<`/`>` are already standalone
    /// `Tok::Op` tokens (the lexer matches `<=`/`>=` longest-first), so
    /// `model<"filter">` lexes as `Ident("model")`, `Op("<")`,
    /// `Str("filter")`, `Op(">")` with no grammar ambiguity against
    /// comparison operators. Any other tag word followed by `<` is a parse
    /// error (refinement only makes sense for `model`/`record`).
    fn param_type_tag(&mut self) -> PResult<Option<TypeTag>> {
        let word = match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                s
            }
            Tok::Keyword(k) if k == "none" => {
                self.bump();
                "none".to_string()
            }
            _ => {
                return self.err(
                    "expected a type name after ':' (e.g. num, vec, mat, model, any)",
                )
            }
        };
        if word == "any" {
            return Ok(None);
        }
        if !TYPE_TAGS.contains(&word.as_str()) {
            return self.err(format!(
                "unknown type tag `{word}` after ':' -- expected one of: {}, or any",
                TYPE_TAGS.join(", ")
            ));
        }
        if self.eat_op("<") {
            let refine = match word.as_str() {
                "model" => match self.peek().clone() {
                    Tok::Str(s) => {
                        self.bump();
                        s
                    }
                    _ => {
                        return self.err(
                            "expected a string literal kind after `model<`, e.g. model<\"filter\">",
                        )
                    }
                },
                "record" => self.expect_name()?,
                other => {
                    return self.err(format!(
                        "type tag `{other}` does not support a `<...>` refinement -- only `model<\"kind\">` and `record<Tag>` do"
                    ))
                }
            };
            self.expect_op(">")?;
            return Ok(Some(TypeTag { name: word, refine: Some(refine) }));
        }
        Ok(Some(TypeTag::plain(word)))
    }

    /// `name(params) := expr` — canonical one-line function definition (§41.1).
    fn fn_def_short(&mut self, name: String) -> PResult<Stmt> {
        self.bump(); // name
        let params = self.param_list()?;
        self.expect_op(":=")?;
        let body = self.expression()?;
        Ok(Stmt::DefFn { name, params, body })
    }

    /// `function name(params) … end [function]` — multi-statement function.
    /// Phase 3 (§5): `name` may also be an operator spelling (`function
    /// +(a: record, b: record) ... end function`), via `expect_fn_name`.
    fn function_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // function
        let name = self.expect_fn_name()?;
        let params = self.param_list()?;
        let body = self.block(&["end"])?;
        self.expect_end(&["function"])?;
        Ok(Stmt::Function { name, params, body, memoize: false })
    }

    /// Like [`expect_name`](Self::expect_name), but also accepts one of
    /// [`OVERLOADABLE_OPERATORS`] as a function name — the grammar
    /// accommodation § multiple dispatch phase 3 (design doc §5) needs for
    /// `function +(a: record, b: record) ... end function` to overload the
    /// `+` operator for a user record type. Only `function_stmt` (the block
    /// form) calls this: the one-line `name(params) := expr` form is only
    /// even attempted by `statement()`'s own lookahead when the current
    /// token is already a `Tok::Ident` (line ~785 above), which an operator
    /// token never is — so that spelling is unambiguously out of reach here
    /// without a separate change to `statement()`'s own dispatch, not asked
    /// for by phase 3's block-form example.
    fn expect_fn_name(&mut self) -> PResult<String> {
        if let Tok::Op(op) = self.peek().clone() {
            if OVERLOADABLE_OPERATORS.contains(&op) {
                self.bump();
                return Ok(op.to_string());
            }
        }
        self.expect_name()
    }

    /// `enum Name variant1[, variant2, ...] ... end [enum]` — see
    /// `Stmt::EnumDef`'s doc comment. Variant names may be spread one per
    /// line or comma-separated on one line (or a mix); duplicates within
    /// the same block are rejected here, at parse time, rather than left
    /// for the interpreter to catch later.
    fn enum_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // enum
        let name = self.expect_name()?;
        self.skip_terms();
        let mut variants: Vec<String> = Vec::new();
        loop {
            if matches!(self.peek(), Tok::Eof) {
                return self.err("expected `end` to close `enum` block");
            }
            if self.at_kw("end") {
                break;
            }
            loop {
                let v = self.expect_name()?;
                if variants.contains(&v) {
                    return self.err(format!(
                        "enum `{name}` declares variant `{v}` more than once"
                    ));
                }
                variants.push(v);
                if self.eat_op(",") {
                    continue;
                }
                break;
            }
            self.expect_stmt_end(&["end"])?;
            self.skip_terms();
        }
        self.expect_end(&["enum"])?;
        if variants.is_empty() {
            return self.err(format!("enum `{name}` declares no variants"));
        }
        Ok(Stmt::EnumDef { name, variants })
    }

    /// `for <name> = <range-expr> ... end for` (numeric range/`Value::Vec`,
    /// the original form) or `for <name> in <expr> ... end for` (§ `for`
    /// over collections, scoped 2026-08-26) — both spellings produce the
    /// exact same `Stmt::For` node; there is no separate AST shape for
    /// `in`. The two forms aren't actually distinguished by grammar intent,
    /// only by what the range expression evaluates to at runtime
    /// (`Interp::eval_for_iterable`): a `Value::List` (from `list_dir`/
    /// `pmap`/`grep`/`head`/`tail`/etc., possibly holding non-numeric
    /// elements) needs `in`'s new grammar slot to have anywhere to be
    /// written at all, since `for x = xs` used to hard-parse only as far as
    /// `range_expr()` reaching a numeric range or `Value::Vec` — but a
    /// `Value::Vec`/numeric range works identically under either spelling
    /// (`for x = 1 to 10` and `for x in 1 to 10` are the same loop), kept
    /// that way for consistency rather than restricting `in` to lists only.
    fn for_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // for
        let var = self.expect_name()?;
        if !self.eat_kw("in") {
            self.expect_op("=")?;
        }
        let range = self.range_expr()?;
        // `for i = 1 to 3 do ... end for`. `while` and `wait until` have
        // always taken the optional `do`; `for` did not, so the one loop
        // form that reads most like English was the one that would not
        // parse -- and the error pointed at `do` as an unexpected keyword
        // in an expression, which says nothing about the loop.
        let _ = self.eat_kw("do");
        let body = self.block(&["end", "else"])?;
        // `for … else`: the arm that runs when the loop was NOT broken out
        // of. `else` is already a keyword, so this costs no new syntax.
        let else_ = if self.eat_kw("else") {
            self.block(&["end"])?
        } else {
            Vec::new()
        };
        self.expect_end(&["for"])?;
        // `end for i` — optional, and CHECKED against the loop it closes.
        //
        // The reason to allow it is a stack of `end for` / `end for` with
        // nothing to say which is which; the reason to CHECK it is that a
        // label nobody verifies is worse than none, because it is then a
        // comment that can be wrong. A mismatch means the loops are nested
        // differently than they read, which is exactly the mistake the
        // label was written to catch.
        //
        // Only ever an identifier sitting on the same line: after a
        // newline the next `Ident` is the following statement.
        if let Tok::Ident(name) = self.peek().clone() {
            if name == var {
                self.bump();
            } else {
                return self.err(format!(
                    "`end for {name}` closes a loop over `{var}` -- either the name is wrong \
                     or these loops are nested differently than they look"
                ));
            }
        }
        Ok(Stmt::For { var, range, body, else_ })
    }

    /// `select case <subject>` / `case <v>[, <v>…]` / `case else` /
    /// `end select`.
    fn select_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // select
        self.bump(); // case
        let subject = self.expression()?;
        let mut arms: Vec<(Vec<Expr>, Vec<Stmt>)> = Vec::new();
        let mut else_: Vec<Stmt> = Vec::new();
        let mut seen_else = false;
        loop {
            self.skip_terms();
            if matches!(self.peek(), Tok::Eof) {
                return self.err("`select case` reached the end of the file without `end select`");
            }
            if self.at_kw("end") {
                break;
            }
            if !matches!(self.peek(), Tok::Ident(c) if c == "case") {
                return self.err(
                    "inside `select case`, every branch starts with `case` (or `case else`) -- \
                     statements cannot sit directly between the subject and the first `case`",
                );
            }
            self.bump(); // case
            if self.at_kw("else") {
                self.bump();
                // A second `case else` is a mistake worth naming: only one
                // of them could ever run, so the other is dead code the
                // author almost certainly meant to be reachable.
                if seen_else {
                    return self.err("`select case` already has a `case else`");
                }
                seen_else = true;
                else_ = self.block(&["end", "case"])?;
                continue;
            }
            // `case 1, 2, 3` — any of them matches.
            let mut vals = vec![self.expression()?];
            while self.eat_op(",") {
                vals.push(self.expression()?);
            }
            let body = self.block(&["end", "case"])?;
            arms.push((vals, body));
        }
        self.expect_end(&["select"])?;
        Ok(Stmt::Select { subject, arms, else_ })
    }

    /// `do … loop while|until <cond>` and `repeat … until <cond>`.
    ///
    /// Both spellings build the same node; which one the author wrote is
    /// kept only so far as `until` inverting the test.
    fn do_loop_stmt(&mut self) -> PResult<Stmt> {
        let repeat = self.at_kw("repeat");
        self.bump(); // do | repeat
        // `do while <cond>` / `do until <cond>` -- the PRE-test spellings.
        // Accepted and turned into the ordinary pre-test loops rather than
        // silently treated as post-test ones: someone writing `do while`
        // means "test first", and quietly running the body once before the
        // first test would be a wrong answer that looks right.
        if !repeat && (self.at_kw("while") || self.at_kw("until")) {
            let until = self.at_kw("until");
            self.bump();
            let cond = self.expression()?;
            let body = self.block(&["end", "loop"])?;
            if self.at_kw("end") {
                self.expect_end(&["do", "while", "until", "loop"])?;
            } else {
                self.bump(); // loop
            }
            let cond = if until {
                Expr::Unary { op: "not".into(), rhs: Box::new(cond) }
            } else {
                cond
            };
            return Ok(Stmt::While { cond, body });
        }
        // `while` belongs in the stop list as much as `until` does: the
        // post-test form closes with EITHER, and leaving it out ran the
        // body straight past `while C` to the end of the file.
        let body = self.block(&["loop", "until", "while", "end"])?;
        // `do … until C` / `do … while C` close with the bare keyword;
        // `repeat … until C` likewise; `do … loop while|until C` closes
        // with `loop` first.
        let until = if self.at_kw("until") {
            self.bump();
            true
        } else if self.at_kw("while") {
            self.bump();
            false
        } else if matches!(self.peek(), Tok::Ident(w) if w == "loop") {
            self.bump();
            if self.at_kw("until") {
                self.bump();
                true
            } else if self.at_kw("while") {
                self.bump();
                false
            } else {
                return self.err("`do … loop` needs `while` or `until` and a condition");
            }
        } else {
            return self.err(if repeat {
                "`repeat` needs a closing `until <condition>`"
            } else {
                "`do` needs a closing `loop while <condition>` or `loop until <condition>`"
            });
        };
        let cond = self.expression()?;
        Ok(Stmt::DoLoop { body, cond, until })
    }

    fn while_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // while
        let cond = self.expression()?;
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&["while"])?;
        Ok(Stmt::While { cond, body })
    }

    /// `wait until COND [do] ... end [until]` — see the call site's doc
    /// comment in `statement()` for why this desugars directly to
    /// `Stmt::While { cond: not(COND), body }` instead of a dedicated AST
    /// node. Negating the already-parsed condition tree (`Expr::Unary { op:
    /// "not", .. }`) rather than re-lexing `"not (" + text + ")"` sidesteps
    /// any precedence question entirely — it wraps the whole parsed
    /// expression regardless of what operators are inside it.
    fn wait_until_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // wait
        self.bump(); // until
        let cond = self.expression()?;
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&["until"])?;
        Ok(Stmt::While {
            cond: Expr::Unary {
                op: "not".to_string(),
                rhs: Box::new(cond),
            },
            body,
        })
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // if
        let cond = self.expression()?;
        let _ = self.eat_kw("then");
        let then = self.block(&["else", "elseif", "end"])?;
        let mut else_ = Vec::new();
        if self.at_elseif_like() {
            // desugar `elseif`/`else if` into a nested if in the else
            // branch — the nested `elseif_tail` calls never consume `end`
            // themselves (see that function's own comment), so this outer
            // call is the one that must eat the chain's shared `end if`.
            else_ = vec![self.elseif_tail()?];
        } else if self.eat_kw("else") {
            else_ = self.block(&["end"])?;
        }
        self.expect_end(&["if"])?;
        Ok(Stmt::If { cond, then, else_ })
    }

    /// True at the start of an `elseif`/`else if` chain link — either the
    /// single-token `elseif` keyword, or `else` immediately followed by
    /// `if` with nothing between them (no intervening `Tok::Newline`,
    /// since `else` is not in the lexer's `suppress_newline_after` set —
    /// see `qu-lexer`'s `suppress_newline_after`, confirmed there's no
    /// special-casing needed: `else` then `if` on the SAME line lexes as
    /// two adjacent tokens with no `Newline` between them, while `else`
    /// then `if` on a LATER line always has a real `Tok::Newline` in
    /// between, so this token-adjacency check alone is unambiguous). Ahmed
    /// confirmed (2026-08-31) he wants the two spellings fully
    /// interchangeable. Checked BEFORE the ordinary `else` handling so a
    /// genuine `else` followed later by an unrelated, separately-closed
    /// nested `if` (on its own line) is completely unaffected — it falls
    /// through to the plain `self.block(&["end"])` arm exactly as before,
    /// and that nested `if` parses (and closes) itself as an ordinary
    /// statement, needing its own `end if`.
    fn at_elseif_like(&self) -> bool {
        self.at_kw("elseif") || (self.at_kw("else") && matches!(self.peek_at(1), Tok::Keyword("if")))
    }

    /// Consumes whichever spelling `at_elseif_like` matched (1 token for
    /// `elseif`, 2 for `else`+`if`) and parses the rest of the chain
    /// link, sharing every bit of logic between the two spellings from
    /// here on (single code path, per the "don't duplicate this" design
    /// goal) — including which call in the chain consumes the shared
    /// closing `end`/`end if`: never this function itself, always the
    /// outermost `if_stmt`, exactly like the single-keyword `elseif` this
    /// replaces (formerly `if_stmt_from_elseif`) already worked.
    fn elseif_tail(&mut self) -> PResult<Stmt> {
        if self.at_kw("elseif") {
            self.bump(); // elseif
        } else {
            self.bump(); // else
            self.bump(); // if
        }
        let cond = self.expression()?;
        let _ = self.eat_kw("then");
        let then = self.block(&["else", "elseif", "end"])?;
        let mut else_ = Vec::new();
        if self.at_elseif_like() {
            else_ = vec![self.elseif_tail()?];
            return Ok(Stmt::If { cond, then, else_ });
        }
        if self.eat_kw("else") {
            else_ = self.block(&["end"])?;
        }
        // the outermost `if_stmt` consumes the chain's shared `end if`
        Ok(Stmt::If { cond, then, else_ })
    }

    // `try` closes with bare `end` (no `end try`) — the one deliberate
    // exception to §16's `end <introducer>` rule, matching MATLAB.
    fn try_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // try
        let body = self.block(&["catch", "finally", "else", "end"])?;
        let mut catch_var = None;
        let mut has_catch = false;
        let mut handler = Vec::new();
        if self.eat_kw("catch") {
            has_catch = true;
            // optional bound name on the same line: `catch err`
            if !matches!(self.peek(), Tok::Newline | Tok::Eof) && !self.at_op(";") {
                catch_var = Some(self.expect_name()?);
            }
            handler = self.block(&["end", "finally", "else"])?;
        }
        // `else` after the handler: the no-exception arm.
        let else_ = if self.eat_kw("else") {
            self.block(&["end", "finally"])?
        } else {
            Vec::new()
        };
        // `finally` is contextual, not a keyword: it is an ordinary
        // identifier everywhere else, so a variable named `finally` keeps
        // working. Recognised here only where a block terminator is due.
        let finally = if matches!(self.peek(), Tok::Ident(w) if w == "finally") {
            self.bump();
            self.block(&["end"])?
        } else {
            Vec::new()
        };
        // `end` or `end try`, both accepted.
        //
        // §16's rule is `end <introducer>` and `try` was carved out as the
        // one MATLAB-style exception to it. Carving it out made bare `end`
        // work; it did not make `end try` WRONG, and someone who has
        // written `end if` and `end for` all the way down a file will
        // write `end try` without thinking about it. `expect_end` already
        // treats the tail as optional, so accepting both costs one word
        // and removes a papercut that only ever produced a parse error at
        // the line AFTER the mistake.
        self.expect_end(&["try"])?;
        Ok(Stmt::Try { body, catch_var, handler, finally, has_catch, else_ })
    }

    // `unsafe [as name] ... end` — like `try`, closes with bare `end`.
    fn unsafe_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // unsafe
        let mut report_var = None;
        if self.eat_kw("as") {
            report_var = Some(self.expect_name()?);
        }
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::Unsafe { body, report_var })
    }

    // `parallel for Name in RangeExpr ... end parallel` — see
    // `Stmt::ParallelFor`'s doc comment. `in` (not `=`, unlike plain `for`)
    // matches the grammar's own spelling exactly (§46.5); no ambiguity with
    // the unit-conversion `in` (`x in kHz`), which only ever appears
    // mid-expression, never right after a bare loop-variable name.
    fn parallel_for_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // parallel
        self.bump(); // for
        let var = self.expect_name()?;
        if !self.eat_kw("in") {
            return self.err("expected `in` after `parallel for <name>`");
        }
        let range = self.range_expr()?;
        let reductions = self.reduce_clause()?;
        let body = self.block(&["end"])?;
        self.expect_end(&["parallel"])?;
        Ok(Stmt::ParallelFor { var, range, body, reductions })
    }

    // `with reduce(+: total, max: best)` after the range. Optional: absent
    // means the loop behaves exactly as it did before reductions existed.
    //
    // The `with` is REQUIRED and is not decoration. A bare `reduce` after
    // the range cannot work: the lexer turns `<number> <identifier>` into
    // a unit literal (`5 kHz`), so `1 to 100 reduce(...)` arrives as
    // `UnitLit(100, "reduce")` and the clause is unreachable -- the first
    // draft of this failed exactly that way, with "unknown unit `reduce`".
    // `with` is a reserved keyword, so it cannot be absorbed into a unit,
    // and it matches the language's existing `pool p with cpu = 1` shape.
    //
    // `reduce` itself stays CONTEXTUAL: `with` is only consumed when
    // `reduce` follows it, so any other `with` form is left alone.
    fn reduce_clause(&mut self) -> PResult<Vec<Reduction>> {
        let is_clause = self.at_kw("with")
            && matches!(self.peek_at(1), Tok::Ident(w) if w == "reduce");
        if !is_clause {
            return Ok(Vec::new());
        }
        self.bump(); // with
        self.bump(); // reduce
        if !self.eat_op("(") {
            return self.err("expected `(` after `reduce`, as in `with reduce(+: total)`");
        }
        let mut out = Vec::new();
        loop {
            // `+`/`*` lex as operators and `min`/`max` as identifiers, so
            // take either shape here and let `from_symbol` be the single
            // place that decides which spellings are real.
            let sym = match self.peek().clone() {
                Tok::Op(o) => {
                    self.bump();
                    o.to_string()
                }
                Tok::Ident(w) => {
                    self.bump();
                    w
                }
                other => {
                    return self.err(format!(
                        "expected a reduction operator (`+`, `*`, `min`, `max`), found {other:?}"
                    ))
                }
            };
            let Some(op) = ReduceOp::from_symbol(&sym) else {
                return self.err(format!(
                    "unsupported reduction operator `{sym}` -- `parallel for` reduces with \
                     `+`, `*`, `min` or `max`"
                ));
            };
            if !self.eat_op(":") {
                return self
                    .err("expected `:` after the reduction operator, as in `reduce(+: total)`");
            }
            let var = self.expect_name()?;
            out.push(Reduction { op, var });
            if self.eat_op(",") {
                continue;
            }
            break;
        }
        if !self.eat_op(")") {
            return self.err("expected `)` to close `reduce(`");
        }
        Ok(out)
    }

    // `pool <name> with cpu = <N>[, gpu = <N>]
    //      policy = round_robin        # or priority | fair | affinity
    //      on full = queue             # or reject | spill_to(cpu)
    //  end pool` — see `Stmt::Pool`'s doc comment. The `with cpu = ...`
    // resource list is comma-separated on the header line; `policy`/`on
    // full` are optional config lines in the body, one per line, in any
    // order, defaulting to `round_robin`/`queue` if omitted.
    fn pool_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // pool
        let name = self.expect_name()?;
        if !self.eat_kw("with") {
            return self.err("expected `with` after `pool <name>`");
        }
        let mut cpu: Option<Expr> = None;
        let mut gpu: Option<Expr> = None;
        let mut remote: Option<Expr> = None;
        loop {
            let key = self.expect_name()?;
            self.expect_op("=")?;
            let val = self.expression()?;
            match key.as_str() {
                "cpu" => cpu = Some(val),
                "gpu" => gpu = Some(val),
                "remote" => remote = Some(val),
                other => {
                    return self.err(format!(
                        "pool: unknown resource `{other}` (expected `cpu`, `gpu`, or `remote`)"
                    ))
                }
            }
            if self.eat_op(",") {
                continue;
            }
            break;
        }
        // `cpu = <N>` is required UNLESS `remote = (...)` names at least one
        // remote worker (a pool of purely remote workers, no local cpu, is
        // a real and explicitly-supported shape — see `PoolSpec::build`'s
        // doc comment for the runtime-side validation, which is where "at
        // least one of cpu/remote" is actually enforced since `remote`
        // hasn't been evaluated yet here).
        let cpu = match cpu {
            Some(cpu) => cpu,
            None if remote.is_some() => Expr::Float(0.0),
            None => return self.err("pool: `with` needs at least `cpu = <N>` (or `remote = (...)`)"),
        };
        self.expect_stmt_end(&["end"])?;
        self.skip_terms();

        let mut policy = "round_robin".to_string();
        let mut on_full = "queue".to_string();
        loop {
            if self.at_kw("end") {
                break;
            }
            if matches!(self.peek(), Tok::Eof) {
                return self.err("expected `end` to close `pool` block");
            }
            let key = self.expect_name()?;
            if key == "policy" {
                self.expect_op("=")?;
                policy = self.expect_name()?;
            } else if key == "on" {
                let key2 = self.expect_name()?;
                if key2 != "full" {
                    return self.err("expected `on full = ...` inside a `pool` block");
                }
                self.expect_op("=")?;
                let word = self.expect_name()?;
                on_full = if word == "spill_to" {
                    self.expect_op("(")?;
                    let target = self.expect_name()?;
                    self.expect_op(")")?;
                    format!("spill_to({target})")
                } else {
                    word
                };
            } else {
                return self.err(format!(
                    "pool: unknown option `{key}` (expected `policy` or `on full`)"
                ));
            }
            self.expect_stmt_end(&["end"])?;
            self.skip_terms();
        }
        self.expect_end(&["pool"])?;
        Ok(Stmt::Pool { name, cpu, gpu, remote, policy, on_full })
    }

    // `every|after|at <ms> [do] ... end` — see `Stmt::Timer`'s doc comment.
    // `do` is eaten-if-present, not required, matching `while`'s convention
    // (§16's block-introducer keywords are consistently optional here).
    fn timer_stmt(&mut self, kind: &str) -> PResult<Stmt> {
        self.bump(); // every/after/at
        let interval = self.expression()?;
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::Timer { kind: kind.to_string(), interval, body })
    }

    fn on_elapsed_stmt(&mut self, once: bool) -> PResult<Stmt> {
        self.bump(); // on
        self.bump(); // elapsed/elapsedOnce
        self.expect_op("(")?;
        let timer = self.expression()?;
        self.expect_op(")")?;
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::OnElapsed { once, timer, body })
    }

    // `watch file(path) [do] ... end` — see `Stmt::Watch`'s doc comment.
    fn watch_file_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // watch
        self.bump(); // file
        let args = self.call_args()?;
        let path = match args.as_slice() {
            [Arg::Pos(e)] => e.clone(),
            [Arg::Named(name, _)] => {
                return self.err(format!("watch file(path): unexpected named argument `{name}=` — expected a plain path"));
            }
            other => {
                return self.err(format!(
                    "watch file(path): expected exactly one argument (the path), got {}",
                    other.len()
                ));
            }
        };
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::Watch { kind: WatchKind::File(path), body })
    }

    // `watch url(url, [interval_seconds=]) [do] ... end` — see
    // `Stmt::Watch`'s doc comment. The interval may be given positionally
    // (`watch url(u, 10)`) or as `interval_seconds=10` — either lands in the
    // same `Option<Expr>` slot.
    fn watch_url_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // watch
        self.bump(); // url
        let args = self.call_args()?;
        if args.is_empty() {
            return self.err("watch url(url, [interval_seconds=]): needs at least the url".to_string());
        }
        let url = match &args[0] {
            Arg::Pos(e) => e.clone(),
            Arg::Named(name, _) => {
                return self.err(format!("watch url(...): the first argument must be a plain url, not `{name}=`"));
            }
        };
        let mut interval = None;
        for a in &args[1..] {
            match a {
                Arg::Pos(e) => interval = Some(e.clone()),
                Arg::Named(name, e) if name == "interval_seconds" => interval = Some(e.clone()),
                Arg::Named(name, _) => {
                    return self.err(format!("watch url(...): unknown argument `{name}=` (expected `interval_seconds=`)"));
                }
            }
        }
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::Watch { kind: WatchKind::Url(url, interval), body })
    }

    // `watch <name> [do] ... end` — bare variable-value watch. See
    // `Stmt::Watch`'s doc comment.
    fn watch_var_stmt(&mut self) -> PResult<Stmt> {
        self.bump(); // watch
        let name = self.expect_name()?;
        let _ = self.eat_kw("do");
        let body = self.block(&["end"])?;
        self.expect_end(&[])?;
        Ok(Stmt::Watch { kind: WatchKind::Var(name), body })
    }

    /// consume `end` and an optional matching tail keyword (e.g. `end for`).
    fn expect_end(&mut self, tails: &[&str]) -> PResult<()> {
        if !self.eat_kw("end") {
            return self.err("expected `end`");
        }
        if let Tok::Keyword(k) = self.peek() {
            if tails.contains(k) {
                self.bump();
            }
        } else if let Tok::Ident(w) = self.peek().clone() {
            if tails.contains(&w.as_str()) {
                self.bump();
            }
        }
        Ok(())
    }

    fn ident_or_kw_word(&mut self, what: &str) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            Tok::Keyword(k) => {
                self.bump();
                Ok(k.to_string())
            }
            _ => self.err(format!("expected {}", what)),
        }
    }

    fn expect_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => self.err("expected an identifier"),
        }
    }

    // ---- expressions ----

    /// assignment RHS may be a comma-ellipsis range (`1, 2, ..., 50`), an
    /// ordinary range (`a to b step c`), or a plain expression.
    fn assign_rhs(&mut self) -> PResult<Expr> {
        // `Expr ',' Expr ',' '...' ',' Expr` — the step is the gap between the
        // first two elements; look ahead past the second element for `, ...`
        // before committing (otherwise this is an ordinary comma, which
        // `assign_rhs` does not itself consume — e.g. matrix/tuple contexts
        // parse their own commas).
        let start = self.range_expr()?;
        if self.at_op(",") {
            let save = self.i;
            self.bump(); // ,
            let second = self.range_expr()?;
            if self.at_op(",") && matches!(self.peek_at(1), Tok::Op("...")) {
                self.bump(); // ,
                self.bump(); // ...
                self.expect_op(",")?;
                let end = self.range_expr()?;
                let step = bin("-", second, start.clone());
                return Ok(Expr::Range {
                    start: Box::new(start),
                    end: Box::new(end),
                    step: Some(Box::new(step)),
                });
            }
            // not a comma-ellipsis range after all: rewind:
            self.i = save;
        }
        Ok(start)
    }

    fn range_expr(&mut self) -> PResult<Expr> {
        let start = self.expression()?;
        if self.eat_kw("to") {
            let end = self.expression()?;
            let mut step = None;
            if self.eat_kw("step") {
                step = Some(Box::new(self.expression()?));
            }
            // optional `skip eps` guard — parse and discard for now
            if self.eat_kw("skip") {
                let _ = self.expression()?;
            }
            return Ok(Expr::Range {
                start: Box::new(start),
                end: Box::new(end),
                step,
            });
        }
        // compact MATLAB-order range literal: `start:step:stop`, e.g.
        // `1:0.1:3`. A bare `:` is otherwise meaningless at this grammar
        // layer (slices only ever appear inside `[...]`, parsed separately
        // by `index_one`), so this is unambiguous and purely additive — it
        // requires exactly two colons (three parts); anything else rewinds
        // and is left for the caller to reject.
        if self.at_op(":") {
            let save = self.i;
            self.bump(); // :
            if let Ok(mid) = self.expression() {
                if self.eat_op(":") {
                    let stop = self.expression()?;
                    return Ok(Expr::Range {
                        start: Box::new(start),
                        end: Box::new(stop),
                        step: Some(Box::new(mid)),
                    });
                }
                // Only one colon: plain `start:stop`, implied step 1 (same
                // default as the bare `to` form above with no `step`
                // keyword) -- e.g. `1:10`.
                return Ok(Expr::Range {
                    start: Box::new(start),
                    end: Box::new(mid),
                    step: None,
                });
            }
            self.i = save;
        }
        Ok(start)
    }

    fn expression(&mut self) -> PResult<Expr> {
        // `x => body`, checked before anything else consumes the name.
        //
        // `=>`, not `->`: `->` is ALREADY the reshape operator, and
        // `v -> (2, 3)` is a reshape of v. A lambda hooked on `->` sits in
        // front of that and eats it -- which is exactly what happened, and
        // what `reshape_operator_matches_reshape_function_output` caught.
        // `=>` was not even lexed, so it costs nothing to claim.
        //
        // Hooked at the TOP of the expression chain so the body extends as
        // far as an expression can: `x => x^2 + 1` is one lambda whose
        // body is the whole sum, not a lambda returning `x^2` with a
        // stray `+ 1` after it.
        if let (Tok::Ident(p), Tok::Op("=>")) = (self.peek().clone(), self.peek_at(1)) {
            self.bump(); // param
            self.bump(); // =>
            let body = self.expression()?;
            // Builds the SAME `Expr::Lambda` the `(x) := expr` and
            // `function(x) ... end function` forms build, rather than a
            // second lambda with its own rules. `=>` is a spelling, not a
            // separate feature: one parameter, no parentheses, for the
            // common case of writing a function exactly where it is
            // passed. It closes over its scope like the other two, so
            // there is one answer to "what does a lambda see", not two
            // that look identical on the page.
            return Ok(Expr::Lambda {
                params: vec![Param { name: p, ty: None, default: None }],
                body: FnBody::Expr(Box::new(body)),
            });
        }
        // `run <queueExpr> on <poolExpr>` (§47.3) — see `Expr::Run`'s doc
        // comment for why this is hooked here (top of the expression
        // chain) rather than in `statement()`. Contextual: try the shape
        // and rewind if it doesn't pan out (no bare `on` identifier where
        // expected), so `run = 5` and a variable/function literally named
        // `run` used any other way still parse normally — the same
        // try-then-rewind technique `range_expr`'s compact `a:b:c` colon
        // form and `assign_rhs`'s comma-ellipsis lookahead already use.
        if let Tok::Ident(w) = self.peek().clone() {
            if w == "run"
                && !matches!(
                    self.peek_at(1),
                    Tok::Op("=" | ":=" | "+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^=")
                )
            {
                let save = self.i;
                self.bump(); // run
                if let Ok(queue) = self.ternary() {
                    if let Tok::Ident(on) = self.peek().clone() {
                        if on == "on" {
                            self.bump(); // on
                            if let Ok(pool) = self.ternary() {
                                return Ok(Expr::Run {
                                    queue: Box::new(queue),
                                    pool: Box::new(pool),
                                });
                            }
                        }
                    }
                }
                self.i = save;
            }
        }
        self.ternary()
    }

    // TernaryExpr ::= Coalesce ('?' TernaryExpr ':' TernaryExpr)?
    fn ternary(&mut self) -> PResult<Expr> {
        let cond = self.coalesce()?;
        if self.eat_op("?") {
            let then = self.ternary()?;
            self.expect_op(":")?;
            let else_ = self.ternary()?;
            return Ok(Expr::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                else_: Box::new(else_),
            });
        }
        Ok(cond)
    }

    // CoalesceExpr ::= AsExpr ('??' AsExpr)*  (left-associative is fine here:
    // `a ?? b ?? c` reads the same either way once `a` is non-none)
    fn coalesce(&mut self) -> PResult<Expr> {
        let mut e = self.as_expr()?;
        while self.eat_op("??") {
            let rhs = self.as_expr()?;
            e = Expr::Coalesce { lhs: Box::new(e), rhs: Box::new(rhs) };
        }
        Ok(e)
    }

    // AsExpr ::= ReshapeExpr ('as' Postfix)?
    fn as_expr(&mut self) -> PResult<Expr> {
        let mut e = self.reshape_expr()?;
        while self.eat_kw("as") {
            let c = self.postfix()?;
            e = Expr::As {
                value: Box::new(e),
                contract: Box::new(c),
            };
        }
        Ok(e)
    }

    // ReshapeExpr ::= Pipe ('->' '(' Expr ',' Expr ')')*
    //
    // `expr -> (rows, cols)` — sugar for `reshape(expr, rows, cols)`,
    // rewritten directly at parse time into that exact `Call` AST (no new
    // `Expr` variant, no `qu-interp` changes at all), so evaluating the
    // sugar is byte-identical to calling `reshape(...)` directly.
    //
    // Precedence: sits *above* `pipe()` (a full `x |> f` chain fully
    // resolves before `->` ever sees it) but *below* `as`/`??`/the
    // ternary `? :` (those wrap the finished reshape). Concretely this
    // means `->` swallows the **entire** preceding pipe/arithmetic
    // expression as its value operand rather than grabbing just the
    // nearest operand:
    //   * `a + b -> (r, c)`   == `(a + b) -> (r, c)`   (reshapes the sum)
    //   * `x |> f -> (r, c)`  == `(x |> f) -> (r, c)`  (reshapes the piped result)
    //   * `df.price -> (n,1)`== `(df.price) -> (n, 1)` (field access is
    //     already resolved deep inside `pipe()`'s postfix/power layers)
    // This is the same "postfix qualifier over the whole expression built
    // so far" shape as the existing `as` contract operator immediately
    // above it — deliberately, since both are shape/type-transform
    // qualifiers. Left-associative, so a chained
    // `x -> (2,3) -> (3,2)` reshapes twice in sequence.
    //
    // Scoped to exactly the 2-argument `-> (r, c)` form (matching
    // `reshape(expr, r, c)`'s own signature) — no 1-argument `-> n`
    // variant.
    fn reshape_expr(&mut self) -> PResult<Expr> {
        let mut e = self.pipe()?;
        while self.at_op("->") {
            self.bump();
            self.expect_op("(")?;
            let rows = self.range_expr()?;
            self.expect_op(",")?;
            let cols = self.range_expr()?;
            self.expect_op(")")?;
            e = Expr::Call {
                callee: Box::new(Expr::Name("reshape".to_string())),
                args: vec![Arg::Pos(e), Arg::Pos(rows), Arg::Pos(cols)],
            };
        }
        Ok(e)
    }

    fn pipe(&mut self) -> PResult<Expr> {
        let mut e = self.unit_expr()?;
        while self.eat_op("|>") {
            let stage = self.unit_expr()?;
            e = Expr::Pipe {
                value: Box::new(e),
                stage: Box::new(stage),
            };
        }
        Ok(e)
    }

    // UnitExpr ::= Or ['in' UnitName]
    fn unit_expr(&mut self) -> PResult<Expr> {
        let e = self.or_expr()?;
        if self.at_kw("in") {
            // only treat as unit-conversion if followed by a unit name ident
            if let Tok::Ident(u) = self.peek_at(1).clone() {
                if qu_lexer::is_unit(&u) {
                    self.bump(); // in
                    self.bump(); // unit
                    return Ok(Expr::InUnit {
                        value: Box::new(e),
                        unit: u,
                    });
                }
            }
        }
        Ok(e)
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut e = self.and_expr()?;
        while self.at_kw("or") {
            self.bump();
            let r = self.and_expr()?;
            e = bin("or", e, r);
        }
        Ok(e)
    }
    fn and_expr(&mut self) -> PResult<Expr> {
        let mut e = self.eq_expr()?;
        while self.at_kw("and") {
            self.bump();
            let r = self.eq_expr()?;
            e = bin("and", e, r);
        }
        Ok(e)
    }
    fn eq_expr(&mut self) -> PResult<Expr> {
        let mut e = self.cmp_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Op(o @ ("==" | "!=")) => *o,
                _ => break,
            };
            self.bump();
            let r = self.cmp_expr()?;
            e = bin(op, e, r);
        }
        Ok(e)
    }
    fn cmp_expr(&mut self) -> PResult<Expr> {
        let mut e = self.add_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Op(o @ ("<" | "<=" | ">" | ">=")) => *o,
                _ => break,
            };
            self.bump();
            let r = self.add_expr()?;
            e = bin(op, e, r);
        }
        Ok(e)
    }
    fn add_expr(&mut self) -> PResult<Expr> {
        let mut e = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Op(o @ ("+" | "-")) => *o,
                // `|`/`&` are not Qu operators (Qu has no bitwise/short-circuit
                // operator family) — caught here, at parse time, with an
                // actionable message, rather than silently parsing `|` into a
                // `Binary` node that only fails once the interpreter evaluates
                // it (the previous behavior: a confusing runtime-only error
                // deep in `binop`, and `&` didn't even get that far).
                Tok::Op("|") => {
                    return self.err(
                        "`|` is not a Qu operator — use `bitor(a, b)` for bitwise OR, or `or` for logical OR",
                    );
                }
                Tok::Op("&") => {
                    return self.err(
                        "`&` is not a Qu operator — use `bitand(a, b)` for bitwise AND, or `and` for logical AND",
                    );
                }
                _ => break,
            };
            self.bump();
            let r = self.mul_expr()?;
            e = bin(op, e, r);
        }
        Ok(e)
    }
    fn mul_expr(&mut self) -> PResult<Expr> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Op(o @ ("*" | "/" | "\\" | ".*" | "./" | ".\\")) => *o,
                Tok::Keyword("mod") => "mod",
                _ => break,
            };
            self.bump();
            let r = self.unary()?;
            e = bin(op, e, r);
        }
        Ok(e)
    }
    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            Tok::Op(o @ ("+" | "-" | "~")) => Some(o.to_string()),
            Tok::Keyword("not") => Some("not".to_string()),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let rhs = self.unary()?;
            return Ok(Expr::Unary {
                op,
                rhs: Box::new(rhs),
            });
        }
        self.power()
    }
    // PowerExpression ::= Postfix (('^'|'**') Unary)?   (right associative)
    //
    // `^T`/`^H` transpose sugar was deliberately rejected: `x^T` is ambiguous
    // with "raise `x` to the power of a variable named `T`" and silently
    // prefers the transpose reading, shadowing the variable with no warning.
    // `.T`/`.H` (field-style, unambiguous — `.` never means "raise to a
    // power") and `'`/`.'` (postfix, also unambiguous) already cover the same
    // conjugate/plain distinction without that trap; use those instead.
    fn power(&mut self) -> PResult<Expr> {
        let base = self.postfix()?;
        if self.at_op("^") || self.at_op("**") || self.at_op(".^") {
            let op = if self.at_op("**") {
                "**"
            } else if self.at_op(".^") {
                ".^"
            } else {
                "^"
            };
            self.bump();
            let exp = self.unary()?; // right-assoc
            return Ok(bin(op, base, exp));
        }
        Ok(base)
    }

    // `table.load(path)` / `table.write(t, path)` / `timer.start()` /
    // `timer.elapsed()` / `timer.stop()` / `signals.square(...)` /
    // `signals.impulse(...)` / `signals.pwm(...)` / `signals.sawtooth(...)`
    // / `signals.triangle(...)` — a small, closed set of reserved-
    // namespace-dot aliases (§ ergonomic API layer, 2026-08-31; `signals.`
    // added § argument-validation audit, 2026-09-01). `table`/`timer`/
    // `signals` are not real values or a general namespacing mechanism —
    // this recognizes only the exact token sequence `ident '.' ident '('`
    // where the leading ident is literally `table`, `timer`, or `signals`,
    // and rewrites it *syntactically*, at parse time, straight to the
    // `Call` AST of the real underlying builtin (`table.load(x)` becomes
    // exactly the same AST as `read_csv(x)`; `timer.start()` becomes
    // exactly `tic()`; `signals.square(f, fs, n)` becomes exactly
    // `square(f, fs, n)`). No new `Expr` variant, no `qu-interp` changes,
    // and evaluation is therefore byte-identical to calling the underlying
    // builtin directly.
    //
    // Deliberately narrow: only the exact `(namespace, method)` pairs in
    // `namespace_method_alias` are recognized — anything else spelled
    // `table.foo(...)`/`timer.foo(...)`/`signals.foo(...)` is a clear parse
    // error naming the bad method, not a silently-accepted arbitrary
    // `namespace.method(...)` call. And this only fires when the lookahead
    // is *exactly* `ident '.' ident '('`: a bare `table`/`timer`/`signals`
    // used as an ordinary variable (`table = 5; print(table)`; `signals =
    // 5; print(signals)`), the existing `table(...)` constructor call, or
    // `table.someCol` field access on a real `Table` value bound to a
    // variable named `table`, all fall straight through to `primary()`
    // below, completely unaffected.
    fn postfix_primary(&mut self) -> PResult<Expr> {
        if let Tok::Ident(ns) = self.peek().clone() {
            if matches!(ns.as_str(), "table" | "timer" | "signals") {
                if let (Tok::Op("."), Tok::Ident(method)) =
                    (self.peek_at(1).clone(), self.peek_at(2).clone())
                {
                    if matches!(self.peek_at(3), Tok::Op("(")) {
                        let Some(real_name) = namespace_method_alias(&ns, &method) else {
                            return self.err(format!(
                                "`{ns}.{method}(...)` is not a recognized `{ns}` method"
                            ));
                        };
                        self.bump(); // namespace ident
                        self.bump(); // .
                        self.bump(); // method ident
                        let args = self.call_args()?;
                        return Ok(Expr::Call {
                            callee: Box::new(Expr::Name(real_name.to_string())),
                            args,
                        });
                    }
                }
            }
        }
        self.primary()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.postfix_primary()?;
        loop {
            if self.at_op("(") {
                let args = self.call_args()?;
                e = Expr::Call {
                    callee: Box::new(e),
                    args,
                };
            } else if self.at_op("[") {
                let indices = self.index_list()?;
                e = Expr::Index {
                    value: Box::new(e),
                    indices,
                };
            } else if self.at_op(".") {
                self.bump();
                let name = self.ident_or_kw_word("field name")?;
                e = Expr::Field {
                    value: Box::new(e),
                    name,
                };
            } else if self.at_op("'") {
                self.bump();
                e = Expr::Transpose { value: Box::new(e), conjugate: true };
            } else if self.at_op(".'") {
                self.bump();
                e = Expr::Transpose { value: Box::new(e), conjugate: false };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn call_args(&mut self) -> PResult<Vec<Arg>> {
        self.expect_op("(")?;
        // Newlines inside the parentheses are whitespace.
        //
        // A call with eight named arguments -- a `table(...)` of columns, a
        // plot with its styling -- is unreadable on one line and could not
        // be written on several: the parser stopped at the first newline
        // and reported "expected `)`". Every language this one borrows
        // from lets an argument list breathe, and the documentation is
        // full of calls that want to.
        //
        // Only INSIDE the parentheses, and only where a token break is
        // already required (after `(`, after each `,`, before `)`), so a
        // missing comma is still the error it always was rather than two
        // arguments silently becoming one.
        let mut args = Vec::new();
        self.skip_newlines_in_parens();
        if self.at_op(")") {
            self.bump();
            return Ok(args);
        }
        loop {
            // named argument?  (Ident | Keyword) '=' Expr   (but not '==')
            //
            // A bare KEYWORD is accepted here too (2026-08-26, added for
            // Ahmed's `net.fit(..., backend="pytorch")` ask): `backend` is
            // one of this lexer's dozen-ish hard keywords (the top-level
            // `backend auto`/`backend native` STATEMENT — see this parser's
            // own `at_kw("backend")` check elsewhere), so without this a
            // call argument literally could not spell `backend=` at all —
            // `python_exec`'s own `vars=`/`engine=`/`loss=`/`optimizer=`
            // kwargs all happen to be plain idents (never added to
            // `qu_lexer::KEYWORDS`), so this collision was never hit before.
            // Call-argument position is unambiguous: a keyword immediately
            // followed by `=` here is never meaningful as anything OTHER
            // than a named argument (nobody writes `f(if=5)` meaning the
            // `if` keyword), so this is a narrow, purely additive
            // extension — it does not touch `backend auto`-style
            // statement-HEAD parsing (a completely different call site,
            // resolved before an argument list is ever in play) or change
            // what any keyword means in any other position.
            let named_key = match self.peek().clone() {
                Tok::Ident(name) => Some(name),
                Tok::Keyword(name) => Some(name.to_string()),
                _ => None,
            };
            if let Some(name) = named_key {
                if matches!(self.peek_at(1), Tok::Op("=")) {
                    self.bump(); // name
                    self.bump(); // =
                    let v = self.range_expr()?; // allow ranges here too (e.g. `table(x = 0 to 4)`)
                    args.push(Arg::Named(name, v));
                    self.skip_newlines_in_parens();
                    if self.eat_op(",") {
                        self.skip_newlines_in_parens();
                        continue;
                    } else {
                        break;
                    }
                }
            }
            let v = self.range_expr()?; // allow ranges as args (e.g. 0 to 4)
            args.push(Arg::Pos(v));
            self.skip_newlines_in_parens();
            if self.eat_op(",") {
                self.skip_newlines_in_parens();
                continue;
            } else {
                break;
            }
        }
        self.expect_op(")")?;
        Ok(args)
    }

    /// Consume newline tokens, for the positions inside an argument list
    /// where a line break carries no meaning. See `call_args`.
    fn skip_newlines_in_parens(&mut self) {
        while matches!(self.peek(), Tok::Newline) {
            self.bump();
        }
    }

    fn index_list(&mut self) -> PResult<Vec<Idx>> {
        self.expect_op("[")?;
        let mut idx = Vec::new();
        if self.at_op("]") {
            self.bump();
            return Ok(idx);
        }
        loop {
            idx.push(self.index_one()?);
            if self.eat_op(",") {
                continue;
            } else {
                break;
            }
        }
        self.expect_op("]")?;
        Ok(idx)
    }

    fn index_one(&mut self) -> PResult<Idx> {
        // slice forms (MATLAB order, spec §15): `:`  `a:`  `:b`  `a:b`  `a:step:b`
        let lo = if self.at_op(":") {
            None
        } else {
            Some(self.expression()?)
        };
        if self.eat_op(":") {
            let second = if self.at_op("]") || self.at_op(",") || self.at_op(":") {
                None
            } else {
                Some(self.expression()?)
            };
            if self.eat_op(":") && !(self.at_op("]") || self.at_op(",")) {
                // three parts present: `lo : step : hi` — the middle term
                // just parsed as `second` is the step, not the stop.
                let hi = Some(self.expression()?);
                Ok(Idx::Slice { lo, hi, step: second })
            } else {
                // two parts: `lo:hi`, step left at its implicit default.
                Ok(Idx::Slice { lo, hi: second, step: None })
            }
        } else {
            Ok(Idx::Expr(lo.unwrap()))
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                if let Some(u) = self.unit_suffix() {
                    Ok(Expr::Unit(n as f64, u))
                } else {
                    Ok(Expr::Int(n))
                }
            }
            Tok::Float(f) => {
                self.bump();
                if let Some(u) = self.unit_suffix() {
                    Ok(Expr::Unit(f, u))
                } else {
                    Ok(Expr::Float(f))
                }
            }
            Tok::Imag(f) => {
                self.bump();
                Ok(Expr::Imag(f))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Str(s))
            }
            Tok::RawStr(s) => {
                self.bump();
                Ok(Expr::RawStr(s))
            }
            Tok::UnitLit(v, u) => {
                self.bump();
                Ok(Expr::Unit(v, u))
            }
            Tok::Keyword("true") => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Tok::Keyword("false") => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Tok::Keyword("none") => {
                self.bump();
                Ok(Expr::None)
            }
            // `end` as an index keyword (`x[end]`, `x[2:end]`, `x[end-1]`,
            // scoped 2026-08-26): `end` is already a reserved word (closes
            // every block), so it can never reach here as a plain
            // `Tok::Ident` the way `last`/`first`/`middle` do below — this
            // arm is the only way it can parse as an expression at all, and
            // it only becomes reachable where an expression is expected
            // (e.g. inside `[...]`), never where a block-closing `end` is
            // expected (that's consumed by `block()`/`expect_end` before
            // `statement()`/`expression()` ever run). Reuses the plain
            // `Expr::Name("end")` node rather than a dedicated AST variant —
            // `qu-interp`'s index-length-context stack (see
            // `Interp::index_len_stack`) resolves it, `last`, `middle`, and
            // `first` uniformly at eval time. Evaluating it outside an
            // index context (e.g. bare `x = end`) is a clear runtime error,
            // not a parse-time one.
            Tok::Keyword("end") => {
                self.bump();
                Ok(Expr::Name("end".to_string()))
            }
            Tok::Ident(name) => {
                self.bump();
                // `where <cond>` prefix (paren-less): `idx := where x > limit`.
                // With parentheses it is an ordinary call and handled by postfix.
                if name == "where" && !self.at_op("(") && self.starts_expr() {
                    let cond = self.or_expr()?;
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::Name("where".into())),
                        args: vec![Arg::Pos(cond)],
                    });
                }
                Ok(Expr::Name(name))
            }
            // `function(x, y) ... end function` -- the block form with the
            // name left out. Distinguished from a named definition by the
            // `(` where the name would be, so nothing else had to move.
            Tok::Keyword("function") if matches!(self.peek_at(1), Tok::Op("(")) => {
                self.bump();
                let params = self.param_list()?;
                let body = self.block(&["end"])?;
                self.expect_end(&["function"])?;
                Ok(Expr::Lambda { params, body: FnBody::Block(body) })
            }
            // `(x, y) := expr` -- the one-line form with the name left out,
            // exactly as `name(x) := expr` is the named one.
            //
            // Needs the lookahead because `(a, b)` is also a tuple literal
            // and they are the same tokens until the `:=` after the closing
            // paren decides it. `paren_close_offset` is already here for
            // the named form's identical problem.
            Tok::Op("(")
                if self
                    .paren_close_offset(0)
                    .is_some_and(|after| matches!(self.peek_at(after + 1), Tok::Op(":="))) =>
            {
                let params = self.param_list()?;
                self.expect_op(":=")?;
                let body = self.range_expr()?;
                Ok(Expr::Lambda { params, body: FnBody::Expr(Box::new(body)) })
            }
            Tok::Op("(") => {
                self.bump();
                // Use `range_expr()` (not plain `expression()`) so a range
                // literal parses as a general sub-expression when wrapped in
                // parens, e.g. `sin((0 to 63) * 0.3)` — previously only
                // `assign_rhs()`/call args reached `range_expr()`, so a range
                // nested inside an *extra* pair of parens (as opposed to
                // being the whole call-arg or assignment RHS itself) failed
                // to parse: `expression()` doesn't know about `to`/compact
                // `a:step:b`, so `to`/a bare `:` was left unconsumed and the
                // following `expect_op(")")` errored. `range_expr()` starts
                // by calling `expression()` and only additionally checks for
                // a trailing `to`/`:` range suffix, so any input that used to
                // parse here (a plain expression with no such suffix) parses
                // identically — this is purely additive.
                // `()` is the empty list. It exists because there has to be
                // a way to write one: the previous idiom was `("",)[0:0]`,
                // an empty slice of a one-element tuple, which stopped
                // being empty when slices became inclusive on both ends.
                // A literal is the right spelling for it anyway -- the old
                // one made the reader work out that a slice from 0 to 0
                // was a declaration of emptiness.
                // Newlines inside the parentheses are whitespace, the same
                // as in an argument list. A list long enough to want its own
                // lines used to parse only if the last element carried a
                // trailing comma: a list whose last element was followed
                // by a newline and then the closing paren failed, while
                // the same list with a comma after that element worked.
                // A difference no one could guess and nothing explained.
                // Skipped only where a token break is already required, so
                // a missing comma is still the error it always was.
                self.skip_newlines_in_parens();
                if self.at_op(")") {
                    self.bump();
                    return Ok(Expr::Tuple(Vec::new()));
                }
                let first = self.range_expr()?;
                self.skip_newlines_in_parens();
                if self.at_op(",") {
                    let mut items = vec![first];
                    while self.eat_op(",") {
                        self.skip_newlines_in_parens();
                        if self.at_op(")") {
                            break;
                        }
                        items.push(self.range_expr()?);
                        self.skip_newlines_in_parens();
                    }
                    self.expect_op(")")?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.expect_op(")")?;
                    Ok(first)
                }
            }
            Tok::Op("[") => self.matrix_literal(),
            Tok::Op("{") => self.record_literal(),
            other => self.err(format!("unexpected {:?} in expression", other)),
        }
    }

    /// Consume an identifier immediately after a number as a unit suffix.
    /// Grammar: `UnitLiteral ::= Number Identifier`.
    ///
    /// ANY identifier, not just a known unit (§ unknown units, 2026-09-09,
    /// Ahmed's call). It used to require a known name, so `5 zzz` left
    /// `zzz` unconsumed and died as `expected end of statement, found
    /// Ident("zzz")` — a PARSE error, which meant one typo'd unit killed
    /// the whole file before anything ran, and `unsafe`/`try` could not
    /// contain it. Every other bad-name mistake in Qu is a runtime error
    /// that names the name; this one was not.
    ///
    /// The interpreter rejects an unknown unit by name instead — see
    /// `apply_unit`, which returns `None` for one. That pairing is
    /// load-bearing: widening the grammar without it would turn a typo
    /// into a silent scale-by-one.
    ///
    /// Safe to widen because `<number> <identifier>` was not grammatical
    /// otherwise: matrix literals require commas (`[1 x]` is still a parse
    /// error), `as`/`in`/`do` are keywords rather than identifiers, and a
    /// command verb's words are consumed before this is reached.
    fn unit_suffix(&mut self) -> Option<String> {
        if let Tok::Ident(u) = self.peek().clone() {
            self.bump();
            return Some(u);
        }
        None
    }

    fn matrix_literal(&mut self) -> PResult<Expr> {
        self.expect_op("[")?;
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        if self.at_op("]") {
            self.bump();
            return Ok(Expr::Matrix(rows));
        }
        let mut row: Vec<Expr> = Vec::new();
        loop {
            row.push(self.expression()?);
            if self.eat_op(",") {
                continue;
            }
            if self.eat_op(";") {
                rows.push(std::mem::take(&mut row));
                continue;
            }
            break;
        }
        rows.push(row);
        self.expect_op("]")?;
        Ok(Expr::Matrix(rows))
    }

    fn record_literal(&mut self) -> PResult<Expr> {
        self.expect_op("{")?;
        let mut fields = Vec::new();
        if self.at_op("}") {
            self.bump();
            return Ok(Expr::Record(fields));
        }
        loop {
            let name = self.expect_name()?;
            self.expect_op("=")?;
            let v = self.expression()?;
            fields.push((name, v));
            if self.eat_op(",") {
                continue;
            } else {
                break;
            }
        }
        self.expect_op("}")?;
        Ok(Expr::Record(fields))
    }
}

fn bin(op: &str, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op: op.to_string(),
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Program {
        /// Every existing "first/second/third statement" assertion in
        /// this module predates `Stmt::SourceLine` (the marker `program()`/
        /// `block()` now interleave before each real statement, for
        /// `try/catch`'s `e.line`) — filtering markers out here keeps
        /// `p.real()[0]` reading as "the first *real* statement" without
        /// hand-recalculating raw indices at each call site (which would
        /// silently break again if the marker density ever changes, e.g.
        /// deduplicating same-line markers).
        fn real(&self) -> Vec<&Stmt> {
            self.stmts.iter().filter(|s| !matches!(s, Stmt::SourceLine(_))).collect()
        }
    }

    fn ok(src: &str) -> Program {
        match parse(src) {
            Ok(p) => p,
            Err(e) => panic!("parse failed for {src:?}: {e}"),
        }
    }

    #[test]
    fn assignment_and_precedence() {
        let p = ok("x = 2 + 3 * 4");
        match &p.real()[0] {
            Stmt::Assign { name, rhs, .. } => {
                assert_eq!(name, "x");
                // top op must be `+`
                match rhs {
                    Expr::Binary { op, .. } => assert_eq!(op, "+"),
                    _ => panic!("expected binary"),
                }
            }
            _ => panic!("expected assign"),
        }
    }

    #[test]
    fn deferred_and_contract() {
        ok("Mt := t as matrix(K, N)");
        ok("f as vector(K, 1)");
    }

    #[test]
    fn range_and_for() {
        ok("t = 0 to (N - 1) * dt step dt");
        ok("for k = 0 to K - 1\n plot(k)\n end for");
    }

    #[test]
    fn call_kwargs_and_index_slice() {
        ok("x = sum(Mx, axis = 0)");
        ok("y = Mx[k, :]");
        ok("z = A[1:10, 2]");
    }

    #[test]
    fn commands_and_backend() {
        let p = ok("backend auto\ngrid on\nshow plot");
        assert!(matches!(p.real()[0], Stmt::Backend(_)));
        assert!(matches!(p.real()[1], Stmt::Command { .. }));
        assert!(matches!(p.real()[2], Stmt::Command { .. }));
    }

    #[test]
    fn command_verb_alone_on_its_line_still_parses_as_a_command_not_a_bare_name() {
        // Regression test for a real bug (found 2026-09-02 via QuStudio's
        // own bundled "Window Functions" DSP template, which uses bare
        // `legend` with no trailing word): `next_is_bare_word()` alone
        // required a following ident/keyword, so a COMMAND_VERB used with
        // ZERO words (`legend` / `colorbar` / `box` / `close` / ... alone
        // on a line) never reached `command_stmt` at all -- it fell through
        // to plain identifier-expression parsing and errored at eval time
        // as "not defined", even though every one of these verbs already
        // has a real, intentional zero-word interpreter-side case (e.g.
        // `legend`'s own `w.is_empty()` arm just turns the legend on).
        for verb in ["legend", "colorbar", "box", "close", "next", "hold", "clear", "axis"] {
            let p = ok(&format!("plot(x, y)\n{verb}"));
            assert!(
                matches!(p.real()[1], Stmt::Command { .. }),
                "bare `{verb}` alone should parse as Stmt::Command, not an expression-statement"
            );
        }
        // Same check with a `;`-terminated statement instead of a newline.
        let p = ok("plot(x, y); legend");
        assert!(matches!(p.real()[1], Stmt::Command { .. }));
    }

    #[test]
    fn bare_print_parses_as_a_call_expression_not_a_word_command() {
        // `print argmax(x)` (no parens) must desugar to a real
        // `print(argmax(x))` call — NOT `Stmt::Command`, which only ever
        // collects literal words and can't represent a function call.
        let p = ok("print argmax(x)");
        match &p.real()[0] {
            Stmt::Expr(Expr::Call { callee, args }) => {
                assert!(matches!(callee.as_ref(), Expr::Name(n) if n == "print"));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Stmt::Expr(Call), got {other:?}"),
        }
        // `print(x)` (parens) must still parse the ordinary way, not
        // double-wrap into `print((x))`.
        let p2 = ok("print(argmax(x))");
        match &p2.real()[0] {
            Stmt::Expr(Expr::Call { args, .. }) => assert_eq!(args.len(), 1),
            other => panic!("expected Stmt::Expr(Call), got {other:?}"),
        }
        // `print = 5` still assigns a variable literally named `print`.
        assert!(matches!(ok("print = 5").real()[0], Stmt::Assign { .. }));
    }

    #[test]
    fn matrix_literal() {
        ok("M = [1, 2; 3, 4]");
    }

    #[test]
    fn method_chain() {
        ok("x.fft().abs().plot()");
    }

    #[test]
    fn power_right_assoc() {
        // 2 ^ 3 ^ 2 == 2 ^ (3 ^ 2); just check it parses
        ok("y = 2 ^ 3 ^ 2");
    }

    #[test]
    fn indexed_and_logical_assignment() {
        let p = ok("x[x < 0] = 0");
        assert!(matches!(p.real()[0], Stmt::IndexAssign { .. }));
        let p2 = ok("x[2] += 10");
        match &p2.real()[0] {
            Stmt::IndexAssign { op, .. } => assert_eq!(op, "+"),
            _ => panic!("expected IndexAssign"),
        }
        // `x[k]` without an assignment stays an ordinary indexing expression
        let p3 = ok("y = x[k]");
        assert!(matches!(p3.real()[0], Stmt::Assign { .. }));
    }

    #[test]
    fn where_prefix_and_call() {
        ok("idx := where x > limit");
        ok("idx := where(x > limit)");
    }

    #[test]
    fn colon_equals_function_definition() {
        // `name(args) := expr` is a function definition, not a deferred bind.
        let p = ok("crest(x) := max(abs(x)) / rms(x)");
        match &p.real()[0] {
            Stmt::DefFn { name, params, .. } => {
                assert_eq!(name, "crest");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert!(params[0].ty.is_none());
            }
            other => panic!("expected DefFn, got {other:?}"),
        }
        // a bare name with `:=` is still a deferred value bind.
        assert!(matches!(ok("Mt := t").real()[0], Stmt::Deferred { .. }));
    }

    #[test]
    fn function_block_and_return() {
        let p = ok("function f(n)\n  if n < 2 then\n    return n\n  end if\n  return f(n-1)\nend function");
        assert!(matches!(p.real()[0], Stmt::Function { .. }));
    }

    #[test]
    fn declare_and_initialize() {
        let p = ok("y as vector(2, 2) = [1, 2, 3, 4]");
        assert!(matches!(p.real()[0], Stmt::Declare { .. }));
        // a contract with no `=` stays a plain Contract statement
        assert!(matches!(ok("f as vector(K, 1)").real()[0], Stmt::Contract { .. }));
    }

    #[test]
    fn legacy_def_fn_alias_still_parses() {
        assert!(matches!(ok("def fn twice(x) = 2 * x").real()[0], Stmt::DefFn { .. }));
    }

    // § multiple dispatch phase 1, 2026-08-27 — optional `: Tag` per
    // parameter (design doc §1). See qu-interp for the dispatch-behavior
    // tests; these only check the grammar itself.
    #[test]
    fn typed_param_parses_on_defn_and_function() {
        let p = ok("area(x: vec) := sum(x)");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert_eq!(params[0].ty, Some(TypeTag::plain("vec")));
            }
            other => panic!("expected DefFn, got {other:?}"),
        }
        let p = ok("function filter(x: signal, f: model)\n  return x\nend function");
        match &p.real()[0] {
            Stmt::Function { params, .. } => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].ty, Some(TypeTag::plain("signal")));
                assert_eq!(params[1].ty, Some(TypeTag::plain("model")));
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn untyped_param_still_parses_to_ty_none() {
        let p = ok("f(x, y) := x + y");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => {
                assert_eq!(params.len(), 2);
                assert!(params[0].ty.is_none());
                assert!(params[1].ty.is_none());
            }
            other => panic!("expected DefFn, got {other:?}"),
        }
    }

    #[test]
    fn any_is_a_synonym_for_untyped() {
        let p = ok("f(x: any) := x");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => assert!(params[0].ty.is_none()),
            other => panic!("expected DefFn, got {other:?}"),
        }
    }

    #[test]
    fn none_tag_is_recognized_despite_being_a_reserved_keyword() {
        // `none` is in `qu_lexer::KEYWORDS` (always `Tok::Keyword`, never
        // `Tok::Ident`) unlike every other tag word, so this exercises the
        // one special case in `param_type_tag`.
        let p = ok("f(x: none) := x");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => assert_eq!(params[0].ty, Some(TypeTag::plain("none"))),
            other => panic!("expected DefFn, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_tag_is_a_parse_error() {
        assert!(parse("f(x: banana) := x").is_err());
    }

    // § multiple dispatch phase 2, 2026-08-27 — `model<"kind">`/`record<Tag>`
    // refinements (design doc §1.1). See qu-interp for the dispatch-behavior
    // tests; these only check the grammar itself.
    #[test]
    fn model_kind_refinement_parses() {
        let p = ok(r#"predict(f: model<"filter">, x) := x"#);
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => {
                assert_eq!(
                    params[0].ty,
                    Some(TypeTag { name: "model".to_string(), refine: Some("filter".to_string()) })
                );
                assert!(params[1].ty.is_none());
            }
            other => panic!("expected DefFn, got {other:?}"),
        }
    }

    #[test]
    fn record_tag_refinement_parses() {
        let p = ok("area(shape: record<Circle>) := shape.radius");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => assert_eq!(
                params[0].ty,
                Some(TypeTag { name: "record".to_string(), refine: Some("Circle".to_string()) })
            ),
            other => panic!("expected DefFn, got {other:?}"),
        }
    }

    #[test]
    fn bare_model_and_refined_model_are_distinct_tags() {
        assert_ne!(TypeTag::plain("model"), TypeTag { name: "model".to_string(), refine: Some("filter".to_string()) });
    }

    #[test]
    fn refinement_on_non_model_non_record_tag_is_a_parse_error() {
        assert!(parse(r#"f(x: vec<"foo">) := x"#).is_err());
    }

    #[test]
    fn model_refinement_requires_string_literal() {
        assert!(parse("f(x: model<filter>) := x").is_err());
    }

    #[test]
    fn comma_ellipsis_range_parses_as_a_range() {
        let p = ok("t = 1, 2, ..., 50");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Range { .. }, .. } => {}
            other => panic!("expected a Range assignment, got {other:?}"),
        }
    }

    #[test]
    fn transpose_operators_parse() {
        ok("A = [1,2,3]'");
        ok("A = [1,2,3].'");
        ok("A = [1,2,3].T");
        ok("A = [1,2,3].H");
        ok("A = M .^ 2");
    }

    #[test]
    fn caret_t_is_power_not_transpose() {
        // `x^T` is "x to the power of the variable T" — `^T`/`^H` transpose
        // sugar was rejected specifically because it used to shadow this.
        let p = ok("y = x^T");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Binary { op, .. }, .. } => assert_eq!(op, "^"),
            other => panic!("expected a `^` power expression, got {other:?}"),
        }
    }

    #[test]
    fn ternary_parses_right_associative() {
        let p = ok("y = a ? 1 : b ? 2 : 3");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Ternary { then, else_, .. }, .. } => {
                assert!(matches!(**then, Expr::Int(1)));
                assert!(matches!(**else_, Expr::Ternary { .. }));
            }
            other => panic!("expected a ternary expression, got {other:?}"),
        }
    }

    #[test]
    fn ternary_as_call_argument() {
        ok("plot(x > 0 ? x : -x)");
    }

    #[test]
    fn compact_range_literal_parses_as_range() {
        let p = ok("x = 1:0.1:3");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Range { start, end, step }, .. } => {
                assert!(matches!(**start, Expr::Int(1)));
                assert!(matches!(**end, Expr::Int(3)));
                assert!(matches!(step, Some(s) if matches!(**s, Expr::Float(_))));
            }
            other => panic!("expected a range expression, got {other:?}"),
        }
    }

    #[test]
    fn compact_range_literal_as_call_argument() {
        ok("plot(1:0.1:3, y)");
    }

    #[test]
    fn null_coalesce_parses_and_chains_left_to_right() {
        let p = ok("y = a ?? b ?? c");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Coalesce { lhs, rhs }, .. } => {
                assert!(matches!(**rhs, Expr::Name(ref n) if n == "c"));
                assert!(matches!(**lhs, Expr::Coalesce { .. }));
            }
            other => panic!("expected a coalesce expression, got {other:?}"),
        }
    }

    #[test]
    fn null_coalesce_and_ternary_compose() {
        ok("y = (a ?? 0) > 0 ? 1 : -1");
    }

    #[test]
    fn unsafe_block_parses_with_optional_report_binding() {
        let p = ok("unsafe as failures\n  x = 1\n  y = 2\nend");
        match &p.real()[0] {
            Stmt::Unsafe { report_var, body } => {
                assert_eq!(report_var.as_deref(), Some("failures"));
                // 2 real statements, each preceded by a `SourceLine` marker.
                assert_eq!(body.len(), 4);
            }
            other => panic!("expected an unsafe statement, got {other:?}"),
        }
        ok("unsafe\n  x = 1\nend"); // report binding is optional
    }

    #[test]
    fn try_catch_closes_with_bare_end_not_end_try() {
        let p = ok("try\n  x = 1 / 0\ncatch err\n  print(err)\nend");
        match &p.real()[0] {
            Stmt::Try { catch_var, handler, .. } => {
                assert_eq!(catch_var.as_deref(), Some("err"));
                // 1 real statement, preceded by its `SourceLine` marker.
                assert_eq!(handler.len(), 2);
            }
            other => panic!("expected a try statement, got {other:?}"),
        }
    }

    #[test]
    fn try_without_catch_still_needs_bare_end() {
        ok("try\n  x = 1\nend");
    }

    #[test]
    fn catch_without_a_bound_name_is_allowed() {
        let p = ok("try\n  x = 1\ncatch\n  x = 2\nend");
        match &p.real()[0] {
            Stmt::Try { catch_var, .. } => assert!(catch_var.is_none()),
            other => panic!("expected a try statement, got {other:?}"),
        }
    }

    #[test]
    fn range_expression_works_as_a_named_call_argument() {
        // named args used to parse via `expression()`, not `range_expr()`,
        // so `table(x = 0 to 4)` couldn't parse at all.
        ok("df = table(x = 0 to 4)");
        ok("df = table(x = 1:0.5:3)");
    }

    #[test]
    fn two_part_colon_range_defaults_step_to_one() {
        // `1:10` (plain start:stop, no middle step) used to rewind all the
        // way back to before the first `:`, leaving it unconsumed and
        // erroring downstream -- only the three-part `1:step:10` form
        // worked. Now a single colon implies step 1, same default as bare
        // `to` with no `step` keyword.
        let p = ok("a = 1:10");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Range { start, end, step }, .. } => {
                assert!(matches!(start.as_ref(), Expr::Int(1)));
                assert!(matches!(end.as_ref(), Expr::Int(10)));
                assert!(step.is_none());
            }
            other => panic!("expected a Range assignment, got {other:?}"),
        }
        // three-part form must still produce an explicit step, not None.
        match &ok("b = 1:0.5:3").real()[0] {
            Stmt::Assign { rhs: Expr::Range { step, .. }, .. } => assert!(step.is_some()),
            other => panic!("expected a Range assignment, got {other:?}"),
        }
        // slice indexing (a completely different `Idx::Slice` parse path)
        // must be unaffected.
        ok("y = A[0:10]");
    }

    #[test]
    fn three_part_slice_uses_matlab_start_step_stop_order() {
        // `A[0:2:10]` must mean start=0, step=2, stop=10 (spec §15's own
        // worked example), not Python's start:stop:step.
        let p = ok("y = A[0:2:10]");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Index { indices, .. }, .. } => match &indices[0] {
                Idx::Slice { lo, hi, step } => {
                    assert!(matches!(lo, Some(Expr::Int(0))));
                    assert!(matches!(step, Some(Expr::Int(2))));
                    assert!(matches!(hi, Some(Expr::Int(10))));
                }
                other => panic!("expected a slice index, got {other:?}"),
            },
            other => panic!("expected an index expression, got {other:?}"),
        }
    }

    // ---- enum (§ enum, 2026-08-24) ----

    #[test]
    fn enum_block_one_variant_per_line() {
        let p = ok("enum Season\n  Spring\n  Summer\n  Fall\n  Winter\nend");
        match &p.real()[0] {
            Stmt::EnumDef { name, variants } => {
                assert_eq!(name, "Season");
                assert_eq!(variants, &["Spring", "Summer", "Fall", "Winter"]);
            }
            other => panic!("expected EnumDef, got {other:?}"),
        }
    }

    #[test]
    fn enum_block_comma_separated_and_end_enum() {
        // comma-separated variants on one line, and the optional `end enum`
        // tail spelling (mirrors `end function`/`end for`).
        let p = ok("enum Color\n  Red, Green, Blue\nend enum");
        match &p.real()[0] {
            Stmt::EnumDef { name, variants } => {
                assert_eq!(name, "Color");
                assert_eq!(variants, &["Red", "Green", "Blue"]);
            }
            other => panic!("expected EnumDef, got {other:?}"),
        }
    }

    #[test]
    fn enum_is_contextual_not_reserved() {
        // `enum` immediately followed by an assignment operator is still a
        // plain variable named `enum` — only `enum <Name>` triggers the
        // declaration form, matching `type`/`every`/`parallel`/`on`'s own
        // contextual-keyword convention.
        assert!(matches!(ok("enum = 5").real()[0], Stmt::Assign { .. }));
    }

    // ---- `lazy name = expr` (§ lazy variables, 2026-08-31) ----

    #[test]
    fn lazy_assign_parses_name_and_rhs() {
        let p = ok("lazy x = 2 + 3");
        match &p.real()[0] {
            Stmt::LazyAssign { name, rhs } => {
                assert_eq!(name, "x");
                match rhs {
                    Expr::Binary { op, .. } => assert_eq!(op, "+"),
                    other => panic!("expected a binary rhs, got {other:?}"),
                }
            }
            other => panic!("expected LazyAssign, got {other:?}"),
        }
    }

    #[test]
    fn lazy_is_contextual_not_reserved() {
        // `lazy` immediately followed by `=` (not `<name> =`) is still a
        // plain variable named `lazy` — only the exact `lazy <name> = ...`
        // shape triggers the deferred-binding form, matching `enum`/`type`/
        // `every`/`pool`'s own contextual-keyword convention.
        assert!(matches!(ok("lazy = 5").real()[0], Stmt::Assign { .. }));
    }

    #[test]
    fn lazy_deferred_and_memoize_are_three_distinct_forms() {
        // `lazy x = e`, `x := e`, and `memoize function ...` are three
        // separate features (see `Stmt::LazyAssign`'s own doc comment for
        // why `lazy` doesn't just reuse `:=`) — confirm each keyword/
        // operator produces its own distinct `Stmt` variant, not an
        // accidental alias of one of the others.
        assert!(matches!(ok("lazy x = 1").real()[0], Stmt::LazyAssign { .. }));
        assert!(matches!(ok("x := 1").real()[0], Stmt::Deferred { .. }));
    }

    // ---- `ref name = expr` (§ ref variables, 2026-09-01) ----

    #[test]
    fn ref_assign_parses_name_and_rhs() {
        let p = ok("ref y = x");
        match &p.real()[0] {
            Stmt::RefAssign { name, rhs } => {
                assert_eq!(name, "y");
                assert!(matches!(rhs, Expr::Name(n) if n == "x"));
            }
            other => panic!("expected RefAssign, got {other:?}"),
        }
    }

    #[test]
    fn ref_assign_parses_a_non_identifier_rhs_too() {
        // Deliberately NOT restricted to a bare identifier at the grammar
        // level (see `Stmt::RefAssign`'s own doc comment) — `qu-interp`
        // rejects a non-identifier rhs at execution time with one clear,
        // specific error instead.
        let p = ok("ref y = x + 1");
        match &p.real()[0] {
            Stmt::RefAssign { name, rhs } => {
                assert_eq!(name, "y");
                assert!(matches!(rhs, Expr::Binary { op, .. } if op == "+"));
            }
            other => panic!("expected RefAssign, got {other:?}"),
        }
    }

    #[test]
    fn ref_is_contextual_not_reserved() {
        // `ref` immediately followed by `=` (not `<name> =`) is still a
        // plain variable named `ref` — only the exact `ref <name> = ...`
        // shape triggers aliasing, matching `lazy`/`enum`/`type`'s own
        // contextual-keyword convention.
        assert!(matches!(ok("ref = 5").real()[0], Stmt::Assign { .. }));
    }

    #[test]
    fn enum_rejects_duplicate_variant_names() {
        assert!(parse("enum Season\n  Spring\n  Spring\nend").is_err());
    }

    #[test]
    fn enum_rejects_empty_body() {
        assert!(parse("enum Empty\nend").is_err());
    }

    #[test]
    fn enum_requires_end_to_close() {
        assert!(parse("enum Season\n  Spring").is_err());
    }

    // ---- `@expr` self-assign prefix operator (§ self-assign chain, 2026-08-26) ----

    #[test]
    fn self_assign_desugars_to_plain_assign_one_level() {
        // `@data.drop("target")` == `data = data.drop("target")`.
        let p = ok(r#"@data.drop("target")"#);
        match &p.real()[0] {
            Stmt::Assign { name, op, rhs, in_place } => {
                assert_eq!(name, "data");
                assert_eq!(op, ""); // plain assign, not a compound op
                assert!(*in_place, "`@`-triggered assign must set in_place");
                match rhs {
                    Expr::Call { callee, args } => {
                        match callee.as_ref() {
                            Expr::Field { value, name } => {
                                assert!(matches!(value.as_ref(), Expr::Name(n) if n == "data"));
                                assert_eq!(name, "drop");
                            }
                            other => panic!("expected Field callee, got {other:?}"),
                        }
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("expected Call rhs, got {other:?}"),
                }
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn self_assign_root_identifier_survives_deeper_chains() {
        // `@data.drop("target").corrmat()` still roots at `data`, two
        // method calls deep.
        let p = ok(r#"@data.drop("target").corrmat()"#);
        match &p.real()[0] {
            Stmt::Assign { name, .. } => assert_eq!(name, "data"),
            other => panic!("expected Assign, got {other:?}"),
        }

        // Index + field mixed in: `@table[0].reset()` roots at `table`.
        let p2 = ok("@table[0].reset()");
        match &p2.real()[0] {
            Stmt::Assign { name, .. } => assert_eq!(name, "table"),
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn self_assign_on_vec_append_mutates_via_plain_assign() {
        // simpler chainable-method case per the task's fallback guidance
        // (`append(collection, value)` returns a new list — see
        // qu-interp's verification test using this same builtin).
        let p = ok("@x.append(1)");
        match &p.real()[0] {
            Stmt::Assign { name, op, .. } => {
                assert_eq!(name, "x");
                assert_eq!(op, "");
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn self_assign_rejects_non_identifier_rooted_expressions() {
        // `@(a + b)` — rooted in a `Binary`, not a variable at all.
        assert!(parse("@(a + b)").is_err());
        // `@foo()` — a bare function call has no receiver/base variable;
        // `foo` here names the function being invoked, not a base object
        // being chained from (contrast `@data.drop(x)`, whose callee base
        // IS a variable). See `root_ident`'s doc comment.
        assert!(parse("@foo()").is_err());
        // `@5` — not even a chain, just a literal.
        assert!(parse("@5").is_err());

        // Error message names the problem instead of silently no-op'ing.
        let e = parse("@foo()").unwrap_err();
        assert!(
            e.msg.contains("not rooted in a plain variable"),
            "unexpected message: {}",
            e.msg
        );
    }

    #[test]
    fn self_assign_is_not_a_generalized_assignment_operator() {
        // `@x = x + 1` is NOT valid input: `@` prefixes a bare expression
        // statement, not an assignment. `@x` parses fine on its own
        // (rooted in `x`), so the statement completes there; the trailing
        // `= x + 1` then has nothing left to attach to and is a parse
        // error, same as any stray `=` after a finished statement.
        assert!(parse("@x = x + 1").is_err());
    }

    #[test]
    fn plain_and_compound_assigns_never_set_in_place() {
        // § in-place mutation for `@expr.method()` (2026-09-01): `in_place`
        // must be `true` ONLY for the `@`-triggered desugar above — every
        // other route to `Stmt::Assign` (plain `=`, and every compound
        // `<op>=` spelling) must produce `in_place: false`, so `qu-interp`
        // never attempts an in-place mutation for a statement the script
        // author didn't explicitly mark with `@`.
        match &ok("x = x.sort()").real()[0] {
            Stmt::Assign { in_place, .. } => assert!(!in_place),
            other => panic!("expected Assign, got {other:?}"),
        }
        match &ok("x += 1").real()[0] {
            Stmt::Assign { in_place, .. } => assert!(!in_place),
            other => panic!("expected Assign, got {other:?}"),
        }
        match &ok(r#"x .= drop("y")"#).real()[0] {
            Stmt::Assign { in_place, .. } => assert!(!in_place),
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn a_hard_keyword_can_be_used_as_a_call_argument_name() {
        // Discovered 2026-08-26 while wiring up `net.fit(..., backend=
        // "pytorch")` (Qu's Python-interop work, `qu-interp`): `backend` is
        // one of this lexer's dozen-ish hard keywords (the top-level
        // `backend auto`/`backend native` STATEMENT), so `f(backend="x")`
        // used to be a flat parse error ("unexpected Keyword(\"backend\") in
        // expression") — `call_args`'s named-argument check only recognized
        // `Tok::Ident`. Fixed by also accepting a bare `Tok::Keyword`
        // immediately followed by `=` there (see that function's own doc
        // comment) — this is the regression test for that fix. Uses `mod`
        // (an actual `KEYWORDS` entry) rather than `backend` itself so this
        // test doesn't depend on `backend` specifically staying a keyword.
        let p = ok("f(mod=5, x=1)");
        match &p.real()[0] {
            Stmt::Expr(Expr::Call { args, .. }) => {
                assert_eq!(args.len(), 2);
                match &args[0] {
                    Arg::Named(name, Expr::Int(n)) => {
                        assert_eq!(name, "mod");
                        assert_eq!(*n, 5);
                    }
                    other => panic!("expected Arg::Named(\"mod\", 5), got {other:?}"),
                }
                match &args[1] {
                    Arg::Named(name, _) => assert_eq!(name, "x"),
                    other => panic!("expected Arg::Named(\"x\", _), got {other:?}"),
                }
            }
            other => panic!("expected Stmt::Expr(Call), got {other:?}"),
        }
        // The statement-head use of the SAME keyword is completely
        // untouched by this change — `mod` still works as an ordinary
        // infix operator (`a mod b`), unaffected by call-argument parsing.
        ok("y = 7 mod 3");
    }

    // -------- parse_expr (string interpolation) full-consumption check --------
    //
    // Regression tests for the 2026-08-26 interpolation bug: `parse_expr`
    // (the `{...}` interpolation sub-parser) used to accept any expression
    // that was a *prefix* of the input, silently discarding everything
    // after the point where `range_expr()` stopped consuming tokens. So
    // `n % 2 == 0 ? 100 : 200` (invalid: `%` is not a Qu operator) parsed
    // "successfully" as just `n`, with `% 2 == 0 ? 100 : 200` thrown away.

    #[test]
    fn parse_expr_rejects_trailing_garbage_after_a_valid_prefix() {
        // Exact repro: `%` is invalid, but `n` alone is a complete, valid
        // expression, so the old code silently returned `Ok(Name("n"))`.
        let err = parse_expr("n % 2 == 0 ? 100 : 200").unwrap_err();
        assert!(
            err.msg.contains("Unknown('%')"),
            "expected the error to name the actual offending token, got: {}",
            err.msg
        );
    }

    #[test]
    fn parse_expr_error_points_at_the_leftover_token_not_the_start() {
        // The reported span should sit at the garbage token (col 3, right
        // after `n `), not at 1:1 (a generic "couldn't parse" location).
        let err = parse_expr("n % 2").unwrap_err();
        assert_eq!((err.span.line, err.span.col), (1, 3));
    }

    #[test]
    fn parse_expr_still_errors_on_a_wholly_invalid_expression() {
        // Sanity check this isn't the *only* way parse_expr can fail —
        // an expression invalid from the very first token still errors.
        assert!(parse_expr("% 2").is_err());
    }

    #[test]
    fn parse_expr_accepts_a_variety_of_valid_interpolation_expressions() {
        // These mirror real `print("{...}")` usages from the book/examples
        // and must NOT be affected by the full-consumption check.
        for src in [
            "n",
            "n + 1",
            "(n + 1) * 2",
            "n mod 2 == 0 ? 100 : 200",
            "n + (n mod 2 == 0 ? 1 : 2)",
            "s.upper()",
            "0 to n",
            "1:0.1:3",
            "arr[0]",
            "square(6)",
            "a + b",
            "M[0, 1]",
        ] {
            if let Err(e) = parse_expr(src) {
                panic!("expected {src:?} to parse as a full expression, got error: {e}");
            }
        }
    }

    /// Filters out `Stmt::SourceLine` markers from a raw block body, same as
    /// `Program::real()` above but for a `Vec<Stmt>` (e.g. an `if`/`for`/
    /// `function` body) rather than a whole `Program`.
    fn real_stmts(body: &[Stmt]) -> Vec<&Stmt> {
        body.iter().filter(|s| !matches!(s, Stmt::SourceLine(_))).collect()
    }

    // `;`-separated multiple statements per line (see `skip_terms`/
    // `expect_stmt_end` above, which already treat `;` as an ordinary
    // statement terminator alongside newline/EOF/a block's closing
    // keyword). These regression tests exist because a real user hit a
    // *different* real bug (a template using MATLAB-style `[a, b] = f()`
    // multi-return destructuring, which Qu has never supported) and
    // reasonably suspected `;`-chaining itself was broken. It wasn't — but
    // there was no direct test coverage pinning that down, so add it here.
    #[test]
    fn semicolon_chains_three_statements_at_top_level() {
        let p = ok("x = 1; y = 2; z = 3");
        let stmts = p.real();
        assert_eq!(stmts.len(), 3, "expected 3 statements, got {stmts:?}");
        for (stmt, expected_name) in stmts.iter().zip(["x", "y", "z"]) {
            match stmt {
                Stmt::Assign { name, .. } => assert_eq!(name, expected_name),
                other => panic!("expected Stmt::Assign, got {other:?}"),
            }
        }
    }

    #[test]
    fn semicolon_chain_inside_if_body() {
        let p = ok("if x > 0\n a = 1; b = 2; c = 3\nend");
        match &p.real()[0] {
            Stmt::If { then, .. } => {
                let body = real_stmts(then);
                assert_eq!(body.len(), 3, "expected 3 statements in `if` body, got {body:?}");
            }
            other => panic!("expected Stmt::If, got {other:?}"),
        }
    }

    #[test]
    fn elseif_chain_consumes_its_shared_end() {
        // Regression: `if_stmt`'s elseif branch used to `return` before
        // calling its own `expect_end`, leaving the chain's `end`
        // unconsumed — a bare `if/elseif/end` parsed as a dangling `end`
        // and errored one statement later. A trailing real statement here
        // is the actual proof the `end` got eaten, not just that this one
        // `if` parsed without erroring.
        let p = ok("if x > 10\n a = 1\nelseif x > 3\n a = 2\nend\nb = 5");
        let stmts = p.real();
        assert_eq!(stmts.len(), 2, "expected the if-chain plus the trailing assignment, got {stmts:?}");
        assert!(matches!(&stmts[0], Stmt::If { .. }));
        assert!(matches!(&stmts[1], Stmt::Assign { name, .. } if name == "b"));
    }

    #[test]
    fn elseif_chain_with_multiple_branches_and_a_final_else() {
        let p = ok(
            "if x > 10\n a = 1\nelseif x > 5\n a = 2\nelseif x > 0\n a = 3\nelse\n a = 4\nend\nb = 5",
        );
        let stmts = p.real();
        assert_eq!(stmts.len(), 2, "expected the if-chain plus the trailing assignment, got {stmts:?}");
        match &stmts[0] {
            Stmt::If { else_, .. } => {
                let outer_else = real_stmts(else_);
                match &outer_else[0] {
                    Stmt::If { else_, .. } => {
                        let inner_else = real_stmts(else_);
                        match &inner_else[0] {
                            Stmt::If { else_, .. } => {
                                let final_else = real_stmts(else_);
                                assert!(
                                    matches!(&final_else[0], Stmt::Assign { name, .. } if name == "a"),
                                    "expected the final `else` body, got {final_else:?}"
                                );
                            }
                            other => panic!("expected the 2nd elseif desugared as Stmt::If, got {other:?}"),
                        }
                    }
                    other => panic!("expected the 1st elseif desugared as Stmt::If, got {other:?}"),
                }
            }
            other => panic!("expected Stmt::If, got {other:?}"),
        }
    }

    #[test]
    fn semicolon_chain_inside_for_body() {
        let p = ok("for i = 1 to 3\n a = 1; b = 2; c = 3\nend");
        match &p.real()[0] {
            Stmt::For { body, .. } => {
                let body = real_stmts(body);
                assert_eq!(body.len(), 3, "expected 3 statements in `for` body, got {body:?}");
            }
            other => panic!("expected Stmt::For, got {other:?}"),
        }
    }

    #[test]
    fn semicolon_chain_inside_function_body() {
        let p = ok("function foo()\n a = 1; b = 2; return a + b\nend");
        match &p.real()[0] {
            Stmt::Function { body, .. } => {
                let body = real_stmts(body);
                assert_eq!(body.len(), 3, "expected 3 statements in function body, got {body:?}");
            }
            other => panic!("expected Stmt::Function, got {other:?}"),
        }
    }

    #[test]
    fn semicolon_chain_followed_by_trailing_comment() {
        // A `#` comment after the last `;`-separated statement on a line
        // must not be mistaken for trailing garbage that would trip
        // `expect_stmt_end`.
        let p = ok("x = 1; y = 2 # trailing comment\nz = 3");
        let stmts = p.real();
        assert_eq!(stmts.len(), 3, "expected 3 statements, got {stmts:?}");
    }

    // ------------------------------------------------------------ `->` reshape sugar

    /// Pulls a `reshape(...)` call's 3 `Arg::Pos` expressions out of an
    /// `Expr`, panicking with a descriptive message if the shape doesn't
    /// match — every reshape-sugar test below checks the desugared AST
    /// this way rather than re-parsing with `reshape(...)` text, since the
    /// whole point is that `->` produces *exactly* this `Call` node.
    fn as_reshape_call(e: &Expr) -> (&Expr, &Expr, &Expr) {
        match e {
            Expr::Call { callee, args } => {
                assert!(
                    matches!(callee.as_ref(), Expr::Name(n) if n == "reshape"),
                    "expected callee `reshape`, got {callee:?}"
                );
                assert_eq!(args.len(), 3, "expected 3 args, got {args:?}");
                match (&args[0], &args[1], &args[2]) {
                    (Arg::Pos(a), Arg::Pos(b), Arg::Pos(c)) => (a, b, c),
                    other => panic!("expected 3 positional args, got {other:?}"),
                }
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    fn rhs_of(p: &Program) -> &Expr {
        match &p.real()[0] {
            Stmt::Assign { rhs, .. } => rhs,
            Stmt::Expr(e) => e,
            other => panic!("expected Assign or Expr statement, got {other:?}"),
        }
    }

    #[test]
    fn reshape_operator_desugars_to_a_reshape_call() {
        // `expr -> (rows, cols)` must produce *exactly* the same AST as
        // `reshape(expr, rows, cols)` — it's pure syntactic sugar, so
        // there is no new `Expr` variant and no separate evaluation path.
        let p = ok("x = v -> (2, 3)");
        let (value, rows, cols) = as_reshape_call(rhs_of(&p));
        assert!(matches!(value, Expr::Name(n) if n == "v"));
        assert!(matches!(rows, Expr::Int(2)));
        assert!(matches!(cols, Expr::Int(3)));

        // and it really is byte-for-byte the same shape as parsing the
        // explicit call form.
        let p2 = ok("x = reshape(v, 2, 3)");
        match (rhs_of(&p), rhs_of(&p2)) {
            (Expr::Call { callee: c1, args: a1 }, Expr::Call { callee: c2, args: a2 }) => {
                assert!(matches!((c1.as_ref(), c2.as_ref()), (Expr::Name(n1), Expr::Name(n2)) if n1 == n2));
                assert_eq!(a1.len(), a2.len());
            }
            other => panic!("expected two Calls, got {other:?}"),
        }
    }

    #[test]
    fn reshape_operator_binds_around_field_access() {
        // `df.price -> (n, 1)` == `(df.price) -> (n, 1)` — field access
        // (deep inside `pipe()`'s postfix layer) resolves fully before
        // `->` ever sees it.
        let p = ok("x = df.price -> (n, 1)");
        let (value, _, _) = as_reshape_call(rhs_of(&p));
        match value {
            Expr::Field { value, name } => {
                assert!(matches!(value.as_ref(), Expr::Name(n) if n == "df"));
                assert_eq!(name, "price");
            }
            other => panic!("expected Expr::Field, got {other:?}"),
        }
    }

    #[test]
    fn reshape_operator_binds_around_the_whole_arithmetic_expression() {
        // `a + b -> (2, 2)` == `(a + b) -> (2, 2)`, NOT `a + (b -> (2,2))`:
        // reshape swallows the *entire* preceding arithmetic expression as
        // its value operand, since arithmetic (`add_expr`/`mul_expr`/...)
        // sits entirely *below* `->` in the precedence chain.
        let p = ok("x = a + b -> (2, 2)");
        let (value, _, _) = as_reshape_call(rhs_of(&p));
        match value {
            Expr::Binary { op, lhs, rhs } => {
                assert_eq!(op, "+");
                assert!(matches!(lhs.as_ref(), Expr::Name(n) if n == "a"));
                assert!(matches!(rhs.as_ref(), Expr::Name(n) if n == "b"));
            }
            other => panic!("expected `(a + b)` as the reshape target, got {other:?}"),
        }
    }

    #[test]
    fn reshape_operator_binds_around_the_whole_multiplication() {
        // Same point as the `+` test, for `*` — `x^2` power/postfix and
        // `a*b` multiplication both sit below `->`, so `->` never grabs
        // just the last operand of an arithmetic chain.
        let p = ok("x = a * b -> (2, 2)");
        let (value, _, _) = as_reshape_call(rhs_of(&p));
        assert!(matches!(value, Expr::Binary { op, .. } if op == "*"));
    }

    #[test]
    fn reshape_operator_binds_around_the_whole_pipe_chain() {
        // `x |> f -> (2, 3)` == `(x |> f) -> (2, 3)`: reshapes the piped
        // result, not `f` itself (which would be nonsensical — you can't
        // reshape a function). This is the key test that `->` sits *above*
        // `pipe()` in the precedence table, not below it.
        let p = ok("y = x |> f -> (2, 3)");
        let (value, _, _) = as_reshape_call(rhs_of(&p));
        match value {
            Expr::Pipe { value, stage } => {
                assert!(matches!(value.as_ref(), Expr::Name(n) if n == "x"));
                assert!(matches!(stage.as_ref(), Expr::Name(n) if n == "f"));
            }
            other => panic!("expected `(x |> f)` as the reshape target, got {other:?}"),
        }
    }

    #[test]
    fn reshape_operator_is_left_associative_when_chained() {
        // `v -> (2,3) -> (3,2)` == `reshape(reshape(v,2,3), 3,2)`, applied
        // left-to-right.
        let p = ok("x = v -> (2, 3) -> (3, 2)");
        let (outer_value, outer_rows, outer_cols) = as_reshape_call(rhs_of(&p));
        assert!(matches!(outer_rows, Expr::Int(3)));
        assert!(matches!(outer_cols, Expr::Int(2)));
        let (inner_value, inner_rows, inner_cols) = as_reshape_call(outer_value);
        assert!(matches!(inner_value, Expr::Name(n) if n == "v"));
        assert!(matches!(inner_rows, Expr::Int(2)));
        assert!(matches!(inner_cols, Expr::Int(3)));
    }

    #[test]
    fn reshape_operator_binds_tighter_than_as_contract() {
        // `x -> (2,3) as Mat(2,3)` == `(x -> (2,3)) as Mat(2,3)` — `as`
        // sits directly above `->` in the chain (both are shape/type
        // qualifiers; `as` is the outer one).
        ok("y = x -> (2, 3) as vector(6, 1)");
    }

    #[test]
    fn reshape_operator_rejects_a_single_argument_form() {
        // Scoped deliberately to the 2-argument `-> (r, c)` form (matching
        // `reshape(expr, r, c)`'s own signature) — no `-> n` sugar.
        assert!(parse("x = v -> (4)").is_err());
        assert!(parse("x = v -> 4").is_err());
    }

    // ------------------------------------------------------- `table.`/`timer.` sugar

    #[test]
    fn table_dot_load_rewrites_to_read_csv() {
        let p = ok("t = table.load(\"data.csv\")");
        match rhs_of(&p) {
            Expr::Call { callee, args } => {
                assert!(matches!(callee.as_ref(), Expr::Name(n) if n == "read_csv"));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    #[test]
    fn table_dot_write_rewrites_to_write_csv_same_arg_order() {
        // `table.write(t, path)` must match `write_csv(t, path)`'s existing
        // (table, path) argument order exactly — no reordering.
        let p = ok("table.write(t, \"out.csv\")");
        match rhs_of(&p) {
            Expr::Call { callee, args } => {
                assert!(matches!(callee.as_ref(), Expr::Name(n) if n == "write_csv"));
                match args.as_slice() {
                    [Arg::Pos(a), Arg::Pos(b)] => {
                        assert!(matches!(a, Expr::Name(n) if n == "t"));
                        assert!(matches!(b, Expr::Str(s) if s == "out.csv"));
                    }
                    other => panic!("expected 2 positional args, got {other:?}"),
                }
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    #[test]
    fn timer_dot_methods_rewrite_to_tic_and_toc() {
        let starts = ok("timer.start()");
        match &starts.real()[0] {
            Stmt::Expr(Expr::Call { callee, .. }) => {
                assert!(matches!(callee.as_ref(), Expr::Name(n) if n == "tic"))
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
        for (src, expect) in [("x = timer.elapsed()", "toc"), ("x = timer.stop()", "toc")] {
            let p = ok(src);
            match rhs_of(&p) {
                Expr::Call { callee, .. } => {
                    assert!(matches!(callee.as_ref(), Expr::Name(n) if n == expect))
                }
                other => panic!("expected Expr::Call for {src:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn unrecognized_namespace_method_is_a_clear_parse_error() {
        let err = parse("table.frobnicate(1)").unwrap_err();
        assert!(err.msg.contains("table"), "error should name the namespace: {}", err.msg);
        assert!(err.msg.contains("frobnicate"), "error should name the method: {}", err.msg);

        let err2 = parse("timer.frobnicate(1)").unwrap_err();
        assert!(err2.msg.contains("timer"));
        assert!(err2.msg.contains("frobnicate"));
    }

    #[test]
    fn table_and_timer_still_work_as_ordinary_identifiers() {
        // Plain variable use — must not be shadowed or hijacked by the
        // namespace-dot sugar in any way.
        assert!(matches!(ok("table = 5").real()[0], Stmt::Assign { .. }));
        assert!(matches!(ok("timer = 7").real()[0], Stmt::Assign { .. }));
        let p = ok("print(table)");
        match &p.real()[0] {
            Stmt::Expr(Expr::Call { args, .. }) => {
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Arg::Pos(Expr::Name(n)) => assert_eq!(n, "table"),
                    other => panic!("expected Name(\"table\"), got {other:?}"),
                }
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }

        // The existing `table(...)` constructor call (no dot at all) is a
        // completely different token shape and must keep working exactly
        // as before.
        let p2 = ok("t = table(a = 1)");
        match rhs_of(&p2) {
            Expr::Call { callee, args } => {
                assert!(matches!(callee.as_ref(), Expr::Name(n) if n == "table"));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }

        // A field access on a variable literally named `table`/`timer`
        // that ISN'T immediately followed by `(` (so it can't be the
        // namespace-dot call sugar at all) parses as ordinary field
        // access, e.g. reading a real table value's `.nrows` property.
        let p3 = ok("x = table.nrows");
        match rhs_of(&p3) {
            Expr::Field { value, name } => {
                assert!(matches!(value.as_ref(), Expr::Name(n) if n == "table"));
                assert_eq!(name, "nrows");
            }
            other => panic!("expected Expr::Field, got {other:?}"),
        }

        // A DIFFERENT namespace name followed by `.method(...)` is never
        // intercepted — only the exact closed set of `table`/`timer`
        // leading identifiers is, so `mytable.load(x)` stays an ordinary
        // method-call-shaped `Call{callee: Field{...}}`.
        let p4 = ok("y = mytable.load(x)");
        match rhs_of(&p4) {
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::Field { value, name } => {
                    assert!(matches!(value.as_ref(), Expr::Name(n) if n == "mytable"));
                    assert_eq!(name, "load");
                }
                other => panic!("expected Expr::Field callee, got {other:?}"),
            },
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    // ------------------------------------------------------ `signals.` sugar

    #[test]
    fn signals_dot_methods_rewrite_to_the_bare_builtin_calls() {
        // Unlike `table.load`/`timer.start`, every `signals.*` alias maps
        // to a builtin of the EXACT same name — the point is disambiguation
        // ("this is definitely the square-wave generator, not x*x"), not a
        // nicer spelling. So each rewrite must produce `Call{callee:
        // Name(same_name), args}`, byte-identical in shape to calling the
        // bare name directly.
        for (src, name) in [
            ("signals.square(100, 1000, 10)".to_string(), "square"),
            ("signals.impulse(6, index=2)".to_string(), "impulse"),
            ("m = [0.5]\nsignals.pwm(m, 100, 1000)".to_string(), "pwm"),
            ("signals.sawtooth(100, 1000, 10)".to_string(), "sawtooth"),
            ("signals.triangle(100, 1000, 10)".to_string(), "triangle"),
        ] {
            let p = ok(&src);
            match p.real().last().unwrap() {
                Stmt::Expr(Expr::Call { callee, .. }) => {
                    assert!(
                        matches!(callee.as_ref(), Expr::Name(n) if n == name),
                        "signals.{name}(...) should rewrite to a Call to Name({name:?})"
                    );
                }
                other => panic!("expected Expr::Call, got {other:?}"),
            }
        }
    }

    #[test]
    fn unrecognized_signals_method_is_a_clear_parse_error() {
        let err = parse("signals.frobnicate(1)").unwrap_err();
        assert!(err.msg.contains("signals"), "error should name the namespace: {}", err.msg);
        assert!(err.msg.contains("frobnicate"), "error should name the method: {}", err.msg);
    }

    #[test]
    fn signals_still_works_as_an_ordinary_identifier() {
        // Same guarantee `table`/`timer` already have: a bare `signals`
        // variable, unaffected by the dot-sugar since it's only intercepted
        // when immediately followed by `.ident(`.
        assert!(matches!(ok("signals = 5").real()[0], Stmt::Assign { .. }));
        let p = ok("print(signals)");
        match &p.real()[0] {
            Stmt::Expr(Expr::Call { args, .. }) => {
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Arg::Pos(Expr::Name(n)) => assert_eq!(n, "signals"),
                    other => panic!("expected Name(\"signals\"), got {other:?}"),
                }
            }
            other => panic!("expected Expr::Call, got {other:?}"),
        }
    }

    // -------------------------------------------- `.*=` / `./=` / `^=`

    #[test]
    fn elementwise_and_power_compound_assign_tokens_lex_and_parse() {
        for (src, expected_op) in [
            ("v .*= w", ".*"),
            ("v ./= w", "./"),
            ("v ^= w", "^"),
        ] {
            let p = ok(src);
            match &p.real()[0] {
                Stmt::Assign { name, op, .. } => {
                    assert_eq!(name, "v");
                    assert_eq!(op, expected_op, "for source {src:?}");
                }
                other => panic!("expected Stmt::Assign for {src:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn elementwise_and_power_compound_assign_do_not_swallow_the_next_token() {
        // Regression guard for `match_operator`: before it checked a
        // third character, a 3-char op like `.*=` could never have been
        // matched correctly at all (it only ever compared 2 chars), so
        // this pins down that `.*`/`./` alone (no trailing `=`) still
        // lex as the plain elementwise operators, unaffected.
        let p = ok("x = v .* w");
        match rhs_of(&p) {
            Expr::Binary { op, .. } => assert_eq!(op, ".*"),
            other => panic!("expected Expr::Binary, got {other:?}"),
        }
    }

    // -------------------------------------------------- default parameters

    #[test]
    fn param_list_parses_default_values() {
        let p = ok("function f(x, order = 4, cutoff = 1000)\nend function");
        match &p.real()[0] {
            Stmt::Function { params, .. } => {
                assert_eq!(params.len(), 3);
                assert_eq!(params[0].name, "x");
                assert!(params[0].default.is_none());
                assert_eq!(params[1].name, "order");
                assert!(matches!(params[1].default, Some(Expr::Int(4))));
                assert_eq!(params[2].name, "cutoff");
                assert!(matches!(params[2].default, Some(Expr::Int(1000))));
            }
            other => panic!("expected Stmt::Function, got {other:?}"),
        }
    }

    #[test]
    fn param_list_default_composes_with_a_type_tag() {
        let p = ok("function f(x: vec, order = 4)\nend function");
        match &p.real()[0] {
            Stmt::Function { params, .. } => {
                assert!(params[0].ty.is_some());
                assert!(params[0].default.is_none());
                assert!(params[1].ty.is_none());
                assert!(params[1].default.is_some());
            }
            other => panic!("expected Stmt::Function, got {other:?}"),
        }
    }

    #[test]
    fn param_without_default_after_a_defaulted_one_is_a_parse_error() {
        let err = parse("function f(x, order = 4, y)\nend function").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("order"), "got: {msg}");
        assert!(msg.contains("default"), "got: {msg}");
    }

    #[test]
    fn one_line_colon_equals_functions_also_support_defaults() {
        let p = ok("square(x, p = 2) := x ^ p");
        match &p.real()[0] {
            Stmt::DefFn { params, .. } => {
                assert!(params[0].default.is_none());
                assert!(matches!(params[1].default, Some(Expr::Int(2))));
            }
            other => panic!("expected Stmt::DefFn, got {other:?}"),
        }
    }

    // -------------------------------------------------------- `else if`

    #[test]
    fn else_if_two_words_desugars_identically_to_elseif() {
        let a = ok("if x > 10\n a = 1\nelse if x > 3\n a = 2\nend\nb = 5");
        let b = ok("if x > 10\n a = 1\nelseif x > 3\n a = 2\nend\nb = 5");
        // Both must consume the chain's one shared `end` and leave the
        // trailing `b = 5` as a separate, sibling statement -- not
        // swallowed as a nested body, and not left dangling as a parse
        // error (the original `else if` bug: it used to require its own
        // extra `end if`).
        assert_eq!(a.real().len(), 2);
        assert_eq!(b.real().len(), 2);
        for p in [&a, &b] {
            match &p.real()[0] {
                Stmt::If { else_, .. } => {
                    let outer_else = real_stmts(else_);
                    assert_eq!(outer_else.len(), 1);
                    assert!(matches!(&outer_else[0], Stmt::If { .. }));
                }
                other => panic!("expected Stmt::If, got {other:?}"),
            }
        }
    }

    #[test]
    fn else_if_two_words_chains_multiple_branches() {
        let p = ok(
            "if x > 10\n a = 1\nelse if x > 5\n a = 2\nelse if x > 0\n a = 3\nelse\n a = 4\nend\nb = 5",
        );
        assert_eq!(p.real().len(), 2, "expected the if-chain plus the trailing assignment");
        match &p.real()[0] {
            Stmt::If { else_, .. } => {
                let l1 = real_stmts(else_);
                match &l1[0] {
                    Stmt::If { else_, .. } => {
                        let l2 = real_stmts(else_);
                        match &l2[0] {
                            Stmt::If { else_, .. } => {
                                let l3 = real_stmts(else_);
                                assert!(matches!(&l3[0], Stmt::Assign { name, .. } if name == "a"));
                            }
                            other => panic!("expected 2nd else-if desugared as Stmt::If, got {other:?}"),
                        }
                    }
                    other => panic!("expected 1st else-if desugared as Stmt::If, got {other:?}"),
                }
            }
            other => panic!("expected Stmt::If, got {other:?}"),
        }
    }

    #[test]
    fn a_list_literal_may_break_over_lines_without_a_trailing_comma() {
        // It used to parse only WITH one: a last element followed by a
        // newline and then `)` failed, while the same list with a comma
        // after that element worked. Nothing explained the difference,
        // and a documentation example fell straight into it.
        let p = ok("v = (1,
 2,
 3
)");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Tuple(items), .. } => assert_eq!(items.len(), 3),
            other => panic!("expected a 3-element tuple, got {other:?}"),
        }
        // The trailing-comma form still parses, and to the same thing.
        let q = ok("v = (1,
 2,
 3,
)");
        match &q.real()[0] {
            Stmt::Assign { rhs: Expr::Tuple(items), .. } => assert_eq!(items.len(), 3),
            other => panic!("expected a 3-element tuple, got {other:?}"),
        }
        // And a leading break after `(` is fine too.
        let r = ok("v = (
 1,
 2
)");
        match &r.real()[0] {
            Stmt::Assign { rhs: Expr::Tuple(items), .. } => assert_eq!(items.len(), 2),
            other => panic!("expected a 2-element tuple, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_comma_in_a_multiline_list_is_still_an_error() {
        // Skipping newlines must not turn two elements into one silently.
        assert!(parse("v = (1
 2)").is_err());
    }

    #[test]
    fn plain_else_then_newline_then_if_is_not_treated_as_else_if_chain_sugar() {
        // `else` NOT immediately followed by `if` (a real `Tok::Newline`
        // in between, since `else` isn't in `suppress_newline_after`)
        // must fall through to the ordinary `else` handling: the nested
        // `if` here is parsed as its own statement and needs its own
        // separate `end if`, exactly like before this feature existed.
        let p = ok("if x > 100\n a = 1\nelse\n if x > 3\n  a = 2\n end if\nend if");
        match &p.real()[0] {
            Stmt::If { else_, .. } => {
                let outer_else = real_stmts(else_);
                assert_eq!(outer_else.len(), 1);
                assert!(matches!(&outer_else[0], Stmt::If { .. }));
            }
            other => panic!("expected Stmt::If, got {other:?}"),
        }
    }

    // ------------------------------------------------------------ lambdas

    /// Both lambda forms are the existing named-function forms with the
    /// name left out, so neither needed a new token. `->` was unavailable:
    /// it is already reshape (`expr -> (r, c)`).
    #[test]
    fn a_lambda_has_the_same_two_shapes_a_named_function_has() {
        let p = ok("f = (x, y) := x + y");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Lambda { params, body }, .. } => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "x");
                assert!(matches!(body, FnBody::Expr(_)));
            }
            other => panic!("expected an assignment of a lambda, got {other:?}"),
        }
        let p = ok("f = function(x)\n  return x * 3\nend function");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Lambda { params, body }, .. } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(body, FnBody::Block(b) if real_stmts(b).len() == 1));
            }
            other => panic!("expected an assignment of a lambda, got {other:?}"),
        }
    }

    /// `(a, b)` is also a tuple literal, and the two are the same tokens
    /// until the `:=` after the closing paren decides which. A tuple must
    /// still parse as one.
    #[test]
    fn a_parenthesized_list_without_the_walrus_is_still_a_tuple() {
        let p = ok("t = (1, 2)");
        match &p.real()[0] {
            Stmt::Assign { rhs, .. } => assert!(
                !matches!(rhs, Expr::Lambda { .. }),
                "a tuple literal must not parse as a lambda: {rhs:?}"
            ),
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    /// A named one-line definition is unaffected: the lambda arm only
    /// fires when the `(` sits where the NAME would be.
    #[test]
    fn a_named_one_line_function_is_still_a_definition_not_a_lambda() {
        let p = ok("dbl(x) := x * 2");
        assert!(
            matches!(&p.real()[0], Stmt::DefFn { name, .. } if name == "dbl"),
            "got {:?}",
            p.real()[0]
        );
    }

    #[test]
    fn a_lambda_takes_default_parameters_like_any_other_function() {
        let p = ok("f = (x, k = 10) := x + k");
        match &p.real()[0] {
            Stmt::Assign { rhs: Expr::Lambda { params, .. }, .. } => {
                assert!(params[0].default.is_none());
                assert!(params[1].default.is_some());
            }
            other => panic!("expected an assignment of a lambda, got {other:?}"),
        }
    }
}
