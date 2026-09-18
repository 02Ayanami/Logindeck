use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::Barrier;

use async_trait::async_trait;
use autologin_core::{AppError, CredentialStore, SecretRef};
use secrecy::SecretString;

#[derive(Default)]
struct Credentials {
    puts: Mutex<usize>,
}
#[async_trait]
impl CredentialStore for Credentials {
    async fn put(
        &self,
        _: &SecretRef,
        _label: &str,
        _secret: SecretString,
    ) -> Result<(), AppError> {
        *self.puts.lock().unwrap() += 1;
        Ok(())
    }
    async fn reveal(
        &self,
        _reference: &SecretRef,
        _reason: &str,
    ) -> Result<SecretString, AppError> {
        unreachable!()
    }
    async fn delete(&self, _reference: &SecretRef) -> Result<(), AppError> {
        Ok(())
    }
}

#[tokio::test]
async fn save_request_debug_redacts_password() {
    let request = autologin_core::SaveWebsite::new(
        None,
        "Example",
        "https://example.com",
        "alice",
        SecretString::from("not-visible"),
        "",
    );
    assert!(!format!("{request:?}").contains("not-visible"));
}

#[tokio::test]
async fn website_service_normalizes_idn_origin_and_keeps_password_out_of_metadata() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(Credentials::default());
    let vault = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    let record = vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "HTTPS://BÜCHER.example:443/path",
            "alice",
            SecretString::from("not-visible"),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(record.normalized_origin, "https://xn--bcher-kva.example");
    assert_eq!(*credentials.puts.lock().unwrap(), 1);
    assert!(!format!("{:?}", repos.websites().list().await.unwrap()).contains("not-visible"));
}

#[derive(Default)]
struct TrackingCredentials {
    puts: Mutex<Vec<String>>,
    references: Mutex<Vec<String>>,
    deletes: Mutex<Vec<String>>,
    fail_delete: Mutex<bool>,
}

#[async_trait]
impl CredentialStore for TrackingCredentials {
    async fn put(
        &self,
        reference: &SecretRef,
        _label: &str,
        secret: SecretString,
    ) -> Result<(), AppError> {
        use secrecy::ExposeSecret;
        self.puts
            .lock()
            .unwrap()
            .push(secret.expose_secret().to_owned());
        self.references
            .lock()
            .unwrap()
            .push(reference.as_str().into());
        Ok(())
    }

    async fn reveal(
        &self,
        _reference: &SecretRef,
        _reason: &str,
    ) -> Result<SecretString, AppError> {
        unreachable!("copy is not part of this rollback test")
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
        self.deletes
            .lock()
            .unwrap()
            .push(reference.as_str().to_owned());
        if *self.fail_delete.lock().unwrap() {
            Err(AppError::new("credential.unavailable"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn duplicate_metadata_compensates_the_new_secret_without_persisting_password() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(TrackingCredentials::default());
    let vault = autologin_core::VaultService::new(repos.websites(), credentials.clone());

    vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("first-secret"),
            "",
        ))
        .await
        .unwrap();
    let error = vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Duplicate",
            "https://example.com",
            "alice",
            SecretString::from("second-secret"),
            "",
        ))
        .await
        .unwrap_err();

    assert_eq!(error.code(), "storage.conflict");
    assert_eq!(
        *credentials.deletes.lock().unwrap(),
        vec![credentials.references.lock().unwrap()[1].clone()]
    );
    let rendered_metadata = format!("{:?}", repos.websites().list().await.unwrap());
    assert!(!rendered_metadata.contains("first-secret"));
    assert!(!rendered_metadata.contains("second-secret"));
}

#[tokio::test]
async fn failed_secret_deletion_keeps_website_metadata() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(TrackingCredentials {
        fail_delete: Mutex::new(true),
        ..Default::default()
    });
    let vault = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    let record = vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("only-in-credential-store"),
            "",
        ))
        .await
        .unwrap();

    let error = vault.delete_website(record.id).await.unwrap_err();
    assert_eq!(error.code(), "credential.unavailable");
    assert!(repos.websites().get(record.id).await.unwrap().is_some());
    assert_eq!(
        *credentials.deletes.lock().unwrap(),
        vec![record.password_credential_ref.as_str()]
    );
}

#[derive(Default)]
struct RevealingCredentials {
    reveals: Mutex<usize>,
}
#[async_trait]
impl CredentialStore for RevealingCredentials {
    async fn put(&self, _reference: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        unreachable!()
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        *self.reveals.lock().unwrap() += 1;
        Ok(SecretString::from("clipboard-only-secret"))
    }
    async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
        unreachable!()
    }
}

struct Presence {
    allowed: bool,
    calls: Mutex<usize>,
}
#[async_trait]
impl autologin_core::UserPresence for Presence {
    async fn authenticate(&self, _: &str) -> Result<(), AppError> {
        *self.calls.lock().unwrap() += 1;
        if self.allowed {
            Ok(())
        } else {
            Err(AppError::credential_denied())
        }
    }
}

#[derive(Default)]
struct BoundedClipboard {
    writes: Mutex<usize>,
    cleared_tokens: Mutex<Vec<u64>>,
}
#[async_trait]
impl autologin_core::Clipboard for BoundedClipboard {
    type ChangeToken = u64;
    async fn write(&self, _: SecretString) -> Result<Self::ChangeToken, AppError> {
        *self.writes.lock().unwrap() += 1;
        Ok(41)
    }
    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError> {
        self.cleared_tokens.lock().unwrap().push(*token);
        Ok(())
    }
}

