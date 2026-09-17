//! Truthfulness checks for the generated `builtin_docs` table.
//!
//! In a SEPARATE file from `builtin_docs.rs` on purpose: that file is
//! rewritten wholesale by `tools/gen_builtin_docs.qu`, so tests written
//! inside it are deleted by the next regeneration -- silently, and the
//! suite then reports "0 passed" rather than failing, which reads as
//! success. These live next door instead, where the generator cannot
//! reach them.

use crate::builtin_docs::{builtin_doc, BUILTIN_DOCS};
use crate::BUILTIN_NAMES;

/// `builtin_doc` binary-searches, so an unsorted table would not fail
/// loudly -- it would silently fail to find perfectly good entries, and
/// `help()` would report "no entry" for a documented builtin.
#[test]
fn table_is_sorted_by_name() {
    for pair in BUILTIN_DOCS.windows(2) {
        assert!(
            pair[0].name < pair[1].name,
            "table out of order at {:?} / {:?} -- regenerate with tools/gen_builtin_docs.qu",
            pair[0].name,
            pair[1].name
        );
    }
}

/// Every documented name must be something the engine answers to: a
/// builtin, or a module-qualified function from `MODULE_EXPORTS`.
///
/// This caught 38 entries on its first run. The chapters document more
/// than builtins -- plotting COMMANDS (`axis`, `hold`, `show`), the
/// operator `not`, and command syntaxes with spaces ("watch url") -- and
/// all of them had been swept into a table named for builtins. Worse, 18
/// receiver methods were keyed with the leading dot the chapters write
/// them with (`.push`, `.pop`), so no `help("push")` could ever find them.
#[test]
fn every_documented_name_is_resolvable() {
    let orphans: Vec<&str> = BUILTIN_DOCS
        .iter()
        .map(|d| d.name)
        .filter(|n| !BUILTIN_NAMES.contains(n) && !n.contains('.'))
        .collect();
    assert!(
        orphans.is_empty(),
        "{} documented name(s) are neither builtins nor module-qualified: {:?}",
        orphans.len(),
        orphans
    );
}

/// A lookup key with a space in it can never be the argument of a
/// `help("...")` call, so it is dead weight that also inflates the table.
#[test]
fn every_name_is_a_usable_lookup_key() {
    for d in BUILTIN_DOCS {
        assert!(
            !d.name.contains(' ') && !d.name.starts_with('.') && !d.name.is_empty(),
            "{:?} is not a name anyone can look up",
            d.name
        );
    }
}

/// Binary search off by one at an edge is the classic way for this to
/// pass a spot check and fail in use, so the ends are tested explicitly.
#[test]
fn lookup_finds_first_last_and_middle() {
    let first = BUILTIN_DOCS.first().expect("table is not empty");
    let last = BUILTIN_DOCS.last().expect("table is not empty");
    let middle = &BUILTIN_DOCS[BUILTIN_DOCS.len() / 2];
    for expected in [first, last, middle] {
        let found = builtin_doc(expected.name)
            .unwrap_or_else(|| panic!("could not find {:?}", expected.name));
        assert_eq!(found.name, expected.name);
    }
    assert!(builtin_doc("definitely_not_a_builtin_name").is_none());
}

/// A summary that is a whole paragraph defeats the point: `help()` prints
/// into a terminal, and F1 in Qu Studio into a fixed-height panel.
#[test]
fn summaries_stay_one_line() {
    for d in BUILTIN_DOCS {
        assert!(!d.summary.contains('\n'), "{} has a multi-line summary", d.name);
        assert!(
            d.summary.chars().count() <= 200,
            "{} has a {}-char summary, which is a paragraph, not a line",
            d.name,
            d.summary.chars().count()
        );
    }
}

/// The table is worth having only if it covers most of what people look
/// up. A sharp drop here means the chapters or the parser moved, and
/// `help()` quietly went back to saying nothing useful.
#[test]
fn coverage_stays_high() {
    let documented = BUILTIN_NAMES
        .iter()
        .filter(|n| builtin_doc(n).is_some())
        .count();
    let pct = (documented * 100) / BUILTIN_NAMES.len();
    assert!(
        pct >= 90,
        "only {}/{} builtins ({}%) have documentation -- was ~99% when this was written",
        documented,
        BUILTIN_NAMES.len(),
        pct
    );
}

/// The table is checked into the tree, so a chapter edited without
/// regenerating leaves it stale -- and a stale table is silently wrong
/// rather than loudly broken. This is what turns "somebody must remember
/// to run the generator" into a failing test.
///
/// It deliberately does NOT reimplement the parsing. Reproducing
/// `one_line`/`is_arg_sentence`/`desc_column` here would be a second
/// implementation of the logic in a second language, which is the exact
/// divergence that shipping two generators would cause: within hours of
/// one being ported, two correctness fixes existed in one copy and not
/// the other.
///
/// So it checks the cheap half, with a rule no shared logic is needed for:
/// every backticked name in a chapter table's first cell that the engine
/// actually answers to must HAVE an entry. That catches the staleness that
/// matters -- a builtin documented but absent from the table, so `help()`
/// says nothing about a function whose docs exist. It does not catch an
/// edited description, which is a far cheaper kind of wrong.
#[test]
fn table_has_an_entry_for_every_documented_builtin() {
    let chapters = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../book/src/stdlib");
    let Ok(entries) = std::fs::read_dir(&chapters) else {
        // Not a failure: the crate is buildable from a packaged source
        // tree without the book beside it.
        eprintln!("skipping: no book at {}", chapters.display());
        return;
    };

    let mut missing: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with("| `") {
                continue;
            }
            // First cell only, and only names -- no description parsing.
            let Some(first) = line.split('|').nth(1) else { continue };
            for raw in first.split(',') {
                let name = raw.trim().trim_matches('`').trim();
                // The chapters write receiver methods with a leading dot;
                // the engine knows them without it.
                let name = name.strip_prefix('.').unwrap_or(name);
                if name.is_empty() || name.contains(' ') || name.contains('<') {
                    continue;
                }
                if BUILTIN_NAMES.contains(&name) && builtin_doc(name).is_none() {
                    missing.push(format!("{} ({})", name, path.file_name().unwrap().to_string_lossy()));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "{} builtin(s) are documented in the chapters but absent from the table -- \
         the chapters changed without regenerating. Run: qu run tools/gen_builtin_docs.qu\n{:?}",
        missing.len(),
        missing
    );
}
