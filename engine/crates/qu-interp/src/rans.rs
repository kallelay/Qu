//! rANS (range Asymmetric Numeral Systems, Duda 2013) — the entropy coder
//! behind Zstandard/JPEG XL, over a STATIC two-pass order-0 byte-frequency
//! model. Kept in its own file, same reasoning as `huffman.rs`/
//! `range_coder.rs`/`inflate.rs`.
//!
//! Static (as opposed to `range_coder.rs`'s adaptive model) because rANS's
//! defining quirk makes an adaptive model awkward: **rANS encodes a
//! sequence back-to-front**. The coder is a LIFO stack (each step both
//! consumes and produces state in a way that only inverts cleanly in
//! reverse), so the FIRST symbol decoded is the LAST symbol encoded. An
//! adaptive model updated symbol-by-symbol would therefore need to see the
//! sequence in reverse during encoding but forward during decoding —
//! solvable, but a real added complication for a first implementation. A
//! static table (built by a single pass over the whole input before
//! encoding starts) sidesteps that entirely: both sides need the same
//! table, and unlike `range_coder.rs`'s adaptive model, it isn't derivable
//! from the data alone, so it has to be serialized alongside the encoded
//! bytes — the same shape `huffman.rs`'s `lengths` table plays there.
//!
//! Reference design: Fabian "ryg" Giesen's `ryg_rans` (the standard
//! from-scratch reference every real rANS implementation traces back to) —
//! byte-wise renormalization, a 32-bit state, `SCALE_BITS`-wide power-of-two
//! frequency totals (so `x / total`/`x % total` are enocding-time-free bit
//! ops on the DECODE side, though this implementation keeps the encode side
//! as plain division for clarity over the last few percent of speed).

const SCALE_BITS: u32 = 12;
const TOTAL: u32 = 1 << SCALE_BITS; // 4096 -- every symbol's frequency sums to exactly this
const RANS_L: u32 = 1 << 23; // lower renormalization bound; see module doc

/// A static order-0 model: `freq`/`cum` sum to exactly `TOTAL`, scaled from
/// real symbol counts by the largest-remainder method (plain proportional
/// scaling can round every frequency down to 0 for a rare symbol in a large
/// alphabet — largest-remainder guarantees every symbol that appeared at
/// all keeps a frequency of at least 1, so it stays encodable).
struct Model {
    freq: [u32; 256],
    cum: [u32; 257],
}

impl Model {
    fn from_data(data: &[u8]) -> Self {
        let mut counts = [0u64; 256];
        for &b in data {
            counts[b as usize] += 1;
        }
        let distinct = counts.iter().filter(|&&c| c > 0).count().max(1) as u32;
        let total_count: u64 = counts.iter().sum();

        let mut freq = [0u32; 256];
        let mut remainders: Vec<(u32, usize)> = Vec::new(); // (remainder_numerator, symbol)
        let mut assigned = 0u32;
        // Reserve at least 1 slot per distinct symbol up front (see doc
        // comment), scale the rest proportionally, and hand out the few
        // leftover slots (`TOTAL - assigned`) to whichever symbols' exact
        // scaling rounded down the most — the standard largest-remainder
        // fix-up so the frequencies sum to EXACTLY `TOTAL`, not
        // approximately.
        let scalable = TOTAL - distinct;
        for s in 0..256usize {
            if counts[s] == 0 {
                continue;
            }
            let exact = (counts[s] as u128) * (scalable as u128) / (total_count as u128);
            let f = 1 + exact as u32;
            freq[s] = f;
            assigned += f;
            let remainder = (counts[s] as u128) * (scalable as u128) % (total_count as u128);
            remainders.push((remainder as u32, s));
        }
        remainders.sort_by(|a, b| b.0.cmp(&a.0));
        let mut leftover = TOTAL - assigned;
        for &(_, s) in remainders.iter() {
            if leftover == 0 {
                break;
            }
            freq[s] += 1;
            leftover -= 1;
        }

        let mut cum = [0u32; 257];
        for s in 0..256 {
            cum[s + 1] = cum[s] + freq[s];
        }
        debug_assert_eq!(cum[256], TOTAL);
        Model { freq, cum }
    }

    fn from_lengths(freq: [u32; 256]) -> Self {
        let mut cum = [0u32; 257];
        for s in 0..256 {
            cum[s + 1] = cum[s] + freq[s];
        }
        Model { freq, cum }
    }

    fn symbol_at(&self, target: u32) -> u8 {
        for s in 0..256usize {
            if target < self.cum[s + 1] {
                return s as u8;
            }
        }
        255
    }
}

/// The static frequency table, serialized as a plain 256-entry array —
/// small enough (at most 256 * 4 bytes) not to bother packing tighter, and
/// simple enough that `decode` needs no separate parsing step.
pub struct Encoded {
    pub freq: [u32; 256],
    pub bytes: Vec<u8>,
    pub symbol_count: usize,
}

