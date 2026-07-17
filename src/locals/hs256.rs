//! HMAC-SHA-256 (`HS256`) JWT signer and validator.
//!
//! [`HS256Signer`] implements both [`Sign<T>`](crate::locals::Sign) and
//! [`Validate<T>`](crate::locals::Validate), making it a self-contained symmetric-key
//! token authority suitable for services that both issue and verify their own tokens.

use super::signer_core::{Sign, Validate};
use anyhow::Result;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Serialize, de::DeserializeOwned};

/// A symmetric JWT signer and validator using the HS256 algorithm.
///
/// Wrap in an [`Arc`](std::sync::Arc) and register with
/// [`web::Data::from`](actix_web::web::Data::from) to make it available to
/// [`Auth<T>`](crate::extractors::Auth) and [`middleware::Auth`](crate::middleware).
///
/// # Example
/// ```rust,no_run
/// use actixutils::locals::{HS256Signer, Identity, Sign, Validate};
/// use uuid::Uuid;
///
/// let signer = HS256Signer::new("my-service".to_string(), "secret".to_string());
///
/// let claims = Identity::new(Uuid::new_v4(), vec!["my-service".to_string()]);
/// let token = signer.sign(&claims).unwrap();
/// let decoded: Identity = signer.validate(&token).unwrap();
/// ```
#[derive(Clone)]
pub struct HS256Signer {
    secret: String,
    header: Header,
    validation: Validation,
}

impl HS256Signer {
    /// Create a new `HS256Signer` that signs tokens for `aud` using `secret`.
    ///
    /// # Arguments
    /// * `aud`    — The audience string embedded in every signed token and required
    ///              during validation.
    /// * `secret` — The HMAC shared secret. Keep this private.
    pub fn new(aud: String, secret: String) -> Self {
        let mut vald = Validation::new(Algorithm::HS256);
        vald.set_audience(&[aud]);
        HS256Signer {
            secret,
            header: Header::new(Algorithm::HS256),
            validation: vald,
        }
    }
}

impl<T> Sign<T> for HS256Signer
where
    T: Serialize,
{
    /// Sign `claims` with the HMAC secret and return the compact JWT.
    fn sign(&self, claims: &T) -> Result<String> {
        Ok(encode(
            &self.header,
            claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )?)
    }
}

impl<T> Validate<T> for HS256Signer
where
    T: DeserializeOwned,
{
    /// Verify the token's HS256 signature and deserialise its claims.
    ///
    /// The `aud` claim must match the value supplied to [`HS256Signer::new`].
    fn validate(&self, token: &str) -> Result<T> {
        let data = decode::<T>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &self.validation,
        )?;

        Ok(data.claims)
    }
}
