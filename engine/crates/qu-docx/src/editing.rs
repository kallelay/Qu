//! Editing what a document already says: table rows, columns and merges,
//! character formatting of a span of text, line spacing, moving
//! paragraphs, and creating footnotes and tracked changes.
//!
//! The same rule as the rest of the crate -- parse and rewrite only the
//! parts an operation needs (the main document, plus `styles`, `settings`
//! or `footnotes` when a footnote is created), keep every other part byte
//! for byte, and put new elements where the schema's sequence says.
//!
//! Text spans are addressed the way `add_link`/`add_comment` do it: body
//! paragraph `i` and the text `on` inside it, matched against the text a
//! reader sees. Unlike those, which need the span to start and end between
//! the paragraph's own children, the operations here cut runs wherever they
//! sit -- inside a hyperlink, a tracked insertion, a content control -- so a
//! formatted span never has to be "outside" anything. Field machinery
//! (`w:fldChar` runs, `w:fldSimple`) is the exception: text inside a field
//! is its result, and editing the result of a field is editing something
//! Word will recompute, so tracked changes and note anchors refuse it.

use super::structure::{child_len, insert_ordered, now_iso, para_display_text_of, split_run, CT_BASE, PPR_ORDER, REL_BASE, SETTINGS_ORDER};
use super::*;

/// `CT_RPr` child order.
const RPR_ORDER: &[&str] = &[
    "w:rStyle", "w:rFonts", "w:b", "w:bCs", "w:i", "w:iCs", "w:caps", "w:smallCaps", "w:strike", "w:dstrike", "w:outline", "w:shadow",
    "w:emboss", "w:imprint", "w:noProof", "w:snapToGrid", "w:vanish", "w:webHidden", "w:color", "w:spacing", "w:w", "w:kern",
    "w:position", "w:sz", "w:szCs", "w:highlight", "w:u", "w:effect", "w:bdr", "w:shd", "w:fitText", "w:vertAlign", "w:rtl", "w:cs",
    "w:em", "w:lang", "w:eastAsianLayout", "w:specVanish", "w:oMath", "w:rPrChange",
];

/// `CT_TcPr` child order.
const TCPR_ORDER: &[&str] = &[
    "w:cnfStyle", "w:tcW", "w:gridSpan", "w:hMerge", "w:vMerge", "w:tcBorders", "w:shd", "w:noWrap", "w:tcMar", "w:textDirection",
    "w:tcFitText", "w:vAlign", "w:hideMark", "w:headers", "w:cellIns", "w:cellDel", "w:cellMerge", "w:tcPrChange",
];

/// Character formatting to apply to a span of existing text. `None` leaves
/// a property as it is; `Some(false)` switches an inherited one off.
#[derive(Clone, Debug, Default)]
pub struct RunFormat {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Points.
    pub size: Option<f64>,
    /// `#rrggbb`, `rrggbb` or `auto`.
    pub color: Option<String>,
    pub font: Option<String>,
}

impl RunFormat {
    pub fn is_empty(&self) -> bool {
        self.bold.is_none() && self.italic.is_none() && self.underline.is_none() && self.strike.is_none() && self.size.is_none() && self.color.is_none() && self.font.is_none()
    }
}

/// Paragraph spacing to set. Line spacing is one of the first three (at
/// most one); `before`/`after` are points of space around the paragraph.
#[derive(Clone, Debug, Default)]
pub struct Spacing {
    /// A multiple of the font's line height (1.0 single, 1.5, 2.0 double).
    pub lines: Option<f64>,
    /// An exact line pitch in points.
    pub exactly: Option<f64>,
    /// A minimum line pitch in points.
    pub at_least: Option<f64>,
    pub before: Option<f64>,
    pub after: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Vm {
    No,
    Restart,
    Continue,
}

/// One cell of a table row in grid terms: where it starts, how many grid
/// columns it covers, and its place in a vertical merge.
#[derive(Clone, Copy, Debug)]
struct CellInfo {
    idx: usize,
    start: usize,
    span: usize,
    vm: Vm,
}

impl Document {
    // ------------------------------------------------------------ tables

    fn tbl_mut(&mut self, t: usize) -> Result<&mut Element, String> {
        self.body_mut().elems_mut().filter(|e| e.name == "w:tbl").nth(t).ok_or_else(|| format!("table {t} does not exist"))
    }

    /// Insert a row into body table `t` so that it becomes row `at`
    /// (default: appended). The new row copies the layout of its
    /// neighbour -- cell widths, borders, shading, merges, paragraph and
    /// character formatting of the first run -- but never a header row's
    /// "repeat on every page" flag, and holds `values` (one per cell, the
    /// rest blank).
    pub fn add_row(&mut self, t: usize, values: &[String], at: Option<usize>) -> Result<(), String> {
        let tbl = self.tbl_mut(t)?;
        let rows = row_idxs(tbl);
        let n = rows.len();
        if n == 0 {
            return Err(format!("table {t} has no rows to copy a layout from"));
        }
        let at = at.unwrap_or(n);
        if at > n {
            return Err(format!("cannot insert at row {at} -- table {t} has {n} rows (0-based; {n} appends)"));
        }
        let row_at = |tbl: &Element, r: usize| -> Element {
            match &tbl.children[rows[r]] {
                Node::Elem(e) => e.clone(),
                _ => unreachable!(),
            }
        };
        let is_header = |tr: &Element| tr.child("w:trPr").is_some_and(|p| p.child("w:tblHeader").is_some());
        let mut mi = if at < n { at } else { n - 1 };
        let mut header_only = false;
        if is_header(&row_at(tbl, mi)) {
            match (0..n).find(|&k| !is_header(&row_at(tbl, k))) {
                Some(k) => mi = k,
                None => header_only = true,
            }
        }
        let model = row_at(tbl, mi);
        // Cells that continue a vertical merge keep continuing it when the
        // row goes into the middle of the merged block.
        let continues: Vec<bool> = if at < n && mi == at { row_cells(&model).iter().map(|c| c.vm == Vm::Continue).collect() } else { Vec::new() };
        let mut tr = Element { name: model.name.clone(), attrs: model.attrs.clone(), children: Vec::new() };
        let mut k = 0;
        for node in &model.children {
            match node {
                Node::Elem(c) if c.name == "w:trPr" => {
                    let mut pr = c.clone();
                    for gone in ["w:tblHeader", "w:cnfStyle", "w:ins", "w:del", "w:trPrChange"] {
                        pr.remove_children(gone);
                    }
                    if !pr.children.iter().all(|x| matches!(x, Node::Text(s) if s.trim().is_empty())) && pr.elems().next().is_some() {
                        tr.children.push(Node::Elem(pr));
                    }
                }
                Node::Elem(c) if c.name == "w:tc" => {
                    let text = values.get(k).map(String::as_str).unwrap_or("");
                    let mut cell = new_cell_like(c, if continues.get(k).copied().unwrap_or(false) { "" } else { text }, header_only);
                    if continues.get(k).copied().unwrap_or(false) {
                        if let Some(pr) = cell.child_mut("w:tcPr") {
                            pr.remove_children("w:vMerge");
                            insert_ordered(pr, Element::new("w:vMerge"), TCPR_ORDER);
                        }
                    }
                    tr.children.push(Node::Elem(cell));
                    k += 1;
                }
                // Anything else a row can hold (a content control, a
                // bookmark) belongs to the model row, not to a new one.
                _ => {}
            }
        }
        if values.len() > k {
            return Err(format!("{} values for a row of {k} cells", values.len()));
        }
        let pos = if at < n { rows[at] } else { rows[n - 1] + 1 };
        tbl.children.insert(pos, Node::Elem(tr));
        Ok(())
    }

