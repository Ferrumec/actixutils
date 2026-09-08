use crate::extractors::Session;
use actix_web::{Error, FromRequest, HttpRequest, dev::Payload, error, HttpMessage};
use futures_util::future::LocalBoxFuture;
use tokio::sync::OwnedRwLockReadGuard;

pub struct ReadSession<T>(pub OwnedRwLockReadGuard<T>);

impl<T: 'static> FromRequest for ReadSession<T> {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // clone the Arc out now, while we still have the borrow
        let session = req.extensions().get::<Session<T>>().cloned();

        Box::pin(async move {
            match session {
                Some(session) => Ok(ReadSession(session.data.read_owned().await)),
                None => {
                    tracing::error!(
                        "No session in request. Did you forget to wrap SessionMiddleware?"
                    );
                    Err(error::ErrorInternalServerError(
                        "Session requested without SessionMiddleware",
                    ))
                }
            }
        })
    }
}

use std::ops::Deref;

impl<T> Deref for ReadSession<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}