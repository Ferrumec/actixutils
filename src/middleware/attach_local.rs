//! Generic middleware for scoping a task-local (or similar) around a request.
//!
//! [`AttachLocal<T>`] extracts a `T` via [`FromRequest`] before the rest of the
//! request pipeline runs, then executes the downstream service inside
//! `T::scope(...)`. This is a reusable building block for middleware that needs to
//! make some extracted value available through a Tokio task-local for the lifetime
//! of the request, without threading it through every function signature — e.g. the
//! way [`PaginationMiddleware`](super::PaginationMiddleware) exposes
//! [`Pagination::get`](crate::locals::Pagination::get).

use std::future::{Ready, ready};
use std::marker::PhantomData;
use std::rc::Rc; // use Rc instead of Clone

use actix_web::FromRequest;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};
use actix_web::error::Error;
use futures_util::future::LocalBoxFuture;

/// Implemented by types that can wrap a future so it runs with `self` available as
/// task-local (or similarly scoped) state.
pub trait SetLocal: Sized {
    /// Run `fut` to completion with `self` scoped in for its duration.
    fn scope<F>(self, fut: F) -> impl Future<Output = F::Output>
    where
        F: Future;
}

/// Middleware factory that extracts a `T` per request and scopes it around the rest
/// of the request pipeline via [`SetLocal::scope`].
///
/// `T` must implement both [`FromRequest`] (to extract it) and [`SetLocal`] (to scope
/// it). If extraction fails, the error is converted to an [`actix_web::Error`] and the
/// request is rejected before reaching downstream services.
pub struct AttachLocal<T>(PhantomData<T>);

impl<T> Default for AttachLocal<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AttachLocal<T> {
    /// Create a new `AttachLocal<T>` middleware factory.
    pub fn new() -> Self {
        Self(PhantomData)
    }
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
            _marker: PhantomData,
        }))
    }
}

/// The inner service produced by [`AttachLocal`].
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
            let value = T::from_request(&req, &mut payload)
                .await
                .map_err(Into::into)?;

            // rebuild request with the same payload we just used
            let req = ServiceRequest::from_parts(req, payload);

            // scope it
            value.scope(service.call(req)).await
        })
    }
}
