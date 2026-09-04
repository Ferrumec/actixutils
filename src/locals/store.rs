//! General-purpose async key-value store trait.

use std::error::Error;

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
