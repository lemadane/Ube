# Cache application recipes

These examples use implemented APIs. See the [reference](cache.md), [deployment guide](cache-deployment.md), and [runnable project](../examples/cache/README.md) for configuration and commands.

## Cache-aside with source fallback

Try the cache first. On a miss or cache failure, load the authoritative source. Attempt to cache that result, then return it even if that cache write fails. This example logs cache failures without printing cache keys, values, or credentials:

```ube
class App {
    string loadProfile(Cache cache, string key) {
        try {
            final cached = cache.get(key)
            if (cached != null) {
                Console.printLine('cache hit')
                return cached ?? ''
            }
        } catch (Error err) {
            Log.warn('Cache read failed; loading the authoritative source')
        }

        Console.printLine('source read')
        // Replace this sample value with your database or service call.
        final profile = '{"id":42,"name":"Alice"}'
        try {
            cache.set(key, profile, 60000)
        } catch (Error err) {
            Log.warn('Cache write failed; returning the source value')
        }
        return profile
    }

    start() {
        final cache = Cache.memory(4096)
        try {
            Console.printLine(this.loadProfile(cache, 'user:42'))
            Console.printLine(this.loadProfile(cache, 'user:42'))
        } finally {
            cache.close()
        }
    }
}
```

The source read above is a fixed sample value, not a database query. Replace that marked line with the real source operation. Keep source failures outside the cache-error handlers so a database failure does not look like a successful empty result.

Run it using the [cache-aside example](../examples/cache/README.md#cache-aside-application-pattern). It prints a source read on the first request and a cache hit on the second. An empty string is a valid cache value; check for null rather than an empty value when distinguishing misses.

For a shared backend, replace `Cache.memory(4096)` with `Cache.connect(address, token)`. The same application pattern applies. It does not prevent multiple processes from loading the same missing record at once. There is no built-in request coalescing or stampede protection.

## Atomic counter across tasks

```ube
class App {
    async int incrementMany(Cache cache, int iterations) {
        var index = 0
        for (index < iterations) {
            cache.increment('count', 1)
            index = index + 1
        }
        return iterations
    }

    start() {
        final cache = Cache.memory(4096)
        try {
            cache.set('count', '0', 0)
            final first = this.incrementMany(cache, 100)
            final second = this.incrementMany(cache, 100)
            first.await()
            second.await()
            Console.printLine(cache.get('count') ?? 'missing')
        } finally {
            cache.close()
        }
    }
}
```

Run the [counter example](../examples/cache/README.md#atomic-increments-from-concurrent-tasks) to get `200`. Each call to `increment` holds the owning store's lock and, for a persistent cache, commits its update before returning. Remote clients use the same operation on the owning server.

Initialize a counter once with `cache.add('count', '0', 0)` when multiple processes may initialize it. A repeated `set('count', '0', 0)` would reset an existing count. This example uses a fresh local cache, so its initial `set` is intentional.

Do not implement increments by reading a string, computing a new number, and calling `set`: another task can update the key between those operations. Negative deltas are allowed, and a counter's original TTL is retained. Counters can still expire or be evicted, so do not use cache counters as the authoritative ledger for billing or durable business records.

## Invalidate after a source update

After committing an authoritative database update, delete its cached entry:

```ube
// Inside an application method, after the source transaction has committed:
cache.delete('user:42')
```

A later request reloads the value. Decide how your application handles invalidation errors: a short TTL can limit staleness, while a durable retry mechanism may be appropriate when stale data matters. A database transaction and a cache operation are separate; Ube does not supply a cross-system atomic transaction.

## Treat a timeout as an uncertain mutation result

A remote write may commit before its response is lost. Read-only lookups can usually be retried according to application policy. Automatically retrying `increment` can double-apply it. `add` returning false only establishes that a key exists; it does not prove which request created it. There are no mutation request IDs or deduplication guarantees in the current protocol.

## Use `add` for conditional insertion

```ube
// Inside an application method:
final inserted = cache.add('feature:banner', 'welcome', 30000)
Console.printLine(inserted)
```

Concurrent insertions have one winner while the key remains present. Do not treat this alone as a safe distributed lock: there is no compare-and-delete, fencing token, or lease ownership check, and entries can be evicted before their TTL.

## Choose TTLs and storage deliberately

| Need | Choice |
| --- | --- |
| Disposable local acceleration | `Cache.memory` |
| Local cache that survives application restart | `Cache.open` |
| Shared cache with independent application restarts | `ube cache --disk ...` plus `Cache.connect` |
| Buffered disk writes with possible recent-write loss after host failure | `Cache.openBuffered` or `--buffered` |
| Values that must eventually refresh | Nonzero millisecond TTL |
| No time-based expiration | TTL `0`; capacity eviction and explicit deletion still apply |

A longer TTL reduces source reads but permits stale data for longer. No automatic replication or application-specific invalidation occurs. Keep an authoritative source when losing an entry would be unacceptable.
