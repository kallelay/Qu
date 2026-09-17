//! What a rendered figure must never do.
//!
//! Every plotting bug found on 2026-09-04/05 -- duplicate tick labels, two
//! notations on one axis, an axis running to negative time, thirteen
//! crammed labels on a short subplot, bold tick numbers, fake bold -- was
//! invisible to a passing unit suite and only surfaced when somebody
//! looked at a picture. Each was also perfectly detectable from the SVG.
//!
//! So these are property assertions on real rendered output, not golden
//! files. A golden SVG would fail on every deliberate change (a colour, a
//! margin, a font) and teach everyone to re-bless it without reading the
//! diff, which is worse than no test. These check the things that are
//! wrong no matter what the figure is supposed to look like.

use qu_interp::plotting::{self, Figure};
use qu_interp::Interp;

fn render(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("script failed: {e}\n{src}"));
    plotting::render_svg(&it.figure, it.figure.width, it.figure.height, false)
}

fn figure_of(src: &str) -> Figure {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("script failed: {e}\n{src}"));
    it.figure.clone()
}

/// Every `<text>` in the SVG as (font-size, font-weight-or-none, content),
/// with any `<tspan>` markup flattened the way a reader sees it.
fn texts(svg: &str) -> Vec<(f64, Option<u32>, String)> {
    let mut out = Vec::new();
    for chunk in svg.split("<text").skip(1) {
        let Some(head_end) = chunk.find('>') else { continue };
        let head = &chunk[..head_end];
        let body_end = chunk.find("</text>").unwrap_or(chunk.len());
        let body = &chunk[head_end + 1..body_end];
        let attr = |name: &str| -> Option<String> {
            let pat = format!("{name}=\"");
            let i = head.find(&pat)? + pat.len();
            let j = head[i..].find('"')? + i;
            Some(head[i..j].to_string())
        };
        let size = attr("font-size").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let weight = attr("font-weight").and_then(|v| v.parse().ok());
        // Strip tspan tags but keep their text, so `10⁻⁶` reads as one label.
        let mut flat = String::new();
        let mut in_tag = false;
        for c in body.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => flat.push(c),
                _ => {}
            }
        }
        out.push((size, weight, flat));
    }
    out
}

/// Tick labels on the left of a panel, top to bottom.
fn y_tick_labels(svg: &str, geom: &plotting::PanelGeom) -> Vec<String> {
    let mut rows: Vec<(f64, String)> = Vec::new();
    for chunk in svg.split("<text").skip(1) {
        let Some(head_end) = chunk.find('>') else { continue };
        let head = &chunk[..head_end];
        if !head.contains("text-anchor=\"end\"") {
            continue;
        }
        let num = |name: &str| -> Option<f64> {
            let pat = format!("{name}=\"");
            let i = head.find(&pat)? + pat.len();
            let j = head[i..].find('"')? + i;
            head[i..j].parse().ok()
        };
        let (Some(x), Some(y)) = (num("x"), num("y")) else { continue };
        if x >= geom.left || y < geom.top - 2.0 || y > geom.bottom + 2.0 {
            continue;
        }
        let body_end = chunk.find("</text>").unwrap_or(chunk.len());
        let mut flat = String::new();
        let mut in_tag = false;
        for c in chunk[head_end + 1..body_end].chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => flat.push(c),
                _ => {}
            }
        }
        rows.push((y, flat));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    rows.into_iter().map(|(_, t)| t).collect()
}

fn geometry(svg: &str) -> Vec<plotting::PanelGeom> {
    // Re-derive from the figure rather than parsing the metadata JSON: the
    // point here is to check what was DRAWN, and the metadata is generated
    // from the same pass, so parsing it would test nothing extra.
    let _ = svg;
    Vec::new()
}

// ---------------------------------------------------------------- ticks