    /// Insert a column into body table `t` so that it becomes grid column
    /// `at` (default: appended), holding `values` (one per row, the rest
    /// blank). The table keeps its width: the existing columns shrink to
    /// make room, as Word does. Where the new column falls inside a merged
    /// cell, that cell grows to cover it and takes no value.
    pub fn add_column(&mut self, t: usize, values: &[String], at: Option<usize>) -> Result<(), String> {
        let tbl = self.tbl_mut(t)?;
        let rows = row_idxs(tbl);
        if rows.is_empty() {
            return Err(format!("table {t} has no rows"));
        }
        let widths = grid_widths(tbl);
        let n = grid_count(tbl);
        let at = at.unwrap_or(n);
        if at > n {
            return Err(format!("cannot insert at column {at} -- table {t} has {n} columns (0-based; {n} appends)"));
        }
        if values.len() > rows.len() {
            return Err(format!("{} values for a table of {} rows", values.len(), rows.len()));
        }
        let total: i64 = widths.iter().sum();
        let resize = widths.len() == n && n > 0 && total > 0;
        let scale = if resize { n as f64 / (n as f64 + 1.0) } else { 1.0 };
        let new_w = if resize { (total as f64 / (n as f64 + 1.0)).round() as i64 } else { 0 };
        for (r, &ri) in rows.iter().enumerate() {
            let Node::Elem(tr) = &mut tbl.children[ri] else { unreachable!() };
            if resize {
                for c in tr.elems_mut().filter(|c| c.name == "w:tc") {
                    if let Some(w) = tcw(c) {
                        set_tcw(c, (w as f64 * scale).round() as i64);
                    }
                }
            }
            let cells = row_cells(tr);
            if cells.is_empty() {
                continue;
            }
            if let Some(c) = cells.iter().find(|c| c.start < at && at < c.start + c.span) {
                let Node::Elem(tc) = &mut tr.children[c.idx] else { unreachable!() };
                let pr = tc.ensure_first_child("w:tcPr");
                set_attr_child(pr, TCPR_ORDER, "w:gridSpan", &[("w:val", &(c.span + 1).to_string())]);
                if resize {
                    if let Some(w) = tcw(tc) {
                        set_tcw(tc, w + new_w);
                    }
                }
                continue;
            }
            let nb = cells.iter().find(|c| c.start + c.span == at && at > 0).or_else(|| cells.iter().find(|c| c.start == at)).or(cells.last()).copied().unwrap();
            let Node::Elem(src) = &tr.children[nb.idx] else { unreachable!() };
            let mut cell = new_cell_like(src, values.get(r).map(String::as_str).unwrap_or(""), false);
            if resize {
                set_tcw(&mut cell, new_w);
            }
            let pos = match cells.iter().find(|c| c.start == at) {
                Some(c) => c.idx,
                None => cells.last().map(|c| c.idx + 1).unwrap_or(tr.children.len()),
            };
            tr.children.insert(pos, Node::Elem(cell));
        }
        if let Some(grid) = tbl.child_mut("w:tblGrid") {
            if resize {
                let mut w: Vec<i64> = widths.iter().map(|&x| (x as f64 * scale).round() as i64).collect();
                w.insert(at, new_w);
                // Rounding must not move the table's right edge.
                let drift = total - w.iter().sum::<i64>();
                if let Some(last) = w.last_mut() {
                    *last += drift;
                }
                let col_pos: Vec<usize> = grid.children.iter().enumerate().filter(|(_, x)| matches!(x, Node::Elem(e) if e.name == "w:gridCol")).map(|(i, _)| i).collect();
                for (k, &ci) in col_pos.iter().enumerate() {
                    let nk = if k >= at { k + 1 } else { k };
                    if let Node::Elem(e) = &mut grid.children[ci] {
                        e.set_attr("w:w", &w[nk].to_string());
                    }
                }
                let ins = col_pos.get(at).copied().unwrap_or_else(|| col_pos.last().map(|x| x + 1).unwrap_or(grid.children.len()));
                grid.children.insert(ins, Node::Elem(Element::new("w:gridCol").with_attr("w:w", &w[at].to_string())));
            } else {
                let w = widths.get(at.saturating_sub(1)).copied().unwrap_or(0).to_string();
                let col_pos: Vec<usize> = grid.children.iter().enumerate().filter(|(_, x)| matches!(x, Node::Elem(e) if e.name == "w:gridCol")).map(|(i, _)| i).collect();
                let ins = col_pos.get(at).copied().unwrap_or_else(|| col_pos.last().map(|x| x + 1).unwrap_or(grid.children.len()));
                grid.children.insert(ins, Node::Elem(Element::new("w:gridCol").with_attr("w:w", &w)));
            }
        }
        Ok(())
    }

