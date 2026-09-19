# Qu debug mode — design plan (`qu repl` / `qu kernel`)

Planning pass, 2026-09-19. Read-only exploration against `master` at `a1803d88`, plus `f2405001` on `claude/jupyter-kernel-fixes`. Nothing here is implemented.

## 1. Ground truth (verified, with line numbers)

**The two persistent-session entry points.**

- `cmd_repl` — `engine/crates/qu-cli/src/main.rs:1242`. One `Interp` for the whole session (:1243). `qu repl <file.qu>` pre-runs the file into *that same* interpreter before the first prompt (:1259-1266). Meta-commands are recognised at :1286-1323: a `:`-prefixed line, plus bare `help`/`?`/`quit`/`exit`; currently `:vars`/`:whos`, `:clear`, `:cancel`/`:abort`, `:help`, `:quit`. Crucially, meta-commands are matched **before** the continuation buffer (`buf`) is appended to, so they work even inside a half-typed block — a debug command typed at a paused prompt will slot into exactly this dispatch. Completion test is `qu_interp::input_looks_complete` (moved out of the CLI at :1197-1202 precisely so the kernel can share it). Execution goes through `it.run_repl_line(&src)` (:1338).
- `cmd_kernel` — `main.rs:1408`. Line-delimited JSON, one object per line, flushed per response (`write_kernel_response`, :1482). Ops today: `run`, `restart`, `vars` (:1434-1463); unknown op → `{"op":"error",...}` and the process survives (:1464-1474). Execution goes through `it.run(code)` (:1438).
- `qu-jupyter` (`engine/crates/qu-jupyter/src/kernel.rs`) is a third consumer of the same `Interp`, with a control channel running as its own tokio task (kernel.rs:~70-130) and cell execution on a blocking thread (`handle_execute`, :324).

**The shared funnel.** Both paths bottom out in `Interp::run` (`qu-interp/src/lib.rs:6485`) → `exec_block` (:6590) → `exec` (:7457). `run_repl_line` (:6547) walks `prog.stmts` itself rather than calling `exec_block`, but calls the same `exec` per statement (:6571, :6578, :6581).

**The interrupt precedent — confirmed per-statement, not per-block.** `f2405001` adds `pub interrupt: Option<Arc<AtomicBool>>` to `Interp` and polls it at the **top of the `for s in stmts` loop inside `exec_block`**, i.e. once before every statement, returning `e("interrupted")`. It is `None` by default, mirroring the existing `on_print: Option<Box<dyn FnMut(&str) + Send>>` hook (lib.rs:3939 region). This is exactly the granularity a breakpoint needs, and exactly the opt-in shape debug state should copy.

**Source positions already exist.** `Stmt::SourceLine(u32)` — `qu-syntax/src/lib.rs:759`, emitted by the parser before every statement at top level and in block bodies (`stmts.push(Stmt::SourceLine(self.span().line))` at :1166 and :1200). The interpreter consumes it at `lib.rs:7469`: it sets `self.current_line` (field at :4052) and does nothing else; `try`/`catch` reads it for `e.line`. **So Qu already carries per-statement line numbers through execution.** No AST change is needed for breakpoints or a step cursor — only column info is missing (line granularity is all MATLAB/pdb-style debugging needs). Helpers that filter markers out exist at `qu-syntax/src/lib.rs:3554` and :4355.

**Call structure for step-into/step-out.** User functions run through `call_block_fn` (`lib.rs:13462`) and `call_user` (:13583), both of which push exactly one frame via `push_call_frame` (:13610, depth cap 1000) and pop via `pop_call_frame` (:13666). `self.frames.len()` **is** the call depth (stated explicitly at :13596-13598), and `call_stack: Vec<String>` already holds the function-name stack. Step over/into/out therefore reduce to comparisons of `frames.len()` against a recorded depth — no new bookkeeping.

