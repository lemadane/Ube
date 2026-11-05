//! Server-side sessions. Only opaque, OS-random identifiers travel in cookies.
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub(crate) const COOKIE_NAME: &str = "ube_session";
const LIFETIME: Duration = Duration::from_secs(30 * 60);
type Result<T> = std::result::Result<T, String>;
pub(crate) type Store = Arc<Mutex<SessionStore>>;
#[derive(Default)]
pub(crate) struct SessionStore {
    entries: HashMap<String, Entry>,
}
struct Entry {
    values: HashMap<String, String>,
    expires: Instant,
}
struct Active {
    store: Store,
    id: Option<String>,
    cookie: CookieChange,
}
#[derive(Default)]
enum CookieChange {
    #[default]
    None,
    Set,
    Delete,
}
#[derive(Default)]
struct Registry {
    next: usize,
    handles: HashMap<usize, Active>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
fn fail(message: String) {
    crate::exceptions::set_pending(crate::exceptions::generic(format!("Session: {message}")));
}
fn text(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err("keys and values must be non-null strings".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8 string".into())
}
fn string(value: &str) -> *mut c_char {
    let ptr = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if !ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), ptr as *mut u8, value.len());
            *ptr.add(value.len()) = 0;
        }
    }
    ptr
}
fn cookie_id(cookies: &str) -> Option<&str> {
    let mut ids = cookies
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .filter(|(name, _)| name.trim() == COOKIE_NAME)
        .map(|(_, value)| value.trim());
    let id = ids.next()?;
    if ids.next().is_some()
        || id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    Some(id)
}
pub(crate) fn begin(store: Store, cookies: &str) -> Result<usize> {
    let id = {
        let mut state = store.lock().unwrap();
        state
            .entries
            .retain(|_, entry| entry.expires > Instant::now());
        cookie_id(cookies)
            .filter(|id| state.entries.contains_key(*id))
            .map(str::to_owned)
    };
    let mut registry = registry().lock().unwrap();
    registry.next = registry
        .next
        .checked_add(1)
        .ok_or("session handle limit reached")?;
    let handle = registry.next;
    registry.handles.insert(
        handle,
        Active {
            store,
            id,
            cookie: CookieChange::None,
        },
    );
    Ok(handle)
}
pub(crate) fn finish(handle: usize, secure: bool) -> Option<String> {
    let active = registry().lock().unwrap().handles.remove(&handle)?;
    let value = match active.cookie {
        CookieChange::None => return None,
        CookieChange::Set => active.id?,
        CookieChange::Delete => String::new(),
    };
    let deleted = matches!(active.cookie, CookieChange::Delete);
    Some(format!(
        "{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax{}{}",
        if secure { "; Secure" } else { "" },
        if deleted { "; Max-Age=0" } else { "" }
    ))
}
fn run<T: Default>(
    handle: *mut c_void,
    action: impl FnOnce(&mut Active, &mut SessionStore) -> Result<T>,
) -> T {
    let mut registry = registry().lock().unwrap();
    let result = match registry.handles.get_mut(&(handle as usize)) {
        Some(active) => {
            let store = active.store.clone();
            let mut state = store.lock().unwrap();
            state
                .entries
                .retain(|_, entry| entry.expires > Instant::now());
            action(active, &mut state)
        }
        None => Err("handle is invalid or its request has finished".into()),
    };
    match result {
        Ok(value) => value,
        Err(message) => {
            fail(message);
            T::default()
        }
    }
}
fn new_id(state: &SessionStore) -> Result<String> {
    loop {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|_| "could not generate a secure session ID")?;
        let id = hex::encode(bytes);
        if !state.entries.contains_key(&id) {
            return Ok(id);
        }
    }
}
fn ensure<'a>(active: &mut Active, state: &'a mut SessionStore) -> Result<&'a mut Entry> {
    if active.id.is_none() {
        let id = new_id(state)?;
        state.entries.insert(
            id.clone(),
            Entry {
                values: HashMap::new(),
                expires: Instant::now() + LIFETIME,
            },
        );
        active.id = Some(id);
        active.cookie = CookieChange::Set;
    }
    state
        .entries
        .get_mut(active.id.as_ref().unwrap())
        .ok_or_else(|| "session expired or was invalidated".into())
}
#[no_mangle]
pub extern "C" fn ube_session_get(handle: *mut c_void, key: *const c_char) -> *mut c_char {
    run(handle, |active, state| {
        let key = text(key)?;
        Ok(active
            .id
            .as_ref()
            .and_then(|id| state.entries.get(id))
            .and_then(|entry| entry.values.get(&key))
            .map(|value| string(value))
            .unwrap_or(std::ptr::null_mut()))
    })
}
#[no_mangle]
pub extern "C" fn ube_session_set(handle: *mut c_void, key: *const c_char, value: *const c_char) {
    run(handle, |active, state| {
        let (key, value) = (text(key)?, text(value)?);
        ensure(active, state)?.values.insert(key, value);
        Ok(())
    });
}
#[no_mangle]
pub extern "C" fn ube_session_remove(handle: *mut c_void, key: *const c_char) {
    run(handle, |active, state| {
        let key = text(key)?;
        if let Some(entry) = active.id.as_ref().and_then(|id| state.entries.get_mut(id)) {
            entry.values.remove(&key);
        }
        Ok(())
    });
}
#[no_mangle]
pub extern "C" fn ube_session_regenerate(handle: *mut c_void) {
    run(handle, |active, state| {
        let id = new_id(state)?;
        let mut entry = if let Some(old) = active.id.as_ref() {
            state
                .entries
                .remove(old)
                .ok_or("session expired or was invalidated")?
        } else {
            Entry {
                values: HashMap::new(),
                expires: Instant::now() + LIFETIME,
            }
        };
        entry.expires = Instant::now() + LIFETIME;
        state.entries.insert(id.clone(), entry);
        active.id = Some(id);
        active.cookie = CookieChange::Set;
        Ok(())
    });
}
#[no_mangle]
pub extern "C" fn ube_session_destroy(handle: *mut c_void) {
    run(handle, |active, state| {
        if let Some(id) = active.id.take() {
            state.entries.remove(&id);
        }
        active.cookie = CookieChange::Delete;
        Ok(())
    });
}
pub(crate) fn cleanup() {
    registry().lock().unwrap().handles.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    fn set(handle: usize, key: &str, value: &str) {
        ube_session_set(
            handle as _,
            CString::new(key).unwrap().as_ptr(),
            CString::new(value).unwrap().as_ptr(),
        );
        assert_eq!(crate::exceptions::take_pending(), 0);
    }
    fn get(handle: usize, key: &str) -> Option<String> {
        let ptr = ube_session_get(handle as _, CString::new(key).unwrap().as_ptr());
        assert_eq!(crate::exceptions::take_pending(), 0);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().into())
        }
    }
    fn cookie(header: &str) -> &str {
        header.split(';').next().unwrap()
    }
    #[test]
    fn lazy_sessions_persist_and_are_isolated() {
        let store = Store::default();
        let handle = begin(store.clone(), "").unwrap();
        assert_eq!(get(handle, "missing"), None);
        assert!(finish(handle, false).is_none());
        assert!(store.lock().unwrap().entries.is_empty());
        let handle = begin(store.clone(), &format!("{COOKIE_NAME}={}", "a".repeat(64))).unwrap();
        set(handle, "name", "🍠");
        let header = finish(handle, true).unwrap();
        assert!(header.contains("; Secure"));
        assert!(header.contains("HttpOnly; SameSite=Lax"));
        assert_ne!(cookie(&header), format!("{COOKIE_NAME}={}", "a".repeat(64)));
        let handle = begin(store.clone(), cookie(&header)).unwrap();
        assert_eq!(get(handle, "name").as_deref(), Some("🍠"));
        let ptr = ube_session_get(handle as _, CString::new("name").unwrap().as_ptr());
        set(handle, "name", "updated");
        assert_eq!(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap(), "🍠");
        let other = begin(Store::default(), cookie(&header)).unwrap();
        assert_eq!(get(other, "name"), None);
        assert!(finish(other, false).is_none());
        assert_eq!(get(handle, "name").as_deref(), Some("updated"));
        ube_session_remove(handle as _, CString::new("name").unwrap().as_ptr());
        assert_eq!(get(handle, "name"), None);
        assert!(finish(handle, false).is_none());
        let other = begin(Store::default(), cookie(&header)).unwrap();
        assert_eq!(get(other, "name"), None);
        assert!(finish(other, false).is_none());
    }
    #[test]
    fn regeneration_destroy_and_stale_handles() {
        let store = Store::default();
        let initial = begin(store.clone(), "").unwrap();
        set(initial, "userId", "123");
        let old = finish(initial, false).unwrap();
        let handle = begin(store.clone(), cookie(&old)).unwrap();
        let concurrent = begin(store.clone(), cookie(&old)).unwrap();
        ube_session_regenerate(handle as _);
        assert_eq!(get(handle, "userId").as_deref(), Some("123"));
        let rotated = finish(handle, false).unwrap();
        assert_ne!(cookie(&old), cookie(&rotated));
        assert_eq!(get(concurrent, "userId"), None);
        ube_session_set(
            concurrent as _,
            CString::new("x").unwrap().as_ptr(),
            CString::new("y").unwrap().as_ptr(),
        );
        assert_ne!(crate::exceptions::take_pending(), 0); // Cannot resurrect a rotated ID.
        finish(concurrent, false);
        let handle = begin(store.clone(), cookie(&rotated)).unwrap();
        ube_session_destroy(handle as _);
        assert_eq!(get(handle, "userId"), None);
        let deleted = finish(handle, false).unwrap();
        assert!(deleted.contains("Max-Age=0"));
        assert!(!deleted.contains("; Secure"));
        assert!(store.lock().unwrap().entries.is_empty());
        ube_session_destroy(handle as _);
        assert_ne!(crate::exceptions::take_pending(), 0); // Finished request handle.
    }
    #[test]
    fn expiration_and_duplicate_cookies_do_not_authenticate() {
        let store = Store::default();
        let handle = begin(store.clone(), "").unwrap();
        set(handle, "user", "123");
        let header = finish(handle, false).unwrap();
        let duplicate = format!("{}; {}", cookie(&header), cookie(&header));
        let handle = begin(store.clone(), &duplicate).unwrap();
        assert_eq!(get(handle, "user"), None);
        finish(handle, false);
        for entry in store.lock().unwrap().entries.values_mut() {
            entry.expires = Instant::now() - Duration::from_secs(1);
        }
        let handle = begin(store.clone(), cookie(&header)).unwrap();
        assert_eq!(get(handle, "user"), None);
        assert!(store.lock().unwrap().entries.is_empty());
        assert!(finish(handle, false).is_none());
        assert_eq!(cookie_id("ube_session=bad"), None);
    }
    #[test]
    fn null_arguments_are_catchable_and_do_not_create_sessions() {
        let store = Store::default();
        let handle = begin(store.clone(), "").unwrap();
        ube_session_set(handle as _, std::ptr::null(), std::ptr::null());
        assert_ne!(crate::exceptions::take_pending(), 0);
        assert!(store.lock().unwrap().entries.is_empty());
        assert!(finish(handle, false).is_none());
    }
}

// Bind OAuth transactions to the server-owned session identity, never a value
// supplied through application session keys. Rotation invalidates pending flows.
pub(crate) fn identity(handle: *mut c_void, create: bool) -> Result<String> {
    let mut registry = registry().lock().unwrap();
    let active = registry
        .handles
        .get_mut(&(handle as usize))
        .ok_or("session request has finished")?;
    let store = active.store.clone();
    let mut state = store.lock().unwrap();
    state
        .entries
        .retain(|_, entry| entry.expires > Instant::now());
    if create {
        ensure(active, &mut state)?;
    }
    let id = active
        .id
        .as_ref()
        .filter(|id| state.entries.contains_key(*id))
        .ok_or("OAuth callback requires the original active session")?;
    Ok(id.clone())
}
