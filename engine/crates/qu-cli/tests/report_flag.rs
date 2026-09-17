//! Integration tests for `qu run <file.qu> --report <path.html>` and its
//! in-script sibling, the `write_report(path)` builtin (see
//! `qu-cli/src/report.rs` and `qu_interp::report`'s module doc comments
//! for the split of responsibilities between them). These spawn the
//! actual compiled `qu` binary against real temp scripts and inspect the
//! real generated HTML file — the flag parsing, additive-stdout
//! behavior, and file-writing side effect these exercise can only be
//! observed end-to-end through `qu-cli`'s own `main`, not through
//! `qu-interp`'s unit tests (which never go through `cmd_run` at all).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn write_script(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("qu_cli_report_flag_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    path
}

fn out_path(name: &str) -> PathBuf {
    std::env::temp_dir().join("qu_cli_report_flag_tests").join(name)
}

// ---- Additive: normal stdout is unaffected by --report ----

#[test]
fn report_flag_does_not_change_normal_stdout() {
    let script = write_script("additive.qu", "print(\"hello from additive test\")\n");
    let report = out_path("additive_report.html");
    let _ = std::fs::remove_file(&report);

    let plain = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap()])
        .output()
        .expect("failed to run qu");
    let with_report = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .output()
        .expect("failed to run qu");

    assert_eq!(
        String::from_utf8_lossy(&plain.stdout),
        String::from_utf8_lossy(&with_report.stdout),
        "adding --report must not change what gets printed to stdout"
    );
    assert!(with_report.status.success());
    assert!(report.exists(), "--report must write the report file in addition to stdout");
}

// ---- Real source + real output, single cell (no #%% markers) ----

