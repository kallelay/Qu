//! § bytes, encodings and binary layout (2026-09-16) — `bytes_ops.rs`.
//!
//! Every digest and encoding below is pinned to a **published** test vector
//! (RFC 4648 for base64, the CRC-32 "check" value from the CRC catalogue,
//! FIPS 180-4's own `"abc"` example for SHA-256) rather than to whatever
//! this implementation happened to produce on the day it was written. A
//! self-generated expectation pins the bug as firmly as the behaviour.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn text_of(it: &Interp, name: &str) -> String {
    match it.get(name) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{name}` is {other:?}, expected text"),
    }
}

fn nums_of(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(v)) => v.to_vec(),
        other => panic!("`{name}` is {other:?}, expected a vector"),
    }
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

#[test]
fn sha256_matches_fips_180_4_abc() {
    let it = run(r#"h = sha256("abc")"#);
    assert_eq!(
        text_of(&it, "h"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_matches_the_empty_string_vector() {
    let it = run(r#"h = sha256("")"#);
    assert_eq!(
        text_of(&it, "h"),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn crc32_matches_the_catalogue_check_value() {
    // The CRC catalogue's check value for CRC-32/ISO-HDLC over "123456789"
    // is 0xCBF43926.
    let it = run(r#"c = crc32("123456789")"#);
    assert_eq!(num_of(&it, "c"), 0xCBF4_3926u32 as f64);
}

#[test]
fn base64_matches_rfc4648_padding_cases() {
    // The three padding lengths, which is where hand-rolled base64 breaks.
    let it = run(
        r#"
a = base64_encode("Man")
b = base64_encode("Ma")
c = base64_encode("M")
"#,
    );
    assert_eq!(text_of(&it, "a"), "TWFu");
    assert_eq!(text_of(&it, "b"), "TWE=");
    assert_eq!(text_of(&it, "c"), "TQ==");
}

#[test]
fn base64_round_trips_every_byte_value() {
    let it = run(
        r#"
all = 0 to 255
same = base64_decode(base64_encode(all))
"#,
    );
    let out = nums_of(&it, "same");
    assert_eq!(out.len(), 256, "a full byte range must survive the round trip");
    for (i, x) in out.iter().enumerate() {
        assert_eq!(*x, i as f64, "byte {i} changed across encode/decode");
    }
}

#[test]
fn hex_round_trips_and_rejects_odd_length() {
    let it = run(r#"h = hex_encode(hex_decode("DEADbeef"))"#);
    assert_eq!(text_of(&it, "h"), "deadbeef");

    let mut bad = Interp::new();
    let err = bad.run(r#"x = hex_decode("abc")"#).unwrap_err();
    assert!(
        format!("{err}").contains("odd number"),
        "an odd-length hex string must be refused, got: {err}"
    );
}

#[test]
fn pack_round_trips_mixed_fields() {
    let it = run(
        r#"
v = unpack(pack([1, -2, 3.5], "u8,i16,f32"), "u8,i16,f32")
"#,
    );
    assert_eq!(nums_of(&it, "v"), vec![1.0, -2.0, 3.5]);
}

#[test]
fn pack_endianness_is_not_cosmetic() {
    // 258 = 0x0102. The two orders must actually differ, and each must be
    // the documented one -- a test that only checked "round trips" would
    // pass with endianness ignored entirely.
    let it = run(
        r#"
be = pack([258], "u16", endian="big")
le = pack([258], "u16")
"#,
    );
    assert_eq!(nums_of(&it, "be"), vec![1.0, 2.0]);
    assert_eq!(nums_of(&it, "le"), vec![2.0, 1.0]);
}

#[test]
fn pack_refuses_an_out_of_range_field_rather_than_wrapping() {
    let mut it = Interp::new();
    let err = it.run(r#"x = pack([300], "u8")"#).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("300") && msg.contains("0..255"),
        "the error must name the value and the field range, got: {msg}"
    );
}

#[test]
fn unpack_refuses_a_short_buffer_rather_than_zero_filling() {
    let mut it = Interp::new();
    let err = it.run(r#"x = unpack([1, 2], "u32")"#).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("4 bytes") && msg.contains("2"),
        "the error must name both the required and available sizes, got: {msg}"
    );
}

#[test]
fn a_byte_buffer_is_validated_not_masked() {
    let mut it = Interp::new();
    let err = it.run(r#"x = crc32([1, 2, 300])"#).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("byte 2") && msg.contains("0..255"),
        "the offending index and the range must both be named, got: {msg}"
    );
}

#[test]
fn entropy_spans_zero_to_eight_bits() {
    let it = run(
        r#"
flat = entropy([7, 7, 7, 7])
full = entropy(0 to 255)
"#,
    );
    assert_eq!(num_of(&it, "flat"), 0.0, "a constant buffer carries no information");
    assert!(
        (num_of(&it, "full") - 8.0).abs() < 1e-12,
        "a uniform byte distribution is exactly 8 bits/byte"
    );
}

#[test]
fn hexdump_shows_offset_hex_and_ascii() {
    let it = run(r#"d = hexdump("AB")"#);
    let d = text_of(&it, "d");
    assert!(d.starts_with("00000000  "), "offset column missing: {d:?}");
    assert!(d.contains("41 42"), "hex column missing: {d:?}");
    assert!(d.trim_end().ends_with("AB"), "ascii column missing: {d:?}");
}

#[test]
fn bytes_write_then_read_returns_the_same_buffer() {
    let dir = std::env::temp_dir().join("qu_bytes_ops_roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("buf.bin");
    let p = path.to_string_lossy().replace('\\', "/");
    let it = run(&format!(
        r#"
n = bytes_write("{p}", [0, 127, 255])
back = bytes_read("{p}")
part = bytes_read("{p}", from=1, len=2)
"#
    ));
    assert_eq!(num_of(&it, "n"), 3.0);
    assert_eq!(nums_of(&it, "back"), vec![0.0, 127.0, 255.0]);
    assert_eq!(nums_of(&it, "part"), vec![127.0, 255.0]);
    let _ = std::fs::remove_file(&path);
}
