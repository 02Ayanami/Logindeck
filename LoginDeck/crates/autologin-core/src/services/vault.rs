//! Password-bearing website vault operations.

use std::{fmt, sync::Arc, time::Duration};

use crate::mutation_lock::MutationLock;
use secrecy::SecretString;
use url::Url;

use crate::{
    AppError, CaptureSource, Clipboard, CredentialStore, PendingSecretCleanupRepository,
    SecretCleanupKind, SecretRef, UserPresence, WebsiteId, WebsiteRecord, WebsitesRepository,
};

const COMPENSATION_FAILED: &str = "credential.compensation_failed";
const COMPENSATION_TRACKING_FAILED: &str = "credential.compensation_tracking_failed";
const CLEANUP_REQUIRED: &str = "credential.cleanup_required";
const CLEANUP_TRACKING_FAILED: &str = "credential.cleanup_tracking_failed";

/// Input for a website save. The password deliberately has no serializable representation.
pub struct SaveWebsite {
    pub account_name: Option<String>,
    pub capture_request_id: Option<String>,
    pub id: Option<WebsiteId>,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: Option<SecretString>,
    pub notes: String,
}

impl SaveWebsite {
    pub fn new(
        id: Option<WebsiteId>,
        name: impl Into<String>,
        url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<Option<SecretString>>,
        notes: impl Into<String>,
    ) -> Self {
        Self {
            account_name: None,
            capture_request_id: None,
            id,
            name: name.into(),
            url: url.into(),
            username: username.into(),
            password: password.into(),
            notes: notes.into(),
        }
    }
}

impl fmt::Debug for SaveWebsite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveWebsite")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("notes", &self.notes)
            .finish()
    }
}

/// Website metadata and Keychain coordination. All services built from the same repositories
/// share the mutex, which covers lookup, credential write, metadata transaction and cleanup.
#[derive(Clone)]
pub struct VaultService {
    websites: WebsitesRepository,
    cleanup: PendingSecretCleanupRepository,
    credentials: Arc<dyn CredentialStore>,
    mutation: Arc<MutationLock>,
}

impl VaultService {
    pub async fn capture_confirmed(
        &self,
        request_id: &str,
        origin: String,
        username: String,
        password: SecretString,
    ) -> Result<String, AppError> {
        let _guard = self.mutation.lock().await?;
        if let Some(receipt) = self.websites.capture_receipt(request_id).await? {
            let _ = self.drain_pending_locked().await;
            return Ok(receipt);
        }
        let (_, origin) = validated_url(&origin)?;
        let username = username.trim().to_owned();
        let current = self.capture_target(&origin, &username).await?;
        let group = self
            .websites
            .list()
            .await?
            .into_iter()
            .find(|record| record.normalized_origin == origin);
        let name = group
            .as_ref()
            .map(|record| record.name.clone())
            .unwrap_or_else(|| Url::parse(&origin).unwrap().host_str().unwrap().to_owned());
        let url = group
            .as_ref()
            .map(|record| record.url.clone())
            .unwrap_or(origin);
        let notes = current
            .as_ref()
            .map(|record| record.notes.clone())
            .unwrap_or_default();
        let mut input = SaveWebsite::new(
            current.map(|record| record.id),
            name,
            url,
            username,
            password,
            notes,
        );
        input.capture_request_id = Some(request_id.to_owned());
        self.save_locked(input, Some(CaptureSource::BrowserExtension))
            .await?;
        self.websites
            .capture_receipt(request_id)
            .await?
            .ok_or_else(|| AppError::new("storage.database"))
    }
    pub async fn next_account_number(&self, scope: &str) -> Result<i64, AppError> {
        self.websites.next_account_number(scope, false).await
    }

