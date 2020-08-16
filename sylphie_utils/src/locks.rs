use dashmap::DashMap;
use futures::Future;
use futures::task::{Waker, Context, Poll};
use fxhash::FxBuildHasher;
use std::hash::Hash;
use std::pin::Pin;

/// A set of locks keyed on a value.
pub struct LockSet<K: Clone + Hash + Eq + Send + Sync + 'static> {
    locks: DashMap<K, Vec<Waker>, FxBuildHasher>,
}
impl <K: Clone + Hash + Eq + Send + Sync + 'static> LockSet<K> {
    /// Creates a new lock set.
    pub fn new() -> Self {
        Default::default()
    }

    /// Locks a given key.
    pub fn lock<'a>(&'a self, key: K) -> impl Future<Output = LockSetGuard<'a, K>> + 'a {
        WaitForLockSetFut { key, parent: self }
    }

    /// Locks a given key, if it is not already locked.
    pub fn try_lock(&self, key: K) -> Option<LockSetGuard<'_, K>> {
        let entry = self.locks.entry(key.clone()).or_default();
        if entry.is_empty() {
            Some(LockSetGuard { key, parent: self })
        } else {
            None
        }
    }
}
impl <K: Clone + Hash + Eq + Send + Sync + 'static> Default for LockSet<K> {
    fn default() -> Self {
        LockSet { locks: Default::default() }
    }
}

struct WaitForLockSetFut<'a, K: Clone + Hash + Eq + Send + Sync + 'static> {
    key: K,
    parent: &'a LockSet<K>,
}
impl <'a, K: Clone + Hash + Eq + Send + Sync + 'static> Future for WaitForLockSetFut<'a, K> {
    type Output = LockSetGuard<'a, K>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut entry = self.parent.locks.entry(self.key.clone()).or_default();
        if entry.is_empty() {
            Poll::Ready(LockSetGuard {
                key: self.key.clone(),
                parent: self.parent,
            })
        } else {
            entry.push(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// A guard returned for an active lock in a lock set.
pub struct LockSetGuard<'a, K: Clone + Hash + Eq + Send + Sync + 'static> {
    key: K,
    parent: &'a LockSet<K>,
}
impl <'a, K: Clone + Hash + Eq + Send + Sync + 'static> Drop for LockSetGuard<'a, K> {
    fn drop(&mut self) {
        // wake all wakers associated with the lock
        for waker in self.parent.locks.remove(&self.key).unwrap().1 {
            waker.wake();
        }
    }
}