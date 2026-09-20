use crate::sqlite::{map_error, RepositoryState};
use crate::{
    AppError, ApplicationAccount, ApplicationAccountId, ApplicationId, ApplicationRecord,
    CaptureSource, DiscoverySource, Locale, LoginTemplateId, LoginTemplateRecord,
    PendingSecretCleanup, Platform, PreferenceKey, PreferenceValue, SecretCleanupKind, SecretRef,
    SettingRecord, WebsiteId, WebsiteRecord,
};
use sqlx::{sqlite::SqliteRow, Pool, Row, Sqlite};
use std::sync::Arc;
use uuid::Uuid;
#[derive(Clone)]
pub struct WebsitesRepository {
    pub(crate) state: Arc<RepositoryState>,
}
#[derive(Clone)]
pub struct ApplicationsRepository {
    pub(crate) state: Arc<RepositoryState>,
}
#[derive(Clone)]
pub struct ApplicationAccountsRepository {
    pub(crate) state: Arc<RepositoryState>,
}
#[derive(Clone)]
pub struct LoginTemplatesRepository {
    pub(crate) state: Arc<RepositoryState>,
}
#[derive(Clone)]
pub struct SettingsRepository {
    pub(crate) state: Arc<RepositoryState>,
}
#[derive(Clone)]
pub struct PendingSecretCleanupRepository {
    pub(crate) state: Arc<RepositoryState>,
}
fn changed(n: u64) -> Result<(), AppError> {
    if n == 0 {
        Err(AppError::new("storage.not_found"))
    } else {
        Ok(())
    }
}
fn text(r: &SqliteRow, c: &str) -> Result<String, AppError> {
    r.try_get(c).map_err(map_error)
}
fn opt(r: &SqliteRow, c: &str) -> Result<Option<String>, AppError> {
    r.try_get(c).map_err(map_error)
}
fn uuid(r: &SqliteRow, c: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(&text(r, c)?).map_err(|_| AppError::new("storage.invalid_data"))
}
fn b(r: &SqliteRow, c: &str) -> Result<bool, AppError> {
    r.try_get(c).map_err(map_error)
}
fn web(r: SqliteRow) -> Result<WebsiteRecord, AppError> {
    Ok(WebsiteRecord {
        account_name: text(&r, "account_name")?,
        id: WebsiteId::from_uuid(uuid(&r, "id")?),
        name: text(&r, "name")?,
        url: text(&r, "url")?,
        normalized_origin: text(&r, "normalized_origin")?,
        username: text(&r, "username")?,
        password_credential_ref: SecretRef::new(text(&r, "password_credential_ref")?)
            .map_err(|_| AppError::new("storage.invalid_data"))?,
        capture_source: match text(&r, "capture_source")?.as_str() {
            "manual" => CaptureSource::Manual,
            "browser_extension" => CaptureSource::BrowserExtension,
            _ => return Err(AppError::new("storage.invalid_data")),
        },
        notes: text(&r, "notes")?,
        created_at: text(&r, "created_at")?,
        updated_at: text(&r, "updated_at")?,
    })
}
fn app(r: SqliteRow) -> Result<ApplicationRecord, AppError> {
    let alternate_launch_targets: Vec<String> =
        serde_json::from_str(&text(&r, "alternate_launch_targets")?)
            .map_err(|_| AppError::new("storage.invalid_data"))?;
    crate::validate_launch_targets(&alternate_launch_targets)
        .map_err(|_| AppError::new("storage.invalid_data"))?;
    let display_name = text(&r, "display_name")?;
    let platform_application_id = text(&r, "platform_application_id")?;
    let launch_target = text(&r, "launch_target")?;
    let path_access_ref = opt(&r, "path_access_ref")?;
    let version = opt(&r, "version")?;
    let signature_identity = text(&r, "signature_identity")?;
    crate::validate_application_metadata(
        &display_name,
        &platform_application_id,
        &launch_target,
        &alternate_launch_targets,
        version.as_deref(),
        &signature_identity,
        path_access_ref.as_deref(),
    )
    .map_err(|_| AppError::new("storage.invalid_data"))?;
    Ok(ApplicationRecord {
        id: ApplicationId::from_uuid(uuid(&r, "id")?),
        platform: match text(&r, "platform")?.as_str() {
            "macos" => Platform::Macos,
            "windows" => Platform::Windows,
            _ => return Err(AppError::new("storage.invalid_data")),
        },
        display_name,
        platform_application_id,
        launch_target,
        alternate_launch_targets,
        path_access_ref,
        version,
        signature_identity,
        discovery_source: match text(&r, "discovery_source")?.as_str() {
            "automatic" => DiscoverySource::Automatic,
            "manual_import" => DiscoverySource::ManualImport,
            _ => return Err(AppError::new("storage.invalid_data")),
        },
        is_present: b(&r, "is_present")?,
        is_hidden: b(&r, "is_hidden")?,
        icon_cache_ref: opt(&r, "icon_cache_ref")?,
        last_discovered_at: text(&r, "last_discovered_at")?,
        created_at: text(&r, "created_at")?,
        updated_at: text(&r, "updated_at")?,
    })
}
fn validate_application_record(x: &ApplicationRecord) -> Result<(), AppError> {
    crate::validate_application_metadata(
        &x.display_name,
        &x.platform_application_id,
        &x.launch_target,
        &x.alternate_launch_targets,
        x.version.as_deref(),
        &x.signature_identity,
        x.path_access_ref.as_deref(),
    )
}
fn account(r: SqliteRow) -> Result<ApplicationAccount, AppError> {
    Ok(ApplicationAccount {
        login_method: match text(&r, "login_method")?.as_str() {
            "password" => crate::LoginMethod::Password,
            "sms" => crate::LoginMethod::Sms,
            "qr" => crate::LoginMethod::Qr,
            _ => return Err(AppError::new("storage.invalid_data")),
        },
        phone: opt(&r, "phone")?,
        id: ApplicationAccountId::from_uuid(uuid(&r, "id")?),
        application_id: ApplicationId::from_uuid(uuid(&r, "application_id")?),
        display_name: text(&r, "display_name")?,
        username: text(&r, "username")?,
        password_credential_ref: opt(&r, "password_credential_ref")?
            .map(SecretRef::new)
            .transpose()
            .map_err(|_| AppError::new("storage.invalid_data"))?,
        auto_submit_enabled: b(&r, "auto_submit_enabled")?,
        last_login_status: opt(&r, "last_login_status")?,
        last_login_at: opt(&r, "last_login_at")?,
        created_at: text(&r, "created_at")?,
        updated_at: text(&r, "updated_at")?,
    })
}
fn template(r: SqliteRow) -> Result<LoginTemplateRecord, AppError> {
    Ok(LoginTemplateRecord {
        id: LoginTemplateId::from_uuid(uuid(&r, "id")?),
        application_id: ApplicationId::from_uuid(uuid(&r, "application_id")?),
        window_signature: text(&r, "window_signature")?,
        window_size_class: text(&r, "window_size_class")?,
        username_rect: text(&r, "username_rect")?,
        password_rect: text(&r, "password_rect")?,
        submit_rect: text(&r, "submit_rect")?,
        provider_id: text(&r, "provider_id")?,
        model_name: text(&r, "model_name")?,
        confidence_summary: text(&r, "confidence_summary")?,
        user_confirmed_at: opt(&r, "user_confirmed_at")?,
        created_at: text(&r, "created_at")?,
        updated_at: text(&r, "updated_at")?,
    })
}
impl WebsitesRepository {
    pub async fn begin_deletion(&self, kind: &str, id: &str) -> Result<(), AppError> {
        sqlx::query("INSERT OR IGNORE INTO pending_deletions(kind,owner_id) VALUES(?,?)")
            .bind(kind)
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(())
    }
    pub async fn cancel_deletion(&self, kind: &str, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM pending_deletions WHERE kind=? AND owner_id=?")
            .bind(kind)
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(())
    }
    pub async fn pending_deletions(&self) -> Result<Vec<(String, String)>, AppError> {
        sqlx::query_as("SELECT kind,owner_id FROM pending_deletions")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)
    }
    pub async fn finish_deletion(&self, kind: &str, id: &str) -> Result<(), AppError> {
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        match kind {
            "website" => {
                sqlx::query("DELETE FROM websites WHERE id=?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_error)?;
            }
            "account" => {
                sqlx::query("DELETE FROM application_accounts WHERE id=?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_error)?;
            }
            "application" => {
                sqlx::query("DELETE FROM login_templates WHERE application_id=?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_error)?;
                sqlx::query("DELETE FROM application_accounts WHERE application_id=?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_error)?;
                sqlx::query("DELETE FROM applications WHERE id=?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_error)?;
            }
            _ => return Err(AppError::new("storage.invalid_data")),
        }
        sqlx::query("DELETE FROM pending_deletions WHERE kind=? AND owner_id=?")
            .bind(kind)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        tx.commit().await.map_err(map_error)
    }
    pub async fn capture_receipt(&self, id: &str) -> Result<Option<String>, AppError> {
        sqlx::query_scalar("SELECT response FROM capture_receipts WHERE request_id=?")
            .bind(id)
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)
    }
    pub async fn store_capture_receipt(&self, id: &str, response: &str) -> Result<(), AppError> {
        sqlx::query("INSERT INTO capture_receipts(request_id,response,created_at) VALUES(?,?,strftime('%s','now'))")
            .bind(id).bind(response).execute(&self.state.pool).await.map_err(map_error)?;
        Ok(())
    }
    pub async fn next_account_number(&self, scope: &str, reserve: bool) -> Result<i64, AppError> {
        if reserve {
            return sqlx::query_scalar("INSERT INTO account_sequences(scope,last_number) VALUES(?,1) ON CONFLICT(scope) DO UPDATE SET last_number=last_number+1 RETURNING last_number")
                .bind(scope).fetch_one(&self.state.pool).await.map_err(map_error);
        }
        let previous: Option<i64> =
            sqlx::query_scalar("SELECT last_number FROM account_sequences WHERE scope=?")
                .bind(scope)
                .fetch_optional(&self.state.pool)
                .await
                .map_err(map_error)?;
        let labels: Vec<String> = if let Some(origin) = scope.strip_prefix("website:") {
            sqlx::query_scalar("SELECT account_name FROM websites WHERE normalized_origin=?")
                .bind(origin)
                .fetch_all(&self.state.pool)
                .await
                .map_err(map_error)?
        } else if let Some(id) = scope.strip_prefix("application:") {
            sqlx::query_scalar(
                "SELECT display_name FROM application_accounts WHERE application_id=?",
            )
            .bind(id)
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
        } else {
            Vec::new()
        };
        let highest = labels
            .iter()
            .filter_map(|label| {
                label
                    .strip_prefix("账号 ")
                    .or_else(|| label.strip_prefix("Account "))
            })
            .filter_map(|number| number.parse::<i64>().ok())
            .max()
            .unwrap_or(0);
        previous
            .unwrap_or(0)
            .max(highest)
            .checked_add(1)
            .ok_or_else(|| AppError::new("storage.invalid_data"))
    }

    pub async fn edit_group(
        &self,
        origin: &str,
        name: &str,
        url: &str,
        new_origin: &str,
    ) -> Result<(), AppError> {
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        if origin != new_origin {
            let existing: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM websites WHERE normalized_origin=?")
                    .bind(new_origin)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(map_error)?;
            if existing > 0 {
                return Err(AppError::new("storage.conflict"));
            }
        }
        sqlx::query(
            "UPDATE websites SET name=?,url=?,normalized_origin=? WHERE normalized_origin=?",
        )
        .bind(name)
        .bind(url)
        .bind(new_origin)
        .bind(origin)
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
        sqlx::query("INSERT INTO account_sequences(scope,last_number) SELECT ?,last_number FROM account_sequences WHERE scope=? ON CONFLICT(scope) DO UPDATE SET last_number=MAX(last_number,excluded.last_number)")
            .bind(format!("website:{new_origin}")).bind(format!("website:{origin}")).execute(&mut *tx).await.map_err(map_error)?;
        tx.commit().await.map_err(map_error)
    }
    pub async fn insert(&self, x: WebsiteRecord) -> Result<WebsiteRecord, AppError> {
        sqlx::query("INSERT INTO websites (id,name,url,normalized_origin,username,password_credential_ref,capture_source,notes,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(x.id.as_uuid().to_string())
            .bind(&x.name)
            .bind(&x.url)
            .bind(&x.normalized_origin)
            .bind(&x.username)
            .bind(x.password_credential_ref.as_str())
            .bind(match x.capture_source {
                CaptureSource::Manual => "manual",
                CaptureSource::BrowserExtension => "browser_extension",
            })
            .bind(&x.notes)
            .bind(&x.created_at)
            .bind(&x.updated_at)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(x)
    }
    pub async fn get(&self, id: WebsiteId) -> Result<Option<WebsiteRecord>, AppError> {
        sqlx::query("SELECT * FROM websites WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)?
            .map(web)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<WebsiteRecord>, AppError> {
        sqlx::query("SELECT * FROM websites ORDER BY created_at,id")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(web)
            .collect()
    }
    pub async fn update(&self, x: WebsiteRecord) -> Result<WebsiteRecord, AppError> {
        let r=sqlx::query("UPDATE websites SET name=?,url=?,normalized_origin=?,username=?,password_credential_ref=?,capture_source=?,notes=?,created_at=?,updated_at=? WHERE id=?").bind(&x.name).bind(&x.url).bind(&x.normalized_origin).bind(&x.username).bind(x.password_credential_ref.as_str()).bind(match x.capture_source{CaptureSource::Manual=>"manual",CaptureSource::BrowserExtension=>"browser_extension"}).bind(&x.notes).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&self.state.pool).await.map_err(map_error)?;
        changed(r.rows_affected())?;
        Ok(x)
    }
    pub async fn delete(&self, id: WebsiteId) -> Result<Option<WebsiteRecord>, AppError> {
        del(&self.state.pool, "websites", id.as_uuid().to_string(), web).await
    }
}
async fn del<T, F>(p: &Pool<Sqlite>, table: &str, id: String, map: F) -> Result<Option<T>, AppError>
where
    F: Fn(SqliteRow) -> Result<T, AppError>,
{
    let mut tx = p.begin().await.map_err(map_error)?;
    let q = format!("SELECT * FROM {table} WHERE id=?");
    let x = sqlx::query(&q)
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_error)?
        .map(map)
        .transpose()?;
    if x.is_some() {
        let q = format!("DELETE FROM {table} WHERE id=?");
        sqlx::query(&q)
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
    }
    tx.commit().await.map_err(map_error)?;
    Ok(x)
}
impl ApplicationsRepository {
    pub async fn insert(&self, x: ApplicationRecord) -> Result<ApplicationRecord, AppError> {
        validate_application_record(&x)?;
        sqlx::query("INSERT INTO applications (id,platform,display_name,platform_application_id,launch_target,alternate_launch_targets,path_access_ref,version,signature_identity,discovery_source,is_present,is_hidden,icon_cache_ref,last_discovered_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(x.id.as_uuid().to_string())
            .bind(match x.platform {
                Platform::Macos => "macos",
                Platform::Windows => "windows",
            })
            .bind(&x.display_name)
            .bind(&x.platform_application_id)
            .bind(&x.launch_target)
            .bind(serde_json::to_string(&x.alternate_launch_targets).map_err(|_| AppError::new("storage.invalid_data"))?)
            .bind(&x.path_access_ref)
            .bind(&x.version)
            .bind(&x.signature_identity)
            .bind(match x.discovery_source {
                DiscoverySource::Automatic => "automatic",
                DiscoverySource::ManualImport => "manual_import",
            })
            .bind(x.is_present)
            .bind(x.is_hidden)
            .bind(&x.icon_cache_ref)
            .bind(&x.last_discovered_at)
            .bind(&x.created_at)
            .bind(&x.updated_at)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(x)
    }
    pub async fn get(&self, id: ApplicationId) -> Result<Option<ApplicationRecord>, AppError> {
        sqlx::query("SELECT * FROM applications WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)?
            .map(app)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<ApplicationRecord>, AppError> {
        sqlx::query("SELECT * FROM applications ORDER BY created_at,id")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(app)
            .collect()
    }
    pub async fn update(&self, x: ApplicationRecord) -> Result<ApplicationRecord, AppError> {
        validate_application_record(&x)?;
        let r=sqlx::query("UPDATE applications SET platform=?,display_name=?,platform_application_id=?,launch_target=?,alternate_launch_targets=?,path_access_ref=?,version=?,signature_identity=?,discovery_source=?,is_present=?,is_hidden=?,icon_cache_ref=?,last_discovered_at=?,created_at=?,updated_at=? WHERE id=?").bind(match x.platform{Platform::Macos=>"macos", Platform::Windows=>"windows"}).bind(&x.display_name).bind(&x.platform_application_id).bind(&x.launch_target).bind(serde_json::to_string(&x.alternate_launch_targets).map_err(|_| AppError::new("storage.invalid_data"))?).bind(&x.path_access_ref).bind(&x.version).bind(&x.signature_identity).bind(match x.discovery_source{DiscoverySource::Automatic=>"automatic",DiscoverySource::ManualImport=>"manual_import"}).bind(x.is_present).bind(x.is_hidden).bind(&x.icon_cache_ref).bind(&x.last_discovered_at).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&self.state.pool).await.map_err(map_error)?;
        changed(r.rows_affected())?;
        Ok(x)
    }
    pub async fn delete(&self, id: ApplicationId) -> Result<Option<ApplicationRecord>, AppError> {
        del(
            &self.state.pool,
            "applications",
            id.as_uuid().to_string(),
            app,
        )
        .await
    }

    pub async fn delete_with_account_cleanup(
        &self,
        id: ApplicationId,
        now: &str,
    ) -> Result<Option<(ApplicationRecord, Vec<SecretRef>)>, AppError> {
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        let record = sqlx::query("SELECT * FROM applications WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_error)?
            .map(app)
            .transpose()?;
        let Some(record) = record else {
            tx.commit().await.map_err(map_error)?;
            return Ok(None);
        };
        let accounts = sqlx::query("SELECT * FROM application_accounts WHERE application_id=?")
            .bind(id.as_uuid().to_string())
            .fetch_all(&mut *tx)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(account)
            .collect::<Result<Vec<_>, _>>()?;
        let mut references = Vec::new();
        for account in accounts {
            if let Some(reference) = account.password_credential_ref {
                enqueue_cleanup_tx(
                    &mut tx,
                    &reference,
                    SecretCleanupKind::ApplicationAccount,
                    Some(account.id.as_uuid().to_string()),
                    "application_deleted",
                    now,
                )
                .await?;
                references.push(reference);
            }
        }
        sqlx::query("DELETE FROM login_templates WHERE application_id=?")
            .bind(id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        sqlx::query("DELETE FROM application_accounts WHERE application_id=?")
            .bind(id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        sqlx::query("DELETE FROM applications WHERE id=?")
            .bind(id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        tx.commit().await.map_err(map_error)?;
        Ok(Some((record, references)))
    }
}
impl ApplicationAccountsRepository {
    pub async fn insert(&self, x: ApplicationAccount) -> Result<ApplicationAccount, AppError> {
        sqlx::query("INSERT INTO application_accounts (id,application_id,display_name,username,login_method,phone,password_credential_ref,auto_submit_enabled,last_login_status,last_login_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(x.id.as_uuid().to_string())
            .bind(x.application_id.as_uuid().to_string())
            .bind(&x.display_name)
            .bind(&x.username).bind(x.login_method.as_str()).bind(&x.phone)
            .bind(x.password_credential_ref.as_ref().map(SecretRef::as_str))
            .bind(x.auto_submit_enabled)
            .bind(&x.last_login_status)
            .bind(&x.last_login_at)
            .bind(&x.created_at)
            .bind(&x.updated_at)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(x)
    }
    pub async fn get(
        &self,
        id: ApplicationAccountId,
    ) -> Result<Option<ApplicationAccount>, AppError> {
        sqlx::query("SELECT * FROM application_accounts WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)?
            .map(account)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<ApplicationAccount>, AppError> {
        sqlx::query("SELECT * FROM application_accounts ORDER BY created_at,id")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(account)
            .collect()
    }
    pub async fn list_for_application(
        &self,
        id: ApplicationId,
    ) -> Result<Vec<ApplicationAccount>, AppError> {
        sqlx::query(
            "SELECT * FROM application_accounts WHERE application_id=? ORDER BY created_at,id",
        )
        .bind(id.as_uuid().to_string())
        .fetch_all(&self.state.pool)
        .await
        .map_err(map_error)?
        .into_iter()
        .map(account)
        .collect()
    }
    pub async fn update(&self, x: ApplicationAccount) -> Result<ApplicationAccount, AppError> {
        let r=sqlx::query("UPDATE application_accounts SET application_id=?,display_name=?,username=?,login_method=?,phone=?,password_credential_ref=?,auto_submit_enabled=?,last_login_status=?,last_login_at=?,created_at=?,updated_at=? WHERE id=?").bind(x.application_id.as_uuid().to_string()).bind(&x.display_name).bind(&x.username).bind(x.login_method.as_str()).bind(&x.phone).bind(x.password_credential_ref.as_ref().map(SecretRef::as_str)).bind(x.auto_submit_enabled).bind(&x.last_login_status).bind(&x.last_login_at).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&self.state.pool).await.map_err(map_error)?;
        changed(r.rows_affected())?;
        Ok(x)
    }
    pub async fn delete(
        &self,
        id: ApplicationAccountId,
    ) -> Result<Option<ApplicationAccount>, AppError> {
        del(
            &self.state.pool,
            "application_accounts",
            id.as_uuid().to_string(),
            account,
        )
        .await
    }
}

impl LoginTemplatesRepository {
    pub async fn insert(&self, x: LoginTemplateRecord) -> Result<LoginTemplateRecord, AppError> {
        sqlx::query("INSERT INTO login_templates (id,application_id,window_signature,window_size_class,username_rect,password_rect,submit_rect,provider_id,model_name,confidence_summary,user_confirmed_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(x.id.as_uuid().to_string())
            .bind(x.application_id.as_uuid().to_string())
            .bind(&x.window_signature)
            .bind(&x.window_size_class)
            .bind(&x.username_rect)
            .bind(&x.password_rect)
            .bind(&x.submit_rect)
            .bind(&x.provider_id)
            .bind(&x.model_name)
            .bind(&x.confidence_summary)
            .bind(&x.user_confirmed_at)
            .bind(&x.created_at)
            .bind(&x.updated_at)
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(x)
    }
    pub async fn get(&self, id: LoginTemplateId) -> Result<Option<LoginTemplateRecord>, AppError> {
        sqlx::query("SELECT * FROM login_templates WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)?
            .map(template)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<LoginTemplateRecord>, AppError> {
        sqlx::query("SELECT * FROM login_templates ORDER BY created_at,id")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(template)
            .collect()
    }
    pub async fn list_for_application(
        &self,
        id: ApplicationId,
    ) -> Result<Vec<LoginTemplateRecord>, AppError> {
        sqlx::query("SELECT * FROM login_templates WHERE application_id=? ORDER BY created_at,id")
            .bind(id.as_uuid().to_string())
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(template)
            .collect()
    }
    pub async fn update(&self, x: LoginTemplateRecord) -> Result<LoginTemplateRecord, AppError> {
        let r=sqlx::query("UPDATE login_templates SET application_id=?,window_signature=?,window_size_class=?,username_rect=?,password_rect=?,submit_rect=?,provider_id=?,model_name=?,confidence_summary=?,user_confirmed_at=?,created_at=?,updated_at=? WHERE id=?").bind(x.application_id.as_uuid().to_string()).bind(&x.window_signature).bind(&x.window_size_class).bind(&x.username_rect).bind(&x.password_rect).bind(&x.submit_rect).bind(&x.provider_id).bind(&x.model_name).bind(&x.confidence_summary).bind(&x.user_confirmed_at).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&self.state.pool).await.map_err(map_error)?;
        changed(r.rows_affected())?;
        Ok(x)
    }
    pub async fn delete(
        &self,
        id: LoginTemplateId,
    ) -> Result<Option<LoginTemplateRecord>, AppError> {
        del(
            &self.state.pool,
            "login_templates",
            id.as_uuid().to_string(),
            template,
        )
        .await
    }
}
#[derive(Clone, serde::Serialize)]
pub struct BrowserCaptureSettings {
    pub enabled: bool,
    pub revision: i64,
    pub last_connected_at: Option<i64>,
}
impl SettingsRepository {
    pub async fn browser_capture(&self) -> Result<BrowserCaptureSettings, AppError> {
        let row = sqlx::query("SELECT enabled, revision, last_connected_at FROM browser_capture_settings WHERE id = 1")
            .fetch_one(&self.state.pool).await.map_err(map_error)?;
        Ok(BrowserCaptureSettings {
            enabled: row.try_get("enabled").map_err(map_error)?,
            revision: row.try_get("revision").map_err(map_error)?,
            last_connected_at: row.try_get("last_connected_at").map_err(map_error)?,
        })
    }
    pub async fn set_browser_capture(
        &self,
        enabled: bool,
    ) -> Result<BrowserCaptureSettings, AppError> {
        let _guard = self.state.vault_mutation.lock().await?;
        sqlx::query("UPDATE browser_capture_settings SET enabled = ?, revision = revision + 1 WHERE id = 1 AND enabled != ?")
            .bind(enabled).bind(enabled).execute(&self.state.pool).await.map_err(map_error)?;
        self.browser_capture().await
    }
    pub async fn note_browser_connection(&self) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE browser_capture_settings SET last_connected_at = unixepoch() WHERE id = 1",
        )
        .execute(&self.state.pool)
        .await
        .map_err(map_error)?;
        Ok(())
    }

    pub async fn get(&self, key: PreferenceKey) -> Result<Option<SettingRecord>, AppError> {
        sqlx::query("SELECT key, value FROM settings WHERE key = ?")
            .bind(preference_key_name(key))
            .fetch_optional(&self.state.pool)
            .await
            .map_err(map_error)?
            .map(setting)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<SettingRecord>, AppError> {
        sqlx::query("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(setting)
            .collect()
    }
    pub async fn set(&self, setting: SettingRecord) -> Result<SettingRecord, AppError> {
        let (key, value) = preference_parts(&setting)?;
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").bind(key).bind(value).execute(&self.state.pool).await.map_err(map_error)?;
        Ok(setting)
    }
    pub async fn delete(&self, key: PreferenceKey) -> Result<Option<SettingRecord>, AppError> {
        let mut transaction = self.state.pool.begin().await.map_err(map_error)?;
        let setting = sqlx::query("SELECT key, value FROM settings WHERE key = ?")
            .bind(preference_key_name(key))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_error)?
            .map(setting)
            .transpose()?;
        if setting.is_some() {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(preference_key_name(key))
                .execute(&mut *transaction)
                .await
                .map_err(map_error)?;
        }
        transaction.commit().await.map_err(map_error)?;
        Ok(setting)
    }
    pub async fn locale(&self) -> Result<Locale, AppError> {
        match self.get(PreferenceKey::UiLocale).await? {
            None => Ok(Locale::En),
            Some(SettingRecord {
                value: PreferenceValue::UiLocale(locale),
                ..
            }) => Ok(locale),
        }
    }
    pub async fn set_locale(&self, locale: Locale) -> Result<(), AppError> {
        self.set(SettingRecord::new(
            PreferenceKey::UiLocale,
            PreferenceValue::UiLocale(locale),
        ))
        .await?;
        Ok(())
    }
}
fn preference_key_name(key: PreferenceKey) -> &'static str {
    match key {
        PreferenceKey::UiLocale => "ui_locale",
    }
}
fn preference_parts(setting: &SettingRecord) -> Result<(&'static str, &'static str), AppError> {
    match (&setting.key, &setting.value) {
        (PreferenceKey::UiLocale, PreferenceValue::UiLocale(Locale::En)) => Ok(("ui_locale", "en")),
        (PreferenceKey::UiLocale, PreferenceValue::UiLocale(Locale::ZhCn)) => {
            Ok(("ui_locale", "zh-CN"))
        }
    }
}

fn setting(row: SqliteRow) -> Result<SettingRecord, AppError> {
    match (text(&row, "key")?.as_str(), text(&row, "value")?.as_str()) {
        ("ui_locale", "en") => Ok(SettingRecord::new(
            PreferenceKey::UiLocale,
            PreferenceValue::UiLocale(Locale::En),
        )),
        ("ui_locale", "zh-CN") => Ok(SettingRecord::new(
            PreferenceKey::UiLocale,
            PreferenceValue::UiLocale(Locale::ZhCn),
        )),
        _ => Err(AppError::new("storage.invalid_data")),
    }
}

fn cleanup(row: SqliteRow) -> Result<PendingSecretCleanup, AppError> {
    let kind = match text(&row, "kind")?.as_str() {
        "website" => SecretCleanupKind::Website,
        "application_account" => SecretCleanupKind::ApplicationAccount,
        _ => return Err(AppError::new("storage.invalid_data")),
    };
    Ok(PendingSecretCleanup {
        secret_ref: SecretRef::new(text(&row, "secret_ref")?)
            .map_err(|_| AppError::new("storage.invalid_data"))?,
        kind,
        owner_id: opt(&row, "owner_id")?,
        context: text(&row, "context")?,
        attempts: row.try_get::<i64, _>("attempts").map_err(map_error)? as u64,
        created_at: text(&row, "created_at")?,
        updated_at: text(&row, "updated_at")?,
    })
}

impl PendingSecretCleanupRepository {
    pub async fn list(&self) -> Result<Vec<PendingSecretCleanup>, AppError> {
        sqlx::query("SELECT * FROM pending_secret_cleanup ORDER BY created_at, secret_ref")
            .fetch_all(&self.state.pool)
            .await
            .map_err(map_error)?
            .into_iter()
            .map(cleanup)
            .collect()
    }
    pub async fn enqueue(
        &self,
        secret_ref: &SecretRef,
        kind: SecretCleanupKind,
        owner_id: Option<String>,
        context: &str,
        now: &str,
    ) -> Result<(), AppError> {
        sqlx::query("INSERT OR IGNORE INTO pending_secret_cleanup(secret_ref,kind,owner_id,context,attempts,created_at,updated_at) VALUES(?,?,?,?,0,?,?)")
            .bind(secret_ref.as_str()).bind(kind.as_str()).bind(owner_id).bind(context).bind(now).bind(now)
            .execute(&self.state.pool).await.map_err(map_error)?;
        Ok(())
    }
    pub async fn remove(&self, secret_ref: &SecretRef) -> Result<(), AppError> {
        sqlx::query("DELETE FROM pending_secret_cleanup WHERE secret_ref=?")
            .bind(secret_ref.as_str())
            .execute(&self.state.pool)
            .await
            .map_err(map_error)?;
        Ok(())
    }
    pub async fn record_failed_attempt(
        &self,
        secret_ref: &SecretRef,
        now: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE pending_secret_cleanup SET attempts=attempts+1,updated_at=? WHERE secret_ref=?",
        )
        .bind(now)
        .bind(secret_ref.as_str())
        .execute(&self.state.pool)
        .await
        .map_err(map_error)?;
        Ok(())
    }
}

async fn enqueue_cleanup_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    reference: &SecretRef,
    kind: SecretCleanupKind,
    owner_id: Option<String>,
    context: &str,
    now: &str,
) -> Result<(), AppError> {
    sqlx::query("INSERT OR IGNORE INTO pending_secret_cleanup(secret_ref,kind,owner_id,context,attempts,created_at,updated_at) VALUES(?,?,?,?,0,?,?)")
        .bind(reference.as_str()).bind(kind.as_str()).bind(owner_id).bind(context).bind(now).bind(now)
        .execute(&mut **tx).await.map_err(map_error)?;
    Ok(())
}

impl WebsitesRepository {
    pub fn pending_secret_cleanup(&self) -> PendingSecretCleanupRepository {
        PendingSecretCleanupRepository {
            state: self.state.clone(),
        }
    }
    pub async fn save_with_cleanup(
        &self,
        x: WebsiteRecord,
        previous: Option<&SecretRef>,
        now: &str,
        receipt: Option<(&str, &str)>,
    ) -> Result<WebsiteRecord, AppError> {
        let scope = format!("website:{}", x.normalized_origin);
        let next = if previous.is_none() {
            Some(self.next_account_number(&scope, false).await?)
        } else {
            None
        };
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        if let Some(next) = next {
            sqlx::query("INSERT INTO account_sequences(scope,last_number) VALUES(?,?) ON CONFLICT(scope) DO UPDATE SET last_number=MAX(last_number,excluded.last_number)")
                .bind(&scope).bind(next).execute(&mut *tx).await.map_err(map_error)?;
        }
        if let Some(previous) = previous {
            let result = sqlx::query("UPDATE websites SET name=?,url=?,normalized_origin=?,username=?,password_credential_ref=?,capture_source=?,notes=?,created_at=?,updated_at=? WHERE id=?")
                .bind(&x.name).bind(&x.url).bind(&x.normalized_origin).bind(&x.username).bind(x.password_credential_ref.as_str()).bind(match x.capture_source { CaptureSource::Manual => "manual", CaptureSource::BrowserExtension => "browser_extension" }).bind(&x.notes).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&mut *tx).await.map_err(map_error)?;
            changed(result.rows_affected())?;
            if previous != &x.password_credential_ref {
                enqueue_cleanup_tx(
                    &mut tx,
                    previous,
                    SecretCleanupKind::Website,
                    Some(x.id.as_uuid().to_string()),
                    "replaced",
                    now,
                )
                .await?;
            }
        } else {
            sqlx::query("INSERT INTO websites (id,name,url,normalized_origin,username,password_credential_ref,capture_source,notes,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
                .bind(x.id.as_uuid().to_string()).bind(&x.name).bind(&x.url).bind(&x.normalized_origin).bind(&x.username).bind(x.password_credential_ref.as_str()).bind(match x.capture_source { CaptureSource::Manual => "manual", CaptureSource::BrowserExtension => "browser_extension" }).bind(&x.notes).bind(&x.created_at).bind(&x.updated_at).execute(&mut *tx).await.map_err(map_error)?;
        }
        sqlx::query("DELETE FROM pending_secret_cleanup WHERE secret_ref=?")
            .bind(x.password_credential_ref.as_str())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        sqlx::query("UPDATE websites SET account_name=? WHERE id=?")
            .bind(&x.account_name)
            .bind(x.id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        if let Some((id, response)) = receipt {
            sqlx::query(
                "INSERT INTO capture_receipts(request_id,response,created_at) VALUES(?,?,?)",
            )
            .bind(id)
            .bind(response)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        Ok(x)
    }
    pub async fn delete_with_cleanup(
        &self,
        id: WebsiteId,
        now: &str,
    ) -> Result<Option<WebsiteRecord>, AppError> {
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        let record = sqlx::query("SELECT * FROM websites WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_error)?
            .map(web)
            .transpose()?;
        if let Some(ref value) = record {
            enqueue_cleanup_tx(
                &mut tx,
                &value.password_credential_ref,
                SecretCleanupKind::Website,
                Some(id.as_uuid().to_string()),
                "deleted",
                now,
            )
            .await?;
            sqlx::query("DELETE FROM websites WHERE id=?")
                .bind(id.as_uuid().to_string())
                .execute(&mut *tx)
                .await
                .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        Ok(record)
    }
}

impl ApplicationAccountsRepository {
    pub fn pending_secret_cleanup(&self) -> PendingSecretCleanupRepository {
        PendingSecretCleanupRepository {
            state: self.state.clone(),
        }
    }
    pub async fn save_with_cleanup(
        &self,
        x: ApplicationAccount,
        previous: Option<&ApplicationAccount>,
        now: &str,
    ) -> Result<ApplicationAccount, AppError> {
        let scope = format!("application:{}", x.application_id.as_uuid());
        let sequences = WebsitesRepository {
            state: self.state.clone(),
        };
        let next = if previous.is_none() {
            Some(sequences.next_account_number(&scope, false).await?)
        } else {
            None
        };
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        if let Some(next) = next {
            sqlx::query("INSERT INTO account_sequences(scope,last_number) VALUES(?,?) ON CONFLICT(scope) DO UPDATE SET last_number=MAX(last_number,excluded.last_number)")
                .bind(&scope).bind(next).execute(&mut *tx).await.map_err(map_error)?;
        }
        if let Some(previous) = previous {
            let result=sqlx::query("UPDATE application_accounts SET application_id=?,display_name=?,username=?,login_method=?,phone=?,password_credential_ref=?,auto_submit_enabled=?,last_login_status=?,last_login_at=?,created_at=?,updated_at=? WHERE id=?").bind(x.application_id.as_uuid().to_string()).bind(&x.display_name).bind(&x.username).bind(x.login_method.as_str()).bind(&x.phone).bind(x.password_credential_ref.as_ref().map(SecretRef::as_str)).bind(x.auto_submit_enabled).bind(&x.last_login_status).bind(&x.last_login_at).bind(&x.created_at).bind(&x.updated_at).bind(x.id.as_uuid().to_string()).execute(&mut *tx).await.map_err(map_error)?;
            changed(result.rows_affected())?;
            if let Some(reference) = previous
                .password_credential_ref
                .as_ref()
                .filter(|reference| Some(*reference) != x.password_credential_ref.as_ref())
            {
                enqueue_cleanup_tx(
                    &mut tx,
                    reference,
                    SecretCleanupKind::ApplicationAccount,
                    Some(x.id.as_uuid().to_string()),
                    "replaced",
                    now,
                )
                .await?;
            }
        } else {
            sqlx::query("INSERT INTO application_accounts (id,application_id,display_name,username,login_method,phone,password_credential_ref,auto_submit_enabled,last_login_status,last_login_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)").bind(x.id.as_uuid().to_string()).bind(x.application_id.as_uuid().to_string()).bind(&x.display_name).bind(&x.username).bind(x.login_method.as_str()).bind(&x.phone).bind(x.password_credential_ref.as_ref().map(SecretRef::as_str)).bind(x.auto_submit_enabled).bind(&x.last_login_status).bind(&x.last_login_at).bind(&x.created_at).bind(&x.updated_at).execute(&mut *tx).await.map_err(map_error)?;
        }
        sqlx::query("DELETE FROM pending_secret_cleanup WHERE secret_ref=?")
            .bind(x.password_credential_ref.as_ref().map(SecretRef::as_str))
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        tx.commit().await.map_err(map_error)?;
        Ok(x)
    }
    pub async fn delete_with_cleanup(
        &self,
        id: ApplicationAccountId,
        now: &str,
    ) -> Result<Option<ApplicationAccount>, AppError> {
        let mut tx = self.state.pool.begin().await.map_err(map_error)?;
        let record = sqlx::query("SELECT * FROM application_accounts WHERE id=?")
            .bind(id.as_uuid().to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_error)?
            .map(account)
            .transpose()?;
        if let Some(ref value) = record {
            if let Some(reference) = &value.password_credential_ref {
                enqueue_cleanup_tx(
                    &mut tx,
                    reference,
                    SecretCleanupKind::ApplicationAccount,
                    Some(id.as_uuid().to_string()),
                    "deleted",
                    now,
                )
                .await?;
            }
            sqlx::query("DELETE FROM application_accounts WHERE id=?")
                .bind(id.as_uuid().to_string())
                .execute(&mut *tx)
                .await
                .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        Ok(record)
    }
}

#[cfg(test)]
mod application_validation_tests {
    use super::*;

    fn application(launch_target: &str) -> ApplicationRecord {
        ApplicationRecord {
            id: ApplicationId::new(),
            platform: Platform::Macos,
            display_name: "Example".into(),
            platform_application_id: "com.example.app".into(),
            launch_target: launch_target.into(),
            alternate_launch_targets: vec![],
            path_access_ref: None,
            version: None,
            signature_identity: "signature".into(),
            discovery_source: DiscoverySource::Automatic,
            is_present: true,
            is_hidden: false,
            icon_cache_ref: None,
            last_discovered_at: "1".into(),
            created_at: "1".into(),
            updated_at: "1".into(),
        }
    }

    #[tokio::test]
    async fn application_row_loading_rejects_an_empty_primary_target() {
        let repos = crate::SqliteRepositories::connect("sqlite::memory:")
            .await
            .unwrap();
        repos.migrate().await.unwrap();
        let valid = repos
            .applications()
            .insert(application("/Applications/Example.app"))
            .await
            .unwrap();
        sqlx::query("UPDATE applications SET launch_target='' WHERE id=?")
            .bind(valid.id.as_uuid().to_string())
            .execute(&repos.state.pool)
            .await
            .unwrap();

        let error = repos.applications().get(valid.id).await.unwrap_err();
        assert_eq!(error.code(), "storage.invalid_data");
    }

    #[tokio::test]
    async fn application_repository_rejects_persisting_an_empty_primary_target() {
        let repos = crate::SqliteRepositories::connect("sqlite::memory:")
            .await
            .unwrap();
        repos.migrate().await.unwrap();

        let error = repos
            .applications()
            .insert(application(""))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "validation.invalid_field");
        assert!(repos.applications().list().await.unwrap().is_empty());
    }
}
