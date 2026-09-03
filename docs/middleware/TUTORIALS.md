# `middleware` — Tutorials

These tutorials build up a small Actix-Web service step by step, adding one
middleware at a time. Each section assumes the previous ones are in place, so
by the end you have a service with request tracing, JWT auth, rate limiting,
pagination, idempotent writes, and route-level permissions all working
together correctly-ordered.

If you only need a single middleware in isolation, see **EXAMPLES.md**
instead.

---

## 1. Start with request tracing: `RequestId`

`RequestId` is almost always the outermost middleware, because other pieces
(`ReadContext<T>`) depend on it having already run.

```rust
use actix_web::{web, App, HttpServer, HttpResponse};
use actixutils::middleware::RequestId;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .wrap(RequestId)
            .route("/ping", web::get().to(ping))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

async fn ping() -> HttpResponse {
    HttpResponse::Ok().finish()
}
```

Run it and check the response headers — every response now carries an
`X-Request-Id: <uuid>` header, and every `tracing` span emitted while
handling the request includes the same id. Handlers can read it back with:

```rust
use actix_web::{HttpRequest, HttpMessage};
use actixutils::middleware::RequestIdStr;

async fn ping(req: HttpRequest) -> HttpResponse {
    let rid = req.extensions().get::<RequestIdStr>().unwrap().0.clone();
    tracing::info!(request_id = %rid, "handled ping");
    HttpResponse::Ok().finish()
}
```

---

## 2. Add JWT authentication: `Auth<T>`

Next, protect an API scope with bearer-token auth. `Auth<T>` needs a
validator — anything implementing `actixutils::locals::Validate<T>`. We'll use
the crate's `HS256Signer` against an `Identity` claims type.

```rust
use actix_web::{web, App};
use actixutils::locals::{HS256Signer, Identity, Validate};
use actixutils::middleware::{Auth, RequestId};
use std::sync::Arc;

let signer: Arc<dyn Validate<Identity>> =
    Arc::new(HS256Signer::new("my-service".to_string(), "super-secret".to_string()));

App::new()
    .wrap(RequestId)
    .service(
        web::scope("/api")
            .wrap(Auth { validator: signer.clone() })
            .route("/me", web::get().to(me_handler)),
    );

async fn me_handler(req: actix_web::HttpRequest) -> actix_web::HttpResponse {
    use actix_web::HttpMessage;
    let identity = req.extensions().get::<Identity>().cloned();
    actix_web::HttpResponse::Ok().json(identity.is_some())
}
```

**What happens on a request:**
1. `Auth<T>` looks for the token in the `Authorization: Bearer <token>` header,
   falling back to an `access_token` cookie.
2. Missing token → `401 Unauthorized` immediately.
3. `signer.validate(&token)` fails → `401 Unauthorized`, with the underlying
   error logged via `tracing::warn!`.
4. On success, the resulting `Identity` (or whatever `T` you chose) is
   inserted into request extensions for every downstream handler and
   middleware to read.

If a value of type `T` is *already* in extensions (e.g. inserted by an outer
middleware layer during testing), `Auth<T>` skips validation entirely — handy
for injecting a fake principal in integration tests without a real token.

---

## 3. Rate limit per authenticated user: `RateLimiter<T>`

`RateLimiter<T>` needs a key type that implements both `FromRequest` (to pull
it out of the request) and `GetId` (to turn it into a hashable key). We'll key
on the `Identity` extractor from step 2.

First, implement `GetId` once for your key type:

```rust
use actixutils::extractors::Jwt;
use actixutils::locals::Identity;
use actixutils::locals::rate_limiter::GetId;
use uuid::Uuid;

impl GetId for Jwt<Identity> {
    type Id = Uuid;
    fn id(&self) -> Uuid {
        self.0.sub
    }
}
```

`RateLimiter::new` also needs a backing store — `Arc<dyn locals::Store<Uuid, VecDeque<Instant>>>`
here, since `Jwt<Identity>::Id` is `Uuid`. actixutils doesn't ship a default
in-memory `Store`, so supply a minimal one (a `HashMap` behind a lock is
enough for a single process; see `src/middleware/test_session.rs` for the
same pattern used with sessions):

