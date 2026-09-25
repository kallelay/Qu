//! PDF structure, as a Qu module rather than as builtins.
//!
//! `import pdf` then `pdf.page_count(path)`, `pdf.info`, `pdf.merge`,
//! `pdf.extract_pages`, `pdf.extract_text`. Nothing here enters the global
//! builtin table unless a script asks for it by name -- see
//! `MODULE_EXPORTS` in `qu-interp`, and `qu-codec`/`qu-xlsx` for the same
//! split.
//!
//! The crate deals only in plain bytes and plain Rust types: it takes a
//! PDF's bytes and returns counts, strings and more bytes. Turning any of
//! that into a `Value::Record` or a `Value::Vec` is `qu-interp`'s job, so
//! this crate stays testable without an interpreter -- which is how the
//! tests at the bottom can build a PDF, merge it with itself and count the
//! pages of the result with nothing else in the process.
//!
//! # Scope, and what is deliberately absent
//!
//! `docs/design/toolkit-pdf.md` sketches a much larger surface: rendering,
//! OCR, forms, redaction, signatures, PDF/A. All of those need a native
//! library (PDFium, MuPDF) or a rendered bitmap. This crate is the
//! structural half only -- the part `lopdf` does natively, with no linked
//! library -- because that is the part that can ship without changing what
//! it costs to build the engine at all.
//!
//! # A warning about `extract_text`
//!
//! PDF does not store paragraphs. It stores "draw glyph X at (321, 418)",
//! so text extraction is a reconstruction, not a read. `lopdf`'s
//! reconstruction is real but shallow: it concatenates the operands of the
//! text-showing operators in content-stream order. Where a document embeds
//! a subset font with a non-standard encoding and no `ToUnicode` map --
//! which is what most LaTeX/pdfTeX output does -- the bytes in the content
//! stream are glyph indices, not characters, and what comes back is
//! mojibake rather than an error. See `extract_text`'s own doc comment and
//! the `extract_text_on_latex_output_is_not_readable` test, which pins
//! that behaviour rather than pretending it away.

use std::io::Cursor;

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

/// What a PDF says about itself, without extracting anything from it.
///
/// Separate from any content operation because opening the object graph is
/// cheap and reading pages is not -- and because every field here comes
/// from the trailer's `/Info` dictionary or the page tree, both of which a
/// PDF is required to have.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PdfInfo {
    pub page_count: usize,
    /// The header's version string, e.g. `"1.4"`.
    pub version: String,
    /// `None` rather than `""` where the `/Info` dictionary omits the key,
    /// which is the common case: "this PDF does not say" and "this PDF
    /// says the title is empty" are different facts, and an empty string
    /// would merge them.
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    /// The `/Keywords` entry, a single string (often comma- or
    /// space-separated by convention, never split here -- the PDF spec
    /// does not mandate a separator and guessing one would be inventing
    /// structure the file never claimed to have).
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    /// Whether the file carries an `/Encrypt` dictionary. Reported because
    /// it explains, in advance, why the other operations here may fail on
    /// a file that opens fine in a viewer that asked for a password.
    pub encrypted: bool,
}

fn load(bytes: &[u8]) -> Result<Document, String> {
    Document::load_mem(bytes).map_err(|err| format!("not a readable PDF: {err}"))
}

/// Page count, metadata and version in one pass.
pub fn info(bytes: &[u8]) -> Result<PdfInfo, String> {
    let doc = load(bytes)?;
    // `get_pages` walks the page tree, resolving `/Kids` recursively, so
    // this is the real leaf count and not the root `/Count` field -- which
    // a malformed or incrementally-updated file can disagree with.
    let page_count = doc.get_pages().len();

    // The `/Info` dictionary is an indirect reference from the trailer.
    // `get_object` resolves it; a file without one is legal, and every
    // field then stays `None`.
    let info_dict = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|obj| match obj {
            Object::Reference(id) => doc.get_object(*id).ok(),
            other => Some(other),
        })
        .and_then(|obj| obj.as_dict().ok());

    let field = |key: &[u8]| -> Option<String> {
        let obj = info_dict?.get(key).ok()?;
        // `decode_text_string` handles both PDFDocEncoding and the
        // UTF-16BE-with-BOM form the spec also allows; a raw `as_str`
        // would hand back the BOM and NUL-interleaved bytes verbatim.
        let s = lopdf::decode_text_string(obj).ok()?;
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    Ok(PdfInfo {
        page_count,
        version: doc.version.clone(),
        title: field(b"Title"),
        author: field(b"Author"),
        subject: field(b"Subject"),
        keywords: field(b"Keywords"),
        creator: field(b"Creator"),
        producer: field(b"Producer"),
        encrypted: doc.trailer.get(b"Encrypt").is_ok(),
    })
}

/// Just the page count, for the overwhelmingly common case.
///
/// Not a wrapper over `info` for speed reasons -- it is the same parse --
/// but because a caller that wants a number should not have to know the
/// name of a struct field to get one.
pub fn page_count(bytes: &[u8]) -> Result<usize, String> {
    Ok(load(bytes)?.get_pages().len())
}

/// The page numbers a document actually has, in order, one-based.
///
/// Exposed because PDF page numbering is not guaranteed dense: `get_pages`
/// keys by position in the page tree, and a caller validating a
/// user-supplied page list needs to know what is legal.
pub fn page_numbers(bytes: &[u8]) -> Result<Vec<u32>, String> {
    Ok(load(bytes)?.get_pages().into_keys().collect())
}

/// Text reconstructed from the content streams of `pages` (one-based), or
/// of every page when `pages` is `None`.
///
/// **This is the weak one, and it is the reason this crate's doc comment
/// carries a warning.** PDF stores "draw glyph X at (321, 418)", not
/// paragraphs, so extraction is a reconstruction. `lopdf` reconstructs
/// only what the text-showing operators carry, which is real text when the
/// fonts use a standard encoding or ship a `/ToUnicode` map, and nothing
/// at all when they do not.
///
/// Measured on this repository's own 34 committed PDFs (2026-09-23, see
/// `board2.txt`): 22 gave readable text, 12 gave **the empty string** --
/// every `pdfTeX`-produced file among them, plus several Adobe
/// Illustrator and cairo ones. None gave mojibake. That is the failure
/// this function refuses to pass on silently:
///
/// An empty result is returned as `Ok("")` only when the requested pages
/// embed no fonts at all, i.e. when "there is no text here" is the honest
/// answer -- a scanned page, a pure vector figure. When the pages *do*
/// embed fonts and extraction still produced nothing, that is the
/// extractor failing, not the document being empty, and this returns
/// `Err` saying so. A caller then knows to reach for a real text-layer
/// tool instead of concluding the PDF was blank.
///
/// Word and line breaks are approximate even on the documents that work:
/// PDF has no obligation to encode a space character, inter-word spacing
/// is often kerning, and `lopdf` breaks per text-showing operator rather
/// than per word. Treat the output as searchable, not as a transcript.
pub fn extract_text(bytes: &[u8], pages: Option<&[u32]>) -> Result<String, String> {
    let doc = load(bytes)?;
    let page_ids = doc.get_pages();
    let available: Vec<u32> = page_ids.keys().copied().collect();
    let wanted: Vec<u32> = match pages {
        None => available.clone(),
        Some(ps) => {
            for p in ps {
                if !available.contains(p) {
                    return Err(format!(
                        "extract_text: no page {p} -- this document has {}",
                        describe_pages(&available)
                    ));
                }
            }
            ps.to_vec()
        }
    };
    let text = doc
        .extract_text(&wanted)
        .map_err(|err| format!("extract_text: {err}"))?;

    if text.trim().is_empty() {
        // The discriminator. Fonts reachable from the page but no text out
        // means the extractor failed; no fonts reachable at all means the
        // page really has no text (a scanned bitmap, a pure vector
        // drawing) and `""` is the true answer.
        let fonts: usize = wanted
            .iter()
            .filter_map(|p| page_ids.get(p))
            .map(|id| reachable_fonts(&doc, *id))
            .sum();
        if fonts > 0 {
            return Err(format!(
                "extract_text: produced no text, but the requested page(s) can reach \
                 {fonts} embedded font(s) -- so this is the extractor failing on this \
                 document, not an empty document. lopdf reads the page's own content \
                 stream only: it does not descend into Form XObjects (what `pdfcrop` and \
                 Illustrator wrap page content in), and it reads glyphs only from fonts \
                 with a standard encoding or a /ToUnicode map. \
                 page_count/info/merge/extract_pages are unaffected."
            ));
        }
    }
    Ok(text)
}

/// How many embedded fonts a page can reach, descending through Form
/// XObjects.
///
/// Counting only the page's own `/Resources/Font` is not enough and the
/// difference is the whole point: on this repository's `pdfcrop`-produced
/// files the page has zero fonts of its own and one Form XObject that has
/// all of them. A check that stopped at the page would have reported "no
/// fonts, so the empty result is honest" on exactly the documents where
/// the empty result is a failure -- the check would never have reached its
/// subject.
fn reachable_fonts(doc: &Document, page_id: lopdf::ObjectId) -> usize {
    fn as_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
        match obj {
            Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok(),
            other => other.as_dict().ok(),
        }
    }

    fn walk(
        doc: &Document,
        res: &lopdf::Dictionary,
        seen: &mut std::collections::BTreeSet<lopdf::ObjectId>,
        depth: usize,
    ) -> usize {
        // Bounded because a malformed file can make the resource graph
        // cyclic through something other than an object id.
        if depth > 8 {
            return 0;
        }
        let mut n = res
            .get(b"Font")
            .ok()
            .and_then(|o| as_dict(doc, o))
            .map_or(0, |f| f.len());
        if let Some(xo) = res.get(b"XObject").ok().and_then(|o| as_dict(doc, o)) {
            for (_, v) in xo.iter() {
                let id = match v {
                    Object::Reference(id) => *id,
                    _ => continue,
                };
                if !seen.insert(id) {
                    continue;
                }
                let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
                    continue;
                };
                let is_form = matches!(
                    stream.dict.get(b"Subtype").and_then(|o| o.as_name()),
                    Ok(b"Form")
                );
                if !is_form {
                    continue;
                }
                if let Some(inner) = stream.dict.get(b"Resources").ok().and_then(|o| as_dict(doc, o))
                {
                    n += walk(doc, inner, seen, depth + 1);
                }
            }
        }
        n
    }

    let Ok((own, inherited)) = doc.get_page_resources(page_id) else {
        return 0;
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut n = own.map_or(0, |d| walk(doc, d, &mut seen, 0));
    for id in inherited {
        if let Ok(d) = doc.get_dictionary(id) {
            n += walk(doc, d, &mut seen, 0);
        }
    }
    n
}

fn describe_pages(available: &[u32]) -> String {
    match (available.first(), available.last()) {
        (Some(a), Some(b)) if a != b => format!("pages {a}..{b}"),
        (Some(a), _) => format!("only page {a}"),
        _ => "no pages at all".to_string(),
    }
}

/// A new PDF holding just `pages` (one-based) of `bytes`, in the
/// document's own order.
///
/// Implemented by deleting the complement rather than by copying the
/// wanted pages into a fresh document: the pages that stay keep their own
/// resources, fonts and annotations by construction, and `lopdf`'s
/// `delete_pages` already knows how to unhook a page from the page tree
/// and fix `/Count`. Building up from empty would mean re-deriving the
/// resource graph by hand, which is where a page-extractor usually goes
/// wrong (a page that renders blank because its font went missing, and no
/// error anywhere).
///
/// Page order is the document's, not the caller's: `extract_pages(b, &[3,
/// 1])` gives pages 1 and 3 in that order. Reordering is a different
/// operation and is not implemented here.
pub fn extract_pages(bytes: &[u8], pages: &[u32]) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("extract_pages: no pages asked for".to_string());
    }
    let mut doc = load(bytes)?;
    let available: Vec<u32> = doc.get_pages().into_keys().collect();
    for p in pages {
        if !available.contains(p) {
            return Err(format!(
                "extract_pages: no page {p} -- this document has {}",
                describe_pages(&available)
            ));
        }
    }
    let drop: Vec<u32> = available
        .iter()
        .copied()
        .filter(|p| !pages.contains(p))
        .collect();
    doc.delete_pages(&drop);
    // `delete_pages` unhooks pages but leaves their objects behind; the
    // renumber compacts the id space so the saved file is not mostly
    // orphans.
    doc.renumber_objects();
    save(&mut doc)
}

/// Every page of every input, concatenated into one document.
///
/// Follows `lopdf`'s own `examples/merge.rs` recipe: renumber each input's
/// objects into a disjoint id range, collect the page objects separately
/// from everything else, then build one `/Pages` node whose `/Kids` is all
/// of them and one `/Catalog` pointing at it. Bookmarks/outlines are
/// dropped -- that example drops them too, and a half-merged outline tree
/// pointing at pages that moved is worse than none.
pub fn merge(docs: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    if docs.is_empty() {
        return Err("merge: nothing to merge".to_string());
    }

    let mut max_id = 1u32;
    let mut pages = std::collections::BTreeMap::new();
    let mut objects = std::collections::BTreeMap::new();

    for (i, bytes) in docs.iter().enumerate() {
        let mut doc = load(bytes).map_err(|err| format!("merge: input {}: {err}", i + 1))?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for id in doc.get_pages().into_values() {
            if let Ok(obj) = doc.get_object(id) {
                pages.insert(id, obj.to_owned());
            }
        }
        objects.extend(doc.objects);
    }

    if pages.is_empty() {
        return Err("merge: the inputs have no pages between them".to_string());
    }

    // Version: the highest of the inputs would be more correct, but every
    // structural feature this function uses exists in 1.5, and claiming a
    // lower version than the page content needs is the failure that
    // matters. 1.5 is what lopdf's own merge example writes.
    let mut out = Document::with_version("1.5");

    let mut catalog: Option<(lopdf::ObjectId, Object)> = None;
    let mut pages_node: Option<(lopdf::ObjectId, Object)> = None;
    for (id, object) in objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                let keep = catalog.as_ref().map_or(id, |(id, _)| *id);
                catalog = Some((keep, object));
            }
            b"Pages" => {
                if let Ok(dict) = object.as_dict() {
                    let mut dict = dict.clone();
                    if let Some((_, prev)) = pages_node.as_ref() {
                        if let Ok(old) = prev.as_dict() {
                            dict.extend(old);
                        }
                    }
                    let keep = pages_node.as_ref().map_or(id, |(id, _)| *id);
                    pages_node = Some((keep, Object::Dictionary(dict)));
                }
            }
            // Pages are re-parented below; outlines are dropped, see the
            // doc comment.
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                out.objects.insert(id, object);
            }
        }
    }

    let (pages_id, pages_object) =
        pages_node.ok_or_else(|| "merge: no page tree in any input".to_string())?;
    let (catalog_id, catalog_object) =
        catalog.ok_or_else(|| "merge: no catalog in any input".to_string())?;

    for (id, object) in &pages {
        if let Ok(dict) = object.as_dict() {
            let mut dict = dict.clone();
            dict.set("Parent", pages_id);
            out.objects.insert(*id, Object::Dictionary(dict));
        }
    }

    if let Ok(dict) = pages_object.as_dict() {
        let mut dict = dict.clone();
        dict.set("Count", pages.len() as u32);
        dict.set(
            "Kids",
            pages.into_keys().map(Object::Reference).collect::<Vec<_>>(),
        );
        out.objects.insert(pages_id, Object::Dictionary(dict));
    }

    if let Ok(dict) = catalog_object.as_dict() {
        let mut dict = dict.clone();
        dict.set("Pages", pages_id);
        dict.remove(b"Outlines");
        out.objects.insert(catalog_id, Object::Dictionary(dict));
    }

    out.trailer.set("Root", catalog_id);
    // The objects above went in by direct insertion, which does not touch
    // `max_id` -- so it has to be restated before renumbering, or the
    // renumber allocates ids that are already in use.
    out.max_id = out.objects.len() as u32;
    out.renumber_objects();
    save(&mut out)
}

