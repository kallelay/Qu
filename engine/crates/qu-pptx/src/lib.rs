//! PowerPoint `.pptx` for Qu, reached through `import pptx`.
//!
//! Same surgical rule as `qu-docx` (see `docs/design/toolkit-office.md`):
//! the original package is kept, only the parts an operation touches are
//! re-serialized, and everything else -- media, embedded fonts, charts,
//! SmartArt, animations, comments, vendor extensions -- passes through
//! byte for byte. A slide is a canvas of shapes positioned in EMU; this
//! crate speaks millimetres and points at its boundary and converts.
//!
//! Slides are numbered from 0 in presentation order (the order of
//! `p:sldIdLst`, which is what PowerPoint shows, not the part file names).

mod edit;
pub use edit::{Order, ShapeInfo, ShapeRef, THEME_SLOTS};

use qu_ooxml::xml::{Doc, Element, Node};
use qu_ooxml::{mm_to_emu, relative_target, replace_in_paragraph, resolve_target, Package, DRAWING};

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const CT_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const REL_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
/// PowerPoint's built-in "Medium Style 2 - Accent 1" table style.
const TABLE_STYLE: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

pub struct Presentation {
    pkg: Package,
    main: String,
    pres: Doc,
}

/// Text formatting for `add_text`.
#[derive(Clone, Debug, Default)]
pub struct TextFormat {
    /// Points.
    pub size: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<String>,
    pub font: Option<String>,
    /// `left`, `center`, `right`, `justify`.
    pub align: Option<String>,
}

