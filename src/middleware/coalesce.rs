use std::collections::HashMap;
use std::future::{Ready, ready};
use std::hash::Hash;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::ErrorInternalServerError;
use actix_web::http::StatusCode;
use actix_web::http::header::HeaderMap;
use actix_web::{Error, HttpResponse};
use bytes::Bytes;
use futures_util::future::LocalBoxFuture;

/// Shared response data replicated to all coalesced followers.
#[derive(Clone, Debug)]
pub struct CoalescedResponse {
    /// The HTTP status code returned by the leader's execution.
    pub status: StatusCode,
    /// The response headers produced by the leader's execution.
    pub headers: HeaderMap,
    /// The fully-buffered response body shared with all followers.
    pub body: Bytes,
}

/// Represents an in-flight execution for a given coalescing key.
struct Flight {
    result: Mutex<Option<Result<CoalescedResponse, String>>>,
    notify: tokio::sync::Notify,
}

/// Shared state holding active in-flight flights.
struct SharedState<K> {
    map: Mutex<HashMap<K, Arc<Flight>>>,
}

/// Request-coalescing (singleflight) middleware for Actix Web.
pub struct Singleflight<K, KeyFn> {
    key_fn: KeyFn,
    state: Arc<SharedState<K>>,
}

impl<K, KeyFn> Singleflight<K, KeyFn>
where
    K: Eq + Hash + Clone + 'static,
    KeyFn: Fn(&ServiceRequest) -> K + Clone + 'static,
{
    /// Construct a `Singleflight` transform that groups concurrent requests
    /// by the key returned from `key_fn`.
    ///
    /// Requests that map to the same key while an execution is already
    /// in flight wait for that execution to finish and receive a clone of
    /// its response, instead of hitting the wrapped service themselves.
    pub fn new(key_fn: KeyFn) -> Self {
        Self {
            key_fn,
            state: Arc::new(SharedState {
                map: Mutex::new(HashMap::new()),
            }),
        }
    }
}

impl<S, B, K, KeyFn> Transform<S, ServiceRequest> for Singleflight<K, KeyFn>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    B::Error: Into<Error>,
    K: Eq + Hash + Clone + 'static,
    KeyFn: Fn(&ServiceRequest) -> K + Clone + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = SingleflightMiddleware<S, K, KeyFn>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SingleflightMiddleware {
            service: Rc::new(service),
            key_fn: self.key_fn.clone(),
            state: self.state.clone(),
        }))
    }
}

/// Per-worker service produced by [`Singleflight`]; does the actual
/// request-coalescing work.
pub struct SingleflightMiddleware<S, K, KeyFn> {
    service: Rc<S>,
    key_fn: KeyFn,
    state: Arc<SharedState<K>>,
}

impl<S, B, K, KeyFn> Service<ServiceRequest> for SingleflightMiddleware<S, K, KeyFn>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
    B::Error: Into<Error>,
    K: Eq + Hash + Clone + 'static,
    KeyFn: Fn(&ServiceRequest) -> K + Clone + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let key = (self.key_fn)(&req);
        let state = self.state.clone();
        let service = self.service.clone();

        Box::pin(async move {
            let mut is_leader = false;
            let flight = {
                let mut map = state.map.lock().unwrap();
                match map.get(&key) {
                    Some(existing_flight) => existing_flight.clone(),
                    None => {
                        is_leader = true;
                        let new_flight = Arc::new(Flight {
                            result: Mutex::new(None),
                            notify: tokio::sync::Notify::new(),
                        });
                        map.insert(key.clone(), new_flight.clone());
                        new_flight
                    }
                }
            };

            if is_leader {
                struct LeaderGuard<K: Eq + Hash + Clone> {
                    state: Arc<SharedState<K>>,
                    key: K,
                    flight: Arc<Flight>,
                    completed: bool,
                }

                impl<K: Eq + Hash + Clone> Drop for LeaderGuard<K> {
                    fn drop(&mut self) {
                        if !self.completed {
                            let mut res = self.flight.result.lock().unwrap();
                            if res.is_none() {
                                *res =
                                    Some(Err("Leader execution cancelled or panicked".to_string()));
                            }
                            drop(res);
                            self.flight.notify.notify_waiters();

                            let mut map = self.state.map.lock().unwrap();
                            map.remove(&self.key);
                        }
                    }
                }

                let mut guard = LeaderGuard {
                    state: state.clone(),
                    key: key.clone(),
                    flight: flight.clone(),
                    completed: false,
                };

                let execution_result = service.call(req).await;

                // Deconstruct ServiceResponse to recover the ServiceRequest on success.
                let (req_opt, coalesced_result) = match execution_result {
                    Ok(sr) => {
                        let (req, res) = sr.into_parts();
                        let status = res.status();
                        let headers = res.headers().clone();
                        match actix_web::body::to_bytes(res.into_body()).await {
                            Ok(body) => {
                                let coalesced = CoalescedResponse {
                                    status,
                                    headers,
                                    body,
                                };
                                (Some(req), Ok(coalesced))
                            }
                            Err(err) => (Some(req), Err(err.into())),
                        }
                    }
                    Err(err) => (None, Err(err)),
                };

                // Store a cloneable result for followers (Error -> String).
                let stored_result = match &coalesced_result {
                    Ok(coalesced) => Ok(coalesced.clone()),
                    Err(err) => Err(err.to_string()),
                };

                {
                    let mut res_slot = flight.result.lock().unwrap();
                    *res_slot = Some(stored_result);
                }
                flight.notify.notify_waiters();

                {
                    let mut map = state.map.lock().unwrap();
                    map.remove(&key);
                }

                guard.completed = true;

                // Build the leader's response.
                // Build the leader's response.
                match coalesced_result {
                    Ok(coalesced) => {
                        let req = req_opt.expect("leader Ok path has req");
                        let mut builder = HttpResponse::build(coalesced.status);
                        for (name, value) in coalesced.headers.iter() {
                            builder.insert_header((name.clone(), value.clone()));
                        }
                        let http_res = builder.body(BoxBody::new(coalesced.body));
                        Ok(ServiceResponse::new(req, http_res))
                    }
                    Err(err) => Err(err),
                }
            } else {
                loop {
                    {
                        let res_slot = flight.result.lock().unwrap();
                        if let Some(ref res) = *res_slot {
                            match res {
                                Ok(coalesced) => {
                                    let mut builder = HttpResponse::build(coalesced.status);
                                    for (name, value) in coalesced.headers.iter() {
                                        builder.insert_header((name.clone(), value.clone()));
                                    }
                                    let http_res =
                                        builder.body(BoxBody::new(coalesced.body.clone()));
                                    return Ok(req.into_response(http_res));
                                }
                                Err(err) => {
                                    return Err(ErrorInternalServerError(err.clone()));
                                }
                            }
                        }
                    }
                    flight.notify.notified().await;
                }
            }
        })
    }
}
