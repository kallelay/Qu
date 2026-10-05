//! Structure editing: hyperlinks, comments, bookmarks and cross-references,
//! fields (table of contents, page numbers), headers and footers, sections
//! and page setup, and bulleted/numbered lists.
//!
//! The same rule as the rest of the crate: an operation parses and rewrites
//! only the parts it has to (the main document, plus the one of `styles`,
//! `settings`, `numbering`, `comments` or a header/footer it needs), and
//! creates a missing part -- with its content type and relationship --
//! rather than assuming a template put it there. New elements are inserted
//! where the schema's sequence puts them (`w:sectPr`, `w:pPr`, `w:settings`
//! are ordered, and Word refuses a file that gets the order wrong).
//!
//! Fields are written dirty (`w:dirty="true"`) with `w:updateFields` set
//! for a table of contents, so the word processor computes them on open.
//! Nothing here pretends to lay out pages: no TOC entries or page numbers
//! are invented.

use super::*;

pub(crate) const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const CT_BASE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

/// `CT_SectPr` child order (header and footer references share rank 0).
const SECT_ORDER: &[&str] = &[
    "w:headerReference", "w:footnotePr", "w:endnotePr", "w:type", "w:pgSz", "w:pgMar", "w:paperSrc", "w:pgBorders", "w:lnNumType",
    "w:pgNumType", "w:cols", "w:formProt", "w:vAlign", "w:noEndnote", "w:titlePg", "w:textDirection", "w:bidi", "w:rtlGutter",
    "w:docGrid", "w:printerSettings", "w:sectPrChange",
];

/// `CT_PPr` child order.
pub(crate) const PPR_ORDER: &[&str] = &[
    "w:pStyle", "w:keepNext", "w:keepLines", "w:pageBreakBefore", "w:framePr", "w:widowControl", "w:numPr", "w:suppressLineNumbers",
    "w:pBdr", "w:shd", "w:tabs", "w:suppressAutoHyphens", "w:kinsoku", "w:wordWrap", "w:overflowPunct", "w:topLinePunct",
    "w:autoSpaceDE", "w:autoSpaceDN", "w:bidi", "w:adjustRightInd", "w:snapToGrid", "w:spacing", "w:ind", "w:contextualSpacing",
    "w:mirrorIndents", "w:suppressOverlap", "w:jc", "w:textDirection", "w:textAlignment", "w:textboxTightWrap", "w:outlineLvl",
    "w:divId", "w:cnfStyle", "w:rPr", "w:sectPr", "w:pPrChange",
];

/// `CT_Settings` child order.
pub(crate) const SETTINGS_ORDER: &[&str] = &[
    "w:writeProtection", "w:view", "w:zoom", "w:removePersonalInformation", "w:removeDateAndTime", "w:doNotDisplayPageBoundaries",
    "w:displayBackgroundShape", "w:printPostScriptOverText", "w:printFractionalCharacterWidth", "w:printFormsData",
    "w:embedTrueTypeFonts", "w:embedSystemFonts", "w:saveSubsetFonts", "w:saveFormsData", "w:mirrorMargins",
    "w:alignBordersAndEdges", "w:bordersDoNotSurroundHeader", "w:bordersDoNotSurroundFooter", "w:gutterAtTop",
    "w:hideSpellingErrors", "w:hideGrammaticalErrors", "w:activeWritingStyle", "w:proofState", "w:formsDesign",
    "w:attachedTemplate", "w:linkStyles", "w:stylePaneFormatFilter", "w:stylePaneSortMethod", "w:documentType", "w:mailMerge",
    "w:revisionView", "w:trackRevisions", "w:doNotTrackMoves", "w:doNotTrackFormatting", "w:documentProtection",
    "w:autoFormatOverride", "w:styleLockTheme", "w:styleLockQFSet", "w:defaultTabStop", "w:autoHyphenation",
    "w:consecutiveHyphenLimit", "w:hyphenationZone", "w:doNotHyphenateCaps", "w:showEnvelope", "w:summaryLength",
    "w:clickAndTypeStyle", "w:defaultTableStyle", "w:evenAndOddHeaders", "w:bookFoldRevPrinting", "w:bookFoldPrinting",
    "w:bookFoldPrintingSheets", "w:drawingGridHorizontalSpacing", "w:drawingGridVerticalSpacing",
    "w:displayHorizontalDrawingGridEvery", "w:displayVerticalDrawingGridEvery", "w:doNotUseMarginsForDrawingGridOrigin",
    "w:drawingGridHorizontalOrigin", "w:drawingGridVerticalOrigin", "w:doNotShadeFormData", "w:noPunctuationKerning",
    "w:characterSpacingControl", "w:printTwoOnOne", "w:strictFirstAndLastChars", "w:noLineBreaksAfter", "w:noLineBreaksBefore",
    "w:savePreviewPicture", "w:doNotValidateAgainstSchema", "w:saveInvalidXml", "w:ignoreMixedContent",
    "w:alwaysShowPlaceholderText", "w:doNotDemarcateInvalidXml", "w:saveXmlDataOnly", "w:useXSLTWhenSaving", "w:saveThroughXslt",
    "w:showXMLTags", "w:alwaysMergeEmptyNamespace", "w:updateFields", "w:hdrShapeDefaults", "w:footnotePr", "w:endnotePr",
    "w:compat", "w:docVars", "w:rsids", "m:mathPr", "w:attachedSchema", "w:themeFontLang", "w:clrSchemeMapping",
    "w:doNotIncludeSubdocsInStats", "w:doNotAutoCompressPictures", "w:forceUpgrade", "w:captions", "w:readModeInkLockDown",
    "w:smartTagType", "sl:schemaLibrary", "w:shapeDefaults", "w:doNotEmbedSmartTags", "w:decimalSymbol", "w:listSeparator",
];

/// What a new table of contents shows until the word processor builds it.
/// Word replaces it on open (the field is dirty and `w:updateFields` is
/// set). LibreOffice does NOT rebuild a TOC when it loads a .docx -- checked
/// 2026-09-30 with and without Word's `w:sdt` "Table of Contents" wrapper --
/// so there the reader needs to be told, rather than shown an empty gap or
/// invented entries.
pub const TOC_PLACEHOLDER: &str = "Table of contents -- update fields to build it (Word: F9; LibreOffice: Tools > Update > Update All).";

/// Page sizes by name, portrait, millimetres.
const PAGE_SIZES: &[(&str, f64, f64)] = &[
    ("a3", 297.0, 420.0),
    ("a4", 210.0, 297.0),
    ("a5", 148.0, 210.0),
    ("b5", 176.0, 250.0),
    ("letter", 215.9, 279.4),
    ("legal", 215.9, 355.6),
];

/// One section's page setup, as `sections()` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionInfo {
    pub width_mm: f64,
    pub height_mm: f64,
    pub landscape: bool,
    pub top_mm: f64,
    pub bottom_mm: f64,
    pub left_mm: f64,
    pub right_mm: f64,
    /// How the section starts: `next_page`, `continuous`, `even_page`,
    /// `odd_page`, `next_column`.
    pub start: String,
    /// Header kinds (`default`, `first`, `even`) the section defines
    /// itself; the others are inherited from the section before it.
    pub headers: Vec<String>,
    pub footers: Vec<String>,
}

impl Document {
    // ------------------------------------------------------------ links

    /// `(text, url)` for every hyperlink in the body (tables included);
    /// an internal link to a bookmark comes back as `#name`.
    pub fn links(&self) -> Vec<(String, String)> {
        let rels = self.pkg.rels(&self.main).unwrap_or_default();
        self.body()
            .find_all("w:hyperlink")
            .into_iter()
            .map(|h| {
                let target = h.attr("r:id").and_then(|id| rels.iter().find(|r| r.id == id)).map(|r| r.target.clone()).unwrap_or_default();
                let url = match h.attr("w:anchor") {
                    Some(a) if target.is_empty() => format!("#{a}"),
                    Some(a) => format!("{target}#{a}"),
                    None => target,
                };
                (para_display_text(h), url)
            })
            .collect()
    }

