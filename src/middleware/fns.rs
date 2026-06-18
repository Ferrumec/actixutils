use crate::auth::Auth;
use crate::{Authority, Identity};
use actix_web::{
    Error, HttpResponse,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
};
use std::pin::Pin;

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
