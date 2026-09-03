# `middleware` — Overview

`actixutils::middleware` is a collection of independent, composable Actix-Web
middleware components. Each one solves a single cross-cutting HTTP concern —
authentication, rate limiting, caching, request coalescing, sessions,
pagination, request tracing, authorization, idempotency, timeouts, client-IP
resolution, timing-attack mitigation — and they're designed to be mixed and
matched per route, per scope, or per app, rather than adopted as an
all-or-nothing framework.

## Design philosophy

Every middleware in this module follows the same shape: a lightweight
configuration struct (e.g. `RateLimiter`, `Auth<T>`) implements Actix's
`Transform` trait, which produces a per-worker service wrapper that does the
actual request interception. Where a piece of state needs to be shared beyond
the middleware itself (a store trait, a task-local snapshot, a plain data
struct), that piece lives in `crate::locals` and is re-exported from
`middleware` for convenience. This keeps the "how do I configure it" surface
(`middleware`) separate from the "what shape is the data" surface (`locals`).

The one deliberate exception is `Session<T>`: the extractor itself lives in
`crate::extractors` (and is re-exported at the crate root as
`actixutils::Session`), while its `Transform` counterpart,
`SessionMiddleware<T>`, lives here in `middleware`. Everything else that
handlers pull out of the request via `FromRequest` — `Jwt<T>`, `Filters`,
`ClientIp` — also lives in `extractors`; only their `Transform`-based
middleware counterparts (`Auth<T>`, `PathParams`, `ClientIpMiddleware`) live
here.

Two related but distinct patterns show up repeatedly:

- **Extensions-based middleware** — inserts a value into
  `req.extensions_mut()` so downstream handlers can pull it out with
  `HttpMessage::extensions()` or a `FromRequest` extractor. Used by `Auth<T>`,
  `RequestId`, `SessionMiddleware<T>`, `Context`, `ClientIpMiddleware`,
  `PathParams`.
- **Task-local middleware** — scopes a value around the rest of the request
  future using a Tokio task-local, so it's reachable from *any* function in
  the call stack (e.g. a repository function three layers deep) without
  threading it through every signature. Used by `Pagination` and the generic
  `AttachLocal<T>`.

## Middleware catalog

| Middleware | Purpose | Backing state |
|---|---|---|
| `Auth<T>` (feature `jwt`) | Validates a Bearer JWT (header or `access_token` cookie) once per request, stores claims of type `T` in request extensions | `Arc<dyn Validate<T>>` you supply |
| `ResponseEqualizer` | Pads every response to a minimum duration (+ optional jitter) to blunt timing-attack information leaks | none (stateless) |
| `RateLimiter<T>` | Sliding-window, per-identity request limiting; returns `429` when exceeded | `Arc<dyn locals::Store<T::Id, VecDeque<Instant>>>` you supply |
| `Idempotency<Store>` | Caches responses by an `Idempotency-Key` header so retried mutations aren't re-executed | `Arc<Store: locals::IdempotencyStore>` you supply |
| `Cache` | Caches `GET` responses keyed on `host + path + query`; skips non-success statuses, `Set-Cookie`, and `no-store`/`private` responses | `Arc<dyn locals::Store<String, cache::CachedResponse>>` you supply |
| `Singleflight<K, KeyFn>` | Coalesces concurrent requests mapping to the same key into a single execution, cloning the result to all waiters | in-process `HashMap<K, Flight>` behind a mutex |
| `TimeoutMiddleware` | Races the wrapped service against a fixed `Duration`; returns `504 Gateway Timeout` on expiry | none (stateless) |
| `ClientIpMiddleware` | Resolves the real client IP, trusting `X-Forwarded-For` only from configured proxy networks | `locals::ProxyConfig` (trusted CIDR list) |
| `PathParams` | Merges matched path parameters on top of the query-string `Filters` map | writes `extractors::Filters` into extensions |
| `RequestId` | Generates a UUIDv4 per request, records it on the tracing span, adds `X-Request-Id` to the response | none — writes `RequestIdStr` to extensions |
| `Context` / `ReadContext<T>` (feature `es`) | Builds a per-request event-publishing context (request id + user id + event bus handle) | `Arc<dyn typed_eventbus::EventStream>` you supply |
| `Pagination` / `PaginationMiddleware` | Parses `?page=&limit=` and exposes it via a task-local | Tokio task-local (`PAGINATION`) |
| `SessionMiddleware<T>` | Cookie-based, server-side session storage with dirty-tracking; extractor is `extractors::Session<T>` | `Arc<dyn locals::Store<Uuid, T>>` you supply |
| `AttachLocal<T>` / `SetLocal` | Generic building block: extracts a `T` and scopes the rest of the request inside `T::scope(...)` | whatever `T::scope` scopes |
| `identity` / `authority(bit)` (feature `jwt`) | `Next`-style functions (for `wrap_fn`/`from_fn`) that gate on a valid `Identity`/`Authority` JWT | delegates to the `Jwt<T>` extractor |
| `Permissions<P>` (submodule `permission`) | Route-level, bitmask-based RBAC keyed on `(HTTP method, path)` | `PermissionSet`, loaded from JSON or built in code |

## Module layout