/// A rectangle in millimetres from the slide's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Presentation {
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
            .map(|r| resolve_target("", &r.target))
            .ok_or("not a presentation: no main part")?;
        let ct = pkg.override_type(&main).unwrap_or_default();
        if !ct.contains("presentationml") {
            return Err(format!("not a PowerPoint presentation (its main part is `{ct}`)"));
        }
        let pres = pkg.get_xml(&main)?;
        Ok(Presentation { pkg, main, pres })
    }

    /// A new, empty 16:9 presentation with four layouts: Title Slide,
    /// Title and Content, Title Only, Blank.
    pub fn new() -> Self {
        let mut pkg = Package::empty();
        for (name, body) in TEMPLATE {
            pkg.set(name, body.as_bytes().to_vec());
        }
        pkg.set("docProps/core.xml", qu_ooxml::new_core_xml("Qu").into_bytes());
        Self::from_package(pkg).expect("the built-in template is a valid presentation")
    }

    fn flush(&mut self) {
        self.pkg.set_xml(&self.main, &self.pres);
    }

    pub fn to_bytes(&mut self) -> Result<Vec<u8>, String> {
        self.flush();
        self.pkg.to_bytes()
    }

    pub fn save(&mut self, path: &str) -> Result<(), String> {
        self.flush();
        self.pkg.save(path)
    }

    // ------------------------------------------------------------ slides

    fn slide_ids(&self) -> Vec<(String, String)> {
        self.pres
            .root
            .child("p:sldIdLst")
            .map(|l| l.elems().filter(|e| e.name == "p:sldId").map(|e| (e.attr("id").unwrap_or("").to_string(), e.attr("r:id").unwrap_or("").to_string())).collect())
            .unwrap_or_default()
    }

    /// Slide part names in presentation order.
    pub fn slide_parts(&self) -> Result<Vec<String>, String> {
        let rels = self.pkg.rels(&self.main)?;
        self.slide_ids()
            .into_iter()
            .map(|(_, rid)| {
                rels.iter().find(|r| r.id == rid).map(|r| resolve_target(&self.main, &r.target)).ok_or_else(|| format!("slide relationship `{rid}` is missing"))
            })
            .collect()
    }

    pub fn slide_count(&self) -> usize {
        self.slide_ids().len()
    }

    fn slide_part(&self, i: usize) -> Result<String, String> {
        let parts = self.slide_parts()?;
        parts.get(i).cloned().ok_or_else(|| format!("slide {i} does not exist -- the presentation has {} (0-based)", parts.len()))
    }

    /// The text of slide `i`: one line per non-empty paragraph, shapes and
    /// tables in document order.
    pub fn slide_text(&self, i: usize) -> Result<String, String> {
        let d = self.pkg.get_xml(&self.slide_part(i)?)?;
        Ok(doc_lines(&d.root).join("\n"))
    }

    /// The slide's title placeholder text, if it has one.
    pub fn slide_title(&self, i: usize) -> Result<Option<String>, String> {
        let d = self.pkg.get_xml(&self.slide_part(i)?)?;
        Ok(d.root.find_all("p:sp").into_iter().find(|sp| placeholder_type(sp).is_some_and(|t| t == "title" || t == "ctrTitle")).map(|sp| doc_lines(sp).join(" ")))
    }

    /// The speaker notes of slide `i` ("" if none).
    pub fn notes(&self, i: usize) -> Result<String, String> {
        match self.notes_part(&self.slide_part(i)?)? {
            None => Ok(String::new()),
            Some(np) => {
                let d = self.pkg.get_xml(&np)?;
                Ok(d.root.find_all("p:sp").into_iter().filter(|sp| placeholder_type(sp) == Some("body")).flat_map(|sp| doc_lines(sp)).collect::<Vec<_>>().join("\n"))
            }
        }
    }

    fn notes_part(&self, slide: &str) -> Result<Option<String>, String> {
        Ok(self.pkg.rels(slide)?.into_iter().find(|r| r.rel_type.ends_with("/notesSlide")).map(|r| resolve_target(slide, &r.target)))
    }

    /// Indices of slides whose text contains `needle`.
    pub fn find_text(&self, needle: &str) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for i in 0..self.slide_count() {
            if self.slide_text(i)?.contains(needle) {
                out.push(i);
            }
        }
        Ok(out)
    }

    /// Replace across every slide (and, with `notes`, the speaker notes),
    /// across formatting runs. Returns the count.
    pub fn replace_text(&mut self, old: &str, new: &str, notes: bool) -> Result<usize, String> {
        let mut n = 0;
        for slide in self.slide_parts()? {
            let mut parts = vec![slide.clone()];
            if notes {
                parts.extend(self.notes_part(&slide)?);
            }
            for part in parts {
                let mut d = self.pkg.get_xml(&part)?;
                let mut here = 0;
                for path in qu_ooxml::paragraph_paths(&d.root, DRAWING) {
                    here += replace_in_paragraph(qu_ooxml::at_path_mut(&mut d.root, &path), DRAWING, old, new);
                }
                if here > 0 {
                    self.pkg.set_xml(&part, &d);
                    n += here;
                }
            }
        }
        Ok(n)
    }

    pub fn delete_slide(&mut self, i: usize) -> Result<(), String> {
        let part = self.slide_part(i)?;
        let (sid, rid) = self.slide_ids()[i].clone();
        if let Some(np) = self.notes_part(&part)? {
            self.remove_part(&np)?;
        }
        self.remove_part(&part)?;
        self.pkg.remove_rel(&self.main, &rid)?;
        // The slide list, plus any section (p14:sectionLst) listing it.
        self.pres.root.walk_mut(&mut |e| {
            if e.name == "p:sldIdLst" {
                e.children.retain(|n| !matches!(n, Node::Elem(c) if c.attr("r:id") == Some(rid.as_str())));
            } else if e.name.ends_with(":sldIdLst") {
                e.children.retain(|n| !matches!(n, Node::Elem(c) if c.attr("id") == Some(sid.as_str())));
            }
        });
        Ok(())
    }

    fn remove_part(&mut self, part: &str) -> Result<(), String> {
        self.pkg.remove(part);
        self.pkg.remove(&qu_ooxml::rels_path(part));
        self.pkg.remove_override(part)
    }

    /// Move slide `from` so it ends up at position `to`.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<(), String> {
        let n = self.slide_count();
        if from >= n || to >= n {
            return Err(format!("move_slide({from}, {to}) -- the presentation has {n} slides (0-based)"));
        }
        let lst = self.pres.root.child_mut("p:sldIdLst").expect("slides exist");
        let mut ids: Vec<Node> = lst.children.iter().filter(|c| matches!(c, Node::Elem(e) if e.name == "p:sldId")).cloned().collect();
        let item = ids.remove(from);
        ids.insert(to, item);
        lst.children = ids;
        Ok(())
    }

    /// Copy slide `i` (shapes, images, layout link -- not its notes) and
    /// insert the copy right after it. Returns the new slide's index.
    pub fn duplicate_slide(&mut self, i: usize) -> Result<usize, String> {
        let src = self.slide_part(i)?;
        let dst = self.pkg.next_name("ppt/slides/slide", ".xml");
        self.pkg.set(&dst, self.pkg.get(&src).unwrap().to_vec());
        let src_rels = qu_ooxml::rels_path(&src);
        if let Some(bytes) = self.pkg.get(&src_rels) {
            let mut d = qu_ooxml::xml::parse(&String::from_utf8_lossy(bytes))?;
            // A notes slide and comments belong to one slide only.
            d.root.children.retain(|n| !matches!(n, Node::Elem(e) if e.attr("Type").is_some_and(|t| t.ends_with("/notesSlide") || t.ends_with("/comments"))));
            self.pkg.set_xml(&qu_ooxml::rels_path(&dst), &d);
        }
        self.pkg.add_override(&dst, CT_SLIDE)?;
        self.insert_slide_ref(&dst, Some(i + 1))?;
        Ok(i + 1)
    }

    /// Hide (`true`) or show slide `i` in the slide show.
    pub fn set_hidden(&mut self, i: usize, hidden: bool) -> Result<(), String> {
        let part = self.slide_part(i)?;
        let mut d = self.pkg.get_xml(&part)?;
        if hidden {
            d.root.set_attr("show", "0");
        } else {
            d.root.remove_attr("show");
        }
        self.pkg.set_xml(&part, &d);
        Ok(())
    }

    /// Layout names, in the order `add_slide(layout=)` indexes them.
    pub fn layouts(&self) -> Result<Vec<String>, String> {
        Ok(self.layout_parts()?.into_iter().map(|(_, n)| n).collect())
    }

    fn layout_parts(&self) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        for r in self.pkg.rels(&self.main)?.into_iter().filter(|r| r.rel_type.ends_with("/slideMaster")) {
            let master = resolve_target(&self.main, &r.target);
            let mrels = self.pkg.rels(&master)?;
            let md = self.pkg.get_xml(&master)?;
            let ids: Vec<String> = md.root.child("p:sldLayoutIdLst").map(|l| l.elems().filter_map(|e| e.attr("r:id").map(String::from)).collect()).unwrap_or_default();
            for rid in ids {
                if let Some(lr) = mrels.iter().find(|x| x.id == rid) {
                    let lp = resolve_target(&master, &lr.target);
                    let name = self.pkg.get_xml(&lp).ok().and_then(|d| d.root.child("p:cSld").and_then(|c| c.attr("name").map(String::from))).unwrap_or_default();
                    out.push((lp, name));
                }
            }
        }
        Ok(out)
    }

    /// Add a slide from a layout (by name, case-insensitive, or `None` for
    /// "Title and Content" when present), filling its title and body
    /// placeholders. `at` inserts at that index instead of appending.
    /// Returns the new slide's index.
    pub fn add_slide(&mut self, layout: Option<&str>, title: Option<&str>, body: &[String], at: Option<usize>) -> Result<usize, String> {
        let layouts = self.layout_parts()?;
        if layouts.is_empty() {
            return Err("the presentation has no slide layouts to build a slide from".into());
        }
        let (lpart, _) = match layout {
            Some(want) => layouts
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(want))
                .or_else(|| want.parse::<usize>().ok().and_then(|i| layouts.get(i)))
                .ok_or_else(|| format!("no layout `{want}` -- this presentation has: {}", layouts.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ")))?,
            None => layouts.iter().find(|(_, n)| n.eq_ignore_ascii_case("Title and Content")).unwrap_or(&layouts[0]),
        }
        .clone();
        let ld = self.pkg.get_xml(&lpart)?;
        let phs: Vec<(Option<String>, Option<String>)> = ld
            .root
            .find_all("p:ph")
            .into_iter()
            .map(|ph| (ph.attr("type").map(String::from), ph.attr("idx").map(String::from)))
            .collect();
        let mut tree = sp_tree_root();
        let mut next_id = 2;
        if let Some(t) = title {
            let ph = phs.iter().find(|(t, _)| matches!(t.as_deref(), Some("title") | Some("ctrTitle"))).ok_or("this layout has no title placeholder -- pick another layout=")?;
            tree.children.push(Node::Elem(placeholder_sp(next_id, "Title", ph, &[t.to_string()])));
            next_id += 1;
        }
        if !body.is_empty() {
            let ph = phs
                .iter()
                .find(|(t, idx)| matches!(t.as_deref(), None | Some("body") | Some("obj") | Some("subTitle")) && idx.is_some())
                .ok_or("this layout has no body placeholder -- pick another layout=, or use add_text")?;
            tree.children.push(Node::Elem(placeholder_sp(next_id, "Content", ph, body)));
        }
        let slide = Element::new("p:sld")
            .with_attr("xmlns:a", NS_A)
            .with_attr("xmlns:r", NS_R)
            .with_attr("xmlns:p", NS_P)
            .with_child(Element::new("p:cSld").with_child(tree))
            .with_child(Element::new("p:clrMapOvr").with_child(Element::new("a:masterClrMapping")));
        let part = self.pkg.next_name("ppt/slides/slide", ".xml");
        self.pkg.set_xml(&part, &Doc::new(slide));
        self.pkg.add_rel(&part, REL_LAYOUT, &relative_target(&part, &lpart))?;
        self.pkg.add_override(&part, CT_SLIDE)?;
        self.insert_slide_ref(&part, at)
    }

    fn insert_slide_ref(&mut self, part: &str, at: Option<usize>) -> Result<usize, String> {
        let rid = self.pkg.add_rel(&self.main, REL_SLIDE, &relative_target(&self.main, part))?;
        let max_id = self.slide_ids().iter().filter_map(|(id, _)| id.parse::<u64>().ok()).max().unwrap_or(255);
        let entry = Element::new("p:sldId").with_attr("id", &(max_id + 1).to_string()).with_attr("r:id", &rid);
        if self.pres.root.child("p:sldIdLst").is_none() {
            // Schema order: sldMasterIdLst, notesMasterIdLst,
            // handoutMasterIdLst, sldIdLst, sldSz, ...
            let pos = self.pres.root.children.iter().position(|n| matches!(n, Node::Elem(e) if !matches!(e.name.as_str(), "p:sldMasterIdLst" | "p:notesMasterIdLst" | "p:handoutMasterIdLst"))).unwrap_or(self.pres.root.children.len());
            self.pres.root.children.insert(pos, Node::Elem(Element::new("p:sldIdLst")));
        }
        let lst = self.pres.root.child_mut("p:sldIdLst").unwrap();
        let n = lst.elems().filter(|e| e.name == "p:sldId").count();
        let at = at.unwrap_or(n).min(n);
        let slot = lst.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Elem(e) if e.name == "p:sldId")).nth(at).map(|(i, _)| i).unwrap_or(lst.children.len());
        lst.children.insert(slot, Node::Elem(entry));
        Ok(at)
    }

    /// Slide width and height in millimetres.
    pub fn slide_size(&self) -> (f64, f64) {
        let sz = self.pres.root.child("p:sldSz");
        let get = |k: &str, d: f64| sz.and_then(|s| s.attr(k)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
        (get("cx", 9144000.0) / qu_ooxml::EMU_PER_MM, get("cy", 6858000.0) / qu_ooxml::EMU_PER_MM)
    }

    // ------------------------------------------------------------ shapes

    fn edit_tree(&mut self, i: usize, f: impl FnOnce(&mut Element, u64) -> Result<(), String>) -> Result<String, String> {
        let part = self.slide_part(i)?;
        let mut d = self.pkg.get_xml(&part)?;
        let next_id = d.root.find_all("p:cNvPr").iter().filter_map(|c| c.attr("id")?.parse::<u64>().ok()).max().unwrap_or(1) + 1;
        let tree = d.root.child_mut("p:cSld").and_then(|c| c.child_mut("p:spTree")).ok_or("the slide has no shape tree")?;
        f(tree, next_id)?;
        self.pkg.set_xml(&part, &d);
        Ok(part)
    }

    /// Add a text box at `r` (mm). Newlines start new paragraphs.
    pub fn add_text(&mut self, i: usize, text: &str, r: Rect, fmt: &TextFormat) -> Result<(), String> {
        let rpr = run_props(fmt)?;
        let algn = align_attr(fmt.align.as_deref())?;
        self.edit_tree(i, |tree, id| {
            let mut body = Element::new("p:txBody").with_child(Element::new("a:bodyPr").with_attr("wrap", "square").with_attr("rtlCol", "0")).with_child(Element::new("a:lstStyle"));
            for line in text.split('\n') {
                body = body.with_child(paragraph(line, &rpr, algn));
            }
            let sp = Element::new("p:sp")
                .with_child(
                    Element::new("p:nvSpPr")
                        .with_child(Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &format!("TextBox {id}")))
                        .with_child(Element::new("p:cNvSpPr").with_attr("txBox", "1"))
                        .with_child(Element::new("p:nvPr")),
                )
                .with_child(Element::new("p:spPr").with_child(xfrm("a:xfrm", r)).with_child(rect_geom()).with_child(Element::new("a:noFill")))
                .with_child(body);
            tree.children.push(Node::Elem(sp));
            Ok(())
        })?;
        Ok(())
    }

    /// Add a PNG/JPEG/GIF at `r` (mm); a zero width or height follows the
    /// image's aspect ratio, and both zero means 96 dpi, centred.
    pub fn add_image(&mut self, i: usize, bytes: &[u8], r: Rect) -> Result<(), String> {
        let (pw, ph, ext, ct) = qu_ooxml::image_info(bytes)?;
        if pw == 0 || ph == 0 {
            return Err("add_image: the image has zero size".into());
        }
        let aspect = ph as f64 / pw as f64;
        let (sw, sh) = self.slide_size();
        let mut r = r;
        match (r.w > 0.0, r.h > 0.0) {
            (true, false) => r.h = r.w * aspect,
            (false, true) => r.w = r.h / aspect,
            (false, false) => {
                r.w = (pw as f64 * 25.4 / 96.0).min(sw * 0.9);
                r.h = r.w * aspect;
                if r.h > sh * 0.9 {
                    r.h = sh * 0.9;
                    r.w = r.h / aspect;
                }
                r.x = (sw - r.w) / 2.0;
                r.y = (sh - r.h) / 2.0;
            }
            _ => {}
        }
        let part = self.slide_part(i)?;
        let media = self.pkg.next_name("ppt/media/image", &format!(".{ext}"));
        self.pkg.set(&media, bytes.to_vec());
        self.pkg.ensure_default_type(ext, ct)?;
        let rid = self.pkg.add_rel(&part, qu_ooxml::REL_IMAGE, &relative_target(&part, &media))?;
        self.edit_tree(i, |tree, id| {
            let pic = Element::new("p:pic")
                .with_child(
                    Element::new("p:nvPicPr")
                        .with_child(Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &format!("Picture {id}")))
                        .with_child(Element::new("p:cNvPicPr").with_child(Element::new("a:picLocks").with_attr("noChangeAspect", "1")))
                        .with_child(Element::new("p:nvPr")),
                )
                .with_child(Element::new("p:blipFill").with_child(Element::new("a:blip").with_attr("r:embed", &rid)).with_child(Element::new("a:stretch").with_child(Element::new("a:fillRect"))))
                .with_child(Element::new("p:spPr").with_child(xfrm("a:xfrm", r)).with_child(rect_geom()));
            tree.children.push(Node::Elem(pic));
            Ok(())
        })?;
        Ok(())
    }

    /// Add a table at `r` (mm), `rows` of cell texts; `header` styles the
    /// first row as a header. Uses PowerPoint's built-in Medium Style 2.
    pub fn add_table(&mut self, i: usize, rows: &[Vec<String>], r: Rect, header: bool, size_pt: f64) -> Result<(), String> {
        let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return Err("add_table: the table has no cells".into());
        }
        let col_w = mm_to_emu(r.w / ncols as f64);
        let row_h = mm_to_emu(r.h / rows.len() as f64);
        let sz = ((size_pt * 100.0).round() as i64).to_string();
        let mut tbl = Element::new("a:tbl").with_child({
            let mut p = Element::new("a:tblPr").with_attr("bandRow", "1");
            if header {
                p.set_attr("firstRow", "1");
            }
            p.with_child(Element::new("a:tableStyleId").with_text(TABLE_STYLE))
        });
        let mut grid = Element::new("a:tblGrid");
        for _ in 0..ncols {
            grid = grid.with_child(Element::new("a:gridCol").with_attr("w", &col_w.to_string()));
        }
        tbl = tbl.with_child(grid);
        for row in rows {
            let mut tr = Element::new("a:tr").with_attr("h", &row_h.to_string());
            for c in 0..ncols {
                let text = row.get(c).map(String::as_str).unwrap_or("");
                let rpr = Element::new("a:rPr").with_attr("lang", "en-US").with_attr("sz", &sz).with_attr("dirty", "0");
                tr = tr.with_child(
                    Element::new("a:tc")
                        .with_child(Element::new("a:txBody").with_child(Element::new("a:bodyPr")).with_child(Element::new("a:lstStyle")).with_child(paragraph(text, &rpr, None)))
                        .with_child(Element::new("a:tcPr")),
                );
            }
            tbl = tbl.with_child(tr);
        }
        self.edit_tree(i, |tree, id| {
            let gf = Element::new("p:graphicFrame")
                .with_child(
                    Element::new("p:nvGraphicFramePr")
                        .with_child(Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &format!("Table {id}")))
                        .with_child(Element::new("p:cNvGraphicFramePr").with_child(Element::new("a:graphicFrameLocks").with_attr("noGrp", "1")))
                        .with_child(Element::new("p:nvPr")),
                )
                .with_child(xfrm("p:xfrm", r))
                .with_child(Element::new("a:graphic").with_child(Element::new("a:graphicData").with_attr("uri", "http://schemas.openxmlformats.org/drawingml/2006/table").with_child(tbl)));
            tree.children.push(Node::Elem(gf));
            Ok(())
        })?;
        Ok(())
    }

    // ------------------------------------------------------------ document

    pub fn info(&self) -> Result<Vec<(String, String)>, String> {
        let mut out = self.pkg.core_properties();
        let (w, h) = self.slide_size();
        out.push(("slides".into(), self.slide_count().to_string()));
        out.push(("width_mm".into(), format!("{w:.1}")));
        out.push(("height_mm".into(), format!("{h:.1}")));
        out.push(("layouts".into(), self.layouts()?.join(", ")));
        Ok(out)
    }

    pub fn set_properties(&mut self, props: &[(&str, &str)]) -> Result<(), String> {
        self.pkg.set_core_properties(props)
    }

    /// One `## Slide N: title` section per slide, other text as bullets,
    /// notes as a quote.
    pub fn to_markdown(&self) -> Result<String, String> {
        let mut out = Vec::new();
        for i in 0..self.slide_count() {
            let title = self.slide_title(i)?;
            let mut s = match &title {
                Some(t) => format!("## Slide {}: {}", i + 1, t),
                None => format!("## Slide {}", i + 1),
            };
            for line in self.slide_text(i)?.lines() {
                if Some(line) != title.as_deref() {
                    s.push_str(&format!("\n- {line}"));
                }
            }
            let notes = self.notes(i)?;
            if !notes.trim().is_empty() {
                s.push_str(&format!("\n\n> {}", notes.replace('\n', "\n> ")));
            }
            out.push(s);
        }
        Ok(out.join("\n\n") + "\n")
    }
}

