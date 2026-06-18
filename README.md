# Actixutils

A comprehensive authentication, session management, and middleware utilities library for Actix-web applications. This package provides battle-tested components for JWT signing/validation, rate limiting, session management, and request authentication.

## Overview

Actixutils is a Rust library that simplifies building secure and scalable web applications with Actix-web. It provides:

- **JWT Authentication** - Support for both HMAC (HS256) and RSA (RS256) algorithms
- **Session Management** - Pluggable session storage with cookie-based retrieval
- **Rate Limiting** - Flexible middleware for rate limiting based on custom identities
- **Authorization** - Role-based permission checking
- **Constant-Time Response** - Protection against timing attacks
- **Token Validation** - Pluggable validators for authentication handlers

## Core Features

### JWT Support

#### HMAC (HS256)
```rust
use actixutils::HS256Signer;

let signer = HS256Signer::new("my-app".to_string(), "secret-key".to_string());

// Sign claims
let token = signer.sign(&claims)?;

// Validate token
let identity: Identity = signer.validate(&token)?;
```

#### RSA (RS256)
```rust
use actixutils::{RS256Signer, RS256Validator};

let signer = RS256Signer::new(private_key, "my-app".to_string());
let validator = RS256Validator::new(public_key, "my-app".to_string());

// Sign and validate
let token = signer.sign(&claims)?;
let identity: Identity = validator.validate(&token)?;
```

### Authentication & Authorization

#### Identity & Authority Models

**Identity** - Basic JWT claims:
```rust
pub struct Identity {
    pub aud: Vec<String>,      // Audience
    pub iat: usize,            // Issued at
    pub exp: usize,            // Expiration
    pub sub: Uuid,             // Subject (user ID)
}
```

**Authority** - Extended claims with role-based access:
```rust
pub struct Authority {
    pub aud: Vec<String>,      // Audience
    pub iat: usize,            // Issued at
    pub exp: usize,            // Expiration
    pub sub: Uuid,             // Subject (user ID)
    pub role: u128,            // Role bitmask for permissions
    pub rcpt: Uuid,            // Recipient ID
}
```

#### Permission Checking

```rust
let authority = Authority::new(user_id, role_bitmask, recipient_id, audiences);

// Check if user has permission
if authority.check(permission_id) {
    // User has permission
}
```

### Request Extractors

#### Auth Extractor

Extract and validate JWT tokens from `Authorization: Bearer <token>` header:

```rust
use actixutils::Auth;
use actix_web::{web, HttpResponse};

async fn protected_route(auth: Auth<Identity>) -> HttpResponse {
    // auth.0 contains the validated Identity
    HttpResponse::Ok().json(&auth.0)
}
```

Setup in app configuration:
```rust
use actix_web::web;
use std::sync::Arc;
use actixutils::{HS256Signer, Auth};

let signer = Arc::new(HS256Signer::new("my-app".to_string(), "secret".to_string()));
web::scope("/api")
    .app_data(web::Data::from(signer.clone()))
    .route("/protected", web::get().to(protected_route))
```

#### Session Extractor

Extract sessions from cookies:

```rust
use actixutils::Session;
use actix_web::HttpResponse;

pub trait SessionStore<T>: Send + Sync {
    fn get(&self, session_id: &str) -> Option<T>;
}

async fn with_session(session: Session<UserSession>) -> HttpResponse {
    // session.0 contains the session data
    HttpResponse::Ok().json(&session.0)
}
```

#### Access Extractor

Extract tokens from headers or cookies with flexible validation:

```rust
use actixutils::Access;
use actix_web::HttpResponse;

async fn flexible_auth(access: Access) -> HttpResponse {
    // Manually validate with HMAC
    let identity = access.validate_hmac("secret", "my-app".to_string())?;
    
    // Or validate with RSA
    let identity = access.validate_rsa(&public_key, "my-app".to_string())?;
    
    HttpResponse::Ok().json(&identity)
}
```

### Middleware

#### Rate Limiter

Flexible rate limiting middleware that tracks requests per identity:

```rust
use actixutils::middleware::RateLimiter;
use std::time::Duration;
use actix_web::web;

// Implement GetId for your identity type
impl GetId for Identity {
    type Id = Uuid;
    fn id(&self) -> Self::Id {
        self.sub
    }
}

// Setup middleware
let rate_limiter = RateLimiter::new(
    100,                           // max requests
    Duration::from_secs(60),       // time window
);

web::scope("/api")
    .wrap(rate_limiter)
    .route("/endpoint", web::get().to(handler))
```