**Interpreter state shape (matters for rewind).** `Interp` (:3781) is a large struct holding `env: HashMap<String, Value>` (:3783), `frames` (:3796), `frame_refs` (:3802), `frame_globals` (:3807), `global_refs`, `rng: Rng` (:4010, a single `u64` splitmix state, :3378), `declared`, `current_line`, plus genuinely un-snapshottable things: `workers`, `pool_registry`, `gui`, `llm_models`, live `Value::File`/`Serial`/`TcpConn`/`Fifo` handles (all `Arc<StdMutex<…>>`, lib.rs:657/716/839/851), and a `StdMutex`/`Condvar`. `Interp` is **not** `Clone` and cannot be made so. `Value` **is** `Clone` (:404) and is `Arc`-backed for every bulk type (`Vec`/`Mat`/`CVec`/`CMat`, :417-428), so cloning the variable environment is refcount bumps, not data copies. This is the single fact that makes state snapshots affordable.

**A real hazard: the fast paths bypass the funnel entirely.**
- `run_fast_for` (`lib.rs:7180`) runs the *whole* loop by calling `run_fast_stmts` (:5355) directly — no `exec`, no `exec_block`, no `SourceLine`. Same for `run_fast_while` (:7224). Dispatch at :7807 via `try_fast_for_plan` (:7087).
- Soft-compiled functions (`compiled_funcs`, :3892; `compile_fast_stmts`, :5398) **discard `SourceLine` markers outright** (:5414, with a comment saying the compiled body "never needs a current line of its own").

Consequence: a breakpoint on a line inside a numeric `for` loop or a soft-compiled function would **silently never fire**. That is precisely the silent-divergence failure mode the README's *Design commitments* section ("Silence is the worst failure", README.md:186) says Qu refuses. **Debug mode must disable both optimizers while active** (see §6). This also casts doubt on `f2405001`'s parenthetical claim that the fast path needs no interrupt check — flagged separately as follow-up work, out of scope here.

## 2. What is being asked for

Ahmed: "reconsider adding a debug mode — step up, step down, change the execution cursor (rewind or skip)". Concretely: breakpoints, step over/into/out, inspect state while paused, and move the cursor backward or forward without running what it passes over.

The first three are well-trodden. The cursor movement is the part that needs a real decision, because it collides with Qu's stated identity.

## 3. The pause mechanism (prerequisite for everything else)

Qu's evaluator is a synchronous recursive tree-walker. "Pause" means "stop in the middle of a deep Rust call stack and take commands without unwinding". Two shapes:

**(i) Re-entrant callback (recommended).** Add to `Interp`, in the same opt-in style as `on_print`/`interrupt`:

```rust
pub debug: Option<DebugState>,
```

where `DebugState` carries the breakpoint set, the step mode, and a host callback:

```rust
pub on_pause: Box<dyn FnMut(&DebugEvent) -> DebugCommand + Send>,
```

At the pause point, `exec_block` calls `on_pause` and **blocks inside it** until the host returns a command. The whole Rust stack stays alive, so locals, frames and the resume point cost nothing to preserve.
- `qu repl`: the callback reads stdin inline and prints at a `dbg>` prompt. Trivial.
- `qu kernel`: the callback sends a `{"op":"paused",…}` line on stdout and blocks on a channel fed by the stdin reader thread — which means `cmd_kernel`'s stdin loop must move to its own thread with the interpreter on another. That is the one structural change the kernel needs, and `qu-jupyter` already demonstrates the pattern (execution on `spawn_blocking`, control on its own task).

**(ii) Yield-based / state-machine interpreter.** Turn the evaluator into something resumable. This is a rewrite of a 77k-line tree-walker. **Rejected.**

State inspection while paused is free: `global_bindings()` (:6512) plus `frames.last()` plus `call_stack` give variables and a backtrace with no new machinery. Evaluating an expression at the pause point (`dbg> x(3)`) reuses `run_repl_line` against the live `Interp` — the frames are still pushed, so it naturally evaluates in the paused scope.

## 4. Breakpoints and stepping

**Where the check goes.** The top of `exec_block`'s statement loop, immediately beside the interrupt poll, and inside `run_repl_line`'s loop (:6558) for parity. Because `Stmt::SourceLine` is executed as a statement (:7469), the cleanest form is: after `exec` handles a `SourceLine`, ask the debugger whether it wants to stop at `self.current_line`. One predicate:

```
should_pause = breakpoints.contains(&(file, line))
            || step_mode.satisfied(frames.len(), line)
```