    /// Make text in body paragraph `i` a hyperlink to `url` -- all of it,
    /// or the first occurrence of `on`. A `url` starting with `#` links to
    /// a bookmark in the document.
    pub fn add_link(&mut self, i: usize, url: &str, on: Option<&str>) -> Result<(), String> {
        if url.is_empty() || url == "#" {
            return Err("the link target is empty".into());
        }
        let slot = self.para_slot(i)?;
        let range = {
            let p = self.para_at(slot);
            isolate(p, on)?
        };
        let Some((a, b)) = range else { return Err(format!("paragraph {i} has no text to link")) };
        for n in &self.para_at(slot).children[a..=b] {
            if let Node::Elem(e) = n {
                if !matches!(e.name.as_str(), "w:r" | "w:bookmarkStart" | "w:bookmarkEnd" | "w:proofErr" | "w:commentRangeStart" | "w:commentRangeEnd") {
                    return Err(if e.name == "w:hyperlink" {
                        "that text is already (partly) a link".to_string()
                    } else {
                        format!("that text overlaps a `{}` (a field or tracked change) -- choose text outside it with on=", e.name)
                    });
                }
            }
        }
        let style = self.ensure_char_style("hyperlink", "Hyperlink", hyperlink_style)?;
        let mut h = Element::new("w:hyperlink");
        if let Some(anchor) = url.strip_prefix('#') {
            h.set_attr("w:anchor", anchor);
        } else {
            self.ensure_ns("xmlns:r", NS_R);
            let id = self.pkg.add_external_rel(&self.main, &format!("{REL_BASE}/hyperlink"), url)?;
            h.set_attr("r:id", &id);
        }
        h.set_attr("w:history", "1");
        let p = self.para_at(slot);
        let mut inner: Vec<Node> = p.children.drain(a..=b).collect();
        for n in inner.iter_mut() {
            if let Node::Elem(r) = n {
                if r.name == "w:r" {
                    set_run_style(r, &style);
                }
            }
        }
        h.children = inner;
        p.children.insert(a, Node::Elem(h));
        Ok(())
    }

    // ------------------------------------------------------------ comments

    /// Attach a comment to body paragraph `i` -- all of it, or the first
    /// occurrence of `on`. `date` is ISO 8601; `None` means now (UTC).
    pub fn add_comment(&mut self, i: usize, text: &str, on: Option<&str>, author: &str, initials: &str, date: Option<&str>) -> Result<(), String> {
        let slot = self.para_slot(i)?;
        let range = isolate(self.para_at(slot), on)?;
        let part = match self.related_part("/comments") {
            Some(p) => p,
            None => {
                let p = self.fresh_part("word/comments.xml");
                let root = Element::new("w:comments").with_attr("xmlns:w", NS_W).with_attr("xmlns:r", NS_R);
                self.pkg.set_xml(&p, &Doc::new(root));
                self.pkg.add_override(&p, &format!("{CT_BASE}.comments+xml"))?;
                self.pkg.add_rel(&self.main, &format!("{REL_BASE}/comments"), &qu_ooxml::relative_target(&self.main, &p))?;
                p
            }
        };
        let mut d = self.pkg.get_xml(&part)?;
        let mut used: Vec<i64> = d.root.elems().filter(|c| c.name == "w:comment").filter_map(|c| c.attr("w:id")?.parse().ok()).collect();
        for tag in ["w:commentRangeStart", "w:commentReference"] {
            used.extend(self.doc.root.find_all(tag).iter().filter_map(|c| c.attr("w:id")?.parse::<i64>().ok()));
        }
        let id = (used.into_iter().max().unwrap_or(-1) + 1).to_string();
        let date = date.map(String::from).unwrap_or_else(now_iso);
        let mut c = Element::new("w:comment").with_attr("w:id", &id).with_attr("w:author", author).with_attr("w:date", &date);
        if !initials.is_empty() {
            c.set_attr("w:initials", initials);
        }
        for (li, line) in text.split('\n').enumerate() {
            let mut p = Element::new("w:p");
            if li == 0 {
                p = p.with_child(Element::new("w:r").with_child(Element::new("w:annotationRef")));
            }
            p = p.with_child(run(line, None));
            c = c.with_child(p);
        }
        d.root.children.push(Node::Elem(c));
        self.pkg.set_xml(&part, &d);
        let reference = Element::new("w:r").with_child(Element::new("w:commentReference").with_attr("w:id", &id));
        let p = self.para_at(slot);
        let (start, end) = match range {
            Some((a, b)) => (a, b + 1),
            None => (p.children.len(), p.children.len()),
        };
        p.children.insert(end, Node::Elem(reference));
        p.children.insert(end, Node::Elem(Element::new("w:commentRangeEnd").with_attr("w:id", &id)));
        p.children.insert(start, Node::Elem(Element::new("w:commentRangeStart").with_attr("w:id", &id)));
        Ok(())
    }

    // ------------------------------------------------------------ bookmarks

