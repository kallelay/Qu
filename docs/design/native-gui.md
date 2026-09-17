# Native GUI assessment and initial implementation

Qu already has named functions, method-call syntax, keyword arguments, numerical
arrays, and immediate-mode `ui_*` controls. Those controls rerun a script and are
not suitable for owning a long-lived serial connection. Studio currently invokes
the interpreter as a subprocess. Retaining that isolation lets a slow callback be
stopped without freezing the editor.

The initial retained API uses existing syntax: `t = Frame("Title")`,
`panel = t.add("panel", layout="row")`, `button.on("click", "handler")`,
and `widget.set(text="Updated")`. A handler is a previously declared named
function receiving `{target, type, value}`. Anonymous functions and `Frame t`
declarations are not introduced as incidental parser changes. Property updates
use `.set(...)` to validate related changes atomically.

Frames, panels, labels, buttons, sliders, number and text inputs, checkboxes, and
plots share an interpreter-owned tree. Events execute serially in that same
interpreter. Initialization is not replayed. The existing `ui_*` API remains
available for immediate-mode scripts.

Plot assessment: the shared Studio plot viewer already supplies design controls,
labels, export, zoom, and equal-axis scaling. GUI plots reuse it. Updates validate
equal x/y lengths and finite values before replacing either array; malformed
updates preserve the prior plot. Each plot has its own labels and data. Nyquist
plots can request `equal_aspect=true`.

`qu gui file.qu` is a host transport: it emits a JSON snapshot, then reads one
JSON event per stdin line and emits an updated snapshot. It does not itself
open an operating-system window. Script output is carried inside packets.
Hosts serialize events, render text as text, and terminate the child on Stop.

This first implementation is a code-defined GUI runtime. Timers, asynchronous
callbacks, and standalone window packaging still require additional work. Long
serial reads should remain on the native acquisition worker, rather than
inside a GUI callback.

**A drag-and-drop editor now exists** (QuStudio's "Designer" tab,
`qu-ui-components/src/components/GuiDesigner.tsx`) — see
`docs/design/qu-gui-designer-spec.md` for its design. It targets this same
API one-way: place widgets visually, it generates the `Frame`/`.add`/`.on`
lines above, hand that source to `GuiPanel`'s existing runtime to actually
run. Source-preserving round trips (reading a hand-edited script back into
the designer's tree) remain future work — the spec explains why that is a
separate, larger project (no AST-preserving unparser exists for Qu yet).
