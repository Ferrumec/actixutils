use std::sync::Arc;

use crate::Validate;
use actix_web::{Error, FromRequest, HttpRequest, dev::Payload, error::ErrorUnauthorized};
use futures_util::future::{Ready, ready};

pub struct Auth<T>(pub T);

impl<T: 'static> FromRequest for Auth<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // 1. Get app state
        let state = match req.app_data::<Arc<dyn Validate<T>>>() {
            Some(data) => data,
            None => {
                return ready(Err(ErrorUnauthorized("Missing app state")));
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
