# Office toolkit (DOCX/PPTX/XLSX) — design notes and roadmap

Ahmed's own brainstorm (2026-09-18), captured here in full rather than only
in chat history — same pattern as [toolkit-pdf.md](toolkit-pdf.md)
(written earlier the same session) and
[toolkit-file.md](toolkit-file.md)/[toolkit-image.md](toolkit-image.md)/
[toolkit-text.md](toolkit-text.md). Likely belongs alongside those in
Ahmed's broader "Utopic ... Specification" (`specs.md`) too — this session
doesn't have visibility into that file's current state, so not merged in
here.

**Status**: pure roadmap for DOCX and PPTX (no crate exists yet). XLSX is
partially shipped already — see "What's already real" below — and this
doc is mostly roadmap for XLSX too, since what's shipped covers only bulk
read/write, not the rich cell/formula/chart API sketched here.

**Ahmed's own framing, and why it's right**: "we probably need to have
them as lib files not core!" This matches the repo's own standing policy
almost word for word — `IMPL.md` §7 ("Dependency and performance policy"):
*"The semantic core remains small and deterministic... Add a dependency
only at the milestone that needs it and behind a feature when a
minimal/WASM build should not pay for it."* Office formats are a textbook
case: three separate, fairly heavy OOXML dependency trees (`docx-rs`+,
`pptx`+, `calamine`+`rust_xlsxwriter`+`umya-spreadsheet`) that the large
majority of Qu programs (signal processing, ML, plotting, EIS) will never
touch. None of this belongs in `qu-core` (the dependency-free numerics
library) or as an always-on dependency of `qu-interp`.

## What's already real (don't re-derive from scratch)

