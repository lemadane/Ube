use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[repr(C)]
pub struct UbeCurrency {
    pub code: *mut c_char,
}

#[repr(C)]
pub struct UbeMoney {
    pub amount: f64,
    pub currency: *mut UbeCurrency,
}

#[repr(C)]
pub struct UbePercent {
    pub value: f64,
}

#[repr(C)]
pub struct UbeRate {
    pub value: f64,
}

#[no_mangle]
pub extern "C" fn ube_currency_create(code_ptr: *const c_char) -> *mut UbeCurrency {
    let code_str = if !code_ptr.is_null() {
        unsafe { CStr::from_ptr(code_ptr).to_string_lossy().to_string() }
    } else {
        "PHP".to_string()
    };
    let c_str = CString::new(code_str).unwrap();
    let curr = Box::new(UbeCurrency {
        code: c_str.into_raw(),
    });
    Box::into_raw(curr)
}

#[no_mangle]
pub extern "C" fn ube_currency_code(ptr: *mut UbeCurrency) -> *mut c_char {
    if !ptr.is_null() {
        unsafe { (*ptr).code }
    } else {
        CString::new("PHP").unwrap().into_raw()
    }
}

#[no_mangle]
pub extern "C" fn ube_money_create(amount: f64, curr_ptr: *mut UbeCurrency) -> *mut UbeMoney {
    let curr = if !curr_ptr.is_null() {
        curr_ptr
    } else {
        ube_currency_create(CString::new("PHP").unwrap().as_ptr())
    };
    let money = Box::new(UbeMoney {
        amount,
        currency: curr,
    });
    Box::into_raw(money)
}

#[no_mangle]
pub extern "C" fn ube_money_php(amount: f64) -> *mut UbeMoney {
    ube_money_create(
        amount,
        ube_currency_create(CString::new("PHP").unwrap().as_ptr()),
    )
}

#[no_mangle]
pub extern "C" fn ube_money_usd(amount: f64) -> *mut UbeMoney {
    ube_money_create(
        amount,
        ube_currency_create(CString::new("USD").unwrap().as_ptr()),
    )
}

#[no_mangle]
pub extern "C" fn ube_money_eur(amount: f64) -> *mut UbeMoney {
    ube_money_create(
        amount,
        ube_currency_create(CString::new("EUR").unwrap().as_ptr()),
    )
}

#[no_mangle]
pub extern "C" fn ube_money_jpy(amount: f64) -> *mut UbeMoney {
    ube_money_create(
        amount,
        ube_currency_create(CString::new("JPY").unwrap().as_ptr()),
    )
}

#[no_mangle]
pub extern "C" fn ube_money_gbp(amount: f64) -> *mut UbeMoney {
    ube_money_create(
        amount,
        ube_currency_create(CString::new("GBP").unwrap().as_ptr()),
    )
}

#[no_mangle]
pub extern "C" fn ube_money_amount(ptr: *mut UbeMoney) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).amount }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_money_currency(ptr: *mut UbeMoney) -> *mut UbeCurrency {
    if !ptr.is_null() {
        unsafe { (*ptr).currency }
    } else {
        ube_currency_create(CString::new("PHP").unwrap().as_ptr())
    }
}

#[no_mangle]
pub extern "C" fn ube_money_add(m1: *mut UbeMoney, m2: *mut UbeMoney) -> *mut UbeMoney {
    if !m1.is_null() && !m2.is_null() {
        unsafe {
            let c1 = CStr::from_ptr((*(*m1).currency).code).to_string_lossy();
            let c2 = CStr::from_ptr((*(*m2).currency).code).to_string_lossy();
            if c1 != c2 {
                eprintln!("Error: Money currency mismatch: {} vs {}", c1, c2);
                std::process::exit(1);
            }
            ube_money_create((*m1).amount + (*m2).amount, (*m1).currency)
        }
    } else {
        m1
    }
}

#[no_mangle]
pub extern "C" fn ube_money_subtract(m1: *mut UbeMoney, m2: *mut UbeMoney) -> *mut UbeMoney {
    if !m1.is_null() && !m2.is_null() {
        unsafe {
            let c1 = CStr::from_ptr((*(*m1).currency).code).to_string_lossy();
            let c2 = CStr::from_ptr((*(*m2).currency).code).to_string_lossy();
            if c1 != c2 {
                eprintln!("Error: Money currency mismatch: {} vs {}", c1, c2);
                std::process::exit(1);
            }
            ube_money_create((*m1).amount - (*m2).amount, (*m1).currency)
        }
    } else {
        m1
    }
}

#[no_mangle]
pub extern "C" fn ube_money_multiply_percent(
    m: *mut UbeMoney,
    p: *mut UbePercent,
) -> *mut UbeMoney {
    if !m.is_null() && !p.is_null() {
        unsafe {
            let res_amt = (*m).amount * ((*p).value / 100.0);
            ube_money_create(res_amt, (*m).currency)
        }
    } else {
        m
    }
}

#[no_mangle]
pub extern "C" fn ube_exchange_convert(
    m: *mut UbeMoney,
    target_curr: *mut UbeCurrency,
    rate: f64,
) -> *mut UbeMoney {
    if !m.is_null() {
        unsafe {
            let converted_amount = (*m).amount * rate;
            ube_money_create(converted_amount, target_curr)
        }
    } else {
        m
    }
}

#[no_mangle]
pub extern "C" fn ube_percent_create(value: f64) -> *mut UbePercent {
    let p = Box::new(UbePercent { value });
    Box::into_raw(p)
}

#[no_mangle]
pub extern "C" fn ube_percent_value(ptr: *mut UbePercent) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).value }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_rate_create(value: f64) -> *mut UbeRate {
    let r = Box::new(UbeRate { value });
    Box::into_raw(r)
}

#[no_mangle]
pub extern "C" fn ube_rate_value(ptr: *mut UbeRate) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).value }
    } else {
        0.0
    }
}
