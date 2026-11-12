//! [`ArcMap`]: an ordered persistent map whose values sit behind [`Arc`].
//!
//! `im::OrdMap` keeps 64 entries inline per B-tree node and moves whole
//! nodes through the stack while inserting, removing and cloning. The stack
//! an operation needs grows with the entry size: measured in a debug build
//! at 20 000 entries (insert, clone, remove), 16-byte entries need about
//! 260 KiB, 144-byte ones (an event) about 1.7 MiB and 392-byte ones (a
//! strip; a sequence is larger) about 4.2 MiB, which overflows a 2 MiB test
//! thread and a 512 KiB Swift cooperative-pool thread (a release build
//! needs about 820 KiB for 416-byte entries). Behind an
//! `Arc` every entry is the key plus one pointer, so the stack stays small
//! whatever the value; cloning the map still shares structure, and a value
//! is copied only when it is changed while shared.
//!
//! The API mirrors the parts of `OrdMap` the model uses, handing out `&V`.

use im::OrdMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::fmt;
use std::ops::RangeBounds;
use std::sync::Arc;

/// An ordered persistent map with boxed values (see the module docs).
#[derive(Clone)]
pub struct ArcMap<K, V> {
    inner: OrdMap<K, Shared<V>>,
}

/// A shared value that compares equal at once when both sides are the same
/// allocation, so comparing or diffing two versions of a map costs only the
/// entries that changed.
#[derive(Debug)]
struct Shared<V>(Arc<V>);

impl<V> Clone for Shared<V> {
    fn clone(&self) -> Self {
        Shared(Arc::clone(&self.0))
    }
}

impl<V: PartialEq> PartialEq for Shared<V> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl<V> Shared<V> {
    fn new(v: V) -> Self {
        Shared(Arc::new(v))
    }
    fn get(&self) -> &V {
        &self.0
    }
}

impl<V: Clone> Shared<V> {
    fn make_mut(&mut self) -> &mut V {
        Arc::make_mut(&mut self.0)
    }
    fn into_inner(self) -> V {
        Arc::unwrap_or_clone(self.0)
    }
}

impl<K: Ord + Clone, V: Clone> Default for ArcMap<K, V> {
    fn default() -> Self {
        ArcMap::new()
    }
}

impl<K: Ord + Clone, V: Clone> ArcMap<K, V> {
    /// An empty map.
    pub fn new() -> Self {
        ArcMap {
            inner: OrdMap::new(),
        }
    }

    /// A map holding one entry.
    pub fn unit(k: K, v: V) -> Self {
        ArcMap {
            inner: OrdMap::unit(k, Shared::new(v)),
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// No entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// The value at `k`.
    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        Q: Ord + ?Sized,
        K: Borrow<Q>,
    {
        self.inner.get(k).map(Shared::get)
    }

    /// The value at `k`, copied first when shared.
    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        Q: Ord + ?Sized,
        K: Borrow<Q>,
    {
        self.inner.get_mut(k).map(Shared::make_mut)
    }

    /// Whether `k` has an entry.
    pub fn contains_key<Q>(&self, k: &Q) -> bool
    where
        Q: Ord + ?Sized,
        K: Borrow<Q>,
    {
        self.inner.contains_key(k)
    }

    /// Sets `k` to `v`; returns the value it replaced.
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        self.inner.insert(k, Shared::new(v)).map(Shared::into_inner)
    }

    /// Removes `k`; returns its value.
    pub fn remove<Q>(&mut self, k: &Q) -> Option<V>
    where
        Q: Ord + ?Sized,
        K: Borrow<Q>,
    {
        self.inner.remove(k).map(Shared::into_inner)
    }

    /// Keys in order.
    pub fn keys(&self) -> Keys<'_, K, V> {
        Keys {
            inner: self.inner.keys(),
        }
    }

