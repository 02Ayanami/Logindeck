use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// A stable, user-facing error returned across the application boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppError {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl AppError {
    pub const VALIDATION_INVALID_FIELD: &'static str = "validation.invalid_field";
    pub const CREDENTIAL_DENIED: &'static str = "credential.denied";
    pub const APPLICATION_NOT_FOUND: &'static str = "application.not_found";
    pub const APPLICATION_SIGNATURE_CHANGED: &'static str = "application.signature_changed";
    pub const STORAGE_CONFLICT: &'static str = "storage.conflict";
    pub const STORAGE_REFERENTIAL_INTEGRITY: &'static str = "storage.referential_integrity";
    pub const STORAGE_DATABASE: &'static str = "storage.database";

    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Adds a complete set of interpolation values while preserving the stable error code.
    pub fn with_params(mut self, params: BTreeMap<String, String>) -> Self {
        self.params = params;
        self
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn invalid_field(field: impl Into<String>) -> Self {
        Self::new(Self::VALIDATION_INVALID_FIELD).with_param("field", field)
    }

    pub fn credential_denied() -> Self {
        Self::new(Self::CREDENTIAL_DENIED)
    }

    pub fn application_not_found() -> Self {
        Self::new(Self::APPLICATION_NOT_FOUND)
    }

    pub fn application_signature_changed() -> Self {
        Self::new(Self::APPLICATION_SIGNATURE_CHANGED)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for AppError {}
