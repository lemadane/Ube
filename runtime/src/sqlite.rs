//! SQLite handles are opaque IDs; stale handles never dereference freed memory.
//! Statements retain SQL and bindings and use SQLite's per-connection statement cache.
use rusqlite::{params_from_iter, types::Value, Connection, OpenFlags, Statement};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
    time::Duration,
};

type Handle = *mut c_void;
type DbResult<T> = Result<T, String>;
#[derive(Default)]
struct State {
    next_id: usize,
    connections: HashMap<usize, Connection>,
    statements: HashMap<usize, Prepared>,
    results: HashMap<usize, ResultSet>,
}
struct Prepared {
    connection: usize,
    sql: String,
    bindings: Vec<Value>,
    names: Vec<Option<String>>,
}
struct ResultSet {
    rows: Vec<Vec<Value>>,
    columns: Vec<String>,
    position: usize,
}
impl State {
    fn result(&mut self, result: ResultSet) -> Handle {
        self.next_id += 1;
        self.results.insert(self.next_id, result);
        self.next_id as Handle
    }
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
fn fail(error: impl std::fmt::Display) {
    crate::exceptions::set_pending(crate::exceptions::named(
        "SQLiteError",
        format!("SQLite: {error}"),
    ));
}
fn text(ptr: *const c_char) -> DbResult<String> {
    if ptr.is_null() {
        return Err("null string argument".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|e| e.to_string())
}
fn cstring(text: &str) -> DbResult<*mut c_char> {
    CString::new(text)
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}
fn run<T: Default>(f: impl FnOnce(&mut State) -> DbResult<T>) -> T {
    match f(&mut state().lock().unwrap()) {
        Ok(value) => value,
        Err(error) => {
            fail(error);
            T::default()
        }
    }
}
fn connection(s: &State, handle: Handle) -> DbResult<&Connection> {
    s.connections
        .get(&(handle as usize))
        .ok_or_else(|| "invalid or closed connection".into())
}
fn prepared(s: &State, handle: Handle) -> DbResult<&Prepared> {
    let p = s
        .statements
        .get(&(handle as usize))
        .ok_or("invalid or closed statement")?;
    connection(s, p.connection as Handle)?;
    Ok(p)
}
fn result(s: &State, handle: Handle) -> DbResult<&ResultSet> {
    s.results
        .get(&(handle as usize))
        .ok_or_else(|| "invalid or closed result".into())
}
fn index(index: i32, len: usize, base: usize) -> DbResult<usize> {
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(base))
        .filter(|&i| i < len)
        .ok_or_else(|| format!("index {index} out of range (indexes start at {base})"))
}

// Validate the whole SQL string, including its tail, without executing anything.
// SQLite itself parses strings/comments/triggers so semicolons in them are safe.
fn single_sql(db: &Connection, sql: &str) -> DbResult<()> {
    let sql = CString::new(sql).map_err(|e| e.to_string())?;
    unsafe {
        let mut cursor = sql.as_ptr();
        let mut count = 0;
        while *cursor != 0 {
            let mut stmt = std::ptr::null_mut();
            let mut tail = std::ptr::null();
            let code =
                rusqlite::ffi::sqlite3_prepare_v2(db.handle(), cursor, -1, &mut stmt, &mut tail);
            if !stmt.is_null() {
                rusqlite::ffi::sqlite3_finalize(stmt);
                count += 1;
            }
            if code != rusqlite::ffi::SQLITE_OK {
                return Err(CStr::from_ptr(rusqlite::ffi::sqlite3_errmsg(db.handle()))
                    .to_string_lossy()
                    .into_owned());
            }
            if count > 1 {
                return Err("expected one SQL statement; use executeBatch for scripts".into());
            }
            if tail.is_null() || tail == cursor {
                break;
            }
            cursor = tail;
        }
        if count != 1 {
            return Err("SQL must contain one statement".into());
        }
    }
    Ok(())
}
fn collect(stmt: &mut Statement<'_>, bindings: &[Value]) -> DbResult<ResultSet> {
    if stmt.column_count() == 0 {
        return Err("query requires result columns; use execute for this statement".into());
    }
    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let count = columns.len();
    let mut cursor = stmt
        .query(params_from_iter(bindings))
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
        rows.push(
            (0..count)
                .map(|i| row.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?,
        );
    }
    Ok(ResultSet {
        rows,
        columns,
        position: 0,
    })
}
fn execute(stmt: &mut Statement<'_>, bindings: &[Value]) -> DbResult<i64> {
    if stmt.column_count() != 0 {
        return Err("statement returns columns; use query (including for RETURNING)".into());
    }
    stmt.execute(params_from_iter(bindings))
        .map(|n| n as i64)
        .map_err(|e| e.to_string())
}
fn open(path: *const c_char, readonly: bool) -> Handle {
    run(|s| {
        let flags = if readonly {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        let db = Connection::open_with_flags(text(path)?, flags | OpenFlags::SQLITE_OPEN_URI)
            .map_err(|e| e.to_string())?;
        s.next_id += 1;
        s.connections.insert(s.next_id, db);
        Ok(s.next_id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_open(path: *const c_char) -> Handle {
    open(path, false)
}
#[no_mangle]
pub extern "C" fn ube_sqlite_open_read_only(path: *const c_char) -> Handle {
    open(path, true)
}
#[no_mangle]
pub extern "C" fn ube_sqlite_version() -> *mut c_char {
    run(|_| cstring(rusqlite::version()))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_execute(handle: Handle, sql: *const c_char) -> i64 {
    run(|s| {
        let db = connection(s, handle)?;
        let sql = text(sql)?;
        single_sql(db, &sql)?;
        execute(
            &mut *db.prepare_cached(&sql).map_err(|e| e.to_string())?,
            &[],
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_execute_batch(handle: Handle, sql: *const c_char) {
    run(|s| {
        connection(s, handle)?
            .execute_batch(&text(sql)?)
            .map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_query(handle: Handle, sql: *const c_char) -> Handle {
    run(|s| {
        let rows = {
            let db = connection(s, handle)?;
            let sql = text(sql)?;
            single_sql(db, &sql)?;
            collect(
                &mut *db.prepare_cached(&sql).map_err(|e| e.to_string())?,
                &[],
            )?
        };
        Ok(s.result(rows))
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_prepare(handle: Handle, sql: *const c_char) -> Handle {
    run(|s| {
        let sql = text(sql)?;
        let names = {
            let db = connection(s, handle)?;
            single_sql(db, &sql)?;
            let stmt = db.prepare_cached(&sql).map_err(|e| e.to_string())?;
            (1..=stmt.parameter_count())
                .map(|i| stmt.parameter_name(i).map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let bindings = vec![Value::Null; names.len()];
        s.next_id += 1;
        s.statements.insert(
            s.next_id,
            Prepared {
                connection: handle as usize,
                sql,
                bindings,
                names,
            },
        );
        Ok(s.next_id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_close(handle: Handle) {
    let mut s = state().lock().unwrap();
    s.statements.retain(|_, p| p.connection != handle as usize);
    s.connections.remove(&(handle as usize));
}
macro_rules! transaction {
    ($name:ident, $sql:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: Handle) {
            run(|s| {
                connection(s, handle)?
                    .execute_batch($sql)
                    .map_err(|e| e.to_string())
            })
        }
    };
}
transaction!(ube_sqlite_begin, "BEGIN");
transaction!(ube_sqlite_commit, "COMMIT");
transaction!(ube_sqlite_rollback, "ROLLBACK");
#[no_mangle]
pub extern "C" fn ube_sqlite_in_transaction(handle: Handle) -> bool {
    run(|s| Ok(!connection(s, handle)?.is_autocommit()))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_busy_timeout(handle: Handle, milliseconds: i32) {
    run(|s| {
        let milliseconds =
            u64::try_from(milliseconds).map_err(|_| "busy timeout must be nonnegative")?;
        connection(s, handle)?
            .busy_timeout(Duration::from_millis(milliseconds))
            .map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_last_insert_row_id(handle: Handle) -> i64 {
    run(|s| Ok(connection(s, handle)?.last_insert_rowid()))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_changes(handle: Handle) -> i64 {
    run(|s| Ok(connection(s, handle)?.changes() as i64))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_total_changes(handle: Handle) -> i64 {
    run(|s| Ok(connection(s, handle)?.total_changes() as i64))
}

#[no_mangle]
pub extern "C" fn ube_sqlite_read_only(handle: Handle) -> bool {
    run(|s| {
        connection(s, handle)?
            .is_readonly(rusqlite::DatabaseName::Main)
            .map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_backup(handle: Handle, path: *const c_char) {
    run(|s| {
        connection(s, handle)?
            .backup(rusqlite::DatabaseName::Main, text(path)?, None)
            .map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_restore(handle: Handle, path: *const c_char) {
    run(|s| {
        // A missing source must fail rather than create an empty source database.
        let source = Connection::open_with_flags(text(path)?, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        let db = s
            .connections
            .get_mut(&(handle as usize))
            .ok_or("invalid or closed connection")?;
        if !db.is_autocommit() {
            return Err("restore requires no active transaction".into());
        }
        db.flush_prepared_statement_cache();
        let backup = rusqlite::backup::Backup::new(&source, db).map_err(|e| e.to_string())?;
        match backup.step(-1).map_err(|e| e.to_string())? {
            rusqlite::backup::StepResult::Done => Ok(()),
            _ => Err("restore could not complete because the database is busy or locked".into()),
        }
    })
}

fn bind(handle: Handle, parameter: i32, value: DbResult<Value>) {
    run(|s| {
        let p = prepared(s, handle)?;
        let i = index(parameter, p.bindings.len(), 1)?;
        let value = value?;
        s.statements.get_mut(&(handle as usize)).unwrap().bindings[i] = value;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_null(h: Handle, i: i32) {
    bind(h, i, Ok(Value::Null));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_int(h: Handle, i: i32, v: i32) {
    bind(h, i, Ok(Value::Integer(v as i64)));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_long(h: Handle, i: i32, v: i64) {
    bind(h, i, Ok(Value::Integer(v)));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_decimal(h: Handle, i: i32, v: f64) {
    bind(
        h,
        i,
        if v.is_finite() {
            Ok(Value::Real(v))
        } else {
            Err("decimal binding must be finite".into())
        },
    );
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_bool(h: Handle, i: i32, v: bool) {
    bind(h, i, Ok(Value::Integer(i64::from(v))));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_string(h: Handle, i: i32, v: *const c_char) {
    bind(h, i, text(v).map(Value::Text));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_bind_blob_hex(h: Handle, i: i32, v: *const c_char) {
    bind(
        h,
        i,
        text(v).and_then(|s| {
            hex::decode(s)
                .map(Value::Blob)
                .map_err(|e| format!("invalid BLOB hex: {e}"))
        }),
    );
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_parameter_count(h: Handle) -> i32 {
    run(|s| Ok(prepared(s, h)?.bindings.len() as i32))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_parameter_name(h: Handle, i: i32) -> *mut c_char {
    run(|s| {
        let p = prepared(s, h)?;
        cstring(
            p.names[index(i, p.names.len(), 1)?]
                .as_deref()
                .unwrap_or(""),
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_parameter_index(h: Handle, name: *const c_char) -> i32 {
    run(|s| {
        let p = prepared(s, h)?;
        let name = text(name)?;
        Ok(p.names
            .iter()
            .position(|n| n.as_deref() == Some(name.as_str()))
            .map_or(0, |i| i as i32 + 1))
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_clear_bindings(h: Handle) {
    run(|s| {
        prepared(s, h)?;
        s.statements
            .get_mut(&(h as usize))
            .unwrap()
            .bindings
            .fill(Value::Null);
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_execute(h: Handle) -> i64 {
    run(|s| {
        let p = prepared(s, h)?;
        let db = connection(s, p.connection as Handle)?;
        execute(
            &mut *db.prepare_cached(&p.sql).map_err(|e| e.to_string())?,
            &p.bindings,
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_query(h: Handle) -> Handle {
    run(|s| {
        let rows = {
            let p = prepared(s, h)?;
            let db = connection(s, p.connection as Handle)?;
            collect(
                &mut *db.prepare_cached(&p.sql).map_err(|e| e.to_string())?,
                &p.bindings,
            )?
        };
        Ok(s.result(rows))
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_statement_close(h: Handle) {
    state().lock().unwrap().statements.remove(&(h as usize));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_close(h: Handle) {
    state().lock().unwrap().results.remove(&(h as usize));
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_next(h: Handle) -> bool {
    run(|s| {
        let result = s
            .results
            .get_mut(&(h as usize))
            .ok_or("invalid or closed result")?;
        if result.position <= result.rows.len() {
            result.position += 1;
        }
        Ok(result.position <= result.rows.len())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_reset(h: Handle) {
    run(|s| {
        let result = s
            .results
            .get_mut(&(h as usize))
            .ok_or("invalid or closed result")?;
        result.position = 0;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_row_count(h: Handle) -> i64 {
    run(|s| Ok(result(s, h)?.rows.len() as i64))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_column_count(h: Handle) -> i32 {
    run(|s| Ok(result(s, h)?.columns.len() as i32))
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_column_name(h: Handle, i: i32) -> *mut c_char {
    run(|s| {
        let r = result(s, h)?;
        cstring(&r.columns[index(i, r.columns.len(), 0)?])
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_column_index(h: Handle, name: *const c_char) -> i32 {
    run(|s| {
        let name = text(name)?;
        Ok(result(s, h)?
            .columns
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&name))
            .map_or(-1, |i| i as i32))
    })
}
fn value(s: &State, h: Handle, column: i32) -> DbResult<&Value> {
    let result = result(s, h)?;
    let row = result
        .position
        .checked_sub(1)
        .and_then(|i| result.rows.get(i))
        .ok_or("result is not positioned on a row; call next first")?;
    Ok(&row[index(column, row.len(), 0)?])
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_column_type(h: Handle, i: i32) -> *mut c_char {
    run(|s| {
        cstring(match value(s, h, i)? {
            Value::Null => "null",
            Value::Integer(_) => "integer",
            Value::Real(_) => "real",
            Value::Text(_) => "text",
            Value::Blob(_) => "blob",
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_string(h: Handle, i: i32) -> *mut c_char {
    run(|s| match value(s, h, i)? {
        Value::Text(text) => cstring(text),
        _ => Err("column is not text".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_blob_hex(h: Handle, i: i32) -> *mut c_char {
    run(|s| match value(s, h, i)? {
        Value::Blob(bytes) => cstring(&hex::encode(bytes)),
        _ => Err("column is not a BLOB".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_int(h: Handle, i: i32) -> i32 {
    run(|s| match value(s, h, i)? {
        Value::Integer(n) => {
            i32::try_from(*n).map_err(|_| "integer does not fit in int; use getLong".into())
        }
        _ => Err("column is not an integer".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_long(h: Handle, i: i32) -> i64 {
    run(|s| match value(s, h, i)? {
        Value::Integer(n) => Ok(*n),
        _ => Err("column is not an integer".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_bool(h: Handle, i: i32) -> bool {
    run(|s| match value(s, h, i)? {
        Value::Integer(0) => Ok(false),
        Value::Integer(1) => Ok(true),
        _ => Err("column is not a boolean integer (0 or 1)".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_get_decimal(h: Handle, i: i32) -> f64 {
    run(|s| match value(s, h, i)? {
        Value::Real(n) => Ok(*n),
        Value::Integer(n) => Ok(*n as f64),
        _ => Err("column is not numeric".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_sqlite_result_is_null(h: Handle, i: i32) -> bool {
    run(|s| Ok(matches!(value(s, h, i)?, Value::Null)))
}
pub fn cleanup() {
    let mut s = state().lock().unwrap();
    s.results.clear();
    s.statements.clear();
    s.connections.clear();
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sql(text: &str) -> CString {
        CString::new(text).unwrap()
    }
    #[test]
    fn file_persistence_and_result_validation() {
        let path = std::env::temp_dir().join(format!("ube-sqlite-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_arg = sql(path.to_str().unwrap());
        let db = ube_sqlite_open(path_arg.as_ptr());
        assert!(!db.is_null());
        ube_sqlite_execute(db, sql("CREATE TABLE items (id INTEGER)").as_ptr());
        assert_eq!(
            ube_sqlite_execute(db, sql("INSERT INTO items VALUES (42)").as_ptr()),
            1
        );
        ube_sqlite_close(db);
        let db = ube_sqlite_open(path_arg.as_ptr());
        let rows = ube_sqlite_query(db, sql("SELECT id FROM items").as_ptr());
        assert!(!rows.is_null());
        ube_sqlite_close(db);
        assert_eq!(ube_sqlite_result_get_long(rows, 0), 0);
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert!(ube_sqlite_result_next(rows));
        assert_eq!(ube_sqlite_result_get_long(rows, 0), 42);
        assert!(ube_sqlite_result_get_string(rows, 0).is_null());
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert_eq!(ube_sqlite_result_get_long(rows, -1), 0);
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert!(!ube_sqlite_result_next(rows));
        assert!(!ube_sqlite_result_next(rows));
        ube_sqlite_result_close(rows);
        assert!(!ube_sqlite_result_next(rows));
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert_eq!(crate::exceptions::take_pending(), 0);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod full_tests {
    use super::*;
    struct Files(Vec<std::path::PathBuf>);
    impl Files {
        fn new() -> Self {
            Self(Vec::new())
        }
        fn path(&mut self, suffix: &str) -> CString {
            let path = std::env::temp_dir().join(format!(
                "ube-sqlite-full-{}-{suffix}.db",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            self.0.push(path.clone());
            CString::new(path.to_str().unwrap()).unwrap()
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    fn sql(s: &str) -> CString {
        CString::new(s).unwrap()
    }
    fn success() {
        assert_eq!(crate::exceptions::take_pending(), 0);
    }
    fn error() {
        assert_ne!(crate::exceptions::take_pending(), 0);
    }
    fn string(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null());
        unsafe { CString::from_raw(ptr) }.into_string().unwrap()
    }
    fn scalar(db: Handle, query: &str) -> i64 {
        let rows = ube_sqlite_query(db, sql(query).as_ptr());
        assert!(!rows.is_null());
        assert!(ube_sqlite_result_next(rows));
        let value = ube_sqlite_result_get_long(rows, 0);
        ube_sqlite_result_close(rows);
        success();
        value
    }
    #[test]
    fn backup_restore_readonly_and_rollback_on_close() {
        let mut files = Files::new();
        let path = files.path("source");
        let copy = files.path("backup");
        let missing = files.path("missing");
        let db = ube_sqlite_open(path.as_ptr());
        assert!(!ube_sqlite_read_only(db));
        ube_sqlite_execute_batch(
            db,
            sql("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (10);").as_ptr(),
        );
        ube_sqlite_backup(db, copy.as_ptr());
        success();
        ube_sqlite_execute(db, sql("UPDATE t SET n=20").as_ptr());
        ube_sqlite_restore(db, missing.as_ptr());
        error();
        assert_eq!(scalar(db, "SELECT n FROM t"), 20);
        ube_sqlite_restore(db, copy.as_ptr());
        success();
        assert_eq!(scalar(db, "SELECT n FROM t"), 10);
        ube_sqlite_begin(db);
        ube_sqlite_execute(db, sql("DELETE FROM t").as_ptr());
        ube_sqlite_restore(db, copy.as_ptr());
        error();
        ube_sqlite_close(db); // An uncommitted transaction is rolled back.
        let readonly = ube_sqlite_open_read_only(path.as_ptr());
        assert!(ube_sqlite_read_only(readonly));
        assert_eq!(scalar(readonly, "SELECT n FROM t"), 10);
        ube_sqlite_execute(readonly, sql("DELETE FROM t").as_ptr());
        error();
        ube_sqlite_close(readonly);
        assert!(ube_sqlite_open_read_only(missing.as_ptr()).is_null());
        error();
        assert!(!std::path::Path::new(missing.to_str().unwrap()).exists());
    }
    #[test]
    fn busy_timeout_foreign_keys_savepoints_and_parameter_reuse() {
        let mut files = Files::new();
        let path = files.path("locking");
        let db = ube_sqlite_open(path.as_ptr());
        let other = ube_sqlite_open(path.as_ptr());
        ube_sqlite_busy_timeout(other, 0);
        ube_sqlite_execute_batch(db, sql("PRAGMA foreign_keys=ON; CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id));").as_ptr());
        ube_sqlite_execute(db, sql("INSERT INTO child VALUES (99)").as_ptr());
        error();
        ube_sqlite_execute(db, sql("BEGIN IMMEDIATE").as_ptr());
        ube_sqlite_execute(other, sql("INSERT INTO parent VALUES(1)").as_ptr());
        error();
        ube_sqlite_rollback(db);
        let insert = ube_sqlite_prepare(other, sql("INSERT INTO parent VALUES(:id)").as_ptr());
        assert_eq!(ube_sqlite_statement_parameter_count(insert), 1);
        assert_eq!(
            ube_sqlite_statement_parameter_index(insert, sql(":id").as_ptr()),
            1
        );
        assert_eq!(
            ube_sqlite_statement_parameter_index(insert, sql(":missing").as_ptr()),
            0
        );
        assert_eq!(
            string(ube_sqlite_statement_parameter_name(insert, 1)),
            ":id"
        );
        ube_sqlite_statement_bind_int(insert, 1, 1);
        assert_eq!(ube_sqlite_statement_execute(insert), 1);
        ube_sqlite_statement_bind_long(insert, 1, 2);
        assert_eq!(ube_sqlite_statement_execute(insert), 1);
        assert_eq!(ube_sqlite_total_changes(other), 2);
        ube_sqlite_execute_batch(
            other,
            sql("SAVEPOINT point; INSERT INTO parent VALUES(3); ROLLBACK TO point; RELEASE point;")
                .as_ptr(),
        );
        assert_eq!(scalar(other, "SELECT count(*) FROM parent"), 2);
        ube_sqlite_statement_close(insert);
        ube_sqlite_statement_close(insert);
        assert_eq!(ube_sqlite_statement_execute(insert), 0);
        error();
        ube_sqlite_close(db);
        ube_sqlite_close(other);
        success();
    }
    #[test]
    fn bindings_types_empty_blobs_and_closed_handles() {
        let db = ube_sqlite_open(sql(":memory:").as_ptr());
        let p = ube_sqlite_prepare(
            db,
            sql("SELECT ?1 AS data, ?2 AS flag, ?3 AS number, ?4 AS text, ?5 AS nullable").as_ptr(),
        );
        ube_sqlite_statement_bind_blob_hex(p, 1, sql("").as_ptr());
        ube_sqlite_statement_bind_bool(p, 2, false);
        ube_sqlite_statement_bind_decimal(p, 3, -0.25);
        ube_sqlite_statement_bind_string(p, 4, sql("Unicode: 🍠").as_ptr());
        let rows = ube_sqlite_statement_query(p);
        assert!(ube_sqlite_result_next(rows));
        assert_eq!(string(ube_sqlite_result_get_blob_hex(rows, 0)), "");
        assert_eq!(string(ube_sqlite_result_column_type(rows, 0)), "blob");
        assert!(!ube_sqlite_result_get_bool(rows, 1));
        assert_eq!(ube_sqlite_result_get_decimal(rows, 2), -0.25);
        assert_eq!(string(ube_sqlite_result_get_string(rows, 3)), "Unicode: 🍠");
        assert!(ube_sqlite_result_is_null(rows, 4));
        success();
        assert_eq!(
            ube_sqlite_result_column_index(rows, sql("missing").as_ptr()),
            -1
        );
        assert!(ube_sqlite_result_get_string(rows, 4).is_null());
        error();
        assert!(ube_sqlite_result_column_name(rows, 5).is_null());
        error();
        ube_sqlite_statement_bind_decimal(p, 3, f64::NAN);
        error();
        ube_sqlite_statement_bind_blob_hex(p, 1, sql("bad").as_ptr());
        error();
        ube_sqlite_statement_bind_string(p, 4, std::ptr::null());
        error();
        ube_sqlite_statement_clear_bindings(p);
        let cleared = ube_sqlite_statement_query(p);
        assert!(ube_sqlite_result_next(cleared));
        assert!(ube_sqlite_result_is_null(cleared, 0));
        ube_sqlite_result_close(cleared);
        ube_sqlite_close(db);
        assert!(ube_sqlite_statement_query(p).is_null());
        error();
        // Detached result snapshots survive statement and connection closure.
        assert_eq!(string(ube_sqlite_result_get_blob_hex(rows, 0)), "");
        ube_sqlite_result_close(rows);
        success();
    }
    #[test]
    fn single_statement_validation_parses_sql_instead_of_splitting_semicolons() {
        let db = ube_sqlite_open(sql(":memory:").as_ptr());
        ube_sqlite_execute(
            db,
            sql("CREATE TABLE t(n TEXT); -- tail comment\n").as_ptr(),
        );
        success();
        ube_sqlite_execute(db, sql("CREATE TRIGGER duplicate AFTER INSERT ON t WHEN NEW.n != 'duplicate' BEGIN INSERT INTO t VALUES('duplicate'); END;").as_ptr());
        success();
        assert_eq!(
            ube_sqlite_execute(db, sql("INSERT INTO t VALUES('semi;colon')").as_ptr()),
            1
        );
        assert_eq!(scalar(db, "SELECT count(*) FROM t; /* comment */"), 2);
        assert!(ube_sqlite_prepare(db, sql("SELECT 1; SELECT 2").as_ptr()).is_null());
        error();
        assert!(ube_sqlite_prepare(db, sql("-- empty").as_ptr()).is_null());
        error();
        assert!(ube_sqlite_query(db, sql("SELECT 1; invalid SQL").as_ptr()).is_null());
        error();
        ube_sqlite_close(db);
        success();
    }
}
