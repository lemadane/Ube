# Ube documentation

Start with the [project README](../README.md) for language syntax, toolchain setup, and project commands. Guides below describe the implemented APIs and their current limits. Each runnable source must have the same filename as its declared class, including case.

## Strings and text blocks

- [Syntax, interpolation, escapes, and migration notes](../README.md#23-string-literals--text-blocks)
- [Language specification](../LANGUAGE_SPEC_DRAFT.md#15-string-literals-and-interpolated-text-blocks)
- [Runnable text-block example](../examples/text-blocks/App.ube)

## Language loops

Use `each (item from items)` for collection iteration, including comprehensions, and `for` for conditional and infinite loops. See the [loop syntax and examples](../README.md#8-each-and-for-loop-use-cases-with-break-and-continue) and the [runnable loop sample](../examples/loops/App.ube).

## Cache

- [Cache reference](cache.md): constructors, methods, TTL, eviction, capacity, persistence, and test coverage
- [Runnable examples](../examples/cache/README.md): complete source files, commands, and expected outputs
- [App recipes](cache-recipes.md): cache-aside fallback, atomic counters, invalidation, and uncertain mutation outcomes
- [Deployment and recovery](cache-deployment.md): standalone options, embedded hosting, multi-machine configuration, backup, and troubleshooting
- [TCP protocol](cache-protocol.md): framing, authentication, fields, results, and limits

## Runtime and application services

- [Function values](functions.md): typed functions, captures, block lambdas, and `Async.start`
- [Static members](static-members.md): fields, methods, qualification, and initialization

- [Error stack traces](stacktraces.md): captured call frames, task submission/await chains, readable diagnostics, and rethrows
- [Concurrency](concurrency.md): tasks, channels, cancellation, cleanup, yielding, and backend limits
- [Observability](observability.md): health, readiness, metrics, and cache measurements
- [Memory](memory.md): allocation, ownership, garbage collection, and task roots
- [Environment and secrets](environment.md): encrypted configuration and runtime access
- [Authentication](authentication.md): JWT and OAuth APIs
- [Sessions](sessions.md): application sessions and cookies
- [Financial types](financial.md): money and related calculations

## Serialization and storage

- [KeyValueStore](key-value-store.md): persistent JSON records, ownership, durability, and native stress tests
- [XML](xml.md): automatic object/list/map serialization, documents, options, namespaces, and traversal
- [JSON](json.md): object/list/map serialization, real parsing/getters, and unsupported values
- [Runnable persistent-object example](../examples/key-value-store/README.md)

- [SQL](sql.md): SQL access and parameters
- [Query builder](query-builder.md): model-free SELECT queries, projections, filters, and transactions
- [SQLite](sqlite.md): embedded SQL storage
- [ORM](orm.md): models, grouped predicates, projections, aggregates, joins, and transactions

## Networking

- [Request bodies](request-bodies.md): body framing, limits, and error handling
- [HTTP/2](http2.md)
- [HTTP/3](http3.md)

## Testing and project references

- [Testing](testing.md): Testicle and native serialization end-to-end regressions
- [Mockery](mockery.md)
- [RestInPeace](rest-in-peace.md): HTTP application tests
- [Stabilization](stabilization.md)
- [Language specification](../LANGUAGE_SPEC_DRAFT.md)
- [Compiler/runtime architecture](../ARCHITECTURE.md)
- [Milestones](../MILESTONES.md)
