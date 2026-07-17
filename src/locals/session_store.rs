//! Backing store trait for [`Session<T>`](crate::extractors::Session).

/// Backing store for [`Session<T>`](crate::extractors::Session) extraction.
///
/// Implement this trait on your own store type (e.g. a DashMap, Redis client
/// wrapper, or database-backed store) and register it with Actix-web's app data
/// as `Arc<dyn SessionStore<T>>`.
pub trait SessionStore<T>: Send + Sync {
    /// Look up a session by its ID string.
    ///
    /// Returns `Some(T)` if a valid, unexpired session exists, or `None` if the
    /// session is unknown or has expired.
    fn get(&self, session_id: &str) -> Option<T>;
}