    /// Merge the rectangle of grid cells from (`r1`,`c1`) to (`r2`,`c2`),
    /// inclusive and 0-based, into one cell: a horizontal span where the
    /// range crosses columns, a vertical merge where it crosses rows. The
    /// merged cell keeps the top-left cell's formatting; the text of the
    /// other cells follows it, one paragraph each, empty ones dropped.
    /// Columns here are GRID columns, which for a table with no merged
    /// cells are the columns `tables()` shows.
    pub fn merge_cells(&mut self, t: usize, r1: usize, c1: usize, r2: usize, c2: usize) -> Result<(), String> {
        if r1 > r2 || c1 > c2 {
            return Err(format!("the range runs backwards -- give the top-left corner first (row {r1}, column {c1} to row {r2}, column {c2})"));
        }
        if r1 == r2 && c1 == c2 {
            return Err("nothing to merge -- the range is a single cell".into());
        }
        let tbl = self.tbl_mut(t)?;
        let rows = row_idxs(tbl);
        if r2 >= rows.len() {
            return Err(format!("table {t} has {} rows -- row {r2} does not exist (0-based)", rows.len()));
        }
        let ncols = grid_count(tbl);
        if c2 >= ncols {
            return Err(format!("table {t} has {ncols} columns -- column {c2} does not exist (0-based)"));
        }
        let cells_of = |tbl: &Element, r: usize| -> Vec<CellInfo> {
            match &tbl.children[rows[r]] {
                Node::Elem(tr) => row_cells(tr),
                _ => unreachable!(),
            }
        };
        // Check the whole range before touching anything.
        for r in r1..=r2 {
            let cells = cells_of(tbl, r);
            let first = cells.iter().find(|c| c.start <= c1 && c1 < c.start + c.span).ok_or_else(|| format!("row {r} has no cell at column {c1}"))?;
            let last = cells.iter().find(|c| c.start <= c2 && c2 < c.start + c.span).ok_or_else(|| format!("row {r} has no cell at column {c2}"))?;
            if first.start != c1 || last.start + last.span - 1 != c2 {
                return Err(format!("the range cuts through a merged cell in row {r} -- widen it to take in the whole cell (columns {}-{})", first.start, last.start + last.span - 1));
            }
            let inside = cells.iter().filter(|c| c.start >= c1 && c.start + c.span - 1 <= c2);
            if r == r1 && inside.clone().any(|c| c.vm == Vm::Continue) {
                return Err(format!("the range starts in the middle of a vertically merged cell -- start at its top row"));
            }
        }
        if r2 + 1 < rows.len() && cells_of(tbl, r2 + 1).iter().any(|c| c.start >= c1 && c.start + c.span - 1 <= c2 && c.vm == Vm::Continue) {
            return Err("the range ends in the middle of a vertically merged cell -- extend it to the cell's last row".into());
        }
        let widths = grid_widths(tbl);
        let span = c2 - c1 + 1;
        // The text that follows the anchor cell.
        let mut carried: Vec<Element> = Vec::new();
        for r in r1..=r2 {
            let cells = cells_of(tbl, r);
            let Node::Elem(tr) = &tbl.children[rows[r]] else { unreachable!() };
            for c in cells.iter().filter(|c| c.start >= c1 && c.start + c.span - 1 <= c2) {
                if r == r1 && c.start == c1 {
                    continue;
                }
                let Node::Elem(tc) = &tr.children[c.idx] else { unreachable!() };
                carried.extend(tc.elems().filter(|p| p.name == "w:p" && !blank_para(p)).cloned());
            }
        }
        for r in r1..=r2 {
            let cells = cells_of(tbl, r);
            let mine: Vec<CellInfo> = cells.into_iter().filter(|c| c.start >= c1 && c.start + c.span - 1 <= c2).collect();
            let Node::Elem(tr) = &mut tbl.children[rows[r]] else { unreachable!() };
            let width = if widths.len() >= c2 + 1 {
                Some(widths[c1..=c2].iter().sum::<i64>())
            } else {
                let ws: Vec<Option<i64>> = mine.iter().map(|c| match &tr.children[c.idx] {
                    Node::Elem(tc) => tcw(tc),
                    _ => None,
                }).collect();
                ws.iter().copied().sum::<Option<i64>>()
            };
            for c in mine[1..].iter().rev() {
                tr.children.remove(c.idx);
            }
            let Node::Elem(tc) = &mut tr.children[mine[0].idx] else { unreachable!() };
            {
                let pr = tc.ensure_first_child("w:tcPr");
                if span > 1 {
                    set_attr_child(pr, TCPR_ORDER, "w:gridSpan", &[("w:val", &span.to_string())]);
                } else {
                    pr.remove_children("w:gridSpan");
                }
                pr.remove_children("w:vMerge");
                if r1 < r2 {
                    let mut vm = Element::new("w:vMerge");
                    if r == r1 {
                        vm.set_attr("w:val", "restart");
                    }
                    insert_ordered(pr, vm, TCPR_ORDER);
                }
            }
            if let Some(w) = width {
                set_tcw(tc, w);
            }
            if r == r1 {
                if !carried.is_empty() {
                    if tc.elems().filter(|p| p.name == "w:p").all(blank_para) {
                        tc.children.retain(|n| !matches!(n, Node::Elem(p) if p.name == "w:p"));
                    }
                    tc.children.extend(carried.drain(..).map(Node::Elem));
                }
                if !tc.elems().any(|p| p.name == "w:p") {
                    tc.children.push(Node::Elem(Element::new("w:p")));
                }
            } else {
                let ppr = tc.elems().find(|p| p.name == "w:p").and_then(|p| p.child("w:pPr")).cloned();
                tc.children.retain(|n| !matches!(n, Node::Elem(p) if p.name != "w:tcPr"));
                let mut p = Element::new("w:p");
                if let Some(ppr) = ppr {
                    p.children.push(Node::Elem(ppr));
                }
                tc.children.push(Node::Elem(p));
            }
        }
        Ok(())
    }

