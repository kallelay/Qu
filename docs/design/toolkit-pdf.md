# PDF toolkit — design notes and roadmap

Ahmed's own brainstorm (2026-09-18), captured here in full rather than only
in chat history, so it survives as a real design reference — same pattern
as [toolkit-file.md](toolkit-file.md), [toolkit-image.md](toolkit-image.md),
[toolkit-text.md](toolkit-text.md). Likely belongs alongside those in
Ahmed's broader "Utopic ... Specification" (`specs.md`) too, since it's the
same style of brainstorm as the one that produced toolkit-file.md — not
merged in here since this session doesn't have visibility into that file's
current state.

**Status**: pure roadmap. Not scoped to any version, not started. Ahmed's
own framing: "In one version of the roadmap we need pdf toolkit." Recorded
here per the same "capture as a todo list, don't build it under time
pressure" instruction that shaped toolkit-file.md.

**A note on the pseudocode below**: it's written in Rust-flavored camelCase
(`pdf.pageCount()`, `pdf.addPage()`) because it doubles as a sketch of the
underlying Rust crate wiring, not as literal Qu syntax. Whoever picks this
up needs to translate the API surface to Qu's actual conventions before
building anything — snake_case builtins, UFCS dot-call dispatch (`x.f(y)`
desugars to `f(x, y)` for any free function, confirmed general-purpose this
session), kwargs via `style_num_checked`/`style_str` (not positional
optional args), and `Units::Mm`/`Units::Pt`-style enums become string
kwargs (`unit="mm"`) matching how the rest of the stdlib takes unit
arguments. So `pdf.rotatePage(3, 90)` becomes `rotate_page(pdf, 3, 90)` /
`pdf.rotate_page(3, 90)`, `pdf.pageCount()` becomes `page_count(pdf)`, etc.
None of the examples below have been through that translation.

**On the external-crate research**: version numbers and crate capabilities
below are "as of September 18, 2026," per Ahmed's own note — re-verify
before actually building on any of them (crates move fast, this is a
snapshot not a guarantee), same caution as
[[qu-record-the-probe]]/[[qu-validated-against-what]]. Also worth flagging
up front: `qu-core`'s own Cargo.toml states it is "dependency-free by
design for the M0–M2 core... std only" — none of `lopdf`/`pdf-manip`/
`pdfium-render`/`mupdf`/`ocrs` belong in `qu-core`. This is a `qu-interp`-
or-later (or even a separate `qu-pdf` crate) concern, and several of the
candidate backends need a linked native library at runtime (PDFium, MuPDF),
which is a real distribution/packaging cost — worth weighing against
`lopdf`+`pdf-manip` (pure Rust, no native dependency) as a leaner first cut
even if it means giving up rendering/OCR/forms/redaction until a second
pass.

---

## Why PDF needs its own semantic layer

Treating a PDF as opaque binary data makes even simple operations
unnecessarily painful. A PDF is effectively:

```text
PDF
├── document/catalog
├── pages
│   ├── content streams
│   ├── text/glyphs
│   ├── vector paths
│   ├── images
│   ├── fonts
│   ├── annotations
│   └── form widgets
├── bookmarks/outlines
├── links
├── attachments
├── forms
├── signatures
├── metadata/XMP
├── security
└── object/xref graph
```

The catch: PDF text is usually *not* stored as editable paragraphs. It's
often a sequence of positioned glyphs — "draw glyph X at coordinate
`(321, 418)`." So `pdf.replaceText()` can be made to feel intuitive, but
under the hood it's much harder than `text.replace()`.

## Top-level shape

```text
Pdf::open("paper.pdf")
pdf.info()
pdf.pages()
pdf.metadata()
pdf.save("output.pdf")
```

Semantic hierarchy:

```text
Pdf
├── pages
├── text
├── images
├── vectors
├── annotations
├── forms
├── bookmarks
├── links
├── attachments
├── signatures
├── metadata
├── security
├── optimize
├── accessibility
├── archival
├── render
├── ocr
└── lowLevel
```

