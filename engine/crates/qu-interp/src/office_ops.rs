//! `import docx`, `import pptx`, and the workbook half of `import xlsx`
//! (see `docs/design/toolkit-office.md`).
//!
//! An opened document is a HANDLE: `doc = docx.open("paper.docx")` returns
//! a small `Model` value naming a slot in this interpreter's
//! `office_handles`, and every edit (`docx.replace_text(doc, ...)`) changes
//! the document behind it until `docx.save_as(doc, path)`. That is file-
//! handle semantics, deliberately: an Office document is a mutable thing
//! with identity, and copying the whole package into a new value on every
//! edit would be both slow and a lie about what is happening. `discard`
//! releases a slot early; otherwise documents live until the run ends.
//!
//! Names are chosen never to coincide with a builtin (`save_as`, not
//! `save`; `get_cell`, not `get`): `import` opens a module's names bare,
//! and a bare name that is also a builtin becomes an ambiguity error for
//! every later call of that builtin -- see `open_module_name`.
//!
//! Lengths (`x=`, `y=`, `w=`, `h=`, `width=`, `height=`) take millimetres
//! as plain numbers or any length quantity (`20 mm`, `2 cm`, `0.5 m`);
//! font sizes are points.

use crate::{arg_get, display_value, e, style_entry, style_num, style_str, table, text_arg, truthy, Dim, EvalError, Interp, ModelHandle, UnitTag, Value, R};
use std::sync::Arc;

pub const DOCX_NAMES: &[&str] = &[
    "new", "open", "save_as", "discard", "full_text", "paragraphs", "headings", "find_text", "replace_text", "set_paragraph",
    "insert_paragraph", "remove_paragraph", "add_heading", "add_paragraph", "add_page_break", "add_table", "add_image", "tables",
    "set_cell", "comments", "footnotes", "endnotes", "info", "set_info", "accept_changes", "reject_changes", "to_markdown", "to_latex",
    "to_pdf", "links", "add_link", "add_comment", "bookmarks", "add_bookmark", "add_cross_ref", "fields", "add_field", "add_toc",
    "sections", "page_setup", "add_section_break", "header_text", "footer_text", "set_header", "set_footer", "add_list_item", "set_list",
    "add_column", "add_footnote", "add_row", "format_text", "line_spacing", "merge_cells", "move_paragraph", "split_cell", "track_delete",
    "track_insert",
];

pub const PPTX_NAMES: &[&str] = &[
    "new", "open", "save_as", "discard", "info", "set_info", "slide_count", "slides", "slide_text", "slide_title", "notes", "find_text",
    "replace_text", "layouts", "add_slide", "delete_slide", "move_slide", "duplicate_slide", "hide_slide", "unhide_slide", "add_text",
    "add_image", "add_table", "to_markdown", "to_pdf", "set_notes", "shapes", "set_shape_text", "delete_shape", "move_shape", "resize_shape",
    "set_slide_title", "bring_to_front", "send_to_back", "set_link", "links", "theme_colors", "theme_fonts", "set_theme_colors",
    "set_theme_fonts", "add_chart", "add_nyquist_chart", "add_shape", "align_shapes", "rotate_shape",
];

pub const XLSX_NAMES: &[&str] = &[
    "new", "open", "save_as", "discard", "get_cell", "set_cell", "formula", "set_formula", "fill_formula", "get_range", "set_range",
    "used_range", "add_sheet", "rename_sheet", "delete_sheet", "insert_rows", "delete_rows", "insert_columns", "delete_columns",
    "column_width", "row_height", "format_cells", "merge", "freeze_panes", "define_name", "to_pdf", "add_chart", "add_nyquist_chart",
    "conditional_format", "color_scale", "add_validation", "copy_sheet", "hide_sheet", "clear", "move_range", "sort_range", "autofilter",
    "create_table", "add_comment", "add_image",
];

const LENGTH: Dim = Dim([0, 1, 0, 0, 0, 0, 0]);

/// A length keyword in millimetres: a plain number, or a length quantity.
fn mm_kw(style: &[(String, Value)], key: &str, f: &str) -> R<Option<f64>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, v)) => mm_value(v, &format!("{f}: `{key}=`")).map(Some),
    }
}

fn mm_value(v: &Value, what: &str) -> R<f64> {
    match v {
        Value::Num(n) if n.is_finite() => Ok(*n),
        // A scalar quantity (`6 cm`) is a `Unit`, SI-normalised to metres.
        Value::Unit(m, UnitTag::Dim(d, _)) if *d == LENGTH => Ok(m * 1000.0),
        Value::Quantity(inner, UnitTag::Dim(d, _)) if *d == LENGTH => match **inner {
            Value::Num(m) => Ok(m * 1000.0),
            _ => e(format!("{what} takes one length, not a collection")),
        },
        Value::Quantity(_, _) | Value::Unit(_, _) => e(format!("{what} is a length -- give millimetres or a length like `20 mm`")),
        other => e(format!("{what} takes a length, found {}", other.type_name())),
    }
}

fn kw_bool(style: &[(String, Value)], key: &str) -> Option<bool> {
    style_entry(style, key).map(|(_, v)| truthy(v))
}

#[cfg(feature = "docx")]
/// A text keyword; anything but a string is an error, not ignored.
fn kw_text(style: &[(String, Value)], key: &str, f: &str) -> R<Option<String>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, Value::Str(s))) => Ok(Some(s.clone())),
        Some((_, v)) => e(format!("{f}: `{key}=` takes a string, found {}", v.type_name())),
    }
}

#[cfg(feature = "docx")]
/// A non-negative whole-number keyword.
fn kw_index(style: &[(String, Value)], key: &str, f: &str) -> R<Option<usize>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, v)) => v.as_index().map(Some).map_err(|m| EvalError { msg: format!("{f}: `{key}=`: {m}") }),
    }
}

#[cfg(feature = "docx")]
/// A number keyword; anything but a number is an error, not ignored.
fn kw_num(style: &[(String, Value)], key: &str, f: &str) -> R<Option<f64>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, Value::Num(n))) if n.is_finite() => Ok(Some(*n)),
        Some((_, v)) => e(format!("{f}: `{key}=` takes a number, found {}", v.type_name())),
    }
}

#[cfg(feature = "docx")]
/// The row/column values of `add_row`/`add_column`: a list or vector, one
/// item per cell, or nothing for a blank row/column.
fn cell_values(args: &[Value], i: usize, f: &str) -> R<Vec<String>> {
    match arg_get(args, i) {
        None | Some(Value::Nothing) => Ok(Vec::new()),
        Some(Value::List(items)) => Ok(items.iter().map(cell_text).collect()),
        Some(Value::Vec(xs)) => Ok(xs.iter().map(|x| display_value(&Value::Num(*x))).collect()),
        Some(other) => e(format!("{f}: the values come as a list, one per cell, found {}", other.type_name())),
    }
}

/// `size=` for page setup: a name (`"A4"`, `"Letter"`) or `[width, height]`
/// in millimetres or lengths.
#[cfg(feature = "docx")]
fn page_size_kw(style: &[(String, Value)], f: &str) -> R<Option<(f64, f64)>> {
    let Some((_, v)) = style_entry(style, "size") else { return Ok(None) };
    let what = format!("{f}: `size=`");
    match v {
        Value::Str(s) => qu_docx::page_size_named(s)
            .map(Some)
            .ok_or_else(|| EvalError { msg: format!("{what} \"{s}\" -- use {} or [width, height]", qu_docx::page_size_names()) }),
        Value::Vec(xs) if xs.len() == 2 => Ok(Some((xs[0], xs[1]))),
        Value::List(xs) if xs.len() == 2 => Ok(Some((mm_value(&xs[0], &what)?, mm_value(&xs[1], &what)?))),
        Value::Quantity(inner, UnitTag::Dim(d, _)) if *d == LENGTH => match &**inner {
            Value::Vec(xs) if xs.len() == 2 => Ok(Some((xs[0] * 1000.0, xs[1] * 1000.0))),
            _ => e(format!("{what} takes a page name or [width, height]")),
        },
        other => e(format!("{what} takes a page name or [width, height], found {}", other.type_name())),
    }
}

fn index_arg(args: &[Value], i: usize, f: &str, what: &str) -> R<usize> {
    arg_get(args, i)
        .ok_or_else(|| EvalError { msg: format!("{f}: missing {what}") })?
        .as_index()
        .map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") })
}

fn record(pairs: Vec<(String, String)>) -> Value {
    Value::Record(Arc::new(
        pairs
            .into_iter()
            .map(|(k, v)| {
                // Counts come back as numbers; everything else (dates,
                // "revision" strings, titles) stays text.
                let val = match v.parse::<f64>() {
                    Ok(n) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') => Value::Num(n),
                    _ => Value::Str(v),
                };
                (k, val)
            })
            .collect(),
    ))
}

fn strs(v: Vec<String>) -> Value {
    Value::List(Arc::new(v.into_iter().map(Value::Str).collect()))
}

fn nums(v: Vec<usize>) -> Value {
    Value::Vec(Arc::new(v.into_iter().map(|i| i as f64).collect()))
}