impl Default for Presentation {
    fn default() -> Self {
        Self::new()
    }
}

fn doc_lines(root: &Element) -> Vec<String> {
    qu_ooxml::paragraph_paths(root, DRAWING)
        .iter()
        .map(|p| qu_ooxml::paragraph_text(qu_ooxml::at_path(root, p), DRAWING))
        .filter(|t| !t.trim().is_empty())
        .collect()
}

fn placeholder_type(sp: &Element) -> Option<&str> {
    let ph = sp.child("p:nvSpPr")?.child("p:nvPr")?.child("p:ph")?;
    Some(ph.attr("type").unwrap_or("body"))
}

fn sp_tree_root() -> Element {
    Element::new("p:spTree")
        .with_child(
            Element::new("p:nvGrpSpPr")
                .with_child(Element::new("p:cNvPr").with_attr("id", "1").with_attr("name", ""))
                .with_child(Element::new("p:cNvGrpSpPr"))
                .with_child(Element::new("p:nvPr")),
        )
        .with_child(
            Element::new("p:grpSpPr").with_child(
                Element::new("a:xfrm")
                    .with_child(Element::new("a:off").with_attr("x", "0").with_attr("y", "0"))
                    .with_child(Element::new("a:ext").with_attr("cx", "0").with_attr("cy", "0"))
                    .with_child(Element::new("a:chOff").with_attr("x", "0").with_attr("y", "0"))
                    .with_child(Element::new("a:chExt").with_attr("cx", "0").with_attr("cy", "0")),
            ),
        )
}

