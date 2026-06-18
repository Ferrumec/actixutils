use std::{
    future::{Ready, ready},
    rc::Rc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use actix_web::{
    Error,
    body::MessageBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::LocalBoxFuture;
use rand::RngExt;
use tokio::time::sleep;

#[derive(Clone)]
pub struct ResponseEqualizer {
    min_duration: Duration,
    max_jitter: Duration,
}

impl ResponseEqualizer {
    pub fn new(min_duration: Duration) -> Self {
        Self {
            min_duration,
            max_jitter: Duration::ZERO,
        }
    }

    pub fn with_jitter(min_duration: Duration, max_jitter: Duration) -> Self {
        Self {
            min_duration,
            max_jitter,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for ResponseEqualizer
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ResponseEqualizerMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ResponseEqualizerMiddleware {
            service: Rc::new(service),
            min_duration: self.min_duration,
            max_jitter: self.max_jitter,
        }))
    }
}

pub struct ResponseEqualizerMiddleware<S> {
    service: Rc<S>,
    min_duration: Duration,
    max_jitter: Duration,
}

impl<S, B> Service<ServiceRequest> for ResponseEqualizerMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let min_duration = self.min_duration;
        let max_jitter = self.max_jitter;

        Box::pin(async move {
            let started = Instant::now();

            let response = service.call(req).await?;

            let elapsed = started.elapsed();

            if elapsed < min_duration {
                sleep(min_duration - elapsed).await;
            }

            if !max_jitter.is_zero() {
                let jitter_ns = rand::rng().random_range(0..=max_jitter.as_nanos() as u64);

                sleep(Duration::from_nanos(jitter_ns)).await;
            }

            Ok(response)
        })
    }
}
