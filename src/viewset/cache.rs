pub trait Cache<K, V> {
    fn get(&self, key: &K) -> Option<V>;
    fn set(&self, key: K, value: V);
    fn delete(&self, key: &K);
    fn clear(&self);
}

use moka::sync::Cache as MokaCache;

pub struct DefaultCache<K, V> {
    cache: MokaCache<K, V>,
}

impl<K, V> DefaultCache<K, V>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(max_capacity: u64) -> Self {
        Self {
            cache: MokaCache::builder().max_capacity(max_capacity).build(),
        }
    }
}

impl<K, V> Cache<K, V> for DefaultCache<K, V>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: &K) -> Option<V> {
        self.cache.get(key)
    }

    fn set(&self, key: K, value: V) {
        self.cache.insert(key, value);
    }

    fn delete(&self, key: &K) {
        self.cache.invalidate(key);
    }

    fn clear(&self) {
        self.cache.invalidate_all();
    }
}
