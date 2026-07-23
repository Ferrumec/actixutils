# actixutils

Reusable middleware, extractors, and framework-agnostic building blocks for
[Actix-web](https://actix.rs/) applications: JWT authentication, cookie sessions,
rate limiting, idempotency, pagination, request IDs, timing-attack mitigation,
typed-eventbus context propagation, and (behind the `viewset` feature) a small
Django-REST-Framework-inspired CRUD toolkit over `sqlx` + Postgres.

This README documents the crate as it currently exists in `src/`. It doesn't cover
work in progress that hasn't landed yet (e.g. `viewset`'s SQLite support).

## Crate layout

The crate is split into three top-level modules, by whether an item depends on
`actix-web`:

| Module | Contains |
|---|---|
| `extractors` | Types implementing `FromRequest`: `Jwt<T>`, `Filters` |
| `middleware` | Types implementing `Transform`: the full middleware suite, including the `Session<T>` extractor and its `SessionMiddleware` |
| `locals` | Framework-agnostic pieces: claim structs, signing/validation traits, store traits, task-local state |

Plus an optional fourth module:

| Module | Feature flag | Contains |
|---|---|---|
| `viewset` | `viewset` | Generic CRUD toolkit (ViewSet → Service → Repository → Postgres) |

The most commonly used `extractors` and `locals` items are re-exported at the crate
root, so `actixutils::Jwt`, `actixutils::Identity`, etc. work without a submodule path.

## Feature flags

| Flag | Enables |
|---|---|
| `jwt` | JWT support: the `Jwt<T>` extractor, `middleware::Auth`, `HS256Signer`, `RS256Signer`/`RS256Validator`, the `identity`/`authority` helper functions |
| `es` | Event-stream context propagation: `locals::Context`, `middleware::{Context, ReadContext}` (requires `typed-eventbus`) |
| `viewset` | The `viewset` module (requires `sqlx`, `serde_json`, `thiserror`, `rust_decimal`, `viewset-macros`) |

None of these are enabled by default — enable whichever your application needs in
`Cargo.toml`.

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

## Sessions

Cookie-based, server-side sessions live in `middleware`, **not** `extractors`:

- **`middleware::Session<T>`** — a `FromRequest` handle to the current request's
  session value. `read()`/`write()` return async `RwLock` guards; any `write()` marks
  the session dirty.
- **`middleware::SessionMiddleware<S>`** — resolves the session cookie (default name
  `"session"`, configurable via `.cookie_name(...)`), loads/saves through a
  caller-supplied async store, and persists dirty sessions back after the handler
  runs. `SessionMiddleware::new` falls back to a fresh default session on a
  missing/invalid cookie; `SessionMiddleware::required` instead rejects the request
  with `401 Unauthorized`.
- The backing store trait (`load`/`save`/`delete`) is defined locally inside
  `middleware::session` and implemented by the application.

> **Note:** `locals::SessionStore<T>` is a separate, synchronous session-store trait
> re-exported at the crate root. It is **not** wired into `middleware::Session` /
> `SessionMiddleware` — those use their own async trait. `locals::SessionStore<T>` is
> provided as a standalone building block if you want to roll your own session
> lookup outside the built-in middleware.

## Middleware suite

| Middleware | What it does |
|---|---|
| `Auth<T>` | Validates a Bearer JWT (header or `access_token` cookie) and stores claims in request extensions |
| `ResponseEqualizer` | Pads every response to a minimum duration (optionally plus random jitter), mitigating timing side-channels on auth/lookup endpoints |
| `RateLimiter<T>` | Sliding-window per-identity rate limiting; keys on any extractor implementing `locals::rate_limiter::GetId`; in-memory `DashMap` store, single-instance only |
| `Idempotency<S>` | Caches responses by an `Idempotency-Key` header to prevent duplicate mutations on retried requests; pluggable `IdempotencyStore` |
| `RequestId` / `RequestIdStr` | Generates a UUIDv4 per request, records it in the tracing span, stores it in extensions, and returns it as `X-Request-Id` |
| `Context` / `ReadContext<T>` (feature `es`) | Builds a per-request typed-eventbus publishing context from the request ID and an identity's UUID |
| `Pagination` / `PaginationMiddleware` | Parses `?page=&limit=` into a task-local, readable anywhere via `Pagination::get()` without threading it through function signatures |
| `Session<T>` / `SessionMiddleware` | Cookie-based server-side sessions (see above) |
| `AttachLocal<T>` / `SetLocal` | Generic helper: extracts a `T` up front, then runs the rest of the request inside `T::scope(...)` — the mechanism `PaginationMiddleware` is built on |

## The `viewset` module (feature `viewset`)

A small, Django-REST-Framework-inspired CRUD toolkit for building admin-style REST
APIs on `actix-web` + `sqlx` + **Postgres** (the current `Entity` trait is bound to
`FromRow<'r, PgRow>`; SQLite support is not present in this snapshot).

Request flow: `ViewSet → Service → Repository → Database`. Each layer is a trait with
default implementations built from `Entity` metadata, so a new resource typically
needs only a handful of `impl` blocks plus entity metadata (usually generated via
`#[derive(Entity)]` from the `viewset-macros` crate).

| Item | Role |
|---|---|
| `Entity` | Static metadata: table/PK/column names, `CreateDto`/`UpdateDto`/`ResponseDto`, searchable/sortable/filterable columns, optional soft-delete column |
| `Repository` | Database access only. Default methods build dynamic SQL from `Entity` metadata via `sqlx::QueryBuilder`; override any method for custom SQL |
| `Service` | Business logic layer. Default methods delegate to `Repository`; override `before_*`/`after_*` hooks for validation, auth checks, transactions, events, or caching |
| `ViewSet` | HTTP layer. `configure()` registers the standard list/retrieve/create/update/delete routes; override individual `handle_*` methods to customize |
| `RequestContext<U>` | Per-request bag of `db`, optional authenticated `user`, `permissions`, `tenant_id`, `request_id`, `trace_id`, `locale`. Applications implement `FromRequest` for their own `RequestContext<YourUser>` |
| `ApiError` / `ApiResult<T>` | Shared error enum implementing `ResponseError`, with `sqlx::Error` conversion |
| `SqlType` / `SqlValue` / `Field` | Typed column metadata so inserts/updates bind native Postgres types instead of everything going through `jsonb` |

## Testing

`middleware::test_session` (compiled only under `#[cfg(test)]`) contains an
in-memory `SessionStore` implementation and integration tests exercising
`Session<T>` / `SessionMiddleware` end to end — a useful reference for implementing
your own store.

## License

MIT (see `Cargo.toml`).
