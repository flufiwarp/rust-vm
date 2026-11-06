use serde_json::{Map, Value};

use crate::{project::error::ProjectError};

pub(crate) fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub (crate) fn json_is_null(value: &Value) -> bool {
    matches!(value, Value::Null)
}

pub(crate) fn json_object<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Map<String, Value>, ProjectError> {
    value
        .as_object()
        .ok_or_else(|| ProjectError::unexpected(field, "object", value))
}

pub(crate) fn json_array<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Vec<Value>, ProjectError> {
    value
        .as_array()
        .ok_or_else(|| ProjectError::unexpected(field, "array", value))
}

pub(crate) fn json_str<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, ProjectError> {
    value
        .as_str()
        .ok_or_else(|| ProjectError::unexpected(field, "string", value))
}

pub(crate) fn json_f64(value: &Value, field: &'static str) -> Result<f64, ProjectError> {
    value
        .as_f64()
        .ok_or_else(|| ProjectError::unexpected(field, "number", value))
}

pub(crate) fn json_bool(value: &Value, field: &'static str) -> Result<bool, ProjectError> {
    value
        .as_bool()
        .ok_or_else(|| ProjectError::unexpected(field, "boolean", value))
}

pub(crate) fn json_usize(value: &Value, field: &'static str) -> Result<usize, ProjectError> {
    let number = value
        .as_u64()
        .ok_or_else(|| ProjectError::unexpected(field, "non-negative integer", value))?;

    usize::try_from(number).map_err(|_| ProjectError::invalid(field, number.to_string()))
}

pub(crate) fn json_i32(value: &Value, field: &'static str) -> Result<i32, ProjectError> {
    let number = value
        .as_i64()
        .ok_or_else(|| ProjectError::unexpected(field, "integer", value))?;

    i32::try_from(number).map_err(|_| ProjectError::invalid(field, number.to_string()))
}

pub(crate) fn f64_field(
    value: &Value,
    field: &'static str,
    default: f64,
) -> Result<f64, ProjectError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => json_f64(value, field),
    }
}

pub(crate) fn bool_field(
    value: &Value,
    field: &'static str,
    default: bool,
) -> Result<bool, ProjectError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => json_bool(value, field),
    }
}

pub(crate) fn usize_field(
    value: &Value,
    field: &'static str,
    default: usize,
) -> Result<usize, ProjectError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => json_usize(value, field),
    }
}

pub(crate) fn i32_field(
    value: &Value,
    field: &'static str,
    default: i32,
) -> Result<i32, ProjectError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => json_i32(value, field),
    }
}