/// Encodes `data` via static-model rANS. Returns `None` for empty input,
/// same convention `huffman::encode` uses.
pub fn encode(data: &[u8]) -> Option<Encoded> {
    if data.is_empty() {
        return None;
    }
    let model = Model::from_data(data);
    let mut x: u32 = RANS_L;
    let mut out: Vec<u8> = Vec::new();

    // Back-to-front (see module doc) -- this is not a style choice, it is
    // what makes the forward-order decode below correct at all.
    for &b in data.iter().rev() {
        let freq = model.freq[b as usize];
        let cum = model.cum[b as usize];
        // Renormalize BEFORE updating state (the encode-side mirror of
        // decode's renormalize-after): push out whole bytes until `x`
        // divided by `freq` and rescaled back up by `TOTAL` would still fit
        // in 32 bits, i.e. until encoding this symbol cannot overflow.
        let x_max = ((RANS_L >> SCALE_BITS) << 8) * freq;
        while x >= x_max {
            out.push((x & 0xff) as u8);
            x >>= 8;
        }
        x = ((x / freq) << SCALE_BITS) + (x % freq) + cum;
    }
    // 4 bytes of final state, then the renormalization bytes -- written in
    // encode order here and corrected to true stream order by the reverse
    // below, rather than writing into a pre-sized buffer from its end (the
    // usual production trick): simpler to get right, at the cost of one
    // extra full-buffer pass.
    //
    // BIG-endian here, deliberately: `out.reverse()` reverses every byte's
    // POSITION, including within this 4-byte group, so pushing the state
    // little-endian would have `decode`'s `u32::from_le_bytes` read the
    // bytes back in the wrong order (byte-swapped) after that reversal.
    // Pushing big-endian means the reversal itself produces the correct
    // little-endian layout at the front of the final stream. (Caught by
    // an actual round-trip failure, not spotted by inspection -- exactly
    // why every codec here has a round-trip test, not just "compiles.")
    out.extend_from_slice(&x.to_be_bytes());
    out.reverse();

    Some(Encoded { freq: model.freq, bytes: out, symbol_count: data.len() })
}

/// Inverts [`encode`]: same static table (rebuilt from `freq`, not
/// re-derived from `bytes` — a static model has no way to recompute itself
/// from the compressed output alone, which is exactly why it has to travel
/// alongside it), decoding forward through `bytes` to recover `data` in its
/// ORIGINAL order despite `encode` having consumed it in reverse.
pub fn decode(freq: [u32; 256], bytes: &[u8], symbol_count: usize) -> Vec<u8> {
    let model = Model::from_lengths(freq);
    let mut pos = 4usize.min(bytes.len());
    let mut x = u32::from_le_bytes([
        bytes.first().copied().unwrap_or(0),
        bytes.get(1).copied().unwrap_or(0),
        bytes.get(2).copied().unwrap_or(0),
        bytes.get(3).copied().unwrap_or(0),
    ]);

    let mut out = Vec::with_capacity(symbol_count);
    for _ in 0..symbol_count {
        let slot = x & (TOTAL - 1);
        let symbol = model.symbol_at(slot);
        let f = model.freq[symbol as usize];
        let c = model.cum[symbol as usize];
        x = f * (x >> SCALE_BITS) + slot - c;
        while x < RANS_L {
            let byte = bytes.get(pos).copied().unwrap_or(0);
            pos += 1;
            x = (x << 8) | byte as u32;
        }
        out.push(symbol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let enc = encode(data).expect("non-empty input encodes");
        let dec = decode(enc.freq, &enc.bytes, enc.symbol_count);
        assert_eq!(dec, data, "round-trip mismatch for {} bytes", data.len());
    }

    #[test]
    fn empty_input_encodes_to_nothing() {
        assert!(encode(&[]).is_none());
    }

    #[test]
    fn round_trips_ordinary_text() {
        roundtrip(b"the quick brown fox jumps over the lazy dog, again and again and again");
    }

    #[test]
    fn round_trips_a_single_repeated_byte() {
        roundtrip(&[9u8; 3000]);
    }

    #[test]
    fn round_trips_two_symbols() {
        roundtrip(b"ababababababababab");
    }

    #[test]
    fn round_trips_all_256_byte_values() {
        let mut data = Vec::new();
        for _ in 0..10 {
            data.extend(0..=255u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn round_trips_a_single_byte_input() {
        roundtrip(&[200u8]);
    }

    #[test]
    fn round_trips_highly_skewed_frequencies_with_a_rare_symbol() {
        // Exercises the largest-remainder fix-up: `z` appears once in
        // 5001 bytes, rare enough that plain proportional scaling to a
        // 4096-total table would round it down to frequency 0.
        let mut data = vec![b'a'; 5000];
        data.push(b'z');
        roundtrip(&data);
    }

    #[test]
    fn compresses_skewed_data_below_its_original_size() {
        let mut data = vec![b'a'; 20_000];
        for i in 0..200 {
            data.push((b'b' + (i % 5)) as u8);
        }
        let enc = encode(&data).unwrap();
        // freq table overhead is fixed (256 * 4 bytes) regardless of input
        // size, so a large enough skewed input still compresses hard even
        // after accounting for it.
        assert!(
            enc.bytes.len() + 1024 < data.len() / 4,
            "expected real compression on skewed data: {} bytes vs {} original",
            enc.bytes.len(),
            data.len()
        );
    }

    #[test]
    fn round_trips_pseudo_random_incompressible_data() {
        let mut state = 0x9E3779B97F4A7C15u64;
        let mut data = Vec::with_capacity(4000);
        for _ in 0..4000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            data.push((state & 0xFF) as u8);
        }
        roundtrip(&data);
    }
}
