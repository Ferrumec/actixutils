use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

#[derive(Clone)]
pub struct CachedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

pub enum IdempotencyState {
    InProgress,
    Completed(CachedResponse),
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Attempt to reserve the key.
    ///
    /// Returns:
    /// - Ok(true) if caller owns execution.
    /// - Ok(false) if key already exists.
    async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error>;

    async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error>;

    async fn complete(&self, key: &str, response: CachedResponse) -> Result<(), Self::Error>;

    async fn release(&self, key: &str) -> Result<(), Self::Error>;
}

use std::sync::Arc;

#[derive(Clone)]
pub struct Idempotency<S> {
    store: Arc<S>,
    ttl: Duration,
    header: &'static str,
}

impl<S> Idempotency<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            ttl: Duration::from_secs(60 * 60),
            header: "Idempotency-Key",
        }
    }

    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    pub fn header(mut self, header: &'static str) -> Self {
        self.header = header;
        self
    }
}

use actix_web::{
    Error,
    body::{BoxBody, MessageBody},
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::rc::Rc;

impl<S, B, Store> Transform<S, ServiceRequest> for Idempotency<Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    Store: IdempotencyStore,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();

    type Transform = IdempotencyMiddleware<S, Store>;

    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(IdempotencyMiddleware {
            service: Rc::new(service),
            store: self.store.clone(),
            ttl: self.ttl,
            header: self.header,
        }))
    }
}

pub struct IdempotencyMiddleware<S, Store> {
    service: Rc<S>,
    store: Arc<Store>,
    ttl: Duration,
    header: &'static str,
}

use actix_web::{HttpResponse, body, http::StatusCode};

impl<S, B, Store> Service<ServiceRequest> for IdempotencyMiddleware<S, Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    Store: IdempotencyStore,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;

    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let store = self.store.clone();
        let ttl = self.ttl;
        let header = self.header;

        Box::pin(async move {
            let Some(key) = req
                .headers()
                .get(header)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
            else {
                let res = service.call(req).await?;
                return Ok(res.map_into_boxed_body());
            };

            match store.acquire(&key, ttl).await {
                Ok(true) => {}
                Ok(false) => match store.get(&key).await {
                    Ok(Some(IdempotencyState::Completed(cached))) => {
                        let mut builder =
                            HttpResponse::build(StatusCode::from_u16(cached.status).unwrap());

                        for (name, value) in cached.headers {
                            builder.append_header((name, value));
                        }

                        let response = builder.body(cached.body);

                        return Ok(req.into_response(response));
                    }

                    Ok(Some(IdempotencyState::InProgress)) => {
                        return Ok(req.into_response(
                            HttpResponse::Conflict().body("request already in progress"),
                        ));
                    }

                    _ => {
                        return Ok(req.into_response(HttpResponse::InternalServerError().finish()));
                    }
                },

                Err(_) => {
                    return Ok(req.into_response(HttpResponse::InternalServerError().finish()));
                }
            }

            let response = service.call(req).await?;

            let (req, res) = response.into_parts();

            let status = res.status();

            let headers = res
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_owned())))
                .collect();

            let body = match body::to_bytes(res.into_body()).await {
                Ok(r) => r,
                Err(_e) => {
                    tracing::error!("failed to get bytes from response body");
                    return Ok(ServiceResponse::new(
                        req,
                        HttpResponse::InternalServerError().finish(),
                    ));
                }
            };

            let cached = CachedResponse {
                status: status.as_u16(),
                headers,
                body: body.clone(),
            };

            if store.complete(&key, cached).await.is_err() {
                let _ = store.release(&key).await;
            }

            Ok(ServiceResponse::new(
                req,
                HttpResponse::build(status).body(body).map_into_boxed_body(),
            ))
        })
    }
}
