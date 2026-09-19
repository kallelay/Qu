//! End-to-end tests for the digital-communications builtins.
//!
//! `comms.rs` unit-tests the arithmetic; these test the half that lives
//! in `lib.rs` and that unit tests cannot reach — the dispatch arms, the
//! argument reading, and the shape of the `Value` each one hands back.
//! A correct algorithm wired to the wrong `Value` variant, or reading
//! argument 1 where it meant argument 0, passes every test in `comms.rs`.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    match it.run(src) {
        Ok(()) => panic!("expected an error, but this ran clean:\n{src}"),
        Err(e) => e.to_string(),
    }
}

fn vec_of(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(xs)) => xs.as_ref().clone(),
        other => panic!("{name} is {other:?}, not a Vec"),
    }
}

fn bool_of(it: &Interp, name: &str) -> bool {
    match it.get(name) {
        Some(Value::Bool(b)) => *b,
        other => panic!("{name} is {other:?}, not a Bool"),
    }
}

fn field<'a>(it: &'a Interp, name: &str, key: &str) -> &'a Value {
    match it.get(name) {
        Some(Value::Record(fields)) => fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or_else(|| {
                panic!(
                    "{name} has no field {key:?}; it has {:?}",
                    fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                )
            }),
        other => panic!("{name} is {other:?}, not a Record"),
    }
}

// ----------------------------------------------------------------- QAM

#[test]
fn qam_modulate_returns_complex_symbols_and_demodulate_inverts_it() {
    let it = run(
        "bits = [0, 1, 1, 0, 1, 0, 0, 1]\n\
         syms = qam_modulate(bits, 16)\n\
         back = qam_demodulate(syms, 16)",
    );
    // 16-QAM packs 4 bits per symbol, so 8 bits is 2 symbols -- and it
    // must be a CVec, not a real Vec of interleaved parts.
    match it.get("syms") {
        Some(Value::CVec(zs)) => {
            assert_eq!(zs.len(), 2, "8 bits at 4 bits/symbol is 2 symbols");
            for z in zs.iter() {
                // Every 16-QAM point sits on the odd-integer grid.
                for part in [z.re, z.im] {
                    assert!(
                        [-3.0, -1.0, 1.0, 3.0].contains(&part),
                        "{part} is not a 16-QAM axis level"
                    );
                }
            }
        }
        other => panic!("syms is {other:?}, not a CVec"),
    }
    assert_eq!(vec_of(&it, "back"), vec_of(&it, "bits"));
}

