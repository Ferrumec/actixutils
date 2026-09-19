use std::{error::Error, hash::Hash, marker::PhantomData, sync::Arc, time::Duration};

use crate::Store;
use crate::locals::store::CacheFactory;
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Serialize, de::DeserializeOwned};

pub struct RedisCache<K, V> {
    connection: ConnectionManager,
    namespace: Vec<u8>,
    ttl: Duration,
    _marker: PhantomData<fn(K) -> V>,
}

impl<K, V> RedisCache<K, V> {
    /// Create a Redis-backed store using the supplied connection manager.
    ///
    /// `namespace` is used to isolate this store from all other Redis data.
    ///
    /// For example:
    ///
    /// `authnz:cache`
    ///
    /// becomes keys such as:
    ///
    /// `authnz:cache:<serialized-key>`
    pub fn new(connection: ConnectionManager, namespace: impl Into<String>, ttl: Duration) -> Self {
        let namespace = namespace.into();

        // The delimiter is important so that:
        //
        // foo + "bar"
        //
        // cannot collide with:
        //
        // fo + "obar"
        //
        // when scanning by namespace.
        let namespace = format!("{namespace}:").into_bytes();

        Self {
            connection,
            namespace,
            ttl,
            _marker: PhantomData,
        }
    }

    fn make_key(&self, key: &K) -> Result<Vec<u8>, Box<dyn Error>>
    where
        K: Serialize,
    {
        let encoded = serde_json::to_vec(key)?;

        let mut redis_key = Vec::with_capacity(self.namespace.len() + encoded.len());

        redis_key.extend_from_slice(&self.namespace);
        redis_key.extend_from_slice(&encoded);

        Ok(redis_key)
    }
}

#[async_trait::async_trait]
impl<K, V> Store<K, V> for RedisCache<K, V>
where
    K: Serialize + Send + Sync + 'static,
    V: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    async fn get(&self, key: &K) -> Result<Option<V>, Box<dyn Error>> {
        let redis_key = self.make_key(key)?;

        let mut connection = self.connection.clone();

        let value: Option<Vec<u8>> = connection.get(redis_key).await?;

        match value {
            Some(value) => {
                let value = serde_json::from_slice(&value)?;

                Ok(Some(value))
            }

            None => Ok(None),
        }
    }

    async fn set(&self, key: &K, value: V) -> Result<(), Box<dyn Error>> {
        let redis_key = self.make_key(key)?;

        let encoded = serde_json::to_vec(&value)?;

        let mut connection = self.connection.clone();

        connection
            .set_ex::<_, _, ()>(redis_key, encoded, self.ttl.as_secs())
            .await?;

        Ok(())
    }

    async fn delete(&self, key: &K) -> Result<(), Box<dyn Error>> {
        let redis_key = self.make_key(key)?;

        let mut connection = self.connection.clone();

        connection.unlink::<_, ()>(redis_key).await?;

        Ok(())
    }
}

impl CacheFactory for ConnectionManager {
    fn new_cache<K, V>(&self, name: &str, ttl: Duration) -> Arc<dyn Store<K, V>>
    where
        K: Hash + Eq + Clone + Serialize + Send + Sync + 'static,
        V: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        Arc::new(RedisCache::new(self.clone(), name, ttl))
    }
}
