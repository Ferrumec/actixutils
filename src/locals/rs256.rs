//! RSA-SHA-256 (`RS256`) JWT signer and validator.
//!
//! Unlike the symmetric [`HS256Signer`](crate::locals::HS256Signer), the RSA approach
//! separates signing from verification:
//!
//! * [`RS256Signer`]    — holds the **private key** and signs tokens. Typically used by an
//!   auth service.
//! * [`RS256Validator`] — holds the **public key** and verifies tokens. Used by downstream
//!   services that trust the auth service's signature.
//!
//! Both types implement the corresponding [`Sign<T>`](crate::locals::Sign) /
//! [`Validate<T>`](crate::locals::Validate) traits.

use super::signer_core::{Sign, Validate};
use anyhow::Result;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Serialize, de::DeserializeOwned};

/// An RSA-based JWT signer that uses a PEM-encoded private key.
///
/// # Example
/// ```rust,no_run
/// use actixutils::locals::{RS256Signer, Sign, Identity};
/// use uuid::Uuid;
///
/// let private_key = std::fs::read_to_string("private.pem").unwrap();
/// let signer = RS256Signer::new(private_key, "my-service".to_string());
///
/// let claims = Identity::new(Uuid::new_v4(), vec!["my-service".to_string()]);
/// let token = signer.sign(&claims).unwrap();
/// ```
#[derive(Clone)]
pub struct RS256Signer {
    enc_key: EncodingKey,
    header: Header,
}

/// An RSA-based JWT validator that uses a PEM-encoded public key.
///
/// Wrap in an [`Arc`](std::sync::Arc) and register as app data so that
/// [`Auth<T>`](crate::extractors::Auth) can use it to verify tokens produced by a remote
/// [`RS256Signer`].
///
/// # Example
/// ```ignore
/// use actixutils::locals::{RS256Validator, Validate, Identity};
///
/// let public_key = std::fs::read_to_string("public.pem").unwrap();
/// let validator = RS256Validator::new(public_key, "my-service".to_string());
///
/// let identity: Identity = validator.validate(&token).unwrap();
/// ```
#[derive(Clone)]
pub struct RS256Validator {
    dec_key: DecodingKey,
    validation: Validation,
}

impl RS256Signer {
    /// Alias for [`RS256Signer::new`] with argument order swapped.
    ///
    /// Provided for ergonomic parity with older call-sites.
    pub fn default(aud: String, private_key: String) -> Self {
        RS256Signer::new(private_key, aud)
    }

    /// Create a new `RS256Signer`.
    ///
    /// # Arguments
    /// * `private_key` — PEM-encoded RSA private key (PKCS#8 or traditional format).
    /// * `aud`         — Audience claim embedded in every signed token.
    ///
    /// # Panics
    /// Panics if `private_key` is not a valid RSA PEM string.
    pub fn new(private_key: String, aud: String) -> Self {
        let enc_key =
            EncodingKey::from_rsa_pem(private_key.as_bytes()).expect("invalid private key");
        let mut vald = Validation::new(Algorithm::RS256);
        vald.set_audience(&[aud]);
        RS256Signer {
            enc_key,
            header: Header::new(Algorithm::RS256),
        }
    }
}

impl RS256Validator {
    /// Alias for [`RS256Validator::new`] with argument order swapped.
    pub fn default(aud: String, public_key: String) -> Self {
        RS256Validator::new(public_key, aud)
    }

    /// Create a new `RS256Validator`.
    ///
    /// # Arguments
    /// * `public_key` — PEM-encoded RSA public key.
    /// * `aud`        — Expected audience claim value.
    ///
    /// # Panics
    /// Panics if `public_key` is not a valid RSA PEM string.
    pub fn new(public_key: String, aud: String) -> Self {
        let dec_key = DecodingKey::from_rsa_pem(public_key.as_bytes()).expect("invalid public key");
        let mut vald = Validation::new(Algorithm::RS256);
        vald.set_audience(&[aud]);
        RS256Validator {
            dec_key,
            validation: vald,
        }
    }
}

impl<T> Sign<T> for RS256Signer
where
    T: Serialize,
{
    /// Sign `claims` with the RSA private key and return the compact JWT.
    fn sign(&self, claims: &T) -> Result<String> {
        Ok(encode(&self.header, claims, &self.enc_key)?)
    }
}

impl<T> Validate<T> for RS256Validator
where
    T: DeserializeOwned,
{
    /// Verify the token's RS256 signature and deserialise its claims.
    ///
    /// The `aud` claim must match the value supplied to [`RS256Validator::new`].
    fn validate(&self, token: &str) -> Result<T> {
        let data = decode::<T>(token, &self.dec_key, &self.validation)?;
        Ok(data.claims)
    }
}
