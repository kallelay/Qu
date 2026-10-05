//! Shape inventory and editing, speaker-notes creation, hyperlinks on text
//! runs, and the theme's colour and font schemes.
//!
//! The same surgical rule as the rest of the crate: an operation parses and
//! re-serializes only the parts it changes -- the one slide, its `.rels`,
//! `[Content_Types].xml` when a part is added, the theme part for a theme
//! edit -- and every other part is written back byte for byte.
//!
//! Shapes are addressed by their `p:cNvPr id` (unique within a slide and
//! stable across saves -- the thing to hold on to) or by their name. Only
//! the slide's top-level shapes are addressable; a group is one shape.

use crate::{paragraph_ppr, placeholder_sp, placeholder_type, Presentation, Rect, NS_A, NS_P, NS_R, REL_SLIDE};
use qu_ooxml::xml::{Doc, Element, Node};
use qu_ooxml::{mm_to_emu, relative_target, resolve_target, EMU_PER_MM};

const CT_NOTES_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const CT_NOTES_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const REL_NOTES_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const REL_NOTES_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

/// The twelve colour slots of a theme's colour scheme, in schema order.
pub const THEME_SLOTS: [&str; 12] = ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];

const SHAPE_TAGS: &[&str] = &["p:sp", "p:pic", "p:graphicFrame", "p:grpSp", "p:cxnSp", "p:contentPart"];

/// Which shape on a slide: its id (as `shapes` reports it) or its name.
#[derive(Clone, Debug)]
pub enum ShapeRef {
    Id(u64),
    Name(String),
}

/// One top-level shape, as `Presentation::shapes` lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeInfo {
    pub id: u64,
    pub name: String,
    /// `text` (a text box), `placeholder`, `shape`, `picture`, `table`,
    /// `chart`, `graphic`, `group`, `connector` or `other`.
    pub kind: String,
    /// The placeholder type (`title`, `body`, ...) for a placeholder.
    pub placeholder: Option<String>,
    /// Position and size in mm. A placeholder that sets none of its own
    /// reports the one it inherits from its layout (or the master); `None`
    /// when neither says.
    pub rect: Option<Rect>,
    pub text: String,
}

/// Where z-order moves a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Front,
    Back,
}

// ------------------------------------------------------------------ helpers

/// The shape a top-level `p:spTree` child describes: itself, or for an
/// `mc:AlternateContent` the first shape of its first branch.
pub(crate) fn shape_elem(e: &Element) -> Option<&Element> {
    if SHAPE_TAGS.contains(&e.name.as_str()) {
        return Some(e);
    }
    if e.name == "mc:AlternateContent" {
        return e.elems().flat_map(|b| b.elems()).find(|c| SHAPE_TAGS.contains(&c.name.as_str()));
    }
    None
}

/// Every shape element a top-level node stands for: itself, or each
/// branch's shape of an `mc:AlternateContent` (an edit must reach both, or
/// the fallback a reader picks shows the old state).
pub(crate) fn shape_elems_mut(e: &mut Element) -> Vec<&mut Element> {
    if SHAPE_TAGS.contains(&e.name.as_str()) {
        return vec![e];
    }
    if e.name == "mc:AlternateContent" {
        return e.elems_mut().flat_map(|b| b.elems_mut()).filter(|c| SHAPE_TAGS.contains(&c.name.as_str())).collect();
    }
    Vec::new()
}

fn nv(shape: &Element) -> Option<&Element> {
    shape.elems().find(|c| c.name.starts_with("p:nv"))
}

pub(crate) fn shape_id(shape: &Element) -> Option<u64> {
    nv(shape)?.child("p:cNvPr")?.attr("id")?.parse().ok()
}

pub(crate) fn shape_name(shape: &Element) -> String {
    nv(shape).and_then(|n| n.child("p:cNvPr")).and_then(|c| c.attr("name")).unwrap_or("").to_string()
}

/// (type, idx) of a placeholder, type defaulting to `body` as the schema says.
pub(crate) fn ph_key(shape: &Element) -> Option<(String, Option<String>)> {
    let ph = nv(shape)?.child("p:nvPr")?.child("p:ph")?;
    Some((ph.attr("type").unwrap_or("body").to_string(), ph.attr("idx").map(String::from)))
}

pub(crate) fn kind_of(shape: &Element) -> &'static str {
    match shape.name.as_str() {
        "p:sp" if ph_key(shape).is_some() => "placeholder",
        "p:sp" if nv(shape).and_then(|n| n.child("p:cNvSpPr")).and_then(|c| c.attr("txBox")) == Some("1") => "text",
        "p:sp" => "shape",
        "p:pic" => "picture",
        "p:graphicFrame" => {
            let uri = shape.find_all("a:graphicData").first().and_then(|g| g.attr("uri")).unwrap_or("").to_string();
            if uri.ends_with("/table") {
                "table"
            } else if uri.contains("chart") {
                "chart"
            } else {
                "graphic"
            }
        }
        "p:grpSp" => "group",
        "p:cxnSp" => "connector",
        _ => "other",
    }
}

pub(crate) fn xfrm_of(shape: &Element) -> Option<&Element> {
    match shape.name.as_str() {
        "p:graphicFrame" => shape.child("p:xfrm"),
        "p:grpSp" => shape.child("p:grpSpPr")?.child("a:xfrm"),
        _ => shape.child("p:spPr")?.child("a:xfrm"),
    }
}

pub(crate) fn rect_of_xfrm(x: &Element) -> Option<Rect> {
    let num = |e: Option<&Element>, k: &str| -> Option<f64> { e?.attr(k)?.parse::<f64>().ok().map(|v| v / EMU_PER_MM) };
    let (off, ext) = (x.child("a:off"), x.child("a:ext"));
    Some(Rect { x: num(off, "x")?, y: num(off, "y")?, w: num(ext, "cx")?, h: num(ext, "cy")? })
}

fn is_title(t: &str) -> bool {
    t == "title" || t == "ctrTitle"
}

/// The placeholder in `tree` that a slide placeholder `key` inherits from:
/// by idx first, then by type (on a master every non-title text
/// placeholder is its `body`).
fn find_ph<'a>(root: &'a Element, key: &(String, Option<String>), master: bool) -> Option<&'a Element> {
    let sps: Vec<&Element> = root.find_all("p:sp");
    if !master {
        if let Some(idx) = &key.1 {
            if let Some(s) = sps.iter().find(|s| ph_key(s).is_some_and(|k| k.1.as_deref() == Some(idx.as_str()))) {
                return Some(s);
            }
        }
    }
    let want = |t: &str| -> String {
        if is_title(t) {
            "title".into()
        } else if master && !matches!(t, "dt" | "ftr" | "sldNum" | "sldImg" | "hdr") {
            "body".into()
        } else {
            t.to_string()
        }
    };
    let target = want(&key.0);
    sps.into_iter().find(|s| ph_key(s).is_some_and(|k| want(&k.0) == target))
}

