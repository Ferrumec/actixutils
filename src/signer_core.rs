//! Core signing and validation traits.
//!
//! These traits are the foundation of actixutils' pluggable JWT design.
//! [`HS256Signer`](crate::HS256Signer), [`RS256Signer`](crate::RS256Signer), and
//! [`RS256Validator`](crate::RS256Validator) all implement one or both traits, and
//! [`Auth<T>`](crate::Auth) / [`middleware::Auth`](crate::middleware) accept any
//! `Arc<dyn Validate<T>>` at runtime.

use anyhow::Result;

/// Encode a claims value as a signed JWT string.
///
/// Implement this trait on any type that can produce a signed token, e.g. a struct
/// holding an [`EncodingKey`](jsonwebtoken::EncodingKey).
///
/// # Type parameters
/// * `T` — A serialisable claims type (e.g. [`Identity`](crate::Identity) or
///   [`Authority`](crate::Authority)).
pub trait Sign<T>: Send + Sync + 'static {
    /// Sign `claims` and return the compact serialisation of the resulting JWT.
    fn sign(&self, claims: &T) -> Result<String>;
}

/// Decode and cryptographically verify a JWT string, yielding the claims on success.
///
/// This trait is used as a trait object (`Arc<dyn Validate<T>>`) in both the
/// [`Auth<T>`](crate::Auth) extractor and the [`middleware::Auth`](crate::middleware)
/// middleware, allowing any compatible signer or dedicated validator to be injected
/// into the application state.
///
/// # Type parameters
/// * `T` — A deserialisable claims type (e.g. [`Identity`](crate::Identity)).
pub trait Validate<T>: Send + Sync + 'static {
    /// Verify `token` and deserialise its payload into `T`.
    ///
    /// Returns `Err` if the token is malformed, the signature is invalid, the
    /// algorithm does not match, or the `aud` claim fails validation.
    fn validate(&self, token: &str) -> Result<T>;
}
