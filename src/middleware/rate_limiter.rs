use std::{
    collections::VecDeque,
    future::{Ready, ready},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use actix_web::{
    Error, FromRequest, HttpResponse,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use dashmap::DashMap;
use futures_util::future::LocalBoxFuture;

pub trait GetId {
    type Id: Eq + Hash + Clone + Send + Sync + 'static;

    fn id(&self) -> Self::Id;
}

pub struct RateLimiter<T>
where
    T: GetId,
{
    max_requests: usize,
    window: Duration,
    store: Arc<DashMap<T::Id, VecDeque<Instant>>>,
    _marker: PhantomData<T>,
}

impl<T> Clone for RateLimiter<T>
where
    T: GetId,
{
    fn clone(&self) -> Self {
        Self {
            max_requests: self.max_requests,
            window: self.window,
            store: Arc::clone(&self.store),
            _marker: PhantomData,
        }
    }
}

impl<T> RateLimiter<T>
where
    T: GetId,
{
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            store: Arc::new(DashMap::new()),
            _marker: PhantomData,
        }
    }
}

impl<S, B, T> Transform<S, ServiceRequest> for RateLimiter<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    T: FromRequest + GetId + 'static,
    T::Future: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimiterMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimiterMiddleware {
            service: Arc::new(service),
            limiter: self.clone(),
            _marker: PhantomData,
        }))
    }
}

pub struct RateLimiterMiddleware<S, T>
where
    T: GetId,
{
    service: Arc<S>,
    limiter: RateLimiter<T>,
    _marker: PhantomData<T>,
}

impl<S, B, T> Service<ServiceRequest> for RateLimiterMiddleware<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    T: FromRequest + GetId + 'static,
    T::Future: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let limiter = self.limiter.clone();
        let service = Arc::clone(&self.service); // <-- clone the Rc, not S

        Box::pin(async move {
            let (http_req, payload) = req.parts_mut();

            if let Ok(identity) = T::from_request(http_req, payload).await {
                let id = identity.id();
                let now = Instant::now();

                let mut entry = limiter.store.entry(id).or_insert_with(VecDeque::new);

                while let Some(timestamp) = entry.front() {
                    if now.duration_since(*timestamp) > limiter.window {
                        entry.pop_front();
                    } else {
                        break;
                    }
                }

                if entry.len() >= limiter.max_requests {
                    // Drop the entry guard BEFORE consuming req
                    drop(entry);

                    let response = req.into_response(
                        HttpResponse::TooManyRequests()
                            .body("Rate limit exceeded")
                            .map_into_right_body(),
                    );
                    return Ok(response);
                }

                entry.push_back(now);
            }

            let res = service.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}
