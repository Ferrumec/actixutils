//! Actix-web request extractors (types implementing [`FromRequest`](actix_web::FromRequest)).
//!
//! | Extractor | What it does |
//! |---|---|
//! | [`Jwt<T>`] | Validates a Bearer JWT (or reuses claims already set by [`middleware::Auth`](crate::middleware::Auth)) and yields `T` |
//! | [`Filters`] | Collects arbitrary `?field=value` query-string pairs into a `HashMap` |
//!
//! The cookie-based [`Session<T>`](crate::middleware::Session) extractor lives in
//! [`crate::middleware`] instead of here, since it is only usable once
//! [`SessionMiddleware`](crate::middleware::SessionMiddleware) has populated the request.
#[cfg(feature = "jwt")]
mod auth;
mod filters;
#[cfg(feature = "jwt")]
pub use auth::Jwt;
pub use filters::Filters;
