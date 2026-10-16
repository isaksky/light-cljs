//! A value shared between programs until one of them changes it.

use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// A program table shared by reference between a program and the snapshots
/// taken of it. Reading goes through [`Deref`]; writing through [`DerefMut`]
/// copies the table first if another program still shares it, so a snapshot
/// keeps the table as it was.
pub struct Shared<T>(Rc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
}

impl<T: Clone> Shared<T> {
    /// The value itself, copied if another program shares it.
    pub fn into_inner(self) -> T {
        Rc::try_unwrap(self.0).unwrap_or_else(|rc| (*rc).clone())
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
        Rc::make_mut(&mut self.0)
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self(Rc::new(T::default()))
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
        Rc::ptr_eq(&self.0, &other.0) || **self == **other
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