    /// `(name, text)` for every bookmark in the body, document order,
    /// Word's hidden ones (`_Toc...`, `_GoBack`) included.
    pub fn bookmarks(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String, String)> = Vec::new(); // (id, name, text)
        let mut open: Vec<usize> = Vec::new();
        fn walk(e: &Element, out: &mut Vec<(String, String, String)>, open: &mut Vec<usize>) {
            for c in e.elems() {
                match c.name.as_str() {
                    "w:bookmarkStart" => {
                        out.push((c.attr("w:id").unwrap_or("").into(), c.attr("w:name").unwrap_or("").into(), String::new()));
                        open.push(out.len() - 1);
                    }
                    "w:bookmarkEnd" => {
                        let id = c.attr("w:id").unwrap_or("");
                        open.retain(|&k| out[k].0 != id);
                    }
                    "w:t" => {
                        for &k in open.iter() {
                            out[k].2.push_str(&c.text());
                        }
                    }
                    "w:tab" => {
                        for &k in open.iter() {
                            out[k].2.push('\t');
                        }
                    }
                    "w:delText" | "w:instrText" => {}
                    "w:p" => {
                        walk(c, out, open);
                        for &k in open.iter() {
                            out[k].2.push('\n');
                        }
                    }
                    _ => walk(c, out, open),
                }
            }
        }
        walk(self.body(), &mut out, &mut open);
        out.into_iter().map(|(_, n, t)| (n, t.trim_end_matches('\n').to_string())).collect()
    }

    /// Put a bookmark named `name` around body paragraph `i` (or the first
    /// occurrence of `on` in it). Names follow Word's rule: a letter, then
    /// letters, digits and `_`, at most 40 characters.
    pub fn add_bookmark(&mut self, i: usize, name: &str, on: Option<&str>) -> Result<(), String> {
        let ok = name.chars().next().is_some_and(|c| c.is_alphabetic())
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && name.chars().count() <= 40;
        if !ok {
            return Err(format!("bookmark name \"{name}\" -- Word takes a letter, then letters, digits or _, at most 40 characters"));
        }
        if self.body().find_all("w:bookmarkStart").iter().any(|b| b.attr("w:name").is_some_and(|n| n.eq_ignore_ascii_case(name))) {
            return Err(format!("the document already has a bookmark named \"{name}\""));
        }
        let slot = self.para_slot(i)?;
        let range = isolate(self.para_at(slot), on)?;
        let id = (self.doc.root.find_all("w:bookmarkStart").iter().filter_map(|b| b.attr("w:id")?.parse::<i64>().ok()).max().unwrap_or(-1) + 1).to_string();
        let p = self.para_at(slot);
        let (start, end) = match range {
            Some((a, b)) => (a, b + 1),
            None => (p.children.len(), p.children.len()),
        };
        p.children.insert(end, Node::Elem(Element::new("w:bookmarkEnd").with_attr("w:id", &id)));
        p.children.insert(start, Node::Elem(Element::new("w:bookmarkStart").with_attr("w:id", &id).with_attr("w:name", name)));
        Ok(())
    }

    // ------------------------------------------------------------ fields

    /// Every field's instruction in the body, document order (`PAGE`,
    /// `TOC \o "1-3" \h \z \u`, `REF intro \h`, ...), simple and complex.
    pub fn fields(&self) -> Vec<String> {
        fn walk(e: &Element, out: &mut Vec<String>, stack: &mut Vec<(usize, bool)>) {
            for c in e.elems() {
                match c.name.as_str() {
                    "w:fldSimple" => {
                        out.push(c.attr("w:instr").unwrap_or("").to_string());
                        walk(c, out, stack);
                    }
                    "w:fldChar" => match c.attr("w:fldCharType") {
                        Some("begin") => {
                            out.push(String::new());
                            stack.push((out.len() - 1, true));
                        }
                        Some("separate") => {
                            if let Some(top) = stack.last_mut() {
                                top.1 = false;
                            }
                        }
                        Some("end") => {
                            stack.pop();
                        }
                        _ => {}
                    },
                    "w:instrText" => {
                        if let Some(&(k, true)) = stack.last() {
                            out[k].push_str(&c.text());
                        }
                    }
                    _ => walk(c, out, stack),
                }
            }
        }
        let mut out = Vec::new();
        walk(self.body(), &mut out, &mut Vec::new());
        out.into_iter().map(|s| s.trim().to_string()).collect()
    }

    /// Append a field with instruction `code` (`PAGE`, `DATE \@ "d MMMM yyyy"`,
    /// `REF name \h`, ...) to the end of body paragraph `i`, marked dirty so
    /// the word processor computes it on open. `shown` is the result to
    /// display until then.
    pub fn add_field(&mut self, i: usize, code: &str, shown: &str) -> Result<(), String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("the field code is empty".into());
        }
        let slot = self.para_slot(i)?;
        let rpr = last_run_rpr(self.para_at(slot));
        self.para_at(slot).children.extend(field_runs(code, shown, rpr).into_iter().map(Node::Elem));
        Ok(())
    }

    /// Append a cross-reference to bookmark `name` at the end of body
    /// paragraph `i`: `show` is `text` (the bookmarked text, `REF`), `page`
    /// (its page number, `PAGEREF`) or `number` (its paragraph number).
    pub fn add_cross_ref(&mut self, i: usize, name: &str, show: &str) -> Result<(), String> {
        let marks = self.bookmarks();
        let Some((name, text)) = marks.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).cloned() else {
            let known: Vec<&str> = marks.iter().map(|(n, _)| n.as_str()).filter(|n| !n.starts_with('_')).collect();
            return Err(format!("no bookmark named \"{name}\" -- the document has: {}", if known.is_empty() { "none".to_string() } else { known.join(", ") }));
        };
        let (code, shown) = match show {
            // The bookmarked text is known now, so it is the true result.
            "text" => (format!("REF {name} \\h"), text),
            // A page or paragraph number depends on layout Qu does not do:
            // leave the result empty for the word processor to fill.
            "page" => (format!("PAGEREF {name} \\h"), String::new()),
            "number" => (format!("REF {name} \\r \\h"), String::new()),
            other => return Err(format!("show=\"{other}\" -- use text, page or number")),
        };
        self.add_field(i, &code, &shown)
    }

    /// Insert a table-of-contents field for heading levels 1..`levels`,
    /// before body paragraph `at` (or at the end), optionally under a
    /// title. The field is left for Word/LibreOffice to fill: it is marked
    /// dirty and the document asks for fields to be updated on open.
    pub fn add_toc(&mut self, levels: u32, at: Option<usize>, title: Option<&str>) -> Result<(), String> {
        if !(1..=9).contains(&levels) {
            return Err(format!("levels={levels} -- a table of contents covers heading levels 1 to 9"));
        }
        let slots = self.para_slots();
        if let Some(a) = at {
            if a > slots.len() {
                return Err(format!("cannot insert at paragraph {a} -- the document has {}", slots.len()));
            }
        }
        let mut blocks = Vec::new();
        if let Some(t) = title {
            let styles = self.style_names();
            let fmt = match styles.iter().find(|(_, n)| n == "toc heading") {
                Some((id, _)) => Format { style: Some(id.clone()), ..Format::default() },
                None => Format { bold: true, size: Some(14.0), ..Format::default() },
            };
            blocks.push(self.make_paragraph(t, &fmt)?);
        }
        let mut p = Element::new("w:p");
        for r in field_runs(&format!("TOC \\o \"1-{levels}\" \\h \\z \\u"), TOC_PLACEHOLDER, None) {
            p.children.push(Node::Elem(r));
        }
        blocks.push(p);
        let at_slot = at.and_then(|a| slots.get(a).copied());
        for (k, b) in blocks.into_iter().enumerate() {
            match at_slot {
                Some(s) => self.body_mut().children.insert(s + k, Node::Elem(b)),
                None => self.append_block(b),
            }
        }
        self.with_settings(|s| {
            ensure_ordered(s, "w:updateFields", SETTINGS_ORDER).set_attr("w:val", "true");
        })
    }

    // ------------------------------------------------------------ sections

    /// Body child slots of the section-ending paragraphs; the last section
    /// (`None`) is the body's own `w:sectPr`.
    fn section_slots(&self) -> Vec<Option<usize>> {
        let mut out: Vec<Option<usize>> = self
            .body()
            .children
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n, Node::Elem(e) if e.name == "w:p" && e.child("w:pPr").and_then(|p| p.child("w:sectPr")).is_some()))
            .map(|(i, _)| Some(i))
            .collect();
        out.push(None);
        out
    }

    pub fn section_count(&self) -> usize {
        self.section_slots().len()
    }

    fn sect(&self, s: usize) -> Option<&Element> {
        match *self.section_slots().get(s)? {
            Some(slot) => match &self.body().children[slot] {
                Node::Elem(p) => p.child("w:pPr")?.child("w:sectPr"),
                _ => None,
            },
            None => self.body().child("w:sectPr"),
        }
    }

    fn sect_mut(&mut self, s: usize) -> Result<&mut Element, String> {
        let slots = self.section_slots();
        let loc = *slots.get(s).ok_or_else(|| format!("section {s} does not exist -- the document has {} (0-based)", slots.len()))?;
        let body = self.body_mut();
        match loc {
            Some(slot) => match &mut body.children[slot] {
                Node::Elem(p) => Ok(p.child_mut("w:pPr").and_then(|p| p.child_mut("w:sectPr")).expect("found by section_slots")),
                _ => unreachable!(),
            },
            None => {
                if body.child("w:sectPr").is_none() {
                    body.children.push(Node::Elem(Element::new("w:sectPr")));
                }
                Ok(body.child_mut("w:sectPr").unwrap())
            }
        }
    }

    /// Page setup of every section, in document order.
    pub fn sections(&self) -> Vec<SectionInfo> {
        (0..self.section_count())
            .map(|s| {
                let empty = Element::new("w:sectPr");
                let sp = self.sect(s).unwrap_or(&empty);
                let pg = sp.child("w:pgSz");
                let mar = sp.child("w:pgMar");
                let tw = |e: Option<&Element>, k: &str, dflt: f64| e.and_then(|e| e.attr(k)).and_then(|v| v.parse::<f64>().ok()).map(|t| twips_to_mm(t.abs())).unwrap_or(dflt);
                let (w, h) = (tw(pg, "w:w", 210.0), tw(pg, "w:h", 297.0));
                let kinds = |tag: &str| sp.elems().filter(|e| e.name == tag).map(|e| e.attr("w:type").unwrap_or("default").to_string()).collect::<Vec<_>>();
                SectionInfo {
                    width_mm: w,
                    height_mm: h,
                    landscape: pg.and_then(|p| p.attr("w:orient")) == Some("landscape") || w > h,
                    top_mm: tw(mar, "w:top", 25.0),
                    bottom_mm: tw(mar, "w:bottom", 25.0),
                    left_mm: tw(mar, "w:left", 25.0),
                    right_mm: tw(mar, "w:right", 25.0),
                    start: match sp.child("w:type").and_then(|t| t.attr("w:val")).unwrap_or("nextPage") {
                        "continuous" => "continuous",
                        "evenPage" => "even_page",
                        "oddPage" => "odd_page",
                        "nextColumn" => "next_column",
                        _ => "next_page",
                    }
                    .to_string(),
                    headers: kinds("w:headerReference"),
                    footers: kinds("w:footerReference"),
                }
            })
            .collect()
    }

    /// Change the page size, orientation and margins of one section, or of
    /// every section (`section = None`). `size` is portrait (width, height)
    /// in mm; `landscape` turns it; margins in mm.
    pub fn page_setup(&mut self, section: Option<usize>, size: Option<(f64, f64)>, landscape: Option<bool>, margins: [Option<f64>; 4]) -> Result<(), String> {
        if let Some((w, h)) = size {
            if !(w > 0.0 && h > 0.0 && w <= 558.8 && h <= 558.8) {
                return Err(format!("page size {w} x {h} mm -- Word takes 0-558.8 mm (22 in) per side"));
            }
        }
        for m in margins.iter().flatten() {
            if !(*m >= 0.0 && *m < 558.8) {
                return Err(format!("margin {m} mm is outside 0-558.8 mm"));
            }
        }
        let targets: Vec<usize> = match section {
            Some(s) if s >= self.section_count() => return Err(format!("section {s} does not exist -- the document has {} (0-based)", self.section_count())),
            Some(s) => vec![s],
            None => (0..self.section_count()).collect(),
        };
        for s in targets {
            let sp = self.sect_mut(s)?;
            if size.is_some() || landscape.is_some() {
                let pg = ensure_ordered(sp, "w:pgSz", SECT_ORDER);
                let cur = |k: &str, d: f64| pg.attr(k).and_then(|v| v.parse::<f64>().ok()).map(twips_to_mm).unwrap_or(d);
                let (cw, ch) = (cur("w:w", 210.0), cur("w:h", 297.0));
                let was_landscape = pg.attr("w:orient") == Some("landscape") || cw > ch;
                let (pw, ph) = size.map(|(a, b)| (a.min(b), a.max(b))).unwrap_or((cw.min(ch), cw.max(ch)));
                let land = landscape.unwrap_or(was_landscape);
                let (w, h) = if land { (ph, pw) } else { (pw, ph) };
                pg.set_attr("w:w", &mm_to_twips(w).to_string());
                pg.set_attr("w:h", &mm_to_twips(h).to_string());
                if land {
                    pg.set_attr("w:orient", "landscape");
                } else {
                    pg.remove_attr("w:orient");
                }
            }
            if margins.iter().any(Option::is_some) {
                let created = sp.child("w:pgMar").is_none();
                let mar = ensure_ordered(sp, "w:pgMar", SECT_ORDER);
                if created {
                    // Every attribute of w:pgMar is required by the schema.
                    for (k, v) in [("w:top", "1417"), ("w:right", "1417"), ("w:bottom", "1134"), ("w:left", "1417"), ("w:header", "708"), ("w:footer", "708"), ("w:gutter", "0")] {
                        mar.set_attr(k, v);
                    }
                }
                for (k, m) in ["w:top", "w:bottom", "w:left", "w:right"].iter().zip(margins) {
                    if let Some(m) = m {
                        mar.set_attr(k, &mm_to_twips(m).to_string());
                    }
                }
            }
        }
        Ok(())
    }

    /// End the current section at the end of the document: what follows
    /// is a new section starting `start` (`next_page`, `continuous`,
    /// `even_page`, `odd_page`), with the same page setup until changed.
    pub fn add_section_break(&mut self, start: &str) -> Result<(), String> {
        let val = match start {
            "next_page" => "nextPage",
            "continuous" => "continuous",
            "even_page" => "evenPage",
            "odd_page" => "oddPage",
            other => return Err(format!("start=\"{other}\" -- use next_page, continuous, even_page or odd_page")),
        };
        let last = self.section_count() - 1;
        let ending = self.sect_mut(last)?.clone();
        let p = Element::new("w:p").with_child(Element::new("w:pPr").with_child(ending));
        self.append_block(p);
        let last = self.section_count() - 1;
        let sp = self.sect_mut(last)?;
        // The new section inherits its headers and footers from the one
        // before it (Word's "link to previous") rather than sharing parts,
        // so setting one section's header never silently changes another's.
        sp.remove_children("w:headerReference");
        sp.remove_children("w:footerReference");
        ensure_ordered(sp, "w:type", SECT_ORDER).set_attr("w:val", val);
        Ok(())
    }

    // ------------------------------------------------------------ headers, footers

    /// The part holding section `s`'s header/footer of `kind`, following
    /// Word's inheritance from earlier sections.
    fn story_part(&self, footer: bool, s: usize, kind: &str) -> Option<String> {
        let tag = if footer { "w:footerReference" } else { "w:headerReference" };
        let rels = self.pkg.rels(&self.main).ok()?;
        (0..=s).rev().find_map(|k| {
            let sp = self.sect(k)?;
            let id = sp.elems().find(|e| e.name == tag && e.attr("w:type").unwrap_or("default") == kind)?.attr("r:id")?;
            rels.iter().find(|r| r.id == id).map(|r| qu_ooxml::resolve_target(&self.main, &r.target))
        })
    }

    /// Section `s`'s header (or footer) text of `kind` (`default`, `first`,
    /// `even`), one line per paragraph; `None` if it has none.
    pub fn story_text(&self, footer: bool, s: usize, kind: &str) -> Result<Option<String>, String> {
        let kind = story_kind(kind)?;
        if s >= self.section_count() {
            return Err(format!("section {s} does not exist -- the document has {} (0-based)", self.section_count()));
        }
        let Some(part) = self.story_part(footer, s, kind) else { return Ok(None) };
        let d = self.pkg.get_xml(&part)?;
        let paths = qu_ooxml::paragraph_paths(&d.root, WORD);
        Ok(Some(paths.iter().map(|p| para_display_text(qu_ooxml::at_path(&d.root, p))).collect::<Vec<_>>().join("\n")))
    }

    /// Set a header (or footer) of `kind` to `text`, replacing its content.
    /// `#page` and `#pages` in `text` become PAGE / NUMPAGES fields. With
    /// `section = None` every header of that kind the document has is set
    /// (the first section gets one if there is none); with `Some(s)` only
    /// section `s`'s, which gets its own if it was inheriting one.
    pub fn set_story(&mut self, footer: bool, text: &str, section: Option<usize>, kind: &str, align: Option<&str>) -> Result<(), String> {
        let kind = story_kind(kind)?;
        let jc = align.map(align_val).transpose()?;
        let tag = if footer { "w:footerReference" } else { "w:headerReference" };
        let rels = self.pkg.rels(&self.main)?;
        let own = |d: &Document, s: usize| -> Option<String> {
            let id = d.sect(s)?.elems().find(|e| e.name == tag && e.attr("w:type").unwrap_or("default") == kind)?.attr("r:id")?.to_string();
            rels.iter().find(|r| r.id == id).map(|r| qu_ooxml::resolve_target(&d.main, &r.target))
        };
        let mut parts: Vec<String> = Vec::new();
        let mut sections: Vec<usize> = Vec::new();
        match section {
            Some(s) => {
                if s >= self.section_count() {
                    return Err(format!("section {s} does not exist -- the document has {} (0-based)", self.section_count()));
                }
                sections.push(s);
                match own(self, s) {
                    Some(p) => parts.push(p),
                    None => parts.push(self.create_story(footer, s, kind)?),
                }
            }
            None => {
                for s in 0..self.section_count() {
                    if let Some(p) = own(self, s) {
                        sections.push(s);
                        if !parts.contains(&p) {
                            parts.push(p);
                        }
                    }
                }
                if parts.is_empty() {
                    sections.push(0);
                    parts.push(self.create_story(footer, 0, kind)?);
                }
            }
        }
        let style_id = self.style_names().into_iter().find(|(_, n)| n == if footer { "footer" } else { "header" }).map(|(id, _)| id);
        for part in &parts {
            let mut d = self.pkg.get_xml(part)?;
            let first = d.root.find_all("w:p").first().map(|p| (*p).clone());
            let mut ppr = first.as_ref().and_then(|p| p.child("w:pPr")).cloned().unwrap_or_else(|| Element::new("w:pPr"));
            if ppr.child("w:pStyle").is_none() {
                if let Some(id) = &style_id {
                    insert_ordered(&mut ppr, Element::new("w:pStyle").with_attr("w:val", id), PPR_ORDER);
                }
            }
            if let Some(v) = jc {
                ensure_ordered(&mut ppr, "w:jc", PPR_ORDER).set_attr("w:val", v);
            }
            let rpr = first.as_ref().and_then(|p| p.find_all("w:r").first().and_then(|r| r.child("w:rPr")).cloned());
            let mut p = Element::new("w:p");
            if !ppr.children.is_empty() {
                p.children.push(Node::Elem(ppr));
            }
            p.children.extend(story_runs(text, rpr).into_iter().map(Node::Elem));
            d.root.children = vec![Node::Elem(p)];
            self.pkg.set_xml(part, &d);
        }
        match kind {
            "first" => {
                for s in sections {
                    ensure_ordered(self.sect_mut(s)?, "w:titlePg", SECT_ORDER);
                }
            }
            "even" => self.with_settings(|st| {
                ensure_ordered(st, "w:evenAndOddHeaders", SETTINGS_ORDER);
            })?,
            _ => {}
        }
        Ok(())
    }

    /// A new, empty header/footer part referenced from section `s`.
    fn create_story(&mut self, footer: bool, s: usize, kind: &str) -> Result<String, String> {
        let (root_tag, stem, word, tag) = if footer { ("w:ftr", "word/footer", "footer", "w:footerReference") } else { ("w:hdr", "word/header", "header", "w:headerReference") };
        let part = self.pkg.next_name(stem, ".xml");
        let root = Element::new(root_tag).with_attr("xmlns:w", NS_W).with_attr("xmlns:r", NS_R).with_child(Element::new("w:p"));
        self.pkg.set_xml(&part, &Doc::new(root));
        self.pkg.add_override(&part, &format!("{CT_BASE}.{word}+xml"))?;
        let rid = self.pkg.add_rel(&self.main, &format!("{REL_BASE}/{word}"), &qu_ooxml::relative_target(&self.main, &part))?;
        self.ensure_ns("xmlns:r", NS_R);
        let sp = self.sect_mut(s)?;
        insert_ordered(sp, Element::new(tag).with_attr("w:type", kind).with_attr("r:id", &rid), SECT_ORDER);
        Ok(part)
    }

    // ------------------------------------------------------------ lists

    /// Append a list paragraph: `kind` `bullet` or `number`, nesting
    /// `level` 0-8. Consecutive items continue one list; `restart` starts
    /// the numbering again from 1.
    pub fn add_list_item(&mut self, text: &str, kind: &str, level: u32, restart: bool) -> Result<(), String> {
        // Checked before appending, so a bad call leaves no stray paragraph.
        if !matches!(kind, "bullet" | "number") {
            return Err(format!("kind=\"{kind}\" -- use bullet or number"));
        }
        if level > 8 {
            return Err(format!("level={level} -- Word lists nest 0 to 8"));
        }
        let p = self.make_paragraph(text, &Format::default())?;
        self.append_block(p);
        let i = self.para_slots().len() - 1;
        self.set_list(i, kind, level, restart)
    }

    /// Make body paragraph `i` a list item (`bullet`/`number`, `level`
    /// 0-8), or plain again with `kind = "none"`.
    pub fn set_list(&mut self, i: usize, kind: &str, level: u32, restart: bool) -> Result<(), String> {
        let slot = self.para_slot(i)?;
        if kind == "none" {
            if let Some(ppr) = self.para_at(slot).child_mut("w:pPr") {
                ppr.remove_children("w:numPr");
            }
            return Ok(());
        }
        if level > 8 {
            return Err(format!("level={level} -- Word lists nest 0 to 8"));
        }
        let num_id = self.list_num(kind, restart)?;
        let list_style = self.style_names().into_iter().find(|(_, n)| n == "list paragraph").map(|(id, _)| id);
        let p = self.para_at(slot);
        let ppr = p.ensure_first_child("w:pPr");
        if let (Some(id), None) = (&list_style, ppr.child("w:pStyle")) {
            insert_ordered(ppr, Element::new("w:pStyle").with_attr("w:val", id), PPR_ORDER);
        }
        ppr.remove_children("w:numPr");
        insert_ordered(
            ppr,
            Element::new("w:numPr")
                .with_child(Element::new("w:ilvl").with_attr("w:val", &level.to_string()))
                .with_child(Element::new("w:numId").with_attr("w:val", &num_id)),
            PPR_ORDER,
        );
        Ok(())
    }

    /// The `w:numId` for a new item of a `kind` list, creating the
    /// numbering part and Qu's list definition when missing.
    fn list_num(&mut self, kind: &str, restart: bool) -> Result<String, String> {
        let name = match kind {
            "bullet" => "Qu bullet",
            "number" => "Qu number",
            other => return Err(format!("kind=\"{other}\" -- use bullet, number or none")),
        };
        let part = match self.related_part("/numbering") {
            Some(p) => p,
            None => {
                let p = self.fresh_part("word/numbering.xml");
                self.pkg.set_xml(&p, &Doc::new(Element::new("w:numbering").with_attr("xmlns:w", NS_W)));
                self.pkg.add_override(&p, &format!("{CT_BASE}.numbering+xml"))?;
                self.pkg.add_rel(&self.main, &format!("{REL_BASE}/numbering"), &qu_ooxml::relative_target(&self.main, &p))?;
                p
            }
        };
        let mut d = self.pkg.get_xml(&part)?;
        let ids = |d: &Doc, tag: &str, attr: &str| -> Vec<i64> { d.root.elems().filter(|e| e.name == tag).filter_map(|e| e.attr(attr)?.parse().ok()).collect() };
        let existing = d.root.elems().find(|a| a.name == "w:abstractNum" && a.child("w:name").and_then(|n| n.attr("w:val")) == Some(name)).map(|a| a.attr("w:abstractNumId").unwrap_or("0").to_string());
        let abs_id = match existing {
            Some(a) => a,
            None => {
                let id = (ids(&d, "w:abstractNum", "w:abstractNumId").into_iter().max().unwrap_or(-1) + 1).to_string();
                let pos = match d.root.children.iter().rposition(|n| matches!(n, Node::Elem(e) if e.name == "w:abstractNum")) {
                    Some(k) => k + 1,
                    None => d.root.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "w:num" || e.name == "w:numIdMacAtCleanup")).unwrap_or(d.root.children.len()),
                };
                d.root.children.insert(pos, Node::Elem(abstract_num(&id, name, kind == "bullet")));
                id
            }
        };
        let uses_abs = |d: &Doc, num: &str| d.root.elems().any(|n| n.name == "w:num" && n.attr("w:numId") == Some(num) && n.child("w:abstractNumId").and_then(|a| a.attr("w:val")) == Some(abs_id.as_str()));
        if !restart {
            // Continue the most recent list of this kind, else any.
            let in_body: Vec<String> = self.body().find_all("w:numId").iter().filter_map(|n| n.attr("w:val").map(String::from)).collect();
            if let Some(n) = in_body.iter().rev().find(|n| uses_abs(&d, n)) {
                return Ok(n.clone());
            }
            if let Some(n) = d.root.elems().find(|n| n.name == "w:num" && n.child("w:abstractNumId").and_then(|a| a.attr("w:val")) == Some(abs_id.as_str())) {
                if let Some(id) = n.attr("w:numId") {
                    return Ok(id.to_string());
                }
            }
        }
        let num_id = (ids(&d, "w:num", "w:numId").into_iter().max().unwrap_or(0) + 1).to_string();
        let mut num = Element::new("w:num").with_attr("w:numId", &num_id).with_child(Element::new("w:abstractNumId").with_attr("w:val", &abs_id));
        if restart {
            for l in 0..9 {
                num = num.with_child(Element::new("w:lvlOverride").with_attr("w:ilvl", &l.to_string()).with_child(Element::new("w:startOverride").with_attr("w:val", "1")));
            }
        }
        let pos = d.root.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "w:numIdMacAtCleanup")).unwrap_or(d.root.children.len());
        d.root.children.insert(pos, Node::Elem(num));
        self.pkg.set_xml(&part, &d);
        Ok(num_id)
    }

    // ------------------------------------------------------------ internals

    pub(crate) fn para_at(&mut self, slot: usize) -> &mut Element {
        match &mut self.body_mut().children[slot] {
            Node::Elem(p) => p,
            _ => unreachable!("slots point at paragraphs"),
        }
    }

    fn ensure_ns(&mut self, prefix: &str, ns: &str) {
        if self.doc.root.attr(prefix).is_none() {
            self.doc.root.set_attr(prefix, ns);
        }
    }

    /// The id of the character style whose (lower-case) name is `name`,
    /// created from `make(id)` when the document lacks it.
    pub(crate) fn ensure_char_style(&mut self, name: &str, id: &str, make: fn(&str) -> Element) -> Result<String, String> {
        if let Some((sid, _)) = self.style_names().into_iter().find(|(_, n)| n == name) {
            return Ok(sid);
        }
        let part = self.styles_part_or_create()?;
        let mut d = self.pkg.get_xml(&part)?;
        let mut sid = id.to_string();
        let taken = |d: &Doc, s: &str| d.root.elems().any(|e| e.name == "w:style" && e.attr("w:styleId") == Some(s));
        let mut k = 1;
        while taken(&d, &sid) {
            k += 1;
            sid = format!("{id}{k}");
        }
        d.root.children.push(Node::Elem(make(&sid)));
        self.pkg.set_xml(&part, &d);
        Ok(sid)
    }

    /// Edit `word/settings.xml`, creating it when the document has none.
    pub(crate) fn with_settings(&mut self, f: impl FnOnce(&mut Element)) -> Result<(), String> {
        let part = match self.related_part("/settings") {
            Some(p) => p,
            None => {
                let p = self.fresh_part("word/settings.xml");
                self.pkg.set_xml(&p, &Doc::new(Element::new("w:settings").with_attr("xmlns:w", NS_W)));
                self.pkg.add_override(&p, &format!("{CT_BASE}.settings+xml"))?;
                self.pkg.add_rel(&self.main, &format!("{REL_BASE}/settings"), &qu_ooxml::relative_target(&self.main, &p))?;
                p
            }
        };
        let mut d = self.pkg.get_xml(&part)?;
        f(&mut d.root);
        self.pkg.set_xml(&part, &d);
        Ok(())
    }
}

