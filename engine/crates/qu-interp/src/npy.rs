//! NumPy `.npy` / `.npz` reading and writing (format versions 1-3).
//!
//! A plain format layer, in the same spirit as `matfile.rs`: it knows bytes
//! and arrays, not the interpreter's `Value`. `interop_ops.rs` does the
//! conversion.
//!
//! These files are untrusted input. Every length read from the file is
//! checked against the bytes that are actually there *before* anything is
//! allocated, element counts are computed with checked arithmetic, and
//! decompression is capped, so a header claiming a 10^18-element array or a
//! deflate bomb fails with a message instead of exhausting memory.
//!
//! What is deliberately **not** handled, each refused by name: object
//! (pickled) arrays -- the pickle is never parsed, let alone run -- string,
//! datetime and structured dtypes, and zip members using anything but
//! "stored" or DEFLATE.

use crate::inflate::inflate_capped;

/// Largest single array this reader will materialise, in bytes. Qu's
/// numbers are `f64`, so this is also roughly the memory a read can cost.
pub const MAX_ARRAY_BYTES: usize = 1 << 30;
/// Largest total uncompressed size over all members of one `.npz`.
pub const MAX_NPZ_TOTAL_BYTES: u64 = 2 << 30;
/// Most members accepted in one `.npz`.
pub const MAX_NPZ_ENTRIES: u64 = 4096;
/// Largest input file (`.npy`, `.npz`, HDF5) the readers will load whole.
pub const MAX_INPUT_FILE_BYTES: u64 = 2 << 30;
/// Largest header dictionary accepted. NumPy's own writer emits well under
/// 1 KiB; the format's v2/v3 length field could claim 4 GiB.
const MAX_HEADER_BYTES: usize = 1 << 20;
/// Largest number of dimensions accepted (NumPy itself caps at 64).
const MAX_NDIM: usize = 64;

/// Decoded array values. Real data is held as `f64` (every integer type up
/// to 2^53 is exact in `f64`; larger values are refused rather than rounded).
#[derive(Debug, Clone, PartialEq)]
pub enum NpyData {
    Real(Vec<f64>),
    Bool(Vec<bool>),
    Complex(Vec<(f64, f64)>),
}

