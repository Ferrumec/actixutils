# `middleware` — Examples

Focused, standalone snippets for each middleware in the module. Unlike
TUTORIALS.md, these don't build on each other — copy whichever section you
need.

---

## `Auth<T>` — JWT bearer authentication

```rust
use actix_web::{web, App};
use actixutils::locals::{HS256Signer, Identity, Validate};
use actixutils::middleware::Auth;
use std::sync::Arc;

let signer: Arc<dyn Validate<Identity>> =
    Arc::new(HS256Signer::new("svc".to_string(), "secret".to_string()));

App::new().service(
    web::scope("/api")
        .wrap(Auth { validator: signer })
        .route("/me", web::get().to(me_handler)),
);
# async fn me_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Reading the claims in a handler:

```rust
use actix_web::{HttpRequest, HttpMessage, HttpResponse};
use actixutils::locals::Identity;

async fn me_handler(req: HttpRequest) -> HttpResponse {
    match req.extensions().get::<Identity>() {
        Some(identity) => HttpResponse::Ok().json(identity),
        None => HttpResponse::InternalServerError().finish(), // shouldn't happen if Auth ran
    }
}
```

Token source: `Authorization: Bearer <token>` header, or an `access_token`
cookie as a fallback. Missing token or a failed `validate()` call both result
in `401 Unauthorized`.

---

## `ResponseEqualizer` — timing-attack mitigation

```rust
use actixutils::middleware::ResponseEqualizer;
use actix_web::{web, App};
use std::time::Duration;

App::new().service(
    web::scope("/auth")
        .wrap(ResponseEqualizer::with_jitter(
            Duration::from_millis(150), // every response takes at least 150ms
            Duration::from_millis(50),  // plus up to 50ms of random jitter
        ))
        .route("/login", web::post().to(login_handler)),
);
# async fn login_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Use `ResponseEqualizer::new(min_duration)` instead if you don't want jitter —
useful on login, password-reset, or "does this account exist" endpoints where
response latency alone could reveal whether a lookup short-circuited.

---

## `RateLimiter<T>` — sliding-window rate limiting

