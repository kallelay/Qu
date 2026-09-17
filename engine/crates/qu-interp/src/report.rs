//! Shared HTML "report" rendering, styled with the retro traffic-light
//! window-chrome look documented in `docs/design/retro-window-chrome.md`
//! (a saved, already-tested CSS/JS asset — reused verbatim here via
//! `include_str!`, not redesigned).
//!
//! Two call sites share this module, and neither one duplicates the
//! rendering logic:
//!
//! 1. **`qu run <file.qu> --report <path.html>`** (`qu-cli`'s
//!    `report.rs`) — the CLI controls execution from the start, so it can
//!    pre-split the script into `#%%` cells and run them one at a time
//!    (see `qu-cli::report::run_cells`), giving REAL per-cell output
//!    attribution: each cell's own code panel is paired with exactly the
//!    output that cell produced.
//! 2. **The in-script `write_report(path)` builtin** (this crate's
//!    `Interp::call_builtin`, § in-program report snapshot, 2026-09-01) —
//!    called from *inside* an already-running script, at any point during
//!    execution. Because normal script execution parses and runs the
//!    WHOLE file in one `Interp::run` call (see `Interp::source`'s doc
//!    comment), there is no way to retroactively say "which `#%%` cell
//!    was executing when `write_report` was called" without re-running
//!    the script cell-by-cell from scratch — which `write_report` cannot
//!    do (the script is already mid-flight, with side effects already
//!    fired). So the builtin always renders a SINGLE combined code+output
//!    pair (the whole script-so-far source, paired with all output
//!    produced so far), even for a script that uses `#%%` markers. This
//!    is a real, deliberate difference from the CLI path, not an
//!    oversight — see `Interp`'s `write_report` match arm for where this
//!    is applied.

/// Escapes `&`/`<`/`>` for embedding as HTML text content (not an
/// attribute value — see `attr_escape` below for that case). Same three
/// characters `qu-cli`'s own `diary.rs::html_escape` escapes for its
/// transcript's `<pre>` blocks; duplicated here (rather than shared)
/// because `qu-cli` is a downstream crate of `qu-interp` and can't be
/// depended on from here.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// One code+output panel pair in the rendered report.
pub struct ReportCell {
    /// Text following a `#%%` marker on this cell's own first line, if any
    /// (empty for an unmarked/implicit cell — see `split_into_cells`).
    pub title: String,
    pub source: String,
    pub output: String,
    /// Every figure THIS cell produced, already rendered to SVG (via
    /// `plotting::render_svg`, same as `--emit-figure`), in the order they
    /// were created. Zero, one, or many: a cell that never plots gets an
    /// empty vec (no Figure panel at all -- see `render_cell_html`); a
    /// cell with one `figure()`/plot pair gets one; a cell that calls
    /// `figure()` more than once (each time with real content) gets one
    /// per distinct figure. Populated by `qu-cli`'s `report::run_cells`
    /// (real per-cell attribution, diffing `Interp::figure_history` and
    /// the still-open current figure around each cell) and by the
    /// in-script `write_report` builtin (all figures produced so far,
    /// attached to its one combined cell — see that builtin's match arm).
    pub figures: Vec<String>,
}

/// The CSS from `docs/design/retro-window-chrome.md`, verbatim, plus a
/// small amount of report-specific page-layout CSS (body/heading spacing,
/// and styling for the two panel bodies the design doc doesn't define
/// because they don't exist there — the Figure panel's plain SVG
/// container and the Profile panel's data table).
pub const REPORT_CSS: &str = include_str!("report_template.css");

/// The minimal Qu syntax highlighter from the design doc, verbatim,
/// plus a small bootstrap that runs it over every `.code-body`'s
/// `data-qu-source` attribute once the page loads (see that attribute's
/// use in `render_cell_html` below for why a data attribute, not raw
/// inner HTML, carries the source text).
pub const REPORT_JS: &str = include_str!("report_template.js");