```rust
use actixutils::locals::Store;
use async_trait::async_trait;
use std::{collections::{HashMap, VecDeque}, error::Error, sync::{Arc, Instant}};
use tokio::sync::RwLock;
use uuid::Uuid;

struct MemoryRateStore {
    inner: RwLock<HashMap<Uuid, VecDeque<Instant>>>,
}

#[async_trait]
impl Store<Uuid, VecDeque<Instant>> for MemoryRateStore {
    async fn get(&self, key: &Uuid) -> Result<Option<VecDeque<Instant>>, Box<dyn Error>> {
        Ok(self.inner.read().await.get(key).cloned())
    }
    async fn set(&self, key: &Uuid, value: VecDeque<Instant>) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.insert(*key, value);
        Ok(())
    }
    async fn delete(&self, key: &Uuid) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.remove(key);
        Ok(())
    }
    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.inner.write().await.clear();
        Ok(())
    }
}
```

Then wrap the scope:

```rust
use actixutils::middleware::RateLimiter;
use std::time::Duration;

let rate_store = Arc::new(MemoryRateStore { inner: RwLock::new(HashMap::new()) });

web::scope("/api")
    .wrap(Auth { validator: signer.clone() })
    .wrap(RateLimiter::<Jwt<Identity>>::new(rate_store, 100, Duration::from_secs(60)))
    .route("/me", web::get().to(me_handler));
```

This allows 100 requests per user per rolling 60-second window. Requests over
the limit get `429 Too Many Requests` without the handler ever running. The
`MemoryRateStore` above is per-process — fine for a single instance, but for
multi-node deployments implement `Store` against a shared backend (Redis or
similar) instead.

> **Note on ordering:** put `RateLimiter` *inside* `Auth` (i.e. `.wrap(Auth)`
> then `.wrap(RateLimiter)`, remembering `.wrap()` calls apply outer-to-inner
> in the order written but execute inner-to-outer at runtime — the important
> thing is that whatever extractor `RateLimiter` uses to identify the caller
> must succeed, which for a JWT-based key means `Auth` isn't strictly required
> first since `RateLimiter` performs its own extraction via `FromRequest`).

---

## 4. Make writes safe to retry: `Idempotency<Store>`

For any POST/PUT/PATCH endpoint where a network retry could double-apply a
mutation (charging a card, creating an order), wrap it in `Idempotency`.

You need a store. Note that `locals::IdempotencyStore` is its own trait
family (`acquire`/`get`/`complete`/`release`), separate from the general
`locals::Store<K, V>` trait used by `RateLimiter`/`Cache`/`SessionMiddleware`
— it isn't a `Store` implementation. For a quick start, an in-memory one:

```rust
use actixutils::locals::{IdempotencyStore, IdempotencyState, CachedResponse};
use async_trait::async_trait;
use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};

#[derive(Default)]
struct MemoryIdempotencyStore {
    entries: Mutex<HashMap<String, IdempotencyState>>,
}

#[async_trait]
impl IdempotencyStore for MemoryIdempotencyStore {
    type Error = std::convert::Infallible;

    async fn acquire(&self, key: &str, _ttl: Duration) -> Result<bool, Self::Error> {
        let mut entries = self.entries.lock().unwrap();
        if entries.contains_key(key) {
            Ok(false)
        } else {
            entries.insert(key.to_string(), IdempotencyState::InProgress);
            Ok(true)
        }
    }

    async fn get(&self, key: &str) -> Result<Option<IdempotencyState>, Self::Error> {
        Ok(match self.entries.lock().unwrap().get(key) {
            Some(IdempotencyState::InProgress) => Some(IdempotencyState::InProgress),
            Some(IdempotencyState::Completed(r)) => Some(IdempotencyState::Completed(r.clone())),
            None => None,
        })
    }

    async fn complete(&self, key: &str, response: CachedResponse) -> Result<(), Self::Error> {
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), IdempotencyState::Completed(response));
        Ok(())
    }

    async fn release(&self, key: &str) -> Result<(), Self::Error> {
        self.entries.lock().unwrap().remove(key);
        Ok(())
    }
}
```

This single-mutex version is not perfectly atomic under extreme concurrency
on `acquire` (a real implementation would want an atomic check-and-insert,
which `HashMap::entry` under a single `Mutex` does provide here) — for
production, back this with Redis (`SET key val NX EX ttl`) or an equivalent
atomic primitive.

Wire it into the payments scope:

```rust
use actixutils::middleware::Idempotency;

let store = Arc::new(MemoryIdempotencyStore::default());

web::scope("/payments")
    .wrap(Idempotency::new(store).ttl(Duration::from_secs(24 * 60 * 60)))
    .route("/charge", web::post().to(charge_handler));
```

