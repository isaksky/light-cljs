//! An append-mostly ID table whose frozen prefix is shared between programs.
//!
//! A deferred unit is analyzed from a snapshot of the program it was defined
//! in. Every table of that snapshot is a prefix of the live program's, so the
//! snapshot shares the frozen chunks by reference and only the entries added
//! after the last freeze are copied. Frozen chunks are trees of bounded leaves:
//! merging chunks shares their entries, and writes copy only the path to a
//! leaf. A snapshot keeps its definition-time entries.

use std::ops::{Index, IndexMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const LEAF_SIZE: usize = 64;

#[derive(Clone)]
enum Chunk<T> {
    Leaf(Vec<T>),
    Branch {
        left: Arc<Chunk<T>>,
        right: Arc<Chunk<T>>,
        len: usize,
    },
}

impl<T> Chunk<T> {
    fn len(&self) -> usize {
        match self {
            Self::Leaf(entries) => entries.len(),
            Self::Branch { len, .. } => *len,
        }
    }

    fn join(left: Arc<Self>, right: Arc<Self>) -> Self {
        let len = left.len() + right.len();
        Self::Branch { left, right, len }
    }

    fn from_entries(entries: Vec<T>) -> Self {
        if entries.len() <= LEAF_SIZE {
            return Self::Leaf(entries);
        }
        fn build<T>(entries: &mut std::vec::IntoIter<T>, len: usize) -> Chunk<T> {
            if len <= LEAF_SIZE {
                return Chunk::Leaf(entries.take(len).collect());
            }
            let left = Arc::new(build(entries, len / 2));
            let right = Arc::new(build(entries, len - len / 2));
            Chunk::join(left, right)
        }
        let len = entries.len();
        build(&mut entries.into_iter(), len)
    }

    /// The leaf containing `index`, and its starting offset in this chunk.
    fn leaf(&self, mut index: usize) -> (usize, &[T]) {
        let mut node = self;
        let mut start = 0;
        loop {
            match node {
                Self::Leaf(entries) => return (start, entries),
                Self::Branch { left, right, .. } => {
                    if index < left.len() {
                        node = left;
                    } else {
                        index -= left.len();
                        start += left.len();
                        node = right;
                    }
                }
            }
        }
    }
}

impl<T: Clone> Chunk<T> {
    fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        match self {
            Self::Leaf(entries) => entries.get_mut(index),
            Self::Branch { left, right, .. } => {
                if index < left.len() {
                    Arc::make_mut(left).get_mut(index)
                } else {
                    Arc::make_mut(right).get_mut(index - left.len())
                }
            }
        }
    }

    fn append_entries(chunk: Arc<Self>, entries: &mut Vec<T>) {
        match Arc::try_unwrap(chunk) {
            Ok(Self::Leaf(mut leaf)) => entries.append(&mut leaf),
            Ok(Self::Branch { left, right, .. }) => {
                Self::append_entries(left, entries);
                Self::append_entries(right, entries);
            }
            Err(chunk) => chunk.copy_entries(entries),
        }
    }

    fn copy_entries(&self, entries: &mut Vec<T>) {
        match self {
            Self::Leaf(leaf) => entries.extend_from_slice(leaf),
            Self::Branch { left, right, .. } => {
                left.copy_entries(entries);
                right.copy_entries(entries);
            }
        }
    }
}

pub struct Arena<T> {
    /// Frozen entries, shared with every table snapshotted from this one.
    chunks: Vec<Arc<Chunk<T>>>,
    /// The ID of each chunk's first entry, parallel to `chunks`.
    starts: Vec<usize>,
    /// The number of frozen entries: the ID of the first tail entry.
    frozen: usize,
    /// Entries after the frozen chunks, owned by this table alone.
    tail: Vec<T>,
    /// The chunk of the last frozen entry looked up: nearby lookups are O(1).
    /// Relaxed: a stale or racing value is only a missed hint, never a wrong
    /// lookup, and on the targets we build it costs what a `Cell` does.
    hint: AtomicUsize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            starts: Vec::new(),
            frozen: 0,
            tail: Vec::new(),
            hint: AtomicUsize::new(0),
        }
    }
}

impl<T> Clone for Arena<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            chunks: self.chunks.clone(),
            starts: self.starts.clone(),
            frozen: self.frozen,
            tail: self.tail.clone(),
            hint: AtomicUsize::new(self.hint.load(Relaxed)),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Arena<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for Arena<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: Eq> Eq for Arena<T> {}