/// Order 4 is QPSK, the case the hand-rolled two-liner this generalizes
/// already covered -- so its answer is the one that must not move.
#[test]
fn order_four_is_qpsk_on_the_unit_odd_grid() {
    let it = run("s = qam_modulate([0, 0, 0, 1, 1, 0, 1, 1], 4)");
    match it.get("s") {
        Some(Value::CVec(zs)) => {
            let got: Vec<(f64, f64)> = zs.iter().map(|z| (z.re, z.im)).collect();
            assert_eq!(
                got,
                vec![(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
            );
        }
        other => panic!("s is {other:?}, not a CVec"),
    }
}

#[test]
fn qam_demodulate_accepts_a_real_vector_as_zero_quadrature() {
    // The I axis alone of 4-QAM: quadrature defaults to 0, which rounds
    // to the level below the midpoint, so this must run and decide
    // rather than reject a real input.
    let it = run("b = qam_demodulate([-1, 1], 4)");
    assert_eq!(vec_of(&it, "b").len(), 4, "2 symbols x 2 bits");
}

#[test]
fn qam_rejects_a_non_square_order_by_name() {
    let msg = err("x = qam_modulate([1, 0, 1, 0, 1], 32)");
    assert!(msg.contains("qam_modulate"), "error must name the builtin: {msg}");
    assert!(
        msg.contains("32"),
        "error must name the offending order: {msg}"
    );
}

#[test]
fn qam_rejects_a_partial_symbol() {
    let msg = err("x = qam_modulate([1, 0, 1], 16)");
    assert!(msg.contains("multiple of 4"), "{msg}");
}

#[test]
fn bit_arguments_reject_a_value_that_is_not_a_bit() {
    let msg = err("x = qam_modulate([0, 1, 2, 1], 4)");
    assert!(msg.contains("0 or 1"), "{msg}");
    assert!(msg.contains("element 3"), "error must locate it: {msg}");
}

// ------------------------------------------------------- Hamming(7,4)

/// Hand-computed from the verified reference: `d = [1,0,1,1]` gives
/// `p1 = 1^0^1 = 0`, `p2 = 1^1^1 = 1`, `p4 = 0^1^1 = 0`, laid out
/// `[p1, p2, d1, p4, d2, d3, d4]`.
const D: [f64; 4] = [1.0, 0.0, 1.0, 1.0];
const CODEWORD: [f64; 7] = [0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0];

#[test]
fn hamming_encode_matches_the_hand_computed_codeword() {
    let it = run("c = hamming74_encode([1, 0, 1, 1])");
    assert_eq!(vec_of(&it, "c"), CODEWORD.to_vec());
}

#[test]
fn hamming_decode_returns_a_record_with_data_corrected_and_positions() {
    let it = run("r = hamming74_decode([0, 1, 1, 0, 0, 1, 1])");
    match field(&it, "r", "data") {
        Value::Vec(xs) => assert_eq!(xs.as_ref().clone(), D.to_vec()),
        other => panic!("data is {other:?}"),
    }
    assert!(
        matches!(field(&it, "r", "corrected"), Value::Bool(false)),
        "a clean codeword must not report a correction"
    );
    match field(&it, "r", "positions") {
        Value::Vec(xs) => assert_eq!(xs.as_ref().clone(), vec![0.0]),
        other => panic!("positions is {other:?}"),
    }
}

/// The reference's own acceptance case, driven through the builtins:
/// every one of the seven single-bit-flip positions must come back as
/// the original data, with the syndrome naming the flipped bit.
#[test]
fn hamming_corrects_all_seven_flip_positions_through_the_builtin() {
    for pos in 0..7usize {
        let mut bad = CODEWORD;
        bad[pos] = 1.0 - bad[pos];
        let src = format!(
            "r = hamming74_decode([{}])",
            bad.iter()
                .map(|b| format!("{b}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let it = run(&src);
        match field(&it, "r", "data") {
            Value::Vec(xs) => {
                assert_eq!(xs.as_ref().clone(), D.to_vec(), "flip at position {}", pos + 1)
            }
            other => panic!("data is {other:?}"),
        }
        assert!(
            matches!(field(&it, "r", "corrected"), Value::Bool(true)),
            "flip at position {} was not reported as corrected",
            pos + 1
        );
        match field(&it, "r", "positions") {
            Value::Vec(xs) => assert_eq!(
                xs.as_ref().clone(),
                vec![(pos + 1) as f64],
                "syndrome must name the 1-indexed flipped bit"
            ),
            other => panic!("positions is {other:?}"),
        }
    }
}

#[test]
fn hamming_round_trips_several_codewords_in_one_call() {
    let it = run(
        "d = [1, 0, 1, 1, 0, 0, 1, 0]\n\
         c = hamming74_encode(d)\n\
         r = hamming74_decode(c)",
    );
    assert_eq!(vec_of(&it, "c").len(), 14);
    match field(&it, "r", "data") {
        Value::Vec(xs) => assert_eq!(xs.as_ref().clone(), vec_of(&it, "d")),
        other => panic!("data is {other:?}"),
    }
}

// ----------------------------------------------------------------- CRC

#[test]
fn crc_accepts_the_polynomial_as_a_bit_vector_or_as_a_number() {
    let it = run(
        "a = crc([1, 1, 0, 1], [1, 0, 1, 1])\n\
         b = crc([1, 1, 0, 1], 11)",
    );
    // Hand-worked: 1101 over x^3 + x + 1 leaves 001.
    assert_eq!(vec_of(&it, "a"), vec![0.0, 0.0, 1.0]);
    assert_eq!(
        vec_of(&it, "b"),
        vec_of(&it, "a"),
        "the two spellings of the same polynomial must agree"
    );
}

#[test]
fn a_message_with_its_crc_appended_passes_the_check() {
    // Qu has no vector-concatenation operator, so the codeword is
    // assembled here from what `crc` actually returned -- which also
    // means the check runs against this run's remainder, not a constant
    // that could drift away from the implementation.
    let msg = [1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0];
    let first = run("m = [1, 0, 1, 1, 0, 0, 1, 0, 1, 1]\nr = crc(m, 11)");
    let r = vec_of(&first, "r");
    assert_eq!(r.len(), 3, "an order-3 polynomial gives 3 bits");

    let codeword: Vec<String> = msg
        .iter()
        .chain(r.iter())
        .map(|b| format!("{b}"))
        .collect();
    let it = run(&format!("ok = crc_check([{}], 11)", codeword.join(", ")));
    assert!(bool_of(&it, "ok"), "a clean codeword must check out");
}

#[test]
fn a_corrupted_codeword_fails_the_check() {
    // 1101 + CRC 001, with the last bit flipped.
    let it = run("ok = crc_check([1, 1, 0, 1, 0, 0, 0], 11)");
    assert!(!bool_of(&it, "ok"));
}

#[test]
fn crc_width_is_the_polynomials_and_not_a_hardcoded_one() {
    let it = run(
        "a = crc([1, 1, 0, 1, 0, 0, 1], 3)\n\
         b = crc([1, 1, 0, 1, 0, 0, 1], 11)\n\
         c = crc([1, 1, 0, 1, 0, 0, 1], 283)",
    );
    assert_eq!(vec_of(&it, "a").len(), 1);
    assert_eq!(vec_of(&it, "b").len(), 3);
    assert_eq!(vec_of(&it, "c").len(), 8);
}

#[test]
fn crc_rejects_a_polynomial_without_its_leading_one() {
    let msg = err("x = crc([1, 0], [0, 1, 1])");
    assert!(msg.contains("leading 1"), "{msg}");
}