// ── Metadata writes ─────────────────────────────────────────────────────

/// Which `/Info` fields `set_metadata` should touch.
///
/// `None` means "leave whatever is already there" and `Some(s)` means
/// "write `s`" -- the caller not passing a keyword and the caller passing
/// one have to stay distinguishable all the way down to here, or
/// `set_metadata(doc, title="New")` would have no way to avoid clobbering
/// an existing `/Author` it was never told about.
///
/// An empty string is a legal `Some("")`. It gets written to the
/// dictionary like any other value, and then reads back as `None` from
/// `info()` -- `info`'s own `field` closure already treats an empty
/// decoded string the same as an absent key (see its doc comment), so
/// `set_metadata(doc, subject="")` and never touching `/Subject` at all
/// end up indistinguishable on the next `info()` call. That is a
/// pre-existing choice of `info`'s, not a new one made here; documented
/// rather than silently inherited. A caller that wants `/Info` gone
/// entirely wants `strip_metadata`, not an empty string in every field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataEdit {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
}

impl MetadataEdit {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.author.is_none()
            && self.subject.is_none()
            && self.keywords.is_none()
            && self.creator.is_none()
    }
}

/// Write `edit`'s fields into `bytes`'s `/Info` dictionary, creating the
/// dictionary if the file does not have one, and leaving every field
/// `edit` did not mention exactly as it was.
///
/// Everything else about the file -- pages, resources, the rest of the
/// trailer -- passes through untouched; only the `/Info` object changes
/// (or is created).
pub fn set_metadata(bytes: &[u8], edit: &MetadataEdit) -> Result<Vec<u8>, String> {
    if edit.is_empty() {
        return Err(
            "nothing to set -- pass at least one of title=, author=, \
             subject=, keywords=, creator="
                .to_string(),
        );
    }
    let mut doc = load(bytes)?;

    // Find the `/Info` object, or make one. The common case is an
    // indirect reference (what every writer, including this crate's own
    // `merge`, produces); a direct dictionary inline in the trailer is
    // legal PDF but cannot be mutated in place the same way, so it is
    // promoted to an indirect object first -- after which every path
    // converges on "an object id whose dictionary gets mutated".
    let info_id = match doc.trailer.get(b"Info").ok().cloned() {
        Some(Object::Reference(id)) => id,
        Some(direct @ Object::Dictionary(_)) => {
            let id = doc.add_object(direct);
            doc.trailer.set("Info", id);
            id
        }
        _ => {
            let id = doc.add_object(Object::Dictionary(lopdf::Dictionary::new()));
            doc.trailer.set("Info", id);
            id
        }
    };

    let dict = doc
        .get_dictionary_mut(info_id)
        .map_err(|err| format!("could not open /Info: {err}"))?;
    let mut set_field = |key: &str, value: &Option<String>| {
        if let Some(v) = value {
            dict.set(key, Object::string_literal(v.clone()));
        }
    };
    set_field("Title", &edit.title);
    set_field("Author", &edit.author);
    set_field("Subject", &edit.subject);
    set_field("Keywords", &edit.keywords);
    set_field("Creator", &edit.creator);

    save(&mut doc)
}

/// Remove a PDF's `/Info` dictionary entirely, and its catalog's
/// `/Metadata` stream reference if it has one.
///
/// `/Info` is the half this is tested against: every fixture that has
/// one round-trips through `info()` before and after, and `after` is
/// empty. `/Metadata` -- an XMP XML stream some writers (Adobe tools
/// among them) attach to the document catalog in ADDITION to `/Info` --
/// is stripped too when present, on the same "leave nothing behind"
/// reading of "strip metadata", but this crate has no XMP reader of its
/// own, so that half is verified only by confirming the reference is
/// gone and the stream object is no longer reachable from the catalog,
/// not by parsing what the stream said.
pub fn strip_metadata(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    doc.trailer.remove(b"Info");
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.remove(b"Metadata");
    }
    save(&mut doc)
}

// ── Text search ─────────────────────────────────────────────────────────

/// One occurrence of a search query in a document's extracted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    /// One-based, the same numbering every other function here uses.
    pub page: u32,
    /// A CHARACTER offset (not byte, not glyph position) into that page's
    /// `extract_text` output. This is the minimum honest answer: PDF's
    /// content stream gives glyph positions, not character offsets, and
    /// `extract_text` already discards that layout information turning
    /// operators into a plain string -- there is no bounding box left by
    /// the time this function sees the text to hand one back. A caller
    /// that needs on-page coordinates needs a different tool than this
    /// crate's structural, non-rendering approach can provide.
    pub offset: usize,
    /// The length, in characters, of the match -- normally just the
    /// query's own character count, carried here so a caller does not
    /// have to recompute it (and get it wrong under case-insensitive
    /// search, where a casefold can change a character's byte length
    /// even though this crate compares char-for-char, not byte-for-byte).
    pub length: usize,
}

/// Every occurrence of `query` across `bytes`'s pages, via the same
/// `extract_text` this module already ships -- this does not run its own
/// text reconstruction, so it inherits `extract_text`'s exact strengths
/// and the same weak spot (the module doc comment's warning about
/// subset-encoded fonts applies here unchanged: a page `extract_text`
/// cannot read is a page `find_text` cannot search, and errors the same
/// way `extract_text` would).
///
/// Case folding is character-by-character (`char::to_lowercase`), not
/// byte comparison, so it is correct for anything Unicode's simple case
/// folding gets right and untested past that.
pub fn find_text(bytes: &[u8], query: &str, case_sensitive: bool) -> Result<Vec<TextMatch>, String> {
    if query.is_empty() {
        return Err("query is empty -- nothing would ever match".to_string());
    }
    let doc = load(bytes)?;
    let pages = doc.get_pages();
    let query_len = query.chars().count();

    let mut out = Vec::new();
    for &page in pages.keys() {
        let text = extract_text(bytes, Some(&[page]))
            .map_err(|msg| format!("page {page}: {msg}"))?;
        for offset in find_char_offsets(&text, query, case_sensitive) {
            out.push(TextMatch {
                page,
                offset,
                length: query_len,
            });
        }
    }
    Ok(out)
}

// ── Page geometry ────────────────────────────────────────────────────────

/// A page's rectangle in default user space units (1/72 inch, i.e. PDF
/// points) -- the four numbers a `/MediaBox`/`/CropBox` array holds, in
/// their own order: lower-left `(x0, y0)`, upper-right `(x1, y1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// One-based `page` -> its object id, or a named error in the same shape
/// `extract_text`/`extract_pages` already use.
fn find_page(doc: &Document, page: u32, short: &str) -> Result<ObjectId, String> {
    let pages = doc.get_pages();
    match pages.get(&page) {
        Some(id) => Ok(*id),
        None => {
            let available: Vec<u32> = pages.into_keys().collect();
            Err(format!(
                "{short}: no page {page} -- this document has {}",
                describe_pages(&available)
            ))
        }
    }
}

/// The same lookup as `find_page`, but against an already-materialized
/// page order rather than re-walking the tree -- what every tree-editing
/// function below uses, since they all need that `Vec<ObjectId>` anyway
/// to build the new `/Kids` order.
fn validate_page(order: &[ObjectId], page: u32, short: &str) -> Result<usize, String> {
    let n = order.len();
    if page < 1 || page as usize > n {
        let available: Vec<u32> = (1..=n as u32).collect();
        return Err(format!(
            "{short}: no page {page} -- this document has {}",
            describe_pages(&available)
        ));
    }
    Ok(page as usize - 1)
}

/// One PDF number -> `f64`, resolving a level of indirection first (a
/// page box's entries are ordinary numbers in every file this crate has
/// seen, but the spec allows an indirect reference and a reader that
/// assumed otherwise is exactly the kind of silent-wrong-answer bug this
/// codebase hardens against elsewhere).
fn number(doc: &Document, obj: &Object) -> Result<f64, String> {
    let (_, obj) = doc.dereference(obj).map_err(|err| err.to_string())?;
    obj.as_float()
        .map(|f| f as f64)
        .map_err(|_| "expected a number".to_string())
}

/// A `/MediaBox`/`/CropBox`-shaped array -> `PageBox`.
fn box_from_object(doc: &Document, obj: &Object) -> Result<PageBox, String> {
    let (_, obj) = doc.dereference(obj).map_err(|err| err.to_string())?;
    let arr = obj
        .as_array()
        .map_err(|_| "a page box must be an array of 4 numbers".to_string())?;
    if arr.len() != 4 {
        return Err(format!(
            "a page box must have exactly 4 numbers, found {}",
            arr.len()
        ));
    }
    Ok(PageBox {
        x0: number(doc, &arr[0])?,
        y0: number(doc, &arr[1])?,
        x1: number(doc, &arr[2])?,
        y1: number(doc, &arr[3])?,
    })
}

/// Walks `id`'s `/Parent` chain looking for `key` (`/MediaBox`,
/// `/CropBox`, `/Rotate`, ...), the same inheritance a real PDF reader
/// resolves -- a leaf page dict that omits one of these is not malformed,
/// it is *inheriting* it from an ancestor `/Pages` node (§7.7.3.4), and a
/// reader that only checked the leaf would silently treat an inherited
/// value as absent. `None` means truly absent: not on the page, not on
/// any ancestor.
fn resolve_box(doc: &Document, mut id: ObjectId, key: &[u8]) -> Result<Option<PageBox>, String> {
    loop {
        let dict = doc.get_dictionary(id).map_err(|err| err.to_string())?;
        if let Ok(obj) = dict.get(key) {
            return Ok(Some(box_from_object(doc, obj)?));
        }
        match dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok()) {
            Some(parent) => id = parent,
            None => return Ok(None),
        }
    }
}

/// The same inheritance walk as `resolve_box`, for `/Rotate` (a plain
/// integer, not an array). Absent anywhere in the chain means `0`
/// (unrotated), which is what the spec says `/Rotate`'s own absence
/// means.
fn resolve_rotate(doc: &Document, mut id: ObjectId) -> Result<i64, String> {
    loop {
        let dict = doc.get_dictionary(id).map_err(|err| err.to_string())?;
        if let Ok(obj) = dict.get(b"Rotate") {
            let (_, obj) = doc.dereference(obj).map_err(|err| err.to_string())?;
            return obj
                .as_i64()
                .or_else(|_| obj.as_float().map(|f| f as i64))
                .map_err(|_| "/Rotate must be a number".to_string());
        }
        match dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok()) {
            Some(parent) => id = parent,
            None => return Ok(0),
        }
    }
}

/// The `/MediaBox` a page prints on, resolving inheritance from an
/// ancestor `/Pages` node when the page does not set one directly (see
/// `resolve_box`'s doc comment for why that is the common case, not a
/// malformed file). A page with no `/MediaBox` anywhere in its ancestry
/// is genuinely malformed -- every PDF page must have one, directly or
/// inherited -- so that case is an error rather than a made-up default.
pub fn media_box(bytes: &[u8], page: u32) -> Result<PageBox, String> {
    let doc = load(bytes)?;
    let id = find_page(&doc, page, "media_box")?;
    resolve_box(&doc, id, b"MediaBox")?.ok_or_else(|| {
        format!("media_box: page {page} has no /MediaBox, directly or inherited -- this PDF is malformed")
    })
}

/// The `/CropBox` a viewer actually shows -- can be smaller than
/// `/MediaBox` (bleed/registration marks live outside it). Per the spec,
/// a page with no `/CropBox` of its own -- and none inherited -- crops to
/// its `/MediaBox`, which is the fallback this returns rather than an
/// error: "no crop box set" and "crops to the whole page" are the same
/// fact to every reader, so treating the first as a failure would be
/// wrong far more often than it would be useful.
pub fn crop_box(bytes: &[u8], page: u32) -> Result<PageBox, String> {
    let doc = load(bytes)?;
    let id = find_page(&doc, page, "crop_box")?;
    if let Some(b) = resolve_box(&doc, id, b"CropBox")? {
        return Ok(b);
    }
    resolve_box(&doc, id, b"MediaBox")?.ok_or_else(|| {
        format!(
            "crop_box: page {page} has no /CropBox or /MediaBox, directly or inherited -- \
             this PDF is malformed"
        )
    })
}