impl NpyData {
    pub fn len(&self) -> usize {
        match self {
            NpyData::Real(v) => v.len(),
            NpyData::Bool(v) => v.len(),
            NpyData::Complex(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An array as stored: its shape and its elements in **C (row-major)**
/// order regardless of how the file stored them.
#[derive(Debug, Clone, PartialEq)]
pub struct NpyArray {
    pub shape: Vec<usize>,
    pub data: NpyData,
}

// ---------------------------------------------------------------- reading

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Float,
    Int,
    Uint,
    Bool,
    Complex,
}

struct Dtype {
    kind: Kind,
    size: usize,
    big: bool,
}

/// Parse a `descr` string such as `<f8`, `>i4`, `|b1`, `<c16`.
fn parse_descr(descr: &str) -> Result<Dtype, String> {
    let b = descr.as_bytes();
    if b.is_empty() {
        return Err("empty dtype descriptor".into());
    }
    let (big, rest) = match b[0] {
        b'<' | b'|' | b'=' => (false, &descr[1..]),
        b'>' => (true, &descr[1..]),
        _ => (false, descr),
    };
    // `=` is native order; every platform Qu ships on is little-endian.
    let mut chars = rest.chars();
    let k = chars.next().ok_or_else(|| format!("dtype `{descr}` has no kind"))?;
    let size_text: String = chars.collect();
    // Refuse the kinds we never decode before looking for a size: NumPy
    // writes object arrays as plain `|O` with no size at all.
    if k == 'O' {
        return Err("this is an object (pickled) array; Qu never unpickles files, so it is \
                    refused -- re-save it from NumPy with a numeric dtype"
            .into());
    }
    // (datetime descriptors carry a unit suffix, `<M8[ns]`; only the leading
    // digits are the size)
    let digits: String = size_text.chars().take_while(|c| c.is_ascii_digit()).collect();
    let size: usize = digits
        .parse()
        .map_err(|_| format!("dtype `{descr}` has no element size"))?;
    if digits.len() != size_text.len() && !matches!(k, 'M' | 'm') {
        return Err(format!("unrecognised dtype descriptor `{descr}`"));
    }
    let (kind, ok) = match k {
        'f' => (Kind::Float, matches!(size, 2 | 4 | 8)),
        'i' => (Kind::Int, matches!(size, 1 | 2 | 4 | 8)),
        'u' => (Kind::Uint, matches!(size, 1 | 2 | 4 | 8)),
        'b' => (Kind::Bool, size == 1),
        'c' => (Kind::Complex, matches!(size, 8 | 16)),
        'O' => {
            return Err("this is an object (pickled) array; Qu never unpickles files, so it is \
                        refused -- re-save it from NumPy with a numeric dtype"
                .into())
        }
        'U' | 'S' | 'a' => {
            return Err(format!("string dtype `{descr}` is not supported (numeric arrays only)"))
        }
        'M' | 'm' => {
            return Err(format!("datetime/timedelta dtype `{descr}` is not supported"))
        }
        'V' => return Err(format!("void/structured dtype `{descr}` is not supported")),
        'g' => return Err("long double dtype is not supported".into()),
        other => return Err(format!("unknown dtype kind `{other}` in `{descr}`")),
    };
    if !ok {
        return Err(format!("dtype `{descr}` has an unsupported element size {size}"));
    }
    Ok(Dtype { kind, size, big })
}

fn f16_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let frac = (h & 0x3ff) as f64;
    match exp {
        0 => sign * frac * 2f64.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => sign * (1.0 + frac / 1024.0) * 2f64.powi(exp - 15),
    }
}

/// Read one element of `size` bytes as a float/int, honouring byte order.
fn read_scalar(chunk: &[u8], kind: Kind, size: usize, big: bool) -> Result<f64, String> {
    let mut buf = [0u8; 8];
    buf[..size].copy_from_slice(chunk);
    if big {
        buf[..size].reverse();
    }
    // `buf` is now little-endian.
    Ok(match (kind, size) {
        (Kind::Float, 2) => f16_to_f64(u16::from_le_bytes([buf[0], buf[1]])),
        (Kind::Float, 4) => f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as f64,
        (Kind::Float, 8) => f64::from_le_bytes(buf),
        (Kind::Int, 1) => buf[0] as i8 as f64,
        (Kind::Int, 2) => i16::from_le_bytes([buf[0], buf[1]]) as f64,
        (Kind::Int, 4) => i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as f64,
        (Kind::Int, 8) => {
            let v = i64::from_le_bytes(buf);
            if v.unsigned_abs() > (1u64 << 53) {
                return Err(format!(
                    "int64 value {v} cannot be held exactly (Qu numbers are 64-bit floats, exact \
                     only up to 2^53)"
                ));
            }
            v as f64
        }
        (Kind::Uint, 1) => buf[0] as f64,
        (Kind::Uint, 2) => u16::from_le_bytes([buf[0], buf[1]]) as f64,
        (Kind::Uint, 4) => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as f64,
        (Kind::Uint, 8) => {
            let v = u64::from_le_bytes(buf);
            if v > (1u64 << 53) {
                return Err(format!(
                    "uint64 value {v} cannot be held exactly (Qu numbers are 64-bit floats, exact \
                     only up to 2^53)"
                ));
            }
            v as f64
        }
        _ => return Err("internal: unexpected scalar dtype".into()),
    })
}

/// Pull the quoted/bare value text for `key` out of the header dictionary.
fn dict_value<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let pat1 = format!("'{key}'");
    let pat2 = format!("\"{key}\"");
    let at = header.find(&pat1).map(|i| i + pat1.len()).or_else(|| {
        header.find(&pat2).map(|i| i + pat2.len())
    })?;
    let rest = header[at..].trim_start().strip_prefix(':')?;
    Some(rest.trim_start())
}

struct Header {
    descr: String,
    fortran: bool,
    shape: Vec<usize>,
}

fn parse_header(text: &str) -> Result<Header, String> {
    let t = text.trim();
    if !t.starts_with('{') {
        return Err("header is not a Python dict".into());
    }
    let d = dict_value(t, "descr").ok_or("header has no 'descr'")?;
    let descr = match d.chars().next() {
        Some(q @ ('\'' | '"')) => {
            let body = &d[1..];
            let end = body.find(q).ok_or("unterminated 'descr' string")?;
            body[..end].to_string()
        }
        Some('[') | Some('(') => {
            return Err("structured/record dtypes are not supported (numeric arrays only)".into())
        }
        _ => return Err("could not read 'descr' from the header".into()),
    };
    let f = dict_value(t, "fortran_order").ok_or("header has no 'fortran_order'")?;
    let fortran = if f.starts_with("True") {
        true
    } else if f.starts_with("False") {
        false
    } else {
        return Err("'fortran_order' is neither True nor False".into());
    };
    let s = dict_value(t, "shape").ok_or("header has no 'shape'")?;
    let s = s.strip_prefix('(').ok_or("'shape' is not a tuple")?;
    let close = s.find(')').ok_or("'shape' tuple is not closed")?;
    let mut shape = Vec::new();
    for part in s[..close].split(',') {
        let p = part.trim().trim_end_matches(['L', 'l']);
        if p.is_empty() {
            continue;
        }
        let n: usize = p
            .parse()
            .map_err(|_| format!("shape entry `{p}` is not a non-negative integer"))?;
        shape.push(n);
        if shape.len() > MAX_NDIM {
            return Err(format!("array has more than {MAX_NDIM} dimensions"));
        }
    }
    Ok(Header { descr, fortran, shape })
}

/// Convert F-order element positions to C order for any rank.
fn fortran_to_c<T: Clone>(data: &[T], shape: &[usize]) -> Vec<T> {
    let n = data.len();
    if shape.len() < 2 || n == 0 {
        return data.to_vec();
    }
    // strides of the Fortran layout
    let mut fstride = vec![1usize; shape.len()];
    for i in 1..shape.len() {
        fstride[i] = fstride[i - 1] * shape[i - 1];
    }
    let mut out = Vec::with_capacity(n);
    let mut idx = vec![0usize; shape.len()];
    for _ in 0..n {
        let src: usize = idx.iter().zip(&fstride).map(|(i, s)| i * s).sum();
        out.push(data[src].clone());
        // increment idx in C order (last axis fastest)
        for ax in (0..shape.len()).rev() {
            idx[ax] += 1;
            if idx[ax] < shape[ax] {
                break;
            }
            idx[ax] = 0;
        }
    }
    out
}

/// Decode one `.npy` file.
pub fn read_npy(bytes: &[u8]) -> Result<NpyArray, String> {
    const MAGIC: &[u8] = b"\x93NUMPY";
    if bytes.len() < 10 || &bytes[..6] != MAGIC {
        return Err("not a NumPy .npy file (bad magic string)".into());
    }
    let major = bytes[6];
    let minor = bytes[7];
    let (hlen, start) = match major {
        1 => (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10usize),
        2 | 3 => {
            if bytes.len() < 12 {
                return Err("truncated .npy header".into());
            }
            (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12usize)
        }
        v => return Err(format!("unsupported .npy format version {v}.{minor}")),
    };
    if hlen > MAX_HEADER_BYTES {
        return Err(format!("header length {hlen} is implausibly large"));
    }
    let end = start
        .checked_add(hlen)
        .filter(|&e| e <= bytes.len())
        .ok_or("truncated .npy file: the header extends past the end of the file")?;
    let htext = if major == 3 {
        std::str::from_utf8(&bytes[start..end]).map_err(|_| "header is not valid UTF-8")?.to_string()
    } else {
        bytes[start..end].iter().map(|&b| b as char).collect::<String>()
    };
    let h = parse_header(&htext)?;
    let dt = parse_descr(&h.descr)?;

    let mut count: usize = 1;
    for &d in &h.shape {
        count = count
            .checked_mul(d)
            .ok_or_else(|| format!("shape {:?} overflows", h.shape))?;
    }
    let item = dt.size;
    // Bound the decoded size too (one f64 or a pair per element), not only
    // the file bytes: a uint8 array of 10^9 elements would otherwise cost 8 GB.
    count
        .checked_mul(item.max(if dt.kind == Kind::Complex { 16 } else { 8 }))
        .filter(|&n| n <= MAX_ARRAY_BYTES)
        .ok_or_else(|| {
            format!("shape {:?} is larger than the {} MiB limit", h.shape, MAX_ARRAY_BYTES >> 20)
        })?;
    let nbytes = count
        .checked_mul(item)
        .filter(|&n| n <= MAX_ARRAY_BYTES)
        .ok_or_else(|| {
            format!(
                "shape {:?} of {item}-byte elements is larger than the {} MiB limit",
                h.shape,
                MAX_ARRAY_BYTES >> 20
            )
        })?;
    let body = &bytes[end..];
    if body.len() < nbytes {
        return Err(format!(
            "truncated .npy file: shape {:?} needs {nbytes} data bytes but only {} are present",
            h.shape,
            body.len()
        ));
    }
    let body = &body[..nbytes];

    let data = match dt.kind {
        Kind::Bool => {
            let v: Vec<bool> = body.iter().map(|&b| b != 0).collect();
            NpyData::Bool(if h.fortran { fortran_to_c(&v, &h.shape) } else { v })
        }
        Kind::Complex => {
            let half = item / 2;
            let mut v = Vec::with_capacity(count);
            for c in body.chunks_exact(item) {
                let re = read_scalar(&c[..half], Kind::Float, half, dt.big)?;
                let im = read_scalar(&c[half..], Kind::Float, half, dt.big)?;
                v.push((re, im));
            }
            NpyData::Complex(if h.fortran { fortran_to_c(&v, &h.shape) } else { v })
        }
        k => {
            let mut v = Vec::with_capacity(count);
            for c in body.chunks_exact(item) {
                v.push(read_scalar(c, k, item, dt.big)?);
            }
            NpyData::Real(if h.fortran { fortran_to_c(&v, &h.shape) } else { v })
        }
    };
    Ok(NpyArray { shape: h.shape, data })
}

// ---------------------------------------------------------------- writing

fn shape_repr(shape: &[usize]) -> String {
    match shape {
        [] => "()".into(),
        [n] => format!("({n},)"),
        _ => format!("({})", shape.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(", ")),
    }
}

/// Encode an array as a version-1.0 `.npy` file: `<f8`, `|b1` or `<c16`,
/// C order. NaN, infinities and the sign of zero are preserved bit for bit.
pub fn write_npy(arr: &NpyArray) -> Vec<u8> {
    let descr = match arr.data {
        NpyData::Real(_) => "<f8",
        NpyData::Bool(_) => "|b1",
        NpyData::Complex(_) => "<c16",
    };
    let mut header =
        format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {}, }}", shape_repr(&arr.shape));
    // magic(6) + version(2) + len(2) + header + '\n' must be a multiple of 64.
    let unpadded = 10 + header.len() + 1;
    let pad = (64 - unpadded % 64) % 64;
    header.push_str(&" ".repeat(pad));
    header.push('\n');
    let mut out = Vec::with_capacity(10 + header.len() + arr.data.len() * 16);
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&(header.len() as u16).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    match &arr.data {
        NpyData::Real(v) => {
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        NpyData::Bool(v) => out.extend(v.iter().map(|&b| b as u8)),
        NpyData::Complex(v) => {
            for (re, im) in v {
                out.extend_from_slice(&re.to_le_bytes());
                out.extend_from_slice(&im.to_le_bytes());
            }
        }
    }
    out
}

