//! Framework-agnostic types, traits, and task-local state.
//!
//! Nothing in this module (or its descendants) depends on `actix-web` directly.
//! These are the building blocks — JWT claim structs, signing/validation traits
//! and their HS256/RS256 implementations, store traits, task-local pagination
//! state, and the event-publishing [`Context`] — that the Actix-web-specific
//! [`crate::extractors`] and [`crate::middleware`] wire into a request pipeline.
//!
//! | Item | What it provides |
//! |---|---|
//! | [`Identity`] / [`Authority`] | Standard JWT claim structs |
//! | [`Sign<T>`] / [`Validate<T>`] | Core signing / validation traits |
//! | [`HS256Signer`] | HMAC-SHA-256 signer + validator |
//! | [`RS256Signer`] / [`RS256Validator`] | RSA-SHA-256 signer / validator |
//! | [`Provider<T>`] | Lightweight dependency-injection trait |
//! | [`SessionStore<T>`] | Backing store trait for [`crate::extractors::Session`] |
//! | [`IdempotencyStore`] | Backing store trait for the idempotency middleware |
//! | [`pagination::Pagination`] | Task-local pagination snapshot |
//! | [`context::Context`] (feature `es`) | Task-scoped event-publishing context |

mod claims;
mod hs256;
mod idempotency;
pub mod pagination;
mod provider;
mod rs256;
mod session_store;
mod signer_core;

pub mod rate_limiter;

#[cfg(feature = "es")]
pub mod context;

pub use claims::{Authority, Identity};
pub use hs256::HS256Signer;
pub use idempotency::{CachedResponse, IdempotencyState, IdempotencyStore};
pub use pagination::Pagination;
pub use provider::Provider;
pub use rs256::{RS256Signer, RS256Validator};
pub use session_store::SessionStore;
pub use signer_core::{Sign, Validate};

#[cfg(feature = "es")]
pub use context::{Context, GetId};
