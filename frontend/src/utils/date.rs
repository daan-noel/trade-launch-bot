/// Format a Unix timestamp (seconds) as `YYYY-MM-DD HH:MM:SS UTC`.
#[allow(dead_code)]
pub fn format_timestamp(ts: i64) -> String {
    // Use the JS `Date` API available in WASM to format the timestamp.
    let ms = (ts as f64) * 1000.0;
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        date.get_utc_full_year(),
        date.get_utc_month() + 1,
        date.get_utc_date(),
        date.get_utc_hours(),
        date.get_utc_minutes(),
        date.get_utc_seconds(),
    )
}

/// Format an ISO 8601 string (e.g. `"2026-05-21T12:34:56.789Z"`) for display.
/// Falls back to the raw string on parse failure.
pub fn format_iso(iso: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(iso));
    let ms = date.get_time();
    if ms.is_nan() {
        return iso.to_string();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        date.get_utc_full_year(),
        date.get_utc_month() + 1,
        date.get_utc_date(),
        date.get_utc_hours(),
        date.get_utc_minutes(),
        date.get_utc_seconds(),
    )
}