impl<T, U> PartialEq<Vec<U>> for Arena<T>
where
    T: PartialEq<U>,
{
    fn eq(&self, other: &Vec<U>) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(a, b)| a == b)
    }
}

impl<T, U, const N: usize> PartialEq<[U; N]> for Arena<T>
where
    T: PartialEq<U>,
{
    fn eq(&self, other: &[U; N]) -> bool {
        self.len() == N && self.iter().zip(other).all(|(a, b)| a == b)
    }
}

impl<T> From<Vec<T>> for Arena<T> {
    fn from(tail: Vec<T>) -> Self {
        Self {
            tail,
            ..Self::default()
        }
    }
}

impl<T> FromIterator<T> for Arena<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.frozen + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of entries in frozen, shareable chunks.
    pub fn frozen(&self) -> usize {
        self.frozen
    }

    pub fn push(&mut self, value: T) {
        self.tail.push(value);
    }

    /// Release spare capacity before retaining a completed analysis. Frozen
    /// entries stay shared; no IDs or values change.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.chunks.shrink_to_fit();
        self.starts.shrink_to_fit();
        self.tail.shrink_to_fit();
    }

    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.tail.extend(iter);
    }

    /// Whether the first `len` entries of both tables are the same shared
    /// chunks, so they are equal without comparing entries.
    pub fn shares_prefix(&self, other: &Self, len: usize) -> bool {
        if len == 0 {
            return true;
        }
        if len > self.frozen || len > other.frozen {
            return false;
        }
        let chunks = self.starts.partition_point(|&start| start < len);
        chunks == other.starts.partition_point(|&start| start < len)
            && self.starts[chunks - 1] + self.chunks[chunks - 1].len() >= len
            && self.chunks[..chunks]
                .iter()
                .zip(&other.chunks[..chunks])
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// The largest frozen subtree that starts at `id`, holds at most `max`
    /// entries and ends by `end`, or else the leaf holding `id`, with where
    /// it starts; `None` for an ID in the tail.
    fn subtree(&self, id: usize, max: usize, end: usize) -> Option<(usize, &Chunk<T>)> {
        if id >= self.frozen {
            return None;
        }
        let (chunk, _) = self.chunk(id);
        let mut start = self.starts[chunk];
        let mut node: &Chunk<T> = &self.chunks[chunk];
        loop {
            if start == id && node.len() <= max && start + node.len() <= end {
                return Some((start, node));
            }
            match node {
                Chunk::Leaf(_) => return Some((start, node)),
                Chunk::Branch { left, right, .. } => {
                    if id < start + left.len() {
                        node = left;
                    } else {
                        start += left.len();
                        node = right;
                    }
                }
            }
        }
    }

    /// The IDs below `len` whose entries differ from `before`'s, a table
    /// this one was cloned from. Subtrees the two still share are skipped
    /// without comparing their entries, at the largest size they share: only
    /// the paths a write copied, and the tail, are compared, so the cost
    /// follows the changes, not the table's size.
    pub fn changed_entries(&self, before: &Self, len: usize) -> Vec<usize>
    where
        T: PartialEq,
    {
        let len = len.min(self.len()).min(before.len());
        let mut changed = Vec::new();
        let mut id = 0;
        let mut max = usize::MAX;
        while id < len {
            if let (Some((start, old)), Some((current_start, current))) =
                (before.subtree(id, max, len), self.subtree(id, max, len))
            {
                if start == current_start && std::ptr::eq(old, current) {
                    id = (start + old.len()).min(len);
                    max = usize::MAX;
                    continue;
                }
                // Look for shared subtrees below the larger of the two.
                if matches!(old, Chunk::Branch { .. }) || matches!(current, Chunk::Branch { .. }) {
                    max = old.len().min(current.len()).saturating_sub(1);
                    continue;
                }
            }
            max = usize::MAX;
            let (start, leaf) = before.leaf(id);
            let (current_start, current) = self.leaf(id);
            if start == current_start
                && leaf.len() == current.len()
                && std::ptr::eq(leaf.as_ptr(), current.as_ptr())
            {
                id = (start + leaf.len()).min(len);
                continue;
            }
            let stop = (start + leaf.len())
                .min(current_start + current.len())
                .min(len);
            for index in id..stop {
                if leaf[index - start] != current[index - current_start] {
                    changed.push(index);
                }
            }
            id = stop;
        }
        changed
    }

    fn chunk(&self, id: usize) -> (usize, usize) {
        let hint = self.hint.load(Relaxed);
        if let Some(&start) = self.starts.get(hint)
            && start <= id
            && id - start < self.chunks[hint].len()
        {
            return (hint, id - start);
        }
        let chunk = self.starts.partition_point(|&start| start <= id) - 1;
        self.hint.store(chunk, Relaxed);
        (chunk, id - self.starts[chunk])
    }

    pub fn get(&self, id: usize) -> Option<&T> {
        if id >= self.frozen {
            return self.tail.get(id - self.frozen);
        }
        let (chunk, index) = self.chunk(id);
        let (start, leaf) = self.chunks[chunk].leaf(index);
        leaf.get(index - start)
    }

    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }

    pub fn last(&self) -> Option<&T> {
        self.len().checked_sub(1).and_then(|id| self.get(id))
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            arena: self,
            front: 0,
            back: self.len(),
            front_leaf: (0, &[]),
            back_leaf: (0, &[]),
        }
    }

    /// The entries with IDs in `range`.
    pub fn range(&self, range: std::ops::Range<usize>) -> Iter<'_, T> {
        Iter {
            arena: self,
            front: range.start.min(self.len()),
            back: range.end.min(self.len()),
            front_leaf: (0, &[]),
            back_leaf: (0, &[]),
        }
    }

    /// The entries from ID `start` on.
    pub fn tail_from(&self, start: usize) -> Iter<'_, T> {
        self.range(start..self.len())
    }

    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.iter().any(|entry| entry == value)
    }

    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.iter().cloned().collect()
    }

    fn leaf(&self, id: usize) -> (usize, &[T]) {
        if id >= self.frozen {
            return (self.frozen, &self.tail);
        }
        let (chunk, index) = self.chunk(id);
        let (start, leaf) = self.chunks[chunk].leaf(index);
        (self.starts[chunk] + start, leaf)
    }
}

