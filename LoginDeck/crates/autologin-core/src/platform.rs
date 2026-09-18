use std::path::Path;

use secrecy::SecretString;

use crate::{AppError, ApplicationRecord, DiscoveredApplication, SecretRef, VerifiedApplication};

#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync {
    fn new_reference(&self) -> Result<SecretRef, AppError> {
        SecretRef::new(uuid::Uuid::new_v4().to_string())
    }
    /// Creates an item at a preallocated, durably tracked reference. Never overwrites an item.
    async fn put(
        &self,
        reference: &SecretRef,
        label: &str,
        secret: SecretString,
    ) -> Result<(), AppError>;
    async fn reveal(&self, reference: &SecretRef, reason: &str) -> Result<SecretString, AppError>;
    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError>;
}

#[async_trait::async_trait]
pub trait UserPresence: Send + Sync {
    async fn authenticate(&self, reason: &str) -> Result<(), AppError>;
}

#[async_trait::async_trait]
pub trait ApplicationCatalog: Send + Sync {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError>;
    async fn import_bundle(&self, path: &Path) -> Result<Vec<DiscoveredApplication>, AppError>;
    /// Revalidates the application identity and launches it as one atomic platform operation.
    ///
    /// Keeping verification and launch together prevents callers from manufacturing a stale or
    /// forged `VerifiedApplication` and passing it to a separate launch method.
    async fn verify_and_launch(
        &self,
        app: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError>;
}

/// A deliberately bounded clipboard interface for time-limited password copies.
///
/// It exposes neither reads nor a way to clear arbitrary clipboard contents. The token returned
/// by `write` can only be used to conditionally clear that exact write later.
#[async_trait::async_trait]
pub trait Clipboard: Send + Sync {
    type ChangeToken: Send + Sync;

    async fn write(&self, value: SecretString) -> Result<Self::ChangeToken, AppError>;
    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError>;
}
