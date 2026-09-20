use autologin_core::{
    ApplicationAccount, ApplicationAccountId, ApplicationId, ApplicationRecord, CaptureSource,
    DiscoverySource, Locale, LoginTemplateId, LoginTemplateRecord, Platform, PreferenceKey,
    PreferenceValue, SecretRef, SettingRecord, SqliteRepositories, WebsiteId, WebsiteRecord,
};

const CREATED_AT: &str = "2026-09-11T00:00:00Z";
const UPDATED_AT: &str = "2026-09-11T00:00:01Z";

async fn prepared_repositories() -> SqliteRepositories {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    repos
}

fn secret_ref() -> SecretRef {
    SecretRef::new("keychain:website:alice").unwrap()
}

fn sample_website() -> WebsiteRecord {
    WebsiteRecord {
        account_name: String::new(),
        id: WebsiteId::new(),
        name: "Example".into(),
        url: "https://example.test/login".into(),
        normalized_origin: "https://example.test".into(),
        username: "alice".into(),
        password_credential_ref: secret_ref(),
        capture_source: CaptureSource::Manual,
        notes: String::new(),
        created_at: CREATED_AT.into(),
        updated_at: UPDATED_AT.into(),
    }
}

fn sample_application() -> ApplicationRecord {
    ApplicationRecord {
        id: ApplicationId::new(),
        platform: Platform::Macos,
        display_name: "Example Desktop".into(),
        platform_application_id: "com.example.desktop".into(),
        launch_target: "example-desktop".into(),
        alternate_launch_targets: vec![],
        path_access_ref: Some("bookmark".into()),
        version: Some("1.0".into()),
        signature_identity: "example-signature".into(),
        discovery_source: DiscoverySource::ManualImport,
        is_present: true,
        is_hidden: false,
        icon_cache_ref: Some("icon-cache".into()),
        last_discovered_at: CREATED_AT.into(),
        created_at: CREATED_AT.into(),
        updated_at: UPDATED_AT.into(),
    }
}

#[tokio::test]
async fn windows_application_round_trips_platform_and_aumid() {
    let repos = prepared_repositories().await;
    let mut application = sample_application();
    application.platform = Platform::Windows;
    application.platform_application_id = "Contoso.Chat_abc".into();
    application.launch_target = "aumid:Contoso.Chat_abc!App".into();

    let saved = repos
        .applications()
        .insert(application.clone())
        .await
        .unwrap();

    assert_eq!(saved.platform, Platform::Windows);
    assert_eq!(saved.launch_target, "aumid:Contoso.Chat_abc!App");
    assert_eq!(
        repos.applications().get(saved.id).await.unwrap(),
        Some(application)
    );
}

#[tokio::test]
async fn application_update_round_trips_windows_platform_and_aumid() {
    let repos = prepared_repositories().await;
    let mut application = repos
        .applications()
        .insert(sample_application())
        .await
        .unwrap();
    assert_eq!(application.platform, Platform::Macos);

    application.platform = Platform::Windows;
    application.platform_application_id = "Contoso.Chat_abc!App".into();
    application.launch_target = "aumid:Contoso.Chat_abc!App".into();
    application.path_access_ref = None;
    repos
        .applications()
        .update(application.clone())
        .await
        .unwrap();

    let reloaded = repos
        .applications()
        .get(application.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.platform, Platform::Windows);
    assert_eq!(reloaded.launch_target, "aumid:Contoso.Chat_abc!App");
    assert_eq!(reloaded, application);
}

fn sample_account(application_id: ApplicationId) -> ApplicationAccount {
    ApplicationAccount {
        id: ApplicationAccountId::new(),
        application_id,
        display_name: "Personal".into(),
        username: "alice".into(),
        login_method: autologin_core::LoginMethod::Password,
        phone: None,
        password_credential_ref: Some(secret_ref()),
        auto_submit_enabled: true,
        last_login_status: None,
        last_login_at: None,
        created_at: CREATED_AT.into(),
        updated_at: UPDATED_AT.into(),
    }
}

fn sample_template(application_id: ApplicationId) -> LoginTemplateRecord {
    LoginTemplateRecord {
        id: LoginTemplateId::new(),
        application_id,
        window_signature: "login-v1".into(),
        window_size_class: "medium".into(),
        username_rect: "{\"x\":0.1}".into(),
        password_rect: "{\"x\":0.2}".into(),
        submit_rect: "{\"x\":0.3}".into(),
        provider_id: "gemini".into(),
        model_name: "gemini-test".into(),
        confidence_summary: "high".into(),
        user_confirmed_at: Some(UPDATED_AT.into()),
        created_at: CREATED_AT.into(),
        updated_at: UPDATED_AT.into(),
    }
}

#[tokio::test]
async fn website_row_contains_only_secret_reference() {
    let repos = prepared_repositories().await;
    let saved = repos.websites().insert(sample_website()).await.unwrap();

    let columns = repos.debug_column_names("websites").await.unwrap();
    assert!(columns.contains(&"password_credential_ref".to_string()));
    assert!(!columns.iter().any(|column| column == "password"));
    assert_eq!(
        repos
            .websites()
            .get(saved.id)
            .await
            .unwrap()
            .unwrap()
            .username,
        "alice"
    );
}

