use crate::json::{trim, Value};

/// Parses "1/3", "25%", "0.25" or a JSON number.
pub fn parse(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }
    let text = trim(value.as_str()?);
    if let Some(percent) = text.strip_suffix('%') {
        return percent.parse::<f64>().ok().map(|n| n / 100.0);
    }
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() == 2 {
        if let (Ok(n), Ok(d)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
            if d != 0.0 {
                return Some(n / d);
            }
        }
    }
    if parts.len() == 1 { text.parse().ok() } else { None }
}

/// The simplest fraction within 0.4% (denominators up to 8), else a percentage.
pub fn format(value: f64) -> String {
    if (value - 1.0).abs() < 1e-6 {
        return "1".into();
    }
    for d in 2..=8 {
        let n = (value * d as f64).round();
        if n > 0.0 && (value - n / d as f64).abs() < 0.004 {
            return format!("{}/{d}", n as i64);
        }
    }
    format!("{}%", (value * 100.0).round() as i64)
}
