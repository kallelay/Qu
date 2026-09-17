//! Authored reports: a document the script composes, element by element.
//!
//! DELIBERATELY NOT `report.rs`, WHICH ALREADY EXISTS. That module renders
//! the AUTOMATIC report -- `write_report(path)` and `qu run --report`
//! snapshot every figure, table and printed line a script produced, with
//! no authoring control at all. This is the other half Ahmed asked for:
//! the script says what goes in, in what order, with styling and math,
//! and it emits to Markdown, HTML, Word and LaTeX.
//!
//! Capture-what-happened and compose-what-I-want are different features,
//! but they must not become two different-looking HTML reports. When the
//! HTML emitter lands it should render through `report.rs`'s existing
//! chrome rather than growing a second look -- the same reasoning as
//! `savefig`, whose formats "share one draw-command list so they never
//! visually disagree".
//!
//! ONE REPRESENTATION, SEVERAL EMITTERS, for that same reason. Four
//! independent exporters drift, and a fit report quoting a different
//! chi-square in Word than in HTML is worse than no report at all.
//!
//! MATH IS STORED WITHOUT DELIMITERS. `tex(x)` returns a bare math-mode
//! string, and the targets disagree about how to wrap it: Markdown and
//! LaTeX want dollars, MathJax wants its own delimiters, and Word wants
//! OMML or a picture -- not a LaTeX string at all. So an element carries
//! the math and its role, and each emitter wraps it. Storing pre-wrapped
//! LaTeX would make three of the four targets wrong.

/// What an element is. The kind decides how every emitter renders it, so
/// adding a format means one match arm per emitter, not a second document.
// `Eq`, not just `PartialEq`: dropped when `Img` gained a `width: Option<f64>`
// -- a float has no total order, so the enum can no longer promise one either.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    /// A heading at `level` 1-6. ONE variant with a level, not six
    /// variants: every emitter then has one arm that computes its own
    /// idiom from the number -- `#` repeated, `<h3>`, `\subsubsection`,
    /// or the Word paragraph style "Heading 3" -- instead of six
    /// near-identical arms per format, which is twenty-four arms to keep
    /// in step across four renderers.
    Heading(u8),
    /// A paragraph of running text.
    P,
    /// Math, stored WITHOUT delimiters -- see the module note.
    Tex,
    /// An inline run inside the flow, stylable after the fact.
    Span,
    /// An image by path, with the sizing a real document needs. The
    /// element's `text` is its caption, if any -- same convention as
    /// `Table`, so "does this element have a caption" is answered the
    /// same way for both rather than one storing it inline and the other
    /// beside it.
    Img(ImgData),
    /// A table. The element's `text` is its caption, if any.
    ///
    /// Header-ness lives here as a PROPERTY, not as a cell type. Ahmed
    /// floated mirroring HTML with `.tr/.td/.th`, and `th` is the reason
    /// not to: Word has `<w:tr>`/`<w:tc>` and no `th` at all -- a header
    /// row there is a row flag -- and LaTeX has `&`, `\\` and `\hline`
    /// with no `th` either. It exists in one of the four backends. That is
    /// the same trap as storing a literal `h1`, which he already ruled
    /// against. `.end_table()` is also avoided: caller-managed closing
    /// state is what produces unclosed-table bugs.
    Table(TableData),
}

/// An image element's own data: where it lives, and how wide it should
/// print. `width` is a fraction of the text width (LaTeX's `\linewidth`,
/// HTML's `100%` scaled) rather than an absolute size, because a report
/// is emitted to formats with different page widths and "0.8" means the
/// same thing -- 80% of whatever column it lands in -- in all of them;
/// an absolute inch figure sized for a one-column draft would overflow a
/// two-column journal class without every figure call being revisited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImgData {
    pub path: String,
    /// Fraction of the line width, `(0, 1]`. `None` falls back to a
    /// sane default per emitter rather than LaTeX's own bare
    /// `\includegraphics{}` (natural size) -- the reason this struct
    /// exists at all is that natural size is how a savefig'd figure
    /// blows out a column.
    pub width: Option<f64>,
    /// A LaTeX `\label{}` key, so a caption written elsewhere in the
    /// document can `\ref{}` this figure. Markdown/HTML have no
    /// equivalent and drop it silently -- cross-references are a LaTeX
    /// document's feature, not a Markdown one.
    pub label: Option<String>,
}

/// How a column is aligned. Carried in the tree rather than recomputed by
/// each emitter, so Markdown's `---:` and LaTeX's `r` are two spellings
/// of one decision instead of two rules that can disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// A table as DATA: cells already rendered to text, plus which columns
/// are numeric.
///
/// A Qu `Table` value is converted into this at the point of the call
/// rather than kept as itself. That is deliberate, and it is a choice
/// against reusing `table::Table::to_tex()`, which already emits a
/// perfectly good `tabular`.
///
/// Reusing it would give the LaTeX backend TWO table renderers -- one for
/// tables that came from a `Table` value and one for tables built from
/// plain rows -- and the moment either grows a caption, a column
/// alignment override or a border, they drift. `tex(table)` is a
/// different feature and keeps its own renderer; a report has one
/// representation and one renderer per format, which is the whole
/// premise of this module.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableData {
    /// Empty means the table has no header row.
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// One entry per column. Short or empty falls back to left.
    pub align: Vec<Align>,
    /// A LaTeX `\label{}` key, same reasoning as `ImgData::label`.
    pub label: Option<String>,
}

