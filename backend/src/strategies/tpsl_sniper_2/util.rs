/// Treat a zero parameter as "unset": maps `Some(0.0)` to `None`. TPSL rule
/// params use 0 as the disabled sentinel, so callers get a clean `Option`.
pub fn none_if_zero_f64(val: Option<f64>) -> Option<f64> {
    match val {
        Some(v) if v == 0.0 => None,
        Some(v) => Some(v),
        None => None,
    }
}

/// Treat a zero parameter as "unset": maps `Some(0)` to `None`.
pub fn none_if_zero_u64(val: Option<u64>) -> Option<u64> {
    match val {
        Some(0) => None,
        Some(v) => Some(v),
        None => None,
    }
}
