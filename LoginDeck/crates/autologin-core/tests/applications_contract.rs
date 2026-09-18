use async_trait::async_trait;
use autologin_core::{
    AppError, ApplicationCatalog, ApplicationRecord, DiscoveredApplication, Platform,
    VerifiedApplication,
};
use secrecy::SecretString;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Barrier;
use tokio::task::JoinSet;

#[derive(Default)]
struct Catalog {
    discovers: Mutex<usize>,
    launches: Mutex<usize>,
}
#[async_trait]
impl ApplicationCatalog for Catalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        *self.discovers.lock().unwrap() += 1;
        Ok(vec![DiscoveredApplication {
            platform: Platform::Macos,
            display_name: "Example".into(),
            platform_application_id: "com.example.app".into(),
            launch_target: "/Applications/Example.app".into(),
            alternate_launch_targets: vec![],
            path_access_ref: None,
            version: None,
            signature_identity: "sig".into(),
            discovery_source: autologin_core::DiscoverySource::Automatic,
        }])
    }
    async fn import_bundle(&self, _: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(vec![])
    }
    async fn verify_and_launch(
        &self,
        _: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        *self.launches.lock().unwrap() += 1;
        unreachable!()
    }
}
#[tokio::test]
async fn rescan_persists_discovery_without_launching() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(Catalog::default());
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );
    let apps = service.rescan_applications().await.unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(*catalog.launches.lock().unwrap(), 0);
}

#[tokio::test]
async fn account_numbering_skips_existing_labels_and_failed_creates_do_not_reserve() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog::default()),
    )
    .with_credentials(Arc::new(FailingDeleteCredentials::default()));
    let app = service.rescan_applications().await.unwrap().remove(0);
    let scope = format!("application:{}", app.id.as_uuid());
    service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            app.id,
            "Account 99",
            "alice",
            SecretString::from("fixture"),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(
        repos
            .websites()
            .next_account_number(&scope, false)
            .await
            .unwrap(),
        100
    );
    assert!(service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            app.id,
            "",
            "",
            SecretString::from("fixture"),
            false
        ))
        .await
        .is_err());
    assert_eq!(
        repos
            .websites()
            .next_account_number(&scope, false)
            .await
            .unwrap(),
        100
    );
    let saved = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            app.id,
            "",
            "bob",
            SecretString::from("fixture"),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(saved.display_name, "Account 100");
    service.delete_application_account(saved.id).await.unwrap();
    assert_eq!(
        repos
            .websites()
            .next_account_number(&scope, false)
            .await
            .unwrap(),
        101
    );
}

#[tokio::test]
async fn rescan_rejects_an_empty_primary_target_without_persisting_it() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(ReconcilingCatalog::default());
    catalog.discovered.lock().unwrap().push(discovered(""));
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog,
    );

    let error = service.rescan_applications().await.unwrap_err();
    assert_eq!(error.code(), "validation.invalid_field");
    assert_eq!(
        error.params.get("field").map(String::as_str),
        Some("launch_target")
    );
    assert!(repos.applications().list().await.unwrap().is_empty());
}

#[derive(Default)]
struct ReconcilingCatalog {
    discovered: Mutex<Vec<DiscoveredApplication>>,
    imported: Mutex<Vec<DiscoveredApplication>>,
    launches: Mutex<usize>,
}

#[async_trait]
impl ApplicationCatalog for ReconcilingCatalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(self.discovered.lock().unwrap().clone())
    }
    async fn import_bundle(&self, _: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(self.imported.lock().unwrap().clone())
    }
    async fn verify_and_launch(
        &self,
        _: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        *self.launches.lock().unwrap() += 1;
        unreachable!()
    }
}

fn discovered(path: &str) -> DiscoveredApplication {
    DiscoveredApplication {
        platform: Platform::Macos,
        display_name: "Example".into(),
        platform_application_id: "com.example.app".into(),
        launch_target: path.into(),
        alternate_launch_targets: vec![],
        path_access_ref: None,
        version: None,
        signature_identity: "sig".into(),
        discovery_source: autologin_core::DiscoverySource::Automatic,
    }
}

