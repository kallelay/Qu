//! A small XML tree that round-trips what it was given.
//!
//! Not a general XML library: OOXML parts are machine-written, namespaced,
//! DTD-free XML, and what an editor over them needs is different from what
//! a data reader needs. Every node the file had comes back out -- comments,
//! processing instructions, the declaration, whitespace between elements,
//! attribute ORDER, unknown elements from vendor extensions -- because the
//! only safe way to edit a document written by someone else's software is
//! to change the part you meant to and nothing else. Names are kept as
//! written, prefix included (`w:p`), since OOXML fixes its prefixes in
//! practice and matching on them is what every consumer does.
//!
//! What is normalised on the way back out: entity spelling (`&#38;` comes
//! back as `&amp;`), attribute quotes (always `"`), and `<x></x>` vs
//! `<x/>` for an element with no children (always `<x/>`). None of those
//! is visible to any reader of the format.

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Elem(Element),
    /// Character data, unescaped.
    Text(String),
    /// Anything kept verbatim: comments, processing instructions, CDATA
    /// sections, a DOCTYPE. Stored with its delimiters.
    Raw(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    /// Everything before the root element (declaration, comments), verbatim.
    pub prolog: String,
    pub root: Element,
    /// Anything after the root element, verbatim.
    pub epilog: String,
}

impl Element {
    pub fn new(name: &str) -> Self {
        Element { name: name.to_string(), attrs: Vec::new(), children: Vec::new() }
    }

    pub fn with_attr(mut self, k: &str, v: &str) -> Self {
        self.set_attr(k, v);
        self
    }

    pub fn with_child(mut self, c: Element) -> Self {
        self.children.push(Node::Elem(c));
        self
    }

    pub fn with_text(mut self, t: &str) -> Self {
        self.children.push(Node::Text(t.to_string()));
        self
    }

    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, k: &str, v: &str) {
        match self.attrs.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => self.attrs.push((k.to_string(), v.to_string())),
        }
    }

    pub fn remove_attr(&mut self, k: &str) {
        self.attrs.retain(|(n, _)| n != k);
    }

    pub fn elems(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Elem(e) => Some(e),
            _ => None,
        })
    }

    pub fn elems_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|n| match n {
            Node::Elem(e) => Some(e),
            _ => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elems().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elems_mut().find(|e| e.name == name)
    }

    /// The child named `name`, created (appended) if absent.
    pub fn ensure_child(&mut self, name: &str) -> &mut Element {
        if self.child(name).is_none() {
            self.children.push(Node::Elem(Element::new(name)));
        }
        self.child_mut(name).unwrap()
    }

    /// The child named `name`, created at the FRONT if absent -- for
    /// property elements (`w:pPr`, `w:rPr`) that the schema requires first.
    pub fn ensure_first_child(&mut self, name: &str) -> &mut Element {
        if self.child(name).is_none() {
            self.children.insert(0, Node::Elem(Element::new(name)));
        }
        self.child_mut(name).unwrap()
    }

    pub fn remove_children(&mut self, name: &str) {
        self.children.retain(|n| !matches!(n, Node::Elem(e) if e.name == name));
    }

    /// Every descendant element named `name`, depth first, document order.
    pub fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for e in self.elems() {
            if e.name == name {
                out.push(e);
            }
            e.descendants(name, out);
        }
    }

    pub fn find_all<'a>(&'a self, name: &str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        self.descendants(name, &mut out);
        out
    }

    /// Concatenated character data of every descendant, document order.
    pub fn text(&self) -> String {
        let mut s = String::new();
        fn walk(e: &Element, s: &mut String) {
            for n in &e.children {
                match n {
                    Node::Text(t) => s.push_str(t),
                    Node::Elem(c) => walk(c, s),
                    Node::Raw(_) => {}
                }
            }
        }
        walk(self, &mut s);
        s
    }

    /// Replace all children with a single text node.
    pub fn set_text(&mut self, t: &str) {
        self.children = vec![Node::Text(t.to_string())];
    }

    /// Apply `f` to every element of the subtree (self included),
    /// children before parents.
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Element)) {
        for n in self.children.iter_mut() {
            if let Node::Elem(e) = n {
                e.walk_mut(f);
            }
        }
        f(self);
    }
}

// ------------------------------------------------------------------ parse

