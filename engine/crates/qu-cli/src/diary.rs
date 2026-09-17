//! Jupyter-notebook-style HTML "diary" export.
//!
//! Ahmed's spec, verbatim: "for jupyter save, it's simple just do code block
//! followed by output as ascii or latex or image that's it, like a diary."
//! So: for each top-level statement in a `.qu` script, in source order, show
//! (1) that statement's own original source text as a code block, then
//! (2) whatever it produced — printed text (`print`/`write`/`writeline`/etc,
//! already routed into `Interp::out`) and/or a figure it created or updated
//! (embedded as an inline `<svg>`, reusing `plotting::render_svg` verbatim —
//! no rasterization, no new rendering path).
//!
//! This is a static transcript, not a literate-programming system: a plain
//! assignment with no printed/plotted side effect just shows its code block
//! with no output section under it.
//!
//! ## Statement-splitting approach (and its tradeoff)
//!
//! `qu_syntax::Stmt` carries no source span today, so there is no direct way
//! to slice the original source text back out for "the N-th top-level
//! statement". Rather than add span-tracking to the shared AST (bigger
//! surgery, real regression risk on a large shared file mid-edit by a
//! concurrent session), this re-derives top-level statement boundaries from
//! the ORIGINAL SOURCE TEXT using only `qu_syntax::parse` itself as the
//! oracle:
//!
//! 1. Tokenize once and collect every `Newline`/`;` token's end offset as a
//!    *candidate* statement-boundary byte position (`Eof` end too, for a
//!    final statement with no trailing separator). The lexer already
//!    suppresses `Newline` emission mid-continuation (trailing operator,
//!    open bracket, comma — see `qu_lexer`'s own doc comment), so every
//!    candidate here is already a position the grammar treats as a
//!    statement separator *somewhere* — either at top level or inside a
//!    nested block body.
//! 2. Walk the candidates in ascending order. For each one beyond the last
//!    committed offset, try `qu_syntax::parse` on the SOURCE PREFIX ending
//!    there. A prefix that stops mid-block (e.g. inside an unclosed `for`)
//!    is missing its `end for` and fails to parse; a prefix that stops right
//!    after a complete top-level statement succeeds. The first candidate
//!    that parses successfully and yields one more top-level statement than
//!    the last commit is exactly the end of the next top-level statement —
//!    take `source[committed..candidate]` as its source-text chunk.
//! 3. As a correctness check, this must account for exactly as many
//!    statements as parsing the whole source does. If it doesn't (some
//!    unanticipated construct), fail safe: show the whole script as one
//!    block rather than risk misattributing output to the wrong statement.
//!
//! This reuses the real grammar (via `parse`'s success/failure) as the
//! source of truth instead of re-implementing block-nesting rules, at the
//! cost of O(statements²) reparsing — irrelevant at diary-script sizes.
//!
//! Each chunk is then run through one persistent `Interp` via `Interp::run`
//! (the same "parse this fragment, execute against already-live state"
//! pattern `qu-cli`'s own REPL already uses), diffing `Interp::out` and
//! `Interp::figures` before/after to detect new printed text and new/changed
//! plots respectively.

use qu_interp::{plotting, Interp};

/// One top-level statement's transcript entry.
pub struct DiaryEntry {
    pub source: String,
    pub printed: Option<String>,
    pub svg: Option<String>,
}

pub struct DiaryReport {
    pub html: String,
    /// Set if the script raised an uncaught error partway through — the HTML
    /// still contains everything up to (and including) the failing
    /// statement, matching `qu run`'s own "print what we have, then report
    /// the error" behavior.
    pub error: Option<String>,
}

