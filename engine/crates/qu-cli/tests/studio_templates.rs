//! Every built-in template in Qu Studio's sidebar must run, and the ones that
//! draw must draw.
//!
//! The templates live as backtick strings in `qu-studio-tauri/src/App.tsx`
//! (`template: \`...\``). Three of them (K-Means, Neural Network, Spectrogram)
//! shipped broken because nothing ever ran them; this test extracts each one,
//! runs it with the real `qu` binary, and fails if it exits non-zero, prints
//! an error, or -- when its code calls a plotting function -- renders no figure.

use std::path::{Path, PathBuf};
use std::process::Command;

fn app_tsx() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../qu-studio-tauri/src/App.tsx")
}

/// The text of every `template: \`...\`` literal, unescaped the way a JS
/// template literal would be. Interpolation (`${`) is rejected: a template with
/// one would not be the program the test is about to run.
fn extract_templates(src: &str) -> Vec<String> {
    let marker = "template: `";
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find(marker) {
        let body = &rest[i + marker.len()..];
        let mut text = String::new();
        let mut chars = body.char_indices().peekable();
        let mut end = None;
        while let Some((idx, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, '`')) => text.push('`'),
                    Some((_, '$')) => text.push('$'),
                    Some((_, '\\')) => text.push('\\'),
                    Some((_, 'n')) => text.push('\n'),
                    Some((_, 't')) => text.push('\t'),
                    Some((_, other)) => {
                        text.push('\\');
                        text.push(other);
                    }
                    None => break,
                },
                '`' => {
                    end = Some(idx);
                    break;
                }
                '$' if matches!(chars.peek(), Some((_, '{'))) => {
                    panic!("a template uses `${{...}}` interpolation; this test cannot run it");
                }
                other => text.push(other),
            }
        }
        let end = end.expect("unterminated template literal in App.tsx");
        out.push(text);
        rest = &body[end + 1..];
    }
    out
}

fn draws(code: &str) -> bool {
    ["plot(", "scatter(", "bar(", "stem(", "imagesc(", "heatmap(", "spectrogram(", "hist("]
        .iter()
        .any(|f| code.contains(f))
}

#[test]
fn every_studio_template_runs_and_the_plotting_ones_plot() {
    let src = std::fs::read_to_string(app_tsx()).expect("read qu-studio-tauri/src/App.tsx");
    let templates = extract_templates(&src);
    assert!(
        templates.len() >= 8,
        "expected at least the 8 built-in templates, found {}",
        templates.len()
    );
    let dir = std::env::temp_dir().join(format!("qu-studio-templates-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    for (n, code) in templates.iter().enumerate() {
        let file = dir.join(format!("template_{n}.qu"));
        let svg = dir.join(format!("template_{n}.svg"));
        std::fs::write(&file, code).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_qu"))
            .arg("run")
            .arg(&file)
            .arg("--emit-figure")
            .arg(&svg)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("spawn qu");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let first_line = code.lines().next().unwrap_or("").trim();
        if !out.status.success() {
            failures.push(format!("template #{n} (`{first_line}`) exited {:?}: {stderr}{stdout}", out.status.code()));
            continue;
        }
        if draws(code) {
            let text = std::fs::read_to_string(&svg).unwrap_or_default();
            if !text.contains("<svg") {
                failures.push(format!("template #{n} (`{first_line}`) calls a plotting function but rendered no figure"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{} template(s) broken:\n{}", failures.len(), failures.join("\n---\n"));
}

#[test]
fn the_extractor_reads_a_template_literal_the_way_js_does() {
    let src = "x = { template: `a\\`b\\\\c\\$d\nend` , y: 1 }";
    let t = extract_templates(src);
    assert_eq!(t, vec!["a`b\\c$d\nend".to_string()]);
}
