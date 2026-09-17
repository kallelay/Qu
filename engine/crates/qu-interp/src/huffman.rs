//! Canonical Huffman coding — a real, from-scratch encoder/decoder over
//! byte data, kept in its own file for the same "minimize collision with
//! concurrent work on `lib.rs`" reasoning `fs_ops`/`queue_pool`/`h5_model`/
//! `gpu_probe`/`inflate` already use.
//!
//! Canonical (not just "a" Huffman code) because the decoder needs to
//! reconstruct the exact same tree from a small, serializable description —
//! a plain code-length-per-symbol table, the same trick DEFLATE itself uses
//! (RFC 1951 §3.2.2) — rather than shipping the tree shape itself. This is
//! also why `inflate.rs`'s own DEFLATE decoder builds its Huffman tables the
//! same way; this module is the general-purpose, standalone version of the
//! same idea.

use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// One priority-queue node while building the Huffman tree: either a leaf
/// (a real byte symbol with its frequency) or an internal node (the sum of
/// two children's frequencies, no symbol of its own).
#[derive(Clone)]
enum Node {
    Leaf { freq: u64, symbol: u8 },
    Internal { freq: u64, left: Box<Node>, right: Box<Node> },
}

impl Node {
    fn freq(&self) -> u64 {
        match self {
            Node::Leaf { freq, .. } | Node::Internal { freq, .. } => *freq,
        }
    }
}

// `BinaryHeap` is a max-heap; reversing the frequency ordering turns it into
// the min-heap Huffman's algorithm needs (always merge the two LOWEST-
// frequency nodes). Ties broken by symbol value / insertion is irrelevant
// to correctness (any tie-break produces a valid, optimal code — canonical
// re-numbering below is what makes the result reproducible regardless).
struct HeapEntry(Node);
impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.0.freq() == other.0.freq()
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.freq().cmp(&self.0.freq())
    }
}

/// Walks the raw (non-canonical) Huffman tree, recording each leaf's code
/// LENGTH only — canonical Huffman never needs the actual bit patterns from
/// this tree, just the length each symbol ended up at (see module doc).
fn collect_lengths(node: &Node, depth: u8, lengths: &mut [u8; 256]) {
    match node {
        Node::Leaf { symbol, .. } => {
            // A single-symbol input never enters a real branch below, so
            // depth 0 is possible here; `encode` special-cases that input
            // shape entirely (see its own comment) rather than emitting a
            // zero-length code, which would be meaningless to write bits
            // for.
            lengths[*symbol as usize] = depth.max(1);
        }
        Node::Internal { left, right, .. } => {
            collect_lengths(left, depth + 1, lengths);
            collect_lengths(right, depth + 1, lengths);
        }
    }
}

/// Builds canonical codes from a code-LENGTH-per-symbol table (RFC 1951
/// §3.2.2's algorithm, the standard one): symbols are assigned codes in
/// ascending (length, symbol-value) order, each new code one more than the
/// previous, left-shifted whenever length increases. This is what makes the
/// code reconstructible from lengths alone — no tree shape needs to travel
/// with the encoded data, only 256 small integers (and most are 0, for
/// symbols that never appear).
fn canonical_codes(lengths: &[u8; 256]) -> [(u32, u8); 256] {
    let max_len = *lengths.iter().max().unwrap_or(&0) as usize;
    let mut len_count = vec![0u32; max_len + 1];
    for &l in lengths.iter() {
        if l > 0 {
            len_count[l as usize] += 1;
        }
    }
    let mut next_code = vec![0u32; max_len + 2];
    let mut code = 0u32;
    for len in 1..=max_len {
        code = (code + len_count[len - 1]) << 1;
        next_code[len] = code;
    }
    let mut codes = [(0u32, 0u8); 256];
    // Ascending symbol value at a fixed length is exactly iteration order
    // here (0..256), so no explicit sort is needed to match RFC 1951's own
    // tie-break rule.
    for symbol in 0..256usize {
        let len = lengths[symbol];
        if len > 0 {
            codes[symbol] = (next_code[len as usize], len);
            next_code[len as usize] += 1;
        }
    }
    codes
}

/// Bit-packs a symbol's canonical code MSB-first into `out` — the direction
/// canonical Huffman decoding reads codes in (see `decode`'s own comment on
/// why MSB-first is what makes prefix-free decoding unambiguous bit by bit).
fn push_bits(out: &mut Vec<u8>, bitpos: &mut usize, code: u32, len: u8) {
    for i in (0..len).rev() {
        let bit = (code >> i) & 1;
        let byte_idx = *bitpos / 8;
        if byte_idx == out.len() {
            out.push(0);
        }
        if bit == 1 {
            out[byte_idx] |= 1 << (7 - (*bitpos % 8));
        }
        *bitpos += 1;
    }
}

/// The result of [`encode`]: the code-length table (needed to reconstruct
/// the canonical codes for decoding), the packed bits, and how many of the
/// last byte's bits are real data (the rest are zero-padding).
pub struct Encoded {
    pub lengths: [u8; 256],
    pub bits: Vec<u8>,
    pub bit_len: usize,
    /// Original input length in bytes — needed because a maximally-skewed
    /// single-symbol input (see `encode`'s own comment) produces a 1-bit-
    /// per-symbol code, and without knowing exactly how many symbols to
    /// stop after, decoding would run past the intended end reading
    /// zero-padding as more (bogus) zero-bit symbols.
    pub symbol_count: usize,
}