#[tokio::test]
async fn locale_defaults_to_en_and_persists() {
    let repos = prepared_repositories().await;

    assert_eq!(repos.settings().locale().await.unwrap(), Locale::En);
    repos.settings().set_locale(Locale::ZhCn).await.unwrap();
    assert_eq!(repos.settings().locale().await.unwrap(), Locale::ZhCn);
}

#[tokio::test]
async fn settings_create_read_update_delete_and_locale_reset() {
    let repos = prepared_repositories().await;
    let settings = repos.settings();
    let key = PreferenceKey::UiLocale;
    assert_eq!(settings.get(key).await.unwrap(), None);

    let zh_cn = SettingRecord::new(key, PreferenceValue::UiLocale(Locale::ZhCn));
    assert_eq!(settings.set(zh_cn.clone()).await.unwrap(), zh_cn);
    assert_eq!(settings.get(key).await.unwrap(), Some(zh_cn));

    let en = SettingRecord::new(key, PreferenceValue::UiLocale(Locale::En));
    assert_eq!(settings.set(en.clone()).await.unwrap(), en);
    assert_eq!(settings.list().await.unwrap(), vec![en.clone()]);
    assert_eq!(settings.delete(key).await.unwrap(), Some(en));
    assert_eq!(settings.get(key).await.unwrap(), None);

    assert_eq!(settings.locale().await.unwrap(), Locale::En);
    settings.set_locale(Locale::ZhCn).await.unwrap();
    assert_eq!(settings.locale().await.unwrap(), Locale::ZhCn);
    settings.delete(key).await.unwrap();
    assert_eq!(settings.locale().await.unwrap(), Locale::En);
}

#[test]
fn closed_preferences_reject_unknown_or_invalid_serialized_values() {
    assert_eq!(
        serde_json::to_string(&PreferenceKey::UiLocale).unwrap(),
        "\"ui_locale\""
    );
    assert!(serde_json::from_str::<PreferenceKey>("\"api_key\"").is_err());
    assert!(
        serde_json::from_str::<PreferenceValue>(r#"{"kind":"ui_locale","value":"api-key"}"#)
            .is_err()
    );
}

#[tokio::test]
async fn migrations_are_idempotent_and_unique_keys_are_enforced() {
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    repos.migrate().await.unwrap();
    let website = sample_website();
    repos.websites().insert(website.clone()).await.unwrap();
    let mut duplicate = website;
    duplicate.id = WebsiteId::new();
    assert!(repos.websites().insert(duplicate).await.is_err());

    let application = sample_application();
    repos
        .applications()
        .insert(application.clone())
        .await
        .unwrap();
    let mut duplicate = application;
    duplicate.id = ApplicationId::new();
    assert!(repos.applications().insert(duplicate).await.is_err());
}

#[tokio::test]
async fn repositories_round_trip_update_list_and_delete_all_metadata() {
    let repos = prepared_repositories().await;
    let mut website = repos.websites().insert(sample_website()).await.unwrap();
    website.notes = "updated".into();
    assert_eq!(
        repos.websites().update(website.clone()).await.unwrap(),
        website
    );
    assert_eq!(
        repos.websites().list().await.unwrap(),
        vec![website.clone()]
    );
    assert_eq!(
        repos.websites().delete(website.id).await.unwrap(),
        Some(website)
    );

    let mut application = repos
        .applications()
        .insert(sample_application())
        .await
        .unwrap();
    application.is_hidden = true;
    assert_eq!(
        repos
            .applications()
            .update(application.clone())
            .await
            .unwrap(),
        application
    );
    assert_eq!(
        repos.applications().list().await.unwrap(),
        vec![application.clone()]
    );

    let mut account = repos
        .application_accounts()
        .insert(sample_account(application.id))
        .await
        .unwrap();
    account.display_name = "Work".into();
    assert_eq!(
        repos
            .application_accounts()
            .update(account.clone())
            .await
            .unwrap(),
        account
    );
    assert_eq!(
        repos.application_accounts().list().await.unwrap(),
        vec![account.clone()]
    );

    let mut template = repos
        .login_templates()
        .insert(sample_template(application.id))
        .await
        .unwrap();
    template.model_name = "gemini-updated".into();
    assert_eq!(
        repos
            .login_templates()
            .update(template.clone())
            .await
            .unwrap(),
        template
    );
    assert_eq!(
        repos.login_templates().list().await.unwrap(),
        vec![template.clone()]
    );

    assert_eq!(
        repos.login_templates().delete(template.id).await.unwrap(),
        Some(template)
    );
    assert_eq!(
        repos
            .application_accounts()
            .delete(account.id)
            .await
            .unwrap(),
        Some(account)
    );
    assert_eq!(
        repos.applications().delete(application.id).await.unwrap(),
        Some(application)
    );
}
