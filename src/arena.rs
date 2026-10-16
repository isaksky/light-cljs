//! An append-mostly ID table whose frozen prefix is shared between programs.
//!
//! A deferred unit is analyzed from a snapshot of the program it was defined
//! in. Every table of that snapshot is a prefix of the live program's, so the
//! snapshot shares the frozen chunks by reference and only the entries added
//! after the last freeze are copied. Writes to a shared chunk copy that chunk
//! alone, so a snapshot keeps its definition-time entries.

use std::ops::{Index, IndexMut};
use std::rc::Rc;

pub struct Arena<T> {
    /// Frozen entries, shared with every table snapshotted from this one.
    chunks: Vec<Rc<Vec<T>>>,
    /// The ID of each chunk's first entry, parallel to `chunks`.
    starts: Vec<usize>,
    /// The number of frozen entries: the ID of the first tail entry.
    frozen: usize,
    /// Entries after the frozen chunks, owned by this table alone.
    tail: Vec<T>,
    /// The chunk of the last frozen entry looked up: nearby lookups are O(1).
    hint: std::cell::Cell<usize>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            starts: Vec::new(),
            frozen: 0,
            tail: Vec::new(),
            hint: std::cell::Cell::new(0),
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
            hint: std::cell::Cell::new(self.hint.get()),
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
                .all(|(a, b)| Rc::ptr_eq(a, b))
    }

    fn chunk(&self, id: usize) -> (usize, usize) {
        let hint = self.hint.get();
        if let Some(&start) = self.starts.get(hint)
            && start <= id
            && id - start < self.chunks[hint].len()
        {
            return (hint, id - start);
        }
        let chunk = self.starts.partition_point(|&start| start <= id) - 1;
        self.hint.set(chunk);
        (chunk, id - self.starts[chunk])
    }

    pub fn get(&self, id: usize) -> Option<&T> {
        if id >= self.frozen {
            return self.tail.get(id - self.frozen);
        }
        let (chunk, index) = self.chunk(id);
        self.chunks[chunk].get(index)
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
            chunk: 0,
        }
    }

    /// The entries with IDs in `range`.
    pub fn range(&self, range: std::ops::Range<usize>) -> Iter<'_, T> {
        Iter {
            arena: self,
            front: range.start.min(self.len()),
            back: range.end.min(self.len()),
            chunk: 0,
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
}

impl<T: Clone> Arena<T> {
    /// Move the tail into a frozen chunk, so a clone shares it instead of
    /// copying it. The entries keep their IDs. A new chunk at least half as
    /// long as the one before it is merged with it into a new chunk, so the
    /// chunks stay logarithmic in the entries: a clone shares that many and
    /// a lookup searches that many, while the merges copy each entry a
    /// logarithmic number of times in all. Chunks shared with a clone are
    /// copied, not changed.
    pub fn freeze(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        self.starts.push(self.frozen);
        self.frozen += self.tail.len();
        self.chunks.push(Rc::new(std::mem::take(&mut self.tail)));
        while self.chunks.len() >= 2 {
            let last = self.chunks.len() - 1;
            if self.chunks[last].len() * 2 < self.chunks[last - 1].len() {
                break;
            }
            let newer = self.chunks.pop().expect("chunk");
            self.starts.pop();
            let older = self.chunks.pop().expect("chunk");
            let start = self.starts.pop().expect("start");
            let mut merged = Vec::with_capacity(older.len() + newer.len());
            merged.extend(Rc::try_unwrap(older).unwrap_or_else(|chunk| (*chunk).clone()));
            merged.extend(Rc::try_unwrap(newer).unwrap_or_else(|chunk| (*chunk).clone()));
            self.starts.push(start);
            self.chunks.push(Rc::new(merged));
        }
        self.hint.set(0);
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
        Rc::make_mut(&mut self.chunks[chunk]).get_mut(index)
    }

    /// Copy every frozen chunk into the tail, so no entry is shared.
    pub fn unfreeze(&mut self) {
        if self.chunks.is_empty() {
            return;
        }
        let mut entries = Vec::with_capacity(self.len());
        for chunk in self.chunks.drain(..) {
            entries.extend(Rc::try_unwrap(chunk).unwrap_or_else(|chunk| (*chunk).clone()));
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
        let (chunk, index) = self.chunk(id);
        &self.chunks[chunk][index]
    }
}

impl<T: Clone> IndexMut<usize> for Arena<T> {
    fn index_mut(&mut self, id: usize) -> &mut T {
        if id >= self.frozen {
            return &mut self.tail[id - self.frozen];
        }
        let (chunk, index) = self.chunk(id);
        &mut Rc::make_mut(&mut self.chunks[chunk])[index]
    }
}

impl<'a, T> IntoIterator for &'a Arena<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        Iter {
            arena: self,
            front: 0,
            back: self.len(),
            chunk: 0,
        }
    }
}

pub struct Iter<'a, T> {
    arena: &'a Arena<T>,
    front: usize,
    back: usize,
    /// The chunk holding `front`, once found.
    chunk: usize,
}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            arena: self.arena,
            front: self.front,
            back: self.back,
            chunk: self.chunk,
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<&'a T> {
        if self.front >= self.back {
            return None;
        }
        let arena = self.arena;
        let entry = if self.front >= arena.frozen {
            &arena.tail[self.front - arena.frozen]
        } else {
            if self.chunk >= arena.chunks.len()
                || self.front < arena.starts[self.chunk]
                || self.front - arena.starts[self.chunk] >= arena.chunks[self.chunk].len()
            {
                self.chunk = arena.chunk(self.front).0;
            }
            &arena.chunks[self.chunk][self.front - arena.starts[self.chunk]]
        };
        self.front += 1;
        Some(entry)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back - self.front;
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }
        self.back -= 1;
        Some(&self.arena[self.back])
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