    pub async fn edit_website_group(
        &self,
        origin: String,
        name: String,
        url: String,
    ) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        validate_text("name", &name)?;
        let (url, new_origin) = validated_url(&url)?;
        self.websites
            .edit_group(&origin, &name, &url, &new_origin)
            .await
    }
    pub fn new(websites: WebsitesRepository, credentials: Arc<dyn CredentialStore>) -> Self {
        let cleanup = websites.pending_secret_cleanup();
        let mutation = websites.state.vault_mutation.clone();
        Self {
            websites,
            cleanup,
            credentials,
            mutation,
        }
    }

    pub async fn pending_cleanup_count(&self) -> Result<usize, AppError> {
        Ok(self.cleanup.list().await?.len())
    }

    pub async fn list_websites(&self) -> Result<Vec<WebsiteRecord>, AppError> {
        self.websites.list().await
    }

    pub async fn save_website(&self, request: SaveWebsite) -> Result<WebsiteRecord, AppError> {
        let _guard = self.mutation.lock().await?;
        self.save_locked(request, None).await
    }

    pub async fn capture_target(
        &self,
        origin: &str,
        username: &str,
    ) -> Result<Option<WebsiteRecord>, AppError> {
        let (_, origin) = validated_url(origin)?;
        let matches: Vec<_> = self
            .websites
            .list()
            .await?
            .into_iter()
            .filter(|x| x.normalized_origin == origin && x.username == username)
            .collect();
        if matches.len() > 1 {
            return Err(AppError::new("storage.conflict"));
        }
        Ok(matches.into_iter().next())
    }

    pub async fn browser_capture_settings(
        &self,
    ) -> Result<crate::BrowserCaptureSettings, AppError> {
        crate::SettingsRepository {
            state: self.websites.state.clone(),
        }
        .browser_capture()
        .await
    }

    pub async fn save_browser_capture(
        &self,
        origin: String,
        username: String,
        password: SecretString,
        expected: Option<WebsiteRecord>,
        revision: i64,
    ) -> Result<WebsiteRecord, AppError> {
        let _guard = self.mutation.lock().await?;
        let config = self.browser_capture_settings().await?;
        if !config.enabled || config.revision != revision {
            return Err(AppError::new("capture.disabled"));
        }
        let (_, normalized) = validated_url(&origin)?;
        let current = self.capture_target(&normalized, &username).await?;
        let unchanged = match (&expected, &current) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.id == b.id
                    && a.password_credential_ref == b.password_credential_ref
                    && a.updated_at == b.updated_at
                    && a.name == b.name
                    && a.notes == b.notes
                    && a.url == b.url
            }
            _ => false,
        };
        if !unchanged {
            return Err(AppError::new("storage.conflict"));
        }
        let name = current
            .as_ref()
            .map(|x| x.name.clone())
            .unwrap_or_else(|| normalized.clone());
        let url = current
            .as_ref()
            .map(|x| x.url.clone())
            .unwrap_or(normalized);
        let notes = current
            .as_ref()
            .map(|x| x.notes.clone())
            .unwrap_or_default();
        self.save_locked(
            SaveWebsite::new(current.map(|x| x.id), name, url, username, password, notes),
            Some(CaptureSource::BrowserExtension),
        )
        .await
    }

    async fn save_locked(
        &self,
        mut request: SaveWebsite,
        source: Option<CaptureSource>,
    ) -> Result<WebsiteRecord, AppError> {
        // A failed retry does not prevent a new durable mutation; the job remains queued.
        let _ = self.drain_pending_locked().await;

        validate_text("name", &request.name)?;
        validate_text("username", &request.username)?;
        validate_text("notes", &request.notes)?;
        let (url, origin) = validated_url(&request.url)?;
        let url = if request.id.is_none() {
            if let Some(group) = self
                .websites
                .list()
                .await?
                .into_iter()
                .find(|record| record.normalized_origin == origin)
            {
                request.name = group.name;
                group.url
            } else {
                url
            }
        } else {
            url
        };
        let previous = match request.id {
            Some(id) => self.websites.get(id).await?,
            None => None,
        };
        if let Some(id) = request.id {
            if self
                .websites
                .pending_deletions()
                .await?
                .iter()
                .any(|(kind, owner)| kind == "website" && owner == &id.as_uuid().to_string())
            {
                return Err(AppError::new("storage.conflict"));
            }
        }
        if request.id.is_some() && previous.is_none() {
            return Err(AppError::new("storage.not_found"));
        }
        let number = if previous.is_none() {
            Some(
                self.websites
                    .next_account_number(&format!("website:{origin}"), false)
                    .await?,
            )
        } else {
            None
        };
        let locale = crate::SettingsRepository {
            state: self.websites.state.clone(),
        }
        .locale()
        .await?;
        let prefix = if locale == crate::Locale::ZhCn {
            "账号 "
        } else {
            "Account "
        };
        let account_name = request
            .account_name
            .filter(|name| !name.trim().is_empty())
            .or_else(|| previous.as_ref().map(|record| record.account_name.clone()))
            .unwrap_or_else(|| format!("{prefix}{}", number.unwrap_or(1)));
        validate_text("name", &account_name)?;

        let now = timestamp_after(previous.as_ref().map(|record| record.updated_at.as_str()));
        let reference = super::credential_write::prepare(
            &self.cleanup,
            self.credentials.as_ref(),
            previous.as_ref().map(|old| &old.password_credential_ref),
            request.password,
            &request.username,
            SecretCleanupKind::Website,
            &now,
        )
        .await?;
        let replaced = previous
            .as_ref()
            .is_none_or(|old| old.password_credential_ref != reference);
        let record = WebsiteRecord {
            account_name,
            id: request.id.unwrap_or_default(),
            name: request.name,
            url,
            normalized_origin: origin,
            username: request.username,
            password_credential_ref: reference.clone(),
            capture_source: source.unwrap_or(CaptureSource::Manual),
            notes: request.notes,
            created_at: previous
                .as_ref()
                .map_or_else(|| now.clone(), |old| old.created_at.clone()),
            updated_at: now.clone(),
        };

        let receipt = request.capture_request_id.as_ref().map(|id| (id.as_str(), serde_json::json!({
            "type":"account.saved", "action":if previous.is_some() { "updated_account" } else { "created_account" },
            "websiteName":record.name, "accountDisplayName":record.account_name, "accountId":record.id
        }).to_string()));
        match self
            .websites
            .save_with_cleanup(
                record,
                previous.as_ref().map(|old| &old.password_credential_ref),
                &now,
                receipt
                    .as_ref()
                    .map(|(id, response)| (*id, response.as_str())),
            )
            .await
        {
            Ok(saved) => {
                if let Some(old) = previous.filter(|_| replaced) {
                    // The new credential and metadata are already committed. Cleanup is
                    // durably queued; failure must not report the save itself as failed.
                    let _ = self
                        .finish_cleanup_locked(&old.password_credential_ref)
                        .await;
                }
                Ok(saved)
            }
            Err(error) => {
                if !replaced {
                    return Err(error);
                }
                self.compensate_new_locked(
                    &reference,
                    SecretCleanupKind::Website,
                    None,
                    "metadata_write_failed",
                    &now,
                )
                .await?;
                Err(error)
            }
        }
    }

    pub async fn delete_website(&self, id: WebsiteId) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        let _ = self.drain_pending_locked().await;
        let old = self
            .websites
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))?;
        let owner = id.as_uuid().to_string();
        self.websites.begin_deletion("website", &owner).await?;
        if let Err(error) = self.credentials.delete(&old.password_credential_ref).await {
            self.websites.cancel_deletion("website", &owner).await?;
            return Err(error);
        }
        self.websites.finish_deletion("website", &owner).await?;
        Ok(())
    }

    pub async fn recover_deletions(&self) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        for (kind, owner) in self.websites.pending_deletions().await? {
            if kind != "website" {
                continue;
            }
            let id = WebsiteId::from_uuid(
                uuid::Uuid::parse_str(&owner).map_err(|_| AppError::new("storage.invalid_data"))?,
            );
            if let Some(record) = self.websites.get(id).await? {
                self.credentials
                    .delete(&record.password_credential_ref)
                    .await?;
            }
            self.websites.finish_deletion(&kind, &owner).await?;
        }
        Ok(())
    }

    /// Retries every durable cleanup job. A job is acknowledged only after Keychain deletion.
    pub async fn retry_pending_secret_cleanup(&self) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        self.drain_pending_locked().await
    }

    pub async fn copy_website_username(&self, id: WebsiteId) -> Result<String, AppError> {
        self.websites
            .get(id)
            .await?
            .map(|record| record.username)
            .ok_or_else(|| AppError::new("storage.not_found"))
    }

    pub async fn copy_website_password<P, B>(
        &self,
        id: WebsiteId,
        presence: Arc<P>,
        clipboard: Arc<B>,
    ) -> Result<(), AppError>
    where
        P: UserPresence + 'static,
        B: Clipboard + 'static,
    {
        let record = self
            .websites
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))?;
        copy_password(
            self.credentials.clone(),
            record.password_credential_ref,
            presence,
            clipboard,
        )
        .await
    }

    async fn compensate_new_locked(
        &self,
        reference: &SecretRef,
        kind: SecretCleanupKind,
        owner: Option<String>,
        context: &str,
        now: &str,
    ) -> Result<(), AppError> {
        if self.credentials.delete(reference).await.is_ok() {
            return self
                .cleanup
                .remove(reference)
                .await
                .map_err(|_| AppError::new(COMPENSATION_TRACKING_FAILED));
        }
        self.cleanup
            .enqueue(reference, kind, owner, context, now)
            .await
            .map_err(|_| AppError::new(COMPENSATION_TRACKING_FAILED))?;
        Err(AppError::new(COMPENSATION_FAILED))
    }

    async fn finish_cleanup_locked(&self, reference: &SecretRef) -> Result<(), AppError> {
        match self.credentials.delete(reference).await {
            Ok(()) => self
                .cleanup
                .remove(reference)
                .await
                .map_err(|_| AppError::new(CLEANUP_TRACKING_FAILED)),
            Err(_) => {
                self.cleanup
                    .record_failed_attempt(reference, &timestamp())
                    .await
                    .map_err(|_| AppError::new(CLEANUP_TRACKING_FAILED))?;
                Err(AppError::new(CLEANUP_REQUIRED))
            }
        }
    }

    async fn drain_pending_locked(&self) -> Result<(), AppError> {
        let jobs = self
            .cleanup
            .list()
            .await
            .map_err(|_| AppError::new(CLEANUP_TRACKING_FAILED))?;
        let mut failed = false;
        for job in jobs {
            if self.finish_cleanup_locked(&job.secret_ref).await.is_err() {
                failed = true;
            }
        }
        if failed {
            Err(AppError::new(CLEANUP_REQUIRED))
        } else {
            Ok(())
        }
    }
}