## Document and page manipulation

```text
pdf.pageCount()

pdf.addPage()
pdf.insertPage(3)
pdf.deletePage(5)

pdf.extractPages(1..=5)
pdf.copyPages(...)

pdf.movePage(5, 2)
pdf.reorderPages(...)
pdf.reversePages()

pdf.rotatePage(3, 90)
pdf.rotatePages(...)

pdf.cropPage(...)
pdf.resizePage(...)

pdf.duplicatePage(...)
```

Multiple documents:

```text
Pdf::merge(["part1.pdf", "part2.pdf", "part3.pdf"])

pdf.splitEvery(10)
pdf.splitAt([5, 20, 35])
pdf.splitByBookmarks()
```

More advanced: `pdf.nUp(2)` / `pdf.nUp(4)`, `pdf.booklet()`,
`pdf.insertBlankPage(...)`.

## Page geometry

```text
page.mediaBox() / cropBox() / bleedBox() / trimBox() / artBox()
page.setCropBox(...)
page.setRotation(...)
page.width() / page.height()
```

PDF coordinates should be abstracted away: `page.point(x, y)`,
`page.rect(x, y, width, height)`, with `Units::Mm`/`Units::Pt`/
`Units::Inch` instead of forcing everything into raw PDF points.

## Text

Extraction: `pdf.extractText()`, `page.extractText()`, `page.words()`,
`page.characters()`, `page.textBlocks()`, `page.textRuns()` — with
geometry (`word.text()`, `word.boundingBox()`).

Search: `pdf.findText("impedance spectroscopy")`,
`pdf.findText("battery", caseSensitive=false)`.

Semantic replacement: `pdf.replaceText("2025", "2026")`, or region-scoped
`page.replaceTextAt(region, "New text")`. Adding text:
`page.addText("Confidential", position)`, with `text.font(...)`,
`text.fontSize(...)`, `text.color(...)`, `text.bold()`, `text.italic()`,
`text.rotate(...)`.

The utopic (hard) case: `pdf.editParagraph(selector, "Completely
rewritten paragraph")` — genuinely harder, likely needs layout
reconstruction/reflow, not just glyph repositioning.

## Images

`page.images()`, `page.extractImage(id)`, `pdf.extractImages()`.
Manipulation: `page.addImage(...)`, `page.replaceImage(id, newImage)`,
`page.removeImage(id)`, `image.move/resize/rotate(...)`. Compression:
`pdf.downsampleImages(150_dpi)`, `pdf.compressImages(quality=85)`.

## Vector graphics

Should mirror whatever object model the SVG toolkit uses: `page.paths()`,
`page.shapes()`, `page.lines()`, `page.rectangles()`, `page.curves()`, with
`object.moveBy/scale/rotate(...)`, `object.setFill/setStroke(...)`,
`object.remove()`, and drawing primitives `page.drawLine/drawRectangle/
drawCircle/drawPath(...)`. Makes PDF annotation/technical drawing much
easier.

## Page objects (lower-level)

`page.objects()` returning `TextObject`/`ImageObject`/`PathObject`/
`FormObject`/`ShadingObject`, each with `boundingBox()`, `transform()`,
`hide()`, `remove()`, `clone()`. This is roughly where PDFium-backed
libraries (`pdfium-render`) earn their keep — page-object introspection and
transformation, plus text/path/bitmap object creation.

## Annotations

`page.annotations()`. Add: `highlight/underline/strikeOut(...)`,
`addComment(...)`, `addFreeText(...)`, `addStamp(...)`, `addLink(...)`,
shape annotations (`addRectangleAnnotation`/`addCircleAnnotation`/
`addLineAnnotation`). Remove: `annotation.delete()`. Flatten:
`pdf.flattenAnnotations()`.

## Redaction — deserves special treatment

