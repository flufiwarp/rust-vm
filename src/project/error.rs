use serde_json::Value;
use thiserror::Error;

use crate::project::utils::json_type_name;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("missing field `{0}`")]
    MissingField(&'static str),

    #[error("field `{field}` expected {expected}, found {found}")]
    UnexpectedType {
        field: String,
        expected: &'static str,
        found: &'static str,
    },

    #[error("field `{field}` has invalid value `{value}`")]
    InvalidValue { field: String, value: String },
    
    #[error("expected block for input")]
    ExpectedBlockForInput,
    #[error("expected primitive for input")]
    ExpectedPrimitiveForInput
}

impl ProjectError {
    pub(crate) fn unexpected(
        field: impl Into<String>,
        expected: &'static str,
        value: &Value,
    ) -> ProjectError {
        ProjectError::UnexpectedType {
            field: field.into(),
            expected,
            found: json_type_name(value),
        }
    }

    pub(crate) fn invalid(field: impl Into<String>, value: impl Into<String>) -> ProjectError {
        ProjectError::InvalidValue {
            field: field.into(),
            value: value.into(),
        }
    }
}