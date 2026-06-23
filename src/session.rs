use std::sync::Arc;

use actix_web::{
    Error, FromRequest, HttpRequest,
    dev::Payload,
    error::{ErrorInternalServerError, ErrorUnauthorized},
};
use futures_util::future::{Ready, ready};

pub trait SessionStore<T>: Send + Sync {
    fn get(&self, session_id: &str) -> Option<T>;
}

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
