//! Circuit specification strings: `"R-p(R,C)"` <-> [`Circuit`].
//!
//! The grammar is `papers/ecm-pf`'s, not invented here:
//!
//! * `A-B-C` is series,
//! * `p(A,B,C)` is parallel,
//! * a leaf is an element letter plus an optional instance tag, so `Rs`,
//!   `Rct` and `R1` are all resistors and the tag exists only to name them,
//! * parentheses may wrap any sub-expression.
//!
//! Parameters are a single flat vector consumed in **left-to-right leaf
//! order**, which is the documented source of truth in
//! [`qu_core::circuit`]. That ordering is why a parameter vector is
//! interchangeable with the `papers/ecm-pf` Qu and Python implementations,
//! and therefore why their 40-topology cross-language check can be pointed
//! at this code as a conformance suite rather than re-derived.
//!
//! The writer is the exact inverse of the reader for topology, so a circuit
//! built by either route round-trips. It deliberately emits **no instance
//! tags**: a tag carries no impedance and inventing one would make the
//! output differ from the input for no reason a reader could act on.

use qu_core::circuit::{Circuit, Element};

/// Element name -> parameter count, the two facts the parser needs.
///
/// `Ws`/`Wo` are the finite-length Warburgs, matching §48's prose. `T`/`O`
/// were the original single letters (from `papers/ecm-pf`) and remain
/// accepted as DEPRECATED ALIASES so that the engine and that lane's library
/// never speak different grammars at the same moment -- their conformance
/// suite passes spec strings through verbatim, so a flag day would break the
/// only independent check either implementation has.
fn nparam_of(name: &str) -> Option<usize> {
    Some(match name {
        "R" | "C" | "L" | "W" => 1,
        "Q" | "G" => 2,
        "Ws" | "Wo" => 2,
        // deprecated aliases for Ws / Wo
        "T" | "O" => 2,
        "P" => 3,
        "H" => 4,
        _ => return None,
    })
}

fn build_leaf(name: &str, p: &[f64]) -> Element {
    match name {
        "R" => Element::Resistor(p[0]),
        "C" => Element::Capacitor(p[0]),
        "L" => Element::Inductor(p[0]),
        "W" => Element::Warburg(p[0]),
        "Q" => Element::Cpe { q: p[0], n: p[1] },
        "Wo" | "O" => Element::FiniteOpen { rw: p[0], tau: p[1] },
        "Ws" | "T" => Element::FiniteShort { rw: p[0], tau: p[1] },
        "G" => Element::Gerischer { zg: p[0], k: p[1] },
        "P" => Element::Porous { rp: p[0], q: p[1], n: p[2] },
        _ => Element::HavriliakNegami { rh: p[0], tau: p[1], a: p[2], g: p[3] },
    }
}

/// The CANONICAL name, which is what the writer emits. Deprecated aliases are
/// read but never written, so a round-trip normalises `T1` to `Ws`.
fn name_of(e: &Element) -> &'static str {
    match e {
        Element::Resistor(_) => "R",
        Element::Capacitor(_) => "C",
        Element::Inductor(_) => "L",
        Element::Warburg(_) => "W",
        Element::Cpe { .. } => "Q",
        Element::FiniteOpen { .. } => "Wo",
        Element::FiniteShort { .. } => "Ws",
        Element::Gerischer { .. } => "G",
        Element::Porous { .. } => "P",
        Element::HavriliakNegami { .. } => "H",
    }
}

/// Split a leaf into its element name and its instance tag, **longest match
/// first**: two characters, then one.
///
/// This is the rule that lets `Ws` be an element at all. `Ws` was already a
/// valid spelling before -- element `W` with instance tag `s` -- so the
/// boundary cannot be "always one character" any more.
///
/// The collision surface is exactly leaves where a `W` carries a tag
/// beginning `s` or `o`, since those now read as `Ws`/`Wo` instead.
/// **Measured against `papers/ecm-pf`'s whole corpus before adopting the
/// rule: zero occurrences**, in the Qu library, the Qu circuits and the
/// Python library alike -- tags there are digits (`R0`, `Q1`, `W1`) or short
/// letters (`RL`). `W1` still reads as `W` + `1`, because `W1` is not an
/// element name.
fn split_element(s: &str) -> Option<(&str, &str)> {
    let two = s.char_indices().nth(2).map(|(i, _)| i).unwrap_or(s.len());
    let head2 = &s[..two];
    if head2.chars().count() == 2 && nparam_of(head2).is_some() {
        return Some((head2, &s[two..]));
    }
    let one = s.char_indices().nth(1).map(|(i, _)| i).unwrap_or(s.len());
    let head1 = &s[..one];
    if nparam_of(head1).is_some() {
        return Some((head1, &s[one..]));
    }
    None
}

