//! § bytes, encodings and binary layout (2026-09-16).
//!
//! The "bytes half" of the text toolkit spec (`docs/design/toolkit-text.md`
//! §1), which the gap analysis found wholly absent: a gap analysis probe put
//! 0 of `pack`, `unpack`, `hexdump`, `crc32`, `sha256`, `base64`, `entropy`
//! in the engine. `read_bytes` exists but is a **serial-port** function
//! (`serial_ops.rs`), not file I/O — another instance of a name that is true
//! and a value that is not, so the file-oriented verbs here are spelled
//! `bytes_read`/`bytes_write` rather than colliding with it.
//!
//! **Representation.** A byte buffer is a `Value::Vec` of numbers in
//! `0..=255`, not a new `Value` variant. `Value` already carries 43 variants
//! and 283 catch-all arms, and every one of those arms is a place a new
//! variant silently does the wrong thing; a vector of small integers reuses
//! slicing, indexing, `len`, and printing for free. The cost is that a byte
//! buffer is not type-distinct from any other vector — the spec asks for a
//! distinct `Bytes` type and this is deliberately less than that. Every
//! function here validates the range and says so on failure rather than
//! truncating, which is the part of the guarantee that can be kept cheaply.
//!
//! **No new dependencies.** CRC-32 and SHA-256 are implemented here rather
//! than pulled in: both are short, both are fully pinned by published test
//! vectors (see `engine/tests/bytes_ops.rs`), and the build stays offline.
//!
//! Follows `fs_ops.rs`'s module shape exactly — one `mod` line and one match
//! arm in `lib.rs`, everything else here — for the same reason it gives:
//! several sessions edit `lib.rs` concurrently and a small footprint is the
//! difference between a clean merge and a conflict.

use crate::{e, int_arg, style_num, style_str, text_arg, to_vec, EvalError, Value, R};

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "bytes_read" => bytes_read(args, style),
        "bytes_write" => bytes_write(args),
        "hexdump" => hexdump(args, style),
        "hex_encode" => hex_encode(args),
        "hex_decode" => hex_decode(args),
        "base64_encode" => base64_encode(args),
        "base64_decode" => base64_decode(args),
        "crc32" => crc32_fn(args),
        "sha256" => sha256_fn(args),
        "entropy" => entropy(args),
        "pack" => pack(args, style),
        "unpack" => unpack(args, style),
        // § addressed bit ops + binary convenience (2026-09-23) -- see
        // each function's own doc comment; grouped here rather than in a
        // separate module since they all share this file's `bytes_arg`/
        // `bytes_value` representation.
        "get_bit" => get_bit(args),
        "set_bit" => set_bit(args, true),
        "clear_bit" => set_bit(args, false),
        "toggle_bit" => toggle_bit(args),
        "swap_endian" => swap_endian(args, style),
        "reverse_bytes" => reverse_bytes(args),
        "pad_bytes" => pad_bytes(args, style),
        "find_hex" => find_hex(args, style),
        "replace_bytes" => replace_bytes(args),
        other => e(format!("bytes_ops: unknown function `{other}`")),
    }
}

// ---------------------------------------------------------------- helpers

/// A byte buffer argument. Rejects out-of-range and non-integral values by
/// naming the offending index -- a buffer that is silently masked to
/// `& 0xff` is the kind of quiet wrong answer this module exists to avoid.
fn bytes_arg(args: &[Value], i: usize, who: &str) -> R<Vec<u8>> {
    let v = match args.get(i) {
        Some(v) => v,
        None => return e(format!("{who}: expected a byte buffer as argument {}", i + 1)),
    };
    if let Value::Str(s) = v {
        return Ok(s.as_bytes().to_vec());
    }
    let xs = to_vec(v).map_err(|_| EvalError {
        msg: format!("{who}: expected a byte buffer (a vector of 0..255) or text"),
    })?;
    let mut out = Vec::with_capacity(xs.len());
    for (k, x) in xs.iter().enumerate() {
        if !x.is_finite() || x.fract() != 0.0 || *x < 0.0 || *x > 255.0 {
            return e(format!(
                "{who}: byte {k} is `{x}`, which is not a whole number in 0..255 -- \
                 a byte buffer is a vector of 0..255, and this is not being masked for you"
            ));
        }
        out.push(*x as u8);
    }
    Ok(out)
}

