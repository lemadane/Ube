# SQL query builder without model classes

Use `db.table(name)` to build a SELECT query without declaring a mapped Ube
class. The builder uses the existing [SQL engine](sql.md), connection pool,
parameter bindings, driver dialect, errors, and materialized `SQLResult` rows.
Use `tx.table(name)` to execute on an existing transaction's reserved connection.

```ube
final query = db.table('users').select('name').where('active', '=', true).orderBy('name', 'asc').limit(20)
try {
    final rows = query.all()
    try {
        for (rows.next()) { print(rows.getString(0)) }
    } finally { rows.close() }
} finally { query.close() }
```

The connection must refer to a database with the requested table. Unlike
`Orm.model(...)`, no class metadata or primary key is required. Rows are read
with the existing SQL result getters, by index or `columnIndex(name)`.

## API

| Operation | Result | Behavior |
| --- | --- | --- |
| `db.table(name)` / `tx.table(name)` | `SQLQuery` | Creates an independent SELECT builder. |
| `select(column)` | `SQLQuery` | Appends a projection; repeat to select multiple columns. Defaults to `*` when no projection is specified. |
| `where(column, operator, value)` | `SQLQuery` | Adds an AND comparison with a bound non-null scalar. |
| `whereNull(column)` / `whereNotNull(column)` | `SQLQuery` | Adds an explicit SQL NULL predicate. |
| `orderBy(column, direction)` | `SQLQuery` | Appends a sort key; direction is `asc` or `desc`, case insensitive. |
| `limit(count)` / `offset(count)` | `SQLQuery` | Sets a nonnegative `int` pagination value. |
| `all()` | `SQLResult` | Executes the current SELECT and returns a materialized cursor. |
| `first()` | `SQLResult` | Executes with a limit of one, or zero if `limit(0)` was set; honors offset. Returns a cursor with zero or one row. |
| `count()` | `long` | Counts matching rows, ignoring projections, ordering, limit, and offset. |
| `toSql()` | `string` | Returns SELECT SQL with dialect-specific placeholders; does not execute it or include bound values. |
| `clone()` | `SQLQuery` | Returns an independent snapshot, including bindings and execution target. |
| `close()` | `void` | Releases this builder; repeated close is safe. |

`SQLQuery` can appear in method parameters and return types. Create it through a
connection or transaction; `SQLQuery()` is not a constructor.

`where` accepts `int`, `long`, `decimal`, `bool`, and `string` values; `byte`
widens to `int`. Supported operators are `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`,
`LIKE`, and `NOT LIKE`. LIKE operators require a string value. Use explicit NULL
predicates instead of passing nullable values. Non-finite decimals are rejected.

Table names are quoted as one identifier component. Column names can use dotted
components, such as `users.name`. Projections also accept `*` and `users.*`.
Identifiers are quoted by the driver dialect; expressions, aliases, and
comma-separated projection lists are not parsed. For two columns, write
`select('id').select('name')`. Column existence and database type compatibility
are checked by the database at execution, rather than against a Ube model.

## Mutation, execution, and lifetime

Fluent methods mutate and return the same builder. Assigning it to another
variable creates an alias; use `clone()` for independent filters or projections.
Each execution snapshots the builder and executes again, so later database
changes can affect subsequent calls. Registry locks do not span database IO.

All execution is explicit and synchronous, following the existing SQL API.
`all()` and `first()` leave the cursor before its first row: call `next()` before
reading values. An empty `first()` result is a non-null cursor whose `next()`
returns false. Results remain readable after closing the query or connection.
Close cursors and builders explicitly in `finally` blocks.

```ube
final tx = db.begin()
final query = tx.table('users').where('active', '=', true)
try {
    // Raw SQL and ORM repositories using this tx share the same connection.
    print(query.count())
    tx.commit()
} finally { query.close() tx.close() }
```

Queries cannot execute after their connection closes or their transaction ends.
Execution failures use the SQL engine's rollback rules and raise `SQLError`.
Builder validation errors also raise `SQLError`; they do not execute SQL or
implicitly roll back a transaction. Invalid fluent calls leave the builder
unchanged.

Each builder allows up to 128 projections, 128 predicates, 128 sort keys, and
16 MiB of retained query text and values. SQL result and parameter limits still
apply. Runtime cleanup releases remaining builders.

This initial SELECT API does not provide joins, grouped/OR predicates,
subqueries, grouping, aggregates other than count, distinct, schema-qualified
table names, write builders, streaming, or schema migrations. Use raw SQL for
those operations, or the [typed ORM](orm.md) for class mapping and its supported
joins.

See [the runnable example](../examples/query-builder/App.ube).
