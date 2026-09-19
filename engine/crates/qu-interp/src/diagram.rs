//! `diagram_pipeline(fn, [file=])` and `algorigram(fn, [file=])` — the
//! "smart art" layer BACKLOG.md (2026-09-10, "Ahmed: 'smart art'") found
//! missing: Qu had box/arrow/text drawing primitives (`rectangle`, `arrow`,
//! `text`, `annotate`) but no LAYOUT — every diagram's coordinates were
//! placed by hand. This module is the first real instance of that gap
//! being filled, scoped to two callers:
//!
//! - `diagram_pipeline`: renders a `|>` pipe chain (`data |> f() |> g()`)
//!   as a left-to-right box-and-arrow diagram, one box per stage.
//! - `algorigram`: renders a user function's control flow (`if`/`else`,
//!   loops) as a top-to-bottom flowchart, walking its `Stmt` body.
//!
//! Both funnel through the same low-level node/edge/`DrawOp` machinery
//! below rather than each hand-placing coordinates — the general layout
//! layer the backlog entry asked for, kept intentionally small (no
//! arbitrary layout "intent" like a grid or a fan-out yet, just the two
//! shapes these two callers need) rather than speculatively broad.
//!
//! Output is SVG only. A whole-figure PNG needs a real rasterizer Qu
//! doesn't have — `savefig`'s own `.png` path already refuses for the same
//! reason (see its `rejects_full_figure_png_with_a_clear_scoped_error`
//! test) — so this module refuses it the same way rather than silently
//! writing something else when a `.png` `file=` is asked for.

use std::fmt::Write as _;

use qu_syntax::{Arg, Expr, FnBody, Param, Stmt};

use crate::plotting::{measure_text, write_svg_op, Anchor, DrawOp};
use crate::{e, EvalError, R};

// ---------------------------------------------------------------------
// Shared node/edge model + layout
// ---------------------------------------------------------------------

const FONT_SIZE: f64 = 13.0;
const PAD_X: f64 = 14.0;
const MIN_W: f64 = 150.0;
const MAX_W: f64 = 360.0;
const BOX_H: f64 = 44.0;
const DIAMOND_H: f64 = 64.0;
const JOIN_R: f64 = 5.0;
/// Vertical distance between one row's center and the next. Fixed rather
/// than measured per-row (which would need every node's height known
/// before any node below it is placed) — comfortably larger than the
/// tallest node (`DIAMOND_H`) plus a real gap for the arrow and its label.
const ROW_PITCH: f64 = 100.0;
/// Horizontal gap between two sibling branch columns (an `if`'s `then`/
/// `else`, or two `select case` arms) at their closest edges.
const H_GAP: f64 = 50.0;
const MARGIN: f64 = 40.0;

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Terminal,
    Process,
    Decision,
    Join,
}

struct FlowNode {
    shape: Shape,
    label: String,
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
}

struct FlowEdge {
    from: usize,
    to: usize,
    label: Option<String>,
    /// A loop's "repeat" edge, drawn as a routed elbow to the side rather
    /// than straight through whatever sits between the two nodes.
    back: bool,
}

struct Ctx {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
}

impl Ctx {
    fn new() -> Self {
        Ctx { nodes: Vec::new(), edges: Vec::new() }
    }

    fn add_node(&mut self, shape: Shape, label: &str, cx: f64, cy: f64) -> usize {
        let (w, h) = match shape {
            Shape::Join => (JOIN_R * 2.0, JOIN_R * 2.0),
            Shape::Decision => (box_width(label), DIAMOND_H),
            _ => (box_width(label), BOX_H),
        };
        self.nodes.push(FlowNode { shape, label: label.to_string(), cx, cy, w, h });
        self.nodes.len() - 1
    }

    fn add_edge(&mut self, from: usize, to: usize, label: Option<String>, back: bool) {
        self.edges.push(FlowEdge { from, to, label, back });
    }
}

/// A label's box width: wide enough for its text (estimated the same way
/// every other Qu label is, via `plotting::measure_text`), clamped so one
/// long argument list can't blow the whole diagram out sideways — text
/// past `MAX_W` wraps at draw time instead (see `write_label_ops`).
fn box_width(label: &str) -> f64 {
    let text_w = measure_text(label, FONT_SIZE, None) + PAD_X * 2.0;
    text_w.max(MIN_W).min(MAX_W)
}