// ------------------------------------------------------------------ helpers

fn story_kind(kind: &str) -> Result<&'static str, String> {
    match kind {
        "default" | "odd" | "all" => Ok("default"),
        "first" => Ok("first"),
        "even" => Ok("even"),
        other => Err(format!("kind=\"{other}\" -- use default, first or even")),
    }
}

fn align_val(a: &str) -> Result<&'static str, String> {
    match a {
        "left" => Ok("left"),
        "center" | "centre" => Ok("center"),
        "right" => Ok("right"),
        "justify" | "justified" | "both" => Ok("both"),
        other => Err(format!("align=\"{other}\" -- use left, center, right or justify")),
    }
}

pub fn page_size_named(name: &str) -> Option<(f64, f64)> {
    let n = name.to_ascii_lowercase();
    PAGE_SIZES.iter().find(|(k, _, _)| *k == n).map(|&(_, w, h)| (w, h))
}

pub fn page_size_names() -> String {
    PAGE_SIZES.iter().map(|(k, _, _)| if k.starts_with('a') || k.starts_with('b') { k.to_uppercase() } else { k.to_string() }).collect::<Vec<_>>().join(", ")
}

fn twips_to_mm(t: f64) -> f64 {
    (t * 25.4 / 1440.0 * 10.0).round() / 10.0
}

