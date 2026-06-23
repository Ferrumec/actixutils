use crate::auth::Auth;
use crate::{Authority, Identity};
use actix_web::web::Query;
use actix_web::{
    Error, HttpResponse,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
};
use serde::Deserialize;
use std::pin::Pin;
use tokio::task_local;

pub async fn identity(
    mut req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let _user = req.extract::<Auth<Identity>>().await?;
    Ok(next.call(req).await?)
}

pub fn authority(
    perm_id: u32,
) -> impl Fn(
    ServiceRequest,
    Next<BoxBody>,
) -> Pin<Box<dyn Future<Output = Result<ServiceResponse<BoxBody>, Error>>>> {
    move |mut req: ServiceRequest, next: Next<BoxBody>| {
        Box::pin(async move {
            let authority = req.extract::<Auth<Authority>>().await?.0;
            let p_value = 1u128 << perm_id; // 2u128.pow(perm_id) works too

            if authority.role & p_value != p_value {
                // 403 Forbidden is more common than 405 for permissions
                Ok(req.into_response(HttpResponse::Forbidden().finish().map_into_boxed_body()))
            } else {
                Ok(next.call(req).await?)
            }
        })
    }
}

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

impl SafePagination {
    fn get(&self) -> Pagination {
        Pagination {
            page: self.page.unwrap_or(0),
            limit: self.limit.unwrap_or(100),
        }
    }
}

pub async fn attach_pagination(
    mut req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let pagination = req.extract::<Query<SafePagination>>().await?.get();
    Ok(PAGINATION
        .scope(pagination, async { next.call(req).await })
        .await?)
}
