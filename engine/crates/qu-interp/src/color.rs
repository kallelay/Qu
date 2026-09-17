//! Resolving a colour the user wrote into the `#rrggbb` the renderers need.
//!
//! Every backend below the plotting layer assumes six hex digits:
//! `hex_to_rgb` reads two characters at a time and `unwrap_or(0)`s what it
//! cannot parse. That made a named colour a silent, backend-dependent wrong
//! answer -- `color = "royalblue"` reached the SVG verbatim, where a browser
//! resolves it and the figure looks right, and reached the PDF as three
//! failed hex parses, where the same figure came out BLACK. A figure that is
//! correct on screen and wrong in the paper is the worst shape a defect can
//! take, so resolution happens once, here, at the boundary.
//!
//! What is accepted: `#rgb`, `#rrggbb`, `#rrggbbaa`, the CSS colour names,
//! and the `rgb(...)`/`rgba(...)` forms. Anything else is an error naming the
//! nearest known colour rather than a black line nobody ordered.

/// The CSS/SVG named colours, sorted so a lookup is a binary search.
///
/// These are the names every browser, SVG viewer and design tool already
/// answers to, so a colour copied out of one of those works here without
/// translation -- which is the entire reason to support names at all.
static NAMED: &[(&str, &str)] = &[
    ("aliceblue", "#f0f8ff"), ("antiquewhite", "#faebd7"), ("aqua", "#00ffff"),
    ("aquamarine", "#7fffd4"), ("azure", "#f0ffff"), ("beige", "#f5f5dc"),
    ("bisque", "#ffe4c4"), ("black", "#000000"), ("blanchedalmond", "#ffebcd"),
    ("blue", "#0000ff"), ("blueviolet", "#8a2be2"), ("brown", "#a52a2a"),
    ("burlywood", "#deb887"), ("cadetblue", "#5f9ea0"), ("chartreuse", "#7fff00"),
    ("chocolate", "#d2691e"), ("coral", "#ff7f50"), ("cornflowerblue", "#6495ed"),
    ("cornsilk", "#fff8dc"), ("crimson", "#dc143c"), ("cyan", "#00ffff"),
    ("darkblue", "#00008b"), ("darkcyan", "#008b8b"), ("darkgoldenrod", "#b8860b"),
    ("darkgray", "#a9a9a9"), ("darkgreen", "#006400"), ("darkgrey", "#a9a9a9"),
    ("darkkhaki", "#bdb76b"), ("darkmagenta", "#8b008b"), ("darkolivegreen", "#556b2f"),
    ("darkorange", "#ff8c00"), ("darkorchid", "#9932cc"), ("darkred", "#8b0000"),
    ("darksalmon", "#e9967a"), ("darkseagreen", "#8fbc8f"), ("darkslateblue", "#483d8b"),
    ("darkslategray", "#2f4f4f"), ("darkslategrey", "#2f4f4f"), ("darkturquoise", "#00ced1"),
    ("darkviolet", "#9400d3"), ("deeppink", "#ff1493"), ("deepskyblue", "#00bfff"),
    ("dimgray", "#696969"), ("dimgrey", "#696969"), ("dodgerblue", "#1e90ff"),
    ("firebrick", "#b22222"), ("floralwhite", "#fffaf0"), ("forestgreen", "#228b22"),
    ("fuchsia", "#ff00ff"), ("gainsboro", "#dcdcdc"), ("ghostwhite", "#f8f8ff"),
    ("gold", "#ffd700"), ("goldenrod", "#daa520"), ("gray", "#808080"),
    ("green", "#008000"), ("greenyellow", "#adff2f"), ("grey", "#808080"),
    ("honeydew", "#f0fff0"), ("hotpink", "#ff69b4"), ("indianred", "#cd5c5c"),
    ("indigo", "#4b0082"), ("ivory", "#fffff0"), ("khaki", "#f0e68c"),
    ("lavender", "#e6e6fa"), ("lavenderblush", "#fff0f5"), ("lawngreen", "#7cfc00"),
    ("lemonchiffon", "#fffacd"), ("lightblue", "#add8e6"), ("lightcoral", "#f08080"),
    ("lightcyan", "#e0ffff"), ("lightgoldenrodyellow", "#fafad2"), ("lightgray", "#d3d3d3"),
    ("lightgreen", "#90ee90"), ("lightgrey", "#d3d3d3"), ("lightpink", "#ffb6c1"),
    ("lightsalmon", "#ffa07a"), ("lightseagreen", "#20b2aa"), ("lightskyblue", "#87cefa"),
    ("lightslategray", "#778899"), ("lightslategrey", "#778899"), ("lightsteelblue", "#b0c4de"),
    ("lightyellow", "#ffffe0"), ("lime", "#00ff00"), ("limegreen", "#32cd32"),
    ("linen", "#faf0e6"), ("magenta", "#ff00ff"), ("maroon", "#800000"),
    ("mediumaquamarine", "#66cdaa"), ("mediumblue", "#0000cd"), ("mediumorchid", "#ba55d3"),
    ("mediumpurple", "#9370db"), ("mediumseagreen", "#3cb371"), ("mediumslateblue", "#7b68ee"),
    ("mediumspringgreen", "#00fa9a"), ("mediumturquoise", "#48d1cc"), ("mediumvioletred", "#c71585"),
    ("midnightblue", "#191970"), ("mintcream", "#f5fffa"), ("mistyrose", "#ffe4e1"),
    ("moccasin", "#ffe4b5"), ("navajowhite", "#ffdead"), ("navy", "#000080"),
    ("oldlace", "#fdf5e6"), ("olive", "#808000"), ("olivedrab", "#6b8e23"),
    ("orange", "#ffa500"), ("orangered", "#ff4500"), ("orchid", "#da70d6"),
    ("palegoldenrod", "#eee8aa"), ("palegreen", "#98fb98"), ("paleturquoise", "#afeeee"),
    ("palevioletred", "#db7093"), ("papayawhip", "#ffefd5"), ("peachpuff", "#ffdab9"),
    ("peru", "#cd853f"), ("pink", "#ffc0cb"), ("plum", "#dda0dd"),
    ("powderblue", "#b0e0e6"), ("purple", "#800080"), ("rebeccapurple", "#663399"),
    ("red", "#ff0000"), ("rosybrown", "#bc8f8f"), ("royalblue", "#4169e1"),
    ("saddlebrown", "#8b4513"), ("salmon", "#fa8072"), ("sandybrown", "#f4a460"),
    ("seagreen", "#2e8b57"), ("seashell", "#fff5ee"), ("sienna", "#a0522d"),
    ("silver", "#c0c0c0"), ("skyblue", "#87ceeb"), ("slateblue", "#6a5acd"),
    ("slategray", "#708090"), ("slategrey", "#708090"), ("snow", "#fffafa"),
    ("springgreen", "#00ff7f"), ("steelblue", "#4682b4"), ("tan", "#d2b48c"),
    ("teal", "#008080"), ("thistle", "#d8bfd8"), ("tomato", "#ff6347"),
    ("turquoise", "#40e0d0"), ("violet", "#ee82ee"), ("wheat", "#f5deb3"),
    ("white", "#ffffff"), ("whitesmoke", "#f5f5f5"), ("yellow", "#ffff00"),
    ("yellowgreen", "#9acd32"),
];