fn bytes_value(b: &[u8]) -> Value {
    Value::Vec(b.iter().map(|x| *x as f64).collect::<Vec<f64>>().into())
}

/// `endian=` keyword, shared by `pack`/`unpack`. Defaults to little-endian,
/// which is what every platform this runs on uses; `"big"`/`"network"` is
/// the wire convention.
fn endian_big(style: &[(String, Value)], who: &str) -> R<bool> {
    match style_str(style, "endian") {
        None => Ok(false),
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "le" | "little" => Ok(false),
            "be" | "big" | "network" => Ok(true),
            other => e(format!(
                "{who}: `endian=\"{other}\"` is not an endianness -- use \
                 \"little\"/\"le\" or \"big\"/\"be\"/\"network\""
            )),
        },
    }
}

// ------------------------------------------------------------------- I/O

fn bytes_read(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let all = std::fs::read(&path).map_err(|err| EvalError {
        msg: format!("bytes_read: could not read `{path}`: {err}"),
    })?;
    let from = style_num(style, "from").unwrap_or(0.0);
    if from < 0.0 || from.fract() != 0.0 {
        return e(format!("bytes_read: `from={from}` must be a whole number >= 0"));
    }
    let from = from as usize;
    if from > all.len() {
        return e(format!(
            "bytes_read: `from={from}` is past the end of `{path}` ({} bytes)",
            all.len()
        ));
    }
    let rest = &all[from..];
    let take = match style_num(style, "len") {
        None => rest.len(),
        Some(n) => {
            if n < 0.0 || n.fract() != 0.0 {
                return e(format!("bytes_read: `len={n}` must be a whole number >= 0"));
            }
            let n = n as usize;
            if n > rest.len() {
                return e(format!(
                    "bytes_read: asked for {n} bytes from offset {from} but only {} remain in `{path}`",
                    rest.len()
                ));
            }
            n
        }
    };
    Ok(bytes_value(&rest[..take]))
}

fn bytes_write(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let b = bytes_arg(args, 1, "bytes_write")?;
    std::fs::write(&path, &b).map_err(|err| EvalError {
        msg: format!("bytes_write: could not write `{path}`: {err}"),
    })?;
    Ok(Value::Num(b.len() as f64))
}

// ------------------------------------------------------------ inspection

fn hexdump(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let b = bytes_arg(args, 0, "hexdump")?;
    let width = match style_num(style, "width") {
        None => 16usize,
        Some(w) => {
            if w < 1.0 || w > 64.0 || w.fract() != 0.0 {
                return e(format!("hexdump: `width={w}` must be a whole number in 1..64"));
            }
            w as usize
        }
    };
    let base = style_num(style, "from").unwrap_or(0.0) as usize;
    let mut out = String::new();
    for (row, chunk) in b.chunks(width).enumerate() {
        let off = base + row * width;
        out.push_str(&format!("{off:08x}  "));
        for i in 0..width {
            match chunk.get(i) {
                Some(x) => out.push_str(&format!("{x:02x} ")),
                None => out.push_str("   "),
            }
        }
        out.push(' ');
        for x in chunk {
            out.push(if *x >= 0x20 && *x < 0x7f { *x as char } else { '.' });
        }
        out.push('\n');
    }
    Ok(Value::Str(out))
}

/// Shannon entropy of the byte distribution, in bits per byte. 0 for a
/// constant buffer, 8 for uniformly random. Useful for telling compressed or
/// encrypted regions from structured ones in a file you are reverse-engineering.
fn entropy(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "entropy")?;
    if b.is_empty() {
        return e("entropy: the buffer is empty, so there is no distribution to measure".to_string());
    }
    let mut counts = [0usize; 256];
    for x in &b {
        counts[*x as usize] += 1;
    }
    let n = b.len() as f64;
    let mut h = 0.0;
    for c in counts.iter() {
        if *c > 0 {
            let p = *c as f64 / n;
            h -= p * p.log2();
        }
    }
    Ok(Value::Num(h))
}

