//! `Auth<T>` — an Actix-web request extractor for JWT-authenticated routes.
//!
//! `Auth<T>` implements [`FromRequest`] and can be used directly as a handler argument.
//! It first checks whether a `T` is already present in the request extensions (e.g. set
//! by the [`middleware::Auth`](crate::middleware) middleware), and if not, falls back to
//! reading the `Authorization: Bearer <token>` header and validating it via the
//! `Arc<dyn Validate<T>>` registered in app data.

use crate::locals::Validate;
use actix_web::HttpMessage;
use actix_web::{
    Error, FromRequest, HttpRequest,
    dev::Payload,
    error::{ErrorInternalServerError, ErrorUnauthorized},
    http::header,
};
use futures_util::future::{Ready, ready};
use std::sync::Arc;

/// An extractor that wraps validated JWT claims of type `T`.
///
/// `T` must implement `Clone + 'static` and an `Arc<dyn Validate<T>>` must be
/// registered with [`web::Data`](actix_web::web::Data) or
/// [`app_data`](actix_web::App::app_data).
///
/// # Extraction order
/// 1. If `T` is already in the request extensions (placed there by
///    [`middleware::Auth`](crate::middleware)), it is cloned and returned immediately.
/// 2. Otherwise the `Authorization: Bearer <token>` header is read, the token is
///    extracted, and `Validate<T>::validate` is called.
///
/// # Errors
/// * `500 Internal Server Error` — `Arc<dyn Validate<T>>` is missing from app data.
/// * `401 Unauthorized` — The header is absent, malformed, or the token is invalid.
///
/// # Example
/// ```rust,no_run
/// use actixutils::extractors::Jwt as Auth;
/// use actixutils::locals::Identity;
/// use actix_web::HttpResponse;
///
/// async fn protected(auth: Auth<Identity>) -> HttpResponse {
///     HttpResponse::Ok().json(&auth.0)
/// }
/// ```
pub struct Jwt<T>(pub T);

impl<T: Clone + 'static> FromRequest for Jwt<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Try getting from request extensions
        if let Some(identity) = req.extensions().get::<T>() {
            return ready(Ok(Jwt(identity.clone())));
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
        let token: Option<String> = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.replace("Bearer ", ""))
            .or_else(|| req.cookie("access_token").map(|c| c.value().to_string()));

        let token = match token {
            Some(t) => t,
            None => return ready(Err(ErrorUnauthorized("Missing authorization header"))),
        };

        // 3. Validate token
        match state.validate(&token) {
            Ok(identity) => ready(Ok(Jwt(identity))),
            Err(e) => {
                tracing::debug!("Invalid token: {e}");
                ready(Err(ErrorUnauthorized("Invalid token")))
            }
        }
    }
}
