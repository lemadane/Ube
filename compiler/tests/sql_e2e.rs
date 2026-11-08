//! Compile through the real CLI and execute separate processes against an actual SQLite file.
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ube-sql-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated SQLite E2E directory");
        fs::write(path.join(".gitignore"), ".build/\n").expect("configure isolated build directory");
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compile(directory: &Path, name: &str, source: &str) -> PathBuf {
    let source_path = directory.join(format!("{name}.ube"));
    let binary = directory.join(name);
    let source_path = support::write_source(&source_path, source);
    // Cargo places the runtime static library beside integration test executables.
    let test = std::env::current_exe().expect("locate test executable");
    let runtime = test.parent().unwrap();
    assert!(
        runtime.join("libube_runtime.a").is_file(),
        "missing runtime archive"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("build")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary)
        .current_dir(runtime)
        .output()
        .expect("run Ube CLI");
    assert!(
        output.status.success(),
        "{name} compilation failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(binary.is_file(), "CLI did not produce {name} binary");
    binary
}

fn run(binary: &Path, directory: &Path, expected: &str) {
    let output = Command::new("timeout")
        .arg("20s")
        .arg(binary)
        .current_dir(directory)
        .output()
        .expect("run compiled Ube program");
    assert!(
        output.status.success(),
        "{} failed with {:?}:\n{}",
        binary.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    assert!(
        output.stderr.is_empty(),
        "unexpected runtime diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sqlite_cli_persistence_binding_rollback_and_legacy_interoperability() {
    let directory = Directory::new();
    let writer = compile(
        &directory.0,
        "writer",
        r#"
class App {
    start() {
        final db = SQL.open('sqlite://data.db?mode=rwc')
        final params = SQLParams.create()
        try {
            db.execute('CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT)', params)
            final tx = db.begin()
            try {
                params.addInt(1)
                params.addString(`Robert'); DROP TABLE users; --`)
                params.addNull('string')
                tx.execute('INSERT INTO users VALUES (?, ?, ?)', params)
                params.clear()
                tx.savepoint('nested')
                tx.execute(`INSERT INTO users VALUES (99, 'discarded savepoint row', NULL)`, params)
                tx.rollbackTo('nested')
                tx.release('nested')
                tx.commit()
            } finally { tx.close() }

            final committed = db.begin()
            try {
                params.addInt(2)
                params.addString('Ube 🍠')
                params.addNull('string')
                committed.execute('INSERT INTO users VALUES (?, ?, ?)', params)
                committed.commit()
            } finally { committed.close() }
            params.clear()

            final failed = db.begin()
            try {
                failed.execute(`INSERT INTO users VALUES (3, 'must roll back', NULL)`, params)
                failed.execute(`INSERT INTO users VALUES (1, 'duplicate key', NULL)`, params)
            } catch (SQLError err) {
                print('constraint rolled back')
            } finally { failed.close() }

            // Closing the parent connection also rolls back an unfinished transaction.
            final unfinished = db.begin()
            unfinished.execute(`INSERT INTO users VALUES (4, 'unfinished', NULL)`, params)
            db.close()
            unfinished.close()
            print('created')
        } finally {
            params.close()
            db.close()
        }
    }
}
"#,
    );
    let reader = compile(
        &directory.0,
        "reader",
        r#"
class App {
    start() {
        final db = SQL.open('sqlite://data.db?mode=ro')
        final params = SQLParams.create()
        try {
            final rows = db.query('SELECT id, name, note FROM users ORDER BY id', params)
            try {
                print(rows.rowCount())
                for (rows.next()) {
                    print(rows.getInt(rows.columnIndex('id')))
                    print(rows.getString(rows.columnIndex('name')))
                    print(rows.isNull(rows.columnIndex('note')))
                }
            } finally { rows.close() }
            try { db.execute(`INSERT INTO users VALUES (5, 'read-only write', NULL)`, params) }
            catch (SQLError err) { print('read-only enforced') }
        } finally { params.close() db.close() }

        // The existing SQLite API reads exactly the same on-disk database.
        final legacy = SQLite.openReadOnly('data.db')
        try {
            final rows = legacy.query('SELECT COUNT(*) FROM users')
            try {
                rows.next()
                print(rows.getLong(0))
            } finally { rows.close() }
        } finally { legacy.close() }
    }
}
"#,
    );

    assert!(!directory.0.join("data.db").exists());
    run(&writer, &directory.0, "constraint rolled back\ncreated\n");
    let data = fs::read(directory.0.join("data.db")).expect("writer persisted the database");
    assert!(
        data.starts_with(b"SQLite format 3\0"),
        "not an actual SQLite database"
    );

    let expected =
        "2\n1\nRobert'); DROP TABLE users; --\ntrue\n2\nUbe 🍠\ntrue\nread-only enforced\n2\n";
    run(&reader, &directory.0, expected);
    // A second fresh process must observe the same committed data after a rejected write.
    run(&reader, &directory.0, expected);
}

#[test]
fn orm_cli_class_mapping_queries_transactions_and_persistence() {
    let directory = Directory::new();
    let writer = compile(
        &directory.0,
        "orm-writer",
        r#"
class User(long id, var string name, var bool active, int? age, long? serial, decimal? score, bool? verified, string? email) {}
class App {
    long countUsers(OrmModel<User> users) {
        final query = users.query()
        try { return query.count() } finally { query.close() }
    }
    start() {
        final db = SQL.open('sqlite://orm.db?mode=rwc')
        final users = Orm.model(db, User, 'users')
        try {
            users.migrate()
            users.migrate()
            users.create(User(1L, `Robert'); DROP TABLE users; --`, true, null, null, null, null, null))
            users.create(User(2L, 'Ube 🍠', false, 42, 5000000000L, 1.5, true, 'ube@example.test'))
            final user = users.get(1L)
            print(user.name())
            print(user.age() ?? -1)
            print(user.verified() ?? false)
            user.name('Ada')
            user.active(false)
            print(users.save(user))
            print(users.find(99L) == null)
            final found = users.find(1L) ?? User(0L, 'missing', false, null, null, null, null, null)
            print(found.name())
            try { users.get(99L) } catch (SQLError err) { print('missing caught') }

            final selected = users.query().where('age', '>=', 40).whereNotNull('email').orderBy('id', 'desc').limit(1)
            try {
                final rows = selected.all()
                try {
                    for (rows.next()) {
                        final item = rows.get()
                        print(item.name())
                        print(item.age() ?? 0)
                        print(item.serial() ?? 0L)
                        print(item.score() ?? 0.0)
                        print(item.verified() ?? false)
                        print(item.email() ?? 'missing')
                    }
                } finally { rows.close() }
            } finally { selected.close() }

            final tx = db.begin()
            final transactional = users.using(tx)
            try {
                transactional.create(User(3L, 'rolled back', true, null, null, null, null, null))
                final params = SQLParams.create()
                try { tx.execute(`UPDATE users SET name = 'raw SQL in same transaction' WHERE id = 3`, params) }
                finally { params.close() }
                print(transactional.get(3L).name())
                tx.rollback()
            } finally { transactional.close() tx.close() }
            print(users.find(3L) == null)

            final committed = db.begin()
            final committedUsers = users.using(committed)
            try {
                committedUsers.create(User(4L, 'committed', true, null, null, null, null, null))
                committed.commit()
                try { committedUsers.get(4L) } catch (SQLError err) { print('finished transaction rejected') }
            } finally { committedUsers.close() committed.close() }
            print(users.get(4L).name())
            print(users.delete(4L))
            print(users.save(User(99L, 'not inserted', true, null, null, null, null, null)))
            print(this.countUsers(users))

            final other = SQL.open('sqlite::memory:')
            final otherTx = other.begin()
            try { users.using(otherTx) }
            catch (SQLError err) { print('wrong connection rejected') }
            finally { otherTx.close() other.close() }
        } finally { users.close() db.close() }
    }
}
"#,
    );
    let reader = compile(
        &directory.0,
        "orm-reader",
        r#"
class User(long id, string name, bool active, int? age, long? serial, decimal? score, bool? verified, string? email) {}
class App {
    start() {
        final db = SQL.open('sqlite://orm.db?mode=ro')
        final users = Orm.model(db, User, 'users')
        try {
            final query = users.query().orderBy('id', 'asc')
            try {
                print(query.count())
                final rows = query.all()
                try {
                    for (rows.next()) {
                        final user = rows.get()
                        print(user.id())
                        print(user.name())
                        print(user.active())
                    }
                } finally { rows.close() }
                query.limit(1).offset(1)
                final first = query.first() ?? User(0L, 'missing', false, null, null, null, null, null)
                print(first.name())
                query.limit(0)
                print(query.first() == null)
            } finally { query.close() }
        } finally { users.close() db.close() }
    }
}
"#,
    );
    run(&writer, &directory.0, "Robert'); DROP TABLE users; --\n-1\nfalse\n1\ntrue\nAda\nmissing caught\nUbe 🍠\n42\n5000000000\n1.5\ntrue\nube@example.test\nraw SQL in same transaction\ntrue\nfinished transaction rejected\ncommitted\n1\n0\n2\nwrong connection rejected\n");
    run(
        &reader,
        &directory.0,
        "2\n1\nAda\nfalse\n2\nUbe 🍠\nfalse\nUbe 🍠\ntrue\n",
    );
}

#[test]
fn orm_cli_inner_left_and_self_joins_keep_typed_results_and_transactions() {
    let directory = Directory::new();
    let executable = compile(
        &directory.0,
        "joins",
        r#"
class User(long id, string name, long? managerId) {}
class Order(long id, long userId, string status) {}
class Line(long id, long orderId) {}
class App {
    start() {
        final db = SQL.open('sqlite::memory:')
        final users = Orm.model(db, User, 'users')
        final orders = Orm.model(db, Order, 'orders')
        final lines = Orm.model(db, Line, 'lines')
        try {
            users.migrate() orders.migrate() lines.migrate()
            users.create(User(1L,'Ada',null))
            users.create(User(2L,'Grace',1L))
            users.create(User(3L,'Lin',1L))
            orders.create(Order(10L,1L,'paid'))
            orders.create(Order(11L,1L,'pending'))
            orders.create(Order(12L,2L,'paid'))
            lines.create(Line(100L,10L))
            final base = users.query()
            final paid = base.join(orders,'o','id','userId').where('o.status','=','paid').orderBy('id','asc')
            try {
                print(base.count())
                print(paid.count())
                final rows = paid.all()
                try { for (rows.next()) { print(rows.get().name()) } } finally { rows.close() }
                final lineQuery = paid.join(lines,'line','o.id','orderId')
                try {
                    print(lineQuery.count())
                    final first = lineQuery.first() ?? User(0L,'missing',null)
                    print(first.name())
                } finally { lineQuery.close() }
                paid.offset(1).limit(1)
                final first = paid.first() ?? User(0L,'missing',null)
                print(first.name())
            } finally { paid.close() }
            final unmatched = base.leftJoin(orders,'o','id','userId').whereNull('o.id')
            try {
                final row = unmatched.first() ?? User(0L,'missing',null)
                print(row.name())
            } finally { unmatched.close() }
            final managers = base.join(users,'manager','managerId','id').where('manager.name','=','Ada')
            try { print(managers.count()) } finally { managers.close() }
            base.close()

            final tx = db.begin()
            final txUsers = users.using(tx)
            final txOrders = orders.using(tx)
            final txBase = txUsers.query()
            try {
                txOrders.create(Order(13L,3L,'paid'))
                final joined = txBase.join(txOrders,'o','id','userId')
                try { print(joined.count()) } finally { joined.close() }
                try { txBase.join(orders,'other','id','userId') }
                catch (SQLError err) { print('transaction mismatch rejected') }
                tx.rollback()
            } finally { txBase.close() txUsers.close() txOrders.close() tx.close() }
            final after = users.query()
            final joined = after.join(orders,'o','id','userId')
            try { print(joined.count()) } finally { joined.close() after.close() }
        } finally { lines.close() orders.close() users.close() db.close() }
    }
}

"#,
    );
    run(
        &executable,
        &directory.0,
        "3\n2\nAda\nGrace\n1\nAda\nGrace\nLin\n2\n4\ntransaction mismatch rejected\n3\n",
    );
}

#[test]
fn model_free_queries_project_bind_clone_and_share_transactions() {
    let directory = Directory::new();
    let executable = compile(&directory.0, "builder", r#"
class App {
    SQLQuery activeUsers(SQL db) { return db.table('users').where('active', '=', true) }
    start() {
        final db = SQL.open('sqlite::memory:')
        final params = SQLParams.create()
        db.execute('CREATE TABLE users(id INTEGER, name TEXT, active INTEGER, score REAL, note TEXT)', params)
        db.execute(`INSERT INTO users VALUES (1, 'Ada', 1, 1.5, NULL), (2, 'Lin', 1, 2.5, 'note'), (3, 'Grace', 0, 3.5, NULL)`, params)
        final base = this.activeUsers(db)
        final query = base.clone().select('users.name').select('score').orderBy('id', 'desc').limit(1).offset(1)
        try {
            print(base.count())
            print(query.toSql())
            final rows = query.all()
            try {
                print(rows.columnCount())
                for (rows.next()) { print(rows.getString(0)) print(rows.getDecimal(1)) }
            } finally { rows.close() }
            print(query.count())
            final first = query.first()
            try { print(first.rowCount()) first.next() print(first.getString(0)) } finally { first.close() }
            query.limit(0)
            final empty = query.first()
            try { print(empty.rowCount()) } finally { empty.close() }
        } finally { query.close() base.close() }

        final numeric = db.table('users').where('id', '>=', 1).where('id', '<', 5000000000L).where('score', '>=', 1.5).whereNotNull('note')
        try { print(numeric.count()) } finally { numeric.close() }
        final nulls = db.table('users').whereNull('note')
        try { print(nulls.count()) } finally { nulls.close() }
        final injection = db.table('users').where('name', '=', `Ada' OR 1=1 --`)
        try { print(injection.count()) } finally { injection.close() }

        final invalid = db.table('users')
        try {
            try { invalid.where('id', '= ? OR 1=1 --', 1) } catch (SQLError e) { print('operator rejected') }
            try { invalid.orderBy('id', 'asc; DROP TABLE users') } catch (SQLError e) { print('direction rejected') }
            try { invalid.limit(-1) } catch (SQLError e) { print('limit rejected') }
            try { invalid.offset(-1) } catch (SQLError e) { print('offset rejected') }
            try { invalid.select('users..name') } catch (SQLError e) { print('identifier rejected') }
            print(invalid.count())
        } finally { invalid.close() }
        try { invalid.all() } catch (SQLError e) { print('closed query rejected') }

        final paged = db.table('users').select('id').orderBy('id', 'asc').offset(2)
        try {
            final rows = paged.all()
            try { rows.next() print(rows.getInt(0)) } finally { rows.close() }
        } finally { paged.close() }

        final tx = db.begin()
        final transactional = tx.table('users')
        try {
            tx.execute(`INSERT INTO users VALUES (4, 'Uncommitted', 1, 4.5, NULL)`, params)
            print(transactional.count())
            tx.rollback()
            try { transactional.count() } catch (SQLError e) { print('finished transaction rejected') }
        } finally { transactional.close() tx.close() }
        final finalQuery = db.table('users')
        final detached = finalQuery.all()
        try {
            print(finalQuery.count())
            db.close()
            print(detached.rowCount())
            try { finalQuery.all() } catch (SQLError e) { print('closed connection rejected') }
        } finally { detached.close() finalQuery.close() params.close() db.close() }
    }
}
"#);
    run(&executable, &directory.0, "2\nSELECT \"users\".\"name\", \"score\" FROM \"users\" WHERE \"active\" = ? ORDER BY \"id\" DESC LIMIT 1 OFFSET 1\n2\nAda\n1.5\n2\n1\nAda\n0\n1\n2\n0\noperator rejected\ndirection rejected\nlimit rejected\noffset rejected\nidentifier rejected\n3\nclosed query rejected\n3\n4\nfinished transaction rejected\n3\n3\nclosed connection rejected\n");
}

#[test]
fn query_builder_signatures_reject_invalid_calls() {
    use ube_compiler::{Lexer, Parser, TypeChecker};
    for statement in [
        "final query = SQLQuery()",
        "final query = SQL.table('users')",
        "final query = db.table(1)",
        "final query = db.table('users').select(['id'])",
        "final query = db.table('users').where('id', '=', null)",
        "final query = db.table('users').where('id', '=', SQLParams.create())",
        "final query = db.table('users').where('id', 1, 2)",
        "final query = db.table('users').where('id', '=')",
        "final query = db.table('users').limit(1.5)",
    ] {
        let source = format!("class App {{ start() {{ final db = SQL.open('sqlite::memory:') {statement} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{statement}: {:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        assert!(!checker.check_program(&program), "invalid call accepted: {statement}");
    }
}