    /// Undo a merge, or cut a plain cell into columns.
    ///
    /// Without `cols`, the merged cell containing grid cell (`row`,`col`)
    /// is split back into its individual cells: its text stays in the
    /// top-left one. With `cols` = N (2 or more) a cell that is not merged
    /// is cut into N side-by-side cells; the other rows keep their layout
    /// by spanning the new columns.
    pub fn split_cell(&mut self, t: usize, row: usize, col: usize, cols: Option<usize>) -> Result<(), String> {
        let tbl = self.tbl_mut(t)?;
        let rows = row_idxs(tbl);
        if row >= rows.len() {
            return Err(format!("table {t} has {} rows -- row {row} does not exist (0-based)", rows.len()));
        }
        let cells_of = |tbl: &Element, r: usize| -> Vec<CellInfo> {
            match &tbl.children[rows[r]] {
                Node::Elem(tr) => row_cells(tr),
                _ => unreachable!(),
            }
        };
        let here = *cells_of(tbl, row).iter().find(|c| c.start <= col && col < c.start + c.span).ok_or_else(|| format!("table {t} row {row} has no cell at column {col}"))?;
        let widths = grid_widths(tbl);
        // The vertical extent of the cell's merge, if any.
        let st = here.start;
        let starts_at = |tbl: &Element, r: usize| cells_of(tbl, r).into_iter().find(|c| c.start == st);
        let mut top = row;
        while top > 0 && starts_at(tbl, top).is_some_and(|c| c.vm == Vm::Continue) {
            top -= 1;
        }
        let mut bottom = row;
        while bottom + 1 < rows.len() && starts_at(tbl, bottom + 1).is_some_and(|c| c.vm == Vm::Continue) {
            bottom += 1;
        }
        let span = starts_at(tbl, top).map(|c| c.span).unwrap_or(here.span);
        match cols {
            None => {
                if top == bottom && span == 1 {
                    return Err(format!("cell ({row}, {col}) is not merged -- to cut a cell into several side-by-side cells give cols=N"));
                }
                for r in top..=bottom {
                    let Some(c) = starts_at(tbl, r) else { continue };
                    let Node::Elem(tr) = &mut tbl.children[rows[r]] else { unreachable!() };
                    let Node::Elem(orig) = tr.children[c.idx].clone() else { unreachable!() };
                    let parts: Vec<i64> = if widths.len() >= st + span { widths[st..st + span].to_vec() } else { even_split(tcw(&orig), span) };
                    let mut new: Vec<Node> = Vec::new();
                    for (k, w) in parts.iter().enumerate() {
                        let mut cell = if k == 0 && r == top { orig.clone() } else { new_cell_like(&orig, "", false) };
                        let pr = cell.ensure_first_child("w:tcPr");
                        pr.remove_children("w:gridSpan");
                        pr.remove_children("w:vMerge");
                        if widths.len() >= st + span || tcw(&orig).is_some() {
                            set_tcw(&mut cell, *w);
                        }
                        new.push(Node::Elem(cell));
                    }
                    tr.children.splice(c.idx..=c.idx, new);
                }
                Ok(())
            }
            Some(n) => {
                if n < 2 {
                    return Err(format!("cols={n} -- a cell is cut into 2 or more"));
                }
                if top != bottom || span != 1 || here.vm != Vm::No {
                    return Err("that cell is merged -- split_cell without cols= undoes the merge first".into());
                }
                if widths.is_empty() || st >= widths.len() || tbl.child("w:tblGrid").is_none() {
                    return Err(format!("table {t} has no column grid to divide"));
                }
                for r in 0..rows.len() {
                    if r != row && !cells_of(tbl, r).iter().any(|c| c.start <= st && st < c.start + c.span) {
                        return Err(format!("row {r} pads the grid at column {st} (gridBefore/gridAfter) -- not supported"));
                    }
                }
                let parts = even_split(Some(widths[st]), n);
                for r in 0..rows.len() {
                    let cells = cells_of(tbl, r);
                    let c = *cells.iter().find(|c| c.start <= st && st < c.start + c.span).unwrap();
                    let Node::Elem(tr) = &mut tbl.children[rows[r]] else { unreachable!() };
                    if r != row {
                        let Node::Elem(tc) = &mut tr.children[c.idx] else { unreachable!() };
                        let pr = tc.ensure_first_child("w:tcPr");
                        set_attr_child(pr, TCPR_ORDER, "w:gridSpan", &[("w:val", &(c.span + n - 1).to_string())]);
                    } else {
                        let Node::Elem(orig) = tr.children[c.idx].clone() else { unreachable!() };
                        let mut new: Vec<Node> = Vec::new();
                        for (k, w) in parts.iter().enumerate() {
                            let mut cell = if k == 0 { orig.clone() } else { new_cell_like(&orig, "", false) };
                            set_tcw(&mut cell, *w);
                            new.push(Node::Elem(cell));
                        }
                        tr.children.splice(c.idx..=c.idx, new);
                    }
                }
                let grid = tbl.child_mut("w:tblGrid").unwrap();
                let col_pos: Vec<usize> = grid.children.iter().enumerate().filter(|(_, x)| matches!(x, Node::Elem(e) if e.name == "w:gridCol")).map(|(i, _)| i).collect();
                let Some(&at) = col_pos.get(st) else {
                    return Err(format!("table {t} has fewer grid columns than its rows use (column {st}) -- not supported"));
                };
                let Node::Elem(proto) = grid.children[at].clone() else { unreachable!() };
                let new: Vec<Node> = parts
                    .iter()
                    .map(|w| {
                        let mut g = proto.clone();
                        g.set_attr("w:w", &w.to_string());
                        Node::Elem(g)
                    })
                    .collect();
                grid.children.splice(at..=at, new);
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------ formatting

    /// Apply character formatting to existing text: the first occurrence
    /// of `on` in body paragraph `i`, every occurrence with `all`, or the
    /// whole paragraph when `on` is `None`. Runs are cut at the span's
    /// edges (the pieces outside keep their formatting exactly) and only
    /// the properties given are changed in the runs inside. Returns how
    /// many spans were formatted.
    pub fn format_text(&mut self, i: usize, on: Option<&str>, all: bool, fmt: &RunFormat) -> Result<usize, String> {
        if fmt.is_empty() {
            return Err("nothing to apply -- give bold=, italic=, underline=, strike=, size=, color= or font=".into());
        }
        let color = fmt.color.as_deref().map(|c| if c.eq_ignore_ascii_case("auto") { Ok("auto".to_string()) } else { hex_color(c) }).transpose()?;
        let half = match fmt.size {
            Some(sz) if !(sz > 0.0 && sz <= 1638.0) => return Err(format!("size={sz} pt is outside what Word stores (0-1638 pt)")),
            Some(sz) => Some(((sz * 2.0).round() as i64).to_string()),
            None => None,
        };
        let slot = self.para_slot(i)?;
        let p = self.para_at(slot);
        let spans = find_spans(p, on, all, i)?;
        split_at_all(p, &spans)?;
        let mut acc = 0;
        each_run(p, &mut acc, &mut |r, s, e| {
            if e > s && spans.iter().any(|&(a, b)| s >= a && e <= b) {
                let rpr = r.ensure_first_child("w:rPr");
                if let Some(v) = fmt.bold {
                    toggle(rpr, "w:b", Some("w:bCs"), v);
                }
                if let Some(v) = fmt.italic {
                    toggle(rpr, "w:i", Some("w:iCs"), v);
                }
                if let Some(v) = fmt.strike {
                    toggle(rpr, "w:strike", None, v);
                }
                if let Some(v) = fmt.underline {
                    set_attr_child(rpr, RPR_ORDER, "w:u", &[("w:val", if v { "single" } else { "none" })]);
                }
                if let Some(c) = &color {
                    set_attr_child(rpr, RPR_ORDER, "w:color", &[("w:val", c)]);
                }
                if let Some(h) = &half {
                    set_attr_child(rpr, RPR_ORDER, "w:sz", &[("w:val", h)]);
                    set_attr_child(rpr, RPR_ORDER, "w:szCs", &[("w:val", h)]);
                }
                if let Some(f) = &fmt.font {
                    set_attr_child(rpr, RPR_ORDER, "w:rFonts", &[("w:ascii", f), ("w:hAnsi", f), ("w:eastAsia", f), ("w:cs", f)]);
                }
            }
        });
        Ok(spans.len())
    }

    /// Set line spacing and/or the space before and after for body
    /// paragraphs `i` through `to` (default: just `i`). Returns how many
    /// paragraphs were changed.
    pub fn line_spacing(&mut self, i: usize, to: Option<usize>, sp: &Spacing) -> Result<usize, String> {
        let kinds = [sp.lines.is_some(), sp.exactly.is_some(), sp.at_least.is_some()].iter().filter(|b| **b).count();
        if kinds > 1 {
            return Err("give one of lines=, exactly= or at_least=, not several".into());
        }
        if kinds == 0 && sp.before.is_none() && sp.after.is_none() {
            return Err("nothing to set -- give lines=, exactly=, at_least=, before= or after=".into());
        }
        if let Some(l) = sp.lines {
            if !(l > 0.0 && l <= 132.0) {
                return Err(format!("lines={l} -- Word takes a multiple above 0 and up to 132"));
            }
        }
        for (name, v) in [("exactly", sp.exactly), ("at_least", sp.at_least)] {
            if let Some(v) = v {
                if !(v > 0.0 && v <= 1584.0) {
                    return Err(format!("{name}={v} pt -- Word takes a line pitch above 0 and up to 1584 pt"));
                }
            }
        }
        for (name, v) in [("before", sp.before), ("after", sp.after)] {
            if let Some(v) = v {
                if !(0.0..=1584.0).contains(&v) {
                    return Err(format!("{name}={v} pt -- Word takes 0 to 1584 pt"));
                }
            }
        }
        let to = to.unwrap_or(i);
        if to < i {
            return Err(format!("to={to} comes before paragraph {i}"));
        }
        let slots: Vec<usize> = (i..=to).map(|k| self.para_slot(k)).collect::<Result<_, _>>()?;
        let tw = |pt: f64| ((pt * 20.0).round() as i64).to_string();
        for slot in &slots {
            let p = self.para_at(*slot);
            let ppr = p.ensure_first_child("w:pPr");
            if ppr.child("w:spacing").is_none() {
                insert_ordered(ppr, Element::new("w:spacing"), PPR_ORDER);
            }
            let s = ppr.child_mut("w:spacing").unwrap();
            if let Some(l) = sp.lines {
                s.set_attr("w:line", &((l * 240.0).round() as i64).to_string());
                s.set_attr("w:lineRule", "auto");
            }
            if let Some(v) = sp.exactly {
                s.set_attr("w:line", &tw(v));
                s.set_attr("w:lineRule", "exact");
            }
            if let Some(v) = sp.at_least {
                s.set_attr("w:line", &tw(v));
                s.set_attr("w:lineRule", "atLeast");
            }
            if let Some(v) = sp.before {
                s.set_attr("w:before", &tw(v));
                s.remove_attr("w:beforeLines");
                s.remove_attr("w:beforeAutospacing");
            }
            if let Some(v) = sp.after {
                s.set_attr("w:after", &tw(v));
                s.remove_attr("w:afterLines");
                s.remove_attr("w:afterAutospacing");
            }
        }
        Ok(slots.len())
    }

    /// Move body paragraph `from` so that it becomes body paragraph `to`
    /// (the list `paragraphs()` returns, after the move). Tables and other
    /// block content stay where they are. A paragraph that ends a section
    /// carries the section break with it, so it is refused.
    pub fn move_paragraph(&mut self, from: usize, to: usize) -> Result<(), String> {
        let slots = self.para_slots();
        let n = slots.len();
        if from >= n {
            return Err(format!("paragraph {from} does not exist -- the document has {n} (0-based)"));
        }
        if to >= n {
            return Err(format!("cannot move to paragraph {to} -- the document has {n}; the moved paragraph ends up at index `to`"));
        }
        if from == to {
            return Ok(());
        }
        if let Node::Elem(p) = &self.body().children[slots[from]] {
            if p.child("w:pPr").is_some_and(|x| x.child("w:sectPr").is_some()) {
                return Err(format!("paragraph {from} ends a section (it holds the section break) -- moving it would move the break"));
            }
        }
        let node = self.body_mut().children.remove(slots[from]);
        let now = self.para_slots();
        let pos = if to < now.len() { now[to] } else { now[now.len() - 1] + 1 };
        self.body_mut().children.insert(pos, node);
        Ok(())
    }

    // ------------------------------------------------------------ footnotes

    /// Create a footnote with `text` anchored after the first occurrence of
    /// `on` in body paragraph `i` (or at the end of the paragraph). Word
    /// numbers footnotes by position. Creates the footnotes part, the
    /// separator notes and the note styles when the document has none.
    pub fn add_footnote(&mut self, i: usize, text: &str, on: Option<&str>) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("the footnote is empty".into());
        }
        let slot = self.para_slot(i)?;
        let place = {
            let p = self.para_at(slot);
            match on {
                None => None,
                Some(_) => {
                    let spans = find_spans(p, on, false, i)?;
                    let end = spans[0].1;
                    split_at_all(p, &[(end, end)])?;
                    Some(insertion_point(p, end, &["w:fldSimple"])?)
                }
            }
        };
        let ref_style = self.ensure_char_style("footnote reference", "FootnoteReference", footnote_reference_style)?;
        let text_style = self.ensure_char_style("footnote text", "FootnoteText", footnote_text_style)?;
        let part = match self.related_part("/footnotes") {
            Some(p) => p,
            None => {
                let p = self.fresh_part("word/footnotes.xml");
                let sep = |kind: &str, id: &str, mark: &str| {
                    Element::new("w:footnote").with_attr("w:type", kind).with_attr("w:id", id).with_child(
                        Element::new("w:p")
                            .with_child(Element::new("w:pPr").with_child(Element::new("w:spacing").with_attr("w:after", "0").with_attr("w:line", "240").with_attr("w:lineRule", "auto")))
                            .with_child(Element::new("w:r").with_child(Element::new(mark))),
                    )
                };
                let root = Element::new("w:footnotes")
                    .with_attr("xmlns:w", NS_W)
                    .with_attr("xmlns:r", NS_R)
                    .with_child(sep("separator", "-1", "w:separator"))
                    .with_child(sep("continuationSeparator", "0", "w:continuationSeparator"));
                self.pkg.set_xml(&p, &Doc::new(root));
                self.pkg.add_override(&p, &format!("{CT_BASE}.footnotes+xml"))?;
                self.pkg.add_rel(&self.main, &format!("{REL_BASE}/footnotes"), &qu_ooxml::relative_target(&self.main, &p))?;
                self.with_settings(|s| {
                    if s.child("w:footnotePr").is_none() {
                        let pr = Element::new("w:footnotePr").with_child(Element::new("w:footnote").with_attr("w:id", "-1")).with_child(Element::new("w:footnote").with_attr("w:id", "0"));
                        insert_ordered(s, pr, SETTINGS_ORDER);
                    }
                })?;
                p
            }
        };
        let mut d = self.pkg.get_xml(&part)?;
        let id = (d.root.elems().filter(|n| n.name == "w:footnote").filter_map(|n| n.attr("w:id")?.parse::<i64>().ok()).max().unwrap_or(0).max(0) + 1).to_string();
        let mut note = Element::new("w:footnote").with_attr("w:id", &id);
        for (li, line) in text.split('\n').enumerate() {
            let mut p = Element::new("w:p").with_child(Element::new("w:pPr").with_child(Element::new("w:pStyle").with_attr("w:val", &text_style)));
            if li == 0 {
                p = p.with_child(Element::new("w:r").with_child(Element::new("w:rPr").with_child(Element::new("w:rStyle").with_attr("w:val", &ref_style))).with_child(Element::new("w:footnoteRef")));
                p.children.push(Node::Elem(run(&format!(" {line}"), None)));
            } else {
                p.children.push(Node::Elem(run(line, None)));
            }
            note = note.with_child(p);
        }
        d.root.children.push(Node::Elem(note));
        self.pkg.set_xml(&part, &d);
        let reference = Element::new("w:r")
            .with_child(Element::new("w:rPr").with_child(Element::new("w:rStyle").with_attr("w:val", &ref_style)))
            .with_child(Element::new("w:footnoteReference").with_attr("w:id", &id));
        let p = self.para_at(slot);
        match place {
            Some((path, idx)) => qu_ooxml::at_path_mut(p, &path).children.insert(idx, Node::Elem(reference)),
            None => p.children.push(Node::Elem(reference)),
        }
        Ok(())
    }

    // ------------------------------------------------------------ tracked changes

    /// Insert `text` into body paragraph `i` as a tracked insertion (a
    /// `w:ins` by `author`, dated `date` or now): directly after the first
    /// occurrence of `after`, directly before the first occurrence of
    /// `before`, or at the end. The text takes the formatting of the run
    /// next to it. Accepting the change keeps it, rejecting removes it.
    pub fn track_insert(&mut self, i: usize, text: &str, after: Option<&str>, before: Option<&str>, author: &str, date: Option<&str>) -> Result<(), String> {
        if text.is_empty() {
            return Err("the text to insert is empty".into());
        }
        if after.is_some() && before.is_some() {
            return Err("give after= or before=, not both".into());
        }
        let id = self.next_revision_id();
        let slot = self.para_slot(i)?;
        let p = self.para_at(slot);
        let total = para_display_text(p).chars().count();
        let off = match (after, before) {
            (Some(a), _) => find_spans(p, Some(a), false, i)?[0].1,
            (_, Some(b)) => find_spans(p, Some(b), false, i)?[0].0,
            _ => total,
        };
        split_at_all(p, &[(off, off)])?;
        let (path, idx) = insertion_point(p, off, &["w:ins", "w:moveTo", "w:fldSimple"])?;
        let runs = collect_runs(p);
        let neighbour = runs.iter().filter(|r| r.end > r.start).find(|r| r.end == off && off > 0).or_else(|| runs.iter().filter(|r| r.end > r.start).find(|r| r.start == off));
        let rpr = neighbour.and_then(|r| match &at_path_ref(p, &r.path).child("w:rPr") {
            Some(x) => {
                let mut x = (*x).clone();
                x.remove_children("w:rPrChange");
                Some(x)
            }
            None => None,
        });
        let ins = Element::new("w:ins")
            .with_attr("w:id", &id.to_string())
            .with_attr("w:author", author)
            .with_attr("w:date", &date.map(String::from).unwrap_or_else(now_iso))
            .with_child(run(text, rpr));
        qu_ooxml::at_path_mut(p, &path).children.insert(idx, Node::Elem(ins));
        Ok(())
    }

    /// Mark text in body paragraph `i` as a tracked deletion: the first
    /// occurrence of `on`, every occurrence with `all`, or all of the
    /// paragraph's text -- and its paragraph mark, so accepting removes the
    /// paragraph -- when `on` is `None`. The text stays in the file as
    /// `w:delText` inside a `w:del` by `author`. Returns how many spans.
    pub fn track_delete(&mut self, i: usize, on: Option<&str>, all: bool, author: &str, date: Option<&str>) -> Result<usize, String> {
        let slot = self.para_slot(i)?;
        let mut next_id = self.next_revision_id();
        let date = date.map(String::from).unwrap_or_else(now_iso);
        let p = self.para_at(slot);
        let spans = find_spans(p, on, all, i)?;
        split_at_all(p, &spans)?;
        for r in collect_runs(p) {
            if r.end > r.start && r.in_field && spans.iter().any(|&(a, b)| r.start >= a && r.end <= b) {
                return Err("that text is (part of) a field's result -- delete the whole field's paragraph text, or choose text outside the field with on=".into());
            }
        }
        let mut acc = 0;
        wrap_deleted(p, &mut acc, &spans, &mut next_id, author, &date);
        if on.is_none() && !p.child("w:pPr").is_some_and(|x| x.child("w:sectPr").is_some()) {
            let ppr = p.ensure_first_child("w:pPr");
            if ppr.child("w:rPr").is_none() {
                insert_ordered(ppr, Element::new("w:rPr"), PPR_ORDER);
            }
            let rpr = ppr.child_mut("w:rPr").unwrap();
            rpr.remove_children("w:del");
            rpr.children.insert(0, Node::Elem(Element::new("w:del").with_attr("w:id", &next_id.to_string()).with_attr("w:author", author).with_attr("w:date", &date)));
        }
        Ok(spans.len())
    }

    /// One more than the largest `w:id` anywhere in the body, so a new
    /// revision never reuses a bookmark's, comment's or revision's id.
    fn next_revision_id(&self) -> i64 {
        fn max_id(e: &Element, max: &mut i64) {
            if let Some(v) = e.attr("w:id").and_then(|v| v.parse::<i64>().ok()) {
                *max = (*max).max(v);
            }
            for c in e.elems() {
                max_id(c, max);
            }
        }
        let mut max = -1;
        max_id(&self.doc.root, &mut max);
        max + 1
    }
}

// ------------------------------------------------------------------ helpers

fn val_of(e: Option<&Element>) -> Option<usize> {
    e.and_then(|e| e.attr("w:val")?.parse().ok())
}

/// Child indices (in the table) of its rows.
fn row_idxs(tbl: &Element) -> Vec<usize> {
    tbl.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::Elem(e) if e.name == "w:tr")).map(|(i, _)| i).collect()
}

