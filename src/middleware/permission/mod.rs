//! # Actix-Web Permissions Middleware
//!
//! A reusable, authentication-backend-agnostic authorization middleware for Actix-Web.
//!
//! This middleware provides role-based access control using a `u128` bitset where each
//! bit represents a specific permission. It integrates cleanly with any authentication
//! mechanism that inserts a [`Principal`] into Actix's request extensions.
//!
//! ## Design Philosophy
//!
//! - **Authentication agnostic**: This middleware does not perform authentication.
//!   It expects a [`Principal`] to already exist in the request extensions, typically
//!   inserted by an upstream authentication middleware.
//! - **Default deny**: If no permission is configured for an endpoint, access is denied.
//! - **u128 bitset roles**: Permission bits are indexed `0..127` starting from the
//!   right-most bit (`1 << 0`).
//! - **Actix-native routing**: Permissions use Actix's [`ResourceDef`](actix_router::ResourceDef)
//!   for route matching, supporting dynamic segments, regex patterns, and all standard
//!   Actix route syntax.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use actix_web::{web, App, HttpResponse};
//! use actixutils_permissions::{PermissionSet, Permissions, Principal};
//!
//! #[derive(Clone)]
//! struct User {
//!     role: u128,
//! }
//!
//! impl Principal for User {
//!     fn role(&self) -> u128 {
//!         self.role
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load permissions from JSON configuration.
//! let permissions = PermissionSet::from_file("permissions.json")?;
//!
//! let app = App::new()
//!     // Authentication middleware runs first and inserts `User` into extensions.
//!     // .wrap(AuthenticationMiddleware::new(...))
//!     // Authorization middleware checks permissions.
//!     .wrap(Permissions::<User>::new(permissions))
//!     .route("/users", web::get().to(|| async { HttpResponse::Ok() }));
//! # Ok(())
//! # }
//! ```
//!
//! ## Permission Bit Mapping
//!
//! The `u128` role bitset uses right-to-left indexing:
//!
//! | `bit_id` | Bit mask      | Decimal value |
//! |----------|---------------|---------------|
//! | 0        | `1 << 0`      | 1             |
//! | 1        | `1 << 1`      | 2             |
//! | 2        | `1 << 2`      | 4             |
//! | ...      | ...           | ...           |
//! | 127      | `1 << 127`    | 2^127         |
//!
//! A user with `role = 0b1011` (decimal 11) has bits 0, 1, and 3 active.
//!
//! ## HTTP Status Codes
//!
//! | Scenario                              | Status | Meaning       |
//! |---------------------------------------|--------|---------------|
//! | No principal in request extensions    | 401    | Unauthorized  |
//! | No permission configured for endpoint | 403    | Forbidden     |
//! | Principal lacks required permission   | 403    | Forbidden     |
//! | Principal has required permission     | 200    | OK (continues)|
//!
//! ## JSON Configuration
//!
//! ```json
//! {
//!   "permissions": [
//!     { "method": "GET",    "url": "/users",       "bit_id": 0 },
//!     { "method": "POST",   "url": "/users",       "bit_id": 1 },
//!     { "method": "GET",    "url": "/users/{id}",  "bit_id": 2 },
//!     { "method": "DELETE", "url": "/users/{id}",  "bit_id": 3 },
//!     { "method": "GET",    "url": "/files/{tail:.*}", "bit_id": 4 }
//!   ]
//! }
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

mod error;
mod middleware;
mod permission;
mod principal;

pub use error::PermissionError;
pub use middleware::Permissions;
pub use permission::{Permission, PermissionSet};
pub use principal::Principal;