/// This element's parameters, in the flat vector's order.
pub fn params_of(e: &Element) -> Vec<f64> {
    match *e {
        Element::Resistor(v) | Element::Capacitor(v) | Element::Inductor(v) | Element::Warburg(v) => {
            vec![v]
        }
        Element::Cpe { q, n } => vec![q, n],
        Element::FiniteOpen { rw, tau } | Element::FiniteShort { rw, tau } => vec![rw, tau],
        Element::Gerischer { zg, k } => vec![zg, k],
        Element::Porous { rp, q, n } => vec![rp, q, n],
        Element::HavriliakNegami { rh, tau, a, g } => vec![rh, tau, a, g],
    }
}

/// Split `s` on `sep` at paren depth zero.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            c if c == sep && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur.trim().to_string());
    out.into_iter().filter(|p| !p.is_empty()).collect()
}

/// True when the outermost parens wrap the WHOLE string, so stripping them
/// changes nothing. `(A)-(B)` starts with `(` and must not be stripped.
fn wraps_whole(s: &str) -> bool {
    if !s.starts_with('(') || !s.ends_with(')') {
        return false;
    }
    let mut depth = 0i32;
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return i == s.chars().count() - 1;
                }
            }
            _ => {}
        }
    }
    false
}

/// Parse `spec`, consuming parameters from `p` starting at `*cursor`.
fn parse_into(spec: &str, p: &[f64], cursor: &mut usize) -> Result<Circuit, String> {
    let s = spec.trim();
    if s.is_empty() {
        return Err("empty sub-expression in the circuit spec".into());
    }
    if wraps_whole(s) {
        let inner: String = s[1..s.len() - 1].to_string();
        return parse_into(&inner, p, cursor);
    }
    let parts = split_top(s, '-');
    if parts.len() > 1 {
        let mut kids = Vec::with_capacity(parts.len());
        for part in &parts {
            kids.push(parse_into(part, p, cursor)?);
        }
        return Ok(Circuit::Series(kids));
    }
    if (s.starts_with("p(") || s.starts_with("P(")) && s.ends_with(')') && wraps_whole(&s[1..]) {
        let inner = &s[2..s.len() - 1];
        let branches = split_top(inner, ',');
        if branches.len() < 2 {
            return Err(format!(
                "`p(...)` needs at least two branches to be a parallel block, found {} in `{s}`",
                branches.len()
            ));
        }
        let mut kids = Vec::with_capacity(branches.len());
        for b in &branches {
            kids.push(parse_into(b, p, cursor)?);
        }
        return Ok(Circuit::Parallel(kids));
    }
    // A leaf: an element name (longest match, two characters then one) and an
    // optional instance tag that carries no impedance and is ignored.
    let (kind, tag) = split_element(s).ok_or_else(|| {
        format!(
            "unknown element in `{s}` -- the elements are R C L Q W Ws Wo G P H \
             (see the circuit chapter; `T`/`O` are accepted as deprecated \
             spellings of `Ws`/`Wo`)"
        )
    })?;
    let n = nparam_of(kind).expect("split_element only returns known names");
    if !tag.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!(
            "`{s}` is not a valid element: an element is a name plus an \
             optional instance tag, like `R`, `Rct`, `Q1` or `Ws1`"
        ));
    }
    if *cursor + n > p.len() {
        return Err(format!(
            "not enough parameters: `{s}` needs {n} starting at index {}, but only {} were given",
            *cursor,
            p.len()
        ));
    }
    let slice = &p[*cursor..*cursor + n];
    *cursor += n;
    Ok(Circuit::Leaf(build_leaf(kind, slice)))
}

/// Parse a spec string and its flat parameter vector into a [`Circuit`].
///
/// Extra parameters are an ERROR rather than being ignored: a parameter
/// vector one element too long almost always means the caller and the spec
/// disagree about the topology, and silently dropping the tail would fit a
/// different circuit than the one they wrote.
pub fn parse(spec: &str, params: &[f64]) -> Result<Circuit, String> {
    let mut cursor = 0usize;
    let c = parse_into(spec, params, &mut cursor)?;
    if cursor != params.len() {
        return Err(format!(
            "`{spec}` takes {cursor} parameters but {} were given -- the spec and the \
             parameter vector disagree about the circuit",
            params.len()
        ));
    }
    Ok(c)
}

