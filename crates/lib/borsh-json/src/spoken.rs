//! An op or a query as its caller meant it: JSON written out as a string, as a model or a client
//! may hand on a field that names no type, is read as the JSON it holds.
use serde_json::Value;

pub fn spoken(value: &Value) -> Value {
    unquoted(value).unwrap_or_else(|| value.clone())
}

// The JSON a value holds where it came as JSON text: an object, an array or a string written out
// as a string.
pub fn unquoted(value: &Value) -> Option<Value> {
    let held: Value = serde_json::from_str(value.as_str()?).ok()?;
    matches!(held, Value::Object(_) | Value::Array(_) | Value::String(_)).then_some(held)
}