`step_mode`:
- `Over` — pause at the next `SourceLine` with `frames.len() <= depth_at_step`.
- `Into` — pause at the next `SourceLine` at any depth.
- `Out` — pause at the next `SourceLine` with `frames.len() < depth_at_step`.
- `Continue` — breakpoints only.

Step-into lands naturally on the callee's first `SourceLine` because `call_block_fn` pushes the frame and then walks the body through `exec_block`. Builtins have no `SourceLine` markers and are therefore never stepped into — correct and honest ("step into `fft`" is meaningless; say so rather than stepping over silently).

**Breakpoint identity must be `(file, line)`, not `line`.** `import "helpers.qu"` pulls in other files (`Stmt::Import`, `qu-syntax/src/lib.rs:290`) and `Stmt::SourceLine` carries only a line number. Slice 1 can key on "the main script" and reject breakpoints in imported files with a clear message; adding a file id to the marker (or a per-program file table) is a later, contained change. Do not pretend a bare line number is unambiguous.

**Source-level breakpoints.** MATLAB has `dbstop`/`keyboard`, Python has `breakpoint()`. Recommendation: a builtin `breakpoint()` that pauses if a debugger is attached and is a **documented no-op with a one-time warning** otherwise — never a hard error, so a script with a stray `breakpoint()` still runs under `qu run`. Do not add a `dbstop` statement form; Qu's keyword budget is already strained and `breakpoint()` needs no grammar change at all.

## 5. "Change the execution cursor" — corrected scope (Ahmed, 2026-09-19)

**Correction to the analysis below, from Ahmed directly:** "rewind" was never meant as replay/undo. It means VB.NET's classic **"Set Next Statement"** — while paused, move the pointer for what runs next to any line, forward or backward, in the same live session. No state restore, no side-effect replay. Variables keep whatever value they currently hold; a line you jump backward past does not re-run unless execution reaches it again; a line you jump forward past (skip) simply never runs, so anything it would have set stays at its old value. This is a well-understood, deliberately "you're on your own" convention — not a promise that the resulting state is what an honest re-run would have produced.

This **removes the core objection to Direction A below** (side effects double-applying) because there is no re-execution implied at all — the user is manually choosing where the pointer goes, the same way `:skip` already does for one statement forward. The honest framing is: **`:goto <line>` (or `:back`/`:next` as line-relative shorthand) simply reassigns the pause point's `(line, stmt_index)` before resuming** — mechanically almost free, since the pause point is already "the next statement `exec_block`'s loop would run." No journal, no snapshot, no effect ledger needed for this feature. It should say plainly, once, that it does not undo anything — same spirit as `:skip`'s existing "say what was skipped" requirement — but it does not need to *refuse* anything, because nothing is being silently redone.

The snapshot/journal/effect-ledger machinery in Directions B/C below may still be worth building **separately**, as an actual "undo what I just did" feature — but that is no longer what "rewind" refers to, and it is not what Ahmed asked for. Treat §5's directions below as an analysis of a *different, optional* feature, not as the design for cursor movement. Slice 3 (§9) should be re-scoped to just `:goto`/`:back N`/`:skip N` (pointer moves), dropping the snapshot/journal work from the critical path entirely — a real complexity and time savings.

### Original analysis (three directions for a *replay-based* rewind — now known not to be the ask, kept for reference)

### Direction A — true program-counter rewind with re-execution
Move a cursor back to statement *k* and re-run from there. What the user probably pictures.

Fatal problem: side effects are not replayable. `write_line` to an open `Value::File` (:657) appends twice. A serial write (:839) is gone. A TCP send (:716) is gone. `print` output is duplicated. Plots accumulate into `figure_history`. Worse, the *pure* parts are perfectly replayable, so a rewind-and-rerun produces a session that looks consistent and is quietly wrong in exactly the places the user isn't watching.

Note one nuance the naive framing gets wrong: `rand`/`randn` are **not** the obstacle. `rng` is a single `u64` (`lib.rs:4010`, `Rng` at :3378) and is trivially snapshot-and-restored, so randomness can be made exactly reproducible across a rewind. The obstacle is I/O and external state, which cannot.

Against a language whose README commits to "Silence is the worst failure" and "wrong answers that look right are the failure mode this is organised against" (README.md:171-190), shipping a rewind that silently double-applies side effects is not a tradeoff — it is a contradiction of the product. **Rejected.**

