# A visual designer for Qu's GUI runtime

Design pass, 2026-09-16, for Ahmed's ask: "something comparable to VB6/
VB.NET forms, or eventually GUIDE/pyqt but for Qu." Companion doc:
`docs/design/native-gui.md`, whose last line already names this as
deferred work — "a drag-and-drop editor... require[s] additional work."
This document is that additional work, scoped.

## 0. What already exists (read before proposing anything)

Qu already has a real, tested, retained-mode GUI runtime — this is not a
green-field feature.

- **`engine/crates/qu-interp/src/gui.rs`.** `t = Frame("Title")` creates a
  root; `t.add(kind, ...props)` appends a child, `kind` ∈ `{panel, label,
  button, slider, number, text, checkbox, plot}`; containers (`frame`,
  `panel`) nest up to 64 deep, 2048 widgets per session. `widget.set(...)`
  updates properties **atomically** (a plot's `x`/`y` validate together, so
  a bad update can't leave mismatched lengths). `widget.on(event, "handler")`
  binds `click` (button), `change` (slider/number/checkbox/text), or `close`
  (frame) to a **previously-declared named function** — anonymous callbacks
  are deliberately not supported (`native-gui.md`: "not introduced as
  incidental parser changes"). `t.show()` makes a frame visible. Layout is
  `layout="row"|"column"|"grid"` on a container — flex/CSS-grid, **no
  freeform x/y placement exists anywhere in the model.**
- **`qu-studio-tauri/src-tauri/src/gui_bridge.rs`.** Spawns `qu gui
  <script>` as a child process, streams line-delimited JSON packets
  (`{gui: {protocol, nodes}, output, error}`) to the frontend over a
  `qu-gui` Tauri event, forwards UI-originated events back over the child's
  stdin. Server-authoritative: the frontend never invents widget state.
- **`qu-studio-tauri/src/GuiPanel.tsx`.** A Monaco editor + Run/Stop, live-
  rendering the returned node tree (`render()`, a `switch` over `node.kind`)
  and dispatching DOM events as `gui_event` calls. This is the **"test run"
  half** of an App-Designer-style tool: it already does everything a
  designer's Run button needs, today, for hand-written source.
- **`engine/examples/gui_signal.qu`** is the one existing example (a slider
  driving a live plot) — used below as the designer's acceptance case.

What's missing, confirmed by grepping the whole repo for `designer|WYSIWYG|
drag-and-drop|DesignSurface|palette|PropertyPanel` (all hits are false
positives — a colour-palette selector, an HTML canvas element): **no visual
placement surface, no property inspector, no code generation from a visual
action.** That's the actual deliverable.

One doc/reality mismatch worth flagging, not fixing here: `docs/
qu-language-spec.md` §47.8 documents a different, more declarative,
apparently-aspirational syntax (`window "..." row slider ... button "..."
on click do ... end end window`) that does not match `gui.rs` at all — no
`knob`/`meter`/`toggle`/`dropdown` keywords exist in the implementation.
Whoever owns spec upkeep should either delete that section or mark it
explicitly as a future direction; a designer must target the *real* API.

## 1. Reference points, and what to take from each

**VB6 / VB.NET WinForms.** The "control tray" (drop a control, it appears
selected with resize handles), the Properties window (grid of name→value,
live-updates the control), and the defining interaction: **double-click a
control in the designer and it drops you into a generated event-handler
stub in the code editor, cursor ready to type.** This is the single most
recognizable "forms designer" behaviour and the one worth reproducing
exactly — it's cheap to build (Qu already requires named handler functions,
so "generate a stub and jump to it" is a strict subset of what the language
already demands) and it's the interaction Ahmed named first.

**MATLAB App Designer — the correct modern reference, not GUIDE.**
Confirmed via research: GUIDE (Java Swing-based, the tool Ahmed's "GUIDE"
mention likely means) was **retired by MathWorks in R2025a** — officially
EOL. App Designer replaced it specifically because GUIDE's `hObject,
eventdata, handles` callback signature and non-object component model
didn't scale to modern (web-based) UI work. App Designer's model: the app
is an object, every component is a property of it, and **callbacks are
generated only on request** (right-click a component → "Callbacks" → pick
an event), not for every component up front. This maps almost exactly onto
Qu's actual API shape (`Frame` + `.add` giving you a handle, `.on(event,
name)` requiring an explicit, named function) — Qu is already
architecturally closer to App Designer than to GUIDE. Lean into that
instead of reinventing a callback model.

**Qt Designer / PyQt Designer.** Produces a separate `.ui` XML file, kept
apart from hand-written code-behind, merged at build/load time. **Deliberately
not adopted here.** Qu has no AST-preserving unparser — there is no way to
read a hand-edited `.qu` script back into a structured form without either
writing a full round-trip parser (a large, separate project the same
`native-gui.md` line calls out as "additional work") or accepting that
hand-edits silently desync the designer's model from the file. The
`.ui`-file approach requires exactly that round trip. Skip it.

**What Qu's own `FigureViewer`/`useFigureTools.ts` already proves works
here.** Every interactive action in the existing figure tools — placing a
vline, dragging an inset, labelling a point — emits **one real, human-
readable line of Qu** at the moment of the action (`vline(2.5, dash=true)`),
handed to the host as `shapeCode` for an explicit "Insert into script"
button. Nothing is retained as hidden drawn-state that only the tool can
interpret. This is the exact right model for a GUI designer too, and it's
already proven, tested, and liked (see `qu-figure-reference-collection`
practice) inside this same codebase. **Adopt it wholesale**: the designer
canvas is a view onto generated source, one direction, always.

## 2. Interaction model

**Canvas = tree, not freeform.** Because `gui.rs` only expresses
row/column/grid containers, "drag a button to pixel (240, 88)" has no
runtime representation — there is nothing to generate. So placement is:
select a container node (starts as the implicit root `Frame`), click (or
drag) a palette entry, it's appended as that container's last child.
Reordering and reparenting happen via an **outline/tree view** alongside
the canvas (standard drag-to-reorder in a tree, well-understood, no new
runtime concept needed) — the live canvas re-renders from the same tree on
every change, so what you see is what `t.show()` will actually produce,
not an approximation.

**Component palette** mirrors `gui.rs`'s `validate()` kind list exactly —
`panel, label, button, slider, number, text, checkbox, plot`, plus the
implicit root `Frame` (created once, not from the palette). No entry the
designer can place should ever be able to fail server-side validation;
palette = ground truth, not a separate wishlist.

**Property inspector**, for the selected node, shows exactly the props
`validate()` accepts for that `kind` — `title`/`text` (string), `layout`
(row/column/grid), `visible`/`disabled`/`equal_aspect` (bool),
`min`/`max`/`value` (slider/number, plus the `min < max`, `min ≤ value ≤
max` constraint `gui.rs` already enforces — mirror it client-side so the
inspector never emits an update `gui.rs` would reject), `value` (checkbox:
bool; text: string), `x`/`y` (plot only, equal-length numeric arrays — for
v1, populate these from a chosen in-scope variable name rather than typing
arrays by hand; see §4). Editing a field regenerates that node's `.set(...)`
/ constructor-argument line live.

**Double-click → handler stub.** Double-clicking a widget that supports an
event (button→click, slider/number/checkbox/text→change, frame→close)
inserts (if absent) a named function stub —

```qu
function button1_click(event)

end function
```

— places the cursor inside it in the paired code editor, and adds the
`.on("click", "button1_click")` line. This is the VB6/App-Designer
"generate and jump" interaction, reproduced faithfully. If a handler by
that name already exists, jump to it instead of duplicating.

**Run** hands the generated source to the *existing* `GuiPanel` runtime —
literally the same `gui_start`/`gui_event`/`gui_stop` Tauri commands — so
the designer never owns a second execution path to keep in sync. See §5 for
the concrete factoring.

## 3. Naming and code shape

Generated names follow `{kind}{n}` (`button1`, `slider1`, `panel1`, ...),
incrementing per kind within the current design — matches VB6's `Command1`/
`Text1` convention, which is exactly this scheme. A name is user-editable
in the inspector (rename propagates to every reference: the assignment, any
`.on(...)` targets, any handler-stub name already generated from the old
one) — cheap now because the designer owns the whole generated block; once
a human hand-edits the script the designer's rename can no longer safely
touch it (see §6).

Reuse, don't reimplement, two small pure helpers already in
`qu-ui-components/src/utils/useFigureTools.ts`:
- `quote(text)` — Qu string-literal escaping, including doubling `{`/`}`
  (Qu string interpolation would otherwise corrupt a label containing a
  brace). Export it if not already exported; a widget's `text=`/`title=`
  needs the identical escaping a figure annotation's label needs.
- `forCode(value)` — 4-significant-figure numeric formatting ("what a
  person would have typed"). Reuse for `min=`/`max=`/`value=`.

Example output for a frame with a row panel holding a slider and a label,
plus a plot reacting to the slider — i.e., designer-built equivalent of
`engine/examples/gui_signal.qu`:

```qu
t = Frame("Signal laboratory")
panel1 = t.add("panel", layout="row")
slider1 = panel1.add("slider", min=0, max=5, value=1)
label1 = panel1.add("label", text="Adjust amplitude")
plot1 = t.add("plot", title="Sine wave", xlabel="Time", ylabel="Amplitude", x=x, y=sin(x))

function slider1_change(event)
    plot1.set(y=event.value * sin(x))
    label1.set(text="Amplitude updated")
end function

slider1.on("change", "slider1_change")
t.show()
```

Indistinguishable in shape from what a person would hand-write — that's
the test for whether the generator is doing its job.

## 4. Known hard edges (write these down now, don't discover them mid-build)

- **Plot data (`x`/`y`) can't be authored visually.** A plot widget's data
  arrays come from script-level variables (`x = linspace(...)`, `y =
  sin(x)`), which exist before the GUI tree is built and aren't themselves
  GUI nodes. v1: the inspector offers a text field for the **variable name**
  to bind (`x=x, y=sin(x)` as raw Qu expressions, inserted verbatim, not
  parsed/validated by the designer) — the designer trusts the script to
  define them, the same way `gui_signal.qu` does by hand. Do not attempt to
  synthesize plot data from the designer; that's a data-analysis surface,
  not a forms-layout one.
- **One-way generation, not round-trip.** Once generated code is inserted
  into the editor and hand-edited (renamed a variable, added a computation
  inside a handler, restructured a panel by hand), the designer's internal
  tree and the file can diverge silently. Mitigation for v1: generation
  always inserts into a clearly delimited block (e.g. a `# --- designer:
  begin ---` / `# --- designer: end ---` comment pair) and the designer
  refuses to re-render from a block whose markers it can't find intact —
  fail loud (a banner: "this section was hand-edited; re-open from source
  not supported yet"), never silently overwrite hand-written code. A real
  round-trip parser is future work, same as `native-gui.md` already says.
- **No freeform layout.** Said above, worth repeating: nothing in `gui.rs`
  can express absolute position or z-order. A user who expects VB6's
  pixel-perfect control tray will be surprised by row/column/grid-only
  layout. This is a real, current runtime limitation, not a designer
  omission — flag it in the UI (e.g. a one-line note in the canvas empty
  state) rather than let people discover it by confusion.
- **Widget-kind gap: no dropdown.** ~~The immediate-mode API already has
  `ui_select` (`lib.rs:16481` area) but `gui.rs`'s retained kind set has no
  `select`.~~ **Closed 2026-09-16.** `select` is a real retained kind now:
  a `validate()` arm for an `options` prop (≤1000 strings), `select` in the
  kind list and in `.on()`'s `change` arm, and a `<select>` case in both
  render switches. One constraint came out of building it that is worth
  knowing: `options` and `value` validate as a **pair** — a value outside
  the list is refused, the same atomicity rule `x`/`y` already have — so a
  UI that edits the option list must re-point `value` in the same update
  (`guiDesign.ts`'s `setOptions` is the one place that does this).

## 5. Implementation shape (frontend-only)

No `qu-interp`/`lib.rs` changes required for the designer itself — everything
it needs already exists and is validated server-side. This matters in a
tree with 20+ concurrent sessions: staying out of `lib.rs`/`gui.rs` avoids
the merge contention those files see constantly (`[[qu-shared-tree-practice]]`).

- `qu-ui-components/src/components/GuiDesigner.tsx` — palette, canvas
  (reusing/factoring `GuiPanel`'s `render()` switch so the designer's
  preview and the real runtime's rendering can't drift apart), outline
  tree, property inspector.
- `qu-ui-components/src/utils/guiCodegen.ts` — pure `designTreeToQu(tree):
  string[]`, sharing `quote`/`forCode` from `useFigureTools.ts`.
- `qu-studio-tauri/src/App.tsx` — new top-level tab (`activeTab` gains
  `'designer'`; a pill button in the existing icon+label row, an authoring
  mode rather than a serial/live-data workflow so it does not belong nested
  under "Live").
- Run button: either embed `GuiPanel` directly, fed the generated source, or
  factor `GuiPanel`'s run-loop into a shared `useGuiRuntime` hook consumed
  by both — do not fork a second `gui_start` caller.

## 6. Explicitly out of scope for v1

- Round-trip re-import of hand-edited designer blocks (§4).
- Freeform/absolute positioning (runtime can't express it; would need a
  `gui.rs` change, a separate, larger proposal).
- New widget kinds beyond `select` (dropdown) — no evidence in `board2.txt`
  of a stated priority for radio/list/image/tabs/menu/progress; add on
  request, not speculatively.
- Standalone window packaging (`native-gui.md` already names this as
  deferred, unrelated to the designer).

## v2, 2026-09-16 — "more advanced, like a VB6/VB.NET forms editor"

Ahmed's follow-up ask. What landed, and the reasoning that is not obvious
from the diff:

- **Properties window** driven by a `PropSpec` table (`propsFor(kind)`)
  rather than a chain of `kind === "..."` guards in JSX, categorized
  Appearance/Behavior/Layout/Data with an A→Z toggle, and one editor per
  prop TYPE (enum dropdown for `layout`, checkbox for bools, a line-per-
  entry list for `options`). The table is the point: a hand-written guard
  chain had already drifted, offering `title` for the frame while offering
  `visible`/`disabled` for nothing at all. Multi-select edits the
  **intersection** of the selected kinds' props, as VB6 does — the union
  would offer `min` for a button and generate a node `gui.rs` refuses.
- **The outline is now load-bearing, not decorative.** Reorder and
  reparent have no freeform-drag equivalent in a row/column/grid runtime,
  so before this there was no way to perform either at all — a widget
  could only be appended and deleted. Drag in the tree or on the form,
  or Ctrl+↑/↓.
- **Selection handles are drawn hollow and inert on purpose.** VB6 itself
  draws them that way when a container governs a control's size, which is
  every control here: there is no width/height prop to write a dragged
  size into. Solid, draggable-looking handles would promise a resize the
  engine cannot express.
- **Undo/redo covers the design tree.** It previously existed only via the
  generated-code textarea, i.e. not for any action a person takes in the
  designer.
- **The root frame is drawn as a window** — a title bar carrying the
  `title` the generated `Frame(...)` actually holds, so the canvas reads
  as a form. Without the three traffic-light dots
  `docs/design/retro-window-chrome.md` pairs with that bar: Ahmed had
  those removed from the TabBar and Terminal the same day (board2,
  quworkspace-a6, "remove these"), and a ruling on the chrome outranks
  the older style reference. Worth knowing before anyone "restores" them
  here for consistency with that doc.
- Copy/paste/duplicate, F2 / double-click rename, Tab and arrow-key
  selection, a grouped and searchable toolbox.

Two things found while building, both fixed here: root props other than
`title` were silently dropped by codegen (a frame's `layout` could be set
in the inspector and never appeared in the output, because `Frame(...)`
takes the title positionally and `gui.rs` overwrites `title`/`visible`
after applying kwargs — they now emit as a following `.set(...)`); and the
designer's actions mutated state inside React state updaters, so two
placements in one tick lost the nesting of the first.

## Acceptance case

Build `engine/examples/gui_signal.qu`'s scenario (amplitude slider driving
a live sine plot, with a status label) **using the designer** instead of by
hand, run it via the existing `GuiPanel` runtime, and confirm dragging the
slider updates the plot — the same behaviour the hand-written example
already has, reached by a different, visual path.
