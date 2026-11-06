# Cache

Ube provides one string key/value API for embedded memory caches, persistent embedded caches, and clients of a standalone shared cache. This is a Ube cache protocol, not a Memcached protocol implementation.

## Guides and runnable examples

- [Runnable project and expected output](../examples/cache/README.md)
- [Application recipes](cache-recipes.md): cache-aside fallback and concurrent counters
- [Deployment and recovery](cache-deployment.md): CLI options, shared hosting, disk lifecycle, backups, and troubleshooting
- [TCP protocol](cache-protocol.md): framing, authentication, request/response fields, and limits
- [Metrics and health](observability.md)

All examples use single-quoted Ube strings. Cache operations are synchronous; they do not return tasks. `Cache` is built in and requires no import or third-party package. Compile against the matching compiler/runtime revision.

For records that must remain until explicitly changed or deleted, use [KeyValueStore](key-value-store.md). Its database format and retention rules are separate from Cache. Cache values are strings: serialize objects, lists, or maps with [`.toJson()`](json.md) or [`.toXml()`](xml.md), then parse retrieved text explicitly with `Json.toObject()` or `Xml.toObject()`. Cache does not infer the payload format.

## Constructors

| Static method | Result | Meaning |
| --- | --- | --- |
| `Cache.memory(long maxBytes)` | `Cache` | Independent memory-only store |
| `Cache.open(string path, long maxBytes)` | `Cache` | Dedicated persistent store, durable writes |
| `Cache.openBuffered(string path, long maxBytes)` | `Cache` | Dedicated persistent store, buffered synchronization |
| `Cache.connect(string address, string token)` | `Cache` | Client of an existing shared server |

`maxBytes` must be positive. Numeric parameters accept `int` values through widening to `long`. Paths, addresses, keys, values, and tokens must be non-null strings. `Cache()` is not a constructor; choose a backend explicitly. Persistent modes use a dedicated cache database, not an application's existing SQL database. Parent directories must exist.

## Embedded

```ube
class Main {
    main() {
        final cache = Cache.memory(67108864)
        cache.set('user:42', '{"name":"Alice"}', 60000)
        final user = cache.get('user:42') // string?, null on a miss
        Console.printLine(user ?? 'not cached')
        cache.close()
    }
}
```

Use `Cache.open('data/cache.db', 67108864)` for durable persistence, or `Cache.openBuffered('data/cache.db', 67108864)` for buffered persistence. Create the parent directory first. These constructors have the same instance API as `Cache.memory`. Each independently created memory cache is separate.

| Instance method | Result | Behavior |
| --- | --- | --- |
| `get(string key)` | `string?` | Null means missing or expired; an empty string is a valid hit. Network/storage failures throw instead of returning null. |
| `set(string key, string value, long ttlMs)` | `void` | Replace a value and its expiration. Zero TTL means no expiration. |
| `add(string key, string value, long ttlMs)` | `bool` | Insert only if absent or expired; return whether inserted. |
| `increment(string key, long delta)` | `long` | Atomically update a signed 64-bit integer stored as a string. Preserve the original expiration. Missing keys, non-integer values, and overflow raise errors. Negative deltas are supported. |
| `delete(string key)` | `bool` | Remove a key; return whether an unexpired key existed. |
| `listen(string address, string token)` | `void` | Expose a local cache to remote clients on a separate TCP listener in this process. Remote client handles cannot start listeners. |
| `close()` | `void` | Stop and join any listener, close the store/client handle, and release its resources. Repeated closes are safe. Other operations on a closed handle throw. |

`set` and `add` always require the TTL argument; there is no implicit default. `get` neither extends the deadline nor reloads an evicted value from disk. `listen` can be called once per local cache and does not block the caller while serving. To stop that listener, close its cache. There is no separate listener-stop or remote-shutdown method. Closing a client does not stop its server.

Operations are synchronized within the owning cache. Concurrent adds to a missing key have one winner; increments are atomic across threads and network clients of that server. A read followed by a separate write is not an atomic transaction. Use `close` when finished; runtime cleanup also closes caches after joining application tasks.

TTL is measured in milliseconds. Expiration is an absolute Unix timestamp, so restarting does not reset the TTL. Expired entries are treated as misses and reclaimed lazily during operations and metric collection. Keep the system clock accurate. Keys contain 1–1,024 UTF-8 bytes; values contain at most 1 MiB. Neither may contain NUL bytes. Invalid arguments, closed handles, unavailable servers, authentication failures, capacity errors, and storage failures raise catchable errors.

## Standalone shared server

Supply a token through the service's environment (at least 16 and at most 4,096 bytes), then run:

```bash
ube cache --bind 127.0.0.1:6380 --memory 67108864 \
  --disk /var/lib/my-app/cache.db --metrics 9090
```

The default token variable is `UBE_CACHE_TOKEN`; `--token-env NAME` selects another variable. The default bind address is `127.0.0.1:6380`, and the default memory budget is 64 MiB. Omit `--disk` for an ephemeral server. `--buffered` selects buffered persistence and requires `--disk`. `--metrics PORT` starts the localhost-only observability listener with readiness enabled. `ube cache --help` lists options. A service manager owns the process lifetime; terminate it through the manager.

Applications connect through the same API:

```ube
class Main {
    main() {
        final cache = Cache.connect('127.0.0.1:6380', Env.require('UBE_CACHE_TOKEN'))
        cache.set('shared:greeting', 'hello', 60000)
        Console.printLine(cache.get('shared:greeting') ?? 'missing')
        cache.close()
    }
}
```

