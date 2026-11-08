# Unified SQL execution

`SQL` connects to PostgreSQL, MySQL/MariaDB, and SQLite through one execution
API. Raw SQL, the [standalone query builder](query-builder.md), and the ORM share its
parameter values, result metadata, connection pools, errors, and transaction
handles. The existing [SQLite API](sqlite.md) remains available for
SQLite-specific features such as backup and restore.

The [typed ORM](orm.md) now uses this execution foundation for class mapping,
CRUD, fluent model queries, create-if-missing tables, and transaction-bound
repositories. `db.table(name)` and `tx.table(name)` build model-free SELECT
queries with projections, bound filters, ordering, and pagination. Live-schema-checked
SQL, migration diffs, streaming, and asynchronous SQL built-ins are not implemented
yet. SQL methods block their calling Ube thread while database IO runs on
runtime workers. They do not return `Task` values. Runtime typed accessors are
not compile-time database-schema validation.

## Opening a database

```ube
final db = SQL.open('sqlite::memory:')
// For a new file: SQL.open('sqlite://app.db?mode=rwc')
// PostgreSQL: SQL.open('postgres://user:password@localhost:5432/app')
// MySQL: SQL.open('mysql://user:password@localhost:3306/app')
// MariaDB: SQL.open('mariadb://user:password@localhost:3306/app')
```

`postgresql://` is also accepted. File SQLite URLs open an existing file unless
`mode=rwc` requests creation; `mode=ro` opens read-only. Paths are relative to
the working directory. Two `sqlite::memory:` handles have separate databases.

| Method | Result | Behavior |
| --- | --- | --- |
| `SQL.open(url)` | `SQL` | Opens a client with pool limit 5 and timeout 5000 ms. |
| `SQL.openPool(url, maxConnections, timeoutMs)` | `SQL` | Pool limit 1–128; timeout 1–300000 ms. |
| `db.dialect()` | `string` | `postgres`, `mysql` (also MariaDB), or `sqlite`. |
| `db.placeholder(index)` | `string` | PostgreSQL `$1`, `$2`, …; MySQL/SQLite `?`. Indexes are 1–32766. |
| `db.quoteIdentifier(name)` | `string` | Quotes one identifier component for this database. |
| `db.execute(sql, params)` | `long` | Runs one statement and returns affected rows. |
| `db.query(sql, params)` | `SQLResult` | Runs one statement and materializes its rows. |
| `db.table(name)` | `SQLQuery` | Creates a model-free SELECT builder; see [query builder](query-builder.md). |
| `db.begin()` | `SQLTransaction` | Reserves a connection and begins a transaction. |
| `db.close()` | `void` | Invalidates the client, rolls back its outstanding transactions, and closes its pool. |

Opening verifies connectivity immediately. Network databases use pools with
prepared-statement caching. SQLite uses one persistent connection per client,
regardless of the requested pool size, preserving in-memory data and transaction
identity. A transaction holds its reserved connection until it finishes. When
all connections are reserved, other operations wait up to their timeout; use
`tx.query/execute` inside a transaction rather than its parent `db` methods.

Network databases default to verified TLS (`sslmode=verify-full` for PostgreSQL,
`ssl-mode=verify_identity` for MySQL), except localhost, 127.0.0.1, and ::1, which
default to TLS disabled for local development. Explicit URL options override
these defaults. PostgreSQL `sslrootcert` and MySQL `ssl-ca` configure trusted
CA files; normal driver URL options are supported. Do not embed credentials in
source files. SQL statement logging is disabled, and errors omit query text,
bound values, connection URLs, and server-provided messages. Database rejection
errors include the database's error code.

The timeout bounds pool acquisition and query execution together. A timed-out
network connection is discarded. A timeout does not guarantee the server
canceled an operation or that a write did not commit. SQLite's worker may finish
an ongoing operation after the call times out. Cleanup has its own timeout.
Do not automatically retry writes after an ambiguous timeout or commit failure.

