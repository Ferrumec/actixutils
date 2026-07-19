use std::{
    future::{ready, Ready},
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
};

use actix_web::{
    body::MessageBody,
    dev::{Payload, Service, ServiceRequest, ServiceResponse, Transform},
    error,
    Error, FromRequest, HttpMessage, HttpRequest,
};
use async_trait::async_trait;
use futures_util::future::LocalBoxFuture;
use tokio::sync::RwLock;

pub type SharedSession<T> = Arc<RwLock<T>>;

/// ===========================
/// Session Store
/// ===========================

#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    type Session: Send + Sync + Clone + Default + 'static;

    async fn load(
        &self,
        session_id: &str,
    ) -> Result<Option<Self::Session>, Error>;

    async fn save(
        &self,
        session_id: &str,
        session: &Self::Session,
    ) -> Result<(), Error>;

    async fn delete(
        &self,
        session_id: &str,
    ) -> Result<(), Error>;
}

/// ===========================
/// Extractor
/// ===========================

pub struct Session<T>(pub SharedSession<T>);

impl<T: Send + Sync + 'static> FromRequest for Session<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(
        req: &HttpRequest,
        _: &mut Payload,
    ) -> Self::Future {
        match req.extensions().get::<SharedSession<T>>() {
            Some(session) => ready(Ok(Session(session.clone()))),
            None => ready(Err(error::ErrorUnauthorized("No session"))),
        }
    }
}

/// ===========================
/// Middleware
/// ===========================

pub struct SessionMiddleware<S> {
    store: Arc<S>,
    cookie_name: String,
}

impl<S> SessionMiddleware<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
        }
    }

    pub fn cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_name = name.into();
        self
    }
}

impl<S, B, Store> Transform<S, ServiceRequest> for SessionMiddleware<Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    Store: SessionStore + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = SessionMiddlewareService<S, Store>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SessionMiddlewareService {
            service:service.into(),
            store: self.store.clone(),
            cookie_name: self.cookie_name.clone(),
        }))
    }
}

pub struct SessionMiddlewareService<S, Store> {
    service: Rc<S>,
    store: Arc<Store>,
    cookie_name: String,
}

impl<S, B, Store> Service<ServiceRequest> for SessionMiddlewareService<S, Store>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    Store: SessionStore + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(
        &self,
        req: ServiceRequest,
    ) -> Self::Future {
        let store = self.store.clone();
        let cookie_name = self.cookie_name.clone();
let service = self.service.clone();
        Box::pin(async move {
            let session_id = req
                .cookie(&cookie_name)
                .map(|c| c.value().to_owned());

            let session = if let Some(ref id) = session_id {
                store
                    .load(id)
                    .await?
                    .map(|s| Arc::new(RwLock::new(s)))
            } else {
                None
            };

            if let Some(ref s) = session {
                req.extensions_mut().insert(s.clone());
            }

            let res = service.call(req).await?;

            if let (Some(id), Some(session)) = (session_id, session) {
                let session = session.read().await;
                store.save(&id, &*session).await?;
            }

            Ok(res)
        })
    }
}