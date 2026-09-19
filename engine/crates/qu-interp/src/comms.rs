//! Digital-communications primitives: square Gray-coded QAM mapping, the
//! Hamming(7,4) single-error-correcting codec, and a width-generic CRC.
//!
//! Deliberately free of `Value` and of any engine type, so each piece can
//! be unit-tested as plain arithmetic. `lib.rs` owns the argument reading
//! and the `Value` shaping; everything here is `&[u8]` bits in, bits (or
//! `(re, im)` pairs) out.
//!
//! Bit-order convention, uniform across all three: a bit vector is
//! MSB-first, one 0/1 per element. That is how the course material and
//! every standards document writes a codeword, and mixing the two
//! conventions inside one module is the classic way for an encoder and
//! its own decoder to agree with each other while agreeing with nobody
//! else.

// ---------------------------------------------------------------- Gray

/// The Gray-code label a square constellation assigns to level index `i`.
///
/// Adjacent levels differ in exactly one label bit, which is the whole
/// point: a symbol that lands on the wrong side of a decision boundary
/// then costs one bit error, not `m`.
fn gray_encode(i: u32) -> u32 {
    i ^ (i >> 1)
}

/// Inverse of [`gray_encode`] -- recovers the level index from its label.
fn gray_decode(g: u32) -> u32 {
    let mut b = g;
    let mut shift = 1;
    while shift < 32 {
        b ^= b >> shift;
        shift <<= 1;
    }
    b
}

/// MSB-first bits to the integer they spell.
fn bits_to_u32(bits: &[u8]) -> u32 {
    bits.iter().fold(0u32, |acc, &b| (acc << 1) | u32::from(b))
}

/// `n` as `width` MSB-first bits, appended to `out`.
fn push_bits(out: &mut Vec<u8>, n: u32, width: usize) {
    for k in (0..width).rev() {
        out.push(((n >> k) & 1) as u8);
    }
}

// ----------------------------------------------------------------- QAM

/// The shape of one square QAM constellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QamParams {
    /// Bits per symbol, `log2(order)`.
    pub k: usize,
    /// Bits per axis, `k / 2`.
    pub m: usize,
    /// Amplitude levels per axis, `sqrt(order)`.
    pub levels: usize,
}

/// Validates `order` and derives the constellation's shape.
///
/// Square QAM needs `log2(order)` to be EVEN, so the bits split evenly
/// between the two axes -- 4, 16, 64, 256, ... Cross constellations (32,
/// 128) are a different geometry with a different labelling and are
/// rejected rather than silently approximated by a rectangle.
pub fn qam_params(order: u64) -> Result<QamParams, String> {
    if order < 4 || !order.is_power_of_two() {
        return Err(format!(
            "order must be a power of two of at least 4 (4, 16, 64, 256, ...), got {order}"
        ));
    }
    let k = order.trailing_zeros() as usize;
    if k % 2 != 0 {
        return Err(format!(
            "order {order} is not a square constellation: log2({order}) = {k} is odd, so the \
             bits do not split evenly between I and Q. Use 4, 16, 64, 256, ..."
        ));
    }
    Ok(QamParams {
        k,
        m: k / 2,
        levels: 1usize << (k / 2),
    })
}

/// Maps `bits` onto the order-`order` square Gray-coded constellation.
///
/// Each symbol takes `k = log2(order)` bits: the first `k/2` label the
/// in-phase level, the rest the quadrature level. Points sit on the
/// unnormalized odd-integer grid (`+-1`, `+-3`, ... for 16-QAM), the same
/// convention `qammod` uses by default -- no energy normalization, so the
/// mapping is exact in floating point and `qam_demodulate` inverts it
/// with no tolerance question to answer.
pub fn qam_modulate(bits: &[u8], order: u64) -> Result<Vec<(f64, f64)>, String> {
    let p = qam_params(order)?;
    if bits.len() % p.k != 0 {
        return Err(format!(
            "order-{order} QAM takes {} bits per symbol, so the bit count must be a multiple \
             of {}; got {}",
            p.k,
            p.k,
            bits.len()
        ));
    }
    let offset = p.levels as f64 - 1.0;
    Ok(bits
        .chunks(p.k)
        .map(|chunk| {
            let i = gray_decode(bits_to_u32(&chunk[..p.m]));
            let q = gray_decode(bits_to_u32(&chunk[p.m..]));
            (2.0 * f64::from(i) - offset, 2.0 * f64::from(q) - offset)
        })
        .collect())
}