/// Truncates a label that would need more than `max_chars` — box text is a
/// diagram annotation, not the source of truth, so a long expression
/// reads better shortened than it would wrapped across many lines.
fn shorten(label: String, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label;
    }
    let mut s: String = label.chars().take(max_chars.saturating_sub(1)).collect();
    s.push('…');
    s
}

// ---------------------------------------------------------------------
// `Expr`/`Arg` -> a short, readable label. Not a round-trippable printer
// (no operator-precedence parenthesization) — good enough for a box
// caption, same bar `display_value` sets for a runtime value.
// ---------------------------------------------------------------------

pub(crate) fn expr_label(ex: &Expr) -> String {
    match ex {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => crate::fmt_num(*f),
        Expr::Imag(f) => format!("{}i", crate::fmt_num(*f)),
        Expr::Str(s) | Expr::RawStr(s) => format!("\"{s}\""),
        Expr::Unit(v, u) => format!("{} {u}", crate::fmt_num(*v)),
        Expr::Bool(b) => b.to_string(),
        Expr::None => "none".to_string(),
        Expr::Name(n) => n.clone(),
        Expr::Unary { op, rhs } => format!("{op}{}", expr_label(rhs)),
        Expr::Binary { op, lhs, rhs } => format!("{} {op} {}", expr_label(lhs), expr_label(rhs)),
        Expr::Range { start, end, step: None } => format!("{} to {}", expr_label(start), expr_label(end)),
        Expr::Range { start, end, step: Some(s) } => {
            format!("{} to {} step {}", expr_label(start), expr_label(end), expr_label(s))
        }
        Expr::InUnit { value, unit } => format!("{} in {unit}", expr_label(value)),
        Expr::As { value, contract } => format!("{} as {}", expr_label(value), expr_label(contract)),
        Expr::Pipe { value, stage } => format!("{} |> {}", expr_label(value), expr_label(stage)),
        Expr::Ternary { cond, then, else_ } => {
            format!("{} ? {} : {}", expr_label(cond), expr_label(then), expr_label(else_))
        }
        Expr::Coalesce { lhs, rhs } => format!("{} ?? {}", expr_label(lhs), expr_label(rhs)),
        Expr::Call { callee, args } => {
            format!("{}({})", expr_label(callee), args.iter().map(arg_label).collect::<Vec<_>>().join(", "))
        }
        Expr::Index { value, .. } => format!("{}[…]", expr_label(value)),
        Expr::Field { value, name } => format!("{}.{name}", expr_label(value)),
        Expr::Transpose { value, conjugate } => format!("{}{}", expr_label(value), if *conjugate { "'" } else { ".'" }),
        Expr::Matrix(_) => "[matrix]".to_string(),
        Expr::Record(_) => "{record}".to_string(),
        Expr::Tuple(items) => format!("({})", items.iter().map(expr_label).collect::<Vec<_>>().join(", ")),
        Expr::Lambda { .. } => "<lambda>".to_string(),
        _ => "<expr>".to_string(),
    }
}

fn arg_label(a: &Arg) -> String {
    match a {
        Arg::Pos(ex) => expr_label(ex),
        Arg::Named(name, ex) => format!("{name}={}", expr_label(ex)),
    }
}

