//! The storage behind `Value::Dict`: insertion-ordered pairs plus a lazily
//! built hash index.
//!
//! `Dict` used to be a bare `Vec<(String, Value)>`, so `get` was a linear
//! scan and `set` cloned every key and value into a new vector: 5000
//! `set` + `get` took ~370x longer than the same loop in Python and 1e5
//! inserts did not finish. Order, key normalisation and value semantics are
//! unchanged; only the lookup is.
//!
//! * `get` on a dict of more than [`INDEX_MIN`] entries builds a
//!   `HashMap<key, position>` once (in a `OnceLock`, so a shared, immutable
//!   dict can build it behind a `&self`) and reuses it.
//! * Mutation is `&mut self` (reached through `Arc::make_mut`), so a dict
//!   nobody else holds is updated in place and keeps its index current; a
//!   shared one is cloned first, and the clone starts without an index (it
//!   is rebuilt on its first lookup).
//! * Small dicts never build an index: a scan of 16 short keys is cheaper
//!   than hashing.
//!
//! `Deref<Target = Vec<..>>` keeps every read-only use (`len`, `iter`,
//! `is_empty`, slicing) working as before. There is deliberately no
//! `DerefMut`: writes go through [`DictData::set`] so the index cannot go
//! stale.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::OnceLock;

use crate::Value;

/// Below this many entries a linear scan beats building and probing a map.
const INDEX_MIN: usize = 16;

#[derive(Debug, Default)]
pub struct DictData {
    pairs: Vec<(String, Value)>,
    /// key -> position of its FIRST pair. Absent until a big dict is searched.
    index: OnceLock<HashMap<String, usize>>,
}

impl Clone for DictData {
    fn clone(&self) -> Self {
        // The index is derived data: the clone rebuilds it if it needs one.
        DictData { pairs: self.pairs.clone(), index: OnceLock::new() }
    }
}

impl From<Vec<(String, Value)>> for DictData {
    fn from(pairs: Vec<(String, Value)>) -> Self {
        DictData { pairs, index: OnceLock::new() }
    }
}

impl Deref for DictData {
    type Target = Vec<(String, Value)>;
    fn deref(&self) -> &Self::Target {
        &self.pairs
    }
}

impl DictData {
    /// Position of `key`, if present.
    pub fn position(&self, key: &str) -> Option<usize> {
        if self.pairs.len() <= INDEX_MIN {
            return self.pairs.iter().position(|(k, _)| k == key);
        }
        let index = self.index.get_or_init(|| {
            let mut m = HashMap::with_capacity(self.pairs.len());
            for (i, (k, _)) in self.pairs.iter().enumerate() {
                m.entry(k.clone()).or_insert(i);
            }
            m
        });
        index.get(key).copied()
    }

    /// The value stored under `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.position(key).map(|i| &self.pairs[i].1)
    }

    /// Bind `key` to `value`: replaces in place if the key exists (keeping
    /// its position), appends otherwise. Keeps a built index current.
    pub fn set(&mut self, key: String, value: Value) {
        match self.position(&key) {
            Some(i) => self.pairs[i].1 = value,
            None => {
                let i = self.pairs.len();
                if let Some(index) = self.index.get_mut() {
                    index.insert(key.clone(), i);
                }
                self.pairs.push((key, value));
            }
        }
    }

    /// The pairs, consuming the dict.
    pub fn into_pairs(self) -> Vec<(String, Value)> {
        self.pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::Num(x)
    }

    #[test]
    fn set_replaces_in_place_and_appends_new_keys_in_order() {
        let mut d = DictData::default();
        d.set("a".into(), n(1.0));
        d.set("b".into(), n(2.0));
        d.set("a".into(), n(9.0));
        let keys: Vec<&str> = d.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b"]);
        assert!(matches!(d.get("a"), Some(Value::Num(x)) if *x == 9.0));
        assert!(d.get("zzz").is_none());
    }

    #[test]
    fn a_big_dict_uses_the_index_and_it_stays_correct_through_inserts() {
        let mut d = DictData::default();
        for i in 0..5000 {
            d.set(format!("k{i}"), n(i as f64));
            if i % 997 == 0 {
                // a lookup mid-growth builds the index; later inserts must update it
                assert!(d.get(&format!("k{i}")).is_some());
            }
        }
        assert_eq!(d.len(), 5000);
        for i in [0usize, 1, 16, 17, 999, 4999] {
            assert!(matches!(d.get(&format!("k{i}")), Some(Value::Num(x)) if *x == i as f64), "k{i}");
        }
        d.set("k17".into(), n(-1.0));
        assert!(matches!(d.get("k17"), Some(Value::Num(x)) if *x == -1.0));
        assert_eq!(d.len(), 5000, "an update does not grow the dict");
        assert_eq!(d.position("k17"), Some(17), "an update keeps the key's position");
    }

    #[test]
    fn a_clone_does_not_share_a_stale_index() {
        let mut d = DictData::default();
        for i in 0..100 {
            d.set(format!("k{i}"), n(i as f64));
        }
        assert!(d.get("k50").is_some()); // build the index
        let mut c = d.clone();
        c.set("extra".into(), n(1.0));
        assert!(c.get("extra").is_some());
        assert!(d.get("extra").is_none(), "the original is untouched");
        assert!(c.get("k50").is_some());
    }

    #[test]
    fn duplicate_keys_resolve_to_the_first_pair() {
        let d = DictData::from(
            (0..20).map(|i| (format!("k{}", i % 10), n(i as f64))).collect::<Vec<_>>(),
        );
        assert!(matches!(d.get("k3"), Some(Value::Num(x)) if *x == 3.0));
    }
}