/// Nearest-constellation-point decision, the inverse of [`qam_modulate`].
///
/// Because the constellation is a product of two independent PAM axes,
/// the nearest point is found per axis by rounding -- there is no need to
/// search all `order` points, and the two agree exactly.
pub fn qam_demodulate(symbols: &[(f64, f64)], order: u64) -> Result<Vec<u8>, String> {
    let p = qam_params(order)?;
    let offset = p.levels as f64 - 1.0;
    let top = p.levels as f64 - 1.0;
    let mut out = Vec::with_capacity(symbols.len() * p.k);
    for (idx, &(re, im)) in symbols.iter().enumerate() {
        if !re.is_finite() || !im.is_finite() {
            return Err(format!(
                "symbol {} is not finite, so it has no nearest constellation point",
                idx + 1
            ));
        }
        for axis in [re, im] {
            // `clamp` before `as u32`: a symbol far outside the
            // constellation (deep fade, wrong gain) must decide to the
            // edge point, not wrap through a negative cast.
            let level = (((axis + offset) / 2.0).round()).clamp(0.0, top) as u32;
            push_bits(&mut out, gray_encode(level), p.m);
        }
    }
    Ok(out)
}

// ------------------------------------------------------- Hamming(7,4)

/// What [`hamming74_decode`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hamming74Decoded {
    /// The recovered data bits, 4 per input codeword.
    pub data: Vec<u8>,
    /// Per codeword, the 1-indexed bit position that was flipped back, or
    /// 0 for a codeword that arrived with a zero syndrome.
    pub positions: Vec<usize>,
    /// True when at least one codeword needed a correction.
    pub corrected: bool,
}

/// Hamming(7,4) encoder, one 7-bit codeword per 4 data bits.
///
/// Parity layout is the classic one the lesson this ports uses:
/// `[p1, p2, d1, p4, d2, d3, d4]`, parity bits at the power-of-two
/// positions, so a nonzero syndrome reads out as the 1-indexed position
/// of the corrupted bit directly.
pub fn hamming74_encode(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() % 4 != 0 {
        return Err(format!(
            "hamming74_encode takes 4 data bits per codeword, so the bit count must be a \
             multiple of 4; got {}",
            data.len()
        ));
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 7);
    for d in data.chunks(4) {
        let p1 = d[0] ^ d[1] ^ d[3];
        let p2 = d[0] ^ d[2] ^ d[3];
        let p4 = d[1] ^ d[2] ^ d[3];
        out.extend_from_slice(&[p1, p2, d[0], p4, d[1], d[2], d[3]]);
    }
    Ok(out)
}

/// Hamming(7,4) decoder: corrects any single bit flip per codeword.
///
/// A double flip produces a nonzero syndrome too, and this -- like every
/// (7,4) decoder -- will "correct" it to the wrong codeword. The code's
/// minimum distance is 3: one error corrected, two detected only if you
/// do not also correct. That limit is the code's, not this port's.
pub fn hamming74_decode(codeword: &[u8]) -> Result<Hamming74Decoded, String> {
    if codeword.len() % 7 != 0 {
        return Err(format!(
            "hamming74_decode takes 7-bit codewords, so the bit count must be a multiple of 7; \
             got {}",
            codeword.len()
        ));
    }
    let mut data = Vec::with_capacity(codeword.len() / 7 * 4);
    let mut positions = Vec::with_capacity(codeword.len() / 7);
    let mut corrected = false;
    for block in codeword.chunks(7) {
        let mut c = [0u8; 7];
        c.copy_from_slice(block);
        let s1 = c[0] ^ c[2] ^ c[4] ^ c[6];
        let s2 = c[1] ^ c[2] ^ c[5] ^ c[6];
        let s4 = c[3] ^ c[4] ^ c[5] ^ c[6];
        let syndrome = (usize::from(s4) << 2) | (usize::from(s2) << 1) | usize::from(s1);
        if syndrome > 0 {
            c[syndrome - 1] ^= 1;
            corrected = true;
        }
        positions.push(syndrome);
        data.extend_from_slice(&[c[2], c[4], c[5], c[6]]);
    }
    Ok(Hamming74Decoded {
        data,
        positions,
        corrected,
    })
}

