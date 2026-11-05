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