/// Sets a page's `/Rotate` -- degrees a viewer turns the page CLOCKWISE
/// before display (§7.7.3.4). `degrees` is ADDED to whatever rotation the
/// page already has (its own, or inherited), the "increment" convention
/// most PDF libraries' `rotate` takes: calling this twice with `90` turns
/// a page 180° total rather than discarding what was there. Must be a
/// multiple of 90 -- `/Rotate` is only ever defined in quarter turns, and
/// anything else means nothing to a reader. The result is normalized into
/// `0..360`.
pub fn rotate_page(bytes: &[u8], page: u32, degrees: i64) -> Result<Vec<u8>, String> {
    if degrees % 90 != 0 {
        return Err(format!(
            "rotate_page: {degrees} is not a multiple of 90 -- /Rotate only turns a page in \
             quarter turns"
        ));
    }
    let mut doc = load(bytes)?;
    let id = find_page(&doc, page, "rotate_page")?;
    let current = resolve_rotate(&doc, id)?;
    let new_rotation = (current + degrees).rem_euclid(360);
    let dict = doc.get_dictionary_mut(id).map_err(|err| err.to_string())?;
    dict.set("Rotate", new_rotation);
    save(&mut doc)
}

/// Sets a page's `/CropBox` to `(x0, y0)`-`(x1, y1)`, validated against
/// its `/MediaBox` (resolving inheritance the same way `media_box` does)
/// rather than accepted blindly: a crop rectangle that is inverted,
/// zero-area, or reaches outside the media box is not something a real
/// viewer renders sensibly, so it is refused here with a message that
/// says which bound was violated instead of silently producing a PDF
/// whose crop nobody asked for.
pub fn crop_page(bytes: &[u8], page: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Vec<u8>, String> {
    if !(x1 > x0 && y1 > y0) {
        return Err(format!(
            "crop_page: not a rectangle -- need x1 > x0 and y1 > y0, got ({x0}, {y0}) to ({x1}, {y1})"
        ));
    }
    let mut doc = load(bytes)?;
    let id = find_page(&doc, page, "crop_page")?;
    let media = resolve_box(&doc, id, b"MediaBox")?.ok_or_else(|| {
        format!("crop_page: page {page} has no /MediaBox, directly or inherited -- this PDF is malformed")
    })?;
    let eps = 1e-6;
    if x0 < media.x0 - eps || y0 < media.y0 - eps || x1 > media.x1 + eps || y1 > media.y1 + eps {
        return Err(format!(
            "crop_page: crop rectangle ({x0}, {y0})-({x1}, {y1}) falls outside page {page}'s \
             /MediaBox ({}, {})-({}, {})",
            media.x0, media.y0, media.x1, media.y1
        ));
    }
    let dict = doc.get_dictionary_mut(id).map_err(|err| err.to_string())?;
    dict.set(
        "CropBox",
        vec![x0.into(), y0.into(), x1.into(), y1.into()],
    );
    save(&mut doc)
}

// ── Page-tree ops ───────────────────────────────────────────────────────

/// The page attributes PDF defines as inheritable (§7.7.3.4):
/// `/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`. A leaf page dict that
/// omits one inherits it from the nearest `/Pages` ancestor that sets it
/// -- which is exactly why the tree-editing functions below cannot just
/// reparent a page under a new `/Pages` node without first resolving and
/// copying down whatever it would otherwise stop inheriting: a page whose
/// size came from an intermediate node (not the root) would silently
/// change size, or lose it, the moment it moved to sit directly under a
/// different one.
const INHERITABLE_PAGE_KEYS: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Makes `page_id`'s dict self-sufficient for every key in
/// `INHERITABLE_PAGE_KEYS` it does not already set directly, by walking
/// its OWN `/Parent` chain (before any reparenting happens) and copying
/// down the first value found. A no-op for the common case where a page
/// already carries its own `/MediaBox` etc.
fn bake_inherited_attrs(doc: &mut Document, page_id: ObjectId) {
    let mut resolved: Vec<(&'static [u8], Object)> = Vec::new();
    {
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            return;
        };
        let mut missing: Vec<&'static [u8]> = INHERITABLE_PAGE_KEYS
            .iter()
            .copied()
            .filter(|k| !page_dict.has(k))
            .collect();
        if missing.is_empty() {
            return;
        }
        let mut parent = page_dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        while let Some(pid) = parent {
            let Ok(pdict) = doc.get_dictionary(pid) else { break };
            missing.retain(|k| match pdict.get(k) {
                Ok(v) => {
                    resolved.push((k, v.clone()));
                    false
                }
                Err(_) => true,
            });
            if missing.is_empty() {
                break;
            }
            parent = pdict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        }
    }
    if resolved.is_empty() {
        return;
    }
    if let Ok(page_dict) = doc.get_dictionary_mut(page_id) {
        for (k, v) in resolved {
            page_dict.set(k, v);
        }
    }
}

/// Rebuilds `doc`'s page tree as one flat `/Pages` node whose `/Kids` is
/// exactly `order` -- the same single-level `/Kids`+`/Count` splice
/// `merge` performs when composing several documents' page trees into one
/// (see its own doc comment), reused here for a single document's
/// structural edits (add/delete/duplicate/move/reverse/split) so they
/// share one tree-editing implementation instead of five ad hoc ones.
///
/// Every id in `order` is baked (`bake_inherited_attrs`) before it is
/// reparented, so flattening cannot silently change what a page inherits.
/// Objects that fall out of the tree this way (old intermediate `/Pages`
/// nodes, a deleted leaf page) are left in place rather than pruned --
/// the same choice `extract_pages` already makes (see its own doc
/// comment: "`delete_pages` unhooks pages but leaves their objects
/// behind"), compacted away by the `renumber_objects()` call every public
/// function here already makes afterward.
fn rebuild_flat_page_tree(doc: &mut Document, order: &[ObjectId]) -> Result<(), String> {
    let pages_id = doc
        .catalog()
        .map_err(|err| format!("could not read the document catalog: {err}"))?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|err| format!("could not find the page tree: {err}"))?;

    for &id in order {
        bake_inherited_attrs(doc, id);
        if let Ok(dict) = doc.get_dictionary_mut(id) {
            dict.set("Parent", pages_id);
        }
    }

    let pages_dict = doc
        .get_dictionary_mut(pages_id)
        .map_err(|err| format!("could not update the page tree: {err}"))?;
    pages_dict.set("Count", order.len() as u32);
    pages_dict.set(
        "Kids",
        order.iter().map(|&id| Object::Reference(id)).collect::<Vec<_>>(),
    );
    Ok(())
}

/// A blank page inserted at position `index` (one-based -- the page
/// number the new page will HAVE once inserted, so `add_page(bytes, 1,
/// ...)` makes it the new first page). `index` may be one past the
/// current last page (i.e. `page_count + 1`) to append. `width`/`height`
/// are in PDF points (1/72 inch); the new page carries a real, empty
/// content stream (a legitimate blank page, not a missing one) and an
/// empty `/Resources` dictionary.
pub fn add_page(bytes: &[u8], index: u32, width: f64, height: f64) -> Result<Vec<u8>, String> {
    if !(width > 0.0 && height > 0.0) {
        return Err(format!(
            "add_page: page size must be positive, got {width} x {height}"
        ));
    }
    let mut doc = load(bytes)?;
    let mut order: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let n = order.len();
    if index < 1 || index as usize > n + 1 {
        return Err(format!(
            "add_page: index {index} -- this document has {n} page(s), so index can be 1..={}",
            n + 1
        ));
    }
    let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![0.0.into(), 0.0.into(), width.into(), height.into()],
        "Resources" => dictionary! {},
        "Contents" => content_id,
    });
    order.insert(index as usize - 1, page_id);
    rebuild_flat_page_tree(&mut doc, &order)?;
    doc.renumber_objects();
    save(&mut doc)
}

/// Removes page `page`. Every other page's own resources, fonts and
/// content are untouched. Deleting a document's only page is a named
/// error rather than a silently produced zero-page file no reader can
/// open -- the same non-empty guarantee `extract_pages`/`merge` already
/// enforce.
pub fn delete_page(bytes: &[u8], page: u32) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let mut order: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let idx = validate_page(&order, page, "delete_page")?;
    if order.len() == 1 {
        return Err(
            "delete_page: this document has only one page -- deleting it would leave a \
             document with none"
                .to_string(),
        );
    }
    order.remove(idx);
    rebuild_flat_page_tree(&mut doc, &order)?;
    doc.renumber_objects();
    save(&mut doc)
}

/// A copy of `page`, inserted immediately after it. The copy is a new
/// page object -- rotating or cropping one afterward does not touch the
/// other -- but shares its `/Contents` and `/Resources` objects with the
/// original, which is legal and ordinary in PDF (several pages pointing
/// at the same content stream) and far cheaper than duplicating a
/// stream's bytes for an operation that, by itself, changes nothing about
/// what the page draws.
pub fn duplicate_page(bytes: &[u8], page: u32) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let mut order: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let idx = validate_page(&order, page, "duplicate_page")?;
    let original_id = order[idx];
    // Baked BEFORE cloning so the copy does not inherit from the
    // ORIGINAL's ancestor chain once `rebuild_flat_page_tree` reparents
    // both of them elsewhere.
    bake_inherited_attrs(&mut doc, original_id);
    let cloned = doc
        .get_object(original_id)
        .map_err(|err| format!("duplicate_page: {err}"))?
        .clone();
    let new_id = doc.add_object(cloned);
    order.insert(idx + 1, new_id);
    rebuild_flat_page_tree(&mut doc, &order)?;
    doc.renumber_objects();
    save(&mut doc)
}

/// Relocates page `from` so it becomes page `to`, keeping every other
/// page's relative order -- `move_page(doc, 5, 1)` makes page 5 the new
/// first page; `move_page(doc, 1, 5)` makes the old first page the new
/// fifth. Both are one-based and must already be within the document's
/// page range: this relocates a page that is already there, it does not
/// also insert or delete one.
pub fn move_page(bytes: &[u8], from: u32, to: u32) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let mut order: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let from_idx = validate_page(&order, from, "move_page")?;
    let to_idx = validate_page(&order, to, "move_page")?;
    let moved = order.remove(from_idx);
    order.insert(to_idx, moved);
    rebuild_flat_page_tree(&mut doc, &order)?;
    doc.renumber_objects();
    save(&mut doc)
}

/// Reverses page order: what was page 1 becomes the last page and vice
/// versa. A pure re-splice like every other function here -- no page's
/// own content, fonts or size changes, only which position it sits at.
pub fn reverse_pages(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let mut order: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if order.is_empty() {
        return Err("reverse_pages: this document has no pages".to_string());
    }
    order.reverse();
    rebuild_flat_page_tree(&mut doc, &order)?;
    doc.renumber_objects();
    save(&mut doc)
}

/// `bytes` split into consecutive chunks of up to `n` pages each, in
/// document order -- the last chunk may be shorter. Built directly on
/// `extract_pages` (one call per chunk) rather than a parallel tree
/// implementation, the same reuse this module's own doc comment on
/// `merge` asks of anything that needs the page tree spliced.
pub fn split_every(bytes: &[u8], n: u32) -> Result<Vec<Vec<u8>>, String> {
    if n < 1 {
        return Err("split_every: n must be at least 1".to_string());
    }
    let total = page_count(bytes)?;
    if total == 0 {
        return Err("split_every: this document has no pages".to_string());
    }
    let mut out = Vec::new();
    let mut start = 1u32;
    while (start as usize) <= total {
        let end = (start + n - 1).min(total as u32);
        let chunk: Vec<u32> = (start..=end).collect();
        out.push(extract_pages(bytes, &chunk)?);
        start = end + 1;
    }
    Ok(out)
}

/// Character-offset occurrences of `needle` in `haystack`.
///
/// Not a byte search (`str::find`) because case-insensitive matching
/// would then have to lowercase the whole haystack first, and a
/// length-changing casefold (rare, but real: a German `ß` lowercases to
/// `ss`) would shift every offset after it out from under the ORIGINAL
/// string's byte positions. Comparing char windows directly sidesteps
/// that: offsets are always counted against `haystack` as given, never
/// against a transformed copy of it.
fn find_char_offsets(haystack: &str, needle: &str, case_sensitive: bool) -> Vec<usize> {
    let h: Vec<char> = haystack.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || h.len() < n.len() {
        return Vec::new();
    }
    let eq = |a: char, b: char| -> bool {
        if case_sensitive {
            a == b
        } else {
            a.to_lowercase().eq(b.to_lowercase())
        }
    };
    let mut out = Vec::new();
    for start in 0..=(h.len() - n.len()) {
        if h[start..start + n.len()].iter().zip(n.iter()).all(|(&a, &b)| eq(a, b)) {
            out.push(start);
        }
    }
    out
}

// ── Text diff ────────────────────────────────────────────────────────────

/// One line of a page-level text diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// Present, unchanged, in both pages' text.
    Same(String),
    /// Present in `a`'s page text, absent from `b`'s.
    Removed(String),
    /// Present in `b`'s page text, absent from `a`'s.
    Added(String),
}

/// One page whose text differs between the two documents (or that only
/// one of them has).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTextDiff {
    /// One-based.
    pub page: u32,
    /// `b` has fewer pages than this, so there is nothing on its side to
    /// compare -- `lines` is empty and every line of `a`'s text is, by
    /// construction, "only in a".
    pub only_in_a: bool,
    /// The mirror image of `only_in_a`.
    pub only_in_b: bool,
    /// Populated only when the page exists in both documents; a
    /// line-based diff of `extract_text(a, page)` against
    /// `extract_text(b, page)`.
    pub lines: Vec<DiffLine>,
}

/// The result of comparing two documents' text page by page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDiff {
    pub page_count_a: usize,
    pub page_count_b: usize,
    /// Only the pages that differ (including "only on one side"). A page
    /// whose text is identical on both sides is not listed -- the same
    /// "say what changed, not what didn't" shape `structural_diff`'s
    /// `differing_dimensions` uses.
    pub differing_pages: Vec<PageTextDiff>,
}