// ----------------------------------------------------------------- CRC

/// Checks a generator polynomial and returns its CRC width in bits.
fn crc_width(poly: &[u8]) -> Result<usize, String> {
    if poly.len() < 2 {
        return Err(format!(
            "a CRC generator polynomial needs at least 2 bits (leading 1 plus the width); got {}",
            poly.len()
        ));
    }
    if poly[0] != 1 {
        return Err(
            "a CRC generator polynomial must be written MSB-first with its leading 1 \
             included, e.g. x^3 + x + 1 as [1, 0, 1, 1] or as the number 11"
                .to_string(),
        );
    }
    Ok(poly.len() - 1)
}

/// Mod-2 long division of `buf` by `poly`, in place, leaving the
/// remainder in the trailing `poly.len() - 1` positions.
fn divide_mod2(buf: &mut [u8], poly: &[u8]) {
    let width = poly.len() - 1;
    if buf.len() < poly.len() {
        return;
    }
    for i in 0..(buf.len() - width) {
        if buf[i] == 1 {
            for (j, &p) in poly.iter().enumerate() {
                buf[i + j] ^= p;
            }
        }
    }
}

/// The CRC remainder of `bits` under generator `poly`, as `width` bits.
///
/// Textbook unreflected, zero-initialized polynomial division: the
/// message is shifted left by the CRC width and divided mod 2. It is NOT
/// any one named catalogue variant -- CRC-32 and friends additionally
/// specify an init value, input/output reflection and a final XOR, and
/// this takes the polynomial as data rather than hardcoding a width, so
/// the caller picks the geometry.
pub fn crc(bits: &[u8], poly: &[u8]) -> Result<Vec<u8>, String> {
    let width = crc_width(poly)?;
    let mut buf = Vec::with_capacity(bits.len() + width);
    buf.extend_from_slice(bits);
    buf.resize(bits.len() + width, 0);
    divide_mod2(&mut buf, poly);
    Ok(buf[bits.len()..].to_vec())
}