/// Parse a spec string into a circuit whose parameters are all `1.0` -- the
/// TOPOLOGY, without a claim about its values.
///
/// That is what a fitter needs from a spec string: `circuit_fit` is handed
/// `"R-p(R,C)"` and has to know how many parameters that is and where each
/// one sits, before it has any idea what they are worth.
///
/// Implemented by running the real parser against a filler vector rather
/// than by counting parameters with a second walk of the grammar. A separate
/// counting pass would be a second implementation of the same grammar, free
/// to drift from this one, and the drift would show up as a fit of a
/// different circuit than the string names.
pub fn parse_template(spec: &str) -> Result<Circuit, String> {
    const CEILING: usize = 128;
    let filler = vec![1.0f64; CEILING + 8];
    let mut cursor = 0usize;
    let c = parse_into(spec, &filler, &mut cursor)?;
    if cursor > CEILING {
        return Err(format!(
            "`{spec}` has more than {CEILING} parameters, which is past anything this is for"
        ));
    }
    Ok(c)
}

/// Render a circuit's topology back to a spec string.
pub fn to_spec(c: &Circuit) -> String {
    match c {
        Circuit::Leaf(e) => name_of(e).to_string(),
        Circuit::Series(parts) => parts
            .iter()
            .map(|p| match p {
                // A parallel block is already bracketed by `p(...)`; a nested
                // series needs parens or the `-` would re-associate flat.
                Circuit::Series(_) => format!("({})", to_spec(p)),
                _ => to_spec(p),
            })
            .collect::<Vec<_>>()
            .join("-"),
        Circuit::Parallel(parts) => format!(
            "p({})",
            parts.iter().map(to_spec).collect::<Vec<_>>().join(",")
        ),
    }
}

