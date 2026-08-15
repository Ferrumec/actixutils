use std::error::Error;

#[async_trait::async_trait]
pub trait Store<K, V> {
    async fn get(&self, key: &K) -> Result<Option<V>, Box<dyn Error>>;
    async fn set(&self, key: &K, value: V) -> Result<(), Box<dyn Error>>;
    async fn delete(&self, key: &K) -> Result<(), Box<dyn Error>>;
    async fn clear(&self) -> Result<(), Box<dyn Error>>;
}