fn placeholder_sp(id: u64, name: &str, ph: &(Option<String>, Option<String>), lines: &[String]) -> Element {
    let mut phe = Element::new("p:ph");
    if let Some(t) = &ph.0 {
        phe.set_attr("type", t);
    }
    if let Some(i) = &ph.1 {
        phe.set_attr("idx", i);
    }
    let rpr = Element::new("a:rPr").with_attr("lang", "en-US").with_attr("dirty", "0");
    let mut body = Element::new("p:txBody").with_child(Element::new("a:bodyPr")).with_child(Element::new("a:lstStyle"));
    for l in lines {
        body = body.with_child(paragraph(l, &rpr, None));
    }
    Element::new("p:sp")
        .with_child(
            Element::new("p:nvSpPr")
                .with_child(Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &format!("{name} {}", id - 1)))
                .with_child(Element::new("p:cNvSpPr").with_child(Element::new("a:spLocks").with_attr("noGrp", "1")))
                .with_child(Element::new("p:nvPr").with_child(phe)),
        )
        .with_child(Element::new("p:spPr"))
        .with_child(body)
}

fn paragraph(text: &str, rpr: &Element, algn: Option<&str>) -> Element {
    paragraph_ppr(text, rpr, algn.map(|a| Element::new("a:pPr").with_attr("algn", a)))
}