### Direction B — full state snapshots at each pause
At every pause (or every statement), clone `env`, `frames`, `frame_globals`, `rng`, `current_line`, `declared`. Rewind = restore a snapshot; **no re-execution at all**. The clone is cheap because `Value`'s bulk variants are `Arc` (:417-428) — a 10M-element array costs a refcount bump.

Honest limits, which must be stated in the UI, not buried:
- Restoring `env` does **not** undo side effects. A file written before the rewind is still written. The `Arc<StdMutex<…>>` handles are *shared* on clone (lib.rs:657 and friends), so a "restored" file handle is the same live OS handle at the same seek position — the snapshot restores the *binding*, not the resource.
- `Interp` as a whole cannot be snapshotted (workers, GUI, mutex/condvar), so the snapshot is explicitly a **variable-environment** snapshot, and must be named that way.
- Memory is bounded by the number of distinct `Value`s written, not by snapshot count, but a pathological loop rebinding a large array each iteration will still grow.

### Direction C — per-statement undo journal (diffs), plus an effect ledger
Instead of a whole-environment snapshot per step, record the *delta*: for each statement executed under debug, the list of `(scope, name, previous Option<Value>)` for every binding it changed, plus the prior `rng` state and `current_line`. Rewinding one statement replays the journal backwards. Memory is proportional to what actually changed, which for the typical `for` loop over scalars is a few bytes per step, and the journal doubles as a "what did this line change" display — genuinely useful independent of rewind.

Capturing the deltas needs a write hook on `var_set`/`var_remove`/the `ref`-alias path — a handful of call sites, all already funnelled (`var_set`/`var_get` are noted as the single scoping funnel at lib.rs:3788-3790). Index-assignment through `Arc::make_mut` mutates in place; the journal must record the pre-image `Value` (an `Arc` clone taken *before* `make_mut`, which is exactly when the copy-on-write would fire anyway).

Alongside the journal, keep an **effect ledger**: each journal entry is tagged with whether that statement performed an irreversible effect (wrote a file, wrote serial/TCP, printed, drew, spawned a worker, mutated a shared handle). This is a per-builtin classification — a boolean on the builtin dispatch, defaulting to "effectful" for anything unclassified, since the safe default under Qu's own commitments is to assume irreversibility, not to assume purity.

Then rewind behaves like this:
- Rewinding across only pure statements: silent, exact, uncontroversial.
- Rewinding across an effectful statement: **name the effects and stop.** `cannot rewind past line 14: it wrote to "out.csv" (2 lines) and drew on figure 1 — these cannot be undone. use :rewind --anyway to restore variables only, leaving those effects in place.`

### Recommendation
**Direction C, with B as its fallback implementation for the first slice.** Ship "rewind = restore variable state, never re-execute" — and make the refusal-to-rewind-past-effects the default behaviour, overridable with an explicit flag that prints what is being left behind. This is the only version of the feature that is *Qu-shaped*: it gives the user the thing they actually want 90% of the time (go back and look, or go back and try a different value) without ever producing a session state that differs from what an honest execution would have produced, and it refuses out loud in the other 10% instead of guessing.

Start with B (whole-environment snapshot at each pause, `Arc`-cheap, ~150 lines) because it is provably correct and needs no write hooks, and migrate to C's journal when per-statement rewind inside loops makes snapshot-per-pause too coarse. C's effect ledger, however, should land with the *first* rewind command, not later — the honesty is the feature, not a refinement of it.

**"Skip"** (advance past the next statement without running it) is much easier than rewind and should ship with it: the pause point is before `exec(s)`, so skipping is `continue` in `exec_block`'s loop. It needs one guarantee that must be stated: skipping a `for`/`while`/`if`/`function` header skips the **entire construct**, not its first line, because `Stmt::For` is one statement containing its body. Skip must print what it skipped (`skipped: for i = 1 to 100 … end (lines 8-12)`), never just advance silently. Skipping *into* an arbitrary line ("jump to line 20") is a different and much worse feature — it can land mid-block with bindings that never got made — and should be refused for now.

## 6. Optimizer interaction (non-optional)

