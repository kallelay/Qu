//! Word `.docx` for Qu, reached through `import docx`.
//!
//! Surgical editing, per `docs/design/toolkit-office.md`: a document is
//! opened as its original OOXML package, the parts an operation touches are
//! parsed into an order-preserving tree and edited in place, and every other
//! part -- themes, fonts, embedded objects, custom XML, macros, vendor
//! extensions -- is written back byte for byte. Nothing is ever rebuilt
//! from a parsed model, which is the failure mode that makes a toy Office
//! library lose half of someone's thesis.
//!
//! "Paragraph `i`" always means the i-th paragraph directly in the body
//! (0-based), the list `paragraphs()` returns; paragraphs inside tables are
//! reached through `tables()`/`set_cell`.

use qu_ooxml::xml::{Doc, Element, Node};
use qu_ooxml::{replace_in_paragraph, Package, WORD};

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Text width of an A4 page with 25 mm margins, the default image width cap.
const TEXT_WIDTH_MM: f64 = 160.0;

pub struct Document {
    pkg: Package,
    main: String,
    doc: Doc,
}

/// Character formatting for `add_paragraph`/`insert_paragraph`.
#[derive(Clone, Debug, Default)]
pub struct Format {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Points.
    pub size: Option<f64>,
    pub font: Option<String>,
    /// `#rrggbb` or `rrggbb`.
    pub color: Option<String>,
    /// `left`, `center`, `right`, `justify`.
    pub align: Option<String>,
    /// A paragraph style id, e.g. `Heading1` or `Quote`.
    pub style: Option<String>,
}

impl Document {
    // ------------------------------------------------------------ open/save

    pub fn open(path: &str) -> Result<Self, String> {
        Self::from_package(Package::open(path)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    fn from_package(pkg: Package) -> Result<Self, String> {
        let main = pkg
            .rels("")?
            .into_iter()
            .find(|r| r.rel_type.ends_with("/officeDocument"))
            .map(|r| qu_ooxml::resolve_target("", &r.target))
            .ok_or("not a Word document: no main document part")?;
        let ct = pkg.override_type(&main).unwrap_or_default();
        if !ct.contains("wordprocessingml") {
            return Err(format!("not a Word document (its main part is `{ct}`)"));
        }
        let doc = pkg.get_xml(&main)?;
        if doc.root.child("w:body").is_none() {
            return Err("not a Word document: the main part has no body".into());
        }
        Ok(Document { pkg, main, doc })
    }

    /// A new, empty A4 document with Normal/Title/Heading 1-3 styles.
    pub fn new() -> Self {
        let mut pkg = Package::empty();
        pkg.set("[Content_Types].xml", TEMPLATE_CONTENT_TYPES.as_bytes().to_vec());
        pkg.set("_rels/.rels", TEMPLATE_ROOT_RELS.as_bytes().to_vec());
        pkg.set("word/document.xml", TEMPLATE_DOCUMENT.as_bytes().to_vec());
        pkg.set("word/_rels/document.xml.rels", TEMPLATE_DOC_RELS.as_bytes().to_vec());
        pkg.set("word/styles.xml", TEMPLATE_STYLES.as_bytes().to_vec());
        pkg.set("word/settings.xml", TEMPLATE_SETTINGS.as_bytes().to_vec());
        pkg.set("docProps/core.xml", qu_ooxml::new_core_xml("Qu").into_bytes());
        pkg.set("docProps/app.xml", TEMPLATE_APP.as_bytes().to_vec());
        Self::from_package(pkg).expect("the built-in template is a valid document")
    }

    fn flush(&mut self) {
        self.pkg.set_xml(&self.main, &self.doc);
    }

    pub fn to_bytes(&mut self) -> Result<Vec<u8>, String> {
        self.flush();
        self.pkg.to_bytes()
    }

    pub fn save(&mut self, path: &str) -> Result<(), String> {
        self.flush();
        self.pkg.save(path)
    }

    fn body(&self) -> &Element {
        self.doc.root.child("w:body").expect("checked at open")
    }

    fn body_mut(&mut self) -> &mut Element {
        self.doc.root.child_mut("w:body").expect("checked at open")
    }

    /// Child indices (in the body) of the body-level paragraphs.
    fn para_slots(&self) -> Vec<usize> {
        self.body()
            .children
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n, Node::Elem(e) if e.name == "w:p"))
            .map(|(i, _)| i)
            .collect()
    }

    fn para_slot(&self, i: usize) -> Result<usize, String> {
        let slots = self.para_slots();
        slots.get(i).copied().ok_or_else(|| format!("paragraph {i} does not exist -- the document has {} (0-based)", slots.len()))
    }

    // ------------------------------------------------------------ reading