#[tokio::test]
async fn rescan_reuses_identity_across_moves_marks_missing_and_deduplicates_stably() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(ReconcilingCatalog::default());
    catalog
        .discovered
        .lock()
        .unwrap()
        .push(discovered("/Applications/Example.app"));
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );

    let first = service.rescan_applications().await.unwrap().remove(0);
    let mut duplicate = first.clone();
    duplicate.id = autologin_core::ApplicationId::new();
    duplicate.launch_target = "/Applications/Example 2.app".into();
    duplicate.created_at = "9999999999".into();
    repos
        .applications()
        .insert(duplicate.clone())
        .await
        .unwrap();

    *catalog.discovered.lock().unwrap() = vec![discovered("/Applications/Example 2.app")];
    let moved = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(moved.id, first.id);
    assert_eq!(moved.launch_target, "/Applications/Example 2.app");
    assert!(
        !repos
            .applications()
            .get(duplicate.id)
            .await
            .unwrap()
            .unwrap()
            .is_present
    );

    catalog.discovered.lock().unwrap().clear();
    assert!(service.rescan_applications().await.unwrap().is_empty());
    assert!(
        !repos
            .applications()
            .get(first.id)
            .await
            .unwrap()
            .unwrap()
            .is_present
    );
    assert_eq!(*catalog.launches.lock().unwrap(), 0);
}

#[derive(Default)]
struct FailingDeleteCredentials {
    fail_delete: Mutex<bool>,
    deletes: Mutex<Vec<String>>,
    references: Mutex<Vec<String>>,
}

#[async_trait]
impl autologin_core::CredentialStore for FailingDeleteCredentials {
    async fn put(
        &self,
        reference: &autologin_core::SecretRef,
        _: &str,
        _: SecretString,
    ) -> Result<(), AppError> {
        self.references
            .lock()
            .unwrap()
            .push(reference.as_str().into());
        Ok(())
    }
    async fn reveal(
        &self,
        _: &autologin_core::SecretRef,
        _: &str,
    ) -> Result<SecretString, AppError> {
        unreachable!()
    }
    async fn delete(&self, reference: &autologin_core::SecretRef) -> Result<(), AppError> {
        self.deletes.lock().unwrap().push(reference.as_str().into());
        if *self.fail_delete.lock().unwrap() {
            Err(AppError::new("credential.unavailable"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn account_cleanup_is_durable_and_explicit_retry_acks_only_after_keychain_success() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(Catalog::default());
    let credentials = Arc::new(FailingDeleteCredentials {
        fail_delete: Mutex::new(true),
        ..Default::default()
    });
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog,
    )
    .with_credentials(credentials.clone());
    let application = service.rescan_applications().await.unwrap().remove(0);
    let account = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            application.id,
            "Personal",
            "alice",
            SecretString::from("secret"),
            false,
        ))
        .await
        .unwrap();

    assert_eq!(
        service
            .delete_application_account(account.id)
            .await
            .unwrap_err()
            .code(),
        "credential.unavailable"
    );
    let pending = repos.pending_secret_cleanup().list().await.unwrap();
    assert!(pending.is_empty());
    assert!(repos
        .application_accounts()
        .get(account.id)
        .await
        .unwrap()
        .is_some());

    assert_eq!(
        service
            .remove_application(application.id)
            .await
            .unwrap_err()
            .code(),
        "application.has_accounts"
    );
    *credentials.fail_delete.lock().unwrap() = false;
    service
        .delete_application_account(account.id)
        .await
        .unwrap();
    service.remove_application(application.id).await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn application_selection_deletes_unchecked_app_accounts_and_tracks_secret_cleanup() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(FailingDeleteCredentials::default());
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog::default()),
    )
    .with_credentials(credentials.clone());
    let application = service.rescan_applications().await.unwrap().remove(0);
    let account = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            application.id,
            "Personal",
            "alice",
            SecretString::from("secret"),
            false,
        ))
        .await
        .unwrap();
    *credentials.fail_delete.lock().unwrap() = true;

    assert_eq!(
        service
            .apply_discovered_selection(vec![], vec![application.id])
            .await
            .unwrap_err()
            .code(),
        "credential.cleanup_required"
    );
    assert!(repos
        .applications()
        .get(application.id)
        .await
        .unwrap()
        .is_none());
    assert!(repos
        .application_accounts()
        .get(account.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repos.pending_secret_cleanup().list().await.unwrap().len(),
        1
    );

    *credentials.fail_delete.lock().unwrap() = false;
    service.retry_pending_secret_cleanup().await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

struct AccountBarrierCredentials {
    references: Mutex<Vec<String>>,
    deleted: Mutex<Vec<String>>,
    block_next_put: AtomicBool,
    fail_delete: AtomicBool,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

#[async_trait]
impl autologin_core::CredentialStore for AccountBarrierCredentials {
    async fn put(
        &self,
        reference: &autologin_core::SecretRef,
        _: &str,
        _: SecretString,
    ) -> Result<(), AppError> {
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
    async fn reveal(
        &self,
        _: &autologin_core::SecretRef,
        _: &str,
    ) -> Result<SecretString, AppError> {
        unreachable!()
    }
    async fn delete(&self, reference: &autologin_core::SecretRef) -> Result<(), AppError> {
        self.deleted.lock().unwrap().push(reference.as_str().into());
        if self.fail_delete.load(Ordering::SeqCst) {
            Err(AppError::new("credential.unavailable"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn separate_account_services_serialize_update_and_delete_without_orphaning_secrets() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(Catalog::default());
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let credentials = Arc::new(AccountBarrierCredentials {
        references: Mutex::new(Vec::new()),
        deleted: Mutex::new(Vec::new()),
        block_next_put: AtomicBool::new(false),
        fail_delete: AtomicBool::new(false),
        entered: entered.clone(),
        release: release.clone(),
    });
    let updater = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    )
    .with_credentials(credentials.clone());
    let deleter = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog,
    )
    .with_credentials(credentials.clone());
    let application = updater.rescan_applications().await.unwrap().remove(0);
    let original = updater
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            application.id,
            "Personal",
            "alice",
            SecretString::from("first"),
            false,
        ))
        .await
        .unwrap();

    credentials.block_next_put.store(true, Ordering::SeqCst);
    let update = tokio::spawn({
        let updater = updater.clone();
        async move {
            updater
                .save_application_account(autologin_core::SaveApplicationAccount::new(
                    Some(original.id),
                    application.id,
                    "Personal",
                    "alice",
                    SecretString::from("replacement"),
                    false,
                ))
                .await
        }
    });
    entered.wait().await;
    let delete = tokio::spawn(async move { deleter.delete_application_account(original.id).await });
    release.wait().await;

    update.await.unwrap().unwrap();
    delete.await.unwrap().unwrap();
    assert!(repos
        .application_accounts()
        .get(original.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        *credentials.deleted.lock().unwrap(),
        *credentials.references.lock().unwrap()
    );
}

struct ConcurrentDiscoveryCatalog {
    ready: Arc<Barrier>,
}

#[async_trait]
impl ApplicationCatalog for ConcurrentDiscoveryCatalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        self.ready.wait().await;
        Ok(vec![discovered("/Applications/Rescan.app")])
    }

    async fn import_bundle(&self, _: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        self.ready.wait().await;
        Ok(vec![discovered("/Applications/Imported.app")])
    }

    async fn verify_and_launch(
        &self,
        _: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        unreachable!()
    }
}

#[tokio::test]
async fn concurrent_rescan_and_import_reconcile_one_identity_without_a_conflict() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(ConcurrentDiscoveryCatalog {
            ready: Arc::new(Barrier::new(2)),
        }),
    );

    let mut tasks = JoinSet::new();
    tasks.spawn({
        let service = service.clone();
        async move { ("rescan", service.rescan_applications().await) }
    });
    tasks.spawn({
        let service = service.clone();
        async move {
            (
                "import",
                service
                    .import_applications(vec!["/tmp/example.app".into()])
                    .await,
            )
        }
    });
    let (_, first) = tasks.join_next().await.unwrap().unwrap();
    let (_, second) = tasks.join_next().await.unwrap().unwrap();
    let first = first.unwrap();
    let second = second.unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].id, second[0].id);
    let records = repos.applications().list().await.unwrap();
    assert_eq!(records.iter().filter(|record| record.is_present).count(), 1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, first[0].id);
    assert_eq!(records[0].launch_target, second[0].launch_target);
}