## Typed parameters

```ube
final params = SQLParams.create()
params.addInt(7)
params.addString('O\'Brien')
db.execute('INSERT INTO users(id, name) VALUES (?, ?)', params)
params.clear()
params.close()
```

Always pass a `SQLParams` handle, including an empty one for statements without
parameters. Values are bound through the driver and never concatenated into
SQL. Ordinary Ube string interpolation is **not** automatic SQL binding.

| Method | Binding |
| --- | --- |
| `SQLParams.create()` | Creates an empty reusable parameter list. |
| `params.addInt(value)` | Signed 32-bit integer. |
| `params.addLong(value)` | Signed 64-bit integer. |
| `params.addDecimal(value)` | Finite double-precision floating point; not an exact SQL DECIMAL. |
| `params.addBool(value)` | Boolean; SQLite stores it as 0/1. |
| `params.addString(value)` | Non-null UTF-8 string. |
| `params.addBlobHex(hex)` | Binary bytes encoded as an even-length hex string. Empty blobs are allowed. |
| `params.addNull(type)` | Typed SQL NULL: `int`, `long`, `decimal`, `bool`, `string`, or `blob`. |
| `params.count()` | Parameter count (`int`). |
| `params.clear()` | Removes bindings for reuse. |
| `params.close()` | Releases the handle; repeated close is safe. |

Use native placeholders: PostgreSQL `$1`, `$2`, …; MySQL and SQLite `?`.
PostgreSQL repeated `$1` references reuse one parameter. Bind types must match
the database schema; explicit SQL casts can disambiguate types. Queries are
prepared before execution and reject a mismatched parameter count. SQLite named
or numbered placeholders use their native binding index order; portable code
should use positional placeholders. A call snapshots the bindings, so later
changes to a shared parameter handle cannot change an in-flight query.

The binding limit is 32766 values and 16 MiB of payload per call. Values cannot
represent table/column identifiers or SQL expressions. For dynamic identifiers,
use `quoteIdentifier` separately for each component and join components with
`.`. For example, quoting `public.users` produces a **single** identifier named
`public.users`, not a schema-qualified name. Query builders can use this helper
and `placeholder` without guessing database dialect syntax. Identifier quoting
does not authorize access to arbitrary user-selected tables.

## Transactions and savepoints

```ube
final tx = db.begin()
try {
    tx.execute('UPDATE accounts SET balance = balance - 10 WHERE id = 1', params)
    tx.execute('UPDATE accounts SET balance = balance + 10 WHERE id = 2', params)
    tx.commit()
} finally {
    tx.close()
}
```

In this example, `params` must be empty. The transaction executes all operations
on the same reserved connection. Commit occurs only when explicitly requested;
`close()` rolls back an unfinished transaction. Use `finally` to cover arbitrary
Ube exceptions. Dropping a Ube variable does not deterministically close it.

| Method | Behavior |
| --- | --- |
| `tx.execute(sql, params)` | Same affected-row result as `db.execute`. |
| `tx.query(sql, params)` | Same detached result API as `db.query`. |
| `tx.savepoint(name)` | Creates a safely quoted savepoint. |
| `tx.rollbackTo(name)` | Rewinds to that savepoint without ending the transaction. |
| `tx.release(name)` | Releases the savepoint. |
| `tx.commit()` | Commits and invalidates the transaction handle. |
| `tx.rollback()` | Rolls back and invalidates it; safe when already finished. |
| `tx.close()` | Same rollback behavior; safe after commit or an SQL error. |

Driver errors, binding-count errors, result-conversion errors, result-limit
errors, and execution timeouts inside a transaction trigger an attempted full
rollback and invalidate the transaction. Errors rejected before execution (such
as invalid handles or a multiple-statement string) do not execute SQL; the
`finally` block is still needed. A savepoint-operation failure also invalidates
the transaction. No automatic retries occur. Rollback after a network failure
is best effort; commit failures can have an uncertain outcome.

