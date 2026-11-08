# Deploying a Ube cache

See [Cache](cache.md) for the API and [runnable examples](../examples/cache/README.md) for a complete local walkthrough.

## Choose the owner

| Deployment | Owner and lifetime | Suitable use |
| --- | --- | --- |
| `Cache.memory(bytes)` | App process; data disappears when it exits | Local, disposable acceleration |
| `Cache.open(path, bytes)` | App process; disk data is recovered on reopen | Local cache that survives application restarts |
| Embedded `cache.listen(address, token)` | App process owns both store and TCP listener | Sharing a cache hosted by an existing application |
| `ube cache` | Independent service process | Sharing across application servers with independent restarts |

A disk-backed cache loads its retained, unexpired entries into memory. It is not a tiered cache that fetches evicted entries back from disk: eviction removes the persisted entry too. Reopening with a smaller budget can permanently evict entries. An embedded listener is a separate TCP listener in the application process, not routes attached to the application's HTTP server.

## Standalone options

```text
ube cache [--bind IP:port] [--memory bytes] [--disk path]
          [--buffered] [--metrics port] [--token-env name]
```

| Option | Default | Behavior |
| --- | --- | --- |
| `--bind IP:port` | `127.0.0.1:6380` | Numeric IPv4 or bracketed IPv6 endpoint, nonzero port |
| `--memory bytes` | `67108864` | Positive accounted-entry byte budget |
| `--disk path` | Absent | Enable persistence; parent directory must already exist |
| `--buffered` | Absent | Use SQLite `synchronous=NORMAL`; requires `--disk` |
| `--metrics port` | Absent | Enable localhost HTTP health/readiness/Prometheus listener |
| `--token-env name` | `UBE_CACHE_TOKEN` | Read authentication token from this process environment variable |
| `--help`, `-h` | — | Print help and exit |

Use different cache and metrics ports. Relative paths resolve against the process working directory. Neither the CLI nor `Cache.open` creates parent directories. For example, from a writable application directory:

```bash
mkdir -p data
read -rsp 'Cache token (at least 16 bytes): ' UBE_CACHE_TOKEN
printf '\n'
export UBE_CACHE_TOKEN
ube cache --bind 127.0.0.1:6380 --disk data/cache.db --metrics 9090
```

The token must contain 16–4,096 UTF-8 bytes. Supply the same token to each client, for example through its existing secret configuration and `Env.require('UBE_CACHE_TOKEN')`. The CLI reads the real service environment; it does not load an application's encrypted `.build/.conf` automatically. A service manager must inject the token into the standalone process environment. The token is not printed on startup or exposed in metrics.

## Multiple machines

Bind the cache to its intended private interface, for example:

```bash
ube cache --bind 10.0.0.10:6380 --disk data/cache.db --metrics 9090
```

Use that numeric endpoint in each application:

```ube
final cache = Cache.connect('10.0.0.10:6380', Env.require('UBE_CACHE_TOKEN'))
```

This example address is a placeholder for your deployment. The application snippet assumes an enclosing method and a token already supplied by your environment/vault. The raw protocol carries tokens and values without encryption: use an encrypted tunnel or TLS proxy across untrusted networks. Authentication grants access to the whole cache; there are no per-key access controls. Configure network access to admit only intended clients. There is no built-in TLS, DNS discovery, replication, failover, or sharding.

The metrics listener continues to bind only to `127.0.0.1`, even when the cache listener binds a private network address. Scrape it on the host or expose it through a separately configured authenticated proxy.

## Process lifetime and shutdown

`ube cache` runs in the foreground and does not daemonize. A service manager or container supervisor should own its lifetime and restart policy. It remains alive until terminated. There is no remote shutdown command and no custom graceful signal handler; an interrupted/terminated standalone process relies on WAL recovery at restart. Durable mutations have committed before acknowledgement.

Embedded applications should close caches in `finally`. Runtime cleanup also stops and joins cache listeners after joining application tasks. An embedded listener does not keep a completed `start()` alive. See the [interactive embedded example](../examples/cache/server/App.ube); pressing Enter or closing stdin allows it to finish.