fn mm_to_twips(mm: f64) -> i64 {
    (mm * 1440.0 / 25.4).round() as i64
}

fn rank(order: &[&str], name: &str) -> Option<usize> {
    let name = if name == "w:footerReference" { "w:headerReference" } else { name };
    order.iter().position(|n| *n == name)
}

/// Insert `el` among `parent`'s children where `order` (a schema
/// sequence) puts it: before the first child that must come after it.
pub(crate) fn insert_ordered(parent: &mut Element, el: Element, order: &[&str]) {
    let r = rank(order, &el.name).unwrap_or(usize::MAX);
    let pos = parent
        .children
        .iter()
        .position(|n| matches!(n, Node::Elem(e) if rank(order, &e.name).is_some_and(|x| x > r)))
        .unwrap_or(parent.children.len());
    parent.children.insert(pos, Node::Elem(el));
}

fn ensure_ordered<'a>(parent: &'a mut Element, name: &str, order: &[&str]) -> &'a mut Element {
    if parent.child(name).is_none() {
        insert_ordered(parent, Element::new(name), order);
    }
    parent.child_mut(name).unwrap()
}

fn set_run_style(r: &mut Element, style: &str) {
    let rpr = r.ensure_first_child("w:rPr");
    rpr.remove_children("w:rStyle");
    rpr.children.insert(0, Node::Elem(Element::new("w:rStyle").with_attr("w:val", style)));
}