fn params_label(params: &[Param]) -> String {
    params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------
// Feature 1: `diagram_pipeline` — a `|>` chain as a left-to-right chain
// of boxes. A pipe chain never branches, so this needs none of the
// recursive width/merge machinery below — it is its own, much simpler,
// straight-line layout.
// ---------------------------------------------------------------------

/// Flattens a left-associative `Pipe` chain (`Pipe{value: Pipe{value: a,
/// stage: b}, stage: c}`, from `a |> b |> c`) into stage labels in source
/// order: `[a, b, c]`.
fn flatten_pipe(ex: &Expr, out: &mut Vec<String>) {
    if let Expr::Pipe { value, stage } = ex {
        flatten_pipe(value, out);
        out.push(shorten(expr_label(stage), 28));
    } else {
        out.push(shorten(expr_label(ex), 28));
    }
}

pub(crate) fn render_pipeline_svg(pipeline_expr: &Expr, title: Option<&str>) -> R<String> {
    if !matches!(pipeline_expr, Expr::Pipe { .. }) {
        return e(
            "diagram_pipeline: expected a pipeline that uses |>, e.g. `() := data |> lowpass(fc=1000) |> fft()` -- \
             found a plain expression with no pipe stage",
        );
    }
    let mut stages = Vec::new();
    flatten_pipe(pipeline_expr, &mut stages);

    let mut ctx = Ctx::new();
    let cy = MARGIN + BOX_H / 2.0;
    let mut x = MARGIN;
    let mut prev: Option<usize> = None;
    for (i, label) in stages.iter().enumerate() {
        let shape = if i == 0 { Shape::Terminal } else { Shape::Process };
        let w = box_width(label);
        let cx = x + w / 2.0;
        let id = ctx.add_node(shape, label, cx, cy);
        if let Some(p) = prev {
            ctx.add_edge(p, id, None, false);
        }
        prev = Some(id);
        x += w + H_GAP;
    }
    let width = (x - H_GAP + MARGIN).max(MIN_W + 2.0 * MARGIN);
    let height = BOX_H + 2.0 * MARGIN;
    Ok(render_svg_document(&ctx, width, height, title))
}

// ---------------------------------------------------------------------
// Feature 2: `algorigram` — a function's `Stmt` body as a flowchart.
// ---------------------------------------------------------------------

/// How much horizontal room a statement list needs, so sibling branches
/// (an `if`'s `then`/`else`, `select case`'s arms, `try`/`catch`) get
/// non-overlapping columns. Recursive rather than a fixed per-branch
/// width: a branch that itself branches needs more room than one that
/// doesn't, and this is the one place that decides how much.
fn required_width(stmts: &[Stmt]) -> f64 {
    let mut w: f64 = MIN_W;
    for s in stmts {
        let sw = match s {
            Stmt::SourceLine(_) => continue,
            Stmt::If { then, else_, .. } => {
                let else_w = if else_.is_empty() { MIN_W } else { required_width(else_) };
                required_width(then) + H_GAP + else_w
            }
            Stmt::Select { arms, else_, .. } => {
                let mut total = 0.0;
                let mut n = 0usize;
                for (_, body) in arms {
                    total += required_width(body) + H_GAP;
                    n += 1;
                }
                if !else_.is_empty() {
                    total += required_width(else_) + H_GAP;
                    n += 1;
                }
                if n > 0 {
                    total -= H_GAP;
                }
                total.max(MIN_W)
            }
            Stmt::Try { body, handler, .. } => required_width(body) + H_GAP + required_width(handler),
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoLoop { body, .. } => required_width(body),
            _ => MIN_W,
        };
        w = w.max(sw);
    }
    w
}

/// One statement's own label, for the shapes that don't need special
/// branch/merge handling (an ordinary `Process` box). `None` omits the
/// statement from the diagram entirely -- for the module/declarative
/// statements (`import`, `global`, `backend`, ...) that carry no control
/// flow and would only add clutter, not for anything that actually runs.
fn stmt_label(s: &Stmt) -> Option<String> {
    let label = match s {
        Stmt::SourceLine(_) => return None,
        Stmt::Return(ex) => format!("return {}", expr_label(ex)),
        Stmt::Break(n) if *n <= 1 => "break".to_string(),
        Stmt::Break(n) => format!("break {n}"),
        Stmt::Continue => "continue".to_string(),
        Stmt::Redo => "redo".to_string(),
        Stmt::Expr(ex) => expr_label(ex),
        Stmt::Assign { name, op, rhs, .. } => format!("{name} {op}= {}", expr_label(rhs)),
        Stmt::FieldAssign { name, field, rhs } => format!("{name}.{field} = {}", expr_label(rhs)),
        Stmt::IndexAssign { name, op, rhs, .. } => format!("{name}[…] {op}= {}", expr_label(rhs)),
        Stmt::Deferred { name, rhs } => format!("{name} := {}", expr_label(rhs)),
        Stmt::LazyAssign { name, rhs } => format!("lazy {name} = {}", expr_label(rhs)),
        Stmt::RefAssign { name, rhs } => format!("ref {name} = {}", expr_label(rhs)),
        Stmt::Const { name, rhs } => format!("const {name} = {}", expr_label(rhs)),
        Stmt::Contract { name, contract } => format!("{name} as {}", expr_label(contract)),
        Stmt::UnitDecl { name, .. } => format!("unit {name}"),
        Stmt::EnumDef { name, .. } => format!("enum {name}"),
        Stmt::Command { verb, words } => {
            if words.is_empty() {
                verb.clone()
            } else {
                format!("{verb} {}", words.join(" "))
            }
        }
        // Statements that carry no control flow of their own -- shown as a
        // plain labeled box rather than a full breakdown, since walking
        // their own body/params isn't this diagram's job (`function`/`:=`
        // nested inside a body, `parallel for`, `watch`, `timer`, `pool`,
        // `unsafe`). Better a generic box than silently vanishing, which
        // would misrepresent the control flow as simpler than it is.
        Stmt::ParallelFor { .. } => "parallel for".to_string(),
        Stmt::Unsafe { .. } => "unsafe".to_string(),
        Stmt::Timer { .. } => "timer".to_string(),
        Stmt::OnElapsed { .. } => "on elapsed".to_string(),
        Stmt::Watch { .. } => "watch".to_string(),
        Stmt::Pool { .. } => "pool".to_string(),
        Stmt::Function { name, .. } => format!("function {name}"),
        Stmt::DefFn { name, .. } => format!("function {name}"),
        Stmt::Declare { .. } => "declare".to_string(),
        // Module/declarative statements: no control flow, omitted.
        Stmt::Import { .. }
        | Stmt::Global(_)
        | Stmt::Backend(_)
        | Stmt::UnitMeans { .. }
        | Stmt::IgnoreWarning(_)
        | Stmt::TypeMode { .. } => return None,
        // The five variants handled with their own branch/merge layout in
        // `layout_stmts` are matched there before falling through to this
        // function, so they never reach here in practice.
        Stmt::If { .. } | Stmt::While { .. } | Stmt::DoLoop { .. } | Stmt::For { .. } | Stmt::Select { .. } | Stmt::Try { .. } => {
            return None;
        }
    };
    Some(shorten(label, 40))
}

/// Lays out one statement list top-to-bottom starting at `(cx, y)`,
/// connecting the first node drawn to `prev` (labeling that one edge with
/// `first_label`, e.g. an `if`'s "yes"). Returns the node to connect
/// FROM next and that edge's pending label -- for a non-empty list this
/// is `(last node, None)`; for an EMPTY list (an `if` with no `else`)
/// nothing was drawn, so the caller's `first_label` passes straight
/// through unattached, for the merge step below to use instead.
fn layout_stmts(
    stmts: &[Stmt],
    cx: f64,
    mut y: f64,
    prev: usize,
    first_label: Option<String>,
    ctx: &mut Ctx,
) -> (usize, Option<String>, f64) {
    let mut prev = prev;
    let mut pending = first_label;
    for s in stmts {
        match s {
            Stmt::SourceLine(_) => continue,
            Stmt::If { cond, then, else_ } => {
                let id = ctx.add_node(Shape::Decision, &shorten(expr_label(cond), 36), cx, y);
                ctx.add_edge(prev, id, pending.take(), false);
                let branch_y = y + ROW_PITCH;
                let then_w = required_width(then);
                let else_w = if else_.is_empty() { MIN_W } else { required_width(else_) };
                let then_cx = cx - H_GAP / 2.0 - then_w / 2.0;
                let else_cx = cx + H_GAP / 2.0 + else_w / 2.0;
                let (then_exit, then_label, then_y) = if then.is_empty() {
                    (id, Some("yes".to_string()), branch_y)
                } else {
                    layout_stmts(then, then_cx, branch_y, id, Some("yes".to_string()), ctx)
                };
                let (else_exit, else_label, else_y) = if else_.is_empty() {
                    (id, Some("no".to_string()), branch_y)
                } else {
                    layout_stmts(else_, else_cx, branch_y, id, Some("no".to_string()), ctx)
                };
                let merge_y = then_y.max(else_y);
                let join = ctx.add_node(Shape::Join, "", cx, merge_y);
                ctx.add_edge(then_exit, join, then_label, false);
                ctx.add_edge(else_exit, join, else_label, false);
                prev = join;
                y = merge_y + ROW_PITCH;
            }
            Stmt::While { cond, body } => {
                let id = ctx.add_node(Shape::Decision, &shorten(format!("while {}", expr_label(cond)), 36), cx, y);
                ctx.add_edge(prev, id, pending.take(), false);
                let (exit_id, exit_label, next_y) = loop_body(body, cx, y, id, ctx);
                let _ = (exit_id, exit_label);
                prev = id;
                pending = Some("done".to_string());
                y = next_y;
            }
            Stmt::DoLoop { body, cond, until } => {
                let head = ctx.add_node(Shape::Process, "loop", cx, y);
                ctx.add_edge(prev, head, pending.take(), false);
                let body_y = y + ROW_PITCH;
                let (body_exit, _, next_y) = layout_stmts(body, cx, body_y, head, None, ctx);
                let verb = if *until { "until" } else { "while" };
                let id = ctx.add_node(Shape::Decision, &shorten(format!("{verb} {}", expr_label(cond)), 32), cx, next_y);
                ctx.add_edge(body_exit, id, None, false);
                ctx.add_edge(id, head, Some("repeat".to_string()), true);
                prev = id;
                pending = Some("done".to_string());
                y = next_y + ROW_PITCH;
            }
            Stmt::For { var, range, body, else_ } => {
                let id = ctx.add_node(Shape::Decision, &shorten(format!("for {var} in {}", expr_label(range)), 36), cx, y);
                ctx.add_edge(prev, id, pending.take(), false);
                let (_, _, mut next_y) = loop_body(body, cx, y, id, ctx);
                prev = id;
                pending = Some("done".to_string());
                if !else_.is_empty() {
                    let (else_exit, else_label, else_y) = layout_stmts(else_, cx, next_y, id, pending.take(), ctx);
                    prev = else_exit;
                    pending = else_label;
                    next_y = else_y;
                }
                y = next_y;
            }
            Stmt::Select { subject, arms, else_ } => {
                let id = ctx.add_node(Shape::Decision, &shorten(format!("select {}", expr_label(subject)), 36), cx, y);
                ctx.add_edge(prev, id, pending.take(), false);
                let branch_y = y + ROW_PITCH;
                let mut widths: Vec<f64> = arms.iter().map(|(_, body)| required_width(body)).collect();
                if !else_.is_empty() {
                    widths.push(required_width(else_));
                }
                let total: f64 = widths.iter().sum::<f64>() + H_GAP * (widths.len().saturating_sub(1)) as f64;
                let mut cursor = cx - total / 2.0;
                let mut exits = Vec::new();
                for ((values, body), w) in arms.iter().zip(widths.iter()) {
                    let arm_cx = cursor + w / 2.0;
                    cursor += w + H_GAP;
                    let label = shorten(values.iter().map(expr_label).collect::<Vec<_>>().join(", "), 20);
                    let (exit_id, exit_label, exit_y) = if body.is_empty() {
                        (id, Some(label), branch_y)
                    } else {
                        layout_stmts(body, arm_cx, branch_y, id, Some(label), ctx)
                    };
                    exits.push((exit_id, exit_label, exit_y));
                }
                if !else_.is_empty() {
                    let arm_cx = cursor + widths.last().copied().unwrap_or(MIN_W) / 2.0;
                    exits.push(layout_stmts(else_, arm_cx, branch_y, id, Some("else".to_string()), ctx));
                }
                let merge_y = exits.iter().map(|(_, _, y)| *y).fold(branch_y, f64::max);
                let join = ctx.add_node(Shape::Join, "", cx, merge_y);
                for (exit_id, exit_label, _) in exits {
                    ctx.add_edge(exit_id, join, exit_label, false);
                }
                prev = join;
                y = merge_y + ROW_PITCH;
            }
            Stmt::Try { body, handler, else_, finally, .. } => {
                let id = ctx.add_node(Shape::Decision, "try", cx, y);
                ctx.add_edge(prev, id, pending.take(), false);
                let branch_y = y + ROW_PITCH;
                let mut try_body = body.clone();
                try_body.extend(else_.iter().cloned());
                let handler_w = required_width(handler);
                let try_w = required_width(&try_body);
                let try_cx = cx - H_GAP / 2.0 - try_w / 2.0;
                let handler_cx = cx + H_GAP / 2.0 + handler_w / 2.0;
                let (try_exit, try_label, try_y) = if try_body.is_empty() {
                    (id, Some("ok".to_string()), branch_y)
                } else {
                    layout_stmts(&try_body, try_cx, branch_y, id, Some("ok".to_string()), ctx)
                };
                let (catch_exit, catch_label, catch_y) = if handler.is_empty() {
                    (id, Some("error".to_string()), branch_y)
                } else {
                    layout_stmts(handler, handler_cx, branch_y, id, Some("error".to_string()), ctx)
                };
                let merge_y = try_y.max(catch_y);
                let join = ctx.add_node(Shape::Join, "", cx, merge_y);
                ctx.add_edge(try_exit, join, try_label, false);
                ctx.add_edge(catch_exit, join, catch_label, false);
                prev = join;
                y = merge_y + ROW_PITCH;
                if !finally.is_empty() {
                    let (fin_exit, _, fin_y) = layout_stmts(finally, cx, y, prev, None, ctx);
                    prev = fin_exit;
                    y = fin_y;
                }
            }
            other => {
                if let Some(label) = stmt_label(other) {
                    let id = ctx.add_node(Shape::Process, &label, cx, y);
                    ctx.add_edge(prev, id, pending.take(), false);
                    prev = id;
                    y += ROW_PITCH;
                }
            }
        }
    }
    (prev, pending, y)
}

/// Shared `while`/`for` body-plus-back-edge shape: lay the body out below
/// the header, then loop back up to it. Returns the header's own id (the
/// node the NEXT statement connects from) — matches `While`/`For`'s call
/// sites, which both continue from the header with a "done" label rather
/// than from inside the loop.
fn loop_body(body: &[Stmt], cx: f64, header_y: f64, header: usize, ctx: &mut Ctx) -> (usize, Option<String>, f64) {
    if body.is_empty() {
        return (header, None, header_y + ROW_PITCH);
    }
    let body_y = header_y + ROW_PITCH;
    let (body_exit, _, next_y) = layout_stmts(body, cx, body_y, header, Some("yes".to_string()), ctx);
    ctx.add_edge(body_exit, header, Some("repeat".to_string()), true);
    (header, None, next_y)
}

pub(crate) fn render_algorigram_svg(name: &str, params: &[Param], body: &FnBody) -> R<String> {
    let mut ctx = Ctx::new();
    let start_label = if params.is_empty() { name.to_string() } else { format!("{name}({})", params_label(params)) };
    let cx = 0.0; // placeholder, corrected once the real width is known below
    let start = ctx.add_node(Shape::Terminal, &start_label, cx, MARGIN + BOX_H / 2.0);
    let body_y = MARGIN + BOX_H / 2.0 + ROW_PITCH;
    let (last, _, end_y) = match body {
        FnBody::Expr(ex) => {
            let id = ctx.add_node(Shape::Process, &shorten(format!("return {}", expr_label(ex)), 40), cx, body_y);
            ctx.add_edge(start, id, None, false);
            (id, None, body_y + ROW_PITCH)
        }
        FnBody::Block(stmts) => layout_stmts(stmts, cx, body_y, start, None, &mut ctx),
    };
    let end = ctx.add_node(Shape::Terminal, "end", cx, end_y);
    ctx.add_edge(last, end, None, false);

    // Every node above was placed relative to `cx = 0`, spreading to
    // negative x for left branches. Shift the whole diagram right by
    // however far left it went, so nothing renders off-canvas.
    let min_x = ctx.nodes.iter().map(|n| n.cx - n.w / 2.0).fold(f64::INFINITY, f64::min);
    let shift = MARGIN - min_x.min(0.0);
    for n in &mut ctx.nodes {
        n.cx += shift;
    }
    let width = ctx.nodes.iter().map(|n| n.cx + n.w / 2.0).fold(0.0, f64::max) + MARGIN;
    let height = ctx.nodes.iter().map(|n| n.cy + n.h / 2.0).fold(0.0, f64::max) + MARGIN;
    Ok(render_svg_document(&ctx, width, height, Some(&start_label)))
}

// ---------------------------------------------------------------------
// `Ctx` -> `DrawOp`s -> a standalone SVG document (no `Figure`/axes —
// `plotting::render_svg` is hard-wired to those, so this writes its own
// minimal wrapper around the same per-op writer, `write_svg_op`).
// ---------------------------------------------------------------------

const INK: &str = "#333333";
const FILL: &str = "#eef3fb";
const DECISION_FILL: &str = "#fdf1df";
const TERMINAL_FILL: &str = "#e6f4ea";

fn node_ops(n: &FlowNode, ops: &mut Vec<DrawOp>) {
    match n.shape {
        Shape::Join => {
            ops.push(DrawOp::Circle { cx: n.cx, cy: n.cy, r: JOIN_R, fill: Some(INK.to_string()), stroke: None });
            return;
        }
        Shape::Decision => {
            let (hw, hh) = (n.w / 2.0, n.h / 2.0);
            ops.push(DrawOp::Polygon {
                points: vec![(n.cx, n.cy - hh), (n.cx + hw, n.cy), (n.cx, n.cy + hh), (n.cx - hw, n.cy)],
                fill: Some(DECISION_FILL.to_string()),
                stroke: Some(INK.to_string()),
                opacity: 1.0,
                width: 1.2,
            });
        }
        Shape::Terminal => {
            ops.push(DrawOp::Rect {
                x: n.cx - n.w / 2.0,
                y: n.cy - n.h / 2.0,
                w: n.w,
                h: n.h,
                fill: Some(TERMINAL_FILL.to_string()),
                stroke: Some(INK.to_string()),
                opacity: 1.0,
                radius: n.h / 2.0,
            });
        }
        Shape::Process => {
            ops.push(DrawOp::Rect {
                x: n.cx - n.w / 2.0,
                y: n.cy - n.h / 2.0,
                w: n.w,
                h: n.h,
                fill: Some(FILL.to_string()),
                stroke: Some(INK.to_string()),
                opacity: 1.0,
                radius: 6.0,
            });
        }
    }
    write_wrapped_label(&n.label, n.cx, n.cy, n.w - PAD_X * 2.0, ops);
}

/// A label wider than its box wraps onto up to two lines, centered on the
/// node, rather than overflowing its edges (which `write_svg_op` would
/// otherwise draw silently past the box border).
fn write_wrapped_label(label: &str, cx: f64, cy: f64, max_w: f64, ops: &mut Vec<DrawOp>) {
    if measure_text(label, FONT_SIZE, None) <= max_w || !label.contains(' ') {
        ops.push(DrawOp::Text {
            x: cx,
            y: cy + FONT_SIZE * 0.35,
            text: label.to_string(),
            size: FONT_SIZE,
            anchor: Anchor::Middle,
            rotate: 0.0,
            color: INK.to_string(),
            italic: false,
        });
        return;
    }
    let words: Vec<&str> = label.split(' ').collect();
    let mid = words.len() / 2;
    let (top, bottom) = (words[..mid].join(" "), words[mid..].join(" "));
    for (i, line) in [top, bottom].into_iter().enumerate() {
        ops.push(DrawOp::Text {
            x: cx,
            y: cy + FONT_SIZE * 0.35 + (i as f64 - 0.5) * (FONT_SIZE + 2.0),
            text: line,
            size: FONT_SIZE,
            anchor: Anchor::Middle,
            rotate: 0.0,
            color: INK.to_string(),
            italic: false,
        });
    }
}

/// Where a straight line from `(cx, cy)` towards `(tx, ty)` crosses this
/// node's bounding box — used to start/end an arrow AT a node's edge
/// instead of at its center. Diamonds and the terminal's rounded corners
/// are approximated by their bounding box; close enough for an arrow
/// tip, which only needs to land near the shape, not exactly on its
/// outline.
fn boundary_point(n: &FlowNode, towards: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (towards.0 - n.cx, towards.1 - n.cy);
    if dx.abs() < 1e-9 && dy.abs() < 1e-9 {
        return (n.cx, n.cy + n.h / 2.0);
    }
    let (hw, hh) = (n.w / 2.0 + 1.0, n.h / 2.0 + 1.0);
    let scale = if dx.abs() < 1e-9 {
        hh / dy.abs()
    } else if dy.abs() < 1e-9 {
        hw / dx.abs()
    } else {
        (hw / dx.abs()).min(hh / dy.abs())
    };
    (n.cx + dx * scale, n.cy + dy * scale)
}

/// A straight arrow's shaft + triangular head — the same shape
/// `plotting.rs`'s callout-arrow rendering already draws inline for
/// `arrow`/`annotate`, factored out here so a second, unrelated call site
/// doesn't hand-roll the geometry a second time.
pub(crate) fn arrow_ops(x1: f64, y1: f64, x2: f64, y2: f64, color: &str, width: f64) -> Vec<DrawOp> {
    let mut ops = Vec::new();
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return ops;
    }
    const HEAD: f64 = 9.0;
    const HALF_WIDTH: f64 = 3.6;
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    let shaft_end = (x2 - ux * HEAD * 0.6, y2 - uy * HEAD * 0.6);
    ops.push(DrawOp::Line { x1, y1, x2: shaft_end.0, y2: shaft_end.1, color: color.to_string(), width });
    let base = (x2 - ux * HEAD, y2 - uy * HEAD);
    ops.push(DrawOp::Polygon {
        points: vec![(x2, y2), (base.0 + nx * HALF_WIDTH, base.1 + ny * HALF_WIDTH), (base.0 - nx * HALF_WIDTH, base.1 - ny * HALF_WIDTH)],
        fill: Some(color.to_string()),
        stroke: None,
        opacity: 1.0,
        width: 1.0,
    });
    ops
}