#[test]
fn single_cell_report_contains_real_source_and_output() {
    let script = write_script(
        "single_cell.qu",
        "x = 21\nprint(\"the answer is {x * 2}\")\n",
    );
    let report = out_path("single_cell_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).expect("report file should exist");
    // Real source text, not a placeholder -- present verbatim inside the
    // `data-qu-source` attribute the client-side highlighter reads.
    assert!(html.contains("x = 21"), "expected the script's own source in the report");
    assert!(html.contains("print(&quot;the answer is"), "expected the print call's source, attribute-escaped");
    // Real captured output, not a placeholder.
    assert!(html.contains("the answer is 42"), "expected the script's real printed output in the report");
    // Exactly one code/terminal panel pair for a script with no #%% markers.
    assert_eq!(html.matches("class=\"win code-win\"").count(), 1);
    assert_eq!(html.matches("class=\"win term-win\"").count(), 1);
}

// ---- Multi-cell (#%%) reports: one pair per cell ----

#[test]
fn multi_cell_report_has_one_panel_pair_per_cell_with_correct_attribution() {
    let script = write_script(
        "multi_cell.qu",
        "#%% Setup\nx = 5\nprint(\"setup: {x}\")\n#%% Use it\ny = x + 1\nprint(\"use: {y}\")\n",
    );
    let report = out_path("multi_cell_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).expect("report file should exist");
    assert_eq!(html.matches("class=\"win code-win\"").count(), 2, "expected one code panel per #%% cell");
    assert_eq!(html.matches("class=\"win term-win\"").count(), 2, "expected one terminal panel per #%% cell");

    // Real per-cell attribution: cell 1's output must appear before cell
    // 2's source, and cell 2's own print must not leak into cell 1's
    // output panel.
    let setup_output_pos = html.find("setup: 5").expect("cell 1's own output present");
    let use_source_pos = html.find("y = x + 1").expect("cell 2's own source present");
    let use_output_pos = html.find("use: 6").expect("cell 2's own output present");
    assert!(setup_output_pos < use_source_pos, "cell 1 output must come before cell 2 source");
    assert!(use_source_pos < use_output_pos, "cell 2 source must come before cell 2 output");

    // Cell titles (text after `#%%`) show up in the panel titles.
    assert!(html.contains("Setup"));
    assert!(html.contains("Use it"));
}

// ---- Figures: present only when the script actually plots ----

#[test]
fn report_includes_figure_panel_only_when_a_figure_was_produced() {
    let with_fig = write_script("with_fig.qu", "plot(0 to 4, 0 to 4)\n");
    let without_fig = write_script("without_fig.qu", "x = 1\nprint(x)\n");
    let report_with = out_path("with_fig_report.html");
    let report_without = out_path("without_fig_report.html");
    let _ = std::fs::remove_file(&report_with);
    let _ = std::fs::remove_file(&report_without);

    Command::new(qu_bin())
        .args(["run", with_fig.to_str().unwrap(), "--report", report_with.to_str().unwrap()])
        .status()
        .unwrap();
    Command::new(qu_bin())
        .args(["run", without_fig.to_str().unwrap(), "--report", report_without.to_str().unwrap()])
        .status()
        .unwrap();

    let html_with = std::fs::read_to_string(&report_with).unwrap();
    let html_without = std::fs::read_to_string(&report_without).unwrap();

    assert!(html_with.contains("<svg"), "expected an embedded SVG when the script plotted something");
    assert!(html_with.contains("win-title\">Figure<"), "expected a Figure panel title");

    assert!(!html_without.contains("<svg"), "no figure was produced, so no SVG should appear");
    assert!(
        !html_without.contains("class=\"fig-body\""),
        "no figure was produced, so there should be no figure panel at all (not an empty one)"
    );
}

// ---- Real per-cell FIGURE attribution (not just output) ----

#[test]
fn two_cells_two_figures_attach_one_svg_per_cell_in_order() {
    // Cell 1 plots one figure; cell 2 starts a genuinely SEPARATE figure
    // (via `figure()`) and plots a different one. Before the per-cell
    // figure fix, the report showed exactly one trailing global `<svg>`
    // (whatever `it.figure` happened to be at the very end) instead of one
    // figure attached to each cell that actually produced one.
    let script = write_script(
        "two_figures.qu",
        "#%% First\nplot(0 to 4, 0 to 4)\n#%% Second\nfigure()\nplot(0 to 4, (0 to 4) * 2)\n",
    );
    let report = out_path("two_figures_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).expect("report file should exist");

    // Exactly 2 figures total -- not 0, not 1 (the old bug), not 3.
    assert_eq!(html.matches("<svg").count(), 2, "expected exactly one SVG per cell");
    assert_eq!(html.matches("class=\"fig-body\"").count(), 2);

    // Each figure appears within ITS OWN cell's panel pair, in cell order:
    // cell 1's code+term+figure, then cell 2's code+term+figure.
    let cell1_code = html.find("class=\"win code-win\"").expect("cell 1's code panel");
    let cell1_fig = html.find("class=\"fig-body\"").expect("cell 1's figure panel");
    let cell2_code = html[cell1_fig..].find("class=\"win code-win\"").map(|i| i + cell1_fig).expect("cell 2's code panel");
    let cell2_fig = html[cell2_code..].find("class=\"fig-body\"").map(|i| i + cell2_code).expect("cell 2's figure panel");

    assert!(cell1_code < cell1_fig, "cell 1's own code must precede cell 1's own figure");
    assert!(cell1_fig < cell2_code, "cell 1's figure must precede cell 2's code (not trail after everything)");
    assert!(cell2_code < cell2_fig, "cell 2's own code must precede cell 2's own figure");
}

#[test]
fn cell_with_no_figure_shows_no_figure_panel_even_between_cells_that_have_one() {
    let script = write_script(
        "sandwiched_no_figure.qu",
        "#%% Has a figure\nplot(0 to 2, 0 to 2)\n#%% No figure here\nx = 1\nprint(\"x = {x}\")\n",
    );
    let report = out_path("sandwiched_no_figure_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).expect("report file should exist");
    assert_eq!(html.matches("<svg").count(), 1, "only cell 1 plotted; cell 2 must not get a figure panel");

    // Cell 2's own output must not be sandwiched between cell 1's code and
    // its figure -- i.e. the figure really is attached right after cell
    // 1's own panels, not floating loose near cell 2.
    let cell1_fig = html.find("class=\"fig-body\"").unwrap();
    let cell2_output = html.find("x = 1").unwrap();
    assert!(cell1_fig < cell2_output, "cell 1's figure must come before cell 2's output");
}

#[test]
fn one_cell_with_two_figure_calls_shows_both_in_order() {
    let script = write_script(
        "two_figures_one_cell.qu",
        "plot(0 to 4, 0 to 4)\nfigure()\nplot(0 to 4, (0 to 4) * 3)\n",
    );
    let report = out_path("two_figures_one_cell_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).expect("report file should exist");
    // Single cell (no #%% markers) that produced 2 distinct figures.
    assert_eq!(html.matches("class=\"win code-win\"").count(), 1);
    assert_eq!(html.matches("<svg").count(), 2, "both figure() calls' plots must be captured, not just the last");
    assert_eq!(html.matches("class=\"fig-body\"").count(), 2);
    // Numbered titles distinguish the two figures within the one cell.
    assert!(html.contains("Figure 1/2"));
    assert!(html.contains("Figure 2/2"));
}

// ---- --profile is optional, and only included when passed alongside --report ----

#[test]
fn profile_section_is_absent_unless_profile_flag_is_also_passed() {
    let script = write_script("profile_optional.qu", "x = 1\nprint(x)\n");
    let report_no_profile = out_path("profile_optional_no_profile.html");
    let report_with_profile = out_path("profile_optional_with_profile.html");
    let _ = std::fs::remove_file(&report_no_profile);
    let _ = std::fs::remove_file(&report_with_profile);

    Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report_no_profile.to_str().unwrap()])
        .status()
        .unwrap();
    Command::new(qu_bin())
        .args([
            "run",
            script.to_str().unwrap(),
            "--profile",
            "--report",
            report_with_profile.to_str().unwrap(),
        ])
        .status()
        .unwrap();

    let html_no_profile = std::fs::read_to_string(&report_no_profile).unwrap();
    let html_with_profile = std::fs::read_to_string(&report_with_profile).unwrap();

    assert!(!html_no_profile.contains("win-title\">Profile<"), "no --profile was passed, so no Profile section should appear");
    assert!(html_with_profile.contains("win-title\">Profile<"), "--profile was passed alongside --report, so a Profile section should appear");
    assert!(html_with_profile.contains("wall-clock time"));
}

// ---- HTML-escaping correctness: `<`/`>`/`&` in output or comments ----

#[test]
fn report_html_escapes_special_characters_in_output_and_source() {
    let script = write_script(
        "escaping.qu",
        "# a comment with <tags> & \"quotes\"\nprint(\"a<b & c>d\")\n",
    );
    let report = out_path("escaping_report.html");
    let _ = std::fs::remove_file(&report);

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    let html = std::fs::read_to_string(&report).unwrap();
    // The captured OUTPUT (rendered as plain HTML text, not an attribute)
    // must have its `<`/`>`/`&` entity-escaped.
    assert!(html.contains("a&lt;b &amp; c&gt;d"), "expected the printed output to be HTML-escaped");
    // The raw output text must never appear unescaped anywhere (which
    // would either break the HTML or, worse, be interpreted as markup).
    assert!(!html.contains("a<b & c>d"));
    // The report as a whole must still be well-formed enough that a
    // trivial tag-balance sanity check on the visible structure holds:
    // the file must not have literally split in half at the raw `<b`.
    assert!(html.contains("</html>"), "report must still be a complete, well-formed document");
}

// ---- A script that errors partway through still produces a report ----

#[test]
fn report_still_generated_when_script_errors_partway_through() {
    let script = write_script(
        "errors.qu",
        "print(\"before the crash\")\nthis_is_not_a_real_function()\nprint(\"never reached\")\n",
    );
    let report = out_path("errors_report.html");
    let _ = std::fs::remove_file(&report);

    let output = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap(), "--report", report.to_str().unwrap()])
        .output()
        .expect("failed to run qu");

    // `qu run` itself still fails (unaffected by --report)...
    assert!(!output.status.success());
    // ...but the report was still written, with whatever ran before the
    // failure, not silently skipped.
    assert!(report.exists(), "a report should still be generated even though the script errored");
    let html = std::fs::read_to_string(&report).unwrap();
    assert!(html.contains("before the crash"));
    // The script has no `#%%` markers, so it's a single cell whose CODE
    // panel legitimately shows the whole file (including the unreached
    // line) -- same documented behavior as the `write_report` snapshot
    // case. What must NOT happen is the unreached print's OUTPUT showing
    // up in the terminal panel.
    let term_body = {
        let start = html.find("class=\"term-body\"").expect("terminal panel present");
        let open_end = html[start..].find('>').map(|i| start + i + 1).unwrap();
        let close = html[open_end..].find("</pre>").map(|i| open_end + i).unwrap();
        &html[open_end..close]
    };
    assert!(term_body.contains("before the crash"));
    assert!(!term_body.contains("never reached"), "output after the failure point must not appear in the captured terminal panel");
    assert!(html.contains("win-title\">Error<"), "expected an Error panel");
    assert!(html.contains("this_is_not_a_real_function"), "expected the real error message in the report");
}

// ---- The in-script write_report(path) builtin ----

#[test]
fn write_report_builtin_generates_a_report_from_inside_the_script() {
    let report = out_path("write_report_builtin.html");
    let _ = std::fs::remove_file(&report);
    let script = write_script(
        "write_report_builtin.qu",
        &format!(
            "x = 7\nprint(\"builtin report, x = {{x}}\")\nwrite_report(\"{}\")\n",
            report.to_string_lossy().replace('\\', "\\\\")
        ),
    );

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    assert!(report.exists(), "write_report(path) should write a real file");
    let html = std::fs::read_to_string(&report).unwrap();
    assert!(html.contains("builtin report, x = 7"), "expected the real captured output");
    assert!(html.contains("write_report"), "expected the script's own source (including the write_report call itself)");
    assert_eq!(html.matches("class=\"win code-win\"").count(), 1, "the builtin always renders a single combined panel");
}

#[test]
fn write_report_builtin_captures_only_output_so_far_not_later_prints() {
    let report = out_path("write_report_snapshot.html");
    let _ = std::fs::remove_file(&report);
    let script = write_script(
        "write_report_snapshot.qu",
        &format!(
            "print(\"seen before the snapshot\")\nwrite_report(\"{}\")\nprint(\"seen after the snapshot\")\n",
            report.to_string_lossy().replace('\\', "\\\\")
        ),
    );

    let status = Command::new(qu_bin())
        .args(["run", script.to_str().unwrap()])
        .status()
        .expect("failed to run qu");
    assert!(status.success());

    let html = std::fs::read_to_string(&report).unwrap();
    // The CODE panel legitimately shows the whole script (see
    // `qu_interp::report`'s module doc comment: `write_report` sees the
    // full source handed to `run()`, since normal execution parses and
    // runs the whole file in one call) -- so it's expected, not a bug,
    // that the later `print(...)` call's own SOURCE TEXT appears
    // in the code panel. What must NOT happen is that later print
    // actually firing and its OUTPUT showing up in the terminal panel.
    let term_body = {
        let start = html.find("class=\"term-body\"").expect("terminal panel present");
        let open_end = html[start..].find('>').map(|i| start + i + 1).unwrap();
        let close = html[open_end..].find("</pre>").map(|i| open_end + i).unwrap();
        &html[open_end..close]
    };
    assert!(term_body.contains("seen before the snapshot"));
    assert!(
        !term_body.contains("seen after the snapshot"),
        "write_report is a snapshot at the point it's called -- later output must not appear in the captured terminal panel, even though the later print's own source text legitimately appears in the code panel"
    );
}
