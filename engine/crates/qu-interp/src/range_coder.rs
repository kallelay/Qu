//! Byte-oriented range coding (a practical, carry-free arithmetic coder)
//! over an adaptive order-0 byte-frequency model. Kept in its own file, same
//! "minimize collision with concurrent `lib.rs` work" reasoning as
//! `huffman.rs`/`inflate.rs`/`fs_ops.rs`.
//!
//! Range coding is arithmetic coding's practical form: instead of tracking
//! an interval as arbitrary-precision fractions, it tracks a `u32` `low`/
//! `range` pair and renormalizes by shifting out whole bytes once the
//! interval narrows below a threshold — the same idea LZMA's own range
//! coder uses. This implementation uses Dmitry Subbotin's CARRY-FREE
//! renormalization rule specifically (`TOP`/`BOTTOM` thresholds below):
//! plain arithmetic coding can produce a carry that propagates backward
//! through already-emitted bytes, which either needs a byte-stuffing
//! scheme or an explicit carry-counter to handle correctly. The carry-free
//! variant sacrifices a small, bounded amount of compression ratio (it
//! forces a renormalization slightly early whenever `low`/`high` disagree
//! in their top byte) in exchange for never needing to look backward at
//! already-written output — worth it here since correctness matters far
//! more than squeezing out the last fraction of a percent.
//!
//! "Adaptive order-0" means there is no separate frequency table to ship
//! alongside the encoded bytes at all (unlike `huffman.rs`, which must send
//! its code-length table): encoder and decoder both start from a flat
//! (all-symbols-equally-likely) model and update it identically after each
//! symbol, so they never diverge without any side channel — the standard
//! technique real adaptive arithmetic coders use.

const TOP: u32 = 1 << 24;
const BOTTOM: u32 = 1 << 16;
/// Frequencies are rescaled before they could push `total` past this, so
/// `range / total` (used every symbol) never underflows to 0 even at
/// `range`'s minimum post-renormalization value (`BOTTOM`).
const MAX_TOTAL: u32 = BOTTOM;

/// Adaptive order-0 byte-frequency model, shared verbatim between encoder
/// and decoder (see module doc for why that's what makes this "adaptive"
/// rather than needing a transmitted table). Every symbol starts at
/// frequency 1 (never 0 — a symbol the model has never seen must still be
/// encodable, just expensively, or the coder could never emit a byte it
/// hadn't already observed at least once).
struct Model {
    freq: [u32; 256],
    cum: [u32; 257],
    total: u32,
}

impl Model {
    fn new() -> Self {
        let mut cum = [0u32; 257];
        for i in 0..=256 {
            cum[i] = i as u32;
        }
        Model { freq: [1; 256], cum, total: 256 }
    }

    /// `(cum_freq_before, freq, total)` for `symbol` — exactly what the
    /// range coder needs to narrow its interval to this symbol's slice.
    fn interval(&self, symbol: u8) -> (u32, u32, u32) {
        (self.cum[symbol as usize], self.freq[symbol as usize], self.total)
    }

    /// Finds which symbol's slice `target` (a cumulative-frequency value,
    /// `0..total`) falls into — the decoder's half of "narrow to this
    /// symbol's interval," done by linear scan since 256 symbols is cheap
    /// enough that a Fenwick tree would only add complexity here, not speed
    /// that matters.
    fn symbol_at(&self, target: u32) -> u8 {
        for s in 0..256usize {
            if target < self.cum[s + 1] {
                return s as u8;
            }
        }
        255 // unreachable for a target < self.total, kept total rather than panic
    }

    fn update(&mut self, symbol: u8) {
        self.freq[symbol as usize] += 32;
        for i in (symbol as usize + 1)..=256 {
            self.cum[i] += 32;
        }
        self.total += 32;
        if self.total >= MAX_TOTAL {
            // Halve every frequency (floor at 1, so no symbol ever becomes
            // provably-impossible) and rebuild the cumulative table --
            // keeps `total` bounded forever without ever forgetting a
            // symbol has been seen at all.
            let mut new_total = 0u32;
            for f in self.freq.iter_mut() {
                *f = (*f / 2).max(1);
                new_total += *f;
            }
            self.cum[0] = 0;
            for i in 0..256 {
                self.cum[i + 1] = self.cum[i] + self.freq[i];
            }
            self.total = new_total;
        }
    }
}

pub struct Encoder {
    low: u32,
    range: u32,
    out: Vec<u8>,
    model: Model,
}

impl Encoder {
    pub fn new() -> Self {
        Encoder { low: 0, range: u32::MAX, out: Vec::new(), model: Model::new() }
    }