Features:
- Per-identity tracking using `GetId` trait
- Configurable request limits and time windows
- Automatic cleanup of expired requests
- Non-blocking async implementation

#### Constant-Time Response Equalizer

Protection against timing attacks by normalizing response times:

```rust
use actixutils::middleware::ResponseEqualizer;
use std::time::Duration;

let equalizer = ResponseEqualizer::new(Duration::from_millis(100));

web::scope("/api")
    .wrap(equalizer)
    .route("/endpoint", web::get().to(handler))
```

#### Auth Middleware

Custom middleware for request authentication and authorization.

### Helper Functions

```rust
use actixutils::middleware::{identity, authority};

// Extractor for Identity from requests
let id = identity(req);

// Extractor for Authority from requests
let auth = authority(req);
```

## Project Structure

```
actixutils/
├── src/
│   ├── lib.rs                    # Main module exports
│   ├── auth.rs                   # Auth extractor
│   ├── common.rs                 # Identity & Authority models
│   ├── headers.rs                # Access extractor
│   ├── session.rs                # Session management
│   ├── hs256.rs                  # HMAC JWT implementation
│   ├── rs256.rs                  # RSA JWT implementation
│   ├── pubkey.rs                 # Public key utilities
│   ├── signer_core.rs            # Sign & Validate traits
│   ├── provider.rs               # Provider trait
│   └── middleware/               # Middleware implementations
│       ├── mod.rs
│       ├── auth.rs               # Auth middleware
│       ├── rate_limiter.rs       # Rate limiting
│       ├── constant_time.rs      # Timing attack protection
│       └── fns.rs                # Helper functions
├── Cargo.toml
└── README.md
```

## Dependencies

- **actix-web** (4.12.1) - Web framework
- **jsonwebtoken** (9.3.1) - JWT signing and validation
- **tokio** (1.52.3) - Async runtime
- **chrono** (0.4.44) - Time handling
- **uuid** (1.23.1) - Unique identifiers
- **dashmap** (6.2.1) - Concurrent hash map
- **futures-util** (0.3.32) - Async utilities
- **serde/serde_json** - Serialization
- **tracing** (0.1.44) - Structured logging
- **anyhow** (1.0.102) - Error handling
- **rand** (0.10.1) - Random number generation
- **reqwest** (0.13.2) - HTTP client

## Complete Example

```rust
use actixutils::{HS256Signer, Identity, Auth};
use actix_web::{web, App, HttpServer, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let signer = Arc::new(HS256Signer::new(
        "my-app".to_string(),
        "super-secret-key".to_string(),
    ));

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::from(signer.clone()))
            .route("/login", web::post().to(login))
            .route("/protected", web::get().to(protected))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn login() -> HttpResponse {
    let identity = Identity::new(
        Uuid::new_v4(),
        vec!["my-app".to_string()],
    );
    // Sign and return token
    HttpResponse::Ok().body("token")
}

async fn protected(auth: Auth<Identity>) -> HttpResponse {
    HttpResponse::Ok().json(&auth.0)
}
```

## Error Handling

The library uses `anyhow::Result` for error handling. Common error scenarios:

- Invalid JWT format or signature
- Expired tokens
- Missing or invalid authorization headers
- Rate limit exceeded (returns 429 Too Many Requests)
- Invalid session cookie

## Security Considerations

- **Timing Attacks**: Use `ResponseEqualizer` middleware for sensitive endpoints
- **Rate Limiting**: Implement per-user rate limiting to prevent brute force attacks
- **Token Expiration**: Tokens expire 500 seconds after issuance by default
- **Algorithm Verification**: Always validate JWT algorithm matches expected algorithm

## Performance

- **Async/await**: All operations are non-blocking
- **Concurrent Storage**: DashMap provides lock-free concurrent access for rate limiting
- **Token Caching**: Consider caching validated tokens to reduce signature verification overhead

## Development

### Prerequisites
- Rust 1.70+
- Tokio runtime

### Building
```bash
cargo build
```

### Testing
```bash
cargo test
```

### Documentation
```bash
cargo doc --open
```

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## License

Not currently licensed. See repository for details.

---

**Repository**: https://github.com/Austin-rgb/actixutils  
**Language**: Rust  
**Current Version**: 0.1.0  
**Edition**: 2024
