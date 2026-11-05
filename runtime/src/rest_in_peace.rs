//! HTTP test requests with bounded reads and the ordinary Ube exception ABI.
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderName, HeaderValue},
    Method,
};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    io::Read,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use url::Url;
type Handle = *mut c_void;
type Result<T> = std::result::Result<T, Failure>;
enum Failure {
    Rest(String),
    Assertion(String),
}
impl From<&str> for Failure {
    fn from(value: &str) -> Self {
        Self::Rest(value.into())
    }
}
const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone)]
struct Request {
    base: Option<Url>,
    headers: HeaderMap,
    query: Vec<(String, String)>,
    body: String,
    timeout: Duration,
    max_bytes: usize,
    ca: Option<reqwest::Certificate>,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            base: None,
            headers: HeaderMap::new(),
            query: vec![],
            body: String::new(),
            timeout: Duration::from_secs(10),
            max_bytes: 1024 * 1024,
            ca: None,
        }
    }
}
struct Response {
    status: i32,
    headers: HeaderMap,
    body: String,
    elapsed: i64,
    protocol: &'static str,
}
#[derive(Default)]
struct Registry {
    next: usize,
    requests: HashMap<usize, Request>,
    responses: HashMap<usize, Arc<Response>>,
}
impl Registry {
    fn id(&mut self) -> Result<Handle> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("REST handle limit reached")?;
        Ok(self.next as Handle)
    }
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
fn finish<T>(result: Result<T>, fallback: T) -> T {
    match result {
        Ok(value) => value,
        Err(Failure::Rest(message)) => {
            crate::exceptions::set_pending(crate::exceptions::named("RestInPeaceError", message));
            fallback
        }
        Err(Failure::Assertion(message)) => {
            let message = CString::new(message.replace('\0', "\\0")).unwrap();
            crate::testing::ube_testicle_check(false, message.as_ptr());
            fallback
        }
    }
}
fn text(value: *const c_char) -> Result<String> {
    if value.is_null() {
        return Err("REST string arguments must not be null".into());
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "REST strings must be UTF-8".into())
}
fn string(value: Result<String>) -> *mut c_char {
    finish(
        value.and_then(|value| {
            if value.contains('\0') {
                return Err("REST text contains a null byte".into());
            }
            let pointer = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
            if pointer.is_null() {
                return Err("REST string allocation failed".into());
            }
            unsafe {
                std::ptr::copy_nonoverlapping(value.as_ptr(), pointer as *mut u8, value.len());
                *pointer.add(value.len()) = 0;
            }
            Ok(pointer)
        }),
        std::ptr::null_mut(),
    )
}
fn url(value: &str) -> Result<Url> {
    if value.len() > 16 * 1024 {
        return Err("REST URL exceeds 16 KiB".into());
    }
    let parsed = Url::parse(value)
        .map_err(|_| Failure::from("REST requires a valid absolute HTTP(S) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("REST URLs require HTTP(S), a host, and no credentials or fragment".into());
    }
    Ok(parsed)
}
fn modify(handle: Handle, apply: impl FnOnce(&mut Request) -> Result<()>) -> Result<Handle> {
    let mut registry = registry().lock().unwrap();
    apply(
        registry
            .requests
            .get_mut(&(handle as usize))
            .ok_or("REST request is closed or invalid")?,
    )?;
    Ok(handle)
}
fn response(handle: Handle) -> Result<Arc<Response>> {
    registry()
        .lock()
        .unwrap()
        .responses
        .get(&(handle as usize))
        .cloned()
        .ok_or_else(|| "REST response is closed or invalid".into())
}
fn header(name: &str, value: &str) -> Result<(HeaderName, HeaderValue)> {
    if name.len() + value.len() > 16 * 1024 {
        return Err("REST header exceeds 16 KiB".into());
    }
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| Failure::from("Invalid REST header name"))?;
    // Framing belongs to the HTTP client, rather than caller-supplied contradictory headers.
    if name == "content-length" || name == "transfer-encoding" {
        return Err("REST client manages Content-Length and Transfer-Encoding".into());
    }
    let value =
        HeaderValue::from_str(value).map_err(|_| Failure::from("Invalid REST header value"))?;
    Ok((name, value))
}
fn set_header(request: &mut Request, name: &str, value: &str) -> Result<()> {
    let (name, value) = header(name, value)?;
    if request.headers.len() >= 128 && !request.headers.contains_key(&name) {
        return Err("REST request exceeds 128 headers".into());
    }
    request.headers.insert(name, value);
    Ok(())
}
#[no_mangle]
pub extern "C" fn ube_rest_given() -> Handle {
    finish(
        (|| {
            let mut r = registry().lock().unwrap();
            let id = r.id()?;
            r.requests.insert(id as usize, Request::default());
            Ok(id)
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_at(base: *const c_char) -> Handle {
    finish(
        (|| {
            let base = url(&text(base)?)?;
            let mut r = registry().lock().unwrap();
            let id = r.id()?;
            r.requests.insert(
                id as usize,
                Request {
                    base: Some(base),
                    ..Request::default()
                },
            );
            Ok(id)
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_base_url(handle: Handle, base: *const c_char) -> Handle {
    finish(
        text(base).and_then(|value| url(&value)).and_then(|base| {
            modify(handle, |r| {
                r.base = Some(base);
                Ok(())
            })
        }),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_header(
    handle: Handle,
    name: *const c_char,
    value: *const c_char,
) -> Handle {
    finish(
        (|| {
            let name = text(name)?;
            let value = text(value)?;
            modify(handle, |r| set_header(r, &name, &value))
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_query(
    handle: Handle,
    name: *const c_char,
    value: *const c_char,
) -> Handle {
    finish(
        (|| {
            let name = text(name)?;
            let value = text(value)?;
            if name.len() + value.len() > 16 * 1024 {
                return Err("REST query pair exceeds 16 KiB".into());
            }
            modify(handle, |r| {
                if r.query.len() >= 128 {
                    return Err("REST request exceeds 128 query pairs".into());
                }
                r.query.push((name, value));
                Ok(())
            })
        })(),
        std::ptr::null_mut(),
    )
}
macro_rules! header_method {
    ($name:ident, $header:expr, $prefix:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: Handle, value: *const c_char) -> Handle {
            finish(
                text(value).and_then(|value| {
                    modify(handle, |r| {
                        set_header(r, $header, &format!("{}{}", $prefix, value))
                    })
                }),
                std::ptr::null_mut(),
            )
        }
    };
}
header_method!(ube_rest_content_type, "Content-Type", "");
header_method!(ube_rest_accept, "Accept", "");
header_method!(ube_rest_bearer, "Authorization", "Bearer ");
fn body(handle: Handle, value: *const c_char, json: bool) -> Result<Handle> {
    let value = text(value)?;
    if value.len() > MAX_BYTES {
        return Err("REST request body exceeds 16 MiB".into());
    }
    if json {
        serde_json::from_str::<serde_json::Value>(&value)
            .map_err(|_| Failure::from("Invalid REST JSON request body"))?;
    }
    modify(handle, |r| {
        if json {
            set_header(r, "Content-Type", "application/json")?;
        }
        r.body = value;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_rest_body(handle: Handle, value: *const c_char) -> Handle {
    finish(body(handle, value, false), std::ptr::null_mut())
}
#[no_mangle]
pub extern "C" fn ube_rest_json(handle: Handle, value: *const c_char) -> Handle {
    finish(body(handle, value, true), std::ptr::null_mut())
}
#[no_mangle]
pub extern "C" fn ube_rest_timeout(handle: Handle, millis: i32) -> Handle {
    finish(
        modify(handle, |r| {
            if !(1..=300_000).contains(&millis) {
                return Err("REST timeout must be 1..300000 milliseconds".into());
            }
            r.timeout = Duration::from_millis(millis as u64);
            Ok(())
        }),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_max_bytes(handle: Handle, bytes: i32) -> Handle {
    finish(
        modify(handle, |r| {
            if bytes < 1 || bytes as usize > MAX_BYTES {
                return Err("REST response limit must be 1..16777216 bytes".into());
            }
            r.max_bytes = bytes as usize;
            Ok(())
        }),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_ca_file(handle: Handle, path: *const c_char) -> Handle {
    finish(
        (|| {
            let mut pem = vec![];
            std::fs::File::open(text(path)?)
                .map_err(|_| Failure::from("Cannot open REST CA certificate"))?
                .take(65537)
                .read_to_end(&mut pem)
                .map_err(|_| Failure::from("Cannot read REST CA certificate"))?;
            if pem.len() > 65536 {
                return Err("REST CA certificate exceeds 64 KiB".into());
            }
            let ca = reqwest::Certificate::from_pem(&pem)
                .map_err(|_| Failure::from("Invalid REST CA certificate"))?;
            modify(handle, |r| {
                r.ca = Some(ca);
                Ok(())
            })
        })(),
        std::ptr::null_mut(),
    )
}
fn transport(error: reqwest::Error) -> Failure {
    Failure::from(if error.is_timeout() {
        "REST request timed out"
    } else if error.is_connect() {
        "REST connection failed"
    } else {
        "REST HTTP transport failed"
    })
}
fn perform(request: Request, method: Method, target: Url) -> Result<Response> {
    let started = Instant::now();
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(request.timeout)
        .connect_timeout(request.timeout.min(Duration::from_secs(5)));
    if let Some(ca) = request.ca {
        builder = builder.add_root_certificate(ca);
    }
    let client = builder
        .build()
        .map_err(|_| Failure::from("Cannot initialize REST HTTP client"))?;
    let response = client
        .request(method, target)
        .timeout(request.timeout)
        .headers(request.headers)
        .body(request.body)
        .send()
        .map_err(transport)?;
    let status = i32::from(response.status().as_u16());
    let headers = response.headers().clone();
    let protocol = match response.version() {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2",
        _ => "unknown",
    };
    let mut bytes = Vec::new();
    response
        .take((request.max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::TimedOut
                || error
                    .get_ref()
                    .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
                    .is_some_and(reqwest::Error::is_timeout)
            {
                Failure::from("REST response body timed out")
            } else {
                Failure::from("Cannot read REST response body")
            }
        })?;
    if bytes.len() > request.max_bytes {
        return Err("REST response body exceeds configured byte limit".into());
    }
    let body = String::from_utf8(bytes)
        .map_err(|_| Failure::from("REST response body is not UTF-8 text"))?;
    if body.contains('\0') {
        return Err("REST response body contains a null byte".into());
    }
    Ok(Response {
        status,
        headers,
        body,
        elapsed: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        protocol,
    })
}
fn send(handle: Handle, method: &str, path: &str) -> Result<Handle> {
    let request = registry()
        .lock()
        .unwrap()
        .requests
        .get(&(handle as usize))
        .cloned()
        .ok_or("REST request is closed or invalid")?;
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| Failure::from("Invalid REST HTTP method"))?;
    if method == Method::CONNECT {
        return Err("REST CONNECT tunnels are unsupported".into());
    }
    if path.len() > 16 * 1024 {
        return Err("REST request path exceeds 16 KiB".into());
    }
    let mut target = if let Some(base) = &request.base {
        base.join(path)
            .map_err(|_| Failure::from("Invalid REST relative URL"))?
    } else {
        url(path)?
    };
    url(target.as_str())?;
    if !request.query.is_empty() {
        target
            .query_pairs_mut()
            .extend_pairs(request.query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    if target.as_str().len() > 16 * 1024 {
        return Err("REST URL with query exceeds 16 KiB".into());
    }
    // Keep reqwest's blocking runtime creation/destruction outside Tokio workers.
    let result = std::thread::spawn(move || perform(request, method, target))
        .join()
        .map_err(|_| Failure::from("REST HTTP worker failed"))??;
    let mut registry = registry().lock().unwrap();
    let id = registry.id()?;
    registry.responses.insert(id as usize, Arc::new(result));
    Ok(id)
}
#[no_mangle]
pub extern "C" fn ube_rest_send(
    handle: Handle,
    method: *const c_char,
    path: *const c_char,
) -> Handle {
    finish(
        (|| send(handle, &text(method)?, &text(path)?))(),
        std::ptr::null_mut(),
    )
}
macro_rules! verb {
    ($function:ident, $method:expr) => {
        #[no_mangle]
        pub extern "C" fn $function(handle: Handle, path: *const c_char) -> Handle {
            finish(
                text(path).and_then(|path| send(handle, $method, &path)),
                std::ptr::null_mut(),
            )
        }
    };
}
verb!(ube_rest_get, "GET");
verb!(ube_rest_post, "POST");
verb!(ube_rest_put, "PUT");
verb!(ube_rest_patch, "PATCH");
verb!(ube_rest_delete, "DELETE");
verb!(ube_rest_head, "HEAD");
verb!(ube_rest_options, "OPTIONS");
#[no_mangle]
pub extern "C" fn ube_rest_status(handle: Handle) -> i32 {
    finish(response(handle).map(|r| r.status), 0)
}
#[no_mangle]
pub extern "C" fn ube_rest_elapsed_ms(handle: Handle) -> i64 {
    finish(response(handle).map(|r| r.elapsed), 0)
}
#[no_mangle]
pub extern "C" fn ube_rest_protocol(handle: Handle) -> *mut c_char {
    string(response(handle).map(|r| r.protocol.into()))
}
#[no_mangle]
pub extern "C" fn ube_rest_response_body(handle: Handle) -> *mut c_char {
    string(response(handle).map(|r| r.body.clone()))
}
fn response_header(handle: Handle, name: *const c_char) -> Result<Option<String>> {
    let name = HeaderName::from_bytes(text(name)?.as_bytes())
        .map_err(|_| Failure::from("Invalid REST response header name"))?;
    response(handle)?
        .headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| "REST header is not UTF-8 text".into())
        })
        .transpose()
}
#[no_mangle]
pub extern "C" fn ube_rest_response_header(handle: Handle, name: *const c_char) -> *mut c_char {
    match finish(response_header(handle, name), None) {
        Some(value) => string(Ok(value)),
        None => std::ptr::null_mut(),
    }
}
#[no_mangle]
pub extern "C" fn ube_rest_has_header(handle: Handle, name: *const c_char) -> bool {
    finish(
        (|| {
            let name = HeaderName::from_bytes(text(name)?.as_bytes())
                .map_err(|_| Failure::from("Invalid REST response header name"))?;
            Ok(response(handle)?.headers.contains_key(name))
        })(),
        false,
    )
}
fn json(handle: Handle, pointer: *const c_char) -> Result<serde_json::Value> {
    let pointer = text(pointer)?;
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err("REST JSON paths use JSON Pointer syntax, such as /user/name".into());
    }
    let mut characters = pointer.chars();
    while let Some(character) = characters.next() {
        if character == '~' && !matches!(characters.next(), Some('0' | '1')) {
            return Err("REST JSON Pointer uses only ~0 and ~1 escapes".into());
        }
    }
    let json: serde_json::Value = serde_json::from_str(&response(handle)?.body)
        .map_err(|_| Failure::from("REST response is not valid JSON"))?;
    json.pointer(&pointer)
        .cloned()
        .ok_or_else(|| Failure::from("REST JSON Pointer does not exist"))
}
#[no_mangle]
pub extern "C" fn ube_rest_json_string(handle: Handle, pointer: *const c_char) -> *mut c_char {
    string(json(handle, pointer).and_then(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "REST JSON value is not a string".into())
    }))
}
macro_rules! json_number {
    ($function:ident, $ty:ty, $getter:ident, $zero:expr) => {
        #[no_mangle]
        pub extern "C" fn $function(handle: Handle, pointer: *const c_char) -> $ty {
            finish(
                json(handle, pointer).and_then(|value| {
                    value
                        .$getter()
                        .ok_or_else(|| "REST JSON value has the wrong type or range".into())
                }),
                $zero,
            )
        }
    };
}
json_number!(ube_rest_json_long, i64, as_i64, 0);
json_number!(ube_rest_json_decimal, f64, as_f64, 0.0);
json_number!(ube_rest_json_bool, bool, as_bool, false);
#[no_mangle]
pub extern "C" fn ube_rest_json_int(handle: Handle, pointer: *const c_char) -> i32 {
    finish(
        json(handle, pointer).and_then(|value| {
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| "REST JSON value is not a 32-bit integer".into())
        }),
        0,
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_json_is_null(handle: Handle, pointer: *const c_char) -> bool {
    finish(json(handle, pointer).map(|value| value.is_null()), false)
}
fn expect(handle: Handle, valid: bool, message: String) -> Result<Handle> {
    if valid {
        Ok(handle)
    } else {
        Err(Failure::Assertion(message))
    }
}
#[no_mangle]
pub extern "C" fn ube_rest_expect_status(handle: Handle, expected: i32) -> Handle {
    finish(
        response(handle).and_then(|r| {
            expect(
                handle,
                r.status == expected,
                format!("REST status: expected {expected}, received {}", r.status),
            )
        }),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_expect_body(handle: Handle, expected: *const c_char) -> Handle {
    finish(
        (|| {
            let expected = text(expected)?;
            expect(
                handle,
                response(handle)?.body == expected,
                "REST response body did not match".into(),
            )
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_expect_header(
    handle: Handle,
    name: *const c_char,
    expected: *const c_char,
) -> Handle {
    finish(
        (|| {
            let expected = text(expected)?;
            expect(
                handle,
                response_header(handle, name)?.as_deref() == Some(expected.as_str()),
                "REST response header did not match".into(),
            )
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_expect_json_string(
    handle: Handle,
    pointer: *const c_char,
    expected: *const c_char,
) -> Handle {
    finish(
        (|| {
            let expected = text(expected)?;
            let value = json(handle, pointer)?;
            expect(
                handle,
                value.as_str() == Some(expected.as_str()),
                "REST JSON string did not match".into(),
            )
        })(),
        std::ptr::null_mut(),
    )
}
#[no_mangle]
pub extern "C" fn ube_rest_request_close(handle: Handle) {
    finish(
        if registry()
            .lock()
            .unwrap()
            .requests
            .remove(&(handle as usize))
            .is_some()
        {
            Ok(())
        } else {
            Err("REST request is closed or invalid".into())
        },
        (),
    );
}
#[no_mangle]
pub extern "C" fn ube_rest_response_close(handle: Handle) {
    finish(
        if registry()
            .lock()
            .unwrap()
            .responses
            .remove(&(handle as usize))
            .is_some()
        {
            Ok(())
        } else {
            Err("REST response is closed or invalid".into())
        },
        (),
    );
}
pub(crate) fn cleanup() {
    let mut r = registry().lock().unwrap();
    r.requests.clear();
    r.responses.clear();
}