/// Split `src` into top-level statement source-text chunks. See the module
/// doc comment for the approach and its documented fallback.
pub fn split_top_level_statements(src: &str) -> Result<Vec<String>, String> {
    let total = qu_syntax::parse(src).map_err(|e| e.to_string())?.stmts.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    let mut bounds: Vec<usize> = qu_lexer::lex(src)
        .into_iter()
        .filter_map(|t| match t.tok {
            qu_lexer::Tok::Newline => Some(t.span.end),
            qu_lexer::Tok::Op(";") => Some(t.span.end),
            _ => None,
        })
        .collect();
    if bounds.last().copied() != Some(src.len()) {
        bounds.push(src.len());
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut chunks = Vec::new();
    let mut committed = 0usize;
    let mut last_count = 0usize;
    for b in bounds {
        if last_count >= total {
            break;
        }
        if b <= committed {
            continue;
        }
        if let Ok(prog) = qu_syntax::parse(&src[..b]) {
            let n = prog.stmts.len();
            if n > last_count {
                chunks.push(src[committed..b].to_string());
                committed = b;
                last_count = n;
            }
        }
    }

    if last_count != total {
        // Fallback documented above: something about this source didn't
        // decompose the way expected — don't guess, show it whole.
        return Ok(vec![src.to_string()]);
    }
    Ok(chunks)
}

/// Run `src` statement-by-statement, building the diary transcript.
pub fn run_diary(src: &str) -> DiaryReport {
    let chunks = match split_top_level_statements(src) {
        Ok(c) => c,
        Err(e) => {
            return DiaryReport {
                html: render_html(&[], Some(&e)),
                error: Some(e),
            }
        }
    };

    let mut it = Interp::new();
    let mut entries = Vec::new();
    let mut error = None;

    for chunk in chunks {
        let out_before = it.out.len();
        let figures_before = it.figures;
        let run_result = it.run(&chunk);

        let printed = if it.out.len() > out_before {
            Some(it.out[out_before..].to_string())
        } else {
            None
        };
        let svg = if it.figures != figures_before {
            Some(plotting::render_svg(&it.figure, it.figure.width, it.figure.height, false))
        } else {
            None
        };
        entries.push(DiaryEntry {
            source: chunk,
            printed,
            svg,
        });

        if let Err(e) = run_result {
            error = Some(e.to_string());
            break;
        }
    }

    DiaryReport {
        html: render_html(&entries, error.as_deref()),
        error,
    }
}

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

fn render_html(entries: &[DiaryEntry], error: Option<&str>) -> String {
    let mut body = String::new();
    for entry in entries {
        body.push_str("<div class=\"entry\">\n");
        body.push_str("<pre class=\"code\"><code>");
        body.push_str(&html_escape(&entry.source));
        body.push_str("</code></pre>\n");
        if let Some(printed) = &entry.printed {
            body.push_str("<pre class=\"output\">");
            body.push_str(&html_escape(printed));
            body.push_str("</pre>\n");
        }
        if let Some(svg) = &entry.svg {
            body.push_str("<div class=\"figure\">\n");
            body.push_str(svg);
            body.push_str("</div>\n");
        }
        body.push_str("</div>\n");
    }
    if let Some(e) = error {
        body.push_str("<div class=\"entry\">\n<pre class=\"error\">Error: ");
        body.push_str(&html_escape(e));
        body.push_str("</pre>\n</div>\n");
    }

    format!(
        "<!doctype html>\n\
<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Qu diary</title>\n\
<style>\n\
body {{ font-family: -apple-system, \"Segoe UI\", sans-serif; margin: 2rem auto; \
max-width: 900px; line-height: 1.4; color: #1a1a1a; background: #fff; padding: 0 1rem; }}\n\
h1 {{ font-size: 1.1rem; color: #666; font-weight: 600; }}\n\
.entry {{ margin-bottom: 1.5rem; }}\n\
pre {{ margin: 0.4rem 0; padding: 0.75rem 1rem; border-radius: 6px; overflow-x: auto; \
font-family: Consolas, Menlo, \"Courier New\", monospace; font-size: 0.85rem; }}\n\
pre.code {{ background: #f5f5f7; border: 1px solid #ddd; white-space: pre; }}\n\
pre.output {{ background: #fbfbe8; border: 1px solid #e6e0a0; white-space: pre-wrap; }}\n\
pre.error {{ background: #fff0f0; border: 1px solid #eaa; color: #900; white-space: pre-wrap; }}\n\
.figure {{ margin: 0.4rem 0; }}\n\
.figure svg {{ max-width: 100%; height: auto; border: 1px solid #ddd; border-radius: 6px; }}\n\
</style>\n</head>\n<body>\n<h1>Qu diary</h1>\n{body}</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_statements_in_order() {
        let src = "x = 5\nprint(x)\ny = x + 1\n";
        let chunks = split_top_level_statements(src).unwrap();
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].contains("x = 5"));
        assert!(chunks[1].contains("print(x)"));
        assert!(chunks[2].contains("y = x + 1"));
    }

    #[test]
    fn splits_multiline_block_as_one_statement() {
        let src = "n = 3\nfor i = 1 to n\n  print(i)\nend for\n";
        let chunks = split_top_level_statements(src).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].contains("for i = 1 to n"));
        assert!(chunks[1].contains("end for"));
    }

    #[test]
    fn diary_html_contains_code_output_and_svg_in_order() {
        // `savefig` really writes to disk, so point it at the OS temp dir
        // rather than littering the crate directory with a stray `.svg`.
        let out_path = std::env::temp_dir().join("qu_diary_test_out.svg");
        let out_path = out_path.to_string_lossy().replace('\\', "/");
        let src = format!(
            "x = 5\nprint(\"hello {{x}}\")\nplot(0 to 9, 0 to 9)\nsavefig(\"{out_path}\")\n"
        );
        let report = run_diary(&src);
        let _ = std::fs::remove_file(&out_path);
        assert!(report.error.is_none(), "unexpected error: {:?}", report.error);
        let html = report.html;

        let assign_pos = html.find("x = 5").expect("assignment code block present");
        let print_code_pos = html.find("print(").expect("print code block present");
        let output_pos = html.find("hello 5").expect("printed text present");
        let plot_code_pos = html.find("plot(0 to 9").expect("plot code block present");
        let svg_pos = html.find("<svg").expect("embedded svg present");

        assert!(assign_pos < print_code_pos, "assignment must come before print");
        assert!(print_code_pos < output_pos, "print code must come before its own output");
        assert!(output_pos < plot_code_pos, "print output must come before the plot statement");
        assert!(plot_code_pos < svg_pos, "plot statement must come before the embedded svg");
    }

    #[test]
    fn no_output_statement_gets_bare_code_block() {
        let src = "x = 5\n";
        let report = run_diary(src);
        assert!(report.error.is_none());
        assert!(report.html.contains("x = 5"));
        assert!(!report.html.contains("class=\"output\""));
        assert!(!report.html.contains("<svg"));
    }

    #[test]
    fn error_partway_through_is_reported_and_still_renders_prior_entries() {
        let src = "print(\"before\")\nthis_is_not_a_thing()\n";
        let report = run_diary(src);
        assert!(report.error.is_some());
        assert!(report.html.contains("before"));
        assert!(report.html.contains("class=\"error\""));
    }
}
