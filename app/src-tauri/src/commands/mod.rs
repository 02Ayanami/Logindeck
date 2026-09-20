pub mod applications;
pub mod edge;
pub mod settings;
pub mod websites;

use autologin_core::AppError;
use serde::Serialize;
use std::collections::BTreeMap;

/// Stable, allowlisted command-boundary error. Never forwards arbitrary AppError codes/params.
#[derive(Clone, Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub params: BTreeMap<String, String>,
}
impl From<AppError> for CommandError {
    fn from(error: AppError) -> Self {
        const CODES: &[&str] = &[
            "validation.invalid_field",
            "credential.denied",
            "credential.cancelled",
            "credential.not_found",
            "credential.missing_entitlement",
            "application.scan_timeout",
            "application.cancelled",
            "credential.unavailable",
            "credential.cleanup_required",
            "credential.compensation_failed",
            "credential.compensation_tracking_failed",
            "credential.cleanup_tracking_failed",
            "clipboard.unavailable",
            "application.not_found",
            "application.signature_changed",
            "application.launch_failed",
            "application.unsupported_target",
            "application.unsupported_import",
            "application.discovery_unavailable",
            "storage.conflict",
            "storage.referential_integrity",
            "storage.not_found",
            "storage.database",
            "platform.unsupported",
            "extension.setup_failed",
            "extension.setup_conflict",
            "extension.resources_missing",
            "extension.edge_unavailable",
        ];
        let code = if CODES.contains(&error.code()) {
            error.code().to_owned()
        } else {
            "internal.error".to_owned()
        };
        let mut params = BTreeMap::new();
        if code == "validation.invalid_field" {
            if let Some(field) = error.params.get("field").filter(|value| {
                matches!(
                    value.as_str(),
                    "name"
                        | "url"
                        | "username"
                        | "password"
                        | "phone"
                        | "login_method"
                        | "application_id"
                        | "notes"
                        | "display_name"
                        | "platform_application_id"
                        | "launch_target"
                        | "version"
                        | "signature_identity"
                        | "path_access_ref"
                        | "paths"
                        | "authentication_reason"
                )
            }) {
                params.insert("field".into(), field.clone());
            }
        }
        Self { code, params }
    }
}

#[cfg(test)]
mod tests {
    use super::CommandError;
    use autologin_core::AppError;

    #[test]
    fn application_validation_fields_have_exact_safe_wire_errors() {
        for field in [
            "platform_application_id",
            "launch_target",
            "version",
            "signature_identity",
            "path_access_ref",
        ] {
            let error = CommandError::from(AppError::invalid_field(field));
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                serde_json::json!({"code": "validation.invalid_field", "params": {"field": field}})
            );
        }
    }

    #[test]
    fn native_launch_errors_keep_their_code_without_os_details() {
        for code in [
            "application.launch_failed",
            "application.unsupported_target",
        ] {
            let error = CommandError::from(AppError::new(code).with_param("path", "private"));
            assert_eq!(error.code, code);
            assert!(error.params.is_empty());
        }
    }

    #[test]
    fn cancellation_and_recoverable_cleanup_errors_remain_stable_at_the_command_boundary() {
        for code in [
            "credential.cancelled",
            "credential.cleanup_required",
            "credential.compensation_failed",
            "credential.compensation_tracking_failed",
            "credential.cleanup_tracking_failed",
        ] {
            assert_eq!(CommandError::from(AppError::new(code)).code, code);
        }
        assert_eq!(
            CommandError::from(AppError::new("untrusted.keychain.error")).code,
            "internal.error"
        );
    }
}