pub fn parse(src: &str) -> Result<Doc, String> {
    let b = src.as_bytes();
    let mut p = Parser { s: src, b, i: 0 };
    let start = p.i;
    // Prolog: declaration, comments, PIs, whitespace, doctype.
    loop {
        p.skip_ws();
        if p.starts("<?") {
            p.until("?>")?;
        } else if p.starts("<!--") {
            p.until("-->")?;
        } else if p.starts("<!DOCTYPE") {
            p.until(">")?;
        } else {
            break;
        }
    }
    let prolog = src[start..p.i].to_string();
    if !p.starts("<") {
        return Err("xml: no root element".into());
    }
    let root = p.element()?;
    let epilog = src[p.i..].to_string();
    Ok(Doc { prolog, root, epilog })
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn starts(&self, pat: &str) -> bool {
        self.b[self.i..].starts_with(pat.as_bytes())
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() && (self.b[self.i] as char).is_ascii_whitespace() {
            self.i += 1;
        }
    }

    /// Advance past the next `pat`; returns the text consumed (inclusive).
    fn until(&mut self, pat: &str) -> Result<&'a str, String> {
        let from = self.i;
        match self.s[self.i..].find(pat) {
            Some(off) => {
                self.i += off + pat.len();
                Ok(&self.s[from..self.i])
            }
            None => Err(format!("xml: unterminated construct at byte {from} (expected `{pat}`)")),
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let from = self.i;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_whitespace() || c == b'>' || c == b'/' || c == b'=' {
                break;
            }
            self.i += 1;
        }
        if self.i == from {
            return Err(format!("xml: expected a name at byte {from}"));
        }
        Ok(self.s[from..self.i].to_string())
    }

    fn element(&mut self) -> Result<Element, String> {
        // at '<'
        self.i += 1;
        let name = self.name()?;
        let mut el = Element { name, attrs: Vec::new(), children: Vec::new() };
        loop {
            self.skip_ws();
            if self.starts("/>") {
                self.i += 2;
                return Ok(el);
            }
            if self.starts(">") {
                self.i += 1;
                break;
            }
            let k = self.name()?;
            self.skip_ws();
            if !self.starts("=") {
                return Err(format!("xml: attribute `{k}` without a value at byte {}", self.i));
            }
            self.i += 1;
            self.skip_ws();
            let q = *self.b.get(self.i).ok_or("xml: truncated attribute")?;
            if q != b'"' && q != b'\'' {
                return Err(format!("xml: unquoted attribute `{k}` at byte {}", self.i));
            }
            self.i += 1;
            let from = self.i;
            while self.i < self.b.len() && self.b[self.i] != q {
                self.i += 1;
            }
            if self.i >= self.b.len() {
                return Err(format!("xml: unterminated attribute `{k}`"));
            }
            let v = unescape(&self.s[from..self.i])?;
            self.i += 1;
            el.attrs.push((k, v));
        }
        // content
        loop {
            if self.i >= self.b.len() {
                return Err(format!("xml: `<{}>` is never closed", el.name));
            }
            if self.starts("</") {
                self.i += 2;
                let close = self.name()?;
                if close != el.name {
                    return Err(format!("xml: `<{}>` closed by `</{close}>`", el.name));
                }
                self.skip_ws();
                if !self.starts(">") {
                    return Err("xml: malformed closing tag".into());
                }
                self.i += 1;
                return Ok(el);
            }
            if self.starts("<!--") {
                let raw = self.until("-->")?;
                el.children.push(Node::Raw(raw.to_string()));
            } else if self.starts("<![CDATA[") {
                let raw = self.until("]]>")?;
                el.children.push(Node::Raw(raw.to_string()));
            } else if self.starts("<?") {
                let raw = self.until("?>")?;
                el.children.push(Node::Raw(raw.to_string()));
            } else if self.starts("<") {
                let child = self.element()?;
                el.children.push(Node::Elem(child));
            } else {
                let from = self.i;
                while self.i < self.b.len() && self.b[self.i] != b'<' {
                    self.i += 1;
                }
                el.children.push(Node::Text(unescape(&self.s[from..self.i])?));
            }
        }
    }
}

pub fn unescape(s: &str) -> Result<String, String> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let semi = tail.find(';').ok_or_else(|| format!("xml: bare `&` in `{}`", &tail[..tail.len().min(20)]))?;
        let ent = &tail[1..semi];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                let c = u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32);
                out.push(c.ok_or_else(|| format!("xml: bad character reference `&{ent};`"))?);
            }
            _ if ent.starts_with('#') => {
                let c = ent[1..].parse::<u32>().ok().and_then(char::from_u32);
                out.push(c.ok_or_else(|| format!("xml: bad character reference `&{ent};`"))?);
            }
            _ => return Err(format!("xml: unknown entity `&{ent};`")),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

// ------------------------------------------------------------------ write

pub fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
}

impl Element {
    pub fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_attr(v, out);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for n in &self.children {
            match n {
                Node::Elem(e) => e.write(out),
                Node::Text(t) => escape_text(t, out),
                Node::Raw(r) => out.push_str(r),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }

    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

impl Doc {
    pub fn to_xml(&self) -> String {
        let mut s = String::with_capacity(4096);
        s.push_str(&self.prolog);
        self.root.write(&mut s);
        s.push_str(&self.epilog);
        s
    }

    /// A fresh document with the standard standalone declaration.
    pub fn new(root: Element) -> Self {
        Doc {
            prolog: "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n".to_string(),
            root,
            epilog: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_what_it_was_given() {
        let src = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<!-- c -->\
<w:document xmlns:w=\"urn:w\" b=\"2\" a=\"1\"><w:body><w:p><w:r><w:t xml:space=\"preserve\"> a &amp; b &lt;c&gt; </w:t></w:r></w:p>\
<x:unknown foo=\"&quot;q&quot;\"><![CDATA[<raw>]]><?pi x?></x:unknown><w:sectPr/></w:body></w:document>";
        let d = parse(src).unwrap();
        assert_eq!(d.to_xml(), src);
        assert_eq!(d.root.attrs[0].0, "xmlns:w", "attribute order is kept");
        assert_eq!(d.root.find_all("w:t")[0].text(), " a & b <c> ");
    }

    #[test]
    fn character_references_decode() {
        let d = parse("<a t=\"&#x41;&#66;\">&#233;</a>").unwrap();
        assert_eq!(d.root.attr("t"), Some("AB"));
        assert_eq!(d.root.text(), "\u{e9}");
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in ["<a>", "<a></b>", "<a x=1/>", "<a>&nope;</a>", "", "<a x=\"1></a>"] {
            assert!(parse(bad).is_err(), "`{bad}` should be refused");
        }
    }
}
