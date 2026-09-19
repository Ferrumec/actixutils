//! General-purpose async key-value store trait.

use std::error::Error;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
/// A general-purpose async key-value storage abstraction.
///
/// Implement this to plug a custom backend (in-memory, Redis, a database
/// table, etc.) into any component that only needs get/set/delete/clear
/// semantics — for example, [`middleware::Cache`](crate::middleware::Cache)
/// uses `Store<String, CachedResponse>` to persist cached HTTP responses.
///
/// Unlike [`middleware::cache::CacheStore`](crate::middleware::CacheStore),
/// this trait has no TTL concept: expiration, if any, is entirely up to the
/// implementation.
#[async_trait::async_trait]
pub trait Store<K, V>: Send + Sync {
    async fn get(&self, key: &K) -> Result<Option<V>, Box<dyn Error>>;

    async fn set(&self, key: &K, value: V) -> Result<(), Box<dyn Error>>;

    async fn delete(&self, key: &K) -> Result<(), Box<dyn Error>>;
}

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Constructs the [`actixutils::Store`] caches used by each repository.
///
/// Implementations decide the concrete cache backend (the binary uses an
/// in-memory `moka` cache; tests may prefer a no-op or deterministic
/// implementation). `name` identifies the logical cache (e.g.
/// `"community_items"`) and `ttl` is the requested expiry for entries.
pub trait CacheFactory {
    fn new_cache<K, V>(&self, name: &str, ttl: Duration) -> Arc<dyn Store<K, V>>
    where
        K: Hash + Eq + Clone + Serialize + Send + Sync + 'static,
        V: Clone + Serialize + DeserializeOwned + Send + Sync + 'static;
}
