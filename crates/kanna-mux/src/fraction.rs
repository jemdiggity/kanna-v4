use crate::json::{trim, Value};

pub fn parse(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }
    let text = trim(value.as_str()?);
    if let Some(percent) = text.strip_suffix('%') {
        return number(percent).map(|n| n / 100.0);
    }
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() == 2 {
        if let (Some(n), Some(d)) = (number(parts[0]), number(parts[1])) {
            if d != 0.0 {
                return Some(n / d);
            }
        }
    }
    if parts.len() == 1 { number(text) } else { None }
}

fn number(text: &str) -> Option<f64> {
    hex(text).or_else(|| text.parse().ok())
}

fn hex(text: &str) -> Option<f64> {
    let (negative, rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let rest = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X"))?;
    let (digits, exponent) = match rest.find(['p', 'P']) {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut mantissa: u128 = 0;
    let mut shift: i64 = 0;
    for (i, c) in whole.chars().chain(fraction.chars()).enumerate() {
        let digit = c.to_digit(16)? as u128;
        if mantissa >> 120 == 0 {
            mantissa = mantissa << 4 | digit;
            if i >= whole.len() {
                shift -= 4;
            }
        } else if i < whole.len() {
            shift += 4;
        }
    }
    let power = match exponent {
        None => 0,
        Some(exponent) => {
            let unsigned = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let magnitude = unsigned.bytes().fold(0i64, |n, b| n.saturating_mul(10).saturating_add((b - b'0') as i64));
            if exponent.starts_with('-') { -magnitude } else { magnitude }
        }
    };
    let mut value = mantissa as f64;
    let mut power = power.saturating_add(shift);
    if value != 0.0 {
        while power > 1000 {
            value *= 2f64.powi(1000);
            power -= 1000;
        }
        while power < -1000 {
            value *= 2f64.powi(-1000);
            power += 1000;
        }
        value *= 2f64.powi(power as i32);
    }
    Some(if negative { -value } else { value })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hex_floats_parse_as_swift_reads_them() {
        for (text, value) in [("0x1p-1", 0.5), ("0x.8", 0.5), ("0x1.", 1.0), ("-0x1p+3", -8.0), ("0x1P-1", 0.5), ("0x1.8p1", 3.0), ("+0X10", 16.0), ("0x1p-1080", 0.0)] {
            assert_eq!(parse(&json!(text)), Some(value), "{text}");
        }
        assert_eq!(parse(&json!("0xffffffffffffffffffffp0")), Some(1.2089258196146292e24));
        assert_eq!(parse(&json!("0x1p99999")), Some(f64::INFINITY));
        assert_eq!(parse(&json!("0x1p-1074")), Some(f64::from_bits(1)));
        assert_eq!(parse(&json!("0x1p-2/0x1p-1")), Some(0.5));
        assert_eq!(parse(&json!("0x8p-4%")), Some(0.005));
        for text in ["0x", "0x1p", "0xg", "0x.", "0x1p1.5", "0x1_0", "0x1p+", "0x1 p1"] {
            assert_eq!(parse(&json!(text)), None, "{text}");
        }
    }
}
