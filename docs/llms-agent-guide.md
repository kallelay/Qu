# Using Qu

Qu is the language for tooling in this repository — parsing a file,
summarising a CSV, checking an assumption, doing arithmetic you would
otherwise do in your head. Python is reserved for what it is uniquely for
here: running the *other* side of a cross-language comparison.

That rule erodes for one reason. Reaching for Python is a reflex, and the
reflex wins whenever "how do I do this in Qu?" takes longer to answer than
just writing the Python. So this guide is organised around the tasks
people reach for Python for, not around the language's features.

**Every example here was run before it was written**, from
`docs/guide_probe.qu`, which contains them all in one file. If you change
an example, run it. The rule exists because the failure mode in this
document is not a broken example — it is a *plausible* one.

---

## First: is the binary you are about to trust current?

Stale artefacts are the most productive source of wrong conclusions in
this project — a stale binary and a stale source tree both answer every
question fluently and never mention being stale. **Timestamps do not tell
you.** Ask the binary something only a current one answers:

```
qu eval 'tic(1)'
```

- suggests **`help("tic")`** → post-2026-09-10, safe to trust
- suggests **`available("tic")`** → older, **do not draw conclusions from it**

Measured across five binaries on one machine:

```
  09-09 14:18   PRE-FIX    qu-studio-tauri sidecar
  09-09 19:59   PRE-FIX    D:/QuWorkspace engine
  09-09 21:37   PRE-FIX    a worktree build
  09-10 15:21   PRE-FIX    <- built the SAME DAY as the one below
  09-10 18:07   POST-FIX
```

Two builds three hours apart on the same day fall on opposite sides. No
ordering of mtimes would have told you which.

I have a stake in this one. The fix it detects is mine, from that
afternoon — and the `09-10 15:21` binary above is also mine, the one I had
just used to verify a different fix. **The probe exists because a stale
binary would have hidden my own work from me**, and it flags my own build.

**It is a FLOOR, not a version.** It separates "predates 2026-09-10" from
"does not". It cannot tell you how current a passing binary is, and once
nothing that old survives it will pass for everything and tell you
nothing. A dated probe that outlives its subject becomes a false
all-clear — which is the same failure one generation on. If you need
"current as of today", pick a defect fixed today and probe for that
instead.

Two companion habits, same reasoning:

- **Read source with `git show master:<path>`**, not from a working tree
  you have not verified. A worktree can be hundreds of commits behind and
  looks identical.
- **A ~2,000-line offset for the same function in the same file is
  staleness, not a different function.** Line-number drift is the tell: a
  citation at 30287 where master has 32386 is one tree being old, not two
  functions with one name.

---

## Quick arithmetic

The commonest thing anyone opens Python for, and the least defensible.

```qu
bytes_moved = 1500000 * 51 * 8
gb = bytes_moved / 1e9
print("moved {gb} GB")                       # moved 0.612 GB

secs = 0.0269
print("effective {bytes_moved / secs / 1e9} GB/s")   # 22.750929 GB/s
```

`{...}` inside a string interpolates an expression, so there is no
`format`/f-string ceremony for the common case. Put a script in a scratch
file and `qu run scratch.qu`.

---

## Reading files

Whole file as text:

```qu
txt = read_all_text(path)
print("chars: {len(txt)}")
```

Line by line — `lines()` splits, `trim()` first so a trailing newline does
not hand you an empty last line:

```qu
for line in lines(trim(txt))
    parts = split(line, ",")
    print("  {parts[0]} -> {parts[1]}")
end for
```

A CSV goes straight to a table, with headers:

```qu
t = read_csv(path, headers = true)
print("rows: {nrow(t)}")
print(head(t, 2))
```

```
name  score
----  -----
a         1
b         4
```

`tmp_file()` gives you a scratch path and `write_text(path, s)` fills it —
enough to make a fixture inline rather than shipping a data file with a
probe script.

Also there: `read_csv`, `write_csv`, `parse_csv`, `parse_json`,
`parse_xml`, `read_all`, `file_exists`, `file_size`, `list_files`, `glob`,
`load`, `save`. For binary, the typed readers — `read_int16`,
`read_uint32`, `read_double`, `read_structs` — are a complete toolkit; see
the worked example below.

---

## Text