fn last_run_rpr(p: &Element) -> Option<Element> {
    p.elems().filter(|e| e.name == "w:r").last().and_then(|r| r.child("w:rPr")).cloned()
}

/// A complex field: begin (dirty) / instruction / separate / result / end.
fn field_runs(code: &str, shown: &str, rpr: Option<Element>) -> Vec<Element> {
    let with = |child: Element| {
        let mut r = Element::new("w:r");
        if let Some(p) = &rpr {
            r.children.push(Node::Elem(p.clone()));
        }
        r.with_child(child)
    };
    let mut out = vec![
        with(Element::new("w:fldChar").with_attr("w:fldCharType", "begin").with_attr("w:dirty", "true")),
        with(Element::new("w:instrText").with_attr("xml:space", "preserve").with_text(&format!(" {code} "))),
        with(Element::new("w:fldChar").with_attr("w:fldCharType", "separate")),
    ];
    if !shown.is_empty() {
        out.push(run(shown, rpr.clone()));
    }
    out.push(with(Element::new("w:fldChar").with_attr("w:fldCharType", "end")));
    out
}

/// Header/footer text as runs, `#page`/`#pages` as fields.
fn story_runs(text: &str, rpr: Option<Element>) -> Vec<Element> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let next = [("#pages", "NUMPAGES"), ("#page", "PAGE")].iter().filter_map(|(tok, code)| rest.find(tok).map(|i| (i, *tok, *code))).min_by_key(|(i, _, _)| *i);
        match next {
            Some((i, tok, code)) => {
                if i > 0 {
                    out.push(run(&rest[..i], rpr.clone()));
                }
                out.extend(field_runs(code, "1", rpr.clone()));
                rest = &rest[i + tok.len()..];
            }
            None => {
                if !rest.is_empty() || out.is_empty() {
                    out.push(run(rest, rpr.clone()));
                }
                return out;
            }
        }
    }
}

/// Displayed length (characters) of one child of a paragraph or a run.
pub(crate) fn child_len(x: &Element) -> usize {
    match x.name.as_str() {
        "w:pPr" | "w:rPr" | "w:p" | "w:delText" | "w:instrText" => 0,
        "w:t" => x.text().chars().count(),
        "w:tab" => 1,
        "w:br" | "w:cr" => usize::from(x.attr("w:type") != Some("page")),
        _ => para_display_text(x).chars().count(),
    }
}

pub(crate) fn text_elem(s: &str) -> Element {
    let mut t = Element::new("w:t").with_text(s);
    if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
        t.set_attr("xml:space", "preserve");
    }
    t
}

/// Split run `r` after `k` displayed characters, both halves keeping its
/// properties.
pub(crate) fn split_run(r: &Element, k: usize) -> Result<(Element, Element), String> {
    let mut left = Element { name: r.name.clone(), attrs: r.attrs.clone(), children: Vec::new() };
    let mut right = left.clone();
    let mut acc = 0;
    for n in &r.children {
        match n {
            Node::Elem(x) if x.name == "w:rPr" => {
                left.children.push(n.clone());
                right.children.push(n.clone());
            }
            Node::Elem(x) => {
                let len = child_len(x);
                if (len == 0 && acc < k) || (len > 0 && acc + len <= k) {
                    left.children.push(n.clone());
                } else if acc >= k {
                    right.children.push(n.clone());
                } else if x.name == "w:t" {
                    let chars: Vec<char> = x.text().chars().collect();
                    let cut = k - acc;
                    left.children.push(Node::Elem(text_elem(&chars[..cut].iter().collect::<String>())));
                    right.children.push(Node::Elem(text_elem(&chars[cut..].iter().collect::<String>())));
                } else {
                    return Err(format!("the text boundary falls inside a `{}`", x.name));
                }
                acc += len;
            }
            other => {
                if acc < k {
                    left.children.push(other.clone());
                } else {
                    right.children.push(other.clone());
                }
            }
        }
    }
    Ok((left, right))
}

fn seg_lens(p: &Element) -> Vec<usize> {
    p.children
        .iter()
        .map(|n| match n {
            Node::Elem(e) => child_len(e),
            _ => 0,
        })
        .collect()
}

/// Split the paragraph's runs so the text `[off]` falls on a child
/// boundary.
fn split_at(p: &mut Element, off: usize) -> Result<(), String> {
    let lens = seg_lens(p);
    let mut acc = 0;
    for (i, len) in lens.into_iter().enumerate() {
        if off > acc && off < acc + len {
            let Node::Elem(e) = &p.children[i] else { unreachable!() };
            if e.name != "w:r" {
                return Err(format!("that text starts or ends inside a `{}` (a link, field or tracked change) -- choose text outside it with on=", e.name));
            }
            let (a, b) = split_run(e, off - acc)?;
            p.children[i] = Node::Elem(a);
            p.children.insert(i + 1, Node::Elem(b));
            return Ok(());
        }
        acc += len;
    }
    Ok(())
}

/// The range of the paragraph's children (inclusive) covering all its
/// content (`on = None`), or exactly the first occurrence of `on` --
/// splitting runs at the edges so the range is whole children. `None`
/// for a paragraph with no content at all.
pub(crate) fn isolate(p: &mut Element, on: Option<&str>) -> Result<Option<(usize, usize)>, String> {
    let Some(needle) = on else {
        let content = |n: &Node| matches!(n, Node::Elem(e) if e.name != "w:pPr");
        let first = p.children.iter().position(content);
        let last = p.children.iter().rposition(content);
        return Ok(first.zip(last));
    };
    if needle.is_empty() {
        return Err("on=\"\" -- give the text to mark".into());
    }
    let full: String = p.children.iter().map(|n| if let Node::Elem(e) = n { if child_len(e) > 0 { para_display_text_of(e) } else { String::new() } } else { String::new() }).collect();
    let Some(byte) = full.find(needle) else {
        return Err(format!("the paragraph does not contain \"{needle}\" (it reads \"{full}\")"));
    };
    let s = full[..byte].chars().count();
    let e = s + needle.chars().count();
    split_at(p, e)?;
    split_at(p, s)?;
    let (mut a, mut b) = (None, None);
    let mut acc = 0;
    for (i, len) in seg_lens(p).into_iter().enumerate() {
        if len > 0 && acc == s && a.is_none() {
            a = Some(i);
        }
        if len > 0 && acc + len == e {
            b = Some(i);
        }
        acc += len;
    }
    match (a, b) {
        (Some(a), Some(b)) if a <= b => Ok(Some((a, b))),
        _ => Err(format!("could not isolate \"{needle}\" in the paragraph")),
    }
}