/// Encodes `data` (arbitrary bytes — a `Str`'s UTF-8 bytes or a raw `Vec` of
/// 0-255 values, the caller's choice) via canonical Huffman coding. Returns
/// `None` only for empty input (nothing to encode, nothing meaningful to
/// return).
pub fn encode(data: &[u8]) -> Option<Encoded> {
    if data.is_empty() {
        return None;
    }
    let mut freq = [0u64; 256];
    for &b in data {
        freq[b as usize] += 1;
    }

    let distinct: Vec<u8> = (0..256u16).filter(|&s| freq[s as usize] > 0).map(|s| s as u8).collect();

    // A single distinct symbol (e.g. `data` is all zero bytes) has no
    // meaningful Huffman TREE at all — there's nothing to branch on. Assign
    // it the trivial 1-bit code "0" directly rather than special-casing a
    // depth-0 tree through the general path below (which `collect_lengths`
    // already guards against via `.max(1)`, but a real tree still needs at
    // least two leaves to build in the first place).
    let mut lengths = [0u8; 256];
    if distinct.len() == 1 {
        lengths[distinct[0] as usize] = 1;
    } else {
        let mut heap: BinaryHeap<HeapEntry> = distinct
            .iter()
            .map(|&s| HeapEntry(Node::Leaf { freq: freq[s as usize], symbol: s }))
            .collect();
        while heap.len() > 1 {
            let HeapEntry(a) = heap.pop().unwrap();
            let HeapEntry(b) = heap.pop().unwrap();
            heap.push(HeapEntry(Node::Internal {
                freq: a.freq() + b.freq(),
                left: Box::new(a),
                right: Box::new(b),
            }));
        }
        let HeapEntry(root) = heap.pop().unwrap();
        collect_lengths(&root, 0, &mut lengths);
    }

    let codes = canonical_codes(&lengths);
    let mut bits = Vec::new();
    let mut bitpos = 0usize;
    for &b in data {
        let (code, len) = codes[b as usize];
        push_bits(&mut bits, &mut bitpos, code, len);
    }

    Some(Encoded { lengths, bits, bit_len: bitpos, symbol_count: data.len() })
}

/// Inverts [`encode`]: rebuilds the same canonical codes from `lengths`
/// (`canonical_codes` is a pure function of the length table, so the
/// decoder reconstructs byte-identical codes without ever seeing the tree),
/// then walks `bits` MSB-first, extending a candidate code one bit at a
/// time until it matches a known (code, length) pair — the defining
/// property of a prefix-free code: at most one such match is ever possible,
/// so the first match found is unambiguously correct, with no backtracking.
pub fn decode(lengths: &[u8; 256], bits: &[u8], bit_len: usize, symbol_count: usize) -> Vec<u8> {
    let codes = canonical_codes(lengths);
    // Reverse lookup: (code, length) -> symbol. A `Vec` of `(u32,u8,u8)`
    // rather than a `HashMap` — at most 256 entries, and this runs once per
    // decode, not once per bit.
    let mut by_code: Vec<(u32, u8, u8)> = Vec::new();
    for symbol in 0..256usize {
        let (code, len) = codes[symbol];
        if len > 0 {
            by_code.push((code, len, symbol as u8));
        }
    }

    let mut out = Vec::with_capacity(symbol_count);
    let mut acc: u32 = 0;
    let mut acc_len: u8 = 0;
    let mut bitpos = 0usize;
    while out.len() < symbol_count && bitpos < bit_len {
        let byte_idx = bitpos / 8;
        let bit = (bits[byte_idx] >> (7 - (bitpos % 8))) & 1;
        acc = (acc << 1) | bit as u32;
        acc_len += 1;
        bitpos += 1;
        if let Some(&(_, _, symbol)) = by_code.iter().find(|&&(c, l, _)| l == acc_len && c == acc) {
            out.push(symbol);
            acc = 0;
            acc_len = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let enc = encode(data).expect("non-empty input encodes");
        let dec = decode(&enc.lengths, &enc.bits, enc.bit_len, enc.symbol_count);
        assert_eq!(dec, data, "round-trip mismatch for {data:?}");
    }

    #[test]
    fn round_trips_ordinary_text() {
        roundtrip(b"the quick brown fox jumps over the lazy dog, again and again");
    }

    #[test]
    fn round_trips_a_single_repeated_byte() {
        // The degenerate single-symbol case `encode` special-cases.
        roundtrip(&[7u8; 500]);
    }

    #[test]
    fn round_trips_two_symbols() {
        roundtrip(b"ababababababab");
    }

    #[test]
    fn round_trips_all_256_byte_values() {
        let data: Vec<u8> = (0..=255u8).collect();
        roundtrip(&data);
    }

    #[test]
    fn round_trips_skewed_frequencies() {
        // One dominant symbol, several rare ones -- the shape Huffman
        // coding is actually FOR (short codes for the common case).
        let mut data = vec![b'a'; 1000];
        data.extend_from_slice(b"bcdxyz");
        roundtrip(&data);
    }

    #[test]
    fn compresses_skewed_data_below_its_original_size() {
        // Not just "round-trips" -- Huffman coding exists to shrink data,
        // so a real skew should actually produce fewer bits than a naive
        // fixed-width (8 bits/symbol) encoding would.
        let mut data = vec![b'a'; 10_000];
        for i in 0..100 {
            data.push((b'b' + (i % 5)) as u8);
        }
        let enc = encode(&data).unwrap();
        let naive_bits = data.len() * 8;
        assert!(
            enc.bit_len < naive_bits / 4,
            "expected real compression on skewed data: {} bits vs {naive_bits} naive",
            enc.bit_len
        );
    }

    #[test]
    fn empty_input_encodes_to_nothing() {
        assert!(encode(&[]).is_none());
    }
}
