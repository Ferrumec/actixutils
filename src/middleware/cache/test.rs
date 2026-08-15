use super::*;
use actix_web::http::StatusCode;
use std::error::Error;
use std::sync::Arc;

use actix_web::HttpResponse;
use actix_web::dev::ServiceRequest;
use actix_web::http::header;

use actix_web::{App, test, web};
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::types::CachedResponse;
use crate::Store;

const DEFAULT_MAX_ENTRIES: usize = 10_000;

#[derive(Clone)]
struct Entry {
    response: CachedResponse,
    expires_at: Instant,
}

impl Entry {
    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// A simple, process-local cache store backed by a `HashMap` behind a
/// `tokio::sync::RwLock`.
///
/// - Expired entries are treated as absent on read, and swept opportunistically
///   on write.
/// - Growth is bounded by `max_entries`; once the limit is reached, the
///   oldest-inserted entries are evicted first (FIFO), which is a
///   deliberately simple policy for this first implementation.
/// - Not shared across processes. For multi-instance deployments, implement
///   [`CacheStore`] against a shared backend (e.g. Redis) instead.
struct MemoryCache {
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    insertion_order: Arc<RwLock<VecDeque<String>>>,
    max_entries: usize,
}

impl MemoryCache {
    /// Create a cache with the default capacity (10,000 entries).
    fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_ENTRIES)
    }

    /// Create a cache bounded to `max_entries` entries.
    fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            insertion_order: Arc::new(RwLock::new(VecDeque::new())),
            max_entries,
        }
    }

    async fn evict_if_needed(&self) {
        let mut entries = self.entries.write().await;

        // Opportunistically drop anything already expired before evicting.
        entries.retain(|_, entry| !entry.is_expired());

        if entries.len() < self.max_entries {
            return;
        }

        let mut order = self.insertion_order.write().await;
        while entries.len() >= self.max_entries {
            match order.pop_front() {
                Some(oldest_key) => {
                    entries.remove(&oldest_key);
                }
                None => break,
            }
        }
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store<String, CachedResponse> for MemoryCache {
    async fn get(&self, key: &String) -> Result<Option<CachedResponse>, Box<dyn Error>> {
        let entries = self.entries.read().await;
        Ok(match entries.get(key) {
            Some(entry) if !entry.is_expired() => Some(entry.response.clone()),
            _ => None,
        })
    }

    async fn set(&self, key: &String, response: CachedResponse) -> Result<(), Box<dyn Error>> {
        self.evict_if_needed().await;
        let ttl = Duration::from_millis(60);
        let entry = Entry {
            response,
            expires_at: Instant::now() + ttl,
        };

        let mut entries = self.entries.write().await;
        let is_new_key = entries.insert(key.to_string(), entry).is_none();
        drop(entries);

        if is_new_key {
            self.insertion_order
                .write()
                .await
                .push_back(key.to_string());
        }
        Ok(())
    }

    async fn delete(&self, key: &String) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.clear();
        self.insertion_order.write().await.clear();
        Ok(())
    }
}
fn cache_key(req: &ServiceRequest) -> String {
    let host = req.connection_info().host().to_string();
    let path = req.uri().path();
    match req.uri().query() {
        Some(query) => format!("{host}{path}?{query}"),
        None => format!("{host}{path}"),
    }
}

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
    let res = test::call_service(&app, test::TestRequest::get().uri("/partial").to_request()).await;

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
