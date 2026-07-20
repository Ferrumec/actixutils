# actixutils

`actixutils` is a collection of reusable middleware, extractors, authentication utilities, and framework components for building secure, production-ready APIs with Actix Web.

It eliminates repetitive infrastructure code by providing ready-to-use implementations for authentication, session management, rate limiting, pagination, request tracing, idempotency, and generic CRUD ViewSets.

## Features

- JWT authentication
  - HS256 (HMAC)
  - RS256 (RSA)
- Authentication extractors
- Session management
- Request context propagation
- Request ID middleware
- Rate limiting
- Idempotency protection
- Pagination utilities
- Constant-time response middleware
- Generic CRUD ViewSet framework (optional feature)
- Strongly typed request extractors

---

# Installation

```toml
[dependencies]
actixutils = "0.1.3"
```

Or from GitHub

```toml
actixutils = { git = "https://github.com/Ferrumec/actixutils" }
```

---

# Modules

## Extractors

Provides custom `FromRequest` implementations.

Included extractors include:

- `Auth<T>`
- `Access`
- `Session<T>`
- `Filters`

These make handler signatures clean and strongly typed.

Example

```rust
async fn profile(
    user: Auth<Identity>,
) -> impl Responder {
    HttpResponse::Ok().json(user.0)
}
```

---

## Middleware

Ready-to-use middleware includes:

- Authentication
- Request ID
- Rate Limiting
- Session Loading
- Pagination
- Request Context
- Constant-Time Responses
- Idempotency
- AttachLocal

Example

```rust
App::new()
    .wrap(RequestId::default())
    .wrap(AuthMiddleware::new(...))
```

---

## Locals

Framework-independent components.

Includes:

- JWT claim models
- Signing traits
- Validation traits
- Session store traits
- Task-local context providers
- Authentication providers

Main types:

- `Identity`
- `Authority`
- `Provider`
- `SessionStore`
- `Sign`
- `Validate`

---

## ViewSet (feature)

Enable with

```toml
actixutils = { version = "...", features = ["viewset"] }
```

Provides reusable abstractions for implementing CRUD APIs.

Components:

- Entity
- Repository
- Service
- ViewSet

Designed to minimize boilerplate by separating:

```
HTTP
    ↓
ViewSet
    ↓
Service
    ↓
Repository
    ↓
Database
```

---

# JWT Authentication

Supported algorithms:

- HS256
- RS256

Example

```rust
let signer = HS256Signer::new(
    "issuer".into(),
    "secret".into(),
);
```

Use with the `Auth<T>` extractor.

---

# Session Management

Session middleware can:

- Read session ID from cookies
- Load session data
- Make session available to handlers
- Persist session changes after the response

Example

```rust
async fn handler(
    session: Session<MySession>,
) {
    // access session data
}
```

---

# Pagination

Automatically extracts pagination parameters.

Typical query:

```
?page=1&pageSize=25
```

Available through pagination middleware and local context.

---

# Request Filters

The `Filters` extractor reads query parameters into a

```rust
HashMap<String, String>
```

Useful for dynamic filtering where a fixed struct would be too restrictive.

---

# Request Context

Provides request-scoped context using task-local storage.

Useful for propagating:

- authenticated user
- request metadata
- pagination
- tracing information

without repeatedly passing values through function arguments.

---

# Request ID

Automatically assigns every request a unique identifier for:

- tracing
- logging
- debugging

---

# Rate Limiting

Built-in middleware for limiting client request rates.

Can be backed by custom implementations through the provided traits.

---

# Idempotency

Provides middleware to safely handle retried requests using idempotency keys.

Useful for payment APIs and other non-repeatable operations.

---

# Public Key Endpoint

The crate can expose an RSA public key endpoint.

```
GET /.well-known/public-key.pem
```

This allows external services to verify JWT signatures.

---

# Example

```rust
use std::sync::Arc;

use actix_web::{web, App, HttpResponse, HttpServer};

use actixutils::{
    Auth,
    HS256Signer,
    Identity,
    Validate,
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let signer = Arc::new(
        HS256Signer::new(
            "example".into(),
            "super-secret".into(),
        )
    );

    HttpServer::new(move || {
        App::new()
            .app_data(
                web::Data::from(
                    signer.clone() as Arc<dyn Validate<Identity>>
                )
            )
            .route("/profile", web::get().to(profile))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}

async fn profile(
    auth: Auth<Identity>,
) -> HttpResponse {
    HttpResponse::Ok().json(auth.0)
}
```

---

# Project Structure

```
src/
 ├── extractors/
 ├── middleware/
 ├── locals/
 ├── viewset/
 ├── pubkey.rs
 └── lib.rs
```

---

# License

MIT License.
