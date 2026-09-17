# Playground

Qu has a browser-based interactive playground with live plots and
diagnostics, separate from this book. It lives in the repository at:

- **`docs/ide.html`** — edit and run the working browser acceptance subset,
  with live plot rendering.
- **`docs/app.html`** — the broader interactive demo shell.

These are a live JavaScript/WASM runtime, not documentation, so they are
deliberately kept out of this book's own `src/` tree rather than absorbed
into it — this page is just a signpost, not a hyperlink, since this book and
`docs/` may be built and hosted independently of each other and a hard-coded
relative link between them would be liable to rot. Open the file directly
(e.g. `file:///.../Qu/docs/ide.html`) or serve the repository root with any
static file server if you want it reachable by URL alongside this book.

## What the playground runs today

The playground executes a deliberately small acceptance subset in an
isolated browser Worker: units, inclusive ranges, scalar/vector expressions,
method-style FFT/absolute value, statistics, printing, plots, and
line-addressed diagnostics. `qu-core` is the authoritative dependency-free
Rust reference for ranges, mean/RMS, and radix-2 FFT; `qu-wasm` exposes the
same functions to WebAssembly. The JavaScript bootstrap kernel is temporary
and is removed feature by feature only after the Rust/WASM path passes the
same acceptance tests — see `docs/README.md` in the repository for the
current status in more detail.

For the full native interpreter (the one this book's Standard Library
Reference documents), run scripts from the command line instead:

```bash
cargo run --manifest-path engine/Cargo.toml -p qu-cli -- run my_script.qu
cargo run --manifest-path engine/Cargo.toml -p qu-cli -- repl
```
