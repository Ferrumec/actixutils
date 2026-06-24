//! Per-request UUID generation middleware.
//!
//! [`RequestId`] generates a fresh `UUIDv4` for each incoming request, stores it
//! as a [`RequestIdStr`] in the request extensions, records it in the current
//! `tracing` span, and appends it to the response as the `X-Request-Id` header.
//!
//! This header is useful for distributed tracing and correlating logs across
//! services. The [`context`](super::context) middleware depends on `RequestId`
//! being applied first.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::middleware::RequestId;
//! use actix_web::{web, App};
//!
//! App::new()
//!     .wrap(RequestId)
//!     .route("/ping", web::get().to(ping));
//! # async fn ping() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! ```

use actix_web::HttpMessage;
use std::{rc::Rc, str::FromStr};

use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::header::{HeaderName, HeaderValue},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use uuid::Uuid;

/// Middleware factory that injects a unique request identifier into every request.
pub struct RequestId;

impl<S, B> Transform<S, ServiceRequest> for RequestId
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestIdMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestIdMiddleware {
            service: Rc::new(service),
        }))
    }
}

/// The inner service produced by [`RequestId`].
pub struct RequestIdMiddleware<S> {
    service: Rc<S>,
}

/// A newtype wrapper around the request-scoped UUID string.
///
/// Stored in request extensions by [`RequestId`] middleware and read by
/// [`ReadContext`](super::context::ReadContext) and any handler that needs the
/// correlation ID.
///
/// # Example
/// ```rust,no_run
/// use actixutils::middleware::RequestIdStr;
/// use actix_web::{HttpRequest, HttpResponse, HttpMessage};
///
/// async fn handler(req: HttpRequest) -> HttpResponse {
///     if let Some(rid) = req.extensions().get::<RequestIdStr>() {
///         println!("request id: {}", rid.0);
///     }
///     HttpResponse::Ok().finish()
/// }
/// ```
#[derive(Clone)]
pub struct RequestIdStr(pub String);

impl<S, B> Service<ServiceRequest> for RequestIdMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let req_id = Uuid::new_v4().to_string();
        req.extensions_mut().insert(RequestIdStr(req_id.clone()));
        tracing::Span::current().record("request_id", &req_id);

        let fut = self.service.call(req);
        Box::pin(async move {
            let mut res = fut.await?;
            res.headers_mut().append(
                HeaderName::from_str("X-Request-Id").unwrap(),
                HeaderValue::from_str(&req_id).unwrap(),
            );
            Ok(res)
        })
    }
}