`RateLimiter::new` takes a backing `Arc<dyn locals::Store<T::Id, VecDeque<Instant>>>`
— there's no default in-memory store shipped with the crate, so supply a small
one (a `HashMap` behind an `RwLock` is enough for a single process). The key
type must implement `GetId`; here it's keyed on the crate's built-in
`ClientIp` extractor (see [`ClientIpMiddleware`](#clientipmiddleware--client-ip--trusted-proxy-resolution)
below for how `ClientIp` gets populated). Swap in a JWT-based extractor for
per-user limits (see TUTORIALS.md §3).

```rust
use actixutils::extractors::ClientIp;
use actixutils::locals::Store;
use actixutils::middleware::{ClientIpMiddleware, RateLimiter};
use actixutils::locals::ProxyConfig;
use actix_web::{web, App};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::net::IpAddr;
use std::sync::{Arc, Instant};
use std::time::Duration;
use tokio::sync::RwLock;

struct MemoryRateStore {
    inner: RwLock<HashMap<IpAddr, VecDeque<Instant>>>,
}

#[async_trait]
impl Store<IpAddr, VecDeque<Instant>> for MemoryRateStore {
    async fn get(&self, key: &IpAddr) -> Result<Option<VecDeque<Instant>>, Box<dyn Error>> {
        Ok(self.inner.read().await.get(key).cloned())
    }
    async fn set(&self, key: &IpAddr, value: VecDeque<Instant>) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.insert(*key, value);
        Ok(())
    }
    async fn delete(&self, key: &IpAddr) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.remove(key);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.clear();
        Ok(())
    }
}

let store = Arc::new(MemoryRateStore { inner: RwLock::new(HashMap::new()) });

App::new().service(
    web::scope("/public")
        .wrap(ClientIpMiddleware::new(ProxyConfig::new(vec![]))) // populates ClientIp
        .wrap(RateLimiter::<ClientIp>::new(store, 20, Duration::from_secs(60)))
        .route("/search", web::get().to(search_handler)),
);
# async fn search_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Requests over the limit receive `429 Too Many Requests` and never reach
`search_handler`. If the identity extractor itself fails (e.g. `ClientIp`
without `ClientIpMiddleware` having run, or a JWT-based key with no valid
token), the request is **not** rejected by `RateLimiter` — it passes through
unlimited, so pair it with an auth middleware (or `ClientIpMiddleware`, as
above) if you need requests without a resolvable identity blocked outright.

---

## `Idempotency<Store>` — safe request deduplication

```rust
use actixutils::middleware::Idempotency;
use actixutils::locals::{IdempotencyStore, IdempotencyState, CachedResponse};
use actix_web::{web, App};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

struct RedisIdempotencyStore { /* ... */ }

#[async_trait]
impl IdempotencyStore for RedisIdempotencyStore {
    type Error = std::io::Error;

    async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error> {
        // SETNX key "in_progress" EX ttl
        Ok(true)
    }
    async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error> {
        Ok(None)
    }
    async fn complete(&self, key: &str, response: CachedResponse) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn release(&self, key: &str) -> Result<(), Self::Error> {
        Ok(())
    }
}

let store = Arc::new(RedisIdempotencyStore { /* ... */ });

App::new().service(
    web::scope("/payments")
        .wrap(
            Idempotency::new(store)
                .ttl(Duration::from_secs(86_400))     // default is 1 hour
                .header("Idempotency-Key"),            // this is also the default
        )
        .route("/charge", web::post().to(charge_handler)),
);
# async fn charge_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Client usage — send the same key on retry:

```text
POST /payments/charge
Idempotency-Key: 6c1b1c1e-2f2a-4c9a-9c2e-3a1f7e9d0b21

{"amount_cents": 2500, "currency": "usd"}
```

- No `Idempotency-Key` header → request passes through untouched, handler
  always runs.
- New key → handler runs once; response cached under that key.
- Same key while the first call is still executing → `409 Conflict`.
- Same key after completion (within TTL) → cached response returned verbatim,
  handler not re-invoked.

---

## `RequestId` — per-request correlation id

```rust
use actixutils::middleware::RequestId;
use actix_web::{web, App};

App::new()
    .wrap(RequestId)
    .route("/ping", web::get().to(ping));
# async fn ping() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Reading it downstream:

```rust
use actixutils::middleware::RequestIdStr;
use actix_web::{HttpRequest, HttpResponse, HttpMessage};

async fn handler(req: HttpRequest) -> HttpResponse {
    if let Some(rid) = req.extensions().get::<RequestIdStr>() {
        tracing::info!(request_id = %rid.0, "handling request");
    }
    HttpResponse::Ok().finish()
}
```

Every response also gets an `X-Request-Id` header automatically, and the id
is recorded on the current `tracing::Span` as the `request_id` field (your
span must declare that field, e.g.
`#[tracing::instrument(fields(request_id))]`, for the recording to take
effect).

---

## `Context` / `ReadContext<T>` — event-publishing context

*(Requires the `es` feature.)* Depends on `RequestId` and an auth middleware
having already populated extensions.

```rust
use actixutils::locals::Authority;
use actixutils::middleware::{RequestId, Auth, ReadContext};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpMessage};
use std::sync::Arc;

App::new()
    .wrap(RequestId)                                          // 1. request id
    .wrap(Auth::<Authority> { validator: signer.clone() })     // 2. inserts Authority
    .wrap(ReadContext::<Authority>::new(event_stream.clone(), "orders-svc".into())) // 3.
    .route("/orders", web::post().to(create_order));

async fn create_order(req: HttpRequest) -> HttpResponse {
    use actixutils::middleware::Context;
    if let Some(ctx) = req.extensions().get::<Context>() {
        // ctx.publish(OrderCreated { .. }).await;
    }
    HttpResponse::Ok().finish()
}
# let signer: Arc<dyn actixutils::locals::Validate<Authority>> = unimplemented!();
# let event_stream: Arc<dyn typed_eventbus::EventStream> = unimplemented!();
```

`ReadContext::new` takes the shared event-stream handle and a producer name
that gets embedded in every published event's metadata. Chain
`.with_user_as_audience(true)` if events published in this context should
also be routed to the acting user (e.g. for a personal activity feed).

---

## `Pagination` / `PaginationMiddleware` — task-local list params

