# qu-jupyter

A Jupyter kernel for Qu. Speaks the real Jupyter messaging protocol (ZeroMQ,
5 sockets, HMAC-signed multipart messages) over a pure-Rust ZMTP
implementation — no system `libzmq` to install or bundle. One
`qu_interp::Interp` session lives for the whole kernel process, so a
notebook's later cells see earlier cells' variables, exactly like `qu repl`.

Both **Jupyter** (JupyterLab, Notebook, `jupyter console`) and **VS Code**'s
Jupyter extension discover kernels the same way — by scanning the standard
`.../jupyter/kernels/<name>/kernel.json` directory — so a single install step
covers both; there is no separate VS Code extension.

## Build

```
cargo build --release -p qu-jupyter
```

Produces `qu-jupyter` (`qu-jupyter.exe` on Windows) next to the `qu` binary.

## Install

```
qu-jupyter install
```

Writes a `kernel.json` kernelspec (pointing `argv` at this exact binary) into
your user Jupyter data directory (`%APPDATA%\jupyter\kernels\qu` on Windows,
`~/Library/Jupyter/kernels/qu` on macOS, `~/.local/share/jupyter/kernels/qu`
on Linux — `$JUPYTER_DATA_DIR` overrides on any platform). Use
`--prefix <dir>` to install into `<dir>/share/jupyter/kernels/qu` instead
(a venv, a shared install).

Re-run after every `qu-jupyter` rebuild you want notebooks to pick up — the
kernelspec's `argv` is an absolute path to the binary at install time, not a
symlink or a `PATH` lookup.

## Use it

**Jupyter**: `jupyter notebook` / `jupyter lab`, then pick "Qu" from the
kernel picker (or `jupyter console --kernel qu`).

**VS Code**: install Microsoft's "Jupyter" extension, open or create a
`.ipynb` file, "Select Kernel" → "Jupyter Kernel..." → "Qu".

A cell behaves like a line typed at `qu repl`: a bare expression or an
unsuppressed assignment echoes `name = <value>`; a trailing `;` suppresses
it. `print`/`writeline`/etc. stream to the cell's output live, not only after
the cell finishes. Any figure the cell drew — with or without an explicit
`show`/`savefig` — renders inline as SVG (`qu-interp` has no in-memory PNG
rasterizer yet; SVG is what Jupyter's rich-display protocol wants anyway).
Tab completion works against the session's current variable bindings.

## What's not implemented yet

- **Interrupt**: `interrupt_request` is acknowledged but can't actually stop
  a running cell — Qu's tree-walking evaluator has no cooperative
  cancellation point today. A runaway cell can still be stopped by killing
  the kernel process (Jupyter's "restart kernel").
- **stdin/`input()`**: the stdin channel is bound (so frontends that probe it
  don't error) but not serviced — Qu has no `input()`-style builtin yet.
- **PNG output**: figures are SVG-only, inherited from `qu-interp` itself
  (`savefig(..., "png")` errors there too, for the same reason).
- **ipywidgets / comm messages**: `comm_info_request` stubs an empty reply;
  `comm_open`/`comm_msg` aren't handled.

## Design notes (for anyone maintaining this)

- `src/protocol.rs` — wire framing and HMAC signing/verification, no
  knowledge of socket types. Unit-tested against round-tripping and a
  tampered-signature rejection.
- `src/connection.rs` — parses Jupyter's connection file.
- `src/kernel.rs` — the socket set (shell/control/iopub/stdin/heartbeat,
  each its own task) and message dispatch. See `handle_execute`'s doc
  comment for why cell execution runs on a `spawn_blocking` worker thread
  rather than directly on the async task (a Qu script has no yield points
  and can run arbitrarily long — or loop forever — and would otherwise
  starve the kernel's own heartbeat/control handling), and why `dispatch`
  wraps *every* shell request in a `busy`/`idle` iopub status pair (spec
  requirement, and also what makes `jupyter_client`'s `wait_for_ready()`
  handshake succeed at all under PUB/SUB's slow-joiner race).
- `src/install.rs` — kernelspec writer.

Verified end-to-end against a real `jupyter_client` (`BlockingKernelClient`
+ `KernelManager`, launching the actual built binary as a subprocess and
talking real ZMQ to it) — persistent state across cells, live stdout
streaming, figure `display_data`, error recovery without killing the
session, and `is_complete_request` — not just unit tests against the framing
code in isolation.
