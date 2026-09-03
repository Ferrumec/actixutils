# actixutils

Reusable middleware, extractors, and framework-agnostic building blocks for
[Actix-web](https://actix.rs/) applications: JWT authentication, cookie
sessions, rate limiting, idempotency, GET-response caching, request
coalescing, pagination, request IDs, per-request timeouts, trusted-proxy
client-IP resolution, bitmask route permissions, timing-attack mitigation,
and typed-eventbus context propagation.

Note that this project is still a work in progress and is still going
through changes rapidly. We deeply welcome ideas for feature additions and
optimizations.

See [CHANGELOG.md](/CHANGELOG.md) for release history.

This README documents the crate as it currently exists in `src/`.

[Documentation](https://ferrumec.github.io/actixutils/)

## Crate layout

The crate is split into three top-level modules, by whether an item depends on
`actix-web`:

| Module | Contains |
|---|---|
| `extractors` | Types implementing `FromRequest`: `Jwt<T>`, `Session<T>`, `Filters`, `ClientIp` |
| `middleware` | Types implementing `Transform`: the full middleware suite, including `SessionMiddleware` |
| `locals` | Framework-agnostic pieces: claim structs, signing/validation traits, store traits, task-local state |

Plus a standalone `pubkey` module (see [JWT authentication](#jwt-authentication-jwt-feature) below).

The most commonly used `extractors` and `locals` items are re-exported at the crate
root, so `actixutils::Jwt`, `actixutils::Identity`, `actixutils::Session`, etc. work
without a submodule path.

## Feature flags

| Flag | Enables |
|---|---|
| `jwt` | JWT support: the `Jwt<T>` extractor, `middleware::Auth`, `HS256Signer`, `RS256Signer`/`RS256Validator`, the `identity`/`authority` helper functions |
| `es` | Event-stream context propagation: `locals::Context`, `middleware::{Context, ReadContext}` (requires `typed-eventbus`) |

Neither is enabled by default — enable whichever your application needs in
`Cargo.toml`. Everything else described below (extractors, sessions, rate
limiting, idempotency, caching, coalescing, timeouts, client IP, pagination,
permissions, request IDs) is available without any feature flag.

## Quick start

```rust,no_run
use actixutils::{HS256Signer, Identity, Jwt as Auth};
use actix_web::{web, App, HttpServer, HttpResponse};
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let signer = Arc::new(HS256Signer::new(
        "my-app".to_string(),
        "super-secret-key".to_string(),
    ));

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::from(signer.clone() as Arc<dyn actixutils::Validate<Identity>>))
            .route("/protected", web::get().to(protected))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn protected(auth: Auth<Identity>) -> HttpResponse {
    HttpResponse::Ok().json(&auth.0)
}
```

## JWT authentication (`jwt` feature)

Two independent ways to require a valid JWT, sharing the same signer/validator:

- **`extractors::Jwt<T>`** — validates per-handler. Add it as a handler argument;
  if `T` isn't already in the request extensions, it reads the bearer token from the
  `Authorization` header (falling back to an `access_token` cookie) and validates it
  via an `Arc<dyn Validate<T>>` registered in app data.
- **`middleware::Auth<T>`** — validates once per request via `.wrap(...)` and stores
  the claims in the request extensions for every downstream handler/middleware. Same
  token sources as the extractor. If claims are already present (e.g. from an outer
  layer), validation is skipped.

Signers/validators:

- **`HS256Signer`** — symmetric HMAC-SHA-256. Implements both `Sign<T>` and
  `Validate<T>`, so one instance can issue and verify its own tokens.
- **`RS256Signer`** / **`RS256Validator`** — asymmetric RSA-SHA-256. An auth service
  holds the private key (`RS256Signer`) and signs; downstream services hold only the
  public key (`RS256Validator`) and verify.

Claim structs (`locals`):

- **`Identity`** — minimal claims: `sub`, `aud`, `iat`, `exp`. 500-second expiry from
  creation.
- **`Authority`** — adds `role` (a `u128` permission bitmask) and `rcpt` (a target
  resource/tenant UUID). Check a permission bit with `Authority::check(perm_id)`.

`middleware::{identity, authority}` are `Next`-style functions for
`actix_web::middleware::from_fn`, offering the same checks without a struct-based
middleware.

`pubkey::configure` serves an RSA public key at `GET /.well-known/public-key.pem`,
read from the `validate.key` environment variable — handy for RS256 downstream
services that need to fetch the issuing service's public key.

## The `Store<K, V>` trait

Several pieces of the crate need a generic, async, get/set/delete/clear
key-value backend, and all of them share the same trait:
`locals::Store<K, V>`. You implement it once per backend (in-memory, Redis,
a database table, ...) and reuse it for:

- **`middleware::RateLimiter<T>`** — `Store<T::Id, VecDeque<Instant>>`
- **`middleware::Cache`** — `Store<String, cache::CachedResponse>`
- **`extractors::Session<T>` / `middleware::SessionMiddleware<T>`** — `Store<Uuid, T>`

`actixutils` does not ship a first-party in-memory implementation of
`Store` — you supply one (a `HashMap` behind a lock is enough for a single
process; see the store implementation in `src/middleware/test_session.rs`
for a minimal reference). This is a separate, more general trait from
`locals::IdempotencyStore` and `middleware::cache::CacheStore`, which are
TTL-aware trait families used only by `Idempotency`, as documented below.

## Sessions

Cookie-based, server-side sessions are split across two modules:

- **`extractors::Session<T>`** (re-exported as `actixutils::Session`) — a
  `FromRequest` handle to the current request's session value. `read()`/`write()`
  return async `RwLock` guards; any `write()` marks the session dirty.
- **`middleware::SessionMiddleware<T>`** — resolves the session cookie (default name
  `"session"`, configurable via `.cookie_name(...)`), loads/saves through a
  caller-supplied `Arc<dyn locals::Store<Uuid, T>>`, and persists dirty sessions back
  after the handler runs. `SessionMiddleware::new` falls back to a fresh default
  session on a missing/invalid cookie (and issues a new cookie on the response);
  `SessionMiddleware::required` instead rejects the request with `401 Unauthorized`.

There is no separate, session-specific store trait — `SessionMiddleware<T>` is
backed directly by the general-purpose `locals::Store<Uuid, T>` described above, so
any `Store` implementation you already have for rate limiting or caching can back
sessions too (with a different `T`).

## Middleware suite

| Middleware | What it does |
|---|---|
| `Auth<T>` | Validates a Bearer JWT (header or `access_token` cookie) and stores claims in request extensions *(feature `jwt`)* |
| `ResponseEqualizer` | Pads every response to a minimum duration (optionally plus random jitter), mitigating timing side-channels on auth/lookup endpoints |
| `RateLimiter<T>` | Sliding-window per-identity rate limiting; keys on any extractor implementing `locals::rate_limiter::GetId`; backed by a caller-supplied `Store` |
| `Idempotency<Store>` | Caches responses by an `Idempotency-Key` header to prevent duplicate mutations on retried requests; pluggable `IdempotencyStore` |
| `Cache` | GET-only HTTP response caching, keyed on `host + path + query`; backed by a caller-supplied `Store` |
| `Singleflight<K, KeyFn>` | Request coalescing: concurrent requests that map to the same key share a single execution of the wrapped service |
| `TimeoutMiddleware` | Fails a request with `504 Gateway Timeout` if it exceeds a fixed duration |
| `ClientIpMiddleware` | Resolves the real client IP from `X-Forwarded-For`, honouring a configured set of trusted proxy networks; exposed via the `ClientIp` extractor |
| `PathParams` | Merges matched path parameters into `Filters`, overlaying them on the query string |
| `RequestId` / `RequestIdStr` | Generates a UUIDv4 per request, records it in the tracing span, stores it in extensions, and returns it as `X-Request-Id` |
| `Context` / `ReadContext<T>` (feature `es`) | Builds a per-request typed-eventbus publishing context from the request ID and an identity's UUID |
| `Pagination` / `PaginationMiddleware` | Parses `?page=&limit=` into a task-local, readable anywhere via `Pagination::get()` without threading it through function signatures |
| `SessionMiddleware<T>` | Cookie-based server-side sessions (see above) |
| `AttachLocal<T>` / `SetLocal` | Generic helper: extracts a `T` up front, then runs the rest of the request inside `T::scope(...)` — the mechanism `PaginationMiddleware` is built on |
| `Permissions<P>` (submodule `permission`) | Route-level, `u128`-bitmask RBAC keyed on `(HTTP method, path)`, matched with Actix's native `ResourceDef` syntax |

See [docs/middleware](docs/middleware/index.md) for a design overview, ordering
guidance, tutorials, and focused examples for each middleware.

## Testing

`middleware::test_session` (compiled only under `#[cfg(test)]`) contains an
in-memory `locals::Store<Uuid, T>` implementation and integration tests exercising
`Session<T>` / `SessionMiddleware` end to end — a useful reference for implementing
your own store.

## License

MIT (see `Cargo.toml`).
