use std::env;

use actix_web::{
    HttpResponse, Responder, get,
    web::{self, ServiceConfig},
};

struct PublicKey {
    key: String,
}

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
