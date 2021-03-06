use tokio::sync::RwLock;

use std::hash::Hash; 
use std::time::{Duration, Instant}; 
use std::collections::HashMap; 
use std::sync::{Arc, Mutex}; 
 
/// Value for our kv store with optional expiring time
/// This is created automatically can enter kv pairs normally. 
/// But when recieving Values they can use this to get expire
#[derive(Clone)]
pub struct Value<V> {
    pub value: V,
    expire: Option<Instant>,
}

impl<V> Value<V> {
    /// Create a new Value and add a expiration time if needed
    pub fn new(value: V, duration: Option<Duration>) -> Self {
        let expire: Option<Instant> = match duration {
            Some(d) => Some(Instant::now() + d),
            None => None
        };
        Value { value, expire }
    }

    /// Check if Value is expired
    pub fn expired(&self) -> bool {
        match self.expire {
            Some(t) =>  t < Instant::now(),
            None => false
        }
    }
}

// Our underlieing Data store
//
// Could use skip list, balanced/trie trees, hashes
// Choose Hashmap since 1) hashes most popular
// 2) pretty optimized implementation and quick to do
/// Thread safe async key value cache with option expiring time
/// Read & updates can be done concurrently but only one thread can 
/// insert or delete at a time.
pub struct Kv<K, V> {
    // RwLock provides thread saftey and allows multiple readers
    // If a reader needs to write they can gain access from the Mutex
    items: RwLock<HashMap<K, Arc<Mutex<Value<V>>> >>,
}

impl<K, V> Kv<K, V> {
    /// Creates a new instance of an in memory kv storage
    pub fn new() -> Self {
        Kv {
            items: RwLock::new(HashMap::new()),
        }
    }

    /// Gets the value for a given key
    /// Returns Option
    /// None if note found otherwise a clone of the value
    pub async fn get(&self, key: &K) -> Option<V>
    where
        K: Eq + Hash,
        V: Clone,
    {
        // Get value using a ReadLock
        let map = self.items.read().await;

        // If some value extract it from mutex
        // if not release the lock
        if let Some(arc) = map.get(key) {
            // Clone the Arc so we can release the rwlock.
            let mutex = arc.clone();
            drop(map);

            // Clone value from the mutex and return it dropping the mutex lock
            let clone = mutex.lock().expect("mutex poisoned").value.clone();
            return Some(clone)
        }
        None
    }

    /// Update if value already exists, otherwise creates a new entry.
    /// This returns a clone() of the old value if updating 
    /// otherwise the same is returned that is inserted.
    pub async fn set(&self, key: K, value: V) -> V
    where
        K: Eq + Hash,
        V: Clone,
    {

        // First we Check to see if the value already exists.
        // This would prevent us from having to use a WriteLock,
        // since we can update the mutex instead.

        // Get value using a ReadLock
        let map = self.items.read().await;

        // If some value we update it
        if let Some(arc) = map.get(&key) {
            // Clone the Arc so we can release the rwlock.
            let mutex = arc.clone();
            drop(map);

            // Lock the mutex so we can use it.
            let mut old = mutex.lock().expect("mutex poisoned");

            // Clone the old value to return
            let clone = old.value.clone();

            old.value = value;

            return clone
        } else {
            // Drop our read lock so we can get a write lock and update the value
            drop(map);
            let insert = self.items
                .write()
                .await
                .insert(
                    key,
                    Arc::new(Mutex::new(Value::new(value.clone(), None))),
                );
            match insert {
                None => value,
                Some(v) => v.lock().expect("mutex posioned").value.clone(),
            }
        }
    }

    /// Like `get()` but returns Value struct with value and expiration time
    pub async fn get_with_expire(&self, key: &K) -> Option<Value<V>>
    where
        K: Eq + Hash,
        V: Clone,
    {
        // Get value using a ReadLock
        let map = self.items.read().await;

        // If some value extract it from mutex
        // if not release the lock
        if let Some(arc) = map.get(key) {
            // Clone the Arc so we can release the rwlock.
            let mutex = arc.clone();
            drop(map);

            // Clone value from the mutex and return it dropping the mutex lock
            let clone = mutex.lock().expect("mutex poisoned").clone();
            return Some(clone)
        }
        None
    }

    /// Like `set()` but allows to update or set a Duration until the item expires
    /// Returns Value (both value and experation time)
    pub async fn set_with_expire(&self, key: K, value: V, duration: Duration) -> Value<V>
    where
        K: Eq + Hash,
        V: Clone,
    {

        // First we Check to see if the value already exists.
        // This would prevent us from having to use a WriteLock,
        // since we can update the mutex instead.

        // Get value using a ReadLock
        let map = self.items.read().await;

        // If some value we update it
        if let Some(arc) = map.get(&key) {
            // Clone the Arc so we can release the rwlock.
            let mutex = arc.clone();
            drop(map);

            // Lock the mutex so we can use it.
            let mut old = mutex.lock().expect("mutex poisoned");

            // Clone the old value to return
            let clone = old.clone();

            old.value = value;
            old.expire = Some(Instant::now() + duration);

            return clone
        } else {
            // Drop our read lock so we can get a write lock and add a new value
            drop(map);
            let insert = self.items
                .write()
                .await
                .insert(
                    key,
                    Arc::new(Mutex::new(Value::new(value.clone(), Some(duration)))),
                );
            match insert {
                None => Value::new(value, Some(duration)),
                Some(v) => v.lock().expect("mutex posioned").clone(),
            }
        }
    }

