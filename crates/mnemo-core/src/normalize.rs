use crate::{MnemoError, MnemoResult};

pub fn normalize_required(field: &str, value: String) -> MnemoResult<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        Err(MnemoError::InvalidRequest(format!("{field} is required")))
    } else {
        Ok(normalized)
    }
}

pub fn normalize_optional(value: String) -> Option<String> {
    let normalized = value.trim().to_string();
    (!normalized.is_empty()).then_some(normalized)
}
