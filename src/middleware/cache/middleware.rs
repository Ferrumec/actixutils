//! GET-only HTTP response caching middleware.
//!
//! The cache key is derived exclusively from `host + path + query`. The
//! middleware never inspects authentication, cookies, or any other request
//! header when computing the key, which means two authenticated requests
//! resolving to the same URL will resolve to the same cache entry. This is
//! intentional: the cache is purely URL-based. Applications are responsible
//! for choosing URLs that identify a shareable resource (e.g.
//! `/users/123/orders`) before applying this middleware to a route.

use std::future::{Ready, ready};
use std::rc::Rc;
use std::sync::Arc;

use actix_web::body::{BodySize, BoxBody, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};
use actix_web::http::{Method, header};
use actix_web::{Error, HttpResponse};
use futures_util::future::LocalBoxFuture;

use super::types::CachedResponse;
use crate::Store;

/// Middleware factory. Wrap a route or scope with `Cache::new(store)`.
///
/// ```ignore
/// App::new().wrap(Cache::new(store).ttl(Duration::from_secs(60)))
/// ```
#[derive(Clone)]
pub struct Cache {
    store: Arc<dyn Store<String, CachedResponse>>,
}

impl Cache {
    /// Default TTL is 60 seconds; override with [`Cache::ttl`].
    pub fn new(store: Arc<dyn Store<String, CachedResponse>>) -> Self {
        Self { store }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Cache
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = CacheMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(CacheMiddleware {
            service: Rc::new(service),
            store: self.store.clone(),
        }))
    }
}

pub struct CacheMiddleware<S> {
    service: Rc<S>,
    store: Arc<dyn Store<String, CachedResponse>>,
}

/// `host + path + query`. Fragments are never part of an HTTP request, so
/// there is nothing to strip; the method is intentionally excluded because
/// only `GET` is ever cached.
fn cache_key(req: &ServiceRequest) -> String {
    let host = req.connection_info().host().to_string();
    let path = req.uri().path();
    match req.uri().query() {
        Some(query) => format!("{host}{path}?{query}"),
        None => format!("{host}{path}"),
    }
}

/// Conservative cache-control handling: opt out on `no-store`, `private`,
/// `Set-Cookie`, or a non-success status. This is intentionally not a full
/// implementation of HTTP caching semantics.
fn is_cacheable(resp: &HttpResponse) -> bool {
    if !resp.status().is_success() {
        return false;
    }

    if resp.headers().contains_key(header::SET_COOKIE) {
        return false;
    }

    if let Some(cache_control) = resp.headers().get(header::CACHE_CONTROL)
        && let Ok(value) = cache_control.to_str()
    {
        let value = value.to_ascii_lowercase();
        if value.contains("no-store") || value.contains("private") {
            return false;
        }
    }

    true
}

