# SQLite

SQLite is bundled into the Ube runtime. No separate database server or SQLite
installation is required. The API supports files, `:memory:`, and SQLite `file:`
URIs. SQL includes normal SQLite DDL, queries, indexes, views, triggers,
constraints, PRAGMAs, savepoints, and attached databases.

## Connections

| Method | Result | Behavior |
| --- | --- | --- |
| `SQLite.open(string path)` | `SQLite` | Open or create a database. |
| `SQLite.openReadOnly(string path)` | `SQLite` | Open an existing database in SQLite read-only mode. |
| `SQLite.version()` | `string` | Bundled SQLite version. |
| `db.execute(string sql)` | `long` | Execute one statement without result columns; return affected rows. |
| `db.query(string sql)` | `SQLiteResult` | Execute one statement with result columns, including `RETURNING` and PRAGMAs. |
| `db.executeBatch(string sql)` | `void` | Execute a SQL script containing multiple statements. |
| `db.prepare(string sql)` | `SQLiteStatement` | Validate one statement and create reusable parameter bindings. |
| `db.begin()` | `void` | Begin a deferred transaction. |
| `db.commit()` | `void` | Commit the current transaction. |
| `db.rollback()` | `void` | Roll back the current transaction. |
| `db.inTransaction()` | `bool` | Whether SQLite autocommit is disabled. |
| `db.busyTimeout(int milliseconds)` | `void` | Set SQLite's busy timeout; zero disables waiting, negative values fail. |
| `db.lastInsertRowId()` | `long` | Last inserted row ID for this connection. |
| `db.changes()` | `long` | SQLite's affected-row count from the most recent completed write. |
| `db.totalChanges()` | `long` | SQLite's cumulative changes on this connection, including changes later rolled back. |
| `db.readOnly()` | `bool` | Whether the main database is read-only. |
| `db.backup(string path)` | `void` | Copy the main database using SQLite's backup API; an existing destination is replaced. |
| `db.restore(string path)` | `void` | Replace the main database from an existing backup file. Requires no active transaction. |
| `db.close()` | `void` | Release the connection and its statements; roll back any uncommitted transaction. |

`execute`, `query`, and `prepare` reject empty SQL and multiple statements. Use
`executeBatch` for scripts. `execute` rejects statements with result columns before
stepping them; use `query` for `INSERT`, `UPDATE`, or `DELETE` with `RETURNING`.
Queries run to completion when called, including their writes, even if no rows
are subsequently read. Result metadata is available even for zero rows.

A batch stops at its first error and is **not implicitly atomic**. Wrap it in a
transaction if all statements must succeed together. Nested transactions use
SQLite savepoints through SQL:

```ube
db.execute('SAVEPOINT changes')
// perform work
db.execute('ROLLBACK TO changes')
db.execute('RELEASE changes')
```

Use `db.execute('BEGIN IMMEDIATE')` or `BEGIN EXCLUSIVE` to choose another
transaction mode. SQLite connection settings retain SQLite's defaults; for
example, enable foreign-key enforcement explicitly with
`db.execute('PRAGMA foreign_keys = ON')` outside a transaction. Use
`db.query('PRAGMA journal_mode = WAL')` when a PRAGMA returns rows.

## Parameters and reusable statements

Bind values instead of interpolating them into SQL. Values are passed separately
to SQLite, preserving quotes, Unicode, and SQL-looking input as data.
Parameters use **one-based** indexes and can be `?`, `?NNN`, `:name`, `@name`, or
`$name`. Named parameters use their exact SQLite prefix.

```ube
final insert = db.prepare('INSERT INTO items(name, active) VALUES (:name, :active)')
try {
    insert.bindString(insert.parameterIndex(':name'), 'Ube')
    insert.bindBool(insert.parameterIndex(':active'), true)
    insert.execute()
    insert.bindString(insert.parameterIndex(':name'), 'Another item')
    insert.execute()
} finally {
    insert.close()
}
```

