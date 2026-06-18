use crate::{Sign, Validate};
use anyhow::Result;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Clone)]
pub struct HS256Signer {
    secret: String,
    header: Header,
    validation: Validation,
}

impl HS256Signer {
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
    fn validate(&self, token: &str) -> Result<T> {
        let data = decode::<T>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &self.validation,
        )?;

        Ok(data.claims)
    }
}
