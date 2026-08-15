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
    Error, HttpMessage,
    body::MessageBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error,
};

use crate::Store;
use crate::extractors::Session;
use futures_util::future::LocalBoxFuture;
use std::{
    future::{Ready, ready},
    rc::Rc,
    sync::{Arc, atomic::Ordering},
    task::{Context, Poll},
};
use uuid::Uuid;

/// Middleware factory for cookie-based session storage.
///
/// Construct with [`SessionMiddleware::new`] (missing/invalid session cookies fall
/// back to a fresh default session) or [`SessionMiddleware::required`] (missing/invalid
/// cookies are rejected with `401 Unauthorized`). Customise the cookie name with
/// [`cookie_name`](Self::cookie_name).
pub struct SessionMiddleware<S> {
    store: Arc<dyn Store<Uuid, S>>,
    cookie_name: String,
    required: bool,
}

impl<Sess> SessionMiddleware<Sess> {
    /// Create a `SessionMiddleware` backed by `store`.
    ///
    /// A request with no session cookie, or one that fails to parse as a `Uuid`, is
    /// given a fresh default session (a new cookie is issued on the response) rather
    /// than being rejected. The cookie name defaults to `"session"`.
    pub fn new(store: Arc<dyn Store<Uuid, Sess>>) -> Self {
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
    pub fn required(store: Arc<dyn Store<Uuid, Sess>>) -> Self {
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

impl<S, B, Sess: Default + Clone + 'static> Transform<S, ServiceRequest> for SessionMiddleware<Sess>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = SessionMiddlewareService<S, Sess>;
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
pub struct SessionMiddlewareService<R, S> {
    service: Rc<R>,
    store: Arc<dyn Store<Uuid, S>>,
    cookie_name: String,
    required: bool,
}

impl<S, B, Sess: Default + Clone + 'static> Service<ServiceRequest>
    for SessionMiddlewareService<S, Sess>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
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
                    let session_data = store.get(&id).await?.unwrap_or_default();
                    req.extensions_mut().insert(session_data.clone());
                    (id, Session::new(session_data), new_session)
                }
                None => {
                    if required {
                        return Err(error::ErrorUnauthorized("no session"));
                    }
                    let id = Uuid::new_v4();
                    let session = Session::new(Sess::default());
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
                store.set(&session_id, session_data.clone()).await?;
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