impl TableData {
    /// The widest row decides the column count: a ragged table is padded
    /// rather than rejected, because losing a cell silently is worse than
    /// an empty one, and every backend needs a rectangle.
    pub fn columns(&self) -> usize {
        self.rows.iter().map(|r| r.len()).chain(std::iter::once(self.header.len())).max().unwrap_or(0)
    }

    fn align_of(&self, col: usize) -> Align {
        self.align.get(col).copied().unwrap_or(Align::Left)
    }
}

/// Presentation attached to one element. Empty by default: an element
/// never styled must emit as plain text, because Markdown and LaTeX have
/// nowhere to put a background colour and should not invent one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub color: Option<String>,
    pub bg_color: Option<String>,
    pub border: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

impl Style {
    pub fn is_plain(&self) -> bool {
        self.color.is_none()
            && self.bg_color.is_none()
            && self.border.is_none()
            && !self.bold
            && !self.italic
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub kind: ElementKind,
    pub text: String,
    pub style: Style,
}

/// One document.
#[derive(Clone, Debug, Default)]
pub struct ReportDoc {
    pub title: String,
    pub elements: Vec<Element>,
}

/// A handle. Like `plotting::ArtistRef`: indices into state the
/// interpreter owns rather than the data itself, so a handle captured
/// from a `span` call still refers to the live element after later
/// appends. `element: None` means the handle IS the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportRef {
    pub doc: usize,
    pub element: Option<usize>,
}

impl ReportDoc {
    pub fn new(title: impl Into<String>) -> Self {
        ReportDoc { title: title.into(), elements: Vec::new() }
    }

    /// Appends and returns the new element's index, so a builder call can
    /// hand back a handle to what it just made.
    pub fn push(&mut self, kind: ElementKind, text: impl Into<String>) -> usize {
        self.elements.push(Element { kind, text: text.into(), style: Style::default() });
        self.elements.len() - 1
    }
}

/// A figure/table path relative to `from` (the document's own directory
/// at the point it gets written), given `to` (the path as the script
/// wrote it, already made absolute at `image()`-call time -- see that
/// builtin's own comment).
///
/// This is the fix for the gap a specification-by-example pass on a real
/// paper found immediately: a report built from the repository root and
/// compiled from its own subdirectory (the normal case -- a report and
/// its `\input`-ed figures section rarely share a working directory with
/// whatever process generated the figures) silently dropped the image,
/// because the path Qu wrote was correct FROM THE SCRIPT's directory and
/// meaningless from the DOCUMENT's. `\includegraphics`/`<img src>`/
/// Markdown's `![]()` all resolve relative to the file that names them,
/// not to any other process's notion of "current directory".
///
/// Walks both paths' components, drops the shared prefix, then emits one
/// `..` per remaining component of `from` followed by what's left of
/// `to`. Falls back to `to` unchanged if the two share no common
/// ancestor at all (e.g. different drive letters on Windows) -- there is
/// no relative path in that case, and an absolute one is more useful
/// than a broken one.
fn relative_path(from: &std::path::Path, to: &std::path::Path) -> String {
    use std::path::Component;
    let from_comps: Vec<Component> = from.components().collect();
    let to_comps: Vec<Component> = to.components().collect();
    let shared = from_comps.iter().zip(to_comps.iter()).take_while(|(a, b)| a == b).count();
    if shared == 0 && !from_comps.is_empty() && !to_comps.is_empty() && from_comps[0] != to_comps[0] {
        return to.to_string_lossy().replace('\\', "/");
    }
    let mut out = std::path::PathBuf::new();
    for _ in shared..from_comps.len() {
        out.push("..");
    }
    for comp in &to_comps[shared..] {
        out.push(comp.as_os_str());
    }
    if out.as_os_str().is_empty() {
        ".".to_string()
    } else {
        out.to_string_lossy().replace('\\', "/")
    }
}

/// Markdown, the first emitter.
///
/// Style is lossy here rather than smuggled through as raw HTML: a
/// Markdown file that only renders correctly in an HTML viewer is not
/// Markdown, and the four formats exist because they are read in
/// different places. Bold and italic survive because Markdown has them;
/// colours and borders do not.
pub fn to_markdown(doc: &ReportDoc) -> String {
    to_markdown_with_base(doc, None)
}

