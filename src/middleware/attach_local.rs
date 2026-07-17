use std::future::{ready, Ready};
use std::marker::PhantomData;
use std::rc::Rc; // use Rc instead of Clone

use actix_web::dev::{
    forward_ready, Service, ServiceRequest, ServiceResponse, Transform,
};
use actix_web::error::Error;
use actix_web::FromRequest;
use futures_util::future::LocalBoxFuture;

pub trait SetLocal: Sized {
    fn scope<F>(self, fut: F) -> impl Future<Output = F::Output>
    where
        F: Future;
}

pub struct AttachLocal<T>(PhantomData<T>);

impl<T> AttachLocal<T> {
    pub fn new() -> Self { Self(PhantomData) }
}

impl<S, B, T> Transform<S, ServiceRequest> for AttachLocal<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    T: FromRequest + SetLocal + 'static,
    <T as FromRequest>::Error: Into<Error>, // FIX 3: allow ? to convert error
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = AttachLocalMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AttachLocalMiddleware { 
            service: Rc::new(service), // wrap in Rc
            _marker: PhantomData 
        }))
    }
}

pub struct AttachLocalMiddleware<S, T> {
    service: Rc<S>, // FIX 1: Rc instead of Clone
    _marker: PhantomData<T>,
}

impl<S, B, T> Service<ServiceRequest> for AttachLocalMiddleware<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    T: FromRequest + SetLocal + 'static,
    <T as FromRequest>::Error: Into<Error>, // FIX 3
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone(); // Rc::clone is cheap
        Box::pin(async move {
            // Unpack request
            let (req, mut payload) = req.into_parts();
            
            // extract. from_request consumes &mut Payload
            let value = T::from_request(&req, &mut payload).await.map_err(Into::into)?;
            
            // rebuild request with the same payload we just used
            let req = ServiceRequest::from_parts(req, payload);
            
            // scope it
            value.scope(service.call(req)).await
        })
    }
}