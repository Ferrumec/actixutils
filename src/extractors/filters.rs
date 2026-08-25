//! Ad-hoc query-string filter extractor.
//!
//! [`Filters`] collects arbitrary `?field=value` query-string pairs into a
//! `HashMap`, for handlers that accept a flexible set of filter parameters
//! rather than a fixed, strongly-typed query struct.

use actix_web::{
    Error, FromRequest, HttpMessage, HttpRequest, dev::Payload, error::ErrorBadRequest, web,
};
use futures_util::future::LocalBoxFuture;
use serde::Deserialize;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
/// Extractor that collects query-string parameters into a `HashMap`.
///
/// If [`middleware::PathParams`](crate::middleware::PathParams) (or any
/// other middleware) has already inserted a `Filters` value into the
/// request extensions, that value is reused as-is; otherwise this parses
/// the raw query string directly. Derefs to `HashMap<String, String>` for
/// convenient lookups.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Filters(pub HashMap<String, String>);

impl Deref for Filters {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<HashMap<String, String>> for Filters {
    fn from(value: HashMap<String, String>) -> Self {
        Filters(value)
    }
}

impl From<HashMap<&str, String>> for Filters {
    fn from(value: HashMap<&str, String>) -> Self {
        let value: HashMap<String, String> = value
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Filters(value)
    }
}

impl DerefMut for Filters {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromRequest for Filters {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        // Prefer the Filters already constructed by middleware.
        if let Some(filters) = req.extensions().get::<Filters>() {
            return Box::pin(std::future::ready(Ok(filters.clone())));
        }

        // Otherwise parse the query string directly.
        let fut = web::Query::<HashMap<String, String>>::from_request(req, payload);

        Box::pin(async move {
            let query = fut.await.map_err(ErrorBadRequest)?;
            Ok(Filters(query.into_inner()))
        })
    }
}
