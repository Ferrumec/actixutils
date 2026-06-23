use actix_web::HttpMessage;
use std::{rc::Rc, str::FromStr};

use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::header::{HeaderName, HeaderValue},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use uuid::Uuid;

pub struct RequestId;

impl<S, B> Transform<S, ServiceRequest> for RequestId
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestIdMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestIdMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct RequestIdMiddleware<S> {
    service: Rc<S>,
}
#[derive(Clone)]
pub struct RequestIdStr(pub String);

impl<S, B> Service<ServiceRequest> for RequestIdMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let req_id = Uuid::new_v4().to_string();
        req.extensions_mut().insert(RequestIdStr(req_id.clone()));
        tracing::Span::current().record("request_id", &req_id);

        let fut = self.service.call(req);
        Box::pin(async move {
            let mut res = fut.await?;
            res.headers_mut().append(
                HeaderName::from_str("X-Request-Id").unwrap(),
                HeaderValue::from_str(&req_id).unwrap(),
            );
            Ok(res)
        })
    }
}
