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
    "to_pdf",
];

pub const PPTX_NAMES: &[&str] = &[
    "new", "open", "save_as", "discard", "info", "set_info", "slide_count", "slides", "slide_text", "slide_title", "notes", "find_text",
    "replace_text", "layouts", "add_slide", "delete_slide", "move_slide", "duplicate_slide", "hide_slide", "unhide_slide", "add_text",
    "add_image", "add_table", "to_markdown", "to_pdf",
];

pub const XLSX_NAMES: &[&str] = &[
    "new", "open", "save_as", "discard", "get_cell", "set_cell", "formula", "set_formula", "fill_formula", "get_range", "set_range",
    "used_range", "add_sheet", "rename_sheet", "delete_sheet", "insert_rows", "delete_rows", "insert_columns", "delete_columns",
    "column_width", "row_height", "format_cells", "merge", "freeze_panes", "define_name", "to_pdf",
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
                    other => e(format!("docx.{other} is not a docx function")),
                }
            }
        }
    }

    // ------------------------------------------------------------ pptx

    #[cfg(feature = "pptx")]
    fn pptx_call(&mut self, name: &str, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        use qu_pptx::{Presentation, Rect, TextFormat};
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
                    other => e(format!("pptx.{other} is not a pptx function")),
                }
            }
        }
    }

    // ------------------------------------------------------------ xlsx workbook

    #[cfg(feature = "xlsx")]
    fn workbook_call(&mut self, name: &str, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        use qu_xlsx::workbook::{CellFormat, CellValue, Workbook};
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
                    other => e(format!("xlsx.{other} is not an xlsx workbook function")),
                }
            }
        }
    }
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
