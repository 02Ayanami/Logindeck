//! Install and register LoginDeck's fixed Microsoft Edge native-messaging integration.

use autologin_core::AppError;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::application_data_directory;

const HOST_NAME: &str = "com.autologin.native";
const HOST_FILE: &str = "autologin-native-host.exe";
const PRODUCTION_REGISTRY_KEY: &str =
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.autologin.native";
const EXTENSION_ID: &str = include_str!("../../../browser-extension/extension-id.txt");
const IDENTITY: &str = include_str!("../../../browser-extension/identity.json");

fn failed(_: impl std::fmt::Debug) -> AppError {
    AppError::new("extension.setup_failed")
}

fn missing(_: impl std::fmt::Debug) -> AppError {
    AppError::new("extension.resources_missing")
}

fn conflict() -> AppError {
    AppError::new("extension.setup_conflict")
}

pub fn extension_path() -> Result<PathBuf, AppError> {
    Ok(application_data_directory()?.join("edge-extension"))
}

pub fn install(bundled: &Path) -> Result<PathBuf, AppError> {
    install_at(
        bundled,
        &application_data_directory()?,
        PRODUCTION_REGISTRY_KEY,
    )
}

/// Test seam for exercising the installer with a unique owned root and HKCU key.
#[doc(hidden)]
pub fn install_at(
    bundled: &Path,
    install_root: &Path,
    registry_subkey: &str,
) -> Result<PathBuf, AppError> {
    if !install_root.is_absolute()
        || registry_subkey.is_empty()
        || registry_subkey.contains('\0')
        || (!registry_subkey.starts_with(r"Software\LoginDeck\Tests\")
            && registry_subkey != PRODUCTION_REGISTRY_KEY)
    {
        return Err(conflict());
    }

    let source = bundled.join("extension");
    validate_bundle(&source, bundled)?;
    let extension = install_root.join("edge-extension");
    let native = install_root.join("native-host");
    let executable = native.join(HOST_FILE);
    let manifest_path = native.join(format!("{HOST_NAME}.json"));
    let expected_registration = absolute_text(&manifest_path)?;

    if let Some(previous) = registry_value(registry_subkey)? {
        if !previous.eq_ignore_ascii_case(&expected_registration) {
            return Err(conflict());
        }
    }

    ensure_directory(install_root)?;
    ensure_directory(&native)?;
    replace_extension_tree(&source, &extension, install_root)?;
    replace_file(
        &executable,
        &fs::read(bundled.join(HOST_FILE)).map_err(missing)?,
    )?;

    let host_manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "LoginDeck Edge login capture",
        "path": absolute_text(&executable)?,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", EXTENSION_ID.trim())]
    });
    replace_file(
        &manifest_path,
        &serde_json::to_vec_pretty(&host_manifest).map_err(failed)?,
    )?;
    write_registry_value(registry_subkey, &expected_registration)?;
    Ok(extension)
}

fn validate_bundle(extension: &Path, bundled: &Path) -> Result<(), AppError> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(extension.join("manifest.json")).map_err(missing)?)
            .map_err(missing)?;
    let identity: serde_json::Value = serde_json::from_str(IDENTITY).map_err(missing)?;
    if manifest["manifest_version"] != 3 || manifest["key"] != identity["key"] {
        return Err(missing("invalid extension identity"));
    }
    let host = fs::read(bundled.join(HOST_FILE)).map_err(missing)?;
    if host.is_empty() {
        return Err(missing("empty native host"));
    }
    Ok(())
}

fn absolute_text(path: &Path) -> Result<String, AppError> {
    if !path.is_absolute() {
        return Err(conflict());
    }
    path.to_str().map(str::to_owned).ok_or_else(conflict)
}

#[cfg(target_os = "windows")]
fn is_reparse(path: &Path) -> Result<bool, AppError> {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    Ok(fs::symlink_metadata(path)
        .map_err(failed)?
        .file_attributes()
        & FILE_ATTRIBUTE_REPARSE_POINT.0
        != 0)
}

#[cfg(not(target_os = "windows"))]
fn is_reparse(path: &Path) -> Result<bool, AppError> {
    Ok(fs::symlink_metadata(path)
        .map_err(failed)?
        .file_type()
        .is_symlink())
}

fn ensure_directory(path: &Path) -> Result<(), AppError> {
    if fs::symlink_metadata(path).is_ok() {
        if is_reparse(path)? || !fs::metadata(path).map_err(failed)?.is_dir() {
            return Err(conflict());
        }
        return Ok(());
    }
    fs::create_dir(path).map_err(failed)
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir(target).map_err(failed)?;
    for entry in fs::read_dir(source).map_err(missing)? {
        let entry = entry.map_err(missing)?;
        let kind = entry.file_type().map_err(missing)?;
        let destination = target.join(entry.file_name());
        if kind.is_symlink() || is_reparse(&entry.path())? {
            return Err(missing("linked extension resource"));
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination).map_err(failed)?;
        } else {
            return Err(missing("unsupported extension resource"));
        }
    }
    Ok(())
}

