//! Actix-web middleware components.
//!
//! All middleware in this module follows the standard Actix pattern: a lightweight
//! configuration struct (e.g. [`RateLimiter`]) implements
//! [`Transform`](actix_web::dev::Transform), producing a per-worker service wrapper
//! that does the actual work.
//!
//! | Middleware | What it does |
//! |---|---|
//! | [`Auth<T>`] | Validates a Bearer JWT and stores claims in request extensions |
//! | [`ResponseEqualizer`] | Pads responses to a minimum duration, mitigating timing attacks |
//! | [`RateLimiter<T>`] | Sliding-window per-identity rate limiting |
//! | [`Idempotency<S>`] | Caches responses by idempotency key to prevent duplicate mutations |
//! | [`RequestId`] / [`RequestIdStr`] | Generates a UUID per request and adds `X-Request-Id` to responses |
//! | [`Context`] / [`ReadContext<T>`] | Builds an event-stream publishing context per request |
//! | [`Pagination`] / [`PaginationMiddleware`] | Parses `?page=&limit=` and stores params in a task-local |
//!
//! ## Helper functions
//!
//! [`identity`] and [`authority`] are `Next`-style middleware functions (for use with
//! [`wrap_fn`](actix_web::web::ServiceConfig)) that validate the request against the
//! [`Identity`](crate::Identity) and [`Authority`](crate::Authority) types respectively.

mod auth;
mod constant_time;
mod fns;
mod request_id;
//#[cfg(feature="es")]
mod context;
mod idempotency;
mod rate_limiter;
mod pagination;
pub use pagination::{Pagination, PaginationMiddleware};
use crate::Authority;
pub use auth::Auth;
pub use constant_time::ResponseEqualizer;
pub use context::{Context, GetId, ReadContext};
pub use fns::{authority, identity};
pub use idempotency::Idempotency;
pub use rate_limiter::RateLimiter;
pub use request_id::{RequestId, RequestIdStr};
use uuid::Uuid;

impl GetId for Authority {
    fn get_id(&self) -> Uuid {
        self.sub.clone()
    }
}