/// Resolve anything the user may have written to `#rrggbb` or `#rrggbbaa`.
///
/// `Err` carries a ready-to-print reason, including the nearest known name
/// when the input looks like a misspelling rather than a different idea.
pub fn resolve(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("a colour cannot be the empty string".into());
    }

    if let Some(hex) = s.strip_prefix('#') {
        let ok = hex.chars().all(|c| c.is_ascii_hexdigit());
        return match (hex.len(), ok) {
            // `#abc` is the CSS shorthand for `#aabbcc`, and people type it.
            (3, true) => {
                let mut out = String::from("#");
                for c in hex.chars() {
                    out.push(c.to_ascii_lowercase());
                    out.push(c.to_ascii_lowercase());
                }
                Ok(out)
            }
            (6, true) | (8, true) => Ok(format!("#{}", hex.to_lowercase())),
            _ => Err(format!(
                "`{input}` is not a colour -- after `#` expected 3, 6 or 8 hex digits, found {}",
                hex.len()
            )),
        };
    }

    // `rgb(255, 0, 0)` / `rgba(255, 0, 0, 0.5)` as written text, for a colour
    // pasted out of a stylesheet. The builtins of the same name build the hex
    // directly and never come through here.
    let lower = s.to_lowercase();
    if let Some(rest) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")) {
        let body = rest
            .strip_suffix(')')
            .ok_or_else(|| format!("`{input}` is missing its closing parenthesis"))?;
        let parts: Vec<&str> = body
            .split([',', '/', ' '])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() != 3 && parts.len() != 4 {
            return Err(format!(
                "`{input}`: rgb takes 3 numbers and rgba 4, found {}",
                parts.len()
            ));
        }
        let mut chan = [0u8; 4];
        for (i, p) in parts.iter().enumerate() {
            let v: f64 = p
                .trim()
                .parse()
                .map_err(|_| format!("`{input}`: `{p}` is not a number"))?;
            // The fourth channel is an opacity in 0..1, the CSS convention,
            // unless it is plainly a 0..255 byte like the other three.
            let scaled = if i == 3 && v <= 1.0 { v * 255.0 } else { v };
            if !(0.0..=255.0).contains(&scaled) {
                return Err(format!("`{input}`: channel {} is outside 0..255", i + 1));
            }
            chan[i] = scaled.round() as u8;
        }
        return Ok(if parts.len() == 4 {
            from_rgba(chan[0], chan[1], chan[2], chan[3])
        } else {
            from_rgb(chan[0], chan[1], chan[2])
        });
    }

    let key = lower.replace([' ', '-', '_'], "");
    if let Ok(i) = NAMED.binary_search_by(|(n, _)| (*n).cmp(&key.as_str())) {
        return Ok(NAMED[i].1.to_string());
    }

    Err(format!("`{input}` is not a colour{}", nearest(&key)))
}