/// A paragraph of one run with the given paragraph properties.
fn paragraph_ppr(text: &str, rpr: &Element, ppr: Option<Element>) -> Element {
    let mut p = Element::new("a:p");
    if let Some(ppr) = ppr {
        p = p.with_child(ppr);
    }
    if text.is_empty() {
        let mut end = rpr.clone();
        end.name = "a:endParaRPr".into();
        return p.with_child(end);
    }
    p.with_child(Element::new("a:r").with_child(rpr.clone()).with_child(Element::new("a:t").with_text(text)))
}

fn run_props(f: &TextFormat) -> Result<Element, String> {
    let mut rpr = Element::new("a:rPr").with_attr("lang", "en-US");
    if let Some(sz) = f.size {
        if !(1.0..=4000.0).contains(&sz) {
            return Err(format!("size={sz} pt is outside what PowerPoint stores (1-4000 pt)"));
        }
        rpr.set_attr("sz", &((sz * 100.0).round() as i64).to_string());
    }
    if f.bold {
        rpr.set_attr("b", "1");
    }
    if f.italic {
        rpr.set_attr("i", "1");
    }
    rpr.set_attr("dirty", "0");
    if let Some(c) = &f.color {
        let h = c.trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("color=\"{c}\" -- use #rrggbb"));
        }
        rpr = rpr.with_child(Element::new("a:solidFill").with_child(Element::new("a:srgbClr").with_attr("val", &h.to_uppercase())));
    }
    if let Some(font) = &f.font {
        rpr = rpr.with_child(Element::new("a:latin").with_attr("typeface", font));
    }
    Ok(rpr)
}