impl<T: Clone> Arena<T> {
    /// Move the tail into a frozen chunk, so a clone shares it instead of
    /// copying it. The entries keep their IDs. A new chunk at least half as
    /// long as the one before it is merged with it into a new chunk, so the
    /// chunks stay logarithmic in the entries: a clone shares that many and
    /// a lookup searches that many. A merge joins tree roots without copying
    /// entries retained by earlier snapshots.
    pub fn freeze(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        self.starts.push(self.frozen);
        self.frozen += self.tail.len();
        self.chunks
            .push(Arc::new(Chunk::from_entries(std::mem::take(
                &mut self.tail,
            ))));
        while self.chunks.len() >= 2 {
            let last = self.chunks.len() - 1;
            if self.chunks[last].len() * 2 < self.chunks[last - 1].len() {
                break;
            }
            let newer = self.chunks.pop().expect("chunk");
            self.starts.pop();
            let older = self.chunks.pop().expect("chunk");
            let start = self.starts.pop().expect("start");
            self.starts.push(start);
            self.chunks.push(Arc::new(Chunk::join(older, newer)));
        }
        *self.hint.get_mut() = 0;
    }

    pub fn extend_from_slice(&mut self, entries: &[T]) {
        self.tail.extend_from_slice(entries);
    }

    /// Every entry mutably; the frozen chunks are copied first.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.unfreeze();
        self.tail.iter_mut()
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        if id >= self.frozen {
            return self.tail.get_mut(id - self.frozen);
        }
        let (chunk, index) = self.chunk(id);
        Arc::make_mut(&mut self.chunks[chunk]).get_mut(index)
    }

    /// Copy every frozen chunk into the tail, so no entry is shared.
    pub fn unfreeze(&mut self) {
        if self.chunks.is_empty() {
            return;
        }
        let mut entries = Vec::with_capacity(self.len());
        for chunk in self.chunks.drain(..) {
            Chunk::append_entries(chunk, &mut entries);
        }
        entries.append(&mut self.tail);
        self.starts.clear();
        self.frozen = 0;
        self.tail = entries;
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.frozen {
            self.unfreeze();
        }
        self.tail.truncate(len.saturating_sub(self.frozen));
    }
}

impl<T> Index<usize> for Arena<T> {
    type Output = T;
    fn index(&self, id: usize) -> &T {
        if id >= self.frozen {
            return &self.tail[id - self.frozen];
        }
        let (start, leaf) = self.leaf(id);
        &leaf[id - start]
    }
}