#[tokio::test]
async fn failed_account_metadata_compensation_stays_queued_until_keychain_delete_succeeds() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let credentials = Arc::new(AccountBarrierCredentials {
        references: Mutex::new(Vec::new()),
        deleted: Mutex::new(Vec::new()),
        block_next_put: AtomicBool::new(true),
        fail_delete: AtomicBool::new(true),
        entered: entered.clone(),
        release: release.clone(),
    });
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog::default()),
    )
    .with_credentials(credentials.clone());
    let application = service.rescan_applications().await.unwrap().remove(0);

    let save = tokio::spawn({
        let service = service.clone();
        async move {
            service
                .save_application_account(autologin_core::SaveApplicationAccount::new(
                    None,
                    application.id,
                    "Personal",
                    "alice",
                    SecretString::from("secret"),
                    false,
                ))
                .await
        }
    });
    entered.wait().await;
    // The account service validated this application before storing the credential. Removing it
    // now makes the metadata insert fail at its foreign-key boundary.
    repos.applications().delete(application.id).await.unwrap();
    release.wait().await;

    assert_eq!(
        save.await.unwrap().unwrap_err().code(),
        "credential.compensation_failed"
    );
    let pending = repos.pending_secret_cleanup().list().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].secret_ref.as_str(),
        credentials.references.lock().unwrap()[0]
    );

    credentials.fail_delete.store(false, Ordering::SeqCst);
    service.retry_pending_secret_cleanup().await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn duplicate_locations_are_persisted_without_launching() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(ReconcilingCatalog::default());
    let mut item = discovered("/Applications/Example.app");
    item.alternate_launch_targets = vec!["/Users/alice/Applications/Example.app".into()];
    catalog.discovered.lock().unwrap().push(item);
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );
    let first = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(
        first.alternate_launch_targets,
        vec!["/Users/alice/Applications/Example.app"]
    );
    assert_eq!(*catalog.launches.lock().unwrap(), 0);
}