```rust
use actixutils::middleware::PaginationMiddleware;
use actixutils::locals::Pagination;
use actix_web::{web, App, HttpResponse};

App::new().service(
    web::scope("/items")
        .wrap(PaginationMiddleware)
        .route("", web::get().to(list_items)),
);

async fn list_items() -> HttpResponse {
    let p = Pagination::get();
    HttpResponse::Ok().json(serde_json::json!({ "page": p.page, "limit": p.limit }))
}
```

`GET /items?page=2&limit=25` → `Pagination { page: 2, limit: 25 }`.
`GET /items` (no query params) → defaults of `page: 0, limit: 100`.

---

## `Session<T>` / `SessionMiddleware<T>` — cookie sessions

`Session<T>` lives in `actixutils::extractors` (re-exported as
`actixutils::Session`); `SessionMiddleware<T>` lives in
`actixutils::middleware`. Both are generic over your own session type `T`,
and `SessionMiddleware<T>` is backed by the same general-purpose
`locals::Store<Uuid, T>` trait used by `RateLimiter` and `Cache` — there is
no separate `SessionStore` trait.

```rust
use actixutils::{Session, Store};
use actixutils::middleware::SessionMiddleware;
use actix_web::{web, App, HttpResponse};
use async_trait::async_trait;
use std::{collections::HashMap, error::Error, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CartSession {
    item_ids: Vec<String>,
}

#[derive(Default)]
struct InMemorySessionStore {
    data: RwLock<HashMap<Uuid, CartSession>>,
}

#[async_trait]
impl Store<Uuid, CartSession> for InMemorySessionStore {
    async fn get(&self, session_id: &Uuid) -> Result<Option<CartSession>, Box<dyn Error>> {
        Ok(self.data.read().await.get(session_id).cloned())
    }
    async fn set(&self, session_id: &Uuid, session: CartSession) -> Result<(), Box<dyn Error>> {
        self.data.write().await.insert(*session_id, session);
        Ok(())
    }
    async fn delete(&self, session_id: &Uuid) -> Result<(), Box<dyn Error>> {
        self.data.write().await.remove(session_id);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.data.write().await.clear();
        Ok(())
    }
}

let store: Arc<dyn Store<Uuid, CartSession>> = Arc::new(InMemorySessionStore::default());

App::new()
    .wrap(SessionMiddleware::new(store).cookie_name("cart_session"))
    .route("/cart/add", web::post().to(add_to_cart));

async fn add_to_cart(session: Session<CartSession>) -> HttpResponse {
    let mut cart = session.write().await; // marks the session dirty
    cart.item_ids.push("sku-123".into());
    HttpResponse::Ok().finish()
}
```

- `SessionMiddleware::new(store)` — no cookie, or an unparseable one, gets a
  fresh `CartSession::default()`; a new session cookie is issued on the
  response.
- `SessionMiddleware::required(store)` — same situation instead returns
  `401 Unauthorized` before the handler runs.
- `session.read().await` gives a read-only view without marking anything
  dirty. `session.write().await` marks the session dirty regardless of
  whether you actually mutate it, so the middleware persists it via
  `store.set()` after the handler returns. If you never call `.write()`,
  nothing is saved — the middleware skips the store round-trip for read-only
  requests.

---

## `AttachLocal<T>` / `SetLocal` — generic scoped extraction

Use this when you want the same "extract once, expose everywhere via a
task-local" pattern that `PaginationMiddleware` uses, but for your own type.

```rust
use actixutils::middleware::AttachLocal;
use actix_web::{web, App, FromRequest, HttpRequest, dev::Payload};
use std::future::Future;

#[derive(Clone)]
struct TenantId(String);

tokio::task_local! {
    static TENANT: TenantId;
}

impl actixutils::middleware::SetLocal for TenantId {
    fn scope<F: Future>(self, fut: F) -> impl Future<Output = F::Output> {
        TENANT.scope(self, fut)
    }
}

impl FromRequest for TenantId {
    type Error = actix_web::Error;
    type Future = std::future::Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let tenant = req
            .headers()
            .get("X-Tenant-Id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        std::future::ready(Ok(TenantId(tenant)))
    }
}

App::new().service(
    web::scope("/api")
        .wrap(AttachLocal::<TenantId>::new())
        .route("/data", web::get().to(get_data)),
);

async fn get_data() -> actix_web::HttpResponse {
    let tenant = TENANT.try_with(|t| t.0.clone()).unwrap_or_else(|_| "unknown".into());
    actix_web::HttpResponse::Ok().body(tenant)
}
```