fn edge_ops(edge: &FlowEdge, nodes: &[FlowNode], ops: &mut Vec<DrawOp>) {
    let from = &nodes[edge.from];
    let to = &nodes[edge.to];
    let color = if edge.back { "#8a5a00" } else { INK };
    if !edge.back {
        let p1 = boundary_point(from, (to.cx, to.cy));
        let p2 = boundary_point(to, (from.cx, from.cy));
        ops.extend(arrow_ops(p1.0, p1.1, p2.0, p2.1, color, 1.4));
        if let Some(label) = &edge.label {
            let mx = (p1.0 + p2.0) / 2.0;
            let my = (p1.1 + p2.1) / 2.0;
            let bg_w = measure_text(label, FONT_SIZE - 2.0, None) + 8.0;
            ops.push(DrawOp::Rect { x: mx - bg_w / 2.0, y: my - 8.0, w: bg_w, h: 14.0, fill: Some("#ffffff".into()), stroke: None, opacity: 0.85, radius: 3.0 });
            ops.push(DrawOp::Text { x: mx, y: my + 4.0, text: label.clone(), size: FONT_SIZE - 2.0, anchor: Anchor::Middle, rotate: 0.0, color: color.to_string(), italic: true });
        }
        return;
    }
    // A back-edge (a loop's "repeat" arrow) is routed to the side as an
    // elbow, not drawn straight -- straight would run back UP through
    // every node the loop body just drew, rather than around them.
    let side = -(from.w.max(to.w) / 2.0 + H_GAP / 2.0);
    let x = from.cx + side;
    let p_start = boundary_point(from, (x, from.cy));
    let p_end = boundary_point(to, (x, to.cy));
    ops.push(DrawOp::Polyline { points: vec![(p_start.0, p_start.1), (x, from.cy), (x, to.cy)], color: color.to_string(), width: 1.4, dash: None });
    ops.extend(arrow_ops(x, to.cy, p_end.0, p_end.1, color, 1.4));
    if let Some(label) = &edge.label {
        ops.push(DrawOp::Text { x, y: (from.cy + to.cy) / 2.0, text: label.clone(), size: FONT_SIZE - 2.0, anchor: Anchor::Middle, rotate: 0.0, color: color.to_string(), italic: true });
    }
}