// -------------------------------------------------------------- encodings

fn hex_encode(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "hex_encode")?;
    let mut s = String::with_capacity(b.len() * 2);
    for x in &b {
        s.push_str(&format!("{x:02x}"));
    }
    Ok(Value::Str(s))
}

fn hex_decode(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if t.len() % 2 != 0 {
        return e(format!(
            "hex_decode: `{}` has an odd number of hex digits ({}) -- a byte is two",
            s,
            t.len()
        ));
    }
    let bs = t.as_bytes();
    let mut out = Vec::with_capacity(t.len() / 2);
    let mut i = 0;
    while i < bs.len() {
        let hi = hex_nibble(bs[i], &s)?;
        let lo = hex_nibble(bs[i + 1], &s)?;
        out.push(hi * 16 + lo);
        i += 2;
    }
    Ok(bytes_value(&out))
}

fn hex_nibble(c: u8, whole: &str) -> R<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => e(format!(
            "hex_decode: `{}` is not a hex digit, in `{whole}`",
            c as char
        )),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "base64_encode")?;
    let mut s = String::new();
    for chunk in b.chunks(3) {
        let a = chunk[0] as u32;
        let bb = *chunk.get(1).unwrap_or(&0) as u32;
        let c = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (a << 16) | (bb << 8) | c;
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    Ok(Value::Str(s))
}