/// The paragraphs of `body` (a `p:txBody`/`a:txBody`) replaced by `text`,
/// one paragraph per line, each taking the first paragraph's properties and
/// the first run's formatting -- so a restyled title stays restyled.
fn set_paragraphs(body: &mut Element, text: &str) {
    let first = body.child("a:p").cloned();
    let ppr = first.as_ref().and_then(|p| p.child("a:pPr").cloned());
    let mut rpr = first
        .as_ref()
        .and_then(|p| p.find_all("a:rPr").first().map(|e| (*e).clone()))
        .or_else(|| first.as_ref().and_then(|p| p.child("a:endParaRPr").cloned()).map(|mut e| {
            e.name = "a:rPr".into();
            e
        }))
        .unwrap_or_else(|| Element::new("a:rPr").with_attr("lang", "en-US").with_attr("dirty", "0"));
    // A link on the old first run is not a property of the new text.
    rpr.remove_children("a:hlinkClick");
    rpr.remove_children("a:hlinkMouseOver");
    let pos = body.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "a:p")).unwrap_or(body.children.len());
    body.remove_children("a:p");
    let pos = pos.min(body.children.len());
    for (k, line) in text.split('\n').enumerate() {
        body.children.insert(pos + k, Node::Elem(paragraph_ppr(line, &rpr, ppr.clone())));
    }
}

/// Replace a `p:sp`'s text, creating its text body if it has none.
fn set_shape_body(sp: &mut Element, text: &str) -> Result<(), String> {
    if sp.name != "p:sp" {
        return Err(format!("a {} holds no text of its own -- only text boxes, placeholders and shapes do", kind_of(sp)));
    }
    if sp.child("p:txBody").is_none() {
        let body = Element::new("p:txBody").with_child(Element::new("a:bodyPr").with_attr("rtlCol", "0").with_attr("anchor", "ctr")).with_child(Element::new("a:lstStyle"));
        // Schema order: nvSpPr, spPr, style?, txBody?, extLst?.
        let pos = sp.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:extLst")).unwrap_or(sp.children.len());
        sp.children.insert(pos, Node::Elem(body));
    }
    set_paragraphs(sp.child_mut("p:txBody").unwrap(), text);
    Ok(())
}

/// Every `r:*` attribute value (relationship id) in a subtree.
fn rel_ids(e: &Element, out: &mut Vec<String>) {
    for (k, v) in &e.attrs {
        if k.starts_with("r:") {
            out.push(v.clone());
        }
    }
    for c in e.elems() {
        rel_ids(c, out);
    }
}

/// Put `<a:hlinkClick r:id=rid/>` on a run, replacing any link it had.
fn link_run(run: &mut Element, rid: &str) {
    if run.child("a:rPr").is_none() {
        run.children.insert(0, Node::Elem(Element::new("a:rPr").with_attr("lang", "en-US").with_attr("dirty", "0")));
    }
    let rpr = run.child_mut("a:rPr").unwrap();
    rpr.remove_children("a:hlinkClick");
    // CT_TextCharacterProperties: ... latin, ea, cs, sym, hlinkClick,
    // hlinkMouseOver, rtl, extLst.
    let pos = rpr
        .children
        .iter()
        .position(|n| matches!(n, Node::Elem(e) if matches!(e.name.as_str(), "a:hlinkMouseOver" | "a:rtl" | "a:extLst")))
        .unwrap_or(rpr.children.len());
    rpr.children.insert(pos, Node::Elem(Element::new("a:hlinkClick").with_attr("r:id", rid)));
}

fn run_text(r: &Element) -> String {
    r.child("a:t").map(|t| t.text()).unwrap_or_default()
}

/// Link every occurrence of `needle` among a paragraph's runs, splitting a
/// run where a match starts or ends inside it (both halves keep its
/// formatting). Returns the number of occurrences linked.
fn link_in_paragraph(p: &mut Element, needle: &str, rid: &str) -> usize {
    let runs: Vec<(usize, String)> = p.children.iter().enumerate().filter_map(|(i, n)| match n {
        Node::Elem(e) if e.name == "a:r" => Some((i, run_text(e))),
        _ => None,
    }).collect();
    let full: String = runs.iter().map(|(_, t)| t.as_str()).collect();
    let matches: Vec<(usize, usize)> = full.match_indices(needle).map(|(s, m)| (s, s + m.len())).collect();
    if matches.is_empty() {
        return 0;
    }
    let mut out: Vec<Node> = Vec::with_capacity(p.children.len() + 2 * matches.len());
    let mut run_iter = runs.iter().peekable();
    let mut offset = 0usize;
    for (i, node) in p.children.iter().enumerate() {
        let is_run = run_iter.peek().is_some_and(|(ri, _)| *ri == i);
        if !is_run {
            out.push(node.clone());
            continue;
        }
        let (_, text) = run_iter.next().unwrap();
        let Node::Elem(run) = node else { unreachable!() };
        let (start, end) = (offset, offset + text.len());
        offset = end;
        let mut cuts: Vec<usize> = vec![start, end];
        for &(s, e) in &matches {
            for c in [s, e] {
                if c > start && c < end {
                    cuts.push(c);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mut piece = run.clone();
            if cuts.len() > 2 {
                if let Some(t) = piece.child_mut("a:t") {
                    t.set_text(&full[a..b]);
                }
            }
            if a < b && matches.iter().any(|&(s, e)| a >= s && b <= e) {
                link_run(&mut piece, rid);
            }
            out.push(Node::Elem(piece));
        }
    }
    p.children = out;
    matches.len()
}

pub(crate) fn hex_color(c: &str, what: &str) -> Result<String, String> {
    let h = c.trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{what}=\"{c}\" -- use #rrggbb"));
    }
    Ok(h.to_uppercase())
}

// ------------------------------------------------------------------ impl

impl Presentation {
    /// Parse slide `i`, let `f` edit its shape tree, and write the slide
    /// back only if `f` succeeded. `f` gets the next free shape id.
    pub(crate) fn with_tree<T>(&mut self, i: usize, f: impl FnOnce(&mut Element, u64) -> Result<T, String>) -> Result<(T, String, Doc), String> {
        let part = self.slide_part(i)?;
        let mut d = self.pkg.get_xml(&part)?;
        let next_id = d.root.find_all("p:cNvPr").iter().filter_map(|c| c.attr("id")?.parse::<u64>().ok()).max().unwrap_or(1) + 1;
        let tree = d.root.child_mut("p:cSld").and_then(|c| c.child_mut("p:spTree")).ok_or("the slide has no shape tree")?;
        let out = f(tree, next_id)?;
        self.pkg.set_xml(&part, &d);
        Ok((out, part, d))
    }

    pub(crate) fn slide_doc(&self, i: usize) -> Result<(String, Doc), String> {
        let part = self.slide_part(i)?;
        let d = self.pkg.get_xml(&part)?;
        Ok((part, d))
    }

    /// The part a part's relationship of `ty` points at.
    fn rel_target(&self, part: &str, ty: &str) -> Result<Option<String>, String> {
        Ok(self.pkg.rels(part)?.into_iter().find(|r| r.rel_type.ends_with(ty) && !r.external).map(|r| resolve_target(part, &r.target)))
    }

    /// The rectangle a placeholder inherits from the slide's layout, then
    /// the layout's master.
    pub(crate) fn inherited_rect(&self, slide: &str, key: &(String, Option<String>)) -> Result<Option<Rect>, String> {
        let Some(layout) = self.rel_target(slide, "/slideLayout")? else { return Ok(None) };
        let ld = self.pkg.get_xml(&layout)?;
        if let Some(r) = find_ph(&ld.root, key, false).and_then(xfrm_of).and_then(rect_of_xfrm) {
            return Ok(Some(r));
        }
        let Some(master) = self.rel_target(&layout, "/slideMaster")? else { return Ok(None) };
        let md = self.pkg.get_xml(&master)?;
        Ok(find_ph(&md.root, key, true).and_then(xfrm_of).and_then(rect_of_xfrm))
    }

