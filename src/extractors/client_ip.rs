use crate::locals::rate_limiter::GetId;
use actix_web::{Error, FromRequest, HttpMessage, HttpRequest};
use futures_util::future::{Ready, ready};
use std::net::IpAddr;

#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl ClientIp {
    pub fn ip(&self) -> IpAddr {
        self.0
    }
}

impl GetId for ClientIp {
    type Id = IpAddr;
    fn id(&self) -> Self::Id {
        self.0
    }
}

impl FromRequest for ClientIp {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        ready(req.extensions().get::<ClientIp>().copied().ok_or_else(|| {
            actix_web::error::ErrorInternalServerError("client IP middleware is not configured")
        }))
    }
}
