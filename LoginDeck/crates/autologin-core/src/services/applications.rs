//! Narrow application catalogue and account-vault operations.

use crate::mutation_lock::MutationLock;
use crate::{
    AppError, ApplicationAccount, ApplicationAccountId, ApplicationAccountsRepository,
    ApplicationCatalog, ApplicationId, ApplicationRecord, ApplicationsRepository, Clipboard,
    CredentialStore, DiscoveredApplication, PendingSecretCleanupRepository, SecretCleanupKind,
    SecretRef, UserPresence,
};
use secrecy::SecretString;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

const CLEANUP_REQUIRED: &str = "credential.cleanup_required";
const COMPENSATION_FAILED: &str = "credential.compensation_failed";
const COMPENSATION_TRACKING_FAILED: &str = "credential.compensation_tracking_failed";
const CLEANUP_TRACKING_FAILED: &str = "credential.cleanup_tracking_failed";

/// Password-bearing account input. Its Debug output is deliberately redacted.
pub struct SaveApplicationAccount {
    pub id: Option<ApplicationAccountId>,
    pub application_id: ApplicationId,
    pub display_name: String,
    pub username: String,
    pub password: Option<SecretString>,
    pub auto_submit_enabled: bool,
    pub login_method: crate::LoginMethod,
    pub phone: Option<String>,
}
impl SaveApplicationAccount {
    pub fn new(
        id: Option<ApplicationAccountId>,
        application_id: ApplicationId,
        display_name: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<Option<SecretString>>,
        auto_submit_enabled: bool,
    ) -> Self {
        Self {
            id,
            application_id,
            display_name: display_name.into(),
            username: username.into(),
            password: password.into(),
            auto_submit_enabled,
            login_method: crate::LoginMethod::Password,
            phone: None,
        }
    }
}
impl SaveApplicationAccount {
    pub fn with_login_method(mut self, method: crate::LoginMethod, phone: Option<String>) -> Self {
        self.login_method = method;
        self.phone = phone;
        self
    }
}
impl fmt::Debug for SaveApplicationAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveApplicationAccount")
            .field("id", &self.id)
            .field("application_id", &self.application_id)
            .field("display_name", &self.display_name)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("auto_submit_enabled", &self.auto_submit_enabled)
            .finish()
    }
}

