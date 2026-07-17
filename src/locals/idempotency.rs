//! Store abstraction for the [`Idempotency`](crate::middleware::Idempotency) middleware.

use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

/// A serialisable snapshot of an HTTP response for caching.
#[derive(Clone)]
pub struct CachedResponse {
    /// HTTP status code as a `u16`.
    pub status: u16,
    /// Response headers as `(name, value)` string pairs.
    pub headers: Vec<(String, String)>,
    /// Raw response body bytes.
    pub body: Bytes,
}

/// The lifecycle state of an idempotency key.
pub enum IdempotencyState {
    /// A request with this key is currently being processed.
    InProgress,
    /// A request with this key completed successfully and its response is cached.
    Completed(CachedResponse),
}

/// Backing store abstraction for the [`Idempotency`](crate::middleware::Idempotency) middleware.
///
/// Implementors must guarantee that [`acquire`](Self::acquire) is atomic — i.e. if two
/// concurrent requests arrive with the same key, exactly one should receive `Ok(true)`.
#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// The error type returned by store operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Attempt to reserve `key` for exclusive execution.
    ///
    /// Returns:
    /// * `Ok(true)`  — The caller owns this key and should process the request.
    /// * `Ok(false)` — The key already exists; the caller should check [`get`](Self::get).
    async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error>;

    /// Retrieve the current state of `key`, if any.
    async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error>;

    /// Persist the finished response for `key`.
    async fn complete(&self, key: &str, response: CachedResponse) -> Result<(), Self::Error>;

    /// Release an in-progress reservation for `key` (called on error paths).
    async fn release(&self, key: &str) -> Result<(), Self::Error>;
}