/// Escapes `s` for embedding inside a double-quoted HTML attribute value.
/// Browsers HTML-entity-decode attribute values before handing them to
/// JS (`element.getAttribute(...)`), so this round-trips the exact
/// original text back to `REPORT_JS`'s `highlightQu` — unlike
/// `html_escape` (used for the plain-text `.term-body` output, which is
/// never re-parsed by JS), this also escapes `"` since that's the
/// delimiter here, not `<`/`>`/`&` alone.
fn attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// A line matching `^#%%.*$` (anchored to the very start of the line — an
/// indented or mid-line `#%%` does NOT count) starts a new cell; text
/// before the first such marker (or the whole file, if there is no
/// marker at all) forms an implicit, untitled leading cell. This mirrors
/// `qu-ui-components/src/utils/cells.ts`'s `parseCells` semantics exactly
/// (that module's own doc comment and test suite are the spec) — ported
/// to Rust here since a CLI/interpreter-level report has no TS/browser
/// runtime to call into. Each returned cell includes its own `#%%`
/// marker line verbatim (if it has one): it's just a `#` comment to Qu,
/// so re-running it as part of the cell's own source is harmless, same
/// as `cells.ts` documents.
pub fn split_into_cells(src: &str) -> Vec<(String, String)> {
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();

    let marker_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("#%%"))
        .map(|(i, _)| i)
        .collect();

    let boundaries: Vec<usize> = if marker_indices.first() == Some(&0) {
        marker_indices
    } else {
        std::iter::once(0).chain(marker_indices).collect()
    };

    boundaries
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = if i + 1 < boundaries.len() {
                boundaries[i + 1] - 1
            } else {
                lines.len() - 1
            };
            let title = if lines[start].starts_with("#%%") {
                lines[start][3..].trim().to_string()
            } else {
                String::new()
            };
            let source = lines[start..=end].join("\n");
            (title, source)
        })
        .collect()
}

/// Renders `stats` (from `Interp::profile_stats`) as an HTML table for the
/// report's optional Profile panel — the same "call count / total time /
/// time %" data `qu-cli`'s own `render_profile_report` prints as plain
/// text for `--profile-output`, styled with the report's CSS instead.
/// `elapsed`/`peak_rss` are `None` when the caller has no such data —
/// `qu-cli`'s `--report` (paired with `--profile`) has both (from its own
/// wall-clock timer and `resource.rs`'s watchdog); the in-script
/// `write_report` builtin has neither (RSS sampling and total-run timing
/// are both `qu-cli`-external concerns, outside anything `Interp` tracks
/// about itself) and so renders a function-stats-only table — a real,
/// smaller, and clearly labeled subset rather than a fabricated total.
pub fn render_profile_section_html(
    mut stats: Vec<(&str, u64, std::time::Duration)>,
    elapsed: Option<std::time::Duration>,
    peak_rss: Option<u64>,
) -> String {
    stats.sort_by(|a, b| b.2.cmp(&a.2));

    let mut body = String::new();
    if let Some(e) = elapsed {
        body.push_str(&format!(
            "<p>wall-clock time: {:.3}s</p>\n",
            e.as_secs_f64()
        ));
        match peak_rss {
            Some(bytes) => body.push_str(&format!(
                "<p>peak RSS: {} MB</p>\n",
                bytes / (1024 * 1024)
            )),
            None => body.push_str("<p>peak RSS: unavailable on this platform</p>\n"),
        }
    }

    if stats.is_empty() {
        body.push_str("<p>(no user-function calls were tracked)</p>\n");
        return body;
    }

    let total_secs = elapsed.map(|e| e.as_secs_f64().max(1e-12));
    body.push_str("<table>\n<thead><tr><th>function</th><th>calls</th><th>total time</th>");
    if total_secs.is_some() {
        body.push_str("<th>time %</th>");
    }
    body.push_str("</tr></thead>\n<tbody>\n");
    for (name, calls, dur) in &stats {
        let secs = dur.as_secs_f64();
        body.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{:.3}s</td>",
            html_escape(name),
            calls,
            secs
        ));
        if let Some(total) = total_secs {
            body.push_str(&format!("<td>{:.1}%</td>", 100.0 * secs / total));
        }
        body.push_str("</tr>\n");
    }
    body.push_str("</tbody>\n</table>\n");
    body
}

fn win_chrome(title: &str) -> String {
    format!(
        "<div class=\"win-chrome\"><span class=\"win-dot r\"></span><span class=\"win-dot y\"></span><span class=\"win-dot g\"></span><span class=\"win-title\">{}</span></div>\n",
        html_escape(title)
    )
}

