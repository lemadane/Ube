//! Bounded local/persistent key-value caches and a shared TCP cache service.
mod store;
mod transport;
use serde_json::Value;
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    net::SocketAddr,
    path::Path,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use store::Store;
use transport::Server;
type Result<T> = std::result::Result<T, String>;
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
static EVICTIONS: AtomicU64 = AtomicU64::new(0);
static EXPIRATIONS: AtomicU64 = AtomicU64::new(0);
fn stat(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}
enum Backend {
    Local {
        store: Arc<Mutex<Store>>,
        server: Mutex<Option<Server>>,
    },
    Remote {
        address: SocketAddr,
        token: String,
    },
}
fn handles() -> &'static Mutex<HashMap<usize, Arc<Backend>>> {
    static HANDLES: OnceLock<Mutex<HashMap<usize, Arc<Backend>>>> = OnceLock::new();
    HANDLES.get_or_init(Default::default)
}
fn register(backend: Backend) -> Result<*mut c_void> {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    let id = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| "Cache handle limit reached")?;
    handles().lock().unwrap().insert(id, Arc::new(backend));
    Ok(id as *mut c_void)
}
fn lookup(handle: *mut c_void) -> Result<Arc<Backend>> {
    handles()
        .lock()
        .unwrap()
        .get(&(handle as usize))
        .cloned()
        .ok_or("Invalid or closed cache handle".into())
}
fn text<'a>(pointer: *const c_char) -> Result<&'a str> {
    if pointer.is_null() {
        return Err("Cache string arguments cannot be null".into());
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|_| "Cache strings must be UTF-8".into())
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
fn local(path: Option<&Path>, limit: i64, durable: bool) -> Result<*mut c_void> {
    let store = Store::open(path, limit, durable)?;
    register(Backend::Local {
        store: Arc::new(Mutex::new(store)),
        server: Mutex::new(None),
    })
}
#[no_mangle]
pub extern "C" fn ube_cache_memory(limit: i64) -> *mut c_void {
    run(|| local(None, limit, true))
}
#[no_mangle]
pub extern "C" fn ube_cache_open(path: *const c_char, limit: i64) -> *mut c_void {
    run(|| local(Some(Path::new(text(path)?)), limit, true))
}
#[no_mangle]
pub extern "C" fn ube_cache_open_buffered(path: *const c_char, limit: i64) -> *mut c_void {
    run(|| local(Some(Path::new(text(path)?)), limit, false))
}
#[no_mangle]
pub extern "C" fn ube_cache_connect(address: *const c_char, token: *const c_char) -> *mut c_void {
    run(|| {
        let token = text(token)?;
        transport::token_valid(token)?;
        register(Backend::Remote {
            address: transport::address(text(address)?)?,
            token: token.into(),
        })
    })
}
fn execute(handle: *mut c_void, op: &str, key: &str, value: &str, number: i64) -> Result<Value> {
    match lookup(handle)?.as_ref() {
        Backend::Local { store, .. } => store.lock().unwrap().execute(op, key, value, number),
        Backend::Remote { address, token } => {
            transport::request(*address, token, op, key, value, number)
        }
    }
}
fn string(value: &str) -> Result<*mut c_char> {
    if value.contains('\0') {
        return Err("Cache values returned to Ube cannot contain NUL bytes".into());
    }
    let output = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if output.is_null() {
        return Err("Cannot allocate cache result".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), output.cast(), value.len());
        *output.add(value.len()) = 0;
    }
    Ok(output)
}
#[no_mangle]
pub extern "C" fn ube_cache_get(handle: *mut c_void, key: *const c_char) -> *mut c_char {
    run(|| match execute(handle, "get", text(key)?, "", 0)? {
        Value::Null => Ok(std::ptr::null_mut()),
        Value::String(value) => string(&value),
        _ => Err("Invalid cache get response".into()),
    })
}
#[no_mangle]
pub extern "C" fn ube_cache_set(
    handle: *mut c_void,
    key: *const c_char,
    value: *const c_char,
    ttl: i64,
) {
    run(|| execute(handle, "set", text(key)?, text(value)?, ttl).map(|_| ()))
}
#[no_mangle]
pub extern "C" fn ube_cache_add(
    handle: *mut c_void,
    key: *const c_char,
    value: *const c_char,
    ttl: i64,
) -> bool {
    run(|| {
        execute(handle, "add", text(key)?, text(value)?, ttl)?
            .as_bool()
            .ok_or("Invalid cache add response".into())
    })
}
#[no_mangle]
pub extern "C" fn ube_cache_increment(handle: *mut c_void, key: *const c_char, delta: i64) -> i64 {
    run(|| {
        execute(handle, "increment", text(key)?, "", delta)?
            .as_i64()
            .ok_or("Invalid cache increment response".into())
    })
}
#[no_mangle]
pub extern "C" fn ube_cache_delete(handle: *mut c_void, key: *const c_char) -> bool {
    run(|| {
        execute(handle, "delete", text(key)?, "", 0)?
            .as_bool()
            .ok_or("Invalid cache delete response".into())
    })
}
#[no_mangle]
pub extern "C" fn ube_cache_listen(
    handle: *mut c_void,
    address: *const c_char,
    token: *const c_char,
) {
    run(|| match lookup(handle)?.as_ref() {
        Backend::Local { store, server } => {
            let mut slot = server.lock().unwrap();
            if store.lock().unwrap().is_closed() {
                return Err("Cache is closed".into());
            }
            if slot.is_some() {
                return Err("Cache listener is already running".into());
            }
            *slot = Some(Server::start(
                transport::address(text(address)?)?,
                text(token)?,
                store.clone(),
            )?);
            Ok(())
        }
        Backend::Remote { .. } => Err("A remote cache client cannot start a listener".into()),
    })
}
fn stop(backend: &Backend) {
    if let Backend::Local { store, server } = backend {
        let mut slot = server.lock().unwrap();
        slot.take();
        store.lock().unwrap().close();
    }
}
#[no_mangle]
pub extern "C" fn ube_cache_close(handle: *mut c_void) {
    let backend = handles().lock().unwrap().remove(&(handle as usize));
    if let Some(backend) = backend {
        stop(&backend);
    }
}
pub fn cleanup() {
    let backends = std::mem::take(&mut *handles().lock().unwrap());
    for backend in backends.into_values() {
        stop(&backend);
    }
}
pub(crate) fn prometheus() -> String {
    let backends: Vec<_> = handles().lock().unwrap().values().cloned().collect();
    let (mut items, mut bytes) = (0, 0);
    for backend in backends {
        if let Backend::Local { store, .. } = backend.as_ref() {
            let (i, b) = store.lock().unwrap().sizes();
            items += i;
            bytes += b;
        }
    }
    let mut result = String::new();
    for (name, value) in [
        ("hits", HITS.load(Ordering::Relaxed)),
        ("misses", MISSES.load(Ordering::Relaxed)),
        ("writes", WRITES.load(Ordering::Relaxed)),
        ("evictions", EVICTIONS.load(Ordering::Relaxed)),
        ("expirations", EXPIRATIONS.load(Ordering::Relaxed)),
    ] {
        result.push_str(&format!(
            "# TYPE ube_cache_{name}_total counter\nube_cache_{name}_total {value}\n"
        ));
    }
    result.push_str(&format!("# TYPE ube_cache_items gauge\nube_cache_items {items}\n# TYPE ube_cache_accounted_bytes gauge\nube_cache_accounted_bytes {bytes}\n"));
    result
}

