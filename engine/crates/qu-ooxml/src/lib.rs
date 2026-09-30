//! The OOXML package layer shared by `qu-docx` and `qu-pptx`.
//!
//! A `.docx`/`.pptx`/`.xlsx` is a ZIP of XML "parts" wired together by
//! relationship files and a content-type map (OPC, ECMA-376 part 2). This
//! crate is that layer and nothing format-specific:
//!
//! * [`Package`] -- the parts, in their original order, as bytes. A part
//!   nobody edits is written back byte for byte: macros, custom XML,
//!   embedded workbooks, fonts, vendor extensions all survive a round trip
//!   because they are never parsed at all.
//! * [`xml`] -- a tree that round-trips what it parsed, for the parts that
//!   ARE edited.
//! * relationships, content types and core properties -- the bookkeeping
//!   every added image, slide or header needs.
//! * [`replace_in_paragraph`] -- find-and-replace that works when Word or
//!   PowerPoint has split the text across several formatting runs, which
//!   they do constantly (spell-check marks, revision ids, a bolded letter).
//!
//! The design rule, from `docs/design/toolkit-office.md`: preserve the
//! package by default, change the minimum XML, never rebuild a document
//! from a parsed model.

pub mod chart;
pub mod xml;

use std::io::{Cursor, Read, Write};
use xml::{Doc, Element, Node};

// ------------------------------------------------------------------ package

pub struct Package {
    parts: Vec<(String, Vec<u8>)>,
}

