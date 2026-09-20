use crate::mutation_lock::MutationLock;
use crate::{
    AppError, ApplicationAccountsRepository, ApplicationsRepository, LoginTemplatesRepository,
    PendingSecretCleanupRepository, SettingsRepository, WebsitesRepository,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteConnection},
    Connection, Row, SqlitePool,
};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Clone)]
pub(crate) struct RepositoryState {
    pub(crate) pool: SqlitePool,
    /// One database owns one cross-store mutation lane. Repository-derived services must share it.
    pub(crate) vault_mutation: Arc<MutationLock>,
    // A named shared-cache in-memory database exists only while this connection remains open.
    _memory_keeper: Option<Arc<Mutex<SqliteConnection>>>,
}

#[derive(Clone)]
pub struct SqliteRepositories {
    pub(crate) state: Arc<RepositoryState>,
}
impl SqliteRepositories {
    pub async fn connect(url: &str) -> Result<Self, AppError> {
        Self::connect_inner(url, None).await
    }
    #[cfg(test)]
    pub(crate) async fn connect_with_aggressive_idle_timeout_for_test(
        url: &str,
        idle_timeout: Duration,
    ) -> Result<Self, AppError> {
        Self::connect_inner(url, Some(idle_timeout)).await
    }
    async fn connect_inner(url: &str, idle_timeout: Option<Duration>) -> Result<Self, AppError> {
        let memory_url = if is_memory_url(url) {
            Some(format!(
                "sqlite:file:autologin-memory-{}?mode=memory&cache=shared",
                Uuid::new_v4()
            ))
        } else {
            None
        };
        let database_url = memory_url.as_deref().unwrap_or(url);
        let opts = SqliteConnectOptions::from_str(database_url)
            .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))?
            .foreign_keys(true)
            .create_if_missing(true);
        let lock_path = if memory_url.is_some() {
            None
        } else {
            Some(opts.get_filename().with_extension("mutation.lock"))
        };
        let memory_keeper = match memory_url {
            Some(_) => Some(Arc::new(Mutex::new(
                SqliteConnection::connect_with(&opts)
                    .await
                    .map_err(map_error)?,
            ))),
            None => None,
        };
        let pool = memory_pool_options(idle_timeout)
            .connect_with(opts)
            .await
            .map_err(map_error)?;
        Ok(Self {
            state: Arc::new(RepositoryState {
                pool,
                vault_mutation: Arc::new(MutationLock::new(lock_path)),
                _memory_keeper: memory_keeper,
            }),
        })
    }
    pub async fn migrate(&self) -> Result<(), AppError> {
        let _guard = self.state.vault_mutation.lock().await?;
        MIGRATOR
            .run(&self.state.pool)
            .await
            .map_err(|_| AppError::new(AppError::STORAGE_DATABASE))
    }
    pub fn websites(&self) -> WebsitesRepository {
        WebsitesRepository {
            state: self.state.clone(),
        }
    }
    pub fn applications(&self) -> ApplicationsRepository {
        ApplicationsRepository {
            state: self.state.clone(),
        }
    }
    pub fn application_accounts(&self) -> ApplicationAccountsRepository {
        ApplicationAccountsRepository {
            state: self.state.clone(),
        }
    }
    pub fn login_templates(&self) -> LoginTemplatesRepository {
        LoginTemplatesRepository {
            state: self.state.clone(),
        }
    }
    pub fn settings(&self) -> SettingsRepository {
        SettingsRepository {
            state: self.state.clone(),
        }
    }
    pub fn pending_secret_cleanup(&self) -> PendingSecretCleanupRepository {
        PendingSecretCleanupRepository {
            state: self.state.clone(),
        }
    }
    pub async fn debug_column_names(&self, table: &str) -> Result<Vec<String>, AppError> {
        let query = match table {
            "websites" => "PRAGMA table_info(websites)",
            "applications" => "PRAGMA table_info(applications)",
            "application_accounts" => "PRAGMA table_info(application_accounts)",
            "login_templates" => "PRAGMA table_info(login_templates)",
            "settings" => "PRAGMA table_info(settings)",
            "pending_secret_cleanup" => "PRAGMA table_info(pending_secret_cleanup)",
            _ => return Err(AppError::invalid_field("table")),
        };
        sqlx::query(query)
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(|r| r.try_get("name").map_err(map_error))
            .collect()
    }
}
fn is_memory_url(url: &str) -> bool {
    matches!(url, "sqlite::memory:" | ":memory:")
}
fn memory_pool_options(idle_timeout: Option<Duration>) -> sqlx::sqlite::SqlitePoolOptions {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(idle_timeout)
        .max_lifetime(None)
}
pub(crate) fn map_error(e: sqlx::Error) -> AppError {
    use sqlx::error::ErrorKind;
    match &e {
        sqlx::Error::Database(d) => match d.kind() {
            ErrorKind::UniqueViolation => AppError::new(AppError::STORAGE_CONFLICT),
            ErrorKind::ForeignKeyViolation => {
                AppError::new(AppError::STORAGE_REFERENTIAL_INTEGRITY)
            }
            _ => AppError::new(AppError::STORAGE_DATABASE),
        },
        _ => AppError::new(AppError::STORAGE_DATABASE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_reopens_database_created_from_canonical_lf_migrations() {
        let directory = std::env::temp_dir().join(format!("logindeck-startup-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let url = format!("sqlite://{}", directory.join("autologin.sqlite3").display());
        let previous = SqliteRepositories::connect(&url).await.unwrap();
        // Simulate a previous build from Git's canonical LF migration bytes.
        let canonical = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(
                MIGRATOR
                    .iter()
                    .map(|migration| {
                        sqlx::migrate::Migration::new(
                            migration.version,
                            migration.description.clone(),
                            migration.migration_type,
                            migration.sql.replace("\r\n", "\n").into(),
                            migration.no_tx,
                        )
                    })
                    .collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        canonical.run(&previous.state.pool).await.unwrap();
        previous.settings().set_browser_capture(true).await.unwrap();
        previous.state.pool.close().await;
        drop(previous);

        let reopened = SqliteRepositories::connect(&url).await.unwrap();
        let startup = reopened.migrate().await;
        // Keep SQLx's underlying cause visible in a failing test, never in a client error.
        let diagnostic = if startup.is_err() {
            Some(MIGRATOR.run(&reopened.state.pool).await)
        } else {
            None
        };
        let saved_setting = reopened.settings().browser_capture().await.unwrap();
        reopened.state.pool.close().await;
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();

        assert!(
            startup.is_ok(),
            "startup: {startup:?}; SQLx: {diagnostic:?}"
        );
        assert!(saved_setting.enabled, "startup must preserve existing data");
    }

    #[tokio::test]
    async fn repository_handle_keeps_memory_database_alive_after_aggregate_drop() {
        let repos = SqliteRepositories::connect_with_aggressive_idle_timeout_for_test(
            "sqlite::memory:",
            Duration::from_millis(1),
        )
        .await
        .unwrap();
        repos.migrate().await.unwrap();
        let websites = repos.websites();
        drop(repos);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(websites.list().await.is_ok());
    }
}

#[cfg(test)]
mod login_method_migration_tests {
    use super::*;
    #[tokio::test]
    async fn existing_password_account_survives_login_method_migration() {
        let repos = SqliteRepositories::connect("sqlite::memory:")
            .await
            .unwrap();
        let mut old = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(
                MIGRATOR.iter().filter(|m| m.version < 5).cloned().collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        old.set_ignore_missing(true);
        old.run(&repos.state.pool).await.unwrap();
        let mut app = crate::ApplicationRecord::new(
            crate::Platform::Macos,
            "com.example.app".into(),
            "Fixture".into(),
        )
        .unwrap();
        app.launch_target = "/Applications/Fixture.app".into();
        let app = repos.applications().insert(app).await.unwrap();
        let id = crate::ApplicationAccountId::new();
        sqlx::query("INSERT INTO application_accounts VALUES (?,?,?,?,?,?,?,?,?,?)")
            .bind(id.as_uuid().to_string())
            .bind(app.id.as_uuid().to_string())
            .bind("Fixture")
            .bind("alice")
            .bind("fixture-reference")
            .bind(true)
            .bind("old-status")
            .bind("1")
            .bind("1")
            .bind("2")
            .execute(&repos.state.pool)
            .await
            .unwrap();
        repos.migrate().await.unwrap();
        repos.migrate().await.unwrap();
        let saved = repos.application_accounts().get(id).await.unwrap().unwrap();
        assert_eq!(saved.login_method, crate::LoginMethod::Password);
        assert_eq!(
            saved.password_credential_ref.unwrap().as_str(),
            "fixture-reference"
        );
        assert_eq!(saved.username, "alice");
        assert_eq!(saved.created_at, "1");
        assert!(saved.auto_submit_enabled);
        assert!(saved.phone.is_none());
        assert_eq!(saved.last_login_status.as_deref(), Some("old-status"));
    }
}