#[test]
fn no_axis_prints_the_same_tick_label_twice() {
    // Found live: an axis stepping by 0.0005 rounded each tick to three
    // decimals on its own and printed `0.002` twice.
    for src in [
        "x = linspace(0, 1, 20)\ny = x * 0.003\nplot(x, y)",
        "x = linspace(0, 1, 20)\ny = x * 0.0007\nplot(x, y)",
        "x = linspace(0, 1, 20)\ny = x * 123456.0\nplot(x, y)",
        "x = linspace(0, 1, 20)\ny = x * 1e-7\nplot(x, y)",
    ] {
        let fig = figure_of(src);
        let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
        let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
        for g in &geoms {
            let labels = y_tick_labels(&svg, g);
            let mut seen = std::collections::HashSet::new();
            for label in &labels {
                assert!(
                    seen.insert(label.clone()),
                    "duplicate y tick label {label:?} in {labels:?}\nscript: {src}"
                );
            }
        }
    }
}

#[test]
fn an_axis_does_not_mix_decimal_and_scientific_labels() {
    // Found live: `0.001` and `5.0e-4` on the same axis, because each tick
    // crossed the notation threshold independently.
    let fig = figure_of("x = linspace(0, 1, 20)\ny = x * 0.003\nplot(x, y)");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    for g in &geoms {
        let labels = y_tick_labels(&svg, g);
        // Zero is written "0" in either notation and is not a mixture.
        let meaningful: Vec<&String> = labels.iter().filter(|l| l.as_str() != "0").collect();
        // Scientific labels carry a superscript digit or a multiplication
        // sign. Testing for the substring "10" would match the perfectly
        // ordinary decimal label `0.0010`.
        let is_sci = |l: &str| {
            l.contains('\u{00D7}') || l.chars().any(|c| ('\u{2070}'..='\u{209F}').contains(&c))
        };
        let sci = meaningful.iter().filter(|l| is_sci(l)).count();
        assert!(
            sci == 0 || sci == meaningful.len(),
            "axis mixes notations: {labels:?}"
        );
    }
}

#[test]
fn scientific_ticks_use_a_superscript_not_e_notation() {
    // `1.0e-6` is programmer notation and does not belong on a figure.
    let svg = render("x = linspace(0, 1, 20)\ny = x * 1e-6\nplot(x, y)");
    assert!(
        !svg.contains("e-6") && !svg.contains("e-06"),
        "found e-notation in a rendered figure"
    );
    assert!(
        svg.contains("<tspan"),
        "expected raised superscript tspans for a 1e-6 axis"
    );
}

#[test]
fn short_panels_do_not_crowd_their_tick_labels() {
    // Found live: a stacked subplot a third of full height still asked for
    // ten ticks and rendered thirteen almost touching.
    let fig = figure_of(
        "x = linspace(0, 0.1, 200)\n\
         subplot(3, 1, 1)\nplot(x, sin(x))\n\
         subplot(3, 1, 2)\nplot(x, cos(x))\n\
         subplot(3, 1, 3)\nplot(x, x)",
    );
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    assert_eq!(geoms.len(), 3, "expected three panels");
    for g in &geoms {
        let labels = y_tick_labels(&svg, g);
        if labels.len() < 2 {
            continue;
        }
        let gap = (g.bottom - g.top) / (labels.len() - 1) as f64;
        assert!(
            gap >= fig.tick_size * 1.5,
            "y ticks {gap:.1}px apart with {}px text -- too crowded ({} labels)",
            fig.tick_size,
            labels.len()
        );
    }
}

#[test]
fn a_full_size_panel_still_gets_a_useful_number_of_ticks() {
    // The other side of the thinning: it must not strip a roomy axis bare.
    let fig = figure_of("x = linspace(0, 10, 100)\nplot(x, sin(x))");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    let labels = y_tick_labels(&svg, &geoms[0]);
    assert!(
        labels.len() >= 5,
        "a full-height panel should carry several ticks, got {labels:?}"
    );
}

// ----------------------------------------------------------------- axes