fn align_attr(a: Option<&str>) -> Result<Option<&'static str>, String> {
    Ok(match a {
        None => None,
        Some("left") => Some("l"),
        Some("center") | Some("centre") => Some("ctr"),
        Some("right") => Some("r"),
        Some("justify") | Some("justified") => Some("just"),
        Some(other) => return Err(format!("align=\"{other}\" -- use left, center, right or justify")),
    })
}

fn xfrm(name: &str, r: Rect) -> Element {
    Element::new(name)
        .with_child(Element::new("a:off").with_attr("x", &mm_to_emu(r.x).to_string()).with_attr("y", &mm_to_emu(r.y).to_string()))
        .with_child(Element::new("a:ext").with_attr("cx", &mm_to_emu(r.w).to_string()).with_attr("cy", &mm_to_emu(r.h).to_string()))
}

fn rect_geom() -> Element {
    Element::new("a:prstGeom").with_attr("prst", "rect").with_child(Element::new("a:avLst"))
}

// ------------------------------------------------------------------ template

const TEMPLATE: &[(&str, &str)] = &[
    ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/slideLayouts/slideLayout2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/slideLayouts/slideLayout3.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/slideLayouts/slideLayout4.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/presProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presProps+xml"/><Override PartName="/ppt/viewProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml"/><Override PartName="/ppt/tableStyles.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#),
    ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#),
    ("docProps/app.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>Qu</Application><PresentationFormat>Widescreen</PresentationFormat></Properties>"#),
    ("ppt/presentation.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr></p:defaultTextStyle></p:presentation>"#),
    ("ppt/_rels/presentation.xml.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps" Target="presProps.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps" Target="viewProps.xml"/><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles" Target="tableStyles.xml"/></Relationships>"#),
    ("ppt/presProps.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentationPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#),
    ("ppt/viewProps.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:viewPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>"#),
    ("ppt/tableStyles.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"/>"#),
    ("ppt/slideMasters/slideMaster1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="838200" y="365125"/><a:ext cx="10515600" cy="1325563"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Text Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="838200" y="1825625"/><a:ext cx="10515600" cy="4351338"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:pPr lvl="0"/><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master text styles</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/><p:sldLayoutId id="2147483650" r:id="rId2"/><p:sldLayoutId id="2147483651" r:id="rId3"/><p:sldLayoutId id="2147483652" r:id="rId4"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="0"/></a:spcBef><a:buNone/><a:defRPr sz="4400" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/><a:ea typeface="+mj-ea"/><a:cs typeface="+mj-cs"/></a:defRPr></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr marL="228600" indent="-228600" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPts val="1000"/></a:spcBef><a:buFont typeface="Arial"/><a:buChar char="&#8226;"/><a:defRPr sz="2800" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr><a:lvl2pPr marL="685800" indent="-228600" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPts val="500"/></a:spcBef><a:buFont typeface="Arial"/><a:buChar char="&#8226;"/><a:defRPr sz="2400" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl2pPr></p:bodyStyle><p:otherStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr><a:lvl1pPr marL="0" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:defRPr sz="1800" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"#),
    ("ppt/slideMasters/_rels/slideMaster1.xml.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout3.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout4.xml"/><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#),
    ("ppt/slideLayouts/slideLayout1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="title" preserve="1"><p:cSld name="Title Slide"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="1524000" y="1122363"/><a:ext cx="9144000" cy="2387600"/></a:xfrm></p:spPr><p:txBody><a:bodyPr anchor="b"><a:normAutofit/></a:bodyPr><a:lstStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Subtitle 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="1524000" y="3602038"/><a:ext cx="9144000" cy="1655762"/></a:xfrm></p:spPr><p:txBody><a:bodyPr><a:normAutofit/></a:bodyPr><a:lstStyle><a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master subtitle style</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#),
    ("ppt/slideLayouts/slideLayout2.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="obj" preserve="1"><p:cSld name="Title and Content"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Content Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr lvl="0"/><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master text styles</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#),
    ("ppt/slideLayouts/slideLayout3.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="titleOnly" preserve="1"><p:cSld name="Title Only"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#),
    ("ppt/slideLayouts/slideLayout4.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1"><p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#),
    ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", LAYOUT_RELS),
    ("ppt/slideLayouts/_rels/slideLayout2.xml.rels", LAYOUT_RELS),
    ("ppt/slideLayouts/_rels/slideLayout3.xml.rels", LAYOUT_RELS),
    ("ppt/slideLayouts/_rels/slideLayout4.xml.rels", LAYOUT_RELS),
    ("ppt/theme/theme1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Qu"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:shade val="80000"/></a:schemeClr></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:shade val="90000"/></a:schemeClr></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#),
];

const LAYOUT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(p: &mut Presentation) -> Presentation {
        Presentation::from_bytes(&p.to_bytes().unwrap()).unwrap()
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    #[test]
    fn build_and_rearrange_slides() {
        let mut p = Presentation::new();
        assert_eq!(p.layouts().unwrap(), ["Title Slide", "Title and Content", "Title Only", "Blank"]);
        p.add_slide(Some("Title Slide"), Some("Impedance Spectroscopy"), &["2025 lecture".into()], None).unwrap();
        p.add_slide(None, Some("Results"), &["point one".into(), "point two".into()], None).unwrap();
        p.add_slide(Some("blank"), None, &[], None).unwrap();
        p.add_text(2, "free text\nsecond line", Rect { x: 20.0, y: 15.0, w: 100.0, h: 20.0 }, &TextFormat { size: Some(28.0), bold: true, ..Default::default() }).unwrap();
        p.add_image(2, &png(400, 200), Rect { x: 120.0, y: 40.0, w: 100.0, h: 0.0 }).unwrap();
        p.add_table(1, &[vec!["f".into(), "Z".into()], vec!["1000".into(), "42".into()]], Rect { x: 20.0, y: 100.0, w: 120.0, h: 20.0 }, true, 14.0).unwrap();
        let mut b = roundtrip(&mut p);
        assert_eq!(b.slide_count(), 3);
        assert_eq!(b.slide_title(0).unwrap().as_deref(), Some("Impedance Spectroscopy"));
        assert!(b.slide_text(1).unwrap().contains("point two"));
        assert!(b.slide_text(1).unwrap().contains("42"), "table text is slide text");
        assert!(b.slide_text(2).unwrap().contains("second line"));
        assert_eq!(b.replace_text("2025", "2026", false).unwrap(), 1);
        b.move_slide(2, 0).unwrap();
        assert!(b.slide_text(0).unwrap().contains("free text"));
        assert_eq!(b.duplicate_slide(1).unwrap(), 2);
        assert_eq!(b.slide_count(), 4);
        assert_eq!(b.slide_title(2).unwrap().as_deref(), Some("Impedance Spectroscopy"));
        b.delete_slide(2).unwrap();
        b.set_hidden(0, true).unwrap();
        let c = roundtrip(&mut b);
        assert_eq!(c.slide_count(), 3);
        assert!(c.to_markdown().unwrap().contains("## Slide 2: Impedance Spectroscopy\n- 2026 lecture"));
        assert_eq!(c.find_text("point").unwrap(), vec![2]);
    }

    #[test]
    fn untouched_parts_are_byte_identical() {
        let mut p = Presentation::new();
        p.add_slide(None, Some("x"), &[], None).unwrap();
        let theme_before = p.pkg.get("ppt/theme/theme1.xml").unwrap().to_vec();
        let mut b = roundtrip(&mut p);
        b.replace_text("x", "y", true).unwrap();
        let c = roundtrip(&mut b);
        assert_eq!(c.pkg.get("ppt/theme/theme1.xml").unwrap(), theme_before.as_slice());
    }

    #[test]
    fn bad_indices_and_layouts_are_named_errors() {
        let mut p = Presentation::new();
        assert!(p.slide_text(0).unwrap_err().contains("has 0"));
        assert!(p.add_slide(Some("Nope"), None, &[], None).unwrap_err().contains("Title Slide"));
        p.add_slide(Some("Blank"), None, &[], None).unwrap();
        assert!(p.add_slide(Some("Blank"), Some("t"), &[], None).unwrap_err().contains("no title placeholder"));
    }
}