/// `#rrggbb` from three channels.
pub fn from_rgb(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#rrggbbaa` from four.
pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

/// " -- did you mean `steelblue`?", when the input is close enough to be a
/// typo rather than a different idea.
fn nearest(key: &str) -> String {
    let mut best: Option<(usize, &str)> = None;
    for (name, _) in NAMED {
        let d = edit_distance(key, name);
        if d <= 3 && best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, name));
        }
    }
    match best {
        Some((_, name)) => format!(" -- did you mean `{name}`?"),
        None => " -- expected a CSS colour name, `#rrggbb`, or rgb(r, g, b)".into(),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 3 {
        return usize::MAX;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_so_the_binary_search_is_valid() {
        for w in NAMED.windows(2) {
            assert!(w[0].0 < w[1].0, "{} then {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn a_name_resolves_to_the_hex_every_browser_uses() {
        assert_eq!(resolve("royalblue").unwrap(), "#4169e1");
        assert_eq!(resolve("RoyalBlue").unwrap(), "#4169e1");
        assert_eq!(resolve("cornflower blue").unwrap(), "#6495ed");
    }

    #[test]
    fn hex_passes_through_and_the_three_digit_shorthand_expands() {
        assert_eq!(resolve("#4169E1").unwrap(), "#4169e1");
        assert_eq!(resolve("#abc").unwrap(), "#aabbcc");
        assert_eq!(resolve("#4169e180").unwrap(), "#4169e180");
    }

    #[test]
    fn the_rgb_forms_parse_and_a_fractional_alpha_is_an_opacity() {
        assert_eq!(resolve("rgb(65, 105, 225)").unwrap(), "#4169e1");
        assert_eq!(resolve("rgba(255, 0, 0, 0.5)").unwrap(), "#ff000080");
        // A fourth channel plainly given as a byte is taken as one.
        assert_eq!(resolve("rgba(255, 0, 0, 255)").unwrap(), "#ff0000ff");
    }

    /// The whole point: an unknown colour must not reach a renderer, where
    /// it draws correctly in SVG and black in PDF.
    #[test]
    fn an_unknown_colour_is_an_error_that_names_the_nearest_one() {
        let err = resolve("royalbleu").unwrap_err();
        assert!(err.contains("royalblue"), "got {err}");
        let err = resolve("notacolour").unwrap_err();
        assert!(err.contains("not a colour"), "got {err}");
        assert!(resolve("#12345").is_err());
    }
}

// ── colour spaces ──────────────────────────────────────────────────────
//
// RGB is what a screen takes and a poor space to think in: to make a
// colour "the same but lighter" you must change all three channels by
// unequal amounts, and to walk a hue you must know the hexagon by heart.
// Each space below separates one perceptual question from the others, and
// which you want depends on the question:
//
//   HSV  — hue, saturation, value. The one for GENERATING a series of
//          related colours: fix S and V, walk H, and every colour in the
//          set carries the same weight. This is how a categorical palette
//          gets built.
//   HSL  — like HSV but symmetric about 50% lightness, so "the same
//          colour, lighter" is one number and white and black sit at the
//          two ends rather than both at one.
//   Lab  — CIE L*a*b*, the only one here that is perceptually uniform: a
//          given distance is roughly the same perceived difference
//          anywhere in the space. That is what makes it right for
//          sequential colour maps, and for asking whether two colours can
//          be told apart at all.
//   CMYK — the printer's space. Here because a figure that goes to a
//          journal is printed, and a colour outside the CMYK gamut prints
//          as something else without warning.
//
// Every conversion takes and returns plain numbers, so they compose with
// the rest of the language rather than needing a colour type to exist.

/// RGB (0-255 each) to HSV: hue in degrees 0-360, saturation and value
/// each 0-1.
pub fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    // A grey has no hue at all. Returning 0 rather than NaN keeps it usable
    // in the arithmetic that walks the hue of a series.
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// HSV to RGB (0-255 each). Hue wraps, so 400 degrees is 40.
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0);
    let c = v.clamp(0.0, 1.0) * s.clamp(0.0, 1.0);
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v.clamp(0.0, 1.0) - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

/// RGB to HSL: hue in degrees, saturation and lightness each 0-1.
pub fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (h, _, _) = rgb_to_hsv(r, g, b);
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    (h, s.clamp(0.0, 1.0), l)
}

/// HSL to RGB (0-255 each).
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0);
    let l = l.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s.clamp(0.0, 1.0);
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

