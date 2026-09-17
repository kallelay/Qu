# Before writing or running any `.qu` script

Read **`docs/llms-agent-guide.md`** first. It exists specifically because
agent sessions working in this repo (this one included, repeatedly, in
one night) keep writing throwaway verification/test scripts in Python-
or MATLAB-shaped Qu and getting SILENT wrong answers rather than a
parse error — the dangerous kind, because nothing says the script did
anything wrong.

The three that have actually cost time, so far, if you don't have time
to read the whole guide right now:

- **Comments are `#`, not `//`.** `//` parses as division and fails at
  the very first line.
- **`regex_match(s, pattern)`, `regex_find(s, pattern)`, `regex_count(s,
  pattern)` take the STRING first, the pattern second** — opposite of
  Python's `re.match(pattern, s)`. Reversed, there is no error: it just
  silently asks whether the literal pattern text appears inside the
  string, and usually answers `false`/no matches forever.
- **`tic()`/`toc()` take no arguments.** One global stopwatch, not a
  handle — `toc(t0)` is a rejected keyword-shaped call, not a working
  per-timer read.

The full guide also covers: inclusive ranges/slices (`0 to 9` is ten
values), column-major `as matrix(r, c)`, `.* .^` for elementwise ops,
`global` for writing an outer variable from inside a function, validated
keyword arguments, unexpected argument orders in several other builtins,
reserved names, string escape sequences, complex-number limits, how to
build an empty collection, and `zeros`/`ones` shape rules.

Check `help("name")` and the builtin index before assuming a
function exists or guessing its signature from another language.
(Do **not** use `available("name")` for this — despite the name it is a
serial-port function, and `available("set")` answers "expected a serial
port handle". This file told agents to use it until 2026-09-11 and cost
several sessions the same detour.)
`engine/target/release/qu run script.qu` is how you actually run
anything — build the engine first if it's stale (`qu-studio-bundles-
its-own-qu-exe` applies to the standalone binary too: it does not
rebuild itself).
