//! Compile through the real CLI and execute separate processes against an actual SQLite file.
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
    fs::write(&source_path, source).expect("write Ube source");
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
class Application {
    main() {
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
class Application {
    main() {
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
    let writer = compile(&directory.0, "orm-writer", r#"
class User(long id, var string name, var bool active, int? age, long? serial, decimal? score, bool? verified, string? email) {}
class Application {
    long countUsers(OrmModel<User> users) {
        final query = users.query()
        try { return query.count() } finally { query.close() }
    }
    main() {
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
"#);
    let reader = compile(&directory.0, "orm-reader", r#"
class User(long id, string name, bool active, int? age, long? serial, decimal? score, bool? verified, string? email) {}
class Application {
    main() {
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
"#);
    run(&writer, &directory.0, "Robert'); DROP TABLE users; --\n-1\nfalse\n1\ntrue\nAda\nmissing caught\nUbe 🍠\n42\n5000000000\n1.5\ntrue\nube@example.test\nraw SQL in same transaction\ntrue\nfinished transaction rejected\ncommitted\n1\n0\n2\nwrong connection rejected\n");
    run(&reader, &directory.0, "2\n1\nAda\nfalse\n2\nUbe 🍠\nfalse\nUbe 🍠\ntrue\n");
}
