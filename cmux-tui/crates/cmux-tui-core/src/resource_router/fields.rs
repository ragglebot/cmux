//! Typed reads of catalog-validated request fields.

use serde_json::{Map, Value, json};

use super::validation_error;
use crate::resource::{ResourceError, WireDecimal};

pub(crate) fn expected_revision(fields: &Map<String, Value>) -> Result<Option<u64>, ResourceError> {
    fields
        .get("expected_revision")
        .map(|value| {
            serde_json::from_value::<WireDecimal>(value.clone()).map(WireDecimal::get).map_err(
                |error| {
                    validation_error(
                        "expected_revision must be an unsigned decimal string",
                        json!({"error":error.to_string()}),
                    )
                },
            )
        })
        .transpose()
}

pub(crate) fn required_string<'a>(
    fields: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, ResourceError> {
    fields
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| validation_error("required string field is missing", json!({"field":field})))
}

pub(crate) fn optional_string(
    fields: &Map<String, Value>,
    field: &str,
) -> Result<Option<String>, ResourceError> {
    fields
        .get(field)
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| validation_error("field must be a string", json!({"field":field})))
        })
        .transpose()
}

pub(crate) fn required_u64(fields: &Map<String, Value>, field: &str) -> Result<u64, ResourceError> {
    fields.get(field).and_then(Value::as_u64).ok_or_else(|| {
        validation_error("required unsigned integer field is missing", json!({"field":field}))
    })
}
