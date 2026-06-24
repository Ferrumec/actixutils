//! JWT authentication middleware.
//!
//! [`Auth<T>`] is the middleware counterpart to the [`crate::Auth`] extractor.
//! While the extractor validates on each handler invocation, this middleware validates
//! once per request and stores the claims in the request extensions, making them
//! available to all downstream handlers and middleware in the chain.
//!
//! Use this when you want to protect an entire scope rather than individual routes.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::{HS256Signer, Identity};
//! use actixutils::middleware::Auth;
//! use actix_web::{web, App};
//! use std::sync::Arc;
//!
//! let signer: Arc<dyn actixutils::Validate<Identity>> =
//!     Arc::new(HS256Signer::new("svc".to_string(), "secret".to_string()));
//!
//! App::new().service(
//!     web::scope("/api")
//!         .wrap(Auth { validator: signer })
//!         .route("/me", web::get().to(me_handler))
//! );
//! # async fn me_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! ```

use crate::Validate;
use actix_web::{
    Error, HttpMessage,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    http::header,
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::sync::Arc;

/// Middleware factory for JWT bearer-token authentication.
///
/// Register a `Arc<dyn Validate<T>>` as `validator`. On every request the
/// middleware extracts the bearer token from the `Authorization` header (or the
/// `access_token` cookie), calls `validate`, and — on success — inserts the
/// resulting `T` into the request extensions for downstream use.
///
/// If a `T` is already present in the extensions (e.g. from an outer middleware
/// layer), the validation step is skipped.
pub struct Auth<T> {
    /// The token validator. Must be thread-safe (`Send + Sync`).
    pub validator: Arc<dyn Validate<T>>,
}

impl<S, B, T: 'static> Transform<S, ServiceRequest> for Auth<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddleware<S, T>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddleware {
            service: Arc::new(service),
            signer: self.validator.clone(),
        }))
    }
}

/// The inner service produced by [`Auth`].
pub struct AuthMiddleware<S, T> {
    service: Arc<S>,
    signer: Arc<dyn Validate<T>>,
}

impl<S, B, T: 'static> Service<ServiceRequest> for AuthMiddleware<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let svc = Arc::clone(&self.service);
        let signer = Arc::clone(&self.signer);

        Box::pin(async move {
            // Skip validation if claims are already present (e.g. from an outer layer)
            if req.extensions().contains::<T>() {
                return svc.call(req).await;
            };

            let token: Option<String> = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .map(|s| s.replace("Bearer ", ""))
                .or_else(|| req.cookie("access_token").map(|c| c.value().to_string()));

            let token = match token {
                Some(t) => t,
                None => return Err(ErrorUnauthorized("Missing authorization header")),
            };

            // Validate token and get claims
            let claims = match signer.validate(&token) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("JWT validation error: {:?}", e);
                    return Err(ErrorUnauthorized("Invalid or expired token"));
                }
            };

            // Store claims in request extensions for downstream handlers
            req.extensions_mut().insert(claims);

            svc.call(req).await
        })
    }
}
