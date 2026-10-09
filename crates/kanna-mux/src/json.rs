//! JSON helpers. kmux's JSON numbers are doubles, as in the Swift core:
//! whole numbers are written without a fraction (`1`, not `1.0`), and two
//! numbers are equal when their values are.

pub use serde_json::{json, Value};

/// A JSON object.
pub type Map = serde_json::Map<String, Value>;

/// A number, written as an integer when it is whole.
pub fn num(value: f64) -> Value {
    if value.round() == value && value.abs() < 1e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// A string or null.
pub fn opt(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |v| Value::String(v.into()))
}

/// `value` with every number in kmux's form (see `num`).
pub fn normalized(value: &Value) -> Value {
    match value {
        Value::Number(n) => n.as_f64().map_or(Value::Null, num),
        Value::Array(items) => Value::Array(items.iter().map(normalized).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(k, v)| (k.clone(), normalized(v))).collect()),
        other => other.clone(),
    }
}

/// Compact, keys sorted, slashes not escaped.
pub fn encode(value: &Value) -> String {
    serde_json::to_string(&normalized(value)).unwrap_or_else(|_| "null".into())
}

/// Indented, keys sorted: the state file.
pub fn encode_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(&normalized(value)).unwrap_or_else(|_| "null".into())
}

/// Equal as JSON values, comparing numbers by value.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

/// `value[key]` as a string, if it is one.
pub fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Whether `value[key]` is `true`.
pub fn is_true(value: &Value, key: &str) -> bool {
    value.get(key) == Some(&Value::Bool(true))
}

/// Trims spaces and tabs (Swift's `.whitespaces`), not line breaks.
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}