Each listener has one accept thread, four worker threads, and a queue of up to 64 accepted connections. Frame size is bounded, and connection/read/write timeouts apply. A full queue can close a new connection; applications should handle that as a cache failure. Disk operations serialize through the cache's store lock. Disk stalls and synchronous journal writes can delay requests. Do not interpret health endpoint responsiveness as proof that a disk write will succeed.

Cache calls are synchronous. Use an async method when you need the call offloaded from the caller; methods containing these operations use Ube's blocking task backend. Do not assume that cancellation can interrupt an in-progress filesystem call.

## Durability and recovery

| Mode | Configuration | Acknowledgement and recovery |
| --- | --- | --- |
| Memory | `Cache.memory` / no `--disk` | Process memory only; no restart recovery |
| Durable | `Cache.open` / `--disk` | SQLite WAL transaction committed with `synchronous=FULL` before success |
| Buffered | `Cache.openBuffered` / `--disk --buffered` | Atomic WAL transactions with `synchronous=NORMAL`; recent acknowledgements can be lost after machine/power failure |

SQLite checkpoints the WAL into the main database after approximately 256 WAL pages. This is periodic journal checkpointing, not a user-facing snapshot/export API. Clean embedded close requests a truncating checkpoint. Correct durable recovery depends on storage honoring synchronization. End-to-end tests cover forced process death; physical power-loss behavior has not been experimentally verified.

On restart, the cache validates its database identity/version, discards expired entries, and enforces the configured budget. It retains absolute expiration deadlines, not the TTL duration from startup. Keep the host clock accurate. Corrupt files, unrelated SQLite databases, unsupported versions, and conflicting opens cause errors rather than an automatic empty reset.

Only one owning cache can open a given database at a time. Clients must connect over TCP; multiple application processes must not open the same disk file to share an in-memory index. This rule also applies to two separate `Cache.open` calls while the first remains open.

## Backup and disk placement

Keep the database and any `-wal`/`-shm` companions together. To take a filesystem backup with this version:

1. Stop the owning cache and ensure it has exited.
2. Copy the database and all existing companion files as one set.
3. Keep the source set intact before restarting the original service.
4. Restore into a separate directory while the destination cache is stopped, then start it with the restored database path.

Do not delete, edit, or copy individual active database/WAL files. No live backup, restore, or snapshot CLI is implemented. Test a restore on a separate cache before relying on a backup operationally. A restored entry keeps its original expiration and may already be expired.

Store data outside `.build` when it must survive `ube clean`. Ignore local cache data in Git, for example:

```gitignore
.build/
data/
```

That example assumes `data/` is dedicated to generated local data. Do not ignore a directory containing source assets you intend to version. The memory budget is not a disk quota: SQLite may retain free pages from previously larger contents. Monitor free disk space separately.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Missing token or token-length error | Service environment variable exists; token is 16–4,096 bytes |
| Authentication failure | Clients and owning server have the same token; connection occurs on the first operation |
| Cannot connect | Correct numeric IP/port, process running, network path reachable |
| Cannot bind / already running | Port already in use, duplicate `listen`, or invalid endpoint |
| Database locked / cannot open | Another cache owns the file, missing parent directory, or filesystem permissions |
| Database/version error | File belongs to Ube cache format version 1 and is intact; do not reset it automatically |
| Entry exceeds limit | Key + value + 64 bytes must fit the budget; value must also be at most 1 MiB |
| Unexpected miss after restart | TTL elapsed, entry was evicted, budget shrank, or a different disk path/working directory was used |
| Lost recent buffered writes | Buffered mode permits loss following host/power failure; use durable mode when needed |
| Increment fails | Key exists, value parses as signed 64-bit integer, result does not overflow |
| Timeout after a mutation | The mutation may already have committed; do not blindly repeat an increment |
| Metrics unavailable | `--metrics` / `Observability.start` enabled; scrape the separate localhost port |

See [recipes](cache-recipes.md) for cache-aside handling and [observability](observability.md) for endpoint semantics.
