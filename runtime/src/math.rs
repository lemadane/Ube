use crate::collections::{ube_list_create, ube_list_get, ube_list_len, UbeList};
use std::ffi::CString;
use std::os::raw::c_char;

// Constants
#[no_mangle]
pub extern "C" fn ube_math_pi() -> f64 {
    std::f64::consts::PI
}
#[no_mangle]
pub extern "C" fn ube_math_e() -> f64 {
    std::f64::consts::E
}
#[no_mangle]
pub extern "C" fn ube_math_tau() -> f64 {
    std::f64::consts::TAU
}

// Powers & Roots
#[no_mangle]
pub extern "C" fn ube_math_abs(val: f64) -> f64 {
    val.abs()
}
#[no_mangle]
pub extern "C" fn ube_math_min(a: f64, b: f64) -> f64 {
    a.min(b)
}
#[no_mangle]
pub extern "C" fn ube_math_max(a: f64, b: f64) -> f64 {
    a.max(b)
}
#[no_mangle]
pub extern "C" fn ube_math_sqrt(val: f64) -> f64 {
    val.sqrt()
}
#[no_mangle]
pub extern "C" fn ube_math_cbrt(val: f64) -> f64 {
    val.cbrt()
}
#[no_mangle]
pub extern "C" fn ube_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

// Rounding & Precision
#[no_mangle]
pub extern "C" fn ube_math_floor(val: f64) -> f64 {
    val.floor()
}
#[no_mangle]
pub extern "C" fn ube_math_ceil(val: f64) -> f64 {
    val.ceil()
}
#[no_mangle]
pub extern "C" fn ube_math_round(val: f64) -> f64 {
    val.round()
}
#[no_mangle]
pub extern "C" fn ube_math_round_decimals(val: f64, decimals: i64) -> f64 {
    let factor = 10.0_f64.powi(decimals as i32);
    (val * factor).round() / factor
}
#[no_mangle]
pub extern "C" fn ube_math_truncate(val: f64) -> f64 {
    val.trunc()
}
#[no_mangle]
pub extern "C" fn ube_math_clamp(val: f64, min: f64, max: f64) -> f64 {
    val.max(min).min(max)
}
#[no_mangle]
pub extern "C" fn ube_math_sign(val: f64) -> i64 {
    if val > 0.0 {
        1
    } else if val < 0.0 {
        -1
    } else {
        0
    }
}

// Trigonometry
#[no_mangle]
pub extern "C" fn ube_math_sin(val: f64) -> f64 {
    val.sin()
}
#[no_mangle]
pub extern "C" fn ube_math_cos(val: f64) -> f64 {
    val.cos()
}
#[no_mangle]
pub extern "C" fn ube_math_tan(val: f64) -> f64 {
    val.tan()
}
#[no_mangle]
pub extern "C" fn ube_math_asin(val: f64) -> f64 {
    val.asin()
}
#[no_mangle]
pub extern "C" fn ube_math_acos(val: f64) -> f64 {
    val.acos()
}
#[no_mangle]
pub extern "C" fn ube_math_atan(val: f64) -> f64 {
    val.atan()
}
#[no_mangle]
pub extern "C" fn ube_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
#[no_mangle]
pub extern "C" fn ube_math_radians(deg: f64) -> f64 {
    deg.to_radians()
}
#[no_mangle]
pub extern "C" fn ube_math_degrees(rad: f64) -> f64 {
    rad.to_degrees()
}

// Logarithms & Exponents
#[no_mangle]
pub extern "C" fn ube_math_log(val: f64) -> f64 {
    val.ln()
}
#[no_mangle]
pub extern "C" fn ube_math_log10(val: f64) -> f64 {
    val.log10()
}
#[no_mangle]
pub extern "C" fn ube_math_log2(val: f64) -> f64 {
    val.log2()
}
#[no_mangle]
pub extern "C" fn ube_math_exp(val: f64) -> f64 {
    val.exp()
}

// Numeric Checks
#[no_mangle]
pub extern "C" fn ube_math_is_nan(val: f64) -> bool {
    val.is_nan()
}
#[no_mangle]
pub extern "C" fn ube_math_is_infinite(val: f64) -> bool {
    val.is_infinite()
}
#[no_mangle]
pub extern "C" fn ube_math_is_finite(val: f64) -> bool {
    val.is_finite()
}
#[no_mangle]
pub extern "C" fn ube_math_is_even(val: i64) -> bool {
    val % 2 == 0
}
#[no_mangle]
pub extern "C" fn ube_math_is_odd(val: i64) -> bool {
    val % 2 != 0
}

// GCD & LCM
fn gcd_impl(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let temp = b;
        b = a % b;
        a = temp;
    }
    a
}
#[no_mangle]
pub extern "C" fn ube_math_gcd(a: i64, b: i64) -> i64 {
    gcd_impl(a, b)
}
#[no_mangle]
pub extern "C" fn ube_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a.abs() * b.abs()) / gcd_impl(a, b)
    }
}

// Random
#[no_mangle]
pub extern "C" fn ube_random_int(min: i64, max: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let span = (max - min + 1).max(1);
    min + ((now as i64).abs() % span)
}
#[no_mangle]
pub extern "C" fn ube_random_decimal() -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    ((now % 1_000_000) as f64) / 1_000_000.0
}
#[no_mangle]
pub extern "C" fn ube_random_bool() -> bool {
    ube_random_int(0, 1) == 1
}
#[no_mangle]
pub extern "C" fn ube_random_uuid() -> *mut c_char {
    let uuid = format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        ube_random_int(0, 0xFFFFFFFF),
        ube_random_int(0, 0xFFFF),
        ube_random_int(0, 0xFFF),
        ube_random_int(0, 0x3FFF) | 0x8000,
        ube_random_int(0, 0xFFFFFFFFFFFF)
    );
    CString::new(uuid).unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn ube_random_bytes(count: i64) -> *mut UbeList {
    let list = ube_list_create();
    for _ in 0..count {
        crate::collections::add_typed(list, ube_random_int(0, 255), 1, 2, "long");
    }
    list
}

