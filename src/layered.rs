//! A map shared between a macro host and the analysis frames taken from it.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// A map whose frames share its entries by reference. Entries are written to
/// a small layer over a middle layer over a shared base. All three are
/// shared until a write, so a frame costs three reference increments and a
/// lookup at most three probes. Once the small layer grows past a bound it
/// is merged into the middle one, and once that grows past its own bound,
/// into a new base.
///
/// A write after a frame was taken copies the small layer, and a merge
/// copies the layer merged into while a frame shares it. With bounds of
/// about the cube root and its square of the map's size, a run of writes,
/// each after a frame, copies about twice the cube root of the size per
/// write, where two layers copied the square root (light-cljs-994: a
/// namespace's top-level pass takes a frame for each definition and
/// registers the next one's names after it).
#[derive(Debug)]
pub struct Layered<K, V> {
    base: Arc<HashMap<K, V>>,
    /// Entries over `base`; `None` removes a base entry.
    mid: Arc<HashMap<K, Option<V>>>,
    /// This map's entries over `mid` and `base`; `None` removes an entry
    /// of either.
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
            mid: Arc::new(HashMap::new()),
            own: Arc::new(HashMap::new()),
            len: 0,
        }
    }
}

impl<K: Clone, V: Clone> Clone for Layered<K, V> {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            mid: self.mid.clone(),
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
            None => self.lower(key),
        }
    }

    /// The entry for `key` below the own layer.
    fn lower<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if self.mid.is_empty() {
            return self.base.get(key);
        }
        match self.mid.get(key) {
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
            let value = self.lower(key)?.clone();
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
            None => self.lower(&key).cloned(),
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
        let previous = if self.lower(key).is_some() {
            match Arc::make_mut(&mut self.own).insert(key.to_owned(), None) {
                Some(previous) => previous,
                None => self.lower(key).cloned(),
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
            .filter(|(key, _)| !self.own.contains_key(*key) && !self.mid.contains_key(*key))
            .chain(
                self.mid
                    .iter()
                    .filter(|(key, _)| !self.own.contains_key(*key))
                    .filter_map(|(key, value)| value.as_ref().map(|value| (key, value))),
            )
            .chain(
                self.own
                    .iter()
                    .filter_map(|(key, value)| value.as_ref().map(|value| (key, value))),
            )
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }

    /// The keys whose entries may differ between this map and `before`,
    /// with repeats: when the two share their base, only the keys of the
    /// layers over it that they do not share, which are bounded; otherwise
    /// every key of both. A layer shared with another map is copied before
    /// it is written, so a shared layer has the same entries in both.
    pub fn changed_keys<'a>(&'a self, before: &'a Self) -> Vec<&'a K> {
        if !Arc::ptr_eq(&self.base, &before.base) {
            return self
                .base
                .keys()
                .chain(self.mid.keys())
                .chain(self.own.keys())
                .chain(before.base.keys())
                .chain(before.mid.keys())
                .chain(before.own.keys())
                .collect();
        }
        let mut changed = Vec::new();
        if !Arc::ptr_eq(&self.mid, &before.mid) {
            changed.extend(self.mid.keys().chain(before.mid.keys()));
        }
        if !Arc::ptr_eq(&self.own, &before.own) {
            changed.extend(self.own.keys().chain(before.own.keys()));
        }
        changed
    }

    /// The most entries the own layer holds before it is merged into the
    /// middle one, and the middle one before it is merged into the base:
    /// about the cube root of the map's size and its square, so that
    /// copying the own layer after a frame, the middle one on a merge into
    /// it and the base on a merge into that cost about the same per write.
    fn bounds(&self) -> (usize, usize) {
        let own = 32 + cube_root(self.len);
        (own, own * own)
    }

    /// Merge the own layer into the middle one once it is past its bound,
    /// and the middle one into the base once that is past its own, so a
    /// frame copies at most that many entries. A layer no frame shares
    /// takes the entries in place; only a shared one is copied, since
    /// copying it on every merge makes a run of inserts cost the size of
    /// the layer per insert (light-cljs-418).
    fn compact(&mut self) {
        let (own_bound, mid_bound) = self.bounds();
        if self.own.len() <= own_bound {
            return;
        }
        let own = std::mem::take(&mut self.own);
        let base = &self.base;
        let mid = Arc::make_mut(&mut self.mid);
        let mut merge = |key: K, value: Option<V>| match value {
            Some(value) => {
                mid.insert(key, Some(value));
            }
            // A removal stays a removal while the base has the entry.
            None if base.contains_key(&key) => {
                mid.insert(key, None);
            }
            None => {
                mid.remove(&key);
            }
        };
        match Arc::try_unwrap(own) {
            Ok(own) => own.into_iter().for_each(|(key, value)| merge(key, value)),
            Err(own) => own
                .iter()
                .for_each(|(key, value)| merge(key.clone(), value.clone())),
        }
        if self.mid.len() <= mid_bound {
            return;
        }
        let mid = std::mem::take(&mut self.mid);
        let merged = Arc::make_mut(&mut self.base);
        let mut merge = |key: K, value: Option<V>| match value {
            Some(value) => {
                merged.insert(key, value);
            }
            None => {
                merged.remove(&key);
            }
        };
        match Arc::try_unwrap(mid) {
            Ok(mid) => mid.into_iter().for_each(|(key, value)| merge(key, value)),
            Err(mid) => mid
                .iter()
                .for_each(|(key, value)| merge(key.clone(), value.clone())),
        }
    }
}

/// The integer cube root of `n`, rounded down.
fn cube_root(n: usize) -> usize {
    let mut root = (n as f64).cbrt() as usize;
    while root * root * root > n {
        root -= 1;
    }
    while (root + 1) * (root + 1) * (root + 1) <= n {
        root += 1;
    }
    root
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
            mid: Arc::new(HashMap::new()),
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

/// A set over a [`Layered`] map, with the methods of a `HashSet` the program
/// tables use: a frame shares it as the map is shared, and
/// [`Self::changed_keys`] finds what changed since an earlier frame without
/// walking the whole set (light-cljs-997).
#[derive(Debug)]
pub struct LayeredSet<K>(Layered<K, ()>);

impl<K> Default for LayeredSet<K> {
    fn default() -> Self {
        Self(Layered::default())
    }
}

impl<K: Clone> Clone for LayeredSet<K> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K: Eq + Hash + Clone> PartialEq for LayeredSet<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Eq + Hash + Clone> LayeredSet<K> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.0.contains_key(key)
    }

    /// Whether `key` was not in the set.
    pub fn insert(&mut self, key: K) -> bool {
        if self.0.contains_key(&key) {
            return false;
        }
        self.0.insert(key, ());
        true
    }

    /// Whether `key` was in the set.
    pub fn remove<Q>(&mut self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq + ToOwned<Owned = K>,
    {
        self.0.remove(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.0.keys()
    }

    /// The keys that may be in one of this set and `before` but not the
    /// other, with repeats ([`Layered::changed_keys`]).
    pub fn changed_keys<'a>(&'a self, before: &'a Self) -> Vec<&'a K> {
        self.0.changed_keys(&before.0)
    }
}

impl<'a, K: Eq + Hash + Clone> IntoIterator for &'a LayeredSet<K> {
    type Item = &'a K;
    type IntoIter = Box<dyn Iterator<Item = &'a K> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<K: Eq + Hash + Clone> FromIterator<K> for LayeredSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        Self(iter.into_iter().map(|key| (key, ())).collect())
    }
}

impl<K: Eq + Hash + Clone> Extend<K> for LayeredSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, keys: I) {
        for key in keys {
            self.insert(key);
        }
    }
}