```
middleware/
├── mod.rs             # re-exports, module-level docs, the table above
├── auth.rs             → Auth<T>                    (feature = "jwt")
├── constant_time.rs     → ResponseEqualizer
├── rate_limiter.rs      → RateLimiter<T>
├── idempotency.rs        → Idempotency<Store>
├── cache/                → Cache, CacheMiddleware, CacheStore, CachedResponse
│   ├── mod.rs
│   ├── middleware.rs        # Cache transform + cache-key/cache-control logic
│   ├── store.rs              # CacheStore trait (a separate trait family from
│   │                          # locals::Store; Cache itself is backed by
│   │                          # locals::Store, see docs/index.md)
│   └── types.rs               → CachedResponse
├── coalesce.rs            → Singleflight<K, KeyFn>
├── timeout.rs              → TimeoutMiddleware
├── client_ip.rs             → ClientIpMiddleware   (extractor: extractors::ClientIp)
├── path_params.rs            → PathParams
├── request_id.rs              → RequestId, RequestIdStr
├── context.rs                  → Context, ReadContext<T>   (feature = "es")
├── pagination.rs                 → Pagination, PaginationMiddleware
├── session.rs                     → SessionMiddleware<T>   (extractor: extractors::Session<T>)
├── attach_local.rs                 → AttachLocal<T>, SetLocal
├── fns.rs                            → identity, authority()   (feature = "jwt")
└── permission/
    ├── mod.rs               # crate-level docs for the RBAC submodule
    ├── principal.rs         → Principal trait
    ├── permission.rs        → Permission, PermissionSet
    ├── middleware.rs        → Permissions<P>
    └── error.rs              → PermissionError
```

Two modules are feature-gated:

- `context` requires the `es` feature (event-stream / `typed-eventbus`
  integration).
- `fns` and `auth` require the `jwt` feature (`fns` depends on the `Jwt<T>`
  extractor; `auth` provides the `Auth<T>` transform).

> The `cache::store::CacheStore` trait is defined but currently unused by
> the `Cache` middleware itself — `Cache` is backed by the general-purpose
> `locals::Store<String, CachedResponse>` instead (see [docs/index.md](../index.md#the-storek-v-trait)).
> Don't implement `CacheStore` expecting `Cache::new` to accept it; implement
> `locals::Store` instead.

## Ordering matters

Because several middleware read state that another middleware wrote into
request extensions, registration order (outer → inner, i.e. the *last*
`.wrap()` call runs *first*) is significant:

1. **`RequestId`** should generally be outermost — `ReadContext<T>` requires
   `RequestIdStr` to already be present.
2. **`ClientIpMiddleware`** should run before anything that keys off
   `extractors::ClientIp` (e.g. a `RateLimiter<ClientIp>`), since that
   extractor errors out with `500` if the middleware never ran.
3. **An auth middleware** (`Auth<T>`, `SessionMiddleware`, or the `Jwt<T>`
   extractor via `identity`/`authority`) must run before anything that reads
   the resulting principal — `ReadContext<T>`, `Permissions<P>`,
   `RateLimiter<T>` when keyed on an authenticated identity, and
   `authority(bit)`.
4. **`ReadContext<T>`** depends on both (1) and (3) having already populated
   extensions.
5. **`Permissions<P>`** depends on a `Principal`-implementing type already
   being in extensions from step (3).
6. **`PathParams`** should run before any handler that reads `Filters` via
   `web::ReqData<Filters>` and expects path segments included, but has no
   dependency on the other middleware here.

A typical stack, outermost first:

```text
RequestId → ClientIpMiddleware → Auth<Identity> → ReadContext<Identity> → Permissions<Identity> → handler
```

## When to reach for which middleware

- Protecting an entire scope with JWT auth → `Auth<T>`.
- Mitigating username-enumeration / login timing attacks → `ResponseEqualizer`.
- Stopping abuse or enforcing quotas per user/IP → `RateLimiter<T>`.
- Making POST/PUT/PATCH safe to retry → `Idempotency<Store>`.
- Avoiding redundant work on cacheable `GET` endpoints → `Cache`.
- Preventing a thundering herd of identical concurrent requests from all
  hitting a slow backend → `Singleflight<K, KeyFn>`.
- Bounding how long a request is allowed to take → `TimeoutMiddleware`.
- Resolving the real client IP behind a trusted proxy/load balancer →
  `ClientIpMiddleware` (paired with the `ClientIp` extractor).
- Unifying path and query parameters into one map → `PathParams`.
- Correlating logs/traces across services → `RequestId`.
- Publishing domain events with request/user context attached → `Context` /
  `ReadContext<T>`.
- List endpoints that need `?page=&limit=` without threading params through
  every layer → `Pagination` / `PaginationMiddleware`.
- Stateful, cookie-backed sessions (as opposed to stateless JWTs) →
  `extractors::Session<T>` / `SessionMiddleware<T>`.
- You have your own task-local-like type and want the same
  extract-then-scope pattern `Pagination` uses → `AttachLocal<T>`.
- Fine-grained, per-route bitmask permissions independent of *how* the user
  was authenticated → `Permissions<P>` (the `permission` submodule).
- A single "does this bit exist" gate without a full `PermissionSet` →
  `authority(bit)` / `identity` from `fns.rs`.

See [TUTORIALS](TUTORIALS.md) for a walkthrough of assembling several of these into a
real app, and [EXAMPLES](EXAMPLES.md) for focused, copy-pasteable snippets for each
middleware.