impl Package {
    pub fn empty() -> Self {
        Package { parts: Vec::new() }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| format!("not an Office file (not a ZIP package): {e}"))?;
        let mut parts = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| format!("damaged package entry {i}: {e}"))?;
            if f.is_dir() {
                continue;
            }
            let mut data = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut data).map_err(|e| format!("damaged package entry `{}`: {e}", f.name()))?;
            parts.push((f.name().to_string(), data));
        }
        if !parts.iter().any(|(n, _)| n == "[Content_Types].xml") {
            return Err("not an Office file: the package has no [Content_Types].xml".into());
        }
        Ok(Package { parts })
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("could not read `{path}`: {e}"))?;
        Self::from_bytes(&bytes).map_err(|e| format!("`{path}`: {e}"))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // [Content_Types].xml first: the spec does not require it, but
        // some consumers sniff the first entry.
        let mut order: Vec<&(String, Vec<u8>)> = self.parts.iter().collect();
        order.sort_by_key(|(n, _)| n != "[Content_Types].xml");
        for (name, data) in order {
            w.start_file(name.as_str(), opts).map_err(|e| format!("writing `{name}`: {e}"))?;
            w.write_all(data).map_err(|e| format!("writing `{name}`: {e}"))?;
        }
        Ok(w.finish().map_err(|e| format!("finishing the package: {e}"))?.into_inner())
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes).map_err(|e| format!("could not write `{path}`: {e}"))
    }

    pub fn names(&self) -> Vec<String> {
        self.parts.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.parts.iter().any(|(n, _)| n == name)
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, d)| d.as_slice())
    }

    pub fn get_str(&self, name: &str) -> Result<String, String> {
        let b = self.get(name).ok_or_else(|| format!("the package has no part `{name}`"))?;
        // A UTF-8 BOM is legal at the start of a part.
        let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
        String::from_utf8(b.to_vec()).map_err(|_| format!("part `{name}` is not UTF-8 XML"))
    }

    pub fn get_xml(&self, name: &str) -> Result<Doc, String> {
        xml::parse(&self.get_str(name)?).map_err(|e| format!("part `{name}`: {e}"))
    }

    pub fn set(&mut self, name: &str, data: Vec<u8>) {
        match self.parts.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = data,
            None => self.parts.push((name.to_string(), data)),
        }
    }

    pub fn set_xml(&mut self, name: &str, doc: &Doc) {
        self.set(name, doc.to_xml().into_bytes());
    }

    pub fn remove(&mut self, name: &str) {
        self.parts.retain(|(n, _)| n != name);
    }

    /// A fresh part name `prefix{N}{suffix}` not yet in the package, N >= 1.
    pub fn next_name(&self, prefix: &str, suffix: &str) -> String {
        (1..).map(|n| format!("{prefix}{n}{suffix}")).find(|c| !self.has(c)).unwrap()
    }

    // -------------------------------------------------------- content types

    /// Make sure files with extension `ext` have a content type.
    pub fn ensure_default_type(&mut self, ext: &str, content_type: &str) -> Result<(), String> {
        let mut ct = self.get_xml("[Content_Types].xml")?;
        let has = ct.root.elems().any(|e| e.name == "Default" && e.attr("Extension").is_some_and(|x| x.eq_ignore_ascii_case(ext)));
        if !has {
            let d = Element::new("Default").with_attr("Extension", ext).with_attr("ContentType", content_type);
            // Defaults conventionally precede Overrides.
            let pos = ct.root.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "Override")).unwrap_or(ct.root.children.len());
            ct.root.children.insert(pos, Node::Elem(d));
            self.set_xml("[Content_Types].xml", &ct);
        }
        Ok(())
    }

    pub fn add_override(&mut self, part: &str, content_type: &str) -> Result<(), String> {
        let mut ct = self.get_xml("[Content_Types].xml")?;
        let pn = format!("/{}", part.trim_start_matches('/'));
        ct.root.children.retain(|n| !matches!(n, Node::Elem(e) if e.name == "Override" && e.attr("PartName") == Some(pn.as_str())));
        ct.root.children.push(Node::Elem(Element::new("Override").with_attr("PartName", &pn).with_attr("ContentType", content_type)));
        self.set_xml("[Content_Types].xml", &ct);
        Ok(())
    }

    pub fn remove_override(&mut self, part: &str) -> Result<(), String> {
        let mut ct = self.get_xml("[Content_Types].xml")?;
        let pn = format!("/{}", part.trim_start_matches('/'));
        ct.root.children.retain(|n| !matches!(n, Node::Elem(e) if e.name == "Override" && e.attr("PartName") == Some(pn.as_str())));
        self.set_xml("[Content_Types].xml", &ct);
        Ok(())
    }

    pub fn override_type(&self, part: &str) -> Option<String> {
        let ct = self.get_xml("[Content_Types].xml").ok()?;
        let pn = format!("/{}", part.trim_start_matches('/'));
        let found = ct.root.elems().find(|e| e.name == "Override" && e.attr("PartName") == Some(pn.as_str())).and_then(|e| e.attr("ContentType").map(String::from));
        found
    }

    // -------------------------------------------------------- relationships

    pub fn rels(&self, part: &str) -> Result<Vec<Rel>, String> {
        let rp = rels_path(part);
        if !self.has(&rp) {
            return Ok(Vec::new());
        }
        let d = self.get_xml(&rp)?;
        Ok(d.root
            .elems()
            .filter(|e| e.name == "Relationship")
            .map(|e| Rel {
                id: e.attr("Id").unwrap_or("").to_string(),
                rel_type: e.attr("Type").unwrap_or("").to_string(),
                target: e.attr("Target").unwrap_or("").to_string(),
                external: e.attr("TargetMode") == Some("External"),
            })
            .collect())
    }

    /// Add a relationship from `part` and return its new id (`rIdN`).
    pub fn add_rel(&mut self, part: &str, rel_type: &str, target: &str) -> Result<String, String> {
        self.add_rel_mode(part, rel_type, target, false)
    }

    /// Add a relationship to something outside the package (a hyperlink's
    /// URL): `TargetMode="External"`, target written as given.
    pub fn add_external_rel(&mut self, part: &str, rel_type: &str, target: &str) -> Result<String, String> {
        self.add_rel_mode(part, rel_type, target, true)
    }

    fn add_rel_mode(&mut self, part: &str, rel_type: &str, target: &str, external: bool) -> Result<String, String> {
        let rp = rels_path(part);
        let mut d = if self.has(&rp) {
            self.get_xml(&rp)?
        } else {
            Doc::new(Element::new("Relationships").with_attr("xmlns", NS_PKG_RELS))
        };
        let used: Vec<String> = d.root.elems().filter_map(|e| e.attr("Id").map(String::from)).collect();
        let id = (1..).map(|n| format!("rId{n}")).find(|c| !used.contains(c)).unwrap();
        let mut rel = Element::new("Relationship").with_attr("Id", &id).with_attr("Type", rel_type).with_attr("Target", target);
        if external {
            rel.set_attr("TargetMode", "External");
        }
        d.root.children.push(Node::Elem(rel));
        self.set_xml(&rp, &d);
        Ok(id)
    }

    pub fn remove_rel(&mut self, part: &str, id: &str) -> Result<(), String> {
        let rp = rels_path(part);
        if !self.has(&rp) {
            return Ok(());
        }
        let mut d = self.get_xml(&rp)?;
        d.root.children.retain(|n| !matches!(n, Node::Elem(e) if e.name == "Relationship" && e.attr("Id") == Some(id)));
        self.set_xml(&rp, &d);
        Ok(())
    }

    // -------------------------------------------------------- core properties

    /// `docProps/core.xml` as (name, value) pairs, prefixes stripped:
    /// title, subject, creator, keywords, description, lastModifiedBy,
    /// revision, created, modified, category.
    pub fn core_properties(&self) -> Vec<(String, String)> {
        let Ok(d) = self.get_xml("docProps/core.xml") else { return Vec::new() };
        d.root
            .elems()
            .map(|e| (e.name.rsplit(':').next().unwrap_or(&e.name).to_string(), e.text()))
            .collect()
    }

    /// Set core properties by short name (`title`, `subject`, `creator`,
    /// `keywords`, `description`, `lastModifiedBy`, `category`), creating
    /// `docProps/core.xml` (and its package relationship) if missing.
    pub fn set_core_properties(&mut self, props: &[(&str, &str)]) -> Result<(), String> {
        if !self.has("docProps/core.xml") {
            self.set_xml("docProps/core.xml", &Doc::new(core_props_root()));
            self.add_override("docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml")?;
            self.add_rel("", REL_CORE_PROPS, "docProps/core.xml")?;
        }
        let mut d = self.get_xml("docProps/core.xml")?;
        for (k, v) in props {
            let prefixed = match *k {
                "title" | "subject" | "creator" | "description" => format!("dc:{k}"),
                "keywords" | "lastModifiedBy" | "category" | "revision" => format!("cp:{k}"),
                other => return Err(format!("unknown document property `{other}`")),
            };
            let el = d.root.ensure_child(&prefixed);
            el.set_text(v);
        }
        self.set_xml("docProps/core.xml", &d);
        Ok(())
    }
}