// SecureRandom
#[no_mangle]
pub extern "C" fn ube_securerandom_bytes(count: i64) -> *mut UbeList {
    ube_random_bytes(count)
}

// Statistics
#[no_mangle]
pub extern "C" fn ube_statistics_mean(list: *mut UbeList) -> f64 {
    let len = ube_list_len(list);
    if len == 0 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..len {
        let val = ube_list_get(list, i);
        sum += f64::from_bits(val as u64);
    }
    sum / (len as f64)
}

#[no_mangle]
pub extern "C" fn ube_statistics_median(list: *mut UbeList) -> f64 {
    let len = ube_list_len(list);
    if len == 0 {
        return 0.0;
    }
    let mut vec = Vec::new();
    for i in 0..len {
        let val = ube_list_get(list, i);
        vec.push(f64::from_bits(val as u64));
    }
    vec.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = len / 2;
    if len % 2 == 0 {
        (vec[(mid - 1) as usize] + vec[mid as usize]) / 2.0
    } else {
        vec[mid as usize]
    }
}

#[no_mangle]
pub extern "C" fn ube_statistics_variance(list: *mut UbeList) -> f64 {
    let len = ube_list_len(list);
    if len <= 1 {
        return 0.0;
    }
    let mean = ube_statistics_mean(list);
    let mut sum_sq_diff = 0.0;
    for i in 0..len {
        let val = f64::from_bits(ube_list_get(list, i) as u64);
        sum_sq_diff += (val - mean).powi(2);
    }
    sum_sq_diff / (len as f64)
}

#[no_mangle]
pub extern "C" fn ube_statistics_std_dev(list: *mut UbeList) -> f64 {
    ube_statistics_variance(list).sqrt()
}

// Combinatorics
fn fact_impl(n: i64) -> i64 {
    if n <= 1 {
        1
    } else {
        n * fact_impl(n - 1)
    }
}
#[no_mangle]
pub extern "C" fn ube_probability_factorial(n: i64) -> i64 {
    fact_impl(n)
}
#[no_mangle]
pub extern "C" fn ube_probability_permutations(n: i64, k: i64) -> i64 {
    if k > n || k < 0 {
        0
    } else {
        fact_impl(n) / fact_impl(n - k)
    }
}
#[no_mangle]
pub extern "C" fn ube_probability_combinations(n: i64, k: i64) -> i64 {
    if k > n || k < 0 {
        0
    } else {
        fact_impl(n) / (fact_impl(k) * fact_impl(n - k))
    }
}

// Probability Distributions
#[no_mangle]
pub extern "C" fn ube_probability_normal(x: f64, mean: f64, std_dev: f64) -> f64 {
    if std_dev <= 0.0 {
        return 0.0;
    }
    let exponent = -0.5 * ((x - mean) / std_dev).powi(2);
    (1.0 / (std_dev * (2.0 * std::f64::consts::PI).sqrt())) * exponent.exp()
}
#[no_mangle]
pub extern "C" fn ube_probability_binomial(k: i64, n: i64, p: f64) -> f64 {
    let comb = ube_probability_combinations(n, k) as f64;
    comb * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32)
}
#[no_mangle]
pub extern "C" fn ube_probability_poisson(k: i64, lambda: f64) -> f64 {
    (lambda.powi(k as i32) * (-lambda).exp()) / (fact_impl(k) as f64)
}
#[no_mangle]
pub extern "C" fn ube_probability_uniform(x: f64, min: f64, max: f64) -> f64 {
    if x >= min && x <= max && max > min {
        1.0 / (max - min)
    } else {
        0.0
    }
}

// Enhanced Statistics
#[no_mangle]
pub extern "C" fn ube_statistics_percentile(list: *mut UbeList, p: f64) -> f64 {
    let len = ube_list_len(list);
    if len == 0 {
        return 0.0;
    }
    let mut vec = Vec::new();
    for i in 0..len {
        let val = ube_list_get(list, i);
        vec.push(f64::from_bits(val as u64));
    }
    vec.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (p / 100.0) * ((len - 1) as f64);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let weight = rank - (lower as f64);
    if lower == upper {
        vec[lower]
    } else {
        vec[lower] * (1.0 - weight) + vec[upper] * weight
    }
}

#[no_mangle]
pub extern "C" fn ube_statistics_covariance(x_list: *mut UbeList, y_list: *mut UbeList) -> f64 {
    let len_x = ube_list_len(x_list);
    let len_y = ube_list_len(y_list);
    let len = len_x.min(len_y);
    if len <= 1 {
        return 0.0;
    }
    let mean_x = ube_statistics_mean(x_list);
    let mean_y = ube_statistics_mean(y_list);
    let mut sum = 0.0;
    for i in 0..len {
        let x_val = f64::from_bits(ube_list_get(x_list, i) as u64);
        let y_val = f64::from_bits(ube_list_get(y_list, i) as u64);
        sum += (x_val - mean_x) * (y_val - mean_y);
    }
    sum / (len as f64)
}

#[no_mangle]
pub extern "C" fn ube_statistics_correlation(x_list: *mut UbeList, y_list: *mut UbeList) -> f64 {
    let std_x = ube_statistics_std_dev(x_list);
    let std_y = ube_statistics_std_dev(y_list);
    if std_x == 0.0 || std_y == 0.0 {
        return 0.0;
    }
    ube_statistics_covariance(x_list, y_list) / (std_x * std_y)
}
