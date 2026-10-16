//! A small, fast, NON-cryptographic hasher for the interpreter's own tables
//! (variable frames, the global environment, the user-function tables).
//!
//! WHY. The std `HashMap` defaults to SipHash-1-3, which is hardened against
//! hash flooding and costs ~15-25 ns per short key. Every variable read,
//! every assignment and every call hashes a variable or function NAME several
//! times, and the keys are identifiers taken from the program being run, so
//! flooding resistance buys nothing here while the hash cost was one of the
//! largest single items in a plain `x = y` (see `docs/perf-0.4.9.md`,
//! "Interpreter overhead addendum").
//!
//! The mixing function is the multiply-rotate scheme `rustc-hash` 2.x uses
//! (rotate in `finish` so the low bits `hashbrown` indexes with depend on the
//! whole key). Written out here rather than added as a dependency: the
//! vendored registry is offline and the whole thing is thirty lines.
//!
//! Iteration order of a map using this hasher is deterministic but still
//! unspecified; nothing in the interpreter may depend on it (and nothing
//! could before either, with the randomly-seeded std hasher).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const K: u64 = 0xf135_7aea_2e62_a9c5;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = self.hash.wrapping_add(word).wrapping_mul(K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, b: &[u8]) {
        // Identifiers are almost always under 16 bytes, so the short cases
        // read their bytes with a fixed number of loads (no loop, no copy
        // into a scratch buffer -- the first version of this function did
        // that and was SLOWER than SipHash on a 5-byte name). The head and
        // tail words overlap for lengths that are not a multiple of the
        // word size, which covers every byte. (`str`'s hash also writes a
        // 0xff terminator, so "a" and "a\0" differ even before this.)
        let n = b.len();
        if n >= 8 {
            let mut i = 0;
            while i + 8 < n {
                self.add(u64::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4], b[i + 5], b[i + 6], b[i + 7]]));
                i += 8;
            }
            let t = &b[n - 8..];
            self.add(u64::from_le_bytes([t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7]]));
        } else if n >= 4 {
            let lo = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64;
            let hi = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]) as u64;
            self.add(lo | (hi << 32));
        } else if n > 0 {
            self.add(b[0] as u64 | ((b[n / 2] as u64) << 8) | ((b[n - 1] as u64) << 16) | ((n as u64) << 24));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash.rotate_left(26)
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
/// `HashMap` with the fast hasher. Construct with `FastMap::default()`.
pub type FastMap<K, V> = HashMap<K, V, FxBuild>;
pub type FastSet<K> = std::collections::HashSet<K, FxBuild>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_short_names_do_not_collide_in_a_map() {
        let mut m: FastMap<String, usize> = FastMap::default();
        for i in 0..5000 {
            m.insert(format!("k{i}"), i);
            m.insert(format!("{i}"), i + 1);
        }
        for i in 0..5000 {
            assert_eq!(m[&format!("k{i}")], i);
            assert_eq!(m[&format!("{i}")], i + 1);
        }
        m.insert("a".into(), 1);
        m.insert("a\0".into(), 2);
        assert_eq!(m["a"], 1);
        assert_eq!(m["a\0"], 2);
    }

    #[test]
    fn borrowed_str_lookup_matches_owned_key() {
        let mut m: FastMap<String, i32> = FastMap::default();
        m.insert("alpha_beta_gamma_delta".to_string(), 7);
        assert_eq!(m.get("alpha_beta_gamma_delta"), Some(&7));
        assert_eq!(m.get("alpha_beta_gamma_delt"), None);
    }
}
