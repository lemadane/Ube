# ORM: ordinary classes, explicit persistence

Ube's ORM combines fluent queries with direct class mapping. It uses the existing
[SQL execution layer](sql.md), drivers, parameter bindings, errors, pools, and
transactions. It is native to Ube; GORM and Hibernate are design references, not
runtime dependencies.

```ube
class User(long id, var string name, bool active, string? email) {}

class Application {
    main() {
        final db = SQL.open('sqlite::memory:')
        final users = Orm.model(db, User, 'users')
        try {
            users.migrate()
            users.create(User(1L, 'Ada', true, null))
            final user = users.get(1L)
            user.name('Ada Lovelace')
            users.save(user)
            print(user.name())
        } finally { users.close() db.close() }
    }
}
```

Models are ordinary classes with constructor fields. An ORM model must have a
non-null `int` or `long` field named `id`; IDs are assigned by the application.
All mapped fields must be public `int`, `long`, `decimal`, `bool`, or `string`
constructor fields, optionally nullable except for `id`. Sum classes, embedded
objects, collections, binary fields, and relationships are not mapped yet.
Models may contain ordinary methods. Mutability is controlled by Ube's `var`
fields, and does not imply automatic persistence.

There are no mapping annotations or XML files. All constructor fields map to
columns of the same names. The table name is explicit and is quoted as one
identifier component, so SQL keywords and punctuation do not become executable
SQL. Schema-qualified table mapping is not implemented.

## Models and repositories

| Operation | Result | Meaning |
| --- | --- | --- |
| `Orm.model(db, User, 'users')` | `OrmModel<User>` | Creates a typed mapping on an existing `SQL` client. Pass the class, not an instance. |
| `users.migrate()` | `void` | Executes `CREATE TABLE IF NOT EXISTS` for this model. |
| `users.create(user)` | `long` | Inserts all fields, including the application-assigned ID. |
| `users.save(user)` | `long` | Updates all non-ID fields of the row matching `user.id()`. |
| `users.get(id)` | `User` | Fetches by primary key; throws `SQLError` if absent. |
| `users.find(id)` | `User?` | Fetches by primary key; returns `null` if absent. |
| `users.delete(id)` | `long` | Deletes only the row with the supplied primary key. |
| `users.query()` | `OrmQuery<User>` | Creates a new mutable query. |
| `users.using(tx)` | `OrmModel<User>` | Creates a mapping pinned to an existing `SQLTransaction`. |
| `users.close()` | `void` | Releases the mapping handle without closing its SQL connection. |

`create` rejects duplicate IDs through the database's primary-key constraint.
`save` does not insert missing rows and writes zero, false, empty strings, and
NULL values explicitly. It returns the database's affected-row count (typically
0 for a missing ID). There is no implicit upsert or unscoped bulk update/delete.
MySQL/MariaDB's matched/changed-row behavior follows its connection settings.

`migrate()` creates a missing table; it does not alter an existing schema, add
indexes, or perform schema reconciliation. Run it explicitly outside application
transactions. Existing tables must have compatible columns and constraints.
Use reviewed SQL migrations for schema changes. The default mapping uses native
signed integer columns, floating-point columns for `decimal`, text for strings,
and boolean columns (integer 0/1 on SQLite). Exact SQL DECIMAL/NUMERIC mapping is
not provided by Ube's floating-point `decimal` type.

For an optional lookup, use the nullable result explicitly:

```ube
final user = users.find(7L) ?? User(0L, 'Guest', false, null)
```

The compiler preserves the model through `OrmModel<User>`, `OrmQuery<User>`, and
`OrmRows<User>`. These types can also appear in method signatures. Passing an
unrelated class to `create` or `save` is a compile-time error. `get`, `find`, and
row mapping construct ordinary Ube objects in constructor-field order. Each
read produces a fresh object; objects do not have a hidden persistence context.

## Fluent queries

