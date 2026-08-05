# `middleware` — Overview

`actixutils::middleware` is a collection of independent, composable Actix-Web
middleware components. Each one solves a single cross-cutting HTTP concern —
authentication, rate limiting, sessions, pagination, request tracing,
authorization, idempotency, timing-attack mitigation — and they're designed to
be mixed and matched per route, per scope, or per app, rather than adopted as
an all-or-nothing framework.

## Design philosophy

Every middleware in this module follows the same shape: a lightweight
configuration struct (e.g. `RateLimiter`, `Auth<T>`) implements Actix's
`Transform` trait, which produces a per-worker service wrapper that does the
actual request interception. Where a piece of state needs to be shared beyond
the middleware itself (a store trait, a task-local snapshot, a plain data
struct), that piece lives in `crate::locals` and is re-exported from
`middleware` for convenience. This keeps the "how do I configure it" surface
(`middleware`) separate from the "what shape is the data" surface (`locals`).

Two related but distinct patterns show up repeatedly:

- **Extensions-based middleware** — inserts a value into
  `req.extensions_mut()` so downstream handlers can pull it out with
  `HttpMessage::extensions()` or a `FromRequest` extractor. Used by `Auth<T>`,
  `RequestId`, `Session<T>`, `Context`.
- **Task-local middleware** — scopes a value around the rest of the request
  future using a Tokio task-local, so it's reachable from *any* function in
  the call stack (e.g. a repository function three layers deep) without
  threading it through every signature. Used by `Pagination` and the generic
  `AttachLocal<T>`.

## Middleware catalog

| Middleware | Purpose | Backing state |
|---|---|---|
| `Auth<T>` | Validates a Bearer JWT (header or `access_token` cookie) once per request, stores claims of type `T` in request extensions | `Arc<dyn Validate<T>>` you supply |
| `ResponseEqualizer` | Pads every response to a minimum duration (+ optional jitter) to blunt timing-attack information leaks | none (stateless) |
| `RateLimiter<T>` | Sliding-window, per-identity request limiting; returns `429` when exceeded | in-memory `DashMap`, keyed by `T::Id` |
| `Idempotency<Store>` | Caches responses by an `Idempotency-Key` header so retried mutations aren't re-executed | `Arc<Store: IdempotencyStore>` you supply |
| `RequestId` | Generates a UUIDv4 per request, records it on the tracing span, adds `X-Request-Id` to the response | none — writes `RequestIdStr` to extensions |
| `Context` / `ReadContext<T>` | Builds a per-request event-publishing context (request id + user id + event bus handle) | `Arc<dyn EventStream>` you supply |
| `Pagination` / `PaginationMiddleware` | Parses `?page=&limit=` and exposes it via a task-local | Tokio task-local (`PAGINATION`) |
| `Session<T>` / `SessionMiddleware<Store>` | Cookie-based, server-side session storage with dirty-tracking | `Arc<Store: SessionStore>` you supply |
| `AttachLocal<T>` / `SetLocal` | Generic building block: extracts a `T` and scopes the rest of the request inside `T::scope(...)` | whatever `T::scope` scopes |
| `identity` / `authority(bit)` | `Next`-style functions (for `wrap_fn`/`from_fn`) that gate on a valid `Identity`/`Authority` JWT | delegates to the `Jwt<T>` extractor |
| `Permissions<P>` (submodule `permission`) | Route-level, bitmask-based RBAC keyed on `(HTTP method, path)` | `Arc<PermissionSet>`, loaded from JSON or built in code |

## Module layout

```
middleware/
├── mod.rs             # re-exports, module-level docs, the table above
├── auth.rs             → Auth<T>
├── constant_time.rs     → ResponseEqualizer
├── rate_limiter.rs      → RateLimiter<T>
├── idempotency.rs       → Idempotency<Store>
├── request_id.rs        → RequestId, RequestIdStr
├── context.rs            → Context, ReadContext<T>   (feature = "es")
├── pagination.rs         → Pagination, PaginationMiddleware
├── session.rs             → Session<T>, SessionMiddleware<Store>
├── attach_local.rs        → AttachLocal<T>, SetLocal
├── fns.rs                  → identity, authority()   (feature = "jwt")
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
- `fns` requires the `jwt` feature (depends on the `Jwt<T>` extractor).

## Ordering matters

Because several middleware read state that another middleware wrote into
request extensions, registration order (outer → inner, i.e. the *last*
`.wrap()` call runs *first*) is significant:

1. **`RequestId`** should generally be outermost — `ReadContext<T>` requires
   `RequestIdStr` to already be present.
2. **An auth middleware** (`Auth<T>`, `SessionMiddleware`, or the `Jwt<T>`
   extractor via `identity`/`authority`) must run before anything that reads
   the resulting principal — `ReadContext<T>`, `Permissions<P>`,
   `RateLimiter<T>` when keyed on an authenticated identity, and
   `authority(bit)`.
3. **`ReadContext<T>`** depends on both (1) and (2) having already populated
   extensions.
4. **`Permissions<P>`** depends on a `Principal`-implementing type already
   being in extensions from step (2).

A typical stack, outermost first:

```text
RequestId → Auth<Identity> → ReadContext<Identity> → Permissions<Identity> → handler
```

## When to reach for which middleware

- Protecting an entire scope with JWT auth → `Auth<T>`.
- Mitigating username-enumeration / login timing attacks → `ResponseEqualizer`.
- Stopping abuse or enforcing quotas per user/IP → `RateLimiter<T>`.
- Making POST/PUT/PATCH safe to retry → `Idempotency<Store>`.
- Correlating logs/traces across services → `RequestId`.
- Publishing domain events with request/user context attached → `Context` /
  `ReadContext<T>`.
- List endpoints that need `?page=&limit=` without threading params through
  every layer → `Pagination` / `PaginationMiddleware`.
- Stateful, cookie-backed sessions (as opposed to stateless JWTs) →
  `Session<T>` / `SessionMiddleware<Store>`.
- You have your own task-local-like type and want the same
  extract-then-scope pattern `Pagination` uses → `AttachLocal<T>`.
- Fine-grained, per-route bitmask permissions independent of *how* the user
  was authenticated → `Permissions<P>` (the `permission` submodule).
- A single "does this bit exist" gate without a full `PermissionSet` →
  `authority(bit)` / `identity` from `fns.rs`.

See [TUTORIALS](TUTORIALS.md) for a walkthrough of assembling several of these into a
real app, and [EXAMPLES](EXAMPLES.md) for focused, copy-pasteable snippets for each
middleware.