/// Table-shaped input as rows of display strings: a `Table` (its column
/// names become the first row), a matrix, a list of lists, or a vector (one
/// column). Returns (rows, had_header_row).
fn text_rows(v: &Value, f: &str) -> R<(Vec<Vec<String>>, bool)> {
    match v {
        Value::Table(t) => {
            let names: Vec<String> = t.column_names().iter().map(|s| s.to_string()).collect();
            let cols: Vec<Vec<String>> = names.iter().map(|n| t.col_as_strings(n)).collect::<Result<_, _>>().map_err(|msg| EvalError { msg })?;
            let mut rows = vec![names];
            for r in 0..t.nrows() {
                rows.push(cols.iter().map(|c| c[r].clone()).collect());
            }
            Ok((rows, true))
        }
        Value::Mat(m) => {
            let (r, c) = m.shape();
            Ok(((0..r).map(|i| (0..c).map(|j| display_value(&Value::Num(m.get(i, j).unwrap_or(f64::NAN)))).collect()).collect(), false))
        }
        Value::Vec(xs) => Ok((xs.iter().map(|x| vec![display_value(&Value::Num(*x))]).collect(), false)),
        Value::List(rows) => Ok((
            rows.iter()
                .map(|r| match r {
                    Value::List(cells) => cells.iter().map(cell_text).collect(),
                    Value::Vec(xs) => xs.iter().map(|x| display_value(&Value::Num(*x))).collect(),
                    other => vec![cell_text(other)],
                })
                .collect(),
            false,
        )),
        other => e(format!("{f}: expected a table, matrix or list of rows, found {}", other.type_name())),
    }
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Nothing => String::new(),
        other => display_value(other),
    }
}

fn read_bytes(v: &Value, f: &str) -> R<Vec<u8>> {
    match v {
        Value::Str(path) => std::fs::read(path).map_err(|err| EvalError { msg: format!("{f}: could not read `{path}`: {err}") }),
        Value::Vec(xs) => xs.iter().map(|&b| if (0.0..=255.0).contains(&b) && b.fract() == 0.0 { Ok(b as u8) } else { e(format!("{f}: a byte vector holds whole numbers 0-255, found {b}")) }).collect(),
        other => e(format!("{f}: expected an image path or bytes, found {}", other.type_name())),
    }
}

fn write_pdf(bytes: &[u8], ext: &str, out: &str, style: &[(String, Value)], f: &str) -> R<Value> {
    let timeout = style_num(style, "timeout").unwrap_or(300.0);
    let pdf = qu_ooxml::convert_with_office(bytes, ext, "pdf", timeout.max(1.0) as u64).map_err(|msg| EvalError { msg: format!("{f}: {msg}") })?;
    std::fs::write(out, &pdf).map_err(|err| EvalError { msg: format!("{f}: could not write `{out}`: {err}") })?;
    Ok(Value::Num(pdf.len() as f64))
}

impl Interp {
    fn office_store<T: std::any::Any + Send>(&mut self, kind: &str, obj: T, path: &str) -> Value {
        let id = match self.office_handles.iter().position(|s| s.is_none()) {
            Some(i) => {
                self.office_handles[i] = Some(Box::new(obj));
                i
            }
            None => {
                self.office_handles.push(Some(Box::new(obj)));
                self.office_handles.len() - 1
            }
        };
        Value::Model(Arc::new(ModelHandle::new(kind, vec![("id".into(), Value::Num(id as f64)), ("path".into(), Value::Str(path.to_string()))])))
    }

    fn office_slot(&self, v: Option<&Value>, kind: &str, f: &str) -> R<usize> {
        let noun = match kind {
            "docx" => "a docx.open/docx.new document",
            "pptx" => "a pptx.open/pptx.new presentation",
            _ => "an xlsx.open/xlsx.new workbook",
        };
        match v {
            Some(Value::Model(m)) if m.kind == kind => match m.field("id") {
                Some(Value::Num(n)) if (*n as usize) < self.office_handles.len() && self.office_handles[*n as usize].is_some() => Ok(*n as usize),
                _ => e(format!("{f}: this {kind} handle was discarded")),
            },
            Some(other) => e(format!("{f}: the first argument must be {noun}, found {}", other.type_name())),
            None => e(format!("{f}: needs {noun} as its first argument")),
        }
    }

    fn office_mut<T: std::any::Any + Send>(&mut self, v: Option<&Value>, kind: &str, f: &str) -> R<&mut T> {
        let i = self.office_slot(v, kind, f)?;
        Ok(self.office_handles[i].as_mut().unwrap().downcast_mut::<T>().expect("slot kind matches handle kind"))
    }

    pub(crate) fn office_call(&mut self, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        let (module, name) = f.split_once("::").unwrap_or(("", f));
        let fname = format!("{module}.{name}");
        let fq = fname.as_str();
        if name == "discard" {
            let i = self.office_slot(args.first(), module_kind(module), fq)?;
            self.office_handles[i] = None;
            return Ok(Value::Nothing);
        }
        match module {
            #[cfg(feature = "docx")]
            "docx" => self.docx_call(name, fq, args, style),
            #[cfg(feature = "pptx")]
            "pptx" => self.pptx_call(name, fq, args, style),
            #[cfg(feature = "xlsx")]
            "xlsx" => self.workbook_call(name, fq, args, style),
            _ => e(format!("{fq}: this build of Qu was compiled without the `{module}` module")),
        }
    }

    // ------------------------------------------------------------ docx