fn base64_decode(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let t: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if t.len() % 4 != 0 {
        return e(format!(
            "base64_decode: input length {} is not a multiple of 4 -- this is \
             standard base64 with padding, not the URL-safe unpadded variant",
            t.len()
        ));
    }
    let mut out = Vec::new();
    for chunk in t.chunks(4) {
        let pad = chunk.iter().filter(|c| **c == b'=').count();
        let mut n: u32 = 0;
        for (i, c) in chunk.iter().enumerate() {
            let v = if *c == b'=' {
                0
            } else {
                match B64.iter().position(|x| x == c) {
                    Some(p) => p as u32,
                    None => {
                        return e(format!(
                            "base64_decode: `{}` is not a base64 character",
                            *c as char
                        ))
                    }
                }
            };
            n |= v << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(bytes_value(&out))
}

// --------------------------------------------------------------- digests

fn crc32_fn(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "crc32")?;
    let mut crc: u32 = 0xffff_ffff;
    for x in &b {
        crc ^= *x as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    Ok(Value::Num((crc ^ 0xffff_ffff) as f64))
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_bytes(msg: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut data = msg.to_vec();
    let bitlen = (msg.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bitlen.to_be_bytes());

    for block in data.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut ee, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = ee.rotate_right(6) ^ ee.rotate_right(11) ^ ee.rotate_right(25);
            let ch = (ee & f) ^ ((!ee) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = ee;
            ee = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(ee);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

fn sha256_fn(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "sha256")?;
    let d = sha256_bytes(&b);
    let mut s = String::with_capacity(64);
    for x in &d {
        s.push_str(&format!("{x:02x}"));
    }
    Ok(Value::Str(s))
}

// ---------------------------------------------------------- pack/unpack

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl Ty {
    fn width(self) -> usize {
        match self {
            Ty::U8 | Ty::I8 => 1,
            Ty::U16 | Ty::I16 => 2,
            Ty::U32 | Ty::I32 | Ty::F32 => 4,
            Ty::U64 | Ty::I64 | Ty::F64 => 8,
        }
    }
}

/// Parse `"u16,i32,f64"` or `"u16 i32 f64"` into a field list. A count
/// prefix repeats a field: `"4u8"` is four bytes.
fn parse_spec(spec: &str, who: &str) -> R<Vec<Ty>> {
    let mut out = Vec::new();
    for raw in spec.split(|c: char| c == ',' || c.is_whitespace()) {
        let tok = raw.trim();
        if tok.is_empty() {
            continue;
        }
        let digits: String = tok.chars().take_while(|c| c.is_ascii_digit()).collect();
        let (count, name) = if digits.is_empty() {
            (1usize, tok)
        } else {
            (
                digits.parse::<usize>().unwrap_or(1),
                &tok[digits.len()..],
            )
        };
        let ty = match name.to_ascii_lowercase().as_str() {
            "u8" | "byte" => Ty::U8,
            "i8" => Ty::I8,
            "u16" => Ty::U16,
            "i16" => Ty::I16,
            "u32" => Ty::U32,
            "i32" => Ty::I32,
            "u64" => Ty::U64,
            "i64" => Ty::I64,
            "f32" | "float" => Ty::F32,
            "f64" | "double" => Ty::F64,
            other => {
                return e(format!(
                    "{who}: `{other}` is not a field type -- use u8/i8/u16/i16/u32/i32/\
                     u64/i64/f32/f64, optionally with a repeat count like `4u8`"
                ))
            }
        };
        for _ in 0..count {
            out.push(ty);
        }
    }
    if out.is_empty() {
        return e(format!("{who}: the layout spec `{spec}` describes no fields"));
    }
    Ok(out)
}

fn unpack(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let b = bytes_arg(args, 0, "unpack")?;
    let spec = text_arg(args, 1)?;
    let fields = parse_spec(&spec, "unpack")?;
    let big = endian_big(style, "unpack")?;
    let need: usize = fields.iter().map(|t| t.width()).sum();
    if b.len() < need {
        return e(format!(
            "unpack: layout `{spec}` needs {need} bytes but the buffer has {} -- \
             short reads are an error here, not a zero fill",
            b.len()
        ));
    }
    let mut out = Vec::with_capacity(fields.len());
    let mut off = 0usize;
    for t in &fields {
        let w = t.width();
        let s = &b[off..off + w];
        let mut buf = [0u8; 8];
        buf[..w].copy_from_slice(s);
        if big {
            buf[..w].reverse();
        }
        let raw = u64::from_le_bytes(buf);
        let v = match t {
            Ty::U8 => raw as u8 as f64,
            Ty::I8 => raw as u8 as i8 as f64,
            Ty::U16 => raw as u16 as f64,
            Ty::I16 => raw as u16 as i16 as f64,
            Ty::U32 => raw as u32 as f64,
            Ty::I32 => raw as u32 as i32 as f64,
            Ty::U64 => raw as f64,
            Ty::I64 => raw as i64 as f64,
            Ty::F32 => f32::from_bits(raw as u32) as f64,
            Ty::F64 => f64::from_bits(raw),
        };
        out.push(v);
        off += w;
    }
    Ok(Value::Vec(out.into()))
}

fn pack(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let vals = to_vec(args.first().unwrap_or(&Value::Nothing)).map_err(|_| EvalError {
        msg: "pack: expected a vector of values as argument 1".to_string(),
    })?;
    let spec = text_arg(args, 1)?;
    let fields = parse_spec(&spec, "pack")?;
    let big = endian_big(style, "pack")?;
    if vals.len() != fields.len() {
        return e(format!(
            "pack: layout `{spec}` has {} fields but {} values were given",
            fields.len(),
            vals.len()
        ));
    }
    let mut out = Vec::new();
    for (i, (t, v)) in fields.iter().zip(vals.iter()).enumerate() {
        let w = t.width();
        let raw: u64 = match t {
            Ty::F32 => (*v as f32).to_bits() as u64,
            Ty::F64 => v.to_bits(),
            _ => {
                if v.fract() != 0.0 || !v.is_finite() {
                    return e(format!(
                        "pack: value {} for an integer field is `{v}`, which is not a whole number",
                        i + 1
                    ));
                }
                let iv = *v as i64;
                let (lo, hi) = match t {
                    Ty::U8 => (0i64, 255i64),
                    Ty::I8 => (-128, 127),
                    Ty::U16 => (0, 65535),
                    Ty::I16 => (-32768, 32767),
                    Ty::U32 => (0, 4_294_967_295),
                    Ty::I32 => (-2_147_483_648, 2_147_483_647),
                    Ty::U64 => (0, i64::MAX),
                    Ty::I64 => (i64::MIN, i64::MAX),
                    _ => unreachable!(),
                };
                if iv < lo || iv > hi {
                    return e(format!(
                        "pack: value {} is {iv}, outside the range {lo}..{hi} of its field -- \
                         this is an error rather than a wrap, because a silently wrapped \
                         field is a wrong answer that looks like a right one",
                        i + 1
                    ));
                }
                iv as u64
            }
        };
        let mut buf = raw.to_le_bytes();
        if big {
            buf[..w].reverse();
        }
        out.extend_from_slice(&buf[..w]);
    }
    Ok(bytes_value(&out))
}

// ------------------------------------------------------- bit-addressed ops
//
// Bit `i` of a buffer is bit `i % 8` of byte `i / 8`, counting from the
// LSB of each byte (bit 0 of byte 0 is the buffer's least significant bit
// overall) -- the same convention `read_int`/`write_int`'s little-endian
// default already implies bit-for-bit, so a script mixing byte- and
// bit-level access of the same buffer gets one consistent answer rather
// than two conventions that happen to share a name.

fn bit_index(args: &[Value], buf_len: usize, who: &str) -> R<(usize, u8)> {
    let i = int_arg(args, 1)?;
    if i < 0 || i as usize >= buf_len * 8 {
        return e(format!(
            "{who}: bit index {i} is out of range for a {buf_len}-byte buffer (0..{})",
            buf_len * 8
        ));
    }
    let i = i as usize;
    Ok((i / 8, (i % 8) as u8))
}

fn get_bit(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "get_bit")?;
    let (byte_i, bit_i) = bit_index(args, b.len(), "get_bit")?;
    Ok(Value::Num(((b[byte_i] >> bit_i) & 1) as f64))
}

fn set_bit(args: &[Value], to_one: bool) -> R<Value> {
    let who = if to_one { "set_bit" } else { "clear_bit" };
    let mut b = bytes_arg(args, 0, who)?;
    let (byte_i, bit_i) = bit_index(args, b.len(), who)?;
    if to_one {
        b[byte_i] |= 1 << bit_i;
    } else {
        b[byte_i] &= !(1 << bit_i);
    }
    Ok(bytes_value(&b))
}

fn toggle_bit(args: &[Value]) -> R<Value> {
    let mut b = bytes_arg(args, 0, "toggle_bit")?;
    let (byte_i, bit_i) = bit_index(args, b.len(), "toggle_bit")?;
    b[byte_i] ^= 1 << bit_i;
    Ok(bytes_value(&b))
}

// -------------------------------------------------------- binary convenience

/// `swap_endian(buf, [width=4])` — reverses the byte order WITHIN each
/// `width`-byte chunk (a buffer of big-endian `u32`s becomes little-endian,
/// or back), as opposed to `reverse_bytes` below, which reverses the
/// buffer's overall order. `buf.len()` must divide evenly by `width` --
/// silently dropping a partial trailing chunk would be a quiet wrong
/// answer, the exact thing this module's own doc comment says to avoid.
fn swap_endian(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let b = bytes_arg(args, 0, "swap_endian")?;
    let width = style_num(style, "width")
        .or_else(|| args.get(1).and_then(|v| v.as_num().ok()))
        .unwrap_or(4.0);
    if width < 1.0 || width.fract() != 0.0 {
        return e(format!("swap_endian: `width={width}` must be a whole number >= 1"));
    }
    let width = width as usize;
    if b.len() % width != 0 {
        return e(format!(
            "swap_endian: the buffer is {} bytes, which doesn't divide evenly by width={width}",
            b.len()
        ));
    }
    let mut out = b.clone();
    for chunk in out.chunks_mut(width) {
        chunk.reverse();
    }
    Ok(bytes_value(&out))
}

/// `reverse_bytes(buf)` — the whole buffer, end to end. Not the same
/// operation as `swap_endian`; see that function's own doc comment for the
/// distinction.
fn reverse_bytes(args: &[Value]) -> R<Value> {
    let mut b = bytes_arg(args, 0, "reverse_bytes")?;
    b.reverse();
    Ok(bytes_value(&b))
}

/// `pad_bytes(buf, length, [value=0], [side="right"])` — pads `buf` up to
/// `length` bytes with `value`, on the given side. A `buf` already at or
/// past `length` is returned unchanged (truncating would silently discard
/// real data; that is `slice`'s job, not padding's).
fn pad_bytes(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let b = bytes_arg(args, 0, "pad_bytes")?;
    let length = int_arg(args, 1)?;
    if length < 0 {
        return e(format!("pad_bytes: `length={length}` must be >= 0"));
    }
    let length = length as usize;
    let value = style_num(style, "value").unwrap_or(0.0);
    if !(0.0..=255.0).contains(&value) || value.fract() != 0.0 {
        return e(format!("pad_bytes: `value={value}` must be a whole number in 0..255"));
    }
    let value = value as u8;
    let side = style_str(style, "side").unwrap_or_else(|| "right".to_string());
    if b.len() >= length {
        return Ok(bytes_value(&b));
    }
    let fill = vec![value; length - b.len()];
    let out = match side.as_str() {
        "right" => [b.as_slice(), fill.as_slice()].concat(),
        "left" => [fill.as_slice(), b.as_slice()].concat(),
        other => {
            return e(format!(
                "pad_bytes: `side=\"{other}\"` is not a side -- use \"left\" or \"right\""
            ))
        }
    };
    Ok(bytes_value(&out))
}

/// A hex-string pattern (no leading `0x`, whitespace ignored, e.g.
/// `"deadbeef"`) parsed into raw bytes -- the shared parser `find_hex`
/// needs and `hex_decode` above already has inline; pulled out here so
/// neither copy drifts from the other's definition of a valid pattern.
fn parse_hex_pattern(s: &str, who: &str) -> R<Vec<u8>> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if t.len() % 2 != 0 {
        return e(format!(
            "{who}: `{s}` has an odd number of hex digits ({}) -- a byte is two",
            t.len()
        ));
    }
    let bs = t.as_bytes();
    let mut out = Vec::with_capacity(t.len() / 2);
    let mut i = 0;
    while i < bs.len() {
        let hi = hex_nibble(bs[i], s)?;
        let lo = hex_nibble(bs[i + 1], s)?;
        out.push(hi * 16 + lo);
        i += 2;
    }
    Ok(out)
}

/// `find_hex(buf, pattern, [from=0])` — the byte offset of the first
/// occurrence of `pattern` (a hex string) in `buf` at or after `from`, or
/// `-1` if it doesn't occur -- matching `index_of`'s own "not found is -1,
/// not an error" convention for the same kind of search.
fn find_hex(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let b = bytes_arg(args, 0, "find_hex")?;
    let pat_s = text_arg(args, 1)?;
    let pat = parse_hex_pattern(&pat_s, "find_hex")?;
    let from = style_num(style, "from").unwrap_or(0.0);
    if from < 0.0 || from.fract() != 0.0 {
        return e(format!("find_hex: `from={from}` must be a whole number >= 0"));
    }
    let from = (from as usize).min(b.len());
    if pat.is_empty() {
        return e("find_hex: the pattern is empty".to_string());
    }
    if pat.len() > b.len() {
        return Ok(Value::Num(-1.0));
    }
    for start in from..=(b.len() - pat.len()) {
        if b[start..start + pat.len()] == pat[..] {
            return Ok(Value::Num(start as f64));
        }
    }
    Ok(Value::Num(-1.0))
}

/// `replace_bytes(buf, pattern, replacement)` — every non-overlapping
/// occurrence of `pattern` (a hex string) replaced with `replacement` (also
/// a hex string, independent length -- the result can be a different size
/// than `buf`). Scans left to right, same as `replace`'s own text
/// semantics; a match consumes its bytes before the next search starts, so
/// overlapping occurrences are not double-counted.
fn replace_bytes(args: &[Value]) -> R<Value> {
    let b = bytes_arg(args, 0, "replace_bytes")?;
    let pat = parse_hex_pattern(&text_arg(args, 1)?, "replace_bytes")?;
    let rep = parse_hex_pattern(&text_arg(args, 2)?, "replace_bytes")?;
    if pat.is_empty() {
        return e("replace_bytes: the pattern is empty".to_string());
    }
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if i + pat.len() <= b.len() && b[i..i + pat.len()] == pat[..] {
            out.extend_from_slice(&rep);
            i += pat.len();
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Ok(bytes_value(&out))
}