If `T::from_request` fails, the error is converted into an `actix_web::Error`
and the request is rejected before the downstream service ever runs.

---

## `identity` / `authority(bit)` — `Next`-style JWT guards

*(Requires the `jwt` feature.)* Alternative to `Auth<T>` for cases where you
want route-level `from_fn` gating rather than a `Transform`-based middleware.

```rust
use actixutils::middleware::{identity, authority};
use actix_web::{web, middleware::from_fn, App};

App::new().service(
    web::scope("/api")
        .wrap(from_fn(identity)) // require any valid Identity JWT
        .route("/profile", web::get().to(profile_handler))
        .service(
            web::scope("/billing")
                .wrap(from_fn(authority(3))) // additionally require permission bit 3
                .route("/invoices", web::get().to(invoices_handler)),
        ),
);
# async fn profile_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
# async fn invoices_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

`identity` fails with `401 Unauthorized` (raised by the underlying
`Jwt<Identity>` extractor) if the token is missing or invalid. `authority(n)`
fails with `401` under the same conditions, or `403 Forbidden` if the token
is valid but bit `n` of `Authority::role` isn't set.

---

## `Permissions<P>` — bitmask RBAC (the `permission` submodule)

```rust
use actix_web::{web, App, HttpResponse, http::Method};
use actixutils::middleware::{Permission, PermissionSet, Permissions, Principal};

#[derive(Clone)]
struct User { role: u128 }

impl Principal for User {
    fn role(&self) -> u128 { self.role }
}

// Build in code...
let permissions = PermissionSet::new(vec![
    Permission::new(Method::GET, "/users", 0).unwrap(),
    Permission::new(Method::POST, "/users", 1).unwrap(),
    Permission::new(Method::GET, "/users/{id}", 2).unwrap(),
    Permission::new(Method::DELETE, "/files/{tail:.*}", 4).unwrap(),
]).unwrap();

// ...or load from JSON:
// let permissions = PermissionSet::from_file("permissions.json").unwrap();

App::new()
    // An upstream auth middleware must insert a `User` into extensions first.
    .wrap(Permissions::<User>::new(permissions))
    .route("/users", web::get().to(|| async { HttpResponse::Ok() }));
```

Route patterns use Actix's native `ResourceDef` syntax, so dynamic segments
(`{id}`), regex segments (`{id:\d+}`), and tail-matching (`{tail:.*}`) all
work exactly as they would in a normal Actix route.

| Scenario | Response |
|---|---|
| No permission entry matches `(method, path)` | `403 Forbidden` (default-deny) |
| Permission matches, but no `Principal` in extensions | `401 Unauthorized` |
| Principal present, but the required bit isn't set | `403 Forbidden` |
| Principal present with the required bit set | request proceeds |

`PermissionSet` validates on construction (via `new`, `from_file`,
`from_reader`, or `from_json`): every `bit_id` must be `0..128`, and no two
entries may share the same `(method, route)` pair — construction returns a
`PermissionError` otherwise.

---

## `Cache` — GET-only HTTP response caching

Like `RateLimiter` and `SessionMiddleware`, `Cache` is backed by the general
`locals::Store<K, V>` trait — here `Store<String, cache::CachedResponse>` —
not by the `cache::CacheStore` trait (which exists in `src/middleware/cache/store.rs`
but is not currently wired into `Cache`; implement `locals::Store` instead).

```rust
use actixutils::locals::Store;
use actixutils::middleware::cache::CachedResponse;
use actixutils::middleware::Cache;
use actix_web::{web, App};
use async_trait::async_trait;
use std::{collections::HashMap, error::Error, sync::Arc, time::{Duration, Instant}};
use tokio::sync::RwLock;

struct MemoryCache {
    entries: RwLock<HashMap<String, (CachedResponse, Instant)>>,
    ttl: Duration,
}

#[async_trait]
impl Store<String, CachedResponse> for MemoryCache {
    async fn get(&self, key: &String) -> Result<Option<CachedResponse>, Box<dyn Error>> {
        let entries = self.entries.read().await;
        Ok(match entries.get(key) {
            Some((resp, expires_at)) if Instant::now() < *expires_at => Some(resp.clone()),
            _ => None,
        })
    }
    async fn set(&self, key: &String, value: CachedResponse) -> Result<(), Box<dyn Error>> {
        self.entries
            .write()
            .await
            .insert(key.clone(), (value, Instant::now() + self.ttl));
        Ok(())
    }
    async fn delete(&self, key: &String) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.remove(key);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.clear();
        Ok(())
    }
}