/// Standalone command shares the exact embedded store and transport implementation.
pub fn command(arguments: &[String]) -> Result<()> {
    if arguments == ["--help"] || arguments == ["-h"] {
        println!("Usage: ube cache [--bind IP:port] [--memory bytes] [--disk path] [--buffered] [--metrics port] [--token-env name]");
        println!("Defaults: 127.0.0.1:6380, 67108864 accounted bytes, token from UBE_CACHE_TOKEN. Disk writes are durable unless --buffered is supplied.");
        return Ok(());
    }
    let mut bind = "127.0.0.1:6380".to_owned();
    let mut limit = 64 * 1024 * 1024;
    let mut path = None;
    let mut token_env = "UBE_CACHE_TOKEN".to_owned();
    let mut durable = true;
    let mut metrics = None;
    let mut index = 0;
    while index < arguments.len() {
        let flag = &arguments[index];
        if flag == "--buffered" {
            durable = false;
        } else {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or_else(|| format!("Missing value for {flag}"))?;
            match flag.as_str() {
                "--bind" => bind = value.clone(),
                "--memory" => {
                    limit = value
                        .parse()
                        .map_err(|_| "--memory requires a positive byte count")?
                }
                "--disk" => path = Some(value.clone()),
                "--token-env" => token_env = value.clone(),
                "--metrics" => {
                    let port: u16 = value
                        .parse()
                        .map_err(|_| "--metrics requires a port from 1 to 65535")?;
                    if port == 0 {
                        return Err("Metrics port must be nonzero".into());
                    }
                    metrics = Some(port);
                }
                _ => return Err(format!("Unknown cache option: {flag}")),
            }
        }
        index += 1;
    }
    if !durable && path.is_none() {
        return Err("--buffered requires --disk".into());
    }
    let token = std::env::var(&token_env)
        .map_err(|_| format!("Set {token_env} to a cache authentication token"))?;
    transport::token_valid(&token)?;
    let bind_address = transport::address(&bind)?;
    let handle = local(path.as_deref().map(Path::new), limit, durable)?;
    let backend = lookup(handle)?;
    if let Backend::Local { store, server } = backend.as_ref() {
        let listener = match Server::start(bind_address, &token, store.clone()) {
            Ok(listener) => listener,
            Err(error) => {
                ube_cache_close(handle);
                return Err(error);
            }
        };
        *server.lock().unwrap() = Some(listener);
    }
    if let Some(port) = metrics {
        if let Err(error) = crate::observability::start(port) {
            ube_cache_close(handle);
            return Err(error);
        }
        crate::observability::ube_health_ready(true);
    }
    println!(
        "Ube cache listening on {bind} ({})",
        if path.is_none() {
            "memory"
        } else if durable {
            "durable"
        } else {
            "buffered"
        }
    );
    // A service manager owns the standalone process lifetime. Every durable operation
    // has already committed before acknowledgement, including on forced termination.
    loop {
        std::thread::park();
    }
}
