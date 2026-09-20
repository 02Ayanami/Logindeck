use autologin_core::{
    AppError, CredentialStore, SaveWebsite, SecretRef, SqliteRepositories, VaultService,
};
use secrecy::SecretString;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
struct Store {
    fail_delete: AtomicBool,
}

#[tokio::test]
async fn consecutive_browser_updates_have_distinct_change_markers() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let vault = VaultService::new(repos.websites(), Arc::new(Store::default()));
    let mut previous = String::new();
    for index in 0..10 {
        vault
            .capture_confirmed(
                &format!("rapid-{index}"),
                "https://example.com".into(),
                "alice".into(),
                SecretString::from("fixture"),
            )
            .await
            .unwrap();
        let records = vault.list_websites().await.unwrap();
        assert_eq!(records.len(), 1);
        assert_ne!(records[0].updated_at, previous);
        previous = records[0].updated_at.clone();
    }
}
#[async_trait::async_trait]
impl CredentialStore for Store {
    async fn put(&self, _: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        panic!("unused")
    }
    async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            Err(AppError::new("credential.unavailable"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn manual_rename_is_not_a_browser_password_update() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let vault = VaultService::new(repos.websites(), Arc::new(Store::default()));
    vault
        .capture_confirmed(
            "create",
            "https://example.com".into(),
            "alice".into(),
            SecretString::from("fixture"),
        )
        .await
        .unwrap();
    let first = vault.list_websites().await.unwrap().remove(0);
    let mut edit = SaveWebsite::new(
        Some(first.id),
        "Example",
        "https://example.com",
        "alice",
        None::<SecretString>,
        "",
    );
    edit.account_name = Some("Personal".into());
    let renamed = vault.save_website(edit).await.unwrap();
    assert_eq!(
        renamed.capture_source,
        autologin_core::CaptureSource::Manual
    );
    assert_eq!(
        renamed.password_credential_ref,
        first.password_credential_ref
    );
    vault
        .capture_confirmed(
            "update",
            "https://example.com".into(),
            "alice".into(),
            SecretString::from("next"),
        )
        .await
        .unwrap();
    let updated = vault.list_websites().await.unwrap().remove(0);
    assert_eq!(updated.account_name, "Personal");
    assert_eq!(
        updated.capture_source,
        autologin_core::CaptureSource::BrowserExtension
    );
}

// Regressions found during the full code review.
#[tokio::test]
async fn failed_save_preserves_the_displayed_default_number() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let vault = VaultService::new(repos.websites(), Arc::new(Store::default()));
    assert_eq!(
        vault
            .next_account_number("website:https://example.com")
            .await
            .unwrap(),
        1
    );
    assert!(vault
        .save_website(SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            None::<SecretString>,
            ""
        ))
        .await
        .is_err());
    assert!(vault.list_websites().await.unwrap().is_empty());
    assert_eq!(
        vault
            .next_account_number("website:https://example.com")
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn cleanup_failure_preserves_success_and_durable_recovery() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let store = Arc::new(Store::default());
    let vault = VaultService::new(repos.websites(), store.clone());
    let first = vault
        .save_website(SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("old"),
            "",
        ))
        .await
        .unwrap();
    store.fail_delete.store(true, Ordering::SeqCst);
    let receipt = vault
        .capture_confirmed(
            "review-request",
            "https://example.com".into(),
            "alice".into(),
            SecretString::from("new"),
        )
        .await
        .unwrap();
    assert_eq!(
        receipt,
        vault
            .capture_confirmed(
                "review-request",
                "https://example.com".into(),
                "alice".into(),
                SecretString::from("new")
            )
            .await
            .unwrap()
    );
    let current = vault.list_websites().await.unwrap().remove(0);
    assert_eq!(current.id, first.id);
    assert_ne!(
        current.password_credential_ref,
        first.password_credential_ref
    );
    assert!(repos
        .websites()
        .capture_receipt("review-request")
        .await
        .unwrap()
        .is_some());
    assert!(!repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    store.fail_delete.store(false, Ordering::SeqCst);
    vault.retry_pending_secret_cleanup().await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}
