pub fn ignore_zero_f64(val: Option<f64>) -> Option<f64> {
    match val {
        Some(v) if v == 0.0 => None,
        Some(v) => Some(v),
        None => None,
    }
}

pub fn ignore_zero_u64(val: Option<u64>) -> Option<u64> {
    match val {
        Some(0) => None,
        Some(v) => Some(v),
        None => None,
    }
}