fn core_props_root() -> Element {
    Element::new("cp:coreProperties")
        .with_attr("xmlns:cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties")
        .with_attr("xmlns:dc", "http://purl.org/dc/elements/1.1/")
        .with_attr("xmlns:dcterms", "http://purl.org/dc/terms/")
        .with_attr("xmlns:dcmitype", "http://purl.org/dc/dcmitype/")
        .with_attr("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance")
}

/// The core-properties part for a new document, with `title`/`creator`.
pub fn new_core_xml(creator: &str) -> String {
    let mut root = core_props_root();
    root.children.push(Node::Elem(Element::new("dc:title")));
    root.children.push(Node::Elem(Element::new("dc:creator").with_text(creator)));
    Doc::new(root).to_xml()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rel {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub external: bool,
}

pub const NS_PKG_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const REL_CORE_PROPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
pub const REL_EXT_PROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
pub const REL_OFFICE_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
pub const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
pub const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// `word/document.xml` -> `word/_rels/document.xml.rels`; `""` (the
/// package itself) -> `_rels/.rels`.
pub fn rels_path(part: &str) -> String {
    if part.is_empty() {
        return "_rels/.rels".to_string();
    }
    match part.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target relative to the part that owns it:
/// (`ppt/presentation.xml`, `slides/slide1.xml`) -> `ppt/slides/slide1.xml`.
pub fn resolve_target(source_part: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let base: Vec<&str> = match source_part.rfind('/') {
        Some(i) => source_part[..i].split('/').collect(),
        None => Vec::new(),
    };
    let mut parts: Vec<&str> = base;
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The inverse of `resolve_target`: the path of `part` as written from
/// inside `source_part`'s folder.
pub fn relative_target(source_part: &str, part: &str) -> String {
    let base: Vec<&str> = match source_part.rfind('/') {
        Some(i) => source_part[..i].split('/').collect(),
        None => Vec::new(),
    };
    let target: Vec<&str> = part.split('/').collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out: Vec<String> = std::iter::repeat("..".to_string()).take(base.len() - common).collect();
    out.extend(target[common..].iter().map(|s| s.to_string()));
    out.join("/")
}

// ------------------------------------------------------------------ convert

/// Where LibreOffice is: `QU_SOFFICE` if set, else `soffice`/`libreoffice`
/// on PATH, else the standard install locations.
pub fn find_office() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("QU_SOFFICE") {
        let p = std::path::PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let exe = if cfg!(windows) { ["soffice.exe", "soffice.com"] } else { ["soffice", "libreoffice"] };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for e in exe {
                let c = dir.join(e);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    let fixed: &[&str] = if cfg!(windows) {
        &["C:\\Program Files\\LibreOffice\\program\\soffice.exe", "C:\\Program Files (x86)\\LibreOffice\\program\\soffice.exe"]
    } else if cfg!(target_os = "macos") {
        &["/Applications/LibreOffice.app/Contents/MacOS/soffice"]
    } else {
        &["/usr/bin/soffice", "/usr/lib/libreoffice/program/soffice", "/opt/libreoffice/program/soffice"]
    };
    fixed.iter().map(std::path::PathBuf::from).find(|p| p.is_file())
}

/// Convert an Office file (`bytes`, of type `ext`) to `to` (`pdf`, `html`,
/// `txt`, ...) by running LibreOffice headless in a private profile, so it
/// neither needs nor disturbs a LibreOffice the user has open.
///
/// Laying out a document -- fonts, line breaking, pagination, floats -- is
/// a word processor's whole job; this hands it to one rather than
/// pretending to. No LibreOffice, no conversion: the error says how to get
/// it rather than producing something that only looks like a PDF.
pub fn convert_with_office(bytes: &[u8], ext: &str, to: &str, timeout_s: u64) -> Result<Vec<u8>, String> {
    let office = find_office().ok_or(
        "converting needs LibreOffice, which was not found -- install it (libreoffice.org), or point \
         QU_SOFFICE at its soffice executable",
    )?;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("qu-office-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("convert: cannot create a temporary folder: {e}"))?;
    let cleanup = |r: Result<Vec<u8>, String>| {
        let _ = std::fs::remove_dir_all(&dir);
        r
    };
    let input = dir.join(format!("document.{ext}"));
    if let Err(e) = std::fs::write(&input, bytes) {
        return cleanup(Err(format!("convert: cannot write the temporary input: {e}")));
    }
    let profile = dir.join("profile");
    let profile_url = format!("file:///{}", profile.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    let mut child = match std::process::Command::new(&office)
        .arg("--headless")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg(to)
        .arg("--outdir")
        .arg(&dir)
        .arg(&input)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return cleanup(Err(format!("convert: could not start LibreOffice at `{}`: {e}", office.display()))),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed().as_secs() >= timeout_s => {
                let _ = child.kill();
                let _ = child.wait();
                return cleanup(Err(format!("convert: LibreOffice took longer than {timeout_s} s and was stopped")));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return cleanup(Err(format!("convert: waiting for LibreOffice failed: {e}"))),
        }
    }
    let mut stderr = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut stderr);
    }
    let out_ext = to.split(':').next().unwrap_or(to);
    match std::fs::read(dir.join(format!("document.{out_ext}"))) {
        Ok(b) => cleanup(Ok(b)),
        Err(_) => {
            let why = stderr.lines().filter(|l| !l.contains("javaldx")).collect::<Vec<_>>().join(" ");
            cleanup(Err(format!("convert: LibreOffice produced no {out_ext} ({})", if why.trim().is_empty() { "no message" } else { why.trim() })))
        }
    }
}

// ------------------------------------------------------------------ images

/// Pixel size and content type of a PNG, JPEG or GIF, from its header.
pub fn image_info(bytes: &[u8]) -> Result<(u32, u32, &'static str, &'static str), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        return Ok((w, h, "png", "image/png"));
    }
    if bytes.starts_with(b"GIF8") && bytes.len() >= 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return Ok((w, h, "gif", "image/gif"));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // Walk the JPEG segments to the first start-of-frame marker.
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Ok((w, h, "jpeg", "image/jpeg"));
            }
            i += 2 + len;
        }
        return Err("image: a JPEG with no frame header".into());
    }
    Err("image: only PNG, JPEG and GIF can be embedded".into())
}

