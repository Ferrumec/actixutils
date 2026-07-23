//! Cookie-based, server-side session storage.
//!
//! This is the crate's built-in session mechanism: [`SessionMiddleware`] resolves a
//! session cookie to a value of type `T` on each request, exposes it to handlers via
//! the [`Session<T>`] extractor, and persists any changes back to a caller-supplied
//! store after the response is produced.
//!
//! Note this module defines its own async [`SessionStore`] trait, distinct from the
//! synchronous [`locals::SessionStore<T>`](crate::locals::SessionStore) trait exported
//! elsewhere in the crate — that other trait is not used by this middleware.
//!
//! # Example
//! ```ignore
//! use actixutils::middleware::{Session, SessionMiddleware};
//! use actix_web::{web, App, HttpResponse};
//! use std::sync::Arc;
//!
//! async fn get_counter(session: Session<MySession>) -> HttpResponse {
//!     let s = session.read().await;
//!     HttpResponse::Ok().json(&*s)
//! }
//!
//! App::new()
//!     .wrap(SessionMiddleware::new(Arc::new(my_store)))
//!     .route("/counter", web::get().to(get_counter));
//! # #[derive(Clone, Default, serde::Serialize)]
//! # struct MySession;
//! # let my_store: MyStore = unimplemented!();
//! # struct MyStore;
//! ```

use actix_web::{
    Error, FromRequest, HttpMessage, HttpRequest,
    body::MessageBody,
    dev::{Payload, Service, ServiceRequest, ServiceResponse, Transform},
    error,
};
use async_trait::async_trait;
use futures_util::future::LocalBoxFuture;
use std::{
    future::{Ready, ready},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use tokio::sync::RwLock;
use uuid::Uuid;

type SharedSession<T> = Arc<RwLock<T>>;

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

/// A handle to the current request's session data, obtained via
/// [`FromRequest`] once [`SessionMiddleware`] has populated the request extensions.
///
/// Cloning is cheap (it clones the underlying `Arc`s and shares the same data).
/// Call [`read`](Self::read) for a read-only view or [`write`](Self::write) to mutate
/// the session; any call to `write` marks the session dirty so
/// [`SessionMiddleware`] persists it via the store after the handler returns.
pub struct Session<T> {
    data: SharedSession<T>,
    dirty: Arc<AtomicBool>,
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

/// Middleware factory for cookie-based session storage.
///
/// Construct with [`SessionMiddleware::new`] (missing/invalid session cookies fall
/// back to a fresh default session) or [`SessionMiddleware::required`] (missing/invalid
/// cookies are rejected with `401 Unauthorized`). Customise the cookie name with
/// [`cookie_name`](Self::cookie_name).
pub struct SessionMiddleware<S> {
    store: Arc<S>,
    cookie_name: String,
    required: bool,
}

impl<S> SessionMiddleware<S> {
    /// Create a `SessionMiddleware` backed by `store`.
    ///
    /// A request with no session cookie, or one that fails to parse as a `Uuid`, is
    /// given a fresh default session (a new cookie is issued on the response) rather
    /// than being rejected. The cookie name defaults to `"session"`.
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
            required: false,
        }
    }

    /// Create a `SessionMiddleware` backed by `store` that rejects requests without a
    /// valid session cookie.
    ///
    /// A request with no session cookie, or one that fails to parse as a `Uuid`,
    /// causes the middleware to return `401 Unauthorized` before the handler runs.
    pub fn required(store: Arc<S>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
            required: true,
        }
    }

    /// Override the session cookie name (default: `"session"`).
    pub fn cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_name = name.into();
        self
    }
}

impl<S, B, Store> Transform<S, ServiceRequest> for SessionMiddleware<Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    Store: SessionStore + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = SessionMiddlewareService<S, Store>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;
    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SessionMiddlewareService {
            service: service.into(),
            store: self.store.clone(),
            cookie_name: self.cookie_name.clone(),
            required: self.required,
        }))
    }
}

/// The inner service produced by [`SessionMiddleware`].
pub struct SessionMiddlewareService<S, Store> {
    service: Rc<S>,
    store: Arc<Store>,
    cookie_name: String,
    required: bool,
}

impl<S, B, Store> Service<ServiceRequest> for SessionMiddlewareService<S, Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    Store: SessionStore + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let store = self.store.clone();
        let cookie_name = self.cookie_name.clone();
        let service = self.service.clone();
        let required = self.required;
        Box::pin(async move {
            let (session_id, session, new_session) = match req.cookie(&cookie_name) {
                Some(cookie) => {
                    let mut new_session = false;
                    let id = match Uuid::parse_str(cookie.value()) {
                        Ok(id) => id,
                        Err(_) => {
                            if required {
                                return Err(error::ErrorUnauthorized("no session"));
                            }
                            new_session = true;
                            Uuid::new_v4()
                        }
                    };
                    let session_data = store.load(&id).await?.unwrap_or_default();
                    (id, Session::new(session_data), new_session)
                }
                None => {
                    if required {
                        return Err(error::ErrorUnauthorized("no session"));
                    }
                    let id = Uuid::new_v4();
                    let session = Session::new(Store::Session::default());
                    session.dirty.store(true, Ordering::Relaxed); // new session must be saved once
                    (id, session, true)
                }
            };

            let session = Arc::new(session);
            req.extensions_mut().insert(session.clone());

            let mut res = service.call(req).await?;

            // Only save if dirty
            if session.is_dirty() {
                let session_data = session.read().await;
                store.save(&session_id, &*session_data).await?;
                session.set_clean(); // reset flag
            }

            if new_session {
                use actix_web::cookie::Cookie;
                let cookie = Cookie::build(cookie_name, session_id.to_string())
                    .path("/")
                    .http_only(true)
                    .finish();
                res.response_mut()
                    .add_cookie(&cookie)
                    .map_err(error::ErrorInternalServerError)?;
            }
            Ok(res)
        })
    }
}
