//! Event-stream publishing context middleware.
//!
//! [`ReadContext<T>`] builds a per-request [`Context`](crate::locals::context::Context)
//! value that bundles the request ID, the authenticated user's UUID, an
//! [`EventStream`] handle, and a producer name. Handlers and services can then call
//! [`Context::publish`](crate::locals::context::Context::publish) to emit domain
//! events without needing to carry these dependencies in their function signatures.
//!
//! This middleware depends on two upstream middleware being applied first:
//! 1. [`RequestId`](super::RequestId) — must have stored a [`RequestIdStr`] in the
//!    request extensions.
//! 2. Any auth middleware that stores a `T: GetId` in the extensions (e.g.
//!    [`Auth<Authority>`](super::Auth)).
//!
//! # Example
//! ```rust,no_run
//! use actixutils::locals::Authority;
//! use actixutils::middleware::{RequestId, ReadContext, Context};
//! use actix_web::{web, App, HttpResponse, HttpMessage};
//! use std::sync::Arc;
//!
//! async fn create_item(req: actix_web::HttpRequest) -> HttpResponse {
//!     if let Some(ctx) = req.extensions().get::<Context>() {
//!         // ctx.publish(MyEvent { ... }).await;
//!     }
//!     HttpResponse::Ok().finish()
//! }
//! ```

pub use crate::locals::context::{Context, GetId};
use crate::middleware::RequestIdStr;
use actix_web::HttpMessage;
use actix_web::error::ErrorInternalServerError;
use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::LocalBoxFuture;
use std::future::{Ready, ready};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as Ctx, Poll};
use typed_eventbus::EventStream;
use uuid::Uuid;

/// Middleware factory that constructs a [`Context`] for each request.
///
/// `T` must implement [`GetId`] and must be present in the request extensions when
/// this middleware runs (i.e. an auth middleware must have run first).
pub struct ReadContext<T> {
    es: Arc<dyn EventStream>,
    name: String,
    user_as_audience: bool,
    _marker: PhantomData<T>,
}

impl<T> Clone for ReadContext<T> {
    fn clone(&self) -> Self {
        Self {
            es: self.es.clone(),
            name: self.name.clone(),
            user_as_audience: self.user_as_audience,
            _marker: PhantomData::<T>,
        }
    }
}

impl<T> ReadContext<T> {
    /// Create a new `ReadContext` middleware factory.
    ///
    /// # Arguments
    /// * `es`   — Shared handle to the event stream (e.g. NATS, Kafka, in-memory bus).
    /// * `name` — Producer / service name embedded in every published event's metadata.
    pub fn new(es: Arc<dyn EventStream>, name: String) -> Self {
        Self {
            es,
            name,
            user_as_audience: false,
            _marker: PhantomData,
        }
    }

    pub fn with_user_as_audience(mut self, status: bool) -> Self {
        self.user_as_audience = status;
        self
    }
}

impl<S, B, T: GetId + 'static> Transform<S, ServiceRequest> for ReadContext<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ReadContextService<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ReadContextService {
            service: Rc::new(service),
            state: self.clone(),
        }))
    }
}

/// The inner service produced by [`ReadContext`].
pub struct ReadContextService<S, T> {
    service: Rc<S>,
    state: ReadContext<T>,
}

impl<S, B, T: GetId + 'static> Service<ServiceRequest> for ReadContextService<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Ctx<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let es = self.state.es.clone();
        let producer = self.state.name.clone();
        let svc = self.service.clone();
        let user_as_audience = self.state.user_as_audience;
        Box::pin(async move {
            let request_id = match req.extensions().get::<RequestIdStr>() {
                Some(r) => match Uuid::parse_str(&r.0) {
                    Ok(u) => u,
                    Err(_) => {
                        tracing::error!("Invalid RequestIdStr uuid format");
                        return Err(ErrorInternalServerError("Invalid request id"));
                    }
                },
                None => {
                    tracing::error!("RequestIdStr not found in request data");
                    return Err(ErrorInternalServerError("Missing request id"));
                }
            };

            let user_id = match req.extensions().get::<T>() {
                Some(u) => u,
                None => {
                    tracing::error!("Identity not found in request data");
                    return Err(ErrorInternalServerError("Missing identity"));
                }
            }
            .get_id();

            let ctx = Context {
                request_id,
                user_id,
                es,
                producer,
                user_as_audience,
            };

            req.extensions_mut().insert(ctx);
            svc.call(req).await
        })
    }
}
