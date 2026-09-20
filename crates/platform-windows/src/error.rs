#[cfg(test)]
mod tests {
    use super::{credential_error, credential_last_status, record_last_status};

    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_NOT_FOUND: u32 = 1_168;
    const ERROR_NO_SUCH_LOGON_SESSION: u32 = 1_313;

    #[test]
    fn native_statuses_map_to_stable_credential_errors() {
        assert_eq!(
            credential_error(ERROR_NOT_FOUND).code(),
            "credential.not_found"
        );
        assert_eq!(
            credential_error(ERROR_NO_SUCH_LOGON_SESSION).code(),
            "credential.unavailable"
        );
        assert_eq!(
            credential_error(ERROR_ACCESS_DENIED).code(),
            "credential.denied"
        );
        assert_eq!(credential_error(999_999).code(), "credential.unavailable");
    }

    #[test]
    fn native_status_is_recorded_without_error_message_params() {
        record_last_status(ERROR_ACCESS_DENIED);
        assert_eq!(credential_last_status(), Some(ERROR_ACCESS_DENIED as i32));
        assert!(credential_error(ERROR_ACCESS_DENIED).params.is_empty());
    }
}

use std::sync::atomic::{AtomicI32, Ordering};

use autologin_core::AppError;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_FOUND: u32 = 1_168;
const ERROR_NO_SUCH_LOGON_SESSION: u32 = 1_313;
const NO_STATUS: i32 = i32::MIN;

static LAST_STATUS: AtomicI32 = AtomicI32::new(NO_STATUS);

/// Records a failing native credential status without preserving its message text.
///
/// Credential backends call this only when a native failure is exposed to their caller. A
/// successful operation, or an expected `ERROR_NOT_FOUND` used internally to establish that a
/// create target is absent, neither clears nor replaces the last exposed failure.
pub fn record_last_status(status: u32) {
    LAST_STATUS.store(status as i32, Ordering::Relaxed);
}

/// Returns the most recently exposed failing native credential status, if any.
pub fn credential_last_status() -> Option<i32> {
    let status = LAST_STATUS.load(Ordering::Relaxed);
    (status != NO_STATUS).then_some(status)
}

/// Converts a native credential status to a stable, non-sensitive application error.
pub fn credential_error(status: u32) -> AppError {
    record_last_status(status);

    match status {
        ERROR_NOT_FOUND => AppError::new("credential.not_found"),
        ERROR_NO_SUCH_LOGON_SESSION => AppError::new("credential.unavailable"),
        ERROR_ACCESS_DENIED => AppError::credential_denied(),
        _ => AppError::new("credential.unavailable"),
    }
}