`engine/crates/qu-xlsx` already exists — "Excel .xlsx reading and writing
for Qu, as an importable module" (its own `Cargo.toml` description),
gated behind `qu-interp`'s `xlsx` feature and forwarded through `qu-cli`'s
own `xlsx` feature (`xlsx = ["qu-interp/xlsx"]`) — this is the exact
"lib not core, opt-in feature" pattern Ahmed is asking for, already
proven out by three other optional features the same way (`gpu`,
`backend-torch`, `h5-models`, per `IMPL.md`'s own note on `h5-models`:
*"'call vendor libraries for a real FORMAT, don't hand-roll it' policy...
Optional, default-off (`--features h5-models`)... confirmed the default
build [doesn't pay for it]"*). Office support should be three more crates
in this same family, not a departure from it.

Today `qu-xlsx` (267 lines) is genuinely minimal: `sheet_names(path)`,
`read_sheet(path, sheet, headers)` → a whole sheet as a table, and
`write_sheet(...)` → write a whole sheet from data. Backed by `calamine`
(reader, also covers `.xls`/`.xlsb`/`.ods`) for reading and
`rust_xlsxwriter` for writing — both already Cargo dependencies, already
vetted. No cell-level access (`sheet["A1"]`), no formulas, no charts, no
formatting, no ranges, no conditional formatting, no named ranges, and
critically: `rust_xlsxwriter` is write-only, so today's `write_sheet`
creates a new workbook, it cannot modify one cell of an existing `.xlsx`
in place. Everything else in this doc's XLSX section is the roadmap on
top of that real foundation, not a description of what exists.

No `qu-docx` or `qu-pptx` crate exists at all yet — those sections below
are 100% net-new.

## A note on the pseudocode below

Same caveat as `toolkit-pdf.md`: written in Rust-flavored `snake_case`
method-call style as a sketch of the underlying Rust crate wiring, not
literal Qu syntax. Translate to Qu's actual conventions before building
anything — UFCS dot-call dispatch (`x.f(y)` desugars to `f(x, y)` for any
free function, confirmed general-purpose this session), kwargs via
`style_num_checked`/`style_str` rather than positional optional
arguments, and unit values (`20_mm`, `28_pt`) become Qu's existing
`Value::Unit` machinery (`20 mm`, already a real, parsed unit literal —
see `docs/design/physical-units.md`) rather than a new numeric-literal
suffix syntax. `sheet["A1"] = "Frequency"` (Rust indexing-assignment
sugar) has no direct Qu equivalent yet either — check
`docs/design/toolkit-signal.md`'s frequency/time indexing work (`X[440
Hz]`, `s[1 s : 2 s]`, shipped this session on `claude/signal-indexing`)
for the nearest precedent before inventing a new indexing form for cells.

## Why DOCX/PPTX/XLSX need their own semantic layer

All three are OOXML ZIP packages (XML + relationships + media + themes +
metadata + embedded objects) under the hood, but the right mental model
is completely different per format:

```text
OfficeDocument
├── Docx   -- flowed text: paragraphs, runs, styles, tracked changes
├── Pptx   -- a vector-graphics canvas per slide: shapes, positions, animation
└── Xlsx   -- a grid: cells, formulas, ranges, charts
       ↓
OOXML Package
├── ZIP parts
├── XML
├── relationships
├── media
├── themes
├── metadata
└── embedded objects
```

## DOCX — Word documents

Semantic hierarchy:

```text
Docx
├── sections
├── paragraphs
│   ├── runs
│   ├── text
│   └── formatting
├── headings
├── lists
├── tables
├── images
├── shapes
├── equations
├── hyperlinks
├── bookmarks
├── comments
├── footnotes
├── endnotes
├── headers
├── footers
├── fields
├── styles
├── numbering
├── references
├── trackedChanges
├── metadata
└── lowLevelOOXML
```

Core operations:

```text
doc = Docx::open("paper.docx")

doc.replace_text("2025", "2026")
doc.paragraph(4).set_text("New paragraph text")
doc.add_heading("Results", 1)
doc.add_paragraph("The measured impedance was ...")
doc.add_image("figure.png")
doc.table(0).cell(2, 3).set_text("42.3")
doc.save("paper_modified.docx")
```

Text: `doc.text()`, `doc.find_text(...)`, `doc.replace_text(...)`,
`doc.paragraphs()`, `doc.headings()`,
`doc.insert_paragraph/remove_paragraph/move_paragraph(...)`.

Formatting: `run.bold(true)`, `run.italic(true)`, `run.font_size(12 pt)`,
`run.font("Arial")`, `run.color("#000000")`,
`paragraph.alignment(Justified)`, `paragraph.line_spacing(1.5)`.

Tables: `doc.tables()`, `table.rows/columns()`,
`table.add_row/add_column()`, `table.cell(row, col)`,
`table.merge_cells/split_cell(...)`, `table.auto_fit()`.

Sections/pages: `doc.sections()`, `section.orientation(Landscape)`,
`section.page_size(A4)`, `section.margin_top/margin_bottom(20 mm)`,
`section.add_page_break()`. Headers/footers:
`section.header().set_text(...)`, `section.add_page_number()`.

Academic-document operations (the ones most relevant to Ahmed's own use
case — thesis/paper editing): `doc.comments()`, `doc.footnotes()`,
`doc.endnotes()`, `doc.bookmarks()`, `doc.cross_references()`,
`doc.fields()`, `doc.table_of_contents()`, `doc.track_changes()`,
`doc.accept_changes()`, `doc.reject_changes()`.

### Candidate backend and the safety split that matters most

`docx-rs` 0.4.22 can generate DOCX and has a reader, but arbitrary
read-edit-write of an existing, complex Word document is the hard case —
a documented risk from another Rust project integrating it: rebuilding
from its parsed representation can lose unsupported OOXML constructs.
`office_oxide` 0.1.11's DOCX editing layer takes the safer approach
deliberately: it preserves the package and performs targeted
modifications rather than reconstructing the whole document; its
`EditableDocx` currently supports in-place-style text replacement while
preserving unrelated OOXML parts.

This is the single most important architectural decision for DOCX, and it
generalizes to PPTX/XLSX too:

```text
safe semantic operation
       ↓
can modify parsed model safely?
   ↙              ↘
 yes               no
 ↓                  ↓
semantic edit    surgical XML edit
                      ↓
                 preserve unknown parts
```

Preserve the original OOXML package by default. Editing one paragraph,
one cell, one textbox should change the minimum necessary XML and leave
unknown extensions, embedded files, macros, custom XML, themes,
relationships, and vendor-specific data untouched wherever possible —
the difference between a toy Office library and a genuinely robust file
tool. `office_oxide`'s targeted-edit approach is the right shape to build
on for exactly this reason, even though it's early (0.1.x).

## PPTX — PowerPoint

Completely different mental model from DOCX — a vector-graphics canvas
per slide:

```text
Presentation
├── slides
│   ├── shapes, textBoxes, images, tables, charts, connectors
│   ├── media, groups, notes, animations
├── masters
├── layouts
├── themes
├── sections
├── comments
├── metadata
└── relationships
```

```text
ppt = Pptx::open("lecture.pptx")
slide = ppt.slide_mut(3)
slide.add_text("Impedance Spectroscopy").at(20 mm, 15 mm).size(28 pt).bold()
slide.add_image("eis.png").at(120 mm, 40 mm).size(100 mm, 70 mm)
ppt.save("lecture_new.pptx")
```

Slides: `ppt.slides()`, `add_slide/insert_slide(3)`, `delete_slide(5)`,
`duplicate_slide(2)`, `move_slide(8, 3)`, `hide_slide/unhide_slide(4)`.

Objects: `slide.objects/text_boxes/images/shapes/tables/charts()`,
`slide.find("Impedance")`. Transform: `object.move_to(x, y)`,
`object.resize(w, h)`, `object.rotate(30 deg)`,
`object.align_left/align_center()`,
`object.bring_to_front/send_to_back()`.

Text: `textbox.text/set_text(...)`, `textbox.font_size/font/bold(...)`,
`textbox.align(...)`, `textbox.auto_fit()`.

Shapes: `slide.add_rectangle/add_circle/add_arrow/add_connector(...)`,
`shape.fill/stroke(...)`.

Tables: `slide.add_table(rows, cols)`, `table.cell(r, c).set_text(...)`,
`table.add_row/add_column()`, `table.merge_cells(...)`.

Charts (worth pairing with Qu's own plotting semantics — see below):
`slide.add_chart(Line)`, `chart.set_title(...)`,
`chart.add_series/set_categories(...)`, `chart.legend/axis(...)`.

Masters/themes: `ppt.master/layouts/theme()`, `ppt.set_theme(...)`,
`ppt.apply_layout(...)`. Notes: `slide.notes/set_notes(...)`.

Animations (aspirational — the crate landscape barely covers this):
`object.animate().entrance(Fade).duration(500 ms)`,
`object.animate().after(previous).motion_path(...)`.

### Candidate backend

The `pptx` 0.1.0 crate is pure Rust and already supports opening,
inspecting, modifying, and saving existing presentations — documented
coverage includes slides, shapes, text, tables, charts, SmartArt parsing,
animations, video, notes, embedded fonts, `.pptm`, and
validation/repair. Real example shape:

```rust
let mut ppt = pptx::Presentation::open("input.pptx")?;
let layouts = ppt.slide_layouts()?;
ppt.add_slide(&layouts[0])?;
ppt.move_slide(0, 1)?;
ppt.save("output.pptx")?;
```

Still a 0.1.x ecosystem — put a real abstraction layer in front of it
(same `qu-pptx`-as-importable-module shape as `qu-xlsx`) rather than
exposing its types through Qu's builtin surface directly.

## XLSX — Excel

Richest model of the three:

```text
Workbook
├── worksheets
│   ├── cells, rows, columns, ranges, formulas
│   ├── tables, charts, pivotTables, filters
│   ├── conditionalFormatting, validation, images, comments, sparklines
├── names
├── styles
├── themes
├── connections
├── VBA
└── metadata
```

The target ergonomics — deliberately much simpler than today's
`read_sheet`/`write_sheet` whole-table shape:

```text
wb = Xlsx::open("results.xlsx")
sheet = wb.sheet_mut("Results")
sheet["A1"] = "Frequency"
sheet["B1"] = "Zreal"
sheet["A2"] = 1000.0
wb.save("results_new.xlsx")
```

Ranges: `sheet.range("A1:C20")`. Manipulation:
`sheet.insert_row/delete_row(...)`,
`sheet.insert_column/delete_column(...)`, `sheet.move_range(...)`,
`sheet.clear(...)`.

Formulas: `sheet["C2"].formula("=A2+B2")`,
`sheet.fill_formula("C2:C100", "=A2+B2")`. Higher-level:
`sheet.sum/average/sort/filter(...)`.

Formatting: `sheet["A1"].bold().background(...).center()`,
`sheet.column("A").width(20)`, `sheet.row(1).height(30)`,
`sheet.freeze_panes("A2")`.

Tables: `sheet.create_table("A1:D100")`, `table.name(...)`,
`table.add_total_row()`, `table.filter(...)`.

Conditional formatting:
`sheet.range("C2:C100").conditional_format().greater_than(100).fill(...)`.
Validation: `sheet.range("A2:A100").validation().between(0, 100)`.

Charts — worth exposing scientific-plotting-flavored convenience wrappers
given Ahmed's own domain (EIS/signal work), even though under the hood
it's just an Excel XY chart:

```text
chart = sheet.add_chart(Scatter)
chart.x("A2:A100"); chart.y("B2:B100"); chart.title("Impedance")

# convenience wrappers worth having:
sheet.add_xy_chart(x=frequency, y=magnitude)
sheet.add_nyquist_chart(real=z_real, imag=-z_imag)
```

Named ranges: `wb.define_name("Frequency", "Measurements!$A$2:$A$100")`.
Sheets: `wb.sheets()`, `wb.add_sheet(...)`, `wb.copy_sheet(...)`,
`wb.rename_sheet/delete_sheet/hide_sheet(...)`.

### Candidate backends — a clean three-way split already true today

- **`calamine`** (already a `qu-xlsx` dependency) — best-in-class pure-Rust
  **reader**, also covers `.xls`/`.xlsb`/`.xlsm`/`.xla`/`.xlam`/ODS. Keep
  using it for read-only/data-analytics access.
- **`rust_xlsxwriter`** (already a `qu-xlsx` dependency) — strongest
  option for **creating new** XLSX files: formulas, formatting, charts,
  tables, conditional formatting, validation, images, sparklines, macros,
  Excel 365 functions, defined names. Deliberately write-only — cannot
  modify an existing workbook, which is exactly today's `write_sheet`
  limitation.
- **`umya-spreadsheet`** — the missing piece for **read + modify + save**
  of an existing workbook in place: pure Rust, reads XLSX, supports lazy
  reading for large workbooks, lets you modify structures, writes back
  out. This is the one genuinely new dependency needed to close the gap
  between today's `qu-xlsx` and the cell-level API above.

```text
Read only / data analytics  →  calamine (have it)
Create new XLSX             →  rust_xlsxwriter (have it)
Edit existing XLSX in place →  umya-spreadsheet (need it)
```

## A shared Office abstraction — evaluate before committing to three crates

`office_oxide` 0.1.11 is attempting exactly this: one Rust abstraction
over DOCX/XLSX/PPTX (and legacy DOC/XLS/PPT), with a core module for
common OOXML concepts (OPC packages, relationships, content types,
themes, DrawingML) and a `Document` type that dispatches:

```rust
let document = office_oxide::Document::open("input.docx")?;
document.as_docx(); document.plain_text(); document.to_markdown();
document.save_as(...);
```

Worth evaluating, but per the same reasoning `toolkit-pdf.md` gave for
PDFium/MuPDF: don't bind the whole design to a 0.1.x crate. Build Qu's
own `OfficeDocument`/`Docx`/`Pptx`/`Xlsx` abstraction and treat
`office_oxide` (and everything else here) as a swappable backend choice,
the same way `qu-xlsx` already keeps `calamine`/`rust_xlsxwriter` behind
its own module boundary rather than exposing their types directly.

## Proposed Qu crate layout (mirrors `qu-xlsx`'s existing shape)

```text
engine/crates/
├── qu-xlsx/   (exists — expand: cell/range/formula/chart API, add umya-spreadsheet)
├── qu-docx/   (new — docx-rs and/or office_oxide backend)
└── qu-pptx/   (new — the `pptx` crate as backend)
```

Each wired into `qu-interp` and forwarded through `qu-cli` as its own
opt-in feature (`docx`, `pptx`, alongside the existing `xlsx`), exactly
like `gpu`/`backend-torch`/`h5-models`/`xlsx` today — so a minimal/WASM
build keeps paying nothing for formats a given script never touches, per
`IMPL.md` §7. The public Qu-level API stays consistent across all three
the same way `toolkit-pdf.md` proposed for PDF:

```text
OfficeFile::open("input.docx").word().replace_text("old", "new").save("output.docx")
OfficeFile::open("input.pptx").presentation().slide(4).add_image("figure.png")
OfficeFile::open("input.xlsx").workbook().sheet("Measurements").range("A2:C100").sort_by("A")
```

giving the file toolkit its full intended shape:

```text
File
├── Text
├── Binary
├── Image (Raster / Vector)
├── Signal
├── Pdf
├── Office (Docx / Pptx / Xlsx)
└── ML/Data
```
