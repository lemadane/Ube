use rust_decimal::Decimal;
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
    exact: Decimal,
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

fn money_error(message: impl Into<String>) -> *mut UbeMoney {
    crate::exceptions::set_pending(crate::exceptions::named("MoneyError", message.into()));
    std::ptr::null_mut()
}

fn decimal_text(text: &str) -> Result<Decimal, &'static str> {
    // Fixed-point input only: no exponent, separators, whitespace, or rounding.
    let digits = text.strip_prefix('-').unwrap_or(text);
    if text.len() > 64
        || digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || digits.matches('.').count() > 1
        || !digits.bytes().any(|b| b.is_ascii_digit())
    {
        return Err("Money requires fixed-point decimal text");
    }
    Decimal::from_str_exact(text)
        .map_err(|_| "Money decimal is out of range or exceeds 28 fractional digits")
}

fn exact_result(mut mantissa: i128, mut scale: u32) -> Result<Decimal, &'static str> {
    while scale > 0 && mantissa % 10 == 0 {
        mantissa /= 10;
        scale -= 1;
    }
    Decimal::try_from_i128_with_scale(mantissa, scale)
        .map_err(|_| "Money arithmetic overflow or precision loss")
}

fn exact_add(left: Decimal, right: Decimal, subtract: bool) -> Result<Decimal, &'static str> {
    let scale = left.scale().max(right.scale());
    let align = |value: Decimal| {
        value
            .mantissa()
            .checked_mul(10i128.checked_pow(scale - value.scale())?)
    };
    let left = align(left).ok_or("Money arithmetic overflow or precision loss")?;
    let right = align(right).ok_or("Money arithmetic overflow or precision loss")?;
    let result = if subtract {
        left.checked_sub(right)
    } else {
        left.checked_add(right)
    };
    exact_result(
        result.ok_or("Money arithmetic overflow or precision loss")?,
        scale,
    )
}

fn exact_multiply(left: Decimal, right: Decimal, percent: bool) -> Result<Decimal, &'static str> {
    exact_result(
        left.mantissa()
            .checked_mul(right.mantissa())
            .ok_or("Money arithmetic overflow or precision loss")?,
        left.scale() + right.scale() + if percent { 2 } else { 0 },
    )
}

fn exact_money(amount: Decimal, currency: *mut UbeCurrency) -> *mut UbeMoney {
    let currency = if currency.is_null() {
        ube_currency_create(c"PHP".as_ptr())
    } else {
        currency
    };
    Box::into_raw(Box::new(UbeMoney {
        amount: amount.as_f64(),
        currency,
        exact: amount,
    }))
}

#[no_mangle]
pub extern "C" fn ube_money_create(amount: f64, currency: *mut UbeCurrency) -> *mut UbeMoney {
    match decimal_text(&amount.to_string()) {
        Ok(amount) => exact_money(amount, currency),
        Err(message) => money_error(message),
    }
}

#[no_mangle]
pub extern "C" fn ube_money_parse(amount: *const c_char, code: *const c_char) -> *mut UbeMoney {
    if amount.is_null() || code.is_null() {
        return money_error("Money.parse requires decimal text and a currency code");
    }
    let code_bytes = unsafe { CStr::from_ptr(code) }.to_bytes();
    if code_bytes.len() != 3 || !code_bytes.iter().all(u8::is_ascii_uppercase) {
        return money_error("Money currency codes require three uppercase ASCII letters");
    }
    let amount = unsafe { CStr::from_ptr(amount) }
        .to_str()
        .ok()
        .and_then(|text| decimal_text(text).ok());
    match amount {
        Some(amount) => exact_money(amount, ube_currency_create(code)),
        None => money_error("Money.parse requires an exact decimal within the supported range"),
    }
}

#[no_mangle]
pub extern "C" fn ube_money_amount_text(ptr: *mut UbeMoney) -> *mut c_char {
    if ptr.is_null() {
        money_error("Money.amountText requires an instance");
        return std::ptr::null_mut();
    }
    CString::new(unsafe { &*ptr }.exact.normalize().to_string())
        .unwrap()
        .into_raw()
}