#[test]
fn a_non_negative_quantity_never_gets_a_negative_axis() {
    // Found live: a signal sampled from t=0 was given an axis starting at
    // -0.0205 s, and a 0-5kHz spectrum one starting at -247 Hz. Padding
    // must not invent values on the far side of zero.
    for src in [
        "t = 0 to 0.4 step 0.001\nplot(t, sin(t))",
        "f = linspace(0, 5000, 500)\nplot(f, f * 0.1)",
        "x = linspace(0, 10, 50)\nplot(x, x * x)",
    ] {
        let fig = figure_of(src);
        let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
        for g in &geoms {
            assert!(
                g.xmin >= -1e-12,
                "x axis starts at {} for data that never goes negative\n{src}",
                g.xmin
            );
        }
    }
}

#[test]
fn data_that_does_cross_zero_still_gets_room_on_both_sides() {
    let fig = figure_of("x = linspace(-5, 5, 50)\nplot(x, x)");
    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    assert!(geoms[0].xmin < -5.0, "expected padding below -5, got {}", geoms[0].xmin);
    assert!(geoms[0].xmax > 5.0);
}

#[test]
fn stacked_panels_share_their_left_and_right_edges() {
    // Subplots whose frames start at different x read as a misprint.
    let fig = figure_of(
        "x = linspace(0, 1, 50)\n\
         subplot(2, 1, 1)\nplot(x, x * 1000)\n\
         subplot(2, 1, 2)\nplot(x, x * 0.001)",
    );
    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    assert_eq!(geoms.len(), 2);
    assert!(
        (geoms[0].left - geoms[1].left).abs() < 0.01,
        "left edges differ: {} vs {}",
        geoms[0].left,
        geoms[1].left
    );
    assert!((geoms[0].right - geoms[1].right).abs() < 0.01);
}

// ------------------------------------------------------------ typography

#[test]
fn tick_numbers_are_never_bold() {
    // Found live: the bold rule compared against an absolute pixel size, so
    // scaling the type up for publication pushed tick numbers over the
    // threshold and set them bold, which no journal figure does.
    for src in [
        "plot([1,2,3])\ntitle(\"T\")\nxlabel(\"X\")",
        "theme(\"publication\")\nplot([1,2,3])\ntitle(\"T\")\nxlabel(\"X\")",
        "theme(\"publication\")\nfigure_size(1600, 1100)\nplot([1,2,3])\ntitle(\"T\")",
    ] {
        let fig = figure_of(src);
        let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
        for (size, weight, text) in texts(&svg) {
            if (size - fig.tick_size).abs() < 1e-9 {
                assert!(
                    weight.is_none_or(|w| w < 600),
                    "tick label {text:?} is bold at weight {weight:?}\n{src}"
                );
            }
        }
    }
}

#[test]
fn titles_are_bold_at_a_weight_that_has_a_drawn_face() {
    // 600 has no drawn face in a regular+bold family, so a browser fakes it
    // even with a real bold embedded. Ask for one that exists.
    let fig = figure_of("theme(\"publication\")\nplot([1,2,3])\ntitle(\"T\")");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    let title = texts(&svg)
        .into_iter()
        .find(|(_, _, t)| t == "T")
        .expect("no title in the figure");
    assert_eq!(title.1, Some(700), "title should be weight 700, got {:?}", title.1);
}

#[test]
fn publication_embeds_a_real_bold_face_not_a_synthesised_one() {
    // Declaring `font-weight: 100 900` on a single static file tells the
    // browser one outline covers every weight, and it then smears the
    // regular to fake bold.
    let fig = figure_of("theme(\"publication\")\nplot([1,2,3])\ntitle(\"T\")");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, true);
    assert_eq!(
        svg.matches("@font-face").count(),
        2,
        "expected separate regular and bold faces"
    );
    assert!(svg.contains("font-weight:400"), "no regular face declared");
    assert!(svg.contains("font-weight:700"), "no bold face declared");
    assert!(
        !svg.contains("font-weight:100 900"),
        "a static font must not claim the whole weight range"
    );
}

