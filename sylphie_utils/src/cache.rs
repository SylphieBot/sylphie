use arc_swap::*;
use dashmap::DashMap;
use futures::Future;
use fxhash::FxBuildHasher;
use std::hash::Hash;
use std::time::Instant;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use sylphie_core::errors::*;

struct LruEntry<K, V> {
    key: K,
    value: V,
    last_touched: AtomicU32,
    is_busy: AtomicBool,
}
impl <K, V> LruEntry<K, V> {
    fn touch(&self, base_time: Instant) {
        self.last_touched.store((Instant::now() - base_time).as_secs() as u32, Ordering::Relaxed);
    }
}

struct LruData<K: Eq + Hash + 'static, V: 'static> {
    lru: plru::DynamicCache,
    cache_data: Vec<ArcSwapOption<LruEntry<K, V>>>,
    key_lookup: DashMap<K, usize, FxBuildHasher>,
    base_time: Instant,
}
impl <K: Eq + Hash + 'static, V: 'static> LruData<K, V> {
    fn new(lines: usize) -> Self {
        LruData {
            lru: plru::create(lines),
            cache_data: vec![ArcSwapOption::empty(); lines],
            key_lookup: Default::default(),
            base_time: Instant::now(),
        }
    }
}

/// A concurrent LRU cache.
pub struct LruCache<
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static
> {
    data: ArcSwap<LruData<K, V>>,
}
impl <
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static
> LruCache<K, V> {
    /// Creates a new LRU cache with a given number of lines.
    pub fn new(lines: usize) -> Self {
        LruCache {
            data: ArcSwap::from_pointee(LruData::new(lines)),
        }
    }

    fn check_cached(&self, key: &K) -> Option<V> {
        let lock = self.data.load();

        if let Some(cache_line) = lock.key_lookup.get(key) {
            let line_no = *cache_line;
            std::mem::drop(cache_line);

            let line_contents = lock.cache_data[line_no].load();
            if let Some(line) = &*line_contents {
                line.touch(lock.base_time);
                lock.lru.touch(line_no);
                if &line.key == key {
                    return Some(line.value.clone())
                }
            }
        }
        None
    }

    fn try_insert_loop(&self, key: K, entry: Option<Arc<LruEntry<K, V>>>, do_replace: bool) {
        let lock = self.data.load();

        // check if we already have a cache line for this item
        let fixed_line_no = if let Some(cache_line) = lock.key_lookup.get(&key) {
            let line_no = *cache_line;
            std::mem::drop(cache_line);

            let line_contents = lock.cache_data[line_no].load();
            if let Some(line) = &*line_contents {
                if &line.key == &key {
                    // if we aren't replacing things, we've found the key now.
                    // bail out early
                    if !do_replace {
                        return
                    }

                    // mark the line busy, touch the plru and prepare to set the cache item to
                    // this line.
                    if !line.is_busy.compare_and_swap(false, true, Ordering::Relaxed) {
                        line.touch(lock.base_time);
                        Some(line_no)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let (line_no, already_exists) = match fixed_line_no {
            Some(line_no) => (line_no, true),
            None => (lock.lru.replace(), false),
        };

        // remove the lookup entry for the last thing to touch the cache
        if !already_exists {
            let line_contents = lock.cache_data[line_no].load();
            if let Some(line) = line_contents.as_ref() {
                if line.is_busy.compare_and_swap(false, true, Ordering::Relaxed) {
                    // this is being replaced by something else, let's not touch it
                    return self.try_insert_loop(key, entry, do_replace);
                }
                lock.key_lookup.remove(&line.key);
            }
        }

        // put our new cache entry in the, well, cache
        lock.lru.touch(line_no);
        lock.cache_data[line_no].store(entry.clone());
        if already_exists {
            entry.unwrap().is_busy.compare_and_swap(true, false, Ordering::Relaxed);
        } else {
            lock.key_lookup.insert(key, line_no);
        }
    }
    fn insert_cache(&self, key: K, value: V, do_replace: bool) {
        let entry = Arc::new(LruEntry {
            key: key.clone(),
            value: value.clone(),
            last_touched: Default::default(),
            is_busy: Default::default(),
        });
        entry.touch(self.data.load().base_time);
        self.try_insert_loop(key, Some(entry), do_replace);
    }
    fn invalidate_cache(&self, key: &K) -> bool {
        let lock = self.data.load();

        if let Some(cache_line) = lock.key_lookup.get(key) {
            let line_no = *cache_line;
            std::mem::drop(cache_line);

            let line_contents = lock.cache_data[line_no].load();
            if let Some(line) = line_contents.as_ref() {
                if line.is_busy.compare_and_swap(false, true, Ordering::Relaxed) {
                    return false
                }
                lock.key_lookup.remove(&line.key);
            }
        }

        true
    }

    /// Caches a given function.
    pub fn cached(&self, key: K, make_new: impl FnOnce() -> Result<V>) -> Result<V> {
        if let Some(v) = self.check_cached(&key) {
            Ok(v)
        } else {
            let value = make_new()?;
            self.insert_cache(key, value.clone(), false);
            Ok(value)
        }
    }

    /// Inserts a value into the cache.
    pub fn insert(&self, key: K, value: V) {
        self.insert_cache(key, value, true);
    }

    /// Invalidates the cache for a given key.
    pub fn invalidate(&self, key: &K) {
        self.invalidate_cache(key);
    }

    /// Caches a given future.
    ///
    /// The future is not run if a cached value is already available.
    pub async fn cached_async(
        &self, key: K, make_new: impl Future<Output = Result<V>>,
    ) -> Result<V> {
        if let Some(v) = self.check_cached(&key) {
            Ok(v)
        } else {
            let value = make_new.await?;
            self.insert_cache(key, value.clone(), false);
            Ok(value)
        }
    }
}