    /// Values in key order.
    pub fn values(&self) -> Values<'_, K, V> {
        Values {
            inner: self.inner.values(),
        }
    }

    /// Entries in key order.
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            inner: self.inner.iter(),
        }
    }

    /// Entries whose keys are in `range`, in order.
    pub fn range<R, BK>(&self, range: R) -> Iter<'_, K, V>
    where
        R: RangeBounds<BK>,
        K: Borrow<BK>,
        BK: Ord + ?Sized,
    {
        Iter {
            inner: self.inner.range(range),
        }
    }

    /// The entry with the smallest key.
    pub fn get_min(&self) -> Option<(&K, &V)> {
        self.inner.get_min().map(|(k, v)| (k, v.get()))
    }

    /// The entry with the largest key.
    pub fn get_max(&self) -> Option<(&K, &V)> {
        self.inner.get_max().map(|(k, v)| (k, v.get()))
    }

    /// The keys whose entries differ between `self` and `other` (added,
    /// removed or changed), in order. Shared parts of the two maps are
    /// skipped, so this costs about the number of changed entries.
    pub fn changed_keys<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = &'a K> + 'a
    where
        V: PartialEq,
    {
        self.inner.diff(&other.inner).map(|d| match d {
            im::ordmap::DiffItem::Add(k, _)
            | im::ordmap::DiffItem::Remove(k, _)
            | im::ordmap::DiffItem::Update { new: (k, _), .. } => k,
        })
    }

    /// Whether both maps share all their storage (a cheap equality test).
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.inner.ptr_eq(&other.inner)
    }
}

/// Iterator over an [`ArcMap`]'s entries.
pub struct Iter<'a, K, V> {
    inner: im::ordmap::Iter<'a, K, Shared<V>>,
}

impl<'a, K: Ord + Clone, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, v)| (k, v.get()))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K: Ord + Clone, V> DoubleEndedIterator for Iter<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(|(k, v)| (k, v.get()))
    }
}

/// Iterator over an [`ArcMap`]'s keys.
pub struct Keys<'a, K, V> {
    inner: im::ordmap::Keys<'a, K, Shared<V>>,
}

impl<'a, K: Ord + Clone, V> Iterator for Keys<'a, K, V> {
    type Item = &'a K;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K: Ord + Clone, V> DoubleEndedIterator for Keys<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

/// Iterator over an [`ArcMap`]'s values.
pub struct Values<'a, K, V> {
    inner: im::ordmap::Values<'a, K, Shared<V>>,
}

impl<'a, K: Ord + Clone, V> Iterator for Values<'a, K, V> {
    type Item = &'a V;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(Shared::get)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K: Ord + Clone, V> DoubleEndedIterator for Values<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(Shared::get)
    }
}

impl<'a, K: Ord + Clone, V: Clone> IntoIterator for &'a ArcMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K: Ord + Clone, V: Clone> FromIterator<(K, V)> for ArcMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(it: I) -> Self {
        ArcMap {
            inner: it.into_iter().map(|(k, v)| (k, Shared::new(v))).collect(),
        }
    }
}

impl<K: Ord + Clone, V: Clone> Extend<(K, V)> for ArcMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, it: I) {
        for (k, v) in it {
            self.insert(k, v);
        }
    }
}

impl<K: Ord + Clone, V: Clone, const N: usize> From<[(K, V); N]> for ArcMap<K, V> {
    fn from(a: [(K, V); N]) -> Self {
        a.into_iter().collect()
    }
}

/// Panics when `k` has no entry, like `OrdMap`'s index (kept so the maps
/// read as before; model code itself uses [`ArcMap::get`]).
impl<K, V, Q> std::ops::Index<&Q> for ArcMap<K, V>
where
    K: Ord + Clone + Borrow<Q>,
    V: Clone,
    Q: Ord + ?Sized,
{
    type Output = V;
    #[allow(clippy::indexing_slicing)]
    fn index(&self, k: &Q) -> &V {
        self.inner[k].get()
    }
}

impl<K: Ord + Clone, V: Clone + PartialEq> PartialEq for ArcMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        // Shared nodes and shared values are skipped.
        self.ptr_eq(other) || self.inner == other.inner
    }
}

impl<K: Ord + Clone, V: Clone + Eq> Eq for ArcMap<K, V> {}

impl<K: Ord + Clone + fmt::Debug, V: Clone + fmt::Debug> fmt::Debug for ArcMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Serialized like an `OrdMap`: a map in key order.
impl<K: Ord + Clone + Serialize, V: Clone + Serialize> Serialize for ArcMap<K, V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.iter())
    }
}

impl<'de, K, V> Deserialize<'de> for ArcMap<K, V>
where
    K: Ord + Clone + Deserialize<'de>,
    V: Clone + Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = std::collections::BTreeMap::<K, V>::deserialize(d)?;
        Ok(m.into_iter().collect())
    }
}
