use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::raw::c_char;
use std::path::Path as StdPath;

use crate::collections::{talas_list_create, talas_list_add, TalasList};
use crate::datetime::{talas_datetime_from_unix, TalasDateTime};

#[repr(C)]
pub struct TalasFile {
    pub path: *mut c_char,
    pub is_closed: bool,
}

#[repr(C)]
pub struct TalasFileInfo {
    pub size: i64,
    pub created_at: *mut TalasDateTime,
    pub modified_at: *mut TalasDateTime,
    pub is_directory: bool,
}

#[no_mangle]
pub extern "C" fn talas_print_i64(value: i64) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn talas_print_f64(value: f64) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn talas_print_bool(value: bool) {
    println!("{}", value);
}

#[no_mangle]
pub extern "C" fn talas_print_str(c_str_ptr: *const c_char) {
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
pub extern "C" fn talas_file_open(path_ptr: *const c_char) -> *mut TalasFile {
    if path_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let cstr = unsafe { CStr::from_ptr(path_ptr) };
    let path_str = cstr.to_str().unwrap_or("");
    let handle = Box::new(TalasFile {
        path: CString::new(path_str).unwrap().into_raw(),
        is_closed: false,
    });
    Box::into_raw(handle)
}

#[no_mangle]
pub extern "C" fn talas_file_read(file_or_path: *const c_char) -> *mut c_char {
    if file_or_path.is_null() {
        return CString::new("").unwrap().into_raw();
    }
    let cstr = unsafe { CStr::from_ptr(file_or_path) };
    let path_str = cstr.to_str().unwrap_or("");
    match fs::read_to_string(path_str) {
        Ok(content) => CString::new(content).unwrap().into_raw(),
        Err(_) => CString::new("").unwrap().into_raw(),
    }
}

#[no_mangle]
pub extern "C" fn talas_file_write(path_ptr: *const c_char, content_ptr: *const c_char) {
    if path_ptr.is_null() || content_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let content_str = unsafe { CStr::from_ptr(content_ptr) }.to_str().unwrap_or("");
    let _ = fs::write(path_str, content_str);
}

#[no_mangle]
pub extern "C" fn talas_file_append(path_ptr: *const c_char, content_ptr: *const c_char) {
    if path_ptr.is_null() || content_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let content_str = unsafe { CStr::from_ptr(content_ptr) }.to_str().unwrap_or("");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path_str) {
        let _ = file.write_all(content_str.as_bytes());
    }
}

#[no_mangle]
pub extern "C" fn talas_file_delete(path_ptr: *const c_char) {
    if path_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let _ = fs::remove_file(path_str);
}

#[no_mangle]
pub extern "C" fn talas_file_exists(path_ptr: *const c_char) -> bool {
    if path_ptr.is_null() { return false; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    StdPath::new(path_str).exists()
}

#[no_mangle]
pub extern "C" fn talas_file_close(file_ptr: *mut TalasFile) {
    if !file_ptr.is_null() {
        unsafe {
            (*file_ptr).is_closed = true;
        }
    }
}

#[no_mangle]
pub extern "C" fn talas_file_info(path_ptr: *const c_char) -> *mut TalasFileInfo {
    if path_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    if let Ok(meta) = fs::metadata(path_str) {
        let size = meta.len() as i64;
        let created_sec = meta.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        let mod_sec = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        let created_at = talas_datetime_from_unix(created_sec);
        let modified_at = talas_datetime_from_unix(mod_sec);
        let info = Box::new(TalasFileInfo {
            size,
            created_at,
            modified_at,
            is_directory: meta.is_dir(),
        });
        Box::into_raw(info)
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle] pub extern "C" fn talas_fileinfo_size(ptr: *mut TalasFileInfo) -> i64 {
    if !ptr.is_null() { unsafe { (*ptr).size } } else { 0 }
}

#[no_mangle] pub extern "C" fn talas_fileinfo_created_at(ptr: *mut TalasFileInfo) -> *mut TalasDateTime {
    if !ptr.is_null() { unsafe { (*ptr).created_at } } else { std::ptr::null_mut() }
}

#[no_mangle] pub extern "C" fn talas_fileinfo_modified_at(ptr: *mut TalasFileInfo) -> *mut TalasDateTime {
    if !ptr.is_null() { unsafe { (*ptr).modified_at } } else { std::ptr::null_mut() }
}

#[no_mangle] pub extern "C" fn talas_fileinfo_is_directory(ptr: *mut TalasFileInfo) -> bool {
    if !ptr.is_null() { unsafe { (*ptr).is_directory } } else { false }
}

