//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression.
//!
//! Written here rather than pulled in as a dependency, for the same reason
//! `image.rs` writes its own zlib *stream*: this is a small, completely
//! specified, and permanently stable format, and the crate already avoids
//! optional-feature dependencies for anything it can carry itself.
//!
//! What needs it: MATLAB `.mat` Level 5 files store almost every variable
//! inside a `miCOMPRESSED` (type 15) element, which is a zlib stream. So
//! there is no reading a real `.mat` file without an inflater -- MATLAB
//! and `scipy.io.savemat` both compress by default.
//!
//! The decoder is the straightforward canonical-Huffman one (the shape of
//! zlib's own `puff.c` reference decoder): symbols are decoded bit by bit
//! against per-length counts rather than through a lookup table. That is
//! several times slower than a production inflater and completely
//! irrelevant here -- the largest file in the motivating data set is 13 MB,
//! and decoding is not what dominates reading it.

/// Reads bits LSB-first within each byte, which is the order DEFLATE uses
/// for everything *except* the Huffman codes themselves (those are packed
/// MSB-first within the code, and `decode` reassembles them accordingly).
struct BitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte to pull into the accumulator.
    pos: usize,
    /// Bits already pulled but not yet consumed, right-aligned.
    bits: u32,
    count: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, bits: 0, count: 0 }
    }

    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.count < n {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or_else(|| "compressed stream ended mid-symbol".to_string())?;
            self.pos += 1;
            self.bits |= (byte as u32) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bits & ((1u32 << n) - 1);
        self.bits >>= n;
        self.count -= n;
        Ok(v)
    }

    /// Drop the partial byte. A stored block always begins byte-aligned.
    fn align(&mut self) {
        let drop = self.count % 8;
        self.bits >>= drop;
        self.count -= drop;
    }

    /// Consume whole bytes, preferring any still sitting in the accumulator.
    fn take_bytes(&mut self, n: usize, out: &mut Vec<u8>) -> Result<(), String> {
        for _ in 0..n {
            if self.count >= 8 {
                out.push((self.bits & 0xff) as u8);
                self.bits >>= 8;
                self.count -= 8;
            } else {
                let byte = *self
                    .data
                    .get(self.pos)
                    .ok_or_else(|| "compressed stream ended inside a stored block".to_string())?;
                self.pos += 1;
                out.push(byte);
            }
        }
        Ok(())
    }
}

/// A canonical Huffman table, held as RFC 1951 describes it: how many codes
/// exist at each bit length, and the symbols in canonical order.
struct Huffman {
    /// `count[l]` = number of codes of length `l`; index 0 is unused.
    count: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    /// Build from a code-length-per-symbol list. Lengths of 0 mean "symbol
    /// not present", which is normal -- a dynamic block routinely omits
    /// most of the 288-symbol alphabet.
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut count = [0u16; 16];
        for &l in lengths {
            if l as usize >= 16 {
                return Err(format!("Huffman code length {l} exceeds the 15-bit maximum"));
            }
            count[l as usize] += 1;
        }
        // Length 0 is "absent", not a real code, so it plays no part in the
        // completeness check below.
        count[0] = 0;

        // Reject an over-subscribed table (more codes at some length than
        // the tree can hold). An *under*-subscribed one is allowed: a
        // distance table with a single used code is legal and appears in
        // real streams.
        let mut left = 1i32;
        for l in 1..16 {
            left <<= 1;
            left -= count[l] as i32;
            if left < 0 {
                return Err("over-subscribed Huffman table in compressed stream".into());
            }
        }

        // Canonical order: by code length, then by symbol value.
        let mut offs = [0u16; 16];
        for l in 1..15 {
            offs[l + 1] = offs[l] + count[l];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { count, symbols })
    }

    /// Walk the code lengths from short to long, accumulating bits MSB-first
    /// into `code`, until the value falls inside the range assigned to that
    /// length.
    fn decode(&self, r: &mut BitReader) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= r.take(1)? as i32;
            let count = self.count[len] as i32;
            if code - first < count {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("invalid Huffman code in compressed stream".into())
    }
}

