use async_trait::async_trait;
use autologin_core::*;
use secrecy::SecretString;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
struct Catalog(Arc<AtomicUsize>);
#[async_trait]
impl ApplicationCatalog for Catalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(vec![])
    }
    async fn import_bundle(&self, _: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(vec![])
    }
    async fn verify_and_launch(
        &self,
        app: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(VerifiedApplication {
            application: app.clone(),
            verified_launch_target: app.launch_target.clone(),
            launched_process_id: 42,
        })
    }
}
struct Credentials(Arc<AtomicUsize>);
#[async_trait]
impl CredentialStore for Credentials {
    async fn put(&self, _: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(SecretString::from("fixture"))
    }
    async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
        Ok(())
    }
}
#[tokio::test]
async fn preparation_attests_but_never_reveals_and_changed_accounts_are_rejected() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let reveals = Arc::new(AtomicUsize::new(0));
    let mut app =
        ApplicationRecord::new(Platform::Macos, "com.tencent.qq".into(), "QQ".into()).unwrap();
    app.launch_target = "/Applications/QQ.app".into();
    app.signature_identity = "fixture".into();
    app.is_present = true;
    let app = repos.applications().insert(app).await.unwrap();
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog(launches.clone())),
    )
    .with_credentials(Arc::new(Credentials(reveals.clone())));
    let account = service
        .save_application_account(SaveApplicationAccount::new(
            None,
            app.id,
            "Fixture",
            "0000000000",
            Some(SecretString::from("fixture")),
            false,
        ))
        .await
        .unwrap();
    let (expected, verified, adapter) = service.prepare_fill(account.id).await.unwrap();
    assert_eq!(verified.launched_process_id, 42);
    assert_eq!(adapter.id(), "qq-macos");
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(reveals.load(Ordering::SeqCst), 0);
    service
        .save_application_account(SaveApplicationAccount::new(
            Some(account.id),
            app.id,
            "Fixture",
            "changed",
            None,
            false,
        ))
        .await
        .unwrap();
    assert!(service.reveal_for_fill(&expected).await.is_err());
    assert_eq!(reveals.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_password_accounts_work_without_a_credential_store_and_never_enter_fill() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let mut app =
        ApplicationRecord::new(Platform::Macos, "com.tencent.qq".into(), "Fixture".into()).unwrap();
    app.launch_target = "/Applications/QQ.app".into();
    app.signature_identity = "fixture".into();
    app.is_present = true;
    let app = repos.applications().insert(app).await.unwrap();
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog(launches.clone())),
    );
    for (method, phone) in [(LoginMethod::Sms, Some("+8613800000000".to_owned()))] {
        let account = service
            .save_application_account(
                SaveApplicationAccount::new(None, app.id, "Fixture", "", None, false)
                    .with_login_method(method, phone.clone()),
            )
            .await
            .unwrap();
        assert!(account.password_credential_ref.is_none());
        assert_eq!(account.phone, phone);
        assert!(service.prepare_fill(account.id).await.is_err());
        assert!(service.reveal_for_fill(&account).await.is_err());
        let edited = service
            .save_application_account(
                SaveApplicationAccount::new(Some(account.id), app.id, "Renamed", "", None, false)
                    .with_login_method(method, phone),
            )
            .await
            .unwrap();
        assert_eq!(edited.id, account.id);
        assert_eq!(
            service.prepare_manual_login(account.id).await.unwrap(),
            edited
        );
        service
            .delete_application_account(account.id)
            .await
            .unwrap();
    }
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    assert!(repos
        .application_accounts()
        .list()
        .await
        .unwrap()
        .is_empty());
}

