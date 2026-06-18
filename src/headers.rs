use crate::common::Identity;
use actix_web::{Error, FromRequest, HttpRequest, error::ErrorUnauthorized, http::header};
use anyhow::Result;
use futures_util::future::{Ready, ready};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
pub struct Access {
    pub token: String,
}

impl Access {
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
