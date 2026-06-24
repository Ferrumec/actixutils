//! Idempotency middleware for safe request deduplication.
//!
//! [`Idempotency<S>`] intercepts requests that carry an `Idempotency-Key` header
//! (configurable via [`Idempotency::header`]).  On the first call with a given key the
//! request is executed normally; the response is then stored in the provided
//! [`IdempotencyStore`].  Subsequent calls with the same key within the TTL window
//! receive the cached response immediately, **without** invoking the handler again.
//!
//! This is essential for mutation endpoints (POST, PUT, PATCH) where network retries
//! could otherwise cause duplicate state changes (e.g. double-charging a payment).
//!
//! ## Store contract
//!
//! You must supply a concrete [`IdempotencyStore`] implementation.  The middleware
//! communicates with it through three operations:
//!
//! * [`acquire`](IdempotencyStore::acquire) — atomically reserves a key. Returns
//!   `true` if the caller now owns execution, `false` if the key already existed.
//! * [`complete`](IdempotencyStore::complete) — stores the finished response.
//! * [`release`](IdempotencyStore::release) — removes an in-progress reservation on
//!   error.
//! * [`get`](IdempotencyStore::get) — retrieves the current state of a key.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::middleware::{Idempotency, IdempotencyStore, IdempotencyState, CachedResponse};
//! use actix_web::{web, App};
//! use async_trait::async_trait;
//! use std::{sync::Arc, time::Duration};
//!
//! struct MyStore; // backed by Redis, DashMap, etc.
//!
//! #[async_trait]
//! impl IdempotencyStore for MyStore {
//!     type Error = std::io::Error;
//!     async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error> { Ok(true) }
//!     async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error> { Ok(None) }
//!     async fn complete(&self, key: &str, r: CachedResponse) -> Result<(), Self::Error> { Ok(()) }
//!     async fn release(&self, key: &str) -> Result<(), Self::Error> { Ok(()) }
//! }
//!
//! App::new().service(
//!     web::scope("/payments")
//!         .wrap(Idempotency::new(Arc::new(MyStore)).ttl(Duration::from_secs(86400)))
//!         .route("/charge", web::post().to(charge_handler))
//! );
//! # async fn charge_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

/// A serialisable snapshot of an HTTP response for caching.
#[derive(Clone)]
pub struct CachedResponse {
    /// HTTP status code as a `u16`.
    pub status: u16,
    /// Response headers as `(name, value)` string pairs.
    pub headers: Vec<(String, String)>,
    /// Raw response body bytes.
    pub body: Bytes,
}

/// The lifecycle state of an idempotency key.
pub enum IdempotencyState {
    /// A request with this key is currently being processed.
    InProgress,
    /// A request with this key completed successfully and its response is cached.
    Completed(CachedResponse),
}

/// Backing store abstraction for the [`Idempotency`] middleware.
///
/// Implementors must guarantee that [`acquire`](Self::acquire) is atomic — i.e. if two
/// concurrent requests arrive with the same key, exactly one should receive `Ok(true)`.
#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// The error type returned by store operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Attempt to reserve `key` for exclusive execution.
    ///
    /// Returns:
    /// * `Ok(true)`  — The caller owns this key and should process the request.
    /// * `Ok(false)` — The key already exists; the caller should check [`get`](Self::get).
    async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error>;

    /// Retrieve the current state of `key`, if any.
    async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error>;

    /// Persist the finished response for `key`.
    async fn complete(&self, key: &str, response: CachedResponse) -> Result<(), Self::Error>;

    /// Release an in-progress reservation for `key` (called on error paths).
    async fn release(&self, key: &str) -> Result<(), Self::Error>;
}

use std::sync::Arc;

/// Middleware factory for idempotent request handling.
///
/// Create with [`Idempotency::new`], optionally customise TTL or header name,
/// then wrap a scope or app with it.
#[derive(Clone)]
pub struct Idempotency<S> {
    store: Arc<S>,
    ttl: Duration,
    header: &'static str,
}

impl<S> Idempotency<S> {
    /// Create a new `Idempotency` middleware using `store`.
    ///
    /// Defaults: TTL = 1 hour, header name = `"Idempotency-Key"`.
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            ttl: Duration::from_secs(60 * 60),
            header: "Idempotency-Key",
        }
    }

    /// Override the cache TTL (default: 1 hour).
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Override the header name used to look up the idempotency key (default: `"Idempotency-Key"`).
    pub fn header(mut self, header: &'static str) -> Self {
        self.header = header;
        self
    }
}

use actix_web::{
    Error,
    body::{BoxBody, MessageBody},
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::rc::Rc;

impl<S, B, Store> Transform<S, ServiceRequest> for Idempotency<Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    Store: IdempotencyStore,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();

    type Transform = IdempotencyMiddleware<S, Store>;

    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(IdempotencyMiddleware {
            service: Rc::new(service),
            store: self.store.clone(),
            ttl: self.ttl,
            header: self.header,
        }))
    }
}

/// The inner service produced by [`Idempotency`].
pub struct IdempotencyMiddleware<S, Store> {
    service: Rc<S>,
    store: Arc<Store>,
    ttl: Duration,
    header: &'static str,
}

use actix_web::{HttpResponse, body, http::StatusCode};

impl<S, B, Store> Service<ServiceRequest> for IdempotencyMiddleware<S, Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    Store: IdempotencyStore,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;

    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let store = self.store.clone();
        let ttl = self.ttl;
        let header = self.header;

        Box::pin(async move {
            // If no idempotency key header, pass through normally
            let Some(key) = req
                .headers()
                .get(header)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
            else {
                let res = service.call(req).await?;
                return Ok(res.map_into_boxed_body());
            };

            match store.acquire(&key, ttl).await {
                Ok(true) => {}
                Ok(false) => match store.get(&key).await {
                    Ok(Some(IdempotencyState::Completed(cached))) => {
                        let mut builder =
                            HttpResponse::build(StatusCode::from_u16(cached.status).unwrap());

                        for (name, value) in cached.headers {
                            builder.append_header((name, value));
                        }

                        let response = builder.body(cached.body);
                        return Ok(req.into_response(response));
                    }

                    Ok(Some(IdempotencyState::InProgress)) => {
                        return Ok(req.into_response(
                            HttpResponse::Conflict().body("request already in progress"),
                        ));
                    }

                    _ => {
                        return Ok(req.into_response(HttpResponse::InternalServerError().finish()));
                    }
                },

                Err(_) => {
                    return Ok(req.into_response(HttpResponse::InternalServerError().finish()));
                }
            }

            let response = service.call(req).await?;
            let (req, res) = response.into_parts();
            let status = res.status();

            let headers = res
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_owned())))
                .collect();

            let body = match body::to_bytes(res.into_body()).await {
                Ok(r) => r,
                Err(_e) => {
                    tracing::error!("failed to get bytes from response body");
                    return Ok(ServiceResponse::new(
                        req,
                        HttpResponse::InternalServerError().finish(),
                    ));
                }
            };

            let cached = CachedResponse {
                status: status.as_u16(),
                headers,
                body: body.clone(),
            };

            if store.complete(&key, cached).await.is_err() {
                let _ = store.release(&key).await;
            }

            Ok(ServiceResponse::new(
                req,
                HttpResponse::build(status).body(body).map_into_boxed_body(),
            ))
        })
    }
}