`page.drawBlackRectangle(area)` is **not** redaction — the underlying text
is still extractable. The correct API needs to actually remove the
content:

```text
page.redact(area)
page.applyRedactions()
# or:
pdf.redactText("secret")
```

MuPDF's Rust bindings are the relevant prior art here: `PdfPage` exposes
redaction annotations plus `apply_redactions()`/
`apply_redactions_with_options()` — i.e. actually applying the redaction to
page content, not just painting over it. For a privacy-oriented API,
`overlay()` and `redact()` should be two very explicitly distinct
operations, not two names for the same unsafe thing.

## Forms

`pdf.forms()`, `pdf.formFields()`, `pdf.field("first_name")` →
`field.name/type/value()`, `field.setValue("Ahmed")`. Field types: Text,
Checkbox, Radio, Dropdown, List, Button, Signature. Convenience:
`pdf.fillForm({"name": "Ahmed", "university": "TU Chemnitz"})`. Flatten:
`pdf.flattenForm()`. `pdfium-render` already supports form-field
introspection and filling.

## Bookmarks / outlines

`pdf.bookmarks()`, `pdf.addBookmark("Introduction", page=1)`,
`removeBookmark/renameBookmark/moveBookmark(...)`, and automatically:
`pdf.generateBookmarksFromHeadings()`.

## Links

`page.links()`, `page.addLink(area, "https://...")`,
`page.addInternalLink(area, page=15)`, `link.remove()`, plus
`pdf.findBrokenLinks()`.

## Attachments

`pdf.attachments()`, `pdf.addAttachment("data.csv")`,
`pdf.extractAttachment(...)`, `pdf.removeAttachment(...)`.

## Metadata

`pdf.metadata()`, `pdf.title/author/subject/keywords()`,
`pdf.setTitle/setAuthor(...)`. XMP: `pdf.xmp()`, `pdf.setXmp(...)`. Clean:
`pdf.stripMetadata()`.

## Security

`pdf.isEncrypted()`, `pdf.encrypt(password)` / `pdf.decrypt(password)`, or
more explicitly `pdf.encrypt(ownerPassword, userPassword,
algorithm=AES256)`. Permissions: `pdf.permissions()`,
`pdf.allowPrinting/allowCopying/allowEditing/allowAnnotations(bool)`.
`pdf-manip` already provides encryption/decryption with AES-256 alongside
page manipulation.

## Digital signatures

`pdf.sign(...)`, `pdf.signatures()`, `pdf.verifySignatures()`, with
`signature.signer/time/certificate/validity/coveredRevision()`. Worth
keeping as its own subsystem — certificates, incremental PDF updates, and
byte-range signatures are a genuinely separate problem from page
manipulation.

## OCR

`pdf.isScanned()` → `pdf.ocr()` or `pdf.ocr(languages=["eng", "deu"])`.
Goal: original scanned image + an invisible searchable text layer.
Pipeline shape: PDF renderer → page bitmap → OCR engine → recognized
words + bounding boxes → new invisible text layer. `ocrs` is a native-Rust
OCR option (no external OCR engine dependency).

## Rendering

`page.render()`, `page.render(dpi=200)` or `page.render(width=2000)`.
Export: `page.toPng()`, `page.toJpeg()`, `pdf.renderPages()`. Useful even
for pure editing work as a verification loop: render original → edit →
save → reopen → render edited → compare. Catches broken fonts, wrong
coordinates, invisible objects, clipping, malformed content streams —
things a "no error was thrown" check would miss entirely. (Same spirit as
this session's own [[qu-checks-that-never-reached-their-subject]] lesson:
a save that doesn't error isn't proof the PDF is still correct.)

## Comparison

`pdfA.compare(pdfB)`, `pdfA.structuralDiff(pdfB)`, `pdfA.textDiff(pdfB)`,
`pdfA.visualDiff(pdfB)`, with `diff.changedPages()`/
`diff.changedRegions()`.