fn row_cells(tr: &Element) -> Vec<CellInfo> {
    let mut g = val_of(tr.child("w:trPr").and_then(|p| p.child("w:gridBefore"))).unwrap_or(0);
    let mut out = Vec::new();
    for (idx, n) in tr.children.iter().enumerate() {
        let Node::Elem(c) = n else { continue };
        if c.name != "w:tc" {
            continue;
        }
        let pr = c.child("w:tcPr");
        let span = val_of(pr.and_then(|p| p.child("w:gridSpan"))).unwrap_or(1).max(1);
        let vm = match pr.and_then(|p| p.child("w:vMerge")) {
            None => Vm::No,
            Some(m) if m.attr("w:val") == Some("restart") => Vm::Restart,
            Some(_) => Vm::Continue,
        };
        out.push(CellInfo { idx, start: g, span, vm });
        g += span;
    }
    out
}

fn grid_widths(tbl: &Element) -> Vec<i64> {
    tbl.child("w:tblGrid").map(|g| g.elems().filter(|c| c.name == "w:gridCol").map(|c| c.attr("w:w").and_then(|v| v.parse().ok()).unwrap_or(0)).collect()).unwrap_or_default()
}

/// How many grid columns the table has.
fn grid_count(tbl: &Element) -> usize {
    let g = grid_widths(tbl).len();
    if g > 0 {
        return g;
    }
    tbl.elems().filter(|r| r.name == "w:tr").map(|r| row_cells(r).last().map(|c| c.start + c.span).unwrap_or(0)).max().unwrap_or(0)
}