struct CleanupCredentials {
    fail_delete: std::sync::atomic::AtomicBool,
    deletes: AtomicUsize,
    puts: AtomicUsize,
}
#[async_trait]
impl CredentialStore for CleanupCredentials {
    async fn put(&self, _: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
        panic!("non-password flows must not reveal")
    }
    async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        if self.fail_delete.load(Ordering::SeqCst) {
            Err(AppError::new("credential.denied"))
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn changing_login_method_commits_cleanup_and_requires_new_password_when_switching_back() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let mut app =
        ApplicationRecord::new(Platform::Macos, "com.example.app".into(), "Fixture".into())
            .unwrap();
    app.launch_target = "/Applications/Fixture.app".into();
    let app = repos.applications().insert(app).await.unwrap();
    let credentials = Arc::new(CleanupCredentials {
        fail_delete: true.into(),
        deletes: AtomicUsize::new(0),
        puts: AtomicUsize::new(0),
    });
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog(Arc::new(AtomicUsize::new(0)))),
    )
    .with_credentials(credentials.clone());
    let account = service
        .save_application_account(SaveApplicationAccount::new(
            None,
            app.id,
            "Fixture",
            "alice",
            Some(SecretString::from("fixture")),
            false,
        ))
        .await
        .unwrap();
    let committed = service
        .save_application_account(
            SaveApplicationAccount::new(Some(account.id), app.id, "Fixture", "", None, false)
                .with_login_method(LoginMethod::Sms, Some("13800000000".into())),
        )
        .await
        .unwrap();
    assert_eq!(committed.login_method, LoginMethod::Sms);
    let saved = repos
        .application_accounts()
        .get(account.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.login_method, LoginMethod::Sms);
    assert!(saved.password_credential_ref.is_none());
    let jobs = repos.pending_secret_cleanup().list().await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        Some(&jobs[0].secret_ref),
        account.password_credential_ref.as_ref()
    );
    credentials.fail_delete.store(false, Ordering::SeqCst);
    service.retry_pending_secret_cleanup().await.unwrap();
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(credentials.puts.load(Ordering::SeqCst), 1);
    assert!(service
        .save_application_account(SaveApplicationAccount::new(
            Some(account.id),
            app.id,
            "Fixture",
            "alice",
            None,
            false
        ))
        .await
        .is_err());
    let restored = service
        .save_application_account(SaveApplicationAccount::new(
            Some(account.id),
            app.id,
            "Fixture",
            "alice",
            Some(SecretString::from("new fixture")),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(restored.login_method, LoginMethod::Password);
    assert_ne!(
        restored.password_credential_ref,
        account.password_credential_ref
    );
    assert!(restored.phone.is_none());
}

#[tokio::test]
async fn invalid_method_payloads_are_rejected_without_keychain_or_launch() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let mut app =
        ApplicationRecord::new(Platform::Macos, "com.example.app".into(), "Fixture".into())
            .unwrap();
    app.launch_target = "/Applications/Fixture.app".into();
    let app = repos.applications().insert(app).await.unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog(launches.clone())),
    );
    for phone in [
        None,
        Some(""),
        Some("123"),
        Some("+123hello"),
        Some("1234567890123456"),
    ] {
        let err = service
            .save_application_account(
                SaveApplicationAccount::new(None, app.id, "Fixture", "", None, false)
                    .with_login_method(LoginMethod::Sms, phone.map(str::to_owned)),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "validation.invalid_field");
    }
    for method in [LoginMethod::Sms, LoginMethod::Qr] {
        let phone = (method == LoginMethod::Sms).then(|| "13800000000".into());
        assert!(service
            .save_application_account(
                SaveApplicationAccount::new(
                    None,
                    app.id,
                    "Fixture",
                    "",
                    Some(SecretString::from("must not save")),
                    false
                )
                .with_login_method(method, phone)
            )
            .await
            .is_err());
    }
    assert!(repos
        .application_accounts()
        .list()
        .await
        .unwrap()
        .is_empty());
    assert!(repos
        .pending_secret_cleanup()
        .list()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(launches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn retired_qr_records_cannot_be_saved_or_launched_but_can_be_deleted() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let mut app =
        ApplicationRecord::new(Platform::Macos, "com.tencent.qq".into(), "QQ".into()).unwrap();
    app.launch_target = "/Applications/QQ.app".into();
    app.signature_identity = "fixture".into();
    app.is_present = true;
    let app = repos.applications().insert(app).await.unwrap();
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        Arc::new(Catalog(launches.clone())),
    );
    let request = || {
        SaveApplicationAccount::new(None, app.id, "Legacy", "", None, false)
            .with_login_method(LoginMethod::Qr, None)
    };
    assert!(service.save_application_account(request()).await.is_err());
    let legacy = repos
        .application_accounts()
        .insert(ApplicationAccount {
            id: ApplicationAccountId::new(),
            application_id: app.id,
            display_name: "Legacy".into(),
            username: "note".into(),
            login_method: LoginMethod::Qr,
            phone: None,
            password_credential_ref: None,
            auto_submit_enabled: false,
            last_login_status: None,
            last_login_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .await
        .unwrap();
    let mut edit = request();
    edit.id = Some(legacy.id);
    assert!(service.save_application_account(edit).await.is_err());
    assert!(service.prepare_manual_login(legacy.id).await.is_err());
    assert!(service.prepare_fill(legacy.id).await.is_err());
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    assert_eq!(
        repos.application_accounts().get(legacy.id).await.unwrap(),
        Some(legacy.clone())
    );
    service.delete_application_account(legacy.id).await.unwrap();
    assert!(repos
        .application_accounts()
        .list()
        .await
        .unwrap()
        .is_empty());
}