/// The sRGB transfer function in reverse: display value to linear light.
///
/// This step is what separates a correct conversion from the common wrong
/// one. sRGB is gamma-encoded, and averaging or interpolating the encoded
/// values gives a colour that is visibly too dark -- the classic mistake in
/// every hand-rolled gradient.
fn srgb_to_linear(c: f64) -> f64 {
    let c = c / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    let v = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).clamp(0.0, 255.0)
}

/// D65, the white point sRGB is defined against.
const WHITE: (f64, f64, f64) = (0.95047, 1.0, 1.08883);

/// RGB to CIE L*a*b*: L in 0-100, a and b roughly -128 to 127.
pub fn rgb_to_lab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (rl, gl, bl) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    // sRGB to CIE XYZ, the standard matrix.
    let x = (0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl) / WHITE.0;
    let y = (0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl) / WHITE.1;
    let z = (0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl) / WHITE.2;
    let f = |t: f64| -> f64 {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// CIE L*a*b* back to RGB (0-255 each), clamped into gamut.
pub fn lab_to_rgb(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inv = |t: f64| -> f64 {
        if t.powi(3) > 0.008856 {
            t.powi(3)
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let (x, y, z) = (inv(fx) * WHITE.0, inv(fy) * WHITE.1, inv(fz) * WHITE.2);
    let rl = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let gl = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    (linear_to_srgb(rl), linear_to_srgb(gl), linear_to_srgb(bl))
}

/// RGB to CMYK, each 0-1.
///
/// The naive conversion, which is what an uncalibrated workflow uses and
/// what a journal's "convert to CMYK" step does unless you supply a
/// profile. Not colorimetrically exact -- that needs an ICC profile for the
/// specific press -- but it answers the question people actually have,
/// which is roughly how much ink this colour costs.
pub fn rgb_to_cmyk(r: f64, g: f64, b: f64) -> (f64, f64, f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let k = 1.0 - r.max(g).max(b);
    if (1.0 - k).abs() < 1e-12 {
        return (0.0, 0.0, 0.0, 1.0);
    }
    (
        (1.0 - r - k) / (1.0 - k),
        (1.0 - g - k) / (1.0 - k),
        (1.0 - b - k) / (1.0 - k),
        k,
    )
}

/// CMYK back to RGB (0-255 each).
pub fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> (f64, f64, f64) {
    let k = k.clamp(0.0, 1.0);
    (
        255.0 * (1.0 - c.clamp(0.0, 1.0)) * (1.0 - k),
        255.0 * (1.0 - m.clamp(0.0, 1.0)) * (1.0 - k),
        255.0 * (1.0 - y.clamp(0.0, 1.0)) * (1.0 - k),
    )
}

/// The three channels of a resolved `#rrggbb`, as 0-255 numbers.
pub fn channels(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| -> f64 {
        u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64
    };
    (byte(0), byte(2), byte(4))
}

/// Perceived difference between two colours: Euclidean distance in Lab,
/// the CIE76 metric.
///
/// Roughly: under 1 is invisible, 2-3 is where most people start to notice,
/// over 10 is plainly a different colour. This is the number to check when
/// asking whether two series in a figure can be told apart -- the RGB
/// values differing is not the same question, and is often reassuring when
/// it should not be.
pub fn delta_e(a: &str, b: &str) -> Result<f64, String> {
    let pa = channels(&resolve(a)?);
    let pb = channels(&resolve(b)?);
    let (l1, a1, b1) = rgb_to_lab(pa.0, pa.1, pa.2);
    let (l2, a2, b2) = rgb_to_lab(pb.0, pb.1, pb.2);
    Ok(((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt())
}

#[cfg(test)]
mod space_tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn hsv_round_trips_through_rgb() {
        for &(r, g, b) in &[(255.0, 0.0, 0.0), (65.0, 105.0, 225.0), (18.0, 200.0, 77.0)] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!(
                close(r, r2, 0.6) && close(g, g2, 0.6) && close(b, b2, 0.6),
                "({r},{g},{b}) -> ({h},{s},{v}) -> ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn hsl_round_trips_and_agrees_with_hsv_on_hue() {
        let (h1, _, _) = rgb_to_hsv(65.0, 105.0, 225.0);
        let (h2, s, l) = rgb_to_hsl(65.0, 105.0, 225.0);
        assert!(close(h1, h2, 1e-9), "hue should not depend on the space: {h1} vs {h2}");
        let (r, g, b) = hsl_to_rgb(h2, s, l);
        assert!(close(r, 65.0, 0.6) && close(g, 105.0, 0.6) && close(b, 225.0, 0.6));
    }

    #[test]
    fn the_known_hues_land_where_the_colour_wheel_says() {
        assert!(close(rgb_to_hsv(255.0, 0.0, 0.0).0, 0.0, 1e-9));
        assert!(close(rgb_to_hsv(255.0, 255.0, 0.0).0, 60.0, 1e-9));
        assert!(close(rgb_to_hsv(0.0, 255.0, 0.0).0, 120.0, 1e-9));
        assert!(close(rgb_to_hsv(0.0, 0.0, 255.0).0, 240.0, 1e-9));
        // A grey has no hue; 0 rather than NaN keeps it usable.
        assert_eq!(rgb_to_hsv(128.0, 128.0, 128.0).0, 0.0);
    }

    #[test]
    fn lab_round_trips_and_puts_white_and_black_where_it_should() {
        let (l, a, b) = rgb_to_lab(255.0, 255.0, 255.0);
        assert!(
            close(l, 100.0, 0.01) && close(a, 0.0, 0.01) && close(b, 0.0, 0.01),
            "white is L=100, a=b=0: got ({l}, {a}, {b})"
        );
        assert!(close(rgb_to_lab(0.0, 0.0, 0.0).0, 0.0, 0.01));

        let (l, a, b) = rgb_to_lab(65.0, 105.0, 225.0);
        let (r, g, bb) = lab_to_rgb(l, a, b);
        assert!(
            close(r, 65.0, 0.6) && close(g, 105.0, 0.6) && close(bb, 225.0, 0.6),
            "round trip: ({r}, {g}, {bb})"
        );
    }

    #[test]
    fn cmyk_round_trips_and_black_is_all_key() {
        assert_eq!(rgb_to_cmyk(0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 1.0));
        let (c, m, y, k) = rgb_to_cmyk(255.0, 0.0, 0.0);
        assert!(close(c, 0.0, 1e-9) && close(m, 1.0, 1e-9));
        assert!(close(y, 1.0, 1e-9) && close(k, 0.0, 1e-9));
        let (r, g, b) = cmyk_to_rgb(c, m, y, k);
        assert!(close(r, 255.0, 0.5) && close(g, 0.0, 0.5) && close(b, 0.0, 0.5));
    }

    /// The number that answers "can a reader tell these two series apart".
    #[test]
    fn delta_e_orders_colours_the_way_an_eye_does() {
        let same = delta_e("#4169e1", "#4169e1").unwrap();
        let near = delta_e("#4169e1", "#4269e2").unwrap();
        let far = delta_e("#4169e1", "#dc143c").unwrap();
        assert!(same < 1e-9);
        assert!(near < 1.0, "a one-step change should be invisible: {near}");
        assert!(far > 40.0, "blue against crimson should be unmistakable: {far}");
    }
}