#[no_mangle]
pub extern "C" fn ube_money_multiply_percent_text(
    ptr: *mut UbeMoney,
    value: *const c_char,
) -> *mut UbeMoney {
    if ptr.is_null() || value.is_null() {
        return money_error("Money.multiplyPercent requires an instance and decimal text");
    }
    let parsed = unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .and_then(|text| decimal_text(text).ok());
    let Some(percent) = parsed else {
        return money_error("Money.multiplyPercent requires exact decimal text");
    };
    match exact_multiply(unsafe { &*ptr }.exact, percent, true) {
        Ok(amount) => exact_money(amount, unsafe { &*ptr }.currency),
        Err(message) => money_error(message),
    }
}

#[no_mangle]
pub extern "C" fn ube_money_convert_exact(
    ptr: *mut UbeMoney,
    code: *const c_char,
    value: *const c_char,
) -> *mut UbeMoney {
    if ptr.is_null() || value.is_null() {
        return money_error("Money.convertExact requires an instance and decimal text");
    }
    let parsed = unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .and_then(|text| decimal_text(text).ok());
    let Some(rate) = parsed else {
        return money_error("Money.convertExact requires exact decimal text");
    };
    match exact_multiply(unsafe { &*ptr }.exact, rate, false) {
        Ok(amount) => {
            let text = CString::new(amount.to_string()).unwrap();
            ube_money_parse(text.as_ptr(), code)
        }
        Err(message) => money_error(message),
    }
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
                let error = crate::exceptions::named(
                    "CurrencyMismatchError",
                    format!("Money currency mismatch: {} vs {}", c1, c2),
                );
                crate::exceptions::set_pending(error);
                return std::ptr::null_mut();
            }
            match exact_add((*m1).exact, (*m2).exact, false) {
                Ok(amount) => exact_money(amount, (*m1).currency),
                Err(message) => money_error(message),
            }
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
                let error = crate::exceptions::named(
                    "CurrencyMismatchError",
                    format!("Money currency mismatch: {} vs {}", c1, c2),
                );
                crate::exceptions::set_pending(error);
                return std::ptr::null_mut();
            }
            match exact_add((*m1).exact, (*m2).exact, true) {
                Ok(amount) => exact_money(amount, (*m1).currency),
                Err(message) => money_error(message),
            }
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
            let percent = match decimal_text(&(*p).value.to_string()) {
                Ok(value) => value,
                Err(message) => return money_error(message),
            };
            match exact_multiply((*m).exact, percent, true) {
                Ok(amount) => exact_money(amount, (*m).currency),
                Err(message) => money_error(message),
            }
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
            let rate = match decimal_text(&rate.to_string()) {
                Ok(value) => value,
                Err(message) => return money_error(message),
            };
            match exact_multiply((*m).exact, rate, false) {
                Ok(amount) => exact_money(amount, target_curr),
                Err(message) => money_error(message),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_mismatches_return_null_and_set_typed_pending_error() {
        let php = ube_money_php(10.0);
        let usd = ube_money_usd(5.0);
        for operation in [ube_money_add, ube_money_subtract] {
            assert!(operation(php, usd).is_null());
            let error = crate::exceptions::take_pending();
            assert_ne!(error, 0);
            let record = unsafe { &*(error as *const crate::exceptions::ExceptionRecord) };
            assert_eq!(record.type_name, "CurrencyMismatchError");
            assert_eq!(
                crate::exceptions::message(error),
                "Money currency mismatch: PHP vs USD"
            );
            assert_eq!(ube_money_amount(php), 10.0);
            assert_eq!(ube_money_amount(usd), 5.0);
        }
        unsafe {
            for money in [php, usd] {
                let money = Box::from_raw(money);
                let currency = Box::from_raw(money.currency);
                drop(CString::from_raw(currency.code));
            }
        }
    }

    #[test]
    fn matching_currency_codes_allow_arithmetic_without_mutating_operands() {
        let left = ube_money_php(10.0);
        let right = ube_money_php(5.0);
        let sum = ube_money_add(left, right);
        let difference = ube_money_subtract(left, right);
        assert_ne!(sum, left);
        assert_ne!(difference, left);
        assert_eq!(ube_money_amount(sum), 15.0);
        assert_eq!(ube_money_amount(difference), 5.0);
        assert_eq!(ube_money_amount(left), 10.0);
        assert_eq!(ube_money_amount(right), 5.0);
        assert_eq!(crate::exceptions::take_pending(), 0);
        unsafe {
            drop(Box::from_raw(sum));
            drop(Box::from_raw(difference));
            for money in [left, right] {
                let money = Box::from_raw(money);
                let currency = Box::from_raw(money.currency);
                drop(CString::from_raw(currency.code));
            }
        }
    }
}
