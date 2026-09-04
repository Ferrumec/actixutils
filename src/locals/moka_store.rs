use crate::Store;
use moka::future::Cache;
use std::error::Error;
use std::hash::Hash;

#[async_trait::async_trait]
impl<K: Clone + Hash + Eq + Send + Sync + 'static, V: Clone + Send + Sync + 'static> Store<K, V>
    for Cache<K, V>
{
    /// Look up the value stored under `key`, if any.
    async fn get(&self, key: &K) -> Result<Option<V>, Box<dyn Error>> {
        Ok(self.get(key).await)
    }

    /// Store `value` under `key`, replacing any existing entry.
    async fn set(&self, key: &K, value: V) -> Result<(), Box<dyn Error>> {
        self.insert(key.clone(), value).await;
        Ok(())
    }

    /// Remove the entry stored under `key`, if present.
    async fn delete(&self, key: &K) -> Result<(), Box<dyn Error>> {
        self.remove(key).await;
        Ok(())
    }

    /// Remove every entry from the store.
    async fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.invalidate_all();
        self.run_pending_tasks().await;
        Ok(())
    }
}