/// Same as `to_markdown`, but every `Img`'s path is rewritten relative
/// to `base_dir` (the document's own eventual directory) first -- see
/// `relative_path`'s own comment for why this has to happen at emit
/// time rather than at `image()`-call time. `None` (what the bare
/// `to_markdown` above passes, and what every existing test still
/// exercises) leaves paths exactly as the script wrote them, unchanged
/// behaviour for every caller that predates this.
pub fn to_markdown_with_base(doc: &ReportDoc, base_dir: Option<&std::path::Path>) -> String {
    let resolve_path = |p: &str| match base_dir {
        Some(base) => relative_path(base, std::path::Path::new(p)),
        None => p.to_string(),
    };
    let mut out = String::new();
    if !doc.title.trim().is_empty() {
        out.push_str("# ");
        out.push_str(doc.title.trim());
        out.push_str("\n\n");
    }
    for el in &doc.elements {
        match el.kind {
            ElementKind::Heading(level) => {
                out.push_str(&"#".repeat(clamp_level(level) as usize));
                out.push(' ');
                out.push_str(&inline_md(el));
                out.push_str("\n\n");
            }
            ElementKind::P | ElementKind::Span => {
                out.push_str(&inline_md(el));
                out.push_str("\n\n");
            }
            // Display math. The dollars are added HERE, by the emitter.
            ElementKind::Tex => {
                out.push_str("$$\n");
                out.push_str(el.text.trim());
                out.push_str("\n$$\n\n");
            }
            ElementKind::Img(ref img) => {
                // The caption is the alt text -- Markdown has nowhere
                // else to put it, and a reader with images off should
                // still learn what the figure was of.
                out.push_str("![");
                out.push_str(el.text.trim());
                out.push_str("](");
                out.push_str(&resolve_path(img.path.trim()));
                out.push_str(")\n\n");
            }
            ElementKind::Table(ref t) => {
                let n = t.columns();
                if n > 0 {
                    // Markdown's table syntax REQUIRES a header row --
                    // there is no headerless form -- so a table without
                    // one gets a row of empty cells rather than emitting
                    // a broken table. The other backends can express it
                    // properly; this is Markdown's limitation, taken here
                    // rather than pushed onto the caller.
                    let head: Vec<String> = (0..n)
                        .map(|i| md_cell(t.header.get(i).map(String::as_str).unwrap_or("")))
                        .collect();
                    out.push_str("| ");
                    out.push_str(&head.join(" | "));
                    out.push_str(" |\n|");
                    for i in 0..n {
                        out.push_str(match t.align_of(i) {
                            Align::Right => " ---: |",
                            Align::Left => " --- |",
                        });
                    }
                    out.push('\n');
                    for row in &t.rows {
                        let cells: Vec<String> = (0..n)
                            .map(|i| md_cell(row.get(i).map(String::as_str).unwrap_or("")))
                            .collect();
                        out.push_str("| ");
                        out.push_str(&cells.join(" | "));
                        out.push_str(" |\n");
                    }
                    out.push('\n');
                }
                if !el.text.trim().is_empty() {
                    out.push('*');
                    out.push_str(el.text.trim());
                    out.push_str("*\n\n");
                }
            }
        }
    }
    out
}

/// A pipe inside a cell would end the cell early -- the same defect that
/// cut this project's own generated reference tables in half earlier
/// today. Escaped rather than dropped.
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Levels outside 1-6 are clamped rather than rejected. Markdown, HTML
/// and Word all stop at six, and a caller who asks for a seventh wants a
/// deep heading, not an error part-way through building a document.
fn clamp_level(level: u8) -> u8 {
    level.clamp(1, 6)
}

/// LaTeX, the second emitter -- and the reason the tree is semantic.
///
/// Ahmed put it exactly right: "h1 will become \section{} in tex right?
/// and <h1> in html but different in XML word". Same element, three
/// unrelated spellings. The tree records WHAT a block is; each renderer
/// decides how its own format says it. Word will be the sharpest case,
/// since a .docx has no heading tag at all -- a heading there is an
/// ordinary paragraph carrying the "Heading 1" style.
pub fn to_latex(doc: &ReportDoc) -> String {
    to_latex_with_base(doc, None)
}

