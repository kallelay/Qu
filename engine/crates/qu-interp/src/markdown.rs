//! Markdown to HTML and back.
//!
//! Both directions, because either one alone is a trap: a report generator
//! that can only go forward leaves you with HTML you cannot edit, and a
//! scraper that can only come back leaves you with Markdown you cannot
//! publish. Round-tripping the subset below is checked by the tests at the
//! bottom rather than asserted here.
//!
//! **The subset.** Headings, paragraphs, unordered and ordered lists,
//! fenced code with its language, blockquotes, pipe tables, thematic
//! breaks, and inline code, bold, italic, links and images. That is what
//! technical documents are actually made of. Deliberately absent: setext
//! headings, reference links, footnotes, nested lists, and raw inline HTML
//! passthrough — each is a real feature of the format and each doubles the
//! parser, and none of them appears in the documents this exists for.
//!
//! Written here rather than pulled in: `pulldown-cmark` would do the
//! forward direction better than this does, and nothing on crates.io does
//! the reverse direction well. Carrying half of it as a dependency and
//! hand-writing the other half is the worst of both.

/// Convert Markdown to HTML.
pub fn md_to_html(src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    let mut para: Vec<String> = Vec::new();

    // A paragraph ends at a blank line or at any block that starts; this
    // closure is called at every one of those places.
    macro_rules! flush_para {
        () => {
            if !para.is_empty() {
                out.push_str("<p>");
                out.push_str(&inline(&para.join(" ")));
                out.push_str("</p>\n");
                para.clear();
            }
        };
    }

    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();

        if t.is_empty() {
            flush_para!();
            i += 1;
            continue;
        }

        // Fenced code. The info string becomes a language class, which is
        // what every highlighter on the receiving end looks for.
        if let Some(rest) = t.strip_prefix("```") {
            flush_para!();
            let lang = rest.trim();
            let mut body = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                body.push_str(lines[i]);
                body.push('\n');
                i += 1;
            }
            i += 1; // the closing fence
            out.push_str("<pre><code");
            if !lang.is_empty() {
                out.push_str(&format!(" class=\"language-{}\"", escape(lang)));
            }
            out.push('>');
            out.push_str(&escape(&body));
            out.push_str("</code></pre>\n");
            continue;
        }

        if t == "---" || t == "***" || t == "___" {
            flush_para!();
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }

        if t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count();
            if level <= 6 && t.chars().nth(level) == Some(' ') {
                flush_para!();
                let text = t[level + 1..].trim();
                out.push_str(&format!("<h{level}>{}</h{level}>\n", inline(text)));
                i += 1;
                continue;
            }
        }

        if let Some(rest) = t.strip_prefix("> ").or_else(|| t.strip_prefix(">")) {
            flush_para!();
            let mut body = vec![rest.trim().to_string()];
            i += 1;
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let l = lines[i].trim_start().trim_start_matches('>').trim();
                body.push(l.to_string());
                i += 1;
            }
            out.push_str(&format!("<blockquote><p>{}</p></blockquote>\n", inline(&body.join(" "))));
            continue;
        }

        if is_bullet(t) || is_ordered(t) {
            flush_para!();
            let ordered = is_ordered(t);
            let tag = if ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag}>\n"));
            while i < lines.len() {
                let l = lines[i].trim();
                let item = if ordered { strip_ordered(l) } else { strip_bullet(l) };
                let Some(first) = item else { break };
                // Continuation lines: an indented line under an item
                // belongs to it, the way a wrapped sentence does.
                let mut text = vec![first.to_string()];
                i += 1;
                while i < lines.len() {
                    let n = lines[i];
                    if n.trim().is_empty() || is_bullet(n.trim()) || is_ordered(n.trim()) {
                        break;
                    }
                    if !n.starts_with(' ') && !n.starts_with('\t') {
                        break;
                    }
                    text.push(n.trim().to_string());
                    i += 1;
                }
                out.push_str(&format!("<li>{}</li>\n", inline(&text.join(" "))));
            }
            out.push_str(&format!("</{tag}>\n"));
            continue;
        }

        // Pipe table. The first row is the header when a separator row
        // (`|---|---|`) follows it, which is how the format distinguishes
        // a table from a run of pipes.
        if t.starts_with('|') {
            flush_para!();
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut header = false;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let l = lines[i].trim();
                if is_table_rule(l) {
                    header = !rows.is_empty();
                    i += 1;
                    continue;
                }
                rows.push(split_row(l));
                i += 1;
            }
            out.push_str("<table>\n");
            for (n, row) in rows.iter().enumerate() {
                let cell = if header && n == 0 { "th" } else { "td" };
                out.push_str("<tr>");
                for c in row {
                    out.push_str(&format!("<{cell}>{}</{cell}>", inline(c)));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
            continue;
        }

        para.push(t.to_string());
        i += 1;
    }
    flush_para!();
    out
}

