use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::os::raw::c_char;
use std::path::Path as StdPath;
use std::sync::atomic::{AtomicBool, Ordering};

mod http;
mod http3;

use crate::collections::{ube_list_create, UbeList};
use crate::datetime::{ube_datetime_from_unix, UbeDateTime};

// Runtime failures use the same pending exception channel as generated methods.
fn fail(message: impl Into<String>) {
    crate::exceptions::set_pending(crate::exceptions::generic(message.into()));
}

fn io_failure(operation: &str, path: &str, error: std::io::Error) {
    let name = match error.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        std::io::ErrorKind::AlreadyExists => "AlreadyExistsError",
        std::io::ErrorKind::InvalidInput => "InvalidPathError",
        _ => "FileError",
    };
    crate::exceptions::set_pending(crate::exceptions::named(
        name,
        format!("Cannot {operation} '{path}': {error}"),
    ));
}

fn text_arg(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        fail("I/O argument must not be null");
        return None;
    }
    match unsafe { CStr::from_ptr(ptr) }.to_str() {
        Ok(value) => Some(value.to_owned()),
        Err(error) => {
            fail(format!("Invalid UTF-8 I/O argument: {error}"));
            None
        }
    }
}

fn file_handles() -> &'static std::sync::Mutex<std::collections::HashMap<usize, Option<String>>> {
    static HANDLES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<usize, Option<String>>>,
    > = std::sync::OnceLock::new();
    HANDLES.get_or_init(Default::default)
}

fn file_path(ptr: *const c_char) -> Option<String> {
    if let Some(path) = file_handles().lock().unwrap().get(&(ptr as usize)) {
        if path.is_none() {
            fail("File is closed");
        }
        return path.clone();
    }
    text_arg(ptr)
}

#[repr(C)]
pub struct UbeFile {
    pub path: *mut c_char,
    pub is_closed: bool,
}

#[repr(C)]
pub struct UbeFileInfo {
    pub size: i64,
    pub created_at: *mut UbeDateTime,
    pub modified_at: *mut UbeDateTime,
    pub is_directory: bool,
}

#[no_mangle]
pub extern "C" fn ube_file_open(path_ptr: *const c_char) -> *mut UbeFile {
    let Some(path) = text_arg(path_ptr) else {
        return std::ptr::null_mut();
    };
    let handle = Box::into_raw(Box::new(UbeFile {
        path: CString::new(path.clone()).unwrap().into_raw(),
        is_closed: false,
    }));
    file_handles()
        .lock()
        .unwrap()
        .insert(handle as usize, Some(path));
    handle
}