let store = Arc::new(MemoryCache {
    entries: RwLock::new(HashMap::new()),
    ttl: Duration::from_secs(60),
});

App::new().service(
    web::scope("/catalog")
        .wrap(Cache::new(store))
        .route("/products", web::get().to(list_products)),
);
# async fn list_products() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Behaviour to keep in mind:

- Only `GET` requests are ever looked up or stored; every other method
  passes straight through.
- The cache key is `host + path + query` — it never looks at headers,
  cookies, or auth state, so don't put this in front of routes whose
  response depends on who's asking.
- A response is skipped from caching if its status isn't a 2xx, if it sets
  `Set-Cookie`, or if its `Cache-Control` contains `no-store` or `private`.
- Only responses with a known, finite body length are buffered and cached;
  streaming or chunked responses are returned as-is and never cached.
- TTL is entirely up to your `Store` implementation — `Cache` itself has no
  `.ttl()` method; expire entries however your `set`/`get` logic decides to
  (as in `MemoryCache` above).

---

## `Singleflight<K, KeyFn>` — request coalescing

Groups concurrent requests that map to the same key so only one of them
actually reaches the wrapped service; the rest wait and receive a cloned
copy of the leader's response.

```rust
use actixutils::middleware::Singleflight;
use actix_web::{web, App};

App::new().service(
    web::scope("/reports")
        .wrap(Singleflight::new(|req: &actix_web::dev::ServiceRequest| {
            // Coalesce by path + query, so two clients requesting the same
            // expensive report at once only compute it once.
            req.uri().to_string()
        }))
        .route("/{name}", web::get().to(generate_report)),
);
# async fn generate_report() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

The response body is fully buffered so it can be cloned to every follower,
so this is best suited to endpoints with bounded, non-streaming responses
(e.g. an expensive read that several clients might request at once) rather
than large downloads. If the leader's execution errors, panics, or is
cancelled, all waiting followers receive an error instead of hanging
forever.

---

## `TimeoutMiddleware` — per-request deadline

```rust
use actixutils::middleware::TimeoutMiddleware;
use actix_web::{web, App};
use std::time::Duration;

App::new().service(
    web::scope("/slow-integration")
        .wrap(TimeoutMiddleware::new(Duration::from_secs(5)))
        .route("/sync", web::post().to(sync_handler)),
);
# async fn sync_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

If `sync_handler` (plus anything else in the wrapped chain) hasn't produced a
response within 5 seconds, the in-flight future is dropped and the client
receives `504 Gateway Timeout` instead.

---

## `ClientIpMiddleware` — client IP / trusted-proxy resolution

Resolves the real client IP even when requests pass through one or more
reverse proxies, without blindly trusting a spoofable `X-Forwarded-For`
header from just anyone.

```rust
use actixutils::extractors::ClientIp;
use actixutils::locals::ProxyConfig;
use actixutils::middleware::ClientIpMiddleware;
use actix_web::{web, App, HttpResponse};

let proxy_config = ProxyConfig::new(vec![
    "10.0.0.0/8".parse().unwrap(),      // internal load balancer subnet
    "172.16.0.0/12".parse().unwrap(),
]);

App::new()
    .wrap(ClientIpMiddleware::new(proxy_config))
    .route("/whoami", web::get().to(whoami));

async fn whoami(ip: ClientIp) -> HttpResponse {
    HttpResponse::Ok().body(ip.ip().to_string())
}
```

- If the direct peer address is in a trusted proxy network, the middleware
  walks `X-Forwarded-For` from right to left (closest hop to furthest) and
  returns the first address that *isn't* itself a trusted proxy — i.e. the
  original client.
- If the peer isn't a trusted proxy, its address is used directly and
  `X-Forwarded-For` is ignored (so an untrusted client can't spoof its own
  IP by setting the header).
- The `ClientIp` extractor returns `500 Internal Server Error` if
  `ClientIpMiddleware` never ran for this request — always pair the two.