impl<T: Clone> IndexMut<usize> for Arena<T> {
    fn index_mut(&mut self, id: usize) -> &mut T {
        if id >= self.frozen {
            return &mut self.tail[id - self.frozen];
        }
        let (chunk, index) = self.chunk(id);
        Arc::make_mut(&mut self.chunks[chunk])
            .get_mut(index)
            .expect("arena index")
    }
}

impl<'a, T> IntoIterator for &'a Arena<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

pub struct Iter<'a, T> {
    arena: &'a Arena<T>,
    front: usize,
    back: usize,
    /// Leaves for sequential access without descending the tree per entry.
    front_leaf: (usize, &'a [T]),
    back_leaf: (usize, &'a [T]),
}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            arena: self.arena,
            front: self.front,
            back: self.back,
            front_leaf: self.front_leaf,
            back_leaf: self.back_leaf,
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        if n >= self.back.saturating_sub(self.front) {
            self.front = self.back;
            return None;
        }
        self.front += n;
        self.next()
    }

    fn count(self) -> usize {
        self.back.saturating_sub(self.front)
    }

    fn last(mut self) -> Option<Self::Item> {
        self.next_back()
    }

    fn next(&mut self) -> Option<&'a T> {
        if self.front >= self.back {
            return None;
        }
        let (start, leaf) = self.front_leaf;
        if self.front < start || self.front - start >= leaf.len() {
            self.front_leaf = self.arena.leaf(self.front);
        }
        let entry = &self.front_leaf.1[self.front - self.front_leaf.0];
        self.front += 1;
        Some(entry)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back - self.front;
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        if n >= self.back.saturating_sub(self.front) {
            self.front = self.back;
            return None;
        }
        self.back -= n;
        self.next_back()
    }

    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }
        self.back -= 1;
        let (start, leaf) = self.back_leaf;
        if self.back < start || self.back - start >= leaf.len() {
            self.back_leaf = self.arena.leaf(self.back);
        }
        Some(&self.back_leaf.1[self.back - self.back_leaf.0])
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

