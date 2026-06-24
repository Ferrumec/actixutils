//! Public key exposure utilities.
//!
//! This module provides two ways to share a service's RSA public key with
//! external consumers:
//!
//! * [`configure`] — registers a `GET /.well-known/public-key.pem` route in an
//!   Actix-web [`ServiceConfig`](actix_web::web::ServiceConfig). The key is read
//!   once at startup from the `validate.key` environment variable.
//! * [`remote_public_key`] — fetches a PEM key from a remote URL specified by the
//!   `REMOTE_PUBLIC_KEY` environment variable. Useful for downstream services that
//!   need to retrieve the auth service's public key at runtime.

use std::env;

use actix_web::{
    HttpResponse, Responder, get,
    web::{self, ServiceConfig},
};

struct PublicKey {
    key: String,
}

/// Register a `GET /.well-known/public-key.pem` endpoint in the given service config.
///
/// The key is loaded from the `validate.key` environment variable at the time this
/// function is called.
///
/// # Panics
/// Panics if the `validate.key` environment variable is not set.
///
/// # Example
/// ```rust,no_run
/// use actix_web::{App, web};
/// use actixutils::pubkey;
///
/// App::new().configure(pubkey::configure);
/// ```
pub fn configure(cfg: &mut ServiceConfig) {
    let key = env::var("validate.key").expect("validate.key not set");
    let data = PublicKey { key };
    cfg.service(
        web::scope("")
            .app_data(web::Data::new(data))
            .service(public_key),
    );
}

#[get("/.well-known/public-key.pem")]
async fn public_key(data: web::Data<PublicKey>) -> impl Responder {
    HttpResponse::Ok().body(data.key.clone())
}