/// Page-by-page text diff of two documents, built entirely on
/// `extract_text` and `page_count` -- no separate text-reconstruction
/// path, so it inherits exactly the same strengths and failure mode as
/// every other text-reading function in this crate.
///
/// A page present in only one document counts as "differs" (`only_in_a`
/// or `only_in_b`) without attempting a diff against nothing. A page
/// present in both is diffed line-by-line (an LCS alignment over
/// `str::lines()`) only when the two texts are not byte-identical --
/// which is also why an unchanged page never appears in the output at
/// all, not even as an empty diff.
pub fn text_diff(bytes_a: &[u8], bytes_b: &[u8]) -> Result<TextDiff, String> {
    let page_count_a = page_count(bytes_a).map_err(|msg| format!("document a: {msg}"))?;
    let page_count_b = page_count(bytes_b).map_err(|msg| format!("document b: {msg}"))?;
    let max_pages = page_count_a.max(page_count_b);

    let mut differing_pages = Vec::new();
    for page in 1..=(max_pages as u32) {
        let in_a = (page as usize) <= page_count_a;
        let in_b = (page as usize) <= page_count_b;
        match (in_a, in_b) {
            (true, false) => differing_pages.push(PageTextDiff {
                page,
                only_in_a: true,
                only_in_b: false,
                lines: Vec::new(),
            }),
            (false, true) => differing_pages.push(PageTextDiff {
                page,
                only_in_a: false,
                only_in_b: true,
                lines: Vec::new(),
            }),
            (true, true) => {
                let ta = extract_text(bytes_a, Some(&[page]))
                    .map_err(|msg| format!("document a, page {page}: {msg}"))?;
                let tb = extract_text(bytes_b, Some(&[page]))
                    .map_err(|msg| format!("document b, page {page}: {msg}"))?;
                if ta != tb {
                    differing_pages.push(PageTextDiff {
                        page,
                        only_in_a: false,
                        only_in_b: false,
                        lines: line_diff(&ta, &tb),
                    });
                }
            }
            (false, false) => unreachable!("page {page} <= max({page_count_a}, {page_count_b})"),
        }
    }

    Ok(TextDiff {
        page_count_a,
        page_count_b,
        differing_pages,
    })
}

/// A minimal LCS-based line diff, the textbook dynamic-program: the
/// longest common subsequence of the two line lists, walked back into a
/// same/added/removed sequence. Good enough for a page of reconstructed
/// PDF text (at most a few hundred lines); not the algorithm to reach
/// for on a whole book.
fn line_diff(a: &str, b: &str) -> Vec<DiffLine> {
    let a_lines: Vec<&str> = a.lines().collect();
    let b_lines: Vec<&str> = b.lines().collect();
    let n = a_lines.len();
    let m = b_lines.len();

    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a_lines[i] == b_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a_lines[i] == b_lines[j] {
            out.push(DiffLine::Same(a_lines[i].to_string()));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(DiffLine::Removed(a_lines[i].to_string()));
            i += 1;
        } else {
            out.push(DiffLine::Added(b_lines[j].to_string()));
            j += 1;
        }
    }
    while i < n {
        out.push(DiffLine::Removed(a_lines[i].to_string()));
        i += 1;
    }
    while j < m {
        out.push(DiffLine::Added(b_lines[j].to_string()));
        j += 1;
    }
    out
}

// ── Structural diff ────────────────────────────────────────────────────

/// One page's `/MediaBox`-derived size on each side, when they disagree
/// (or when the page exists on only one side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDimDiff {
    /// One-based.
    pub page: u32,
    /// `(width, height)` in PDF units (1/72 inch), or `None` when the
    /// page does not exist on that side.
    pub a: Option<(f64, f64)>,
    pub b: Option<(f64, f64)>,
}

/// A coarse, non-content comparison of two documents' structure.
///
/// Deliberately shallow -- page count, per-page `/MediaBox` size,
/// whether each document has an outline (`/Outlines` on the catalog) and
/// how many total annotations each has. Real bookmark-by-bookmark or
/// annotation-by-annotation comparison is a larger feature (and, for
/// bookmarks/annotations specifically, overlaps work in progress
/// elsewhere on this toolkit); this is the v1 that does not block on
/// that landing first.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralDiff {
    pub page_count_a: usize,
    pub page_count_b: usize,
    /// Only pages whose size differs, or that exist on only one side --
    /// same "differences only" shape as `TextDiff::differing_pages`.
    pub differing_dimensions: Vec<PageDimDiff>,
    pub has_bookmarks_a: bool,
    pub has_bookmarks_b: bool,
    pub annotation_count_a: usize,
    pub annotation_count_b: usize,
}

pub fn structural_diff(bytes_a: &[u8], bytes_b: &[u8]) -> Result<StructuralDiff, String> {
    let doc_a = load(bytes_a).map_err(|msg| format!("document a: {msg}"))?;
    let doc_b = load(bytes_b).map_err(|msg| format!("document b: {msg}"))?;
    let pages_a = doc_a.get_pages();
    let pages_b = doc_b.get_pages();
    let page_count_a = pages_a.len();
    let page_count_b = pages_b.len();
    let max_pages = page_count_a.max(page_count_b);

    let mut differing_dimensions = Vec::new();
    for page in 1..=(max_pages as u32) {
        let dim_a = pages_a.get(&page).and_then(|&id| page_media_box(&doc_a, id));
        let dim_b = pages_b.get(&page).and_then(|&id| page_media_box(&doc_b, id));
        let in_a = pages_a.contains_key(&page);
        let in_b = pages_b.contains_key(&page);
        let differs = !in_a || !in_b || dim_a != dim_b;
        if differs {
            differing_dimensions.push(PageDimDiff {
                page,
                a: dim_a,
                b: dim_b,
            });
        }
    }

    let has_bookmarks_a = doc_a.catalog().map(|c| c.has(b"Outlines")).unwrap_or(false);
    let has_bookmarks_b = doc_b.catalog().map(|c| c.has(b"Outlines")).unwrap_or(false);
    let annotation_count_a = pages_a
        .values()
        .map(|&id| doc_a.get_page_annotations(id).map(|v| v.len()).unwrap_or(0))
        .sum();
    let annotation_count_b = pages_b
        .values()
        .map(|&id| doc_b.get_page_annotations(id).map(|v| v.len()).unwrap_or(0))
        .sum();

    Ok(StructuralDiff {
        page_count_a,
        page_count_b,
        differing_dimensions,
        has_bookmarks_a,
        has_bookmarks_b,
        annotation_count_a,
        annotation_count_b,
    })
}

/// A page's `/MediaBox` as `(width, height)`, walking up `/Parent` when
/// the page itself does not carry one -- `/MediaBox` is an inheritable
/// attribute in the PDF page-tree sense, so a page that omits it is
/// still using a real, defined size, not "no size".
fn page_media_box(doc: &Document, page_id: lopdf::ObjectId) -> Option<(f64, f64)> {
    fn as_f64(obj: &Object) -> Option<f64> {
        obj.as_float()
            .map(|f| f as f64)
            .ok()
            .or_else(|| obj.as_i64().ok().map(|i| i as f64))
    }

    let mut current = Some(page_id);
    let mut depth = 0;
    while let Some(id) = current {
        if depth > 32 {
            return None;
        }
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(arr) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
            if arr.len() == 4 {
                let nums: Vec<f64> = arr.iter().filter_map(as_f64).collect();
                if nums.len() == 4 {
                    let (llx, lly, urx, ury) = (nums[0], nums[1], nums[2], nums[3]);
                    return Some(((urx - llx).abs(), (ury - lly).abs()));
                }
            }
        }
        current = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        depth += 1;
    }
    None
}

/// `bytes` split into exactly two documents at `index` (one-based): the
/// first holds pages `1..index`, the second `index..=page_count`. Both
/// halves must come out non-empty, so `index` must be strictly between 1
/// and the page count -- `split_at(doc, 1)` (an empty first half) and
/// `split_at(doc, page_count)` (an empty second half) are refused by name
/// rather than silently handed back as the whole document plus a
/// zero-page file, which `extract_pages` already refuses to build.
pub fn split_at(bytes: &[u8], index: u32) -> Result<(Vec<u8>, Vec<u8>), String> {
    let total = page_count(bytes)?;
    if total < 2 {
        return Err(format!(
            "split_at: this document has {total} page(s) -- splitting into two non-empty \
             documents needs at least 2"
        ));
    }
    if index < 2 || index as usize > total {
        return Err(format!(
            "split_at: index {index} -- must be 2..={total} so both halves are non-empty \
             (the first half would be pages 1..{index}, the second {index}..{total})"
        ));
    }
    let first: Vec<u32> = (1..index).collect();
    let second: Vec<u32> = (index..=total as u32).collect();
    Ok((extract_pages(bytes, &first)?, extract_pages(bytes, &second)?))
}

fn save(doc: &mut Document) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    doc.save_to(&mut buf)
        .map_err(|err| format!("could not write PDF: {err}"))?;
    Ok(buf.into_inner())
}

/// The object id of a document's page `page` (one-based), or a named
/// error listing what pages there actually are -- the same shape
/// `extract_pages`/`extract_text` already use for the same question,
/// shared here so `outlines`/`annotations`/`attachments` phrase it
/// identically.
fn require_page(doc: &Document, page: u32) -> Result<lopdf::ObjectId, String> {
    let pages = doc.get_pages();
    match pages.get(&page).copied() {
        Some(id) => Ok(id),
        None => Err(format!(
            "no page {page} -- this document has {}",
            describe_pages(&pages.into_keys().collect::<Vec<_>>())
        )),
    }
}

/// PDF text strings allow either raw PDFDocEncoding or UTF-16BE with a
/// leading byte-order mark -- `lopdf::decode_text_string` (used by `info`
/// above and by `outline_rows` below) reads both. ASCII is written as
/// plain bytes; anything else as UTF-16BE+BOM, the same choice `lopdf`'s
/// own `Document::add_bookmark` helper makes and for the same reason:
/// PDFDocEncoding cannot represent arbitrary Unicode, and UTF-16BE can.
fn encode_pdf_text(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        s.as_bytes().to_vec()
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(s.encode_utf16().flat_map(u16::to_be_bytes));
        bytes
    }
}

// ── Bookmarks / outlines ────────────────────────────────────────────────
//
// A PDF outline is a tree hung off the catalog's `/Outlines` entry: each
// node has `/First`/`/Last` pointing at its first/last child and
// `/Prev`/`/Next` threading it to its siblings, and a leaf's `/Dest` (or
// its `/A` GoTo action's `/D`) is `[page_ref, /Fit, ...]`.
//
// Exposed here as a FLAT `Vec<OutlineEntry>`, depth-first pre-order,
// rather than a nested tree: this crate has no `Value` to build a nested
// structure out of (see the module doc comment), and a flat table with a
// `level` and a `parent` column lets `qu-interp` turn it into either a
// `Value::Table` of rows or a nested structure without this crate having
// to know which. `parent` is an index back into this SAME `Vec`, and it
// is also the handle `add_bookmark`'s own `parent` argument takes: both
// go through `outline_rows` below, the one traversal that assigns the
// indices, so an index taken from one call always names the same entry
// in another call on the same bytes.

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    /// 0 for a top-level entry, 1 for its children, and so on.
    pub level: usize,
    /// Index into the same `Vec` of the entry this one is nested under,
    /// or `None` for a top-level entry.
    pub parent: Option<usize>,
    /// The one-based page this entry's destination resolves to. `None`
    /// for a named destination, a non-`GoTo` action (`GoToR`, `URI`,
    /// ...), or anything else this crate does not chase further -- the
    /// same "say so rather than guess" gap `extract_text` documents for
    /// a different limitation.
    pub page: Option<u32>,
}

/// The document's outline tree, flattened. An empty `Vec`, not an error,
/// for a PDF with no `/Outlines` at all -- "no bookmarks" is the ordinary
/// case, not a malformed one.
pub fn outlines(bytes: &[u8]) -> Result<Vec<OutlineEntry>, String> {
    let doc = load(bytes)?;
    Ok(outline_rows(&doc).into_iter().map(|(_, entry)| entry).collect())
}

/// Add a new outline entry titled `title`, pointing at `page` (one-based),
/// optionally nested under an existing entry named by `parent` -- an
/// index from this same document's own `outlines()` output (see
/// `OutlineEntry::parent`'s doc comment for why that index is stable
/// across the two calls).
///
/// Only the new entry's immediate parent's `/Count` is updated; an
/// ancestor further up the tree keeps whatever count it already had.
/// `/Count` is documented in the PDF spec as informational -- the number
/// next to a collapsed section, not a structural field anything is
/// required to trust -- but a reader that recomputes it strictly would
/// show a stale count on an ancestor two or more levels above a newly
/// added grandchild.
pub fn add_bookmark(bytes: &[u8], title: &str, page: u32, parent: Option<usize>) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let page_id = require_page(&doc, page)?;

    let parent_id = match parent {
        Some(idx) => Some(nth_outline_id(&doc, idx).ok_or_else(|| format!("no outline entry at index {idx}"))?),
        None => None,
    };

    let root_id = ensure_outlines_root(&mut doc);
    let container_id = parent_id.unwrap_or(root_id);

    let new_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal(encode_pdf_text(title)),
        "Dest" => vec![Object::Reference(page_id), Object::Name(b"Fit".to_vec())],
        "Parent" => container_id,
    });

    append_outline_child(&mut doc, container_id, new_id);
    save(&mut doc)
}

/// `outlines()`'s traversal, kept separate because `add_bookmark` needs
/// the object id each row came from (to resolve `parent=`) and
/// `outlines()` itself does not -- one walk, two callers.
fn outline_rows(doc: &Document) -> Vec<(lopdf::ObjectId, OutlineEntry)> {
    let mut out = Vec::new();
    let Some(first_id) = outline_root_first(doc) else {
        return out;
    };
    let page_by_id: std::collections::HashMap<lopdf::ObjectId, u32> =
        doc.get_pages().into_iter().map(|(n, id)| (id, n)).collect();
    walk_outline_siblings(doc, first_id, 0, None, &page_by_id, &mut out, 0);
    out
}

fn nth_outline_id(doc: &Document, idx: usize) -> Option<lopdf::ObjectId> {
    outline_rows(doc).into_iter().nth(idx).map(|(id, _)| id)
}

/// The object id of the catalog's `/Outlines` node's first child, or
/// `None` if the document has no `/Outlines`, or has one with no entries
/// yet -- both are "no bookmarks", not an error.
fn outline_root_first(doc: &Document) -> Option<lopdf::ObjectId> {
    let catalog = doc.catalog().ok()?;
    let outlines_id = match catalog.get(b"Outlines").ok()? {
        Object::Reference(id) => *id,
        _ => return None,
    };
    let outlines_dict = doc.get_dictionary(outlines_id).ok()?;
    match outlines_dict.get(b"First").ok()? {
        Object::Reference(id) => Some(*id),
        _ => None,
    }
}

/// Bounded the same way `reachable_fonts`'s `walk` is above -- a
/// malformed file can make `/Next` or `/First` cyclic, and a real outline
/// tree is a few dozen entries deep at most.
const MAX_OUTLINE_DEPTH: usize = 64;

