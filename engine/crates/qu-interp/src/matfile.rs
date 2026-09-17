//! Reading MATLAB Level 5 `.mat` files.
//!
//! The format is documented in MathWorks' "MAT-File Format" guide, and is
//! simple enough to read directly: a 128-byte text header, then a flat
//! sequence of tagged data elements. Every variable is an `miMATRIX`
//! element, which itself contains sub-elements (flags, dimensions, name,
//! then the data). In practice almost every one of those is wrapped in an
//! `miCOMPRESSED` element -- a zlib stream -- because both MATLAB's `save`
//! and `scipy.io.savemat` compress by default. See [`crate::inflate`].
//!
//! **Level 5 only.** MATLAB v7.3 files are HDF5 containers with a
//! completely different structure; they announce themselves in the header
//! text and are rejected with a message saying so rather than being
//! misparsed. That is a real limit and worth stating plainly: a v7.3 file
//! needs the HDF5 path, not this one.
//!
//! Struct and cell arrays are supported because real stored results use
//! them -- the file this was written for keeps everything under a
//! `config` and a `results` struct, and reading only plain matrices would
//! not have got a single number out of it.

use crate::inflate::zlib_decompress;

/// A value read out of a `.mat` file, before it is turned into a
/// `Value`. Kept separate from the interpreter's own type so this module
/// stays a plain format reader.
#[derive(Debug, Clone)]
pub enum MatValue {
    /// Numeric or logical array, in MATLAB's column-major order, with its
    /// dimensions. A scalar is a 1x1.
    Numeric { dims: Vec<usize>, data: Vec<f64> },
    /// Complex numeric array: real and imaginary parts, same length.
    Complex { dims: Vec<usize>, re: Vec<f64>, im: Vec<f64> },
    /// `mxCHAR` -- MATLAB stores strings as char arrays.
    Str(String),
    /// `mxSTRUCT`. Field order is the file's order, which is the order the
    /// script that wrote it used, so it is worth preserving.
    ///
    /// A struct *array* (dims other than 1x1) is flattened to its first
    /// element, matching what `scipy.io.loadmat(...)[name][0, 0]` gets you
    /// and what the scripts being ported actually index.
    Struct(Vec<(String, MatValue)>),
    /// `mxCELL`, in column-major order.
    Cell { dims: Vec<usize>, items: Vec<MatValue> },
    /// A class this reader does not decode (function handles, objects,
    /// sparse arrays). Carried as a named placeholder rather than an error
    /// so one exotic field cannot make a whole file unreadable.
    Unsupported(&'static str),
}

impl MatValue {
    /// Total element count implied by the dimensions.
    fn numel(dims: &[usize]) -> usize {
        dims.iter().product::<usize>().max(0)
    }
}

// --- MAT data types (the `miXXX` tag values) -----------------------------
const MI_INT8: u32 = 1;
const MI_UINT8: u32 = 2;
const MI_INT16: u32 = 3;
const MI_UINT16: u32 = 4;
const MI_INT32: u32 = 5;
const MI_UINT32: u32 = 6;
const MI_SINGLE: u32 = 7;
const MI_DOUBLE: u32 = 9;
const MI_INT64: u32 = 12;
const MI_UINT64: u32 = 13;
const MI_MATRIX: u32 = 14;
const MI_COMPRESSED: u32 = 15;
const MI_UTF8: u32 = 16;
const MI_UTF16: u32 = 17;
const MI_UTF32: u32 = 18;

// --- Array classes (the low byte of the array flags) ---------------------
const MX_CELL: u8 = 1;
const MX_STRUCT: u8 = 2;
const MX_OBJECT: u8 = 3;
const MX_CHAR: u8 = 4;
const MX_SPARSE: u8 = 5;
const MX_DOUBLE: u8 = 6;
const MX_SINGLE: u8 = 7;
const MX_INT8: u8 = 8;
const MX_UINT8: u8 = 9;
const MX_INT16: u8 = 10;
const MX_UINT16: u8 = 11;
const MX_INT32: u8 = 12;
const MX_UINT32: u8 = 13;
const MX_INT64: u8 = 14;
const MX_UINT64: u8 = 15;

/// One tagged element: its type, and the bytes of its payload.
struct Element<'a> {
    kind: u32,
    data: &'a [u8],
    /// Bytes consumed from the source, including the tag and any padding.
    consumed: usize,
}