/// Qualified names retain their original spelling for iteration and artifacts,
/// but can be looked up with borrowed namespace/member parts. The borrowed key
/// and the stored key hash the same pair, including whether a slash is present.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QualifiedName(String);

trait NameParts {
    fn parts(&self) -> (Option<&str>, &str);
}

impl NameParts for QualifiedName {
    fn parts(&self) -> (Option<&str>, &str) {
        name_parts(&self.0)
    }
}

fn name_parts(name: &str) -> (Option<&str>, &str) {
    match name.split_once('/') {
        Some((namespace, member)) => (Some(namespace), member),
        None => (None, name),
    }
}

impl NameParts for (Option<&str>, &str) {
    fn parts(&self) -> (Option<&str>, &str) {
        *self
    }
}

impl Hash for QualifiedName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.parts().hash(state);
    }
}

impl Hash for dyn NameParts + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.parts().hash(state);
    }
}

impl PartialEq for dyn NameParts + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.parts() == other.parts()
    }
}
impl Eq for dyn NameParts + '_ {}

impl<'a> Borrow<dyn NameParts + 'a> for QualifiedName {
    fn borrow(&self) -> &(dyn NameParts + 'a) {
        self
    }
}

/// The same layered snapshot semantics for compiler names. No auxiliary index
/// needs invalidating when a declaration is replaced or a host is cloned.
pub(crate) struct QualifiedNames<V>(Layered<QualifiedName, V>);

