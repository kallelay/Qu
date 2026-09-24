//! § `image.regions` via `import image` (2026-09-24).
//!
//! `text_and_regions.rs` covers the older, unrelated `image_regions(...)`
//! flat alias (`regions_ops.rs` -- a separate, hand-duplicated
//! implementation predating this module, not touched here). Nothing in this
//! crate's own test suite previously exercised the NAMESPACED
//! `image.regions(...)` dispatch arm (`f == "image::regions"` in `lib.rs`,
//! behind `#[cfg(feature = "image")]`) at all -- `qu-image`'s own unit tests
//! cover `qu_image::regions` directly, but the dispatch arm's own glue code
//! (`Value::Model` unwrapping, the `intensity_image=` keyword's
//! `Value::Image` handling and its wrong-type error) has no coverage
//! anywhere else. This file is that coverage, gated on the same feature the
//! arm itself requires so the default (no `--features image`) build and
//! test run are unaffected.

#![cfg(feature = "image")]

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

/// Same 2x2 blob at (5,5) `text_and_regions.rs`'s `BLOB` fixture uses, plus
/// `import image` so the namespaced `image.regions` is reachable.
const BLOB: &str = r#"
import image
m = zeros(20, 20)
m[5,5] = 255.0
m[5,6] = 255.0
m[6,5] = 255.0
m[6,6] = 255.0
lab = bwlabel(image_from_matrix(m))
"#;

#[test]
fn image_regions_returns_the_new_shape_columns_for_a_2x2_square() {
    // A solid 2x2 square is its own known-answer fixture: it is fully
    // convex, isotropic (its second moments in x and y are equal, and the
    // cross moment is zero by symmetry), and its 4-pixel border ring is
    // walked by 4 orthogonal steps (2*(W+H-2) = 2*(2+2-2) = 4), not the
    // 8-pixel "edge crossing" count some tools would report.
    let it = run(&format!(
        "{BLOB}\nr = image.regions(lab)\n\
         p = r.perimeter_px[0]\ne = r.eccentricity[0]\no = r.orientation[0]\ns = r.solidity[0]\n"
    ));
    assert_eq!(num_of(&it, "p"), 4.0, "a 2x2 square's border ring is 4 orthogonal steps");
    assert_eq!(num_of(&it, "e"), 0.0, "a square is isotropic: eccentricity 0");
    assert_eq!(num_of(&it, "o"), 0.0, "an isotropic region's orientation is 0 by convention");
    assert_eq!(num_of(&it, "s"), 1.0, "a solid square is fully convex: solidity 1.0");
}

#[test]
fn image_regions_intensity_mean_reads_the_original_image_not_the_labels() {
    // A second, independent image (not the label matrix) holding four
    // distinct grayscale values at exactly the blob's four pixels --
    // `image_from_matrix` writes R=G=B, so BT.601 luma recovers the input
    // value exactly, and the expected mean is not tied to this crate's own
    // implementation of anything.
    let it = run(&format!(
        "{BLOB}\nmi = zeros(20, 20)\n\
         mi[5,5] = 10.0\nmi[5,6] = 30.0\nmi[6,5] = 50.0\nmi[6,6] = 90.0\n\
         img = image_from_matrix(mi)\n\
         r = image.regions(lab, intensity_image = img)\n\
         v = r.intensity_mean[0]\n"
    ));
    assert_eq!(num_of(&it, "v"), (10.0 + 30.0 + 50.0 + 90.0) / 4.0);
}

#[test]
fn image_regions_omits_intensity_mean_when_no_intensity_image_is_given() {
    let it = run(&format!("{BLOB}\nr = image.regions(lab)\n"));
    let Some(Value::Table(t)) = it.get("r") else {
        panic!("`r` is not a Table");
    };
    assert!(
        !t.column_names().contains(&"intensity_mean"),
        "intensity_mean must not appear without intensity_image="
    );
}

#[test]
fn image_regions_rejects_a_non_image_intensity_argument() {
    let mut it = Interp::new();
    let err = it
        .run(&format!("{BLOB}\nr = image.regions(lab, intensity_image = 5)\n"))
        .unwrap_err();
    assert!(
        format!("{err}").contains("intensity_image=") && format!("{err}").contains("must be an Image"),
        "got: {err}"
    );
}
