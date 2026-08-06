use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorGatewayTimeout,
    Error,
};
use futures::future::{ok, Ready};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::timeout as tokio_timeout;

pub struct TimeoutMiddleware {
    timeout: Duration,
}

impl TimeoutMiddleware {
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
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

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