impl<V> Default for QualifiedNames<V> {
    fn default() -> Self {
        Self(Layered::default())
    }
}
impl<V: Clone> Clone for QualifiedNames<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: Clone> QualifiedNames<V> {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn get(&self, key: &str) -> Option<&V> {
        self.0.get(&name_parts(key) as &dyn NameParts)
    }
    pub(crate) fn get_parts(&self, namespace: &str, member: &str) -> Option<&V> {
        if namespace.contains('/') {
            // Preserve concatenated-string semantics even for an unusual
            // namespace containing a slash; split at the first one as usual.
            return self.get(&format!("{namespace}/{member}"));
        }
        self.0.get(&(Some(namespace), member) as &dyn NameParts)
    }
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub(crate) fn insert(&mut self, key: String, value: V) -> Option<V> {
        self.0.insert(QualifiedName(key), value)
    }
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.0.iter().map(|(key, value)| (&key.0, value))
    }
    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys().map(|key| &key.0)
    }
    pub(crate) fn changed_keys<'a>(&'a self, before: &'a Self) -> Vec<&'a String> {
        self.0
            .changed_keys(&before.0)
            .into_iter()
            .map(|key| &key.0)
            .collect()
    }
}

impl<V: Clone, Q: AsRef<str> + ?Sized> std::ops::Index<&Q> for QualifiedNames<V> {
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key.as_ref())
            .expect("no entry found for qualified name")
    }
}

#[cfg(test)]
mod tests {
    use super::{Layered, LayeredSet, QualifiedName, QualifiedNames};