While `debug` is `Some(_)`:
- `try_fast_for_plan` (:7087) and `try_fast_while_plan` (:7198) must return `None`.
- `compiled_funcs` (:3892) must be bypassed so bodies run through `exec_block`.

Otherwise breakpoints inside numeric loops and soft-compiled functions silently never fire. This costs real speed under debug — which is fine and normal (every debugger does it) — but it must be *said*: the debug banner should state that optimizations are off, so nobody benchmarks under a debugger. This also means `debug` must invalidate the `compiled_funcs` cache on attach/detach, the same way the cache is already invalidated on redefinition (:3869-3870).

## 7. Where debug state lives, and who gets it

**In `Interp`, shared by both frontends.** `pub debug: Option<DebugState>` next to `interrupt` and `on_print`. `None` by default ⇒ zero behaviour change for `qu run`, for every one of the 1953 tests, and for `eval`. Both `qu repl` and `qu kernel` attach their own `on_pause` callback.

**Both frontends get commands — not kernel-only.** `qu repl` already has a `:`-prefixed meta-command dispatcher that survives inside a pending block (main.rs:1286-1323), a `qu repl <file.qu>` preload path (:1259) that gives a file to set breakpoints *in*, and a human sitting at it. It is the cheapest possible place to prove the pause mechanism works, needs no threading change, and is where Ahmed himself will try this first. Kernel-only would mean the mechanism's first exercise is also its hardest integration.

### `qu repl` meta-commands
```
:break 14            set breakpoint at line 14 of the loaded file
:break f             break on entry to function f
:breaks              list them (with hit counts)
:delete 14 | :delete all
:run | :continue     resume
```
and, only while paused (prompt changes to `dbg 14>`):
```
:next / :n           step over
:step / :s           step into
:out / :finish       step out
:where / :bt         backtrace (call_stack + current_line)
:locals              current frame's bindings
:skip                don't run the next statement; say what was skipped
:back [n]            rewind n statements (refuses across effects; --anyway overrides)
:stop                abandon the rest of the run, keep the state, return to qu>
```
Any non-`:` line typed at `dbg>` is evaluated in the paused scope via `run_repl_line` — that is the single most useful thing a debugger does, and it is free here.

