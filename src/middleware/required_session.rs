//! Cookie-based, server-side session storage.
//!
//! This is the crate's built-in session mechanism: [`RequiredSession`] resolves a
//! session cookie to a value of type `T` on each request, exposes it to handlers via
//! the [`Session<T>`] extractor, and persists any changes back to a caller-supplied
//! [`Store`](crate::Store) after the response is produced.
//!
//! # Example
//! ```ignore
//! use actixutils::middleware::{Session, RequiredSession};
//! use actix_web::{web, App, HttpResponse};
//! use std::sync::Arc;
//!
//! async fn get_counter(session: Session<MySession>) -> HttpResponse {
//!     let s = session.read().await;
//!     HttpResponse::Ok().json(&*s)
//! }
//!
//! App::new()
//!     .wrap(RequiredSession::new(Arc::new(my_store)))
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
    sync::Arc,
    task::{Context, Poll},
};
use uuid::Uuid;

/// Middleware factory for cookie-based session storage.
///
/// Construct with [`RequiredSession::new`]. Missing, invalid, or expired
/// session cookies are rejected with `401 Unauthorized`. Customise the cookie
/// name with [`cookie_name`](Self::cookie_name).
pub struct RequiredSession<S> {
    store: Arc<dyn Store<Uuid, S>>,
    cookie_name: String,
}

impl<Sess> RequiredSession<Sess> {
    /// Create a `RequiredSession` backed by `store`.
    ///
    /// A request with no session cookie, an invalid `Uuid`, a missing store
    /// entry, or an expired session is rejected with `401 Unauthorized`.
    /// The cookie name defaults to `"session"`.
    pub fn new(store: Arc<dyn Store<Uuid, Sess>>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
        }
    }

    /// Override the session cookie name (default: `"session"`).
    pub fn cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_name = name.into();
        self
    }
}

impl<S, B, Sess: Clone + 'static> Transform<S, ServiceRequest> for RequiredSession<Sess>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RequiredSessionService<S, Sess>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;
    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequiredSessionService {
            service: service.into(),
            store: self.store.clone(),
            cookie_name: self.cookie_name.clone(),
        }))
    }
}

/// The inner service produced by [`RequiredSession`].
pub struct RequiredSessionService<R, S> {
    service: Rc<R>,
    store: Arc<dyn Store<Uuid, S>>,
    cookie_name: String,
}

impl<S, B, Sess: Clone + 'static> Service<ServiceRequest> for RequiredSessionService<S, Sess>
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
        Box::pin(async move {
            let (session_id, session) = match req.cookie(&cookie_name) {
                Some(cookie) => {
                    let id = match Uuid::parse_str(cookie.value()) {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(error::ErrorUnauthorized("no session"));
                        }
                    };
                    let session_data = match store.get(&id).await? {
                        Some(data) => data,
                        None => {
                            return Err(error::ErrorUnauthorized("no session"));
                        }
                    };

                    req.extensions_mut().insert(session_data.clone());
                    (id, Session::new(session_data))
                }
                None => {
                    return Err(error::ErrorUnauthorized("no session"));
                }
            };

            let session = Arc::new(session);
            req.extensions_mut().insert(session.clone());

            let res = service.call(req).await?;

            // Only save if dirty
            if session.is_dirty() {
                let session_data = session.read().await;
                store.set(&session_id, session_data.clone()).await?;
                session.set_clean(); // reset flag
            }

            Ok(res)
        })
    }
}
