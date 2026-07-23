//! A general-purpose, synchronous session-store trait.
//!
//! Note: this is *not* the trait used by the crate's built-in cookie-session
//! implementation. [`middleware::Session<T>`](crate::middleware::Session) and
//! [`middleware::SessionMiddleware`](crate::middleware::SessionMiddleware) are backed by
//! their own async `SessionStore` trait defined locally in `middleware::session`. This
//! trait is provided as a lightweight, synchronous alternative for callers who want to
//! roll their own session lookup outside of that middleware.

/// A general-purpose, synchronous backing store for session-style lookups.
///
/// Implement this trait on your own store type (e.g. a DashMap, Redis client
/// wrapper, or database-backed store). It is not wired into
/// [`middleware::SessionMiddleware`](crate::middleware::SessionMiddleware) — see the
/// module documentation for details.
pub trait SessionStore<T>: Send + Sync {
    /// Look up a session by its ID string.
    ///
    /// Returns `Some(T)` if a valid, unexpired session exists, or `None` if the
    /// session is unknown or has expired.
    fn get(&self, session_id: &str) -> Option<T>;
}
