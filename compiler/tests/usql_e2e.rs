//! USQL through the real compiler, native executable, and SQLite engine.
use std::{fs, process::Command};

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let result = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
}

#[test]
fn values_are_bound_and_statements_snapshot_values_and_are_reusable() {
    run(
        r#"class App {
        SQLStatement find(string name) { return sql`SELECT name FROM users WHERE name = ${name}` }
        start() {
            final db = SQL.open('sqlite::memory:')
            db.execute(sql`CREATE TABLE users (id INTEGER, name TEXT, active INTEGER, score REAL)`)
            final name = 'O\'Brien; DROP TABLE users; --'
            print(db.execute(sql`INSERT INTO users VALUES (${7}, ${name}, ${true}, ${1.5})`))
            var selected = name
            final statement = this.find(selected)
            selected = 'different'
            Gc.collect()
            final rows = db.query(statement)
            rows.next()
            print(rows.getString(0))
            rows.close()
            final again = db.query(statement)
            print(again.rowCount())
            again.close()
            final values = db.query(sql`SELECT id, active, score, ${5000000000L} FROM users`)
            values.next()
            print(values.getInt(0))
            print(values.getBool(1))
            print(values.getDecimal(2) == 1.5)
            print(values.getLong(3))
            values.close()
            final params = SQLParams.create()
            params.addInt(7)
            final legacy = db.query('SELECT name FROM users WHERE id = ?', params)
            print(legacy.rowCount())
            legacy.close()
            params.close()
            db.close()
        }
    }"#,
        "1\nO'Brien; DROP TABLE users; --\n1\n7\ntrue\ntrue\n5000000000\n1\n",
    );
}

#[test]
fn transactions_closures_conditionals_and_errors_use_existing_sql_semantics() {
    run(
        r#"class App { start() {
        final db = SQL.open('sqlite::memory:')
        db.execute(sql`CREATE TABLE items (id INTEGER PRIMARY KEY)`)
        final tx = db.begin()
        final id = 42
        final make = () -> sql`INSERT INTO items VALUES (${id})`
        Gc.collect()
        print(tx.execute(make()))
        final query = true then sql`SELECT id FROM items` else sql`SELECT 0`
        final rows = tx.query(query)
        rows.next()
        print(rows.getInt(0))
        rows.close()
        tx.rollback()
        final empty = db.query(query)
        print(empty.rowCount())
        empty.close()
        final failed = db.begin()
        failed.execute(sql`INSERT INTO items VALUES (${id})`)
        try { failed.execute(sql`INSERT INTO items VALUES (${id})`) }
        catch (SQLError error) { print('duplicate blocked') }
        try { failed.commit() } catch (SQLError error) { print('transaction invalidated') }
        failed.close()
        try { db.query(sql`SELECT 1; SELECT 2`) }
        catch (SQLError error) { print('script blocked') }
        db.close()
        try { db.query(query) } catch (SQLError error) { print('closed') }
    } }"#,
        "1\n42\n0\nduplicate blocked\ntransaction invalidated\nscript blocked\nclosed\n",
    );
}

#[test]
fn expressions_evaluate_once_in_order_and_temporary_statements_are_collected() {
    run(
        r#"class App {
        int value(int n) { print(n) return n }
        int fail() { throw 'failed' }
        failureOnly() {
            try { this.fail() } catch (GenericError error) { }
        }
        build() {
            for (i = 1 to 400) { final temporary = sql`SELECT ${i}` }
            try { final partial = sql`SELECT ${1}, ${this.fail()}` }
            catch (GenericError error) { }
        }
        start() {
            final db = SQL.open('sqlite::memory:')
            final statement = sql`SELECT ${this.value(1)}, ${this.value(2)}`
            final execute = () -> db.query(statement)
            Gc.collect()
            final rows = execute()
            rows.next()
            print(rows.getInt(0) + rows.getInt(1))
            rows.close()
            Gc.collect()
            // Exception records retain their own object. Measure that baseline
            // separately so this check detects retained template nodes.
            final beforeFailure = Gc.objects()
            this.failureOnly()
            Gc.collect()
            final exceptionObjects = Gc.objects() - beforeFailure
            final before = Gc.objects()
            this.build()
            Gc.collect()
            print(Gc.objects() - before == exceptionObjects)
            final again = db.query(statement)
            print(again.rowCount())
            again.close()
            db.close()
        }
    }"#,
        "1\n2\n3\ntrue\n1\n",
    );
}