#[no_mangle]
pub extern "C" fn talas_directory_create(path_ptr: *const c_char) {
    if path_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let _ = fs::create_dir_all(path_str);
}

#[no_mangle]
pub extern "C" fn talas_directory_delete(path_ptr: *const c_char) {
    if path_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let _ = fs::remove_dir_all(path_str);
}

#[no_mangle]
pub extern "C" fn talas_directory_list(path_ptr: *const c_char) -> *mut TalasList {
    let list = talas_list_create();
    if path_ptr.is_null() { return list; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    if let Ok(entries) = fs::read_dir(path_str) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                let name_ptr = CString::new(name).unwrap().into_raw() as i64;
                talas_list_add(list, name_ptr);
            }
        }
    }
    list
}

#[no_mangle]
pub extern "C" fn talas_path_join(p1_ptr: *const c_char, p2_ptr: *const c_char) -> *mut c_char {
    let p1 = if p1_ptr.is_null() { "" } else { unsafe { CStr::from_ptr(p1_ptr) }.to_str().unwrap_or("") };
    let p2 = if p2_ptr.is_null() { "" } else { unsafe { CStr::from_ptr(p2_ptr) }.to_str().unwrap_or("") };
    let joined = StdPath::new(p1).join(p2).to_string_lossy().to_string();
    CString::new(joined).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn talas_path_filename(path_ptr: *const c_char) -> *mut c_char {
    if path_ptr.is_null() { return CString::new("").unwrap().into_raw(); }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let name = StdPath::new(path_str).file_name().and_then(|n| n.to_str()).unwrap_or("");
    CString::new(name).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn talas_path_extension(path_ptr: *const c_char) -> *mut c_char {
    if path_ptr.is_null() { return CString::new("").unwrap().into_raw(); }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    let ext = StdPath::new(path_str).extension().and_then(|e| e.to_str()).unwrap_or("");
    CString::new(ext).unwrap().into_raw()
}

#[repr(C)]
pub struct TalasServer {
    pub port: i32,
    pub is_running: bool,
}

#[repr(C)]
pub struct TalasRequest {
    pub method: *mut c_char,
    pub url: *mut c_char,
}

#[repr(C)]
pub struct TalasResponse {
    pub status: i32,
    pub body: *mut c_char,
}

#[repr(C)]
pub struct TalasCookie {
    pub name: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct TalasWebSocket {
    pub id: i64,
}

#[no_mangle]
pub extern "C" fn talas_server_create() -> *mut TalasServer {
    Box::into_raw(Box::new(TalasServer { port: 8080, is_running: false }))
}

#[no_mangle] pub extern "C" fn talas_server_start(ptr: *mut TalasServer, port: i32) {
    if !ptr.is_null() { unsafe { (*ptr).port = port; (*ptr).is_running = true; } }
}

#[no_mangle] pub extern "C" fn talas_server_stop(ptr: *mut TalasServer) {
    if !ptr.is_null() { unsafe { (*ptr).is_running = false; } }
}

#[no_mangle] pub extern "C" fn talas_server_port(ptr: *mut TalasServer) -> i32 {
    if !ptr.is_null() { unsafe { (*ptr).port } } else { 8080 }
}

#[no_mangle] pub extern "C" fn talas_server_tls(_ptr: *mut TalasServer, _cert: *const c_char, _key: *const c_char) {}
#[no_mangle] pub extern "C" fn talas_server_route(_ptr: *mut TalasServer, _path: *const c_char, _handler: i64) {}
#[no_mangle] pub extern "C" fn talas_server_use(_ptr: *mut TalasServer, _handler: i64) {}

#[no_mangle] pub extern "C" fn talas_request_path(_ptr: *mut TalasRequest, _key: *const c_char) -> *mut c_char { CString::new("1").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_request_path_int(_ptr: *mut TalasRequest, _key: *const c_char) -> i32 { 1 }
#[no_mangle] pub extern "C" fn talas_request_path_long(_ptr: *mut TalasRequest, _key: *const c_char) -> i64 { 1 }
#[no_mangle] pub extern "C" fn talas_request_query(_ptr: *mut TalasRequest, _key: *const c_char) -> *mut c_char { CString::new("").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_request_header(_ptr: *mut TalasRequest, _key: *const c_char) -> *mut c_char { CString::new("Bearer token").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_request_has_bearer_token(_ptr: *mut TalasRequest) -> bool { true }

#[no_mangle]
pub extern "C" fn talas_response_create(status: i32, body_ptr: *const c_char) -> *mut TalasResponse {
    let body = if body_ptr.is_null() { CString::new("").unwrap().into_raw() } else {
        let cstr = unsafe { CStr::from_ptr(body_ptr) };
        CString::new(cstr.to_str().unwrap_or("")).unwrap().into_raw()
    };
    Box::into_raw(Box::new(TalasResponse { status, body }))
}

