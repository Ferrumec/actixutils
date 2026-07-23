//! Core signing and validation traits.
//!
//! These traits are the foundation of actixutils' pluggable JWT design.
//! [`HS256Signer`](crate::locals::HS256Signer), [`RS256Signer`](crate::locals::RS256Signer), and
//! [`RS256Validator`](crate::locals::RS256Validator) all implement one or both traits, and
//! [`Jwt<T>`](crate::extractors::Jwt) / [`middleware::Auth`](crate::middleware::Auth) accept any
//! `Arc<dyn Validate<T>>` at runtime.

use anyhow::Result;

/// Encode a claims value as a signed JWT string.
///
/// Implement this trait on any type that can produce a signed token, e.g. a struct
/// holding an [`EncodingKey`](jsonwebtoken::EncodingKey).
///
/// # Type parameters
/// * `T` — A serialisable claims type (e.g. [`Identity`](crate::locals::Identity) or
///   [`Authority`](crate::locals::Authority)).
pub trait Sign<T>: Send + Sync + 'static {
    /// Sign `claims` and return the compact serialisation of the resulting JWT.
    fn sign(&self, claims: &T) -> Result<String>;
}

/// Decode and cryptographically verify a JWT string, yielding the claims on success.
///
/// This trait is used as a trait object (`Arc<dyn Validate<T>>`) in both the
/// [`Jwt<T>`](crate::extractors::Jwt) extractor and the [`middleware::Auth`](crate::middleware::Auth)
/// middleware, allowing any compatible signer or dedicated validator to be injected
/// into the application state.
///
/// # Type parameters
/// * `T` — A deserialisable claims type (e.g. [`Identity`](crate::locals::Identity)).
pub trait Validate<T>: Send + Sync + 'static {
    /// Verify `token` and deserialise its payload into `T`.
    ///
    /// Returns `Err` if the token is malformed, the signature is invalid, the
    /// algorithm does not match, or the `aud` claim fails validation.
    fn validate(&self, token: &str) -> Result<T>;
}