## Optimization

`pdf.optimize()`, `pdf.compressStreams()`, `pdf.deduplicateObjects()`,
`pdf.downsampleImages()`, `pdf.removeUnusedObjects/removeUnusedFonts()`,
`pdf.subsetFonts()`, and `pdf.linearize()` for fast web viewing.
`pdf-manip` already covers optimization, image downsampling, font
subsetting, and object-level cleanup.

## Repair and validation

`pdf.validate()` reporting xref errors, broken references, invalid
streams, missing fonts, bad page tree, unsupported encryption, malformed
metadata; `pdf.repair()`; `pdf.rebuildXref()` where appropriate.

## PDF/A (archival)

`pdf.convertToPdfA()`, `pdf.convertToPdfA("A-2b")`,
`pdf.validatePdfA()`. `pdf-manip` already has a PDF/A pipeline defaulting
to PDF/A-2b, plus font/XMP/structure/color-space remediation helpers.

## PDF/UA and accessibility

`pdf.accessibilityReport()`, `pdf.tagStructure()`,
`pdf.setReadingOrder()`, `image.setAltText(...)`, `table.markAsTable()`,
`heading.markLevel(2)`, `pdf.convertToPdfUa()`, `pdf.validatePdfUa()`.
Full accessibility remediation is a substantially harder problem than
ordinary page manipulation — `pdf-manip` has PDF/UA remediation helpers
but this is the part most likely to need real, non-mechanical work.

---

## Candidate Rust crates (snapshot, 2026-09-18 — re-verify before building)

| Crate | Best role | Main tradeoff |
|---|---|---|
| `lopdf` 0.45 | Raw/object-level PDF manipulation | Need to understand PDF internals; pure Rust, no native dep; needs Rust 1.85+ |
| `pdf-manip` 1.0.0-beta.18 | High-level page/edit/security/optimization ops (merge/split/extract/delete/insert/rearrange/rotate/crop, AES-256 encryption, watermarks, bookmarks, text edit/replace, headers/footers, image insertion, optimization, PDF/A + PDF/UA tooling) | Still beta; pure Rust, no native dep |
| `pdfium-render` 0.9.4 | Rendering, extraction, forms, page objects, attachments, annotations, document creation | Requires PDFium native library linked/loaded at runtime |
| `mupdf` 0.8 | Very broad read/render/edit engine; best redaction story (`apply_redactions`) | Native MuPDF dependency |
| `printpdf` 0.12.8 | PDF generation/content creation | Better suited to creation than arbitrary surgery on existing PDFs |
| `ocrs` 0.13.1 | OCR | OCR only, needs rendered page images as input |

### Preferred architecture: don't force one backend to do everything

```text
                ┌───────────────┐
                │ Your nice API │
                └───────┬───────┘
                        │
        ┌───────────────┼────────────────┐
        │               │                │
        ▼               ▼                ▼
 structural         rendering        OCR / AI
 manipulation       / semantic
        │               │                │
 lopdf             PDFium           ocrs
   +                  or
 pdf-manip          MuPDF
```

Public API stays backend-agnostic:

```text
pdf = Pdf::open("input.pdf")
pdf.pages().delete(5).rotate(2, 90)
pdf.replaceText("old", "new")
pdf.save("output.pdf")
pdf.verify()
```

The caller doesn't need to know whether a given operation used `lopdf`,
PDFium, or MuPDF underneath.

A leaner first cut worth considering, given `qu-core`'s no-native-deps
norm: ship structural editing (`lopdf` + `pdf-manip`, pure Rust, no linked
library) first, and treat rendering/OCR/forms/redaction (which need
PDFium, MuPDF, or `ocrs`) as an optional second phase — mirrors how
`qu-interp`'s own `gpu`/`llm`/`h5-models` features are already off-by-
default, opt-in Cargo features rather than always-on dependencies.
