# Observability

Ube provides one optional observability facility with three static APIs. It runs a separate HTTP listener **inside the application process**, on IPv4 localhost. It starts only when requested and uses one dedicated thread, independent of application request handlers and task workers.

```ube
class App {
    start() {
        Observability.start(9090)
        Health.ready(true)

        Metrics.increment('orders_created_total')
        Metrics.add('orders_created_total', 2)
        Metrics.gauge('orders_pending', 12)
        Metrics.record('order_processing_seconds', 0.025)

        // Run the application here; the listener stops when the program exits.
    }
}
```

`start(port)` binds `127.0.0.1:port`. Ports must be 1–65535. Bind failures, invalid ports, and starting an already running listener raise catchable errors. `Observability.stop()` stops and joins the listener; repeated stops are safe. It can then be restarted. Stopping the listener preserves measurements and readiness. Runtime cleanup stops it automatically and clears measurements. The listener does not keep a finished start method alive.

| Endpoint | Response |
| --- | --- |
| `GET /health/live` | HTTP 200 and `{"status":"UP"}` while the listener responds |
| `GET /health/ready` | HTTP 503 and `{"status":"DOWN"}` initially; HTTP 200 and `{"status":"UP"}` after `Health.ready(true)` |
| `GET /metrics` | Prometheus text format, version 0.0.4 |

`Health.ready(false)` marks the application unavailable without changing liveness. Readiness is application controlled: it does not automatically probe databases or other dependencies. Liveness proves the management thread responds, not that every application worker is healthy.

Only GET is supported (other methods return 405); unknown paths return 404. There are no environment, configuration, secret, heap-dump, or remote mutation endpoints. Metric names and values are public to local scrapers: do not put secrets in metric names. There is no authentication or TLS; external access requires a separately configured authenticated proxy. This first listener serves one connection at a time, with bounded request size and read/write timeouts, and is intended for local health checks and scraping.

## Measurements

| Method | Behavior |
| --- | --- |
| `Metrics.increment(name)` | Add one to a counter |
| `Metrics.add(name, amount)` | Add a nonnegative amount to a counter |
| `Metrics.gauge(name, value)` | Set a gauge, including negative values |
| `Metrics.record(name, seconds)` | Record a nonnegative duration; export `<name>_count` and `<name>_sum` |

Numeric values accept integers or decimals and must be finite. Timings are summaries with count and sum; they do not calculate percentiles or histogram buckets. Names must start with an ASCII letter or underscore, contain only ASCII letters, digits, and underscores, and be at most 128 bytes. The `ube_` prefix is reserved. A metric cannot change type or collide with another metric's exported series. The registry accepts up to 1,024 application metrics, has no labels, and synchronizes updates across tasks. Invalid names, values, type conflicts, exhausted capacity, and overflow raise catchable errors. Choose stable names such as `orders_created_total`, not names containing user IDs or request IDs.

Automatic measurements:

- `ube_uptime_seconds`: time since runtime initialization.
- `ube_ready`: application readiness, zero or one.
- `ube_tasks_registered`: registered language task handles, including completed handles retained until disposal or cleanup.
- `ube_tasks_active`: registered tasks whose results have not completed, including waiting tasks. This is not a count of occupied threads.
- `ube_gc_objects`: current tracing-heap object count.
- `ube_gc_reclaimed_total`: objects reclaimed by the tracing collector.
- `ube_process_resident_memory_bytes`: resident memory on Linux when `/proc/self/status` is available; omitted elsewhere.

Collection does not start an HTTP server implicitly. Registering application metrics before `start` is supported. Automatic task and GC measurements are snapshots and may change during a scrape.

This is an initial implementation of metrics and health exposure. Distributed tracing, exporter plugins, dependency health probes, automatic application HTTP instrumentation, suspended-task/queue breakdowns, and attaching endpoints to an existing application server are not implemented.

## Cache measurements

An application using `Observability.start`, or a standalone server started with `ube cache --metrics PORT`, also exports:

| Metric | Type | Meaning |
| --- | --- | --- |
| `ube_cache_hits_total` | Counter | Successful local-store lookups that found an unexpired value |
| `ube_cache_misses_total` | Counter | Local-store lookups returning no value |
| `ube_cache_writes_total` | Counter | Successful sets, inserted adds, increments, and deletions of existing entries |
| `ube_cache_evictions_total` | Counter | Entries evicted while applying live writes |
| `ube_cache_expirations_total` | Counter | Expired entries removed during live operations or metric collection |
| `ube_cache_items` | Gauge | Current unexpired entries across locally registered stores |
| `ube_cache_accounted_bytes` | Gauge | Keys, values, and the fixed per-entry accounting allowance across those stores |

Counters have process lifetime and do not restore from disk. Expiration/eviction pruning performed while reopening a database is not counted as a live removal. A rejected write, an add that returns false, or deletion of a missing key does not increase the write counter. Network/authentication errors are not cache misses.

Remote client processes do not export their server's cache contents through these gauges; scrape the owning server. No keys, values, tokens, or database paths become metric labels. Accounted bytes are not resident process memory; use the separate resident-memory measurement where supported.

For the [embedded server example](../examples/cache/server/App.ube) or a standalone cache started with `--metrics 9090`:

```bash
curl -fsS http://127.0.0.1:9090/health/live
curl -fsS http://127.0.0.1:9090/health/ready
curl -fsS http://127.0.0.1:9090/metrics
```

Cache readiness remains application controlled. The standalone command marks ready after successful startup; it does not continuously probe disk health. See [Cache](cache.md#observability) and [deployment](cache-deployment.md) for operating limits.