#[test]
fn unsafe_contexts_and_unsupported_types_fail_check() {
    for (body, diagnostic) in [
        ("final q = sql`SELECT '${1}'`", "SQL interpolation inside"),
        ("final q = sql`SELECT \"${1}\"`", "SQL interpolation inside"),
        ("final q = sql`SELECT [${1}]`", "SQL interpolation inside"),
        (
            "final q = sql`SELECT \\`${1}\\``",
            "SQL interpolation inside",
        ),
        ("final q = sql`SELECT $$${1}$$`", "SQL interpolation inside"),
        (
            "final q = sql`SELECT $tag$${1}$tag$`",
            "SQL interpolation inside",
        ),
        (
            "final q = sql`SELECT 1 -- ${1}`",
            "SQL interpolation inside",
        ),
        ("final q = sql`SELECT 1 # ${1}`", "SQL interpolation inside"),
        (
            "final q = sql`SELECT /* ${1} */ 1`",
            "SQL interpolation inside",
        ),
        (
            "final q = sql`SELECT /*! executable */ 1`",
            "Executable SQL comments",
        ),
        ("final q = sql`SELECT :name`", "Native SQL placeholders"),
        ("final q = sql`SELECT $1`", "Native SQL placeholders"),
        ("final q = sql`SELECT ?`", "Native SQL placeholders"),
        ("final q = sql`SELECT 1${2}`", "complete value position"),
        (
            "final q = sql`SELECT * FROM ${'users'}`",
            "identifiers must be static",
        ),
        ("final q = sql``", "must not be empty"),
        (
            "final q = sql`SELECT ${[1, 2]}`",
            "SQL interpolation requires",
        ),
        (
            "final q = sql`SELECT ${null}`",
            "SQL interpolation requires",
        ),
        (
            "final q = sql`SELECT ${sql`SELECT 1`}`",
            "SQL interpolation requires",
        ),
        (
            "final q = sql`SELECT ${print('x')}`",
            "SQL interpolation requires",
        ),
        (
            "int? value = null final q = sql`SELECT ${value}`",
            "SQL interpolation requires",
        ),
        (
            "final q = sql`SELECT ${() -> 1}`",
            "SQL interpolation requires",
        ),
        ("final q = SQLStatement()", "cannot be constructed"),
    ] {
        check_rejected(&format!("class App {{ start() {{ {body} }} }}"), diagnostic);
    }
}

fn check_rejected(source: &str, diagnostic: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("check")
        .arg(entry)
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success() && message.contains(diagnostic),
        "{source}: {message}"
    );
}

#[test]
fn statement_types_cannot_be_forged() {
    for source in [
        "class App { SQLStatement q() { return 'SELECT 1' } start() {} }",
        "class App { SQLStatement q() { return print('x') } start() {} }",
        "class App { SQLStatement q() { yield 'SELECT 1' } start() {} }",
        "class App { take(SQLStatement q) {} start() { this.take('SELECT 1') } }",
        "class App { start() { final q = true then sql`SELECT 1` else 'SELECT 1' } }",
        "class App { start() { final q = if (true) { yield sql`SELECT 1` } else { yield 'SELECT 2' } } }",
        "class App { start() { final q = if (true) { yield sql`SELECT 1` } } }",
    ] { check_rejected(source, ""); }
}

#[test]
fn documented_example_executes() {
    run(include_str!("../../examples/usql/App.ube"), "1: O'Brien\n");
}

#[test]
fn byte_bindings_and_static_sql_quotes_preserve_their_meaning() {
    run(
        r#"class App {
        start() {
            final db = SQL.open('sqlite::memory:')
            byte value
            final rows = db.query(sql`SELECT ${value}, 'it''s literal', '$1', '?' /* static */`)
            rows.next()
            print(rows.getInt(0))
            print(rows.getString(1))
            print(rows.getString(2))
            print(rows.getString(3))
            rows.close()
            db.close()
        }
    }"#,
        "0\nit's literal\n$1\n?\n",
    );
}