/// The text one paragraph child contributes (matches `child_len`).
pub(crate) fn para_display_text_of(x: &Element) -> String {
    match x.name.as_str() {
        "w:t" => x.text(),
        "w:tab" => "\t".into(),
        "w:br" | "w:cr" if x.attr("w:type") != Some("page") => "\n".into(),
        "w:br" | "w:cr" => String::new(),
        _ => para_display_text(x),
    }
}

fn hyperlink_style(id: &str) -> Element {
    Element::new("w:style")
        .with_attr("w:type", "character")
        .with_attr("w:styleId", id)
        .with_child(Element::new("w:name").with_attr("w:val", "Hyperlink"))
        .with_child(Element::new("w:uiPriority").with_attr("w:val", "99"))
        .with_child(Element::new("w:unhideWhenUsed"))
        .with_child(Element::new("w:rPr").with_child(Element::new("w:color").with_attr("w:val", "0563C1")).with_child(Element::new("w:u").with_attr("w:val", "single")))
}

/// Qu's nine-level bullet or number definition.
fn abstract_num(id: &str, name: &str, bullet: bool) -> Element {
    let mut a = Element::new("w:abstractNum")
        .with_attr("w:abstractNumId", id)
        .with_child(Element::new("w:multiLevelType").with_attr("w:val", "hybridMultilevel"))
        .with_child(Element::new("w:name").with_attr("w:val", name));
    for l in 0..9u32 {
        let (fmt, text) = if bullet {
            ("bullet", ["\u{2022}", "\u{25E6}", "\u{25AA}"][(l % 3) as usize].to_string())
        } else {
            (["decimal", "lowerLetter", "lowerRoman"][(l % 3) as usize], format!("%{}.", l + 1))
        };
        let left = 720 * (l + 1);
        a = a.with_child(
            Element::new("w:lvl")
                .with_attr("w:ilvl", &l.to_string())
                .with_child(Element::new("w:start").with_attr("w:val", "1"))
                .with_child(Element::new("w:numFmt").with_attr("w:val", fmt))
                .with_child(Element::new("w:lvlText").with_attr("w:val", &text))
                .with_child(Element::new("w:lvlJc").with_attr("w:val", "left"))
                .with_child(Element::new("w:pPr").with_child(Element::new("w:ind").with_attr("w:left", &left.to_string()).with_attr("w:hanging", "360"))),
        );
    }
    a
}

/// Now, UTC, as ISO 8601 (`2026-09-30T12:34:56Z`).
pub(crate) fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    iso_from_unix(secs)
}

