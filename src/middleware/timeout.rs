//! Per-request timeout middleware.
//!
//! [`TimeoutMiddleware`] wraps every request in a fixed [`Duration`] budget.
//! If the wrapped service hasn't produced a response by the deadline, the
//! in-flight future is dropped and a `504 Gateway Timeout` is returned
//! instead.

use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorGatewayTimeout,
};
use futures::future::{Ready, ok};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::timeout as tokio_timeout;

/// Middleware factory that fails requests exceeding a fixed [`Duration`]
/// with a `504 Gateway Timeout`.
pub struct TimeoutMiddleware {
    timeout: Duration,
}

impl TimeoutMiddleware {
    /// Create a `TimeoutMiddleware` that aborts requests taking longer than
    /// `timeout` to complete.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl<S, B> Transform<S, ServiceRequest> for TimeoutMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = TimeoutMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(TimeoutMiddlewareService {
            service,
            timeout: self.timeout,
        })
    }
}

/// Per-worker service produced by [`TimeoutMiddleware`]; races the wrapped
/// service against the configured deadline.
pub struct TimeoutMiddlewareService<S> {
    service: S,
    timeout: Duration,
}

impl<S, B> Service<ServiceRequest> for TimeoutMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let duration = self.timeout;
        let fut = self.service.call(req);

        Box::pin(async move {
            match tokio_timeout(duration, fut).await {
                Ok(result) => result,
                Err(_) => Err(ErrorGatewayTimeout("Request timed out")),
            }
        })
    }
}