/// English Metric Units: 914400 per inch, 36000 per millimetre, 12700 per point.
pub const EMU_PER_MM: f64 = 36000.0;
pub const EMU_PER_PT: f64 = 12700.0;

pub fn mm_to_emu(mm: f64) -> i64 {
    (mm * EMU_PER_MM).round() as i64
}

// ------------------------------------------------------------------ text

/// Paragraph-level text model for find/replace, parameterised by the
/// format's element names: Word is (`w:p`, `w:t`), DrawingML (PowerPoint,
/// charts, text boxes) is (`a:p`, `a:t`).
#[derive(Clone, Copy)]
pub struct TextNames {
    pub para: &'static str,
    pub text: &'static str,
    /// Word needs `xml:space="preserve"` on a text element with edge spaces.
    pub preserve_space: bool,
}

pub const WORD: TextNames = TextNames { para: "w:p", text: "w:t", preserve_space: true };
pub const DRAWING: TextNames = TextNames { para: "a:p", text: "a:t", preserve_space: false };

/// Paths (child indices from `root`) to every paragraph element under it,
/// document order, nested ones included.
pub fn paragraph_paths(root: &Element, names: TextNames) -> Vec<Vec<usize>> {
    fn walk(e: &Element, names: TextNames, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, n) in e.children.iter().enumerate() {
            if let Node::Elem(c) = n {
                path.push(i);
                if c.name == names.para {
                    out.push(path.clone());
                }
                walk(c, names, path, out);
                path.pop();
            }
        }
    }
    let mut out = Vec::new();
    walk(root, names, &mut Vec::new(), &mut out);
    out
}