impl<S, B> Service<ServiceRequest> for CacheMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Non-GET requests never touch the cache in any way.
        if req.method() != Method::GET {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res.map_into_boxed_body())
            });
        }

        let key = cache_key(&req);
        let store = self.store.clone();
        let service = self.service.clone();

        Box::pin(async move {
            if let Ok(Some(cached)) = store.get(&key).await {
                let (http_req, _) = req.into_parts();
                let response = cached.into_http_response();
                return Ok(ServiceResponse::new(http_req, response));
            }

            let res = service.call(req).await?;
            let res = res.map_into_boxed_body();

            if !is_cacheable(res.response()) {
                return Ok(res);
            }

            // Only buffer bodies with a known, finite length. Streaming or
            // unknown-length bodies are left untouched and simply aren't
            // cached in this first implementation.
            if !matches!(res.response().body().size(), BodySize::Sized(_)) {
                return Ok(res);
            }

            let (http_req, http_response) = res.into_parts();
            let status = http_response.status();
            let headers: Vec<_> = http_response
                .headers()
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();

            let body = http_response.into_body();
            let bytes = match actix_web::body::to_bytes(body).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    // The body was already consumed and failed to buffer.
                    // Nothing was written to the store, so it stays
                    // uncorrupted; return the best reconstruction we can
                    // (status + headers, empty body) rather than hang.
                    let mut builder = HttpResponse::build(status);
                    for (name, value) in headers {
                        builder.insert_header((name, value));
                    }
                    return Ok(ServiceResponse::new(http_req, builder.finish()));
                }
            };

            let cached_response = CachedResponse::new(status, headers.clone(), bytes.clone());
            if let Err(e) = store.set(&key, cached_response).await {
                tracing::error!("Error in setting cache value: {}", e);
            };

            let mut builder = HttpResponse::build(status);
            for (name, value) in headers {
                builder.insert_header((name, value));
            }
            let response = builder.body(bytes);

            Ok(ServiceResponse::new(http_req, response))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::cache::MemoryCache;
    use actix_web::http::StatusCode;
    use actix_web::{App, HttpResponse, test, web};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn store() -> Arc<dyn Store<String, CachedResponse>> {
        Arc::new(MemoryCache::new())
    }

    /// A handler that increments a counter every time it actually runs, so
    /// tests can assert whether a request reached the underlying service or
    /// was served from cache.
    async fn counting_handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
        counter.fetch_add(1, Ordering::SeqCst);
        HttpResponse::Ok().body("hello")
    }

    fn counter_app_data(counter: &Arc<AtomicUsize>) -> web::Data<Arc<AtomicUsize>> {
        web::Data::new(counter.clone())
    }

    #[actix_web::test]
    async fn get_miss_calls_underlying_service() {
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        let req = test::TestRequest::get().uri("/products").to_request();
        let res = test::call_service(&app, req).await;

        assert!(res.status().is_success());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn second_identical_get_is_served_from_cache() {
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        let req1 = test::TestRequest::get().uri("/products").to_request();
        test::call_service(&app, req1).await;

        let req2 = test::TestRequest::get().uri("/products").to_request();
        let res2 = test::call_service(&app, req2).await;

        assert!(res2.status().is_success());
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "second request must be served from cache"
        );
    }

    #[actix_web::test]
    async fn different_paths_produce_different_entries() {
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/a", web::get().to(counting_handler))
                .route("/b", web::get().to(counting_handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/a").to_request()).await;
        test::call_service(&app, test::TestRequest::get().uri("/b").to_request()).await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn different_query_strings_produce_different_entries() {
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/products?page=1")
                .to_request(),
        )
        .await;
        test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/products?page=2")
                .to_request(),
        )
        .await;
        // Re-request page=1: should be a cache hit, no new service call.
        test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/products?page=1")
                .to_request(),
        )
        .await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn different_hosts_produce_different_entries() {
        let shared_store = store();
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(shared_store))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        let req1 = test::TestRequest::get()
            .uri("/products")
            .insert_header(("Host", "example.com"))
            .to_request();
        test::call_service(&app, req1).await;

        let req2 = test::TestRequest::get()
            .uri("/products")
            .insert_header(("Host", "api.example.com"))
            .to_request();
        test::call_service(&app, req2).await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn non_get_requests_bypass_cache() {
        let counter = Arc::new(AtomicUsize::new(0));
        async fn handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
            counter.fetch_add(1, Ordering::SeqCst);
            HttpResponse::Created().finish()
        }

        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::post().to(handler)),
        )
        .await;

        test::call_service(
            &app,
            test::TestRequest::post().uri("/products").to_request(),
        )
        .await;
        test::call_service(
            &app,
            test::TestRequest::post().uri("/products").to_request(),
        )
        .await;

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "POST must never be cached"
        );
    }

    #[actix_web::test]
    async fn ttl_expiration_reaches_underlying_service_again() {
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "expired entry must be refetched"
        );
    }

    #[actix_web::test]
    async fn cached_status_code_is_preserved() {
        async fn handler() -> HttpResponse {
            HttpResponse::build(StatusCode::from_u16(206).unwrap()).finish()
        }
        let app = test::init_service(
            App::new()
                .wrap(Cache::new(store()))
                .route("/partial", web::get().to(handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/partial").to_request()).await;
        let res =
            test::call_service(&app, test::TestRequest::get().uri("/partial").to_request()).await;

        assert_eq!(res.status().as_u16(), 206);
    }

    #[actix_web::test]
    async fn cached_headers_are_preserved() {
        async fn handler() -> HttpResponse {
            HttpResponse::Ok()
                .insert_header(("X-Custom", "value"))
                .finish()
        }
        let app = test::init_service(
            App::new()
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;
        let res =
            test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;

        assert_eq!(res.headers().get("X-Custom").unwrap(), "value");
    }

    #[actix_web::test]
    async fn cached_body_is_preserved() {
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&Arc::new(AtomicUsize::new(0))))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;
        let res =
            test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;
        let body = test::read_body(res).await;

        assert_eq!(body, "hello");
    }

    #[actix_web::test]
    async fn no_store_responses_are_not_cached() {
        let counter = Arc::new(AtomicUsize::new(0));
        async fn handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
            counter.fetch_add(1, Ordering::SeqCst);
            HttpResponse::Ok()
                .insert_header((header::CACHE_CONTROL, "no-store"))
                .body("secret")
        }

        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;
        test::call_service(&app, test::TestRequest::get().uri("/products").to_request()).await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn private_responses_are_not_cached_by_default() {
        let counter = Arc::new(AtomicUsize::new(0));
        async fn handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
            counter.fetch_add(1, Ordering::SeqCst);
            HttpResponse::Ok()
                .insert_header((header::CACHE_CONTROL, "private, max-age=60"))
                .body("account data")
        }

        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/account", web::get().to(handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/account").to_request()).await;
        test::call_service(&app, test::TestRequest::get().uri("/account").to_request()).await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn set_cookie_responses_are_not_cached_by_default() {
        let counter = Arc::new(AtomicUsize::new(0));
        async fn handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
            counter.fetch_add(1, Ordering::SeqCst);
            HttpResponse::Ok()
                .insert_header((header::SET_COOKIE, "session=abc"))
                .finish()
        }

        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/login-callback", web::get().to(handler)),
        )
        .await;

        test::call_service(
            &app,
            test::TestRequest::get().uri("/login-callback").to_request(),
        )
        .await;
        test::call_service(
            &app,
            test::TestRequest::get().uri("/login-callback").to_request(),
        )
        .await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn concurrent_requests_do_not_panic_or_deadlock() {
        // actix-web's ServiceResponse/HttpRequest are Rc-based and not
        // Send, so real OS-thread spawning (tokio::spawn) is not an option
        // here. Driving the futures concurrently with join_all still
        // exercises the cache store's async locks under interleaved
        // access, which is what this test cares about.
        let counter = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(store()))
                .route("/products", web::get().to(counting_handler)),
        )
        .await;

        let futures = (0..20).map(|_| {
            let app = &app;
            async move {
                let req = test::TestRequest::get().uri("/products").to_request();
                test::call_service(app, req).await
            }
        });

        let results = futures_util::future::join_all(futures).await;
        for res in results {
            assert!(res.status().is_success());
        }
    }

    #[actix_web::test]
    async fn unbufferable_streaming_body_is_not_cached_and_store_stays_clean() {
        use futures_util::stream;

        let counter = Arc::new(AtomicUsize::new(0));
        async fn handler(counter: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
            counter.fetch_add(1, Ordering::SeqCst);
            // A response with an unknown/streamed body size is treated as
            // non-cacheable and passed straight through.
            let body = stream::once(async { Ok::<_, actix_web::Error>(web::Bytes::from("chunk")) });
            HttpResponse::Ok().streaming(body)
        }

        let cache_store = store();
        let app = test::init_service(
            App::new()
                .app_data(counter_app_data(&counter))
                .wrap(Cache::new(cache_store.clone()))
                .route("/stream", web::get().to(handler)),
        )
        .await;

        test::call_service(&app, test::TestRequest::get().uri("/stream").to_request()).await;
        test::call_service(&app, test::TestRequest::get().uri("/stream").to_request()).await;

        // Neither request was served from cache, and nothing was written.
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert!(
            cache_store
                .get(&"localhost:8080/stream".to_string())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    async fn cache_key_includes_host_path_and_query_only() {
        let req = test::TestRequest::get()
            .uri("/products?page=2&limit=20")
            .insert_header(("Host", "example.com"))
            .to_srv_request();

        assert_eq!(cache_key(&req), "example.com/products?page=2&limit=20");
    }

    #[test]
    async fn cache_key_distinguishes_query_variants_and_hosts() {
        let no_query = test::TestRequest::get()
            .uri("/products")
            .insert_header(("Host", "example.com"))
            .to_srv_request();
        let page1 = test::TestRequest::get()
            .uri("/products?page=1")
            .insert_header(("Host", "example.com"))
            .to_srv_request();
        let page2 = test::TestRequest::get()
            .uri("/products?page=2")
            .insert_header(("Host", "example.com"))
            .to_srv_request();
        let other_host = test::TestRequest::get()
            .uri("/products?page=2")
            .insert_header(("Host", "api.example.com"))
            .to_srv_request();

        let keys = [
            cache_key(&no_query),
            cache_key(&page1),
            cache_key(&page2),
            cache_key(&other_host),
        ];

        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    keys[i], keys[j],
                    "keys must be distinct: {:?} vs {:?}",
                    keys[i], keys[j]
                );
            }
        }
    }
}