/// A cell's width in twips, if it states one that way.
fn tcw(tc: &Element) -> Option<i64> {
    let w = tc.child("w:tcPr")?.child("w:tcW")?;
    if !matches!(w.attr("w:type"), None | Some("dxa")) {
        return None;
    }
    w.attr("w:w")?.parse().ok()
}

fn set_tcw(tc: &mut Element, w: i64) {
    let pr = tc.ensure_first_child("w:tcPr");
    set_attr_child(pr, TCPR_ORDER, "w:tcW", &[("w:w", &w.to_string()), ("w:type", "dxa")]);
}

/// `parent`'s child `name` replaced by a new one with `attrs`, in schema order.
fn set_attr_child(parent: &mut Element, order: &[&str], name: &str, attrs: &[(&str, &str)]) {
    parent.remove_children(name);
    let mut e = Element::new(name);
    for (k, v) in attrs {
        e.set_attr(k, v);
    }
    insert_ordered(parent, e, order);
}

/// A boolean run property, on or explicitly off (`w:val="0"` beats a style).
fn toggle(rpr: &mut Element, tag: &str, cs: Option<&str>, on: bool) {
    for t in std::iter::once(tag).chain(cs) {
        rpr.remove_children(t);
        let mut e = Element::new(t);
        if !on {
            e.set_attr("w:val", "0");
        }
        insert_ordered(rpr, e, RPR_ORDER);
    }
}

