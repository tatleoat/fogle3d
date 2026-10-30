// SPDX-License-Identifier: MIT
//! Copy-on-write pools for the large parts of a frame's state (unit blurmem1). Nothing here
//! reproduces a function of the original.
//!
//! The triangle and vertex pools, the texture coordinates, the pictures and the tree's nodes
//! and links are written while a frame is set up (the geometry stage, the motion-blur walk's
//! copies, the tree's build) and only read while its lines are sampled. The band threads
//! ([`crate::bands`]) each work on a copy of the scene state; with these pools a copy shares
//! the arrays instead of duplicating them (a motion-blurred frame can hold millions of
//! triangles: one copy per thread was most of the memory such a frame took).
//!
//! A [`Pool`] reads as the `Vec` it holds. A write through it (`DerefMut`) first makes the
//! array its own when another copy shares it, so a thread that writes sees exactly what a
//! private copy would have shown it, and the others are untouched: the output cannot depend
//! on the sharing. The writes that do happen while sampling (the shadow walk's "met" marks)
//! go to a scratch of the thread's own instead ([`crate::trace::MetMarks`]); [`copies`]
//! counts the arrays copied on a write while shared, which the bands' log line shows.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A `Vec<T>` shared between copies until one of them writes to it.
pub struct Pool<T>(Arc<Vec<T>>);

static COPIES: AtomicU64 = AtomicU64::new(0);
static COPIED_BYTES: AtomicU64 = AtomicU64::new(0);

/// How many pools were copied because one was written while shared, and their bytes, since
/// the process started.
pub fn copies() -> (u64, u64) {
    (COPIES.load(Ordering::Relaxed), COPIED_BYTES.load(Ordering::Relaxed))
}

impl<T> Pool<T> {
    pub fn new() -> Pool<T> {
        Pool(Arc::new(Vec::new()))
    }

    /// Whether another copy shares the array.
    pub fn is_shared(&self) -> bool {
        Arc::strong_count(&self.0) > 1
    }

    /// The bytes of the array's items (not its spare capacity).
    pub fn bytes(&self) -> usize {
        self.0.len() * std::mem::size_of::<T>()
    }
}

impl<T: Clone> Pool<T> {
    /// The array, copied when shared.
    pub fn into_vec(self) -> Vec<T> {
        Arc::try_unwrap(self.0).unwrap_or_else(|a| (*a).clone())
    }
}

impl<T> Default for Pool<T> {
    fn default() -> Pool<T> {
        Pool::new()
    }
}

impl<T> Clone for Pool<T> {
    /// Another handle on the same array (no items are copied).
    fn clone(&self) -> Pool<T> {
        Pool(Arc::clone(&self.0))
    }
}

impl<T> Deref for Pool<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Clone> DerefMut for Pool<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        if Arc::get_mut(&mut self.0).is_none() {
            COPIES.fetch_add(1, Ordering::Relaxed);
            COPIED_BYTES.fetch_add(self.bytes() as u64, Ordering::Relaxed);
        }
        Arc::make_mut(&mut self.0)
    }
}

impl<T: fmt::Debug> fmt::Debug for Pool<T> {
    /// As the `Vec`'s.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq> PartialEq for Pool<T> {
    fn eq(&self, other: &Pool<T>) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl<T> From<Vec<T>> for Pool<T> {
    fn from(v: Vec<T>) -> Pool<T> {
        Pool(Arc::new(v))
    }
}

impl<T> FromIterator<T> for Pool<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Pool<T> {
        Pool(Arc::new(iter.into_iter().collect()))
    }
}

impl<'a, T> IntoIterator for &'a Pool<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> std::slice::Iter<'a, T> {
        self.0.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut Pool<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> std::slice::IterMut<'a, T> {
        self.deref_mut().iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_while_shared_copies_and_leaves_the_other_alone() {
        let mut a: Pool<u32> = vec![1, 2, 3].into();
        let b = a.clone();
        assert!(a.is_shared() && Arc::ptr_eq(&a.0, &b.0));
        let before = copies().0;
        a[1] = 7;
        assert!(copies().0 > before);
        assert_eq!(*a, vec![1, 7, 3]);
        assert_eq!(*b, vec![1, 2, 3]);
        assert!(!a.is_shared() && !b.is_shared());
        // a write to an array no other copy holds copies nothing
        let mut c: Pool<u32> = vec![5].into();
        c.push(6);
        assert_eq!(format!("{c:?}"), format!("{:?}", vec![5u32, 6]));
        assert_eq!(c.into_vec(), vec![5, 6]);
    }
}