    #[cfg(feature = "docx")]
    fn docx_call(&mut self, name: &str, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        use qu_docx::{Document, Format};
        let err = |msg: String| EvalError { msg: format!("{f}: {msg}") };
        match name {
            "new" => Ok(self.office_store("docx", Document::new(), "")),
            "open" => {
                let path = text_arg(args, 0)?;
                let d = Document::open(&path).map_err(err)?;
                Ok(self.office_store("docx", d, &path))
            }
            _ => {
                let d: &mut Document = self.office_mut(args.first(), "docx", f)?;
                let fmt = || -> R<Format> {
                    Ok(Format {
                        bold: kw_bool(style, "bold").unwrap_or(false),
                        italic: kw_bool(style, "italic").unwrap_or(false),
                        underline: kw_bool(style, "underline").unwrap_or(false),
                        size: style_num(style, "size"),
                        font: style_str(style, "font"),
                        color: style_str(style, "color"),
                        align: style_str(style, "align"),
                        style: style_str(style, "style"),
                    })
                };
                match name {
                    "save_as" => {
                        d.save(&text_arg(args, 1)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "full_text" => Ok(Value::Str(d.text())),
                    "paragraphs" => Ok(strs(d.paragraphs())),
                    "headings" => Ok(Value::List(Arc::new(
                        d.headings().into_iter().map(|(l, t)| Value::Record(Arc::new(vec![("level".into(), Value::Num(l as f64)), ("text".into(), Value::Str(t))]))).collect(),
                    ))),
                    "find_text" => Ok(nums(d.find_text(&text_arg(args, 1)?))),
                    "replace_text" => Ok(Value::Num(d.replace_text(&text_arg(args, 1)?, &text_arg(args, 2)?).map_err(err)? as f64)),
                    "set_paragraph" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        d.set_paragraph(i, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "insert_paragraph" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let text = text_arg(args, 2)?;
                        d.insert_paragraph(i, &text, &fmt()?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "remove_paragraph" => {
                        d.remove_paragraph(index_arg(args, 1, f, "paragraph index")?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_heading" => {
                        let level = match arg_get(args, 2) {
                            Some(v) => v.as_index().map_err(|m| err(format!("level: {m}")))?,
                            None => 1,
                        };
                        d.add_heading(&text_arg(args, 1)?, level as u32).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_paragraph" => {
                        let text = text_arg(args, 1)?;
                        d.add_paragraph(&text, &fmt()?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_page_break" => {
                        d.add_page_break();
                        Ok(Value::Nothing)
                    }
                    "add_table" => {
                        let (rows, had_header) = text_rows(arg_get(args, 1).ok_or_else(|| err("needs the table data".into()))?, f)?;
                        d.add_table(&rows, kw_bool(style, "header").unwrap_or(had_header)).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_image" => {
                        let w = mm_kw(style, "width", f)?;
                        let h = mm_kw(style, "height", f)?;
                        let bytes = read_bytes(arg_get(args, 1).ok_or_else(|| err("needs an image path".into()))?, f)?;
                        d.add_image(&bytes, w, h).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "tables" => Ok(Value::List(Arc::new(
                        d.tables().into_iter().map(|t| Value::List(Arc::new(t.into_iter().map(strs).collect()))).collect(),
                    ))),
                    "set_cell" => {
                        let t = index_arg(args, 1, f, "table index")?;
                        let r = index_arg(args, 2, f, "row")?;
                        let c = index_arg(args, 3, f, "column")?;
                        d.set_cell(t, r, c, &cell_text(arg_get(args, 4).ok_or_else(|| err("needs the new text".into()))?)).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "comments" => Ok(Value::List(Arc::new(
                        d.comments()
                            .into_iter()
                            .map(|(a, dt, t)| Value::Record(Arc::new(vec![("author".into(), Value::Str(a)), ("date".into(), Value::Str(dt)), ("text".into(), Value::Str(t))])))
                            .collect(),
                    ))),
                    "footnotes" => Ok(strs(d.notes("footnotes"))),
                    "endnotes" => Ok(strs(d.notes("endnotes"))),
                    "info" => Ok(record(d.info())),
                    "set_info" => {
                        let props = info_props(style, f)?;
                        let refs: Vec<(&str, &str)> = props.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                        d.set_properties(&refs).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "accept_changes" => Ok(Value::Num(d.resolve_changes(true).map_err(err)? as f64)),
                    "reject_changes" => Ok(Value::Num(d.resolve_changes(false).map_err(err)? as f64)),
                    "to_markdown" => Ok(Value::Str(d.to_markdown())),
                    "to_latex" => {
                        let full = kw_bool(style, "full").unwrap_or(true);
                        let (tex, files) = d.to_latex(full);
                        if let Some(path) = arg_get(args, 1).map(cell_text) {
                            let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
                            for (rel, bytes) in &files {
                                let target = dir.join(rel);
                                if let Some(p) = target.parent() {
                                    std::fs::create_dir_all(p).map_err(|er| err(format!("could not create `{}`: {er}", p.display())))?;
                                }
                                std::fs::write(&target, bytes).map_err(|er| err(format!("could not write `{}`: {er}", target.display())))?;
                            }
                            std::fs::write(&path, &tex).map_err(|er| err(format!("could not write `{path}`: {er}")))?;
                        }
                        Ok(Value::Str(tex))
                    }
                    "to_pdf" => {
                        let out = text_arg(args, 1)?;
                        let bytes = d.to_bytes().map_err(err)?;
                        write_pdf(&bytes, "docx", &out, style, f)
                    }
                    // ---- structure: links, comments, bookmarks, fields
                    "links" => Ok(Value::List(Arc::new(
                        d.links().into_iter().map(|(t, u)| Value::Record(Arc::new(vec![("text".into(), Value::Str(t)), ("url".into(), Value::Str(u))]))).collect(),
                    ))),
                    "add_link" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let url = text_arg(args, 2)?;
                        let on = kw_text(style, "on", f)?;
                        d.add_link(i, &url, on.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_comment" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let text = text_arg(args, 2)?;
                        let on = kw_text(style, "on", f)?;
                        let author = kw_text(style, "author", f)?.unwrap_or_else(|| "Qu".into());
                        let initials = kw_text(style, "initials", f)?.unwrap_or_default();
                        let date = kw_text(style, "date", f)?;
                        d.add_comment(i, &text, on.as_deref(), &author, &initials, date.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "bookmarks" => Ok(Value::List(Arc::new(
                        d.bookmarks().into_iter().map(|(n, t)| Value::Record(Arc::new(vec![("name".into(), Value::Str(n)), ("text".into(), Value::Str(t))]))).collect(),
                    ))),
                    "add_bookmark" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let name = text_arg(args, 2)?;
                        let on = kw_text(style, "on", f)?;
                        d.add_bookmark(i, &name, on.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_cross_ref" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let name = text_arg(args, 2)?;
                        let show = kw_text(style, "show", f)?.unwrap_or_else(|| "text".into());
                        d.add_cross_ref(i, &name, &show).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "fields" => Ok(strs(d.fields())),
                    "add_field" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let code = text_arg(args, 2)?;
                        let shown = kw_text(style, "text", f)?.unwrap_or_default();
                        d.add_field(i, &code, &shown).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_toc" => {
                        let levels = kw_index(style, "levels", f)?.unwrap_or(3);
                        let at = kw_index(style, "at", f)?;
                        let title = kw_text(style, "title", f)?;
                        d.add_toc(levels.min(u32::MAX as usize) as u32, at, title.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    // ---- sections, page setup, headers and footers
                    "sections" => Ok(Value::List(Arc::new(
                        d.sections()
                            .into_iter()
                            .map(|s| {
                                Value::Record(Arc::new(vec![
                                    ("width".into(), Value::Num(s.width_mm)),
                                    ("height".into(), Value::Num(s.height_mm)),
                                    ("orientation".into(), Value::Str(if s.landscape { "landscape" } else { "portrait" }.into())),
                                    ("top".into(), Value::Num(s.top_mm)),
                                    ("bottom".into(), Value::Num(s.bottom_mm)),
                                    ("left".into(), Value::Num(s.left_mm)),
                                    ("right".into(), Value::Num(s.right_mm)),
                                    ("start".into(), Value::Str(s.start)),
                                    ("headers".into(), strs(s.headers)),
                                    ("footers".into(), strs(s.footers)),
                                ]))
                            })
                            .collect(),
                    ))),
                    "page_setup" => {
                        let section = kw_index(style, "section", f)?;
                        let size = page_size_kw(style, f)?;
                        let landscape = match kw_text(style, "orientation", f)?.as_deref() {
                            None => None,
                            Some("portrait") => Some(false),
                            Some("landscape") => Some(true),
                            Some(o) => return e(format!("{f}: orientation=\"{o}\" -- use portrait or landscape")),
                        };
                        let all = mm_kw(style, "margins", f)?;
                        let mut margins = [None; 4];
                        for (k, key) in ["top", "bottom", "left", "right"].iter().enumerate() {
                            margins[k] = mm_kw(style, key, f)?.or(all);
                        }
                        if size.is_none() && landscape.is_none() && margins.iter().all(Option::is_none) {
                            return e(format!("{f}: give at least one of size=, orientation=, margins=, top=, bottom=, left=, right="));
                        }
                        d.page_setup(section, size, landscape, margins).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_section_break" => {
                        let start = kw_text(style, "start", f)?.unwrap_or_else(|| "next_page".into());
                        d.add_section_break(&start).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "header_text" | "footer_text" => {
                        let section = kw_index(style, "section", f)?.unwrap_or(0);
                        let kind = kw_text(style, "kind", f)?.unwrap_or_else(|| "default".into());
                        Ok(d.story_text(name == "footer_text", section, &kind).map_err(err)?.map(Value::Str).unwrap_or(Value::Nothing))
                    }
                    "set_header" | "set_footer" => {
                        let text = text_arg(args, 1)?;
                        let section = kw_index(style, "section", f)?;
                        let kind = kw_text(style, "kind", f)?.unwrap_or_else(|| "default".into());
                        let align = kw_text(style, "align", f)?;
                        d.set_story(name == "set_footer", &text, section, &kind, align.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    // ---- lists
                    "add_list_item" | "set_list" => {
                        let kind = kw_text(style, "kind", f)?.unwrap_or_else(|| "bullet".into());
                        let level = kw_index(style, "level", f)?.unwrap_or(0);
                        let restart = kw_bool(style, "restart").unwrap_or(false);
                        let level = u32::try_from(level).unwrap_or(u32::MAX);
                        if name == "add_list_item" {
                            if kind == "none" {
                                return e(format!("{f}: kind=\"none\" makes a plain paragraph -- use docx.add_paragraph"));
                            }
                            d.add_list_item(&text_arg(args, 1)?, &kind, level, restart).map_err(err)?;
                        } else {
                            d.set_list(index_arg(args, 1, f, "paragraph index")?, &kind, level, restart).map_err(err)?;
                        }
                        Ok(Value::Nothing)
                    }
                    // ---- editing existing content: tables, formatting, notes, revisions
                    "add_row" | "add_column" => {
                        let t = index_arg(args, 1, f, "table index")?;
                        let values = cell_values(args, 2, f)?;
                        let at = kw_index(style, "at", f)?;
                        if name == "add_row" {
                            d.add_row(t, &values, at).map_err(err)?;
                        } else {
                            d.add_column(t, &values, at).map_err(err)?;
                        }
                        Ok(Value::Nothing)
                    }
                    "merge_cells" => {
                        let t = index_arg(args, 1, f, "table index")?;
                        let (r1, c1) = (index_arg(args, 2, f, "first row")?, index_arg(args, 3, f, "first column")?);
                        let (r2, c2) = (index_arg(args, 4, f, "last row")?, index_arg(args, 5, f, "last column")?);
                        d.merge_cells(t, r1, c1, r2, c2).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "split_cell" => {
                        let t = index_arg(args, 1, f, "table index")?;
                        let (row, col) = (index_arg(args, 2, f, "row")?, index_arg(args, 3, f, "column")?);
                        let cols = kw_index(style, "cols", f)?;
                        d.split_cell(t, row, col, cols).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "format_text" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let on = kw_text(style, "on", f)?;
                        let all = kw_bool(style, "all").unwrap_or(false);
                        let fmt = qu_docx::RunFormat {
                            bold: kw_bool(style, "bold"),
                            italic: kw_bool(style, "italic"),
                            underline: kw_bool(style, "underline"),
                            strike: kw_bool(style, "strike"),
                            size: kw_num(style, "size", f)?,
                            color: kw_text(style, "color", f)?,
                            font: kw_text(style, "font", f)?,
                        };
                        Ok(Value::Num(d.format_text(i, on.as_deref(), all, &fmt).map_err(err)? as f64))
                    }
                    "line_spacing" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let to = kw_index(style, "to", f)?;
                        let sp = qu_docx::Spacing {
                            lines: kw_num(style, "lines", f)?,
                            exactly: kw_num(style, "exactly", f)?,
                            at_least: kw_num(style, "at_least", f)?,
                            before: kw_num(style, "before", f)?,
                            after: kw_num(style, "after", f)?,
                        };
                        Ok(Value::Num(d.line_spacing(i, to, &sp).map_err(err)? as f64))
                    }
                    "move_paragraph" => {
                        let (from, to) = (index_arg(args, 1, f, "paragraph to move")?, index_arg(args, 2, f, "target position")?);
                        d.move_paragraph(from, to).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_footnote" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let text = text_arg(args, 2)?;
                        let on = kw_text(style, "on", f)?;
                        d.add_footnote(i, &text, on.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "track_insert" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let text = text_arg(args, 2)?;
                        let after = kw_text(style, "after", f)?;
                        let before = kw_text(style, "before", f)?;
                        let author = kw_text(style, "author", f)?.unwrap_or_else(|| "Qu".into());
                        let date = kw_text(style, "date", f)?;
                        d.track_insert(i, &text, after.as_deref(), before.as_deref(), &author, date.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "track_delete" => {
                        let i = index_arg(args, 1, f, "paragraph index")?;
                        let on = kw_text(style, "on", f)?;
                        let all = kw_bool(style, "all").unwrap_or(false);
                        let author = kw_text(style, "author", f)?.unwrap_or_else(|| "Qu".into());
                        let date = kw_text(style, "date", f)?;
                        Ok(Value::Num(d.track_delete(i, on.as_deref(), all, &author, date.as_deref()).map_err(err)? as f64))
                    }
                    other => e(format!("docx.{other} is not a docx function")),
                }
            }
        }
    }

    // ------------------------------------------------------------ pptx

    #[cfg(feature = "pptx")]
    fn pptx_call(&mut self, name: &str, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        use qu_pptx::{Order, Presentation, Rect, TextFormat, THEME_SLOTS};
        let err = |msg: String| EvalError { msg: format!("{f}: {msg}") };
        match name {
            "new" => Ok(self.office_store("pptx", Presentation::new(), "")),
            "open" => {
                let path = text_arg(args, 0)?;
                let p = Presentation::open(&path).map_err(err)?;
                Ok(self.office_store("pptx", p, &path))
            }
            _ => {
                let rect = || -> R<Rect> {
                    Ok(Rect {
                        x: mm_kw(style, "x", f)?.unwrap_or(0.0),
                        y: mm_kw(style, "y", f)?.unwrap_or(0.0),
                        w: mm_kw(style, "w", f)?.unwrap_or(0.0),
                        h: mm_kw(style, "h", f)?.unwrap_or(0.0),
                    })
                };
                let p: &mut Presentation = self.office_mut(args.first(), "pptx", f)?;
                let slide = |i: usize| index_arg(args, i, f, "slide index");
                match name {
                    "save_as" => {
                        p.save(&text_arg(args, 1)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "info" => Ok(record(p.info().map_err(err)?)),
                    "set_info" => {
                        let props = info_props(style, f)?;
                        let refs: Vec<(&str, &str)> = props.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                        p.set_properties(&refs).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "slide_count" => Ok(Value::Num(p.slide_count() as f64)),
                    "slides" => Ok(strs((0..p.slide_count()).map(|i| p.slide_text(i)).collect::<Result<_, _>>().map_err(err)?)),
                    "slide_text" => Ok(Value::Str(p.slide_text(slide(1)?).map_err(err)?)),
                    "slide_title" => Ok(p.slide_title(slide(1)?).map_err(err)?.map(Value::Str).unwrap_or(Value::Nothing)),
                    "notes" => Ok(Value::Str(p.notes(slide(1)?).map_err(err)?)),
                    "find_text" => Ok(nums(p.find_text(&text_arg(args, 1)?).map_err(err)?)),
                    "replace_text" => {
                        let notes = kw_bool(style, "notes").unwrap_or(true);
                        Ok(Value::Num(p.replace_text(&text_arg(args, 1)?, &text_arg(args, 2)?, notes).map_err(err)? as f64))
                    }
                    "layouts" => Ok(strs(p.layouts().map_err(err)?)),
                    "add_slide" => {
                        let layout = style_entry(style, "layout").map(|(_, v)| cell_text(v));
                        let title = style_str(style, "title");
                        let body: Vec<String> = match style_entry(style, "body") {
                            None => Vec::new(),
                            Some((_, Value::List(items))) => items.iter().map(cell_text).collect(),
                            Some((_, v)) => cell_text(v).split('\n').map(String::from).collect(),
                        };
                        let at = match style_entry(style, "at") {
                            Some((_, v)) => Some(v.as_index().map_err(|m| err(format!("at=: {m}")))?),
                            None => None,
                        };
                        Ok(Value::Num(p.add_slide(layout.as_deref(), title.as_deref(), &body, at).map_err(err)? as f64))
                    }
                    "delete_slide" => {
                        p.delete_slide(slide(1)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "move_slide" => {
                        p.move_slide(slide(1)?, index_arg(args, 2, f, "target position")?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "duplicate_slide" => Ok(Value::Num(p.duplicate_slide(slide(1)?).map_err(err)? as f64)),
                    "hide_slide" | "unhide_slide" => {
                        p.set_hidden(slide(1)?, name == "hide_slide").map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_text" => {
                        let i = slide(1)?;
                        let text = text_arg(args, 2)?;
                        let mut r = rect()?;
                        if r.w == 0.0 {
                            r.w = 100.0;
                        }
                        if r.h == 0.0 {
                            r.h = 20.0;
                        }
                        let fmt = TextFormat {
                            size: style_num(style, "size"),
                            bold: kw_bool(style, "bold").unwrap_or(false),
                            italic: kw_bool(style, "italic").unwrap_or(false),
                            color: style_str(style, "color"),
                            font: style_str(style, "font"),
                            align: style_str(style, "align"),
                        };
                        p.add_text(i, &text, r, &fmt).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_image" => {
                        let i = slide(1)?;
                        let r = rect()?;
                        let bytes = read_bytes(arg_get(args, 2).ok_or_else(|| err("needs an image path".into()))?, f)?;
                        p.add_image(i, &bytes, r).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_table" => {
                        let i = slide(1)?;
                        let (rows, had_header) = text_rows(arg_get(args, 2).ok_or_else(|| err("needs the table data".into()))?, f)?;
                        let mut r = rect()?;
                        if r.w == 0.0 {
                            r.w = 200.0;
                        }
                        if r.h == 0.0 {
                            r.h = 10.0 * rows.len() as f64;
                        }
                        let size = style_num(style, "size").unwrap_or(14.0);
                        p.add_table(i, &rows, r, kw_bool(style, "header").unwrap_or(had_header), size).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "to_markdown" => Ok(Value::Str(p.to_markdown().map_err(err)?)),
                    "to_pdf" => {
                        let out = text_arg(args, 1)?;
                        let bytes = p.to_bytes().map_err(err)?;
                        write_pdf(&bytes, "pptx", &out, style, f)
                    }
                    "set_notes" => {
                        let i = slide(1)?;
                        p.set_notes(i, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "shapes" => {
                        let num_or_none = |v: Option<f64>| v.map(Value::Num).unwrap_or(Value::Nothing);
                        let rows = p.shapes(slide(1)?).map_err(err)?;
                        Ok(Value::List(Arc::new(
                            rows.into_iter()
                                .map(|s| {
                                    Value::Record(Arc::new(vec![
                                        ("id".into(), Value::Num(s.id as f64)),
                                        ("name".into(), Value::Str(s.name)),
                                        ("kind".into(), Value::Str(s.kind)),
                                        ("placeholder".into(), s.placeholder.map(Value::Str).unwrap_or(Value::Nothing)),
                                        ("x".into(), num_or_none(s.rect.map(|r| r.x))),
                                        ("y".into(), num_or_none(s.rect.map(|r| r.y))),
                                        ("w".into(), num_or_none(s.rect.map(|r| r.w))),
                                        ("h".into(), num_or_none(s.rect.map(|r| r.h))),
                                        ("text".into(), Value::Str(s.text)),
                                    ]))
                                })
                                .collect(),
                        )))
                    }
                    "set_shape_text" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        p.set_shape_text(i, &s, &text_arg(args, 3)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "delete_shape" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        p.delete_shape(i, &s).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "move_shape" | "resize_shape" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        let (k1, k2) = if name == "move_shape" { ("x", "y") } else { ("w", "h") };
                        let (a, b) = (mm_kw(style, k1, f)?, mm_kw(style, k2, f)?);
                        if a.is_none() && b.is_none() {
                            return e(format!("{f}: give {k1}= and/or {k2}="));
                        }
                        let (x, y, w, h) = if name == "move_shape" { (a, b, None, None) } else { (None, None, a, b) };
                        p.place_shape(i, &s, x, y, w, h).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "set_slide_title" => {
                        let i = slide(1)?;
                        p.set_slide_title(i, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "bring_to_front" | "send_to_back" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        p.order_shape(i, &s, if name == "bring_to_front" { Order::Front } else { Order::Back }).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "set_link" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        let url = text_arg(args, 3)?;
                        let only = style_entry(style, "text").map(|(_, v)| cell_text(v));
                        Ok(Value::Num(p.set_link(i, &s, &url, only.as_deref()).map_err(err)? as f64))
                    }
                    "links" => {
                        let rows = p.links(slide(1)?).map_err(err)?;
                        Ok(Value::List(Arc::new(
                            rows.into_iter()
                                .map(|(id, text, url)| Value::Record(Arc::new(vec![("shape".into(), Value::Num(id as f64)), ("text".into(), Value::Str(text)), ("url".into(), Value::Str(url))])))
                                .collect(),
                        )))
                    }
                    "theme_colors" => Ok(Value::Record(Arc::new(p.theme_colors().map_err(err)?.into_iter().map(|(k, v)| (k, Value::Str(v))).collect()))),
                    "theme_fonts" => {
                        let (major, minor) = p.theme_fonts().map_err(err)?;
                        Ok(Value::Record(Arc::new(vec![("major".into(), Value::Str(major)), ("minor".into(), Value::Str(minor))])))
                    }
                    "set_theme_colors" => {
                        // Each slot is its own keyword, so a misspelt one is
                        // an unread keyword and errors like any other.
                        let mut given = Vec::new();
                        for slot in THEME_SLOTS {
                            if let Some((_, v)) = style_entry(style, slot) {
                                given.push((slot, cell_text(v)));
                            }
                        }
                        if given.is_empty() {
                            // Returning here pre-empts the unread-keyword
                            // check, so name a misspelt slot ourselves.
                            let why = match style.first() {
                                Some((k, _)) => format!("`{k}` is not a theme colour"),
                                None => "no colour given".into(),
                            };
                            return e(format!("{f}: {why} -- give one or more of {}=, e.g. accent1=\"#1F77B4\"", THEME_SLOTS.join("=, ")));
                        }
                        let refs: Vec<(&str, &str)> = given.iter().map(|(k, v)| (*k, v.as_str())).collect();
                        p.set_theme_colors(&refs).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "set_theme_fonts" => {
                        let major = style_entry(style, "major").map(|(_, v)| cell_text(v));
                        let minor = style_entry(style, "minor").map(|(_, v)| cell_text(v));
                        if major.is_none() && minor.is_none() {
                            return e(format!("{f}: give major= (headings) and/or minor= (body text)"));
                        }
                        p.set_theme_fonts(major.as_deref(), minor.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_chart" => {
                        let i = slide(1)?;
                        let kind = qu_pptx::chart_kind(&text_arg(args, 2)?).map_err(err)?;
                        let ys = chart_y(arg_get(args, 3).ok_or_else(|| err("needs the y values (a vector, or a list of vectors for several series)".into()))?, f)?;
                        let xs = match style_entry(style, "x") {
                            Some((_, v)) => chart_x(v, f)?,
                            None => Vec::new(),
                        };
                        let mut o = pptx_chart_options(style, f, false)?;
                        if let Some((_, n)) = style_entry(style, "names") {
                            o.names = match n {
                                Value::List(items) => items.iter().map(cell_text).collect(),
                                other => vec![cell_text(other)],
                            };
                        }
                        if let Some((_, c)) = style_entry(style, "colors") {
                            o.colors = match c {
                                Value::List(items) => items.iter().map(|c| color_hex(&cell_text(c), f, "colors=")).collect::<R<_>>()?,
                                other => vec![color_hex(&cell_text(other), f, "colors=")?],
                            };
                        }
                        Ok(Value::Num(p.add_chart(i, kind, &xs, &ys, &o).map_err(err)? as f64))
                    }
                    "add_nyquist_chart" => {
                        let i = slide(1)?;
                        // Either Z' and Z'' as two vectors, or one complex vector.
                        let (re, im) = match arg_get(args, 2) {
                            Some(z @ (Value::CVec(_) | Value::Complex(_) | Value::Spectrum(..))) => {
                                let zs = z.as_complex_flat().map_err(|m| err(m))?;
                                (zs.iter().map(|c| c.re).collect::<Vec<f64>>(), zs.iter().map(|c| c.im).collect::<Vec<f64>>())
                            }
                            Some(re) => {
                                let im = arg_get(args, 3).ok_or_else(|| err("needs Z' and Z'' (two vectors), or one complex vector".into()))?;
                                (chart_vec(re, f, "Z'")?, chart_vec(im, f, "Z''")?)
                            }
                            None => return e(format!("{f}: needs Z' and Z'' (two vectors), or one complex vector")),
                        };
                        let mut o = pptx_chart_options(style, f, true)?;
                        if let Some(n) = style_str(style, "name") {
                            o.names = vec![n];
                        }
                        if let Some(c) = color_kw(style, "color", f)? {
                            o.colors = vec![c];
                        }
                        let negate = kw_bool(style, "negate").unwrap_or(true);
                        Ok(Value::Num(p.add_nyquist_chart(i, &re, &im, negate, &o).map_err(err)? as f64))
                    }
                    "add_shape" => {
                        use qu_pptx::{Geometry, ShapeKind, ShapeStyle};
                        let i = slide(1)?;
                        let kind = ShapeKind::parse(&text_arg(args, 2)?).map_err(err)?;
                        let need = |k: &str| -> R<f64> { mm_kw(style, k, f)?.ok_or_else(|| err(format!("{k}= is required for a {}", if kind.is_line() { "line or arrow: give x=, y=, x2=, y2=" } else { "rect, ellipse or rounded_rect: give x=, y=, w=, h=" }))) };
                        let geom = if kind.is_line() {
                            Geometry::Segment { x1: need("x")?, y1: need("y")?, x2: need("x2")?, y2: need("y2")? }
                        } else {
                            Geometry::Frame(Rect { x: need("x")?, y: need("y")?, w: need("w")?, h: need("h")? })
                        };
                        let paint = |key: &str| -> R<Option<String>> {
                            match style_entry(style, key) {
                                None => Ok(None),
                                Some((_, Value::Str(s))) if s.eq_ignore_ascii_case("none") => Ok(Some("none".into())),
                                Some((_, Value::Str(s))) => color_hex(s, f, &format!("{key}=:")).map(Some),
                                Some((_, v)) => e(format!("{f}: {key}= takes a colour such as \"#1F77B4\" or \"red\" (or \"none\"), found {}", v.type_name())),
                            }
                        };
                        let mut st = ShapeStyle { stroke: paint("stroke")?, stroke_pt: style_num(style, "stroke_width"), dash: style_str(style, "dash"), name: style_str(style, "name"), ..Default::default() };
                        if !kind.is_line() {
                            st.fill = paint("fill")?;
                            if kind == ShapeKind::RoundedRect {
                                st.radius = style_num(style, "radius");
                            }
                            st.text = style_entry(style, "text").map(|(_, v)| cell_text(v));
                            if st.text.is_some() {
                                st.text_fmt = TextFormat {
                                    size: style_num(style, "size"),
                                    bold: kw_bool(style, "bold").unwrap_or(false),
                                    italic: kw_bool(style, "italic").unwrap_or(false),
                                    color: color_kw(style, "color", f)?,
                                    font: style_str(style, "font"),
                                    align: style_str(style, "align"),
                                };
                            }
                        }
                        Ok(Value::Num(p.add_shape(i, kind, geom, &st).map_err(err)? as f64))
                    }
                    "rotate_shape" => {
                        let (i, s) = (slide(1)?, shape_ref(args, 2, f)?);
                        let deg = arg_get(args, 3).ok_or_else(|| err("missing the angle in degrees (clockwise)".into()))?.as_num().map_err(|m| err(format!("angle: {m}")))?;
                        p.rotate_shape(i, &s, deg).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "align_shapes" => {
                        use qu_pptx::{Align, AlignTo};
                        let i = slide(1)?;
                        let shapes = shape_refs(arg_get(args, 2), f)?;
                        let how = Align::parse(&text_arg(args, 3)?).map_err(err)?;
                        let to = match style_str(style, "to").as_deref() {
                            None | Some("selection") | Some("shapes") => AlignTo::Selection,
                            Some("slide") => AlignTo::Slide,
                            Some(other) => return e(format!("{f}: to=\"{other}\" -- use \"selection\" (line the shapes up with each other) or \"slide\"")),
                        };
                        p.align_shapes(i, &shapes, how, to).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    other => e(format!("pptx.{other} is not a pptx function")),
                }
            }
        }
    }

    // ------------------------------------------------------------ xlsx workbook

    #[cfg(feature = "xlsx")]
    fn workbook_call(&mut self, name: &str, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        use qu_xlsx::surgical::TotalFn;
        use qu_xlsx::workbook::{CellFormat, CellValue, ClearWhat, SheetVisibility, SortKey, Workbook};
        let err = |msg: String| EvalError { msg: format!("{f}: {msg}") };
        match name {
            "new" => Ok(self.office_store("xlsx", Workbook::new(), "")),
            "open" => {
                let path = text_arg(args, 0)?;
                let w = Workbook::open(&path).map_err(err)?;
                Ok(self.office_store("xlsx", w, &path))
            }
            _ => {
                let wb: &mut Workbook = self.office_mut(args.first(), "xlsx", f)?;
                // Sheet by name, or by 0-based index.
                let sheet = |wb: &Workbook, i: usize| -> R<String> {
                    match arg_get(args, i) {
                        Some(Value::Num(n)) => {
                            let names = wb.sheet_names();
                            names.get(*n as usize).cloned().ok_or_else(|| err(format!("sheet {n} does not exist -- the workbook has {}", names.len())))
                        }
                        Some(v) => Ok(cell_text(v)),
                        None => e(format!("{f}: missing the sheet name")),
                    }
                };
                let to_cell = |v: &Value| -> CellValue {
                    match v {
                        Value::Num(x) => CellValue::Num(*x),
                        Value::Bool(b) => CellValue::Bool(*b),
                        Value::Nothing => CellValue::Empty,
                        Value::Str(s) => CellValue::Str(s.clone()),
                        other => CellValue::Str(display_value(other)),
                    }
                };
                let from_cell = |c: CellValue| -> Value {
                    match c {
                        CellValue::Empty => Value::Nothing,
                        CellValue::Num(x) => Value::Num(x),
                        CellValue::Str(s) => Value::Str(s),
                        CellValue::Bool(b) => Value::Bool(b),
                    }
                };
                match name {
                    "save_as" => {
                        wb.save(&text_arg(args, 1)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "get_cell" => {
                        let s = sheet(wb, 1)?;
                        Ok(from_cell(wb.get(&s, &text_arg(args, 2)?).map_err(err)?))
                    }
                    "set_cell" => {
                        let s = sheet(wb, 1)?;
                        let v = to_cell(arg_get(args, 3).ok_or_else(|| err("needs the value".into()))?);
                        wb.set(&s, &text_arg(args, 2)?, &v).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "formula" => {
                        let s = sheet(wb, 1)?;
                        Ok(wb.formula(&s, &text_arg(args, 2)?).map_err(err)?.map(Value::Str).unwrap_or(Value::Nothing))
                    }
                    "set_formula" => {
                        let s = sheet(wb, 1)?;
                        wb.set_formula(&s, &text_arg(args, 2)?, &text_arg(args, 3)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "fill_formula" => {
                        let s = sheet(wb, 1)?;
                        Ok(Value::Num(wb.fill_formula(&s, &text_arg(args, 2)?, &text_arg(args, 3)?).map_err(err)? as f64))
                    }
                    "get_range" => {
                        let s = sheet(wb, 1)?;
                        let rows = wb.range(&s, &text_arg(args, 2)?).map_err(err)?;
                        if kw_bool(style, "numeric").unwrap_or(false) {
                            let data: Vec<Vec<f64>> = rows
                                .into_iter()
                                .map(|r| r.into_iter().map(|c| if let CellValue::Num(x) = c { x } else { f64::NAN }).collect())
                                .collect();
                            let m = crate::Matrix::from_rows(&data).map_err(|se| err(se.to_string()))?;
                            return Ok(Value::Mat(Arc::new(m)));
                        }
                        Ok(Value::List(Arc::new(rows.into_iter().map(|r| Value::List(Arc::new(r.into_iter().map(from_cell).collect()))).collect())))
                    }
                    "set_range" => {
                        let s = sheet(wb, 1)?;
                        let anchor = text_arg(args, 2)?;
                        let data = arg_get(args, 3).ok_or_else(|| err("needs the data".into()))?;
                        let rows: Vec<Vec<CellValue>> = match data {
                            Value::Table(t) => {
                                let names: Vec<String> = t.column_names().iter().map(|n| n.to_string()).collect();
                                let mut rows = Vec::new();
                                if kw_bool(style, "header").unwrap_or(true) {
                                    rows.push(names.iter().map(|n| CellValue::Str(n.clone())).collect());
                                }
                                for r in 0..t.nrows() {
                                    rows.push(
                                        names
                                            .iter()
                                            .map(|n| match t.col(n) {
                                                Some(table::Column::Num(xs)) => CellValue::Num(xs[r]),
                                                Some(table::Column::Str(ss)) => CellValue::Str(ss[r].clone()),
                                                None => CellValue::Empty,
                                            })
                                            .collect(),
                                    );
                                }
                                rows
                            }
                            Value::Mat(m) => {
                                let (r, c) = m.shape();
                                (0..r).map(|i| (0..c).map(|j| CellValue::Num(m.get(i, j).unwrap_or(f64::NAN))).collect()).collect()
                            }
                            Value::Vec(xs) => xs.iter().map(|x| vec![CellValue::Num(*x)]).collect(),
                            Value::List(rows) => rows
                                .iter()
                                .map(|r| match r {
                                    Value::List(cells) => cells.iter().map(to_cell).collect(),
                                    Value::Vec(xs) => xs.iter().map(|x| CellValue::Num(*x)).collect(),
                                    other => vec![to_cell(other)],
                                })
                                .collect(),
                            other => return e(format!("{f}: expected a table, matrix, vector or list of rows, found {}", other.type_name())),
                        };
                        wb.set_range(&s, &anchor, &rows).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "used_range" => {
                        let s = sheet(wb, 1)?;
                        Ok(wb.used_range(&s).map_err(err)?.map(Value::Str).unwrap_or(Value::Nothing))
                    }
                    "add_sheet" => {
                        wb.add_sheet(&text_arg(args, 1)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "rename_sheet" => {
                        let s = sheet(wb, 1)?;
                        wb.rename_sheet(&s, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "delete_sheet" => {
                        let s = sheet(wb, 1)?;
                        wb.delete_sheet(&s).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "insert_rows" | "delete_rows" => {
                        let s = sheet(wb, 1)?;
                        let at = index_arg(args, 2, f, "row number (1-based)")? as u32;
                        let n = match arg_get(args, 3) {
                            Some(v) => v.as_index().map_err(|m| err(format!("count: {m}")))? as u32,
                            None => 1,
                        };
                        if at == 0 {
                            return e(format!("{f}: rows are numbered from 1, as Excel shows them"));
                        }
                        if name == "insert_rows" { wb.insert_rows(&s, at, n) } else { wb.delete_rows(&s, at, n) }.map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "insert_columns" | "delete_columns" => {
                        let s = sheet(wb, 1)?;
                        let col = text_arg(args, 2)?;
                        let n = match arg_get(args, 3) {
                            Some(v) => v.as_index().map_err(|m| err(format!("count: {m}")))? as u32,
                            None => 1,
                        };
                        if name == "insert_columns" { wb.insert_columns(&s, &col, n) } else { wb.delete_columns(&s, &col, n) }.map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "column_width" => {
                        let s = sheet(wb, 1)?;
                        let w = arg_get(args, 3).ok_or_else(|| err("needs the width".into()))?.as_num().map_err(|m| err(m))?;
                        wb.column_width(&s, &text_arg(args, 2)?, w).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "row_height" => {
                        let s = sheet(wb, 1)?;
                        let row = index_arg(args, 2, f, "row number (1-based)")? as u32;
                        let h = arg_get(args, 3).ok_or_else(|| err("needs the height".into()))?.as_num().map_err(|m| err(m))?;
                        wb.row_height(&s, row, h).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "format_cells" => {
                        let s = sheet(wb, 1)?;
                        let cf = CellFormat {
                            bold: kw_bool(style, "bold"),
                            italic: kw_bool(style, "italic"),
                            size: style_num(style, "size"),
                            font: style_str(style, "font"),
                            color: style_str(style, "color"),
                            background: style_str(style, "background"),
                            number_format: style_str(style, "number_format"),
                            align: style_str(style, "align"),
                        };
                        Ok(Value::Num(wb.format(&s, &text_arg(args, 2)?, &cf).map_err(err)? as f64))
                    }
                    "merge" => {
                        let s = sheet(wb, 1)?;
                        wb.merge(&s, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "freeze_panes" => {
                        let s = sheet(wb, 1)?;
                        wb.freeze_panes(&s, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "define_name" => {
                        wb.define_name(&text_arg(args, 1)?, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "to_pdf" => {
                        let out = text_arg(args, 1)?;
                        let bytes = wb.to_bytes().map_err(err)?;
                        write_pdf(&bytes, "xlsx", &out, style, f)
                    }
                    "add_chart" => {
                        let s = sheet(wb, 1)?;
                        let kind = qu_xlsx::workbook::chart_kind(&text_arg(args, 2)?).map_err(err)?;
                        let mut o = chart_options(style, f, kind, false)?;
                        o.y = str_list(arg_get(args, 3).ok_or_else(|| err("needs the y range(s), e.g. \"B2:B40\"".into()))?, f, "y")?;
                        if let Some(x) = style_entry(style, "x") {
                            o.x = str_list(&x.1, f, "x=")?;
                        }
                        if let Some((_, n)) = style_entry(style, "names") {
                            o.names = match n {
                                Value::List(items) => items.iter().map(cell_text).collect(),
                                other => vec![cell_text(other)],
                            };
                        }
                        if let Some(c) = style_entry(style, "colors") {
                            o.colors = str_list(&c.1, f, "colors=")?.iter().map(|c| color_hex(c, f, "colors=")).collect::<R<_>>()?;
                        }
                        wb.add_chart(&s, &o).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_nyquist_chart" => {
                        let s = sheet(wb, 1)?;
                        let re = text_arg(args, 2)?;
                        let im = text_arg(args, 3)?;
                        let mut o = chart_options(style, f, qu_xlsx::workbook::ChartKind::Scatter, true)?;
                        if let Some(n) = style_str(style, "name") {
                            o.names = vec![n];
                        }
                        if let Some(c) = color_kw(style, "color", f)? {
                            o.colors = vec![c];
                        }
                        let negate = kw_bool(style, "negate").unwrap_or(true);
                        let helper = match negate {
                            true => style_str(style, "helper"),
                            false => None,
                        };
                        let out = wb.add_nyquist_chart(&s, &re, &im, negate, helper.as_deref(), o).map_err(err)?;
                        Ok(out.map(Value::Str).unwrap_or(Value::Nothing))
                    }
                    "conditional_format" => {
                        use qu_xlsx::surgical::{CfRule, CfStyle};
                        use qu_xlsx::workbook::{cf_operator, cf_value_num, cf_value_text};
                        let s = sheet(wb, 1)?;
                        let range = text_arg(args, 2)?;
                        let rule_name = text_arg(args, 3)?;
                        let value = |i: usize| -> R<qu_xlsx::surgical::CfValue> {
                            match arg_get(args, i) {
                                Some(Value::Num(x)) if x.is_finite() => Ok(cf_value_num(*x)),
                                Some(Value::Str(t)) => Ok(cf_value_text(t)),
                                Some(Value::Bool(b)) => Ok(qu_xlsx::surgical::CfValue::Formula(if *b { "TRUE" } else { "FALSE" }.into())),
                                Some(other) => e(format!("{f}: the value to compare with must be a number or a string, found {}", other.type_name())),
                                None => e(format!("{f}: `{rule_name}` needs a value to compare with")),
                            }
                        };
                        let rule = if rule_name == "contains" {
                            CfRule::Contains(text_arg(args, 4)?)
                        } else {
                            let op = cf_operator(&rule_name).ok_or_else(|| {
                                err(format!(
                                    "rule \"{rule_name}\" -- use greater_than, less_than, greater_equal, less_equal, equal, not_equal, between, not_between or contains (xlsx.color_scale for a colour scale)"
                                ))
                            })?;
                            let two = matches!(op, "between" | "notBetween");
                            CfRule::Cell { op, a: value(4)?, b: if two { Some(value(5)?) } else { None } }
                        };
                        let mut st = CfStyle { fill: color_kw(style, "fill", f)?, color: color_kw(style, "color", f)?, bold: kw_bool(style, "bold").unwrap_or(false) };
                        if st.fill.is_none() && st.color.is_none() && !st.bold {
                            // Excel's own default highlight: light red fill, dark red text.
                            st = CfStyle { fill: Some("#FFC7CE".into()), color: Some("#9C0006".into()), bold: false };
                        }
                        wb.conditional_format(&s, &range, rule, st).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "color_scale" => {
                        let s = sheet(wb, 1)?;
                        let range = text_arg(args, 2)?;
                        let low = color_kw(style, "low", f)?.unwrap_or_else(|| "#F8696B".into());
                        let mid = color_kw(style, "mid", f)?;
                        let high = color_kw(style, "high", f)?.unwrap_or_else(|| "#63BE7B".into());
                        let colors = match mid {
                            Some(m) => vec![low, m, high],
                            None => vec![low, high],
                        };
                        wb.conditional_format(&s, &range, qu_xlsx::surgical::CfRule::Scale(colors), Default::default()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_validation" => {
                        use qu_xlsx::surgical::ValKind;
                        let s = sheet(wb, 1)?;
                        let range = text_arg(args, 2)?;
                        let kind_name = text_arg(args, 3)?;
                        let num = |key: &str| -> R<Option<f64>> {
                            match style_entry(style, key) {
                                None => Ok(None),
                                Some((_, v)) => v.as_num().map(Some).map_err(|_| err(format!("{key}= must be a number, found {}", v.type_name()))),
                            }
                        };
                        let kind = match kind_name.as_str() {
                            "list" => match (style_entry(style, "values"), style_str(style, "source")) {
                                (Some((_, v)), None) => ValKind::List(match v {
                                    Value::List(items) => items.iter().map(cell_text).collect(),
                                    Value::Vec(xs) => xs.iter().map(|x| display_value(&Value::Num(*x))).collect(),
                                    other => vec![cell_text(other)],
                                }),
                                (None, Some(src)) => ValKind::ListFrom(qu_xlsx::surgical::parse_ref(&src, &s).map_err(err)?),
                                _ => return e(format!("{f}: a list validation takes values=[...] or source=\"D2:D9\" (one of them)")),
                            },
                            "whole" => ValKind::Number("whole", num("min")?, num("max")?),
                            "decimal" => ValKind::Number("decimal", num("min")?, num("max")?),
                            "custom" => ValKind::Custom(style_str(style, "formula").ok_or_else(|| err("custom needs formula=\"...\"".into()))?),
                            other => return e(format!("{f}: kind \"{other}\" -- use list, whole, decimal or custom")),
                        };
                        wb.add_validation(
                            &s,
                            &range,
                            kind,
                            style_str(style, "prompt"),
                            style_str(style, "prompt_title"),
                            style_str(style, "error"),
                            style_str(style, "error_title"),
                            style_str(style, "error_style"),
                            kw_bool(style, "allow_blank").unwrap_or(true),
                        )
                        .map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "copy_sheet" => {
                        let s = sheet(wb, 1)?;
                        wb.copy_sheet(&s, &text_arg(args, 2)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "hide_sheet" => {
                        let s = sheet(wb, 1)?;
                        // `state=` or a third positional word.
                        let state = style_str(style, "state").or_else(|| arg_get(args, 2).map(cell_text)).unwrap_or_else(|| "hidden".into());
                        wb.set_sheet_visibility(&s, SheetVisibility::parse(&state).map_err(err)?).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "clear" => {
                        let s = sheet(wb, 1)?;
                        let what = ClearWhat::parse(&style_str(style, "what").unwrap_or_else(|| "all".into())).map_err(err)?;
                        Ok(Value::Num(wb.clear(&s, &text_arg(args, 2)?, what).map_err(err)? as f64))
                    }
                    "move_range" => {
                        let s = sheet(wb, 1)?;
                        let to_sheet = match style_entry(style, "to_sheet") {
                            None => None,
                            Some((_, Value::Num(n))) => Some(wb.sheet_names().get(*n as usize).cloned().ok_or_else(|| err(format!("to_sheet {n} does not exist")))?),
                            Some((_, v)) => Some(cell_text(v)),
                        };
                        let copy = kw_bool(style, "copy").unwrap_or(false);
                        let n = wb.move_range(&s, &text_arg(args, 2)?, &text_arg(args, 3)?, to_sheet.as_deref(), copy).map_err(err)?;
                        Ok(Value::Num(n as f64))
                    }
                    "sort_range" => {
                        let s = sheet(wb, 1)?;
                        let range = text_arg(args, 2)?;
                        let by = style_entry(style, "by").map(|(_, v)| v.clone()).or_else(|| arg_get(args, 3).cloned()).ok_or_else(|| err("needs the column to sort by: by=\"C\" (a sheet column) or by=1 (first column of the range)".into()))?;
                        let key = |v: &Value| -> R<SortKey> {
                            match v {
                                Value::Num(n) if *n >= 1.0 && n.fract() == 0.0 => Ok(SortKey::Position(*n as u32)),
                                Value::Str(l) => Ok(SortKey::Letter(l.trim().to_ascii_uppercase())),
                                other => e(format!("{f}: by= takes a column letter like \"C\" or a position like 2 (or a list of them), found {}", display_value(other))),
                            }
                        };
                        // `[2, 1]` and `[true, false]` are numeric vectors in Qu, `["A", "B"]` a list.
                        let keys: Vec<SortKey> = match &by {
                            Value::List(items) => items.iter().map(key).collect::<R<_>>()?,
                            Value::Vec(xs) => xs.iter().map(|x| key(&Value::Num(*x))).collect::<R<_>>()?,
                            one => vec![key(one)?],
                        };
                        let flags: Option<Vec<bool>> = match style_entry(style, "desc") {
                            None => None,
                            Some((_, Value::List(items))) => Some(items.iter().map(truthy).collect()),
                            Some((_, Value::Vec(xs))) => Some(xs.iter().map(|x| *x != 0.0).collect()),
                            Some((_, v)) => Some(vec![truthy(v)]),
                        };
                        let desc: Vec<bool> = match flags {
                            None => vec![false; keys.len()],
                            Some(fl) if fl.len() == 1 => vec![fl[0]; keys.len()],
                            Some(fl) if fl.len() == keys.len() => fl,
                            Some(fl) => return e(format!("{f}: {} sort keys but {} desc= flags", keys.len(), fl.len())),
                        };
                        let header = kw_bool(style, "header").unwrap_or(false);
                        let keyed: Vec<(SortKey, bool)> = keys.into_iter().zip(desc).collect();
                        wb.sort_range(&s, &range, &keyed, header).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "autofilter" => {
                        let s = sheet(wb, 1)?;
                        if kw_bool(style, "remove").unwrap_or(false) {
                            wb.autofilter(&s, None).map_err(err)?;
                        } else {
                            wb.autofilter(&s, Some(&text_arg(args, 2)?)).map_err(err)?;
                        }
                        Ok(Value::Nothing)
                    }
                    "create_table" => {
                        let s = sheet(wb, 1)?;
                        let total = match style_entry(style, "total") {
                            None | Some((_, Value::Bool(false))) | Some((_, Value::Nothing)) => None,
                            Some((_, Value::Bool(true))) => Some(TotalFn::Sum),
                            Some((_, Value::Str(t))) => Some(TotalFn::parse(t).map_err(err)?),
                            Some((_, v)) => return e(format!("{f}: total= takes true, or sum/average/count/min/max, found {}", v.type_name())),
                        };
                        let header = kw_bool(style, "header").unwrap_or(true);
                        let stripes = kw_bool(style, "stripes").unwrap_or(true);
                        let tstyle = style_str(style, "style");
                        wb.create_table(&s, &text_arg(args, 2)?, &text_arg(args, 3)?, header, tstyle.as_deref(), stripes, total).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_comment" => {
                        let s = sheet(wb, 1)?;
                        let author = style_str(style, "author");
                        wb.add_comment(&s, &text_arg(args, 2)?, &text_arg(args, 3)?, author.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    "add_image" => {
                        let s = sheet(wb, 1)?;
                        let (w, h) = (mm_kw(style, "width", f)?, mm_kw(style, "height", f)?);
                        let (at, alt) = (style_str(style, "at"), style_str(style, "alt"));
                        let bytes = read_bytes(arg_get(args, 2).ok_or_else(|| err("needs an image path (PNG or JPEG)".into()))?, f)?;
                        wb.add_image(&s, bytes, at.as_deref(), w, h, alt.as_deref()).map_err(err)?;
                        Ok(Value::Nothing)
                    }
                    other => e(format!("xlsx.{other} is not an xlsx workbook function")),
                }
            }
        }
    }
}

/// A shape on a slide: its id (a whole number, from `pptx.shapes`) or its
/// name.
#[cfg(feature = "pptx")]
fn shape_ref(args: &[Value], i: usize, f: &str) -> R<qu_pptx::ShapeRef> {
    match arg_get(args, i) {
        Some(Value::Num(n)) if *n >= 0.0 && n.fract() == 0.0 => Ok(qu_pptx::ShapeRef::Id(*n as u64)),
        Some(Value::Str(s)) => Ok(qu_pptx::ShapeRef::Name(s.clone())),
        Some(other) => e(format!("{f}: the shape is its id (a whole number) or its name, found {}", other.type_name())),
        None => e(format!("{f}: missing the shape -- its id or name, as pptx.shapes lists them")),
    }
}

/// Several shapes: a list (or vector) of ids and/or names, or just one.
#[cfg(feature = "pptx")]
fn shape_refs(v: Option<&Value>, f: &str) -> R<Vec<qu_pptx::ShapeRef>> {
    match v {
        Some(Value::List(items)) => items.iter().enumerate().map(|(k, it)| shape_ref(std::slice::from_ref(it), 0, f).map_err(|m| EvalError { msg: format!("{} (item {})", m.msg, k + 1) })).collect(),
        Some(Value::Vec(ids)) => ids.iter().map(|n| shape_ref(&[Value::Num(*n)], 0, f)).collect(),
        Some(one) => Ok(vec![shape_ref(std::slice::from_ref(one), 0, f)?]),
        None => e(format!("{f}: missing the shapes -- a list of ids or names, as pptx.shapes lists them")),
    }
}

/// One series of numbers: a vector, a signal, or a list of numbers.
#[cfg(feature = "pptx")]
fn chart_vec(v: &Value, f: &str, what: &str) -> R<Vec<f64>> {
    match v {
        Value::List(items) => items
            .iter()
            .map(|i| match i {
                Value::Num(n) => Ok(*n),
                other => e(format!("{f}: {what} takes numbers, found {} in the list", other.type_name())),
            })
            .collect(),
        Value::Vec(_) | Value::Signal(..) | Value::Num(_) => v.as_flat().map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") }),
        other => e(format!("{f}: {what} takes a vector of numbers, found {}", other.type_name())),
    }
}

/// Whether a list entry is itself a data series (so the list is several).
#[cfg(feature = "pptx")]
fn is_series(v: &Value) -> bool {
    matches!(v, Value::Vec(_) | Value::Signal(..) | Value::List(_))
}

/// `y`: one vector, a list of vectors, or a matrix (one series per column).
#[cfg(feature = "pptx")]
fn chart_y(v: &Value, f: &str) -> R<Vec<Vec<f64>>> {
    match v {
        Value::Mat(m) if m.shape().1 > 1 && m.shape().0 > 1 => {
            let (r, c) = m.shape();
            Ok((0..c).map(|j| (0..r).map(|i| m.get(i, j).unwrap_or(f64::NAN)).collect()).collect())
        }
        Value::List(items) if items.iter().any(is_series) => items.iter().map(|i| chart_vec(i, f, "y")).collect(),
        other => Ok(vec![match other {
            Value::Mat(m) => m.as_slice().to_vec(),
            o => chart_vec(o, f, "y")?,
        }]),
    }
}

/// `x=`: numbers or text labels shared by every series, or a list with one
/// of those per series.
#[cfg(feature = "pptx")]
fn chart_x(v: &Value, f: &str) -> R<Vec<qu_pptx::ChartX>> {
    use qu_pptx::ChartX;
    let one = |v: &Value| -> R<ChartX> {
        match v {
            Value::List(items) if !items.is_empty() && items.iter().all(|i| matches!(i, Value::Str(_))) => Ok(ChartX::Labels(items.iter().map(cell_text).collect())),
            other => Ok(ChartX::Numbers(chart_vec(other, f, "x=")?)),
        }
    };
    match v {
        Value::List(items) if items.iter().any(is_series) => items.iter().map(one).collect(),
        other => Ok(vec![one(other)?]),
    }
}

/// `[x, y]` in millimetres or lengths: a two-element vector or list.
#[cfg(feature = "pptx")]
fn point_kw(style: &[(String, Value)], key: &str, f: &str) -> R<Option<(f64, f64)>> {
    let Some((_, v)) = style_entry(style, key) else { return Ok(None) };
    let what = format!("{f}: `{key}=`");
    match v {
        Value::Vec(xs) if xs.len() == 2 => Ok(Some((xs[0], xs[1]))),
        Value::List(xs) if xs.len() == 2 => Ok(Some((mm_value(&xs[0], &what)?, mm_value(&xs[1], &what)?))),
        Value::Quantity(inner, UnitTag::Dim(d, _)) if *d == LENGTH => match &**inner {
            Value::Vec(xs) if xs.len() == 2 => Ok(Some((xs[0] * 1000.0, xs[1] * 1000.0))),
            _ => e(format!("{what} takes [x, y], the chart's top-left corner")),
        },
        other => e(format!("{what} takes [x, y], the chart's top-left corner, found {}", other.type_name())),
    }
}

/// The keywords `add_chart` and `add_nyquist_chart` share on a slide. A
/// Nyquist chart computes its own axis limits, so those are not read for it
/// (giving them is then an unread-keyword error rather than ignored).
#[cfg(feature = "pptx")]
fn pptx_chart_options(style: &[(String, Value)], f: &str, nyquist: bool) -> R<qu_pptx::ChartOptions> {
    let num = |key: &str| -> R<Option<f64>> {
        match style_entry(style, key) {
            None => Ok(None),
            Some((_, v)) => v.as_num().map(Some).map_err(|_| EvalError { msg: format!("{f}: {key}= must be a number, found {}", v.type_name()) }),
        }
    };
    let mut o = qu_pptx::ChartOptions::default();
    o.title = style_str(style, "title");
    o.x_title = style_str(style, "x_title");
    o.y_title = style_str(style, "y_title");
    o.at = point_kw(style, "at", f)?;
    o.width_mm = mm_kw(style, "width", f)?;
    o.height_mm = mm_kw(style, "height", f)?;
    o.lines = kw_bool(style, "lines");
    o.markers = kw_bool(style, "markers");
    o.legend = kw_bool(style, "legend");
    o.name = style_str(style, "shape_name");
    if !nyquist {
        o.x_min = num("x_min")?;
        o.x_max = num("x_max")?;
        o.y_min = num("y_min")?;
        o.y_max = num("y_max")?;
        o.x_log = kw_bool(style, "x_log").unwrap_or(false);
        o.y_log = kw_bool(style, "y_log").unwrap_or(false);
        o.equal_axes = kw_bool(style, "equal_axes").unwrap_or(false);
    }
    Ok(o)
}

/// A range argument: one string, or a list of them (one per series).
#[cfg(feature = "xlsx")]
fn str_list(v: &Value, f: &str, what: &str) -> R<Vec<String>> {
    match v {
        Value::Str(s) => Ok(vec![s.clone()]),
        Value::List(items) => items
            .iter()
            .map(|i| match i {
                Value::Str(s) => Ok(s.clone()),
                other => e(format!("{f}: {what} takes range strings like \"B2:B40\", found {}", other.type_name())),
            })
            .collect(),
        other => e(format!("{f}: {what} takes a range string like \"B2:B40\" or a list of them, found {}", other.type_name())),
    }
}

/// A colour by name (`"red"`), `#rgb` or `#rrggbb`, as everywhere else in Qu.
#[cfg(any(feature = "xlsx", feature = "pptx"))]
fn color_hex(c: &str, f: &str, what: &str) -> R<String> {
    crate::color::resolve(c).map_err(|m| EvalError { msg: format!("{f}: {what} {m}") })
}

#[cfg(any(feature = "xlsx", feature = "pptx"))]
fn color_kw(style: &[(String, Value)], key: &str, f: &str) -> R<Option<String>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, Value::Str(s))) => color_hex(s, f, &format!("{key}=:")).map(Some),
        Some((_, v)) => e(format!("{f}: {key}= takes a colour such as \"#FFC7CE\" or \"red\", found {}", v.type_name())),
    }
}

/// The keywords `add_chart` and `add_nyquist_chart` share. A Nyquist
/// chart computes its own axis limits, so those are not read for it (and
/// giving them is then an unread-keyword error rather than ignored).
#[cfg(feature = "xlsx")]
fn chart_options(style: &[(String, Value)], f: &str, kind: qu_xlsx::workbook::ChartKind, nyquist: bool) -> R<qu_xlsx::workbook::ChartOptions> {
    let num = |key: &str| -> R<Option<f64>> {
        match style_entry(style, key) {
            None => Ok(None),
            Some((_, v)) => v.as_num().map(Some).map_err(|_| EvalError { msg: format!("{f}: {key}= must be a number, found {}", v.type_name()) }),
        }
    };
    let mut o = qu_xlsx::workbook::ChartOptions::new(kind);
    o.title = style_str(style, "title");
    o.x_title = style_str(style, "x_title");
    o.y_title = style_str(style, "y_title");
    o.at = style_str(style, "at");
    o.width_mm = mm_kw(style, "width", f)?;
    o.height_mm = mm_kw(style, "height", f)?;
    o.lines = kw_bool(style, "lines");
    o.markers = kw_bool(style, "markers");
    o.legend = kw_bool(style, "legend");
    if !nyquist {
        o.x_min = num("x_min")?;
        o.x_max = num("x_max")?;
        o.y_min = num("y_min")?;
        o.y_max = num("y_max")?;
        o.x_log = kw_bool(style, "x_log").unwrap_or(false);
        o.y_log = kw_bool(style, "y_log").unwrap_or(false);
        o.equal_axes = kw_bool(style, "equal_axes").unwrap_or(false);
    }
    Ok(o)
}

fn module_kind(module: &str) -> &'static str {
    match module {
        "docx" => "docx",
        "pptx" => "pptx",
        _ => "xlsx",
    }
}

/// `set_info(doc, title=, author=, subject=, keywords=, description=, category=)`.
fn info_props(style: &[(String, Value)], f: &str) -> R<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (key, prop) in [("title", "title"), ("author", "creator"), ("subject", "subject"), ("keywords", "keywords"), ("description", "description"), ("category", "category")] {
        if let Some(v) = style_str(style, key) {
            out.push((prop.to_string(), v));
        }
    }
    if out.is_empty() {
        return e(format!("{f}: give at least one of title=, author=, subject=, keywords=, description=, category="));
    }
    Ok(out)
}
