//! Storage abstraction for the HTTP cache middleware.

use std::time::Duration;

use async_trait::async_trait;

use super::types::CachedResponse;

/// Pluggable storage backend for cached HTTP responses.
///
/// The middleware is deliberately decoupled from any specific backend.
/// [`MemoryCache`](super::memory::MemoryCache) is provided for development
/// and single-process use; other backends (e.g. Redis) can implement this
/// trait later without changing the middleware itself.
#[async_trait]
pub trait CacheStore: Send + Sync + 'static {
    /// Look up a cache entry by key. Implementations are responsible for
    /// treating expired entries as absent.
    async fn get(&self, key: &str) -> Option<CachedResponse>;

    /// Store a response under `key`, expiring it after `ttl`.
    async fn set(&self, key: &str, response: CachedResponse, ttl: Duration);

    /// Remove a single entry, if present.
    async fn delete(&self, key: &str);

    /// Remove all entries.
    async fn clear(&self);
}