// RFC 1951 §3.2.5. Length codes 257-285: base length and extra bits.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order dynamic blocks store the code-length alphabet's own lengths in
/// (RFC 1951 §3.2.7) -- chosen so the rarely-used entries cluster at the
/// end and can be omitted.
const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Decompress a raw DEFLATE stream (no zlib or gzip wrapper).
pub fn inflate(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut r = BitReader::new(input);
    let mut out: Vec<u8> = Vec::with_capacity(input.len() * 4);
    loop {
        let is_final = r.take(1)? == 1;
        match r.take(2)? {
            0 => {
                r.align();
                let mut hdr = Vec::with_capacity(4);
                r.take_bytes(4, &mut hdr)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]);
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
                if len != !nlen {
                    return Err("stored block length does not match its complement".into());
                }
                r.take_bytes(len as usize, &mut out)?;
            }
            1 => {
                // Fixed tables, RFC 1951 §3.2.6. Built per block rather
                // than cached: fixed blocks are rare in practice (both
                // MATLAB and zlib emit dynamic ones for anything but tiny
                // input) and 320 bytes of setup is not worth a static.
                let mut lit_lengths = [0u8; 288];
                for (i, l) in lit_lengths.iter_mut().enumerate() {
                    *l = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let lit = Huffman::new(&lit_lengths)?;
                let dist = Huffman::new(&[5u8; 30])?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            2 => {
                let hlit = r.take(5)? as usize + 257;
                let hdist = r.take(5)? as usize + 1;
                let hclen = r.take(4)? as usize + 4;

                let mut cl_lengths = [0u8; 19];
                for &slot in CODE_LENGTH_ORDER.iter().take(hclen) {
                    cl_lengths[slot] = r.take(3)? as u8;
                }
                let cl = Huffman::new(&cl_lengths)?;

                // The literal/length and distance code lengths are stored
                // as one run-length-coded sequence spanning both tables.
                let mut lengths = vec![0u8; hlit + hdist];
                let mut i = 0;
                while i < lengths.len() {
                    let sym = cl.decode(&mut r)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            // Repeat the previous length 3-6 times.
                            let prev = *lengths
                                .get(i.wrapping_sub(1))
                                .ok_or("code-length repeat with no previous length")?;
                            let n = 3 + r.take(2)? as usize;
                            for _ in 0..n {
                                if i >= lengths.len() {
                                    return Err("code-length repeat runs past the table".into());
                                }
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 | 18 => {
                            let n = if sym == 17 {
                                3 + r.take(3)? as usize
                            } else {
                                11 + r.take(7)? as usize
                            };
                            if i + n > lengths.len() {
                                return Err("zero-length run runs past the table".into());
                            }
                            i += n; // already zero
                        }
                        _ => return Err(format!("invalid code-length symbol {sym}")),
                    }
                }

                let lit = Huffman::new(&lengths[..hlit])?;
                let dist = Huffman::new(&lengths[hlit..])?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            _ => return Err("reserved DEFLATE block type 3".into()),
        }
        if is_final {
            return Ok(out);
        }
    }
}

/// Decode one Huffman-coded block's symbols into `out`.
fn inflate_block(
    r: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let idx = sym as usize - 257;
                let len = LENGTH_BASE[idx] as usize + r.take(LENGTH_EXTRA[idx] as u32)? as usize;
                let dsym = dist.decode(r)? as usize;
                if dsym >= 30 {
                    return Err(format!("invalid distance symbol {dsym}"));
                }
                let d = DIST_BASE[dsym] as usize + r.take(DIST_EXTRA[dsym] as u32)? as usize;
                if d > out.len() {
                    return Err("back-reference points before the start of the output".into());
                }
                // Byte at a time on purpose: an overlapping copy (d < len)
                // is legal and common -- it is how DEFLATE encodes runs --
                // so the bytes being read may be ones this loop just wrote.
                let start = out.len() - d;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return Err(format!("invalid literal/length symbol {sym}")),
        }
    }
}

