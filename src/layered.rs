//! A map shared between a macro host and the analysis frames taken from it.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

/// A map whose frames share its entries by reference. Entries are written to
/// a small layer of this map's own, over a base shared with every frame taken
/// from it; when the layer grows past a bound, the two are merged into a new
/// base, so a frame costs the layer and a lookup costs two probes.
#[derive(Debug)]
pub(crate) struct Layered<K, V> {
    base: Rc<HashMap<K, V>>,
    /// This map's entries over `base`; `None` removes a base entry.
    own: HashMap<K, Option<V>>,
    len: usize,
}

impl<K, V> Default for Layered<K, V> {
    fn default() -> Self {
        Self {
            base: Rc::new(HashMap::new()),
            own: HashMap::new(),
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
            self.own.insert(key.to_owned(), Some(value));
            self.compact();
        }
        self.own.get_mut(key)?.as_mut()
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let previous = match self.own.insert(key.clone(), Some(value)) {
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
            match self.own.insert(key.to_owned(), None) {
                Some(previous) => previous,
                None => self.base.get(key).cloned(),
            }
        } else {
            self.own.remove(key).flatten()
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
        for (key, value) in self.own.drain() {
            match value {
                Some(value) => {
                    merged.insert(key, value);
                }
                None => {
                    merged.remove(&key);
                }
            }
        }
        self.base = Rc::new(merged);
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
            base: Rc::new(base),
            own: HashMap::new(),
        }
    }
}