/// Same as `to_latex`, but every `Img`'s path is rewritten relative to
/// `base_dir` first -- see `relative_path` and `to_markdown_with_base`'s
/// matching comment. This is the one that matters most in practice:
/// `\includegraphics` resolves relative to the `.tex` file being
/// compiled, and a report generated from one directory and compiled
/// from another (the normal case for a real paper) silently dropped its
/// figures without this.
pub fn to_latex_with_base(doc: &ReportDoc, base_dir: Option<&std::path::Path>) -> String {
    let resolve_path = |p: &str| match base_dir {
        Some(base) => relative_path(base, std::path::Path::new(p)),
        None => p.to_string(),
    };
    let mut out = String::new();
    if !doc.title.trim().is_empty() {
        out.push_str("\\title{");
        out.push_str(&escape_tex(doc.title.trim()));
        out.push_str("}\n\\maketitle\n\n");
    }
    for el in &doc.elements {
        match el.kind {
            ElementKind::Heading(level) => {
                // LaTeX names its levels rather than numbering them, and
                // runs out at subsubsection -- below that it has no
                // sectioning command, so the deeper levels fall back to
                // paragraph/subparagraph, which is what those commands
                // are for.
                let cmd = match clamp_level(level) {
                    1 => "section",
                    2 => "subsection",
                    3 => "subsubsection",
                    4 => "paragraph",
                    _ => "subparagraph",
                };
                out.push('\\');
                out.push_str(cmd);
                out.push('{');
                out.push_str(&inline_tex(el));
                out.push_str("}\n\n");
            }
            ElementKind::P | ElementKind::Span => {
                out.push_str(&inline_tex(el));
                out.push_str("\n\n");
            }
            // Display math: LaTeX's own delimiters, added here.
            ElementKind::Tex => {
                out.push_str("\\[\n");
                out.push_str(el.text.trim());
                out.push_str("\n\\]\n\n");
            }
            // A real `figure` float, not a bare `\includegraphics`: a
            // paper-shaped document needs the figure to float to a
            // sensible page, a caption set in the class's own figure
            // style, and a width that does not depend on the image's
            // native pixel size -- `\includegraphics{path}` alone prints
            // at native resolution, which is how a 3000px savefig'd PDF
            // blows off the page edge. `[htbp]` is LaTeX's own
            // conventional default placement, not a strong opinion.
            ElementKind::Img(ref img) => {
                out.push_str("\\begin{figure}[htbp]\n\\centering\n\\includegraphics[width=");
                let w = img.width.unwrap_or(0.8).clamp(0.05, 1.0);
                out.push_str(&format!("{w}"));
                out.push_str("\\linewidth]{");
                out.push_str(&resolve_path(img.path.trim()));
                out.push('}');
                if !el.text.trim().is_empty() {
                    out.push_str("\n\\caption{");
                    out.push_str(&escape_tex(el.text.trim()));
                    out.push('}');
                }
                if let Some(label) = &img.label {
                    out.push_str("\n\\label{");
                    out.push_str(label.trim());
                    out.push('}');
                }
                out.push_str("\n\\end{figure}\n\n");
            }
            // A `table` float wrapping the `tabular`, same reasoning as
            // the figure float above: a bare `tabular` with a `\caption`
            // dangling after it is not a numbered, floatable table to
            // LaTeX, just a caption paragraph that happens to follow one
            // -- `\ref{}` to it resolves to nothing, and it cannot move
            // to the top of the next page the way a real float can. The
            // caption goes ABOVE the tabular here, unlike a figure's
            // (below): that is the convention every LaTeX table in print
            // follows, not a stylistic default of this emitter's own.
            ElementKind::Table(ref t) => {
                let n = t.columns();
                let has_caption = !el.text.trim().is_empty();
                let wrap = has_caption || t.label.is_some();
                if wrap {
                    out.push_str("\\begin{table}[htbp]\n\\centering\n");
                    if has_caption {
                        out.push_str("\\caption{");
                        out.push_str(&escape_tex(el.text.trim()));
                        out.push_str("}\n");
                    }
                    if let Some(label) = &t.label {
                        out.push_str("\\label{");
                        out.push_str(label.trim());
                        out.push_str("}\n");
                    }
                }
                if n > 0 {
                    let spec: String = (0..n)
                        .map(|i| match t.align_of(i) {
                            Align::Right => 'r',
                            Align::Left => 'l',
                        })
                        .collect();
                    out.push_str("\\begin{tabular}{");
                    out.push_str(&spec);
                    out.push_str("}\n\\hline\n");
                    // Unlike Markdown, LaTeX has no required header, so a
                    // headerless table simply has no header row and no
                    // second rule -- the representation carries the fact
                    // and each format says what it can.
                    if !t.header.is_empty() {
                        out.push_str(&tex_row(&t.header, n));
                        out.push_str("\\hline\n");
                    }
                    for row in &t.rows {
                        out.push_str(&tex_row(row, n));
                    }
                    out.push_str("\\hline\n\\end{tabular}\n");
                }
                if wrap {
                    out.push_str("\\end{table}\n\n");
                } else {
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// Whether the HTML carries a math renderer, and how.
///
/// `None` is the default and it is not laziness. This project already
/// refuses to depend on the viewer's machine: `savefig` embeds font bytes
/// so a figure does not change appearance elsewhere, the docs site is
/// static, and there is a standing rule that Qu material stays in the
/// repository. A report that renders its own equations only when the
/// reader is online walks away from all three. So the delimiters are
/// always emitted -- they cost nothing and any renderer can find them --
/// and pulling a renderer over the network is something the caller asks
/// for explicitly.
///
/// Vendoring a copy is the third option and the likely long-term answer;
/// it is Ahmed's call, and this enum is where it lands when he makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathJax {
    /// Emit MathJax delimiters, no script. Math shows as its source until
    /// something renders it.
    Off,
    /// Emit delimiters plus the CDN script tag. Needs the network.
    Cdn,
}

/// HTML, the third emitter.
///
/// Shares `report::REPORT_CSS` -- the retro window chrome the AUTOMATIC
/// report already uses -- so the two kinds of report look like one
/// product. It does NOT go through `report::render_report_html`, and the
/// difference matters: that function is shaped around `ReportCell`
/// (source, output, figures), which is a captured run. An authored
/// document is not a list of cells, and forcing it into that shape to
/// reuse the function would be reuse bought by lying about the content.
/// The asset is the reusable part; the renderer is not.
pub fn to_html(doc: &ReportDoc, mathjax: MathJax) -> String {
    to_html_with_base(doc, mathjax, None)
}

/// Same as `to_html`, but every `Img`'s `src` is rewritten relative to
/// `base_dir` first -- see `relative_path`'s comment.
pub fn to_html_with_base(doc: &ReportDoc, mathjax: MathJax, base_dir: Option<&std::path::Path>) -> String {
    let resolve_path = |p: &str| match base_dir {
        Some(base) => relative_path(base, std::path::Path::new(p)),
        None => p.to_string(),
    };
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html><head><meta charset=\"utf-8\">\n");
    out.push_str("<title>");
    out.push_str(&escape_html(if doc.title.trim().is_empty() { "Report" } else { doc.title.trim() }));
    out.push_str("</title>\n<style>\n");
    out.push_str(crate::report::REPORT_CSS);
    out.push_str("\n</style>\n");
    if mathjax == MathJax::Cdn {
        out.push_str(
            "<script id=\"MathJax-script\" async \
             src=\"https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js\"></script>\n",
        );
    }
    out.push_str("</head>\n<body>\n");
    if !doc.title.trim().is_empty() {
        out.push_str("<h1>");
        out.push_str(&escape_html(doc.title.trim()));
        out.push_str("</h1>\n");
    }
    for el in &doc.elements {
        match el.kind {
            ElementKind::Heading(level) => {
                let l = clamp_level(level);
                out.push_str(&format!("<h{l}{}>", style_attr(&el.style)));
                out.push_str(&escape_html(&el.text));
                out.push_str(&format!("</h{l}>\n"));
            }
            ElementKind::P => {
                out.push_str(&format!("<p{}>", style_attr(&el.style)));
                out.push_str(&escape_html(&el.text));
                out.push_str("</p>\n");
            }
            ElementKind::Span => {
                out.push_str(&format!("<span{}>", style_attr(&el.style)));
                out.push_str(&escape_html(&el.text));
                out.push_str("</span>\n");
            }
            // MathJax's own display delimiters, added here. The stored
            // text is bare and is NOT escaped -- escaping a formula would
            // turn its backslashes into printed text, the same asymmetry
            // the LaTeX emitter has.
            ElementKind::Tex => {
                out.push_str("<div class=\"math\">\\[");
                out.push_str(el.text.trim());
                out.push_str("\\]</div>\n");
            }
            ElementKind::Img(ref img) => {
                let w = (img.width.unwrap_or(0.8).clamp(0.05, 1.0) * 100.0).round();
                out.push_str("<figure><img src=\"");
                out.push_str(&escape_html(&resolve_path(img.path.trim())));
                out.push_str("\" alt=\"");
                out.push_str(&escape_html(el.text.trim()));
                out.push_str("\" style=\"width:");
                out.push_str(&w.to_string());
                out.push_str("%\">");
                if !el.text.trim().is_empty() {
                    out.push_str("<figcaption>");
                    out.push_str(&escape_html(el.text.trim()));
                    out.push_str("</figcaption>");
                }
                out.push_str("</figure>\n");
            }
            ElementKind::Table(ref t) => {
                let n = t.columns();
                out.push_str("<table>\n");
                // HTML is the ONE backend of the four that has a `th`.
                // That is exactly why header-ness is a property of the
                // table rather than a cell type in the tree: put `th`
                // in the tree and three formats inherit a concept they
                // cannot express.
                if !t.header.is_empty() {
                    out.push_str("<tr>");
                    for i in 0..n {
                        out.push_str("<th>");
                        out.push_str(&escape_html(
                            t.header.get(i).map(String::as_str).unwrap_or(""),
                        ));
                        out.push_str("</th>");
                    }
                    out.push_str("</tr>\n");
                }
                for row in &t.rows {
                    out.push_str("<tr>");
                    for i in 0..n {
                        let a = match t.align_of(i) {
                            Align::Right => " style=\"text-align:right\"",
                            Align::Left => "",
                        };
                        out.push_str(&format!("<td{a}>"));
                        out.push_str(&escape_html(row.get(i).map(String::as_str).unwrap_or("")));
                        out.push_str("</td>");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
                if !el.text.trim().is_empty() {
                    out.push_str("<p class=\"caption\"><em>");
                    out.push_str(&escape_html(el.text.trim()));
                    out.push_str("</em></p>\n");
                }
            }
        }
    }
    out.push_str("</body></html>\n");
    out
}

/// HTML is the only one of the four that can carry a colour or a border,
/// so this is where `Style` finally means something. The other emitters
/// drop it rather than approximate it.
fn style_attr(s: &Style) -> String {
    if s.is_plain() {
        return String::new();
    }
    let mut css = String::new();
    if let Some(c) = &s.color {
        css.push_str(&format!("color:{};", escape_html(c)));
    }
    if let Some(c) = &s.bg_color {
        css.push_str(&format!("background-color:{};", escape_html(c)));
    }
    if let Some(b) = &s.border {
        css.push_str(&format!("border:{};", escape_html(b)));
    }
    if s.bold {
        css.push_str("font-weight:bold;");
    }
    if s.italic {
        css.push_str("font-style:italic;");
    }
    format!(" style=\"{css}\"")
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn tex_row(cells: &[String], n: usize) -> String {
    let padded: Vec<String> =
        (0..n).map(|i| escape_tex(cells.get(i).map(String::as_str).unwrap_or(""))).collect();
    format!("{} \\\\\n", padded.join(" & "))
}

/// The characters that would otherwise be markup. Not applied to a `Tex`
/// element: that one IS LaTeX, and escaping it would turn a formula into
/// a printed backslash.
fn escape_tex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}

fn inline_tex(el: &Element) -> String {
    let mut s = escape_tex(&el.text);
    if el.style.bold {
        s = format!("\\textbf{{{s}}}");
    }
    if el.style.italic {
        s = format!("\\textit{{{s}}}");
    }
    s
}

fn inline_md(el: &Element) -> String {
    let mut s = el.text.clone();
    if el.style.bold {
        s = format!("**{s}**");
    }
    if el.style.italic {
        s = format!("*{s}*");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_headings_and_paragraphs_in_order() {
        let mut d = ReportDoc::new("Fit report");
        d.push(ElementKind::Heading(1), "Summary");
        d.push(ElementKind::P, "The model converged.");
        d.push(ElementKind::Heading(2), "Parameters");
        let md = to_markdown(&d);
        assert!(md.starts_with("# Fit report\n\n"), "got: {md}");
        let h1 = md.find("# Summary").expect("h1");
        let p = md.find("The model converged.").expect("p");
        let h2 = md.find("## Parameters").expect("h2");
        assert!(h1 < p && p < h2, "elements must emit in insertion order: {md}");
    }

    /// The module's central claim, pinned. If math is ever stored with
    /// dollars, the HTML and Word emitters inherit delimiters they cannot
    /// use.
    #[test]
    fn math_is_stored_without_delimiters_and_wrapped_by_the_emitter() {
        let mut d = ReportDoc::new("");
        let i = d.push(ElementKind::Tex, "chi^2 = 1.4");
        assert_eq!(d.elements[i].text, "chi^2 = 1.4", "stored math must be bare");
        assert!(to_markdown(&d).contains("$$\nchi^2 = 1.4\n$$"), "emitter adds the dollars");
    }

    /// A handle keeps pointing at its element after later appends -- the
    /// whole reason a captured span can be restyled afterwards.
    #[test]
    fn a_handle_survives_later_appends() {
        let mut d = ReportDoc::new("t");
        let first = d.push(ElementKind::P, "one");
        d.push(ElementKind::P, "two");
        d.elements[first].style.bold = true;
        assert_eq!(d.elements[first].text, "one");
        let md = to_markdown(&d);
        assert!(md.contains("**one**") && md.contains("two"), "got: {md}");
    }

    #[test]
    fn an_unstyled_element_emits_as_plain_text() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::P, "plain");
        assert!(Style::default().is_plain());
        let md = to_markdown(&d);
        assert!(md.contains("plain") && !md.contains("**"), "got: {md}");
    }

    /// Ahmed's own question, pinned as a test: the SAME element must come
    /// out as `# ` in Markdown and `\section{}` in LaTeX. If these ever
    /// agree textually, the tree has stopped being semantic and started
    /// storing one format's markup.
    #[test]
    fn one_heading_renders_in_each_formats_own_idiom() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Heading(1), "Results");
        d.push(ElementKind::Heading(3), "Detail");
        let md = to_markdown(&d);
        let tex = to_latex(&d);
        assert!(md.contains("# Results"), "markdown h1: {md}");
        assert!(md.contains("### Detail"), "markdown h3: {md}");
        assert!(tex.contains("\\section{Results}"), "latex h1: {tex}");
        assert!(tex.contains("\\subsubsection{Detail}"), "latex h3: {tex}");
        assert!(!tex.contains("# Results"), "latex must not carry markdown: {tex}");
    }

    /// LaTeX runs out of sectioning commands at level 3; deeper headings
    /// have to land on paragraph/subparagraph rather than silently
    /// vanishing or emitting a command that does not exist.
    #[test]
    fn latex_maps_deep_headings_onto_the_commands_it_has() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Heading(4), "Deep");
        d.push(ElementKind::Heading(9), "Deeper than exists");
        let tex = to_latex(&d);
        assert!(tex.contains("\\paragraph{Deep}"), "got: {tex}");
        assert!(tex.contains("\\subparagraph{"), "level 9 clamps to 6: {tex}");
    }

    /// Prose is escaped so a stray underscore cannot become markup, but a
    /// Tex element is NOT -- escaping it would print the backslashes of
    /// the formula instead of rendering it.
    #[test]
    fn prose_is_escaped_for_latex_but_math_is_not() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::P, "50% of R_ct & C_dl");
        d.push(ElementKind::Tex, "\\frac{1}{j\\omega C}");
        let tex = to_latex(&d);
        assert!(tex.contains("50\\% of R\\_ct \\& C\\_dl"), "prose must be escaped: {tex}");
        assert!(tex.contains("\\frac{1}{j\\omega C}"), "math must pass through: {tex}");
    }

    fn demo_table() -> TableData {
        TableData {
            header: vec!["parameter".into(), "value".into()],
            rows: vec![
                vec!["R_s".into(), "12.4".into()],
                vec!["C_dl".into(), "3.1e-5".into()],
            ],
            align: vec![Align::Left, Align::Right],
            label: None,
        }
    }

    /// One table, two idioms -- the same claim as the heading test, which
    /// is what stops a table node from quietly becoming stored markup.
    #[test]
    fn one_table_renders_in_each_formats_own_idiom() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Table(demo_table()), "");
        let md = to_markdown(&d);
        let tex = to_latex(&d);
        assert!(md.contains("| parameter | value |"), "markdown header: {md}");
        assert!(md.contains("| --- | ---: |"), "markdown alignment row: {md}");
        assert!(tex.contains("\\begin{tabular}{lr}"), "latex alignment spec: {tex}");
        assert!(tex.contains("parameter & value \\\\"), "latex header: {tex}");
        assert!(!tex.contains("| parameter"), "latex must not carry markdown: {tex}");
    }

    /// Alignment is decided once, in the tree, and spelled differently by
    /// each backend -- `---:` and `r` are the same fact.
    #[test]
    fn alignment_is_one_decision_spelled_two_ways() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Table(demo_table()), "");
        assert!(to_markdown(&d).contains("---:"), "right-aligned column in markdown");
        assert!(to_latex(&d).contains("{lr}"), "same column right-aligned in latex");
    }

    /// LaTeX escaping reaches inside cells. `R_s` unescaped is a hard
    /// LaTeX error inside a tabular, not a cosmetic one.
    #[test]
    fn table_cells_are_escaped_for_latex() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Table(demo_table()), "");
        let tex = to_latex(&d);
        assert!(tex.contains("R\\_s"), "underscore in a cell must be escaped: {tex}");
    }

    /// A pipe inside a cell ends the cell early in Markdown -- the exact
    /// defect that cut this project's own generated tables in half.
    #[test]
    fn a_pipe_in_a_cell_does_not_end_the_row() {
        let mut d = ReportDoc::new("");
        d.push(
            ElementKind::Table(TableData {
                header: vec!["expr".into()],
                rows: vec![vec!["a|b".into()]],
                align: vec![],
                label: None,
            }),
            "",
        );
        let md = to_markdown(&d);
        assert!(md.contains("a\\|b"), "pipe must be escaped: {md}");
    }

    /// Ragged input is padded, not rejected: every backend needs a
    /// rectangle, and dropping a cell silently is worse than an empty one.
    #[test]
    fn a_ragged_table_is_padded_to_a_rectangle() {
        let mut d = ReportDoc::new("");
        d.push(
            ElementKind::Table(TableData {
                header: vec!["a".into(), "b".into(), "c".into()],
                rows: vec![vec!["1".into()]],
                align: vec![],
                label: None,
            }),
            "",
        );
        let md = to_markdown(&d);
        let row = md.lines().find(|l| l.starts_with("| 1")).expect("data row");
        assert_eq!(row.matches('|').count(), 4, "3 columns means 4 pipes: {row}");
    }

    /// The third idiom for one element, completing Ahmed's own sentence:
    /// `# ` in Markdown, `\section{}` in LaTeX, `<h1>` in HTML.
    #[test]
    fn a_heading_is_a_third_thing_again_in_html() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Heading(1), "Summary");
        d.push(ElementKind::Heading(4), "Deep");
        let html = to_html(&d, MathJax::Off);
        assert!(html.contains("<h1>Summary</h1>"), "html h1: {html}");
        // HTML has six levels where LaTeX had three, so level 4 is a real
        // <h4> here and a \paragraph there -- the same fact, two limits.
        assert!(html.contains("<h4>Deep</h4>"), "html h4: {html}");
    }

    /// Prose is escaped, math is not -- the same asymmetry as LaTeX, and
    /// the reason it is worth a test in each format is that getting it
    /// backwards produces a document that renders, wrongly.
    #[test]
    fn html_escapes_prose_but_not_math() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::P, "a < b & c");
        d.push(ElementKind::Tex, "\\frac{a}{b}");
        let html = to_html(&d, MathJax::Off);
        assert!(html.contains("a &lt; b &amp; c"), "prose escaped: {html}");
        assert!(html.contains("\\[\\frac{a}{b}\\]"), "math untouched: {html}");
    }

    /// The network is opt-in. A report that only renders its equations
    /// when the reader is online contradicts how the rest of this project
    /// treats external dependencies, so the default emits delimiters and
    /// no script.
    #[test]
    fn mathjax_is_off_by_default_and_opt_in() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Tex, "x^2");
        assert!(!to_html(&d, MathJax::Off).contains("cdn.jsdelivr.net"), "no network by default");
        assert!(to_html(&d, MathJax::Cdn).contains("cdn.jsdelivr.net"), "opt-in works");
        // Delimiters are there either way: they cost nothing and any
        // renderer, vendored or otherwise, can find them later.
        assert!(to_html(&d, MathJax::Off).contains("\\[x^2\\]"));
    }

    /// Style is the one thing HTML can express and the others cannot.
    /// Asserted in both directions so "lossy elsewhere" stays deliberate.
    #[test]
    fn style_reaches_html_and_is_dropped_by_the_others() {
        let mut d = ReportDoc::new("");
        let i = d.push(ElementKind::Span, "warn");
        d.elements[i].style.color = Some("crimson".into());
        d.elements[i].style.bg_color = Some("#fee".into());
        let html = to_html(&d, MathJax::Off);
        assert!(html.contains("color:crimson;"), "html carries colour: {html}");
        assert!(html.contains("background-color:#fee;"), "and background: {html}");
        assert!(!to_markdown(&d).contains("crimson"), "markdown has nowhere to put it");
        assert!(!to_latex(&d).contains("crimson"), "nor latex");
    }

    /// A title is optional: an untitled document must not emit a stray
    /// empty heading, which every downstream format would then carry.
    #[test]
    fn an_empty_title_emits_no_heading() {
        let mut d = ReportDoc::new("   ");
        d.push(ElementKind::P, "body");
        let md = to_markdown(&d);
        assert!(!md.starts_with('#'), "got: {md}");
    }

    /// A figure is a real `figure` float with a width, not a bare
    /// `\includegraphics{}` at native size -- the whole reason `ImgData`
    /// carries a width is that a savefig'd figure's native size is
    /// column-breakingly large, so a report emitting one without a width
    /// argument would reproduce that exact failure at the LaTeX layer
    /// even though the plotting side has already been fixed for it.
    #[test]
    fn a_figure_is_a_real_float_with_a_width_caption_and_label() {
        let mut d = ReportDoc::new("");
        d.push(
            ElementKind::Img(ImgData { path: "fig1.pdf".into(), width: Some(0.6), label: Some("fig:one".into()) }),
            "A test figure",
        );
        let tex = to_latex(&d);
        assert!(tex.contains("\\begin{figure}[htbp]"), "got: {tex}");
        assert!(tex.contains("\\includegraphics[width=0.6\\linewidth]{fig1.pdf}"), "got: {tex}");
        assert!(tex.contains("\\caption{A test figure}"), "got: {tex}");
        assert!(tex.contains("\\label{fig:one}"), "got: {tex}");
        assert!(tex.contains("\\end{figure}"), "got: {tex}");
    }

    /// No explicit width falls back to a real default rather than an
    /// empty `[width=]` (a LaTeX error) or bare `\includegraphics{}`
    /// (native size, the thing this struct exists to prevent).
    #[test]
    fn a_figure_with_no_explicit_width_still_gets_one() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Img(ImgData { path: "fig1.pdf".into(), width: None, label: None }), "");
        let tex = to_latex(&d);
        assert!(tex.contains("\\includegraphics[width=0.8\\linewidth]"), "got: {tex}");
    }

    /// A table with a caption or label becomes a real `table` float, not
    /// a `tabular` with a caption paragraph dangling after it -- the
    /// caption has to be INSIDE the float and ABOVE the tabular (table
    /// convention, the opposite of a figure's) for `\ref{}` and
    /// automatic numbering to work at all.
    #[test]
    fn a_captioned_table_is_a_real_float_with_the_caption_above_the_tabular() {
        let mut d = ReportDoc::new("");
        d.push(
            ElementKind::Table(TableData {
                header: vec!["a".into()],
                rows: vec![vec!["1".into()]],
                align: vec![],
                label: Some("tab:one".into()),
            }),
            "A caption",
        );
        let tex = to_latex(&d);
        assert!(tex.contains("\\begin{table}[htbp]"), "got: {tex}");
        let cap = tex.find("\\caption{A caption}").expect("caption");
        let tab = tex.find("\\begin{tabular}").expect("tabular");
        assert!(cap < tab, "caption must precede the tabular: {tex}");
        assert!(tex.contains("\\label{tab:one}"), "got: {tex}");
        assert!(tex.contains("\\end{table}"), "got: {tex}");
    }

    /// A table with neither caption nor label stays a bare `tabular`,
    /// unchanged from before floats existed -- `demo_table()`'s own
    /// existing tests pin exactly this, so this test only has to state
    /// the boundary explicitly: no wrapper is added on their behalf.
    #[test]
    fn an_uncaptioned_unlabeled_table_stays_a_bare_tabular() {
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Table(demo_table()), "");
        let tex = to_latex(&d);
        assert!(!tex.contains("\\begin{table}"), "got: {tex}");
    }

    /// The bug a real paper's figure-and-table section found immediately:
    /// a report built from one directory and compiled from another
    /// (`\input`-ed into a document living somewhere else, the normal
    /// case) silently dropped the figure, because the path was correct
    /// from the SCRIPT's directory and meaningless from the DOCUMENT's.
    #[test]
    fn an_image_path_is_relativized_against_the_documents_own_directory() {
        use std::path::Path;
        let mut d = ReportDoc::new("");
        d.push(
            ElementKind::Img(ImgData {
                path: "/repo/papers/why-cf-matters/img/fig.pdf".into(),
                width: None,
                label: None,
            }),
            "",
        );
        let tex = to_latex_with_base(&d, Some(Path::new("/repo/specimen_test")));
        assert!(
            tex.contains("{../papers/why-cf-matters/img/fig.pdf}"),
            "got: {tex}"
        );
        // Unchanged when no base is given -- every caller before this
        // feature existed, and every other test in this file, gets the
        // path exactly as written.
        let tex_no_base = to_latex(&d);
        assert!(tex_no_base.contains("{/repo/papers/why-cf-matters/img/fig.pdf}"), "got: {tex_no_base}");
    }

    /// Same document, same directory: the relative path is `.` worth of
    /// nothing, i.e. just the filename, not `../reportdir/fig.pdf`.
    #[test]
    fn an_image_in_the_same_directory_as_the_document_needs_no_prefix() {
        use std::path::Path;
        let mut d = ReportDoc::new("");
        d.push(ElementKind::Img(ImgData { path: "/repo/out/fig.pdf".into(), width: None, label: None }), "");
        let tex = to_latex_with_base(&d, Some(Path::new("/repo/out")));
        assert!(tex.contains("{fig.pdf}"), "got: {tex}");
    }
}