fn walk_outline_siblings(
    doc: &Document,
    mut cur: lopdf::ObjectId,
    level: usize,
    parent: Option<usize>,
    page_by_id: &std::collections::HashMap<lopdf::ObjectId, u32>,
    out: &mut Vec<(lopdf::ObjectId, OutlineEntry)>,
    depth: usize,
) {
    if depth > MAX_OUTLINE_DEPTH {
        return;
    }
    loop {
        let Ok(dict) = doc.get_dictionary(cur) else { return };
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| lopdf::decode_text_string(o).ok())
            .unwrap_or_default();
        let page = resolve_outline_page(doc, dict, page_by_id);
        let first_child = match dict.get(b"First") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        let next_sibling = match dict.get(b"Next") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };

        let this_index = out.len();
        out.push((cur, OutlineEntry { title, level, parent, page }));

        if let Some(first_id) = first_child {
            walk_outline_siblings(doc, first_id, level + 1, Some(this_index), page_by_id, out, depth + 1);
        }

        match next_sibling {
            Some(n) => cur = n,
            None => return,
        }
    }
}

/// The page an outline entry's `/Dest` (or its `/A` GoTo action's `/D`)
/// points at, resolved to a one-based page number.
fn resolve_outline_page(
    doc: &Document,
    dict: &lopdf::Dictionary,
    page_by_id: &std::collections::HashMap<lopdf::ObjectId, u32>,
) -> Option<u32> {
    let dest_obj: Object = if let Ok(d) = dict.get(b"Dest") {
        d.clone()
    } else {
        let action = match dict.get(b"A").ok()? {
            Object::Reference(id) => doc.get_object(*id).ok()?.clone(),
            other => other.clone(),
        };
        let action_dict = action.as_dict().ok()?;
        if action_dict.get(b"S").ok()?.as_name().ok()? != b"GoTo" {
            return None;
        }
        action_dict.get(b"D").ok()?.clone()
    };

    let dest_obj = match &dest_obj {
        Object::Reference(id) => doc.get_object(*id).ok()?.clone(),
        _ => dest_obj,
    };

    match dest_obj.as_array().ok()?.first()? {
        Object::Reference(id) => page_by_id.get(id).copied(),
        _ => None,
    }
}

/// The catalog's `/Outlines` dictionary, creating an empty one (and
/// wiring it into the catalog) if the document does not have one yet.
fn ensure_outlines_root(doc: &mut Document) -> lopdf::ObjectId {
    if let Some(Object::Reference(id)) = doc.catalog().ok().and_then(|c| c.get(b"Outlines").ok()) {
        return *id;
    }
    let root_id = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "Count" => 0,
    });
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.set("Outlines", root_id);
    }
    root_id
}

/// Append `child_id` as the LAST child of `parent_id` -- walking to the
/// end of whatever chain is already there via `/Next` rather than just
/// setting `/First`, which would silently drop every existing child.
/// Bumps `parent_id`'s own `/Count`; see `add_bookmark`'s doc comment for
/// the one thing this does not do (fix up further ancestors).
fn append_outline_child(doc: &mut Document, parent_id: lopdf::ObjectId, child_id: lopdf::ObjectId) {
    let first_child = match doc.get_dictionary(parent_id).ok().and_then(|d| d.get(b"First").ok()) {
        Some(Object::Reference(id)) => Some(*id),
        _ => None,
    };

    match first_child {
        None => {
            if let Ok(dict) = doc.get_dictionary_mut(parent_id) {
                dict.set("First", child_id);
                dict.set("Last", child_id);
            }
        }
        Some(mut cur) => {
            loop {
                let next = match doc.get_dictionary(cur).ok().and_then(|d| d.get(b"Next").ok()) {
                    Some(Object::Reference(id)) => Some(*id),
                    _ => None,
                };
                match next {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            if let Ok(dict) = doc.get_dictionary_mut(cur) {
                dict.set("Next", child_id);
            }
            if let Ok(dict) = doc.get_dictionary_mut(child_id) {
                dict.set("Prev", cur);
            }
            if let Ok(dict) = doc.get_dictionary_mut(parent_id) {
                dict.set("Last", child_id);
            }
        }
    }

    if let Ok(dict) = doc.get_dictionary_mut(parent_id) {
        let count = dict.get(b"Count").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
        dict.set("Count", if count < 0 { count - 1 } else { count + 1 });
    }
}

// ── Annotations / links ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationEntry {
    /// Position in the page's `/Annots` array -- the handle
    /// `remove_annotation` takes. Stable as long as nothing else on the
    /// same page has added or removed an annotation in between.
    pub index: usize,
    /// The annotation's `/Subtype`, verbatim -- `"Text"`, `"Square"`,
    /// `"Highlight"`, `"Link"`, or anything else a PDF can carry here.
    pub subtype: String,
    /// `[x0, y0, x1, y1]` in default user space, exactly as the PDF
    /// stores it -- not normalized to `x0 < x1`.
    pub rect: [f64; 4],
    pub contents: Option<String>,
}

/// A page's annotations -- an empty `Vec`, not an error, for a page with
/// no `/Annots` at all.
pub fn annotations(bytes: &[u8], page: u32) -> Result<Vec<AnnotationEntry>, String> {
    let doc = load(bytes)?;
    let page_id = require_page(&doc, page)?;
    let page_dict = doc.get_dictionary(page_id).map_err(|err| err.to_string())?;
    let Some(items) = annots_array(&doc, page_dict) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let resolved = match item {
            Object::Reference(id) => doc.get_object(*id).ok(),
            other => Some(other),
        };
        let Some(dict) = resolved.and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        let subtype = dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_default();
        let rect = dict
            .get(b"Rect")
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(rect_from_array)
            .unwrap_or([0.0; 4]);
        let contents = dict
            .get(b"Contents")
            .ok()
            .and_then(|o| lopdf::decode_text_string(o).ok())
            .filter(|s| !s.is_empty());
        out.push(AnnotationEntry { index: i, subtype, rect, contents });
    }
    Ok(out)
}

fn annots_array<'a>(doc: &'a Document, page_dict: &'a lopdf::Dictionary) -> Option<&'a Vec<Object>> {
    match page_dict.get(b"Annots").ok()? {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_array().ok(),
        Object::Array(a) => Some(a),
        _ => None,
    }
}

fn rect_from_array(arr: &Vec<Object>) -> Option<[f64; 4]> {
    if arr.len() != 4 {
        return None;
    }
    let mut r = [0.0; 4];
    for (i, v) in arr.iter().enumerate() {
        r[i] = v.as_float().ok()? as f64;
    }
    Some(r)
}

/// Add a simple annotation -- a `/Text` note, a `/Square`, a
/// `/Highlight`, or any other subtype PDF defines -- to a page.
/// `subtype` is written verbatim as `/Subtype`; this crate does not
/// validate it against PDF's own list, trusting a caller-supplied name
/// the same way `extract_pages` trusts a caller-supplied page list.
pub fn add_annotation(
    bytes: &[u8],
    page: u32,
    subtype: &str,
    rect: [f64; 4],
    text: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let page_id = require_page(&doc, page)?;

    let mut dict = dictionary! {
        "Type" => "Annot",
        "Subtype" => Object::Name(subtype.as_bytes().to_vec()),
        "Rect" => rect_to_array(rect),
    };
    if let Some(t) = text {
        dict.set("Contents", Object::string_literal(encode_pdf_text(t)));
    }
    let annot_id = doc.add_object(dict);
    push_annot(&mut doc, page_id, annot_id);
    save(&mut doc)
}

/// Remove the `index`-th entry of a page's `/Annots` array (the index
/// `annotations()` reports), deleting the underlying object too so it
/// does not linger as an orphan the next save carries forward.
pub fn remove_annotation(bytes: &[u8], page: u32, index: usize) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let page_id = require_page(&doc, page)?;
    let mut items = annots_owned(&doc, page_id);
    if index >= items.len() {
        return Err(format!("no annotation {index} on page {page} -- it has {}", items.len()));
    }
    let removed = items.remove(index);
    if let Object::Reference(id) = removed {
        doc.delete_object(id);
    }
    if let Ok(dict) = doc.get_dictionary_mut(page_id) {
        dict.set("Annots", items);
    }
    save(&mut doc)
}

/// A `/Link` annotation whose destination is another page of the SAME
/// document -- `/Dest`, `[page_ref, /Fit]`, the same shape `add_bookmark`
/// writes, rather than a `/A` GoTo action; both are legal for a
/// same-document link and `/Dest` is the simpler of the two.
pub fn add_link(bytes: &[u8], page: u32, rect: [f64; 4], target_page: u32) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let page_id = require_page(&doc, page)?;
    let target_id = require_page(&doc, target_page)?;

    let dict = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => rect_to_array(rect),
        "Dest" => vec![Object::Reference(target_id), Object::Name(b"Fit".to_vec())],
        "Border" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(0)],
    };
    let annot_id = doc.add_object(dict);
    push_annot(&mut doc, page_id, annot_id);
    save(&mut doc)
}

fn rect_to_array(rect: [f64; 4]) -> Vec<Object> {
    rect.iter().map(|&v| v.into()).collect()
}

/// The page's `/Annots` array, owned -- read out, cloned, ready to be
/// pushed onto or filtered and written straight back with `dict.set`.
/// A page's `/Annots` is legal as either a direct array or a reference to
/// one; this collapses that choice to a plain `Vec` either way, and every
/// write from here on produces a direct array.
fn annots_owned(doc: &Document, page_id: lopdf::ObjectId) -> Vec<Object> {
    let existing = doc.get_dictionary(page_id).ok().and_then(|d| d.get(b"Annots").ok()).cloned();
    match existing {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => doc.get_object(id).ok().and_then(|o| o.as_array().ok()).cloned().unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn push_annot(doc: &mut Document, page_id: lopdf::ObjectId, annot_id: lopdf::ObjectId) {
    let mut items = annots_owned(doc, page_id);
    items.push(Object::Reference(annot_id));
    if let Ok(dict) = doc.get_dictionary_mut(page_id) {
        dict.set("Annots", items);
    }
}

// ── Attachments (embedded files) ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentEntry {
    pub filename: String,
    /// The embedded stream's decompressed length, in bytes.
    pub size: usize,
}

/// The catalog's `/Names/EmbeddedFiles` name tree, listed. An empty
/// `Vec`, not an error, for a document with no embedded files.
///
/// Only the FLAT form of the name tree -- a leaf `/Names` array, no
/// `/Kids` -- is read. `add_attachment` never produces anything else, so
/// the two round-trip; a PDF from another tool that subdivides a large
/// embedded-file count across `/Kids` is not walked here, and reports no
/// attachments rather than a partial list -- the same "honest empty over
/// silently-partial" choice `extract_text` documents for a different gap.
pub fn attachments(bytes: &[u8]) -> Result<Vec<AttachmentEntry>, String> {
    let doc = load(bytes)?;
    let Some(names) = embedded_file_names(&doc) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for pair in names.chunks(2) {
        let [name_obj, spec_obj] = pair else { continue };
        let Ok(filename) = lopdf::decode_text_string(name_obj) else {
            continue;
        };
        let Some(size) = embedded_file_size(&doc, spec_obj) else {
            continue;
        };
        out.push(AttachmentEntry { filename, size });
    }
    Ok(out)
}

/// Embed `data` under `filename`, creating the `/Names`/`/EmbeddedFiles`
/// chain if the document does not have one yet.
///
/// Appended, not merged by name: adding the same `filename` twice yields
/// two entries, and `extract_attachment` reads back the FIRST match --
/// documented rather than silently overwritten, since "the document now
/// carries two files under this name" is itself sometimes the point (two
/// revisions kept side by side).
pub fn add_attachment(bytes: &[u8], filename: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;

    let ef_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Params" => dictionary! { "Size" => data.len() as i64 },
        },
        data.to_vec(),
    ));
    let spec_id = doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal(encode_pdf_text(filename)),
        "EF" => dictionary! { "F" => ef_id },
    });

    let ef_dict_id = ensure_embedded_files_dict(&mut doc);
    if let Ok(dict) = doc.get_dictionary_mut(ef_dict_id) {
        let mut names: Vec<Object> = match dict.get(b"Names") {
            Ok(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        names.push(Object::string_literal(encode_pdf_text(filename)));
        names.push(Object::Reference(spec_id));
        dict.set("Names", names);
    }

    save(&mut doc)
}

/// The raw bytes of the first embedded file named `filename`.
pub fn extract_attachment(bytes: &[u8], filename: &str) -> Result<Vec<u8>, String> {
    let doc = load(bytes)?;
    let Some(names) = embedded_file_names(&doc) else {
        return Err(format!("no attachment named `{filename}` -- this document has none"));
    };
    for pair in names.chunks(2) {
        let [name_obj, spec_obj] = pair else { continue };
        let Ok(name) = lopdf::decode_text_string(name_obj) else {
            continue;
        };
        if name != filename {
            continue;
        }
        return embedded_file_stream(&doc, spec_obj)
            .and_then(|s| s.decompressed_content().ok())
            .ok_or_else(|| format!("`{filename}` has no readable content"));
    }
    Err(format!("no attachment named `{filename}`"))
}

fn embedded_file_names(doc: &Document) -> Option<&Vec<Object>> {
    let catalog = doc.catalog().ok()?;
    let names = doc.get_dict_in_dict(catalog, b"Names").ok()?;
    let embedded = doc.get_dict_in_dict(names, b"EmbeddedFiles").ok()?;
    embedded.get(b"Names").ok()?.as_array().ok()
}

fn embedded_file_stream<'a>(doc: &'a Document, spec_obj: &'a Object) -> Option<&'a lopdf::Stream> {
    let spec_dict = match spec_obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        other => other.as_dict().ok()?,
    };
    let ef = doc.get_dict_in_dict(spec_dict, b"EF").ok()?;
    match ef.get(b"F").ok()? {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok(),
        _ => None,
    }
}

fn embedded_file_size(doc: &Document, spec_obj: &Object) -> Option<usize> {
    embedded_file_stream(doc, spec_obj).and_then(|s| s.decompressed_content().ok()).map(|c| c.len())
}

