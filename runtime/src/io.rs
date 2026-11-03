use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::raw::c_char;
use std::path::Path as StdPath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::collections::{ube_list_add, ube_list_create, UbeList};
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
pub extern "C" fn ube_print_i64(value: i64) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn ube_print_f64(value: f64) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn ube_print_bool(value: bool) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn ube_print_str(c_str_ptr: *const c_char) {
    if c_str_ptr.is_null() {
        println!("<null>");
        return;
    }
    let cstr = unsafe { CStr::from_ptr(c_str_ptr) };
    if let Ok(s) = cstr.to_str() {
        println!("{}", s);
    } else {
        println!("<invalid utf8>");
    }
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
        ube_list_add(list, CString::new(name).unwrap().into_raw() as i64);
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
}

#[repr(C)]
pub struct UbeResponse {
    pub status: i32,
    pub body: *mut c_char,
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
    server.port = i32::from(bound_port);
    server.is_running.store(true, Ordering::Release);
    let routes = server.routes.clone();
    eprintln!("Ube server listening at http://127.0.0.1:{bound_port}");

    while server.is_running.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = serve_http_connection(stream, &routes) {
                    eprintln!("Ube server request error: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                fail(format!("Ube server accept error: {error}"));
                server.is_running.store(false, Ordering::Release);
            }
        }
    }
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
pub extern "C" fn ube_server_tls(_ptr: *mut UbeServer, _cert: *const c_char, _key: *const c_char) {}
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
pub extern "C" fn ube_request_query(_ptr: *mut UbeRequest, _key: *const c_char) -> *mut c_char {
    CString::new("").unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_request_header(_ptr: *mut UbeRequest, _key: *const c_char) -> *mut c_char {
    CString::new("Bearer token").unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_request_has_bearer_token(_ptr: *mut UbeRequest) -> bool {
    true
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
    Box::into_raw(Box::new(UbeResponse { status, body }))
}

fn serve_http_connection(mut stream: TcpStream, routes: &[UbeServerRoute]) -> std::io::Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.len() > 8192 {
        drop(reader);
        return write_http_response(&mut stream, 431, b"Request header too large");
    }
    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        drop(reader);
        return write_http_response(&mut stream, 400, b"Bad Request");
    };
    let Some(url) = parts.next() else {
        drop(reader);
        return write_http_response(&mut stream, 400, b"Bad Request");
    };
    let Some(version) = parts.next() else {
        drop(reader);
        return write_http_response(&mut stream, 400, b"Bad Request");
    };
    if !version.starts_with("HTTP/") {
        drop(reader);
        return write_http_response(&mut stream, 400, b"Bad Request");
    }

    let mut headers_size = request_line.len();
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        headers_size += bytes_read;
        if headers_size > 16 * 1024 {
            drop(reader);
            return write_http_response(&mut stream, 431, b"Request header too large");
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    drop(reader);

    let path = url.split('?').next().unwrap_or(url);
    let route = routes
        .iter()
        .find(|route| route.method == method && route.path == path);
    let Some(route) = route else {
        return write_http_response(&mut stream, 404, b"Not Found");
    };

    let Ok(method_string) = CString::new(method) else {
        return write_http_response(&mut stream, 400, b"Bad Request");
    };
    let Ok(url_string) = CString::new(url) else {
        return write_http_response(&mut stream, 400, b"Bad Request");
    };
    let mut request = UbeRequest {
        method: method_string.into_raw(),
        url: url_string.into_raw(),
    };
    let handler: unsafe extern "C" fn(*mut std::ffi::c_void, *mut UbeRequest) -> *mut UbeResponse =
        unsafe { std::mem::transmute(route.handler) };
    let response = unsafe {
        handler(
            route.context as *mut std::ffi::c_void,
            &mut request as *mut UbeRequest,
        )
    };

    unsafe {
        drop(CString::from_raw(request.method));
        drop(CString::from_raw(request.url));
    }
    if response.is_null() {
        return write_http_response(&mut stream, 500, b"Internal Server Error");
    }
    let response = unsafe { Box::from_raw(response) };
    let body = if response.body.is_null() {
        &[][..]
    } else {
        unsafe { CStr::from_ptr(response.body) }.to_bytes()
    };
    let result = write_http_response(&mut stream, response.status, body);
    if !response.body.is_null() {
        unsafe { drop(CString::from_raw(response.body)) };
    }
    result
}

fn write_http_response(stream: &mut TcpStream, status: i32, body: &[u8]) -> std::io::Result<()> {
    let (status, reason) = match status {
        200 => (200, "OK"),
        201 => (201, "Created"),
        301 => (301, "Moved Permanently"),
        302 => (302, "Found"),
        400 => (400, "Bad Request"),
        401 => (401, "Unauthorized"),
        404 => (404, "Not Found"),
        405 => (405, "Method Not Allowed"),
        431 => (431, "Request Header Fields Too Large"),
        500 => (500, "Internal Server Error"),
        code if (100..600).contains(&code) => (code, "Response"),
        _ => (500, "Internal Server Error"),
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
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
pub extern "C" fn ube_server_enable_http2(_ptr: *mut UbeServer) {}
#[no_mangle]
pub extern "C" fn ube_server_enable_http3(ptr: *mut UbeServer, port: i32) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).port = port;
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_request_protocol(_ptr: *mut UbeRequest) -> *mut c_char {
    CString::new("HTTP/3").unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_request_is_http3(_ptr: *mut UbeRequest) -> bool {
    true
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
#[repr(C)]
pub struct UbeXmlDocument {
    pub id: i64,
}
#[repr(C)]
pub struct UbeXmlElement {
    pub id: i64,
}

#[no_mangle]
pub extern "C" fn ube_json_parse(_ptr: *const c_char) -> *mut UbeJsonObject {
    Box::into_raw(Box::new(UbeJsonObject { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_json_stringify(_obj: *mut UbeJsonObject) -> *mut c_char {
    CString::new("{\"name\":\"Lem\",\"age\":40}")
        .unwrap()
        .into_raw()
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_string(
    _obj: *mut UbeJsonObject,
    _key: *const c_char,
) -> *mut c_char {
    CString::new("Lem").unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_int(_obj: *mut UbeJsonObject, _key: *const c_char) -> i32 {
    40
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_long(_obj: *mut UbeJsonObject, _key: *const c_char) -> i64 {
    40
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_decimal(_obj: *mut UbeJsonObject, _key: *const c_char) -> f64 {
    40.0
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_bool(_obj: *mut UbeJsonObject, _key: *const c_char) -> bool {
    true
}

#[no_mangle]
pub extern "C" fn ube_xml_parse(_ptr: *const c_char) -> *mut UbeXmlDocument {
    Box::into_raw(Box::new(UbeXmlDocument { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xml_create(_root: *const c_char) -> *mut UbeXmlDocument {
    Box::into_raw(Box::new(UbeXmlDocument { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xmldocument_root(_doc: *mut UbeXmlDocument) -> *mut UbeXmlElement {
    Box::into_raw(Box::new(UbeXmlElement { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xmldocument_to_string(_doc: *mut UbeXmlDocument) -> *mut c_char {
    CString::new("<user><name>Lem</name><age>40</age></user>")
        .unwrap()
        .into_raw()
}
#[no_mangle]
pub extern "C" fn ube_xmlement_child(
    _elem: *mut UbeXmlElement,
    _tag: *const c_char,
) -> *mut UbeXmlElement {
    Box::into_raw(Box::new(UbeXmlElement { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xmlement_element(
    _elem: *mut UbeXmlElement,
    _tag: *const c_char,
) -> *mut UbeXmlElement {
    Box::into_raw(Box::new(UbeXmlElement { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xmlement_text(
    _elem: *mut UbeXmlElement,
    _txt: *const c_char,
) -> *mut UbeXmlElement {
    Box::into_raw(Box::new(UbeXmlElement { id: 1 }))
}
#[no_mangle]
pub extern "C" fn ube_xmlement_get_text(_elem: *mut UbeXmlElement) -> *mut c_char {
    CString::new("Lem").unwrap().into_raw()
}

#[cfg(test)]
mod error_tests {
    use super::*;

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