    /// Removes a kv pair returning the value if removed otherwise none
    pub async fn remove(&self, key: &K) -> Option<V>
    where
        K: Eq + Hash,
        V: Clone
    {
        // Get a write lock and remove from database
        self.items
            .write()
            .await
            .remove(key)
            .map(|v| v.lock().expect("mutex poisoned").value.clone())
    }

    /// Removes all the expired items in the cache
    pub async fn remove_expired(&self)
    where
        K: Eq + Hash + Clone,
    {

        // Get a copy of the map with a read lock
        let map = self.items.read().await;

        // Clone keys that are expired
        let expired_keys: Vec<K> = map
            .iter()
            .filter(|(_, item)| item.lock().expect("mutex poisoned").expired())
            .map(|(k, _)| k.clone())
            .collect();

        //release read lock
        drop(map);

        // Get a write lock to remove expired keys
        // This can be an expensive operation since it bocks for awhile
        for key in expired_keys {
            self.items.write().await.remove(&key);
        }
    }

    /// Clear the KV store
    pub async fn clear(&self) {
        self.items.write().await.clear()
    }


}

#[cfg(test)]
mod tests {
    use crate::Kv;
    use std::time::Duration;
    use tokio::time::sleep;

    const KEY: i8 = 0;
    const VALUE: &str = "VALUE";

    #[tokio::test]
    async fn set_and_get() {
        let db = Kv::new();
        db.set(KEY, VALUE).await;
        let value = db.get(&KEY).await;
        match value {
            Some(value) => assert_eq!(value, VALUE),
            None => panic!("value was not found in cache"),
        };
    }

    #[tokio::test]
    async fn set_and_get_with_expire() {
        let db = Kv::new();
        db.set_with_expire(KEY, VALUE, Duration::from_secs(2)).await;
        let value = db.get_with_expire(&KEY).await;
        match value {
            Some(v) => assert_eq!(v.value, VALUE),
            None => panic!("value was not found in cache"),
        };
    }

    #[tokio::test]
    async fn set_replace_existing_value() {
        const NEW_VALUE: &str = "NEW_VALUE";
        let kv = Kv::new();
        kv.set(KEY, VALUE).await;
        kv.set(KEY, NEW_VALUE).await;
        let value = kv.get(&KEY).await;
        match value {
            Some(value) => assert_eq!(value, NEW_VALUE),
            None => panic!("value was not found in cache"),
        };
    }

    #[tokio::test]
    async fn set_replace_existing_value_with_expire() {
        const NEW_VALUE: &str = "NEW_VALUE";
        let kv = Kv::new();
        kv.set_with_expire(KEY, VALUE, Duration::from_secs(2)).await;
        kv.set_with_expire(KEY, NEW_VALUE, Duration::from_secs(2)).await;
        let value = kv.get_with_expire(&KEY).await;
        match value {
            Some(v) => assert_eq!(v.value, NEW_VALUE),
            None => panic!("value was not found in cache"),
        };
    }

    #[tokio::test]
    async fn remove_item() {
        let kv = Kv::new();
        kv.set(KEY, VALUE).await;
        if let None = kv.remove(&KEY).await {
            panic!("none returned from removing existing value")
        };
        if kv.items.read().await.get(&KEY).is_some() {
            panic!("found removed item in cache")
        };
    }

    #[tokio::test]
    async fn remove_return_none_if_not_found() {
        let kv: Kv<i8, &str> = Kv::new();
        if let Some(_) = kv.remove(&KEY).await {
            panic!("non-existent value was returned from remove")
        };
    }

    #[tokio::test]
    async fn remove_expired_item() {
        let kv = Kv::new();
        kv.set_with_expire(KEY, VALUE, Duration::from_millis(50)).await;
        sleep(Duration::from_secs(1)).await;
        kv.remove_expired().await;
        if kv.items.read().await.get(&KEY).is_some() {
            panic!("found expired item in cache")
        };
    }

    #[tokio::test]
    async fn remove_only_expired_items() {
        const NEW_KEY: i8 = 1;
        let kv = Kv::new();
        kv.set_with_expire(KEY, VALUE, Duration::from_millis(50)).await;
        kv.set_with_expire(NEW_KEY, VALUE, Duration::from_secs(2)).await;
        sleep(Duration::from_secs(1)).await;
        kv.remove_expired().await;
        if kv.items.read().await.get(&KEY).is_some() {
            panic!("found expired item in cache")
        };
        if kv.items.read().await.get(&NEW_KEY).is_none() {
            panic!("could not find, not expired item in cache")
        };
    }

    #[tokio::test]
    async fn clear_all_items() {
        const NEW_KEY: i8 = 1;
        let kv = Kv::new();
        kv.set_with_expire(KEY, VALUE, Duration::from_millis(50)).await;
        kv.set_with_expire(NEW_KEY, VALUE, Duration::from_secs(2)).await;
        sleep(Duration::from_secs(1)).await;
        kv.clear().await;
        if kv.items.read().await.get(&KEY).is_some() {
            panic!("found item in cache")
        };
        if kv.items.read().await.get(&NEW_KEY).is_some() {
            panic!("found item in cache")
        };
    }





}