```qu
s = "  Measurement Lane  "
print("[{trim(s)}]")              # [Measurement Lane]
print(upper(trim(s)))             # MEASUREMENT LANE
print(contains(s, "Lane"))        # true
print(replace(s, "Lane", "Window"))
```

Regex — **the string comes first, the pattern second**:

```qu
print(regex_match("abc123", "[0-9]+"))     # true
print(regex_count("a1b22c333", "[0-9]+"))  # 3
```

Also `regex_find`, `regex_find_all`, `regex_groups`, `regex_replace`,
`regex_split`, plus `starts_with`, `ends_with`, `index_of`, `substr`,
`pad_left`, `split`, `chars`.

---

## Collections

A `(...)` literal is a **list** and a `[...]` literal is a **vector**. The
list is the general container; the vector is numeric and is what maths
operates on. Build a list by appending:

```qu
acc = ()
for i = 1 to 6
    acc = append(acc, i * i)
end for
print(acc)          # (1, 4, 9, 16, 25, 36)
print(len(acc))     # 6
```

`filter` and `fold` take the name of a function you defined, **as a
string**:

```qu
function is_even(n)
    return n mod 2 == 0
end function

function add2(a, b)
    return a + b
end function

print(filter(acc, "is_even"))     # (4, 16, 36)
print(fold(acc, "add2", 0))       # 91
```

Also `map`, `reduce`, `sort`, `sort_by`, `unique`, `first`, `last`,
`take`, `drop`, `zip`, `group_by_agg`, `dict`, `keys`, `values`, `items`,
`table`.

---

## Timing

One global stopwatch. `tic()` and `toc()` take no arguments.

```qu
tic()
# ... work ...
dt = toc()          # a number, in seconds
```

---

## Finding your way without leaving Qu

You do not have to grep the source or the book to find a function:

```qu
print(apropos("csv"))
```

```
("csv2json", "csv2xml", "csvify", "json2csv", "parse_csv", "read_csv",
 "write_csv", "xml2csv")
```

`help(name)` prints a function's documentation, `describe(x)` summarises a
value, and `type(x)` names its kind. A misspelled call already suggests
the nearest match, so a wrong guess is cheap.

---

## When you get the arguments wrong

Qu catches it, and tells you the arity:

```qu
tic(1)
# tic: 1 arguments given, but tic reads 0 arguments -- 1 would be silently
# ignored. Check the order, or `help("tic")` for related names and worked
# examples.
```

**There is currently no way to ask a builtin for its signature from inside
Qu.** `help(name)` gives the name, related names, and a pointer to
`catalog/`; it does not state parameters, for any builtin. `apropos(name)`
finds names. Neither answers "what are the arguments, in what order".

For that, read the function's page in the book, or — usually faster — find
a real call:

