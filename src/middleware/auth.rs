use crate::{Identity, Validate};
use actix_web::{
    Error, HttpMessage,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    http::header,
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::sync::Arc;

pub struct Auth {
    pub validator: Arc<dyn Validate<Identity>>,
}

impl<S, B> Transform<S, ServiceRequest> for Auth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddleware {
            service: Arc::new(service),
            signer: self.validator.clone(),
        }))
    }
}

pub struct AuthMiddleware<S> {
    service: Arc<S>,
    signer: Arc<dyn Validate<Identity>>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let svc = Arc::clone(&self.service);
        let signer = Arc::clone(&self.signer);

        Box::pin(async move {
            let token: Option<String> = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .map(|s| s.replace("Bearer ", ""))
                .or_else(|| req.cookie("access_token").map(|c| c.value().to_string()));

            let token = match token {
                Some(t) => t,
                None => return Err(ErrorUnauthorized("Missing authorization header")),
            };

            // Validate token and get claims
            let claims = match signer.validate(&token) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("JWT validation error: {:?}", e);
                    return Err(ErrorUnauthorized("Invalid or expired token"));
                }
            };

            // store claims in request extensions
            req.extensions_mut().insert(claims);

            // forward request to next service
            svc.call(req).await
        })
    }
}