#[test]
fn a_screen_figure_stays_small_and_does_not_embed_fonts() {
    let fig = figure_of("plot([1,2,3])");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    assert!(!svg.contains("@font-face"), "screen figures should not embed fonts");
}

// ------------------------------------------------------------- integrity

#[test]
fn every_figure_carries_its_coordinate_mapping() {
    // The viewer's cursor readout and annotations depend on this; without
    // it they silently degrade to "no readout".
    let svg = render("plot([1,2,3])");
    assert!(svg.contains("<metadata id=\"qu-figure\""), "no geometry metadata");
    // The JSON inside `<metadata>` is XML-escaped, so its quotes arrive as
    // `&quot;` -- the reader in the viewer un-escapes before parsing.
    assert!(svg.contains("&quot;panels&quot;"), "metadata carries no panels");
}

#[test]
fn the_mapping_matches_where_the_markers_were_actually_drawn() {
    // The strongest available check: invert the published mapping at each
    // drawn marker and confirm it returns the value that marker stands for.
    let fig = figure_of("x = linspace(0, 10, 11)\ny = x * 2\nplot(x, y, marker=\"o\")");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);
    let g = &plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1[0];

    let mut checked = 0;
    for chunk in svg.split("<circle").skip(1) {
        let Some(head_end) = chunk.find('>') else { continue };
        let head = &chunk[..head_end];
        let num = |name: &str| -> Option<f64> {
            let pat = format!("{name}=\"");
            let i = head.find(&pat)? + pat.len();
            let j = head[i..].find('"')? + i;
            head[i..j].parse().ok()
        };
        let (Some(cx), Some(cy)) = (num("cx"), num("cy")) else { continue };
        let dx = g.xmin + (cx - g.left) / (g.right - g.left) * (g.xmax - g.xmin);
        let dy = g.ymin + (g.bottom - cy) / (g.bottom - g.top) * (g.ymax - g.ymin);
        // Every point on this plot satisfies y = 2x.
        assert!(
            (dy - 2.0 * dx).abs() < 1e-2,
            "marker at ({cx}, {cy}) inverts to ({dx}, {dy}), which is not on y = 2x"
        );
        checked += 1;
    }
    assert!(checked >= 10, "expected to check every marker, saw {checked}");
}

#[test]
fn a_figure_never_renders_a_non_finite_coordinate() {
    // NaN in the output is invalid SVG: renderers drop the whole element,
    // so a single bad sample silently erases a series.
    let svg = render("x = [1, 2, 3]\ny = [1, nan, 3]\nplot(x, y)\nplot([1,2],[inf,1])");
    for bad in ["NaN", "nan", "inf", "Infinity"] {
        assert!(!svg.contains(bad), "rendered SVG contains {bad:?}");
    }
}

#[test]
fn show_starts_a_new_figure_and_keeps_the_styling() {
    // `show` used to only bump a counter, so two plots merged into one
    // figure and the first was silently discarded.
    let mut it = Interp::new();
    it.run("theme(\"publication\")\nplot([1,2,3])\nshow plot\nplot([4,5,6])")
        .unwrap();
    assert_eq!(it.figure_history.len(), 1, "the first figure was not finalized");
    assert!(it.figure.publication, "styling was lost across `show`");
    assert!(it.figure_history[0].publication);
}

#[test]
fn geometry_helper_is_unused_but_kept_honest() {
    // Guard against the helper above silently becoming a no-op that other
    // tests start relying on.
    assert!(geometry("<svg/>").is_empty());
}

