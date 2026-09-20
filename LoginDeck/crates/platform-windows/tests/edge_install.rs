#![cfg(target_os = "windows")]

use std::{fs, path::PathBuf};

use platform_windows::edge;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
            REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
        },
    },
};

struct Fixture {
    root: PathBuf,
    bundle: PathBuf,
    registry_key: String,
}

impl Fixture {
    fn new() -> Self {
        let id = uuid::Uuid::new_v4();
        let parent = std::env::temp_dir().join(format!("logindeck-edge-{id}"));
        let bundle = parent.join("bundle");
        fs::create_dir_all(bundle.join("extension")).unwrap();
        let identity: serde_json::Value =
            serde_json::from_str(include_str!("../../../browser-extension/identity.json")).unwrap();
        fs::write(
            bundle.join("extension/manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "manifest_version": 3,
                "key": identity["key"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(bundle.join("extension/content.js"), b"current").unwrap();
        fs::write(
            bundle.join("autologin-native-host.exe"),
            b"controlled inert host fixture",
        )
        .unwrap();
        Self {
            root: parent.join("installed"),
            bundle,
            registry_key: format!(r"Software\LoginDeck\Tests\{id}"),
        }
    }

    fn install(&self) -> Result<PathBuf, autologin_core::AppError> {
        edge::install_at(&self.bundle, &self.root, &self.registry_key)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        delete_key(&self.registry_key);
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn delete_key(subkey: &str) {
    let subkey = wide(subkey);
    let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr())) };
}

fn set_registry(subkey: &str, value: &str) {
    let subkey = wide(subkey);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    }
    .ok()
    .unwrap();
    let bytes: Vec<u8> = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    unsafe { RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(&bytes)) }
        .ok()
        .unwrap();
    unsafe { RegCloseKey(key) }.ok().unwrap();
}

fn registry_value(subkey: &str) -> String {
    let subkey = wide(subkey);
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    }
    .ok()
    .unwrap();
    let mut kind = REG_VALUE_TYPE::default();
    let mut bytes = vec![0_u8; 4096];
    let mut size = bytes.len() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        )
    };
    unsafe { RegCloseKey(key) }.ok().unwrap();
    assert_eq!(status, ERROR_SUCCESS);
    assert_eq!(kind, REG_SZ);
    bytes.truncate(size as usize - 2);
    String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn installation_is_repeatable_replaces_stale_files_and_writes_fixed_manifest() {
    let fixture = Fixture::new();
    let extension = fixture.install().unwrap();
    fs::write(extension.join("obsolete.js"), b"stale").unwrap();
    assert_eq!(fixture.install().unwrap(), extension);
    assert!(!extension.join("obsolete.js").exists());

    let registration = registry_value(&fixture.registry_key);
    let host_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&registration).unwrap()).unwrap();
    assert_eq!(host_manifest["name"], "com.autologin.native");
    assert_eq!(host_manifest["type"], "stdio");
    assert_eq!(
        host_manifest["allowed_origins"].as_array().unwrap().len(),
        1
    );
    assert!(PathBuf::from(host_manifest["path"].as_str().unwrap()).exists());
    assert!(!fixture.root.join("edge-extension.installing").exists());
    assert!(!fixture.root.join("edge-extension.previous").exists());
}

#[test]
fn invalid_identity_and_missing_or_empty_host_are_resources_missing() {
    for mutation in ["identity", "missing", "empty"] {
        let fixture = Fixture::new();
        match mutation {
            "identity" => fs::write(
                fixture.bundle.join("extension/manifest.json"),
                br#"{"manifest_version":3,"key":"foreign"}"#,
            )
            .unwrap(),
            "missing" => fs::remove_file(fixture.bundle.join("autologin-native-host.exe")).unwrap(),
            "empty" => fs::write(fixture.bundle.join("autologin-native-host.exe"), b"").unwrap(),
            _ => unreachable!(),
        }
        assert_eq!(
            fixture.install().unwrap_err().code(),
            "extension.resources_missing"
        );
        assert!(!fixture.root.join("edge-extension.installing").exists());
    }
}

#[test]
fn foreign_registration_is_preserved_without_installing_files() {
    let fixture = Fixture::new();
    set_registry(&fixture.registry_key, r"C:\foreign\host.json");
    assert_eq!(
        fixture.install().unwrap_err().code(),
        "extension.setup_conflict"
    );
    assert_eq!(
        registry_value(&fixture.registry_key),
        r"C:\foreign\host.json"
    );
    assert!(!fixture.root.exists());
}

#[test]
fn reparse_point_install_root_is_rejected_when_symlink_creation_is_available() {
    use std::os::windows::fs::symlink_dir;

    let fixture = Fixture::new();
    let actual = fixture.root.parent().unwrap().join("actual");
    fs::create_dir_all(&actual).unwrap();
    if symlink_dir(&actual, &fixture.root).is_err() {
        return;
    }
    assert_eq!(
        fixture.install().unwrap_err().code(),
        "extension.setup_conflict"
    );
}

#[test]
#[ignore = "writes the production current-user Edge registration and LoginDeck app-data paths"]
fn production_bundle_install_smoke() {
    let bundled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src-tauri/resources/edge");
    let extension = edge::install(&bundled).expect("install production Edge integration");
    assert!(extension.join("manifest.json").is_file());
}
