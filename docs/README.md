# Qu Documentation Guide

Qu 0.12.1 is a specification-first language design for signal processing, numerical
computing, data/ML, algorithm evaluation, and deployable scientific systems. No
fully conforming engine is claimed yet; the first executable browser and Rust
vertical slice now exists and is tested against the same numerical semantics.

## Start here

- [Overview](index.html) — the two-minute language tour.
- [Getting started](getting-started/README.md) — three books, in order: [Fundamentals](getting-started/book1-fundamentals.html), [Numerics, DSP & Linear Algebra](getting-started/book2-numerics-dsp.html), [Specialized Domains](getting-started/book3-specialized.html).
- [Playground](ide.html) — edit and execute the working browser subset with live plots and diagnostics.
- [Syntax and ease](design.html#ease) — why the teaching surface is familiar.
- [Why Qu](reference.html) — the end-to-end scientific workflow and diagnostic contract.
- [Algorithm testing](testing.html) — numerical/DSP tests, A*, sandboxes, AI evaluation, and testbenches.
- [Systems and circuits](systems.html#runtime) — simulation, explicit-node circuits, browser/WASM and native SPICE runners, and reusable compilation.
- [Function index](functions.html) — searchable library surface.

## Canonical sources

1. [`qu-language-spec.md`](qu-language-spec.md) defines semantics and design commitments.
2. [`qu-grammar.ebnf`](qu-grammar.ebnf) defines the accepted syntax.
3. The HTML pages are the approachable, task-oriented companion.
4. `../catalog/` contains representative Qu programs.
5. `../IMPL.md` records the implementation architecture, milestones, and engine decisions.

## Implementation status

The Playground currently executes a deliberately small acceptance subset in an
isolated browser Worker: units, inclusive ranges, scalar/vector expressions,
method-style FFT/absolute value, statistics, printing, plots, and line-addressed
diagnostics. It also accepts logical assignment, prefix <code>where</code> indices,
indexed gathering, matrix-shaped random arrays, and layered marker plots.
`../qu-core/` is the authoritative dependency-free Rust reference for
ranges, mean/RMS, and radix-2 FFT; `../qu-wasm/` exposes the same functions to
WebAssembly. The JavaScript bootstrap kernel is temporary. It is removed feature by
feature only after the Rust/WASM path passes the same acceptance tests.

The next implemented core slice adds validated indexed meshes, premade geometric
primitives, exact-clock transform keyframes, and deterministic box/static-triangle
collision-hull artifacts. See [Systems and 3D](systems.html#reach) and the animated
[mesh/hull demo](live.html) — this is still spec/roadmap, not yet backed by a
runnable Qu example.

If prose and grammar disagree, treat it as a specification defect: do not invent a
third interpretation. Record and resolve the conflict in `BOARD.md`.

## Stability

The familiar scripting surface—assignment, parenthesized calls, arrays, explicit
blocks, zero-based indexing, inclusive slices and `to` loops, and `|>`
pipelines—is the canonical teaching surface. Advanced scheduling, distribution,
hardware, and provider integrations are draft designs. OpenAI and other AI systems
are packages, not core language syntax.

## Documentation rule

Every feature must show:

1. the smallest useful example;
2. its correctness and error behavior;
3. its reproducibility and performance implications; and
4. whether it is core syntax, standard library, bundled package, or external package.

That rule keeps Qu approachable for MATLAB, Python, BASIC/VB, Fortran, DSP, and
numerical-computing users while preventing convenient examples from hiding cost or
scientific assumptions.