    #[test]
    fn qualified_names_borrow_parts_without_changing_spelling_or_identity() {
        let mut names = QualifiedNames::default();
        for (i, key) in [
            "plain", "/plain", "a/b", "a/b/c", "a//", "a/", "/", "", "λ/名",
        ]
        .into_iter()
        .enumerate()
        {
            names.insert(key.into(), i);
            assert_eq!(names.get(key), Some(&i));
        }
        assert_eq!(names.get_parts("", "plain"), Some(&1));
        assert_eq!(names.get_parts("a", "b"), Some(&2));
        assert_eq!(names.get_parts("a", "b/c"), Some(&3));
        assert_eq!(names.get_parts("a/b", "c"), Some(&3));
        assert_eq!(names.get_parts("a", "/"), Some(&4));
        assert_eq!(names.get_parts("a", ""), Some(&5));
        assert_eq!(names.get_parts("", ""), Some(&6));
        assert_eq!(names.get_parts("λ", "名"), Some(&8));
        assert_eq!(names.get_parts("a", "bc"), None);
        assert_eq!(names.get_parts("ab", "c"), None);
        assert_eq!(names["plain"], 0);
        assert_eq!(names[&"/plain".to_owned()], 1);
        let mut keys = names.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "", "/", "/plain", "a/", "a//", "a/b", "a/b/c", "plain", "λ/名"
            ]
        );
    }

    #[test]
    fn qualified_names_keep_borrowed_lookup_equivalent_across_snapshot_mutations() {
        let mut names = QualifiedNames::default();
        let mut expected = std::collections::HashMap::new();
        let mut snapshots = Vec::new();
        for i in 0..400 {
            let namespace = format!("ns{}", i % 9);
            let member = format!("v{}", i % 31);
            let key = format!("{namespace}/{member}");
            assert_eq!(
                names.insert(key.clone(), i),
                expected.insert(key.clone(), i)
            );
            assert_eq!(names.get_parts(&namespace, &member), expected.get(&key));
            if i % 17 == 0 {
                snapshots.push((names.clone(), expected.clone()));
            }
            if i % 13 == 0 {
                assert_eq!(
                    names.0.remove(&QualifiedName(key.clone())),
                    expected.remove(&key)
                );
                assert_eq!(names.get_parts(&namespace, &member), expected.get(&key));
            }
        }
        snapshots.push((names.clone(), expected));
        names.clear();
        assert!(names.iter().next().is_none());
        for (snapshot, expected) in snapshots {
            assert_eq!(snapshot.iter().count(), expected.len());
            for (key, value) in expected {
                let (namespace, member) = key.split_once('/').unwrap();
                assert_eq!(snapshot.get(&key), Some(&value));
                assert_eq!(snapshot.get_parts(namespace, member), Some(&value));
            }
        }
    }

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
    fn changed_keys_cover_every_difference_from_an_earlier_frame() {
        let mut map: Layered<String, usize> = (0..100).map(|i| (format!("k{i}"), i)).collect();
        for step in 0..300 {
            let before = map.clone();
            assert!(map.changed_keys(&before).is_empty());
            match step % 3 {
                0 => {
                    map.insert(format!("n{step}"), step);
                }
                1 => {
                    map.insert(format!("k{}", step % 100), step);
                }
                _ => {
                    map.remove(&format!("k{}", step % 100));
                }
            }
            let changed: std::collections::HashSet<&String> =
                map.changed_keys(&before).into_iter().collect();
            for key in map.keys().chain(before.keys()) {
                if map.get(key) != before.get(key) {
                    assert!(changed.contains(key), "{key} at step {step}");
                }
            }
        }
    }

    /// A layered set answers as a `HashSet` would, and its changed keys
    /// cover every difference from an earlier frame across merges.
    #[test]
    fn layered_set_matches_a_hash_set() {
        let mut set: LayeredSet<usize> = (0..100).collect();
        let mut expected: std::collections::HashSet<usize> = (0..100).collect();
        let frame = set.clone();
        for step in 0..400 {
            let key = step * 7 % 250;
            if step % 4 == 3 {
                assert_eq!(set.remove(&key), expected.remove(&key), "step {step}");
            } else {
                assert_eq!(set.insert(key), expected.insert(key), "step {step}");
            }
            assert_eq!(set.len(), expected.len());
        }
        assert_eq!(
            set.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>(),
            expected
        );
        assert!((0..300).all(|key| set.contains(&key) == expected.contains(&key)));
        let changed: std::collections::HashSet<usize> =
            set.changed_keys(&frame).into_iter().copied().collect();
        for key in 0..300 {
            if set.contains(&key) != frame.contains(&key) {
                assert!(changed.contains(&key), "{key}");
            }
        }
        assert_eq!(frame.len(), 100);
        assert!(frame.changed_keys(&frame.clone()).is_empty());
    }

    #[test]
    fn get_mut_survives_compaction_boundary() {
        let mut map: Layered<String, Vec<u32>> =
            (0..100).map(|i| (format!("k{i}"), vec![i])).collect();
        let frame = map.clone();
        // Fill the own layer to its bound; the next write crosses it.
        let mut size = 100;
        while map.own.len() < map.bounds().0 {
            map.insert(format!("k{size}"), vec![size]);
            size += 1;
        }
        map.get_mut("k5").unwrap().push(99);
        assert_eq!(map.get("k5"), Some(&vec![5, 99]));
        map.entry_or_default("k7".to_owned()).push(7);
        assert_eq!(map.get("k7"), Some(&vec![7, 7]));
        map.entry_or_default("new".to_owned()).push(1);
        assert_eq!(map.get("new"), Some(&vec![1]));
        let size = size as usize + 1;
        assert_eq!(map.len(), size);
        assert_eq!(map.iter().count(), size);
        assert_eq!(map.remove("k5"), Some(vec![5, 99]));
        assert!(!map.contains_key("k5"));
        assert_eq!(map.len(), size - 1);
        assert_eq!(map.keys().count(), size - 1);
        assert_eq!(frame.get("k5"), Some(&vec![5]));
        assert_eq!(frame.get("k7"), Some(&vec![7]));
        assert_eq!(frame.len(), 100);
        map.clear();
        assert_eq!(map.len(), 0);
        assert_eq!(frame.len(), 100);
    }

    /// Frames taken between writes keep what they saw while the writes
    /// merge the own layer into the middle one and that into the base,
    /// removals included, and the map agrees with a plain one throughout.
    #[test]
    fn frames_keep_their_entries_across_both_merges() {
        let mut map = Layered::<String, usize>::default();
        let mut oracle = std::collections::HashMap::<String, usize>::new();
        let mut frames = Vec::new();
        let (mut own_merges, mut base_merges) = (0, 0);
        for i in 0..6000 {
            let (mid, base) = (map.mid.clone(), map.base.clone());
            let key = format!("k{}", i % 4500);
            assert_eq!(map.insert(key.clone(), i), oracle.insert(key, i));
            if i % 5 == 0 {
                let gone = format!("k{}", (i * 7) % 4500);
                assert_eq!(map.remove(&gone), oracle.remove(&gone));
            }
            if i % 11 == 0 {
                let key = format!("k{}", (i * 3) % 4500);
                if let Some(value) = map.get_mut(&key) {
                    *value += 1;
                    *oracle.get_mut(&key).unwrap() += 1;
                }
            }
            own_merges += usize::from(!std::sync::Arc::ptr_eq(&mid, &map.mid));
            base_merges += usize::from(!std::sync::Arc::ptr_eq(&base, &map.base));
            if i % 97 == 0 {
                frames.push((map.clone(), oracle.clone()));
            }
            assert_eq!(map.len(), oracle.len());
        }
        assert!(
            own_merges > 10 && base_merges > 1,
            "{own_merges} {base_merges}"
        );
        frames.push((map.clone(), oracle));
        map.clear();
        for (frame, expected) in frames {
            assert_eq!(frame.len(), expected.len());
            assert_eq!(frame.iter().count(), expected.len());
            for (key, value) in &expected {
                assert_eq!(frame.get(key), Some(value));
            }
            for i in 0..4500 {
                let key = format!("k{i}");
                assert_eq!(frame.get(&key), expected.get(&key));
            }
        }
    }
}
