//! Authoritative JSON key/value records. No cache, TTL, or eviction policy.
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
type Result<T> = std::result::Result<T, String>;
const APPLICATION_ID: i64 = 0x55424b56;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
struct Database {
    connection: Option<Connection>,
}
fn handles() -> &'static Mutex<HashMap<usize, Arc<Mutex<Database>>>> {
    static HANDLES: OnceLock<Mutex<HashMap<usize, Arc<Mutex<Database>>>>> = OnceLock::new();
    HANDLES.get_or_init(Default::default)
}
fn text<'a>(pointer: *const c_char) -> Result<&'a str> {
    if pointer.is_null() {
        return Err("KeyValueStore string arguments cannot be null".into());
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|_| "KeyValueStore strings must be UTF-8".into())
}
fn key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 4096 {
        Err("KeyValueStore keys require 1 to 4096 bytes".into())
    } else {
        Ok(())
    }
}
fn json(value: &str) -> Result<()> {
    if value.len() > MAX_JSON_BYTES {
        return Err("KeyValueStore JSON value exceeds 16 MiB".into());
    }
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|error| format!("Invalid KeyValueStore JSON: {error}"))
}
fn run<T: Default>(operation: impl FnOnce() -> Result<T>) -> T {
    match operation() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::generic(error));
            T::default()
        }
    }
}
fn open(path: &Path, durable: bool) -> Result<*mut c_void> {
    if path.as_os_str().is_empty() || path == Path::new(":memory:") {
        return Err("KeyValueStore requires a disk path".into());
    }
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    let id: i64 = db
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let tables: i64 = db
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if id != APPLICATION_ID && (id != 0 || tables != 0) {
        return Err("File is not a KeyValueStore database".into());
    }
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if id == APPLICATION_ID && version != 1 {
        return Err("Unsupported KeyValueStore database version".into());
    }
    db.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=256; PRAGMA cache_size=-2048;").map_err(|e|e.to_string())?;
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if mode != "wal" {
        return Err("KeyValueStore requires WAL support".into());
    }
    db.execute_batch(if durable {
        "PRAGMA synchronous=FULL;"
    } else {
        "PRAGMA synchronous=NORMAL;"
    })
    .map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS records (key TEXT PRIMARY KEY, json TEXT NOT NULL);",
        )
        .map_err(|e| e.to_string())?;
    transaction
        .pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(|e| e.to_string())?;
    transaction
        .pragma_update(None, "user_version", 1)
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    let id = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| "KeyValueStore handle limit reached")?;
    handles().lock().unwrap().insert(
        id,
        Arc::new(Mutex::new(Database {
            connection: Some(db),
        })),
    );
    Ok(id as *mut c_void)
}
fn with<T>(handle: *mut c_void, operation: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
    let database = handles()
        .lock()
        .unwrap()
        .get(&(handle as usize))
        .cloned()
        .ok_or("Invalid or closed KeyValueStore handle")?;
    let mut database = database.lock().unwrap();
    let connection = database
        .connection
        .as_mut()
        .ok_or("KeyValueStore is closed")?;
    operation(connection)
}
fn string(value: &str) -> Result<*mut c_char> {
    let pointer = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if pointer.is_null() {
        return Err("Cannot allocate KeyValueStore result".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), pointer.cast(), value.len());
        *pointer.add(value.len()) = 0;
    }
    Ok(pointer)
}
#[no_mangle]
pub extern "C" fn ube_kv_open(path: *const c_char) -> *mut c_void {
    run(|| open(Path::new(text(path)?), true))
}
#[no_mangle]
pub extern "C" fn ube_kv_open_buffered(path: *const c_char) -> *mut c_void {
    run(|| open(Path::new(text(path)?), false))
}
#[no_mangle]
pub extern "C" fn ube_kv_set(handle: *mut c_void, name: *const c_char, value: *const c_char) {
    run(|| {
        let name = text(name)?;
        let value = text(value)?;
        key(name)?;
        json(value)?;
        with(handle, |db| {
            db.execute("INSERT INTO records(key,json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json",params![name,value]).map(|_|()).map_err(|e|e.to_string())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_kv_add(
    handle: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> bool {
    run(|| {
        let name = text(name)?;
        let value = text(value)?;
        key(name)?;
        json(value)?;
        with(handle, |db| {
            db.execute(
                "INSERT INTO records(key,json) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
                params![name, value],
            )
            .map(|count| count == 1)
            .map_err(|e| e.to_string())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_kv_get(handle: *mut c_void, name: *const c_char) -> *mut c_char {
    run(|| {
        let name = text(name)?;
        key(name)?;
        with(handle, |db| {
            let value: Option<String> = db
                .query_row("SELECT json FROM records WHERE key=?1", [name], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?;
            match value {
                Some(value) => {
                    json(&value)?;
                    string(&value)
                }
                None => Ok(std::ptr::null_mut()),
            }
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_kv_has(handle: *mut c_void, name: *const c_char) -> bool {
    run(|| {
        let name = text(name)?;
        key(name)?;
        with(handle, |db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE key=?1)",
                [name],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_kv_delete(handle: *mut c_void, name: *const c_char) -> bool {
    run(|| {
        let name = text(name)?;
        key(name)?;
        with(handle, |db| {
            db.execute("DELETE FROM records WHERE key=?1", [name])
                .map(|count| count == 1)
                .map_err(|e| e.to_string())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_kv_increment(handle: *mut c_void, name: *const c_char, delta: i64) -> i64 {
    run(|| {
        let name = text(name)?;
        key(name)?;
        with(handle, |db| {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            let value: String = tx
                .query_row("SELECT json FROM records WHERE key=?1", [name], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?
                .ok_or("Cannot increment a missing KeyValueStore key")?;
            let value: serde_json::Value =
                serde_json::from_str(&value).map_err(|e| e.to_string())?;
            let current = value
                .as_i64()
                .ok_or("KeyValueStore increment requires a signed JSON integer")?;
            let updated = current
                .checked_add(delta)
                .ok_or("KeyValueStore integer overflow")?;
            tx.execute(
                "UPDATE records SET json=?2 WHERE key=?1",
                params![name, updated.to_string()],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(updated)
        })
    })
}
fn close(database: &Mutex<Database>) {
    if let Some(connection) = database.lock().unwrap().connection.take() {
        let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}
#[no_mangle]
pub extern "C" fn ube_kv_close(handle: *mut c_void) {
    let database = handles().lock().unwrap().remove(&(handle as usize));
    if let Some(database) = database {
        close(&database);
    }
}
pub fn cleanup() {
    let all = std::mem::take(&mut *handles().lock().unwrap());
    for database in all.into_values() {
        close(&database);
    }
}