#[derive(Clone)]
pub struct ApplicationsService {
    applications: ApplicationsRepository,
    accounts: ApplicationAccountsRepository,
    cleanup: PendingSecretCleanupRepository,
    catalog: Arc<dyn ApplicationCatalog>,
    credentials: Option<Arc<dyn CredentialStore>>,
    mutation: Arc<MutationLock>,
}
impl ApplicationsService {
    pub fn new(
        applications: ApplicationsRepository,
        accounts: ApplicationAccountsRepository,
        catalog: Arc<dyn ApplicationCatalog>,
    ) -> Self {
        let cleanup = accounts.pending_secret_cleanup();
        let mutation = accounts.state.vault_mutation.clone();
        Self {
            applications,
            accounts,
            cleanup,
            catalog,
            credentials: None,
            mutation,
        }
    }
    pub fn with_credentials(mut self, credentials: Arc<dyn CredentialStore>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Discovery is intentionally read/metadata-only: it never invokes verify_and_launch.
    pub async fn rescan_applications(&self) -> Result<Vec<ApplicationRecord>, AppError> {
        self.persist_discovered(self.catalog.discover().await?, true)
            .await
    }
    pub async fn discover_applications(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        self.catalog.discover().await
    }
    pub async fn apply_discovery(
        &self,
        discovered: Vec<DiscoveredApplication>,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        self.persist_discovered(discovered, true).await
    }
    /// Add only the backend-held candidates explicitly selected by the user.
    /// Unselected and previously saved applications are not marked missing.
    pub async fn add_discovered_applications(
        &self,
        selected: Vec<DiscoveredApplication>,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        if selected.is_empty() {
            return Err(AppError::invalid_field("paths"));
        }
        self.persist_discovered(selected, false).await
    }
    pub async fn apply_discovered_selection(
        &self,
        selected: Vec<DiscoveredApplication>,
        removed: Vec<ApplicationId>,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        let unique_removed: HashSet<_> = removed.iter().copied().collect();
        if unique_removed.len() != removed.len() {
            return Err(AppError::new("storage.conflict"));
        }
        let _guard = self.mutation.lock().await?;
        let needs_credentials = {
            let mut needed = false;
            for id in &removed {
                needed |= self
                    .accounts
                    .list_for_application(*id)
                    .await?
                    .iter()
                    .any(|account| account.password_credential_ref.is_some());
            }
            needed
        };
        let credentials = if needs_credentials {
            Some(
                self.credentials
                    .as_ref()
                    .ok_or_else(|| AppError::new("credential.unavailable"))?,
            )
        } else {
            None
        };
        let mut references = Vec::new();
        for id in removed {
            let (_, mut deleted_references) = self
                .applications
                .delete_with_account_cleanup(id, &timestamp())
                .await?
                .ok_or_else(|| AppError::new("storage.conflict"))?;
            references.append(&mut deleted_references);
        }
        let saved = self.persist_discovered_locked(selected, false).await?;
        let mut cleanup_failed = false;
        for reference in references {
            if self
                .finish_cleanup_locked(
                    credentials.expect("removed credentials checked"),
                    &reference,
                )
                .await
                .is_err()
            {
                cleanup_failed = true;
            }
        }
        if cleanup_failed {
            Err(AppError::new(CLEANUP_REQUIRED))
        } else {
            Ok(saved)
        }
    }
    /// Paths come from the dialog but are revalidated by the platform catalog before persistence.
    pub async fn import_applications(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        if paths.is_empty() {
            return Err(AppError::invalid_field("paths"));
        }
        let mut all = Vec::new();
        for path in paths {
            all.extend(self.catalog.import_bundle(Path::new(&path)).await?);
        }
        self.persist_discovered(all, false).await
    }
    pub async fn list_applications(&self) -> Result<Vec<ApplicationRecord>, AppError> {
        self.applications.list().await
    }
    pub async fn get_application(&self, id: ApplicationId) -> Result<ApplicationRecord, AppError> {
        self.applications
            .get(id)
            .await?
            .ok_or_else(AppError::application_not_found)
    }
    pub async fn list_accounts(
        &self,
        application_id: ApplicationId,
    ) -> Result<Vec<ApplicationAccount>, AppError> {
        self.accounts.list_for_application(application_id).await
    }

    pub async fn get_account(
        &self,
        id: ApplicationAccountId,
    ) -> Result<ApplicationAccount, AppError> {
        self.accounts
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))
    }
    pub async fn copy_account_username(
        &self,
        id: ApplicationAccountId,
    ) -> Result<String, AppError> {
        let account = self.get_account(id).await?;
        if account.username.trim().is_empty() {
            return Err(AppError::new("storage.not_found"));
        }
        Ok(account.username)
    }

    pub async fn copy_account_password<P, B>(
        &self,
        id: ApplicationAccountId,
        presence: Arc<P>,
        clipboard: Arc<B>,
    ) -> Result<(), AppError>
    where
        P: UserPresence + 'static,
        B: Clipboard + 'static,
    {
        let account = self.get_account(id).await?;
        let reference = account
            .password_credential_ref
            .ok_or_else(|| AppError::new("credential.not_found"))?;
        let credentials = self
            .credentials
            .clone()
            .ok_or_else(|| AppError::new("credential.unavailable"))?;
        crate::services::copy_password(credentials, reference, presence, clipboard).await
    }

    pub async fn launch_application(&self, id: ApplicationId) -> Result<(), AppError> {
        let application = self.get_application(id).await?;
        if !application.is_present {
            return Err(AppError::application_not_found());
        }
        self.catalog.verify_and_launch(&application).await?;
        Ok(())
    }
    /// Generic workflow preparation, independent of the compiled v1 adapter registry.
    pub async fn prepare_switch(
        &self,
        expected: &ApplicationAccount,
    ) -> Result<crate::VerifiedApplication, AppError> {
        self.check_fill_account(expected).await?;
        if expected.login_method != crate::LoginMethod::Password {
            return Err(AppError::new("application.unsupported_import"));
        }
        let app = self.get_application(expected.application_id).await?;
        if !app.is_present {
            return Err(AppError::application_not_found());
        }
        let verified = self.catalog.verify_and_launch(&app).await?;
        self.check_fill_account(expected).await?;
        Ok(verified)
    }

    /// Backend-only preparation: caller receives an attested process, never a frontend path.
    pub async fn prepare_fill(
        &self,
        id: ApplicationAccountId,
    ) -> Result<
        (
            ApplicationAccount,
            crate::VerifiedApplication,
            crate::adapter::AppDefinition,
        ),
        AppError,
    > {
        let account = self
            .accounts
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))?;
        if account.login_method != crate::LoginMethod::Password {
            return Err(AppError::new("application.unsupported_import"));
        }
        let application = self.get_application(account.application_id).await?;
        let adapter = crate::adapter::builtin_for_application(
            application.platform,
            &application.platform_application_id,
        )
        .map_err(|_| AppError::new("internal.error"))?
        .ok_or_else(|| AppError::new("application.unsupported_import"))?;
        if !application.is_present {
            return Err(AppError::application_not_found());
        }
        let verified = self.catalog.verify_and_launch(&application).await?;
        Ok((account, verified, adapter))
    }

    /// Manual login launches only the stored, signature-verified application.
    pub async fn prepare_manual_login(
        &self,
        id: ApplicationAccountId,
    ) -> Result<ApplicationAccount, AppError> {
        let account = self
            .accounts
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))?;
        if account.login_method != crate::LoginMethod::Sms {
            return Err(AppError::new("application.unsupported_import"));
        }
        let application = self.get_application(account.application_id).await?;
        if !application.is_present {
            return Err(AppError::application_not_found());
        }
        self.catalog.verify_and_launch(&application).await?;
        self.check_fill_account(&account).await?;
        Ok(account)
    }
    pub async fn account_login_method(
        &self,
        id: ApplicationAccountId,
    ) -> Result<crate::LoginMethod, AppError> {
        self.accounts
            .get(id)
            .await?
            .map(|a| a.login_method)
            .ok_or_else(|| AppError::new("storage.not_found"))
    }

    /// Only used after explicit position confirmation. Rechecks changes around system consent.
    pub async fn reveal_for_fill(
        &self,
        expected: &ApplicationAccount,
    ) -> Result<SecretString, AppError> {
        self.check_fill_account(expected).await?;
        let secret = self
            .credentials
            .as_ref()
            .ok_or_else(|| AppError::new("credential.unavailable"))?
            .reveal(
                expected
                    .password_credential_ref
                    .as_ref()
                    .filter(|_| expected.login_method == crate::LoginMethod::Password)
                    .ok_or_else(|| AppError::new("application.unsupported_import"))?,
                "Fill the selected application account",
            )
            .await?;
        self.check_fill_account(expected).await?;
        Ok(secret)
    }

    pub async fn check_fill_account(&self, expected: &ApplicationAccount) -> Result<(), AppError> {
        if self.accounts.get(expected.id).await?.as_ref() != Some(expected) {
            return Err(AppError::new("storage.conflict"));
        }
        Ok(())
    }

    pub async fn save_application_account(
        &self,
        mut request: SaveApplicationAccount,
    ) -> Result<ApplicationAccount, AppError> {
        let _guard = self.mutation.lock().await?;
        let journal = crate::WebsitesRepository {
            state: self.accounts.state.clone(),
        };
        if journal
            .pending_deletions()
            .await?
            .iter()
            .any(|(kind, owner)| {
                (kind == "application" && owner == &request.application_id.as_uuid().to_string())
                    || (kind == "account"
                        && request
                            .id
                            .is_some_and(|id| owner == &id.as_uuid().to_string()))
            })
        {
            return Err(AppError::new("storage.conflict"));
        }
        if request.id.is_none() {
            let sequence = crate::WebsitesRepository {
                state: self.accounts.state.clone(),
            };
            let number = sequence
                .next_account_number(
                    &format!("application:{}", request.application_id.as_uuid()),
                    false,
                )
                .await?;
            if request.display_name.trim().is_empty() {
                let locale = crate::SettingsRepository {
                    state: self.accounts.state.clone(),
                }
                .locale()
                .await?;
                request.display_name = format!(
                    "{}{number}",
                    if locale == crate::Locale::ZhCn {
                        "账号 "
                    } else {
                        "Account "
                    }
                );
            }
        }
        if request.id.is_some() && request.display_name.trim().is_empty() {
            request.display_name = self
                .accounts
                .get(request.id.expect("existing account"))
                .await?
                .ok_or_else(|| AppError::new("storage.not_found"))?
                .display_name;
        }
        bounded_nonblank("display_name", &request.display_name, 256)?;
        match request.login_method {
            crate::LoginMethod::Password => {
                bounded_nonblank("username", &request.username, 512)?;
                if request.phone.is_some() {
                    return Err(AppError::invalid_field("phone"));
                }
            }
            crate::LoginMethod::Sms => {
                let phone = request
                    .phone
                    .as_deref()
                    .ok_or_else(|| AppError::invalid_field("phone"))?;
                let digits = phone.strip_prefix('+').unwrap_or(phone);
                if !(5..=15).contains(&digits.len()) || !digits.bytes().all(|c| c.is_ascii_digit())
                {
                    return Err(AppError::invalid_field("phone"));
                }
                if !request.username.is_empty() {
                    return Err(AppError::invalid_field("username"));
                }
            }
            // Retained only for reading and removing legacy records.
            crate::LoginMethod::Qr => return Err(AppError::invalid_field("login_method")),
        }
        if request.login_method != crate::LoginMethod::Password
            && (request.password.is_some() || request.auto_submit_enabled)
        {
            return Err(AppError::invalid_field("login_method"));
        }
        if self
            .applications
            .get(request.application_id)
            .await?
            .is_none()
        {
            return Err(AppError::application_not_found());
        }
        let previous = match request.id {
            Some(id) => self.accounts.get(id).await?,
            None => None,
        };
        if request.id.is_some() && previous.is_none() {
            return Err(AppError::new("storage.not_found"));
        }
        if previous
            .as_ref()
            .is_some_and(|old| old.application_id != request.application_id)
        {
            return Err(AppError::invalid_field("application_id"));
        }
        let credentials = if request.login_method == crate::LoginMethod::Password
            || previous
                .as_ref()
                .is_some_and(|old| old.password_credential_ref.is_some())
        {
            Some(
                self.credentials
                    .as_ref()
                    .ok_or_else(|| AppError::new("credential.unavailable"))?,
            )
        } else {
            None
        };
        if let Some(credentials) = credentials {
            let _ = self
                .drain_pending_locked_with_credentials(credentials)
                .await;
        }
        let now = timestamp();
        let reference = if request.login_method == crate::LoginMethod::Password {
            Some(
                super::credential_write::prepare(
                    &self.cleanup,
                    credentials.expect("password credentials checked").as_ref(),
                    previous
                        .as_ref()
                        .and_then(|old| old.password_credential_ref.as_ref()),
                    request.password,
                    &request.username,
                    SecretCleanupKind::ApplicationAccount,
                    &now,
                )
                .await?,
            )
        } else {
            None
        };
        let replaced = previous
            .as_ref()
            .and_then(|old| old.password_credential_ref.as_ref())
            != reference.as_ref();
        let same_identity = previous.as_ref().is_some_and(|old| {
            old.login_method == request.login_method
                && old.username == request.username
                && old.phone == request.phone
        });
        let record = ApplicationAccount {
            login_method: request.login_method,
            phone: request.phone,
            id: request.id.unwrap_or_default(),
            application_id: request.application_id,
            display_name: request.display_name,
            username: request.username,
            password_credential_ref: reference.clone(),
            auto_submit_enabled: request.auto_submit_enabled,
            last_login_status: previous
                .as_ref()
                .filter(|_| same_identity)
                .and_then(|old| old.last_login_status.clone()),
            last_login_at: previous
                .as_ref()
                .filter(|_| same_identity)
                .and_then(|old| old.last_login_at.clone()),
            created_at: previous
                .as_ref()
                .map_or_else(|| now.clone(), |old| old.created_at.clone()),
            updated_at: now,
        };
        match self
            .accounts
            .save_with_cleanup(record, previous.as_ref(), &timestamp())
            .await
        {
            Ok(saved) => {
                if let Some(old) = previous
                    .filter(|_| replaced)
                    .and_then(|old| old.password_credential_ref)
                {
                    let _ = self
                        .finish_cleanup_locked(credentials.expect("old credentials checked"), &old)
                        .await;
                }
                Ok(saved)
            }
            Err(error) => {
                if !replaced {
                    return Err(error);
                }
                if let Some(reference) = reference {
                    self.compensate_new_locked(
                        credentials.expect("new credentials checked"),
                        &reference,
                        None,
                        "metadata_write_failed",
                        &timestamp(),
                    )
                    .await?;
                }
                Err(error)
            }
        }
    }
    pub async fn delete_application_account(
        &self,
        id: ApplicationAccountId,
    ) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        let old = self
            .accounts
            .get(id)
            .await?
            .ok_or_else(|| AppError::new("storage.not_found"))?;
        let credentials = if old.password_credential_ref.is_some() {
            Some(
                self.credentials
                    .as_ref()
                    .ok_or_else(|| AppError::new("credential.unavailable"))?,
            )
        } else {
            None
        };
        let journal = crate::WebsitesRepository {
            state: self.accounts.state.clone(),
        };
        let owner = id.as_uuid().to_string();
        journal.begin_deletion("account", &owner).await?;
        if let Some(reference) = old.password_credential_ref {
            if let Err(error) = credentials
                .expect("old credentials checked")
                .delete(&reference)
                .await
            {
                journal.cancel_deletion("account", &owner).await?;
                return Err(error);
            }
        }
        journal.finish_deletion("account", &owner).await?;
        Ok(())
    }

    pub async fn remove_application(&self, id: ApplicationId) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        self.get_application(id).await?;
        let accounts = self.accounts.list_for_application(id).await?;
        if !accounts.is_empty() {
            return Err(AppError::new("application.has_accounts"));
        }
        let journal = crate::WebsitesRepository {
            state: self.accounts.state.clone(),
        };
        let owner = id.as_uuid().to_string();
        journal.begin_deletion("application", &owner).await?;
        journal.finish_deletion("application", &owner).await?;
        Ok(())
    }

    pub async fn recover_deletions(&self) -> Result<(), AppError> {
        let journal = crate::WebsitesRepository {
            state: self.accounts.state.clone(),
        };
        for (kind, owner) in journal.pending_deletions().await? {
            let id =
                uuid::Uuid::parse_str(&owner).map_err(|_| AppError::new("storage.invalid_data"))?;
            match kind.as_str() {
                "account" => {
                    self.delete_application_account(ApplicationAccountId::from_uuid(id))
                        .await?
                }
                "application" => {
                    self.remove_application(ApplicationId::from_uuid(id))
                        .await?
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Retries every durable account cleanup job. A job is acknowledged only after Keychain deletion.
    pub async fn retry_pending_secret_cleanup(&self) -> Result<(), AppError> {
        let _guard = self.mutation.lock().await?;
        let credentials = self
            .credentials
            .as_ref()
            .ok_or_else(|| AppError::new("credential.unavailable"))?;
        self.drain_pending_locked_with_credentials(credentials)
            .await
    }

    async fn persist_discovered(
        &self,
        discovered: Vec<DiscoveredApplication>,
        mark_missing: bool,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        // Catalog I/O stays outside this critical section. Once a discovery snapshot is ready,
        // serialize the read/reconcile/write phase with account and vault mutations.
        let _guard = self.mutation.lock().await?;
        self.persist_discovered_locked(discovered, mark_missing)
            .await
    }

    async fn persist_discovered_locked(
        &self,
        discovered: Vec<DiscoveredApplication>,
        mark_missing: bool,
    ) -> Result<Vec<ApplicationRecord>, AppError> {
        let mut existing = self.applications.list().await?;
        existing.sort_by_key(|record| (record.created_at.clone(), *record.id.as_uuid()));
        let mut by_identity = BTreeMap::<(u8, String), Vec<ApplicationRecord>>::new();
        for record in existing.iter().cloned() {
            by_identity
                .entry(application_identity(
                    record.platform,
                    &record.platform_application_id,
                ))
                .or_default()
                .push(record);
        }

        let mut discovered_by_identity = BTreeMap::new();
        for item in discovered {
            crate::validate_application_metadata(
                &item.display_name,
                &item.platform_application_id,
                &item.launch_target,
                &item.alternate_launch_targets,
                item.version.as_deref(),
                &item.signature_identity,
                item.path_access_ref.as_deref(),
            )?;
            let key = application_identity(item.platform, &item.platform_application_id);
            let replace = discovered_by_identity
                .get(&key)
                .is_none_or(|old: &DiscoveredApplication| item.launch_target < old.launch_target);
            if replace {
                discovered_by_identity.insert(key, item);
            }
        }

        let mut saved = Vec::with_capacity(discovered_by_identity.len());
        let mut reconciled = HashSet::new();
        for (identity, item) in discovered_by_identity {
            let prior = by_identity
                .get(&identity)
                .and_then(|records| records.first());
            let now = timestamp();

            // Existing duplicate identities can coexist because an earlier schema keyed paths too.
            // Preserve the oldest ID, and make every other row absent deterministically. If a
            // duplicate already uses the newly discovered path, move it aside before updating the
            // survivor so the historical unique constraint cannot reject reconciliation.
            if let Some(records) = by_identity.get(&identity) {
                for duplicate in records.iter().skip(1) {
                    reconciled.insert(duplicate.id);
                    let mut duplicate = duplicate.clone();
                    duplicate.is_present = false;
                    duplicate.updated_at = now.clone();
                    if duplicate.launch_target == item.launch_target {
                        duplicate.launch_target =
                            format!("autologin-duplicate://{}", duplicate.id.as_uuid());
                    }
                    self.applications.update(duplicate).await?;
                }
            }
            let (launch_target, alternate_launch_targets) =
                reconciled_locations(prior, &item, mark_missing)?;
            let preserve_preferred = mark_missing
                && launch_target != item.launch_target
                && prior.is_some_and(|old| old.launch_target == launch_target);
            let preserve_path_access_ref = preserve_preferred
                || (mark_missing
                    && item.path_access_ref.is_none()
                    && prior.is_some_and(|old| old.launch_target == item.launch_target));
            let record = ApplicationRecord {
                id: prior.map(|old| old.id).unwrap_or_default(),
                platform: item.platform,
                display_name: item.display_name,
                platform_application_id: item.platform_application_id,
                launch_target,
                alternate_launch_targets,
                path_access_ref: if preserve_path_access_ref {
                    prior.and_then(|old| old.path_access_ref.clone())
                } else {
                    item.path_access_ref
                },
                version: if preserve_preferred {
                    prior.and_then(|old| old.version.clone())
                } else {
                    item.version
                },
                signature_identity: if preserve_preferred {
                    prior.map_or_else(String::new, |old| old.signature_identity.clone())
                } else {
                    item.signature_identity
                },
                discovery_source: if preserve_preferred {
                    prior.map_or(crate::DiscoverySource::Automatic, |old| {
                        old.discovery_source
                    })
                } else {
                    item.discovery_source
                },
                is_present: true,
                is_hidden: prior.is_some_and(|old| old.is_hidden),
                icon_cache_ref: prior.and_then(|old| old.icon_cache_ref.clone()),
                last_discovered_at: now.clone(),
                created_at: prior.map_or_else(|| now.clone(), |old| old.created_at.clone()),
                updated_at: now,
            };
            let outcome = if prior.is_some() {
                self.applications.update(record).await
            } else {
                self.applications.insert(record).await
            };
            let outcome = outcome?;
            reconciled.insert(outcome.id);
            saved.push(outcome);
        }

        if mark_missing {
            for mut old in existing {
                if !reconciled.contains(&old.id) && old.is_present {
                    old.is_present = false;
                    old.updated_at = timestamp();
                    self.applications.update(old).await?;
                }
            }
        }
        Ok(saved)
    }

    async fn compensate_new_locked(
        &self,
        credentials: &Arc<dyn CredentialStore>,
        reference: &SecretRef,
        owner_id: Option<String>,
        context: &str,
        now: &str,
    ) -> Result<(), AppError> {
        if credentials.delete(reference).await.is_ok() {
            return self
                .cleanup
                .remove(reference)
                .await
                .map_err(|_| AppError::new(COMPENSATION_TRACKING_FAILED));
        }
        self.cleanup
            .enqueue(
                reference,
                SecretCleanupKind::ApplicationAccount,
                owner_id,
                context,
                now,
            )
            .await
            .map_err(|_| AppError::new(COMPENSATION_TRACKING_FAILED))?;
        Err(AppError::new(COMPENSATION_FAILED))
    }

    async fn finish_cleanup_locked(
        &self,
        credentials: &Arc<dyn CredentialStore>,
        reference: &SecretRef,
    ) -> Result<(), AppError> {
        match credentials.delete(reference).await {
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

    async fn drain_pending_locked_with_credentials(
        &self,
        credentials: &Arc<dyn CredentialStore>,
    ) -> Result<(), AppError> {
        let jobs = self
            .cleanup
            .list()
            .await
            .map_err(|_| AppError::new(CLEANUP_TRACKING_FAILED))?;
        let mut failed = false;
        for job in jobs {
            if self
                .finish_cleanup_locked(credentials, &job.secret_ref)
                .await
                .is_err()
            {
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

fn application_identity(platform: crate::Platform, bundle_id: &str) -> (u8, String) {
    let platform = match platform {
        crate::Platform::Macos => 0,
        crate::Platform::Windows => 1,
    };
    (platform, bundle_id.to_owned())
}
fn nonblank(field: &'static str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        Err(AppError::invalid_field(field))
    } else {
        Ok(())
    }
}
fn bounded_nonblank(field: &'static str, value: &str, max: usize) -> Result<(), AppError> {
    nonblank(field, value)?;
    if value.len() > max {
        return Err(AppError::invalid_field(field));
    }
    Ok(())
}
fn normalized_alternates(values: &[String], primary: &str) -> Result<Vec<String>, AppError> {
    let mut result = values
        .iter()
        .filter(|value| value.as_str() != primary)
        .cloned()
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    result.truncate(32);
    crate::validate_launch_targets(&result)?;
    Ok(result)
}
fn reconciled_locations(
    prior: Option<&ApplicationRecord>,
    item: &DiscoveredApplication,
    automatic: bool,
) -> Result<(String, Vec<String>), AppError> {
    let mut candidates = item.alternate_launch_targets.clone();
    candidates.push(item.launch_target.clone());
    let preferred_is_discovered = prior.is_some_and(|old| candidates.contains(&old.launch_target));
    if let Some(old) = prior {
        candidates.push(old.launch_target.clone());
        candidates.extend(old.alternate_launch_targets.clone());
    }
    candidates.sort();
    candidates.dedup();
    let primary = if automatic && preferred_is_discovered {
        prior
            .map(|old| old.launch_target.clone())
            .unwrap_or_else(|| item.launch_target.clone())
    } else {
        item.launch_target.clone()
    };
    Ok((
        primary.clone(),
        normalized_alternates(&candidates, &primary)?,
    ))
}
fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
