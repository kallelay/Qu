# FigureViewer: current state vs. "perfect", and what's actually left

Audit pass, 2026-09-16, for Ahmed's "also work on the perfect Graphic/
figure view." Scope: the **interactive viewer** — `qu-ui-components/src/
components/FigureViewer.tsx` + `utils/useFigureTools.ts` — not the static
rendering engine (`engine/crates/qu-interp/src/plotting.rs`), which already
has its own much larger research pass at `docs/design/figure-quality.md`
(ticks, legends-as-drawn, colour, theme architecture — read that for
rendering quality; this document is about the tool you view/annotate/export
a figure with).

`BACKLOG.md`'s "`FigureViewer`'s static-SVG lightbox needs real tools:
zoom, pan, save, or at least annotate" (2026-09-05) is **stale**. All four
are implemented, and the viewer goes considerably further than that entry
asked for. Confirmed by reading the component directly, not by re-running
the backlog's own bug report.

## What's already built (verified in source, not assumed)

- **Pan** — pointer-capture drag when zoomed (`FigureViewer.tsx`'s
  `canvasGestures`), plus double-right-click to return to the panel (a
  deliberate choice: Plotly uses double-click for "reset zoom", so the
  right button keeps "go back" from colliding with "fit to view").
- **Zoom** — mouse wheel, anchored under the pointer so the thing you were
  looking at stays put rather than walking off screen as you zoom (the
  `fx`/`fy` fraction math in the wheel handler); +/-/0 keyboard shortcuts;
  explicit zoom-out/fit/zoom-in buttons; Shift+wheel stays a plain
  horizontal scroll.
- **Save** — per-figure Download (SVG, or the annotated/renamed SVG when
  edited, falling back to the original bytes byte-for-byte when untouched)
  and a whole-collection Save as SVG or PNG-at-3x (`onSaveCollection`).
- **Annotate** — one merged tool (not three): click labels a point, double-
  click marks it with its own coordinates, drag draws an arrow with text.
  Snap-to-data-point is on by default (`useFigureTools`'s `collectPoints`/
  `withSnap`, reading vertices straight out of the rendered SVG, legend
  swatches excluded) and toggleable.
- **Beyond the backlog ask**: free text placement; select-and-retype for any
  text node in the figure (title, axis labels, legend entries — legend
  boxes auto-reflow to fit a renamed entry, `reflowLegendBoxes`); vertical/
  horizontal reference lines; a draggable shaded x-span; a draggable
  magnifying inset rectangle; a live readout (value under cursor, "on a
  data point" indicator) while annotating; thumbnails with lazy rasterized
  previews for the panel, full inline SVG (so tooltips still work) in
  fullscreen; screen/publication theme toggle that writes into the
  **script**, not the rendered image, so the choice survives a re-run.
- **The thing that matters most**: every placed shape/annotation carries
  its own generated Qu line (`shapeCode`), shown before insertion, with an
  explicit "Insert into script" button — nothing exists only in the viewer.
  This is the single strongest idea in the file and should be the template
  for the GUI designer (`docs/design/qu-gui-designer-spec.md` §1 adopts it
  directly).

Against the two named references (Origin, Igor Pro — see below), the gap is
narrow, not wide.

## Reference points

**Igor Pro.** Four annotation types (textboxes, legends, colour scales,
tags) — Qu's annotate/text/point/arrow set already covers the same ground
under different names, plus vline/hline/xspan/inset that Igor's simpler
model doesn't have. Where Igor still leads: **tags stay attached to a wave
and update if the data changes**; Qu's annotations are positions in data
space fixed at placement time, which is the right choice for "this line of
code reproduces this mark" but means an annotation doesn't know it was
"about" a specific sample if the underlying data is later regenerated. Not
worth chasing — the code-generation model is deliberately about the value,
not a live binding, and matches how the rest of Qu treats figures (redraw
from source, don't mutate a picture in place).

**Origin.** Strongest in two areas Qu doesn't touch yet: **legend
interactivity** (click a legend entry to hide/show that series without
touching the script) and **linked/synced axes across a multi-panel
figure** (pan one subplot, linked ones follow). Both are concrete,
scoped gaps — see the ranked list below.

## Gaps, ranked

1. **Legend click-to-isolate.** Click a legend swatch to toggle that
   series' visibility in the *viewer* (not the underlying data — a display
   toggle, the same category as pan/zoom, not a code-generating action).
   High value for figures with 6+ series (`09_six_long_legend_entries.svg`
   in the reference collection is exactly this case) and Origin/most modern
   plotting UIs treat it as table-stakes. Needs a `visibility:hidden` (not
   `display:none`, so it doesn't reflow the legend) toggle on the matching
   series' SVG group, keyed the same way `collectPoints` already excludes
   legend swatches (`rect[rx='6']`) — read the series index from the
   legend row's position, hide the corresponding `<polyline>`/`<circle>`
   group. Purely a viewer convenience; explicitly does **not** generate
   code (there is no Qu builtin for "series N is off" mid-script — this
   would need one if it were meant to persist, which is out of scope here).
2. **Single-figure export size/format choice.** The collection-level Save
   already offers SVG/PNG-at-3x (`onSaveCollection`); the per-figure
   Download button does not — it always ships the native format at native
   resolution. A small popover (SVG / PNG @1x/2x/3x) on the existing
   Download button, reusing whatever rasterization `onSaveCollection`'s PNG
   path already does at the App.tsx host level, closes this asymmetry
   cheaply.
3. **Measurement between two points.** The annotate tool reads one point at
   a time. A lightweight "measure" mode (click two points, show Δx/Δy and,
   for a log axis, the ratio) is a common Origin/Igor workflow (reading a
   peak-to-peak distance, a decade span) that the current single-point
   readout can't do without manual subtraction. Smaller than item 1;
   implement after it.
4. **Keyboard-shortcut discoverability.** +/-/0 (zoom) and arrow keys
   (prev/next figure) already work but are undiscoverable — nothing in the
   UI names them except the fullscreen footer's one sentence ("Wheel to
   zoom · drag to pan · 0 to fit..."). A `?`-triggered small overlay
   listing all shortcuts (a well-worn, cheap pattern) would help; low
   priority since the footer hint already covers the essentials.
5. **Multi-panel linked pan/zoom.** Explicitly **not recommended** for this
   pass. `board2.txt` records Ahmed's own framing of this exact idea as
   unresolved between two different shapes — "(a) linked-panel/subplot axes
   vs. (b) Illustrator-style collective graphic transform" — flagged in his
   own words as a brainstorm, not a spec. Don't guess at which; ask before
   building either.

## Explicitly not proposed

- A rebuild of any kind. The existing tool is already close to the
  Origin/Igor bar; nothing here is a rewrite.
- Legend click-to-isolate generating code — it's a display toggle, not a
  figure edit; conflating the two would violate the file's own "nothing
  exists only in the viewer" rule by creating an exception that fakes it.
- Live-updating annotations bound to re-run data (Igor's "tags," above) —
  contradicts the code-generation model on purpose.

## Suggested next step

Implement #1 (legend click-to-isolate) and #2 (export size/format) directly
in `FigureViewer.tsx`/`useFigureTools.ts`, following the file's existing
patterns (`collectPoints`'s legend-box detection, the code-comment density
and reasoning style already used throughout). Both are additive, low-risk,
and don't touch the rendering engine or any other session's likely area of
work.
