//! GET-only HTTP response caching middleware for actix-web.
//!
//! Drop this module in as `middleware::cache` (i.e. copy this directory to
//! `src/middleware/cache/` and add `pub mod cache;` to `src/middleware/mod.rs`).
//!
//! ```ignore
//! use std::sync::Arc;
//! use std::time::Duration;
//! use actixutils::middleware::cache::{Cache, MemoryCache};
//!
//! let store = Arc::new(MemoryCache::new());
//!
//! App::new().wrap(Cache::new(store).ttl(Duration::from_secs(60)))
//! ```
//!
//! See [`middleware`] for the full behavioural contract (cache key
//! derivation, cache-control handling, streaming-body limitations).

mod memory;
mod middleware;
mod store;
mod types;

pub use memory::MemoryCache;
pub use middleware::{Cache, CacheMiddleware};
pub use store::CacheStore;
pub use types::CachedResponse;