fn iso_from_unix(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document stripped to the bone: content types, package rels and a
    /// body -- no styles, settings, numbering, comments, headers or
    /// footers -- so every create-from-nothing path runs.
    fn bare() -> Document {
        let mut pkg = Package::empty();
        pkg.set("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
        pkg.set("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        pkg.set("word/document.xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Intro text</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">See the </w:t></w:r><w:r><w:t>Qu website now.</w:t></w:r></w:p></w:body></w:document>"#.to_vec());
        pkg.set("customXml/item1.xml", b"<vendor><keep me=\"1\"/></vendor>".to_vec());
        pkg.set("word/theme/theme1.xml", b"<a:theme xmlns:a=\"urn:a\" name=\"Keep\"><a:x/></a:theme>".to_vec());
        Document::from_package(pkg).unwrap()
    }

    fn roundtrip(d: &mut Document) -> Document {
        Document::from_bytes(&d.to_bytes().unwrap()).unwrap()
    }

    fn part(d: &Document, name: &str) -> String {
        d.pkg.get_str(name).unwrap()
    }

    fn assert_untouched(d: &Document) {
        assert_eq!(d.pkg.get("customXml/item1.xml").unwrap(), b"<vendor><keep me=\"1\"/></vendor>");
        assert_eq!(d.pkg.get("word/theme/theme1.xml").unwrap(), b"<a:theme xmlns:a=\"urn:a\" name=\"Keep\"><a:x/></a:theme>");
    }

    #[test]
    fn link_on_text_spanning_runs_splits_them_and_keeps_formatting() {
        let mut d = bare();
        d.add_link(1, "https://qu-lang.org/?a=1&b=2", Some("the Qu")).unwrap();
        let mut back = roundtrip(&mut d);
        assert_untouched(&back);
        assert_eq!(back.links(), vec![("the Qu".to_string(), "https://qu-lang.org/?a=1&b=2".to_string())]);
        assert_eq!(back.paragraphs()[1], "See the Qu website now.", "text unchanged by the split");
        let p = back.body().elems().filter(|e| e.name == "w:p").nth(1).unwrap().clone();
        let h = p.child("w:hyperlink").unwrap();
        let runs: Vec<&Element> = h.elems().filter(|e| e.name == "w:r").collect();
        assert_eq!(runs.len(), 2, "one piece from each original run");
        assert!(runs[0].child("w:rPr").unwrap().child("w:b").is_some(), "the bold run's piece stays bold");
        assert_eq!(runs[0].child("w:rPr").unwrap().elems().next().unwrap().name, "w:rStyle", "rStyle first");
        let rels = part(&back, "word/_rels/document.xml.rels");
        assert!(rels.contains("TargetMode=\"External\"") && rels.contains("https://qu-lang.org/?a=1&amp;b=2"), "{rels}");
        assert!(part(&back, "word/styles.xml").contains("w:styleId=\"Hyperlink\""), "Hyperlink style created with the styles part");
        assert!(part(&back, "[Content_Types].xml").contains("/word/styles.xml"));
        // Internal link, whole paragraph.
        back.add_bookmark(0, "intro", None).unwrap();
        back.add_link(0, "#intro", None).unwrap();
        assert_eq!(back.links()[0], ("Intro text".to_string(), "#intro".to_string()));
        assert!(back.add_link(1, "https://x", Some("the Qu")).unwrap_err().contains("already"));
        assert!(back.add_link(1, "https://x", Some("absent")).unwrap_err().contains("does not contain"));
    }

    #[test]
    fn comment_created_from_nothing_anchors_a_range() {
        let mut d = bare();
        d.add_comment(1, "Cite this.\nSecond line", Some("website"), "Ahmed", "AK", Some("2026-09-30T10:00:00Z")).unwrap();
        d.add_comment(0, "Whole paragraph", None, "Qu", "", Some("2026-09-30T10:01:00Z")).unwrap();
        let back = roundtrip(&mut d);
        assert_untouched(&back);
        assert_eq!(
            back.comments(),
            vec![
                ("Ahmed".to_string(), "2026-09-30T10:00:00Z".to_string(), "Cite this.\nSecond line".to_string()),
                ("Qu".to_string(), "2026-09-30T10:01:00Z".to_string(), "Whole paragraph".to_string())
            ]
        );
        let p1 = back.body().elems().filter(|e| e.name == "w:p").nth(1).unwrap();
        let names: Vec<&str> = p1.elems().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["w:r", "w:r", "w:commentRangeStart", "w:r", "w:commentRangeEnd", "w:r", "w:r"], "{names:?}");
        assert_eq!(para_display_text(p1), "See the Qu website now.");
        assert!(part(&back, "[Content_Types].xml").contains("comments+xml"));
        assert_eq!(back.info().iter().find(|(k, _)| k == "comments").unwrap().1, "2");
    }

    #[test]
    fn bookmarks_and_cross_references() {
        let mut d = Document::new();
        d.add_heading("Method", 1).unwrap();
        d.add_paragraph("As shown in ", &Format::default()).unwrap();
        d.add_bookmark(0, "method", None).unwrap();
        d.add_cross_ref(1, "method", "text").unwrap();
        d.add_cross_ref(1, "METHOD", "page").unwrap();
        d.add_field(1, "DATE \\@ \"yyyy\"", "").unwrap();
        let back = roundtrip(&mut d);
        assert_eq!(back.bookmarks(), vec![("method".to_string(), "Method".to_string())]);
        assert_eq!(back.fields(), vec!["REF method \\h".to_string(), "PAGEREF method \\h".to_string(), "DATE \\@ \"yyyy\"".to_string()]);
        assert_eq!(back.paragraphs()[1], "As shown in Method", "REF shows the true bookmarked text; PAGEREF nothing yet");
        let mut again = back;
        assert!(again.add_bookmark(1, "method", None).unwrap_err().contains("already"));
        assert!(again.add_bookmark(1, "1bad", None).is_err());
        assert!(again.add_cross_ref(1, "nope", "text").unwrap_err().contains("method"));
    }

    #[test]
    fn toc_is_a_dirty_field_with_update_on_open() {
        let mut d = bare();
        d.add_toc(2, Some(0), Some("Contents")).unwrap();
        let back = roundtrip(&mut d);
        assert_untouched(&back);
        assert_eq!(back.fields(), vec!["TOC \\o \"1-2\" \\h \\z \\u".to_string()]);
        assert_eq!(back.paragraphs()[0], "Contents");
        assert_eq!(back.paragraphs()[1], TOC_PLACEHOLDER, "an instruction, no invented entries");
        assert!(part(&back, "word/document.xml").contains("w:fldCharType=\"begin\" w:dirty=\"true\""));
        let settings = part(&back, "word/settings.xml");
        assert!(settings.contains("<w:updateFields w:val=\"true\"/>"), "{settings}");
        assert!(part(&back, "[Content_Types].xml").contains("settings+xml"));
        assert!(d.add_toc(0, None, None).is_err());
    }

    #[test]
    fn settings_elements_go_where_the_schema_puts_them() {
        let mut d = Document::new(); // template settings: defaultTabStop, compat
        d.add_toc(3, None, None).unwrap();
        d.set_story(false, "H", None, "even", None).unwrap();
        let s = d.pkg.get_xml("word/settings.xml").unwrap();
        let names: Vec<&str> = s.root.elems().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["w:defaultTabStop", "w:evenAndOddHeaders", "w:updateFields", "w:compat"]);
    }

    #[test]
    fn headers_footers_created_edited_and_inherited() {
        let mut d = bare();
        assert_eq!(d.story_text(false, 0, "default").unwrap(), None);
        d.set_story(false, "Qu report", None, "default", Some("right")).unwrap();
        d.set_story(true, "Page #page of #pages", None, "default", Some("center")).unwrap();
        d.set_story(false, "Title page", Some(0), "first", None).unwrap();
        let mut back = roundtrip(&mut d);
        assert_untouched(&back);
        assert_eq!(back.story_text(false, 0, "default").unwrap().as_deref(), Some("Qu report"));
        assert_eq!(back.story_text(true, 0, "default").unwrap().as_deref(), Some("Page 1 of 1"));
        assert_eq!(back.story_text(false, 0, "first").unwrap().as_deref(), Some("Title page"));
        let ftr = part(&back, "word/footer1.xml");
        assert!(ftr.contains(" PAGE ") && ftr.contains(" NUMPAGES ") && ftr.contains("<w:jc w:val=\"center\"/>"), "{ftr}");
        let s = &back.sections()[0];
        assert_eq!(s.headers, vec!["default".to_string(), "first".to_string()]);
        assert_eq!(s.footers, vec!["default".to_string()]);
        let sp = back.sect(0).unwrap();
        let names: Vec<&str> = sp.elems().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["w:headerReference", "w:footerReference", "w:headerReference", "w:titlePg"]);
        // A second section inherits; setting it alone gives it its own.
        back.add_section_break("next_page").unwrap();
        back.add_paragraph("Appendix", &Format::default()).unwrap();
        assert_eq!(back.story_text(false, 1, "default").unwrap().as_deref(), Some("Qu report"), "inherited");
        back.set_story(false, "Appendix header", Some(1), "default", None).unwrap();
        assert_eq!(back.story_text(false, 0, "default").unwrap().as_deref(), Some("Qu report"));
        assert_eq!(back.story_text(false, 1, "default").unwrap().as_deref(), Some("Appendix header"));
        // Section-less set changes every existing default header.
        back.set_story(false, "Both", None, "default", None).unwrap();
        assert_eq!(back.story_text(false, 1, "default").unwrap().as_deref(), Some("Both"));
        assert_eq!(back.story_text(false, 0, "default").unwrap().as_deref(), Some("Both"));
        assert_eq!(back.replace_text("Both", "Each").unwrap(), 2, "new header parts are story parts");
        assert!(back.set_story(false, "x", Some(5), "default", None).is_err());
        assert!(back.set_story(false, "x", None, "odd-ish", None).is_err());
    }

    #[test]
    fn page_setup_and_section_breaks() {
        let mut d = bare(); // no sectPr at all
        let s = d.sections();
        assert_eq!(s.len(), 1);
        d.page_setup(None, page_size_named("letter"), Some(true), [Some(20.0), None, Some(30.0), None]).unwrap();
        let s = &d.sections()[0];
        assert_eq!((s.width_mm, s.height_mm, s.landscape), (279.4, 215.9, true));
        assert_eq!((s.top_mm, s.left_mm, s.bottom_mm), (20.0, 30.0, 20.0));
        let x = part(&roundtrip(&mut d), "word/document.xml");
        assert!(x.contains("<w:pgSz w:w=\"15840\" w:h=\"12240\" w:orient=\"landscape\"/>"), "{x}");
        let mut d = Document::new();
        d.add_paragraph("portrait part", &Format::default()).unwrap();
        d.add_section_break("continuous").unwrap();
        d.add_paragraph("landscape part", &Format::default()).unwrap();
        d.page_setup(Some(1), None, Some(true), [None; 4]).unwrap();
        let back = roundtrip(&mut d);
        let s = back.sections();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].width_mm, s[0].height_mm, s[0].landscape, s[0].start.as_str()), (210.0, 297.0, false, "next_page"));
        assert_eq!((s[1].width_mm, s[1].height_mm, s[1].landscape, s[1].start.as_str()), (297.0, 210.0, true, "continuous"));
        assert_eq!(back.paragraphs(), vec!["portrait part", "", "landscape part"]);
        assert!(d.add_section_break("sideways").is_err());
        assert!(d.page_setup(Some(9), None, Some(true), [None; 4]).is_err());
    }

    #[test]
    fn lists_create_numbering_from_nothing_continue_and_restart() {
        let mut d = bare();
        d.add_list_item("apples", "bullet", 0, false).unwrap();
        d.add_list_item("green", "bullet", 1, false).unwrap();
        d.add_list_item("step one", "number", 0, false).unwrap();
        d.add_list_item("step two", "number", 0, false).unwrap();
        d.add_list_item("again one", "number", 0, true).unwrap();
        d.set_list(0, "number", 0, false).unwrap();
        d.set_list(0, "none", 0, false).unwrap();
        let back = roundtrip(&mut d);
        assert_untouched(&back);
        let num = part(&back, "word/numbering.xml");
        let nd = qu_ooxml::xml::parse(&num).unwrap();
        let order: Vec<&str> = nd.root.elems().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["w:abstractNum", "w:abstractNum", "w:num", "w:num", "w:num"], "abstractNums before nums");
        let ids: Vec<(String, String)> = back
            .body()
            .elems()
            .filter(|p| p.name == "w:p")
            .filter_map(|p| {
                let n = p.child("w:pPr")?.child("w:numPr")?;
                Some((n.child("w:ilvl")?.attr("w:val")?.to_string(), n.child("w:numId")?.attr("w:val")?.to_string()))
            })
            .collect();
        assert_eq!(
            ids,
            vec![("0".into(), "1".into()), ("1".into(), "1".into()), ("0".into(), "2".into()), ("0".into(), "2".into()), ("0".into(), "3".into())],
            "bullets share num 1, numbers continue on 2, restart gets 3; paragraph 0 turned back to plain"
        );
        assert!(num.contains("<w:startOverride w:val=\"1\"/>"));
        assert!(back.to_markdown().contains("- apples\n\n- green"));
        assert!(part(&back, "[Content_Types].xml").contains("numbering+xml"));
        let mut again = back;
        assert!(again.add_list_item("x", "roman", 0, false).is_err());
        assert!(again.add_list_item("x", "bullet", 9, false).is_err());
    }

    #[test]
    fn every_new_edit_leaves_unknown_parts_byte_for_byte() {
        let mut d = bare();
        let before_rels_root = d.pkg.get("_rels/.rels").unwrap().to_vec();
        d.add_link(0, "https://a.b", Some("Intro")).unwrap();
        assert_untouched(&d);
        d.add_comment(1, "c", None, "Qu", "", None).unwrap();
        assert_untouched(&d);
        d.add_bookmark(1, "b1", Some("Qu")).unwrap();
        d.add_cross_ref(0, "b1", "text").unwrap();
        d.add_field(0, "PAGE", "").unwrap();
        d.add_toc(3, None, None).unwrap();
        assert_untouched(&d);
        d.set_story(false, "h", None, "default", None).unwrap();
        d.set_story(true, "#page", None, "even", None).unwrap();
        d.page_setup(None, page_size_named("a5"), None, [Some(10.0); 4]).unwrap();
        d.add_section_break("odd_page").unwrap();
        d.add_list_item("i", "number", 2, true).unwrap();
        let back = roundtrip(&mut d);
        assert_untouched(&back);
        assert_eq!(back.pkg.get("_rels/.rels").unwrap(), before_rels_root.as_slice(), "package rels never needed a change");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_790_000_000), "2026-09-21T14:13:20Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
    }
}