```qu
print(help("read_csv"))
#   read_csv is a Qu builtin.
#   related: read, re
#   worked examples live in catalog/ -- search it for `read_csv(`.
```

That last line is the honest route: `catalog/` is full of working scripts,
and a real call answers the question a signature would.

This entry is here twice over. The error message first suggested
`available("tic")` — a **serial port** builtin, so following it produced a
second, stranger error. That was fixed to `help(...)`, still promising
"for what it takes" — which `help` does not do for any of the six builtins
I checked. A quieter false promise, but the same one. The message now
claims only what `help` returns.

**A well-written wrong answer is harder to catch than a badly-written
one.** That message names the failure, the consequence and a remedy; it
reads beautifully; it was reviewed and praised. Nobody ran the remedy.

**This message's own history doubles as a way to date a binary.** Three
texts have existed for this exact error, in order: `available("tic")`
"for what it takes" (wrong — `available` is the serial-port builtin),
`help("tic")` "for what it takes" (still wrong — `help` doesn't state
parameters), then `help("tic")` "for related names and worked examples"
(the current, accurate one, shown above). Running `qu eval 'tic(1)'` and
checking which text comes back tells you which side of each fix the
binary is on — useful when several builds of the same worktree are
floating around a shared machine and you need to know whether the one
in hand actually has a change you're relying on.

One real trial of this, the first done independently rather than by
whoever wrote the fix: a binary came back with the FIRST text
(`available`). Traced to source rather than assumed — that binary's own
worktree still had the old string verbatim, several commits behind
where the second and third fixes landed. The probe called it correctly.

**A failed probe answers "is this binary current for THIS fix", not "is
this binary usable."** Those are different questions, and it is a
mistake to collapse them in either direction: don't discard a binary
that failed this one probe if the work in front of you touches none of
what the probe is dating, and don't wave the failure off as harmless
just because the work in front of you happened to be fine — check what
your work actually depends on before concluding either way. In the one
real trial above, the stale binary was still completely correct for
every builtin the task at hand used (none of them were behind any
recent fix) — both things were true at once, and neither one excused
skipping the check on the message this probe actually dates.

---

## A worked example

`catalog/qu_prm_loader.qu` is a complete binary-format parser — a
Re-Volt `.PRM` 3D mesh reader — written **entirely in Qu**, on
`read_structs` and `read_int16`, with no new engine code. 109 lines,
including a header whose two counts appear in different orders across
game builds and are disambiguated by checking each reading against the
file's actual length.

Read it before assuming a job needs Python. It is the honest answer to
"can Qu really do this kind of work".

---

## Traps

Everything below produces a **wrong answer or a confusing error**, not an
obvious failure. They are here because they cost real time.

**The dangerous failure is not the one that returns nothing — it is the
one that returns something believable.** Read that twice before writing a
smoke test.

### A variable named `where` cannot be subscripted

`where` is a builtin, and assigning to it is accepted. The variable is
**fine** — it is the *indexing* that breaks, because `where[...]` reads as
the masking builtin rather than as an index into your value:

```qu
where = [10, 20, 30]
print(len(where))        # 3       <- the list is intact
print(where[0])          # []      <- no error, no warning
```

That `len` is correct is the cruel part: everything you check to reassure
yourself looks right, and only the subscript lies. **If a variable reads
back empty but its length is correct, check the name against the
builtins.**

`where` is the only name I found that does this. `filter`, `find` and `at`
are builtins too and are **not** affected — `filter = [10, 20, 30]` then
`filter[0]` gives `10`. They are named here only so nobody renames a
working variable on a rumour.

### A reversed regex can return `true`

`regex_match(s, pattern)` — string first. Reversed, it does not reliably
return `false`; it returns `true` whenever the pattern text occurs
literally in the string, which includes the case where you test it against
itself:

```qu
print(regex_match("[0-9]+", "abc123"))    # false
print(regex_match("[0-9]+", "[0-9]+"))    # true   <- a smoke test passes
```

So the tell is a count or result that looks plausible, not a zero.

### A nested list literal is not a matrix

```qu
print(shape([[1, 2], [3, 4]]))            # [4, 1]   -- a column vector
print(shape([1, 2, 3, 4] as matrix(2, 2)))  # [2, 2]
```

No error. Every reduction over it then answers about four numbers rather
than two rows, and `axis=` looks ignored.

And `as matrix` fills **column-major**: `[1,2,3,4] as matrix(2,2)` is
`[1, 3; 2, 4]`, not `[1, 2; 3, 4]`.

### Lists are not vectors

`(1, 2)` has no arithmetic and does not stack:

```qu
print((1,2,3,4) as matrix(2,2))     # cannot reshape a list
print(hstack(("a",), ("b",)))       # expected a number, found list
```

Use `[...]` when you want maths, `(...)` when you want a container.

### Asserting on contiguous text in generated SVG

`render_math_svg` splits text into runs wherever styling changes, so the
input string does not appear contiguously in the output. A test asserting
that it does **fails against correct output** — the renderer is right and
the assertion is wrong, which is the expensive direction.

Assert on a property that survives the split — the script font-size, say —
rather than on the input string appearing in the result.

### `step` is a keyword and fails loudly

Not a silent trap — filed here so it is not confused with one:

```
step = [10, 20, 30]
qu: parse error at 1:1: unexpected Keyword("step") in expression
```

Column 1, line 1, before anything runs. Same for other keywords used as
names.

### `mod` is an operator, not a function

```qu
n mod 2 == 0        # correct
mod(n, 2)           # parse error: unexpected Keyword("mod") in expression
```

Same for `and`, `or`, `not`, `in`, `to`, `step`, `by`.

### Define functions before you use them

A function defined *below* its first use is not found:

```
where: no user function named `is_even`
```

Hoist definitions to the top of a probe script. This bites specifically
when you add a helper while debugging and append it at the bottom.

### A bare `{` in a string interpolates — sometimes silently

`{...}` inside a string is an expression to evaluate. If the name inside
happens to exist, there is **no error at all** and your prose changes
under you:

```qu
set = 42
print("the {set} of values")     # the 42 of values
```

That is the form that bites, because the words most likely to appear in a
sentence — `set`, `value`, `n`, `x` — are also the names most likely to be
live variables.

If the name does *not* exist you get a **runtime** error, not a parse
error:

```qu
print("literal {braces} here")
# qu: runtime error: `braces` is not defined
```

Which matters for tooling: the file passes a parse check clean, so a
corpus-acceptance run that only parses will give it a pass.

A nested double-quoted string inside `{...}` also breaks the parse — hoist
the value to a variable first.

### Every range is inclusive

`0 to 9` is ten values. So is a slice: `v[0:4]` is five elements. To take
the first `n`, write `v[0:n - 1]`.

### Comments are `#`, not `//`

`//` parses as division and fails on line one.

### Elementwise operators are dotted

`*` is matrix multiplication and `^` is matrix power; `.*` and `.^` are
elementwise. On a 300x300 grid `^` silently computes a matrix power.

### Assignment inside a function is local

Reads fall through to module scope; writes do not. Declare `global` to
write through.

### `import` runs the WHOLE target file, top level included

`import "path.qu"` executes every top-level statement in that file, not
just its function definitions — including a demo or self-test written
at the bottom of it. This one needs two files, so it is outside
`guide_probe.qu`'s single-file convention; run directly instead
(`lib.qu` defines `double` and prints on load, `caller.qu` imports it):

```
$ qu run caller.qu
lib_with_toplevel.qu ran its own top level
caller ran, double(21) = 42
imported a second time
```

A caller who only wants `double` gets that print anyway, on the first
import — there is no `if __name__ == "__main__":` equivalent to guard
it. A file meant to be imported for its functions should not put a demo
at its top level; a file that does both (this repo's own convention —
`backend auto` then `#%%`, so a module both documents and proves itself
when run directly) will fire that demo's side effects — extra output,
extra work, and for a plotting demo specifically, extra shapes drawn
into whatever figure the importer already had open (see the two
plotting traps above) — on whoever imports it first.

**One thing this is NOT**: importing the SAME file a second time does
not re-run it — Qu tracks already-imported paths and skips the repeat,
which is exactly what `imported a second time` above proves (no second
`lib_with_toplevel.qu ran its own top level` line). The cost is paid
once, on first import, not on every one.

### A filled shape defaults to 18% opacity

`polygon(xs, ys, fill = "...")`, and any other shape drawn with `fill=`,
renders at `alpha = 0.18` unless you say otherwise. Tuned for stacking
several translucent fills (`area()`-style) so each stays visible under
the others — but it applies just the same to a single shape, and to
anyone drawing SOLID fills, which is exactly what a filled map region or
a triangle-mesh renderer needs:

```qu
figure()
polygon([0.0, 1.0, 1.0], [0.0, 0.0, 1.0], fill = "#ff0000")
savefig(tmp_svg)
print(contains(read_all_text(tmp_svg), "opacity=\"0.18\""))   # true
```

No error, no warning — the SVG just has a faint fill, or, if several such
shapes overlap, the ones meant to occlude each other show straight
through instead. Pass `alpha = 1.0` explicitly for a solid fill.

### `figure_size(...)` sets the canvas, it does not clear it

Only the bare `figure()` (no arguments) starts a blank canvas. Sizing an
existing one leaves whatever was already drawn in place — shapes drawn
before it keep accumulating:

```qu
figure()
polygon([0.0, 1.0, 1.0], [0.0, 0.0, 1.0], fill = "#00ff00", alpha = 1.0)
figure_size(200, 150, unit = "mm")
polygon([2.0, 3.0, 3.0], [0.0, 0.0, 1.0], fill = "#0000ff", alpha = 1.0)
savefig(tmp_svg)
print(regex_count(read_all_text(tmp_svg), "<polygon"))   # 2, not 1
```

Same distinction as matplotlib's `plt.figure()` versus a `figsize=`
kwarg. A function meant to be callable more than once per run — or
called after ANY other plotting in the same run, including another
function's own self-test firing on `import` (see above) — should call
`figure()` itself before drawing, rather than assume it starts clean.
Found by rendering two different meshes back to back through one shared
render function: the second SVG silently contained the first mesh's
leftover triangles too, at their own old coordinates, layered
underneath.

---

### `bitand`/`bitor`/`bitxor`/`bitcmp`/`bitshift` round-trip through
### `f64` on the way OUT, not just on the way in

Every value in Qu is a 64-bit float, exact only up to `2^53`. It is easy
to assume that's only a hazard for the INPUTS to a bitwise op — check
that your operands fit, and the result is safe. It isn't: the engine
computes the correct answer internally as a real integer, then casts
`as f64` to return it, which is a SECOND, separate rounding — one that
can corrupt a result even when both inputs were exact:

```qu
r = bitor(9007199254740992, 1)   # 2^53 OR 1 -- both operands exact
print(r)          # 9007199254740992 -- unchanged!
# true answer:      9007199254740993 -- odd, and f64 only represents
#                   EVEN integers exactly once you're past 2^53
```

Found porting SHA-256 (Qu-8 lane, 2026-09-16): `bitor(21248329,
91260882377506816)` returns `91260882398755152` in Qu; the true integer
OR is `91260882398755145` — silently off by 7, no error, no warning.
Confirmed by reading the source (`engine/crates/qu-interp/src/lib.rs`,
the `"bitand"`/`"bitor"`/`"bitxor"`/`"bitcmp"`/`"bitshift"` builtin
arms): all five share the identical pattern, `(int_arg(...) <op>
int_arg(...)) as f64` — the internal arithmetic is exact, correct
64-bit integer math; only the RETURN cast loses it.

**The practical rule this changes**: it is not enough for the value you
actually care about to fit under `2^53` (or under 32/53 significant
bits, however you're framing it) — every INTERMEDIATE result that
passes back out through one of these five builtins is a chance to lose
precision, even in a chain where the final answer would have been
fine. A 32-bit rotate built as `bitor(bitshift(x, -n), bitshift(x, 32 -
n))` is a real example that breaks: the unmasked left-shift term can
need up to 63 raw bits before you mask it back down to 32, and that
intermediate value round-trips through the same lossy `as f64` return
on its way out of `bitshift` alone, before `bitor` ever sees it. The
fix in that case is to mask each shifted half down to its intended
width BEFORE combining them, not after — never let an oversized
intermediate touch one of these five builtins at all.

**No engine-level fix decided yet** — whether these should gain a
genuine integer return path, or whether large-magnitude bitwise work
is simply out of scope for a float-backed `Value::Num`, is a real
design question, flagged to Ahmed rather than patched here.

---

## When you edit this file

Two rules, both learned by nearly getting them wrong:

1. **Run every example, before and after.** An earlier draft of the
   shadowing entry said `where` was a reserved keyword — which is what the
   failure looks like. Running it showed `where = 1` is fine and the real
   mechanism is builtin shadowing with silent empty results: different,
   and worse. A guide whose purpose is stopping confidently-wrong probes
   cannot itself be confidently wrong.

2. **A correct example can conceal a trap by being correct.** The
   nested-literal entry used `as matrix` — correct, and therefore it never
   showed the form *without* it, which is the one that bites. Look for
   places your examples do the right thing in a way that never
   demonstrates the adjacent wrong thing.

3. **Testing the mechanism is not testing the claim.** The sharpest
   example is this file's own history. An earlier draft said `where` was a
   reserved keyword; that was tested, found wrong, and corrected to
   builtin shadowing — good work. The corrected *mechanism* was then
   applied to four more builtin names without testing any of them. Three
   of the four are innocent, and the fourth (`step`) is a keyword, which
   is what the original discarded instinct had said. A later edit repeated
   the same move: it copied the list of five forward on the strength of
   having verified one.

   Getting the mechanism right does not license generalising past the
   evidence. Run each name.

4. **And the mirror: disproving a claim is not disproving the mechanism
   behind it.** The `where` entry went the other way too. Someone proposed
   that `help` answers for command verbs but not for real builtins — a
   correct model. Their example was wrong (`tic` is in the stub class, not
   the command-verb class), and when the example fell, the model was
   discarded with it and rewritten as "help is uniformly useless". It is
   not: `help("hold")` returns a real description.

   That mattered practically. "Uniformly useless" tells whoever fixes it
   they are starting from zero; "works for one class, not the other" tells
   them to extend a path that already exists. Different job, different
   size. When an example fails, check whether it took the rule down with
   it or only itself.
