/// Format a float to a fixed number of decimal places.
pub fn format_decimal(value: f64, decimals: usize) -> String {
    format!("{:.prec$}", value, prec = decimals)
}

/// Format a token price using engineering-style exponents for small values.
///
/// Examples:
/// - 0.001 -> "1e-3"
/// - 0.0000289 -> "28.9e-6"
/// - 7.04e-14 -> "70.4e-15"
pub fn format_price(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let abs = value.abs();
    if abs >= 1.0 {
        return format_decimal_trim(value, 6);
    }

    let exponent = -(abs.log10().floor() as i32);
    let engineering_exponent = if exponent <= 3 {
        -3
    } else if exponent <= 6 {
        -6
    } else if exponent <= 9 {
        -9
    } else if exponent <= 12 {
        -12
    } else if exponent <= 15 {
        -15
    } else {
        return format!("{:.6e}", value);
    };

    let mantissa = value / 10f64.powi(engineering_exponent);
    format!(
        "{}e{}",
        format_decimal_trim(mantissa, 6),
        engineering_exponent
    )
}

/// Format a float and trim trailing zeros from the decimal part.
pub fn format_decimal_trim(value: f64, decimals: usize) -> String {
    let s = format!("{:.prec$}", value, prec = decimals);
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed.is_empty() {
            s
        } else {
            trimmed.to_string()
        }
    } else {
        s
    }
}

/// Format a float with compact suffixes for thousands, millions, and billions.
pub fn format_compact(value: f64, decimals: usize) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let abs = value.abs();
    let sign = if value.is_sign_negative() { "-" } else { "" };

    if abs >= 1_000_000_000.0 {
        format!(
            "{}{}G",
            sign,
            format_decimal_trim(abs / 1_000_000_000.0, decimals)
        )
    } else if abs >= 1_000_000.0 {
        format!(
            "{}{}M",
            sign,
            format_decimal_trim(abs / 1_000_000.0, decimals)
        )
    } else if abs >= 1_000.0 {
        format!("{}{}K", sign, format_decimal_trim(abs / 1_000.0, decimals))
    } else if abs < 1e-6 {
        format!("{}{:.*e}", sign, decimals, abs)
    } else {
        format_decimal_trim(value, decimals)
    }
}

/// Truncate a long string (e.g. a signature) for display.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

/// Format age in seconds to a compact human-readable string.
/// Examples: "45s", "12m", "3h 20m", "2d 5h"
pub fn format_age(seconds: i64) -> String {
    if seconds < 0 {
        return "?".into();
    }
    if seconds < 60 {
        return format!("{}s", seconds);
    }
    if seconds < 3_600 {
        return format!("{}m", seconds / 60);
    }
    if seconds < 86_400 {
        let h = seconds / 3_600;
        let m = (seconds % 3_600) / 60;
        if m == 0 { format!("{}h", h) } else { format!("{}h {}m", h, m) }
    } else {
        let d = seconds / 86_400;
        let h = (seconds % 86_400) / 3_600;
        if h == 0 { format!("{}d", d) } else { format!("{}d {}h", d, h) }
    }
}

/// Format an integer with thousands-separator commas. e.g. 1234567 → "1,234,567"
pub fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

/// CSS class for age-based colour coding.
pub fn age_class(seconds: i64) -> &'static str {
    if seconds < 3_600 {
        "age-fresh"         // < 1 h  — danger red
    } else if seconds < 86_400 {
        "age-recent"        // < 24 h — warning yellow
    } else if seconds < 604_800 {
        "age-normal"        // < 7 d  — primary green
    } else {
        "age-old"           // > 7 d  — muted
    }
}
