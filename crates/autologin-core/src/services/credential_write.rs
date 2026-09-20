use secrecy::{ExposeSecret, SecretString};

use crate::{
    AppError, CredentialStore, PendingSecretCleanupRepository, SecretCleanupKind, SecretRef,
};

/// Must run while holding the repository's mutation lock, including the subsequent metadata
/// transaction. A crash at any point leaves either a cleanup intent or a committed live reference.
pub(super) async fn prepare(
    cleanup: &PendingSecretCleanupRepository,
    credentials: &dyn CredentialStore,
    previous: Option<&SecretRef>,
    password: Option<SecretString>,
    label: &str,
    kind: SecretCleanupKind,
    now: &str,
) -> Result<SecretRef, AppError> {
    let Some(password) = password else {
        return previous
            .cloned()
            .ok_or_else(|| AppError::invalid_field("password"));
    };
    if password.expose_secret().is_empty() || password.expose_secret().len() > 16 * 1024 {
        return Err(AppError::invalid_field("password"));
    }
    let reference = credentials.new_reference()?;
    cleanup
        .enqueue(&reference, kind, None, "write_pending", now)
        .await?;
    // On an ambiguous failure leave the durable intent for a retry, even if no item was created.
    credentials.put(&reference, label, password).await?;
    Ok(reference)
}
