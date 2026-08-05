//! Types used by the HTTP cache middleware.

use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use bytes::Bytes;

/// A fully-buffered HTTP response, suitable for storing in a [`CacheStore`]
/// and replaying later without touching the original service.
///
/// [`CacheStore`]: crate::middleware::cache::store::CacheStore
#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub status: StatusCode,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub body: Bytes,
}

impl CachedResponse {
    pub fn new(status: StatusCode, headers: Vec<(HeaderName, HeaderValue)>, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    /// Rebuild a fresh `HttpResponse` from this cached entry.
    ///
    /// This never exposes the internal representation to handlers; callers
    /// only ever see a normal `HttpResponse`.
    pub fn into_http_response(self) -> HttpResponse {
        let mut builder = HttpResponse::build(self.status);
        for (name, value) in self.headers {
            builder.insert_header((name, value));
        }
        builder.body(self.body)
    }
}
