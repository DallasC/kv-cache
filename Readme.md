# KV-Cache
A thread safe async in memory kv storage

## Considerations
First I would probably recommend using a pre-existing solution. There have been a lot of engineering hours put into things like `memcached`, `redis`, `FASTER`, `Masstree`, etc. If for whatever reason these solutions don't work and a lighter weight solution is needed this could work.

See the `Design` section for a more detailed explanation of this implementation but at it's core it uses a Read Write lock on a Hashmap to provide thread saftey. Theres some additional optimizations around not needing a full write lock for updates but in the end it still needs locks on inserts and deletes. 

If this is a performance bottleneck & you still don't want to uses a pre-existing solution here are some optimizations you could implement:
1. Implement a lock-free hashmap. This can be done with atomics but can be tricky to get right and adds complexity to the solution. Also, there is the additonal overhead of using atomics but if you need this optimization then the perfromance gains definitely outway the overhead. Alternatively rust has a couple lockfree maps that you could use. 
2. If you want to take the lockfree hashtable implementation further you could optimize the hash index. In a traditional hash index you would use a collision chain but if you opt instead to implement an ordered-ring structure. This would add a slight amount of overhead but would massively speed up reads/writes to commonly accessed memory.This happens because with a collision chain the memory you need to access could be at the end of the chain where as with a ring you can move the head to the memory that needs access the most. Again this would add further complexity and time investment.

## Design

This library provides a thin wrapper around `std::collections::HashMap`. This means performance is close what the Rust's standard library HashMap provides, which is very optimized.

The full implementation looks like 
``` Rust
RwLock<HashMap<K, Arc<Mutex<Value<V>>> >>
```
RwLock is a `tokio::sync::RwLock` this provides us thread saftey by allowing many readers to access the HashMap at once but only at most 1 writer at a time. What this means for performance is that all threads can read at the same time but if you need to write (add, update, delete) only one thread can access at a time.

We optimize this by wrapping the `Value<V>` in a `Arc<Mutex<>>`. What this allows us to do is a Read Lock can now clone the arc, release the readlock, aquire a lock on the mutex and make edits. This means that both (reads, update) can happen by many threads at once and only (add, delete) need slow Write Locks.

`Value<V>` is a small wrapper around a value that adds an additional expiration time. We allow the users to clear all expired KV's from the Map. We do this first by aquiring a Read Lock and scanning the table for expired values (this doesn't lock out all threads since its not a write lock). We collect all the expired `Keys` into a new `Vec`. Then once done we aquire a Write Lock and delete the items. This adds some memory overhead but has the advantage of not having to hold an expensive Write Lock for the whole scan. 

Also, we don't automically delete expired values. This way was chosen so that the user can decide when they want to clear the expired keys. Since it has to use a Write lock it is important to allow the user to schedule the cleanup during a low load time. Also, if you want to return false for a value that is expired it is easy to implement. Use `get_with_expire` and then you can use the `.expired()` method to get a `bool`.

We return `Cloned` values in order to insure memory saftey between threads. This means if your `V` is a large expensive value we have to clone it each time you use `get()` or `get_with_expire()`. In this case I would recommend wrapping it in an `Arc` that way you only have to clone a refernce counter and not the entire value.

As a last note everything is in `lib.rs`. normally i would seperate things but in this case the full the implementation is just `Value<V>` which is very small and the `Kv` implementation so everything fit nicely into one file.
