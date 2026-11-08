# ORM: ordinary classes, explicit persistence

Ube's ORM combines fluent queries with direct class mapping. It uses the existing
[SQL execution layer](sql.md), drivers, parameter bindings, errors, pools, and
transactions. It is native to Ube; GORM and Hibernate are design references, not
runtime dependencies.

```ube
class User(long id, var string name, bool active, string? email)

class App {
    start() {
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
| `orWhere(column, operator, value)` | Adds an OR comparison with the same column and value checks. |
| `whereGroup(query)` / `orWhereGroup(query)` | Appends a snapshot of another query's predicates in parentheses, using AND or OR. |
| `orWhereNull(column)` / `orWhereNotNull(column)` | Adds an OR SQL NULL predicate. |
| `whereNull(column)` / `whereNotNull(column)` | Adds an SQL NULL predicate. |
| `orderBy(column, direction)` | Adds a sort key; direction is `asc` or `desc`. |
| `limit(count)` / `offset(count)` | Sets a nonnegative integer pagination value. |
| `all()` | Returns a materialized `OrmRows<User>` cursor. |
| `first()` | Returns the first matching `User?`, honoring ordering and offset. A zero limit returns null. |
| `count()` | Returns the matching row count, ignoring pagination and ordering. |
| `countColumn(column)` | Counts matching rows whose column is non-NULL, ignoring pagination and ordering. |
| `sum(column)` / `avg(column)` | Returns `decimal?` for a numeric column, or null when no non-NULL values match. |
| `min(column)` / `max(column)` | Returns the nullable column type for numeric or string columns. |
| `select(column)` | Creates an `OrmProjection<Model>` snapshot that returns `SQLResult` cursors. |
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

Filter, ordering, and pagination methods mutate and return the same query
handle. Use a fresh `query()` for an independent filter set; assigning a query to another variable does not clone
it. Execution takes a snapshot. Queries support at most 128 predicate nodes
(including groups), 16 levels of group nesting, 128 sort keys, and 16 MiB of predicate text and values. SQL
parameter/result size limits still apply. Streaming is a future extension.

`OrmRows<User>` supports `next()`, `get()`, `rowCount()`, `reset()`, and `close()`.
Call `next()` before `get()`. Rows are detached from the database connection and
remain readable after it closes. Close result, query, and repository handles in
`finally` blocks; repeated close is safe. Closing a repository releases that
handle; already-created queries retain their mapping, but still require an
active underlying connection or transaction to execute.

## Parenthesized OR groups

```ube
final names = users.query().where('name', '=', 'Ada').orWhere('name', '=', 'Grace')
final active = users.query().where('active', '=', true).whereGroup(names)
try {
    // WHERE active = ? AND (name = ? OR name = ?)
    print(active.count())
} finally { active.close() names.close() }
```

`orWhere`, `orWhereNull`, and `orWhereNotNull` use normal SQL AND/OR precedence.
Use `whereGroup` or `orWhereGroup` for parentheses; groups may contain nested
groups. A leading OR is treated as the first predicate without an OR prefix.

Groups copy only the source query's predicates at the time of the call. Later
source changes or closing the source do not affect the destination. Projections, ordering,
and pagination are not copied. The source must have at least one predicate and
use the same model mapping, SQL connection or transaction, and complete join
configuration. Model/join types are checked at compilation; SQL targets and
mapping details are checked at runtime. Invalid group additions leave the
destination unchanged. Keep both handles and close them independently.

## Projections and aggregate values

```ube
final base = users.query().where('active', '=', true).orderBy('id', 'asc')
final selected = base.select('name').select('email')
try {
    final rows = selected.all()
    try {
        for (rows.next()) {
            print(rows.getString(rows.columnIndex('name')))
            print(rows.isNull(rows.columnIndex('email')))
        }
    } finally { rows.close() }
} finally { selected.close() base.close() }
```

The first `select(column)` on an `OrmQuery<Model>` creates a new
`OrmProjection<Model>` snapshot. The original query still returns complete model
objects. Further `select` calls append columns to the same projection handle.
Projections retain model and join validation through filters, ordering,
pagination, and joins, and can appear in method signatures. Dynamic column
strings are validated at runtime; literal columns are checked at compilation.
Up to 128 projection columns and 16 MiB of projection text are allowed.

Projected `all()` and `first()` return `SQLResult`, rather than partial model
objects. Both leave the cursor before its first row. Projected `first()` returns
a cursor with zero or one row and honors offset and `limit(0)`. Call `next()`
before accessing values, use `isNull()` for nullable values, and close the result.
Column labels match the strings passed to `select`, including qualified labels
such as `o.amount`; select qualified columns to distinguish joined fields.
Nullable columns introduced by a LEFT JOIN remain NULL in projected results.

Aggregates operate on the complete filtered/joined row set, ignoring selected
columns, ordering, limit, and offset:

| Method | Result |
| --- | --- |
| `count()` | `long`: count of matching rows, including join duplicates. |
| `countColumn('email')` | `long`: count of non-NULL column values. |
| `sum('score')` / `avg('score')` | `decimal?`: numeric result; null for no non-NULL values. |
| `min('id')` / `max('id')` | Nullable source-column type; numeric and string columns supported. |

Aggregate column names must be string literals, so the compiler can validate
types and preserve nullable result types. `sum` and `avg` require numeric columns.
`min` and `max` accept numeric or string columns; boolean columns are supported
by `countColumn`. SQL NULL values are excluded. A count returns zero when nothing
matches; the other aggregates return null. `sum` and `avg` use Ube's floating-point
`decimal`, including for integer inputs; large integer sums can lose precision.
Use raw SQL when exact database NUMERIC/DECIMAL results are needed.

For example, `final score = base.avg('score') ?? 0.0` handles an empty average.
These are scalar aggregate methods, not GROUP BY/HAVING or aggregate-expression
projections. Those operations remain available through raw SQL.

See [the runnable example](../examples/orm-queries/App.ube).

## Joining related models

```ube
final base = users.query()
final paid = base.join(orders, 'o', 'id', 'userId').where('o.status', '=', 'paid')
try {
    final rows = paid.all()
    try {
        for (rows.next()) { print(rows.get().name()) }
    } finally { rows.close() }
} finally { paid.close() base.close() }
```

`join(repository, alias, sourceColumn, targetColumn)` creates an INNER JOIN;
`leftJoin(...)` creates a LEFT JOIN with the same arguments. The example joins
`base.id = o.userId` and returns typed `User` objects. `orders` is another
`Orm.model(...)` repository on the same SQL client or exactly the same
`SQLTransaction`. Joining a transaction-bound repository to a pool-bound one,
a different client, or a different transaction raises `SQLError`.

Both join methods return a **new** query containing a snapshot of the source
query's filters, joins, ordering, and pagination. The source remains unchanged;
close both handles. Existing filter and pagination methods continue to mutate
and return their own query handle. Joined query execution follows SQL clause
order: filtering and pagination apply to the complete joined result.

Unqualified columns refer to the base model; `base.id` is its explicit form.
Joined columns use their explicit aliases, such as `o.status`, in `where`,
`whereNull`, `whereNotNull`, and `orderBy`. The alias must be a literal ASCII
identifier of 1–63 characters, unique without regard to case, and cannot be
`base`. Alias references are case-sensitive. Literal column names and join-key
types are checked at compilation; dynamic column strings are validated at
runtime. Join keys must have matching scalar types, allowing int/long pairs;
nullable keys use normal SQL NULL equality semantics.

Chaining can refer to an earlier join's columns, for example
`paid.join(lines, 'line', 'o.id', 'orderId')`. Self-joins work with a distinct
alias: `employees.query().join(employees, 'manager', 'managerId', 'id')`.
Queries support up to 16 joins. A LEFT JOIN followed by `whereNull('o.id')`
finds base records without a matching related record.

Results contain **only the base model's fields**. A base object appears once
per matching joined row; `count()` counts those joined rows, including repeated
base IDs. `first`, ordering, and pagination operate on joined rows as well.
Distinct entity results, relationship hydration,
RIGHT/FULL/CROSS joins, and custom ON expressions are not implemented yet.
This is SQL JOIN, not a set-operation intersection.

See [the runnable join example](../examples/orm-joins/App.ube).

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

See [the runnable example](../examples/orm/App.ube). End-to-end coverage compiles
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
