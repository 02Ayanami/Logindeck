use async_trait::async_trait;
use autologin_core::{
    AppError, Clipboard, ClipboardSession, CredentialStore, SaveWebsite, SecretRef,
    SqliteRepositories, VaultService,
};
use secrecy::SecretString;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

async fn remove_database_file(path: &Path) {
    #[cfg(windows)]
    for attempt in 0..10 {
        match std::fs::remove_file(path) {
            Ok(()) => return,
            Err(error) if error.raw_os_error() == Some(32) && attempt < 9 => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(error) => panic!("failed to remove SQLite test database: {error}"),
        }
    }

    #[cfg(not(windows))]
    std::fs::remove_file(path).unwrap();
}

#[derive(Default)]
struct Store {
    live: Mutex<Vec<SecretRef>>,
    entered: Notify,
    pause: bool,
}
#[async_trait]
impl CredentialStore for Store {
    async fn put(&self, reference: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        self.live.lock().unwrap().push(reference.clone());
        self.entered.notify_one();
        if self.pause {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        panic!("metadata edits must not read passwords")
    }
    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
        self.live.lock().unwrap().retain(|r| r != reference);
        Ok(())
    }
}
fn draft(id: Option<autologin_core::WebsiteId>, password: Option<SecretString>) -> SaveWebsite {
    SaveWebsite::new(
        id,
        "Example",
        "https://example.com",
        "alice",
        password,
        "updated notes",
    )
}

#[tokio::test]
async fn interrupted_keychain_write_is_recovered_after_database_reopen() {
    let path = std::env::temp_dir().join(format!(
        "autologin-recovery-{}.sqlite3",
        uuid::Uuid::new_v4()
    ));
    let url = format!("sqlite://{}", path.display());
    let store = Arc::new(Store {
        pause: true,
        ..Default::default()
    });
    {
        let repos = SqliteRepositories::connect(&url).await.unwrap();
        repos.migrate().await.unwrap();
        let vault = VaultService::new(repos.websites(), store.clone());
        let task = tokio::spawn(async move {
            vault
                .save_website(draft(None, Some(SecretString::from("fixture"))))
                .await
        });
        store.entered.notified().await;
        // The item already exists, but put has not returned and metadata has not been committed.
        let jobs = repos.pending_secret_cleanup().list().await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].secret_ref, store.live.lock().unwrap()[0]);
        assert!(repos.websites().list().await.unwrap().is_empty());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
    let repos = SqliteRepositories::connect(&url).await.unwrap();
    let vault = VaultService::new(repos.websites(), store.clone());
    vault.retry_pending_secret_cleanup().await.unwrap();
    assert!(store.live.lock().unwrap().is_empty());
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    drop(vault);
    drop(repos);
    remove_database_file(&path).await;
}

#[tokio::test]
async fn metadata_edit_keeps_secret_and_successful_replacement_retires_only_old_secret() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let store = Arc::new(Store::default());
    let vault = VaultService::new(repos.websites(), store.clone());
    let original = vault
        .save_website(draft(None, Some(SecretString::from("original"))))
        .await
        .unwrap();
    let edited = vault
        .save_website(draft(Some(original.id), None))
        .await
        .unwrap();
    assert_eq!(
        edited.password_credential_ref,
        original.password_credential_ref
    );
    assert_eq!(store.live.lock().unwrap().len(), 1);
    let replaced = vault
        .save_website(draft(
            Some(original.id),
            Some(SecretString::from("replacement")),
        ))
        .await
        .unwrap();
    assert_ne!(
        replaced.password_credential_ref,
        original.password_credential_ref
    );
    vault.retry_pending_secret_cleanup().await.unwrap();
    assert_eq!(
        *store.live.lock().unwrap(),
        vec![replaced.password_credential_ref]
    );
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn missing_empty_and_oversized_new_passwords_are_rejected_before_keychain_write() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let store = Arc::new(Store::default());
    let vault = VaultService::new(repos.websites(), store.clone());
    for password in [
        None,
        Some(SecretString::from("")),
        Some(SecretString::from("x".repeat(16385))),
    ] {
        assert_eq!(
            vault
                .save_website(draft(None, password))
                .await
                .unwrap_err()
                .code(),
            "validation.invalid_field"
        );
    }
    assert!(store.live.lock().unwrap().is_empty());
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

#[derive(Clone, Default)]
struct Board(Arc<Mutex<(u64, bool)>>);
#[async_trait]
impl Clipboard for Board {
    type ChangeToken = u64;
    async fn write(&self, _: SecretString) -> Result<u64, AppError> {
        let mut state = self.0.lock().unwrap();
        state.0 += 1;
        state.1 = true;
        Ok(state.0)
    }
    async fn clear_if_unchanged(&self, token: &u64) -> Result<(), AppError> {
        let mut state = self.0.lock().unwrap();
        if state.0 == *token {
            state.1 = false;
        }
        Ok(())
    }
}
#[tokio::test]
async fn exit_clears_own_password_and_rejects_late_writes() {
    let board = Board::default();
    let session = ClipboardSession::new(board.clone());
    session.write(SecretString::from("fixture")).await.unwrap();
    session.shutdown().await.unwrap();
    assert!(!board.0.lock().unwrap().1);
    assert!(session.write(SecretString::from("late")).await.is_err());
}
#[tokio::test]
async fn old_timer_and_exit_do_not_clear_new_user_content() {
    let board = Board::default();
    let session = ClipboardSession::new(board.clone());
    let old = session.write(SecretString::from("old")).await.unwrap();
    session.write(SecretString::from("new")).await.unwrap();
    session.clear_if_unchanged(&old).await.unwrap();
    assert!(board.0.lock().unwrap().1);
    board.0.lock().unwrap().0 += 1; // user copied something outside this session
    session.shutdown().await.unwrap();
    assert!(board.0.lock().unwrap().1);
}
