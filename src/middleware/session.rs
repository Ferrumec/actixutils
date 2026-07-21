use actix_web::{
    Error, FromRequest, HttpMessage, HttpRequest,
    body::MessageBody,
    dev::{Payload, Service, ServiceRequest, ServiceResponse, Transform},
    error,
};
use async_trait::async_trait;
use futures_util::future::LocalBoxFuture;
use std::{
    future::{Ready, ready},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use tokio::sync::RwLock;
use uuid::Uuid;

type SharedSession<T> = Arc<RwLock<T>>;

#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    type Session: Send + Sync + Clone + Default + 'static;
    async fn load(&self, session_id: &Uuid) -> Result<Option<Self::Session>, Error>;
    async fn save(&self, session_id: &Uuid, session: &Self::Session) -> Result<(), Error>;
    async fn delete(&self, session_id: &Uuid) -> Result<(), Error>;
}

pub struct Session<T> {
    data: SharedSession<T>,
    dirty: Arc<AtomicBool>,
}

impl<T> Clone for Session<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),   // Arc clone
            dirty: self.dirty.clone(), // Arc clone
        }
    }
}

impl<T> Session<T> {
    pub(crate) fn new(session: T) -> Self {
        Self {
            data: Arc::new(RwLock::new(session)),
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, T> {
        self.data.read().await
    }
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        // <-- &self not &mut self
        self.dirty.store(true, Ordering::Relaxed); // mark dirty on any write
        self.data.write().await
    }
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
    pub(crate) fn set_clean(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }
}

impl<T: Send + Sync + 'static> FromRequest for Session<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;
    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        match req.extensions().get::<Arc<Session<T>>>() {
            Some(session) => ready(Ok((**session).clone())), // clone the Arc<Session<T>>
            None => {
                tracing::error!("No session in request. Did you forget to wrap SessionMiddleware?");
                ready(Err(error::ErrorInternalServerError(
                    "Session requested without SessionMiddleware",
                )))
            }
        }
    }
}

pub struct SessionMiddleware<S> {
    store: Arc<S>,
    cookie_name: String,
    required: bool,
}

impl<S> SessionMiddleware<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
            required: false,
        }
    }

    pub fn required(store: Arc<S>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
            required: true,
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
            service: service.into(),
            store: self.store.clone(),
            cookie_name: self.cookie_name.clone(),
            required: self.required,
        }))
    }
}

pub struct SessionMiddlewareService<S, Store> {
    service: Rc<S>,
    store: Arc<Store>,
    cookie_name: String,
    required: bool,
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

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let store = self.store.clone();
        let cookie_name = self.cookie_name.clone();
        let service = self.service.clone();
        let required = self.required;
        Box::pin(async move {
            let (session_id, session, new_session) = match req.cookie(&cookie_name) {
                Some(cookie) => {
                    let mut new_session = false;
                    let id = match Uuid::parse_str(cookie.value()) {
                        Ok(id) => id,
                        Err(_) => {
                            if required {
                                return Err(error::ErrorUnauthorized("no session"));
                            }
                            new_session = true;
                            Uuid::new_v4()
                        }
                    };
                    let session_data = store.load(&id).await?.unwrap_or_default();
                    (id, Session::new(session_data), new_session)
                }
                None => {
                    if required {
                        return Err(error::ErrorUnauthorized("no session"));
                    }
                    let id = Uuid::new_v4();
                    let session = Session::new(Store::Session::default());
                    session.dirty.store(true, Ordering::Relaxed); // new session must be saved once
                    (id, session, true)
                }
            };

            let session = Arc::new(session);
            req.extensions_mut().insert(session.clone());

            let mut res = service.call(req).await?;

            // Only save if dirty
            if session.is_dirty() {
                let session_data = session.read().await;
                store.save(&session_id, &*session_data).await?;
                session.set_clean(); // reset flag
            }

            if new_session {
                use actix_web::cookie::Cookie;
                let cookie = Cookie::build(cookie_name, session_id.to_string())
                    .path("/")
                    .http_only(true)
                    .finish();
                res.response_mut()
                    .add_cookie(&cookie)
                    .map_err(error::ErrorInternalServerError)?;
            }
            Ok(res)
        })
    }
}