    /// Every paragraph's text in document order (body, tables, text
    /// boxes), one per line.
    pub fn text(&self) -> String {
        qu_ooxml::paragraph_paths(self.body(), WORD)
            .iter()
            .map(|p| para_display_text(qu_ooxml::at_path(self.body(), p)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn paragraphs(&self) -> Vec<String> {
        self.body().elems().filter(|e| e.name == "w:p").map(para_display_text).collect()
    }

    /// `(level, text)` for each heading-styled body paragraph; the Title
    /// style is level 0.
    pub fn headings(&self) -> Vec<(u32, String)> {
        let styles = self.style_names();
        self.body()
            .elems()
            .filter(|e| e.name == "w:p")
            .filter_map(|p| heading_level(p, &styles).map(|l| (l, para_display_text(p))))
            .collect()
    }

    /// Indices of the body paragraphs containing `needle`.
    pub fn find_text(&self, needle: &str) -> Vec<usize> {
        self.paragraphs().iter().enumerate().filter(|(_, t)| t.contains(needle)).map(|(i, _)| i).collect()
    }

    /// Each table as rows of cell texts (a cell's paragraphs joined by
    /// newlines). Body-level tables only, document order.
    pub fn tables(&self) -> Vec<Vec<Vec<String>>> {
        self.body()
            .elems()
            .filter(|e| e.name == "w:tbl")
            .map(|t| {
                t.elems()
                    .filter(|r| r.name == "w:tr")
                    .map(|r| {
                        r.elems()
                            .filter(|c| c.name == "w:tc")
                            .map(|c| c.elems().filter(|p| p.name == "w:p").map(para_display_text).collect::<Vec<_>>().join("\n"))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    /// `(author, date, text)` per comment.
    pub fn comments(&self) -> Vec<(String, String, String)> {
        let Some(part) = self.related_part("/comments") else { return Vec::new() };
        let Ok(d) = self.pkg.get_xml(&part) else { return Vec::new() };
        d.root
            .elems()
            .filter(|c| c.name == "w:comment")
            .map(|c| {
                let text = c.elems().filter(|p| p.name == "w:p").map(para_display_text).collect::<Vec<_>>().join("\n");
                (c.attr("w:author").unwrap_or("").to_string(), c.attr("w:date").unwrap_or("").to_string(), text)
            })
            .collect()
    }

    /// Footnote (`"footnotes"`) or endnote (`"endnotes"`) texts, in id
    /// order, the separator entries Word keeps at ids -1/0 excluded.
    pub fn notes(&self, kind: &str) -> Vec<String> {
        let (suffix, tag) = if kind == "endnotes" { ("/endnotes", "w:endnote") } else { ("/footnotes", "w:footnote") };
        let Some(part) = self.related_part(suffix) else { return Vec::new() };
        let Ok(d) = self.pkg.get_xml(&part) else { return Vec::new() };
        d.root
            .elems()
            .filter(|n| n.name == tag && n.attr("w:type").is_none())
            .map(|n| n.elems().filter(|p| p.name == "w:p").map(para_display_text).collect::<Vec<_>>().join("\n").trim().to_string())
            .collect()
    }

    /// Core properties plus counts: paragraphs, words, characters, tables,
    /// images, comments, headings.
    pub fn info(&self) -> Vec<(String, String)> {
        let mut out = self.pkg.core_properties();
        let text = self.text();
        out.push(("paragraphs".into(), self.paragraphs().len().to_string()));
        out.push(("words".into(), text.split_whitespace().count().to_string()));
        out.push(("characters".into(), text.chars().filter(|c| *c != '\n').count().to_string()));
        out.push(("tables".into(), self.body().elems().filter(|e| e.name == "w:tbl").count().to_string()));
        out.push(("images".into(), self.body().find_all("w:drawing").len().to_string()));
        out.push(("comments".into(), self.comments().len().to_string()));
        out.push(("headings".into(), self.headings().len().to_string()));
        out
    }

    pub fn set_properties(&mut self, props: &[(&str, &str)]) -> Result<(), String> {
        self.pkg.set_core_properties(props)
    }

    /// Markdown: headings as `#`, tables as pipe tables, list paragraphs
    /// as `-` items, images as a placeholder line.
    pub fn to_markdown(&self) -> String {
        let styles = self.style_names();
        let mut out: Vec<String> = Vec::new();
        for e in self.body().elems() {
            match e.name.as_str() {
                "w:p" => {
                    let t = para_display_text(e);
                    let has_image = !e.find_all("w:drawing").is_empty();
                    if t.trim().is_empty() {
                        if has_image {
                            out.push("![image]()".into());
                        }
                        continue;
                    }
                    if let Some(l) = heading_level(e, &styles) {
                        out.push(format!("{} {}", "#".repeat(l.max(1) as usize), t.trim()));
                    } else if e.child("w:pPr").and_then(|p| p.child("w:numPr")).is_some() {
                        out.push(format!("- {}", t.trim()));
                    } else {
                        out.push(t);
                    }
                }
                "w:tbl" => {
                    let rows: Vec<Vec<String>> = e
                        .elems()
                        .filter(|r| r.name == "w:tr")
                        .map(|r| {
                            r.elems()
                                .filter(|c| c.name == "w:tc")
                                .map(|c| c.elems().filter(|p| p.name == "w:p").map(para_display_text).collect::<Vec<_>>().join(" ").replace('|', "\\|"))
                                .collect()
                        })
                        .collect();
                    if let Some(first) = rows.first() {
                        let mut md = format!("| {} |\n|{}|", first.join(" | "), vec!["---"; first.len()].join("|"));
                        for r in &rows[1..] {
                            md.push_str(&format!("\n| {} |", r.join(" | ")));
                        }
                        out.push(md);
                    }
                }
                _ => {}
            }
        }
        out.join("\n\n") + "\n"
    }

    // ------------------------------------------------------------ editing

    /// Replace `old` with `new` everywhere text lives: body, tables, text
    /// boxes, headers, footers, footnotes, endnotes and comments -- across
    /// formatting runs, keeping the formatting of the run each match starts
    /// in. Returns how many were replaced.
    pub fn replace_text(&mut self, old: &str, new: &str) -> Result<usize, String> {
        let mut n = 0;
        for path in qu_ooxml::paragraph_paths(&self.doc.root, WORD) {
            n += replace_in_paragraph(qu_ooxml::at_path_mut(&mut self.doc.root, &path), WORD, old, new);
        }
        for part in self.story_parts()? {
            let mut d = self.pkg.get_xml(&part)?;
            let mut here = 0;
            for path in qu_ooxml::paragraph_paths(&d.root, WORD) {
                here += replace_in_paragraph(qu_ooxml::at_path_mut(&mut d.root, &path), WORD, old, new);
            }
            if here > 0 {
                self.pkg.set_xml(&part, &d);
                n += here;
            }
        }
        Ok(n)
    }

    /// Set body paragraph `i`'s text, keeping its paragraph properties and
    /// the character formatting of its first run.
    pub fn set_paragraph(&mut self, i: usize, text: &str) -> Result<(), String> {
        let slot = self.para_slot(i)?;
        let Node::Elem(p) = &mut self.body_mut().children[slot] else { unreachable!() };
        let rpr = p.find_all("w:r").first().and_then(|r| r.child("w:rPr")).cloned();
        let ppr = p.child("w:pPr").cloned();
        p.children.clear();
        if let Some(ppr) = ppr {
            p.children.push(Node::Elem(ppr));
        }
        p.children.push(Node::Elem(run(text, rpr)));
        Ok(())
    }

    pub fn remove_paragraph(&mut self, i: usize) -> Result<(), String> {
        let slot = self.para_slot(i)?;
        self.body_mut().children.remove(slot);
        Ok(())
    }

    /// Insert a paragraph so that it becomes body paragraph `i`
    /// (`i == paragraphs().len()` appends).
    pub fn insert_paragraph(&mut self, i: usize, text: &str, fmt: &Format) -> Result<(), String> {
        let slots = self.para_slots();
        if i > slots.len() {
            return Err(format!("cannot insert at paragraph {i} -- the document has {}", slots.len()));
        }
        let p = self.make_paragraph(text, fmt)?;
        match slots.get(i) {
            Some(&slot) => self.body_mut().children.insert(slot, Node::Elem(p)),
            None => self.append_block(p),
        }
        Ok(())
    }

    pub fn add_paragraph(&mut self, text: &str, fmt: &Format) -> Result<(), String> {
        let p = self.make_paragraph(text, fmt)?;
        self.append_block(p);
        Ok(())
    }

    /// `level` 0 is the Title style, 1-9 the Heading styles.
    pub fn add_heading(&mut self, text: &str, level: u32) -> Result<(), String> {
        if level > 9 {
            return Err(format!("heading level {level} -- Word has Title (0) and Heading 1-9"));
        }
        let style = self.ensure_heading_style(level)?;
        let fmt = Format { style: Some(style), ..Format::default() };
        self.add_paragraph(text, &fmt)
    }

    pub fn add_page_break(&mut self) {
        let p = Element::new("w:p").with_child(Element::new("w:r").with_child(Element::new("w:br").with_attr("w:type", "page")));
        self.append_block(p);
    }

    /// Append a table; `header` makes the first row bold and repeat on
    /// every page.
    pub fn add_table(&mut self, rows: &[Vec<String>], header: bool) -> Result<(), String> {
        let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return Err("add_table: the table has no cells".into());
        }
        let col_w = (TEXT_WIDTH_MM * 56.7 / ncols as f64).round() as i64; // twips
        let mut borders = Element::new("w:tblBorders");
        for side in ["w:top", "w:left", "w:bottom", "w:right", "w:insideH", "w:insideV"] {
            borders = borders.with_child(Element::new(side).with_attr("w:val", "single").with_attr("w:sz", "4").with_attr("w:space", "0").with_attr("w:color", "auto"));
        }
        let mut tbl = Element::new("w:tbl").with_child(
            Element::new("w:tblPr")
                .with_child(Element::new("w:tblW").with_attr("w:w", "0").with_attr("w:type", "auto"))
                .with_child(borders)
                .with_child(Element::new("w:tblLook").with_attr("w:val", "04A0").with_attr("w:firstRow", "1").with_attr("w:lastRow", "0").with_attr("w:firstColumn", "1").with_attr("w:lastColumn", "0").with_attr("w:noHBand", "0").with_attr("w:noVBand", "1")),
        );
        let mut grid = Element::new("w:tblGrid");
        for _ in 0..ncols {
            grid = grid.with_child(Element::new("w:gridCol").with_attr("w:w", &col_w.to_string()));
        }
        tbl = tbl.with_child(grid);
        for (ri, r) in rows.iter().enumerate() {
            let is_header = header && ri == 0;
            let mut tr = Element::new("w:tr");
            if is_header {
                tr = tr.with_child(Element::new("w:trPr").with_child(Element::new("w:tblHeader")));
            }
            for ci in 0..ncols {
                let text = r.get(ci).map(String::as_str).unwrap_or("");
                let rpr = is_header.then(|| Element::new("w:rPr").with_child(Element::new("w:b")));
                tr = tr.with_child(
                    Element::new("w:tc")
                        .with_child(Element::new("w:tcPr").with_child(Element::new("w:tcW").with_attr("w:w", &col_w.to_string()).with_attr("w:type", "dxa")))
                        .with_child(Element::new("w:p").with_child(run(text, rpr))),
                );
            }
            tbl = tbl.with_child(tr);
        }
        self.append_block(tbl);
        // Word ends every table with a paragraph; so do we, so a table
        // appended last does not butt against the section properties.
        self.append_block(Element::new("w:p"));
        Ok(())
    }

    /// Set the text of cell (`row`, `col`) of body table `t` (all 0-based),
    /// keeping the cell's first-run formatting.
    pub fn set_cell(&mut self, t: usize, row: usize, col: usize, text: &str) -> Result<(), String> {
        let tbl = self
            .body_mut()
            .elems_mut()
            .filter(|e| e.name == "w:tbl")
            .nth(t)
            .ok_or_else(|| format!("table {t} does not exist"))?;
        let tr = tbl.elems_mut().filter(|e| e.name == "w:tr").nth(row).ok_or_else(|| format!("table {t} has no row {row}"))?;
        let tc = tr.elems_mut().filter(|e| e.name == "w:tc").nth(col).ok_or_else(|| format!("table {t} row {row} has no column {col}"))?;
        let first_p = tc.elems().find(|e| e.name == "w:p").cloned().unwrap_or_else(|| Element::new("w:p"));
        let rpr = first_p.find_all("w:r").first().and_then(|r| r.child("w:rPr")).cloned();
        let mut p = Element::new("w:p");
        if let Some(ppr) = first_p.child("w:pPr") {
            p.children.push(Node::Elem(ppr.clone()));
        }
        p.children.push(Node::Elem(run(text, rpr)));
        tc.children.retain(|n| !matches!(n, Node::Elem(e) if e.name == "w:p"));
        tc.children.push(Node::Elem(p));
        Ok(())
    }

    /// Append an image as its own paragraph. With neither size given it is
    /// placed at 96 dpi, capped to the text width; with one, the other
    /// follows the aspect ratio.
    pub fn add_image(&mut self, bytes: &[u8], width_mm: Option<f64>, height_mm: Option<f64>) -> Result<(), String> {
        let (px_w, px_h, ext, ct) = qu_ooxml::image_info(bytes)?;
        if px_w == 0 || px_h == 0 {
            return Err("add_image: the image has zero size".into());
        }
        let aspect = px_h as f64 / px_w as f64;
        let (w, h) = match (width_mm, height_mm) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * aspect),
            (None, Some(h)) => (h / aspect, h),
            (None, None) => {
                let w = (px_w as f64 * 25.4 / 96.0).min(TEXT_WIDTH_MM);
                (w, w * aspect)
            }
        };
        let media = self.pkg.next_name("word/media/image", &format!(".{ext}"));
        self.pkg.set(&media, bytes.to_vec());
        self.pkg.ensure_default_type(ext, ct)?;
        let rid = self.pkg.add_rel(&self.main, qu_ooxml::REL_IMAGE, &qu_ooxml::relative_target(&self.main, &media))?;
        for (prefix, ns) in [("xmlns:r", NS_R), ("xmlns:wp", NS_WP)] {
            if self.doc.root.attr(prefix).is_none() {
                self.doc.root.set_attr(prefix, ns);
            }
        }
        let next_id = self.doc.root.find_all("wp:docPr").iter().filter_map(|d| d.attr("id")?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let (cx, cy) = (qu_ooxml::mm_to_emu(w).to_string(), qu_ooxml::mm_to_emu(h).to_string());
        let name = media.rsplit('/').next().unwrap_or("image");
        let pic = Element::new("pic:pic")
            .with_attr("xmlns:pic", NS_PIC)
            .with_child(Element::new("pic:nvPicPr").with_child(Element::new("pic:cNvPr").with_attr("id", "0").with_attr("name", name)).with_child(Element::new("pic:cNvPicPr")))
            .with_child(
                Element::new("pic:blipFill")
                    .with_child(Element::new("a:blip").with_attr("r:embed", &rid))
                    .with_child(Element::new("a:stretch").with_child(Element::new("a:fillRect"))),
            )
            .with_child(
                Element::new("pic:spPr")
                    .with_child(Element::new("a:xfrm").with_child(Element::new("a:off").with_attr("x", "0").with_attr("y", "0")).with_child(Element::new("a:ext").with_attr("cx", &cx).with_attr("cy", &cy)))
                    .with_child(Element::new("a:prstGeom").with_attr("prst", "rect").with_child(Element::new("a:avLst"))),
            );
        let inline = Element::new("wp:inline")
            .with_attr("distT", "0")
            .with_attr("distB", "0")
            .with_attr("distL", "0")
            .with_attr("distR", "0")
            .with_child(Element::new("wp:extent").with_attr("cx", &cx).with_attr("cy", &cy))
            .with_child(Element::new("wp:effectExtent").with_attr("l", "0").with_attr("t", "0").with_attr("r", "0").with_attr("b", "0"))
            .with_child(Element::new("wp:docPr").with_attr("id", &next_id.to_string()).with_attr("name", &format!("Picture {next_id}")))
            .with_child(Element::new("wp:cNvGraphicFramePr").with_child(Element::new("a:graphicFrameLocks").with_attr("xmlns:a", NS_A).with_attr("noChangeAspect", "1")))
            .with_child(
                Element::new("a:graphic")
                    .with_attr("xmlns:a", NS_A)
                    .with_child(Element::new("a:graphicData").with_attr("uri", "http://schemas.openxmlformats.org/drawingml/2006/picture").with_child(pic)),
            );
        let p = Element::new("w:p").with_child(Element::new("w:r").with_child(Element::new("w:drawing").with_child(inline)));
        self.append_block(p);
        Ok(())
    }

    /// Accept (`true`) or reject (`false`) every tracked change in every
    /// story part. Returns the number of revision marks resolved.
    pub fn resolve_changes(&mut self, accept: bool) -> Result<usize, String> {
        let mut n = resolve_revisions(&mut self.doc.root, accept);
        for part in self.story_parts()? {
            let mut d = self.pkg.get_xml(&part)?;
            let here = resolve_revisions(&mut d.root, accept);
            if here > 0 {
                self.pkg.set_xml(&part, &d);
                n += here;
            }
        }
        Ok(n)
    }

    // ------------------------------------------------------------ internals

    /// Insert a block-level element at the end of the body, before the
    /// final section properties.
    fn append_block(&mut self, e: Element) {
        let body = self.body_mut();
        let pos = body.children.iter().rposition(|n| matches!(n, Node::Elem(e) if e.name == "w:sectPr")).unwrap_or(body.children.len());
        body.children.insert(pos, Node::Elem(e));
    }

    fn make_paragraph(&mut self, text: &str, fmt: &Format) -> Result<Element, String> {
        let mut ppr = Element::new("w:pPr");
        if let Some(s) = &fmt.style {
            ppr = ppr.with_child(Element::new("w:pStyle").with_attr("w:val", s));
        }
        if let Some(a) = &fmt.align {
            let v = match a.as_str() {
                "left" => "left",
                "center" | "centre" => "center",
                "right" => "right",
                "justify" | "justified" | "both" => "both",
                other => return Err(format!("align=\"{other}\" -- use left, center, right or justify")),
            };
            ppr = ppr.with_child(Element::new("w:jc").with_attr("w:val", v));
        }
        let mut rpr = Element::new("w:rPr");
        if let Some(f) = &fmt.font {
            rpr = rpr.with_child(Element::new("w:rFonts").with_attr("w:ascii", f).with_attr("w:hAnsi", f).with_attr("w:cs", f));
        }
        if fmt.bold {
            rpr = rpr.with_child(Element::new("w:b"));
        }
        if fmt.italic {
            rpr = rpr.with_child(Element::new("w:i"));
        }
        if let Some(c) = &fmt.color {
            rpr = rpr.with_child(Element::new("w:color").with_attr("w:val", &hex_color(c)?));
        }
        if let Some(sz) = fmt.size {
            if !(sz > 0.0 && sz <= 1638.0) {
                return Err(format!("size={sz} pt is outside what Word stores (0-1638 pt)"));
            }
            let half = ((sz * 2.0).round() as i64).to_string();
            rpr = rpr.with_child(Element::new("w:sz").with_attr("w:val", &half)).with_child(Element::new("w:szCs").with_attr("w:val", &half));
        }
        if fmt.underline {
            rpr = rpr.with_child(Element::new("w:u").with_attr("w:val", "single"));
        }
        let mut p = Element::new("w:p");
        if !ppr.children.is_empty() {
            p.children.push(Node::Elem(ppr));
        }
        let rpr = (!rpr.children.is_empty()).then_some(rpr);
        p.children.push(Node::Elem(run(text, rpr)));
        Ok(p)
    }

    /// styleId -> lower-cased display name, from `styles.xml`.
    fn style_names(&self) -> Vec<(String, String)> {
        let Some(part) = self.related_part("/styles") else { return Vec::new() };
        let Ok(d) = self.pkg.get_xml(&part) else { return Vec::new() };
        d.root
            .elems()
            .filter(|s| s.name == "w:style")
            .filter_map(|s| Some((s.attr("w:styleId")?.to_string(), s.child("w:name")?.attr("w:val")?.to_lowercase())))
            .collect()
    }

    fn ensure_heading_style(&mut self, level: u32) -> Result<String, String> {
        let want = if level == 0 { "title".to_string() } else { format!("heading {level}") };
        if let Some((id, _)) = self.style_names().into_iter().find(|(_, n)| *n == want) {
            return Ok(id);
        }
        let part = match self.related_part("/styles") {
            Some(p) => p,
            None => {
                let p = "word/styles.xml".to_string();
                self.pkg.set(&p, format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:styles xmlns:w=\"{NS_W}\"/>").into_bytes());
                self.pkg.add_override(&p, CT_STYLES)?;
                self.pkg.add_rel(&self.main, REL_STYLES, "styles.xml")?;
                p
            }
        };
        let mut d = self.pkg.get_xml(&part)?;
        let (id, name) = if level == 0 { ("Title".to_string(), "Title".to_string()) } else { (format!("Heading{level}"), format!("heading {level}")) };
        d.root.children.push(Node::Elem(heading_style(&id, &name, level)));
        self.pkg.set_xml(&part, &d);
        Ok(id)
    }

    /// The part the main document relates to with a type ending `suffix`.
    fn related_part(&self, suffix: &str) -> Option<String> {
        self.pkg.rels(&self.main).ok()?.into_iter().find(|r| r.rel_type.ends_with(suffix)).map(|r| qu_ooxml::resolve_target(&self.main, &r.target))
    }

    /// Headers, footers, footnotes, endnotes, comments.
    fn story_parts(&self) -> Result<Vec<String>, String> {
        Ok(self
            .pkg
            .rels(&self.main)?
            .into_iter()
            .filter(|r| !r.external && ["/header", "/footer", "/footnotes", "/endnotes", "/comments"].iter().any(|s| r.rel_type.ends_with(s)))
            .map(|r| qu_ooxml::resolve_target(&self.main, &r.target))
            .filter(|p| self.pkg.has(p))
            .collect())
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// A paragraph's text as a reader sees it: tabs and line breaks included.
fn para_display_text(p: &Element) -> String {
    fn walk(e: &Element, out: &mut String) {
        for n in &e.children {
            if let Node::Elem(c) = n {
                match c.name.as_str() {
                    "w:t" => out.push_str(&c.text()),
                    "w:tab" => out.push('\t'),
                    "w:br" | "w:cr" => {
                        if c.attr("w:type") != Some("page") {
                            out.push('\n')
                        }
                    }
                    "w:p" | "w:delText" | "w:instrText" => {}
                    _ => walk(c, out),
                }
            }
        }
    }
    let mut s = String::new();
    walk(p, &mut s);
    s
}

fn heading_level(p: &Element, styles: &[(String, String)]) -> Option<u32> {
    let ppr = p.child("w:pPr")?;
    if let Some(id) = ppr.child("w:pStyle").and_then(|s| s.attr("w:val")) {
        let name = styles.iter().find(|(i, _)| i == id).map(|(_, n)| n.as_str()).unwrap_or("");
        if name == "title" || id.eq_ignore_ascii_case("title") {
            return Some(0);
        }
        let n = name.strip_prefix("heading ").or_else(|| id.strip_prefix("Heading"));
        if let Some(l) = n.and_then(|n| n.trim().parse::<u32>().ok()) {
            return Some(l);
        }
    }
    ppr.child("w:outlineLvl").and_then(|o| o.attr("w:val")).and_then(|v| v.parse::<u32>().ok()).filter(|&l| l < 9).map(|l| l + 1)
}

fn heading_style(id: &str, name: &str, level: u32) -> Element {
    let size = match level {
        0 => 56,
        1 => 32,
        2 => 26,
        3 => 24,
        _ => 22,
    };
    let mut ppr = Element::new("w:pPr").with_child(Element::new("w:keepNext")).with_child(Element::new("w:spacing").with_attr("w:before", if level == 0 { "0" } else { "240" }).with_attr("w:after", "80"));
    if level > 0 {
        ppr = ppr.with_child(Element::new("w:outlineLvl").with_attr("w:val", &(level - 1).to_string()));
    }
    let mut rpr = Element::new("w:rPr");
    if level > 0 {
        rpr = rpr.with_child(Element::new("w:b"));
    }
    rpr = rpr.with_child(Element::new("w:sz").with_attr("w:val", &size.to_string())).with_child(Element::new("w:szCs").with_attr("w:val", &size.to_string()));
    Element::new("w:style")
        .with_attr("w:type", "paragraph")
        .with_attr("w:styleId", id)
        .with_child(Element::new("w:name").with_attr("w:val", name))
        .with_child(Element::new("w:basedOn").with_attr("w:val", "Normal"))
        .with_child(Element::new("w:next").with_attr("w:val", "Normal"))
        .with_child(Element::new("w:qFormat"))
        .with_child(ppr)
        .with_child(rpr)
}

fn hex_color(c: &str) -> Result<String, String> {
    let h = c.trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(h.to_uppercase())
    } else {
        Err(format!("color=\"{c}\" -- use #rrggbb"))
    }
}

/// A run carrying `text`, with `\n` as line breaks and `\t` as tabs.
fn run(text: &str, rpr: Option<Element>) -> Element {
    let mut r = Element::new("w:r");
    if let Some(rpr) = rpr {
        r.children.push(Node::Elem(rpr));
    }
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            r.children.push(Node::Elem(Element::new("w:br")));
        }
        for (ti, piece) in line.split('\t').enumerate() {
            if ti > 0 {
                r.children.push(Node::Elem(Element::new("w:tab")));
            }
            if !piece.is_empty() {
                let mut t = Element::new("w:t").with_text(piece);
                if piece.starts_with(char::is_whitespace) || piece.ends_with(char::is_whitespace) {
                    t.set_attr("xml:space", "preserve");
                }
                r.children.push(Node::Elem(t));
            }
        }
    }
    r
}

/// Accept or reject the revision marks under `root`; returns how many.
fn resolve_revisions(root: &mut Element, accept: bool) -> usize {
    let mut count = 0;
    // Property changes: accepting keeps the new properties (drop the
    // record); rejecting restores the old ones stored inside it.
    root.walk_mut(&mut |e| {
        for (change, holder) in [("w:rPrChange", "w:rPr"), ("w:pPrChange", "w:pPr"), ("w:tblPrChange", "w:tblPr"), ("w:trPrChange", "w:trPr"), ("w:tcPrChange", "w:tcPr"), ("w:sectPrChange", "w:sectPr")] {
            if e.name != holder {
                continue;
            }
            if let Some(ch) = e.child(change).cloned() {
                count += 1;
                if accept {
                    e.remove_children(change);
                } else {
                    let old = ch.child(holder).cloned().unwrap_or_else(|| Element::new(holder));
                    // Keep the paragraph's own run-properties mark, which
                    // `w:pPrChange` does not record.
                    let keep: Vec<Node> = e.children.iter().filter(|n| matches!(n, Node::Elem(c) if holder == "w:pPr" && c.name == "w:rPr")).cloned().collect();
                    e.children = old.children.clone();
                    e.children.extend(keep);
                }
            }
        }
    });
    // Insertions/deletions/moves: unwrap the survivors, drop the rest.
    fn content(root: &mut Element, accept: bool, count: &mut usize) {
        let mut out: Vec<Node> = Vec::with_capacity(root.children.len());
        for n in std::mem::take(&mut root.children) {
            match n {
                Node::Elem(mut e) => {
                    let keep = match e.name.as_str() {
                        "w:ins" | "w:moveTo" => Some(accept),
                        "w:del" | "w:moveFrom" => Some(!accept),
                        _ => None,
                    };
                    match keep {
                        Some(k) => {
                            *count += 1;
                            // A paragraph-mark revision is an empty marker
                            // inside w:rPr: resolve by just dropping it.
                            if k {
                                content(&mut e, accept, count);
                                for mut c in e.children {
                                    if let Node::Elem(ce) = &mut c {
                                        undelete(ce);
                                    }
                                    out.push(c);
                                }
                            }
                        }
                        None => {
                            if matches!(e.name.as_str(), "w:moveFromRangeStart" | "w:moveFromRangeEnd" | "w:moveToRangeStart" | "w:moveToRangeEnd") {
                                continue;
                            }
                            content(&mut e, accept, count);
                            out.push(Node::Elem(e));
                        }
                    }
                }
                other => out.push(other),
            }
        }
        root.children = out;
    }
    fn undelete(e: &mut Element) {
        e.walk_mut(&mut |x| match x.name.as_str() {
            "w:delText" => x.name = "w:t".into(),
            "w:delInstrText" => x.name = "w:instrText".into(),
            _ => {}
        });
    }
    content(root, accept, &mut count);
    count
}

// ------------------------------------------------------------------ template

const TEMPLATE_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#;

const TEMPLATE_ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;

const TEMPLATE_DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;

const TEMPLATE_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1134" w:left="1417" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/></w:sectPr></w:body></w:document>"#;

const TEMPLATE_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="80"/></w:pPr><w:rPr><w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="160" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="160" w:after="80"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style></w:styles>"#;

const TEMPLATE_SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:defaultTabStop w:val="708"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#;

const TEMPLATE_APP: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>Qu</Application></Properties>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(d: &mut Document) -> Document {
        Document::from_bytes(&d.to_bytes().unwrap()).unwrap()
    }

    #[test]
    fn build_read_back_and_edit() {
        let mut d = Document::new();
        d.add_heading("Results", 1).unwrap();
        d.add_paragraph("The measured impedance was 2025 ohm.", &Format { bold: true, size: Some(12.0), ..Format::default() }).unwrap();
        d.add_table(&[vec!["f".into(), "Z".into()], vec!["1000".into(), "42.3".into()]], true).unwrap();
        d.add_page_break();
        d.add_paragraph("line one\nline two\tafter tab", &Format::default()).unwrap();
        let mut back = roundtrip(&mut d);
        assert_eq!(back.headings(), vec![(1, "Results".to_string())]);
        assert_eq!(back.tables()[0][1], vec!["1000".to_string(), "42.3".to_string()]);
        assert!(back.paragraphs().contains(&"line one\nline two\tafter tab".to_string()));
        assert_eq!(back.replace_text("2025", "2026").unwrap(), 1);
        back.set_cell(0, 1, 1, "43.0").unwrap();
        back.set_paragraph(0, "Findings").unwrap();
        let again = roundtrip(&mut back);
        assert!(again.text().contains("2026 ohm"));
        assert_eq!(again.tables()[0][1][1], "43.0");
        assert_eq!(again.headings(), vec![(1, "Findings".to_string())], "set_paragraph keeps the heading style");
        assert!(again.to_markdown().starts_with("# Findings\n\n**") || again.to_markdown().starts_with("# Findings\n\nThe"));
    }

    #[test]
    fn unknown_parts_survive_untouched() {
        let mut d = Document::new();
        d.pkg.set("customXml/item1.xml", b"<vendor><keep me=\"1\"/></vendor>".to_vec());
        d.add_paragraph("x", &Format::default()).unwrap();
        let mut back = roundtrip(&mut d);
        back.replace_text("x", "y").unwrap();
        let again = roundtrip(&mut back);
        assert_eq!(again.pkg.get("customXml/item1.xml").unwrap(), b"<vendor><keep me=\"1\"/></vendor>");
    }

    #[test]
    fn tracked_changes_accept_and_reject() {
        let body = r#"<w:p><w:r><w:t xml:space="preserve">keep </w:t></w:r><w:ins w:id="1" w:author="A"><w:r><w:t>new</w:t></w:r></w:ins><w:del w:id="2" w:author="A"><w:r><w:delText>old</w:delText></w:r></w:del></w:p>"#;
        let make = || {
            let mut d = Document::new();
            let p = qu_ooxml::xml::parse(&body.replace("<w:p>", &format!("<w:p xmlns:w=\"{NS_W}\">"))).unwrap().root;
            d.append_block(p);
            d
        };
        let mut a = make();
        assert_eq!(a.resolve_changes(true).unwrap(), 2);
        assert_eq!(a.paragraphs()[0], "keep new");
        let mut r = make();
        r.resolve_changes(false).unwrap();
        assert_eq!(r.paragraphs()[0], "keep old");
    }

    #[test]
    fn a_non_word_package_is_refused() {
        assert!(Document::from_bytes(b"not a zip").is_err());
    }
}