/// One `<div class="win code-win">...</div>` + `<div class="win
/// term-win">...</div>` pair — exactly the HTML structure from the design
/// doc, `data-qu-source`/`data-cmd` carrying the raw (attribute-escaped)
/// source and script title for `REPORT_JS`'s client-side highlighting and
/// the `.term-body::before` "$ qu run ..." prompt line respectively —
/// followed immediately by zero, one, or many Figure panels for whatever
/// this cell itself produced (`cell.figures`), reusing the same `.win`
/// "Figure" panel treatment the single-global case used to render once at
/// the end of the whole report. A cell with no figures renders no Figure
/// panel at all (never an empty one).
fn render_cell_html(page_title: &str, code_title: &str, term_title: &str, cell: &ReportCell) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"win code-win\">\n");
    out.push_str(&win_chrome(code_title));
    out.push_str(&format!(
        "<pre class=\"code-body\" data-qu-source=\"{}\"></pre>\n",
        attr_escape(&cell.source)
    ));
    out.push_str("</div>\n");

    out.push_str("<div class=\"win term-win\">\n");
    out.push_str(&win_chrome(term_title));
    out.push_str(&format!(
        "<pre class=\"term-body\" data-cmd=\"{}\">{}</pre>\n",
        attr_escape(page_title),
        html_escape(&cell.output)
    ));
    out.push_str("</div>\n");

    // A single figure keeps the plain "Figure" title (matches the
    // pre-per-cell-attribution report byte-for-byte in the common case);
    // more than one in the same cell gets numbered so they're
    // distinguishable.
    let n = cell.figures.len();
    for (i, svg) in cell.figures.iter().enumerate() {
        let fig_title = if n > 1 {
            format!("Figure {}/{}", i + 1, n)
        } else {
            "Figure".to_string()
        };
        out.push_str("<div class=\"win\">\n");
        out.push_str(&win_chrome(&fig_title));
        out.push_str("<div class=\"fig-body\">\n");
        out.push_str(svg);
        out.push_str("\n</div>\n</div>\n");
    }
    out
}