/// A circuit's parameters, flattened in left-to-right leaf order.
pub fn to_params(c: &Circuit) -> Vec<f64> {
    match c {
        Circuit::Leaf(e) => params_of(e),
        Circuit::Series(parts) | Circuit::Parallel(parts) => {
            parts.iter().flat_map(to_params).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_randles_spec_parses_to_the_circuit_it_describes() {
        let c = parse("R-p(R,C)", &[0.2, 0.01, 100e-6]).unwrap();
        assert_eq!(
            c,
            Circuit::Series(vec![
                Circuit::Leaf(Element::Resistor(0.2)),
                Circuit::Parallel(vec![
                    Circuit::Leaf(Element::Resistor(0.01)),
                    Circuit::Leaf(Element::Capacitor(100e-6)),
                ]),
            ])
        );
    }

    #[test]
    fn instance_tags_name_a_leaf_without_changing_it() {
        // `Rs` and `R` are the same element; the tag is for the reader.
        let tagged = parse("Rs-p(Rct,Cdl)", &[0.2, 0.01, 100e-6]).unwrap();
        let bare = parse("R-p(R,C)", &[0.2, 0.01, 100e-6]).unwrap();
        assert_eq!(tagged, bare);
    }

    #[test]
    fn parameters_are_consumed_in_left_to_right_leaf_order() {
        // The documented contract, and the one that makes a parameter vector
        // interchangeable with the papers/ecm-pf implementations. A CPE takes
        // two, so the resistor after it must start at index 3.
        let c = parse("R-Q-R", &[1.0, 2.0, 0.5, 3.0]).unwrap();
        assert_eq!(
            c,
            Circuit::Series(vec![
                Circuit::Leaf(Element::Resistor(1.0)),
                Circuit::Leaf(Element::Cpe { q: 2.0, n: 0.5 }),
                Circuit::Leaf(Element::Resistor(3.0)),
            ])
        );
    }

    #[test]
    fn a_wrong_parameter_count_is_an_error_in_both_directions() {
        // Too few is obvious. Too MANY is the interesting one: ignoring the
        // tail would silently fit a different circuit than the caller wrote.
        let short = parse("R-p(R,C)", &[0.2, 0.01]).unwrap_err();
        assert!(short.contains("not enough parameters"), "{short}");
        let long = parse("R-p(R,C)", &[0.2, 0.01, 1e-4, 99.0]).unwrap_err();
        assert!(long.contains("disagree"), "{long}");
    }

    #[test]
    fn an_unknown_element_letter_names_the_alternatives() {
        let err = parse("R-X", &[1.0, 2.0]).unwrap_err();
        assert!(err.contains("unknown element"), "{err}");
        assert!(err.contains("R C L Q W Ws Wo G P H"), "{err}");
    }

    #[test]
    fn ws_and_wo_are_elements_while_w_plus_a_tag_is_still_a_warburg() {
        // The whole point of longest-match, and the case that made the old
        // "one character" rule untenable: `Ws` was ALREADY a valid spelling
        // (a `W` tagged `s`), so adopting it as an element name had to change
        // where the boundary falls.
        let ws = parse("Ws", &[10.0, 1.0]).unwrap();
        assert_eq!(ws, Circuit::Leaf(Element::FiniteShort { rw: 10.0, tau: 1.0 }));
        let wo = parse("Wo", &[10.0, 1.0]).unwrap();
        assert_eq!(wo, Circuit::Leaf(Element::FiniteOpen { rw: 10.0, tau: 1.0 }));

        // `W1` is NOT an element name, so it still splits as `W` + tag `1`
        // and takes one parameter. If longest-match were greedy about any
        // two characters this would break.
        let w1 = parse("W1", &[2.5]).unwrap();
        assert_eq!(w1, Circuit::Leaf(Element::Warburg(2.5)));

        // And the new elements take tags like any other.
        let tagged = parse("Ws1", &[10.0, 1.0]).unwrap();
        assert_eq!(tagged, ws);
    }

    #[test]
    fn the_old_letters_are_read_as_deprecated_aliases_but_never_written() {
        // The alias window exists so the engine and `papers/ecm-pf`'s library
        // are never speaking different grammars at the same moment -- that
        // lane's conformance suite passes spec strings through verbatim, so a
        // flag day would break the only independent check either side has.
        assert_eq!(parse("T1", &[10.0, 1.0]).unwrap(), parse("Ws1", &[10.0, 1.0]).unwrap());
        assert_eq!(parse("O1", &[10.0, 1.0]).unwrap(), parse("Wo1", &[10.0, 1.0]).unwrap());

        // Read, but not written: a round trip NORMALISES the old spelling, so
        // the deprecation actually retires rather than persisting forever.
        let c = parse("R0-p(Q1,R1-T1)", &[1.0, 2.0, 0.9, 3.0, 10.0, 1.0]).unwrap();
        assert_eq!(to_spec(&c), "R-p(Q,R-Ws)");
    }

    #[test]
    fn topology_and_parameters_round_trip() {
        for (spec, params) in [
            ("R", vec![50.0]),
            ("R-p(R,C)", vec![0.2, 0.01, 1e-4]),
            ("R-p(Q,R)-W", vec![0.2, 1e-5, 0.8, 0.01, 2.5]),
            ("p(R,C,L)", vec![1.0, 2.0, 3.0]),
            ("R-p(R-C,Q)", vec![1.0, 2.0, 3.0, 4.0, 0.9]),
        ] {
            let c = parse(spec, &params).unwrap();
            assert_eq!(to_spec(&c), spec, "topology round trip for `{spec}`");
            assert_eq!(to_params(&c), params, "parameter round trip for `{spec}`");
        }
    }

    #[test]
    fn a_nested_series_inside_a_parallel_branch_keeps_its_grouping() {
        // `p(R-C,Q)` is a two-branch parallel whose first branch is itself a
        // series. Flattening it would change the circuit, so the writer must
        // reproduce the grouping the reader saw.
        let c = parse("p(R-C,Q)", &[1.0, 2.0, 3.0, 0.9]).unwrap();
        match &c {
            Circuit::Parallel(parts) => {
                assert_eq!(parts.len(), 2, "two branches, not three");
                assert!(matches!(parts[0], Circuit::Series(_)));
            }
            other => panic!("expected a parallel block, got {other:?}"),
        }
        assert_eq!(to_spec(&c), "p(R-C,Q)");
    }

    #[test]
    fn a_single_branch_parallel_block_is_rejected() {
        // `p(R)` is not parallel anything -- almost certainly a typo for a
        // missing branch, and silently treating it as a bare `R` would hide
        // the mistake.
        let err = parse("p(R)", &[1.0]).unwrap_err();
        assert!(err.contains("at least two branches"), "{err}");
    }
}
