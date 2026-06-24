//! # actixutils
//!
//! A comprehensive authentication, session management, and middleware utilities library
//! for [Actix-web](https://actix.rs/) applications.
//!
//! Actixutils provides battle-tested building blocks for secure, scalable HTTP services:
//!
//! - **JWT authentication** — HS256 (HMAC) and RS256 (RSA) signing and validation
//! - **Request extractors** — [`Auth<T>`], [`Access`], and [`Session<T>`] for handler arguments
//! - **Middleware suite** — authentication, rate limiting, idempotency, pagination, request ID
//!   injection, constant-time response equalisation, and event-stream context propagation
//! - **Role-based authorisation** — bitmask permission checks via [`Authority`]
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use actixutils::{HS256Signer, Identity, Auth};
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
//! | Module / export | What it provides |
//! |---|---|
//! | [`Auth<T>`] | Extractor that validates a Bearer token and yields `T` |
//! | [`Access`] | Extractor that yields the raw token string for manual validation |
//! | [`Session<T>`] / [`SessionStore<T>`] | Cookie-based session extractor |
//! | [`Identity`] / [`Authority`] | Standard JWT claim structs |
//! | [`HS256Signer`] | HMAC-SHA-256 signer + validator |
//! | [`RS256Signer`] / [`RS256Validator`] | RSA-SHA-256 signer / validator |
//! | [`Sign<T>`] / [`Validate<T>`] | Core signing / validation traits |
//! | [`middleware`] | Full middleware suite (see module docs) |
//! | `pubkey::configure` | Actix route that serves the public key at `/.well-known/public-key.pem` |

mod headers;
pub mod pubkey;
pub mod utils;
pub use headers::Access;
pub mod middleware;
mod provider;
pub use provider::Provider;
mod auth;
mod common;
mod hs256;
mod rs256;
mod session;
mod signer_core;
pub use auth::Auth;
pub use common::{Authority, Identity};
pub use hs256::HS256Signer;
pub use rs256::{RS256Signer, RS256Validator};
pub use session::{Session, SessionStore};
pub use signer_core::{Sign, Validate};