Now a client that wants a charge to be retry-safe sends an
`Idempotency-Key: <client-generated-uuid>` header. The first request with a
given key runs the handler normally and caches the resulting status, headers,
and body. Any subsequent request with the same key — while the first is still
in flight, or after it completed — gets the cached outcome instead of
re-running `charge_handler`. Requests with no `Idempotency-Key` header pass
through untouched, so this is safe to apply broadly.

---

## 5. Add pagination without threading params everywhere: `PaginationMiddleware`

For list endpoints, avoid passing `page`/`limit` through every function
signature by reading them from a task-local instead.

```rust
use actixutils::middleware::PaginationMiddleware;
use actixutils::locals::Pagination;

web::scope("/items")
    .wrap(PaginationMiddleware)
    .route("", web::get().to(list_items));

async fn list_items() -> actix_web::HttpResponse {
    let items = repo::list().await; // repo::list reads Pagination::get() internally, several calls deep
    actix_web::HttpResponse::Ok().json(items)
}

mod repo {
    use actixutils::locals::Pagination;

    pub async fn list() -> Vec<String> {
        let p = Pagination::get();
        // SELECT ... LIMIT p.limit OFFSET (p.page * p.limit)
        vec![]
    }
}
```

`?page=` and `?limit=` are optional; missing or unparseable values default to
`page = 0, limit = 100`. Because the value lives in a Tokio task-local
scoped around the whole request future, `repo::list()` can call
`Pagination::get()` without `list_items` ever seeing a `Pagination` value
itself.

---

## 6. Gate individual routes with bitmask permissions: `Permissions<P>`

The `permission` submodule is a separate, authentication-agnostic RBAC layer.
It expects a `Principal` (something with a `role() -> u128`) to already be in
request extensions — typically inserted by the `Auth<T>` middleware from step
2, provided `T` implements `Principal`.

Define the permission map, usually as a JSON file checked into the repo:

```json
{
  "permissions": [
    { "method": "GET",    "url": "/items",       "bit_id": 0 },
    { "method": "POST",   "url": "/items",       "bit_id": 1 },
    { "method": "DELETE", "url": "/items/{id}",  "bit_id": 2 }
  ]
}
```

Load it and wrap the scope, with the permissions middleware *inside* auth so
the principal is already present:

```rust
use actixutils::middleware::{Permissions, PermissionSet, Principal};

#[derive(Clone)]
struct Identity { role: u128 /* ...other claims */ }

impl Principal for Identity {
    fn role(&self) -> u128 { self.role }
}

let permissions = PermissionSet::from_file("permissions.json")?;

web::scope("/items")
    .wrap(Auth { validator: signer.clone() }) // inserts Identity
    .wrap(Permissions::<Identity>::new(permissions))
    .route("", web::get().to(list_items))
    .route("", web::post().to(create_item))
    .route("/{id}", web::delete().to(delete_item));
```

This is **default-deny**: any `(method, path)` combination not listed in
`permissions.json` returns `403 Forbidden`, even for an authenticated user. A
missing principal (auth middleware didn't run, or the user genuinely isn't
authenticated) returns `401 Unauthorized`. A principal whose `role()` bitmask
doesn't have the required bit set returns `403 Forbidden`.

---

## 7. Putting it together

A full stack combining everything above, respecting dependency order
(outermost first):

```rust
App::new()
    .wrap(RequestId)                                  // 1. correlation id, must be outermost
    .service(
        web::scope("/api")
            .wrap(Auth { validator: signer.clone() }) // 2. authenticate, inserts Identity
            .service(
                web::scope("/items")
                    .wrap(Permissions::<Identity>::new(permissions.clone())) // 6. authorize
                    .wrap(PaginationMiddleware)                              // 5. list params
                    .wrap(RateLimiter::<Jwt<Identity>>::new(rate_store.clone(), 100, Duration::from_secs(60))) // 3.
                    .route("", web::get().to(list_items))
                    .route("", web::post().to(create_item)),
            )
            .service(
                web::scope("/payments")
                    .wrap(Idempotency::new(idempotency_store.clone()))       // 4. retry-safety
                    .route("/charge", web::post().to(charge_handler)),
            ),
    );
```

From here, see **EXAMPLES.md** for standalone snippets covering
`ResponseEqualizer`, `Session<T>`, `ReadContext<T>`, `AttachLocal<T>`, `Cache`,
`Singleflight<K, KeyFn>`, `TimeoutMiddleware`, `ClientIpMiddleware`, and the
`identity`/`authority` helper functions that weren't needed in this
walkthrough.