fn to_draw_ops(ctx: &Ctx) -> Vec<DrawOp> {
    let mut ops = Vec::new();
    for edge in &ctx.edges {
        edge_ops(edge, &ctx.nodes, &mut ops);
    }
    for node in &ctx.nodes {
        node_ops(node, &mut ops);
    }
    ops
}

fn render_svg_document(ctx: &Ctx, width: f64, height: f64, title: Option<&str>) -> String {
    let ops = to_draw_ops(ctx);
    let mut body = String::new();
    let _ = write!(body, "<rect x=\"0\" y=\"0\" width=\"{width:.2}\" height=\"{height:.2}\" fill=\"#ffffff\"/>\n");
    for op in &ops {
        write_svg_op(&mut body, op, f64::MAX);
    }
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width:.2} {height:.2}\" width=\"{width:.2}\" height=\"{height:.2}\" font-family=\"Inter, Helvetica, Arial, sans-serif\">\n"
    );
    if let Some(t) = title {
        let _ = write!(out, "<title>{}</title>\n", xml_escape(t));
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes `svg` to `path`, or wraps it in a minimal standalone page for a
/// `.html` path. Any other extension (most importantly `.png`) is refused
/// with the same reasoning `savefig` already gives for a whole-figure
/// PNG: there is no rasterizer in this engine, so writing one would mean
/// silently writing something that is not what the extension promises.
pub(crate) fn write_diagram_file(svg: &str, path: &str) -> R<()> {
    let lower = path.to_ascii_lowercase();
    let text = if lower.ends_with(".svg") {
        svg.to_string()
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>\n{svg}</body></html>\n")
    } else if lower.ends_with(".png") {
        return e(
            "diagram file=: .png needs a real rasterizer, which this engine doesn't have (same scoping as \
             savefig's whole-figure .png) -- write .svg or .html instead",
        );
    } else {
        return e(format!("diagram file=: unsupported extension in `{path}` -- write .svg or .html"));
    };
    std::fs::write(path, text).map_err(|err| EvalError { msg: format!("diagram file=: couldn't write `{path}`: {err}") })
}
