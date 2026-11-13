# USQL: Ube SQL Templates

`sql` followed by a backtick template constructs an immutable `SQLStatement`. Literal SQL and interpolated values stay separate: every `${expression}` becomes a typed bound parameter. Creating a statement does not execute it or open a connection.

```ube
final name = 'O\'Brien'
final statement = sql`
    SELECT id, name FROM users
    WHERE name = ${name}
`
final rows = db.query(statement)
```

Run the [complete example](../examples/usql/App.ube) from the repository root:

```bash
cargo build --workspace
./target/debug/ube-compiler check examples/usql/App.ube
./target/debug/ube-compiler run examples/usql/App.ube
```

Expected output:

```text
1: O'Brien
```

## Statements and execution

| Call | Result | Behavior |
| --- | --- | --- |
| `db.query(statement)` | `SQLResult` | Execute against a connection and materialize rows. |
| `db.execute(statement)` | `long` | Execute against a connection and return affected rows. |
| `tx.query(statement)` | `SQLResult` | Query using the transaction's connection. |
| `tx.execute(statement)` | `long` | Execute using the transaction's connection. |

These overloads take one non-null `SQLStatement`. Existing `query(string, SQLParams)` and `execute(string, SQLParams)` calls remain supported. The separate `SQLite` API does not accept `SQLStatement`; use `SQL.open('sqlite::memory:')` for templates.

A statement can be passed to a method, returned, captured by a closure, selected with `then`/`else`, and executed repeatedly. Expressions evaluate once, from left to right, when the template is constructed. The statement snapshots their values; changing an original variable does not change its bindings. Each execution runs the SQL again against the current database state.

Statements have no connection affinity. Execution generates `$1`, `$2`, … placeholders for PostgreSQL and `?` placeholders for MySQL/MariaDB and SQLite. SQL syntax itself is not translated between dialects.

`SQLStatement` is a reserved type constructed only by checked templates. Ordinary strings are not assignable to it, and `SQLStatement()` is not a constructor. Statements are garbage-collected, including partially built templates whose later interpolation fails. They require no `close()` call. Connections, transactions, and result cursors retain their existing explicit cleanup rules.

## Binding types

Supported interpolations are non-null `string`, `byte`, `int`, `long`, `decimal`, and `bool`. A `byte` widens to an integer. Strings are copied as parameter values, including apostrophes and SQL-looking text. `decimal` uses the existing floating-point SQL binding and must be finite; it is not an exact SQL DECIMAL type.

Nullable values, bare `null`, objects, functions, lists, and nested `SQLStatement` fragments are rejected. Use the existing `SQLParams` API for typed NULL or blob bindings. Templates share the SQL parameter limit of 32766 values and impose a 16 MiB construction budget on literal text and bindings, with a small minimum charge per segment. Result and execution limits remain those of the [SQL API](sql.md).

## SQL contexts

Write interpolations directly in value positions, without SQL quotes:

```ube
// Supported: name is a bound value.
final good = sql`SELECT id FROM users WHERE name = ${name}`

// Rejected: interpolation inside a SQL string literal.
final bad = sql`SELECT id FROM users WHERE name = '${name}'`
```

The compiler performs conservative lexical checks for supported database quoting modes. It rejects interpolation inside strings, quoted identifiers, PostgreSQL dollar-quoted text, and SQL comments. It also rejects interpolation embedded in an identifier/number token and direct interpolation after common identifier-taking keywords such as `FROM` or `JOIN`.

Table and column names must remain static. An interpolation in `SELECT ${name}` is a parameter value, never a dynamic column reference. This first version does not provide identifier helpers, raw fragments, nested template composition, or list expansion for `IN` clauses.

Use `${value}` for every parameter. Native placeholders such as `?`, `$1`, `:name`, and `@name` are rejected outside quoted text or comments. The initial scanner also rejects `?` SQL operators, MySQL/MariaDB executable comments, and some dialect-specific constructs that are ambiguous under another supported quoting mode. Use the existing raw SQL API with explicit bindings for such statements.

Backticks use ordinary Ube text-block escapes. To include an SQL backtick identifier delimiter, write a backslash before it in Ube source. Static SQL strings should use doubled SQL apostrophes for portable quoting. This is a bounded lexical checker, not a complete SQL parser or schema checker; the database checks SQL grammar, table/column existence, and compatible parameter types at execution.

## Errors and transactions

Construction limit failures and non-finite decimals raise `SQLError`. Execution uses the existing synchronous SQL engine: prepared binding, timeouts, result limits, and transaction rollback rules apply unchanged. Multiple statements and raw transaction-control commands are rejected by the execution layer; use transaction methods for commit, rollback, and savepoints.

Ordinary backtick strings still produce strings and interpolate text. They do not acquire SQL binding automatically. Use the `sql` prefix when you want a checked statement with bound parameters.

## Verification

```bash
cargo test -p ube-compiler --test usql_e2e
cargo test -p ube-runtime sql::templates::tests --lib
```

The native tests cover SQLite execution, hostile string values, repeated execution, value snapshots, closure captures, transaction behavior, context/type diagnostics, evaluation order, and GC reclamation. Runtime tests check placeholder generation and binding types for all three dialects; those tests do not require live PostgreSQL or MySQL servers.
