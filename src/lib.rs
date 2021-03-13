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

}

#[cfg(test)]
mod tests {
    use crate::Kv;

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

}
