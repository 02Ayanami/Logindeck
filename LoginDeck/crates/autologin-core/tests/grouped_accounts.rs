use autologin_core::{
    AppError, CredentialStore, SaveWebsite, SecretRef, SqliteRepositories, VaultService,
};
use secrecy::SecretString;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
#[derive(Default)]
struct Store {
    writes: AtomicUsize,
}
#[async_trait::async_trait]
impl CredentialStore for Store {
    async fn put(&self, _: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        panic!("Capture must not read passwords")
    }
    async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
        Ok(())
    }
}

#[tokio::test]
async fn interrupted_deletion_is_finished_on_recovery() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let vault = VaultService::new(repos.websites(), Arc::new(Store::default()));
    let saved = vault
        .save_website(SaveWebsite::new(
            None,
            "Example",
            "https://example.com",
            "alice",
            SecretString::from("one"),
            "",
        ))
        .await
        .unwrap();
    repos
        .websites()
        .begin_deletion("website", &saved.id.as_uuid().to_string())
        .await
        .unwrap();
    vault.recover_deletions().await.unwrap();
    assert!(vault.list_websites().await.unwrap().is_empty());
    assert!(repos
        .websites()
        .pending_deletions()
        .await
        .unwrap()
        .is_empty());
    vault.recover_deletions().await.unwrap();
}
#[tokio::test]
async fn capture_matches_real_accounts_preserves_remarks_and_replays_receipts() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let store = Arc::new(Store::default());
    let vault = VaultService::new(repos.websites(), store.clone());
    let first = vault
        .capture_confirmed(
            "request-1",
            "https://example.com".into(),
            "alice".into(),
            SecretString::from("one"),
        )
        .await
        .unwrap();
    let duplicate = vault
        .capture_confirmed(
            "request-1",
            "https://example.com".into(),
            "alice".into(),
            SecretString::from("one"),
        )
        .await
        .unwrap();
    assert_eq!(first, duplicate);
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    let saved = vault.list_websites().await.unwrap().remove(0);
    assert_eq!(saved.account_name, "Account 1");
    let mut edit = SaveWebsite::new(
        Some(saved.id),
        "Example",
        saved.url,
        saved.username,
        None::<SecretString>,
        "",
    );
    edit.account_name = Some("Personal".into());
    vault.save_website(edit).await.unwrap();
    let updated: serde_json::Value = serde_json::from_str(
        &vault
            .capture_confirmed(
                "request-2",
                "https://example.com".into(),
                "alice".into(),
                SecretString::from("two"),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(updated["action"], "updated_account");
    assert_eq!(updated["accountDisplayName"], "Personal");
    vault
        .capture_confirmed(
            "request-3",
            "https://example.com".into(),
            "bob".into(),
            SecretString::from("three"),
        )
        .await
        .unwrap();
    assert_eq!(vault.list_websites().await.unwrap().len(), 2);
    vault.delete_website(saved.id).await.unwrap();
    assert_eq!(
        vault
            .next_account_number("website:https://example.com")
            .await
            .unwrap(),
        3
    );
}