#[tokio::test]
async fn denied_user_presence_never_reveals_or_writes_a_password() {
    let credentials = Arc::new(RevealingCredentials::default());
    let clipboard = Arc::new(BoundedClipboard::default());
    let presence = Arc::new(Presence {
        allowed: false,
        calls: Mutex::new(0),
    });

    let error = autologin_core::copy_password(
        credentials.clone(),
        SecretRef::new("opaque-ref").unwrap(),
        presence.clone(),
        clipboard.clone(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), "credential.denied");
    assert_eq!(*presence.calls.lock().unwrap(), 1);
    assert_eq!(*credentials.reveals.lock().unwrap(), 0);
    assert_eq!(*clipboard.writes.lock().unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn password_copy_schedules_one_conditional_clear_for_its_own_token() {
    let credentials = Arc::new(RevealingCredentials::default());
    let clipboard = Arc::new(BoundedClipboard::default());
    let presence = Arc::new(Presence {
        allowed: true,
        calls: Mutex::new(0),
    });

    autologin_core::copy_password(
        credentials.clone(),
        SecretRef::new("opaque-ref").unwrap(),
        presence,
        clipboard.clone(),
    )
    .await
    .unwrap();
    assert_eq!(*clipboard.cleared_tokens.lock().unwrap(), Vec::<u64>::new());

    tokio::time::advance(std::time::Duration::from_secs(30)).await;
    tokio::task::yield_now().await;
    assert_eq!(*credentials.reveals.lock().unwrap(), 1);
    assert_eq!(*clipboard.writes.lock().unwrap(), 1);
    assert_eq!(*clipboard.cleared_tokens.lock().unwrap(), vec![41]);
}

#[tokio::test]
async fn failed_deletion_can_be_explicitly_retried_without_losing_metadata() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(TrackingCredentials {
        fail_delete: Mutex::new(true),
        ..Default::default()
    });
    let vault = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    let record = vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("kept-out-of-sqlite"),
            "",
        ))
        .await
        .unwrap();

    assert_eq!(
        vault.delete_website(record.id).await.unwrap_err().code(),
        "credential.unavailable"
    );
    assert_eq!(
        repos.pending_secret_cleanup().list().await.unwrap().len(),
        0
    );

    *credentials.fail_delete.lock().unwrap() = false;
    vault.delete_website(record.id).await.unwrap();
    assert!(repos.websites().get(record.id).await.unwrap().is_none());
    vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Second",
            "https://second.example",
            "alice",
            SecretString::from("new-secret"),
            "",
        ))
        .await
        .unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

struct BarrierCredentials {
    references: Mutex<Vec<String>>,
    deleted: Mutex<Vec<String>>,
    block_next_put: AtomicBool,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

#[async_trait]
impl CredentialStore for BarrierCredentials {
    async fn put(&self, reference: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        self.references
            .lock()
            .unwrap()
            .push(reference.as_str().into());
        if self.block_next_put.swap(false, Ordering::SeqCst) {
            self.entered.wait().await;
            self.release.wait().await;
        }
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        unreachable!()
    }
    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
        self.deleted.lock().unwrap().push(reference.as_str().into());
        Ok(())
    }
}

#[tokio::test]
async fn separate_vault_services_serialize_update_and_delete_without_orphaning_secrets() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let credentials = Arc::new(BarrierCredentials {
        references: Mutex::new(Vec::new()),
        deleted: Mutex::new(Vec::new()),
        block_next_put: AtomicBool::new(false),
        entered: entered.clone(),
        release: release.clone(),
    });
    let updater = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    let deleter = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    let original = updater
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("first"),
            "",
        ))
        .await
        .unwrap();

    credentials.block_next_put.store(true, Ordering::SeqCst);
    let update = tokio::spawn({
        let updater = updater.clone();
        async move {
            updater
                .save_website(autologin_core::SaveWebsite::new(
                    Some(original.id),
                    "Example",
                    "https://example.com",
                    "alice",
                    SecretString::from("replacement"),
                    "",
                ))
                .await
        }
    });
    entered.wait().await;
    let delete = tokio::spawn(async move { deleter.delete_website(original.id).await });
    release.wait().await;

    update.await.unwrap().unwrap();
    delete.await.unwrap().unwrap();
    assert!(repos.websites().get(original.id).await.unwrap().is_none());
    assert_eq!(
        *credentials.deleted.lock().unwrap(),
        *credentials.references.lock().unwrap()
    );
}

#[tokio::test]
async fn failed_metadata_compensation_is_durable_until_keychain_delete_succeeds() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(TrackingCredentials::default());
    let vault = autologin_core::VaultService::new(repos.websites(), credentials.clone());
    vault
        .save_website(autologin_core::SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("first"),
            "",
        ))
        .await
        .unwrap();

    *credentials.fail_delete.lock().unwrap() = true;
    assert_eq!(
        vault
            .save_website(autologin_core::SaveWebsite::new(
                None,
                "Duplicate",
                "https://example.com",
                "alice",
                SecretString::from("second"),
                "",
            ))
            .await
            .unwrap_err()
            .code(),
        "credential.compensation_failed"
    );
    let pending = repos.pending_secret_cleanup().list().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].secret_ref.as_str(),
        credentials.references.lock().unwrap()[1]
    );

    *credentials.fail_delete.lock().unwrap() = false;
    vault.retry_pending_secret_cleanup().await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}
