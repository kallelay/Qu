//! Reads a real MATLAB-written `.mat` file end to end.
//!
//! The unit tests in `matfile.rs` cover the format's pieces with
//! hand-built fixtures. This one covers the thing those cannot: an actual
//! file, written by MATLAB, compressed with dynamic-Huffman blocks, with
//! nested structs -- the combination that either works on the first real
//! file or does not work at all.
//!
//! The file lives outside the repository (it is Ahmed's research data, not
//! a fixture to vendor), so the test skips when it is absent rather than
//! failing on a machine that does not have it.

use qu_interp::matfile::{read_mat, MatValue};

/// Where the real `.mat` fixture lives, from the environment.
///
/// It is a working file from a real project -- too large and too specific
/// to vendor -- so this test has always skipped when it is absent. The path
/// used to be a constant, which put one machine's directory layout, and the
/// name of an unpublished paper, into a file that ships. Same behaviour, no
/// names: set `QU_MATFILE_FIXTURE` to run it.
fn fixture_path() -> Option<String> {
    std::env::var("QU_MATFILE_FIXTURE").ok()
}

fn describe(v: &MatValue) -> String {
    match v {
        MatValue::Numeric { dims, data } => format!("numeric {dims:?} first={:?}", data.first()),
        MatValue::Complex { dims, .. } => format!("complex {dims:?}"),
        MatValue::Str(s) => format!("str {s:?}"),
        MatValue::Struct(f) => format!(
            "struct {{{}}}",
            f.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
        ),
        MatValue::Cell { dims, .. } => format!("cell {dims:?}"),
        MatValue::Unsupported(k) => format!("unsupported {k}"),
    }
}

#[test]
fn reads_the_paper_data_file() {
    let Some(path) = fixture_path() else {
        eprintln!("skipping: set QU_MATFILE_FIXTURE to a real .mat to run this");
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping: {path} not present on this machine");
        return;
    };
    let vars = read_mat(&bytes).expect("the file should parse");
    assert!(!vars.is_empty(), "expected at least one variable");
    for (name, v) in &vars {
        println!("{name}: {}", describe(v));
        if let MatValue::Struct(fields) = v {
            for (k, fv) in fields {
                println!("    .{k}: {}", describe(fv));
            }
        }
    }
    // The scripts being ported index `m['config'][0,0]` and
    // `m['results'][0,0]`, so both have to be present and be structs.
    for want in ["config", "results"] {
        let found = vars.iter().find(|(n, _)| n == want);
        assert!(found.is_some(), "expected a `{want}` variable, got {:?}",
                vars.iter().map(|(n, _)| n).collect::<Vec<_>>());
        assert!(matches!(found.unwrap().1, MatValue::Struct(_)), "`{want}` should be a struct");
    }
}
