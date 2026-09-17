//! `qu run <file.qu> --report <path.html>` — bundles the script's own
//! source, its captured stdout, and any figure it produced into a single
//! self-contained HTML report styled with the retro traffic-light
//! window-chrome look. Additive, matching `--emit-figure`/`--emit-vars`/
//! `--profile-output`'s own pattern (see `main.rs`'s `cmd_run`): normal
//! execution and its stdout are completely unaffected by `--report` being
//! present — this just writes one extra file alongside it.
//!
//! All of the actual HTML/CSS/JS rendering lives in `qu_interp::report`
//! (shared with the in-script `write_report(path)` builtin — see that
//! module's doc comment for the full split of responsibilities). This
//! module is the CLI-only orchestration layer, and it earns its keep over
//! just calling `write_report`'s own logic directly in exactly one way:
//! **real per-`#%%`-cell output attribution**. `write_report` (fired from
//! inside an already-running, monolithically-executed script) can only
//! ever see "the whole script's source" + "all output so far" as one
//! block. `qu run --report`, by contrast, controls execution itself,
//! *before* any of it happens — so it can pre-split the script into cells
//! and run them one at a time, diffing `Interp::out` around each call
//! exactly the way `qu diary` already does per top-level statement (see
//! `diary.rs`'s module doc comment for why that diffing approach is
//! sound). The end state of `it` after `run_cells` (bindings, `it.out`,
//! `it.figures`) is identical to what a single `it.run(&src)` call would
//! have produced — this is a drop-in replacement for that one call, not
//! an alternate execution mode with different semantics.

use qu_interp::report::{render_profile_section_html, render_report_html, split_into_cells, ReportCell};
use qu_interp::Interp;

/// Runs `src`'s `#%%`-delimited cells (or the whole file as a single
/// implicit cell, if it has no markers — see `split_into_cells`) against
/// `it`, in source order, building one `ReportCell` per cell. Stops at the
/// first error (matching `qu run`'s own "execute what we can, then report
/// the error" behavior) and returns it separately, so the cells captured
/// up to that point are still available for the report.
///
/// Real per-cell FIGURE attribution (not just output) works the same way
/// as the existing `it.out` diffing: snapshot what "figures produced so
/// far" looks like before and after each cell, and attach only what's new
/// to that cell's own `ReportCell`. Two things can be "new":
///
/// 1. Any figure `Interp::figure_history` gained during the cell — the
///    interpreter itself finalizes a figure into that history the moment
///    a later `figure()` call replaces it (see `figure_history`'s field
///    doc comment), so a cell that calls `figure()` more than once
///    correctly gets one entry per distinct figure, in creation order.
/// 2. The still-open CURRENT figure (`it.figure`), if this cell actually
///    drew into it — detected by comparing its rendered SVG before vs.
///    after the cell runs, since `Figure`/`Panel` don't implement
///    `PartialEq` (there's no cheaper equality check available, and a
///    report is generated once per run, not on a hot path). A figure
///    left completely untouched by this cell (e.g. it was already
///    finished and attached to an earlier cell, and nothing plots after
///    that) is correctly NOT re-attached.
///
/// These two can overlap: a figure can sit open and unmodified across
/// several cells (nothing plots into it, no `figure()` call ends it)
/// before finally getting finalized into history by a `figure()` call in
/// some LATER cell that never touched it itself. Without care that
/// finalization would attach the very same figure a second time (it was
/// already shown against whichever cell last modified it). The
/// before/after `current_before` comparison in the loop below also
/// filters that history entry out when it's an exact match, so each
/// distinct figure is attached exactly once, to the cell that actually
/// last drew into it.
pub fn run_cells(it: &mut Interp, src: &str) -> (Vec<ReportCell>, Result<(), String>) {
    let mut cells = Vec::new();
    let mut run_result = Ok(());

    for (title, source) in split_into_cells(src) {
        let out_before = it.out.len();
        let hist_before = it.figure_history.len();
        // `it.figure.width`/`height` (see `figure_size(w, h)`) rather than a
        // fixed constant, so a script that customizes its canvas size gets
        // that size in the report too, not the old hardcoded 900x600.
        let (fig_width, fig_height) = (it.figure.width, it.figure.height);
        let current_before = (!it.figure.is_pristine())
            .then(|| qu_interp::plotting::render_svg(&it.figure, fig_width, fig_height, false));

        let res = it.run(&source);
        let output = it.out[out_before..].to_string();

        // A history entry finalized during THIS cell can still be the very
        // same (unmodified) figure that was already attached to an
        // earlier cell via the current-figure diff below — e.g. cell 1
        // plots, cell 2 does nothing to that figure at all, and cell 3
        // finally calls `figure()`, finalizing it into history right as
        // cell 3 runs even though cell 3 never touched it. Compare
        // against `current_before` (what the still-open figure already
        // looked like, and so already got attached, before this cell
        // started) and drop an exact match so it isn't shown twice.
        let mut figures: Vec<String> = it.figure_history[hist_before..]
            .iter()
            .map(|f| qu_interp::plotting::render_svg(f, it.figure.width, it.figure.height, false))
            .filter(|svg| Some(svg) != current_before.as_ref())
            .collect();
        if !it.figure.is_pristine() {
            let current_after = qu_interp::plotting::render_svg(&it.figure, it.figure.width, it.figure.height, false);
            if Some(&current_after) != current_before.as_ref() {
                figures.push(current_after);
            }
        }

        cells.push(ReportCell { title, source, output, figures });
        if let Err(e) = res {
            run_result = Err(e.to_string());
            break;
        }
    }

    (cells, run_result)
}

/// Renders and writes the report file. Figures are already attached
/// per-cell inside `cells` (see `run_cells`) — this no longer needs `it`
/// for anything figure-related, only for `profile_stats()`. `profile`
/// gates the optional Profile section exactly like `qu run --profile`
/// gates `render_profile_report`'s plain-text one — `run_elapsed`/
/// `peak_rss` only matter when it's `true`. `error` is the run's own
/// error message, if any (still writes a report with everything captured
/// before the failure, same "partial output beats no output" behavior
/// `qu run` itself already has for stdout).
#[allow(clippy::too_many_arguments)]
pub fn write_report(
    out_path: &str,
    script_title: &str,
    cells: &[ReportCell],
    it: &Interp,
    profile: bool,
    run_elapsed: std::time::Duration,
    peak_rss: Option<u64>,
    error: Option<&str>,
) -> Result<(), String> {
    let profile_html = if profile {
        Some(render_profile_section_html(
            it.profile_stats(),
            Some(run_elapsed),
            peak_rss,
        ))
    } else {
        None
    };

    let html = render_report_html(script_title, cells, profile_html.as_deref(), error);
    std::fs::write(out_path, html).map_err(|e| format!("cannot write {out_path}: {e}"))
}

/// `qu run <file.qu>`'s own display name for the report: just the file's
/// base name (`demo.qu`, not the whole path) — matches the design doc's
/// example `.win-title`/`data-cmd` value.
pub fn script_title(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}
