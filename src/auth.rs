use crate::Validate;
use actix_web::HttpMessage;
use actix_web::{
    Error, FromRequest, HttpRequest,
    dev::Payload,
    error::{ErrorInternalServerError, ErrorUnauthorized},
};
use futures_util::future::{Ready, ready};
use std::sync::Arc;

pub struct Auth<T>(pub T);

impl<T: Clone + 'static> FromRequest for Auth<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Try getting from request extensions
        match req.extensions().get::<T>() {
            Some(identity) => return ready(Ok(Auth(identity.clone()))),
            None => (),
        };

        // 1. Get app state
        let state = match req.app_data::<Arc<dyn Validate<T>>>() {
            Some(data) => data,
            None => {
                return ready(Err(ErrorInternalServerError(
                    "Auth Extractor: Missing Validate<T> from app state",
                )));
            }
        };

        // 2. Get Authorization header
        let header = match req.headers().get("Authorization") {
            Some(h) => h,
            None => {
                return ready(Err(ErrorUnauthorized("Missing Authorization header")));
            }
        };

        let header_str = match header.to_str() {
            Ok(s) => s,
            Err(_) => {
                return ready(Err(ErrorUnauthorized("Invalid header format")));
            }
        };

        // 3. Extract Bearer token
        let token = match header_str.strip_prefix("Bearer ") {
            Some(t) => t,
            None => {
                return ready(Err(ErrorUnauthorized("Invalid auth scheme")));
            }
        };

        // 4. Validate token
        match state.validate(token) {
            Ok(identity) => ready(Ok(Auth(identity))),
            Err(_) => ready(Err(ErrorUnauthorized("Invalid token"))),
        }
    }
}
