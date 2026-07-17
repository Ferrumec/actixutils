//! Cookie-based session extraction for Actix-web handlers.
//!
//! [`Session<T>`] reads the `session_id` cookie from the incoming request and
//! delegates the look-up to a [`SessionStore<T>`](crate::locals::SessionStore) registered
//! in the application state. The session type `T` is fully generic — it can be a
//! struct, an enum, or any other `'static` value your application stores.
//!
//! # Setup
//!
//! ```rust,no_run
//! use actixutils::extractors::Session;
//! use actixutils::locals::SessionStore;
//! use actix_web::{web, App, HttpResponse};
//! use std::sync::Arc;
//!
//! struct MyStore { /* … */ }
//!
//! impl SessionStore<MySessionData> for MyStore {
//!     fn get(&self, session_id: &str) -> Option<MySessionData> {
//!         // look up session_id in your store
//!         todo!()
//!     }
//! }
//!
//! # #[derive(Clone)] struct MySessionData;
//! async fn handler(session: Session<MySessionData>) -> HttpResponse {
//!     HttpResponse::Ok().finish()
//! }
//!
//! // Register the store as app data:
//! let store: Arc<dyn SessionStore<MySessionData>> = Arc::new(MyStore { /* … */ });
//! App::new().app_data(web::Data::from(store));
//! ```

use std::sync::Arc;

use crate::locals::SessionStore;
use actix_web::{
    Error, FromRequest, HttpRequest,
    dev::Payload,
    error::{ErrorInternalServerError, ErrorUnauthorized},
};
use futures_util::future::{Ready, ready};

/// An extractor that resolves the `session_id` cookie to a typed session value.
///
/// `T` is the session data type returned by [`SessionStore::get`].
///
/// # Errors
/// * `500 Internal Server Error` — `Arc<dyn SessionStore<T>>` is missing from app data.
/// * `401 Unauthorized` — The `session_id` cookie is absent or maps to no stored session.
pub struct Session<T>(pub T);

impl<T: 'static> FromRequest for Session<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // 1. Get session store
        let store = match req.app_data::<Arc<dyn SessionStore<T>>>() {
            Some(store) => store,
            None => {
                return ready(Err(ErrorInternalServerError(
                    "Session Extractor: Missing session store",
                )));
            }
        };

        // 2. Get session cookie
        let cookie = match req.cookie("session_id") {
            Some(cookie) => cookie,
            None => {
                return ready(Err(ErrorUnauthorized("Missing session")));
            }
        };

        // 3. Load session
        match store.get(cookie.value()) {
            Some(session) => ready(Ok(Session(session))),
            None => ready(Err(ErrorUnauthorized("Invalid session"))),
        }
    }
}
