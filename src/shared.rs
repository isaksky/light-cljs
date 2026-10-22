//! A value shared between programs until one of them changes it.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A program table shared by reference between a program and the snapshots
/// taken of it. Reading goes through [`Deref`]; writing through [`DerefMut`]
/// copies the table first if another program still shares it, so a snapshot
/// keeps the table as it was.
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// Whether `a` and `b` are the same table, which neither has changed
    /// since they began sharing it.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T: Clone> Shared<T> {
    /// The value itself, copied if another program shares it.
    pub fn into_inner(self) -> T {
        Arc::try_unwrap(self.0).unwrap_or_else(|rc| (*rc).clone())
    }

    /// An empty extension is a read, not a write. In particular, fragment
    /// iterators often filter out every entry because the prefix is shared.
    pub fn extend<A>(&mut self, entries: impl IntoIterator<Item = A>)
    where
        T: Extend<A>,
    {
        let mut entries = entries.into_iter().peekable();
        if entries.peek().is_some() {
            Arc::make_mut(&mut self.0).extend(entries);
        }
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self(Arc::new(T::default()))
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || **self == **other
    }
}

impl<T, U, const N: usize> PartialEq<[U; N]> for Shared<T>
where
    T: PartialEq<[U; N]>,
{
    fn eq(&self, other: &[U; N]) -> bool {
        **self == *other
    }
}

impl<T, U> PartialEq<Vec<U>> for Shared<T>
where
    T: PartialEq<Vec<U>>,
{
    fn eq(&self, other: &Vec<U>) -> bool {
        **self == *other
    }
}

impl<T, U> PartialEq<&[U]> for Shared<T>
where
    T: PartialEq<[U]>,
{
    fn eq(&self, other: &&[U]) -> bool {
        **self == **other
    }
}

impl<T: Eq> Eq for Shared<T> {}

impl<'a, T> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (**self).into_iter()
    }
}

impl<T, A> FromIterator<A> for Shared<T>
where
    T: FromIterator<A>,
{
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Self::new(T::from_iter(iter))
    }
}

#[cfg(test)]
mod tests {
    use super::Shared;
    use std::sync::Arc;

    #[test]
    fn empty_fragment_extension_keeps_the_shared_table() {
        let mut table = Shared::new(vec![1, 2, 3]);
        let snapshot = table.clone();
        table.extend([4, 5].into_iter().filter(|_| false));
        assert!(Arc::ptr_eq(&table.0, &snapshot.0));
        table.extend([4]);
        assert_eq!(table, [1, 2, 3, 4]);
        assert_eq!(snapshot, [1, 2, 3]);
    }
}