/// Renders the full, self-contained report page. `cells` is already
/// built by the caller (real per-`#%%`-cell attribution for `qu-cli`'s
/// `--report`, or a single combined cell for `write_report` — see this
/// module's doc comment), each carrying its own `figures` (rendered SVGs,
/// via `plotting::render_svg`, embedded directly with no re-rendering) —
/// rendered right after that cell's own code+output panel pair by
/// `render_cell_html`, not as one trailing global section; `profile_html`
/// is `render_profile_section_html`'s output, `None` when profiling
/// wasn't requested; `error` is the run's error message, if the script
/// failed partway through (the report still shows everything captured
/// before the failure, matching `qu run`'s own "print what we have, then
/// report the error" behavior).
pub fn render_report_html(
    page_title: &str,
    cells: &[ReportCell],
    profile_html: Option<&str>,
    error: Option<&str>,
) -> String {
    let multi = cells.len() > 1;
    let mut body = String::new();

    for (i, cell) in cells.iter().enumerate() {
        let code_title = if !multi {
            page_title.to_string()
        } else if cell.title.is_empty() {
            format!("{page_title} — cell {}/{}", i + 1, cells.len())
        } else {
            format!("{page_title} — cell {}/{}: {}", i + 1, cells.len(), cell.title)
        };
        let term_title = if multi {
            format!("Terminal — cell {}/{}", i + 1, cells.len())
        } else {
            "Terminal".to_string()
        };
        body.push_str(&render_cell_html(page_title, &code_title, &term_title, cell));
    }

    if let Some(profile) = profile_html {
        body.push_str("<div class=\"win\">\n");
        body.push_str(&win_chrome("Profile"));
        body.push_str("<div class=\"profile-body\">\n");
        body.push_str(profile);
        body.push_str("</div>\n</div>\n");
    }

    if let Some(e) = error {
        body.push_str("<div class=\"win\">\n");
        body.push_str(&win_chrome("Error"));
        body.push_str("<pre class=\"error-body\">");
        body.push_str(&html_escape(e));
        body.push_str("</pre>\n</div>\n");
    }

    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\n{css}\n</style>\n</head>\n<body>\n<h1>{title}</h1>\n{body}<script>\n{js}\n</script>\n</body>\n</html>\n",
        title = html_escape(page_title),
        css = REPORT_CSS,
        body = body,
        js = REPORT_JS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_markers_is_a_single_untitled_cell() {
        let cells = split_into_cells("x = 1\nprint(x)\n");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, "");
        assert!(cells[0].1.contains("x = 1"));
        assert!(cells[0].1.contains("print(x)"));
    }

    #[test]
    fn splits_on_markers_and_captures_titles() {
        let src = "#%% Setup\nx = 1\n#%% Print it\nprint(x)\n";
        let cells = split_into_cells(src);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].0, "Setup");
        assert!(cells[0].1.contains("x = 1"));
        assert_eq!(cells[1].0, "Print it");
        assert!(cells[1].1.contains("print(x)"));
    }

    #[test]
    fn leading_text_before_first_marker_is_its_own_cell() {
        let src = "x = 1\n#%% Real cell\nprint(x)\n";
        let cells = split_into_cells(src);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].0, "");
        assert!(cells[0].1.contains("x = 1"));
        assert_eq!(cells[1].0, "Real cell");
    }

    #[test]
    fn indented_or_mid_line_hash_percent_percent_is_not_a_marker() {
        let src = "  #%% not a marker (indented)\nx = 1 # #%% also not a marker (mid-line)\n";
        let cells = split_into_cells(src);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, "");
    }

    #[test]
    fn per_cell_figures_render_next_to_their_own_cell_not_as_one_trailing_section() {
        // Marker IDs deliberately avoid any substring overlap with real
        // CSS/JS class names in `REPORT_CSS`/`REPORT_JS` (e.g. a naive
        // `fig-b` marker would false-positive-match `.fig-body`'s class
        // name in the embedded stylesheet) -- these are unique enough to
        // only ever appear where this test actually put them.
        let cells = vec![
            ReportCell {
                title: "First".to_string(),
                source: "plot(a)".to_string(),
                output: "".to_string(),
                figures: vec!["<svg id=\"QQMARKERALPHAQQ\"></svg>".to_string()],
            },
            ReportCell {
                title: "Second".to_string(),
                source: "print(1)".to_string(),
                output: "1\n".to_string(),
                figures: Vec::new(),
            },
            ReportCell {
                title: "Third".to_string(),
                source: "plot(b); figure(); plot(c)".to_string(),
                output: "".to_string(),
                figures: vec![
                    "<svg id=\"QQMARKERBETAQQ\"></svg>".to_string(),
                    "<svg id=\"QQMARKERGAMMAQQ\"></svg>".to_string(),
                ],
            },
        ];
        let html = render_report_html("demo.qu", &cells, None, None);

        // Exactly the 3 real figures -- none fabricated, none dropped.
        assert_eq!(html.matches("<svg").count(), 3);
        assert_eq!(html.matches("class=\"fig-body\"").count(), 3);

        // Cell 2 (no figures) has no figure panel of its own: nothing
        // about it should be adjacent to a fig-body between its own
        // code/term panels and cell 3's.
        let a = html.find("QQMARKERALPHAQQ").unwrap();
        let cell2_source = html.find("print(1)").unwrap();
        let b = html.find("QQMARKERBETAQQ").unwrap();
        let c = html.find("QQMARKERGAMMAQQ").unwrap();
        assert!(a < cell2_source, "cell 1's figure must appear before cell 2's code");
        assert!(cell2_source < b, "cell 2 has no figure; cell 3's first figure comes after cell 2 entirely");
        assert!(b < c, "cell 3's two figures render in creation order");

        // A cell with 2 figures gets numbered titles.
        assert!(html.contains("Figure 1/2"));
        assert!(html.contains("Figure 2/2"));
        // A cell with exactly 1 figure keeps the plain, unnumbered title.
        assert!(html.contains("win-title\">Figure<"));
    }

    #[test]
    fn render_report_html_contains_escaped_source_output_and_no_empty_panels() {
        let cells = vec![ReportCell {
            title: String::new(),
            source: "print(\"a<b & c\")".to_string(),
            output: "a<b & c\n".to_string(),
            figures: Vec::new(),
        }];
        let html = render_report_html("demo.qu", &cells, None, None);
        assert!(html.contains("a&lt;b &amp; c"));
        assert!(html.contains("data-qu-source="));
        assert!(!html.contains("class=\"fig-body\""));
        assert!(!html.contains("class=\"profile-body\""));
        assert!(!html.contains("class=\"error-body\""));
    }
}
