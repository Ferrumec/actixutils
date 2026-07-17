//! Task-local pagination parameter extraction middleware.
//!
//! [`PaginationMiddleware`] parses the `?page=<u32>&limit=<u32>` query parameters
//! from each request and stores the result in a Tokio task-local variable (see
//! [`locals::pagination`](crate::locals::pagination)). Repository functions and
//! handlers can then read the current pagination state anywhere in the call stack
//! via [`Pagination::get`] without needing to thread the parameters through every
//! function signature.
//!
//! Missing parameters default to `page = 0` and `limit = 100`.
//!
//! # Example
//! ```rust,no_run
//! use actixutils::middleware::PaginationMiddleware;
//! use actixutils::locals::Pagination;
//! use actix_web::{web, App, HttpResponse};
//!
//! async fn list_items() -> HttpResponse {
//!     let p = Pagination::get(); // reads from task-local
//!     // SELECT ... LIMIT p.limit OFFSET (p.page * p.limit)
//!     HttpResponse::Ok().finish()
//! }
//!
//! App::new().service(
//!     web::scope("/items")
//!         .wrap(PaginationMiddleware)
//!         .route("", web::get().to(list_items))
//! );
//! ```

pub use crate::locals::Pagination;
use crate::locals::pagination::PAGINATION;
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

/// Deserialisation helper that accepts missing fields gracefully.
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

/// Middleware factory that parses `?page=&limit=` and stores the result in a task-local.
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

/// The inner service produced by [`PaginationMiddleware`].
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