#[test]
fn y_tick_labels_sit_on_their_gridlines_not_above_them() {
    // Reported twice as "an offset problem". SVG and the PDF backend place
    // text by its BASELINE, so a label emitted at its gridline's own y has
    // its feet on the line and its body floating above -- about six pixels
    // at publication type. Measured before the fix: text y and gridline y
    // were identical to 0.00 for every tick, with no `dominant-baseline`
    // anywhere in the output.
    let fig = figure_of("theme(\"publication\")\nx = linspace(0, 10, 100)\nplot(x, sin(x))");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, false);

    // Horizontal gridlines: y1 == y2.
    let mut gridlines: Vec<f64> = Vec::new();
    for chunk in svg.split("<line").skip(1) {
        let Some(head_end) = chunk.find('>') else { continue };
        let head = &chunk[..head_end];
        let num = |name: &str| -> Option<f64> {
            let pat = format!("{name}=\"");
            let i = head.find(&pat)? + pat.len();
            let j = head[i..].find('"')? + i;
            head[i..j].parse().ok()
        };
        let (Some(y1), Some(y2)) = (num("y1"), num("y2")) else { continue };
        if (y1 - y2).abs() < 0.01 {
            gridlines.push(y1);
        }
    }
    assert!(!gridlines.is_empty(), "no horizontal gridlines to compare against");

    let geoms = plotting::build_draw_ops_with_geometry(&fig, fig.width, fig.height).1;
    let mut checked = 0;
    for chunk in svg.split("<text").skip(1) {
        let Some(head_end) = chunk.find('>') else { continue };
        let head = &chunk[..head_end];
        if !head.contains("text-anchor=\"end\"") {
            continue;
        }
        let num = |name: &str| -> Option<f64> {
            let pat = format!("{name}=\"");
            let i = head.find(&pat)? + pat.len();
            let j = head[i..].find('"')? + i;
            head[i..j].parse().ok()
        };
        let (Some(x), Some(y)) = (num("x"), num("y")) else { continue };
        if x >= geoms[0].left {
            continue;
        }
        let nearest = gridlines
            .iter()
            .copied()
            .min_by(|a, b| (a - y).abs().partial_cmp(&(b - y).abs()).unwrap())
            .unwrap();
        let shift = y - nearest;
        // The baseline must sit BELOW the line by roughly half a cap
        // height, so the glyphs straddle it.
        let want = fig.tick_size * 0.35;
        assert!(
            (shift - want).abs() < 1.0,
            "y label baseline is {shift:.2}px from its gridline, expected about {want:.2}px \
             (0 means the label floats above the line)"
        );
        checked += 1;
    }
    assert!(checked >= 3, "expected several y tick labels, checked {checked}");
}