/// Writes a revealed secret and schedules exactly one conditional 30-second cleanup. The task
/// captures only the opaque token, never the secret itself.
pub async fn copy_password<P, B>(
    credentials: Arc<dyn CredentialStore>,
    reference: SecretRef,
    presence: Arc<P>,
    clipboard: Arc<B>,
) -> Result<(), AppError>
where
    P: UserPresence + 'static,
    B: Clipboard + 'static,
{
    presence.authenticate("Copy password").await?;
    let secret = credentials.reveal(&reference, "Copy password").await?;
    let token = clipboard.write(secret).await?;
    let clear_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    tokio::spawn(async move {
        tokio::time::sleep_until(clear_deadline).await;
        for attempt in 0..3 {
            if clipboard.clear_if_unchanged(&token).await.is_ok() {
                return;
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        // The session retains the token for exit cleanup; never log clipboard contents.
        eprintln!("Password clipboard cleanup failed after three attempts");
    });
    Ok(())
}

fn validate_text(field: &'static str, value: &str) -> Result<(), AppError> {
    let max = match field {
        "name" => 256,
        "username" => 512,
        "notes" => 4000,
        _ => usize::MAX,
    };
    if (field != "notes" && value.trim().is_empty()) || value.len() > max {
        Err(AppError::invalid_field(field))
    } else {
        Ok(())
    }
}

fn validated_url(value: &str) -> Result<(String, String), AppError> {
    let url = Url::parse(value).map_err(|_| AppError::invalid_field("url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.cannot_be_a_base()
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::invalid_field("url"));
    }
    let serialized = url.to_string();
    if serialized.len() > 2048 {
        return Err(AppError::invalid_field("url"));
    }
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        return Err(AppError::invalid_field("url"));
    }
    Ok((serialized, origin))
}

fn timestamp() -> String {
    timestamp_after(None)
}

fn timestamp_after(previous: Option<&str>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Keep epoch seconds, adding fractional precision. Accept legacy whole seconds.
    // The mutation lock serializes writes; always advance even if the clock repeats.
    let prior = previous.and_then(|value| {
        let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
        let seconds = seconds.parse::<u128>().ok()?;
        if fraction.len() > 9 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let nanos = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>().ok()? * 10u128.pow(9 - fraction.len() as u32)
        };
        seconds
            .checked_mul(1_000_000_000)?
            .checked_add(nanos)?
            .checked_add(1)
    });
    let value = now.max(prior.unwrap_or(0));
    format!("{}.{:09}", value / 1_000_000_000, value % 1_000_000_000)
}

#[cfg(test)]
mod change_marker_tests {
    #[test]
    fn clock_rollback_and_legacy_seconds_still_advance() {
        assert_eq!(
            super::timestamp_after(Some("9999999999")),
            "9999999999.000000001"
        );
        assert_eq!(
            super::timestamp_after(Some("9999999999.999999999")),
            "10000000000.000000000"
        );
    }
}