```ube
final query = users.query().where('active', '=', true).whereNull('email').orderBy('id', 'desc').limit(20).offset(0)
try {
    final rows = query.all()
    try {
        for (rows.next()) {
            final user = rows.get()
            print(user.name())
        }
    } finally { rows.close() }
} finally { query.close() }
```

| Query method | Behavior |
| --- | --- |
| `where(column, operator, value)` | Adds an AND comparison with a bound non-null scalar value. |
| `whereNull(column)` / `whereNotNull(column)` | Adds an SQL NULL predicate. |
| `orderBy(column, direction)` | Adds a sort key; direction is `asc` or `desc`. |
| `limit(count)` / `offset(count)` | Sets a nonnegative integer pagination value. |
| `all()` | Returns a materialized `OrmRows<User>` cursor. |
| `first()` | Returns the first matching `User?`, honoring ordering and offset. A zero limit returns null. |
| `count()` | Returns the matching row count, ignoring pagination and ordering. |
| `close()` | Releases the query handle. |

Comparisons support `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, `LIKE`, and `NOT LIKE`.
LIKE operators require a string column. Predicate values are always parameters;
they are never interpolated into SQL. NULL predicates use the explicit NULL
methods. Column names are validated against the model and quoted by the driver
dialect; operators and sort directions are restricted to the supported forms.
Integer values may widen from `int` to `long`; other field types must match.

Literal column names and predicate types are checked at compilation. Dynamic
column strings are checked at runtime. These are checks against the Ube model,
not compile-time validation against a live database. Invalid mappings, runtime
field mismatches, invalid operators, and unsupported query values raise
`SQLError`. Rows are validated before objects are constructed.

Query methods mutate and return the same query handle. Use a fresh `query()` for
an independent filter set; assigning a query to another variable does not clone
it. Execution takes a snapshot. Queries support at most 128 predicates and 128
sort keys, and retain the SQL parameter/result size limits. Joins, OR/grouped
predicates, projections, aggregates other than count, and streaming are future
extensions.

`OrmRows<User>` supports `next()`, `get()`, `rowCount()`, `reset()`, and `close()`.
Call `next()` before `get()`. Rows are detached from the database connection and
remain readable after it closes. Close result, query, and repository handles in
`finally` blocks; repeated close is safe. Closing a repository releases that
handle; already-created queries retain their mapping, but still require an
active underlying connection or transaction to execute.

## Shared transactions with raw SQL

```ube
final tx = db.begin()
final transactionalUsers = users.using(tx)
try {
    transactionalUsers.create(User(2L, 'Grace', true, null))
    // tx.execute(sql, params) uses this same reserved connection.
    tx.commit()
} finally {
    transactionalUsers.close()
    tx.close()
}
```

`using(tx)` rejects a transaction owned by another SQL client. Commit and
rollback remain explicit. After either, the transaction-bound repository cannot
execute more operations. Calling `users` instead of `transactionalUsers` uses
the parent pool, outside that transaction. SQL driver failures use the existing
rollback behavior; `finally` covers ORM validation failures and arbitrary Ube
exceptions. Closing the parent SQL client rolls back unfinished transactions.
Nested units of work can use the SQL transaction's savepoint methods.

Changes to a loaded object stay in memory until `save` is called. Automatic
dirty tracking, identity caches, lazy relationships, cascading writes,
auto-generated IDs, optimistic versioning, lifecycle hooks, and automatic
migration diffs are not included in this initial API.

See [the runnable example](../examples/orm.ube). End-to-end coverage compiles
Ube programs through the CLI and verifies object persistence, nullable fields,
CRUD, fluent queries, and shared ORM/raw-SQL transactions:

```bash
cargo test -p ube-compiler --test sql_e2e --offline
```

Live driver tests require disposable databases; they create/drop the table
`ube_orm_integration`. Set `UBE_TEST_POSTGRES_URL` and/or `UBE_TEST_MYSQL_URL` and
select the corresponding ignored test, for example:

```bash
cargo test -p ube-runtime orm::tests::postgres_live -- --ignored
cargo test -p ube-runtime orm::tests::mysql_live -- --ignored
```
