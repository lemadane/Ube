use super::{stat, Result, EVICTIONS, EXPIRATIONS, HITS, MISSES, WRITES};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const ENTRY_OVERHEAD: usize = 64;
const APPLICATION_ID: i64 = 0x55424543;
pub(super) const MAX_VALUE: usize = 1024 * 1024;
struct Entry {
    value: String,
    expires: i64,
    touched: u64,
}
impl Entry {
    fn cost(&self, key: &str) -> usize {
        key.len() + self.value.len() + ENTRY_OVERHEAD
    }
}
pub(super) struct Store {
    entries: BTreeMap<String, Entry>,
    used: usize,
    limit: usize,
    clock: u64,
    db: Option<Connection>,
    closed: bool,
}
fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "System clock is before Unix epoch")?
        .as_millis()
        .try_into()
        .map_err(|_| "System clock overflow".into())
}
fn key_valid(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 1024 || key.contains('\0') {
        Err("Cache keys must contain 1 to 1024 UTF-8 bytes".into())
    } else {
        Ok(())
    }
}
impl Store {
    pub(super) fn open(path: Option<&Path>, limit: i64, durable: bool) -> Result<Self> {
        let limit = usize::try_from(limit).map_err(|_| "Cache byte limit must be positive")?;
        if limit == 0 {
            return Err("Cache byte limit must be positive".into());
        }
        let mut store = Self {
            entries: BTreeMap::new(),
            used: 0,
            limit,
            clock: 0,
            db: None,
            closed: false,
        };
        if let Some(path) = path {
            if path.as_os_str().is_empty() {
                return Err("Cache database path cannot be empty".into());
            }
            if path == Path::new(":memory:") {
                return Err("Persistent caches require a disk path".into());
            }
            let mut db = Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
            )
            .map_err(|e| format!("Cannot open cache database: {e}"))?;
            db.busy_timeout(Duration::from_millis(250))
                .map_err(|e| e.to_string())?;
            let id: i64 = db
                .query_row("PRAGMA application_id", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            let tables: i64 = db
                .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if id != APPLICATION_ID && (id != 0 || tables != 0) {
                return Err("File is not a Ube cache database".into());
            }
            let version: i64 = db
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if id == APPLICATION_ID && version != 1 {
                return Err("Unsupported Ube cache database version".into());
            }
            // Keep an exclusive SQLite lock for the store lifetime. Independent in-memory
            // indexes must never open the same persistent store concurrently.
            db.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=256;").map_err(|e| format!("Cannot lock cache database: {e}"))?;
            let journal: String = db
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if journal != "wal" {
                return Err("Persistent caches require SQLite WAL support".into());
            }
            db.execute_batch(if durable {
                "PRAGMA synchronous=FULL;"
            } else {
                "PRAGMA synchronous=NORMAL;"
            })
            .map_err(|e| e.to_string())?;
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS cache_entries (key TEXT PRIMARY KEY, value TEXT NOT NULL, expires INTEGER NOT NULL, touched INTEGER NOT NULL); PRAGMA application_id=1430406467; PRAGMA user_version=1; COMMIT;").map_err(|e| format!("Cannot initialize cache database: {e}"))?;
            let time = now()?;
            {
                let mut statement = db.prepare("SELECT key, value, expires, touched FROM cache_entries WHERE expires=0 OR expires>?1 ORDER BY touched DESC, key").map_err(|e| e.to_string())?;
                let mut rows = statement.query([time]).map_err(|e| e.to_string())?;
                while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                    let key: String = row.get(0).map_err(|e| e.to_string())?;
                    let value: String = row.get(1).map_err(|e| e.to_string())?;
                    let expires: i64 = row.get(2).map_err(|e| e.to_string())?;
                    key_valid(&key)?;
                    if value.len() > MAX_VALUE || value.contains('\0') || expires < 0 {
                        return Err("Invalid cache database entry".into());
                    }
                    let touched: i64 = row.get(3).map_err(|e| e.to_string())?;
                    if touched < 0 {
                        return Err("Invalid cache recency value".into());
                    }
                    store.clock = store.clock.max(touched as u64);
                    let entry = Entry {
                        value,
                        expires,
                        touched: touched as u64,
                    };
                    if entry.cost(&key) <= store.limit.saturating_sub(store.used) {
                        store.used += entry.cost(&key);
                        store.entries.insert(key, entry);
                    }
                }
            }
            // The bounded in-memory set is authoritative, including expiry and startup
            // eviction when reopening with a smaller budget. Commit that set atomically.
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch("CREATE TEMP TABLE cache_keep (key TEXT PRIMARY KEY);")
                .map_err(|e| e.to_string())?;
            for key in store.entries.keys() {
                tx.execute("INSERT INTO cache_keep VALUES (?1)", [key])
                    .map_err(|e| e.to_string())?;
            }
            tx.execute_batch("DELETE FROM cache_entries WHERE key NOT IN (SELECT key FROM cache_keep); DROP TABLE cache_keep;").map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            store.db = Some(db);
        }
        Ok(store)
    }
    fn purge(&mut self, time: i64) {
        let entries = &mut self.entries;
        let used = &mut self.used;
        entries.retain(|key, entry| {
            if entry.expires != 0 && entry.expires <= time {
                *used -= entry.cost(key);
                stat(&EXPIRATIONS);
                false
            } else {
                true
            }
        });
    }
    pub(super) fn sizes(&mut self) -> (usize, usize) {
        if let Ok(time) = now() {
            self.purge(time);
        }
        (self.entries.len(), self.used)
    }
    pub(super) fn execute(
        &mut self,
        op: &str,
        key: &str,
        value: &str,
        number: i64,
    ) -> Result<Value> {
        if self.closed {
            return Err("Cache is closed".into());
        }
        key_valid(key)?;
        let time = now()?;
        if op == "get" {
            if self
                .entries
                .get(key)
                .is_some_and(|entry| entry.expires != 0 && entry.expires <= time)
            {
                let expired = self.entries.remove(key).unwrap();
                self.used -= expired.cost(key);
                stat(&EXPIRATIONS);
            }
        } else {
            self.purge(time);
        }
        self.clock = self.clock.saturating_add(1);
        if op == "get" {
            return Ok(match self.entries.get_mut(key) {
                Some(entry) => {
                    entry.touched = self.clock;
                    stat(&HITS);
                    json!(entry.value)
                }
                None => {
                    stat(&MISSES);
                    Value::Null
                }
            });
        }
        if op == "delete" {
            if !self.entries.contains_key(key) {
                return Ok(json!(false));
            }
            if let Some(db) = &mut self.db {
                db.execute("DELETE FROM cache_entries WHERE key=?1", [key])
                    .map_err(|e| format!("Cache delete failed: {e}"))?;
            }
            let old = self.entries.remove(key).unwrap();
            self.used -= old.cost(key);
            stat(&WRITES);
            return Ok(json!(true));
        }
        if !matches!(op, "set" | "add" | "increment") {
            return Err("Unknown cache operation".into());
        }
        if matches!(op, "set" | "add") {
            if number < 0 {
                return Err(
                    "Cache TTL must be nonnegative milliseconds; zero means no expiry".into(),
                );
            }
            if value.len() > MAX_VALUE || value.contains('\0') {
                return Err("Cache values must contain at most 1 MiB and no NUL bytes".into());
            }
        }
        if op == "add" && self.entries.contains_key(key) {
            return Ok(json!(false));
        }
        let (value, expires, response) = if op == "increment" {
            let entry = self
                .entries
                .get(key)
                .ok_or("Cannot increment a missing cache key")?;
            let current: i64 = entry
                .value
                .parse()
                .map_err(|_| "Cache value is not a signed integer")?;
            let updated = current
                .checked_add(number)
                .ok_or("Cache integer overflow")?;
            (updated.to_string(), entry.expires, json!(updated))
        } else {
            let expires = if number == 0 {
                0
            } else {
                time.checked_add(number).ok_or("Cache TTL overflow")?
            };
            (value.to_owned(), expires, json!(true))
        };
        let entry = Entry {
            value,
            expires,
            touched: self.clock,
        };
        let cost = entry.cost(key);
        if cost > self.limit {
            return Err("Cache entry exceeds the configured byte limit".into());
        }
        let old_cost = self.entries.get(key).map_or(0, |old| old.cost(key));
        let mut remaining = self.used - old_cost;
        let mut victims = Vec::new();
        if cost > self.limit - remaining {
            let mut candidates: Vec<_> = self
                .entries
                .iter()
                .filter(|(k, _)| k.as_str() != key)
                .map(|(k, v)| (v.touched, k.clone(), v.cost(k)))
                .collect();
            candidates.sort_unstable();
            for (_, candidate, size) in candidates {
                if cost <= self.limit - remaining {
                    break;
                }
                remaining -= size;
                victims.push(candidate);
            }
        }
        if let Some(db) = &mut self.db {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM cache_entries WHERE expires != 0 AND expires <= ?1",
                [time],
            )
            .map_err(|e| e.to_string())?;
            for victim in &victims {
                tx.execute("DELETE FROM cache_entries WHERE key=?1", [victim])
                    .map_err(|e| e.to_string())?;
            }
            tx.execute("INSERT INTO cache_entries(key,value,expires,touched) VALUES(?1,?2,?3,?4) ON CONFLICT(key) DO UPDATE SET value=excluded.value, expires=excluded.expires, touched=excluded.touched", params![key, entry.value, entry.expires, self.clock.min(i64::MAX as u64) as i64]).map_err(|e| format!("Cache write failed: {e}"))?;
            tx.commit()
                .map_err(|e| format!("Cache commit failed: {e}"))?;
        }
        for victim in victims {
            self.entries.remove(&victim);
            stat(&EVICTIONS);
        }
        self.entries.insert(key.into(), entry);
        self.used = remaining + cost;
        stat(&WRITES);
        Ok(response)
    }
    pub(super) fn is_closed(&self) -> bool {
        self.closed
    }
    pub(super) fn close(&mut self) {
        self.closed = true;
        if let Some(db) = self.db.take() {
            let _ = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_operations_limits_and_lru() {
        let mut store = Store::open(None, 132, true).unwrap(); // two 1-byte keys/values
        assert_eq!(store.execute("get", "a", "", 0).unwrap(), Value::Null);
        assert_eq!(store.execute("add", "a", "1", 0).unwrap(), json!(true));
        assert_eq!(store.execute("add", "a", "9", 0).unwrap(), json!(false));
        assert!(store.execute("add", "a", "9", -1).is_err());
        store.execute("set", "b", "2", 0).unwrap();
        assert_eq!(store.execute("get", "a", "", 0).unwrap(), json!("1"));
        store.execute("set", "c", "3", 0).unwrap();
        assert_eq!(store.execute("get", "b", "", 0).unwrap(), Value::Null);
        assert_eq!(store.execute("increment", "a", "", 3).unwrap(), json!(4));
        assert_eq!(store.execute("increment", "a", "", -1).unwrap(), json!(3));
        assert!(store.execute("set", "a", &"x".repeat(133), 0).is_err());
        assert_eq!(store.execute("get", "a", "", 0).unwrap(), json!("3"));
        assert_eq!(store.execute("delete", "a", "", 0).unwrap(), json!(true));
        assert_eq!(store.execute("delete", "a", "", 0).unwrap(), json!(false));
        for (op, key, value, number) in [
            ("set", "", "v", 0),
            ("set", "nul", "x\0y", 0),
            ("set", "k", "v", -1),
            ("set", "k", "v", i64::MAX),
            ("increment", "missing", "", 1),
            ("invalid", "k", "", 0),
        ] {
            assert!(store.execute(op, key, value, number).is_err());
        }
        store.close();
        assert!(store.execute("get", "c", "", 0).is_err());
        assert!(Store::open(None, 0, true).is_err());
        assert!(Store::open(None, -1, true).is_err());
    }
    #[test]
    fn concurrent_add_and_increment_are_atomic() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            Store::open(None, 4096, true).unwrap(),
        ));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let added = store
                        .lock()
                        .unwrap()
                        .execute("add", "counter", "0", 0)
                        .unwrap();
                    for _ in 0..500 {
                        store
                            .lock()
                            .unwrap()
                            .execute("increment", "counter", "", 1)
                            .unwrap();
                    }
                    added == json!(true)
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .filter_map(|t| t.join().unwrap().then_some(()))
                .count(),
            1
        );
        assert_eq!(
            store
                .lock()
                .unwrap()
                .execute("get", "counter", "", 0)
                .unwrap(),
            json!("4000")
        );
    }
    #[test]
    fn persistent_deletes_expiry_eviction_and_locking_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.db");
        {
            let mut store = Store::open(Some(&path), 1024, true).unwrap();
            store.execute("set", "keep", "value", 0).unwrap();
            store.execute("set", "deleted", "gone", 0).unwrap();
            store.execute("delete", "deleted", "", 0).unwrap();
            store.execute("set", "expired", "gone", 1).unwrap();
            assert!(Store::open(Some(&path), 1024, true).is_err());
        }
        std::thread::sleep(Duration::from_millis(10));
        {
            let mut store = Store::open(Some(&path), 1024, true).unwrap();
            assert_eq!(store.execute("get", "keep", "", 0).unwrap(), json!("value"));
            for key in ["deleted", "expired"] {
                assert_eq!(store.execute("get", key, "", 0).unwrap(), Value::Null);
            }
        }
        let mut smaller = Store::open(Some(&path), 1, true).unwrap();
        assert_eq!(smaller.sizes(), (0, 0));
        smaller.close();
        assert_eq!(
            Store::open(Some(&path), 1024, true)
                .unwrap()
                .execute("get", "keep", "", 0)
                .unwrap(),
            Value::Null
        );
    }
    #[test]
    fn disk_write_failure_does_not_publish_changes_or_evictions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.db");
        let mut store = Store::open(Some(&path), 132, true).unwrap();
        store.execute("set", "a", "1", 0).unwrap();
        store.execute("set", "b", "2", 0).unwrap();
        store.db.as_mut().unwrap().execute_batch("CREATE TRIGGER reject_insert BEFORE INSERT ON cache_entries BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
        assert!(store.execute("set", "c", "3", 0).is_err());
        assert!(store.execute("increment", "a", "", 1).is_err());
        for (key, value) in [("a", "1"), ("b", "2")] {
            assert_eq!(store.execute("get", key, "", 0).unwrap(), json!(value));
        }
        store
            .db
            .as_mut()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_insert;")
            .unwrap();
        drop(store);
        let mut reopened = Store::open(Some(&path), 132, true).unwrap();
        assert_eq!(reopened.execute("get", "a", "", 0).unwrap(), json!("1"));
        assert_eq!(reopened.execute("get", "b", "", 0).unwrap(), json!("2"));
    }
    #[test]
    fn durable_evictions_overwrites_and_increment_expiry_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eviction.db");
        {
            let mut store = Store::open(Some(&path), 132, true).unwrap();
            store.execute("set", "a", "1", 10000).unwrap();
            let expiry = store.entries["a"].expires;
            store.execute("increment", "a", "", 1).unwrap();
            assert_eq!(store.entries["a"].expires, expiry);
            store.execute("set", "b", "2", 0).unwrap();
            store.execute("get", "a", "", 0).unwrap();
            store.execute("set", "c", "3", 0).unwrap();
            store.execute("set", "a", "9", 0).unwrap();
        }
        let mut store = Store::open(Some(&path), 132, true).unwrap();
        for (key, value) in [("a", json!("9")), ("b", Value::Null), ("c", json!("3"))] {
            assert_eq!(store.execute("get", key, "", 0).unwrap(), value);
        }
        assert_eq!(store.entries["a"].expires, 0);
        let sync: i64 = store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 2);
        store.close();
        let buffered = Store::open(Some(&path), 132, false).unwrap();
        let sync: i64 = buffered
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 1);
    }
    #[test]
    fn invalid_databases_and_integer_values_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("foreign.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE important(value TEXT);")
            .unwrap();
        drop(db);
        assert!(Store::open(Some(&path), 1024, true).is_err());
        let mut store = Store::open(None, 4096, true).unwrap();
        store
            .execute("set", "max", &i64::MAX.to_string(), 0)
            .unwrap();
        assert!(store.execute("increment", "max", "", 1).is_err());
        store.execute("set", "text", "hello", 0).unwrap();
        assert!(store.execute("increment", "text", "", 1).is_err());
        store.execute("set", "empty", "", 0).unwrap();
        assert_eq!(store.execute("get", "empty", "", 0).unwrap(), json!(""));
        assert!(Store::open(Some(Path::new(":memory:")), 1024, true).is_err());
    }
}