/// True when `codeword` (a message with its CRC already appended)
/// divides cleanly -- i.e. the remainder is all zeros.
pub fn crc_check(codeword: &[u8], poly: &[u8]) -> Result<bool, String> {
    let width = crc_width(poly)?;
    if codeword.len() < width {
        return Err(format!(
            "a codeword carrying a {width}-bit CRC needs at least {width} bits; got {}",
            codeword.len()
        ));
    }
    let mut buf = codeword.to_vec();
    divide_mod2(&mut buf, poly);
    Ok(buf[codeword.len() - width..].iter().all(|&b| b == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_round_trips_over_a_whole_axis() {
        for i in 0..256u32 {
            assert_eq!(gray_decode(gray_encode(i)), i);
        }
    }

    #[test]
    fn gray_neighbours_differ_in_one_bit() {
        for i in 0..255u32 {
            let d = gray_encode(i) ^ gray_encode(i + 1);
            assert_eq!(d.count_ones(), 1, "levels {i}/{} are not Gray-adjacent", i + 1);
        }
    }

    #[test]
    fn qam_rejects_non_square_and_tiny_orders() {
        for bad in [0u64, 1, 2, 3, 6, 32, 128] {
            assert!(qam_params(bad).is_err(), "order {bad} should be rejected");
        }
        for good in [4u64, 16, 64, 256] {
            assert!(qam_params(good).is_ok(), "order {good} should be accepted");
        }
    }

    /// Order 4 IS QPSK, so this pins the generalization against the
    /// two-line hand-rolled mapping it replaces: bit 0 to -1, bit 1 to +1
    /// on each axis.
    #[test]
    fn qpsk_agrees_with_the_hand_rolled_mapping() {
        let bits = [0u8, 0, 0, 1, 1, 0, 1, 1];
        let syms = qam_modulate(&bits, 4).unwrap();
        assert_eq!(
            syms,
            vec![(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
        );
    }

    #[test]
    fn qam_round_trips_every_symbol_of_every_supported_order() {
        for order in [4u64, 16, 64, 256] {
            let p = qam_params(order).unwrap();
            // Every point of the constellation, once.
            let mut bits = Vec::new();
            for s in 0..order as u32 {
                push_bits(&mut bits, s, p.k);
            }
            let syms = qam_modulate(&bits, order).unwrap();
            assert_eq!(syms.len(), order as usize);
            // Distinct points: a labelling bug that collapses two labels
            // onto one point still round-trips for one of them.
            let mut seen: Vec<(i64, i64)> =
                syms.iter().map(|&(a, b)| (a as i64, b as i64)).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), order as usize, "order {order} has colliding points");
            assert_eq!(qam_demodulate(&syms, order).unwrap(), bits, "order {order}");
        }
    }

    #[test]
    fn qam_decides_to_the_nearest_point_under_noise() {
        // 16-QAM, every point nudged by less than half the 2.0 spacing:
        // the decision must not move.
        let p = qam_params(16).unwrap();
        let mut bits = Vec::new();
        for s in 0..16u32 {
            push_bits(&mut bits, s, p.k);
        }
        let clean = qam_modulate(&bits, 16).unwrap();
        let noisy: Vec<(f64, f64)> = clean
            .iter()
            .map(|&(re, im)| (re + 0.4, im - 0.4))
            .collect();
        assert_eq!(qam_demodulate(&noisy, 16).unwrap(), bits);
    }

    #[test]
    fn qam_clamps_symbols_outside_the_constellation() {
        // Far outside on both axes: must decide to a corner, not wrap.
        let bits = qam_demodulate(&[(1e6, -1e6)], 16).unwrap();
        let back = qam_modulate(&bits, 16).unwrap();
        assert_eq!(back, vec![(3.0, -3.0)]);
    }

    #[test]
    fn qam_rejects_a_bit_count_that_is_not_a_whole_number_of_symbols() {
        assert!(qam_modulate(&[1, 0, 1], 16).is_err());
        assert!(qam_modulate(&[1, 0, 1, 1], 16).is_ok());
    }

    /// The verified reference from the lesson, transcribed as data:
    /// `p1 = d1^d2^d4`, `p2 = d1^d3^d4`, `p4 = d2^d3^d4`, laid out as
    /// `[p1, p2, d1, p4, d2, d3, d4]`.
    fn reference_encode(d: [u8; 4]) -> [u8; 7] {
        let p1 = d[0] ^ d[1] ^ d[3];
        let p2 = d[0] ^ d[2] ^ d[3];
        let p4 = d[1] ^ d[2] ^ d[3];
        [p1, p2, d[0], p4, d[1], d[2], d[3]]
    }

    #[test]
    fn hamming_encode_matches_the_reference_on_all_16_words() {
        for n in 0..16u8 {
            let d = [(n >> 3) & 1, (n >> 2) & 1, (n >> 1) & 1, n & 1];
            assert_eq!(
                hamming74_encode(&d).unwrap(),
                reference_encode(d).to_vec(),
                "data word {d:?}"
            );
        }
    }

    #[test]
    fn hamming_corrects_every_single_bit_flip_position() {
        for n in 0..16u8 {
            let d = [(n >> 3) & 1, (n >> 2) & 1, (n >> 1) & 1, n & 1];
            let clean = hamming74_encode(&d).unwrap();
            for pos in 0..7usize {
                let mut corrupt = clean.clone();
                corrupt[pos] ^= 1;
                let got = hamming74_decode(&corrupt).unwrap();
                assert_eq!(got.data, d.to_vec(), "word {d:?}, flip at {pos}");
                assert!(got.corrected, "word {d:?}, flip at {pos}: no correction reported");
                assert_eq!(
                    got.positions,
                    vec![pos + 1],
                    "word {d:?}: syndrome must name the 1-indexed flipped bit"
                );
            }
        }
    }

    #[test]
    fn hamming_reports_no_correction_on_a_clean_codeword() {
        for n in 0..16u8 {
            let d = [(n >> 3) & 1, (n >> 2) & 1, (n >> 1) & 1, n & 1];
            let got = hamming74_decode(&hamming74_encode(&d).unwrap()).unwrap();
            assert_eq!(got.data, d.to_vec());
            assert!(!got.corrected);
            assert_eq!(got.positions, vec![0]);
        }
    }

    #[test]
    fn hamming_handles_several_codewords_in_one_call() {
        let data = [1u8, 0, 1, 1, 0, 0, 1, 0];
        let mut enc = hamming74_encode(&data).unwrap();
        assert_eq!(enc.len(), 14);
        enc[9] ^= 1; // one flip, in the second codeword only
        let got = hamming74_decode(&enc).unwrap();
        assert_eq!(got.data, data.to_vec());
        assert!(got.corrected);
        assert_eq!(got.positions, vec![0, 3]);
    }

    #[test]
    fn hamming_rejects_bit_counts_that_are_not_whole_blocks() {
        assert!(hamming74_encode(&[1, 0, 1]).is_err());
        assert!(hamming74_decode(&[1, 0, 1, 1, 0, 1]).is_err());
    }

    /// Worked by hand: 1101 divided by 1011 (x^3 + x + 1) leaves 001.
    #[test]
    fn crc_matches_a_hand_worked_division() {
        let poly = [1u8, 0, 1, 1];
        assert_eq!(crc(&[1, 1, 0, 1], &poly).unwrap(), vec![0, 0, 1]);
    }

    #[test]
    fn crc_appended_makes_a_codeword_that_checks_out() {
        let poly = [1u8, 0, 0, 0, 1, 0, 0, 1, 1]; // CRC-8, x^8+x^2+x+1
        // A deterministic but non-alternating pattern: `(i*7+3) % 2` is
        // just `(i+1) % 2`, and a message that alternates every bit is
        // exactly the shape a shift-direction bug can survive.
        let mut lcg = 12345u32;
        let msg: Vec<u8> = (0..40)
            .map(|_| {
                lcg = lcg.wrapping_mul(1103515245).wrapping_add(12345);
                ((lcg >> 16) & 1) as u8
            })
            .collect();
        let r = crc(&msg, &poly).unwrap();
        assert_eq!(r.len(), 8);
        let mut codeword = msg.clone();
        codeword.extend_from_slice(&r);
        assert!(crc_check(&codeword, &poly).unwrap());
    }

    #[test]
    fn crc_catches_every_single_bit_error() {
        let poly = [1u8, 0, 1, 1];
        let msg = [1u8, 0, 1, 1, 0, 0, 1, 0, 1, 1];
        let mut codeword = msg.to_vec();
        codeword.extend_from_slice(&crc(&msg, &poly).unwrap());
        for pos in 0..codeword.len() {
            let mut bad = codeword.clone();
            bad[pos] ^= 1;
            assert!(
                !crc_check(&bad, &poly).unwrap(),
                "a single flip at {pos} passed the check"
            );
        }
    }

    #[test]
    fn crc_width_follows_the_polynomial() {
        let msg = [1u8, 1, 0, 1, 0, 0, 1];
        for poly in [
            vec![1u8, 1],
            vec![1, 0, 1, 1],
            vec![1, 0, 0, 0, 1, 0, 0, 1, 1],
            vec![1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
        ] {
            assert_eq!(crc(&msg, &poly).unwrap().len(), poly.len() - 1);
        }
    }

    #[test]
    fn crc_rejects_a_malformed_polynomial() {
        assert!(crc(&[1, 0], &[1]).is_err());
        assert!(crc(&[1, 0], &[0, 1, 1]).is_err());
    }
}
