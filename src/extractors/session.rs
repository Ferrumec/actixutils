use actix_web::{Error, FromRequest, HttpMessage, HttpRequest, dev::Payload, error};
use std::{
    future::{Ready, ready},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::RwLock;

type SharedSession<T> = Arc<RwLock<T>>;

/// A handle to the current request's session data, obtained via
/// [`FromRequest`] once [`SessionMiddleware`] has populated the request extensions.
///
/// Cloning is cheap (it clones the underlying `Arc`s and shares the same data).
/// Call [`read`](Self::read) for a read-only view or [`write`](Self::write) to mutate
/// the session; any call to `write` marks the session dirty so
/// [`SessionMiddleware`] persists it via the store after the handler returns.
pub struct Session<T> {
    data: SharedSession<T>,
    pub(crate) dirty: Arc<AtomicBool>,
}

impl<T> Clone for Session<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),   // Arc clone
            dirty: self.dirty.clone(), // Arc clone
        }
    }
}

impl<T> Session<T> {
    pub(crate) fn new(session: T) -> Self {
        Self {
            data: Arc::new(RwLock::new(session)),
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }
    /// Acquire a read lock and view the current session value.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, T> {
        self.data.read().await
    }
    /// Acquire a write lock to mutate the session value.
    ///
    /// Marks the session dirty (regardless of whether the guard is actually used to
    /// change anything), so [`SessionMiddleware`] will persist it via the store once
    /// the handler finishes.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        // <-- &self not &mut self
        self.dirty.store(true, Ordering::Relaxed); // mark dirty on any write
        self.data.write().await
    }
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
    pub(crate) fn set_clean(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }
}

impl<T: Send + Sync + 'static> FromRequest for Session<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;
    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        match req.extensions().get::<Arc<Session<T>>>() {
            Some(session) => ready(Ok((**session).clone())), // clone the Arc<Session<T>>
            None => {
                tracing::error!("No session in request. Did you forget to wrap SessionMiddleware?");
                ready(Err(error::ErrorInternalServerError(
                    "Session requested without SessionMiddleware",
                )))
            }
        }
    }
}
