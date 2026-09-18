//! Install only LoginDeck's bundled Edge integration into stable per-user paths.
use autologin_core::AppError;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

const HOST: &str = "com.autologin.native";
const EXTENSION_ID: &str = include_str!("../../../browser-extension/extension-id.txt");
#[derive(Serialize)]
pub struct Installation {
    pub extension_path: String,
}
fn failure(_: impl std::fmt::Debug) -> AppError {
    AppError::new("extension.setup_failed")
}
fn resources(_: impl std::fmt::Debug) -> AppError {
    AppError::new("extension.resources_missing")
}
pub fn extension_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/LoginDeck/edge-extension")
}
fn directory(path: &Path) -> Result<(), AppError> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(AppError::new("extension.setup_conflict"));
    }
    fs::create_dir_all(path).map_err(failure)
}
fn copy_tree(source: &Path, target: &Path) -> Result<(), AppError> {
    directory(target)?;
    for entry in fs::read_dir(source).map_err(resources)? {
        let entry = entry.map_err(resources)?;
        let kind = entry.file_type().map_err(resources)?;
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination).map_err(failure)?;
        } else {
            return Err(AppError::new("extension.resources_missing"));
        }
    }
    Ok(())
}
fn replace_file(path: &Path, bytes: &[u8], executable: bool) -> Result<(), AppError> {
    let temporary = path.with_extension("installing");
    // Only fixed app-owned paths are used. Do not follow a pre-existing temporary symlink.
    if temporary.exists() || fs::symlink_metadata(&temporary).is_ok() {
        return Err(AppError::new("extension.setup_conflict"));
    }
    let result = (|| {
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o700 } else { 0o600 });
        }
        let mut file = options.open(&temporary).map_err(failure)?;
        file.write_all(bytes).map_err(failure)?;
        file.sync_all().map_err(failure)?;
        fs::rename(&temporary, path).map_err(failure)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
pub fn install(home: &Path, bundled: &Path) -> Result<Installation, AppError> {
    let source = bundled.join("extension");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("manifest.json")).map_err(resources)?)
            .map_err(resources)?;
    let identity: serde_json::Value =
        serde_json::from_str(include_str!("../../../browser-extension/identity.json"))
            .map_err(resources)?;
    if manifest["key"] != identity["key"] || manifest["manifest_version"] != 3 {
        return Err(resources("invalid extension"));
    }
    let host_bytes = fs::read(bundled.join("autologin-native-host")).map_err(resources)?;
    if host_bytes.is_empty() {
        return Err(resources("empty host"));
    }
    let base = home.join("Library/Application Support/LoginDeck");
    let native_dir = base.join("native-host");
    let executable = native_dir.join("autologin-native-host");
    let registration_dir =
        home.join("Library/Application Support/Microsoft Edge/NativeMessagingHosts");
    let registration = registration_dir.join(format!("{HOST}.json"));
    let expected = serde_json::json!({"name":HOST,"description":"LoginDeck Edge login capture","path":executable,"type":"stdio","allowed_origins":[format!("chrome-extension://{}/", EXTENSION_ID.trim())]});
    if fs::symlink_metadata(&registration).is_ok() {
        if fs::symlink_metadata(&registration)
            .map_err(failure)?
            .file_type()
            .is_symlink()
        {
            return Err(AppError::new("extension.setup_conflict"));
        }
        let previous: serde_json::Value =
            serde_json::from_slice(&fs::read(&registration).map_err(failure)?)
                .map_err(|_| AppError::new("extension.setup_conflict"))?;
        if previous != expected {
            return Err(AppError::new("extension.setup_conflict"));
        }
    }
    directory(&base)?;
    directory(&native_dir)?;
    directory(&registration_dir)?;
    let destination = extension_path(home);
    let staged = base.join("edge-extension.installing");
    let backup = base.join("edge-extension.previous");
    if [&staged, &backup]
        .iter()
        .any(|p| fs::symlink_metadata(p).is_ok())
    {
        return Err(AppError::new("extension.setup_conflict"));
    }
    if fs::symlink_metadata(&destination).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
        return Err(AppError::new("extension.setup_conflict"));
    }
    if let Err(error) = copy_tree(&source, &staged) {
        let _ = fs::remove_dir_all(&staged);
        return Err(error);
    }
    let had_previous = destination.exists();
    if had_previous {
        fs::rename(&destination, &backup).map_err(failure)?;
    }
    if let Err(error) = fs::rename(&staged, &destination) {
        if had_previous {
            let _ = fs::rename(&backup, &destination);
        }
        let _ = fs::remove_dir_all(&staged);
        return Err(failure(error));
    }
    if had_previous {
        fs::remove_dir_all(&backup).map_err(failure)?;
    }
    replace_file(&executable, &host_bytes, true)?;
    replace_file(
        &registration,
        &serde_json::to_vec_pretty(&expected).map_err(failure)?,
        false,
    )?;
    Ok(Installation {
        extension_path: destination.to_string_lossy().into_owned(),
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn installation_is_self_contained_repeatable_and_preserves_foreign_registration() {
        let root = std::env::temp_dir().join(format!("logindeck-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("bundle/edge");
        let home = root.join("home");
        fs::create_dir_all(source.join("extension")).unwrap();
        let identity: serde_json::Value =
            serde_json::from_str(include_str!("../../../browser-extension/identity.json")).unwrap();
        fs::write(
            source.join("extension/manifest.json"),
            serde_json::to_vec(&serde_json::json!({"manifest_version":3,"key":identity["key"]}))
                .unwrap(),
        )
        .unwrap();
        fs::write(source.join("extension/content.js"), "fixture").unwrap();
        fs::write(source.join("autologin-native-host"), "host fixture").unwrap();
        let first = install(&home, &source).unwrap();
        assert_eq!(
            fs::read_to_string(Path::new(&first.extension_path).join("content.js")).unwrap(),
            "fixture"
        );
        fs::write(Path::new(&first.extension_path).join("obsolete.js"), "old").unwrap();
        install(&home, &source).unwrap();
        assert!(!Path::new(&first.extension_path)
            .join("obsolete.js")
            .exists());
        let registry=home.join("Library/Application Support/Microsoft Edge/NativeMessagingHosts/com.autologin.native.json");
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        assert!(Path::new(value["path"].as_str().unwrap()).exists());
        assert_eq!(
            value["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{}/", EXTENSION_ID.trim())])
        );
        fs::write(&registry, "{\"name\":\"other\"}").unwrap();
        assert_eq!(
            install(&home, &source).err().unwrap().code(),
            "extension.setup_conflict"
        );
        assert_eq!(
            fs::read_to_string(&registry).unwrap(),
            "{\"name\":\"other\"}"
        );
        fs::remove_dir_all(&source).unwrap();
        assert!(Path::new(&first.extension_path)
            .join("manifest.json")
            .exists());
        assert_eq!(
            install(&home, &source).err().unwrap().code(),
            "extension.resources_missing"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
