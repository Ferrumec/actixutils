use actix_web::Error;
use uuid::Uuid;
use async_trait::async_trait;
/// Async backing store for [`SessionMiddleware`].
///
/// Implement this on your own persistence layer (database, Redis, in-memory map, ...).
/// `Session` is the session payload type; it must be `Clone + Default` because a
/// missing/invalid cookie yields a fresh `Session::default()` rather than an error
/// (unless the middleware was constructed with [`SessionMiddleware::required`]).
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    /// The session payload type persisted by this store.
    type Session: Send + Sync + Clone + Default + 'static;

    /// Load the session identified by `session_id`, if it exists.
    async fn load(&self, session_id: &Uuid) -> Result<Option<Self::Session>, Error>;

    /// Persist `session` under `session_id`, overwriting any existing value.
    async fn save(&self, session_id: &Uuid, session: &Self::Session) -> Result<(), Error>;

    /// Remove the session identified by `session_id`.
    async fn delete(&self, session_id: &Uuid) -> Result<(), Error>;
}