#[tokio::test]
async fn same_primary_rescan_refreshes_version_and_accepts_full_alternate_merges() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(ReconcilingCatalog::default());
    let mut item = discovered("/Applications/Example.app");
    item.version = Some("1".into());
    *catalog.discovered.lock().unwrap() = vec![item.clone()];
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );
    let first = service.rescan_applications().await.unwrap().remove(0);
    item.version = Some("2".into());
    *catalog.discovered.lock().unwrap() = vec![item.clone()];
    let updated = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.version.as_deref(), Some("2"));
    item.alternate_launch_targets = (0..32)
        .map(|i| format!("/Applications/A{i:02}.app"))
        .collect();
    *catalog.discovered.lock().unwrap() = vec![item.clone()];
    let bounded = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(bounded.alternate_launch_targets.len(), 32);
    item.alternate_launch_targets = (0..32)
        .map(|i| format!("/Applications/B{i:02}.app"))
        .collect();
    *catalog.discovered.lock().unwrap() = vec![item];
    let merged = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(
        merged.alternate_launch_targets,
        bounded.alternate_launch_targets
    );
}

#[tokio::test]
async fn automatic_same_primary_rescan_retains_manual_path_access_and_refreshes_metadata() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let catalog = Arc::new(ReconcilingCatalog::default());
    let mut item = discovered("/Users/alice/Applications/Example.app");
    item.path_access_ref = Some("manual-bookmark".into());
    item.version = Some("1".into());
    item.signature_identity = "old-signature".into();
    item.discovery_source = autologin_core::DiscoverySource::ManualImport;
    *catalog.imported.lock().unwrap() = vec![item.clone()];
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );

    let imported = service
        .import_applications(vec![item.launch_target.clone().into()])
        .await
        .unwrap()
        .remove(0);
    item.display_name = "Example Updated".into();
    item.path_access_ref = None;
    item.version = Some("2".into());
    item.signature_identity = "new-signature".into();
    item.discovery_source = autologin_core::DiscoverySource::Automatic;
    *catalog.discovered.lock().unwrap() = vec![item];

    let rescanned = service.rescan_applications().await.unwrap().remove(0);
    assert_eq!(rescanned.id, imported.id);
    assert_eq!(
        rescanned.path_access_ref.as_deref(),
        Some("manual-bookmark")
    );
    assert_eq!(rescanned.display_name, "Example Updated");
    assert_eq!(rescanned.version.as_deref(), Some("2"));
    assert_eq!(rescanned.signature_identity, "new-signature");
    assert_eq!(
        rescanned.discovery_source,
        autologin_core::DiscoverySource::Automatic
    );
}

#[tokio::test]
async fn account_metadata_edit_keeps_secret_and_rejects_empty_replacement() {
    let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let credentials = Arc::new(FailingDeleteCredentials::default());
    let service = autologin_core::ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog::default()),
    )
    .with_credentials(credentials.clone());
    let application = service.rescan_applications().await.unwrap().remove(0);
    let original = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            None,
            application.id,
            "Personal",
            "alice",
            SecretString::from("fixture"),
            false,
        ))
        .await
        .unwrap();
    let edited = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            Some(original.id),
            application.id,
            "Renamed",
            "alice",
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(
        edited.password_credential_ref,
        original.password_credential_ref
    );
    assert!(edited.auto_submit_enabled);
    assert_eq!(credentials.references.lock().unwrap().len(), 1);
    assert!(credentials.deletes.lock().unwrap().is_empty());
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    let error = service
        .save_application_account(autologin_core::SaveApplicationAccount::new(
            Some(original.id),
            application.id,
            "Renamed",
            "alice",
            SecretString::from(""),
            true,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "validation.invalid_field");
    assert_eq!(credentials.references.lock().unwrap().len(), 1);
}