fn is_bullet(t: &str) -> bool {
    t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")
}

fn strip_bullet(t: &str) -> Option<&str> {
    if is_bullet(t) {
        Some(t[2..].trim())
    } else {
        None
    }
}

fn is_ordered(t: &str) -> bool {
    strip_ordered(t).is_some()
}

fn strip_ordered(t: &str) -> Option<&str> {
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = &t[digits..];
    rest.strip_prefix(". ").map(str::trim)
}

fn is_table_rule(t: &str) -> bool {
    t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn split_row(t: &str) -> Vec<String> {
    let inner = t.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Inline markup. Code spans are taken first so nothing inside them is
/// interpreted, which is the whole point of a code span.
fn inline(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push_str(&escape(&chars[i + 1].to_string()));
            i += 2;
            continue;
        }
        if c == '`' {
            if let Some(end) = find_from(&chars, i + 1, '`') {
                let body: String = chars[i + 1..end].iter().collect();
                out.push_str(&format!("<code>{}</code>", escape(&body)));
                i = end + 1;
                continue;
            }
        }
        if c == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find_pair(&chars, i + 2) {
                let body: String = chars[i + 2..end].iter().collect();
                out.push_str(&format!("<strong>{}</strong>", inline(&body)));
                i = end + 2;
                continue;
            }
        }
        if c == '*' {
            if let Some(end) = find_from(&chars, i + 1, '*') {
                let body: String = chars[i + 1..end].iter().collect();
                if !body.is_empty() {
                    out.push_str(&format!("<em>{}</em>", inline(&body)));
                    i = end + 1;
                    continue;
                }
            }
        }
        // `![alt](src)` before `[text](href)`, since one is a prefix of
        // the other.
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some((text, url, next)) = link_at(&chars, i + 1) {
                out.push_str(&format!("<img src=\"{}\" alt=\"{}\">", escape(&url), escape(&text)));
                i = next;
                continue;
            }
        }
        if c == '[' {
            if let Some((text, url, next)) = link_at(&chars, i) {
                out.push_str(&format!("<a href=\"{}\">{}</a>", escape(&url), inline(&text)));
                i = next;
                continue;
            }
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn find_from(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len()).find(|&k| chars[k] == needle)
}

fn find_pair(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&k| chars[k] == '*' && chars[k + 1] == '*')
}

