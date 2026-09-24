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

use lopdf::{Document, Object};

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

fn save(doc: &mut Document) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    doc.save_to(&mut buf)
        .map_err(|err| format!("could not write PDF: {err}"))?;
    Ok(buf.into_inner())
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
        // Absent, not empty: the fixture writes no /Subject at all.
        assert_eq!(got.subject, None);
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
}
