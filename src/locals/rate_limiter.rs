//! Identity-key trait for the [`RateLimiter`](crate::middleware::RateLimiter) middleware.

use std::hash::Hash;

/// Provides a stable, hashable identity key for rate limiting.
///
/// Implement this on any Actix-web extractor (or wrapper) that identifies a client.
/// The associated `Id` type is used as the hash-map key, so it must be `Eq + Hash + Clone`.
pub trait GetId {
    /// The type used as the rate-limiter map key.
    type Id: Eq + Hash + Clone + Send + Sync + 'static;

    /// Extract the identity key from `self`.
    fn id(&self) -> Self::Id;
}