| Method | Behavior |
| --- | --- |
| `bindInt(int index, int value)` | Bind a 32-bit integer. |
| `bindLong(int index, long value)` | Bind a signed 64-bit integer. |
| `bindDecimal(int index, decimal value)` | Bind a finite floating-point value. |
| `bindBool(int index, bool value)` | Bind integer `0` or `1`. |
| `bindString(int index, string value)` | Bind UTF-8 text. |
| `bindNull(int index)` | Bind SQL NULL. |
| `bindBlobHex(int index, string hex)` | Bind arbitrary binary data from hexadecimal pairs, such as `'00ff80'`. |
| `parameterCount()` | `int`: SQLite parameter slot count (largest slot number for sparse `?NNN`). |
| `parameterName(int index)` | `string`: parameter name with prefix, or `''` for an anonymous parameter. |
| `parameterIndex(string name)` | `int`: parameter slot, or `0` if absent. |
| `clearBindings()` | Reset every parameter to SQL NULL. |
| `execute()` | `long`: execute with current bindings and return affected rows. |
| `query()` | `SQLiteResult`: execute with current bindings and return a result snapshot. |
| `close()` | Release this statement and its bindings. |

Bindings initially contain SQL NULL and persist across executions. Rebind only
the values that change, or use `clearBindings()` first. No explicit execution
reset is needed. Each execution uses the connection's prepared-statement cache;
SQLite handles schema changes on subsequent executions. Statements cannot be
used after their connection closes.

BLOB hex accepts upper- or lowercase pairs without a prefix or separators. `''`
binds an empty BLOB, distinct from NULL. Invalid or odd-length hex raises an
error. This representation preserves all bytes, including NUL and invalid UTF-8.

## Results

Results are materialized snapshots. Close them to release their stored rows.
Call `next()` before accessing each row. Column indexes are **zero-based**.

| Method | Result / behavior |
| --- | --- |
| `next()` | `bool`: advance; false at end. |
| `reset()` | Move before the first row to iterate the same snapshot again. Does not rerun SQL. |
| `rowCount()` | `long`: number of stored rows. |
| `columnCount()` | `int`: number of columns. |
| `columnName(int index)` | `string`: column label or alias. |
| `columnIndex(string name)` | `int`: first case-insensitive matching column, or `-1` if absent. |
| `columnType(int index)` | `string`: current value's `null`, `integer`, `real`, `text`, or `blob` storage type. |
| `getInt(int index)` | `int`: integer, with range checking. |
| `getLong(int index)` | `long`: signed 64-bit integer. |
| `getDecimal(int index)` | `decimal`: real or integer converted to floating point. |
| `getBool(int index)` | `bool`: integer `0` or `1` only. |
| `getString(int index)` | `string`: UTF-8 text. |
| `getBlobHex(int index)` | `string`: BLOB bytes as lowercase hexadecimal pairs. |
| `isNull(int index)` | `bool`: whether the current value is SQL NULL. |
| `close()` | Release the stored rows. |

Use `rows.getString(rows.columnIndex('name'))` for access by column name.
Except for `getDecimal` accepting integers, getters enforce storage types rather
than silently coercing values. Getters on NULL raise an error; check `isNull`
first. Reading before `next()` or after exhaustion also raises an error.
Column names, column counts, and row counts do not require a current row.

## Errors and cleanup

All SQLite failures raise `SQLiteError`, which also matches `Error`. Errors
include SQL syntax, constraints, busy/locked databases, read-only writes,
invalid paths, invalid bindings, type/range mismatches, and closed handles.
The message includes the SQLite failure description when applicable.

```ube
db.begin()
try {
    // perform related writes
    db.commit()
} catch (SQLiteError err) {
    db.rollback()
    Log(err.message())
}
```

Use `finally` to close connections, statements, and results. Repeated `close()`
calls are harmless. Closing a connection invalidates its statements; completed
result snapshots remain usable. Runtime shutdown releases outstanding handles.
A failed SQL statement follows SQLite transaction semantics; it does not
universally roll back the surrounding transaction. Backup/restore failures are
catchable too. Restore never creates a missing source file.

## Current boundaries

This is a synchronous API. Runtime SQLite operations are serialized, and query
results occupy memory proportional to the complete result. Use SQL `LIMIT` and
pagination for large datasets. Text uses Ube's C-string ABI, so embedded NUL text
cannot be returned as a string; use BLOBs for arbitrary binary content.
`getDecimal` converts integers to floating point and can lose integer precision;
use `getLong` for exact integers.

Custom SQL functions/collations, extension loading, incremental BLOB streams,
SQLCipher encryption, and asynchronous/streaming queries are not exposed.
See [the runnable example](../examples/sqlite.ube).
