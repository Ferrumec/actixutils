//! # actixutils
//!
//! A comprehensive authentication, session management, and middleware utilities library
//! for [Actix-web](https://actix.rs/) applications.
//!
//! Actixutils provides battle-tested building blocks for secure, scalable HTTP services:
//!
//! - **JWT authentication** — HS256 (HMAC) and RS256 (RSA) signing and validation
//! - **Request extractors** — [`Jwt<T>`](extractors::Jwt) for JWT-authenticated handler
//!   arguments, plus [`Filters`](extractors::Filters) for ad-hoc query-string filters
//! - **Middleware suite** — authentication, rate limiting, idempotency, pagination, request ID
//!   injection, constant-time response equalisation, cookie-based sessions, and
//!   typed-eventbus context propagation
//! - **Role-based authorisation** — bitmask permission checks via [`Authority`]
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use actixutils::{HS256Signer, Identity, Jwt as Auth};
//! use actix_web::{web, App, HttpServer, HttpResponse};
//! use std::sync::Arc;
//! use uuid::Uuid;
//!
//! #[actix_web::main]
//! async fn main() -> std::io::Result<()> {
//!     let signer = Arc::new(HS256Signer::new(
//!         "my-app".to_string(),
//!         "super-secret-key".to_string(),
//!     ));
//!
//!     HttpServer::new(move || {
//!         App::new()
//!             .app_data(web::Data::from(signer.clone() as Arc<dyn actixutils::Validate<Identity>>))
//!             .route("/protected", web::get().to(protected))
//!     })
//!     .bind("127.0.0.1:8080")?
//!     .run()
//!     .await
//! }
//!
//! async fn protected(auth: Auth<Identity>) -> HttpResponse {
//!     HttpResponse::Ok().json(&auth.0)
//! }
//! ```
//!
//! ## Crate layout
//!
//! The crate is organised into three top-level modules, split by whether an item
//! depends on `actix-web`:
//!
//! | Module | Contains |
//! |---|---|
//! | [`extractors`] | Types implementing [`FromRequest`](actix_web::FromRequest): [`Jwt<T>`](extractors::Jwt), [`Filters`](extractors::Filters) |
//! | [`middleware`] | Types implementing [`Transform`](actix_web::dev::Transform): the full middleware suite, including the [`Session<T>`](middleware::Session) extractor and its [`SessionMiddleware`](middleware::SessionMiddleware) |
//! | [`locals`] | Everything framework-agnostic: claim structs, signing traits, store traits, and task-local state |
//!
//! The most commonly used items from `extractors` and `locals` are also re-exported
//! at the crate root for convenience (as shown in the quick-start example above), so
//! existing code that imports `actixutils::Jwt`, `actixutils::Identity`, etc.
//! continues to work unchanged.
//!
//! | Module / export | What it provides |
//! |---|---|
//! | [`Jwt<T>`] | Extractor that validates a Bearer token (header or `access_token` cookie) and yields `T` |
//! | [`Identity`] / [`Authority`] | Standard JWT claim structs |
//! | [`HS256Signer`] | HMAC-SHA-256 signer + validator |
//! | [`RS256Signer`] / [`RS256Validator`] | RSA-SHA-256 signer / validator |
//! | [`Sign<T>`] / [`Validate<T>`] | Core signing / validation traits |
//! | [`SessionStore<T>`] | General-purpose sync session-store trait (not used by [`middleware::Session`], which has its own async store trait) |
//! | [`middleware`] | Full middleware suite, including [`Session<T>`](middleware::Session) cookie sessions (see module docs) |
//! | `pubkey::configure` | Actix route that serves the public key at `/.well-known/public-key.pem` |

pub mod extractors;
pub mod locals;
pub mod middleware;
pub mod pubkey;

#[cfg(feature = "viewset")]
pub mod viewset;
#[cfg(feature = "jwt")]
pub use extractors::Jwt;
pub use locals::{Authority, Identity, Provider, SessionStore, Sign, Validate};
#[cfg(feature = "jwt")]
pub use locals::{HS256Signer, RS256Signer, RS256Validator};