/// `[text](url)` starting at `chars[at] == '['`. Returns the text, the URL,
/// and the index just past the closing paren.
fn link_at(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let close = find_from(chars, at + 1, ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = find_from(chars, close + 2, ')')?;
    let text: String = chars[at + 1..close].iter().collect();
    let url: String = chars[close + 2..end].iter().collect();
    Some((text, url, end + 1))
}

// ── the other direction ────────────────────────────────────────────────

/// Convert HTML to Markdown.
///
/// A tag walker rather than a full parser: it reads tags in order and keeps
/// a stack of the block it is inside. Malformed HTML degrades to its text
/// rather than erroring, because the input is usually something a page
/// produced rather than something a person wrote, and half a document is
/// more useful than a diagnostic.
pub fn html_to_md(src: &str) -> String {
    let mut out = String::new();
    let mut text = String::new();
    let mut list: Vec<&'static str> = Vec::new();
    let mut ordinal: Vec<usize> = Vec::new();
    let mut in_pre = false;
    let mut pending_row: Vec<String> = Vec::new();
    let mut row_is_header = false;
    let mut table_started = false;

    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0;

    // Text accumulates until a block-level tag closes it; `emit` writes it
    // out with whatever prefix that block needs.
    macro_rules! flush {
        ($prefix:expr) => {{
            let body = squeeze(&text);
            if !body.is_empty() {
                out.push_str($prefix);
                out.push_str(&body);
                out.push_str("\n\n");
            }
            text.clear();
        }};
    }

    while i < bytes.len() {
        if bytes[i] != '<' {
            text.push(bytes[i]);
            i += 1;
            continue;
        }
        let Some(close) = find_from(&bytes, i + 1, '>') else {
            text.push('<');
            i += 1;
            continue;
        };
        let raw: String = bytes[i + 1..close].iter().collect();
        i = close + 1;
        let lower = raw.trim().to_lowercase();
        let name = lower
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("")
            .to_string();
        let closing = lower.starts_with('/');

        match (name.as_str(), closing) {
            ("br", _) => text.push('\n'),
            ("hr", _) => {
                flush!("");
                out.push_str("---\n\n");
            }
            ("h1", true) | ("h2", true) | ("h3", true) | ("h4", true) | ("h5", true)
            | ("h6", true) => {
                let level: usize = name[1..].parse().unwrap_or(1);
                flush!(&format!("{} ", "#".repeat(level)));
            }
            ("p", true) | ("div", true) => flush!(""),
            ("blockquote", true) => flush!("> "),
            ("pre", false) => {
                flush!("");
                in_pre = true;
            }
            ("pre", true) => {
                out.push_str("```\n");
                out.push_str(text.trim_matches('\n'));
                out.push_str("\n```\n\n");
                text.clear();
                in_pre = false;
            }
            ("code", _) if !in_pre => text.push('`'),
            ("strong", _) | ("b", _) => text.push_str("**"),
            ("em", _) | ("i", _) => text.push('*'),
            ("ul", false) => {
                flush!("");
                list.push("-");
                ordinal.push(0);
            }
            ("ol", false) => {
                flush!("");
                list.push("1");
                ordinal.push(0);
            }
            ("ul", true) | ("ol", true) => {
                list.pop();
                ordinal.pop();
                out.push('\n');
            }
            ("li", true) => {
                let marker = match list.last() {
                    Some(&"1") => {
                        let n = ordinal.last_mut().map(|o| {
                            *o += 1;
                            *o
                        });
                        format!("{}. ", n.unwrap_or(1))
                    }
                    _ => "- ".to_string(),
                };
                let body = squeeze(&text);
                if !body.is_empty() {
                    out.push_str(&marker);
                    out.push_str(&body);
                    out.push('\n');
                }
                text.clear();
            }
            ("table", false) => {
                flush!("");
                table_started = false;
            }
            ("table", true) => out.push('\n'),
            ("tr", false) => {
                pending_row.clear();
                row_is_header = false;
            }
            ("th", false) => row_is_header = true,
            ("td", true) | ("th", true) => {
                pending_row.push(squeeze(&text));
                text.clear();
            }
            ("tr", true) => {
                if !pending_row.is_empty() {
                    out.push_str(&format!("| {} |\n", pending_row.join(" | ")));
                    if row_is_header || !table_started {
                        out.push_str(&format!(
                            "|{}\n",
                            pending_row.iter().map(|_| "---|").collect::<String>()
                        ));
                        table_started = true;
                    }
                }
                pending_row.clear();
            }
            ("a", false) => {
                // The href is needed at the CLOSING tag, so it rides along
                // in the text as an opening bracket and is completed there.
                text.push('[');
                LINK_HREF.with(|h| *h.borrow_mut() = attr(&raw, "href"));
            }
            ("a", true) => {
                let href = LINK_HREF.with(|h| h.borrow().clone());
                text.push_str(&format!("]({})", href.unwrap_or_default()));
            }
            ("img", _) => {
                let src = attr(&raw, "src").unwrap_or_default();
                let alt = attr(&raw, "alt").unwrap_or_default();
                text.push_str(&format!("![{alt}]({src})"));
            }
            _ => {}
        }
    }
    flush!("");
    // Entities last, so a `&lt;` in the source does not become a `<` that
    // the tag walker above would then have tried to read as a tag.
    unescape(out.trim_end()).trim_end().to_string() + "\n"
}

thread_local! {
    static LINK_HREF: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The value of one attribute, single or double quoted.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let at = lower.find(&format!("{name}="))?;
    let rest = &tag[at + name.len() + 1..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return Some(rest.split_whitespace().next()?.to_string());
    }
    let end = rest[1..].find(quote)?;
    Some(rest[1..1 + end].to_string())
}

/// Collapse runs of whitespace to single spaces and trim. HTML treats every
/// run of whitespace as one space, and a Markdown document that keeps the
/// source's line wrapping is a document nobody can diff.
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_paragraphs_and_inline_markup() {
        let html = md_to_html("# Title\n\nA **bold** word and `code`.\n");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn a_code_span_is_not_interpreted_as_markup() {
        let html = md_to_html("Use `a * b` and `<tag>`.");
        assert!(html.contains("<code>a * b</code>"), "got {html}");
        assert!(html.contains("&lt;tag&gt;"), "got {html}");
        assert!(!html.contains("<em>"), "got {html}");
    }

    #[test]
    fn a_fence_keeps_its_language_and_escapes_its_body() {
        let html = md_to_html("```qu\nx = a < b\n```\n");
        assert!(html.contains("class=\"language-qu\""), "got {html}");
        assert!(html.contains("a &lt; b"), "got {html}");
    }

    #[test]
    fn lists_tables_and_rules() {
        let html = md_to_html("- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n");
        assert!(html.contains("<ul>\n<li>one</li>"), "got {html}");
        assert!(html.contains("<th>a</th>"), "got {html}");
        assert!(html.contains("<td>1</td>"), "got {html}");
        assert!(html.contains("<hr>"), "got {html}");
    }

    #[test]
    fn an_ordered_list_numbers_itself_rather_than_copying_the_source() {
        // The source says 1. 1. 1. — a common way to write one — and the
        // round trip must still come back 1. 2. 3.
        let md = html_to_md(&md_to_html("1. a\n1. b\n1. c\n"));
        assert!(md.contains("1. a"), "got {md}");
        assert!(md.contains("2. b"), "got {md}");
        assert!(md.contains("3. c"), "got {md}");
    }

    #[test]
    fn links_and_images_survive_both_directions() {
        let html = md_to_html("See [the docs](https://example.com/d) and ![a plot](p.svg).");
        assert!(html.contains("<a href=\"https://example.com/d\">the docs</a>"), "got {html}");
        assert!(html.contains("<img src=\"p.svg\" alt=\"a plot\">"), "got {html}");
        let md = html_to_md(&html);
        assert!(md.contains("[the docs](https://example.com/d)"), "got {md}");
        assert!(md.contains("![a plot](p.svg)"), "got {md}");
    }

    #[test]
    fn a_document_survives_the_round_trip() {
        let src = "# Report\n\nA paragraph with **bold** text.\n\n- one\n- two\n\n## Data\n\n| n | v |\n|---|---|\n| 1 | 2 |\n";
        let back = html_to_md(&md_to_html(src));
        assert!(back.contains("# Report"), "got {back}");
        assert!(back.contains("## Data"), "got {back}");
        assert!(back.contains("**bold**"), "got {back}");
        assert!(back.contains("- one"), "got {back}");
        assert!(back.contains("| 1 | 2 |"), "got {back}");
    }

    #[test]
    fn html_entities_come_back_as_the_characters_they_stand_for() {
        assert_eq!(html_to_md("<p>a &lt; b &amp;&amp; c &gt; d</p>").trim(), "a < b && c > d");
    }

    #[test]
    fn malformed_html_degrades_to_its_text_rather_than_erroring() {
        let md = html_to_md("<p>kept<");
        assert!(md.contains("kept"), "got {md}");
    }
}