/// Read one element's tag and payload at `data[at..]`.
///
/// Handles the "small data element" form: when the upper 16 bits of the
/// first word are non-zero, the element is <= 4 bytes and its tag is
/// packed into a single word rather than two. Missing this makes every
/// short field (a 1x1 flag, a 2-element dimension array) read as garbage.
fn read_element(data: &[u8], at: usize) -> Result<Element<'_>, String> {
    if at + 4 > data.len() {
        return Err("truncated .mat file: an element tag runs past the end".into());
    }
    let word = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    if word >> 16 != 0 {
        // Small element: [size:u16][type:u16][up to 4 bytes of payload].
        let kind = word & 0xffff;
        let size = (word >> 16) as usize;
        if size > 4 || at + 8 > data.len() {
            return Err("malformed small data element in .mat file".into());
        }
        return Ok(Element { kind, data: &data[at + 4..at + 4 + size], consumed: 8 });
    }
    if at + 8 > data.len() {
        return Err("truncated .mat file: an element header runs past the end".into());
    }
    let size = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
    let start = at + 8;
    if start + size > data.len() {
        return Err(format!(
            "truncated .mat file: an element claims {size} bytes but only {} remain",
            data.len() - start
        ));
    }
    // Every element is padded to an 8-byte boundary, except a compressed
    // one, whose payload length is exact.
    let padded = if word == MI_COMPRESSED { size } else { (size + 7) / 8 * 8 };
    Ok(Element { kind: word, data: &data[start..start + size], consumed: 8 + padded })
}

/// Decode a numeric element's payload to `f64`, whatever integer or float
/// type it was stored as.
fn numeric_payload(kind: u32, data: &[u8]) -> Result<Vec<f64>, String> {
    fn chunks<const N: usize>(d: &[u8]) -> impl Iterator<Item = [u8; N]> + '_ {
        d.chunks_exact(N).map(|c| {
            let mut a = [0u8; N];
            a.copy_from_slice(c);
            a
        })
    }
    Ok(match kind {
        MI_INT8 => data.iter().map(|&b| b as i8 as f64).collect(),
        MI_UINT8 => data.iter().map(|&b| b as f64).collect(),
        MI_INT16 => chunks::<2>(data).map(|a| i16::from_le_bytes(a) as f64).collect(),
        MI_UINT16 => chunks::<2>(data).map(|a| u16::from_le_bytes(a) as f64).collect(),
        MI_INT32 => chunks::<4>(data).map(|a| i32::from_le_bytes(a) as f64).collect(),
        MI_UINT32 => chunks::<4>(data).map(|a| u32::from_le_bytes(a) as f64).collect(),
        MI_SINGLE => chunks::<4>(data).map(|a| f32::from_le_bytes(a) as f64).collect(),
        MI_DOUBLE => chunks::<8>(data).map(f64::from_le_bytes).collect(),
        // i64/u64 past 2^53 cannot round-trip through f64. Real stored
        // results use them for counts and indices, which are far below
        // that, so converting is right -- but silently losing precision
        // would not be, hence the check.
        MI_INT64 => chunks::<8>(data)
            .map(|a| {
                let v = i64::from_le_bytes(a);
                if v.unsigned_abs() > (1u64 << 53) {
                    Err(format!("int64 value {v} cannot be represented exactly as a number"))
                } else {
                    Ok(v as f64)
                }
            })
            .collect::<Result<Vec<f64>, String>>()?,
        MI_UINT64 => chunks::<8>(data)
            .map(|a| {
                let v = u64::from_le_bytes(a);
                if v > (1u64 << 53) {
                    Err(format!("uint64 value {v} cannot be represented exactly as a number"))
                } else {
                    Ok(v as f64)
                }
            })
            .collect::<Result<Vec<f64>, String>>()?,
        other => return Err(format!("unsupported numeric data type {other} in .mat file")),
    })
}

