//! `Access` — a low-level request extractor that yields the raw Bearer token.
//!
//! Unlike [`Auth<T>`](crate::extractors::Auth), `Access` does **not** validate the token
//! automatically. It simply extracts the token string from the
//! `Authorization: Bearer <token>` header or the `access_token` cookie, leaving
//! validation to the caller via [`Access::validate_hmac`] or [`Access::validate_rsa`].
//!
//! This is useful when a handler needs to inspect the claims before deciding which
//! algorithm or audience to use for validation.

use crate::locals::Identity;
use actix_web::{Error, FromRequest, HttpRequest, error::ErrorUnauthorized, http::header};
use anyhow::Result;
use futures_util::future::{Ready, ready};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

/// A raw Bearer token extracted from the request.
///
/// The token is sourced (in order of precedence) from:
/// 1. The `Authorization: Bearer <token>` header.
/// 2. The `access_token` cookie.
pub struct Access {
    /// The raw JWT string (no `Bearer ` prefix).
    pub token: String,
}

impl Access {
    /// Validate this token using HMAC-SHA-256 (`HS256`).
    ///
    /// # Arguments
    /// * `secret` — The shared HMAC secret used to verify the signature.
    /// * `aud`    — The expected `aud` claim value.
    ///
    /// # Errors
    /// Returns `Err` if the signature is invalid, the token is expired, or
    /// the audience claim does not match.
    pub fn validate_hmac(&self, secret: &str, aud: String) -> Result<Identity> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[aud]);

        let data = decode::<Identity>(
            &self.token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )?;
        Ok(data.claims)
    }

    /// Validate this token using RSA-SHA-256 (`RS256`).
    ///
    /// # Arguments
    /// * `pubkey` — PEM-encoded RSA public key.
    /// * `aud`    — The expected `aud` claim value.
    ///
    /// # Errors
    /// Returns `Err` if the key is not valid PEM, the signature is invalid,
    /// the token is expired, or the audience claim does not match.
    ///
    /// # Panics
    /// Panics if `pubkey` is not a valid RSA PEM string.
    pub fn validate_rsa(&self, pubkey: &str, aud: String) -> Result<Identity> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[aud]);
        let dec_key = DecodingKey::from_rsa_pem(pubkey.as_bytes()).expect("invalid public key");

        let data = decode::<Identity>(&self.token, &dec_key, &validation)?;
        Ok(data.claims)
    }
}

impl FromRequest for Access {
    type Error = Error;
    type Future = Ready<Result<Access, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let token: Option<String> = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.replace("Bearer ", ""))
            .or_else(|| req.cookie("access_token").map(|c| c.value().to_string()));

        let token = match token {
            Some(t) => t,
            None => return ready(Err(ErrorUnauthorized("Missing authorization header"))),
        };

        ready(Ok(Access { token }))
    }

    fn extract(req: &HttpRequest) -> Self::Future {
        Self::from_request(req, &mut actix_web::dev::Payload::None)
    }
}
