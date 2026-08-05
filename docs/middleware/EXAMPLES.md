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

The key type must implement `GetId`. Here it's keyed on client IP via a
custom extractor; swap in a JWT-based extractor for per-user limits (see
TUTORIALS.md §3).

```rust
use actixutils::middleware::RateLimiter;
use actixutils::locals::rate_limiter::GetId;
use actix_web::{web, App, FromRequest, HttpRequest, dev::Payload};
use std::{future::{ready, Ready}, net::IpAddr, time::Duration};

struct ClientIp(IpAddr);

impl GetId for ClientIp {
    type Id = IpAddr;
    fn id(&self) -> IpAddr { self.0 }
}

impl FromRequest for ClientIp {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let ip = req.peer_addr().map(|a| a.ip()).unwrap_or([0, 0, 0, 0].into());
        ready(Ok(ClientIp(ip)))
    }
}

App::new().service(
    web::scope("/public")
        .wrap(RateLimiter::<ClientIp>::new(20, Duration::from_secs(60)))
        .route("/search", web::get().to(search_handler)),
);
# async fn search_handler() -> actix_web::HttpResponse { actix_web::HttpResponse::Ok().finish() }
```

Requests over the limit receive `429 Too Many Requests` and never reach
`search_handler`. If the identity extractor itself fails (e.g. a JWT-based
key with no valid token), the request is **not** rejected by `RateLimiter` —
it passes through unlimited, so pair it with an auth middleware if you need
unauthenticated requests blocked outright.

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

## `Session<T>` / `SessionMiddleware<Store>` — cookie sessions

```rust
use actixutils::middleware::{Session, SessionMiddleware, SessionStore};
use actix_web::{web, App, HttpResponse, Error};
use async_trait::async_trait;
use std::{collections::HashMap, sync::{Arc, Mutex}};
use uuid::Uuid;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CartSession {
    item_ids: Vec<String>,
}

#[derive(Default)]
struct InMemorySessionStore {
    data: Mutex<HashMap<Uuid, CartSession>>,
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    type Session = CartSession;

    async fn load(&self, session_id: &Uuid) -> Result<Option<CartSession>, Error> {
        Ok(self.data.lock().unwrap().get(session_id).cloned())
    }
    async fn save(&self, session_id: &Uuid, session: &CartSession) -> Result<(), Error> {
        self.data.lock().unwrap().insert(*session_id, session.clone());
        Ok(())
    }
    async fn delete(&self, session_id: &Uuid) -> Result<(), Error> {
        self.data.lock().unwrap().remove(session_id);
        Ok(())
    }
}

let store = Arc::new(InMemorySessionStore::default());

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
  `store.save()` after the handler returns. If you never call `.write()`,
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