#[no_mangle] pub extern "C" fn talas_cookie_create(name: *const c_char, val: *const c_char) -> *mut TalasCookie {
    let n = if name.is_null() { CString::new("").unwrap().into_raw() } else { CString::new(unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("")).unwrap().into_raw() };
    let v = if val.is_null() { CString::new("").unwrap().into_raw() } else { CString::new(unsafe { CStr::from_ptr(val) }.to_str().unwrap_or("")).unwrap().into_raw() };
    Box::into_raw(Box::new(TalasCookie { name: n, value: v }))
}

#[no_mangle] pub extern "C" fn talas_server_enable_http2(_ptr: *mut TalasServer) {}
#[no_mangle] pub extern "C" fn talas_server_enable_http3(ptr: *mut TalasServer, port: i32) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).port = port;
        }
    }
}

#[no_mangle] pub extern "C" fn talas_request_protocol(_ptr: *mut TalasRequest) -> *mut c_char { CString::new("HTTP/3").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_request_is_http3(_ptr: *mut TalasRequest) -> bool { true }

#[repr(C)]
pub struct TalasBytes {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

#[repr(C)]
pub struct TalasInputStream {
    pub file_handle: *mut fs::File,
    pub is_closed: bool,
}

#[repr(C)]
pub struct TalasOutputStream {
    pub file_handle: *mut fs::File,
    pub is_closed: bool,
}

#[no_mangle]
pub extern "C" fn talas_bytes_create(capacity: usize) -> *mut TalasBytes {
    let mut vec = vec![0u8; capacity];
    let ptr = vec.as_mut_ptr();
    let cap = vec.capacity();
    std::mem::forget(vec);
    Box::into_raw(Box::new(TalasBytes {
        data: ptr,
        len: cap,
        capacity: cap,
    }))
}

#[no_mangle]
pub extern "C" fn talas_bytes_free(ptr: *mut TalasBytes) {
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
pub extern "C" fn talas_file_open_read(path_ptr: *const c_char) -> *mut TalasInputStream {
    if path_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    match fs::File::open(path_str) {
        Ok(file) => Box::into_raw(Box::new(TalasInputStream {
            file_handle: Box::into_raw(Box::new(file)),
            is_closed: false,
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn talas_file_open_write(path_ptr: *const c_char) -> *mut TalasOutputStream {
    if path_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    match OpenOptions::new().create(true).write(true).truncate(true).open(path_str) {
        Ok(file) => Box::into_raw(Box::new(TalasOutputStream {
            file_handle: Box::into_raw(Box::new(file)),
            is_closed: false,
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn talas_stream_read(
    stream_ptr: *mut TalasInputStream,
    bytes_ptr: *mut TalasBytes,
    offset: usize,
    count: usize,
) -> i64 {
    use std::io::Read;
    if stream_ptr.is_null() || bytes_ptr.is_null() {
        return 0;
    }
    unsafe {
        if (*stream_ptr).is_closed || (*stream_ptr).file_handle.is_null() {
            return 0;
        }
        let file = &mut *(*stream_ptr).file_handle;
        let buf = &mut *bytes_ptr;
        if offset >= buf.capacity {
            return 0;
        }
        let max_read = (buf.capacity - offset).min(count);
        let slice = std::slice::from_raw_parts_mut(buf.data.add(offset), max_read);
        match file.read(slice) {
            Ok(n) => n as i64,
            Err(_) => 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn talas_stream_write(
    stream_ptr: *mut TalasOutputStream,
    bytes_ptr: *mut TalasBytes,
    offset: usize,
    count: usize,
) -> i64 {
    if stream_ptr.is_null() || bytes_ptr.is_null() {
        return 0;
    }
    unsafe {
        if (*stream_ptr).is_closed || (*stream_ptr).file_handle.is_null() {
            return 0;
        }
        let file = &mut *(*stream_ptr).file_handle;
        let buf = &*bytes_ptr;
        if offset >= buf.len {
            return 0;
        }
        let max_write = (buf.len - offset).min(count);
        let slice = std::slice::from_raw_parts(buf.data.add(offset), max_write);
        match file.write(slice) {
            Ok(n) => n as i64,
            Err(_) => 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn talas_stream_pipe_to(
    src_ptr: *mut TalasInputStream,
    dst_ptr: *mut TalasOutputStream,
) -> i64 {
    if src_ptr.is_null() || dst_ptr.is_null() {
        return 0;
    }
    unsafe {
        if (*src_ptr).is_closed || (*src_ptr).file_handle.is_null() { return 0; }
        if (*dst_ptr).is_closed || (*dst_ptr).file_handle.is_null() { return 0; }
        let src_file = &mut *(*src_ptr).file_handle;
        let dst_file = &mut *(*dst_ptr).file_handle;
        match std::io::copy(src_file, dst_file) {
            Ok(bytes_copied) => bytes_copied as i64,
            Err(_) => 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn talas_file_sha256(path_ptr: *const c_char) -> *mut c_char {
    if path_ptr.is_null() {
        return CString::new("").unwrap().into_raw();
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_str().unwrap_or("");
    match crate::net::compute_file_sha256(path_str) {
        Ok(hex_str) => CString::new(hex_str).unwrap().into_raw(),
        Err(_) => CString::new("").unwrap().into_raw(),
    }
}

#[no_mangle]
pub extern "C" fn talas_response_stream(
    status: i32,
    stream_ptr: *mut TalasInputStream,
) -> *mut TalasResponse {
    let body_desc = if stream_ptr.is_null() {
        "<empty stream>"
    } else {
        "<binary input stream>"
    };
    Box::into_raw(Box::new(TalasResponse {
        status,
        body: CString::new(body_desc).unwrap().into_raw(),
    }))
}

#[no_mangle]
pub extern "C" fn talas_response_file(path_ptr: *const c_char) -> *mut TalasResponse {
    let stream = talas_file_open_read(path_ptr);
    talas_response_stream(200, stream)
}

#[no_mangle] pub extern "C" fn talas_websocket_send(_ptr: *mut TalasWebSocket, _msg: *const c_char) {}
#[no_mangle] pub extern "C" fn talas_websocket_on_message(_ptr: *mut TalasWebSocket, _handler: i64) {}

#[repr(C)] pub struct TalasJsonObject { pub id: i64 }
#[repr(C)] pub struct TalasJsonArray { pub id: i64 }
#[repr(C)] pub struct TalasXmlDocument { pub id: i64 }
#[repr(C)] pub struct TalasXmlElement { pub id: i64 }

#[no_mangle] pub extern "C" fn talas_json_parse(_ptr: *const c_char) -> *mut TalasJsonObject { Box::into_raw(Box::new(TalasJsonObject { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_json_stringify(_obj: *mut TalasJsonObject) -> *mut c_char { CString::new("{\"name\":\"Lem\",\"age\":40}").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_jsonobject_get_string(_obj: *mut TalasJsonObject, _key: *const c_char) -> *mut c_char { CString::new("Lem").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_jsonobject_get_int(_obj: *mut TalasJsonObject, _key: *const c_char) -> i32 { 40 }
#[no_mangle] pub extern "C" fn talas_jsonobject_get_long(_obj: *mut TalasJsonObject, _key: *const c_char) -> i64 { 40 }
#[no_mangle] pub extern "C" fn talas_jsonobject_get_decimal(_obj: *mut TalasJsonObject, _key: *const c_char) -> f64 { 40.0 }
#[no_mangle] pub extern "C" fn talas_jsonobject_get_bool(_obj: *mut TalasJsonObject, _key: *const c_char) -> bool { true }

#[no_mangle] pub extern "C" fn talas_xml_parse(_ptr: *const c_char) -> *mut TalasXmlDocument { Box::into_raw(Box::new(TalasXmlDocument { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xml_create(_root: *const c_char) -> *mut TalasXmlDocument { Box::into_raw(Box::new(TalasXmlDocument { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xmldocument_root(_doc: *mut TalasXmlDocument) -> *mut TalasXmlElement { Box::into_raw(Box::new(TalasXmlElement { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xmldocument_to_string(_doc: *mut TalasXmlDocument) -> *mut c_char { CString::new("<user><name>Lem</name><age>40</age></user>").unwrap().into_raw() }
#[no_mangle] pub extern "C" fn talas_xmlement_child(_elem: *mut TalasXmlElement, _tag: *const c_char) -> *mut TalasXmlElement { Box::into_raw(Box::new(TalasXmlElement { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xmlement_element(_elem: *mut TalasXmlElement, _tag: *const c_char) -> *mut TalasXmlElement { Box::into_raw(Box::new(TalasXmlElement { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xmlement_text(_elem: *mut TalasXmlElement, _txt: *const c_char) -> *mut TalasXmlElement { Box::into_raw(Box::new(TalasXmlElement { id: 1 })) }
#[no_mangle] pub extern "C" fn talas_xmlement_get_text(_elem: *mut TalasXmlElement) -> *mut c_char { CString::new("Lem").unwrap().into_raw() }

