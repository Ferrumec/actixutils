# actixutils

A comprehensive authentication, session management, and middleware utilities library for [Actix-web](https://actix.rs/) applications.

actixutils provides battle-tested building blocks for secure, scalable HTTP services:

- **JWT authentication** — HS256 (HMAC) and RS256 (RSA) signing and validation
- **Request extractors** — `Auth<T>`, `Access`, and `Session<T>` for handler arguments
- **Middleware suite** — authentication, rate limiting, idempotency, pagination, request ID injection, constant-time response equalisation, and typed-eventbus context propagation
- **Role-based authorisation** — 128-bit bitmask permission checks via `Authority`

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
  - [JWT Claims: Identity & Authority](#jwt-claims-identity--authority)
  - [Sign & Validate Traits](#sign--validate-traits)
  - [Signers: HS256 and RS256](#signers-hs256-and-rs256)
- [Request Extractors](#request-extractors)
  - [Auth\<T\>](#autht)
  - [Access](#access)
  - [Session\<T\>](#sessiont)
- [Middleware](#middleware)
  - [Auth Middleware](#auth-middleware)
  - [RequestId](#requestid)
  - [RateLimiter](#ratelimiter)
  - [Idempotency](#idempotency)
  - [ResponseEqualizer](#responseequalizer)
  - [Pagination](#pagination)
  - [ReadContext](#readcontext)
  - [Helper Functions: identity & authority](#helper-functions-identity--authority)
- [Public Key Utilities](#public-key-utilities)
- [Project Structure](#project-structure)
- [Dependencies](#dependencies)
- [Complete Example](#complete-example)
- [Security Considerations](#security-considerations)
- [License](#license)

---

## Installation

Add with defaults features:

```bash
cargo add actixutils
```

Add with event streaming feature to support request context publishing:
```bash
cargo add actixutils -F es
```
## Quick Start

```rust
use actixutils::{HS256Signer, Identity, Auth};
use actix_web::{web, App, HttpServer, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let signer: Arc<dyn actixutils::Validate<Identity>> = Arc::new(
        HS256Signer::new("my-app".to_string(), "super-secret-key".to_string())
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::from(signer.clone()))
            .route("/login",     web::post().to(login))
            .route("/protected", web::get().to(protected))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn login() -> HttpResponse {
    // build and sign an Identity, return the token …
    HttpResponse::Ok().body("token")
}

async fn protected(auth: Auth<Identity>) -> HttpResponse {
    HttpResponse::Ok().json(&auth.0)
}
```

---

## Core Concepts

### JWT Claims: Identity & Authority

`Identity` is a minimal bearer-token claim set. `Authority` extends it with a 128-bit role bitmask and a recipient UUID for fine-grained access control.

```rust
use actixutils::{Identity, Authority};
use uuid::Uuid;

let id   = Identity::new(Uuid::new_v4(), vec!["my-app".to_string()]);
let auth = Authority::new(user_id, role_bitmask, recipient_id, audiences);

// Test a specific permission bit
if auth.check(3) {
    // bit 3 is set
}
```

Both structs expire 500 seconds after creation. `iat` and `exp` are in milliseconds since the Unix epoch.

### Sign & Validate Traits

The library is built around two traits that allow any compatible signer to be passed as a trait object:

```rust
pub trait Sign<T>:     Send + Sync + 'static { fn sign(&self, claims: &T) -> Result<String>; }
pub trait Validate<T>: Send + Sync + 'static { fn validate(&self, token: &str) -> Result<T>; }
```

Register a `Arc<dyn Validate<T>>` as app data to enable `Auth<T>` extraction across your handlers.

### Signers: HS256 and RS256

#### HS256 (symmetric — single service)

```rust
use actixutils::{HS256Signer, Sign, Validate, Identity};

let signer = HS256Signer::new("my-app".to_string(), "secret".to_string());

let token:    String   = signer.sign(&claims)?;
let identity: Identity = signer.validate(&token)?;
```

#### RS256 (asymmetric — split auth/resource)

```rust
use actixutils::{RS256Signer, RS256Validator, Sign, Validate};

// Auth service (holds private key)
let signer    = RS256Signer::new(private_key_pem, "my-app".to_string());
let token     = signer.sign(&claims)?;

// Resource service (holds public key only)
let validator = RS256Validator::new(public_key_pem, "my-app".to_string());
let claims    = validator.validate(&token)?;
```

---

## Request Extractors

### Auth\<T\>

`Auth<T>` is an Actix-web `FromRequest` extractor. Add it as a handler argument and the token is validated automatically.

**Priority:** if `T` is already in the request extensions (set by the Auth middleware), the stored value is returned directly without re-validating the token.

```rust
use actixutils::{Auth, Identity};
use actix_web::HttpResponse;

async fn protected(auth: Auth<Identity>) -> HttpResponse {
    let user_id = auth.0.sub;
    HttpResponse::Ok().json(user_id)
}
```

Register the validator in app state:

```rust
use std::sync::Arc;
use actixutils::{HS256Signer, Validate, Identity};
use actix_web::web;

let v: Arc<dyn Validate<Identity>> =
    Arc::new(HS256Signer::new("svc".to_string(), "secret".to_string()));

App::new().app_data(web::Data::from(v))
```

### Access

`Access` extracts the raw token string for manual validation. Useful when the algorithm or audience is not known until runtime.

Token is read from `Authorization: Bearer <token>` or the `access_token` cookie.

```rust
use actixutils::Access;
use actix_web::HttpResponse;

async fn flexible_auth(access: Access) -> HttpResponse {
    let identity = access.validate_hmac("secret", "my-app".to_string())?;
    // or:
    let identity = access.validate_rsa(&pem, "my-app".to_string())?;
    HttpResponse::Ok().json(identity)
}
```

### Session\<T\>

`Session<T>` reads the `session_id` cookie and delegates to an `Arc<dyn SessionStore<T>>` in app data.

```rust
use actixutils::{Session, SessionStore};
use actix_web::HttpResponse;

// 1. Implement SessionStore for your backing type
struct MyStore;
impl SessionStore<MyUser> for MyStore {
    fn get(&self, session_id: &str) -> Option<MyUser> { /* … */ }
}

// 2. Register it
App::new().app_data(web::Data::from(
    Arc::new(MyStore) as Arc<dyn SessionStore<MyUser>>
));

// 3. Use it
async fn handler(session: Session<MyUser>) -> HttpResponse {
    HttpResponse::Ok().json(&session.0)
}
```

---

## Middleware

### Auth Middleware

Protects an entire scope rather than individual routes. Validates the token once and stores the claims in request extensions.

```rust
use actixutils::middleware::Auth;
use actixutils::{HS256Signer, Identity, Validate};
use actix_web::{web, App};
use std::sync::Arc;

let validator: Arc<dyn Validate<Identity>> =
    Arc::new(HS256Signer::new("svc".to_string(), "secret".to_string()));

App::new().service(
    web::scope("/api")
        .wrap(Auth { validator })
        .route("/me", web::get().to(me_handler))
);
```

### RequestId

Generates a `UUIDv4` per request, stores it as `RequestIdStr` in extensions, records it in the active `tracing` span, and appends `X-Request-Id` to the response.

**Required** by the `ReadContext` middleware.

```rust
use actixutils::middleware::RequestId;

App::new().wrap(RequestId);

// In a handler:
use actixutils::middleware::RequestIdStr;
use actix_web::{HttpRequest, HttpMessage};

async fn handler(req: HttpRequest) -> HttpResponse {
    let rid = req.extensions().get::<RequestIdStr>().unwrap();
    println!("request: {}", rid.0);
    HttpResponse::Ok().finish()
}
```

### RateLimiter

Sliding-window, per-identity rate limiting backed by an in-memory `DashMap`.

**Step 1:** implement `GetId` for your extractor type.

```rust
use actixutils::{Auth, Identity};
use actixutils::middleware::rate_limiter::GetId;
use uuid::Uuid;

impl GetId for Auth<Identity> {
    type Id = Uuid;
    fn id(&self) -> Uuid { self.0.sub }
}
```

**Step 2:** wrap your scope.

```rust
use actixutils::middleware::RateLimiter;
use std::time::Duration;

App::new().service(
    web::scope("/api")
        .wrap(RateLimiter::<Auth<Identity>>::new(100, Duration::from_secs(60)))
);
```

Returns `429 Too Many Requests` when the limit is exceeded.

### Idempotency

Prevents duplicate mutations by caching responses against a client-supplied `Idempotency-Key` header.

You must provide a concrete `IdempotencyStore` implementation (backed by Redis, a `DashMap`, a database, etc.):

```rust
use actixutils::middleware::{Idempotency, IdempotencyStore, IdempotencyState, CachedResponse};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

struct MyStore;

#[async_trait]
impl IdempotencyStore for MyStore {
    type Error = anyhow::Error;
    async fn acquire(&self, key: &str, ttl: Duration) -> Result<bool, Self::Error> { Ok(true) }
    async fn get(&self, key: &str)  -> Result<Option<IdempotencyState>, Self::Error> { Ok(None) }
    async fn complete(&self, key: &str, r: CachedResponse) -> Result<(), Self::Error> { Ok(()) }
    async fn release(&self, key: &str) -> Result<(), Self::Error> { Ok(()) }
}

App::new().service(
    web::scope("/payments")
        .wrap(
            Idempotency::new(Arc::new(MyStore))
                .ttl(Duration::from_secs(86400))   // optional: default 1 hour
                .header("Idempotency-Key")          // optional: this is the default
        )
);
```

Responses for keys that are already being processed return `409 Conflict`.

### ResponseEqualizer

Pads response time to a minimum duration to mitigate timing side-channels on auth and lookup endpoints.

```rust
use actixutils::middleware::ResponseEqualizer;
use std::time::Duration;

App::new().service(
    web::scope("/auth")
        // Always take at least 150 ms; add up to 50 ms of random jitter
        .wrap(ResponseEqualizer::with_jitter(
            Duration::from_millis(150),
            Duration::from_millis(50),
        ))
);
```

Use `ResponseEqualizer::new(duration)` for a fixed floor without jitter.

### Pagination

Parses `?page=<u32>&limit=<u32>` once per request and stores the result in a Tokio task-local, making it readable anywhere in the call stack.

```rust
use actixutils::middleware::{Pagination, PaginationMiddleware};

App::new().service(
    web::scope("/items")
        .wrap(PaginationMiddleware)
        .route("", web::get().to(list))
);

async fn list() -> HttpResponse {
    let p = Pagination::get(); // page: 0, limit: 100 by default
    // SELECT … LIMIT {p.limit} OFFSET {p.page * p.limit}
    HttpResponse::Ok().finish()
}
```

Missing parameters default to `page = 0`, `limit = 100`.

### ReadContext

Builds a per-request [`Context`] that bundles the request ID, authenticated user ID, an `EventStream` handle, and a producer name for emitting domain events.

**Prerequisites:** `RequestId` and an auth middleware storing a `T: GetId` must run first.

```rust
use actixutils::middleware::{RequestId, ReadContext, Context};
use actix_web::{web, App, HttpRequest, HttpMessage, HttpResponse};
use std::sync::Arc;

App::new()
    .wrap(RequestId)
    .wrap(auth_middleware)
    .wrap(ReadContext::<Authority>::new(typed_eventbus.clone(), "my-service".to_string()));

async fn handler(req: HttpRequest) -> HttpResponse {
    if let Some(ctx) = req.extensions().get::<Context>() {
        ctx.publish(MyEvent { /* … */ }).await;
    }
    HttpResponse::Ok().finish()
}
```

### Helper Functions: identity & authority

For use with Actix-web's `from_fn` / `wrap_fn` API. These are simpler alternatives to the full middleware when you just need to guard a scope.

```rust
use actixutils::middleware::{identity, authority};
use actix_web::{web, middleware::from_fn};

web::scope("/api")
    .wrap(from_fn(identity))       // require valid Identity token
    .route("/public", web::get().to(public_handler));

web::scope("/admin")
    .wrap(from_fn(authority(3)))   // require permission bit 3 set
    .route("/users", web::get().to(admin_handler));
```

---

## Public Key Utilities

### Serve a public key endpoint

```rust
use actixutils::pubkey;

// Reads `validate.key` env var and exposes GET /.well-known/public-key.pem
App::new().configure(pubkey::configure);
```

### Fetch a remote public key

```rust
use actixutils::utils::remote_public_key;

// Reads REMOTE_PUBLIC_KEY env var and fetches the PEM via HTTP
let pem = remote_public_key().await;
```

---

## Project Structure

```
actixutils/
├── src/
│   ├── lib.rs                       # Crate root, public re-exports
│   ├── auth.rs                      # Auth<T> extractor
│   ├── common.rs                    # Identity & Authority claim structs
│   ├── context.rs                   # (unused internal draft)
│   ├── headers.rs                   # Access extractor
│   ├── hs256.rs                     # HS256Signer
│   ├── provider.rs                  # Provider<T> trait
│   ├── pubkey.rs                    # Public key route + remote fetch
│   ├── rs256.rs                     # RS256Signer / RS256Validator
│   ├── session.rs                   # Session<T> extractor + SessionStore
│   ├── signer_core.rs               # Sign<T> + Validate<T> traits
│   ├── utils.rs                     # remote_public_key()
│   └── middleware/
│       ├── mod.rs                   # Middleware re-exports
│       ├── auth.rs                  # Auth<T> middleware
│       ├── constant_time.rs         # ResponseEqualizer
│       ├── context.rs               # ReadContext<T> + Context + GetId
│       ├── fns.rs                   # identity() + authority() helpers
│       ├── idempotency.rs           # Idempotency<S> + IdempotencyStore
│       ├── pagination.rs            # PaginationMiddleware + Pagination
│       ├── rate_limiter.rs          # RateLimiter<T> + GetId
│       └── request_id.rs            # RequestId + RequestIdStr
├── Cargo.toml
└── README.md
```

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `actix-web` | 4.12.1 | Web framework |
| `jsonwebtoken` | 9.3.1 | JWT signing and validation |
| `tokio` | 1.52.3 | Async runtime |
| `chrono` | 0.4.44 | UTC timestamps for token issuance |
| `uuid` | 1.23.1 | UUIDs for request IDs and subjects |
| `dashmap` | 6.2.1 | Concurrent hash map for rate limiter store |
| `futures-util` | 0.3.32 | `LocalBoxFuture`, `Ready`, etc. |
| `serde` | 1.0.228 | Serialisation for JWT claims |
| `tracing` | 0.1.44 | Structured logging |
| `anyhow` | 1.0.102 | Error handling |
| `rand` | 0.10.1 | Jitter generation in `ResponseEqualizer` |
| `reqwest` | 0.13.2 | HTTP client for `remote_public_key` |
| `async-trait` | 0.1.89 | `#[async_trait]` for `IdempotencyStore` |
| `bytes` | 1.11.1 | Response body buffering in `Idempotency` |
| `typed-eventbus` | path | Domain event publishing (workspace crate) |

---

## Complete Example

```rust
use actixutils::{HS256Signer, Identity, Authority, Auth};
use actixutils::middleware::{RequestId, RateLimiter, ResponseEqualizer, PaginationMiddleware, Pagination};
use actixutils::middleware::rate_limiter::GetId;
use actix_web::{web, App, HttpServer, HttpResponse};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

impl GetId for Auth<Identity> {
    type Id = Uuid;
    fn id(&self) -> Uuid { self.0.sub }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let signer: Arc<dyn actixutils::Validate<Identity>> = Arc::new(
        HS256Signer::new("my-app".to_string(), "secret".to_string())
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::from(signer.clone()))
            .wrap(RequestId)
            .service(
                web::scope("/api")
                    .wrap(RateLimiter::<Auth<Identity>>::new(100, Duration::from_secs(60)))
                    .wrap(PaginationMiddleware)
                    .route("/items", web::get().to(list_items))
            )
            .service(
                web::scope("/auth")
                    .wrap(ResponseEqualizer::with_jitter(
                        Duration::from_millis(150),
                        Duration::from_millis(50),
                    ))
                    .route("/login", web::post().to(login))
            )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn login() -> HttpResponse {
    HttpResponse::Ok().body("token")
}

async fn list_items(auth: Auth<Identity>) -> HttpResponse {
    let p = Pagination::get();
    HttpResponse::Ok().json(serde_json::json!({
        "user": auth.0.sub,
        "page": p.page,
        "limit": p.limit,
    }))
}
```

---

## Security Considerations

| Concern | Mitigation |
|---|---|
| Timing attacks on auth endpoints | Wrap with `ResponseEqualizer::with_jitter` |
| Brute-force / credential stuffing | Wrap with `RateLimiter` |
| Duplicate POST mutations (payment, order) | Wrap with `Idempotency` |
| Short-lived tokens | Default expiry is 500 s; adjust `Identity::new` / `Authority::new` |
| Algorithm confusion | `HS256Signer` and `RS256Validator` each lock the allowed algorithm |
| Secret leakage | Never log `HS256Signer::secret`; rotate RS256 keys via `pubkey::configure` |

---

## License

Not currently licensed. See repository for details.

---

**Repository**: https://github.com/Ferrumec/actixutils  
**Language**: Rust  
**Current Version**: 0.1.0  
**Edition**: 2024
