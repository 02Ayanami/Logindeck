use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use autologin_core::{
    AppError, ApplicationCatalog, ApplicationsService, ClipboardSession, CredentialStore,
    SqliteRepositories, UserPresence, VaultService,
};
use tauri::{AppHandle, Manager};

use platform_runtime::{NativeApplicationCatalog, NativeClipboard, NativeCredentialStore};

pub struct AppState {
    // Kept open for the entire app lifetime. Only one GUI instance is permitted; core serializes native-host writes separately.
    _instance_lock: std::fs::File,
    pub vault: VaultService,
    pub scan: crate::scan::ScanManager,
    pub applications: ApplicationsService,
    pub settings: autologin_core::SettingsRepository,
    pub presence: Arc<SystemCredentialPresence>,
    pub clipboard: Arc<ClipboardSession<NativeClipboard>>,
}

/// The macOS login keychain enforces the current user-session boundary.
pub struct SystemCredentialPresence;
#[async_trait]
impl UserPresence for SystemCredentialPresence {
    async fn authenticate(&self, reason: &str) -> Result<(), AppError> {
        if reason.trim().is_empty() {
            Err(AppError::invalid_field("authentication_reason"))
        } else {
            Ok(())
        }
    }
}

impl AppState {
    pub async fn initialize(handle: &AppHandle) -> Result<Self, AppError> {
        let data_dir = handle
            .path()
            .app_data_dir()
            .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))?;
        let directory_to_create = data_dir.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(&directory_to_create))
            .await
            .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))?
            .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))?;
        Self::initialize_at(data_dir).await
    }

    pub async fn initialize_at(data_dir: PathBuf) -> Result<Self, AppError> {
        let lock_path = data_dir.join("desktop-instance.lock");
        let instance_lock = tokio::task::spawn_blocking(move || {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(lock_path)
                .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))?;
            file.try_lock()
                .map_err(|_| AppError::new("storage.in_use"))?;
            Ok::<_, AppError>(file)
        })
        .await
        .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))??;
        let database = data_dir.join("autologin.sqlite3");
        let url = format!("sqlite://{}", database.to_string_lossy());
        let repositories = SqliteRepositories::connect(&url).await?;
        repositories.migrate().await?;
        let credentials: Arc<dyn CredentialStore> =
            Arc::new(NativeCredentialStore::new("com.autologin.app".into()));
        let catalog: Arc<dyn ApplicationCatalog> = Arc::new(NativeApplicationCatalog::new());
        let vault = VaultService::new(repositories.websites(), credentials.clone());
        let applications = ApplicationsService::new(
            repositories.applications(),
            repositories.application_accounts(),
            catalog,
        )
        .with_credentials(credentials);
        // Both services share this queue and lock. Retry once at startup, even with no edits.
        // Failure leaves the job durable and does not prevent access to the vault.
        let _ = vault.retry_pending_secret_cleanup().await;
        let _ = vault.recover_deletions().await;
        let _ = applications.recover_deletions().await;
        Ok(Self {
            _instance_lock: instance_lock,
            vault,
            scan: crate::scan::ScanManager::default(),
            applications,
            settings: repositories.settings(),
            presence: Arc::new(SystemCredentialPresence),
            clipboard: Arc::new(ClipboardSession::new(NativeClipboard::new())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn only_one_app_instance_can_open_the_vault_and_lock_releases_on_drop() {
        let path = std::env::temp_dir().join(format!(
            "autologin-instance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir(&path).unwrap();
        tauri::async_runtime::block_on(async {
            let first = AppState::initialize_at(path.clone()).await.unwrap();
            assert!(!first.settings.browser_capture().await.unwrap().enabled);
            first.settings.set_browser_capture(true).await.unwrap();
            let second = AppState::initialize_at(path.clone()).await;
            assert!(matches!(second, Err(ref error) if error.code() == "storage.in_use"));
            drop(first);
            let reopened = AppState::initialize_at(path.clone()).await.unwrap();
            assert!(reopened.settings.browser_capture().await.unwrap().enabled);
            reopened.settings.set_browser_capture(false).await.unwrap();
            drop(reopened);
            let disabled = AppState::initialize_at(path.clone()).await.unwrap();
            assert!(!disabled.settings.browser_capture().await.unwrap().enabled);
            drop(disabled);
        });
        std::fs::remove_dir_all(path).unwrap();
    }
}