/// The catalog's `/Names/EmbeddedFiles` dictionary, creating the whole
/// chain (`/Names`, then `/Names/EmbeddedFiles`, each its own indirect
/// object) if the document does not have one yet. Returns its object id.
fn ensure_embedded_files_dict(doc: &mut Document) -> lopdf::ObjectId {
    let names_dict_id = match doc.catalog().ok().and_then(|c| c.get(b"Names").ok()) {
        Some(Object::Reference(id)) => *id,
        _ => {
            let id = doc.add_object(dictionary! {});
            if let Ok(catalog) = doc.catalog_mut() {
                catalog.set("Names", id);
            }
            id
        }
    };

    let existing = doc.get_dictionary(names_dict_id).ok().and_then(|d| d.get(b"EmbeddedFiles").ok()).cloned();
    match existing {
        Some(Object::Reference(id)) => id,
        _ => {
            let ef_id = doc.add_object(dictionary! { "Names" => Vec::<Object>::new() });
            if let Ok(names_dict) = doc.get_dictionary_mut(names_dict_id) {
                names_dict.set("EmbeddedFiles", ef_id);
            }
            ef_id
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Stream};

    /// A real, valid, `n`-page PDF built here rather than committed as a
    /// binary blob -- the same call `qu-codec`'s tests make (`hound`
    /// encodes, so the fixtures are generated). Each page carries one line
    /// of Courier text, a base-14 font with a standard encoding, which is
    /// the case where text extraction is supposed to work.
    fn build_pdf(n: usize, title: Option<&str>) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut kids = Vec::new();
        for i in 0..n {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new(
                        "Tj",
                        vec![Object::string_literal(format!("PAGE-{}", i + 1))],
                    ),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            });
            kids.push(page_id.into());
        }
        let count = kids.len() as u32;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        if let Some(t) = title {
            let info_id = doc.add_object(dictionary! {
                "Title" => Object::string_literal(t),
                "Author" => Object::string_literal("Qu test suite"),
                "Producer" => Object::string_literal("qu-pdf"),
            });
            doc.trailer.set("Info", info_id);
        }
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn page_count_matches_what_was_built() {
        for n in [1usize, 3, 7] {
            assert_eq!(page_count(&build_pdf(n, None)).unwrap(), n, "n = {n}");
        }
    }

    #[test]
    fn info_reads_metadata_and_version() {
        let got = info(&build_pdf(2, Some("Impedance notes"))).unwrap();
        assert_eq!(got.page_count, 2);
        assert_eq!(got.version, "1.5");
        assert_eq!(got.title.as_deref(), Some("Impedance notes"));
        assert_eq!(got.author.as_deref(), Some("Qu test suite"));
        assert_eq!(got.producer.as_deref(), Some("qu-pdf"));
        // Absent, not empty: the fixture writes no /Subject or /Keywords at
        // all -- `keywords` is exercised for real by the `set_metadata`
        // round-trip tests below.
        assert_eq!(got.subject, None);
        assert_eq!(got.keywords, None);
        assert!(!got.encrypted);
    }

    #[test]
    fn info_on_a_file_with_no_info_dictionary_is_not_an_error() {
        let got = info(&build_pdf(1, None)).unwrap();
        assert_eq!(got.page_count, 1);
        assert_eq!(got.title, None);
    }

    #[test]
    fn not_a_pdf_is_an_error_not_a_zero_page_document() {
        let err = page_count(b"this is not a PDF").unwrap_err();
        assert!(err.contains("not a readable PDF"), "{err}");
    }

    /// The check that actually matters for `merge`: the OUTPUT is a PDF
    /// that reloads and has the right number of pages. "No error was
    /// thrown" would pass on a file no reader can open.
    #[test]
    fn merge_output_reloads_with_every_page() {
        let a = build_pdf(2, Some("A"));
        let b = build_pdf(3, Some("B"));
        let c = build_pdf(1, None);
        let merged = merge(&[a, b, c]).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 6);
        // And the page CONTENT survived, not just the count -- six pages
        // of blank would also count to six.
        let text = extract_text(&merged, None).unwrap();
        for want in ["PAGE-1", "PAGE-2", "PAGE-3"] {
            assert!(text.contains(want), "{want:?} missing from {text:?}");
        }
    }

    #[test]
    fn merge_of_one_document_is_that_document() {
        let a = build_pdf(4, None);
        let merged = merge(std::slice::from_ref(&a)).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 4);
    }

    #[test]
    fn merge_of_nothing_is_an_error() {
        assert!(merge(&[]).unwrap_err().contains("nothing to merge"));
    }

    #[test]
    fn extract_pages_keeps_only_what_was_asked_for() {
        let src = build_pdf(5, Some("Five"));
        let out = extract_pages(&src, &[2, 4]).unwrap();
        assert_eq!(page_count(&out).unwrap(), 2);
        // The RIGHT two pages, which a count alone cannot show.
        let text = extract_text(&out, None).unwrap();
        assert!(text.contains("PAGE-2"), "{text:?}");
        assert!(text.contains("PAGE-4"), "{text:?}");
        assert!(!text.contains("PAGE-1"), "{text:?}");
        assert!(!text.contains("PAGE-3"), "{text:?}");
        assert!(!text.contains("PAGE-5"), "{text:?}");
    }

    #[test]
    fn extract_pages_rejects_a_page_that_is_not_there() {
        let src = build_pdf(3, None);
        let err = extract_pages(&src, &[9]).unwrap_err();
        assert!(err.contains("no page 9"), "{err}");
        assert!(err.contains("pages 1..3"), "{err}");
    }

    #[test]
    fn extract_pages_of_nothing_is_an_error() {
        let err = extract_pages(&build_pdf(2, None), &[]).unwrap_err();
        assert!(err.contains("no pages asked for"), "{err}");
    }

    #[test]
    fn page_numbers_are_one_based_and_dense_here() {
        assert_eq!(page_numbers(&build_pdf(3, None)).unwrap(), vec![1, 2, 3]);
    }

    /// Text extraction on the case it is supposed to handle: a base-14
    /// font with a standard encoding. This is the ONLY case this crate
    /// claims works -- see the module doc comment and the next test.
    #[test]
    fn extract_text_reads_standard_encoded_text() {
        let src = build_pdf(3, None);
        let all = extract_text(&src, None).unwrap();
        assert!(all.contains("PAGE-1"), "{all:?}");
        assert!(all.contains("PAGE-3"), "{all:?}");
        let one = extract_text(&src, Some(&[2])).unwrap();
        assert!(one.contains("PAGE-2"), "{one:?}");
        assert!(!one.contains("PAGE-1"), "{one:?}");
        assert!(!one.contains("PAGE-3"), "{one:?}");
    }

    #[test]
    fn extract_text_rejects_a_page_that_is_not_there() {
        let err = extract_text(&build_pdf(2, None), Some(&[5])).unwrap_err();
        assert!(err.contains("no page 5"), "{err}");
    }

    /// One page, one rectangle, no font anywhere. There is genuinely no
    /// text, so `""` is the true answer and must NOT be an error --
    /// otherwise every scanned page and every pure vector figure fails.
    fn build_textless_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content = Content {
            operations: vec![
                Operation::new("re", vec![72.into(), 72.into(), 200.into(), 100.into()]),
                Operation::new("f", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! {},
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    /// The shape `pdfcrop` produces, and the one that exposed the bug in
    /// the first version of the empty-result check: the page's own
    /// `/Resources` has NO font, the text and the font both live inside a
    /// Form XObject the page invokes with `Do`. `lopdf` does not descend
    /// into it, so extraction yields nothing -- and a check that looked
    /// only at the page's own fonts would have called that honest.
    fn build_form_xobject_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let inner = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![10.into(), 10.into()]),
                Operation::new("Tj", vec![Object::string_literal("HIDDEN-IN-FORM")]),
                Operation::new("ET", vec![]),
            ],
        };
        let form_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => font_id },
                },
            },
            inner.encode().unwrap(),
        ));
        let content = Content {
            operations: vec![Operation::new("Do", vec!["X1".into()])],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            // No "Font" key at all -- that is the whole point.
            "Resources" => dictionary! {
                "XObject" => dictionary! { "X1" => form_id },
            },
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn a_page_with_no_fonts_extracts_to_an_honest_empty_string() {
        let src = build_textless_pdf();
        assert_eq!(page_count(&src).unwrap(), 1);
        assert_eq!(extract_text(&src, None).unwrap().trim(), "");
    }

    /// The regression this crate exists to not have: text that is really
    /// there, extracted as nothing, reported as success.
    #[test]
    fn text_hidden_in_a_form_xobject_is_an_error_not_an_empty_string() {
        let src = build_form_xobject_pdf();
        let err = extract_text(&src, None).unwrap_err();
        assert!(err.contains("Form XObject"), "{err}");
        assert!(err.contains("1 embedded font"), "{err}");
        // And the structural operations still work on the same file,
        // which is what the error message promises.
        assert_eq!(page_count(&src).unwrap(), 1);
        assert_eq!(
            page_count(&extract_pages(&src, &[1]).unwrap()).unwrap(),
            1
        );
    }

    #[test]
    fn reachable_fonts_sees_through_a_form_xobject() {
        let doc = Document::load_mem(&build_form_xobject_pdf()).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        // The page's own resources carry none...
        assert_eq!(doc.get_page_fonts(page_id).map(|f| f.len()).unwrap_or(0), 0);
        // ...but one is reachable through the form. This is the exact gap
        // the first version of the check fell into.
        assert_eq!(reachable_fonts(&doc, page_id), 1);
    }

    // ---- set_metadata / strip_metadata ----

    #[test]
    fn set_metadata_sets_only_the_fields_passed() {
        let src = build_pdf(1, Some("Original Title"));
        let before = info(&src).unwrap();
        assert_eq!(before.author.as_deref(), Some("Qu test suite"));

        let edit = MetadataEdit {
            subject: Some("Impedance".to_string()),
            keywords: Some("battery, eis".to_string()),
            ..Default::default()
        };
        let out = set_metadata(&src, &edit).unwrap();
        let after = info(&out).unwrap();
        // Set:
        assert_eq!(after.subject.as_deref(), Some("Impedance"));
        assert_eq!(after.keywords.as_deref(), Some("battery, eis"));
        // NOT passed -- must survive unchanged, not merely "not cleared":
        assert_eq!(after.title.as_deref(), Some("Original Title"));
        assert_eq!(after.author.as_deref(), Some("Qu test suite"));
        assert_eq!(after.producer.as_deref(), Some("qu-pdf"));
        // Untouched structurally too.
        assert_eq!(after.page_count, 1);
    }

    #[test]
    fn set_metadata_creates_an_info_dict_when_none_exists() {
        let src = build_pdf(2, None);
        assert_eq!(info(&src).unwrap().title, None);
        let out = set_metadata(
            &src,
            &MetadataEdit {
                title: Some("New".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let after = info(&out).unwrap();
        assert_eq!(after.title.as_deref(), Some("New"));
        assert_eq!(after.page_count, 2);
    }

    #[test]
    fn set_metadata_with_no_fields_is_an_error() {
        let src = build_pdf(1, None);
        let err = set_metadata(&src, &MetadataEdit::default()).unwrap_err();
        assert!(err.contains("nothing to set"), "{err}");
    }

    /// Documents the choice `MetadataEdit`'s doc comment makes: an empty
    /// string is a real, written value, and it reads back as `None`
    /// anyway because `info()`'s own `field` closure already collapses an
    /// empty decoded string to `None` -- a pre-existing rule, not
    /// something `set_metadata` adds.
    #[test]
    fn set_metadata_empty_string_reads_back_as_absent() {
        let src = build_pdf(1, Some("Has a title"));
        let out = set_metadata(
            &src,
            &MetadataEdit {
                title: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(info(&out).unwrap().title, None);
    }

    #[test]
    fn strip_metadata_removes_the_info_dict() {
        let src = build_pdf(2, Some("Gone soon"));
        assert!(info(&src).unwrap().title.is_some());
        let out = strip_metadata(&src).unwrap();
        let after = info(&out).unwrap();
        assert_eq!(after.title, None);
        assert_eq!(after.author, None);
        // Structurally untouched.
        assert_eq!(after.page_count, 2);
    }

    #[test]
    fn strip_metadata_on_a_file_with_no_info_dict_is_not_an_error() {
        let src = build_pdf(1, None);
        let out = strip_metadata(&src).unwrap();
        assert_eq!(page_count(&out).unwrap(), 1);
    }

    /// A catalog-level `/Metadata` XMP stream reference, the shape this
    /// repository's OWN committed fixtures actually use: grepping the raw
    /// bytes of the 34 PDFs committed in this repo (2026-09-25) for
    /// `/Metadata` finds it in 7 of them, all under
    /// `example_codes/crest_factor_reduction_project_vf/` (`density.pdf`,
    /// `freqdomain.pdf`, `qinfluence.pdf`, `spklkg.pdf`,
    /// `numberfolds.pdf`, `numberfoldsv.pdf`, `deblur_Z_comp2.pdf`), each
    /// as `<</Length .../Subtype/XML/Type/Metadata>>stream` hung off the
    /// catalog exactly like the one built here -- so this is a synthetic
    /// fixture of a real, present shape, not a hypothetical one.
    fn build_pdf_with_xmp_metadata() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content = Content { operations: vec![] };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! {},
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let xmp = b"<?xpacket begin=\"\"?><x:xmpmeta></x:xmpmeta>".to_vec();
        let metadata_id = doc.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            xmp,
        ));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Metadata" => metadata_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn strip_metadata_removes_the_catalog_xmp_metadata_reference() {
        let src = build_pdf_with_xmp_metadata();
        let before = Document::load_mem(&src).unwrap();
        assert!(before.catalog().unwrap().has(b"Metadata"));
        let out = strip_metadata(&src).unwrap();
        let after = Document::load_mem(&out).unwrap();
        assert!(!after.catalog().unwrap().has(b"Metadata"));
        assert_eq!(page_count(&out).unwrap(), 1);
    }

    /// The real committed fixtures that carry a catalog `/Metadata`
    /// reference, exercised for real rather than only via the synthetic
    /// fixture above -- when present in this checkout (see the
    /// `example_codes/`-exclusion skip pattern used elsewhere in this
    /// file for why "when present" rather than unconditionally).
    #[test]
    fn strip_metadata_removes_xmp_from_a_real_committed_fixture() {
        let p = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../example_codes/crest_factor_reduction_project_vf/density.pdf"
        ));
        if !p.exists() {
            eprintln!("skipping strip_metadata_removes_xmp_from_a_real_committed_fixture: fixture not present in this tree");
            return;
        }
        let src = std::fs::read(p).unwrap();
        assert!(
            Document::load_mem(&src).unwrap().catalog().unwrap().has(b"Metadata"),
            "fixture is expected to carry a catalog /Metadata reference"
        );
        let out = strip_metadata(&src).unwrap();
        assert!(!Document::load_mem(&out).unwrap().catalog().unwrap().has(b"Metadata"));
    }

    // ---- find_text ----

    #[test]
    fn find_text_finds_the_known_page() {
        let src = build_pdf(3, None); // pages say PAGE-1, PAGE-2, PAGE-3
        let hits = find_text(&src, "PAGE-2", true).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].page, 2);
        assert_eq!(hits[0].length, "PAGE-2".chars().count());
    }

    #[test]
    fn find_text_is_case_insensitive_when_asked() {
        let src = build_pdf(1, None); // PAGE-1
        assert!(find_text(&src, "page-1", true).unwrap().is_empty());
        let hits = find_text(&src, "page-1", false).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].page, 1);
    }

    /// The offset is checked against `extract_text`'s OWN output, not
    /// against a hand-picked number -- so this cannot pass by `find_text`
    /// and its own idea of the text agreeing with each other while both
    /// are wrong.
    #[test]
    fn find_text_offset_points_at_the_real_match() {
        let src = build_pdf_with_texts(&["some text with a needle in it"]);
        let full = extract_text(&src, Some(&[1])).unwrap();
        let hits = find_text(&src, "needle", true).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        let chars: Vec<char> = full.chars().collect();
        let end = hits[0].offset + hits[0].length;
        assert!(end <= chars.len(), "{:?} vs {} chars", hits[0], chars.len());
        let got: String = chars[hits[0].offset..end].iter().collect();
        assert_eq!(got, "needle");
    }

    #[test]
    fn find_text_of_an_empty_query_is_an_error() {
        let src = build_pdf(1, None);
        let err = find_text(&src, "", true).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    // ---- text_diff ----

    /// Like `build_pdf`, but each page's text is a caller-chosen string
    /// instead of the fixed `"PAGE-N"` scheme -- `text_diff` needs two
    /// documents whose pages say different, KNOWN things, not merely a
    /// subsequence of one numbering.
    fn build_pdf_with_texts(texts: &[&str]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut kids = Vec::new();
        for text in texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            });
            kids.push(page_id.into());
        }
        let count = kids.len() as u32;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn text_diff_finds_pages_with_changed_text_only() {
        let a = build_pdf_with_texts(&["one", "two", "three"]);
        let b = build_pdf_with_texts(&["one", "TWO-CHANGED", "three"]);
        let diff = text_diff(&a, &b).unwrap();
        assert_eq!(diff.page_count_a, 3);
        assert_eq!(diff.page_count_b, 3);
        assert_eq!(diff.differing_pages.len(), 1, "{:?}", diff.differing_pages);
        assert_eq!(diff.differing_pages[0].page, 2);
        assert!(!diff.differing_pages[0].only_in_a);
        assert!(!diff.differing_pages[0].only_in_b);
        let added = diff.differing_pages[0].lines.iter().any(
            |l| matches!(l, DiffLine::Added(s) if s.contains("TWO-CHANGED")),
        );
        let removed = diff.differing_pages[0].lines.iter().any(
            |l| matches!(l, DiffLine::Removed(s) if s.contains("two") && !s.contains("TWO")),
        );
        assert!(added, "{:?}", diff.differing_pages[0].lines);
        assert!(removed, "{:?}", diff.differing_pages[0].lines);
    }

    #[test]
    fn text_diff_reports_a_page_added_in_b() {
        let a = build_pdf_with_texts(&["one"]);
        let b = build_pdf_with_texts(&["one", "two"]);
        let diff = text_diff(&a, &b).unwrap();
        assert_eq!(diff.page_count_a, 1);
        assert_eq!(diff.page_count_b, 2);
        assert_eq!(diff.differing_pages.len(), 1, "{:?}", diff.differing_pages);
        assert_eq!(diff.differing_pages[0].page, 2);
        assert!(diff.differing_pages[0].only_in_b);
        assert!(!diff.differing_pages[0].only_in_a);
    }

    #[test]
    fn text_diff_of_identical_documents_is_empty() {
        let a = build_pdf_with_texts(&["same", "same too"]);
        let b = build_pdf_with_texts(&["same", "same too"]);
        let diff = text_diff(&a, &b).unwrap();
        assert!(diff.differing_pages.is_empty(), "{:?}", diff.differing_pages);
    }

    // ---- structural_diff ----

    /// Like `build_pdf`, but the `/MediaBox` size is a parameter and page
    /// 1 optionally carries one `/Annots` entry, the catalog optionally
    /// an `/Outlines` reference -- just enough real structure for
    /// `structural_diff` to have something to compare. Nothing here reads
    /// the outline's own contents; only ITS PRESENCE is ever checked, by
    /// `structural_diff` or by these tests.
    fn build_pdf_with_structure(
        n: usize,
        w: f64,
        h: f64,
        annotate_page_1: bool,
        with_outline: bool,
    ) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for i in 0..n {
            let content = Content { operations: vec![] };
            let content_id =
                doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let mut page_dict = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => dictionary! {},
                "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            };
            if annotate_page_1 && i == 0 {
                let annot_id = doc.add_object(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Text",
                    "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                });
                page_dict.set("Annots", vec![Object::Reference(annot_id)]);
            }
            let page_id = doc.add_object(page_dict);
            kids.push(page_id.into());
        }
        let count = kids.len() as u32;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let mut catalog = dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        };
        if with_outline {
            let outline_id = doc.add_object(dictionary! {
                "Type" => "Outlines",
                "Count" => 0,
            });
            catalog.set("Outlines", outline_id);
        }
        let catalog_id = doc.add_object(catalog);
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn structural_diff_flags_a_page_count_difference() {
        let a = build_pdf(2, None);
        let b = build_pdf(3, None);
        let diff = structural_diff(&a, &b).unwrap();
        assert_eq!(diff.page_count_a, 2);
        assert_eq!(diff.page_count_b, 3);
        assert!(
            diff.differing_dimensions
                .iter()
                .any(|d| d.page == 3 && d.a.is_none() && d.b.is_some()),
            "{:?}",
            diff.differing_dimensions
        );
    }

    #[test]
    fn structural_diff_flags_a_page_size_difference() {
        // A4 vs US Letter, in PDF points.
        let a = build_pdf_with_structure(1, 595.0, 842.0, false, false);
        let b = build_pdf_with_structure(1, 612.0, 792.0, false, false);
        let diff = structural_diff(&a, &b).unwrap();
        assert_eq!(diff.page_count_a, 1);
        assert_eq!(diff.page_count_b, 1);
        assert_eq!(diff.differing_dimensions.len(), 1, "{:?}", diff.differing_dimensions);
        assert_eq!(diff.differing_dimensions[0].a, Some((595.0, 842.0)));
        assert_eq!(diff.differing_dimensions[0].b, Some((612.0, 792.0)));
    }

    #[test]
    fn structural_diff_identical_documents_have_no_dimension_differences() {
        let a = build_pdf_with_structure(2, 595.0, 842.0, false, false);
        let b = build_pdf_with_structure(2, 595.0, 842.0, false, false);
        let diff = structural_diff(&a, &b).unwrap();
        assert!(diff.differing_dimensions.is_empty(), "{:?}", diff.differing_dimensions);
    }

    #[test]
    fn structural_diff_reports_bookmarks_and_annotation_counts() {
        let a = build_pdf_with_structure(1, 595.0, 842.0, false, false);
        let b = build_pdf_with_structure(1, 595.0, 842.0, true, true);
        let diff = structural_diff(&a, &b).unwrap();
        assert!(!diff.has_bookmarks_a);
        assert!(diff.has_bookmarks_b);
        assert_eq!(diff.annotation_count_a, 0);
        assert_eq!(diff.annotation_count_b, 1);
    }

    // ── outlines / bookmarks ─────────────────────────────────────────

    // None of this repository's 34 committed PDF fixtures carry a real
    // outline tree, a real annotation, or a real embedded file (checked
    // by hand, 2026-09-25, `grep -al` for `/Outlines`, `/Annots`,
    // `/EmbeddedFiles` across all of them came back empty) -- so every
    // test below builds its own minimal fixture rather than depending on
    // one turning up by chance.

    #[test]
    fn outlines_on_a_plain_pdf_is_empty_not_an_error() {
        let src = build_pdf(2, None);
        assert_eq!(outlines(&src).unwrap(), Vec::new());
    }

    #[test]
    fn add_bookmark_then_outlines_round_trips_title_level_and_page() {
        let src = build_pdf(3, None);
        let with_one = add_bookmark(&src, "Chapter 1", 1, None).unwrap();
        let rows = outlines(&with_one).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].title, "Chapter 1");
        assert_eq!(rows[0].level, 0);
        assert_eq!(rows[0].parent, None);
        assert_eq!(rows[0].page, Some(1));
    }

    #[test]
    fn add_bookmark_appends_as_a_sibling_not_a_replacement() {
        // The bug `append_outline_child` exists to avoid: a naive
        // `/First = new` would silently drop the entry added before it.
        let src = build_pdf(3, None);
        let with_one = add_bookmark(&src, "First", 1, None).unwrap();
        let with_two = add_bookmark(&with_one, "Second", 2, None).unwrap();
        let rows = outlines(&with_two).unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].title, "First");
        assert_eq!(rows[0].page, Some(1));
        assert_eq!(rows[1].title, "Second");
        assert_eq!(rows[1].page, Some(2));
        // Both top-level -- neither nested under the other.
        assert_eq!(rows[0].parent, None);
        assert_eq!(rows[1].parent, None);
    }

    #[test]
    fn add_bookmark_nests_under_an_existing_entry_by_its_outlines_index() {
        let src = build_pdf(4, None);
        let with_top = add_bookmark(&src, "Part I", 1, None).unwrap();
        // `parent = 0` names the entry `outlines()` just reported at
        // index 0 -- the one stable handle this crate exposes.
        let with_child = add_bookmark(&with_top, "Section 1.1", 2, Some(0)).unwrap();
        let rows = outlines(&with_child).unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].title, "Part I");
        assert_eq!(rows[0].level, 0);
        assert_eq!(rows[1].title, "Section 1.1");
        assert_eq!(rows[1].level, 1);
        assert_eq!(rows[1].parent, Some(0));
        assert_eq!(rows[1].page, Some(2));
    }

    #[test]
    fn add_bookmark_rejects_a_page_that_is_not_there() {
        let src = build_pdf(2, None);
        let err = add_bookmark(&src, "Nope", 9, None).unwrap_err();
        assert!(err.contains("no page 9"), "{err}");
    }

    #[test]
    fn add_bookmark_rejects_an_unknown_parent_index() {
        let src = build_pdf(2, None);
        let err = add_bookmark(&src, "Orphan", 1, Some(3)).unwrap_err();
        assert!(err.contains("no outline entry at index 3"), "{err}");
    }

    // ── annotations / links ──────────────────────────────────────────

    #[test]
    fn annotations_on_a_plain_page_is_empty_not_an_error() {
        let src = build_pdf(1, None);
        assert_eq!(annotations(&src, 1).unwrap(), Vec::new());
    }

    #[test]
    fn add_annotation_then_annotations_round_trips_type_rect_and_text() {
        let src = build_pdf(1, None);
        let rect = [10.0, 20.0, 110.0, 60.0];
        let out = add_annotation(&src, 1, "Text", rect, Some("a note")).unwrap();
        let rows = annotations(&out, 1).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].subtype, "Text");
        assert_eq!(rows[0].rect, rect);
        assert_eq!(rows[0].contents.as_deref(), Some("a note"));
    }

    #[test]
    fn add_annotation_without_text_reads_back_with_no_contents() {
        let src = build_pdf(1, None);
        let out = add_annotation(&src, 1, "Square", [0.0, 0.0, 50.0, 50.0], None).unwrap();
        let rows = annotations(&out, 1).unwrap();
        assert_eq!(rows[0].contents, None);
    }

    #[test]
    fn remove_annotation_removes_the_right_one_and_keeps_the_rest() {
        let src = build_pdf(1, None);
        let with_a = add_annotation(&src, 1, "Text", [0.0, 0.0, 10.0, 10.0], Some("A")).unwrap();
        let with_b = add_annotation(&with_a, 1, "Text", [0.0, 0.0, 20.0, 20.0], Some("B")).unwrap();
        let with_c = add_annotation(&with_b, 1, "Text", [0.0, 0.0, 30.0, 30.0], Some("C")).unwrap();
        // Remove the MIDDLE one -- the check that actually distinguishes
        // "removed by index" from "removed by, say, always dropping the
        // last".
        let after = remove_annotation(&with_c, 1, 1).unwrap();
        let rows = annotations(&after, 1).unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].contents.as_deref(), Some("A"));
        assert_eq!(rows[1].contents.as_deref(), Some("C"));
    }

    #[test]
    fn remove_annotation_out_of_range_is_an_error() {
        let src = build_pdf(1, None);
        let err = remove_annotation(&src, 1, 0).unwrap_err();
        assert!(err.contains("no annotation 0"), "{err}");
    }

    #[test]
    fn add_link_points_at_the_target_page_in_the_same_document() {
        let src = build_pdf(3, None);
        let out = add_link(&src, 1, [0.0, 0.0, 100.0, 20.0], 3).unwrap();
        let rows = annotations(&out, 1).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].subtype, "Link");

        // The structural check `annotations()` doesn't surface (it reads
        // type/rect/contents, not `/Dest`): load the result directly and
        // confirm the link's destination really is page 3's own object,
        // not just that SOME link annotation exists.
        let doc = Document::load_mem(&out).unwrap();
        let page1 = *doc.get_pages().get(&1).unwrap();
        let page3 = *doc.get_pages().get(&3).unwrap();
        let annots = doc.get_dictionary(page1).unwrap().get(b"Annots").unwrap().as_array().unwrap();
        let link = doc.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
        let dest = link.get(b"Dest").unwrap().as_array().unwrap();
        assert_eq!(dest[0].as_reference().unwrap(), page3);
    }

    #[test]
    fn add_link_rejects_a_target_page_that_is_not_there() {
        let src = build_pdf(2, None);
        let err = add_link(&src, 1, [0.0, 0.0, 10.0, 10.0], 9).unwrap_err();
        assert!(err.contains("no page 9"), "{err}");
    }

    // ── attachments ───────────────────────────────────────────────────

    #[test]
    fn attachments_on_a_plain_pdf_is_empty_not_an_error() {
        let src = build_pdf(1, None);
        assert_eq!(attachments(&src).unwrap(), Vec::new());
    }

    #[test]
    fn add_attachment_then_attachments_and_extract_attachment_round_trip() {
        let src = build_pdf(1, None);
        let payload = b"line one\nline two\n".to_vec();
        let out = add_attachment(&src, "notes.txt", &payload).unwrap();

        let listed = attachments(&out).unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].filename, "notes.txt");
        assert_eq!(listed[0].size, payload.len());

        let extracted = extract_attachment(&out, "notes.txt").unwrap();
        assert_eq!(extracted, payload);

        // The rest of the document is untouched by embedding a file into
        // it -- same page count, same page content.
        assert_eq!(page_count(&out).unwrap(), 1);
    }

    #[test]
    fn add_attachment_twice_keeps_both_and_extract_reads_the_first() {
        let src = build_pdf(1, None);
        let with_one = add_attachment(&src, "a.bin", b"AAA").unwrap();
        let with_two = add_attachment(&with_one, "b.bin", b"BBB").unwrap();
        let listed = attachments(&with_two).unwrap();
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert_eq!(extract_attachment(&with_two, "a.bin").unwrap(), b"AAA");
        assert_eq!(extract_attachment(&with_two, "b.bin").unwrap(), b"BBB");
    }

    #[test]
    fn extract_attachment_of_an_unknown_name_is_an_error() {
        let src = build_pdf(1, None);
        let with_one = add_attachment(&src, "a.bin", b"AAA").unwrap();
        let err = extract_attachment(&with_one, "missing.bin").unwrap_err();
        assert!(err.contains("no attachment named `missing.bin`"), "{err}");
    }

    #[test]
    fn extract_attachment_on_a_document_with_none_is_an_error() {
        let src = build_pdf(1, None);
        let err = extract_attachment(&src, "whatever").unwrap_err();
        assert!(err.contains("this document has none"), "{err}");
    }

    /// Like `build_pdf`, but with a real TWO-level page tree: an
    /// intermediate `/Pages` node carries `/MediaBox`, and the leaf page
    /// dicts carry none of their own -- the shape `media_box`'s own doc
    /// comment explains is legal and common (every page in a document is
    /// usually the same size, so a real producer sets `/MediaBox` once on
    /// an ancestor rather than repeating it on every leaf). Exists to
    /// prove the geometry readers -- and, more importantly, the
    /// tree-editing functions that flatten the tree and therefore change
    /// what a page's ancestors ARE -- actually resolve and preserve this,
    /// rather than assuming every page already carries its own box.
    fn build_pdf_with_inherited_media_box(n: usize, mb: [f64; 4]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let root_id = doc.new_object_id();
        let mid_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut kids = Vec::new();
        for i in 0..n {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new(
                        "Tj",
                        vec![Object::string_literal(format!("PAGE-{}", i + 1))],
                    ),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => mid_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                // Deliberately NO "MediaBox" here -- it lives on `mid_id`.
            });
            kids.push(page_id.into());
        }
        let count = kids.len() as u32;
        doc.objects.insert(
            mid_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => kids,
                "Count" => count,
                "MediaBox" => vec![mb[0].into(), mb[1].into(), mb[2].into(), mb[3].into()],
            }),
        );
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(mid_id)],
                "Count" => count,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => root_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Cursor::new(Vec::new());
        doc.save_to(&mut buf).unwrap();
        buf.into_inner()
    }

    #[test]
    fn media_box_resolves_a_value_inherited_from_an_intermediate_pages_node() {
        let src = build_pdf_with_inherited_media_box(3, [0.0, 0.0, 595.0, 842.0]);
        let b = media_box(&src, 2).unwrap();
        assert_eq!(
            b,
            PageBox { x0: 0.0, y0: 0.0, x1: 595.0, y1: 842.0 }
        );
    }

    #[test]
    fn media_box_rejects_a_page_that_is_not_there() {
        let src = build_pdf(2, None);
        let err = media_box(&src, 9).unwrap_err();
        assert!(err.contains("no page 9"), "{err}");
    }

    #[test]
    fn crop_box_falls_back_to_media_box_when_no_crop_box_is_set() {
        let src = build_pdf(2, None);
        let b = crop_box(&src, 1).unwrap();
        assert_eq!(
            b,
            PageBox { x0: 0.0, y0: 0.0, x1: 595.0, y1: 842.0 }
        );
    }

    #[test]
    fn rotate_page_increments_and_a_second_90_makes_180() {
        let src = build_pdf(2, None);
        let once = rotate_page(&src, 1, 90).unwrap();
        let twice = rotate_page(&once, 1, 90).unwrap();
        let doc = Document::load_mem(&twice).unwrap();
        let id = *doc.get_pages().get(&1).unwrap();
        let rotate = doc.get_dictionary(id).unwrap().get(b"Rotate").unwrap().as_i64().unwrap();
        assert_eq!(rotate, 180);
    }

    #[test]
    fn rotate_page_wraps_past_360_back_to_0() {
        let src = build_pdf(1, None);
        let out = rotate_page(&src, 1, 360).unwrap();
        let doc = Document::load_mem(&out).unwrap();
        let id = *doc.get_pages().get(&1).unwrap();
        let rotate = doc.get_dictionary(id).unwrap().get(b"Rotate").unwrap().as_i64().unwrap();
        assert_eq!(rotate, 0);
    }

    #[test]
    fn rotate_page_rejects_a_non_multiple_of_90() {
        let src = build_pdf(1, None);
        let err = rotate_page(&src, 1, 45).unwrap_err();
        assert!(err.contains("multiple of 90"), "{err}");
    }

    #[test]
    fn crop_page_sets_a_valid_crop_box_and_leaves_the_media_box_alone() {
        let src = build_pdf(1, None); // MediaBox 0,0,595,842
        let out = crop_page(&src, 1, 10.0, 10.0, 500.0, 800.0).unwrap();
        assert_eq!(
            crop_box(&out, 1).unwrap(),
            PageBox { x0: 10.0, y0: 10.0, x1: 500.0, y1: 800.0 }
        );
        assert_eq!(
            media_box(&out, 1).unwrap(),
            PageBox { x0: 0.0, y0: 0.0, x1: 595.0, y1: 842.0 }
        );
    }

    #[test]
    fn crop_page_rejects_an_inverted_rectangle() {
        let src = build_pdf(1, None);
        let err = crop_page(&src, 1, 100.0, 100.0, 50.0, 50.0).unwrap_err();
        assert!(err.contains("not a rectangle"), "{err}");
    }

    #[test]
    fn crop_page_rejects_a_rectangle_outside_the_media_box() {
        let src = build_pdf(1, None); // MediaBox 0,0,595,842
        let err = crop_page(&src, 1, -10.0, 0.0, 500.0, 800.0).unwrap_err();
        assert!(err.contains("falls outside"), "{err}");
    }

    #[test]
    fn add_page_inserts_a_blank_page_at_the_given_position() {
        let src = build_pdf(3, None); // PAGE-1, PAGE-2, PAGE-3
        let out = add_page(&src, 2, 300.0, 400.0).unwrap();
        assert_eq!(page_count(&out).unwrap(), 4);
        assert_eq!(
            media_box(&out, 2).unwrap(),
            PageBox { x0: 0.0, y0: 0.0, x1: 300.0, y1: 400.0 }
        );
        // The new page is genuinely blank...
        assert_eq!(extract_text(&out, Some(&[2])).unwrap().trim(), "");
        // ...and the pages that followed shifted down by one, keeping
        // their own content.
        assert!(extract_text(&out, Some(&[1])).unwrap().contains("PAGE-1"));
        assert!(extract_text(&out, Some(&[3])).unwrap().contains("PAGE-2"));
        assert!(extract_text(&out, Some(&[4])).unwrap().contains("PAGE-3"));
    }

    #[test]
    fn add_page_can_append_one_past_the_last_page() {
        let src = build_pdf(2, None);
        let out = add_page(&src, 3, 200.0, 200.0).unwrap();
        assert_eq!(page_count(&out).unwrap(), 3);
        assert_eq!(
            media_box(&out, 3).unwrap(),
            PageBox { x0: 0.0, y0: 0.0, x1: 200.0, y1: 200.0 }
        );
    }

    #[test]
    fn add_page_rejects_an_index_out_of_range() {
        let src = build_pdf(2, None);
        let err = add_page(&src, 4, 200.0, 200.0).unwrap_err();
        assert!(err.contains("index 4"), "{err}");
    }

    #[test]
    fn add_page_rejects_a_non_positive_size() {
        let src = build_pdf(1, None);
        let err = add_page(&src, 1, 0.0, 100.0).unwrap_err();
        assert!(err.contains("must be positive"), "{err}");
    }

    #[test]
    fn delete_page_removes_exactly_the_right_page() {
        let src = build_pdf(3, None);
        let out = delete_page(&src, 2).unwrap();
        assert_eq!(page_count(&out).unwrap(), 2);
        let text = extract_text(&out, None).unwrap();
        assert!(text.contains("PAGE-1"), "{text:?}");
        assert!(!text.contains("PAGE-2"), "{text:?}");
        assert!(text.contains("PAGE-3"), "{text:?}");
    }

    #[test]
    fn delete_page_refuses_to_empty_a_single_page_document() {
        let src = build_pdf(1, None);
        let err = delete_page(&src, 1).unwrap_err();
        assert!(err.contains("only one page"), "{err}");
    }

    #[test]
    fn duplicate_page_inserts_a_copy_right_after_the_original() {
        let src = build_pdf(3, None);
        let out = duplicate_page(&src, 2).unwrap();
        assert_eq!(page_count(&out).unwrap(), 4);
        assert!(extract_text(&out, Some(&[1])).unwrap().contains("PAGE-1"));
        assert!(extract_text(&out, Some(&[2])).unwrap().contains("PAGE-2"));
        assert!(extract_text(&out, Some(&[3])).unwrap().contains("PAGE-2")); // the copy
        assert!(extract_text(&out, Some(&[4])).unwrap().contains("PAGE-3"));
    }

    #[test]
    fn move_page_relocates_without_disturbing_the_relative_order_of_the_rest() {
        let src = build_pdf(4, None); // 1,2,3,4
        let out = move_page(&src, 4, 1).unwrap(); // expect: 4,1,2,3
        assert_eq!(page_count(&out).unwrap(), 4);
        for (i, want) in ["PAGE-4", "PAGE-1", "PAGE-2", "PAGE-3"].iter().enumerate() {
            let t = extract_text(&out, Some(&[(i + 1) as u32])).unwrap();
            assert!(t.contains(want), "page {}: {t:?}", i + 1);
        }
    }

    #[test]
    fn move_page_rejects_an_out_of_range_target() {
        let src = build_pdf(3, None);
        let err = move_page(&src, 1, 9).unwrap_err();
        assert!(err.contains("no page 9"), "{err}");
    }

    #[test]
    fn reverse_pages_puts_the_last_page_first() {
        let src = build_pdf(3, None);
        let out = reverse_pages(&src).unwrap();
        assert_eq!(page_count(&out).unwrap(), 3);
        assert!(extract_text(&out, Some(&[1])).unwrap().contains("PAGE-3"));
        assert!(extract_text(&out, Some(&[2])).unwrap().contains("PAGE-2"));
        assert!(extract_text(&out, Some(&[3])).unwrap().contains("PAGE-1"));
    }

    #[test]
    fn split_every_chunks_pages_in_order_with_a_shorter_last_chunk() {
        let src = build_pdf(5, None);
        let parts = split_every(&src, 2).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(page_count(&parts[0]).unwrap(), 2);
        assert_eq!(page_count(&parts[1]).unwrap(), 2);
        assert_eq!(page_count(&parts[2]).unwrap(), 1);
        assert!(extract_text(&parts[0], None).unwrap().contains("PAGE-1"));
        assert!(extract_text(&parts[2], None).unwrap().contains("PAGE-5"));
    }

    #[test]
    fn split_at_splits_into_two_non_empty_documents() {
        let src = build_pdf(5, None);
        let (a, b) = split_at(&src, 3).unwrap();
        assert_eq!(page_count(&a).unwrap(), 2);
        assert_eq!(page_count(&b).unwrap(), 3);
        let ta = extract_text(&a, None).unwrap();
        assert!(ta.contains("PAGE-1") && ta.contains("PAGE-2") && !ta.contains("PAGE-3"), "{ta:?}");
        let tb = extract_text(&b, None).unwrap();
        assert!(tb.contains("PAGE-3") && tb.contains("PAGE-5"), "{tb:?}");
    }

    #[test]
    fn split_at_rejects_a_boundary_that_would_leave_a_half_empty() {
        let src = build_pdf(3, None);
        // index=1 -- the first half (pages 1..1) would be empty.
        assert!(split_at(&src, 1).unwrap_err().contains("must be 2.."));
        // index=4 -- past the last page entirely (the valid range for a
        // 3-page document is 2..=3: index=3 is fine, it makes the second
        // half exactly page 3, still non-empty).
        assert!(split_at(&src, 4).unwrap_err().contains("must be 2.."));
    }

    /// The regression `rebuild_flat_page_tree` would have without baking
    /// inherited attributes first: every tree-editing op reparents pages
    /// directly under the root `/Pages` node, so a page that relied on an
    /// INTERMEDIATE ancestor's `/MediaBox` (never the root's, which here
    /// has none at all) would silently lose its size the moment it moved
    /// -- unless that value is resolved and copied onto the leaf first.
    #[test]
    fn tree_edits_preserve_an_inherited_media_box_after_flattening() {
        let src = build_pdf_with_inherited_media_box(3, [0.0, 0.0, 300.0, 400.0]);
        // Sanity: the source really does rely on inheritance, not its own.
        let doc = Document::load_mem(&src).unwrap();
        let leaf_id = *doc.get_pages().get(&2).unwrap();
        assert!(!doc.get_dictionary(leaf_id).unwrap().has(b"MediaBox"));

        let out = reverse_pages(&src).unwrap();
        for p in 1..=3u32 {
            assert_eq!(
                media_box(&out, p).unwrap(),
                PageBox { x0: 0.0, y0: 0.0, x1: 300.0, y1: 400.0 },
                "page {p}"
            );
        }
    }
}