Multiple application processes pointing at the same server share its entries. Closing a remote client does not stop the server. Restarting an application leaves a standalone server running. Restarting a persistent cache reloads its unexpired, unevicted entries. `connect` validates the address and token locally; network connection and authentication happen on each operation. Addresses must use a numeric IP and nonzero port; DNS names are not supported in this version.

To share between machines, explicitly bind a reachable address and configure networking. The protocol authenticates requests but does **not** provide TLS: use an encrypted tunnel or TLS proxy across untrusted networks. Do not expose the raw listener publicly. The protocol uses length-prefixed JSON, one request per TCP connection, four workers, a bounded connection queue, bounded message sizes, and connection/read/write timeouts. It does not retry mutations automatically: after a lost response, a write may have committed, so blindly retrying an increment may apply it twice.

Embedded `listen` provides the same sharing inside an application process. That listener's lifetime follows the application; standalone mode separates their lifetimes. Two embedded caches never synchronize automatically. Do not share a database file between cache processes; an exclusive SQLite lock rejects concurrent opens. Share through the network listener instead.

## Capacity and eviction

The byte budget accounts for UTF-8 keys, values, and 64 bytes per entry. It is **not a hard process RSS limit**: allocator/tree overhead, SQLite page caches, network buffers, temporary copies, and values returned to callers consume additional memory. An entry larger than the budget is rejected without replacing its previous value. Expired entries are reclaimed before a write; if more space is needed, least recently used entries are evicted. Reads update recency within the running process. Read-only recency updates are not journaled; reopening restores the last persisted write recency. Reopening with a smaller budget discards excess entries and persists those evictions.

The disk file is not restricted to the memory budget and can retain previously allocated free pages. This first implementation uses an ordered map, expiry scans on writes, and sorting when eviction is necessary; it does not claim Memcached-level throughput or latency.

## Persistence and recovery

Persistent caches use a dedicated SQLite database with a write-ahead log (WAL). Each write, deletion, increment, and associated eviction commits before its success response. The default `Cache.open` / `ube cache --disk` uses `synchronous=FULL`. WAL checkpointing updates the main database automatically after approximately 256 WAL pages; clean embedded close also requests a truncating checkpoint. This supplies journal recovery and periodic checkpointing rather than a separate custom snapshot format.

`openBuffered` / `--buffered` uses `synchronous=NORMAL`: transactions remain atomic, but recent acknowledgements can be lost following a machine or power failure. Durable operation also depends on the filesystem and storage device honoring synchronization. A killed process is covered by end-to-end recovery testing; physical power-loss guarantees have not been experimentally verified.

The database is identified and versioned. Unrelated SQLite databases, corrupt files, and unsupported cache versions cause errors rather than silently resetting the cache. Committed deletions, expirations, and evictions do not reappear on reopen. Keep the database, `-wal`, and `-shm` files together; do not delete or copy individual files while the service is live. For now, stop the owner before taking a filesystem backup. Store persistent cache data outside `.build` if it must survive `ube clean`.

This is a cache: entries can expire or be evicted even when persistence is enabled. It does not replace an authoritative database and does not provide replication, sharding, distributed transactions, or failover.

## Observability

An application that calls `Observability.start(port)`, or a standalone cache started with `--metrics PORT`, exposes:

- `ube_cache_hits_total`, `ube_cache_misses_total`
- `ube_cache_writes_total`, `ube_cache_evictions_total`, `ube_cache_expirations_total`
- `ube_cache_items`, `ube_cache_accounted_bytes`

Metrics aggregate locally owned caches in that process, never remote server contents observed by clients. Counters cover the current process lifetime; a restart resets them. Keys, values, authentication tokens, and disk paths are not exported as labels. In standalone mode, scrape the cache server's metrics port.


## Example catalog

| Source | Demonstrates |
| --- | --- |
| [Memory](../examples/cache/memory/Main.ube) | Add, get, increment, delete, and deterministic LRU eviction |
| [Persistence](../examples/cache/persistent/Main.ube) | Separate write/read/delete application invocations |
| [Expiration](../examples/cache/ttl/Main.ube) | Absolute TTL across application downtime |
| [Concurrent counter](../examples/cache/counter/Main.ube) | Two async tasks using atomic increments |
| [Writer](../examples/cache/writer/Main.ube) / [Reader](../examples/cache/reader/Main.ube) | Sharing through an independent server |
| [Embedded server](../examples/cache/server/Main.ube) | TCP sharing, health, metrics, and explicit shutdown |
| [Cache-aside](../examples/cache/cache-aside/Main.ube) | Source fallback and handling cache errors |

Follow the [example README](../examples/cache/README.md) for setup, complete commands, expected outputs, and restart demonstrations.

## Validation coverage

The cache engine tests cover atomic concurrent adds/increments, limits, recency-based eviction, expiration, invalid integer operations, persistent deletes/overwrites/evictions, exclusive ownership, smaller-budget recovery, synchronization settings, and an injected database-write failure that must not publish memory changes.

Native end-to-end tests compile Ube programs and exercise the CLI, standalone and embedded listeners, independent client processes, authentication, malformed frames, slow-client shutdown, metrics, buffered reopen, and forced process death with recovery and expiration during downtime. Run them from the repository root with localhost socket access:

```bash
cargo build -p ube-runtime
cargo test -p ube-runtime --lib cache::
cargo test -p ube-compiler --test cache_e2e
```

These checks establish tested behavior, not production throughput, power-loss guarantees, or multi-node availability. See [deployment limits](cache-deployment.md).
