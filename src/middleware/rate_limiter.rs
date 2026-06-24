//! Sliding-window, per-identity rate limiting middleware.
//!
//! [`RateLimiter<T>`] tracks the number of requests made by each identity within a
//! rolling time window. Identities are extracted from the request using Actix-web's
//! [`FromRequest`] mechanism — any extractor that implements [`GetId`] can be used as
//! the key (e.g. [`Auth<Identity>`](crate::Auth), a session type, or a custom IP
//! extractor).
//!
//! When the limit is exceeded the middleware returns `429 Too Many Requests`
//! immediately, without invoking downstream handlers.
//!
//! The in-memory store is a [`DashMap`](dashmap::DashMap) of `VecDeque<Instant>` per
//! identity. Old timestamps are pruned lazily on each request. This is suitable for
//! single-instance deployments; for multi-node rate limiting you would need to back
//! the store with Redis or a similar shared store.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::{Auth, Identity};
//! use actixutils::middleware::RateLimiter;
//! use actix_web::{web, App};
//! use std::time::Duration;
//! use uuid::Uuid;
//!
//! // Implement GetId for the extractor type you want to key on
//! impl actixutils::middleware::GetId for Auth<Identity> {
//!     type Id = Uuid;
//!     fn id(&self) -> Uuid { self.0.sub }
//! }
//!
//! App::new().service(
//!     web::scope("/api")
//!         .wrap(RateLimiter::<Auth<Identity>>::new(100, Duration::from_secs(60)))
//! );
//! ```

use std::{
    collections::VecDeque,
    future::{Ready, ready},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use actix_web::{
    Error, FromRequest, HttpResponse,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use dashmap::DashMap;
use futures_util::future::LocalBoxFuture;

/// Provides a stable, hashable identity key for rate limiting.
///
/// Implement this on any Actix-web extractor (or wrapper) that identifies a client.
/// The associated `Id` type is used as the hash-map key, so it must be `Eq + Hash + Clone`.
pub trait GetId {
    /// The type used as the rate-limiter map key.
    type Id: Eq + Hash + Clone + Send + Sync + 'static;

    /// Extract the identity key from `self`.
    fn id(&self) -> Self::Id;
}

/// Middleware factory for sliding-window rate limiting.
///
/// `T` must implement both [`FromRequest`] (so it can be extracted per request)
/// and [`GetId`] (so a unique key can be derived).
///
/// # Arguments to [`RateLimiter::new`]
/// * `max_requests` — Maximum number of requests allowed per identity per `window`.
/// * `window`       — Rolling time window duration.
pub struct RateLimiter<T>
where
    T: GetId,
{
    max_requests: usize,
    window: Duration,
    store: Arc<DashMap<T::Id, VecDeque<Instant>>>,
    _marker: PhantomData<T>,
}

impl<T> Clone for RateLimiter<T>
where
    T: GetId,
{
    fn clone(&self) -> Self {
        Self {
            max_requests: self.max_requests,
            window: self.window,
            store: Arc::clone(&self.store),
            _marker: PhantomData,
        }
    }
}

impl<T> RateLimiter<T>
where
    T: GetId,
{
    /// Create a new `RateLimiter`.
    ///
    /// # Arguments
    /// * `max_requests` — Maximum requests allowed per identity within `window`.
    /// * `window`       — Duration of the sliding time window.
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            store: Arc::new(DashMap::new()),
            _marker: PhantomData,
        }
    }
}

impl<S, B, T> Transform<S, ServiceRequest> for RateLimiter<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    T: FromRequest + GetId + 'static,
    T::Future: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimiterMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimiterMiddleware {
            service: Arc::new(service),
            limiter: self.clone(),
            _marker: PhantomData,
        }))
    }
}

/// The inner service produced by [`RateLimiter`].
pub struct RateLimiterMiddleware<S, T>
where
    T: GetId,
{
    service: Arc<S>,
    limiter: RateLimiter<T>,
    _marker: PhantomData<T>,
}

impl<S, B, T> Service<ServiceRequest> for RateLimiterMiddleware<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    T: FromRequest + GetId + 'static,
    T::Future: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let limiter = self.limiter.clone();
        let service = Arc::clone(&self.service);

        Box::pin(async move {
            let (http_req, payload) = req.parts_mut();

            if let Ok(identity) = T::from_request(http_req, payload).await {
                let id = identity.id();
                let now = Instant::now();

                let mut entry = limiter.store.entry(id).or_insert_with(VecDeque::new);

                // Purge timestamps outside the current window
                while let Some(timestamp) = entry.front() {
                    if now.duration_since(*timestamp) > limiter.window {
                        entry.pop_front();
                    } else {
                        break;
                    }
                }

                if entry.len() >= limiter.max_requests {
                    drop(entry);
                    let response = req.into_response(
                        HttpResponse::TooManyRequests()
                            .body("Rate limit exceeded")
                            .map_into_right_body(),
                    );
                    return Ok(response);
                }

                entry.push_back(now);
            }

            let res = service.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}
