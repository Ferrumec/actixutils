//! GET-only HTTP response caching middleware.
//!
//! The cache key is derived exclusively from `host + path + query`. The
//! middleware never inspects authentication, cookies, or any other request
//! header when computing the key, which means two authenticated requests
//! resolving to the same URL will resolve to the same cache entry. This is
//! intentional: the cache is purely URL-based. Applications are responsible
//! for choosing URLs that identify a shareable resource (e.g.
//! `/users/123/orders`) before applying this middleware to a route.

use std::future::{Ready, ready};
use std::rc::Rc;
use std::sync::Arc;

use actix_web::body::{BodySize, BoxBody, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};
use actix_web::http::{Method, header};
use actix_web::{Error, HttpResponse};
use futures_util::future::LocalBoxFuture;

use super::types::CachedResponse;
use crate::Store;

/// Middleware factory. Wrap a route or scope with `Cache::new(store)`.
///
/// ```ignore
/// App::new().wrap(Cache::new(store).ttl(Duration::from_secs(60)))
/// ```
#[derive(Clone)]
pub struct Cache {
    store: Arc<dyn Store<String, CachedResponse>>,
}

impl Cache {
    /// Default TTL is 60 seconds; override with [`Cache::ttl`].
    pub fn new(store: Arc<dyn Store<String, CachedResponse>>) -> Self {
        Self { store }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Cache
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = CacheMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(CacheMiddleware {
            service: Rc::new(service),
            store: self.store.clone(),
        }))
    }
}

pub struct CacheMiddleware<S> {
    service: Rc<S>,
    store: Arc<dyn Store<String, CachedResponse>>,
}

/// `host + path + query`. Fragments are never part of an HTTP request, so
/// there is nothing to strip; the method is intentionally excluded because
/// only `GET` is ever cached.
fn cache_key(req: &ServiceRequest) -> String {
    let host = req.connection_info().host().to_string();
    let path = req.uri().path();
    match req.uri().query() {
        Some(query) => format!("{host}{path}?{query}"),
        None => format!("{host}{path}"),
    }
}

/// Conservative cache-control handling: opt out on `no-store`, `private`,
/// `Set-Cookie`, or a non-success status. This is intentionally not a full
/// implementation of HTTP caching semantics.
fn is_cacheable(resp: &HttpResponse) -> bool {
    if !resp.status().is_success() {
        return false;
    }

    if resp.headers().contains_key(header::SET_COOKIE) {
        return false;
    }

    if let Some(cache_control) = resp.headers().get(header::CACHE_CONTROL)
        && let Ok(value) = cache_control.to_str()
    {
        let value = value.to_ascii_lowercase();
        if value.contains("no-store") || value.contains("private") {
            return false;
        }
    }

    true
}

impl<S, B> Service<ServiceRequest> for CacheMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Non-GET requests never touch the cache in any way.
        if req.method() != Method::GET {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res.map_into_boxed_body())
            });
        }

        let key = cache_key(&req);
        let store = self.store.clone();
        let service = self.service.clone();

        Box::pin(async move {
            if let Ok(Some(cached)) = store.get(&key).await {
                let (http_req, _) = req.into_parts();
                let response = cached.into_http_response();
                return Ok(ServiceResponse::new(http_req, response));
            }

            let res = service.call(req).await?;
            let res = res.map_into_boxed_body();

            if !is_cacheable(res.response()) {
                return Ok(res);
            }

            // Only buffer bodies with a known, finite length. Streaming or
            // unknown-length bodies are left untouched and simply aren't
            // cached in this first implementation.
            if !matches!(res.response().body().size(), BodySize::Sized(_)) {
                return Ok(res);
            }

            let (http_req, http_response) = res.into_parts();
            let status = http_response.status();
            let headers: Vec<_> = http_response
                .headers()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();

            let body = http_response.into_body();
            let bytes = match actix_web::body::to_bytes(body).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    // The body was already consumed and failed to buffer.
                    // Nothing was written to the store, so it stays
                    // uncorrupted; return the best reconstruction we can
                    // (status + headers, empty body) rather than hang.
                    let mut builder = HttpResponse::build(status);
                    for (name, value) in headers {
                        builder.insert_header((name, value));
                    }
                    return Ok(ServiceResponse::new(http_req, builder.finish()));
                }
            };

            let cached_response = CachedResponse::new(status, headers.clone(), bytes.clone());
            if let Err(e) = store.set(&key, cached_response).await {
                tracing::error!("Error in setting cache value: {}", e);
            };

            let mut builder = HttpResponse::build(status);
            for (name, value) in headers {
                builder.insert_header((name, value));
            }
            let response = builder.body(bytes);

            Ok(ServiceResponse::new(http_req, response))
        })
    }
}
