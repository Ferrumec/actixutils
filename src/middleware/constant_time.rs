//! Constant-time response middleware for timing attack mitigation.
//!
//! Authentication and sensitive lookup endpoints can leak information through
//! response timing: a login that fails on a non-existent user returns faster than
//! one that fails on a wrong password, because the password hash check is skipped.
//!
//! [`ResponseEqualizer`] addresses this by ensuring that every response takes *at
//! least* `min_duration` to return. If the handler finishes early, the middleware
//! sleeps for the remaining time. An optional random jitter can be added on top to
//! make statistical timing analysis harder.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::middleware::ResponseEqualizer;
//! use actix_web::{web, App};
//! use std::time::Duration;
//!
//! App::new().service(
//!     web::scope("/auth")
//!         .wrap(ResponseEqualizer::with_jitter(
//!             Duration::from_millis(150), // minimum response time
//!             Duration::from_millis(50),  // add up to 50 ms of random jitter
//!         ))
//!         .route("/login", web::post().to(login_handler)),
//! );
//! # async fn login_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! ```

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

/// Middleware factory that pads response times to a configurable minimum.
///
/// Construct with [`ResponseEqualizer::new`] for a fixed floor, or
/// [`ResponseEqualizer::with_jitter`] to add randomness on top.
#[derive(Clone)]
pub struct ResponseEqualizer {
    min_duration: Duration,
    max_jitter: Duration,
}

impl ResponseEqualizer {
    /// Create a `ResponseEqualizer` with a fixed minimum response time and no jitter.
    ///
    /// # Arguments
    /// * `min_duration` — Every response will take *at least* this long.
    pub fn new(min_duration: Duration) -> Self {
        Self {
            min_duration,
            max_jitter: Duration::ZERO,
        }
    }

    /// Create a `ResponseEqualizer` with both a minimum duration and a random jitter.
    ///
    /// The actual sleep is `max(0, min_duration - elapsed) + random(0..=max_jitter)`.
    ///
    /// # Arguments
    /// * `min_duration` — Minimum response time floor.
    /// * `max_jitter`   — Upper bound of the random jitter added after the floor.
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

/// The inner service produced by [`ResponseEqualizer`].
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

            // Pad to the minimum duration
            if elapsed < min_duration {
                sleep(min_duration - elapsed).await;
            }

            // Add optional random jitter
            if !max_jitter.is_zero() {
                let jitter_ns = rand::rng().random_range(0..=max_jitter.as_nanos() as u64);
                sleep(Duration::from_nanos(jitter_ns)).await;
            }

            Ok(response)
        })
    }
}