// -------------------------------------------------------------------- zip

/// CRC-32 (the zip/PNG polynomial), table-driven.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "truncated zip file".to_string())
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "truncated zip file".to_string())
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    b.get(at..at + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "truncated zip file".to_string())
}

/// One member of a zip archive, decompressed and CRC-checked.
pub struct ZipMember {
    pub name: String,
    pub data: Vec<u8>,
}

/// Read every member of a zip archive (stored or DEFLATE only).
pub fn read_zip(bytes: &[u8]) -> Result<Vec<ZipMember>, String> {
    // End of central directory: scan back over a possible trailing comment.
    if bytes.len() < 22 {
        return Err("not a zip/.npz file (too short)".into());
    }
    let lowest = bytes.len().saturating_sub(22 + 65535);
    let mut eocd = None;
    let mut i = bytes.len() - 22;
    loop {
        if &bytes[i..i + 4] == b"PK\x05\x06" {
            eocd = Some(i);
            break;
        }
        if i == lowest {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.ok_or("not a zip/.npz file (no end-of-central-directory record)")?;
    let mut entries = u16_at(bytes, eocd + 10)? as u64;
    let mut cd_size = u32_at(bytes, eocd + 12)? as u64;
    let mut cd_off = u32_at(bytes, eocd + 16)? as u64;
    if entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF {
        // ZIP64: locator sits immediately before the EOCD.
        let loc = eocd
            .checked_sub(20)
            .filter(|&l| bytes.get(l..l + 4) == Some(b"PK\x06\x07"))
            .ok_or("zip64 archive without a zip64 locator")?;
        let rec = u64_at(bytes, loc + 8)? as usize;
        if bytes.get(rec..rec.saturating_add(4)) != Some(b"PK\x06\x06") {
            return Err("bad zip64 end-of-central-directory record".into());
        }
        entries = u64_at(bytes, rec + 32)?;
        cd_size = u64_at(bytes, rec + 40)?;
        cd_off = u64_at(bytes, rec + 48)?;
    }
    let cd_end = cd_off
        .checked_add(cd_size)
        .filter(|&e| e <= bytes.len() as u64)
        .ok_or("zip central directory extends past the end of the file")?;
    // Every central entry needs >= 46 bytes, which bounds the entry count.
    if entries > cd_size / 46 {
        return Err("zip entry count is inconsistent with its central directory".into());
    }
    if entries > MAX_NPZ_ENTRIES {
        return Err(format!("archive has {entries} entries; the limit is {MAX_NPZ_ENTRIES}"));
    }
    // Pre-pass over the directory: add up the claimed sizes before decoding
    // anything, so members that share a deflate stream cannot each pass a
    // per-member check and only then blow the total.
    {
        let (mut q, mut sum) = (cd_off as usize, 0u64);
        for _ in 0..entries {
            if q + 46 > cd_end as usize || &bytes[q..q + 4] != b"PK\x01\x02" {
                break; // the main loop reports the corruption
            }
            sum = sum.saturating_add(u32_at(bytes, q + 24)? as u64);
            if sum > MAX_NPZ_TOTAL_BYTES {
                return Err(format!(
                    "archive would expand to more than the {} GiB total limit",
                    MAX_NPZ_TOTAL_BYTES >> 30
                ));
            }
            let adv = 46
                + u16_at(bytes, q + 28)? as usize
                + u16_at(bytes, q + 30)? as usize
                + u16_at(bytes, q + 32)? as usize;
            q += adv;
        }
    }
    let mut total_uncompressed = 0u64;
    let mut out = Vec::new();
    let mut p = cd_off as usize;
    let cd_end = cd_end as usize;
    for _ in 0..entries {
        if p + 46 > cd_end || &bytes[p..p + 4] != b"PK\x01\x02" {
            return Err("corrupt zip central directory".into());
        }
        let flags = u16_at(bytes, p + 8)?;
        let method = u16_at(bytes, p + 10)?;
        let crc = u32_at(bytes, p + 16)?;
        let mut csize = u32_at(bytes, p + 20)? as u64;
        let mut usize_ = u32_at(bytes, p + 24)? as u64;
        let nlen = u16_at(bytes, p + 28)? as usize;
        let elen = u16_at(bytes, p + 30)? as usize;
        let clen = u16_at(bytes, p + 32)? as usize;
        let mut lho = u32_at(bytes, p + 42)? as u64;
        let name_at = p + 46;
        let extra_at = name_at + nlen;
        let next = extra_at + elen + clen;
        if next > cd_end {
            return Err("zip central directory entry runs past its directory".into());
        }
        let name = String::from_utf8_lossy(&bytes[name_at..name_at + nlen]).into_owned();
        // zip64 extra field: values appear only for fields that were 0xFFFFFFFF.
        let extra = &bytes[extra_at..extra_at + elen];
        let mut q = 0;
        while q + 4 <= extra.len() {
            let id = u16::from_le_bytes([extra[q], extra[q + 1]]);
            let sz = u16::from_le_bytes([extra[q + 2], extra[q + 3]]) as usize;
            let body = extra.get(q + 4..q + 4 + sz).ok_or("corrupt zip extra field")?;
            if id == 1 {
                let mut r = 0;
                if usize_ == 0xFFFF_FFFF {
                    usize_ = u64_at(body, r)?;
                    r += 8;
                }
                if csize == 0xFFFF_FFFF {
                    csize = u64_at(body, r)?;
                    r += 8;
                }
                if lho == 0xFFFF_FFFF {
                    lho = u64_at(body, r)?;
                }
            }
            q += 4 + sz;
        }
        p = next;

        if flags & 1 != 0 {
            return Err(format!("zip member `{name}` is encrypted"));
        }
        total_uncompressed = total_uncompressed.saturating_add(usize_);
        if total_uncompressed > MAX_NPZ_TOTAL_BYTES {
            return Err(format!(
                "archive would expand to more than the {} GiB total limit",
                MAX_NPZ_TOTAL_BYTES >> 30
            ));
        }
        if usize_ > MAX_ARRAY_BYTES as u64 + (1 << 20) {
            return Err(format!("zip member `{name}` claims {usize_} bytes, over the size limit"));
        }
        let lho = lho as usize;
        if bytes.get(lho..lho.saturating_add(30)).map_or(true, |s| &s[..4] != b"PK\x03\x04") {
            return Err(format!("zip member `{name}`: bad local header offset"));
        }
        let lname = u16_at(bytes, lho + 26)? as usize;
        let lextra = u16_at(bytes, lho + 28)? as usize;
        let dstart = lho + 30 + lname + lextra;
        let dend = (dstart as u64)
            .checked_add(csize)
            .filter(|&e| e <= bytes.len() as u64)
            .ok_or_else(|| format!("zip member `{name}`: data extends past the end of the file"))?
            as usize;
        let raw = &bytes[dstart..dend];
        let data = match method {
            0 => {
                if csize != usize_ {
                    return Err(format!("zip member `{name}`: stored sizes disagree"));
                }
                raw.to_vec()
            }
            8 => inflate_capped(raw, usize_ as usize)
                .map_err(|e| format!("zip member `{name}`: {e}"))?,
            m => {
                return Err(format!(
                    "zip member `{name}` uses compression method {m}; only stored and DEFLATE are supported"
                ))
            }
        };
        if data.len() as u64 != usize_ {
            return Err(format!("zip member `{name}`: decompressed size differs from the header"));
        }
        if crc32(&data) != crc {
            return Err(format!("zip member `{name}`: CRC-32 mismatch (corrupt file)"));
        }
        out.push(ZipMember { name, data });
    }
    Ok(out)
}

/// Write a zip archive with every member "stored" (what `numpy.savez`
/// does). Refuses archives that would need ZIP64.
pub fn write_zip(members: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in members {
        if members.len() > 0xFFFE || data.len() >= 0xFFFF_FFFF || out.len() >= 0xFFFF_FFFF - (1 << 20) {
            return Err("archive too large (ZIP64 output is not supported)".into());
        }
        let crc = crc32(data);
        let off = out.len() as u32;
        let nb = name.as_bytes();
        // local header
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&33u16.to_le_bytes()); // date 1980-01-01
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(data);
        // central header
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&33u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(nb);
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// Read an `.npz`: every `*.npy` member, in archive order, keyed by name
/// without the `.npy` suffix.
pub fn read_npz(bytes: &[u8]) -> Result<Vec<(String, NpyArray)>, String> {
    let mut out = Vec::new();
    for m in read_zip(bytes)? {
        let Some(key) = m.name.strip_suffix(".npy") else { continue };
        let arr = read_npy(&m.data).map_err(|e| format!("member `{}`: {e}", m.name))?;
        out.push((key.to_string(), arr));
    }
    Ok(out)
}

/// Write an `.npz` (uncompressed, like `numpy.savez`).
pub fn write_npz(arrays: &[(String, NpyArray)]) -> Result<Vec<u8>, String> {
    let members: Vec<(String, Vec<u8>)> =
        arrays.iter().map(|(k, a)| (format!("{k}.npy"), write_npy(a))).collect();
    write_zip(&members)
}