fn even_split(total: Option<i64>, n: usize) -> Vec<i64> {
    let total = total.unwrap_or(0);
    let each = total / n as i64;
    let mut v = vec![each; n];
    if let Some(last) = v.last_mut() {
        *last += total - each * n as i64;
    }
    v
}

/// A paragraph with no text and nothing else a reader would see.
fn blank_para(p: &Element) -> bool {
    para_display_text(p).trim().is_empty() && p.find_all("w:drawing").is_empty() && p.find_all("w:pict").is_empty() && p.find_all("w:object").is_empty()
}

/// A fresh cell laid out like `src` (properties, paragraph properties and
/// the first run's character formatting) but holding only `text` and
/// belonging to no merge. `bare` also drops bold, for a header row copied
/// into a body row.
fn new_cell_like(src: &Element, text: &str, bare: bool) -> Element {
    let mut tc = Element::new("w:tc");
    if let Some(pr) = src.child("w:tcPr") {
        let mut pr = pr.clone();
        for gone in ["w:gridSpan", "w:vMerge", "w:hMerge", "w:cnfStyle", "w:cellIns", "w:cellDel", "w:cellMerge", "w:tcPrChange"] {
            pr.remove_children(gone);
        }
        tc.children.push(Node::Elem(pr));
    }
    let first = src.elems().find(|p| p.name == "w:p");
    let mut p = Element::new("w:p");
    if let Some(ppr) = first.and_then(|p| p.child("w:pPr")) {
        let mut ppr = ppr.clone();
        for gone in ["w:sectPr", "w:pPrChange"] {
            ppr.remove_children(gone);
        }
        if let Some(rpr) = ppr.child_mut("w:rPr") {
            for gone in ["w:ins", "w:del", "w:moveFrom", "w:moveTo", "w:rPrChange"] {
                rpr.remove_children(gone);
            }
        }
        p.children.push(Node::Elem(ppr));
    }
    if !text.is_empty() {
        let rpr = first.and_then(|p| p.find_all("w:r").first().and_then(|r| r.child("w:rPr")).cloned()).map(|mut r| {
            r.remove_children("w:rPrChange");
            if bare {
                r.remove_children("w:b");
                r.remove_children("w:bCs");
            }
            r
        });
        p.children.push(Node::Elem(run(text, rpr)));
    }
    tc.children.push(Node::Elem(p));
    tc
}

// ---- text spans

/// Character offsets `(start, end)` of the occurrence(s) of `on` in the
/// paragraph's displayed text, or of all of it when `on` is `None`.
fn find_spans(p: &Element, on: Option<&str>, all: bool, i: usize) -> Result<Vec<(usize, usize)>, String> {
    let full = para_display_text(p);
    let Some(needle) = on else {
        let n = full.chars().count();
        if n == 0 {
            return Err(format!("paragraph {i} has no text"));
        }
        return Ok(vec![(0, n)]);
    };
    if needle.is_empty() {
        return Err("on=\"\" -- give the text to mark".into());
    }
    let len = needle.chars().count();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (byte, _) in full.match_indices(needle) {
        let s = full[..byte].chars().count();
        out.push((s, s + len));
        if !all {
            break;
        }
    }
    if out.is_empty() {
        return Err(format!("paragraph {i} does not contain \"{needle}\" (it reads \"{full}\")"));
    }
    Ok(out)
}

/// Containers whose contents are not editable text: deleted text (it has
/// no displayed length anyway), drawings and other embedded objects.
fn is_opaque(name: &str) -> bool {
    matches!(name, "w:pPr" | "w:rPr" | "w:del" | "w:moveFrom" | "w:drawing" | "w:pict" | "w:object" | "mc:AlternateContent" | "w:p")
}

fn run_len(r: &Element) -> usize {
    r.elems().map(child_len).sum()
}

/// Cut the run containing text offset `off` in two, wherever in the
/// paragraph it sits.
fn split_deep(el: &mut Element, off: usize, acc: &mut usize) -> Result<bool, String> {
    let mut i = 0;
    while i < el.children.len() {
        if let Node::Elem(c) = &mut el.children[i] {
            if c.name == "w:r" {
                let len = run_len(c);
                if off > *acc && off < *acc + len {
                    let (a, b) = split_run(c, off - *acc)?;
                    el.children[i] = Node::Elem(a);
                    el.children.insert(i + 1, Node::Elem(b));
                    return Ok(true);
                }
                *acc += len;
            } else if is_opaque(&c.name) {
                *acc += para_display_text_of(c).chars().count();
            } else if split_deep(c, off, acc)? {
                return Ok(true);
            }
        }
        i += 1;
    }
    Ok(false)
}

/// Make every span edge a run boundary (highest offset first, so earlier
/// cuts do not move later ones).
fn split_at_all(p: &mut Element, spans: &[(usize, usize)]) -> Result<(), String> {
    let mut offs: Vec<usize> = spans.iter().flat_map(|&(a, b)| [a, b]).collect();
    offs.sort_unstable_by(|a, b| b.cmp(a));
    offs.dedup();
    for off in offs {
        split_deep(p, off, &mut 0)?;
    }
    Ok(())
}

/// Call `f(run, start, end)` for every live run, in order, with the text
/// offsets it covers.
fn each_run(el: &mut Element, acc: &mut usize, f: &mut dyn FnMut(&mut Element, usize, usize)) {
    for n in el.children.iter_mut() {
        let Node::Elem(c) = n else { continue };
        if c.name == "w:r" {
            let len = run_len(c);
            f(c, *acc, *acc + len);
            *acc += len;
        } else if is_opaque(&c.name) {
            *acc += para_display_text_of(c).chars().count();
        } else {
            each_run(c, acc, f);
        }
    }
}

struct RunInfo {
    path: Vec<usize>,
    start: usize,
    end: usize,
    /// Part of a field: its instruction, its marks or its result.
    in_field: bool,
    /// Names of the containers between the paragraph and the run.
    anc: Vec<String>,
    /// Field nesting before and after this run (0 = outside any field).
    depth_before: usize,
    depth_after: usize,
}