/// Decode a char element's payload to a `String`.
fn char_payload(kind: u32, data: &[u8]) -> Result<String, String> {
    Ok(match kind {
        // MATLAB writes plain ASCII as uint16 more often than not.
        MI_UINT16 | MI_INT16 | MI_UTF16 => {
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        MI_UTF8 | MI_UINT8 | MI_INT8 => String::from_utf8_lossy(data).into_owned(),
        MI_UTF32 => data
            .chunks_exact(4)
            .filter_map(|c| char::from_u32(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect(),
        other => return Err(format!("unsupported char encoding {other} in .mat file")),
    })
}

/// Parse one `miMATRIX` payload. Returns the variable's name (empty for a
/// nested array, which carries no name of its own) and its value.
fn parse_matrix(body: &[u8]) -> Result<(String, MatValue), String> {
    let mut at = 0usize;

    // 1. Array flags: two uint32s. Byte 0 of the first is the class, byte
    //    1 the flags (bit 3 = complex).
    let flags = read_element(body, at)?;
    if flags.data.len() < 8 {
        return Err("array flags element is too short".into());
    }
    let class = flags.data[0];
    let is_complex = flags.data[1] & 0x08 != 0;
    at += flags.consumed;

    // 2. Dimensions: an int32 array.
    let dims_el = read_element(body, at)?;
    let dims: Vec<usize> = numeric_payload(dims_el.kind, dims_el.data)?
        .into_iter()
        .map(|d| d.max(0.0) as usize)
        .collect();
    at += dims_el.consumed;

    // 3. Name.
    let name_el = read_element(body, at)?;
    let name = String::from_utf8_lossy(name_el.data).into_owned();
    at += name_el.consumed;

    let value = match class {
        MX_CHAR => {
            let el = read_element(body, at)?;
            MatValue::Str(char_payload(el.kind, el.data)?)
        }
        MX_DOUBLE | MX_SINGLE | MX_INT8 | MX_UINT8 | MX_INT16 | MX_UINT16 | MX_INT32
        | MX_UINT32 | MX_INT64 | MX_UINT64 => {
            let re_el = read_element(body, at)?;
            let re = numeric_payload(re_el.kind, re_el.data)?;
            if is_complex {
                at += re_el.consumed;
                let im_el = read_element(body, at)?;
                let im = numeric_payload(im_el.kind, im_el.data)?;
                MatValue::Complex { dims, re, im }
            } else {
                MatValue::Numeric { dims, data: re }
            }
        }
        // Logical arrays reuse a numeric class with the logical flag set;
        // they land in the arm above and read back as 0.0/1.0, which is
        // what every consumer here wants.
        MX_CELL => {
            let n = MatValue::numel(&dims);
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                let el = read_element(body, at)?;
                at += el.consumed;
                if el.kind != MI_MATRIX {
                    return Err("cell array contains a non-matrix element".into());
                }
                items.push(parse_matrix(el.data)?.1);
            }
            MatValue::Cell { dims, items }
        }
        MX_STRUCT => {
            // Field names come as one fixed-width char blob, preceded by
            // the width. Names are NUL-padded to that width.
            let width_el = read_element(body, at)?;
            let width = numeric_payload(width_el.kind, width_el.data)?
                .first()
                .copied()
                .unwrap_or(0.0) as usize;
            at += width_el.consumed;
            if width == 0 {
                return Err("struct field-name width is zero".into());
            }
            let names_el = read_element(body, at)?;
            at += names_el.consumed;
            let names: Vec<String> = names_el
                .data
                .chunks(width)
                .map(|c| {
                    let end = c.iter().position(|&b| b == 0).unwrap_or(c.len());
                    String::from_utf8_lossy(&c[..end]).into_owned()
                })
                .collect();

            // For a struct array the fields repeat per element. Only the
            // first element is kept -- see `MatValue::Struct`.
            let n_elems = MatValue::numel(&dims).max(1);
            let mut fields = Vec::with_capacity(names.len());
            for elem in 0..n_elems {
                for name in &names {
                    let el = read_element(body, at)?;
                    at += el.consumed;
                    if elem == 0 {
                        let v = if el.kind == MI_MATRIX {
                            parse_matrix(el.data)?.1
                        } else {
                            MatValue::Unsupported("field")
                        };
                        fields.push((name.clone(), v));
                    }
                }
            }
            MatValue::Struct(fields)
        }
        MX_OBJECT => MatValue::Unsupported("object"),
        MX_SPARSE => MatValue::Unsupported("sparse array"),
        other => return Err(format!("unsupported array class {other} in .mat file")),
    };
    Ok((name, value))
}

/// Read every top-level variable from a Level 5 `.mat` file's bytes.
///
/// Returns them in file order, which is the order they were saved in.
pub fn read_mat(bytes: &[u8]) -> Result<Vec<(String, MatValue)>, String> {
    if bytes.len() < 128 {
        return Err("not a .mat file: shorter than the 128-byte header".into());
    }
    let header = String::from_utf8_lossy(&bytes[..116]);
    if !header.contains("MATLAB") {
        return Err("not a .mat file: the header does not identify one".into());
    }
    if header.contains("7.3") {
        return Err(
            "this is a MATLAB v7.3 file, which is an HDF5 container, not a Level 5 .mat file -- \
             re-save it from MATLAB with `save(..., '-v7')`, or read it through the HDF5 path"
                .into(),
        );
    }
    // Bytes 126-127 are the endian indicator: 'IM' little-endian, 'MI'
    // big-endian. Everything that writes these files today is
    // little-endian; a big-endian file would need every read swapped, so
    // say so rather than return transposed nonsense.
    if &bytes[126..128] != b"IM" {
        return Err("big-endian .mat files are not supported".into());
    }

    let mut out = Vec::new();
    let mut at = 128usize;
    while at + 8 <= bytes.len() {
        let el = read_element(bytes, at)?;
        at += el.consumed;
        match el.kind {
            MI_COMPRESSED => {
                let raw = zlib_decompress(el.data)?;
                // A compressed element holds exactly one matrix.
                let inner = read_element(&raw, 0)?;
                if inner.kind == MI_MATRIX {
                    out.push(parse_matrix(inner.data)?);
                }
            }
            MI_MATRIX => out.push(parse_matrix(el.data)?),
            // Anything else at top level (a subsystem offset block, say)
            // is not a variable; skipping it is correct, not a failure.
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header gate has to reject v7.3 by name, because those files
    /// otherwise reach the element loop and fail with a confusing
    /// "truncated file" -- the HDF5 signature parses as a nonsense tag.
    #[test]
    fn names_v73_as_the_problem_instead_of_failing_obscurely() {
        let mut b = vec![0u8; 128];
        let msg = b"MATLAB 7.3 MAT-file, Platform: PCWIN64, Created on: today";
        b[..msg.len()].copy_from_slice(msg);
        b[126] = b'I';
        b[127] = b'M';
        let err = read_mat(&b).unwrap_err();
        assert!(err.contains("v7.3"), "{err}");
        assert!(err.contains("HDF5"), "{err}");
    }

    #[test]
    fn rejects_a_file_that_is_not_a_mat_file() {
        let b = vec![b'x'; 200];
        assert!(read_mat(&b).unwrap_err().contains("not a .mat file"));
    }

    #[test]
    fn rejects_a_file_shorter_than_the_header() {
        assert!(read_mat(b"MATLAB").unwrap_err().contains("128-byte header"));
    }

    /// The small-data-element form packs the tag into one word. Getting it
    /// wrong reads every short field as garbage, so it is worth pinning
    /// directly.
    #[test]
    fn reads_the_small_data_element_form() {
        // [size=2 | type=MI_INT32] then 2 bytes of payload.
        let mut b = Vec::new();
        b.extend_from_slice(&((2u32 << 16) | MI_INT32).to_le_bytes());
        b.extend_from_slice(&[7, 0, 0, 0]);
        let el = read_element(&b, 0).unwrap();
        assert_eq!(el.kind, MI_INT32);
        assert_eq!(el.data, &[7, 0]);
        assert_eq!(el.consumed, 8);
    }

    #[test]
    fn decodes_every_numeric_width_to_the_same_values() {
        assert_eq!(numeric_payload(MI_INT8, &[255, 1]).unwrap(), vec![-1.0, 1.0]);
        assert_eq!(numeric_payload(MI_UINT8, &[255, 1]).unwrap(), vec![255.0, 1.0]);
        assert_eq!(numeric_payload(MI_INT16, &[255, 255]).unwrap(), vec![-1.0]);
        assert_eq!(numeric_payload(MI_DOUBLE, &1.5f64.to_le_bytes()).unwrap(), vec![1.5]);
        assert_eq!(numeric_payload(MI_SINGLE, &2.5f32.to_le_bytes()).unwrap(), vec![2.5]);
    }

    /// Integers past 2^53 do not survive the trip through f64, and quietly
    /// rounding one would corrupt a result rather than fail.
    #[test]
    fn refuses_an_int64_too_large_to_represent_exactly() {
        let big = (1i64 << 60).to_le_bytes();
        assert!(numeric_payload(MI_INT64, &big).unwrap_err().contains("exactly"));
    }
}
