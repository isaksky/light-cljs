//! A map shared between a macro host and the analysis frames taken from it.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// A map whose frames share its entries by reference. Entries are written to
/// a small layer over a shared base. Both are shared until a write, so a frame
/// costs two reference increments and a lookup costs two probes. Once the
/// layer grows past a bound, the two are merged into a new base.
#[derive(Debug)]
pub struct Layered<K, V> {
    base: Arc<HashMap<K, V>>,
    /// This map's entries over `base`; `None` removes a base entry.
    own: Arc<HashMap<K, Option<V>>>,
    len: usize,
}

impl<K: Eq + Hash + Clone, V: Clone + PartialEq> PartialEq for Layered<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl<K, V> Default for Layered<K, V> {
    fn default() -> Self {
        Self {
            base: Arc::new(HashMap::new()),
            own: Arc::new(HashMap::new()),
            len: 0,
        }
    }
}

impl<K: Clone, V: Clone> Clone for Layered<K, V> {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            own: self.own.clone(),
            len: self.len,
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Layered<K, V> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        match self.own.get(key) {
            Some(value) => value.as_ref(),
            None => self.base.get(key),
        }
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.get(key).is_some()
    }

    /// The entry for `key`, inserting `V::default()` if there is none.
    pub fn entry_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        if !self.contains_key(&key) {
            self.insert(key.clone(), V::default());
        }
        self.get_mut(&key).expect("inserted")
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq + ToOwned<Owned = K>,
    {
        if !self.own.contains_key(key) {
            let value = self.base.get(key)?.clone();
            // Compact first: the entry must stay in the own layer to be
            // returned from it.
            self.compact();
            Arc::make_mut(&mut self.own).insert(key.to_owned(), Some(value));
        }
        Arc::make_mut(&mut self.own).get_mut(key)?.as_mut()
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let previous = match Arc::make_mut(&mut self.own).insert(key.clone(), Some(value)) {
            Some(previous) => previous,
            None => self.base.get(&key).cloned(),
        };
        if previous.is_none() {
            self.len += 1;
        }
        self.compact();
        previous
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq + ToOwned<Owned = K>,
    {
        let previous = if self.base.contains_key(key) {
            match Arc::make_mut(&mut self.own).insert(key.to_owned(), None) {
                Some(previous) => previous,
                None => self.base.get(key).cloned(),
            }
        } else {
            Arc::make_mut(&mut self.own).remove(key).flatten()
        };
        if previous.is_some() {
            self.len -= 1;
        }
        previous
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.base
            .iter()
            .filter(|(key, _)| !self.own.contains_key(*key))
            .chain(
                self.own
                    .iter()
                    .filter_map(|(key, value)| value.as_ref().map(|value| (key, value))),
            )
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }

    /// Merge the own layer into a new base once it is large next to the base,
    /// so a frame copies at most that many entries.
    fn compact(&mut self) {
        if self.own.len() <= 64 + self.base.len().isqrt() {
            return;
        }
        let mut merged = (*self.base).clone();
        for (key, value) in Arc::make_mut(&mut self.own).drain() {
            match value {
                Some(value) => {
                    merged.insert(key, value);
                }
                None => {
                    merged.remove(&key);
                }
            }
        }
        self.base = Arc::new(merged);
    }
}

impl<K, V, Q> std::ops::Index<&Q> for Layered<K, V>
where
    K: Eq + Hash + Clone + Borrow<Q>,
    V: Clone,
    Q: ?Sized + Hash + Eq,
{
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key).expect("no entry found for key")
    }
}

impl<K: Eq + Hash + Clone, V: Clone> FromIterator<(K, V)> for Layered<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let base: HashMap<K, V> = iter.into_iter().collect();
        Self {
            len: base.len(),
            base: Arc::new(base),
            own: Arc::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Extend<(K, V)> for Layered<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, entries: I) {
        for (key, value) in entries {
            self.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Layered;

    #[test]
    fn frames_keep_own_entries_across_mutation_removal_and_compaction() {
        let mut map = Layered::<String, Vec<usize>>::default();
        let mut oracle = std::collections::HashMap::<String, Vec<usize>>::new();
        let mut frames = Vec::new();
        for i in 0..240 {
            let key = format!("k{i}");
            map.insert(key.clone(), vec![i]);
            oracle.insert(key, vec![i]);
            if i % 7 == 0 {
                frames.push((map.clone(), oracle.clone()));
            }
            let previous = format!("k{}", i / 2);
            map.entry_or_default(previous.clone()).push(i + 1000);
            oracle.entry(previous.clone()).or_default().push(i + 1000);
            if i % 3 == 0 {
                assert_eq!(map.remove(&previous), oracle.remove(&previous));
            }
            if let Some(value) = map.get_mut("k0") {
                value.push(i);
                oracle.get_mut("k0").unwrap().push(i);
            }
        }
        frames.push((map.clone(), oracle));
        map.clear();
        for (frame, expected) in frames {
            assert_eq!(frame.len(), expected.len());
            assert_eq!(frame.iter().count(), expected.len());
            for (key, value) in expected {
                assert_eq!(frame.get(&key), Some(&value));
            }
        }
    }

    #[test]
    fn get_mut_survives_compaction_boundary() {
        let mut map: Layered<String, Vec<u32>> =
            (0..100).map(|i| (format!("k{i}"), vec![i])).collect();
        let frame = map.clone();
        // 64 + isqrt(100) own entries fit; the next write crosses the bound.
        for i in 100..174 {
            map.insert(format!("k{i}"), vec![i]);
        }
        map.get_mut("k5").unwrap().push(99);
        assert_eq!(map.get("k5"), Some(&vec![5, 99]));
        map.entry_or_default("k7".to_owned()).push(7);
        assert_eq!(map.get("k7"), Some(&vec![7, 7]));
        map.entry_or_default("new".to_owned()).push(1);
        assert_eq!(map.get("new"), Some(&vec![1]));
        assert_eq!(map.len(), 175);
        assert_eq!(map.iter().count(), 175);
        assert_eq!(map.remove("k5"), Some(vec![5, 99]));
        assert!(!map.contains_key("k5"));
        assert_eq!(map.len(), 174);
        assert_eq!(map.keys().count(), 174);
        assert_eq!(frame.get("k5"), Some(&vec![5]));
        assert_eq!(frame.get("k7"), Some(&vec![7]));
        assert_eq!(frame.len(), 100);
        map.clear();
        assert_eq!(map.len(), 0);
        assert_eq!(frame.len(), 100);
    }
}
