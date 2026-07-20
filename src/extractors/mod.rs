//! Actix-web request extractors (types implementing [`FromRequest`](actix_web::FromRequest)).
//!
//! | Extractor | What it does |
//! |---|---|
//! | [`Auth<T>`] | Validates a Bearer JWT (or reuses claims set by [`middleware::Auth`](crate::middleware)) and yields `T` |
//! | [`Access`] | Yields the raw Bearer token string for manual validation |
//! | [`Session<T>`] | Resolves the `session_id` cookie to a typed session value via [`SessionStore<T>`](crate::locals::SessionStore) |
#[cfg(feature = "jwt")]
mod auth;
mod filters;
#[cfg(feature = "jwt")]
pub use auth::Jwt;
pub use filters::Filters;
