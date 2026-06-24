use actix_web::web::Query;
use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::LocalBoxFuture;
use serde::Deserialize;
use std::{
    future::{Ready, ready},
    rc::Rc,
};
use tokio::task_local;

task_local! {
    static PAGINATION: Pagination;
}

#[derive(Clone, Copy)]
pub struct Pagination {
    pub page: u32,
    pub limit: u32,
}

impl Default for Pagination {
    fn default() -> Pagination {
        Pagination {
            page: 0,
            limit: 100,
        }
    }
}

impl Pagination {
    pub fn get() -> Pagination {
        PAGINATION.try_with(|p| *p).unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct SafePagination {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

impl Default for SafePagination {
    fn default() -> Self {
        Self {
            page: Some(0),
            limit: Some(100),
        }
    }
}

impl SafePagination {
    fn get(&self) -> Pagination {
        Pagination {
            page: self.page.unwrap_or(0),
            limit: self.limit.unwrap_or(100),
        }
    }
}

pub struct PaginationMiddleware;

impl<S, B> Transform<S, ServiceRequest> for PaginationMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = PaginationMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(PaginationMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub struct PaginationMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for PaginationMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();

        Box::pin(async move {
            let pagination = req
                .extract::<Query<SafePagination>>()
                .await
                .unwrap_or(Query(SafePagination::default()))
                .get();

            PAGINATION
                .scope(pagination, async move { service.call(req).await })
                .await
        })
    }
}
