//! Actix-web middleware components.
//!
//! All middleware in this module follows the standard Actix pattern: a lightweight
//! configuration struct (e.g. [`RateLimiter`]) implements
//! [`Transform`](actix_web::dev::Transform), producing a per-worker service wrapper
//! that does the actual work. Where a middleware has a framework-agnostic
//! counterpart (a store trait, a task-local snapshot, a plain data struct), that
//! piece lives in [`crate::locals`] and is re-exported here for convenience.
//!
//! | Middleware | What it does |
//! |---|---|
//! | [`Auth<T>`] | Validates a Bearer JWT (header or `access_token` cookie) and stores claims in request extensions |
//! | [`ResponseEqualizer`] | Pads responses to a minimum duration, mitigating timing attacks |
//! | [`RateLimiter<T>`] | Sliding-window per-identity rate limiting (key trait: [`locals::rate_limiter::GetId`](crate::locals::rate_limiter::GetId)) |
//! | [`Idempotency<S>`] | Caches responses by idempotency key to prevent duplicate mutations (store trait: [`locals::IdempotencyStore`](crate::locals::IdempotencyStore)) |
//! | [`RequestId`] / [`RequestIdStr`] | Generates a UUID per request and adds `X-Request-Id` to responses |
//! | [`Context`] / [`ReadContext<T>`] | Builds an typed-eventbus publishing context per request |
//! | [`Pagination`] / [`PaginationMiddleware`] | Parses `?page=&limit=` and stores params in a task-local (state: [`locals::pagination`](crate::locals::pagination)) |
//! | [`Session<T>`] / [`SessionMiddleware`] | Cookie-based, server-side session storage; loads/saves via an async store trait implemented by the caller |
//! | [`AttachLocal<T>`] / [`SetLocal`] | Generic helper that extracts a `T` up front and runs the rest of the request inside `T::scope` (e.g. to populate a task-local) |
//!
//! ## Helper functions
//!
//! [`identity`] and [`authority`] are `Next`-style middleware functions (for use with
//! [`wrap_fn`](actix_web::web::ServiceConfig)) that validate the request against the
//! [`Identity`](crate::locals::Identity) and [`Authority`](crate::locals::Authority) types respectively.

mod attach_local;
mod auth;
mod constant_time;
#[cfg(feature = "es")]
mod context;
#[cfg(feature = "jwt")]
mod fns;
mod idempotency;
mod pagination;
mod rate_limiter;
mod request_id;
mod session;
#[cfg(test)]
mod test_session;
pub use attach_local::{AttachLocal, SetLocal};
pub use auth::Auth;
pub use constant_time::ResponseEqualizer;
#[cfg(feature = "es")]
pub use context::{Context, GetId, ReadContext};
#[cfg(feature = "jwt")]
pub use fns::{authority, identity};
pub use idempotency::Idempotency;
pub use pagination::{Pagination, PaginationMiddleware};
pub use rate_limiter::RateLimiter;
pub use request_id::{RequestId, RequestIdStr};
pub use session::{Session, SessionMiddleware};