/// Tick marks exist, point the way the theme says, and let the labels
/// through.
///
/// Every theme carried a `tick_color` and drew nothing with it: the axes
/// showed numbers floating beside a bare line, with no mark tying a label
/// to its position. This pins all three parts of the fix -- that marks are
/// drawn at all, that direction follows the theme, and that an outward
/// mark does not end up underneath its own number.
#[test]
fn axis_tick_marks_follow_the_theme() {
    // `default` (outward) against `ieee` (inward, matching the boxed frame
    // MATLAB gives the figures that journal actually publishes).
    let outward = render("theme(\"default\")
plot([0,1,2],[0,1,4])");
    let inward = render("theme(\"ieee\")
plot([0,1,2],[0,1,4])");
    let none = render("theme(\"minimal\")
plot([0,1,2],[0,1,4])");

    // A tick is a short line; nothing else on these figures is. Counting
    // short lines is what tells the three apart.
    let short_lines = |svg: &str| -> usize {
        svg.match_indices("<line").filter(|(i, _)| {
            let seg = &svg[*i..(*i + 220).min(svg.len())];
            let g = |k: &str| -> Option<f64> {
                let p = seg.find(k)? + k.len() + 2;
                seg[p..].split('"').next()?.parse().ok()
            };
            match (g("x1"), g("y1"), g("x2"), g("y2")) {
                (Some(a), Some(b), Some(c), Some(d)) => {
                    let len = (c - a).hypot(d - b);
                    len > 0.5 && len < 12.0
                }
                _ => false,
            }
        }).count()
    };

    assert!(short_lines(&outward) >= 4, "default theme should draw tick marks");
    assert!(short_lines(&inward) >= 4, "ieee theme should draw tick marks");
    assert_eq!(short_lines(&none), 0, "minimal theme blanks ticks, as theme_minimal() does");
}

// ------------------------------------------------- themes must take effect

/// `ThemeStyle` carried four fields that nothing ever read: `panel_fill`,
/// `tick_text_color`, `label_color` and `title_color`. Every theme rendered
/// `#444` labels and `#5a5a5a` ticks whatever it declared, which is why
/// `classic` and `nature` came out byte-identical despite differing in the
/// table, and why `theme("grey")` had no grey panel. A field that can be set
/// and silently does nothing is worse than a missing feature: it looks like
/// it works.
#[test]
fn a_theme_field_that_can_be_set_takes_effect() {
    let grey = render("theme(\"grey\")\nplot([1,2,3])\ntitle(\"T\")");
    assert!(
        grey.contains("#ebebeb"),
        "theme(\"grey\") declares panel_fill #ebebeb and must actually draw it"
    );

    // Print themes declare pure black and used to render the screen grey.
    for theme in ["nature", "ieee"] {
        let svg = render(&format!("theme(\"{theme}\")\nplot([1,2,3])\ntitle(\"T\")\nxlabel(\"X\")"));
        assert!(
            svg.contains("fill=\"#000000\""),
            "{theme} declares black ink and must not render the screen grey into a journal"
        );
    }

    // Two themes that differ in the table must differ on the page.
    let classic = render("theme(\"classic\")\nplot([1,2,3])\ntitle(\"T\")");
    let nature = render("theme(\"nature\")\nplot([1,2,3])\ntitle(\"T\")");
    assert_ne!(classic, nature, "classic and nature differ in THEMES but render identically");
}

#[test]
fn dark_is_a_real_theme_and_not_a_silent_fallback() {
    let dark = render("theme(\"dark\")\nplot([1,2,3])\ntitle(\"T\")");
    let default = render("theme(\"default\")\nplot([1,2,3])\ntitle(\"T\")");
    assert_ne!(dark, default, "theme(\"dark\") silently rendered the default theme");
    // A dark theme that leaves the canvas white is not a dark theme.
    assert!(dark.contains("#1a1a1d"), "dark theme did not darken the canvas");
    assert!(
        !dark.contains("fill=\"#ffffff\""),
        "dark theme still paints a white surface: {}",
        &dark[..dark.len().min(400)]
    );
}

#[test]
fn an_unknown_theme_name_is_reported_rather_than_silently_substituted() {
    assert!(plotting::theme_exists("dark"), "dark should be a known theme");
    assert!(plotting::theme_exists("NATURE"), "theme lookup is case-insensitive");
    assert!(!plotting::theme_exists("no_such_theme"));
    let mut it = Interp::new();
    it.run("theme(\"no_such_theme\")\nplot([1,2,3])").unwrap();
    let out = it.out.clone();
    assert!(
        out.contains("no theme named"),
        "a typo'd theme name must say so, not quietly hand back the default; got: {out:?}"
    );
}

/// An exponent renders as a raised `<tspan>` carrying the face's own digit,
/// which is correct on the page but flattens to "102" for 10². A log axis
/// then copies out as "100 101 102" -- three consecutive integers instead of
/// three decades.
#[test]
fn math_text_carries_an_unambiguous_label_for_readers_that_do_not_render() {
    let svg = render("x = logspace(0, 6, 50)\nsemilogx(x, x)\nxlabel(\"$10^{3}$ Hz\")");
    for decade in ['\u{2070}', '\u{00B9}', '\u{00B2}', '\u{00B3}'] {
        assert!(
            svg.contains(&format!("aria-label=\"10{decade}")),
            "no unambiguous label for the 10{decade} decade tick"
        );
    }
    // The visible rendering is untouched -- still a real raised tspan.
    assert!(svg.contains("dy=\"-0.350em\""), "the drawn exponent must stay a raised tspan");
}

/// A Tukey box is an outlined shape with a light interior. Drawn as one rect,
/// `opacity` applied to the fill AND the stroke together, so the edge came
/// out at 35% of the fill colour -- the same hue as what it surrounds, at the
/// same transparency, which is no edge at all. The whiskers around it are
/// full-strength lines, so the box read as a faint blob between crisp
/// whiskers, and a 35% edge is exactly what goes muddy in print.
#[test]
fn a_boxplot_has_a_real_edge_and_not_a_washed_out_one() {
    let svg = render("a = randn(60)\nb = randn(60) + 2\nboxplot(a, b)\nylabel(\"V\")");
    let boxes: Vec<&str> = svg.matches("<rect").collect();
    assert!(boxes.len() >= 4, "expected a fill and an edge rect per box, got {}", boxes.len());
    // The edge is opaque and carries no fill.
    assert!(
        svg.contains("fill=\"none\"") && svg.contains("opacity=\"1\""),
        "the box edge must be drawn at full opacity with no fill"
    );
    // Nothing is left drawing an edge at the fill's own transparency.
    assert!(
        !svg.contains("stroke=\"#5B7CFA\" opacity=\"0.35\""),
        "the box edge is still being drawn at the fill's opacity"
    );
}

/// `axis equal` used to equalise data RANGES, which is not what the name
/// promises and, in the case it exists for, did nothing whatever: a shape
/// example already sets matching `xlim`/`ylim`, so the spans arrived equal
/// and the code was a no-op. The panel is wider than it is tall, so equal
/// ranges over an unequal box still means unequal units per pixel -- the
/// documented `circle` example drew an ellipse 58% out of round, identical
/// with and without the flag.
#[test]
fn axis_equal_actually_makes_a_circle_round() {
    /// Width/height of the largest polygon in the SVG -- the circle.
    fn aspect(svg: &str) -> f64 {
        let pts: Vec<(f64, f64)> = svg
            .split("points=\"")
            .skip(1)
            .map(|c| {
                c[..c.find('"').unwrap()]
                    .split_whitespace()
                    .filter_map(|p| {
                        let (a, b) = p.split_once(',')?;
                        Some((a.parse().ok()?, b.parse().ok()?))
                    })
                    .collect::<Vec<(f64, f64)>>()
            })
            .max_by_key(|v| v.len())
            .expect("no polygon in the figure");
        let (xs, ys): (Vec<f64>, Vec<f64>) = pts.into_iter().unzip();
        let w = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
        let h = ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
        w / h
    }

    let src = "xlim(-2, 2)\nylim(-2, 2)\ncircle(0, 0, 1)";
    let round = aspect(&render(&format!("xlim(-2, 2)\nylim(-2, 2)\naxis equal\ncircle(0, 0, 1)")));
    assert!(
        (round - 1.0).abs() < 0.01,
        "`axis equal` must make a circle round, got width/height {round:.4}"
    );
    // And it must still be a real instruction rather than always-on: a
    // figure that did not ask keeps the panel's own aspect.
    let stretched = aspect(&render(src));
    assert!(
        (stretched - 1.0).abs() > 0.1,
        "without `axis equal` the circle should follow the panel, got {stretched:.4}"
    );
}

/// Two order-dependence traps in one test, both of the "accepted then not
/// honoured" family: an explicit `colormap(...)` used to be silently
/// replaced by `theme("publication")`'s own palette if it came first, and
/// `dashes = "--"` used to be a type error because the named spellings
/// lived on a different keyword.
#[test]
fn an_explicit_choice_outranks_a_theme_and_survives_call_order() {
    let colours = |src: &str| -> Vec<String> {
        let svg = render(src);
        let mut got: Vec<String> = svg
            .split("<polyline")
            .skip(1)
            .filter_map(|c| {
                let i = c.find("stroke=\"")? + 8;
                Some(c[i..i + 7].to_string())
            })
            .collect();
        got.sort();
        got.dedup();
        got
    };
    let body = "\nx = linspace(0, 10, 20)\nplot(x, sin(x))\nplot(x, cos(x))";
    let before = colours(&format!("colormap(\"viridis\")\ntheme(\"publication\"){body}"));
    let after = colours(&format!("theme(\"publication\")\ncolormap(\"viridis\"){body}"));
    assert_eq!(
        before, after,
        "colormap must mean the same thing whichever side of theme() it sits on"
    );
    assert!(
        before.iter().any(|c| c.eq_ignore_ascii_case("#440154")),
        "expected viridis, got {before:?} -- the theme overrode an explicit colormap"
    );

    // A theme still picks the palette when nobody asked for one.
    let themed = colours(&format!("theme(\"publication\"){body}"));
    assert_ne!(themed, before, "theme should still choose when no colormap was called");

    // And the named dash spellings work on `dashes` as well as `style`.
    let named = render("plot([0,1],[0,1], dashes=\"--\")");
    let styled = render("plot([0,1],[0,1], style=\"--\")");
    let arr = |s: &str| -> String {
        let i = s.find("stroke-dasharray=\"").expect("no dash array") + 18;
        s[i..].split('"').next().unwrap().to_string()
    };
    assert_eq!(arr(&named), arr(&styled), "dashes=\"--\" must mean what style=\"--\" means");
}

/// The type scale, pinned as ratios and floors rather than as the four
/// numbers themselves.
///
/// Ahmed set these by eye on 2026-09-10 -- screen "still I'm seeing problem
/// in the font size, I still feel it's small", publication "too big for
/// publication" -- so the numbers are his and may move again. What must not
/// drift is the SHAPE: a visible step from tick to label to title, a floor
/// no label falls below, and publication staying close to screen rather
/// than running away from it, which is what made the two complaints one
/// problem.
#[test]
fn the_type_scale_keeps_its_hierarchy_and_its_floor() {
    let sizes = |src: &str| -> Vec<f64> {
        let svg = render(src);
        let mut v: Vec<f64> = svg
            .split("font-size=\"")
            .skip(1)
            .filter_map(|c| c[..c.find('"')?].parse::<f64>().ok())
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        v
    };
    let body = "\nplot([1,2,3])\ntitle(\"T\")\nxlabel(\"X\")\nylabel(\"Y\")";
    for (what, src) in [("screen", body.to_string()), ("publication", format!("theme(\"publication\"){body}"))] {
        let v = sizes(&src);
        assert!(v.len() >= 3, "{what}: expected tick/label/title to differ, got {v:?}");
        let (tick, title) = (v[0], v[v.len() - 1]);

        // A floor. Below roughly this, a tick label stops being readable
        // once the figure is reduced; Nature's own minimum is 5 pt, which
        // is 17.6 canvas units.
        assert!(tick >= 15.0, "{what}: tick type {tick} is below the readable floor");

        // And a hierarchy: the title must lead the ticks clearly, but not
        // so far that it reads as a headline on a poster.
        let ratio = title / tick;
        assert!(
            (1.2..=1.6).contains(&ratio),
            "{what}: title/tick ratio {ratio:.2} is outside the 1.2-1.6 band ({v:?})"
        );
    }

    // The two targets must stay in the same neighbourhood. They drifted to
    // 1.71x, which is exactly why one looked small while the other looked
    // too big -- raising the ticks alone had moved one side and left the
    // gap.
    let screen_tick = sizes(body)[0];
    let print_tick = sizes(&format!("theme(\"publication\"){body}"))[0];
    let gap = print_tick / screen_tick;
    assert!(
        gap <= 1.45,
        "publication runs {gap:.2}x screen; they converged to 1.31x deliberately"
    );
}
