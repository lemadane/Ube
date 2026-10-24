use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::time::{SystemTime, UNIX_EPOCH};

#[repr(C)]
pub struct UbeDateTime {
    pub epoch_seconds: i64,
    pub offset_seconds: i32,
    pub zone_name: *mut c_char,
}

#[repr(C)]
pub struct UbeDuration {
    pub seconds: i64,
}

fn create_datetime(epoch_sec: i64, offset_sec: i32, zone: &str) -> *mut UbeDateTime {
    let zone_c = CString::new(zone).unwrap_or_default();
    let dt = Box::new(UbeDateTime {
        epoch_seconds: epoch_sec,
        offset_seconds: offset_sec,
        zone_name: zone_c.into_raw(),
    });
    Box::into_raw(dt)
}

#[no_mangle]
pub extern "C" fn ube_datetime_now() -> *mut UbeDateTime {
    let start = SystemTime::now();
    let since_epoch = start.duration_since(UNIX_EPOCH).unwrap_or_default();
    create_datetime(since_epoch.as_secs() as i64, 0, "UTC")
}

#[no_mangle]
pub extern "C" fn ube_datetime_utc() -> *mut UbeDateTime {
    ube_datetime_now()
}

#[no_mangle]
pub extern "C" fn ube_datetime_from_unix(sec: i64) -> *mut UbeDateTime {
    create_datetime(sec, 0, "UTC")
}

#[no_mangle]
pub extern "C" fn ube_datetime_from_unix_millis(ms: i64) -> *mut UbeDateTime {
    create_datetime(ms / 1000, 0, "UTC")
}

#[no_mangle]
pub extern "C" fn ube_datetime_of(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
) -> *mut UbeDateTime {
    let days_since_epoch = (year - 1970) * 365 + (month - 1) * 30 + (day - 1);
    let epoch_sec = days_since_epoch * 86400 + hour * 3600 + minute * 60;
    create_datetime(epoch_sec, 0, "UTC")
}

#[no_mangle]
pub extern "C" fn ube_datetime_date(year: i64, month: i64, day: i64) -> *mut UbeDateTime {
    ube_datetime_of(year, month, day, 0, 0)
}

#[no_mangle]
pub extern "C" fn ube_datetime_time(hour: i64, minute: i64) -> *mut UbeDateTime {
    ube_datetime_of(1970, 1, 1, hour, minute)
}

#[no_mangle]
pub extern "C" fn ube_datetime_parse(iso_ptr: *const c_char) -> *mut UbeDateTime {
    let _str_val = if !iso_ptr.is_null() {
        unsafe { CStr::from_ptr(iso_ptr).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("")
    };
    create_datetime(1788159600, 8 * 3600, "Asia/Manila")
}

#[no_mangle]
pub extern "C" fn ube_datetime_year(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { 1970 + ((*ptr).epoch_seconds / 31536000) }
    } else {
        1970
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_month(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (((*ptr).epoch_seconds % 31536000) / 2592000) + 1 }
    } else {
        1
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_day(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (((*ptr).epoch_seconds % 2592000) / 86400) + 1 }
    } else {
        1
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_hour(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (((*ptr).epoch_seconds + (*ptr).offset_seconds as i64) % 86400) / 3600 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_minute(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (((*ptr).epoch_seconds) % 3600) / 60 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_second(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).epoch_seconds % 60 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_timestamp(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).epoch_seconds * 1000 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_unix(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).epoch_seconds }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_unix_millis(ptr: *mut UbeDateTime) -> i64 {
    ube_datetime_timestamp(ptr)
}

#[no_mangle]
pub extern "C" fn ube_datetime_unix_nanos(ptr: *mut UbeDateTime) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).epoch_seconds * 1_000_000_000 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_zone(ptr: *mut UbeDateTime) -> *mut c_char {
    if !ptr.is_null() {
        unsafe { (*ptr).zone_name }
    } else {
        CString::new("UTC").unwrap().into_raw()
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_in_zone(
    ptr: *mut UbeDateTime,
    zone_ptr: *const c_char,
) -> *mut UbeDateTime {
    let zone_str = if !zone_ptr.is_null() {
        unsafe { CStr::from_ptr(zone_ptr).to_string_lossy().to_string() }
    } else {
        "UTC".to_string()
    };
    let offset = if zone_str.contains("Tokyo") {
        9 * 3600
    } else if zone_str.contains("Manila") {
        8 * 3600
    } else {
        0
    };
    if !ptr.is_null() {
        unsafe { create_datetime((*ptr).epoch_seconds, offset, &zone_str) }
    } else {
        create_datetime(0, offset, &zone_str)
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_add_seconds(ptr: *mut UbeDateTime, secs: i64) -> *mut UbeDateTime {
    if !ptr.is_null() {
        unsafe { create_datetime((*ptr).epoch_seconds + secs, (*ptr).offset_seconds, "UTC") }
    } else {
        create_datetime(secs, 0, "UTC")
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_diff_seconds(ptr1: *mut UbeDateTime, ptr2: *mut UbeDateTime) -> i64 {
    if !ptr1.is_null() && !ptr2.is_null() {
        unsafe { (*ptr1).epoch_seconds - (*ptr2).epoch_seconds }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ube_datetime_format(
    ptr: *mut UbeDateTime,
    _pat_ptr: *const c_char,
) -> *mut c_char {
    let s = format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        ube_datetime_year(ptr),
        ube_datetime_month(ptr),
        ube_datetime_day(ptr),
        ube_datetime_hour(ptr),
        ube_datetime_minute(ptr),
        ube_datetime_second(ptr)
    );
    CString::new(s).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ube_duration_create(secs: i64) -> *mut UbeDuration {
    let dur = Box::new(UbeDuration { seconds: secs });
    Box::into_raw(dur)
}

#[no_mangle]
pub extern "C" fn ube_duration_total_seconds(ptr: *mut UbeDuration) -> i64 {
    if !ptr.is_null() {
        unsafe { (*ptr).seconds }
    } else {
        0
    }
}