pub fn at_path<'a>(root: &'a Element, path: &[usize]) -> &'a Element {
    let mut e = root;
    for &i in path {
        e = match &e.children[i] {
            Node::Elem(c) => c,
            _ => unreachable!("paths only point at elements"),
        };
    }
    e
}

pub fn at_path_mut<'a>(root: &'a mut Element, path: &[usize]) -> &'a mut Element {
    let mut e = root;
    for &i in path {
        e = match &mut e.children[i] {
            Node::Elem(c) => c,
            _ => unreachable!("paths only point at elements"),
        };
    }
    e
}

/// Paths (relative to the paragraph) of its own text elements, skipping
/// any nested paragraph (a text box inside a paragraph is its own).
fn own_text_paths(p: &Element, names: TextNames) -> Vec<Vec<usize>> {
    fn walk(e: &Element, names: TextNames, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, n) in e.children.iter().enumerate() {
            if let Node::Elem(c) = n {
                if c.name == names.para {
                    continue;
                }
                path.push(i);
                if c.name == names.text {
                    out.push(path.clone());
                } else {
                    walk(c, names, path, out);
                }
                path.pop();
            }
        }
    }
    let mut out = Vec::new();
    walk(p, names, &mut Vec::new(), &mut out);
    out
}

/// The paragraph's own text: its text elements concatenated.
pub fn paragraph_text(p: &Element, names: TextNames) -> String {
    own_text_paths(p, names).iter().map(|path| at_path(p, path).text()).collect()
}

