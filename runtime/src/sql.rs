//! Shared SQL execution engine. Builders and ORMs can target this same ABI.
//! Runtime handles are monotonic IDs; database IO never holds the registry lock.
use futures_util::TryStreamExt;
use sqlx::{
    any::{AnyArguments, AnyConnectOptions, AnyPoolOptions, AnyTypeInfoKind as Kind},
    Any, AnyPool, Arguments, Column, ConnectOptions, Either, Executor, Row, Statement, Transaction,
    ValueRef,
};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    future::Future,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, RwLock};
type Handle = *mut c_void;
type Result<T> = std::result::Result<T, String>;
const MAX_PARAMETERS: usize = 32766;
const MAX_ROWS: usize = 100_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq)]
enum Dialect {
    Postgres,
    Mysql,
    Sqlite,
}
impl Dialect {
    fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Sqlite => "sqlite",
        }
    }
    fn identifier(self, value: &str) -> Result<String> {
        if value.is_empty() || value.contains('\0') {
            return Err("identifier must be nonempty and contain no null bytes".into());
        }
        let quote = if self == Self::Mysql { '`' } else { '"' };
        Ok(format!(
            "{quote}{}{quote}",
            value.replace(quote, &format!("{quote}{quote}"))
        ))
    }
}
struct Client {
    pool: AnyPool,
    dialect: Dialect,
    timeout: Duration,
    // Every operation holds a read guard; close exclusively invalidates and drains transactions.
    closed: RwLock<bool>,
}
struct Tx {
    owner: usize,
    client: Arc<Client>,
    inner: AsyncMutex<Option<Transaction<'static, Any>>>,
}
#[derive(Clone, Debug)]
enum Param {
    Null(Kind),
    Bool(bool),
    Integer(i32),
    BigInt(i64),
    Double(f64),
    Text(String),
    Blob(Vec<u8>),
}
#[derive(Clone, Debug)]
enum Cell {
    Null,
    Bool(bool),
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
struct Rows {
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
    position: usize,
}
#[derive(Default)]
struct Params {
    values: Vec<Param>,
    bytes: usize,
}
#[derive(Default)]
struct Registry {
    next: usize,
    clients: HashMap<usize, Arc<Client>>,
    transactions: HashMap<usize, Arc<Tx>>,
    params: HashMap<usize, Params>,
    results: HashMap<usize, Rows>,
}
impl Registry {
    fn id(&mut self) -> Result<usize> {
        self.next = self.next.checked_add(1).ok_or("SQL handle limit reached")?;
        Ok(self.next)
    }
}
fn registry() -> &'static Mutex<Registry> {
    static STATE: OnceLock<Mutex<Registry>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
fn run<T: Default>(operation: impl FnOnce() -> Result<T>) -> T {
    match operation() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::named(
                "SQLError",
                format!("SQL: {error}"),
            ));
            T::default()
        }
    }
}
fn text(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err("string arguments must not be null".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8 argument".into())
}
fn string(value: &str) -> Result<*mut c_char> {
    if value.contains('\0') {
        return Err("text contains a null byte; select binary data as a blob".into());
    }
    let ptr = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if ptr.is_null() {
        return Err("allocation failed".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), ptr as *mut u8, value.len());
        *ptr.add(value.len()) = 0;
    }
    Ok(ptr)
}
// Async driver work runs on the IO workers even when called by generated server callbacks.
// Exception state is set only by run() on the original Ube thread.
fn io<T: Send + 'static>(future: impl Future<Output = Result<T>> + Send + 'static) -> Result<T> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    crate::net::get_runtime().spawn(async move {
        let _ = send.send(future.await);
    });
    receive
        .recv()
        .map_err(|_| "SQL IO worker stopped".to_string())?
}
fn client(id: usize) -> Result<Arc<Client>> {
    registry()
        .lock()
        .unwrap()
        .clients
        .get(&id)
        .cloned()
        .ok_or_else(|| "invalid or closed SQL connection".into())
}
fn transaction(id: usize) -> Result<Arc<Tx>> {
    registry()
        .lock()
        .unwrap()
        .transactions
        .get(&id)
        .cloned()
        .ok_or_else(|| "invalid or finished SQL transaction".into())
}
// Do not leak SQL text, parameters, database URLs, or server-provided messages into errors.
fn db_error(error: sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(e) => format!(
            "database rejected the operation (code {})",
            e.code().as_deref().unwrap_or("unknown")
        ),
        sqlx::Error::PoolTimedOut => "connection pool acquisition timed out".into(),
        sqlx::Error::PoolClosed => "connection pool is closed".into(),
        sqlx::Error::Tls(_) => "database TLS handshake or certificate verification failed".into(),
        sqlx::Error::Io(_) => "database network IO failed".into(),
        sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::AnyDriverError(_) => {
            "unsupported database value type; cast it to a supported scalar or text in SQL".into()
        }
        _ => "database operation failed".into(),
    }
}
fn options(input: &str) -> Result<(Dialect, AnyConnectOptions, bool)> {
    let (dialect, mut url) =
        if input.starts_with("postgres://") || input.starts_with("postgresql://") {
            (Dialect::Postgres, input.to_owned())
        } else if input.starts_with("mysql://") {
            (Dialect::Mysql, input.to_owned())
        } else if let Some(tail) = input.strip_prefix("mariadb://") {
            (Dialect::Mysql, format!("mysql://{tail}"))
        } else if input.starts_with("sqlite:") {
            (Dialect::Sqlite, input.to_owned())
        } else {
            return Err(
            "expected a postgres://, postgresql://, mysql://, mariadb://, or sqlite: database URL"
                .into(),
        );
        };
    if input.len() > MAX_BYTES {
        return Err("database URL is too large".into());
    }
    let memory =
        dialect == Dialect::Sqlite && (input == "sqlite::memory:" || input == "sqlite://:memory:");
    if dialect != Dialect::Sqlite {
        let mut parsed = url::Url::parse(&url).map_err(|_| "invalid database URL")?;
        if parsed.host_str().is_none() || parsed.fragment().is_some() {
            return Err("database URL requires a host and no fragment".into());
        }
        let key = if dialect == Dialect::Postgres {
            "sslmode"
        } else {
            "ssl-mode"
        };
        let modes: Vec<_> = parsed
            .query_pairs()
            .filter(|(k, _)| k == "sslmode" || k == "ssl-mode")
            .map(|(_, v)| v)
            .collect();
        if modes.len() > 1 {
            return Err("duplicate TLS mode option".into());
        }
        // Remote connections default to verified TLS. Loopback defaults to local development mode.
        let host_override = if dialect == Dialect::Postgres {
            parsed
                .query_pairs()
                .filter(|(key, _)| key == "host" || key == "hostaddr")
                .last()
                .map(|(_, host)| host.into_owned())
        } else {
            None
        };
        let effective_host = host_override.as_deref().or(parsed.host_str());
        let local = matches!(
            effective_host,
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        ) || effective_host.is_some_and(|host| host.starts_with('/'));
        if modes.is_empty() {
            let mode = match (dialect, local) {
                (Dialect::Mysql, true) => "disabled",
                (Dialect::Mysql, false) => "verify_identity",
                (_, true) => "disable",
                _ => "verify-full",
            };
            parsed.query_pairs_mut().append_pair(key, mode);
        }
        url = parsed.to_string();
    }
    let options = AnyConnectOptions::from_str(&url)
        .map_err(|_| "invalid database connection options")?
        .disable_statement_logging();
    Ok((dialect, options, memory))
}
#[no_mangle]
pub extern "C" fn ube_sql_open(url: *const c_char) -> Handle {
    ube_sql_open_pool(url, 5, 5000)
}
#[no_mangle]
pub extern "C" fn ube_sql_open_pool(url: *const c_char, max: i32, timeout: i32) -> Handle {
    run(|| {
        if !(1..=128).contains(&max) || !(1..=300_000).contains(&timeout) {
            return Err(
                "pool size must be 1..128 and timeout must be 1..300000 milliseconds".into(),
            );
        }
        let (dialect, options, _memory) = options(&text(url)?)?;
        sqlx::any::install_default_drivers();
        let timeout = Duration::from_millis(timeout as u64);
        let pool = io(async move {
            tokio::time::timeout(
                timeout,
                AnyPoolOptions::new()
                    .max_connections(if dialect == Dialect::Sqlite {
                        1
                    } else {
                        max as u32
                    })
                    .min_connections(1)
                    .idle_timeout(None)
                    .max_lifetime(None)
                    .acquire_timeout(timeout)
                    .connect_with(options),
            )
            .await
            .map_err(|_| "database connection timed out".to_owned())?
            .map_err(db_error)
        })?;
        let mut state = registry().lock().unwrap();
        let id = state.id()?;
        state.clients.insert(
            id,
            Arc::new(Client {
                pool,
                dialect,
                timeout,
                closed: RwLock::new(false),
            }),
        );
        Ok(id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_dialect(h: Handle) -> *mut c_char {
    run(|| string(client(h as usize)?.dialect.name()))
}
#[no_mangle]
pub extern "C" fn ube_sql_placeholder(h: Handle, index: i32) -> *mut c_char {
    run(|| {
        let db = client(h as usize)?;
        if !(1..=MAX_PARAMETERS as i32).contains(&index) {
            return Err("placeholder indexes start at 1 and must be <= 32766".into());
        }
        string(&if db.dialect == Dialect::Postgres {
            format!("${index}")
        } else {
            "?".into()
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_quote_identifier(h: Handle, name: *const c_char) -> *mut c_char {
    run(|| string(&client(h as usize)?.dialect.identifier(&text(name)?)?))
}
#[no_mangle]
pub extern "C" fn ube_sql_params_create() -> Handle {
    run(|| {
        let mut s = registry().lock().unwrap();
        let id = s.id()?;
        s.params.insert(id, Params::default());
        Ok(id as Handle)
    })
}
fn push(h: Handle, value: Param) -> Result<()> {
    let mut state = registry().lock().unwrap();
    let params = state
        .params
        .get_mut(&(h as usize))
        .ok_or("invalid or closed SQLParams")?;
    if params.values.len() >= MAX_PARAMETERS {
        return Err("too many SQL parameters".into());
    }
    let size = |v: &Param| match v {
        Param::Text(v) => v.len(),
        Param::Blob(v) => v.len(),
        _ => 8,
    };
    let bytes = size(&value);
    if params.bytes + bytes > MAX_BYTES {
        return Err("SQL parameters exceed 16 MiB".into());
    }
    params.bytes += bytes;
    params.values.push(value);
    Ok(())
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_int(h: Handle, v: i32) {
    run(|| push(h, Param::Integer(v)))
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_long(h: Handle, v: i64) {
    run(|| push(h, Param::BigInt(v)))
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_decimal(h: Handle, v: f64) {
    run(|| {
        if !v.is_finite() {
            return Err("decimal parameter must be finite".into());
        }
        push(h, Param::Double(v))
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_bool(h: Handle, v: bool) {
    run(|| push(h, Param::Bool(v)))
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_string(h: Handle, v: *const c_char) {
    run(|| push(h, Param::Text(text(v)?.into())))
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_blob_hex(h: Handle, v: *const c_char) {
    run(|| {
        push(
            h,
            Param::Blob(
                hex::decode(text(v)?)
                    .map_err(|_| "invalid blob hex")?
                    .into(),
            ),
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_params_add_null(h: Handle, kind: *const c_char) {
    run(|| {
        let kind = match text(kind)?.as_str() {
            "int" => Kind::Integer,
            "long" => Kind::BigInt,
            "decimal" => Kind::Double,
            "bool" => Kind::Bool,
            "string" => Kind::Text,
            "blob" => Kind::Blob,
            _ => return Err("null type must be int, long, decimal, bool, string, or blob".into()),
        };
        push(h, Param::Null(kind))
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_params_count(h: Handle) -> i32 {
    run(|| {
        Ok(registry()
            .lock()
            .unwrap()
            .params
            .get(&(h as usize))
            .ok_or("invalid or closed SQLParams")?
            .values
            .len() as i32)
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_params_clear(h: Handle) {
    run(|| {
        *registry()
            .lock()
            .unwrap()
            .params
            .get_mut(&(h as usize))
            .ok_or("invalid or closed SQLParams")? = Params::default();
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_params_close(h: Handle) {
    registry().lock().unwrap().params.remove(&(h as usize));
}
fn parameters(h: Handle) -> Result<Vec<Param>> {
    registry()
        .lock()
        .unwrap()
        .params
        .get(&(h as usize))
        .map(|p| p.values.clone())
        .ok_or_else(|| "invalid or closed SQLParams".into())
}
fn arguments(params: Vec<Param>) -> Result<AnyArguments<'static>> {
    let mut args = AnyArguments::default();
    macro_rules! add {
        ($value:expr) => {
            args.add($value)
                .map_err(|_| "could not encode SQL parameter")?
        };
    }
    for p in params {
        match p {
            Param::Null(k) => match k {
                Kind::Bool => add!(None::<bool>),
                Kind::Integer => add!(None::<i32>),
                Kind::BigInt => add!(None::<i64>),
                Kind::Double => add!(None::<f64>),
                Kind::Text => add!(None::<String>),
                Kind::Blob => add!(None::<Vec<u8>>),
                _ => return Err("unsupported null parameter type".into()),
            },
            Param::Bool(v) => add!(v),
            Param::Integer(v) => add!(v),
            Param::BigInt(v) => add!(v),
            Param::Double(v) => add!(v),
            Param::Text(v) => add!(v),
            Param::Blob(v) => add!(v),
        }
    }
    Ok(args)
}
// Recognize statement separators only outside literals, quoted identifiers and comments.
// The server still parses and validates syntax. No values are ever interpolated here.
fn validate_sql(sql: &str, dialect: Dialect) -> Result<()> {
    if sql.is_empty() || sql.len() > MAX_BYTES || sql.contains('\0') {
        return Err("SQL must be nonempty, contain no null bytes, and fit in 16 MiB".into());
    }
    let b = sql.as_bytes();
    let mut i = 0;
    let mut ended = false;
    let mut first = None;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"--") || (dialect == Dialect::Mysql && b[i] == b'#') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if dialect != Dialect::Mysql && b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                return Err("unterminated SQL comment".into());
            }
            continue;
        }
        if ended {
            return Err("expected one SQL statement; issue separate calls for scripts".into());
        }
        if first.is_none() {
            let start = i;
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            first = Some(sql[start..i].to_ascii_uppercase());
            if i > start {
                continue;
            }
        }
        if b[i] == b';' {
            ended = true;
            i += 1;
            continue;
        }
        if b[i] == b'\''
            || b[i] == b'"'
            || b[i] == b'`'
            || (dialect == Dialect::Sqlite && b[i] == b'[')
        {
            let quote = b[i];
            let end = if quote == b'[' { b']' } else { quote };
            let escapes = dialect == Dialect::Mysql
                || (dialect == Dialect::Postgres
                    && quote == b'\''
                    && i > 0
                    && matches!(b[i - 1], b'e' | b'E'));
            i += 1;
            let mut closed = false;
            while i < b.len() {
                if escapes && b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                    continue;
                }
                if b[i] == end {
                    i += 1;
                    if i < b.len() && b[i] == end && quote != b'[' {
                        i += 1;
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if !closed {
                return Err("unterminated quoted SQL value".into());
            }
            continue;
        }
        if dialect == Dialect::Postgres && b[i] == b'$' {
            let start = i;
            let mut tag = i + 1;
            if tag < b.len() && (b[tag] == b'$' || b[tag].is_ascii_alphabetic() || b[tag] == b'_') {
                while tag < b.len() && (b[tag].is_ascii_alphanumeric() || b[tag] == b'_') {
                    tag += 1;
                }
                if tag < b.len() && b[tag] == b'$' {
                    let marker = &sql[start..=tag];
                    if let Some(end) = sql[tag + 1..].find(marker) {
                        i = tag + 1 + end + marker.len();
                        continue;
                    }
                    return Err("unterminated dollar-quoted SQL value".into());
                }
            }
        }
        i += 1;
    }
    match first.as_deref() {
        None | Some("") => Err("expected a SQL statement".into()),
        Some("BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "ABORT" | "SAVEPOINT" | "RELEASE" | "SET") => Err("use transaction handles for transaction control; session SET commands are not supported on pooled connections".into()),
        _ => Ok(()),
    }
}
fn cell(row: &sqlx::any::AnyRow, i: usize) -> Result<Cell> {
    let raw = row.try_get_raw(i).map_err(db_error)?;
    if raw.is_null() {
        return Ok(Cell::Null);
    }
    let kind = raw.type_info().kind();
    Ok(match kind {
        Kind::Null => Cell::Null,
        Kind::Bool => Cell::Bool(row.try_get(i).map_err(db_error)?),
        Kind::SmallInt | Kind::Integer | Kind::BigInt => {
            Cell::Integer(row.try_get(i).map_err(db_error)?)
        }
        Kind::Real => Cell::Real(row.try_get::<f32, _>(i).map_err(db_error)? as f64),
        Kind::Double => Cell::Real(row.try_get(i).map_err(db_error)?),
        Kind::Text => Cell::Text(row.try_get(i).map_err(db_error)?),
        Kind::Blob => Cell::Blob(row.try_get(i).map_err(db_error)?),
    })
}
enum Outcome {
    Count(i64),
    Rows(Rows),
}
async fn perform(
    connection: &mut sqlx::AnyConnection,
    sql: &str,
    params: Vec<Param>,
    query: bool,
) -> Result<Outcome> {
    // Preparing first also supplies column metadata for empty result sets.
    let statement = connection.prepare(sql).await.map_err(db_error)?;
    let expected = match statement.parameters() {
        Some(Either::Left(p)) => p.len(),
        Some(Either::Right(n)) => n,
        None => params.len(),
    };
    if expected != params.len() {
        return Err(format!(
            "statement expects {expected} parameters, got {}",
            params.len()
        ));
    }
    let args = arguments(params)?;
    if !query {
        let result = statement
            .query_with(args)
            .execute(connection)
            .await
            .map_err(db_error)?;
        return Ok(Outcome::Count(
            i64::try_from(result.rows_affected()).map_err(|_| "affected-row count exceeds long")?,
        ));
    }
    let columns: Vec<String> = statement
        .columns()
        .iter()
        .map(|c| c.name().to_owned())
        .collect();
    let mut bytes = columns.iter().map(String::len).sum::<usize>();
    let mut rows = vec![];
    let mut stream = statement.query_with(args).fetch(connection);
    while let Some(row) = stream.try_next().await.map_err(db_error)? {
        if rows.len() >= MAX_ROWS {
            return Err("query exceeds 100000 rows; use LIMIT and pagination".into());
        }
        let mut cells = Vec::with_capacity(row.len());
        for i in 0..row.len() {
            let value = cell(&row, i)?;
            bytes += match &value {
                Cell::Text(v) => v.len(),
                Cell::Blob(v) => v.len(),
                _ => 8,
            };
            if bytes > MAX_BYTES {
                return Err("query result exceeds 16 MiB; use LIMIT and pagination".into());
            }
            cells.push(value);
        }
        rows.push(cells);
    }
    Ok(Outcome::Rows(Rows {
        columns,
        rows,
        position: 0,
    }))
}
fn store(outcome: Outcome) -> Result<usize> {
    match outcome {
        Outcome::Rows(rows) => {
            let mut s = registry().lock().unwrap();
            let id = s.id()?;
            s.results.insert(id, rows);
            Ok(id)
        }
        Outcome::Count(_) => Err("expected query rows".into()),
    }
}
fn execute(
    h: Handle,
    sql: *const c_char,
    params: Handle,
    query: bool,
    tx: bool,
) -> Result<Outcome> {
    let id = h as usize;
    let sql = text(sql)?;
    let params = parameters(params)?; // Snapshot: later mutations never affect in-flight queries.
    if tx {
        let transaction = transaction(id)?;
        validate_sql(&sql, transaction.client.dialect)?;
        io(async move {
            let db = &transaction.client;
            let closed = db.closed.read().await;
            if *closed {
                return Err("SQL connection is closed".into());
            }
            let mut guard = transaction.inner.lock().await;
            let connection = guard.as_mut().ok_or("transaction has already finished")?;
            let result = tokio::time::timeout(db.timeout, perform(connection, &sql, params, query))
                .await
                .map_err(|_| "query timed out".to_owned())
                .and_then(|r| r);
            if result.is_err() {
                // An error invalidates the whole transaction, across all database dialects.
                if let Some(tx) = guard.take() {
                    let _ = tokio::time::timeout(db.timeout, tx.rollback()).await;
                }
                registry().lock().unwrap().transactions.remove(&id);
            }
            result
        })
    } else {
        let db = client(id)?;
        validate_sql(&sql, db.dialect)?;
        io(async move {
            let closed = db.closed.read().await;
            if *closed {
                return Err("SQL connection is closed".into());
            }
            let deadline = tokio::time::Instant::now() + db.timeout;
            let mut connection = tokio::time::timeout_at(deadline, db.pool.acquire())
                .await
                .map_err(|_| "pool acquisition timed out")?
                .map_err(db_error)?;
            let result =
                tokio::time::timeout_at(deadline, perform(&mut connection, &sql, params, query))
                    .await;
            // Network protocol state after cancellation must not return to the pool.
            // SQLite has one persistent worker/connection so :memory: data survives errors.
            if result.is_err() && db.dialect != Dialect::Sqlite {
                connection.close_on_drop();
            }
            result.map_err(|_| "query timed out".to_owned())?
        })
    }
}
#[no_mangle]
pub extern "C" fn ube_sql_execute(h: Handle, sql: *const c_char, params: Handle) -> i64 {
    run(|| match execute(h, sql, params, false, false)? {
        Outcome::Count(n) => Ok(n),
        _ => unreachable!(),
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_query(h: Handle, sql: *const c_char, params: Handle) -> Handle {
    run(|| Ok(store(execute(h, sql, params, true, false)?)? as Handle))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_execute(
    h: Handle,
    sql: *const c_char,
    params: Handle,
) -> i64 {
    run(|| match execute(h, sql, params, false, true)? {
        Outcome::Count(n) => Ok(n),
        _ => unreachable!(),
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_query(
    h: Handle,
    sql: *const c_char,
    params: Handle,
) -> Handle {
    run(|| Ok(store(execute(h, sql, params, true, true)?)? as Handle))
}
#[no_mangle]
pub extern "C" fn ube_sql_begin(h: Handle) -> Handle {
    run(|| {
        let owner = h as usize;
        let db = client(owner)?;
        let id = io(async move {
            let closed = db.closed.read().await;
            if *closed {
                return Err("SQL connection is closed".into());
            }
            let tx = tokio::time::timeout(db.timeout, db.pool.begin())
                .await
                .map_err(|_| "transaction acquisition timed out")?
                .map_err(db_error)?;
            let mut s = registry().lock().unwrap();
            let id = s.id()?;
            s.transactions.insert(
                id,
                Arc::new(Tx {
                    owner,
                    client: db.clone(),
                    inner: AsyncMutex::new(Some(tx)),
                }),
            );
            Ok(id)
        })?;
        Ok(id as Handle)
    })
}
fn finish(id: usize, commit: bool, missing_ok: bool) -> Result<()> {
    let tx = match transaction(id) {
        Ok(tx) => tx,
        Err(_) if missing_ok => return Ok(()),
        Err(e) => return Err(e),
    };
    io(async move {
        let closed = tx.client.closed.read().await;
        if *closed {
            return if missing_ok {
                Ok(())
            } else {
                Err("SQL connection is closed".into())
            };
        }
        let mut guard = tx.inner.lock().await;
        let inner = match guard.take() {
            Some(inner) => inner,
            None if missing_ok => return Ok(()),
            None => return Err("transaction has already finished".into()),
        };
        registry().lock().unwrap().transactions.remove(&id);
        tokio::time::timeout(tx.client.timeout, async {
            if commit {
                inner.commit().await
            } else {
                inner.rollback().await
            }
        })
        .await
        .map_err(|_| "transaction completion timed out")?
        .map_err(db_error)
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_commit(h: Handle) {
    run(|| finish(h as usize, true, false))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_rollback(h: Handle) {
    run(|| finish(h as usize, false, true))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_close(h: Handle) {
    run(|| finish(h as usize, false, true))
}
fn savepoint(h: Handle, name: *const c_char, command: &'static str) -> Result<()> {
    let id = h as usize;
    let tx = transaction(id)?;
    let sql = format!("{command} {}", tx.client.dialect.identifier(&text(name)?)?);
    io(async move {
        let closed = tx.client.closed.read().await;
        if *closed {
            return Err("SQL connection is closed".into());
        }
        let mut guard = tx.inner.lock().await;
        let inner = guard.as_mut().ok_or("transaction has already finished")?;
        let result =
            tokio::time::timeout(tx.client.timeout, sqlx::query(&sql).execute(&mut **inner))
                .await
                .map_err(|_| "savepoint operation timed out".to_owned())
                .and_then(|r| r.map(|_| ()).map_err(db_error));
        if result.is_err() {
            if let Some(inner) = guard.take() {
                let _ = tokio::time::timeout(tx.client.timeout, inner.rollback()).await;
            }
            registry().lock().unwrap().transactions.remove(&id);
        }
        result
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_savepoint(h: Handle, name: *const c_char) {
    run(|| savepoint(h, name, "SAVEPOINT"))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_rollback_to(h: Handle, name: *const c_char) {
    run(|| savepoint(h, name, "ROLLBACK TO SAVEPOINT"))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_release(h: Handle, name: *const c_char) {
    run(|| savepoint(h, name, "RELEASE SAVEPOINT"))
}
fn close(id: usize) -> Result<()> {
    let db = registry().lock().unwrap().clients.remove(&id);
    let Some(db) = db else {
        return Ok(());
    };
    io(async move {
        let mut closed = db.closed.write().await;
        *closed = true;
        let transactions = {
            let mut s = registry().lock().unwrap();
            let ids: Vec<_> = s
                .transactions
                .iter()
                .filter_map(|(&key, tx)| (tx.owner == id).then_some(key))
                .collect();
            ids.into_iter()
                .filter_map(|id| s.transactions.remove(&id))
                .collect::<Vec<_>>()
        };
        for tx in transactions {
            if let Some(inner) = tx.inner.lock().await.take() {
                let _ = tokio::time::timeout(db.timeout, inner.rollback()).await;
            }
        }
        tokio::time::timeout(db.timeout, db.pool.close())
            .await
            .map_err(|_| "connection shutdown timed out")?;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_close(h: Handle) {
    run(|| close(h as usize))
}
fn result<T>(h: Handle, f: impl FnOnce(&mut Rows) -> Result<T>) -> Result<T> {
    let mut s = registry().lock().unwrap();
    f(s.results
        .get_mut(&(h as usize))
        .ok_or("invalid or closed SQLResult")?)
}
fn index(i: i32, n: usize) -> Result<usize> {
    usize::try_from(i)
        .ok()
        .filter(|&i| i < n)
        .ok_or_else(|| "column index out of bounds (indexes start at 0)".into())
}
fn value(rows: &Rows, i: i32) -> Result<&Cell> {
    let row = rows
        .position
        .checked_sub(1)
        .and_then(|i| rows.rows.get(i))
        .ok_or("result is not on a row; call next() first")?;
    Ok(&row[index(i, row.len())?])
}
#[no_mangle]
pub extern "C" fn ube_sql_result_next(h: Handle) -> bool {
    run(|| {
        result(h, |r| {
            if r.position <= r.rows.len() {
                r.position += 1;
            }
            Ok(r.position <= r.rows.len())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_reset(h: Handle) {
    run(|| {
        result(h, |r| {
            r.position = 0;
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_row_count(h: Handle) -> i64 {
    run(|| result(h, |r| Ok(r.rows.len() as i64)))
}
#[no_mangle]
pub extern "C" fn ube_sql_result_column_count(h: Handle) -> i32 {
    run(|| result(h, |r| Ok(r.columns.len() as i32)))
}
#[no_mangle]
pub extern "C" fn ube_sql_result_column_name(h: Handle, i: i32) -> *mut c_char {
    run(|| result(h, |r| string(&r.columns[index(i, r.columns.len())?])))
}
#[no_mangle]
pub extern "C" fn ube_sql_result_column_index(h: Handle, name: *const c_char) -> i32 {
    run(|| {
        let name = text(name)?;
        result(h, |r| {
            let matches: Vec<_> = r
                .columns
                .iter()
                .enumerate()
                .filter(|(_, n)| *n == &name)
                .map(|(i, _)| i)
                .collect();
            if matches.len() > 1 {
                return Err("ambiguous column name; use aliases or indexes".into());
            }
            Ok(matches.first().map_or(-1, |&i| i as i32))
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_column_type(h: Handle, i: i32) -> *mut c_char {
    run(|| {
        result(h, |r| {
            string(match value(r, i)? {
                Cell::Null => "null",
                Cell::Bool(_) => "bool",
                Cell::Integer(_) => "integer",
                Cell::Real(_) => "real",
                Cell::Text(_) => "text",
                Cell::Blob(_) => "blob",
            })
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_is_null(h: Handle, i: i32) -> bool {
    run(|| result(h, |r| Ok(matches!(value(r, i)?, Cell::Null))))
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_int(h: Handle, i: i32) -> i32 {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Integer(v) => {
                i32::try_from(*v).map_err(|_| "integer exceeds int; use getLong".into())
            }
            _ => Err("column is not an integer".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_long(h: Handle, i: i32) -> i64 {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Integer(v) => Ok(*v),
            _ => Err("column is not an integer".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_decimal(h: Handle, i: i32) -> f64 {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Real(v) => Ok(*v),
            Cell::Integer(v) => Ok(*v as f64),
            _ => Err("column is not numeric".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_bool(h: Handle, i: i32) -> bool {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Bool(v) => Ok(*v),
            Cell::Integer(0) => Ok(false),
            Cell::Integer(1) => Ok(true),
            _ => Err("column is not bool or integer 0/1".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_string(h: Handle, i: i32) -> *mut c_char {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Text(v) => string(v),
            _ => Err("column is not text".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_get_blob_hex(h: Handle, i: i32) -> *mut c_char {
    run(|| {
        result(h, |r| match value(r, i)? {
            Cell::Blob(v) => string(&hex::encode(v)),
            _ => Err("column is not a blob".into()),
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_result_close(h: Handle) {
    registry().lock().unwrap().results.remove(&(h as usize));
}
pub fn cleanup() {
    let ids = registry()
        .lock()
        .unwrap()
        .clients
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for id in ids {
        let _ = close(id);
    }
    let mut s = registry().lock().unwrap();
    s.params.clear();
    s.results.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    fn c(value: &str) -> CString {
        CString::new(value).unwrap()
    }
    fn ok() {
        let error = crate::exceptions::take_pending();
        assert_eq!(error, 0, "{}", crate::exceptions::message(error));
    }
    fn error() -> String {
        let record = crate::exceptions::take_pending();
        assert_ne!(record, 0);
        crate::exceptions::message(record)
    }
    fn output(value: *mut c_char) -> String {
        assert!(!value.is_null());
        unsafe { CStr::from_ptr(value) }
            .to_str()
            .unwrap()
            .to_owned()
    }
    struct Db(Handle);
    impl Drop for Db {
        fn drop(&mut self) {
            ube_sql_close(self.0);
        }
    }
    struct Params(Handle);
    impl Params {
        fn new() -> Self {
            Self(ube_sql_params_create())
        }
    }
    impl Drop for Params {
        fn drop(&mut self) {
            ube_sql_params_close(self.0);
        }
    }
    fn open(url: &str) -> Db {
        let db = Db(ube_sql_open(c(url).as_ptr()));
        ok();
        assert!(!db.0.is_null());
        db
    }
    fn exec(db: Handle, sql: &str, p: Handle) -> i64 {
        let count = ube_sql_execute(db, c(sql).as_ptr(), p);
        ok();
        count
    }
    fn scalar(db: Handle, sql: &str, p: Handle) -> i64 {
        let r = ube_sql_query(db, c(sql).as_ptr(), p);
        ok();
        assert!(ube_sql_result_next(r));
        let value = ube_sql_result_get_long(r, 0);
        ok();
        ube_sql_result_close(r);
        value
    }
    #[test]
    fn sqlite_binding_results_injection_and_empty_metadata() {
        let db = open("sqlite::memory:");
        let p = Params::new();
        assert_eq!(output(ube_sql_dialect(db.0)), "sqlite");
        exec(db.0, "CREATE TABLE items(id INTEGER, name TEXT, flag INTEGER, score REAL, missing TEXT, data BLOB)", p.0);
        ube_sql_params_add_long(p.0, i64::MAX);
        ube_sql_params_add_string(p.0, c("O'Brien 🍠'); DROP TABLE items; --").as_ptr());
        ube_sql_params_add_bool(p.0, true);
        ube_sql_params_add_decimal(p.0, 2.5);
        ube_sql_params_add_null(p.0, c("string").as_ptr());
        ube_sql_params_add_blob_hex(p.0, c("0001ff").as_ptr());
        assert_eq!(ube_sql_params_count(p.0), 6);
        assert_eq!(
            exec(db.0, "INSERT INTO items VALUES (?, ?, ?, ?, ?, ?)", p.0),
            1
        );
        ube_sql_params_clear(p.0);
        let r = ube_sql_query(db.0, c("SELECT * FROM items").as_ptr(), p.0);
        ok();
        assert_eq!(ube_sql_result_row_count(r), 1);
        assert_eq!(ube_sql_result_column_count(r), 6);
        assert_eq!(output(ube_sql_result_column_name(r, 1)), "name");
        assert_eq!(ube_sql_result_column_index(r, c("missing").as_ptr()), 4);
        assert_eq!(ube_sql_result_column_index(r, c("absent").as_ptr()), -1);
        ube_sql_result_get_long(r, 0);
        assert!(error().contains("call next"));
        assert!(ube_sql_result_next(r));
        assert_eq!(ube_sql_result_get_long(r, 0), i64::MAX);
        ube_sql_result_get_int(r, 0);
        assert!(error().contains("getLong"));
        assert_eq!(
            output(ube_sql_result_get_string(r, 1)),
            "O'Brien 🍠'); DROP TABLE items; --"
        );
        assert!(ube_sql_result_get_bool(r, 2));
        assert_eq!(ube_sql_result_get_decimal(r, 3), 2.5);
        assert!(ube_sql_result_is_null(r, 4));
        assert_eq!(output(ube_sql_result_column_type(r, 4)), "null");
        assert_eq!(output(ube_sql_result_get_blob_hex(r, 5)), "0001ff");
        ube_sql_result_get_string(r, 4);
        assert!(error().contains("not text"));
        ube_sql_result_get_long(r, -1);
        assert!(error().contains("bounds"));
        assert!(!ube_sql_result_next(r));
        assert!(!ube_sql_result_next(r));
        ube_sql_result_reset(r);
        assert!(ube_sql_result_next(r));
        ok();
        // Detached, materialized result remains valid after connection closure.
        ube_sql_close(db.0);
        assert_eq!(ube_sql_result_get_long(r, 0), i64::MAX);
        ok();
        ube_sql_result_close(r);
        ube_sql_result_close(r);
        assert!(!ube_sql_result_next(r));
        error();
        let db = open("sqlite::memory:");
        let r = ube_sql_query(db.0, c("SELECT 1 AS id, 'x' AS name WHERE 0").as_ptr(), p.0);
        ok();
        assert_eq!(ube_sql_result_row_count(r), 0);
        assert_eq!(ube_sql_result_column_count(r), 2);
        assert!(!ube_sql_result_next(r));
        ube_sql_result_close(r);
        let r = ube_sql_query(db.0, c("SELECT 1 AS a, 2 AS a").as_ptr(), p.0);
        ok();
        ube_sql_result_column_index(r, c("a").as_ptr());
        assert!(error().contains("ambiguous"));
        ube_sql_result_close(r);
        ube_sql_params_add_decimal(p.0, f64::NAN);
        error();
        ube_sql_params_add_null(p.0, c("mystery").as_ptr());
        error();
        ube_sql_params_add_string(p.0, std::ptr::null());
        error();
        ube_sql_params_add_blob_hex(p.0, c("abc").as_ptr());
        error();
        assert_eq!(ube_sql_params_count(p.0), 0);
        ube_sql_params_close(p.0);
        ube_sql_params_count(p.0);
        error();
    }
    fn transaction_suite(url: &str) {
        let db = open(url);
        let empty = Params::new();
        exec(db.0, "DROP TABLE IF EXISTS ube_sql_transactions", empty.0);
        exec(
            db.0,
            "CREATE TABLE ube_sql_transactions(id INTEGER PRIMARY KEY)",
            empty.0,
        );
        let tx = ube_sql_begin(db.0);
        ok();
        ube_sql_transaction_execute(
            tx,
            c("INSERT INTO ube_sql_transactions VALUES (1)").as_ptr(),
            empty.0,
        );
        ok();
        ube_sql_transaction_savepoint(tx, c("point'\"`").as_ptr());
        ok();
        ube_sql_transaction_execute(
            tx,
            c("INSERT INTO ube_sql_transactions VALUES (2)").as_ptr(),
            empty.0,
        );
        ok();
        ube_sql_transaction_rollback_to(tx, c("point'\"`").as_ptr());
        ok();
        ube_sql_transaction_release(tx, c("point'\"`").as_ptr());
        ok();
        let rows = ube_sql_transaction_query(
            tx,
            c("SELECT id FROM ube_sql_transactions").as_ptr(),
            empty.0,
        );
        ok();
        assert!(ube_sql_result_next(rows));
        assert_eq!(ube_sql_result_get_long(rows, 0), 1);
        ube_sql_result_close(rows);
        ube_sql_transaction_commit(tx);
        ok();
        assert_eq!(
            scalar(db.0, "SELECT COUNT(*) FROM ube_sql_transactions", empty.0),
            1
        );
        ube_sql_transaction_commit(tx);
        error();
        ube_sql_transaction_close(tx);
        ok();
        let tx = ube_sql_begin(db.0);
        ok();
        ube_sql_transaction_execute(
            tx,
            c("INSERT INTO ube_sql_transactions VALUES (3)").as_ptr(),
            empty.0,
        );
        ok();
        // A database failure rolls back prior changes and invalidates the transaction.
        ube_sql_transaction_execute(
            tx,
            c("INSERT INTO ube_sql_transactions VALUES (1)").as_ptr(),
            empty.0,
        );
        error();
        ube_sql_transaction_commit(tx);
        error();
        ube_sql_transaction_close(tx);
        ok();
        assert_eq!(
            scalar(db.0, "SELECT COUNT(*) FROM ube_sql_transactions", empty.0),
            1
        );
        let tx = ube_sql_begin(db.0);
        ok();
        ube_sql_transaction_execute(
            tx,
            c("INSERT INTO ube_sql_transactions VALUES (4)").as_ptr(),
            empty.0,
        );
        ok();
        ube_sql_transaction_close(tx);
        ok();
        assert_eq!(
            scalar(db.0, "SELECT COUNT(*) FROM ube_sql_transactions", empty.0),
            1
        );
        exec(db.0, "DROP TABLE ube_sql_transactions", empty.0);
        let tx = ube_sql_begin(db.0);
        ok();
        ube_sql_close(db.0);
        ok();
        ube_sql_transaction_query(tx, c("SELECT 1").as_ptr(), empty.0);
        error();
        ube_sql_query(db.0, c("SELECT 1").as_ptr(), empty.0);
        error();
        ube_sql_transaction_close(tx);
        ube_sql_close(db.0);
        ok();
    }
    #[test]
    fn sqlite_transactions_savepoints_error_rollback_and_close() {
        transaction_suite("sqlite::memory:");
    }
    #[test]
    fn parameter_counts_single_statements_timeouts_and_pool_isolation() {
        let db = open("sqlite::memory:");
        let p = Params::new();
        ube_sql_execute(
            db.0,
            c("CREATE TABLE t(id INTEGER); INSERT INTO t VALUES (1)").as_ptr(),
            p.0,
        );
        assert!(error().contains("one SQL statement"));
        exec(db.0, "CREATE TABLE t(id INTEGER)", p.0);
        ube_sql_execute(db.0, c("INSERT INTO t VALUES (?)").as_ptr(), p.0);
        assert!(error().contains("expects 1"));
        ube_sql_execute(db.0, c("BEGIN").as_ptr(), p.0);
        assert!(error().contains("transaction handles"));
        let r = ube_sql_query(
            db.0,
            c("SELECT ';?;--' AS value; -- comment\n/* tail */").as_ptr(),
            p.0,
        );
        ok();
        ube_sql_result_close(r);
        let db2 = open("sqlite::memory:");
        ube_sql_query(db2.0, c("SELECT * FROM t").as_ptr(), p.0);
        error();
        let timed = Db(ube_sql_open_pool(c("sqlite::memory:").as_ptr(), 1, 50));
        ok();
        let tx = ube_sql_begin(timed.0);
        ok();
        ube_sql_query(timed.0, c("SELECT 1").as_ptr(), p.0);
        assert!(error().contains("timed out"));
        ube_sql_transaction_rollback(tx);
        ok();
        assert_eq!(scalar(timed.0, "SELECT 1", p.0), 1);
        // CPU-bound query cancellation must evict its connection rather than reusing a busy one.
        ube_sql_query(timed.0, c("WITH RECURSIVE t(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM t WHERE x<10000000) SELECT sum(x) FROM t").as_ptr(), p.0);
        assert!(error().contains("timed out"));
    }
    #[test]
    fn sqlite_file_persistence_readonly_and_rollback_on_close() {
        let mut id = [0u8; 16];
        getrandom::getrandom(&mut id).unwrap();
        let path = std::env::temp_dir().join(format!("ube-sql-persistence-{}.db", hex::encode(id)));
        struct File(std::path::PathBuf);
        impl Drop for File {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _file = File(path.clone());
        let db = open(&format!("sqlite://{}?mode=rwc", path.display()));
        let params = Params::new();
        exec(db.0, "CREATE TABLE persisted(id INTEGER)", params.0);
        exec(db.0, "INSERT INTO persisted VALUES (1)", params.0);
        let tx = ube_sql_begin(db.0);
        ok();
        ube_sql_transaction_execute(tx, c("INSERT INTO persisted VALUES (2)").as_ptr(), params.0);
        ok();
        ube_sql_close(db.0);
        ok();
        let readonly = open(&format!("sqlite://{}?mode=ro", path.display()));
        assert_eq!(
            scalar(readonly.0, "SELECT COUNT(*) FROM persisted", params.0),
            1
        );
        ube_sql_execute(
            readonly.0,
            c("INSERT INTO persisted VALUES (3)").as_ptr(),
            params.0,
        );
        error();
        assert_eq!(
            scalar(readonly.0, "SELECT COUNT(*) FROM persisted", params.0),
            1
        );
        ube_sql_close(readonly.0);
        ok();
        ube_sql_transaction_close(tx);
        ok();
    }
    #[test]
    fn urls_dialects_tls_defaults_and_sql_scanning() {
        for (url, dialect) in [
            ("postgres://u:p@example.test/db", Dialect::Postgres),
            ("mysql://u:p@example.test/db", Dialect::Mysql),
            ("mariadb://u:p@example.test/db", Dialect::Mysql),
            ("sqlite::memory:", Dialect::Sqlite),
        ] {
            let (actual, _, _) = options(url).unwrap();
            assert_eq!(actual, dialect);
        }
        assert_eq!(
            options("postgres://u@remote/db")
                .unwrap()
                .1
                .database_url
                .query()
                .unwrap(),
            "sslmode=verify-full"
        );
        assert_eq!(
            options("mysql://u@remote/db")
                .unwrap()
                .1
                .database_url
                .query()
                .unwrap(),
            "ssl-mode=verify_identity"
        );
        assert_eq!(
            options("mysql://u@localhost/db")
                .unwrap()
                .1
                .database_url
                .query()
                .unwrap(),
            "ssl-mode=disabled"
        );
        assert_eq!(
            options("mysql://u@remote/db?sslmode=required")
                .unwrap()
                .1
                .database_url
                .query()
                .unwrap(),
            "sslmode=required"
        );
        assert!(options("mysql://u@remote/db?sslmode=required&ssl-mode=disabled").is_err());
        assert!(options("postgres://u@localhost/db?host=remote")
            .unwrap()
            .1
            .database_url
            .query()
            .unwrap()
            .ends_with("sslmode=verify-full"));
        assert!(options("postgres://u@remote/db?host=127.0.0.1")
            .unwrap()
            .1
            .database_url
            .query()
            .unwrap()
            .ends_with("sslmode=disable"));

        for input in ["no", "sqlite", "http://host", "postgres://host/db#secret"] {
            assert!(options(input).is_err());
        }
        for input in [
            "SELECT 1; SELECT 2",
            "COMMIT",
            "/* x */ rollback",
            "--none",
            "SELECT 'oops",
            "SELECT 1 /*oops",
        ] {
            assert!(validate_sql(input, Dialect::Postgres).is_err(), "{input}");
        }
        for input in [
            "SELECT $$;? ' $$",
            "SELECT $tag$; ' $tag$;",
            "SELECT E'\\\';still quoted';",
            "/* nested /* comment */ */ SELECT 1",
            "SELECT '🍠;'",
            "SELECT $1 + $1;",
        ] {
            assert!(validate_sql(input, Dialect::Postgres).is_ok(), "{input}");
        }
        assert!(validate_sql("SELECT `a;b`, 'a\\\';b'", Dialect::Mysql).is_ok());
        assert!(validate_sql("SELECT [a;b]", Dialect::Sqlite).is_ok());
        assert_eq!(
            Dialect::Postgres.identifier("a.b\"").unwrap(),
            "\"a.b\"\"\""
        );
        assert_eq!(Dialect::Mysql.identifier("a`b").unwrap(), "`a``b`");
        assert!(ube_sql_open_pool(c("sqlite::memory:").as_ptr(), 0, 1).is_null());
        error();
    }
    // Opt-in real server tests; CI/local callers provide isolated databases explicitly.
    #[test]
    #[ignore = "requires an isolated PostgreSQL database in UBE_TEST_POSTGRES_URL"]
    fn postgres_live() {
        live_suite(&std::env::var("UBE_TEST_POSTGRES_URL").expect("set UBE_TEST_POSTGRES_URL"));
    }
    #[test]
    #[ignore = "requires an isolated MySQL/MariaDB database in UBE_TEST_MYSQL_URL"]
    fn mysql_live() {
        live_suite(&std::env::var("UBE_TEST_MYSQL_URL").expect("set UBE_TEST_MYSQL_URL"));
    }
    fn live_suite(url: &str) {
        transaction_suite(url);
        let db = open(url);
        let p = Params::new();
        let pg = output(ube_sql_dialect(db.0)) == "postgres";
        ube_sql_params_add_int(p.0, 42);
        ube_sql_params_add_string(p.0, c("O'Brien 🍠").as_ptr());
        let query = if pg {
            "SELECT $1::INTEGER AS id, $2::TEXT AS name"
        } else {
            "SELECT CAST(? AS SIGNED) AS id, CAST(? AS CHAR) AS name"
        };
        let r = ube_sql_query(db.0, c(query).as_ptr(), p.0);
        ok();
        assert!(ube_sql_result_next(r));
        assert_eq!(ube_sql_result_get_long(r, 0), 42);
        assert_eq!(output(ube_sql_result_get_string(r, 1)), "O'Brien 🍠");
        ok();
        ube_sql_result_close(r);
        ube_sql_params_clear(p.0);
        ube_sql_params_add_null(p.0, c("string").as_ptr());
        let query = if pg {
            "SELECT $1::TEXT AS nullable"
        } else {
            "SELECT CAST(? AS CHAR) AS nullable"
        };
        let r = ube_sql_query(db.0, c(query).as_ptr(), p.0);
        ok();
        assert!(ube_sql_result_next(r));
        assert!(ube_sql_result_is_null(r, 0));
        ok();
        ube_sql_result_close(r);
        ube_sql_params_clear(p.0);
        ube_sql_params_add_long(p.0, 5_000_000_000);
        ube_sql_params_add_decimal(p.0, 1.25);
        ube_sql_params_add_bool(p.0, true);
        ube_sql_params_add_blob_hex(p.0, c("0001ff").as_ptr());
        let query = if pg {
            "SELECT $1::BIGINT AS large, $2::DOUBLE PRECISION AS score, $3::BOOLEAN AS flag, $4::BYTEA AS data"
        } else {
            "SELECT CAST(? AS SIGNED) AS large, (? + 0e0) AS score, CAST(? AS SIGNED) AS flag, ? AS data"
        };
        let r = ube_sql_query(db.0, c(query).as_ptr(), p.0);
        ok();
        assert!(ube_sql_result_next(r));
        assert_eq!(ube_sql_result_get_long(r, 0), 5_000_000_000);
        assert_eq!(ube_sql_result_get_decimal(r, 1), 1.25);
        assert!(ube_sql_result_get_bool(r, 2));
        assert_eq!(output(ube_sql_result_get_blob_hex(r, 3)), "0001ff");
        ok();
        ube_sql_result_close(r);
        ube_sql_params_clear(p.0);
        // A shared client accepts simultaneous calls without holding the global handle lock across IO.
        let workers = (0..8)
            .map(|_| {
                let db = db.0 as usize;
                std::thread::spawn(move || {
                    let params = Params::new();
                    assert_eq!(scalar(db as Handle, "SELECT 42", params.0), 42);
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
