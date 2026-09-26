use std::collections::BTreeMap;

use serde_json::Value as Json;
use tcc_state::{export_value, Continuation, Value};

pub fn plain_to_value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(flag) => Value::Bool(*flag),
        Json::Number(number) => Value::Number(number.as_f64().unwrap_or(f64::NAN)),
        Json::String(text) => Value::String(text.clone()),
        Json::Array(items) => Value::Array(items.iter().map(plain_to_value).collect()),
        Json::Object(map) => {
            let mut fields = BTreeMap::new();
            for (key, item) in map {
                fields.insert(key.clone(), plain_to_value(item));
            }
            Value::Object(fields)
        }
    }
}

pub fn value_to_plain(value: &Value) -> Json {
    match value {
        Value::Undefined | Value::Null | Value::Ref(_) => Json::Null,
        Value::Bool(flag) => Json::Bool(*flag),
        Value::Number(number) => number_json(*number),
        Value::String(text) => Json::String(text.clone()),
        Value::Array(items) => Json::Array(items.iter().map(value_to_plain).collect()),
        Value::Object(fields) => {
            let mut map = serde_json::Map::new();
            for (key, item) in fields {
                map.insert(key.clone(), value_to_plain(item));
            }
            Json::Object(map)
        }
    }
}

/// Omitted input is an empty vector. An array is one argument per element.
/// Any other JSON value, including null, is a one-element vector.
pub fn argument_vector(input: Option<&Json>) -> Vec<Value> {
    match input {
        None => Vec::new(),
        Some(Json::Array(items)) => items.iter().map(plain_to_value).collect(),
        Some(other) => vec![plain_to_value(other)],
    }
}

fn number_json(number: f64) -> Json {
    if !number.is_finite() {
        return Json::Null;
    }
    if number.fract() == 0.0 {
        let min = i64::MIN as f64;
        let max = i64::MAX as f64;
        if number >= min && number <= max {
            return Json::Number((number as i64).into());
        }
    }
    serde_json::Number::from_f64(number)
        .map(Json::Number)
        .unwrap_or(Json::Null)
}

pub fn continuation_result(continuation: &Continuation) -> Option<Json> {
    let value = continuation.result.as_ref()?;
    let exported = export_value(&continuation.heap, value).unwrap_or_else(|_| value.clone());
    Some(value_to_plain(&exported))
}
