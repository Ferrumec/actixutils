//! In-memory [`CacheStore`] implementation for development and
//! single-process deployments.

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::types::CachedResponse;
use crate::Store;

const DEFAULT_MAX_ENTRIES: usize = 10_000;

#[derive(Clone)]
struct Entry {
    response: CachedResponse,
    expires_at: Instant,
}

impl Entry {
    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// A simple, process-local cache store backed by a `HashMap` behind a
/// `tokio::sync::RwLock`.
///
/// - Expired entries are treated as absent on read, and swept opportunistically
///   on write.
/// - Growth is bounded by `max_entries`; once the limit is reached, the
///   oldest-inserted entries are evicted first (FIFO), which is a
///   deliberately simple policy for this first implementation.
/// - Not shared across processes. For multi-instance deployments, implement
///   [`CacheStore`] against a shared backend (e.g. Redis) instead.
pub struct MemoryCache {
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    insertion_order: Arc<RwLock<VecDeque<String>>>,
    max_entries: usize,
}

impl MemoryCache {
    /// Create a cache with the default capacity (10,000 entries).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_ENTRIES)
    }

    /// Create a cache bounded to `max_entries` entries.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            insertion_order: Arc::new(RwLock::new(VecDeque::new())),
            max_entries,
        }
    }

    async fn evict_if_needed(&self) {
        let mut entries = self.entries.write().await;

        // Opportunistically drop anything already expired before evicting.
        entries.retain(|_, entry| !entry.is_expired());

        if entries.len() < self.max_entries {
            return;
        }

        let mut order = self.insertion_order.write().await;
        while entries.len() >= self.max_entries {
            match order.pop_front() {
                Some(oldest_key) => {
                    entries.remove(&oldest_key);
                }
                None => break,
            }
        }
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store<String, CachedResponse> for MemoryCache {
    async fn get(&self, key: &String) -> Result<Option<CachedResponse>, Box<dyn Error>> {
        let entries = self.entries.read().await;
        Ok(match entries.get(key) {
            Some(entry) if !entry.is_expired() => Some(entry.response.clone()),
            _ => None,
        })
    }

    async fn set(&self, key: &String, response: CachedResponse) -> Result<(), Box<dyn Error>> {
        self.evict_if_needed().await;
        let ttl = Duration::from_mins(10);
        let entry = Entry {
            response,
            expires_at: Instant::now() + ttl,
        };

        let mut entries = self.entries.write().await;
        let is_new_key = entries.insert(key.to_string(), entry).is_none();
        drop(entries);

        if is_new_key {
            self.insertion_order
                .write()
                .await
                .push_back(key.to_string());
        }
        Ok(())
    }

    async fn delete(&self, key: &String) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.entries.write().await.clear();
        self.insertion_order.write().await.clear();
        Ok(())
    }
}