### `qu kernel` JSON ops
Requests: `set_breakpoints` (whole set, replacing — DAP's shape, not incremental add/remove), `debug_continue`, `debug_step` with `{"mode":"over"|"into"|"out"}`, `debug_skip`, `debug_rewind` `{"count":n,"force":bool}`, `stack_trace`, `scopes`, `debug_evaluate` `{"expr":…,"frame":n}`, `debug_detach`.

New unsolicited response line: `{"op":"paused","reason":"breakpoint"|"step"|"exception","line":N,"file":"…","stack":[…],"variables":[…]}` — emitted from inside a `run`, before that `run`'s own response. This is the one place the existing protocol's strict "one response per request, in the same order" contract (documented at main.rs:1388-1403) is extended; the doc comment must be updated to say so precisely, because Qu Studio's `repl_bridge.rs` currently relies on that contract.

These names and payload shapes are deliberately **DAP-shaped** (`setBreakpoints`, `stackTrace`, `scopes`, `variables`, `continue`, `next`, `stepIn`, `stepOut`, `evaluate`) so the op set does not preclude a real adapter later.

## 8. Jupyter / DAP

One correction to the framing in the request: Jupyter *does* have a debugger story, and it is DAP. Since messaging protocol 5.5, `debug_request`/`debug_reply` on the **control** channel and `debug_event` on iopub carry Debug Adapter Protocol messages verbatim; that is how `ipykernel`+`debugpy` and `xeus-python` back JupyterLab's debugger panel, and `kernel_info_reply` advertises it via `"debugger": true`. `qu-jupyter`'s `kernel_info` (kernel.rs:~477) does not advertise it today, correctly.

So there is exactly one protocol to implement for both JupyterLab's debugger *and* VS Code's Run-and-Debug: DAP. It goes in a later slice (a `qu-dap` crate, or a `debug_request` handler on the control channel — the control channel is already a separate task, which is exactly what a debugger needs). **Out of scope for slices 1-3**, but the op set above is designed so the eventual adapter is a translation layer with no new interpreter work. The one thing to get right now is that a pause is an *event* the kernel pushes, not something a client polls — DAP's `stopped` event has the same shape.

## 9. Phasing

- **Slice 1 — pause + breakpoints + step, `qu repl` only.** `DebugState` on `Interp`, pause hook in `exec_block`/`run_repl_line`, optimizer bypass, `:break`/`:breaks`/`:delete`/`:continue`/`:next`/`:step`/`:out`/`:where`/`:locals`, expression evaluation at the `dbg>` prompt, `breakpoint()` builtin. No rewind, no skip, no kernel. This is the honest, shippable core.
- **Slice 2 — kernel.** Split `cmd_kernel`'s stdin reader from the interpreter thread, add the ops in §7, `paused` event, update the protocol doc comment at main.rs:1356-1407. Qu Studio's gutter breakpoints land here.
- **Slice 3 — cursor movement.** `:skip` (with mandatory "what was skipped" reporting), snapshot-based `:back`, effect ledger and the refuse-across-effects default. This is the slice that needs Ahmed's answer to Q1 before it starts.
- **Slice 4 — DAP.** `debug_request` on `qu-jupyter`'s control channel; `"debugger": true` in `kernel_info_reply`; a VS Code adapter.
- **Slice 5 (optional) — undo journal** replacing snapshots, if loop-granular rewind proves too memory-hungry.

## 10. Documentation truth-update this forces

The moment Slice 1 ships, the wiki's *Running Qu* page is partially false. It currently says none of Qu's run modes fake a debugger and that "Qu's execution model (`qu run <file>`, a one-shot subprocess with no persistent interpreter state) genuinely doesn't support breakpoints or stepping today."

The replacement must be equally precise about what is *still* true — that is the whole value of the original sentence:
- `qu run <file>` remains a one-shot subprocess and **still has no debugger**. Debugging requires a persistent session: `qu repl <file.qu>` or `qu kernel`.
- Breakpoints and stepping are real in those sessions.
- Optimizations are disabled under debug; timings there are not representative.
- Rewind (when it lands) restores **variables only** and cannot undo I/O, and says so at the moment it matters.

Also to update: `engine/crates/qu-jupyter/README.md`'s "What's not implemented yet" list (the interrupt commit already edited it; a debugger bullet belongs there until Slice 4), `README.md`'s feature list, `CHANGELOG.md`, and `cmd_kernel`'s protocol doc comment (main.rs:1356-1407) which currently documents a strict request/response pairing that the `paused` event breaks.

## 11. Open questions for Ahmed

1. **The rewind-honesty tradeoff — the one only you should settle.** Should `:back` across a statement that wrote a file / drove serial / drew a plot **refuse by default** (with `--anyway` to restore variables regardless), or **warn and proceed**? This plan recommends refuse-by-default as the reading most consistent with the README's design commitments, but it makes the feature feel obstructive exactly when a user is debugging messy I/O code — which is when they most want it. Your call, not a planning-level one.
2. **Is a variable-only rewind worth shipping at all**, or is it so far from what "rewind the execution cursor" sounds like that it will mislead more than it helps? An honest alternative is to ship only `:skip` plus "re-run this cell from the top" and never call anything "rewind".
3. **Does `qu run` get a `--debug` flag** (turning a one-shot run into a debuggable one that drops to a `dbg>` prompt on a breakpoint), or is debugging strictly a persistent-session feature? The former is much friendlier and is a small addition once Slice 1 exists; it also changes the wiki's framing considerably more.
4. **`breakpoint()` as a builtin** — acceptable, or does a "debug statement left in source" offend the same way a stray `disp` does? Related: should it warn once when hit with no debugger attached, or be completely silent?
5. **Breakpoints in imported `.qu` files.** Slice 1 proposes rejecting them with a clear message. Is that acceptable, or does your actual workflow (multi-file projects via `import`) make file-scoped breakpoints a slice-1 requirement?
6. **Debug-mode performance disclosure.** Disabling the fast paths can be a 10-50x slowdown on numeric loops. Banner line, one-time warning, or documented only?

## Critical files for implementation

- `engine/crates/qu-interp/src/lib.rs`
- `engine/crates/qu-cli/src/main.rs`
- `engine/crates/qu-syntax/src/lib.rs`
- `engine/crates/qu-jupyter/src/kernel.rs`