    fn normalize(&mut self) {
        // Carry-free (Subbotin) rule: renormalize whenever `low` and
        // `low+range` still share their top byte (the interval is narrow
        // enough that byte is already decided), OR whenever `range` alone
        // has shrunk below `BOTTOM` (about to underflow `range/total`) --
        // the second branch forces early renormalization exactly when
        // `low`/`high` DISAGREE in their top byte, which is the situation
        // that would otherwise need carry propagation to resolve correctly.
        loop {
            if (self.low ^ self.low.wrapping_add(self.range)) < TOP {
                // top byte settled -- emit it
            } else if self.range < BOTTOM {
                self.range = self.low.wrapping_neg() & (BOTTOM - 1);
            } else {
                break;
            }
            self.out.push((self.low >> 24) as u8);
            self.low <<= 8;
            self.range <<= 8;
        }
    }

    pub fn encode_symbol(&mut self, symbol: u8) {
        let (cum, freq, total) = self.model.interval(symbol);
        self.range /= total;
        self.low = self.low.wrapping_add(cum * self.range);
        self.range *= freq;
        self.normalize();
        self.model.update(symbol);
    }

    /// Flushes the remaining state so the decoder can recover the last few
    /// symbols — without this, information still sitting in `low` (not yet
    /// forced out by `normalize`) would simply be lost.
    pub fn finish(mut self) -> Vec<u8> {
        for _ in 0..4 {
            self.out.push((self.low >> 24) as u8);
            self.low <<= 8;
        }
        self.out
    }
}

pub struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    low: u32,
    range: u32,
    code: u32,
    model: Model,
}

impl<'a> Decoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let mut d = Decoder { bytes, pos: 0, low: 0, range: u32::MAX, code: 0, model: Model::new() };
        for _ in 0..4 {
            d.code = (d.code << 8) | d.next_byte() as u32;
        }
        d
    }

    fn next_byte(&mut self) -> u8 {
        let b = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn normalize(&mut self) {
        loop {
            if (self.low ^ self.low.wrapping_add(self.range)) < TOP {
                // fall through to shift below
            } else if self.range < BOTTOM {
                self.range = self.low.wrapping_neg() & (BOTTOM - 1);
            } else {
                break;
            }
            self.code = (self.code << 8) | self.next_byte() as u32;
            self.low <<= 8;
            self.range <<= 8;
        }
    }

    pub fn decode_symbol(&mut self) -> u8 {
        self.range /= self.model.total;
        let target = (self.code.wrapping_sub(self.low)) / self.range;
        // A rescale in the model, or plain rounding at the top of the
        // range, can push `target` one past the last real cumulative slot
        // in rare edge cases -- clamp rather than let `symbol_at` walk off
        // the table, matching the encoder's own `total`-bounded intervals.
        let target = target.min(self.model.total - 1);
        let symbol = self.model.symbol_at(target);
        let (cum, freq, _total) = self.model.interval(symbol);
        self.low = self.low.wrapping_add(cum * self.range);
        self.range *= freq;
        self.normalize();
        self.model.update(symbol);
        symbol
    }
}

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    for &b in data {
        enc.encode_symbol(b);
    }
    enc.finish()
}

pub fn decode(bytes: &[u8], symbol_count: usize) -> Vec<u8> {
    let mut dec = Decoder::new(bytes);
    let mut out = Vec::with_capacity(symbol_count);
    for _ in 0..symbol_count {
        out.push(dec.decode_symbol());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let enc = encode(data);
        let dec = decode(&enc, data.len());
        assert_eq!(dec, data, "round-trip mismatch for {} bytes", data.len());
    }

    #[test]
    fn round_trips_empty_input() {
        roundtrip(&[]);
    }

    #[test]
    fn round_trips_ordinary_text() {
        roundtrip(b"the quick brown fox jumps over the lazy dog, again and again and again");
    }

    #[test]
    fn round_trips_a_single_repeated_byte_many_times() {
        roundtrip(&[42u8; 5000]);
    }

    #[test]
    fn round_trips_all_256_byte_values_repeated() {
        let mut data = Vec::new();
        for _ in 0..20 {
            data.extend(0..=255u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn round_trips_data_long_enough_to_force_model_rescaling() {
        // MAX_TOTAL is 1<<16; with +32 per update that's ~2048 symbols
        // before the model's own halving-rescale path runs at least once --
        // this specifically exercises that path, not just the common case.
        let mut data = Vec::new();
        for i in 0..10_000u32 {
            data.push((i % 17) as u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn compresses_skewed_data_below_its_original_size() {
        let mut data = vec![b'a'; 20_000];
        for i in 0..200 {
            data.push((b'b' + (i % 5)) as u8);
        }
        let enc = encode(&data);
        assert!(
            enc.len() < data.len() / 4,
            "expected real compression on skewed data: {} bytes vs {} original",
            enc.len(),
            data.len()
        );
    }

    #[test]
    fn round_trips_pseudo_random_incompressible_data() {
        // A cheap xorshift, not a real RNG -- just needs to be
        // unpredictable enough that this isn't secretly skewed data.
        let mut state = 0x2545F4914F6CDD1Du64;
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
