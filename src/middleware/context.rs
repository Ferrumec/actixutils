use crate::middleware::RequestIdStr;
use actix_web::HttpMessage;
use actix_web::error::ErrorInternalServerError;
use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use event_stream::{Event, EventMetaData, EventStream, Publishable};
use futures_util::future::LocalBoxFuture;
use std::future::{Ready, ready};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as Ctx, Poll};
use uuid::Uuid;

#[derive(Clone)]
pub struct Context {
    request_id: Uuid,
    user_id: Uuid,
    es: Arc<dyn EventStream>,
    producer: String,
}

impl Context {
    pub async fn publish<T: Publishable + Sync + Send>(&self, payload: T) {
        let emd = EventMetaData::new(self.producer.clone())
            .with_trace_id(self.request_id)
            .with_user_id(self.user_id);
        let event = Event::new(emd, payload);
        if let Err(e) = event.publish(self.es.clone()).await {
            tracing::error!(error = %e, request_id = %self.request_id, "Event publishing failed");
        };
    }
}

pub trait GetId {
    fn get_id(&self) -> Uuid;
}

pub struct ReadContext<T> {
    es: Arc<dyn EventStream>,
    name: String,
    _marker: PhantomData<T>,
}

impl<T> Clone for ReadContext<T> {
    fn clone(&self) -> Self {
        Self {
            es: self.es.clone(),
            name: self.name.clone(),
            _marker: PhantomData::<T>,
        }
    }
}

impl<T> ReadContext<T> {
    pub fn new(es: Arc<dyn EventStream>, name: String) -> Self {
        Self {
            es,
            name,
            _marker: PhantomData,
        }
    }
}

impl<S, B, T: GetId + 'static> Transform<S, ServiceRequest> for ReadContext<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ReadContextService<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ReadContextService {
            service: Rc::new(service),
            state: self.clone(),
        }))
    }
}

pub struct ReadContextService<S, T> {
    service: Rc<S>,
    state: ReadContext<T>,
}

impl<S, B, T: GetId + 'static> Service<ServiceRequest> for ReadContextService<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Ctx<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let es = self.state.es.clone();
        let producer = self.state.name.clone();
        let svc = self.service.clone();

        Box::pin(async move {
            let request_id = match req.extensions().get::<RequestIdStr>() {
                Some(r) => match Uuid::parse_str(&r.0) {
                    Ok(u) => u,
                    Err(_) => {
                        tracing::error!("Invalid RequestIdStr uuid format");
                        return Err(ErrorInternalServerError("Invalid request id"));
                    }
                },
                None => {
                    tracing::error!("RequestIdStr not found in request data");
                    return Err(ErrorInternalServerError("Missing request id"));
                }
            };
            let user_id = match req.extensions().get::<T>() {
                Some(u) => u,
                None => {
                    tracing::error!("Identity not found in request data");
                    return Err(ErrorInternalServerError("Missing identity"));
                }
            }
            .get_id();

            let ctx = Context {
                request_id,
                user_id,
                es,
                producer,
            };

            req.extensions_mut().insert(ctx);
            svc.call(req).await
        })
    }
}
