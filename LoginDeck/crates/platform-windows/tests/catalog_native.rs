#![cfg(windows)]

use autologin_core::{
    ApplicationCatalog, ApplicationsService, DiscoverySource, Platform, SqliteRepositories,
};
use platform_windows::{WindowsApplicationCatalog, WindowsLaunchTarget};
use std::{collections::HashSet, fs, path::PathBuf, sync::Arc};
use windows::{
    core::PCWSTR,
    Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, KEY_WOW64_64KEY, REG_CREATED_NEW_KEY, REG_CREATE_KEY_DISPOSITION,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    },
};

struct OwnedKey(HKEY);
impl Drop for OwnedKey {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// Owns only a UUID-named test directory and a newly created HKCU registration.
/// The executable is inert data and is never launched.
struct RegisteredExecutable {
    directory: PathBuf,
    executable: PathBuf,
    registration: Option<Vec<u16>>,
}
impl RegisteredExecutable {
    fn new() -> Self {
        let suffix = uuid::Uuid::new_v4();
        let directory = std::env::temp_dir().join(format!("logindeck-catalog-test-{suffix}"));
        fs::create_dir(&directory).unwrap();
        let mut fixture = Self {
            executable: directory.join("Chat.exe"),
            directory,
            registration: None,
        };
        fs::write(
            &fixture.executable,
            b"inert catalog fixture; never launched",
        )
        .unwrap();
        let subkey: Vec<u16> = format!(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\LoginDeck.CatalogTest.{suffix}"
        )
        .encode_utf16()
        .chain([0])
        .collect();
        let mut raw = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &mut raw,
                Some(&mut disposition),
            )
        }
        .ok()
        .unwrap();
        let key = OwnedKey(raw);
        // Never overwrite or remove a pre-existing key, even on a UUID collision.
        assert_eq!(disposition, REG_CREATED_NEW_KEY);
        fixture.registration = Some(subkey);
        for (name, value) in [
            ("DisplayName", "LoginDeck Catalog Fixture".to_owned()),
            (
                "DisplayIcon",
                fixture.executable.to_str().unwrap().to_owned(),
            ),
        ] {
            let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let bytes: Vec<u8> = value
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) }
                .ok()
                .unwrap();
        }
        fixture
    }
}
impl Drop for RegisteredExecutable {
    fn drop(&mut self) {
        let removed = self.registration.as_ref().map(|subkey| unsafe {
            RegDeleteKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                KEY_WOW64_64KEY.0,
                None,
            )
            .ok()
        });
        let file_removed = fs::remove_file(&self.executable);
        let directory_removed = fs::remove_dir(&self.directory);
        if !std::thread::panicking() {
            assert!(removed.is_none_or(|result| result.is_ok()));
            assert!(file_removed.is_ok());
            assert!(directory_removed.is_ok());
        }
    }
}

#[tokio::test]
async fn import_then_automatic_discovery_preserves_registered_identity_and_saved_application() {
    let fixture = RegisteredExecutable::new();
    let catalog = Arc::new(WindowsApplicationCatalog::new());
    let repos = SqliteRepositories::connect("sqlite::memory:")
        .await
        .unwrap();
    repos.migrate().await.unwrap();
    let service = ApplicationsService::new(
        repos.applications(),
        repos.application_accounts(),
        catalog.clone(),
    );
    let imported = service
        .import_applications(vec![fixture.executable.clone()])
        .await
        .unwrap()
        .remove(0);
    assert_eq!(imported.discovery_source, DiscoverySource::ManualImport);
    let mut saved = imported.clone();
    saved.is_hidden = true;
    repos.applications().update(saved).await.unwrap();

    let matches: Vec<_> = catalog
        .discover()
        .await
        .unwrap()
        .into_iter()
        .filter(|item| item.launch_target == imported.launch_target)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "test-owned registration must be discovered"
    );
    let automatic = &matches[0];
    assert_eq!(automatic.discovery_source, DiscoverySource::Automatic);
    assert_eq!(imported.signature_identity, automatic.signature_identity);
    assert_eq!(
        imported.platform_application_id, automatic.platform_application_id,
        "import and discovery must share the registered Win32 identity"
    );
    service.apply_discovery(matches).await.unwrap();
    let rows = service.list_applications().await.unwrap();
    assert_eq!(rows.len(), 1, "discovery must reconcile the imported row");
    assert_eq!(rows[0].id, imported.id);
    assert!(
        rows[0].is_hidden,
        "saved user state must survive reconciliation"
    );
    assert!(rows[0].is_present);
    let repeated = service
        .import_applications(vec![fixture.directory.clone()])
        .await
        .unwrap();
    assert_eq!(repeated.len(), 1);
    assert_eq!(repeated[0].id, imported.id);
    assert_eq!(repeated[0].discovery_source, DiscoverySource::ManualImport);
}

#[tokio::test]
async fn live_catalog_is_bounded_typed_unique_and_non_destructive() {
    let items = WindowsApplicationCatalog::new().discover().await.unwrap();
    assert!(items.len() <= 4096);
    let mut ids = HashSet::new();
    for item in &items {
        assert_eq!(item.platform, Platform::Windows);
        assert_eq!(item.discovery_source, DiscoverySource::Automatic);
        assert!(WindowsLaunchTarget::parse(&item.launch_target).is_ok());
        assert!(ids.insert(item.platform_application_id.to_lowercase()));
        assert_eq!(item.path_access_ref, None);
    }
    eprintln!("live catalog: {} validated items", items.len());
}