fn replace_extension_tree(source: &Path, destination: &Path, root: &Path) -> Result<(), AppError> {
    let staged = root.join("edge-extension.installing");
    let backup = root.join("edge-extension.previous");
    for path in [&staged, &backup, destination] {
        if fs::symlink_metadata(path).is_ok() && is_reparse(path)? {
            return Err(conflict());
        }
    }
    if staged.exists() || backup.exists() {
        return Err(conflict());
    }
    if destination.exists() && !destination.is_dir() {
        return Err(conflict());
    }
    if let Err(error) = copy_tree(source, &staged) {
        let _ = fs::remove_dir_all(&staged);
        return Err(error);
    }
    let had_previous = destination.exists();
    if had_previous {
        fs::rename(destination, &backup).map_err(failed)?;
    }
    if let Err(error) = fs::rename(&staged, destination) {
        if had_previous {
            let _ = fs::rename(&backup, destination);
        }
        let _ = fs::remove_dir_all(&staged);
        return Err(failed(error));
    }
    if had_previous {
        fs::remove_dir_all(backup).map_err(failed)?;
    }
    Ok(())
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if fs::symlink_metadata(path).is_ok() && (is_reparse(path)? || !path.is_file()) {
        return Err(conflict());
    }
    let staged = path.with_extension("installing");
    if fs::symlink_metadata(&staged).is_ok() {
        return Err(conflict());
    }
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(failed)?;
        file.write_all(bytes).map_err(failed)?;
        file.sync_all().map_err(failed)?;
        move_replace(&staged, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

#[cfg(target_os = "windows")]
fn move_replace(source: &Path, destination: &Path) -> Result<(), AppError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(failed)
}

#[cfg(not(target_os = "windows"))]
fn move_replace(source: &Path, destination: &Path) -> Result<(), AppError> {
    fs::rename(source, destination).map_err(failed)
}

#[cfg(target_os = "windows")]
fn registry_value(subkey: &str) -> Result<Option<String>, AppError> {
    registry::read(subkey)
}

#[cfg(not(target_os = "windows"))]
fn registry_value(_: &str) -> Result<Option<String>, AppError> {
    Err(AppError::new("platform.unsupported"))
}

#[cfg(target_os = "windows")]
fn write_registry_value(subkey: &str, value: &str) -> Result<(), AppError> {
    registry::write(subkey, value)
}

#[cfg(not(target_os = "windows"))]
fn write_registry_value(_: &str, _: &str) -> Result<(), AppError> {
    Err(AppError::new("platform.unsupported"))
}

#[cfg(target_os = "windows")]
mod registry {
    use super::{conflict, failed};
    use autologin_core::AppError;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
            System::Registry::{
                RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
                HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
                REG_SZ, REG_VALUE_TYPE,
            },
        },
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            let _ = unsafe { RegCloseKey(self.0) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }

    pub(super) fn read(subkey: &str) -> Result<Option<String>, AppError> {
        let subkey = wide(subkey);
        let mut raw = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                KEY_QUERY_VALUE,
                &mut raw,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        let key = Key(raw);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0_u32;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR::null(),
                None,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS || kind != REG_SZ || !(2..=32 * 1024).contains(&size) {
            return Err(conflict());
        }
        let mut bytes = vec![0_u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR::null(),
                None,
                Some(&mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS || kind != REG_SZ || size as usize != bytes.len() {
            return Err(conflict());
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let Some((&0, text)) = units.split_last() else {
            return Err(conflict());
        };
        if text.contains(&0) {
            return Err(conflict());
        }
        String::from_utf16(text).map(Some).map_err(|_| conflict())
    }

    pub(super) fn write(subkey: &str, value: &str) -> Result<(), AppError> {
        if let Some(previous) = read(subkey)? {
            return if previous.eq_ignore_ascii_case(value) {
                Ok(())
            } else {
                Err(conflict())
            };
        }
        let subkey = wide(subkey);
        let mut raw = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut raw,
                None,
            )
        }
        .ok()
        .map_err(failed)?;
        let key = Key(raw);
        let bytes: Vec<u8> = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        unsafe { RegSetValueExW(key.0, PCWSTR::null(), None, REG_SZ, Some(&bytes)) }
            .ok()
            .map_err(failed)
    }
}

#[cfg(target_os = "windows")]
fn shell_open(target: &str, parameters: Option<&str>) -> Result<(), AppError> {
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let wide = |value: &str| value.encode_utf16().chain([0]).collect::<Vec<_>>();
    let verb = wide("open");
    let target = wide(target);
    let parameters = parameters.map(wide);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            parameters
                .as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(AppError::new("extension.edge_unavailable"))
    } else {
        Ok(())
    }
}

pub fn open_extensions() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        shell_open("msedge.exe", Some("edge://extensions/"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::new("platform.unsupported"))
    }
}

pub fn reveal_extension() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        let path = absolute_text(&extension_path()?)?;
        shell_open("explorer.exe", Some(&path))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::new("platform.unsupported"))
    }
}