/// Replace every occurrence of `old` in the paragraph's text with `new`,
/// even across runs. The replacement lands in the run where the match
/// starts (so it takes that run's formatting), and the matched text is
/// removed from the runs it spilled into. Returns the number replaced.
pub fn replace_in_paragraph(p: &mut Element, names: TextNames, old: &str, new: &str) -> usize {
    if old.is_empty() {
        return 0;
    }
    let paths = own_text_paths(p, names);
    let segs: Vec<String> = paths.iter().map(|path| at_path(p, path).text()).collect();
    let full: String = segs.concat();
    if !full.contains(old) {
        return 0;
    }
    // Byte offset where each segment starts in `full`.
    let mut starts = Vec::with_capacity(segs.len());
    let mut acc = 0;
    for s in &segs {
        starts.push(acc);
        acc += s.len();
    }
    let seg_of = |off: usize| starts.iter().rposition(|&s| s <= off).unwrap_or(0);
    let mut out: Vec<String> = vec![String::new(); segs.len()];
    let mut count = 0;
    let mut i = 0;
    while i < full.len() {
        if full[i..].starts_with(old) {
            // Attribute to the first non-empty segment at or after `i`.
            let mut s = seg_of(i);
            while s + 1 < segs.len() && starts[s] + segs[s].len() <= i {
                s += 1;
            }
            out[s].push_str(new);
            i += old.len();
            count += 1;
        } else {
            let c = full[i..].chars().next().unwrap();
            let mut s = seg_of(i);
            while s + 1 < segs.len() && starts[s] + segs[s].len() <= i {
                s += 1;
            }
            out[s].push(c);
            i += c.len_utf8();
        }
    }
    for (path, text) in paths.iter().zip(out) {
        let t = at_path_mut(p, path);
        if names.preserve_space {
            if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
                t.set_attr("xml:space", "preserve");
            }
        }
        t.set_text(&text);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_spans_runs_and_keeps_the_first_runs_formatting() {
        let src = r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hel</w:t></w:r><w:r><w:t>lo wor</w:t></w:r><w:r><w:t>ld!</w:t></w:r></w:p>"#;
        let mut p = xml::parse(src).unwrap().root;
        assert_eq!(replace_in_paragraph(&mut p, WORD, "Hello world", "Bye"), 1);
        assert_eq!(paragraph_text(&p, WORD), "Bye!");
        let ts = p.find_all("w:t");
        assert_eq!(ts[0].text(), "Bye", "replacement lands in the bold run");
        assert_eq!(ts[1].text(), "");
        assert_eq!(ts[2].text(), "!");
    }

    #[test]
    fn replace_counts_and_handles_unicode_and_repeats() {
        let mut p = xml::parse("<a:p><a:r><a:t>2025 · 2025</a:t></a:r><a:r><a:t>·2025</a:t></a:r></a:p>").unwrap().root;
        assert_eq!(replace_in_paragraph(&mut p, DRAWING, "2025", "2026"), 3);
        assert_eq!(paragraph_text(&p, DRAWING), "2026 · 2026·2026");
        assert_eq!(replace_in_paragraph(&mut p, DRAWING, "", "x"), 0);
    }

    #[test]
    fn edge_spaces_get_preserve_in_word() {
        let mut p = xml::parse("<w:p><w:r><w:t>a-b</w:t></w:r></w:p>").unwrap().root;
        replace_in_paragraph(&mut p, WORD, "-", " - ");
        replace_in_paragraph(&mut p, WORD, "a", " ");
        assert_eq!(p.find_all("w:t")[0].attr("xml:space"), Some("preserve"));
    }

    #[test]
    fn target_paths_resolve_both_ways() {
        assert_eq!(resolve_target("ppt/slides/slide1.xml", "../media/image1.png"), "ppt/media/image1.png");
        assert_eq!(resolve_target("ppt/presentation.xml", "slides/slide2.xml"), "ppt/slides/slide2.xml");
        assert_eq!(relative_target("ppt/slides/slide1.xml", "ppt/media/image1.png"), "../media/image1.png");
        assert_eq!(rels_path("word/document.xml"), "word/_rels/document.xml.rels");
        assert_eq!(rels_path(""), "_rels/.rels");
    }

    #[test]
    fn external_rels_carry_target_mode_and_ids_do_not_collide() {
        let mut p = Package::empty();
        let a = p.add_rel("ppt/slides/slide1.xml", REL_IMAGE, "../media/image1.png").unwrap();
        let b = p.add_external_rel("ppt/slides/slide1.xml", REL_HYPERLINK, "https://example.org/a?b=1&c=2").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("rId1", "rId2"));
        let rels = p.rels("ppt/slides/slide1.xml").unwrap();
        assert!(!rels[0].external);
        assert!(rels[1].external);
        assert_eq!(rels[1].target, "https://example.org/a?b=1&c=2", "the URL round-trips through escaping");
        assert!(p.get_str("ppt/slides/_rels/slide1.xml.rels").unwrap().contains("TargetMode=\"External\""));
    }

    #[test]
    fn external_relationships_carry_target_mode() {
        let mut p = Package::empty();
        let a = p.add_rel("word/document.xml", "urn:t/styles", "styles.xml").unwrap();
        let b = p.add_external_rel("word/document.xml", "urn:t/hyperlink", "https://x.org/?a=1&b=2").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("rId1", "rId2"));
        let rels = p.rels("word/document.xml").unwrap();
        assert!(!rels[0].external && rels[1].external);
        assert_eq!(rels[1].target, "https://x.org/?a=1&b=2");
        assert!(p.get_str("word/_rels/document.xml.rels").unwrap().contains("Target=\"https://x.org/?a=1&amp;b=2\" TargetMode=\"External\""));
    }

    #[test]
    fn png_header_gives_its_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(image_info(&png).unwrap().0..image_info(&png).unwrap().1, 640..480);
    }
}