Raw transaction-control statements and session `SET` commands are rejected;
use the transaction API. Multi-statement scripts are rejected before execution.
Native database semantics still apply: MySQL/MariaDB DDL can implicitly commit,
and nontransactional storage engines cannot provide rollback guarantees. Use a
transactional engine such as InnoDB and keep DDL outside application transactions.

## Results

```ube
final rows = db.query('SELECT id, name FROM users', params)
try {
    for (rows.next()) {
        print(rows.getInt(0))
        print(rows.getString(rows.columnIndex('name')))
    }
} finally {
    rows.close()
}
```

Column indexes start at 0. Call `next()` before reading row values. Results are
detached and remain usable after their connection or transaction closes.

| Method | Result |
| --- | --- |
| `rows.next()` | `bool`: advances; repeated calls after the end return false. |
| `rows.reset()` | Rewinds to before the first row. |
| `rows.rowCount()` | `long`: materialized row count. |
| `rows.columnCount()` | `int`: number of columns, including empty results. |
| `rows.columnName(index)` | Column name. |
| `rows.columnIndex(name)` | Case-sensitive name lookup, `-1` if absent; duplicate names raise an error. |
| `rows.columnType(index)` | Current value kind: `null`, `bool`, `integer`, `real`, `text`, or `blob`. |
| `rows.isNull(index)` | Tests SQL NULL. |
| `rows.getInt(index)` | Signed 32-bit integer, checked for overflow. |
| `rows.getLong(index)` | Signed 64-bit integer. |
| `rows.getDecimal(index)` | Floating-point value, or an integer converted to floating point. |
| `rows.getBool(index)` | Boolean, or integer 0/1. |
| `rows.getString(index)` | UTF-8 text. |
| `rows.getBlobHex(index)` | Binary value as lowercase hex. |
| `rows.close()` | Releases rows; repeated close is safe. |

Getters reject incorrect types, SQL NULL, invalid indexes, and an unpositioned
cursor with catchable `SQLError`. Test `isNull()` before calling a getter on a
nullable column. Use aliases for duplicate column names. Embedded NUL text
cannot be represented by Ube strings; retrieve it as binary data instead.

The initial cross-database scalar set covers signed integers, boolean values,
floating point, text, binary data, and NULL. Other native types (for example
exact DECIMAL/NUMERIC, UUID, timestamps, JSON, arrays, unsigned integers beyond
the signed range, or custom types) are not yet a portable mapping contract;
cast them to a supported scalar or text in SQL. Integer-to-floating conversion
can lose precision. For money/exact decimals, transfer decimal strings and keep
exact calculations in a suitable database or application representation.

Results are limited to 100000 rows and 16 MiB of value payload. Use `LIMIT` and
pagination until streaming exists. `query()` can also execute SQL with side
effects, such as `INSERT ... RETURNING`; a result-limit error outside a
transaction does not promise those effects were rolled back.

See [the runnable SQLite example](../examples/sql/App.ube). To run live driver tests
against isolated disposable databases, set `UBE_TEST_POSTGRES_URL` and
`UBE_TEST_MYSQL_URL`, then run:

```bash
cargo test -p ube-runtime sql::tests -- --include-ignored --nocapture
```

The live test helper creates and drops `ube_sql_transactions`. Use dedicated
test databases. The live tests are marked ignored in the default suite. To run just one driver,
select `sql::tests::postgres_live` or `sql::tests::mysql_live` with `--ignored`
and set its corresponding environment variable. SQLite tests run without external
services.

The SQLite end-to-end test compiles programs through the Ube CLI, writes a real
SQLite file, and reopens it in separate processes through both SQL APIs. It
checks parameter binding, persistence, savepoints, rollback, and read-only access:

```bash
cargo test -p ube-compiler --test sql_e2e --offline
```