fn collect_runs(p: &Element) -> Vec<RunInfo> {
    fn walk(el: &Element, path: &mut Vec<usize>, anc: &mut Vec<String>, acc: &mut usize, depth: &mut usize, out: &mut Vec<RunInfo>) {
        for (idx, n) in el.children.iter().enumerate() {
            let Node::Elem(c) = n else { continue };
            if c.name == "w:r" {
                let len = run_len(c);
                let before = *depth;
                let mut marks = false;
                for f in c.elems().filter(|x| x.name == "w:fldChar") {
                    marks = true;
                    match f.attr("w:fldCharType") {
                        Some("begin") => *depth += 1,
                        Some("end") => *depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
                let mut path_here = path.clone();
                path_here.push(idx);
                out.push(RunInfo { path: path_here, start: *acc, end: *acc + len, in_field: before > 0 || marks || anc.iter().any(|a| a == "w:fldSimple"), anc: anc.clone(), depth_before: before, depth_after: *depth });
                *acc += len;
            } else if is_opaque(&c.name) {
                *acc += para_display_text_of(c).chars().count();
            } else {
                path.push(idx);
                anc.push(c.name.clone());
                walk(c, path, anc, acc, depth, out);
                anc.pop();
                path.pop();
            }
        }
    }
    let mut out = Vec::new();
    walk(p, &mut Vec::new(), &mut Vec::new(), &mut 0, &mut 0, &mut out);
    out
}

fn at_path_ref<'a>(p: &'a Element, path: &[usize]) -> &'a Element {
    qu_ooxml::at_path(p, path)
}

/// Where a node must go to sit at text offset `off` (already a run
/// boundary): `(path of the parent, child index)`. After the text run that
/// ends at `off`, lifted out of any container that ends exactly there (so
/// a boundary at a link's edge is outside the link); at offset 0, before
/// the first text run, lifted likewise; at the paragraph's end, last.
/// Refuses a spot inside a field, or inside one of the `forbidden`
/// containers.
fn insertion_point(p: &Element, off: usize, forbidden: &[&str]) -> Result<(Vec<usize>, usize), String> {
    let runs = collect_runs(p);
    let total = para_display_text(p).chars().count();
    let text: Vec<&RunInfo> = runs.iter().filter(|r| r.end > r.start).collect();
    if off >= total || text.is_empty() {
        return Ok((Vec::new(), p.children.len()));
    }
    let (anchor, after) = if off > 0 {
        let prev = text.iter().rev().find(|r| r.end == off).ok_or("could not place the edit at that text boundary")?;
        if prev.depth_after > 0 {
            return Err("that position is inside a field -- anchor on text outside it".into());
        }
        (*prev, true)
    } else {
        let next = text.iter().find(|r| r.start == 0).ok_or("could not place the edit at that text boundary")?;
        if next.depth_before > 0 {
            return Err("that position is inside a field -- anchor on text outside it".into());
        }
        (*next, false)
    };
    let mut depth = anchor.anc.len();
    while depth > 0 {
        let prefix = &anchor.path[..depth];
        let edge = text.iter().filter(|r| r.path.starts_with(prefix)).all(|r| if after { r.end <= off } else { r.start >= off });
        if edge {
            depth -= 1;
        } else {
            break;
        }
    }
    for name in &anchor.anc[..depth] {
        if forbidden.contains(&name.as_str()) {
            return Err(match name.as_str() {
                "w:ins" | "w:moveTo" => "that position is inside a tracked insertion -- anchor on text outside it".to_string(),
                _ => "that position is inside a field -- anchor on text outside it".to_string(),
            });
        }
    }
    Ok((anchor.path[..depth].to_vec(), anchor.path[depth] + usize::from(after)))
}

/// Wrap runs lying inside `spans` in `w:del`, turning their `w:t` into
/// `w:delText`; adjacent runs share one wrapper.
fn wrap_deleted(el: &mut Element, acc: &mut usize, spans: &[(usize, usize)], next_id: &mut i64, author: &str, date: &str) {
    let old = std::mem::take(&mut el.children);
    let mut out: Vec<Node> = Vec::with_capacity(old.len());
    let mut open = false;
    for node in old {
        match node {
            Node::Elem(mut c) => {
                if c.name == "w:r" {
                    let len = run_len(&c);
                    let (s, e) = (*acc, *acc + len);
                    *acc += len;
                    if len > 0 && spans.iter().any(|&(a, b)| s >= a && e <= b) {
                        c.walk_mut(&mut |x| {
                            if x.name == "w:t" {
                                x.name = "w:delText".into();
                                if x.text().starts_with(char::is_whitespace) || x.text().ends_with(char::is_whitespace) {
                                    x.set_attr("xml:space", "preserve");
                                }
                            }
                        });
                        if open {
                            if let Some(Node::Elem(w)) = out.last_mut() {
                                w.children.push(Node::Elem(c));
                            }
                        } else {
                            out.push(Node::Elem(Element::new("w:del").with_attr("w:id", &next_id.to_string()).with_attr("w:author", author).with_attr("w:date", date).with_child(c)));
                            *next_id += 1;
                            open = true;
                        }
                        continue;
                    }
                    open = false;
                    out.push(Node::Elem(c));
                } else if is_opaque(&c.name) {
                    *acc += para_display_text_of(&c).chars().count();
                    open = false;
                    out.push(Node::Elem(c));
                } else {
                    wrap_deleted(&mut c, acc, spans, next_id, author, date);
                    open = false;
                    out.push(Node::Elem(c));
                }
            }
            other => {
                open = false;
                out.push(other);
            }
        }
    }
    el.children = out;
}

fn footnote_reference_style(id: &str) -> Element {
    Element::new("w:style")
        .with_attr("w:type", "character")
        .with_attr("w:styleId", id)
        .with_child(Element::new("w:name").with_attr("w:val", "footnote reference"))
        .with_child(Element::new("w:uiPriority").with_attr("w:val", "99"))
        .with_child(Element::new("w:unhideWhenUsed"))
        .with_child(Element::new("w:rPr").with_child(Element::new("w:vertAlign").with_attr("w:val", "superscript")))
}

fn footnote_text_style(id: &str) -> Element {
    Element::new("w:style")
        .with_attr("w:type", "paragraph")
        .with_attr("w:styleId", id)
        .with_child(Element::new("w:name").with_attr("w:val", "footnote text"))
        .with_child(Element::new("w:basedOn").with_attr("w:val", "Normal"))
        .with_child(Element::new("w:uiPriority").with_attr("w:val", "99"))
        .with_child(Element::new("w:unhideWhenUsed"))
        .with_child(Element::new("w:pPr").with_child(Element::new("w:spacing").with_attr("w:after", "0").with_attr("w:line", "240").with_attr("w:lineRule", "auto")))
        .with_child(Element::new("w:rPr").with_child(Element::new("w:sz").with_attr("w:val", "20")).with_child(Element::new("w:szCs").with_attr("w:val", "20")))
}

#[cfg(test)]
#[path = "editing_tests.rs"]
mod tests;
