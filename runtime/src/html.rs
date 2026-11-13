//! USSR rendering primitives. Html has the same ABI as a string but a distinct
//! compiler type; only checked templates can construct it.
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn ube_html_identity(value: *const c_char) -> *const c_char { value }

fn allowed_url(value: &str) -> bool {
    if value.chars().any(|c| c.is_control() || c == '\\') { return false; }
    let trimmed = value.trim();
    let prefix = trimmed.split(['/', '?', '#']).next().unwrap_or("");
    match prefix.split_once(':') {
        None => true,
        Some((scheme, _)) => matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "mailto" | "tel"),
    }
}

#[no_mangle]
pub extern "C" fn ube_html_escape(value: *const c_char, context: i32) -> *const c_char {
    let value = if value.is_null() { "" } else { unsafe { CStr::from_ptr(value) }.to_str().unwrap_or("") };
    if context == 3 && !allowed_url(value) {
        crate::exceptions::set_pending(crate::exceptions::generic("Unsafe URL in HTML template".into()));
        return c"".as_ptr();
    }
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"), '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"), '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"), _ => output.push(ch),
        }
    }
    CString::new(output).unwrap().into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_reject_executable_schemes_and_browser_normalization_tricks() {
        for url in ["javascript:alert(1)", " JAVASCRIPT:alert(1)", "data:text/html,x", "vbscript:x", "java\nscript:x", "\\evil.test", "https:\\evil.test"] {
            assert!(!allowed_url(url), "{url:?}");
        }
        for url in ["/todos/1", "#item", "?q=hi", "todos/1", "https://example.com", "mailto:a@example.com", "tel:123"] {
            assert!(allowed_url(url), "{url:?}");
        }
    }
}
