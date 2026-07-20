//! `Next`-style middleware functions for identity and authority checks.
//!
//! These functions are designed for use with Actix-web's `wrap_fn` / `from_fn`
//! API.  They integrate with the [`Auth<T>`](crate::Auth) extractor rather than
//! re-implementing token parsing.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::middleware::{identity, authority};
//! use actix_web::{web, middleware::from_fn};
//!
//! web::scope("/api")
//!     .wrap(from_fn(identity))                   // require any authenticated user
//!     .route("/admin", web::get().to(admin_handler))
//!     .wrap(from_fn(authority(0)))              // additionally require permission bit 0
//!     .route("/super", web::get().to(super_handler));
//! # async fn admin_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! # async fn super_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! ```

use crate::extractors::Jwt;
use crate::locals::{Authority, Identity};

use actix_web::{
    Error, HttpResponse,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
};
use std::pin::Pin;

/// A `Next`-style middleware function that ensures the request carries a valid
/// [`Identity`] token.
///
/// On success the request is forwarded. On failure a `401 Unauthorized` response
/// is returned by the underlying [`Auth<Identity>`](crate::Auth) extractor.
pub async fn identity(
    mut req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let _user = req.extract::<Jwt<Identity>>().await?;
    next.call(req).await
}

/// Returns a `Next`-style middleware function that enforces a specific permission bit.
///
/// The returned closure extracts the [`Authority`] from the request (triggering a
/// `401` if absent) and then checks whether bit `perm_id` is set in the role
/// bitmask. If not, a `403 Forbidden` response is returned immediately.
///
/// # Arguments
/// * `perm_id` — Zero-based index of the required permission bit in
///   [`Authority::role`](crate::Authority::role).
///
/// # Example
/// ```rust,no_run
/// use actixutils::middleware::authority;
/// use actix_web::{web, middleware::from_fn};
///
/// // Protect a route with permission bit 3
/// web::scope("/billing")
///     .wrap(from_fn(authority(3)));
/// ```
pub fn authority(
    perm_id: u32,
) -> impl Fn(
    ServiceRequest,
    Next<BoxBody>,
) -> Pin<Box<dyn Future<Output = Result<ServiceResponse<BoxBody>, Error>>>> {
    move |mut req: ServiceRequest, next: Next<BoxBody>| {
        Box::pin(async move {
            let authority = req.extract::<Jwt<Authority>>().await?.0;
            let p_value = 1u128 << perm_id;

            if authority.role & p_value != p_value {
                Ok(req.into_response(HttpResponse::Forbidden().finish().map_into_boxed_body()))
            } else {
                Ok(next.call(req).await?)
            }
        })
    }
}