    fn top_shapes(d: &Doc) -> Vec<&Element> {
        d.root.child("p:cSld").and_then(|c| c.child("p:spTree")).map(|t| t.elems().filter_map(shape_elem).collect()).unwrap_or_default()
    }

    /// The top-level shapes of slide `i`, back to front (the order they are
    /// drawn in).
    pub fn shapes(&self, i: usize) -> Result<Vec<ShapeInfo>, String> {
        let (part, d) = self.slide_doc(i)?;
        let mut out = Vec::new();
        for s in Self::top_shapes(&d) {
            let key = ph_key(s);
            let rect = match xfrm_of(s).and_then(rect_of_xfrm) {
                Some(r) => Some(r),
                None => match &key {
                    Some(k) if s.name == "p:sp" => self.inherited_rect(&part, k)?,
                    _ => None,
                },
            };
            out.push(ShapeInfo {
                id: shape_id(s).unwrap_or(0),
                name: shape_name(s),
                kind: kind_of(s).to_string(),
                placeholder: key.map(|k| k.0),
                rect,
                text: crate::doc_lines(s).join("\n"),
            });
        }
        Ok(out)
    }

    /// Index into the shape tree's children of the shape `r` names.
    pub(crate) fn locate(tree: &Element, r: &ShapeRef) -> Result<usize, String> {
        let all: Vec<(usize, u64, String)> = tree
            .children
            .iter()
            .enumerate()
            .filter_map(|(k, n)| match n {
                Node::Elem(e) => shape_elem(e).map(|s| (k, shape_id(s).unwrap_or(0), shape_name(s))),
                _ => None,
            })
            .collect();
        let hits: Vec<&(usize, u64, String)> = all
            .iter()
            .filter(|(_, id, name)| match r {
                ShapeRef::Id(want) => id == want,
                ShapeRef::Name(want) => name == want,
            })
            .collect();
        match hits.as_slice() {
            [one] => Ok(one.0),
            [] => {
                let have = all.iter().map(|(_, id, n)| format!("{id} `{n}`")).collect::<Vec<_>>().join(", ");
                let what = match r {
                    ShapeRef::Id(id) => format!("id {id}"),
                    ShapeRef::Name(n) => format!("named `{n}`"),
                };
                Err(format!("no shape {what} on this slide -- it has: {}", if have.is_empty() { "no shapes".into() } else { have }))
            }
            many => Err(format!(
                "{} shapes share that name (ids {}) -- use the id",
                many.len(),
                many.iter().map(|h| h.1.to_string()).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Replace the text of a shape (a text box, placeholder or autoshape;
    /// newlines start paragraphs). Formatting of its first run is kept.
    pub fn set_shape_text(&mut self, i: usize, r: &ShapeRef, text: &str) -> Result<(), String> {
        self.with_tree(i, |tree, _| {
            let k = Self::locate(tree, r)?;
            let Node::Elem(node) = &mut tree.children[k] else { unreachable!() };
            for s in shape_elems_mut(node) {
                set_shape_body(s, text)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Set the slide's title: its title placeholder's text, or a new title
    /// placeholder from its layout when the slide has none.
    pub fn set_slide_title(&mut self, i: usize, text: &str) -> Result<(), String> {
        let (part, d) = self.slide_doc(i)?;
        let has = Self::top_shapes(&d).iter().any(|s| s.name == "p:sp" && placeholder_type(s).is_some_and(is_title));
        let layout_ph = if has {
            None
        } else {
            let layout = self.rel_target(&part, "/slideLayout")?.ok_or("the slide has no layout to take a title placeholder from")?;
            let ld = self.pkg.get_xml(&layout)?;
            let ph = ld
                .root
                .find_all("p:ph")
                .into_iter()
                .find(|ph| ph.attr("type").is_some_and(is_title))
                .map(|ph| (ph.attr("type").map(String::from), ph.attr("idx").map(String::from)))
                .ok_or("neither the slide nor its layout has a title placeholder -- add a text box with add_text instead")?;
            Some(ph)
        };
        self.with_tree(i, |tree, next_id| {
            match layout_ph {
                None => {
                    for n in tree.children.iter_mut() {
                        if let Node::Elem(e) = n {
                            if shape_elem(e).is_some_and(|s| s.name == "p:sp" && placeholder_type(s).is_some_and(is_title)) {
                                for s in shape_elems_mut(e) {
                                    set_shape_body(s, text)?;
                                }
                                return Ok(());
                            }
                        }
                    }
                    Ok(())
                }
                Some(ph) => {
                    // Titles go first: drawn at the back, read first by a
                    // screen reader, as PowerPoint places them.
                    let pos = tree.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:grpSpPr")).map(|p| p + 1).unwrap_or(tree.children.len());
                    tree.children.insert(pos, Node::Elem(placeholder_sp(next_id, "Title", &ph, &[text.to_string()])));
                    Ok(())
                }
            }
        })?;
        Ok(())
    }

    /// Delete a shape. A relationship only it used (its image, its links)
    /// goes too, and an image no part refers to any more is removed.
    pub fn delete_shape(&mut self, i: usize, r: &ShapeRef) -> Result<(), String> {
        let (removed, part, d) = self.with_tree(i, |tree, _| {
            let k = Self::locate(tree, r)?;
            Ok(tree.children.remove(k))
        })?;
        let mut gone = Vec::new();
        if let Node::Elem(e) = &removed {
            rel_ids(e, &mut gone);
        }
        let mut still = Vec::new();
        rel_ids(&d.root, &mut still);
        gone.sort();
        gone.dedup();
        let rels = self.pkg.rels(&part)?;
        for rid in gone.iter().filter(|g| !still.contains(g)) {
            let Some(rel) = rels.iter().find(|x| &x.id == rid) else { continue };
            self.pkg.remove_rel(&part, rid)?;
            if !rel.external && rel.rel_type == qu_ooxml::REL_IMAGE {
                let media = resolve_target(&part, &rel.target);
                if !self.part_is_referenced(&media)? {
                    self.pkg.remove(&media);
                    // Media is typed by extension (a Default); touch the
                    // content-type map only if it had its own Override.
                    if self.pkg.override_type(&media).is_some() {
                        self.pkg.remove_override(&media)?;
                    }
                }
            } else if !rel.external && rel.rel_type == qu_ooxml::chart::REL_CHART {
                let chart = resolve_target(&part, &rel.target);
                if !self.part_is_referenced(&chart)? {
                    // The chart and whatever only it used (an embedded
                    // workbook, chart style and colour parts).
                    let owned: Vec<String> = self.pkg.rels(&chart)?.into_iter().filter(|r| !r.external).map(|r| resolve_target(&chart, &r.target)).collect();
                    self.remove_part(&chart)?;
                    for o in owned {
                        if self.pkg.has(&o) && !self.part_is_referenced(&o)? {
                            self.remove_part(&o)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether any relationship in the package still targets `part`.
    fn part_is_referenced(&self, part: &str) -> Result<bool, String> {
        for rp in self.pkg.names().into_iter().filter(|n| n.ends_with(".rels")) {
            // `a/_rels/b.xml.rels` belongs to `a/b.xml`.
            let owner = match rp.rsplit_once("_rels/") {
                Some((dir, file)) => format!("{dir}{}", file.trim_end_matches(".rels")),
                None => continue,
            };
            let src =if rp == "_rels/.rels" { String::new() } else { owner };
            if self.pkg.rels(&src)?.iter().any(|r| !r.external && resolve_target(&src, &r.target) == part) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Move and/or resize a shape (mm; `None` keeps that coordinate). A
    /// placeholder positioned by its layout gets a position of its own,
    /// starting from the inherited one.
    pub fn place_shape(&mut self, i: usize, r: &ShapeRef, x: Option<f64>, y: Option<f64>, w: Option<f64>, h: Option<f64>) -> Result<(), String> {
        for (k, v) in [("w", w), ("h", h)] {
            if v.is_some_and(|v| !(v >= 0.0)) {
                return Err(format!("{k}= must be a length of zero or more"));
            }
        }
        // The inherited rectangle, needed only when the shape has none.
        let (part, d) = self.slide_doc(i)?;
        let tree = d.root.child("p:cSld").and_then(|c| c.child("p:spTree")).ok_or("the slide has no shape tree")?;
        let k = Self::locate(tree, r)?;
        let Node::Elem(node) = &tree.children[k] else { unreachable!() };
        let s = shape_elem(node).unwrap();
        let base = match xfrm_of(s).and_then(rect_of_xfrm) {
            Some(r) => Some(r),
            None => match ph_key(s) {
                Some(key) if s.name == "p:sp" => self.inherited_rect(&part, &key)?,
                _ => None,
            },
        };
        let new = match base {
            Some(b) => Rect { x: x.unwrap_or(b.x), y: y.unwrap_or(b.y), w: w.unwrap_or(b.w), h: h.unwrap_or(b.h) },
            None => match (x, y, w, h) {
                (Some(x), Some(y), Some(w), Some(h)) => Rect { x, y, w, h },
                _ => return Err("this shape has no position of its own or from its layout -- give all of x=, y=, w=, h=".into()),
            },
        };
        self.with_tree(i, |tree, _| {
            let Node::Elem(node) = &mut tree.children[k] else { unreachable!() };
            for s in shape_elems_mut(node) {
                set_xfrm(s, new)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Bring a shape to the front of the slide or send it to the back.
    pub fn order_shape(&mut self, i: usize, r: &ShapeRef, to: Order) -> Result<(), String> {
        self.with_tree(i, |tree, _| {
            let k = Self::locate(tree, r)?;
            let node = tree.children.remove(k);
            let pos = match to {
                // Before a trailing p:extLst, which must stay last.
                Order::Front => tree.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:extLst")).unwrap_or(tree.children.len()),
                Order::Back => tree.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:grpSpPr")).map(|p| p + 1).unwrap_or(0),
            };
            tree.children.insert(pos, node);
            Ok(())
        })?;
        Ok(())
    }

    /// Make text in a shape a hyperlink to `url`: every occurrence of
    /// `text` (split out of its run if it is only part of one), or with
    /// `None` all of the shape's text. Returns how many were linked
    /// (occurrences, or 1 for the whole shape).
    pub fn set_link(&mut self, i: usize, r: &ShapeRef, url: &str, text: Option<&str>) -> Result<usize, String> {
        if url.trim().is_empty() {
            return Err("the link target is empty".into());
        }
        if text == Some("") {
            return Err("text= is empty -- leave it out to link the whole shape".into());
        }
        let part = self.slide_part(i)?;
        let rid = self.pkg.add_external_rel(&part, qu_ooxml::REL_HYPERLINK, url)?;
        let res = self.with_tree(i, |tree, _| {
            let k = Self::locate(tree, r)?;
            let Node::Elem(node) = &mut tree.children[k] else { unreachable!() };
            let mut count = 0;
            for s in shape_elems_mut(node) {
                let mut here = 0;
                let Some(body) = s.child_mut("p:txBody") else {
                    return Err(format!("a {} holds no text to link", kind_of(s)));
                };
                for p in body.elems_mut().filter(|e| e.name == "a:p") {
                    match text {
                        Some(t) => here += link_in_paragraph(p, t, &rid),
                        None => {
                            for run in p.elems_mut().filter(|e| e.name == "a:r") {
                                link_run(run, &rid);
                                here = 1;
                            }
                        }
                    }
                }
                count = count.max(here);
            }
            if count == 0 {
                return Err(match text {
                    Some(t) => format!("`{t}` does not occur in that shape's text"),
                    None => "that shape has no text to link".into(),
                });
            }
            Ok(count)
        });
        match res {
            Ok((n, _, _)) => {
                self.ensure_r_namespace(&part)?;
                Ok(n)
            }
            Err(e) => {
                self.pkg.remove_rel(&part, &rid)?;
                Err(e)
            }
        }
    }

    fn ensure_r_namespace(&mut self, part: &str) -> Result<(), String> {
        let mut d = self.pkg.get_xml(part)?;
        if d.root.attr("xmlns:r").is_none() {
            d.root.set_attr("xmlns:r", NS_R);
            self.pkg.set_xml(part, &d);
        }
        Ok(())
    }

    /// Hyperlinked text on slide `i`: (shape id, linked text, target), one
    /// entry per run of consecutive runs sharing a link.
    pub fn links(&self, i: usize) -> Result<Vec<(u64, String, String)>, String> {
        let (part, d) = self.slide_doc(i)?;
        let rels = self.pkg.rels(&part)?;
        let mut out = Vec::new();
        for s in Self::top_shapes(&d) {
            let id = shape_id(s).unwrap_or(0);
            for p in s.find_all("a:p") {
                let mut cur: Option<(String, String)> = None;
                for run in p.elems() {
                    let rid = (run.name == "a:r").then(|| run.child("a:rPr").and_then(|r| r.child("a:hlinkClick")).and_then(|h| h.attr("r:id"))).flatten();
                    match (rid, &mut cur) {
                        (Some(rid), Some((cr, txt))) if cr == rid => txt.push_str(&run_text(run)),
                        (Some(rid), _) => {
                            if let Some((cr, txt)) = cur.take() {
                                out.push((id, txt, rel_url(&rels, &cr)));
                            }
                            cur = Some((rid.to_string(), run_text(run)));
                        }
                        (None, _) => {
                            if let Some((cr, txt)) = cur.take() {
                                out.push((id, txt, rel_url(&rels, &cr)));
                            }
                        }
                    }
                }
                if let Some((cr, txt)) = cur.take() {
                    out.push((id, txt, rel_url(&rels, &cr)));
                }
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ notes

    /// Set the speaker notes of slide `i` (newlines start paragraphs). A
    /// slide without a notes page gets one, built on the deck's notes
    /// master -- which is created too if the deck has none.
    pub fn set_notes(&mut self, i: usize, text: &str) -> Result<(), String> {
        let slide = self.slide_part(i)?;
        if let Some(np) = self.notes_part(&slide)? {
            let mut d = self.pkg.get_xml(&np)?;
            let next_id = d.root.find_all("p:cNvPr").iter().filter_map(|c| c.attr("id")?.parse::<u64>().ok()).max().unwrap_or(1) + 1;
            let tree = d.root.child_mut("p:cSld").and_then(|c| c.child_mut("p:spTree")).ok_or("the notes page has no shape tree")?;
            let k = tree.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:sp" && placeholder_type(e) == Some("body")));
            match k {
                Some(k) => {
                    let Node::Elem(sp) = &mut tree.children[k] else { unreachable!() };
                    set_shape_body(sp, text)?;
                }
                None => tree.children.push(Node::Elem(notes_body_sp(next_id, text))),
            }
            self.pkg.set_xml(&np, &d);
            return Ok(());
        }
        let master = self.ensure_notes_master()?;
        let np = self.pkg.next_name("ppt/notesSlides/notesSlide", ".xml");
        self.pkg.set_xml(&np, &Doc::new(notes_slide_root(text)));
        self.pkg.add_rel(&np, REL_NOTES_MASTER, &relative_target(&np, &master))?;
        self.pkg.add_rel(&np, REL_SLIDE, &relative_target(&np, &slide))?;
        self.pkg.add_rel(&slide, REL_NOTES_SLIDE, &relative_target(&slide, &np))?;
        self.pkg.add_override(&np, CT_NOTES_SLIDE)
    }

    /// The deck's notes master, creating a minimal one (with its own copy
    /// of the theme, which a notes master must have) when there is none.
    fn ensure_notes_master(&mut self) -> Result<String, String> {
        if let Some(m) = self.rel_target(&self.main.clone(), "/notesMaster")? {
            return Ok(m);
        }
        let theme_src = self.theme_part()?;
        let theme = self.pkg.next_name("ppt/theme/theme", ".xml");
        let bytes = self.pkg.get(&theme_src).ok_or_else(|| format!("the theme part `{theme_src}` is missing"))?.to_vec();
        self.pkg.set(&theme, bytes);
        self.pkg.add_override(&theme, CT_THEME)?;
        let nm = self.pkg.next_name("ppt/notesMasters/notesMaster", ".xml");
        self.pkg.set_xml(&nm, &Doc::new(notes_master_root()));
        self.pkg.add_rel(&nm, REL_THEME, &relative_target(&nm, &theme))?;
        self.pkg.add_override(&nm, CT_NOTES_MASTER)?;
        let main = self.main.clone();
        let rid = self.pkg.add_rel(&main, REL_NOTES_MASTER, &relative_target(&main, &nm))?;
        let root = &mut self.pres.root;
        root.remove_children("p:notesMasterIdLst");
        // Schema order: sldMasterIdLst, notesMasterIdLst, handoutMasterIdLst, ...
        let pos = root.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:sldMasterIdLst")).map(|p| p + 1).unwrap_or(0);
        root.children.insert(pos, Node::Elem(Element::new("p:notesMasterIdLst").with_child(Element::new("p:notesMasterId").with_attr("r:id", &rid))));
        if root.child("p:notesSz").is_none() {
            let pos = root.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "p:sldSz")).map(|p| p + 1).unwrap_or(root.children.len());
            root.children.insert(pos, Node::Elem(Element::new("p:notesSz").with_attr("cx", "6858000").with_attr("cy", "9144000")));
        }
        Ok(nm)
    }

    // ------------------------------------------------------------ theme

    /// The theme the slides use: the first slide master's, else the one
    /// the presentation part names.
    fn theme_part(&self) -> Result<String, String> {
        let main = self.main.clone();
        if let Some(master) = self.rel_target(&main, "/slideMaster")? {
            if let Some(t) = self.rel_target(&master, "/theme")? {
                return Ok(t);
            }
        }
        self.rel_target(&main, "/theme")?.ok_or_else(|| "the presentation has no theme".into())
    }

    /// The theme's colour scheme: (slot, `#RRGGBB`) for the twelve slots,
    /// a system colour reported as the value it was last saved with.
    pub fn theme_colors(&self) -> Result<Vec<(String, String)>, String> {
        let d = self.pkg.get_xml(&self.theme_part()?)?;
        let cs = d.root.child("a:themeElements").and_then(|t| t.child("a:clrScheme")).ok_or("the theme has no colour scheme")?;
        let mut out = Vec::new();
        for slot in THEME_SLOTS {
            let v = cs.child(&format!("a:{slot}")).and_then(|s| s.elems().next()).and_then(|c| match c.name.as_str() {
                "a:srgbClr" => c.attr("val"),
                "a:sysClr" => c.attr("lastClr"),
                _ => None,
            });
            out.push((slot.to_string(), v.map(|v| format!("#{}", v.to_uppercase())).unwrap_or_default()));
        }
        Ok(out)
    }

    /// The theme's (heading, body) Latin typefaces.
    pub fn theme_fonts(&self) -> Result<(String, String), String> {
        let d = self.pkg.get_xml(&self.theme_part()?)?;
        let fs = d.root.child("a:themeElements").and_then(|t| t.child("a:fontScheme")).ok_or("the theme has no font scheme")?;
        let get = |k: &str| fs.child(k).and_then(|f| f.child("a:latin")).and_then(|l| l.attr("typeface")).unwrap_or("").to_string();
        Ok((get("a:majorFont"), get("a:minorFont")))
    }

    /// Set colour-scheme slots (`accent1` = `#RRGGBB`, ...). Only the
    /// theme part is rewritten.
    pub fn set_theme_colors(&mut self, colors: &[(&str, &str)]) -> Result<(), String> {
        let mut vals = Vec::new();
        for (slot, c) in colors {
            if !THEME_SLOTS.contains(slot) {
                return Err(format!("`{slot}` is not a theme colour -- use {}", THEME_SLOTS.join(", ")));
            }
            vals.push((*slot, hex_color(c, slot)?));
        }
        let part = self.theme_part()?;
        let mut d = self.pkg.get_xml(&part)?;
        let cs = d.root.child_mut("a:themeElements").and_then(|t| t.child_mut("a:clrScheme")).ok_or("the theme has no colour scheme")?;
        for (slot, hex) in vals {
            let e = cs.child_mut(&format!("a:{slot}")).ok_or_else(|| format!("the colour scheme has no `{slot}` slot"))?;
            e.children = vec![Node::Elem(Element::new("a:srgbClr").with_attr("val", &hex))];
        }
        self.pkg.set_xml(&part, &d);
        Ok(())
    }

    /// Set the heading (`major`) and/or body (`minor`) Latin typeface.
    pub fn set_theme_fonts(&mut self, major: Option<&str>, minor: Option<&str>) -> Result<(), String> {
        let part = self.theme_part()?;
        let mut d = self.pkg.get_xml(&part)?;
        let fs = d.root.child_mut("a:themeElements").and_then(|t| t.child_mut("a:fontScheme")).ok_or("the theme has no font scheme")?;
        for (k, v) in [("a:majorFont", major), ("a:minorFont", minor)] {
            let Some(v) = v else { continue };
            if v.trim().is_empty() {
                return Err("a typeface name cannot be empty".into());
            }
            let f = fs.child_mut(k).ok_or_else(|| format!("the font scheme has no `{k}`"))?;
            f.ensure_first_child("a:latin").set_attr("typeface", v);
        }
        self.pkg.set_xml(&part, &d);
        Ok(())
    }
}

fn rel_url(rels: &[qu_ooxml::Rel], rid: &str) -> String {
    rels.iter().find(|r| r.id == rid).map(|r| r.target.clone()).unwrap_or_default()
}

pub(crate) fn set_xfrm(s: &mut Element, r: Rect) -> Result<(), String> {
    let x = match s.name.as_str() {
        "p:graphicFrame" => {
            if s.child("p:xfrm").is_none() {
                let pos = s.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name.starts_with("p:nv"))).map(|p| p + 1).unwrap_or(0);
                s.children.insert(pos, Node::Elem(Element::new("p:xfrm")));
            }
            s.child_mut("p:xfrm").unwrap()
        }
        "p:grpSp" => s.child_mut("p:grpSpPr").ok_or("the group has no properties")?.ensure_first_child("a:xfrm"),
        _ => {
            if s.child("p:spPr").is_none() {
                let pos = s.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name.starts_with("p:nv"))).map(|p| p + 1).unwrap_or(0);
                s.children.insert(pos, Node::Elem(Element::new("p:spPr")));
            }
            s.child_mut("p:spPr").unwrap().ensure_first_child("a:xfrm")
        }
    };
    x.ensure_first_child("a:off");
    let off = x.child_mut("a:off").unwrap();
    off.set_attr("x", &mm_to_emu(r.x).to_string());
    off.set_attr("y", &mm_to_emu(r.y).to_string());
    if x.child("a:ext").is_none() {
        let pos = x.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "a:off")).map(|p| p + 1).unwrap_or(0);
        x.children.insert(pos, Node::Elem(Element::new("a:ext")));
    }
    let ext = x.child_mut("a:ext").unwrap();
    ext.set_attr("cx", &mm_to_emu(r.w).to_string());
    ext.set_attr("cy", &mm_to_emu(r.h).to_string());
    Ok(())
}

fn notes_body_sp(id: u64, text: &str) -> Element {
    let mut sp = placeholder_sp(id, "Notes Placeholder", &(Some("body".into()), Some("1".into())), &[]);
    set_paragraphs(sp.child_mut("p:txBody").unwrap(), text);
    sp
}

fn notes_slide_root(text: &str) -> Element {
    let img = Element::new("p:sp")
        .with_child(
            Element::new("p:nvSpPr")
                .with_child(Element::new("p:cNvPr").with_attr("id", "2").with_attr("name", "Slide Image Placeholder 1"))
                .with_child(Element::new("p:cNvSpPr").with_child(Element::new("a:spLocks").with_attr("noGrp", "1").with_attr("noRot", "1").with_attr("noChangeAspect", "1")))
                .with_child(Element::new("p:nvPr").with_child(Element::new("p:ph").with_attr("type", "sldImg"))),
        )
        .with_child(Element::new("p:spPr"));
    let tree = crate::sp_tree_root().with_child(img).with_child(notes_body_sp(3, text));
    Element::new("p:notes")
        .with_attr("xmlns:a", NS_A)
        .with_attr("xmlns:r", NS_R)
        .with_attr("xmlns:p", NS_P)
        .with_child(Element::new("p:cSld").with_child(tree))
        .with_child(Element::new("p:clrMapOvr").with_child(Element::new("a:masterClrMapping")))
}

fn notes_master_root() -> Element {
    let xfrm = |x: i64, y: i64, cx: i64, cy: i64| {
        Element::new("p:spPr")
            .with_child(
                Element::new("a:xfrm")
                    .with_child(Element::new("a:off").with_attr("x", &x.to_string()).with_attr("y", &y.to_string()))
                    .with_child(Element::new("a:ext").with_attr("cx", &cx.to_string()).with_attr("cy", &cy.to_string())),
            )
            .with_child(Element::new("a:prstGeom").with_attr("prst", "rect").with_child(Element::new("a:avLst")))
    };
    let nv = |id: &str, name: &str, ph: Element| {
        Element::new("p:nvSpPr")
            .with_child(Element::new("p:cNvPr").with_attr("id", id).with_attr("name", name))
            .with_child(Element::new("p:cNvSpPr").with_child(Element::new("a:spLocks").with_attr("noGrp", "1")))
            .with_child(Element::new("p:nvPr").with_child(ph))
    };
    let img = Element::new("p:sp")
        .with_child(nv("2", "Slide Image Placeholder 1", Element::new("p:ph").with_attr("type", "sldImg").with_attr("idx", "2")))
        .with_child(xfrm(685800, 1143000, 5486400, 3086100).with_child(Element::new("a:noFill")));
    let body = Element::new("p:sp")
        .with_child(nv("3", "Notes Placeholder 2", Element::new("p:ph").with_attr("type", "body").with_attr("idx", "1")))
        .with_child(xfrm(685800, 4400550, 5486400, 3600450))
        .with_child(
            Element::new("p:txBody")
                .with_child(Element::new("a:bodyPr").with_attr("vert", "horz").with_attr("lIns", "91440").with_attr("tIns", "45720").with_attr("rIns", "91440").with_attr("bIns", "45720").with_attr("rtlCol", "0"))
                .with_child(Element::new("a:lstStyle"))
                .with_child(Element::new("a:p").with_child(Element::new("a:pPr").with_attr("lvl", "0")).with_child(Element::new("a:r").with_child(Element::new("a:rPr").with_attr("lang", "en-US")).with_child(Element::new("a:t").with_text("Click to edit Master text styles")))),
        );
    let lvl1 = Element::new("a:lvl1pPr").with_attr("marL", "0").with_attr("algn", "l").with_attr("defTabSz", "914400").with_attr("rtl", "0").with_attr("eaLnBrk", "1").with_attr("latinLnBrk", "0").with_attr("hangingPunct", "1").with_child(
        Element::new("a:defRPr")
            .with_attr("sz", "1200")
            .with_attr("kern", "1200")
            .with_child(Element::new("a:solidFill").with_child(Element::new("a:schemeClr").with_attr("val", "tx1")))
            .with_child(Element::new("a:latin").with_attr("typeface", "+mn-lt"))
            .with_child(Element::new("a:ea").with_attr("typeface", "+mn-ea"))
            .with_child(Element::new("a:cs").with_attr("typeface", "+mn-cs")),
    );
    let clr_map = [("bg1", "lt1"), ("tx1", "dk1"), ("bg2", "lt2"), ("tx2", "dk2"), ("accent1", "accent1"), ("accent2", "accent2"), ("accent3", "accent3"), ("accent4", "accent4"), ("accent5", "accent5"), ("accent6", "accent6"), ("hlink", "hlink"), ("folHlink", "folHlink")]
        .iter()
        .fold(Element::new("p:clrMap"), |e, (k, v)| e.with_attr(k, v));
    Element::new("p:notesMaster")
        .with_attr("xmlns:a", NS_A)
        .with_attr("xmlns:r", NS_R)
        .with_attr("xmlns:p", NS_P)
        .with_child(
            Element::new("p:cSld")
                .with_child(Element::new("p:bg").with_child(Element::new("p:bgRef").with_attr("idx", "1001").with_child(Element::new("a:schemeClr").with_attr("val", "bg1"))))
                .with_child(crate::sp_tree_root().with_child(img).with_child(body)),
        )
        .with_child(clr_map)
        .with_child(Element::new("p:notesStyle").with_child(lvl1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextFormat;

    const CUSTOM: &str = "customXml/item1.xml";
    const CUSTOM_BYTES: &[u8] = b"<?xml version=\"1.0\"?><root xmlns=\"urn:vendor\"><keep>me &amp; mine</keep></root>";

    fn reopen(p: &mut Presentation) -> Presentation {
        Presentation::from_bytes(&p.to_bytes().unwrap()).unwrap()
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    /// Three slides, none with notes, the deck with no notes master, plus
    /// an unknown customXml part -- reopened, so it is "someone's file".
    fn deck() -> Presentation {
        let mut p = Presentation::new();
        p.add_slide(None, Some("Results"), &["point one".into(), "point two".into()], None).unwrap();
        p.add_text(0, "see the Nyquist plot here", Rect { x: 20.0, y: 150.0, w: 100.0, h: 20.0 }, &TextFormat { size: Some(28.0), bold: true, ..Default::default() }).unwrap();
        p.add_image(0, &png(400, 200), Rect { x: 200.0, y: 60.0, w: 80.0, h: 0.0 }).unwrap();
        p.add_slide(Some("Blank"), None, &[], None).unwrap();
        p.add_slide(Some("Title Only"), None, &[], None).unwrap();
        p.pkg.set(CUSTOM, CUSTOM_BYTES.to_vec());
        reopen(&mut p)
    }

    fn part(p: &Presentation, name: &str) -> Vec<u8> {
        p.pkg.get(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()
    }

    fn snapshot(p: &Presentation) -> Vec<(String, Vec<u8>)> {
        p.pkg.names().into_iter().map(|n| (n.clone(), part(p, &n))).collect()
    }

    /// Parts whose bytes differ (or appeared/disappeared) between snapshots.
    fn changed(a: &[(String, Vec<u8>)], b: &[(String, Vec<u8>)]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (n, bytes) in b {
            if a.iter().find(|(m, _)| m == n).map(|(_, x)| x) != Some(bytes) {
                out.push(n.clone());
            }
        }
        for (n, _) in a {
            if !b.iter().any(|(m, _)| m == n) {
                out.push(format!("-{n}"));
            }
        }
        out.sort();
        out
    }

    fn id_of(p: &Presentation, i: usize, kind: &str) -> u64 {
        p.shapes(i).unwrap().into_iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind}")).id
    }

    #[test]
    fn notes_are_created_on_a_deck_that_has_no_notes_master() {
        let mut p = deck();
        assert!(p.rel_target("ppt/presentation.xml", "/notesMaster").unwrap().is_none(), "the fixture must start with no notes master");
        assert_eq!(p.notes(1).unwrap(), "");
        let before = snapshot(&p);
        p.set_notes(1, "Say the thing.\nThen the other thing.").unwrap();
        let mut q = reopen(&mut p);
        let after = snapshot(&q);
        assert_eq!(q.notes(1).unwrap(), "Say the thing.\nThen the other thing.");
        assert_eq!(q.notes(0).unwrap(), "");
        // Exactly: the new notes page (+rels), the new notes master (+rels)
        // and its theme copy, slide 2's rels, the content-type map and the
        // presentation part (+rels). No slide XML and nothing else.
        assert_eq!(
            changed(&before, &after),
            [
                "[Content_Types].xml",
                "ppt/_rels/presentation.xml.rels",
                "ppt/notesMasters/_rels/notesMaster1.xml.rels",
                "ppt/notesMasters/notesMaster1.xml",
                "ppt/notesSlides/_rels/notesSlide1.xml.rels",
                "ppt/notesSlides/notesSlide1.xml",
                "ppt/presentation.xml",
                "ppt/slides/_rels/slide2.xml.rels",
                "ppt/theme/theme2.xml",
            ]
        );
        assert_eq!(part(&q, CUSTOM), CUSTOM_BYTES);
        assert_eq!(part(&q, "ppt/theme/theme2.xml"), part(&q, "ppt/theme/theme1.xml"), "the notes master's theme is a copy");
        let pres = String::from_utf8(part(&q, "ppt/presentation.xml")).unwrap();
        let (a, b, c) = (pres.find("<p:sldMasterIdLst").unwrap(), pres.find("<p:notesMasterIdLst").unwrap(), pres.find("<p:sldIdLst").unwrap());
        assert!(a < b && b < c, "notesMasterIdLst sits between the master and slide lists: {pres}");
        let ct = String::from_utf8(part(&q, "[Content_Types].xml")).unwrap();
        assert!(ct.contains("/ppt/notesSlides/notesSlide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"));
        assert!(ct.contains("/ppt/notesMasters/notesMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"));

        // A second slide reuses the master; editing existing notes rewrites
        // only that notes page.
        q.set_notes(0, "first").unwrap();
        let mut r = reopen(&mut q);
        assert_eq!(r.pkg.names().iter().filter(|n| n.starts_with("ppt/notesMasters/notesMaster") && n.ends_with(".xml")).count(), 1);
        assert_eq!(r.notes(0).unwrap(), "first");
        let before = snapshot(&r);
        r.set_notes(1, "rewritten").unwrap();
        let s = reopen(&mut r);
        assert_eq!(changed(&before, &snapshot(&s)), ["ppt/notesSlides/notesSlide1.xml"]);
        assert_eq!(s.notes(1).unwrap(), "rewritten");
        assert_eq!(s.notes(0).unwrap(), "first");
        assert!(s.to_markdown().unwrap().contains("> rewritten"));
        // delete_slide takes the created notes page with it.
        let mut t = s;
        t.delete_slide(1).unwrap();
        let t = reopen(&mut t);
        assert!(!t.pkg.names().iter().any(|n| n == "ppt/notesSlides/notesSlide1.xml"));
        assert_eq!(t.notes(0).unwrap(), "first");
    }

    #[test]
    fn shape_inventory_reports_kind_position_and_inherited_geometry() {
        let mut p = deck();
        let s = p.shapes(0).unwrap();
        let kinds: Vec<&str> = s.iter().map(|x| x.kind.as_str()).collect();
        assert_eq!(kinds, ["placeholder", "placeholder", "text", "picture"]);
        assert_eq!(s[0].placeholder.as_deref(), Some("title"));
        assert_eq!(s[0].text, "Results");
        assert_eq!(s[1].text, "point one\npoint two");
        // The title sets no geometry: it inherits the master's title box,
        // 838200,365125 EMU at 10515600 x 1325563.
        let t = s[0].rect.unwrap();
        assert_eq!((t.x, t.y, t.w, t.h), (838200.0 / 36000.0, 365125.0 / 36000.0, 10515600.0 / 36000.0, 1325563.0 / 36000.0));
        assert_eq!(s[2].rect, Some(Rect { x: 20.0, y: 150.0, w: 100.0, h: 20.0 }));
        assert_eq!(s[2].text, "see the Nyquist plot here");
        assert_eq!(s[3].rect, Some(Rect { x: 200.0, y: 60.0, w: 80.0, h: 40.0 }), "height from the 2:1 aspect");
        assert!(p.shapes(1).unwrap().is_empty());
        let msg = p.set_shape_text(0, &ShapeRef::Id(999), "x").unwrap_err();
        assert!(msg.contains("no shape id 999 on this slide -- it has: 2 `Title 1`"), "{msg}");
    }

    #[test]
    fn set_text_keeps_formatting_and_touches_only_that_slide() {
        let mut p = deck();
        let before = snapshot(&p);
        let tb = id_of(&p, 0, "text");
        p.set_shape_text(0, &ShapeRef::Id(tb), "first\nsecond").unwrap();
        let mut q = reopen(&mut p);
        assert_eq!(changed(&before, &snapshot(&q)), ["ppt/slides/slide1.xml"]);
        let s = q.shapes(0).unwrap();
        assert_eq!(s[2].text, "first\nsecond");
        let d = q.pkg.get_xml("ppt/slides/slide1.xml").unwrap();
        let rprs: Vec<_> = d.root.find_all("p:sp")[2].find_all("a:rPr").into_iter().map(|r| (r.attr("sz").map(String::from), r.attr("b").map(String::from))).collect();
        assert_eq!(rprs, vec![(Some("2800".to_string()), Some("1".to_string())); 2], "both paragraphs keep 28 pt bold");
        // By name too; a picture has no text.
        q.set_shape_text(0, &ShapeRef::Name("Title 1".into()), "Renamed").unwrap();
        assert_eq!(q.slide_title(0).unwrap().as_deref(), Some("Renamed"));
        let pic = id_of(&q, 0, "picture");
        assert!(q.set_shape_text(0, &ShapeRef::Id(pic), "x").unwrap_err().contains("picture holds no text"));
    }

    #[test]
    fn move_resize_and_z_order() {
        let mut p = deck();
        let (title, tb) = (p.shapes(0).unwrap()[0].id, id_of(&p, 0, "text"));
        // Moving an inheriting placeholder materialises the rest of its box.
        p.place_shape(0, &ShapeRef::Id(title), Some(10.0), None, None, None).unwrap();
        p.place_shape(0, &ShapeRef::Id(tb), None, None, Some(50.0), Some(12.5)).unwrap();
        let mut q = reopen(&mut p);
        let s = q.shapes(0).unwrap();
        let emu = |v: f64| (v / 36000.0 * 36000.0).round() / 36000.0;
        assert_eq!(s[0].rect, Some(Rect { x: 10.0, y: emu(365125.0), w: emu(10515600.0), h: emu(1325563.0) }));
        assert_eq!(s[2].rect, Some(Rect { x: 20.0, y: 150.0, w: 50.0, h: 12.5 }));
        q.order_shape(0, &ShapeRef::Id(title), Order::Front).unwrap();
        assert_eq!(q.shapes(0).unwrap().last().unwrap().id, title);
        q.order_shape(0, &ShapeRef::Id(tb), Order::Back).unwrap();
        let ids: Vec<u64> = q.shapes(0).unwrap().iter().map(|s| s.id).collect();
        assert_eq!(ids[0], tb);
        assert_eq!(ids.len(), 4);
        // The group header stays in front of every shape.
        let d = q.pkg.get_xml("ppt/slides/slide1.xml").unwrap();
        let names: Vec<String> = d.root.child("p:cSld").unwrap().child("p:spTree").unwrap().elems().map(|e| e.name.clone()).collect();
        assert_eq!(&names[..3], ["p:nvGrpSpPr", "p:grpSpPr", "p:sp"]);
        assert!(q.place_shape(0, &ShapeRef::Id(tb), None, None, Some(-1.0), None).unwrap_err().contains("w="));
    }

    #[test]
    fn delete_shape_drops_its_image_only_when_nothing_else_uses_it() {
        let mut p = deck();
        let copy = p.duplicate_slide(0).unwrap();
        let media = "ppt/media/image1.png";
        let pic = id_of(&p, 0, "picture");
        p.delete_shape(0, &ShapeRef::Id(pic)).unwrap();
        let mut q = reopen(&mut p);
        assert!(q.pkg.has(media), "the duplicate still shows it");
        assert!(!q.pkg.rels("ppt/slides/slide1.xml").unwrap().iter().any(|r| r.rel_type == qu_ooxml::REL_IMAGE));
        assert_eq!(q.shapes(0).unwrap().len(), 3);
        let pic2 = id_of(&q, copy, "picture");
        q.delete_shape(copy, &ShapeRef::Id(pic2)).unwrap();
        let q = reopen(&mut q);
        assert!(!q.pkg.has(media), "now orphaned, so removed");
        assert_eq!(part(&q, CUSTOM), CUSTOM_BYTES);
    }

    #[test]
    fn slide_title_is_set_or_created_from_the_layout() {
        let mut p = deck();
        p.set_slide_title(0, "New results").unwrap();
        assert_eq!(p.slide_title(0).unwrap().as_deref(), Some("New results"));
        assert!(p.set_slide_title(1, "x").unwrap_err().contains("neither the slide nor its layout has a title placeholder"));
        let mut p = reopen(&mut p);
        let before = snapshot(&p);
        p.set_slide_title(2, "Created").unwrap();
        let q = reopen(&mut p);
        assert_eq!(q.slide_title(2).unwrap().as_deref(), Some("Created"));
        assert_eq!(changed(&before, &snapshot(&q)), ["ppt/slides/slide3.xml"]);
    }

    #[test]
    fn hyperlinks_split_runs_and_read_back() {
        let mut p = deck();
        let tb = id_of(&p, 0, "text");
        assert_eq!(p.set_link(0, &ShapeRef::Id(tb), "https://example.org/nyq?a=1&b=2", Some("Nyquist")).unwrap(), 1);
        let mut q = reopen(&mut p);
        assert_eq!(q.links(0).unwrap(), vec![(tb, "Nyquist".to_string(), "https://example.org/nyq?a=1&b=2".to_string())]);
        assert_eq!(q.slide_text(0).unwrap().lines().last(), Some("see the Nyquist plot here"), "splitting runs changes no text");
        let d = q.pkg.get_xml("ppt/slides/slide1.xml").unwrap();
        let sp = d.root.find_all("p:sp")[2].clone();
        let runs: Vec<String> = sp.find_all("a:r").iter().map(|r| r.child("a:t").unwrap().text()).collect();
        assert_eq!(runs, ["see the ", "Nyquist", " plot here"]);
        assert_eq!(sp.find_all("a:rPr").iter().filter(|r| r.attr("sz") == Some("2800")).count(), 3, "every piece keeps the run's size");
        // Not found: an error, and no relationship left behind.
        let nrels = q.pkg.rels("ppt/slides/slide1.xml").unwrap().len();
        assert!(q.set_link(0, &ShapeRef::Id(tb), "https://x", Some("Bode")).unwrap_err().contains("`Bode` does not occur"));
        assert_eq!(q.pkg.rels("ppt/slides/slide1.xml").unwrap().len(), nrels);
        // Every occurrence, and a whole shape.
        q.set_shape_text(0, &ShapeRef::Id(tb), "Z and Z").unwrap();
        assert_eq!(q.set_link(0, &ShapeRef::Id(tb), "mailto:a@b.c", Some("Z")).unwrap(), 2);
        assert_eq!(q.links(0).unwrap().len(), 2);
        assert_eq!(q.set_link(0, &ShapeRef::Name("Title 1".into()), "https://t", None).unwrap(), 1);
        assert_eq!(q.links(0).unwrap()[0].1, "Results");
    }

    #[test]
    fn theme_is_read_and_edited_in_the_theme_part_only() {
        let mut p = deck();
        let c = p.theme_colors().unwrap();
        assert_eq!(c.len(), 12);
        assert_eq!(c[0], ("dk1".to_string(), "#000000".to_string()), "a system colour reads as its saved value");
        assert_eq!(c[4], ("accent1".to_string(), "#4472C4".to_string()));
        assert_eq!(p.theme_fonts().unwrap(), ("Calibri Light".to_string(), "Calibri".to_string()));
        let before = snapshot(&p);
        p.set_theme_colors(&[("accent1", "#1a2b3c"), ("dk1", "101010")]).unwrap();
        p.set_theme_fonts(Some("Georgia"), None).unwrap();
        let mut q = reopen(&mut p);
        assert_eq!(changed(&before, &snapshot(&q)), ["ppt/theme/theme1.xml"]);
        let c = q.theme_colors().unwrap();
        assert_eq!((c[0].1.as_str(), c[4].1.as_str(), c[5].1.as_str()), ("#101010", "#1A2B3C", "#ED7D31"));
        assert_eq!(q.theme_fonts().unwrap(), ("Georgia".to_string(), "Calibri".to_string()));
        assert!(q.set_theme_colors(&[("accent9", "#000000")]).unwrap_err().contains("accent6"));
        assert!(q.set_theme_colors(&[("accent1", "blue")]).unwrap_err().contains("#rrggbb"));
    }
}
