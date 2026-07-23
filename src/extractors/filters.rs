//! [`Filters`] — collects arbitrary query-string parameters for manual filtering.

use actix_web::{Error, FromRequest, HttpRequest, dev::Payload, error::ErrorBadRequest, web};
use futures_util::future::LocalBoxFuture;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

/// An extractor that collects every query-string parameter into a `HashMap<String, String>`.
///
/// Unlike a typed `web::Query<T>`, this accepts any set of key/value pairs without a
/// fixed schema, which is useful for handlers that apply ad-hoc filtering (e.g.
/// `?status=active&owner=42`) against a repository or query builder. Derefs to the
/// inner `HashMap` for convenient access.
#[derive(Debug, Clone, Default)]
pub struct Filters(pub HashMap<String, String>);

impl Deref for Filters {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
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
        let fut = web::Query::<HashMap<String, String>>::from_request(req, payload);

        Box::pin(async move {
            let query = fut.await.map_err(ErrorBadRequest)?;
            Ok(Filters(query.into_inner()))
        })
    }
}
