# Runnable cache examples

These examples cover the cache API in [the reference](../../docs/cache.md) and [deployment guide](../../docs/cache-deployment.md). Every source is named `Main.ube` and declares `class Main`. The shared manifest defaults to the memory example. No external packages are required.

## Setup

From the repository root, build the compiler and its matching runtime, then enter this directory:

```bash
cargo build -p ube-compiler -p ube-runtime
cd examples/cache
UBE_BIN=../../target/debug/ube-compiler
mkdir -p data .build
```

If a matching version is already installed, use `UBE_BIN=ube` instead. Commands below assume the current directory is `examples/cache`. Relative database paths are resolved against that directory. `.build/` and `data/` are ignored by this example project; `ube clean` removes build outputs while `data/` remains.

## Memory cache and eviction

Source: [memory/Main.ube](memory/Main.ube).

```bash
"$UBE_BIN" run memory/Main.ube
```

Expected output:

```text
true
false
1
missing
3
true
false
```

The second `add` leaves the existing value unchanged. Reading `a` makes it more recent than `b`; adding `c` evicts `b` from the two-entry budget. Increment and delete operate on the remaining entry.

## Persistence across application restarts

Source: [persistent/Main.ube](persistent/Main.ube).

```bash
"$UBE_BIN" run persistent/Main.ube -- data/persistent.db set
"$UBE_BIN" run persistent/Main.ube -- data/persistent.db get
"$UBE_BIN" run persistent/Main.ube -- data/persistent.db delete
"$UBE_BIN" run persistent/Main.ube -- data/persistent.db get
```

Each command starts a separate application process. Expected output, in order:

```text
saved
hello from disk
true
missing
```

Use a fresh database or run `set` first to reproduce these results. The example closes the cache in `finally`, including on errors.

## Expiration while the application is stopped

Source: [ttl/Main.ube](ttl/Main.ube). Compile once so compilation time does not consume the two-second TTL:

```bash
"$UBE_BIN" build ttl/Main.ube -o .build/cache-ttl
.build/cache-ttl data/ttl.db set
.build/cache-ttl data/ttl.db get
sleep 3
.build/cache-ttl data/ttl.db get
```

Expected application output:

```text
saved for two seconds
temporary
expired or missing
```

The immediate `get` must execute within two seconds. A paused or heavily loaded machine may already report expiration. The TTL is stored as an absolute deadline and is not renewed by reopening the database.

## Atomic increments from concurrent tasks

Source: [counter/Main.ube](counter/Main.ube).

```bash
"$UBE_BIN" run counter/Main.ube
```

Expected output:

```text
200
```

Two async methods each increment the same key 100 times. Both start before either is awaited. The counter operation is atomic; a separate `get` followed by `set` would not provide that guarantee. Cache calls are synchronous; methods containing these calls use the blocking task backend.

## Standalone server and two application processes

Sources: [writer/Main.ube](writer/Main.ube) and [reader/Main.ube](reader/Main.ube).

In terminal A, run the setup above, then enter a shared token without placing it in shell history:

```bash
read -rsp 'Cache token (at least 16 bytes): ' UBE_CACHE_TOKEN
printf '\n'
export UBE_CACHE_TOKEN
"$UBE_BIN" cache --bind 127.0.0.1:6380 --memory 67108864 \
  --disk data/shared.db --metrics 9090
```

The server remains in the foreground. In terminal B, enter `examples/cache`, set `UBE_BIN` as above, and enter/export the **same token** with the same prompt commands. Then run:

```bash
"$UBE_BIN" run writer/Main.ube
"$UBE_BIN" run reader/Main.ube
```

On a fresh database, the writer prints `1`; the reader prints:

```text
hello from another process
1
```

The greeting expires after 60 seconds. Repeating the writer increases the visit counter; reopening an existing database continues its previous count. Closing either application leaves the server running.

Press Ctrl+C in terminal A, then run the same server command again. Rerun the reader: the visit counter remains; the greeting remains only if its TTL has not elapsed and it has not been evicted. Use a different disk path for a fresh cache instead of deleting files from a running cache.

The clients default to `127.0.0.1:6380`. To use another numeric IP/port, set `UBE_CACHE_ADDRESS` in the client environment and pass the same endpoint to the server's `--bind` option. Authentication does not encrypt traffic; use an encrypted tunnel or TLS proxy across untrusted networks.

## Embedded server inside an application

Source: [server/Main.ube](server/Main.ube). Stop the standalone server first because this example uses the same cache and metrics ports.

With the token exported, run:

```bash
"$UBE_BIN" run server/Main.ube -- data/embedded.db
```

It prints:

```text
Cache listening; press Enter to stop
```

Leave it running and use the same writer and reader in a second terminal. The cache TCP listener uses port 6380; observability uses localhost port 9090. The example stops both listeners in `finally` when you press Enter. An EOF on stdin also ends this interactive example; use the standalone command for an independently supervised cache service.

## Monitoring

While either server is running:

```bash
curl -fsS http://127.0.0.1:9090/health/live
curl -fsS http://127.0.0.1:9090/health/ready
curl -fsS http://127.0.0.1:9090/metrics
```

Health and readiness return `{"status":"UP"}`. Metrics include cache hit/miss counts, writes, evictions, expirations, item count, and accounted bytes. Values vary with your requests and the database's previous contents. Cache keys, values, and tokens are not exported.

## Cache-aside application pattern

Source: [cache-aside/Main.ube](cache-aside/Main.ube).

```bash
"$UBE_BIN" run cache-aside/Main.ube
```

Expected output:

```text
source read
{"id":42,"name":"Alice"}
cache hit
{"id":42,"name":"Alice"}
```

The example uses an in-memory cache and a fixed sample source value. Replace the marked source read with your database/service call. Cache read or write failures are caught so the authoritative source remains available. A source failure should still propagate; a missing cache entry is not proof that a database record does not exist.

## Further reading

- [API, limits, eviction, and persistence](../../docs/cache.md)
- [Deployment, recovery, and troubleshooting](../../docs/cache-deployment.md)
- [Recipes and error handling](../../docs/cache-recipes.md)
- [Wire protocol](../../docs/cache-protocol.md)
- [Metrics and health](../../docs/observability.md)
