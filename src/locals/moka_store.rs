use crate::Store;
use crate::locals::CacheFactory;
use moka::future::Cache;
use std::any::Any;
use std::collections::HashMap;
use std::error::Error;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[async_trait::async_trait]
impl<K: Clone + Hash + Eq + Send + Sync + 'static, V: Clone + Send + Sync + 'static> Store<K, V>
    for Cache<K, V>
{
    /// Look up the value stored under `key`, if any.
    async fn get(&self, key: &K) -> Result<Option<V>, Box<dyn Error>> {
        Ok(self.get(key).await)
    }

    /// Store `value` under `key`, replacing any existing entry.
    async fn set(&self, key: &K, value: V) -> Result<(), Box<dyn Error>> {
        self.insert(key.clone(), value).await;
        Ok(())
    }

    /// Remove the entry stored under `key`, if present.
    async fn delete(&self, key: &K) -> Result<(), Box<dyn Error>> {
        self.remove(key).await;
        Ok(())
    }
}

/// [`CacheFactory`] backed by in-process [`moka`] caches, keyed by `name` so
/// that repeated calls for the same logical cache (e.g. `"membership_items"`,
/// shared between this crate and `groups`) return the *same* underlying
/// cache instead of a fresh, unshared one each time.
///
/// Each cache is created with a fixed maximum capacity of 1000 entries and
/// the TTL supplied to [`CacheFactory::new_cache`] the first time its name
/// is seen. Entries are evicted either when they expire or when the cache
/// reaches its maximum capacity.
#[derive(Clone, Default)]
pub struct MokaCacheFactory {
    registry: Arc<Mutex<HashMap<String, Box<dyn Any + Send + Sync>>>>,
}

impl CacheFactory for MokaCacheFactory {
    fn new_cache<K, V>(&self, name: &str, ttl: Duration) -> Arc<dyn Store<K, V>>
    where
        K: Hash + Eq + Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        let mut registry = self.registry.lock().expect("cache registry lock poisoned");

        if let Some(existing) = registry.get(name) {
            match existing.downcast_ref::<Arc<dyn Store<K, V>>>() {
                Some(cache) => return cache.clone(),
                None => {
                    // Same cache name requested with a different (K, V) pair.
                    // This is a wiring bug, but we'd rather log and hand back
                    // an unshared cache than take the whole process down.
                    tracing::error!(
                        "cache '{name}' was previously created with a different \
                         key/value type; returning an unshared cache instead \
                         of sharing"
                    );

                    let cache: Cache<K, V> = Cache::builder()
                        .max_capacity(1000)
                        .time_to_live(ttl)
                        .build();

                    return Arc::new(cache);
                }
            }
        }

        let cache: Cache<K, V> = Cache::builder()
            .max_capacity(1000)
            .time_to_live(ttl)
            .build();

        let store: Arc<dyn Store<K, V>> = Arc::new(cache);

        registry.insert(name.to_string(), Box::new(store.clone()));

        store
    }
}