impl<'a, T: Clone> IntoIterator for &'a mut Arena<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> std::slice::IterMut<'a, T> {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::Arena;

    #[test]
    fn snapshots_do_not_copy_entries_and_writes_copy_a_bounded_leaf() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct Counted(Rc<Cell<usize>>);
        impl Clone for Counted {
            fn clone(&self) -> Self {
                self.0.set(self.0.get() + 1);
                Self(self.0.clone())
            }
        }
        let copies = Rc::new(Cell::new(0));
        let mut arena = Arena::new();
        let mut versions = Vec::new();
        for _ in 0..300 {
            arena.extend((0..70).map(|_| Counted(copies.clone())));
            arena.freeze();
            versions.push(arena.clone());
        }
        assert_eq!(copies.get(), 0, "freezing and merging only share entries");
        arena[2000] = Counted(copies.clone());
        assert!(copies.get() <= super::LEAF_SIZE);
        assert_eq!(versions.last().unwrap().len(), 21000);
    }

    #[test]
    fn changed_entries_compares_only_leaves_a_write_copied() {
        let mut arena: Arena<usize> = (0..1000).collect();
        arena.freeze();
        let before = arena.clone();
        arena.extend(1000..1100);
        arena.freeze();
        arena[3] = 33;
        arena[700] = 700;
        arena[999] = 0;
        assert_eq!(arena.changed_entries(&before, 1000), [3, 999]);
        assert_eq!(arena.changed_entries(&before, 900), [3]);
        assert_eq!(
            before.changed_entries(&before.clone(), 1000),
            Vec::<usize>::new()
        );
        // A table whose entries were all copied out is compared entry by entry.
        let mut copied = arena.clone();
        copied.unfreeze();
        copied[10] = 1;
        assert_eq!(copied.changed_entries(&before, 1000), [3, 10, 999]);
    }

    #[test]
    fn changed_entries_skip_shared_subtrees_across_merges() {
        // Versions frozen in many steps, merged into trees of several
        // depths, then written: the answer is the entry-by-entry one.
        let mut arena: Arena<usize> = Arena::new();
        for step in 0..40 {
            arena.extend((0..(step * 37 % 300 + 1)).map(|i| step * 1000 + i));
            arena.freeze();
        }
        let before = arena.clone();
        let len = before.len();
        arena.extend(0..500);
        arena.freeze();
        for id in [0, 1, 63, 64, 65, len / 3, len / 2, len - 2] {
            arena[id] = usize::MAX;
        }
        arena[len / 2] = before[len / 2];
        let naive = (0..len)
            .filter(|&id| arena[id] != before[id])
            .collect::<Vec<_>>();
        assert_eq!(arena.changed_entries(&before, len), naive);
        assert_eq!(arena.changed_entries(&before, len / 3), naive[..5]);
        assert!(before.changed_entries(&before.clone(), len).is_empty());
    }

    #[test]
    fn mixed_direction_iteration_crosses_leaves_and_chunks() {
        let mut arena = Arena::new();
        for count in [900, 170, 13] {
            let start = arena.len();
            arena.extend(start..start + count);
            arena.freeze();
        }
        arena.extend(arena.len()..1200);
        let mut entries = arena.range(11..1192);
        let mut expected = 11..1192;
        while !expected.is_empty() {
            assert_eq!(entries.len(), expected.len());
            assert_eq!(entries.next().copied(), expected.next());
            assert_eq!(entries.next_back().copied(), expected.next_back());
        }
        assert_eq!(entries.next(), None);
        assert_eq!(entries.next_back(), None);
        assert_eq!(
            arena.iter().rev().copied().collect::<Vec<_>>(),
            (0..1200).rev().collect::<Vec<_>>()
        );
        let mut entries = arena.iter();
        assert_eq!(entries.nth(299), Some(&299));
        assert_eq!(entries.nth_back(299), Some(&900));
        assert_eq!(entries.clone().count(), 600);
        assert_eq!(entries.clone().last(), Some(&899));
        assert_eq!(entries.nth(usize::MAX), None);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn frozen_versions_preserve_values_and_ids() {
        let mut arena = Arena::new();
        let mut oracle = Vec::new();
        let mut versions = Vec::new();
        for round in 0..40u32 {
            for i in 0..(round % 5 + 1) {
                arena.push(round * 10 + i);
                oracle.push(round * 10 + i);
            }
            arena.freeze();
            versions.push((arena.clone(), oracle.clone()));
        }
        assert!(arena.chunks.len() <= 10, "{} chunks", arena.chunks.len());
        for (version, expected) in &versions {
            assert_eq!(version.len(), expected.len());
            assert_eq!(version.iter().copied().collect::<Vec<_>>(), *expected);
            assert!((0..expected.len()).all(|id| version[id] == expected[id]));
            let (start, end) = (3.min(expected.len()), expected.len().min(17));
            assert_eq!(
                version.range(start..end).copied().collect::<Vec<_>>(),
                expected[start..end]
            );
            assert_eq!(version.last(), expected.last());
        }
    }

    #[test]
    fn frozen_clone_mutation_and_truncation_are_isolated() {
        let mut arena: Arena<u32> = (0..10).collect();
        arena.freeze();
        let snapshot = arena.clone();
        arena[3] = 33;
        *arena.get_mut(4).unwrap() = 44;
        for value in arena.iter_mut() {
            *value += 100;
        }
        arena.extend([7, 8]);
        arena.truncate(6);
        assert_eq!(arena.to_vec(), vec![100, 101, 102, 133, 144, 105]);
        assert_eq!(snapshot.to_vec(), (0..10).collect::<Vec<_>>());
        let mut again = snapshot.clone();
        again.truncate(4);
        again.push(99);
        assert_eq!(again.to_vec(), vec![0, 1, 2, 3, 99]);
        assert_eq!(snapshot.to_vec(), (0..10).collect::<Vec<_>>());
        assert!(snapshot.shares_prefix(&snapshot.clone(), 10));
        assert!(!again.shares_prefix(&snapshot, 4));
    }

    #[test]
    fn concurrent_frozen_reads_preserve_values() {
        let mut arena = Arena::new();
        for count in [900, 170, 13, 64, 1] {
            let start = arena.len();
            arena.extend(start..start + count);
            arena.freeze();
        }
        arena.extend(arena.len()..1200);
        let snapshot = arena.clone();
        let len = arena.len();
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for thread in 0..4 {
                let (arena, snapshot, barrier) = (&arena, &snapshot, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    // Strided orders cross chunks on every read, so the
                    // threads keep overwriting each other's lookup hint.
                    for round in 0..20 {
                        for i in 0..len {
                            let id = (i * 7919 + thread * 131 + round) % len;
                            assert_eq!(arena.get(id), Some(&id));
                            assert_eq!(snapshot[id], id);
                        }
                    }
                    assert!(arena.iter().copied().eq(0..len));
                    assert!(snapshot.range(11..1192).copied().eq(11..1192));
                    assert_eq!(arena.get(len), None);
                });
            }
        });
    }
}