/// Adler-32 (RFC 1950 §9), for verifying an inflated zlib stream.
fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

/// Decompress a zlib stream (RFC 1950): 2-byte header, DEFLATE data,
/// Adler-32 trailer.
pub fn zlib_decompress(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() < 6 {
        return Err("zlib stream is too short to contain a header and checksum".into());
    }
    let cmf = input[0];
    let flg = input[1];
    if cmf & 0x0f != 8 {
        return Err(format!(
            "zlib compression method {} is not DEFLATE",
            cmf & 0x0f
        ));
    }
    if (((cmf as u16) << 8) | flg as u16) % 31 != 0 {
        return Err("zlib header check bits are wrong (not a zlib stream?)".into());
    }
    if flg & 0x20 != 0 {
        // Only ever set by a compressor told to use a shared dictionary,
        // which nothing writing a .mat file does.
        return Err("zlib stream needs a preset dictionary, which is not supported".into());
    }
    let out = inflate(&input[2..])?;

    // The trailer is the last four bytes of the *stream*, which is not
    // necessarily the last four bytes of `input` -- a .mat element is
    // padded to an 8-byte boundary, so there is often trailing slack. The
    // checksum still has to be found and verified: a silently truncated
    // read here would surface later as nonsense numbers in a figure.
    let tail = &input[2..];
    let mut verified = false;
    let expect = adler32(&out);
    for end in (4..=tail.len()).rev() {
        let c = u32::from_be_bytes([tail[end - 4], tail[end - 3], tail[end - 2], tail[end - 1]]);
        if c == expect {
            verified = true;
            break;
        }
        // Only the padding can follow the checksum, so give up quickly
        // rather than scanning the whole stream backwards.
        if tail.len() - end > 8 {
            break;
        }
    }
    if !verified {
        return Err("zlib checksum mismatch -- the compressed data is corrupt".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one stream shape this repo already produces: `image.rs` writes
    /// PNG payloads as zlib stored blocks. Round-tripping it proves the
    /// stored-block path and the Adler-32 agree with the writer.
    #[test]
    fn reads_back_a_stored_block_stream_written_by_the_png_encoder() {
        // Same construction as `image::zlib_stored`, inlined so this test
        // does not depend on a private function in another module.
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut z = vec![0x78, 0x01, 1];
        z.extend_from_slice(&(data.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
        z.extend_from_slice(&data);
        z.extend_from_slice(&adler32(&data).to_be_bytes());
        assert_eq!(zlib_decompress(&z).unwrap(), data);
    }

    /// A fixed-Huffman stream with an *overlapping* back-reference:
    /// "aaaaaaaa" is a literal 'a' then a length-7 distance-1 copy, so the
    /// bytes being read are ones the same loop just wrote. A `copy_within`
    /// or a slice-then-extend gets this wrong; copying a byte at a time is
    /// why the decoder does it that way.
    #[test]
    fn decodes_an_overlapping_back_reference() {
        let z: &[u8] = &[0x78, 0xda, 0x4b, 0x4c, 0x84, 0x00, 0x00, 0x0d, 0xac, 0x03, 0x09];
        assert_eq!(zlib_decompress(z).unwrap(), b"aaaaaaaa");
    }

    /// A fixed-Huffman block with several distinct back-reference lengths.
    #[test]
    fn decodes_a_fixed_huffman_block() {
        let z: &[u8] = &[
            0x78, 0x9c, 0x2b, 0xc9, 0x48, 0x55, 0x28, 0x2c, 0xcd, 0x4c, 0xce, 0x56, 0x48, 0x2a,
            0xca, 0x2f, 0xcf, 0x53, 0x48, 0xcb, 0xaf, 0x50, 0xc8, 0x2a, 0xcd, 0x2d, 0x28, 0x56,
            0xc8, 0x2f, 0x4b, 0x2d, 0x52, 0x28, 0xc9, 0x48, 0x55, 0xc8, 0x49, 0xac, 0xaa, 0x54,
            0x48, 0xc9, 0x4f, 0x07, 0x73, 0x46, 0xd5, 0x8e, 0xaa, 0x1d, 0x55, 0x3b, 0xaa, 0x76,
            0x54, 0xed, 0xa8, 0xda, 0xfc, 0x21, 0xa0, 0x16, 0x00, 0x4c, 0x80, 0x84, 0x07,
        ];
        let expected = "the quick brown fox jumps over the lazy dog ".repeat(40);
        assert_eq!(zlib_decompress(z).unwrap(), expected.as_bytes());
    }

    /// A dynamic-Huffman block -- the one that needs the code-length
    /// alphabet, its own permuted storage order, and the 16/17/18
    /// run-length symbols. This is what a real `.mat` file uses, so a
    /// decoder that handled only stored and fixed blocks would pass every
    /// other test here and still read nothing.
    #[test]
    fn decodes_a_dynamic_huffman_block() {
        let z: &[u8] = &[
            0x78, 0xda, 0x9d, 0xce, 0x8d, 0x09, 0xc4, 0x20, 0x0c, 0x40, 0xe1, 0xd9, 0x12, 0xc5,
            0x9f, 0x90, 0xd2, 0x1a, 0x28, 0x1a, 0x45, 0x41, 0xaa, 0xdd, 0x7f, 0x84, 0xbb, 0x5b,
            0xe1, 0xde, 0xb7, 0xc0, 0x03, 0x00, 0x34, 0xd6, 0x05, 0xe2, 0x53, 0x6e, 0xed, 0x73,
            0xa3, 0x67, 0x29, 0x63, 0x19, 0x4a, 0x3a, 0x21, 0x4a, 0x5b, 0xee, 0xaa, 0xcb, 0x4b,
            0x87, 0x43, 0x37, 0xe5, 0x45, 0xe5, 0xe5, 0x86, 0x69, 0x46, 0x05, 0x79, 0x78, 0x84,
            0x6a, 0x33, 0xde, 0xf0, 0x85, 0xd9, 0xd6, 0x30, 0xf8, 0x11, 0xd0, 0x38, 0x13, 0x36,
            0x7e, 0x0b, 0xad, 0x4c, 0x5b, 0x0f, 0xe8, 0xe2, 0x57, 0xbd, 0xdc, 0x6a, 0x12, 0x61,
            0x6a, 0x22, 0xb3, 0x46, 0x11, 0xf6, 0xb8, 0x67, 0xd7, 0x5b, 0x4e, 0xa6, 0xe0, 0xac,
            0x41, 0xf8, 0xf5, 0xf7, 0xcb, 0x07, 0xd9, 0x21, 0x3f, 0x74,
        ];
        let expected: Vec<u8> = (0..200u32).map(|i| ((i * i / 7) % 40 + 65) as u8).collect();
        assert_eq!(zlib_decompress(z).unwrap(), expected);
    }

    #[test]
    fn rejects_a_stream_that_is_not_zlib() {
        assert!(zlib_decompress(b"not a zlib stream at all").is_err());
    }

    #[test]
    fn rejects_a_truncated_stream_rather_than_returning_partial_data() {
        let data: Vec<u8> = (0..500u32).map(|i| (i % 251) as u8).collect();
        let mut z = vec![0x78, 0x01, 1];
        z.extend_from_slice(&(data.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
        z.extend_from_slice(&data[..200]);
        assert!(zlib_decompress(&z).is_err());
    }

    #[test]
    fn adler32_matches_the_standard_worked_example() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}
