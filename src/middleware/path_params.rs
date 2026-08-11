use actix_web::{
    Error, HttpMessage,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::rc::Rc;

use crate::extractors::Filters;

pub struct PathParams;

impl<S, B> Transform<S, ServiceRequest> for PathParams
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = PathParamsService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(PathParamsService {
            service: Rc::new(service),
        }))
    }
}

pub struct PathParamsService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for PathParamsService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            // Extract query parameters using your existing extractor.
            let mut filters = req.extract::<Filters>().await?;

            // Add matched path parameters.
            //
            // Path parameters take precedence over query parameters.
            for (key, value) in req.match_info().iter() {
                filters.insert(key.to_owned(), value.to_owned());
            }

            // Make Filters available through web::ReqData<Filters>.
            req.extensions_mut().insert(filters);

            service.call(req).await
        })
    }
}