#[no_mangle]
pub extern "C" fn ube_file_read(file_or_path: *const c_char) -> *mut c_char {
    let Some(path) = file_path(file_or_path) else {
        return std::ptr::null_mut();
    };
    match fs::read_to_string(&path) {
        Ok(content) => match CString::new(content) {
            Ok(content) => content.into_raw(),
            Err(_) => {
                fail(format!(
                    "Cannot read '{path}' as a string: embedded NUL byte"
                ));
                std::ptr::null_mut()
            }
        },
        Err(error) => {
            io_failure("read", &path, error);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_write(path_ptr: *const c_char, content_ptr: *const c_char) {
    let Some(path) = file_path(path_ptr) else {
        return;
    };
    let Some(content) = text_arg(content_ptr) else {
        return;
    };
    if let Err(error) = fs::write(&path, content) {
        io_failure("write", &path, error);
    }
}

#[no_mangle]
pub extern "C" fn ube_file_append(path_ptr: *const c_char, content_ptr: *const c_char) {
    let Some(path) = file_path(path_ptr) else {
        return;
    };
    let Some(content) = text_arg(content_ptr) else {
        return;
    };
    if let Err(error) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(content.as_bytes()))
    {
        io_failure("append", &path, error);
    }
}

#[no_mangle]
pub extern "C" fn ube_file_delete(path_ptr: *const c_char) {
    let Some(path) = file_path(path_ptr) else {
        return;
    };
    if let Err(error) = fs::remove_file(&path) {
        io_failure("file_delete", &path, error);
    }
}

#[no_mangle]
pub extern "C" fn ube_file_exists(path_ptr: *const c_char) -> bool {
    let Some(path) = file_path(path_ptr) else {
        return false;
    };
    match StdPath::new(&path).try_exists() {
        Ok(exists) => exists,
        Err(error) => {
            io_failure("inspect", &path, error);
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_close(file_ptr: *mut UbeFile) {
    if !file_ptr.is_null() {
        unsafe {
            (*file_ptr).is_closed = true;
            file_handles()
                .lock()
                .unwrap()
                .insert(file_ptr as usize, None);
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_info(path_ptr: *const c_char) -> *mut UbeFileInfo {
    let Some(path_str) = file_path(path_ptr) else {
        return std::ptr::null_mut();
    };
    match fs::metadata(&path_str) {
        Ok(meta) => {
            let size = meta.len() as i64;
            let created_sec = meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mod_sec = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let created_at = ube_datetime_from_unix(created_sec);
            let modified_at = ube_datetime_from_unix(mod_sec);
            let info = Box::new(UbeFileInfo {
                size,
                created_at,
                modified_at,
                is_directory: meta.is_dir(),
            });
            Box::into_raw(info)
        }
        Err(error) => {
            io_failure("inspect", &path_str, error);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_fileinfo_size(ptr: *mut UbeFileInfo) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).size }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_fileinfo_created_at(ptr: *mut UbeFileInfo) -> *mut UbeDateTime {
    if !ptr.is_null() {
        unsafe { (*ptr).created_at }
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub extern "C" fn ube_fileinfo_modified_at(ptr: *mut UbeFileInfo) -> *mut UbeDateTime {
    if !ptr.is_null() {
        unsafe { (*ptr).modified_at }
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub extern "C" fn ube_fileinfo_is_directory(ptr: *mut UbeFileInfo) -> bool {
    if !ptr.is_null() {
        unsafe { (*ptr).is_directory }
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn ube_directory_create(path_ptr: *const c_char) {
    let Some(path) = file_path(path_ptr) else {
        return;
    };
    if let Err(error) = fs::create_dir_all(&path) {
        io_failure("directory_create", &path, error);
    }
}

#[no_mangle]
pub extern "C" fn ube_directory_delete(path_ptr: *const c_char) {
    let Some(path) = file_path(path_ptr) else {
        return;
    };
    if let Err(error) = fs::remove_dir_all(&path) {
        io_failure("directory_delete", &path, error);
    }
}

#[no_mangle]
pub extern "C" fn ube_directory_list(path_ptr: *const c_char) -> *mut UbeList {
    let list = ube_list_create();
    let Some(path) = text_arg(path_ptr) else {
        return list;
    };
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            io_failure("list", &path, error);
            return list;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                io_failure("list", &path, error);
                return list;
            }
        };
        let Ok(name) = entry.file_name().into_string() else {
            fail(format!("Invalid UTF-8 filename in '{path}'"));
            return list;
        };
        crate::collections::add_typed(list, CString::new(name).unwrap().into_raw() as i64, 1, 6, "string");
    }
    list
}

#[no_mangle]
pub extern "C" fn ube_path_join(p1_ptr: *const c_char, p2_ptr: *const c_char) -> *mut c_char {
    let p1 = if p1_ptr.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p1_ptr) }.to_str().unwrap_or("")
    };
    let p2 = if p2_ptr.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p2_ptr) }.to_str().unwrap_or("")
    };
    let joined = StdPath::new(p1).join(p2).to_string_lossy().to_string();
    CString::new(joined).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ube_path_filename(path_ptr: *const c_char) -> *mut c_char {
    if path_ptr.is_null() {
        return CString::new("").unwrap().into_raw();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let name = StdPath::new(path_str)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    CString::new(name).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ube_path_extension(path_ptr: *const c_char) -> *mut c_char {
    if path_ptr.is_null() {
        return CString::new("").unwrap().into_raw();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let ext = StdPath::new(path_str)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    CString::new(ext).unwrap().into_raw()
}

#[repr(C)]
pub struct UbeServer {
    pub port: i32,
    pub is_running: AtomicBool,
    routes: Vec<UbeServerRoute>,
    http2: bool,
    http3: Option<u16>,
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    sessions: crate::sessions::Store,
}

#[derive(Clone)]
struct UbeServerRoute {
    method: String,
    path: String,
    context: usize,
    handler: usize,
}

#[repr(C)]
pub struct UbeRequest {
    pub method: *mut c_char,
    pub url: *mut c_char,
    protocol: &'static str,
    sessions: crate::sessions::Store,
    cookies: String,
    headers: Vec<(String, String)>,
    session: usize,
    body: Option<Vec<u8>>,
}

#[repr(C)]
pub struct UbeResponse {
    pub status: i32,
    pub body: *mut c_char,
    location: Option<String>,
    content_type: &'static str,
}

#[repr(C)]
pub struct UbeCookie {
    pub name: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct UbeWebSocket {
    pub id: i64,
}

#[no_mangle]
pub extern "C" fn ube_server_create() -> *mut UbeServer {
    Box::into_raw(Box::new(UbeServer {
        port: 8080,
        is_running: AtomicBool::new(false),
        routes: Vec::new(),
        http2: false,
        http3: None,
        tls: None,
        sessions: Default::default(),
    }))
}

#[no_mangle]
pub extern "C" fn ube_server_start(ptr: *mut UbeServer, port: i32) {
    if ptr.is_null() {
        fail(format!("Ube server error: received a null server"));
        return;
    }
    let Ok(port) = u16::try_from(port) else {
        fail(format!(
            "Ube server error: port must be between 0 and 65535"
        ));
        return;
    };

    let server = unsafe { &mut *ptr };
    if server.http3.is_some() && server.tls.is_none() {
        fail("HTTP/3 requires server.tls(certPath, keyPath) before start()");
        return;
    }
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            fail(format!(
                "Ube server error: could not bind to 127.0.0.1:{port}: {error}"
            ));
            return;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        fail(format!(
            "Ube server error: could not configure listener: {error}"
        ));
        return;
    }
    let bound_port = match listener.local_addr() {
        Ok(address) => address.port(),
        Err(error) => {
            fail(format!(
                "Ube server error: could not inspect listener address: {error}"
            ));
            return;
        }
    };
    let udp = if let Some(port) = server.http3 {
        match std::net::UdpSocket::bind(("127.0.0.1", port)) {
            Ok(socket) => {
                match socket.local_addr().and_then(|address| {
                    socket.set_nonblocking(true)?;
                    Ok(address.port())
                }) {
                    Ok(port) => server.http3 = Some(port),
                    Err(error) => {
                        fail(format!("HTTP/3 listener configuration failed: {error}"));
                        return;
                    }
                }
                Some(socket)
            }
            Err(error) => {
                fail(format!("HTTP/3 could not bind UDP port {port}: {error}"));
                return;
            }
        }
    } else {
        None
    };
    server.port = i32::from(bound_port);
    server.is_running.store(true, Ordering::Release);
    let routes = server.routes.clone();
    let scheme = if server.tls.is_some() {
        "https"
    } else {
        "http"
    };
    eprintln!("Ube server listening at {scheme}://127.0.0.1:{bound_port}");

    let tls = server.tls.clone();
    let http2 = server.http2;
    let sessions = server.sessions.clone();
    // Callbacks may inspect the server and stop its atomic running flag. Do not
    // retain an exclusive borrow of the entire server across those callbacks.
    let running = unsafe { &(*ptr).is_running };
    if let Err(error) = http::serve_listener(listener, &routes, tls, http2, running, sessions, udp)
    {
        fail(format!("Ube server accept error: {error}"));
    }
    running.store(false, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn ube_server_stop(ptr: *mut UbeServer) {
    if !ptr.is_null() {
        unsafe { (*ptr).is_running.store(false, Ordering::Release) };
    }
}

#[no_mangle]
pub extern "C" fn ube_server_port(ptr: *mut UbeServer) -> i32 {
    if !ptr.is_null() {
        unsafe { (*ptr).port }
    } else {
        8080
    }
}

#[no_mangle]
pub extern "C" fn ube_server_tls(ptr: *mut UbeServer, cert: *const c_char, key: *const c_char) {
    if ptr.is_null() || cert.is_null() || key.is_null() {
        fail("Ube server TLS configuration requires a server, certificate path, and key path");
        return;
    }
    let cert = unsafe { CStr::from_ptr(cert) }.to_string_lossy();
    let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    match http::tls_config(&cert, &key) {
        Ok(config) => unsafe {
            (*ptr).tls = Some(std::sync::Arc::new(config));
        },
        Err(error) => fail(format!("Ube server TLS configuration failed: {error}")),
    }
}
#[no_mangle]
pub extern "C" fn ube_server_route(
    ptr: *mut UbeServer,
    path: *const c_char,
    method: *const c_char,
    context: *mut std::ffi::c_void,
    handler: i64,
) {
    if ptr.is_null() {
        fail("Ube server error: route registration received a null server");
        return;
    }
    if path.is_null() || method.is_null() || context.is_null() || handler == 0 {
        fail("Ube server error: route registration received invalid arguments");
        return;
    }
    let path = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    let method = unsafe { CStr::from_ptr(method) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        (*ptr).routes.push(UbeServerRoute {
            method,
            path,
            context: context as usize,
            handler: handler as usize,
        });
    }
}
#[no_mangle]
pub extern "C" fn ube_server_use(_ptr: *mut UbeServer, _handler: i64) {}

#[no_mangle]
pub extern "C" fn ube_request_path(_ptr: *mut UbeRequest, _key: *const c_char) -> *mut c_char {
    CString::new("1").unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_request_path_int(_ptr: *mut UbeRequest, _key: *const c_char) -> i32 {
    1
}
#[no_mangle]
pub extern "C" fn ube_request_path_long(_ptr: *mut UbeRequest, _key: *const c_char) -> i64 {
    1
}
#[no_mangle]
pub extern "C" fn ube_request_body(ptr: *mut UbeRequest) -> *mut c_char {
    let body = if ptr.is_null() {
        None
    } else {
        unsafe { &*ptr }.body.as_deref()
    };
    let Some(body) = body else {
        fail("Request.body requires a request with a buffered body");
        return std::ptr::null_mut();
    };
    if let Ok(text) = std::str::from_utf8(body) {
        if let Ok(value) = CString::new(text) {
            return value.into_raw();
        }
    }
    fail("Request.body requires UTF-8 text without null bytes");
    std::ptr::null_mut()
}
#[no_mangle]
pub extern "C" fn ube_request_query(ptr: *mut UbeRequest, key: *const c_char) -> *mut c_char {
    if ptr.is_null() || key.is_null() {
        fail("Request.query requires a request and a key");
        return std::ptr::null_mut();
    }
    let url = unsafe { CStr::from_ptr((*ptr).url) }.to_string_lossy();
    let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or("");
    let mut values = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(name, _)| name == &key)
        .map(|(_, value)| value.into_owned());
    let value = values.next().unwrap_or_default();
    if values.next().is_some() {
        fail("Request.query rejects duplicate parameter names");
        return std::ptr::null_mut();
    }
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => {
            fail("Request.query value contains a null byte");
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_request_header(ptr: *mut UbeRequest, key: *const c_char) -> *mut c_char {
    if ptr.is_null() || key.is_null() {
        fail("Request.header requires a request and a key");
        return std::ptr::null_mut();
    }
    let request = unsafe { &*ptr };
    let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    let mut values = request
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(&key));
    let value = values.next().map(|(_, value)| value.as_str()).unwrap_or("");
    if values.next().is_some() {
        fail("Request.header rejects duplicate header names");
        return std::ptr::null_mut();
    }
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => {
            fail("Request.header value contains a null byte");
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_request_has_bearer_token(ptr: *mut UbeRequest) -> bool {
    if ptr.is_null() {
        return false;
    }
    let request = unsafe { &*ptr };
    let mut values = request
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"));
    let Some((_, value)) = values.next() else {
        return false;
    };
    values.next().is_none()
        && value.split_once(' ').is_some_and(|(kind, token)| {
            kind.eq_ignore_ascii_case("Bearer")
                && !token.is_empty()
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
        })
}

#[no_mangle]
pub extern "C" fn ube_response_redirect(location: *const c_char) -> *mut UbeResponse {
    if location.is_null() {
        fail("Redirect location must not be null");
        return std::ptr::null_mut();
    }
    let location = unsafe { CStr::from_ptr(location) }
        .to_string_lossy()
        .into_owned();
    let relative = location.starts_with('/') && !location.starts_with("//");
    let absolute =
        url::Url::parse(&location).is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
    if (!relative && !absolute) || ::http::HeaderValue::from_str(&location).is_err() {
        fail("Redirect requires an HTTP URL or absolute path without header control characters");
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(UbeResponse {
        status: 302,
        body: CString::new("").unwrap().into_raw(),
        location: Some(location),
        content_type: "text/plain; charset=utf-8",
    }))
}

#[no_mangle]
pub extern "C" fn ube_response_create(status: i32, body_ptr: *const c_char) -> *mut UbeResponse {
    let body = if body_ptr.is_null() {
        CString::new("").unwrap().into_raw()
    } else {
        let cstr = unsafe { CStr::from_ptr(body_ptr) };
        CString::new(cstr.to_str().unwrap_or(""))
            .unwrap()
            .into_raw()
    };
    Box::into_raw(Box::new(UbeResponse {
        status,
        body,
        location: None,
        content_type: "text/plain; charset=utf-8",
    }))
}

/// JSON responses share the response body ABI while preserving their media type.
#[no_mangle]
pub extern "C" fn ube_response_json(body: *const c_char) -> *mut UbeResponse {
    let response = ube_response_create(200, body);
    unsafe { (*response).content_type = "application/json; charset=utf-8"; }
    response
}

/// Html values can only be produced by checked templates in generated code.
#[no_mangle]
pub extern "C" fn ube_response_html(body: *const c_char) -> *mut UbeResponse {
    let response = ube_response_create(200, body);
    unsafe { (*response).content_type = "text/html; charset=utf-8"; }
    response
}

struct ServerContext<'a> {
    routes: &'a [UbeServerRoute],
    sessions: crate::sessions::Store,
    secure: bool,
    alt_svc: Option<String>,
}

#[no_mangle]
pub extern "C" fn ube_request_session(ptr: *mut UbeRequest) -> *mut std::ffi::c_void {
    if ptr.is_null() {
        fail("Session requires an active request");
        return std::ptr::null_mut();
    }
    let request = unsafe { &mut *ptr };
    if request.session == 0 {
        match crate::sessions::begin(request.sessions.clone(), &request.cookies) {
            Ok(handle) => request.session = handle,
            Err(error) => {
                fail(format!("Session: {error}"));
                return std::ptr::null_mut();
            }
        }
    }
    request.session as *mut std::ffi::c_void
}

// The route callback ABI is shared by HTTP/1.1 and HTTP/2. Keep all generated
// callbacks on the server thread; response frames may be sent asynchronously.
fn dispatch_request(
    method: &str,
    url: &str,
    protocol: &'static str,
    context: &ServerContext<'_>,
    cookies: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> (u16, Vec<u8>, Option<String>, Option<String>, &'static str) {
    let path = url.split('?').next().unwrap_or(url);
    let Some(route) = context
        .routes
        .iter()
        .find(|route| route.method == method && route.path == path)
    else {
        return (404, b"Not Found".to_vec(), None, None, "text/plain; charset=utf-8");
    };
    let (Ok(method_string), Ok(url_string)) = (CString::new(method), CString::new(url)) else {
        return (400, b"Bad Request".to_vec(), None, None, "text/plain; charset=utf-8");
    };
    let mut request = UbeRequest {
        method: method_string.into_raw(),
        url: url_string.into_raw(),
        protocol,
        sessions: context.sessions.clone(),
        cookies: cookies.into(),
        headers,
        session: 0,
        body,
    };
    let handler: unsafe extern "C" fn(*mut std::ffi::c_void, *mut UbeRequest) -> *mut UbeResponse =
        unsafe { std::mem::transmute(route.handler) };
    let response = unsafe { handler(route.context as *mut std::ffi::c_void, &mut request) };
    unsafe {
        drop(CString::from_raw(request.method));
        drop(CString::from_raw(request.url));
    }
    let cookie = crate::sessions::finish(request.session, context.secure);
    let failed = crate::exceptions::take_pending() != 0;
    if response.is_null() {
        return (500, b"Internal Server Error".to_vec(), cookie, None, "text/plain; charset=utf-8");
    }
    let response = unsafe { Box::from_raw(response) };
    let body = if response.body.is_null() {
        Vec::new()
    } else {
        let body = unsafe { CString::from_raw(response.body) };
        body.as_bytes().to_vec()
    };
    if failed {
        return (500, b"Internal Server Error".to_vec(), cookie, None, "text/plain; charset=utf-8");
    }
    let status = if (200..600).contains(&response.status) {
        response.status as u16
    } else {
        500
    };
    (status, body, cookie, response.location, response.content_type)
}

#[no_mangle]
pub extern "C" fn ube_cookie_create(name: *const c_char, val: *const c_char) -> *mut UbeCookie {
    let n = if name.is_null() {
        CString::new("").unwrap().into_raw()
    } else {
        CString::new(unsafe { CStr::from_ptr(name) }.to_str().unwrap_or(""))
            .unwrap()
            .into_raw()
    };
    let v = if val.is_null() {
        CString::new("").unwrap().into_raw()
    } else {
        CString::new(unsafe { CStr::from_ptr(val) }.to_str().unwrap_or(""))
            .unwrap()
            .into_raw()
    };
    Box::into_raw(Box::new(UbeCookie { name: n, value: v }))
}

#[no_mangle]
pub extern "C" fn ube_server_enable_http2(ptr: *mut UbeServer) {
    if ptr.is_null() {
        fail("Ube server error: received a null server");
        return;
    }
    unsafe {
        (*ptr).http2 = true;
    }
}
#[no_mangle]
pub extern "C" fn ube_server_enable_http3(ptr: *mut UbeServer, port: i32) {
    if ptr.is_null() {
        fail("HTTP/3 requires a server");
        return;
    }
    if unsafe { (*ptr).is_running.load(Ordering::Acquire) } {
        fail("Configure HTTP/3 before starting the server");
        return;
    }
    let Ok(port) = u16::try_from(port) else {
        fail("HTTP/3 UDP port must be between 0 and 65535");
        return;
    };
    unsafe {
        (*ptr).http3 = Some(port);
    }
}
#[no_mangle]
pub extern "C" fn ube_server_http3_port(ptr: *mut UbeServer) -> i32 {
    if ptr.is_null() {
        return -1;
    }
    unsafe { (*ptr).http3.map(i32::from).unwrap_or(-1) }
}

#[no_mangle]
pub extern "C" fn ube_request_protocol(ptr: *mut UbeRequest) -> *mut c_char {
    let protocol = if ptr.is_null() {
        ""
    } else {
        unsafe { (*ptr).protocol }
    };
    CString::new(protocol).unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_request_is_http3(ptr: *mut UbeRequest) -> bool {
    !ptr.is_null() && unsafe { (*ptr).protocol == "HTTP/3" }
}

#[repr(C)]
pub struct UbeBytes {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

#[repr(C)]
pub struct UbeInputStream {
    pub file_handle: *mut fs::File,
    pub is_closed: bool,
}

#[repr(C)]
pub struct UbeOutputStream {
    pub file_handle: *mut fs::File,
    pub is_closed: bool,
}

#[no_mangle]
pub extern "C" fn ube_bytes_create(capacity: usize) -> *mut UbeBytes {
    let mut vec = vec![0u8; capacity];
    let ptr = vec.as_mut_ptr();
    let cap = vec.capacity();
    std::mem::forget(vec);
    Box::into_raw(Box::new(UbeBytes {
        data: ptr,
        len: cap,
        capacity: cap,
    }))
}

#[no_mangle]
pub extern "C" fn ube_bytes_free(ptr: *mut UbeBytes) {
    if !ptr.is_null() {
        unsafe {
            let b = Box::from_raw(ptr);
            if !b.data.is_null() {
                let _ = Vec::from_raw_parts(b.data, b.len, b.capacity);
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_open_read(path_ptr: *const c_char) -> *mut UbeInputStream {
    let Some(path_str) = file_path(path_ptr) else {
        return std::ptr::null_mut();
    };
    match fs::File::open(&path_str) {
        Ok(file) => Box::into_raw(Box::new(UbeInputStream {
            file_handle: Box::into_raw(Box::new(file)),
            is_closed: false,
        })),
        Err(error) => {
            io_failure("open", &path_str, error);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_open_write(path_ptr: *const c_char) -> *mut UbeOutputStream {
    let Some(path_str) = file_path(path_ptr) else {
        return std::ptr::null_mut();
    };
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path_str)
    {
        Ok(file) => Box::into_raw(Box::new(UbeOutputStream {
            file_handle: Box::into_raw(Box::new(file)),
            is_closed: false,
        })),
        Err(error) => {
            io_failure("open", &path_str, error);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_stream_read(
    stream_ptr: *mut UbeInputStream,
    bytes_ptr: *mut UbeBytes,
    offset: usize,
    count: usize,
) -> i64 {
    use std::io::Read;
    if stream_ptr.is_null() || bytes_ptr.is_null() {
        fail("Invalid or closed stream, buffer, or offset");
        return 0;
    }
    unsafe {
        if (*stream_ptr).is_closed || (*stream_ptr).file_handle.is_null() {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        let file = &mut *(*stream_ptr).file_handle;
        let buf = &mut *bytes_ptr;
        if offset > buf.capacity {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        let max_read = (buf.capacity - offset).min(count);
        let slice = std::slice::from_raw_parts_mut(buf.data.add(offset), max_read);
        match file.read(slice) {
            Ok(n) => n as i64,
            Err(error) => {
                fail(format!("Stream I/O failed: {error}"));
                0
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_stream_write(
    stream_ptr: *mut UbeOutputStream,
    bytes_ptr: *mut UbeBytes,
    offset: usize,
    count: usize,
) -> i64 {
    if stream_ptr.is_null() || bytes_ptr.is_null() {
        fail("Invalid or closed stream, buffer, or offset");
        return 0;
    }
    unsafe {
        if (*stream_ptr).is_closed || (*stream_ptr).file_handle.is_null() {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        let file = &mut *(*stream_ptr).file_handle;
        let buf = &*bytes_ptr;
        if offset > buf.len {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        let max_write = (buf.len - offset).min(count);
        let slice = std::slice::from_raw_parts(buf.data.add(offset), max_write);
        match file.write(slice) {
            Ok(n) => n as i64,
            Err(error) => {
                fail(format!("Stream I/O failed: {error}"));
                0
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_stream_pipe_to(
    src_ptr: *mut UbeInputStream,
    dst_ptr: *mut UbeOutputStream,
) -> i64 {
    if src_ptr.is_null() || dst_ptr.is_null() {
        fail("Invalid or closed stream, buffer, or offset");
        return 0;
    }
    unsafe {
        if (*src_ptr).is_closed || (*src_ptr).file_handle.is_null() {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        if (*dst_ptr).is_closed || (*dst_ptr).file_handle.is_null() {
            fail("Invalid or closed stream, buffer, or offset");
            return 0;
        }
        let src_file = &mut *(*src_ptr).file_handle;
        let dst_file = &mut *(*dst_ptr).file_handle;
        match std::io::copy(src_file, dst_file) {
            Ok(bytes_copied) => bytes_copied as i64,
            Err(error) => {
                fail(format!("Stream I/O failed: {error}"));
                0
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_file_sha256(path_ptr: *const c_char) -> *mut c_char {
    let Some(path_str) = file_path(path_ptr) else {
        return std::ptr::null_mut();
    };
    match crate::net::compute_file_sha256(&path_str) {
        Ok(hex_str) => CString::new(hex_str).unwrap().into_raw(),
        Err(error) => {
            fail(format!("Cannot hash '{path_str}': {error}"));
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_response_stream(
    status: i32,
    stream_ptr: *mut UbeInputStream,
) -> *mut UbeResponse {
    let body_desc = if stream_ptr.is_null() {
        "<empty stream>"
    } else {
        "<binary input stream>"
    };
    Box::into_raw(Box::new(UbeResponse {
        status,
        body: CString::new(body_desc).unwrap().into_raw(),
        location: None,
        content_type: "text/plain; charset=utf-8",
    }))
}

#[no_mangle]
pub extern "C" fn ube_response_file(path_ptr: *const c_char) -> *mut UbeResponse {
    let stream = ube_file_open_read(path_ptr);
    ube_response_stream(200, stream)
}

#[no_mangle]
pub extern "C" fn ube_websocket_send(_ptr: *mut UbeWebSocket, _msg: *const c_char) {}
#[no_mangle]
pub extern "C" fn ube_websocket_on_message(_ptr: *mut UbeWebSocket, _handler: i64) {}

#[repr(C)]
pub struct UbeJsonObject {
    pub id: i64,
}
#[repr(C)]
pub struct UbeJsonArray {
    pub id: i64,
}
#[no_mangle]
pub extern "C" fn ube_json_parse(ptr: *const c_char) -> *mut UbeJsonObject { crate::json::parse(ptr).cast() }
#[no_mangle]
pub extern "C" fn ube_json_stringify(obj: *mut UbeJsonObject) -> *mut c_char { crate::json::stringify(obj.cast()) }
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_string(obj: *mut UbeJsonObject, key: *const c_char) -> *mut c_char { crate::json::get_string(obj.cast(),key) }
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_int(obj: *mut UbeJsonObject, key: *const c_char) -> i32 { crate::json::get_int(obj.cast(),key) }
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_long(obj: *mut UbeJsonObject, key: *const c_char) -> i64 { crate::json::get_long(obj.cast(),key) }
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_decimal(obj: *mut UbeJsonObject, key: *const c_char) -> f64 { crate::json::get_decimal(obj.cast(),key) }
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_bool(obj: *mut UbeJsonObject, key: *const c_char) -> bool { crate::json::get_bool(obj.cast(),key) }

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn json_response_preserves_body_and_media_type() {
        let response = ube_response_json(c"{\"score\":30}".as_ptr());
        unsafe {
            let response = Box::from_raw(response);
            assert_eq!(response.status, 200);
            assert_eq!(response.content_type, "application/json; charset=utf-8");
            let body = CString::from_raw(response.body);
            assert_eq!(body.to_str().unwrap(), "{\"score\":30}");
        }
    }

    #[test]
    fn stream_eof_is_success_but_closed_stream_and_invalid_buffer_raise() {
        let path = std::env::temp_dir().join(format!("ube-stream-errors-{}", std::process::id()));
        fs::write(&path, b"data").unwrap();
        let path_arg = CString::new(path.to_str().unwrap()).unwrap();
        let stream = ube_file_open_read(path_arg.as_ptr());
        let bytes = ube_bytes_create(4);
        assert_eq!(ube_stream_read(stream, bytes, 0, 4), 4);
        assert_eq!(crate::exceptions::take_pending(), 0);
        assert_eq!(ube_stream_read(stream, bytes, 0, 4), 0);
        assert_eq!(crate::exceptions::take_pending(), 0, "EOF is not an error");
        assert_eq!(ube_stream_read(stream, bytes, 5, 1), 0);
        assert_ne!(crate::exceptions::take_pending(), 0);
        unsafe {
            (*stream).is_closed = true;
        }
        assert_eq!(ube_stream_read(stream, bytes, 0, 4), 0);
        assert_ne!(crate::exceptions::take_pending(), 0);
        unsafe {
            let stream = Box::from_raw(stream);
            drop(Box::from_raw(stream.file_handle));
        }
        ube_bytes_free(bytes);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn stream_open_and_directory_creation_failures_set_pending_errors() {
        let path = std::env::temp_dir().join(format!("ube-io-errors-{}", std::process::id()));
        fs::write(&path, b"not a directory").unwrap();
        let nested = CString::new(path.join("child").to_str().unwrap()).unwrap();
        assert!(ube_file_open_read(nested.as_ptr()).is_null());
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert!(ube_file_open_write(nested.as_ptr()).is_null());
        assert_ne!(crate::exceptions::take_pending(), 0);
        ube_directory_create(nested.as_ptr());
        assert_ne!(crate::exceptions::take_pending(), 0);
        fs::remove_file(path).unwrap();
    }
}
